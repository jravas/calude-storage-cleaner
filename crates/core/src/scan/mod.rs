//! Orchestrates the scanners and joins their results.

pub mod buckets;
pub mod desktop;
pub mod live;
pub mod transcripts;
pub mod worktrees;

use crate::model::*;
use crate::paths::{self, norm_key};
use crate::progress::{ProgressSink, ScanEvent};
use crate::safety::{self, SafetyInput};
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct ScanOptions {
    pub roots: Vec<PathBuf>,
}

impl Default for ScanOptions {
    fn default() -> Self {
        ScanOptions {
            roots: vec![paths::home().join("Projects")],
        }
    }
}

struct Index {
    live_by_cwd: HashMap<String, Vec<(u32, String)>>,
    live_hosts: HashMap<String, u32>,
    live_cli: HashSet<String>,
    desktop_by_cwd: HashMap<String, Vec<desktop::DesktopSession>>,
    desktop_by_id: HashMap<String, desktop::DesktopSession>,
    registry_by_cwd: HashMap<String, RegistryEntry>,
    transcript_by_cli: HashMap<String, (u64, Option<String>, Option<i64>)>,
    transcript_by_cwd: HashMap<String, Vec<Session>>,
    default_branch: Mutex<HashMap<String, Option<String>>>,
}

fn build_index(
    live: &[live::LiveSession],
    desktop: &[desktop::DesktopSession],
    registry: Option<&desktop::Registry>,
    transcripts: &[TranscriptDir],
) -> Index {
    let mut ix = Index {
        live_by_cwd: HashMap::new(),
        live_hosts: HashMap::new(),
        live_cli: HashSet::new(),
        desktop_by_cwd: HashMap::new(),
        desktop_by_id: HashMap::new(),
        registry_by_cwd: HashMap::new(),
        transcript_by_cli: HashMap::new(),
        transcript_by_cwd: HashMap::new(),
        default_branch: Mutex::new(HashMap::new()),
    };
    for l in live.iter().filter(|l| l.alive) {
        ix.live_by_cwd
            .entry(norm_key(std::path::Path::new(&l.cwd)))
            .or_default()
            .push((l.pid, l.session_id.clone()));
        if let Some(h) = &l.host_session_id {
            ix.live_hosts.insert(h.clone(), l.pid);
        }
        ix.live_cli.insert(l.session_id.clone());
    }
    for d in desktop {
        let mut keys = HashSet::new();
        for c in [&d.cwd, &d.worktree_path].into_iter().flatten() {
            keys.insert(norm_key(std::path::Path::new(c)));
        }
        for k in keys {
            ix.desktop_by_cwd.entry(k).or_default().push(d.clone());
        }
        ix.desktop_by_id.insert(d.session_id.clone(), d.clone());
    }
    if let Some(r) = registry {
        for e in r.worktrees.values() {
            ix.registry_by_cwd
                .insert(norm_key(std::path::Path::new(&e.path)), e.clone());
        }
    }
    for t in transcripts {
        for s in &t.sessions {
            ix.transcript_by_cli.insert(
                s.cli_id.clone(),
                (
                    s.transcript_bytes + s.sidecar_bytes,
                    s.title.clone(),
                    s.last_ts,
                ),
            );
            if let Some(c) = &s.cwd {
                ix.transcript_by_cwd
                    .entry(norm_key(c))
                    .or_default()
                    .push(s.clone());
            }
        }
    }
    ix
}

