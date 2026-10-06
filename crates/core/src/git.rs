//! Thin wrappers over the git CLI. Never fetches, never prompts.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("git could not be run: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("{0}")]
    Failed(String),
}

fn cmd(dir: &Path) -> Command {
    let mut c = Command::new("git");
    c.arg("-C")
        .arg(dir)
        .arg("--no-optional-locks")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .stdin(Stdio::null());
    c
}

pub fn run(dir: &Path, args: &[&str]) -> Result<String, GitError> {
    let out = cmd(dir).args(args).output()?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(GitError::Failed(if err.is_empty() {
            format!("git {} exited with {}", args.join(" "), out.status)
        } else {
            err
        }))
    }
}

pub fn run_bytes(dir: &Path, args: &[&str]) -> Result<Vec<u8>, GitError> {
    let out = cmd(dir).args(args).output()?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(GitError::Failed(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ))
    }
}

fn count(dir: &Path, args: &[&str]) -> Result<u32, GitError> {
    let s = run(dir, args)?;
    s.trim()
        .parse::<u32>()
        .map_err(|e| GitError::Failed(format!("unexpected count output {s:?}: {e}")))
}

#[derive(Debug, Clone, Default)]
pub struct WtEntry {
    pub path: PathBuf,
    pub head: String,
    pub branch: Option<String>,
    pub detached: bool,
    pub bare: bool,
    pub locked: Option<String>,
    pub prunable: Option<String>,
}

/// `git worktree list --porcelain`, first entry is the main checkout.
pub fn worktree_list(repo: &Path) -> Result<Vec<WtEntry>, GitError> {
    let out = run(repo, &["worktree", "list", "--porcelain"])?;
    let mut entries = Vec::new();
    let mut cur: Option<WtEntry> = None;
    for line in out.lines() {
        if line.is_empty() {
            if let Some(e) = cur.take() {
                entries.push(e);
            }
            continue;
        }
        let (key, val) = match line.split_once(' ') {
            Some((k, v)) => (k, v),
            None => (line, ""),
        };
        match key {
            "worktree" => {
                if let Some(e) = cur.take() {
                    entries.push(e);
                }
                cur = Some(WtEntry {
                    path: PathBuf::from(val),
                    ..Default::default()
                });
            }
            "HEAD" => {
                if let Some(e) = cur.as_mut() {
                    e.head = val.to_string();
                }
            }
            "branch" => {
                if let Some(e) = cur.as_mut() {
                    e.branch = Some(val.trim_start_matches("refs/heads/").to_string());
                }
            }
            "detached" => {
                if let Some(e) = cur.as_mut() {
                    e.detached = true;
                }
            }
            "bare" => {
                if let Some(e) = cur.as_mut() {
                    e.bare = true;
                }
            }
            "locked" => {
                if let Some(e) = cur.as_mut() {
                    e.locked = Some(val.to_string());
                }
            }
            "prunable" => {
                if let Some(e) = cur.as_mut() {
                    e.prunable = Some(val.to_string());
                }
            }
            _ => {}
        }
    }
    if let Some(e) = cur.take() {
        entries.push(e);
    }
    Ok(entries)
}

/// (changed tracked files, untracked files) from `status --porcelain=v2 -z`.
pub fn status_counts(wt: &Path) -> Result<(u32, u32), GitError> {
    let raw = run_bytes(
        wt,
        &["status", "--porcelain=v2", "-z", "--untracked-files=normal"],
    )?;
    let mut dirty = 0u32;
    let mut untracked = 0u32;
    let mut it = raw.split(|b| *b == 0).filter(|s| !s.is_empty());
    while let Some(rec) = it.next() {
        match rec.first() {
            Some(b'1') | Some(b'u') => dirty += 1,
            Some(b'2') => {
                dirty += 1;
                it.next(); // original path of a rename follows as its own NUL field
            }
            Some(b'?') => untracked += 1,
            _ => {}
        }
    }
    Ok((dirty, untracked))
}

pub fn unpushed(wt: &Path) -> Option<u32> {
    count(wt, &["rev-list", "--count", "@{upstream}..HEAD"]).ok()
}

pub fn has_remotes(repo: &Path) -> bool {
    run(repo, &["remote"])
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
}

/// Commits on HEAD that no remote ref contains.
pub fn not_on_remote(wt: &Path) -> Result<u32, GitError> {
    count(wt, &["rev-list", "--count", "HEAD", "--not", "--remotes"])
}

/// Commits on HEAD that no remote and no *other* local branch contains.
pub fn own_commits(wt: &Path, branch: Option<&str>) -> Result<u32, GitError> {
    match branch {
        Some(b) => {
            let ex = format!("--exclude={b}");
            count(
                wt,
                &[
                    "rev-list",
                    "--count",
                    "HEAD",
                    "--not",
                    "--remotes",
                    &ex,
                    "--branches",
                ],
            )
        }
        None => count(
            wt,
            &[
                "rev-list",
                "--count",
                "HEAD",
                "--not",
                "--remotes",
                "--branches",
            ],
        ),
    }
}

pub fn is_tracked(wt: &Path, rel: &Path) -> bool {
    cmd(wt)
        .args(["ls-files", "--error-unmatch", "--"])
        .arg(rel)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn origin_url(repo: &Path) -> Option<String> {
    run(repo, &["remote", "get-url", "origin"])
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn default_branch(repo: &Path) -> Option<String> {
    run(
        repo,
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    )
    .ok()
    .map(|s| s.trim().trim_start_matches("origin/").to_string())
    .filter(|s| !s.is_empty())
}

pub fn worktree_prune(repo: &Path) -> Result<(), GitError> {
    run(repo, &["worktree", "prune"]).map(|_| ())
}

pub fn worktree_remove(repo: &Path, wt: &Path, force: bool) -> Result<(), GitError> {
    let wts = wt.to_string_lossy();
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(&wts);
    run(repo, &args).map(|_| ())
}

pub fn branch_delete(repo: &Path, branch: &str) -> Result<(), GitError> {
    run(repo, &["branch", "-D", branch]).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let st = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(st.success(), "git {args:?} failed");
    }

    #[test]
    fn worktree_list_and_status() {
        let t = tempfile::tempdir().unwrap();
        let repo = t.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        std::fs::write(repo.join("a.txt"), "a").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "one"]);
        let wt = repo.join(".claude/worktrees/x");
        git(
            &repo,
            &["worktree", "add", "-q", "-b", "feat", wt.to_str().unwrap()],
        );
        let list = worktree_list(&repo).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].branch.as_deref(), Some("main"));
        assert_eq!(list[1].branch.as_deref(), Some("feat"));
        assert_eq!(status_counts(&wt).unwrap(), (0, 0));
        std::fs::write(wt.join("a.txt"), "b").unwrap();
        std::fs::write(wt.join("new.txt"), "n").unwrap();
        assert_eq!(status_counts(&wt).unwrap(), (1, 1));
        assert_eq!(unpushed(&wt), None);
        assert_eq!(own_commits(&wt, Some("feat")).unwrap(), 0);
        git(&wt, &["add", "."]);
        git(&wt, &["commit", "-q", "-m", "two"]);
        assert_eq!(own_commits(&wt, Some("feat")).unwrap(), 1);
    }
}
