use cleaner_core::{
    execute, human_bytes, preview, scan, ActionRequest, ArtifactKind, ProgressSink, SafetyState,
    ScanEvent, ScanOptions, Worktree,
};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

struct Stderr;
impl ProgressSink for Stderr {
    fn on(&self, ev: ScanEvent) {
        if let ScanEvent::Progress { phase, current } = ev {
            let mut e = std::io::stderr().lock();
            let _ = writeln!(e, "… {phase} {current}");
        }
    }
}

fn state_str(w: &Worktree) -> String {
    match &w.state {
        SafetyState::Safe => "safe".into(),
        SafetyState::Pooled => "pooled".into(),
        SafetyState::InUse(r) => format!("IN USE {}", serde_json::to_string(r).unwrap_or_default()),
        SafetyState::Caution(r) => {
            format!("caution {}", serde_json::to_string(r).unwrap_or_default())
        }
    }
}

fn usage() {
    eprintln!("usage:");
    eprintln!("  scan [--json] [root...]                         scan (default root: ~/Projects)");
    eprintln!("  scan prune <worktree> [--permanent] [--yes]     remove build artifacts");
    eprintln!("  scan remove <worktree> [--delete-branch] [--permanent] [--yes]");
    eprintln!("  scan forget <transcript-dir>... [--permanent] [--yes]");
}

fn action(args: &[String]) {
    let flag = |f: &str| args.iter().any(|a| a == f);
    let paths: Vec<PathBuf> = args[1..]
        .iter()
        .filter(|a| !a.starts_with("--"))
        .map(PathBuf::from)
        .collect();
    let permanent = flag("--permanent");
    let req = match (args[0].as_str(), paths.as_slice()) {
        ("prune", [w]) => ActionRequest::PruneArtifacts {
            worktree: w.clone(),
            kinds: vec![
                ArtifactKind::Terraform,
                ArtifactKind::NodeModules,
                ArtifactKind::CargoTarget,
                ArtifactKind::Next,
                ArtifactKind::Turbo,
                ArtifactKind::Dist,
            ],
            permanent,
        },
        ("remove", [w]) => ActionRequest::RemoveWorktree {
            worktree: w.clone(),
            delete_branch: flag("--delete-branch"),
            permanent,
        },
        ("forget", dirs) if !dirs.is_empty() => ActionRequest::ForgetTranscripts {
            dirs: dirs.to_vec(),
            permanent,
        },
        _ => {
            usage();
            std::process::exit(2);
        }
    };
    eprintln!("scanning…");
    let report = scan(&ScanOptions::default(), &Stderr, &AtomicBool::new(false));
    let plan = match preview(req, &report) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };
    println!(
        "Plan ({} step(s), {}):",
        plan.steps.len(),
        human_bytes(plan.bytes)
    );
    for st in &plan.steps {
        println!(
            "  {:?} {} {}",
            st.kind,
            st.path.display(),
            if st.bytes > 0 {
                human_bytes(st.bytes)
            } else {
                String::new()
            }
        );
    }
    for w in &plan.warnings {
        println!("  warning: {w}");
    }
    for b in &plan.blockers {
        println!("  BLOCKED: {b}");
    }
    if !flag("--yes") {
        println!("dry run; add --yes to execute");
        return;
    }
    match execute(&plan, flag("--i-understand-this-discards-work")) {
        Ok(r) => {
            println!(
                "done: {} step(s), {} to Trash, {} deleted",
                r.done.len(),
                human_bytes(r.bytes_trashed),
                human_bytes(r.bytes_freed)
            );
            if let Some(f) = r.failed {
                println!("FAILED at {}: {}", f.step.label, f.message);
                std::process::exit(1);
            }
        }
        Err(e) => {
            println!("refused: {e}");
            std::process::exit(1);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if matches!(
        args.first().map(|s| s.as_str()),
        Some("prune" | "remove" | "forget")
    ) {
        return action(&args);
    }
    let mut json = false;
    let mut roots = Vec::new();
    for a in &args {
        match a.as_str() {
            "--json" => json = true,
            "-h" | "--help" => {
                usage();
                return;
            }
            p => roots.push(PathBuf::from(p)),
        }
    }
    let mut opts = ScanOptions::default();
    if !roots.is_empty() {
        opts.roots = roots;
    }
    let cancel = AtomicBool::new(false);
    let report = scan(&opts, &Stderr, &cancel);
    if json {
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
        return;
    }
    for r in &report.repos {
        println!(
            "\n{}  {}{}",
            r.name,
            human_bytes(r.total_bytes),
            if r.stale_entries.is_empty() {
                String::new()
            } else {
                format!("  ({} stale git entries)", r.stale_entries.len())
            }
        );
        for w in std::iter::once(&r.main).chain(r.worktrees.iter()) {
            let arts: Vec<String> = w
                .artifacts
                .iter()
                .fold(
                    std::collections::BTreeMap::<&str, u64>::new(),
                    |mut m, a| {
                        *m.entry(a.kind.label()).or_default() += a.bytes;
                        m
                    },
                )
                .into_iter()
                .map(|(k, b)| format!("{k} {}", human_bytes(b)))
                .collect();
            let title = w
                .sessions
                .first()
                .and_then(|s| s.title.clone())
                .unwrap_or_default();
            println!(
                "  {:<44} {:>9}  {:<34} d{:<3} u{:<9} {}  {}",
                if w.is_main {
                    "(main)".to_string()
                } else {
                    w.name.clone()
                },
                human_bytes(w.bytes_total),
                arts.join(", "),
                w.git.dirty + w.git.untracked,
                match &w.git.unpushed {
                    cleaner_core::Unpushed::Count(n) => n.to_string(),
                    cleaner_core::Unpushed::NoUpstream => format!("noup/{}", w.git.own_commits),
                },
                state_str(w),
                title
            );
        }
    }
    println!(
        "\nTranscripts: {} dirs, {} sessions, {} forgettable",
        report.transcript_dirs.len(),
        report.totals.session_count,
        report
            .transcript_dirs
            .iter()
            .filter(|t| t.forgettable)
            .count()
    );
    println!("\nBuckets:");
    for b in &report.buckets {
        println!(
            "  {:<24} {:>9}  {}",
            b.label,
            human_bytes(b.bytes),
            b.path.display()
        );
    }
    let t = &report.totals;
    println!("\nClaude on disk {}  worktrees {} ({})  artifacts {}  reclaimable {}  errors {}  in {:.1}s",
        human_bytes(t.claude_bytes), t.worktree_count, human_bytes(t.worktree_bytes),
        human_bytes(t.artifact_bytes), human_bytes(t.reclaimable_bytes), report.errors.len(),
        report.duration_ms as f64 / 1000.0);
}