fn enrich(w: &mut Worktree, ix: &Index, now: i64) {
    let key = norm_key(&w.path);
    let mut sessions: Vec<SessionRef> = Vec::new();
    let mut seen_cli = HashSet::new();
    let mut open_in_app = None;
    let mut created = None;
    let mut source_branch = None;
    if let Some(list) = ix.desktop_by_cwd.get(&key) {
        for d in list {
            let cli = d.cli_session_id.clone();
            if let Some(c) = &cli {
                seen_cli.insert(c.clone());
            }
            let tb = cli
                .as_ref()
                .and_then(|c| ix.transcript_by_cli.get(c))
                .map(|t| t.0)
                .unwrap_or(0);
            let live = ix.live_hosts.contains_key(&d.session_id);
            if !d.is_archived && !live && open_in_app.is_none() {
                open_in_app = Some(d.session_id.clone());
            }
            created = match (created, d.created_at) {
                (None, x) => x,
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, None) => a,
            };
            if source_branch.is_none() {
                source_branch = d.source_branch.clone();
            }
            sessions.push(SessionRef {
                cli_id: cli,
                desktop_id: Some(d.session_id.clone()),
                title: d.title.clone(),
                archived: d.is_archived,
                last_activity: d.last_activity_at,
                live,
                transcript_bytes: tb,
            });
        }
    }
    if let Some(list) = ix.transcript_by_cwd.get(&key) {
        for s in list {
            if seen_cli.contains(&s.cli_id) {
                continue;
            }
            sessions.push(SessionRef {
                cli_id: Some(s.cli_id.clone()),
                desktop_id: None,
                title: s.title.clone(),
                archived: true,
                last_activity: s.last_ts,
                live: ix.live_cli.contains(&s.cli_id),
                transcript_bytes: s.transcript_bytes + s.sidecar_bytes,
            });
        }
    }
    sessions.sort_by_key(|s| std::cmp::Reverse(s.last_activity));

    let mut live: Vec<(u32, String)> = ix.live_by_cwd.get(&key).cloned().unwrap_or_default();
    let registry = ix.registry_by_cwd.get(&key).cloned();
    if let Some(r) = &registry {
        if let Some(l) = &r.leased_by {
            if let Some(pid) = ix.live_hosts.get(l) {
                if !live.iter().any(|(p, _)| p == pid) {
                    live.push((*pid, l.clone()));
                }
            } else if let Some(d) = ix.desktop_by_id.get(l) {
                if !d.is_archived && open_in_app.is_none() {
                    open_in_app = Some(l.clone());
                }
            }
        }
        if created.is_none() {
            created = r.created_at;
        }
        if source_branch.is_none() {
            source_branch = r.source_branch.clone();
        }
    }
    if source_branch.is_none() && !w.is_main {
        let mut cache = ix.default_branch.lock().unwrap();
        let repo_key = w.repo.to_string_lossy().into_owned();
        source_branch = cache
            .entry(repo_key)
            .or_insert_with(|| crate::git::default_branch(&w.repo))
            .clone();
    }
    let has_desktop_record = sessions.iter().any(|s| s.desktop_id.is_some());
    let input = SafetyInput {
        is_main: w.is_main,
        live,
        locked: w.git.locked.clone(),
        pooled: registry
            .as_ref()
            .map(|r| r.leased_by.is_none())
            .unwrap_or(false),
        dirty: w.git.dirty,
        untracked: w.git.untracked,
        unpushed: w.git.unpushed.clone(),
        own_commits: w.git.own_commits,
        detached: w.detached,
        open_in_app,
        registered: registry.is_some(),
        has_desktop_record,
        orphan: w.orphan,
        minutes_since_modified: if w.last_modified > 0 {
            (now - w.last_modified) / 60_000
        } else {
            i64::MAX
        },
        git_error: w.git.error.clone(),
    };
    w.state = safety::classify(&input);
    w.prune_allowed = safety::prune_allowed(&w.state, w.is_main);
    w.sessions = sessions;
    w.registry = registry;
    w.created_at = created;
    w.source_branch = source_branch;
}

fn reclaimable(w: &Worktree) -> u64 {
    match w.state {
        SafetyState::Safe => w.bytes_total,
        _ if w.prune_allowed => w
            .artifacts
            .iter()
            .filter(|a| !a.tracked)
            .map(|a| a.bytes)
            .sum(),
        _ => 0,
    }
}

