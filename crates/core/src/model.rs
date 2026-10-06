use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ScanReport {
    pub scanned_at: i64,
    pub duration_ms: u64,
    pub repos: Vec<Repo>,
    pub transcript_dirs: Vec<TranscriptDir>,
    pub buckets: Vec<Bucket>,
    pub totals: Totals,
    pub errors: Vec<ScanError>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Totals {
    /// Worktrees + transcripts + the desktop app's own data.
    pub claude_bytes: u64,
    pub worktree_bytes: u64,
    pub artifact_bytes: u64,
    /// Safe worktrees in full, plus prunable artifacts elsewhere, plus forgettable transcripts.
    pub reclaimable_bytes: u64,
    pub worktree_count: usize,
    pub session_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanError {
    pub path: PathBuf,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Repo {
    pub path: PathBuf,
    pub name: String,
    pub origin: Option<String>,
    pub main: Worktree,
    pub worktrees: Vec<Worktree>,
    /// Registered with git but no longer on disk; `git worktree prune` clears them.
    pub stale_entries: Vec<PathBuf>,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Worktree {
    pub path: PathBuf,
    pub name: String,
    pub repo: PathBuf,
    pub is_main: bool,
    /// On disk under .claude/worktrees but unknown to `git worktree list`.
    pub orphan: bool,
    pub head: String,
    pub branch: Option<String>,
    pub detached: bool,
    pub source_branch: Option<String>,
    pub bytes_total: u64,
    pub bytes_checkout: u64,
    pub artifacts: Vec<Artifact>,
    pub last_modified: i64,
    pub created_at: Option<i64>,
    pub git: GitState,
    pub registry: Option<RegistryEntry>,
    pub sessions: Vec<SessionRef>,
    pub state: SafetyState,
    pub prune_allowed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    pub kind: ArtifactKind,
    pub path: PathBuf,
    pub bytes: u64,
    pub files: u64,
    /// Committed to git; shown but never pruned.
    pub tracked: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ArtifactKind {
    Terraform,
    NodeModules,
    CargoTarget,
    Next,
    Turbo,
    Dist,
}

impl ArtifactKind {
    pub fn label(self) -> &'static str {
        match self {
            ArtifactKind::Terraform => ".terraform",
            ArtifactKind::NodeModules => "node_modules",
            ArtifactKind::CargoTarget => "target",
            ArtifactKind::Next => ".next",
            ArtifactKind::Turbo => ".turbo",
            ArtifactKind::Dist => "dist",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GitState {
    pub dirty: u32,
    pub untracked: u32,
    pub unpushed: Unpushed,
    /// Commits reachable from HEAD that no remote ref contains. None when the repo has no remote.
    pub not_on_remote: Option<u32>,
    /// Commits reachable from HEAD that no other local branch or remote contains.
    pub own_commits: u32,
    pub locked: Option<String>,
    pub prunable: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(tag = "kind", content = "count", rename_all = "camelCase")]
pub enum Unpushed {
    Count(u32),
    #[default]
    NoUpstream,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(tag = "kind", content = "reasons", rename_all = "camelCase")]
pub enum SafetyState {
    InUse(Vec<InUseReason>),
    Pooled,
    Caution(Vec<CautionReason>),
    #[default]
    Safe,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum InUseReason {
    LiveProcess { pid: u32, session: String },
    GitLocked { reason: Option<String> },
    MainCheckout,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum CautionReason {
    Dirty { files: u32 },
    Untracked { files: u32 },
    Unpushed { commits: u32 },
    NoUpstreamWithCommits { commits: u32 },
    DetachedWithOwnCommits { commits: u32 },
    OpenInApp { desktop_id: String },
    Unregistered,
    Orphan,
    ModifiedRecently { minutes: u32 },
    GitError { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct RegistryEntry {
    pub name: String,
    pub path: String,
    pub leased_by: Option<String>,
    pub base_repo: Option<String>,
    pub branch: Option<String>,
    pub source_branch: Option<String>,
    pub created_at: Option<i64>,
    pub pooled_at: Option<i64>,
    pub unclean_since: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRef {
    pub cli_id: Option<String>,
    pub desktop_id: Option<String>,
    pub title: Option<String>,
    pub archived: bool,
    pub last_activity: Option<i64>,
    pub live: bool,
    pub transcript_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub cli_id: String,
    pub desktop_id: Option<String>,
    pub title: Option<String>,
    pub cwd: Option<PathBuf>,
    pub branch: Option<String>,
    pub entrypoint: Option<String>,
    pub first_ts: Option<i64>,
    pub last_ts: Option<i64>,
    pub archived: Option<bool>,
    pub released: bool,
    pub live: bool,
    pub transcript_bytes: u64,
    pub sidecar_bytes: u64,
    pub pr_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptDir {
    pub path: PathBuf,
    pub name: String,
    pub cwd: Option<PathBuf>,
    pub cwd_exists: bool,
    pub bytes: u64,
    pub memory_bytes: u64,
    pub sessions: Vec<Session>,
    pub forgettable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bucket {
    pub label: String,
    pub path: PathBuf,
    pub bytes: u64,
    pub apparent_bytes: Option<u64>,
    pub claude: bool,
    pub note: Option<String>,
}
