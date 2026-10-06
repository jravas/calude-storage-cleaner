//! Fixed list of well-known directories, sized read-only.

use crate::fsize::size_tree;
use crate::model::Bucket;
use crate::paths;
use rayon::prelude::*;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

struct Spec {
    label: &'static str,
    path: PathBuf,
    claude: bool,
    note: Option<&'static str>,
}

pub fn scan_all(cancel: &AtomicBool) -> Vec<Bucket> {
    let home = paths::home();
    let claude = paths::claude_dir();
    let app = paths::app_support();
    let specs = vec![
        Spec {
            label: "Transcripts",
            path: claude.join("projects"),
            claude: true,
            note: Some("Deleted by Claude Code after cleanupPeriodDays (default 30)."),
        },
        Spec {
            label: "File history",
            path: claude.join("file-history"),
            claude: true,
            note: None,
        },
        Spec {
            label: "Debug logs",
            path: claude.join("debug"),
            claude: true,
            note: None,
        },
        Spec {
            label: "Telemetry",
            path: claude.join("telemetry"),
            claude: true,
            note: None,
        },
        Spec {
            label: "Shell snapshots",
            path: claude.join("shell-snapshots"),
            claude: true,
            note: None,
        },
        Spec {
            label: "Todos",
            path: claude.join("todos"),
            claude: true,
            note: None,
        },
        Spec {
            label: "Plans",
            path: claude.join("plans"),
            claude: true,
            note: None,
        },
        Spec {
            label: "VM bundle",
            path: app.join("vm_bundles"),
            claude: true,
            note: Some("Local VM image for cloud/VM sessions."),
        },
        Spec {
            label: "Web cache",
            path: app.join("Cache"),
            claude: true,
            note: Some("Clear from Settings › Storage in the Claude app."),
        },
        Spec {
            label: "Code cache",
            path: app.join("Code Cache"),
            claude: true,
            note: None,
        },
        Spec {
            label: "Claude Code versions",
            path: app.join("claude-code"),
            claude: true,
            note: None,
        },
        Spec {
            label: "Session records",
            path: app.join("claude-code-sessions"),
            claude: true,
            note: None,
        },
        Spec {
            label: "npx cache",
            path: home.join(".npm/_npx"),
            claude: false,
            note: Some("Rebuilt on demand."),
        },
        Spec {
            label: "npm cache",
            path: home.join(".npm/_cacache"),
            claude: false,
            note: None,
        },
        Spec {
            label: "pnpm store",
            path: home.join("Library/pnpm"),
            claude: false,
            note: None,
        },
        Spec {
            label: "Yarn cache",
            path: home.join("Library/Caches/Yarn"),
            claude: false,
            note: None,
        },
        Spec {
            label: "Puppeteer browsers",
            path: home.join(".cache/puppeteer"),
            claude: false,
            note: None,
        },
        Spec {
            label: "Playwright browsers",
            path: home.join("Library/Caches/ms-playwright"),
            claude: false,
            note: None,
        },
        Spec {
            label: "Cursor worktrees",
            path: home.join(".cursor/worktrees"),
            claude: false,
            note: Some("Same pattern as Claude worktrees."),
        },
    ];
    let mut out: Vec<Bucket> = specs
        .into_par_iter()
        .filter(|s| s.path.is_dir())
        .map(|s| Bucket {
            label: s.label.to_string(),
            bytes: size_tree(&s.path, cancel).bytes,
            path: s.path,
            apparent_bytes: None,
            claude: s.claude,
            note: s.note.map(String::from),
        })
        .collect();
    let docker = home.join("Library/Containers/com.docker.docker/Data/vms/0/data/Docker.raw");
    if let Ok(m) = std::fs::symlink_metadata(&docker) {
        out.push(Bucket {
            label: "Docker VM disk".into(),
            path: docker,
            bytes: m.blocks() * 512,
            apparent_bytes: Some(m.len()),
            claude: false,
            note: Some("Sparse file: apparent size is the maximum, not what is used.".into()),
        });
    }
    out.sort_by_key(|b| std::cmp::Reverse(b.bytes));
    out
}