pub fn scan(opts: &ScanOptions, sink: &dyn ProgressSink, cancel: &AtomicBool) -> ScanReport {
    let start = Instant::now();
    let now = paths::now_ms();
    let progress = |phase: &str, current: &str| {
        sink.on(ScanEvent::Progress {
            phase: phase.into(),
            current: current.into(),
        })
    };

    progress("sessions", "");
    let live = live::load();
    let desktop = desktop::load_sessions();
    let registry = desktop::load_registry();

    progress("transcripts", "");
    let (mut transcripts, mut errors) = transcripts::scan_all(cancel);

    let ix = build_index(&live, &desktop, registry.as_ref(), &transcripts);

    let mut extra: Vec<PathBuf> = Vec::new();
    if let Some(r) = &registry {
        extra.extend(
            r.worktrees
                .values()
                .filter_map(|e| e.base_repo.clone())
                .map(PathBuf::from),
        );
    }
    extra.extend(
        desktop
            .iter()
            .filter_map(|d| d.origin_cwd.clone())
            .map(PathBuf::from),
    );
    let repos = worktrees::discover_repos(&opts.roots, &extra);

    let errs = Mutex::new(Vec::new());
    let mut repos: Vec<Repo> = repos
        .par_iter()
        .filter_map(|r| {
            progress("worktrees", &r.to_string_lossy());
            match worktrees::scan_repo(r, cancel) {
                Ok(mut repo) => {
                    enrich(&mut repo.main, &ix, now);
                    for w in &mut repo.worktrees {
                        enrich(w, &ix, now);
                    }
                    sink.on(ScanEvent::Repo {
                        repo: Box::new(repo.clone()),
                    });
                    Some(repo)
                }
                Err(e) => {
                    errs.lock().unwrap().push(e);
                    None
                }
            }
        })
        .collect();
    errors.extend(errs.into_inner().unwrap());
    repos.sort_by_key(|r| std::cmp::Reverse(r.total_bytes));

    progress("buckets", "");
    let buckets = buckets::scan_all(cancel);

    for t in &mut transcripts {
        let Some(c) = &t.cwd else { continue };
        if t.cwd_exists {
            continue;
        }
        let k = norm_key(c);
        let in_use = ix.live_by_cwd.contains_key(&k)
            || ix
                .desktop_by_cwd
                .get(&k)
                .map(|l| l.iter().any(|d| !d.is_archived))
                .unwrap_or(false);
        t.forgettable = !in_use;
    }

    let totals = compute_totals(&repos, &transcripts, &buckets);

    ScanReport {
        scanned_at: now,
        duration_ms: start.elapsed().as_millis() as u64,
        repos,
        transcript_dirs: transcripts,
        buckets,
        totals,
        errors,
    }
}

pub fn compute_totals(repos: &[Repo], transcripts: &[TranscriptDir], buckets: &[Bucket]) -> Totals {
    let mut totals = Totals::default();
    for r in repos {
        for w in std::iter::once(&r.main).chain(r.worktrees.iter()) {
            if !w.is_main {
                totals.worktree_bytes += w.bytes_total;
                totals.worktree_count += 1;
            }
            totals.artifact_bytes += w.artifacts.iter().map(|a| a.bytes).sum::<u64>();
            totals.reclaimable_bytes += reclaimable(w);
        }
    }
    let transcript_bytes: u64 = transcripts.iter().map(|t| t.bytes).sum();
    totals.reclaimable_bytes += transcripts
        .iter()
        .filter(|t| t.forgettable)
        .map(|t| t.bytes)
        .sum::<u64>();
    totals.session_count = transcripts.iter().map(|t| t.sessions.len()).sum();
    totals.claude_bytes = totals.worktree_bytes
        + transcript_bytes
        + buckets
            .iter()
            .filter(|b| b.claude && b.label != "Transcripts")
            .map(|b| b.bytes)
            .sum::<u64>();
    totals
}

/// Re-scan one repo (sizes, git state, sessions, safety) using the transcripts
/// from an earlier full scan. Used to refresh a repo after an action.
pub fn rescan_repo(
    repo: &std::path::Path,
    transcripts: &[TranscriptDir],
    cancel: &AtomicBool,
) -> Result<Repo, ScanError> {
    let live = live::load();
    let desktop = desktop::load_sessions();
    let registry = desktop::load_registry();
    let ix = build_index(&live, &desktop, registry.as_ref(), transcripts);
    let now = paths::now_ms();
    let mut r = worktrees::scan_repo(repo, cancel)?;
    enrich(&mut r.main, &ix, now);
    for w in &mut r.worktrees {
        enrich(w, &ix, now);
    }
    Ok(r)
}
