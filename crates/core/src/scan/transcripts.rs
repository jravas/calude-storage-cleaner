//! ~/.claude/projects/<encoded cwd>/: one dir per working directory,
//! one <sessionId>.jsonl per session plus sidecar dirs.

use crate::fsize::size_tree;
use crate::model::{ScanError, Session, TranscriptDir};
use crate::paths;
use rayon::prelude::*;
use serde::Deserialize;
use std::io::{BufRead, BufReader};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Rec {
    #[serde(rename = "type")]
    kind: Option<String>,
    cwd: Option<String>,
    timestamp: Option<String>,
    git_branch: Option<String>,
    entrypoint: Option<String>,
    custom_title: Option<String>,
    ai_title: Option<String>,
    pr_url: Option<String>,
}

fn parse_ts(s: &str) -> Option<i64> {
    s.parse::<jiff::Timestamp>()
        .ok()
        .map(|t| t.as_millisecond())
}

fn blocks(p: &Path) -> u64 {
    std::fs::symlink_metadata(p)
        .map(|m| m.blocks() * 512)
        .unwrap_or(0)
}

pub fn parse_session(jsonl: &Path) -> Option<Session> {
    let stem = jsonl.file_stem()?.to_string_lossy().into_owned();
    let file = std::fs::File::open(jsonl).ok()?;
    let mut s = Session {
        cli_id: stem.clone(),
        transcript_bytes: blocks(jsonl),
        ..Default::default()
    };
    let mut ai_title = None;
    let reader = BufReader::with_capacity(1 << 20, file);
    for line in reader.split(b'\n') {
        let Ok(line) = line else { break };
        if line.is_empty() {
            continue;
        }
        let Ok(r) = serde_json::from_slice::<Rec>(&line) else {
            continue;
        };
        if s.cwd.is_none() {
            if let Some(c) = r.cwd {
                s.cwd = Some(PathBuf::from(c));
            }
        }
        if let Some(t) = r.timestamp.as_deref().and_then(parse_ts) {
            if s.first_ts.is_none() {
                s.first_ts = Some(t);
            }
            s.last_ts = Some(t);
        }
        if r.git_branch.is_some() {
            s.branch = r.git_branch;
        }
        if s.entrypoint.is_none() && r.entrypoint.is_some() {
            s.entrypoint = r.entrypoint;
        }
        match r.kind.as_deref() {
            Some("custom-title") => {
                if r.custom_title.is_some() {
                    s.title = r.custom_title;
                }
            }
            Some("ai-title") => {
                if r.ai_title.is_some() {
                    ai_title = r.ai_title;
                }
            }
            Some("pr-link") if r.pr_url.is_some() => s.pr_url = r.pr_url,
            _ => {}
        }
    }
    if s.title.is_none() {
        s.title = ai_title;
    }
    if s.last_ts.is_none() {
        s.last_ts = std::fs::metadata(jsonl).ok().map(|m| m.mtime() * 1000);
    }
    let dir = jsonl.parent()?;
    let sidecar = dir.join(&stem);
    if sidecar.is_dir() {
        s.sidecar_bytes = size_tree(&sidecar, &AtomicBool::new(false)).bytes;
    }
    s.released = dir.join(format!("{stem}.desktop-released.json")).exists();
    Some(s)
}

pub fn scan_dir(dir: &Path, cancel: &AtomicBool) -> Option<TranscriptDir> {
    let name = dir.file_name()?.to_string_lossy().into_owned();
    let rd = std::fs::read_dir(dir).ok()?;
    let mut jsonls = Vec::new();
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().and_then(|s| s.to_str()) == Some("jsonl") {
            jsonls.push(p);
        }
    }
    let mut sessions: Vec<Session> = jsonls.par_iter().filter_map(|p| parse_session(p)).collect();
    sessions.sort_by_key(|s| std::cmp::Reverse(s.last_ts));
    let memory = dir.join("memory");
    let memory_bytes = if memory.is_dir() {
        size_tree(&memory, cancel).bytes
    } else {
        0
    };
    let bytes = size_tree(dir, cancel).bytes;
    let cwd = sessions.iter().find_map(|s| s.cwd.clone());
    let cwd_exists = cwd.as_ref().map(|c| c.is_dir()).unwrap_or(false);
    Some(TranscriptDir {
        path: dir.to_path_buf(),
        name,
        cwd,
        cwd_exists,
        bytes,
        memory_bytes,
        sessions,
        forgettable: false,
    })
}

pub fn scan_all(cancel: &AtomicBool) -> (Vec<TranscriptDir>, Vec<ScanError>) {
    let root = paths::claude_dir().join("projects");
    let Ok(rd) = std::fs::read_dir(&root) else {
        return (
            Vec::new(),
            vec![ScanError {
                path: root,
                message: "no transcripts directory".into(),
            }],
        );
    };
    let dirs: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    let mut out: Vec<TranscriptDir> = dirs
        .par_iter()
        .filter_map(|d| scan_dir(d, cancel))
        .collect();
    out.sort_by_key(|t| std::cmp::Reverse(t.bytes));
    (out, Vec::new())
}
