//! The desktop app's own records: session index and worktree registry.

use crate::model::RegistryEntry;
use crate::paths;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct DesktopSession {
    pub session_id: String,
    pub cli_session_id: Option<String>,
    pub prior_cli_session_ids: Vec<String>,
    pub cwd: Option<String>,
    pub origin_cwd: Option<String>,
    pub worktree_path: Option<String>,
    pub branch: Option<String>,
    pub source_branch: Option<String>,
    pub created_at: Option<i64>,
    pub last_activity_at: Option<i64>,
    pub is_archived: bool,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Registry {
    pub schema_version: u32,
    pub worktrees: HashMap<String, RegistryEntry>,
}

pub fn sessions_dir() -> PathBuf {
    paths::app_support().join("claude-code-sessions")
}

pub fn registry_path() -> PathBuf {
    paths::app_support().join("git-worktrees.json")
}

/// claude-code-sessions/<account>/<session>/local_<id>.json
pub fn load_sessions() -> Vec<DesktopSession> {
    let mut out = Vec::new();
    let Ok(accounts) = std::fs::read_dir(sessions_dir()) else {
        return out;
    };
    for acc in accounts.flatten() {
        let Ok(sessions) = std::fs::read_dir(acc.path()) else {
            continue;
        };
        for s in sessions.flatten() {
            let Ok(files) = std::fs::read_dir(s.path()) else {
                continue;
            };
            for f in files.flatten() {
                let name = f.file_name();
                let name = name.to_string_lossy();
                if !(name.starts_with("local_") && name.ends_with(".json")) {
                    continue;
                }
                if let Ok(text) = std::fs::read_to_string(f.path()) {
                    if let Ok(d) = serde_json::from_str::<DesktopSession>(&text) {
                        out.push(d);
                    }
                }
            }
        }
    }
    out
}

pub fn load_registry() -> Option<Registry> {
    let text = std::fs::read_to_string(registry_path()).ok()?;
    serde_json::from_str(&text).ok()
}
