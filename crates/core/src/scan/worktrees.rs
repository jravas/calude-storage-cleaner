//! Repo discovery and per-worktree sizing plus git state.
//! Sessions, registry and safety are joined in `scan::mod`.

use crate::fsize::size_worktree;
use crate::git;
use crate::model::{Artifact, GitState, Repo, ScanError, Unpushed, Worktree};
use crate::paths::norm_key;
use rayon::prelude::*;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

pub fn discover_repos(roots: &[PathBuf], extra: &[PathBuf]) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let mut push = |p: PathBuf| {
        if p.join(".git").exists() && seen.insert(norm_key(&p)) {
            out.push(p);
        }
    };
    for root in roots {
        if root.join(".git").exists() {
            push(root.clone());
            continue;
        }
        if let Ok(rd) = std::fs::read_dir(root) {
            let mut kids: Vec<PathBuf> = rd
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect();
            kids.sort();
            for k in kids {
                push(k);
            }
        }
    }
    for p in extra {
        if p.is_dir() {
            push(p.clone());
        }
    }
    out
}

fn git_state(wt: &Path, branch: Option<&str>, has_remotes: bool) -> GitState {
    let mut g = GitState::default();
    match git::status_counts(wt) {
        Ok((d, u)) => {
            g.dirty = d;
            g.untracked = u;
        }
        Err(e) => g.error = Some(e.to_string()),
    }
    g.unpushed = match git::unpushed(wt) {
        Some(n) => Unpushed::Count(n),
        None => Unpushed::NoUpstream,
    };
    if has_remotes {
        g.not_on_remote = git::not_on_remote(wt).ok();
    }
    g.own_commits = git::own_commits(wt, branch).unwrap_or(0);
    g
}

fn build_worktree(
    repo: &Path,
    entry: &git::WtEntry,
    is_main: bool,
    orphan: bool,
    has_remotes: bool,
    cancel: &AtomicBool,
) -> Worktree {
    let path = entry.path.clone();
    let exclude = if is_main {
        vec![repo.join(".claude/worktrees")]
    } else {
        Vec::new()
    };
    let (checkout, arts) = size_worktree(&path, &exclude, cancel);
    let mut last_modified = checkout.max_mtime;
    let artifacts: Vec<Artifact> = arts
        .into_iter()
        .map(|(a, s)| {
            last_modified = last_modified.max(s.max_mtime);
            let rel = a.path.strip_prefix(&path).unwrap_or(&a.path).to_path_buf();
            Artifact {
                kind: a.kind,
                tracked: git::is_tracked(&path, &rel),
                path: a.path,
                bytes: s.bytes,
                files: s.files,
            }
        })
        .collect();
    let art_bytes: u64 = artifacts.iter().map(|a| a.bytes).sum();
    let mut g = git_state(&path, entry.branch.as_deref(), has_remotes);
    g.locked = entry.locked.clone();
    g.prunable = entry.prunable.clone();
    let name = if is_main {
        repo.file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    } else {
        path.file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    Worktree {
        path,
        name,
        repo: repo.to_path_buf(),
        is_main,
        orphan,
        head: entry.head.clone(),
        branch: entry.branch.clone(),
        detached: entry.detached,
        source_branch: None,
        bytes_total: checkout.bytes + art_bytes,
        bytes_checkout: checkout.bytes,
        artifacts,
        last_modified,
        created_at: None,
        git: g,
        registry: None,
        sessions: Vec::new(),
        state: Default::default(),
        prune_allowed: false,
    }
}

pub fn scan_repo(repo: &Path, cancel: &AtomicBool) -> Result<Repo, ScanError> {
    let entries = git::worktree_list(repo).map_err(|e| ScanError {
        path: repo.to_path_buf(),
        message: e.to_string(),
    })?;
    if entries.is_empty() {
        return Err(ScanError {
            path: repo.to_path_buf(),
            message: "git listed no worktrees".into(),
        });
    }
    let has_remotes = git::has_remotes(repo);
    let (entries, missing): (Vec<git::WtEntry>, Vec<git::WtEntry>) =
        entries.into_iter().partition(|e| e.path.is_dir());
    let stale_entries: Vec<PathBuf> = missing.into_iter().map(|e| e.path).collect();
    if entries.is_empty() || !entries[0].path.is_dir() {
        return Err(ScanError {
            path: repo.to_path_buf(),
            message: "main checkout is missing".into(),
        });
    }
    let listed: HashSet<String> = entries.iter().map(|e| norm_key(&e.path)).collect();

    // Directories under .claude/worktrees that git does not know about.
    let mut orphans: Vec<git::WtEntry> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(repo.join(".claude/worktrees")) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() && !listed.contains(&norm_key(&p)) {
                let head = git::run(&p, &["rev-parse", "HEAD"])
                    .map(|s| s.trim().to_string())
                    .unwrap_or_default();
                let branch = git::run(&p, &["rev-parse", "--abbrev-ref", "HEAD"])
                    .ok()
                    .map(|s| s.trim().to_string())
                    .filter(|s| s != "HEAD" && !s.is_empty());
                orphans.push(git::WtEntry {
                    detached: branch.is_none(),
                    path: p,
                    head,
                    branch,
                    ..Default::default()
                });
            }
        }
    }

    let main = build_worktree(repo, &entries[0], true, false, has_remotes, cancel);
    let mut worktrees: Vec<Worktree> = entries[1..]
        .par_iter()
        .filter(|e| !e.bare)
        .map(|e| build_worktree(repo, e, false, false, has_remotes, cancel))
        .chain(
            orphans
                .par_iter()
                .map(|e| build_worktree(repo, e, false, true, has_remotes, cancel)),
        )
        .collect();
    worktrees.sort_by_key(|w| std::cmp::Reverse(w.bytes_total));
    let total_bytes = main.bytes_total + worktrees.iter().map(|w| w.bytes_total).sum::<u64>();
    Ok(Repo {
        path: repo.to_path_buf(),
        name: repo
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
        origin: git::origin_url(repo),
        main,
        worktrees,
        stale_entries,
        total_bytes,
    })
}
