//! Cleanup actions: build a plan from the last scan, re-validate, execute.
//! Everything goes to the Trash unless `permanent` is set.

use crate::git;
use crate::model::*;
use crate::paths::{self, norm_key};
use crate::scan::live;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ActionRequest {
    #[serde(rename_all = "camelCase")]
    PruneArtifacts {
        worktree: PathBuf,
        kinds: Vec<ArtifactKind>,
        permanent: bool,
    },
    #[serde(rename_all = "camelCase")]
    RemoveWorktree {
        worktree: PathBuf,
        delete_branch: bool,
        permanent: bool,
    },
    #[serde(rename_all = "camelCase")]
    ForgetTranscripts { dirs: Vec<PathBuf>, permanent: bool },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StepKind {
    Trash,
    Delete,
    GitWorktreePrune,
    GitBranchDelete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub label: String,
    pub path: PathBuf,
    pub bytes: u64,
    pub kind: StepKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionPlan {
    pub id: String,
    pub request: ActionRequest,
    pub repo: Option<PathBuf>,
    pub steps: Vec<Step>,
    pub bytes: u64,
    pub warnings: Vec<String>,
    pub blockers: Vec<String>,
    /// True when the plan discards work (changed files or commits that exist nowhere else).
    pub discards_work: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FailedStep {
    pub step: Step,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionResult {
    pub plan_id: String,
    pub repo: Option<PathBuf>,
    pub done: Vec<Step>,
    pub failed: Option<FailedStep>,
    pub bytes_trashed: u64,
    pub bytes_freed: u64,
}

fn new_id() -> String {
    format!("{:x}-{}", paths::now_ms(), std::process::id())
}

fn find_worktree<'a>(report: &'a ScanReport, path: &Path) -> Option<(&'a Repo, &'a Worktree)> {
    let key = norm_key(path);
    for r in &report.repos {
        if norm_key(&r.main.path) == key {
            return Some((r, &r.main));
        }
        if let Some(w) = r.worktrees.iter().find(|w| norm_key(&w.path) == key) {
            return Some((r, w));
        }
    }
    None
}

/// Hard refusals that do not depend on scan state.
fn check_target(path: &Path) -> Result<(), String> {
    let canon = std::fs::canonicalize(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if canon != path {
        return Err(format!(
            "{} is a symlink or not canonical ({}); refusing",
            path.display(),
            canon.display()
        ));
    }
    if path.components().count() < 4 {
        return Err(format!(
            "{} is too close to the root; refusing",
            path.display()
        ));
    }
    let home = paths::home();
    if path == home || !path.starts_with(&home) {
        return Err(format!(
            "{} is outside the home folder; refusing",
            path.display()
        ));
    }
    Ok(())
}

fn removal_step(label: String, path: PathBuf, bytes: u64, permanent: bool) -> Step {
    Step {
        label,
        path,
        bytes,
        kind: if permanent {
            StepKind::Delete
        } else {
            StepKind::Trash
        },
    }
}

pub fn preview(req: ActionRequest, report: &ScanReport) -> Result<ActionPlan, String> {
    let mut plan = ActionPlan {
        id: new_id(),
        request: req.clone(),
        repo: None,
        steps: Vec::new(),
        bytes: 0,
        warnings: Vec::new(),
        blockers: Vec::new(),
        discards_work: false,
    };
    match req {
        ActionRequest::PruneArtifacts {
            worktree,
            kinds,
            permanent,
        } => {
            let (repo, w) = find_worktree(report, &worktree)
                .ok_or("this folder is not in the last scan; rescan first")?;
            plan.repo = Some(repo.path.clone());
            if let Err(e) = check_target(&w.path) {
                plan.blockers.push(e);
            }
            if !w.prune_allowed {
                plan.blockers
                    .push("this worktree is in use; artifacts stay".into());
            }
            let mut tracked = 0;
            let mut gone = 0;
            for a in &w.artifacts {
                if !kinds.contains(&a.kind) {
                    continue;
                }
                if a.tracked {
                    tracked += 1;
                    continue;
                }
                if !a.path.exists() {
                    gone += 1;
                    continue;
                }
                if !a.path.starts_with(&w.path)
                    || a.path
                        .file_name()
                        .map(|n| n.to_string_lossy() != a.kind.label())
                        .unwrap_or(true)
                {
                    plan.blockers
                        .push(format!("unexpected artifact path {}", a.path.display()));
                    continue;
                }
                let rel = a
                    .path
                    .strip_prefix(&w.path)
                    .unwrap_or(&a.path)
                    .display()
                    .to_string();
                plan.steps
                    .push(removal_step(rel, a.path.clone(), a.bytes, permanent));
            }
            if tracked > 0 {
                plan.warnings.push(format!(
                    "{tracked} artifact folder(s) are committed to git and are kept"
                ));
            }
            if gone > 0 {
                plan.warnings.push(format!("{gone} folder(s) listed in the last scan are already gone; rescan to refresh sizes"));
            }
            if plan.steps.is_empty() && plan.blockers.is_empty() {
                plan.blockers.push(if gone > 0 {
                    "already pruned; rescan".into()
                } else {
                    "nothing to prune".into()
                });
            }
            match &w.state {
                SafetyState::Caution(_) | SafetyState::Pooled => plan.warnings.push(
                    "Only build artifacts are removed; the checkout and its changes stay.".into(),
                ),
                _ => {}
            }
            if w.artifacts
                .iter()
                .any(|a| a.kind == ArtifactKind::Terraform && kinds.contains(&a.kind))
            {
                plan.warnings
                    .push("Run `terraform init` again before using this checkout.".into());
            }
        }
        ActionRequest::RemoveWorktree {
            worktree,
            delete_branch,
            permanent,
        } => {
            let (repo, w) = find_worktree(report, &worktree)
                .ok_or("this folder is not in the last scan; rescan first")?;
            plan.repo = Some(repo.path.clone());
            if let Err(e) = check_target(&w.path) {
                plan.blockers.push(e);
            }
            if w.is_main {
                plan.blockers
                    .push("the main checkout cannot be removed".into());
            }
            if !w.path.is_dir() {
                plan.blockers
                    .push("this worktree is already gone; rescan".into());
            }
            if !w.path.starts_with(repo.path.join(".claude/worktrees")) {
                plan.blockers
                    .push("only worktrees under .claude/worktrees can be removed here".into());
            }
            match &w.state {
                SafetyState::InUse(reasons) => {
                    for r in reasons {
                        plan.blockers.push(match r {
                            InUseReason::LiveProcess { pid, .. } => format!("a Claude session is running in it (pid {pid})"),
                            InUseReason::GitLocked { reason } => format!("git has it locked{}", reason.as_ref().map(|r| format!(": {r}")).unwrap_or_default()),
                            InUseReason::MainCheckout => "it is the main checkout".into(),
                        });
                    }
                }
                SafetyState::Pooled => plan.warnings.push(
                    "The Claude app keeps this as a spare for new sessions. It will create a new one when needed.".into(),
                ),
                SafetyState::Caution(reasons) => {
                    for r in reasons {
                        match r {
                            CautionReason::Dirty { files } => { plan.discards_work = true; plan.warnings.push(format!("Discards {files} changed file(s).")); }
                            CautionReason::Untracked { files } => { plan.discards_work = true; plan.warnings.push(format!("Discards {files} untracked file(s).")); }
                            CautionReason::Unpushed { commits } => { plan.discards_work = true; plan.warnings.push(format!("{commits} commit(s) are not pushed; the branch stays so they are not lost.")); }
                            CautionReason::NoUpstreamWithCommits { commits } => { plan.discards_work = true; plan.warnings.push(format!("{commits} commit(s) exist only on this branch; the branch stays.")); }
                            CautionReason::DetachedWithOwnCommits { commits } => { plan.discards_work = true; plan.warnings.push(format!("{commits} commit(s) are on no branch and would be lost.")); }
                            CautionReason::OpenInApp { .. } => plan.warnings.push("The session is still open in the Claude app; it will reopen without this folder.".into()),
                            CautionReason::Unregistered => plan.warnings.push("Not created by the Claude app.".into()),
                            CautionReason::Orphan => plan.warnings.push("git does not list this folder as a worktree.".into()),
                            CautionReason::ModifiedRecently { minutes } => plan.warnings.push(format!("Modified {minutes} min ago.")),
                            CautionReason::GitError { message } => plan.blockers.push(format!("git state unknown: {message}")),
                        }
                    }
                }
                SafetyState::Safe => {}
            }
            plan.steps.push(removal_step(
                format!("worktree {}", w.name),
                w.path.clone(),
                w.bytes_total,
                permanent,
            ));
            plan.steps.push(Step {
                label: "git worktree prune".into(),
                path: repo.path.clone(),
                bytes: 0,
                kind: StepKind::GitWorktreePrune,
            });
            if delete_branch {
                match &w.branch {
                    None => plan
                        .warnings
                        .push("Detached HEAD: no branch to delete.".into()),
                    Some(b) => {
                        let elsewhere = std::iter::once(&repo.main)
                            .chain(repo.worktrees.iter())
                            .any(|o| o.path != w.path && o.branch.as_deref() == Some(b));
                        let is_source = w.source_branch.as_deref() == Some(b)
                            || repo.main.branch.as_deref() == Some(b);
                        if elsewhere || is_source {
                            plan.warnings.push(format!(
                                "Branch {b} is checked out elsewhere or is the base branch; kept."
                            ));
                        } else if w.git.not_on_remote != Some(0) {
                            plan.warnings.push(format!(
                                "Branch {b} has commits that are on no remote; kept."
                            ));
                        } else {
                            plan.steps.push(Step {
                                label: format!("delete branch {b}"),
                                path: repo.path.clone(),
                                bytes: 0,
                                kind: StepKind::GitBranchDelete,
                            });
                        }
                    }
                }
            }
        }
        ActionRequest::ForgetTranscripts { dirs, permanent } => {
            let root = paths::claude_dir().join("projects");
            for d in dirs {
                let Some(t) = report
                    .transcript_dirs
                    .iter()
                    .find(|t| norm_key(&t.path) == norm_key(&d))
                else {
                    plan.blockers
                        .push(format!("{} is not in the last scan", d.display()));
                    continue;
                };
                if !t.path.starts_with(&root) {
                    plan.blockers.push(format!(
                        "{} is not under ~/.claude/projects",
                        t.path.display()
                    ));
                    continue;
                }
                if !t.forgettable {
                    plan.blockers.push(format!(
                        "{} still belongs to an existing folder or an open session",
                        t.name
                    ));
                    continue;
                }
                if let Err(e) = check_target(&t.path) {
                    plan.blockers.push(e);
                    continue;
                }
                plan.steps.push(removal_step(
                    format!(
                        "transcripts for {}",
                        t.cwd
                            .as_ref()
                            .map(|c| c.display().to_string())
                            .unwrap_or(t.name.clone())
                    ),
                    t.path.clone(),
                    t.bytes,
                    permanent,
                ));
            }
            plan.warnings
                .push("These sessions disappear from the Claude app's history.".into());
        }
    }
    plan.bytes = plan.steps.iter().map(|s| s.bytes).sum();
    Ok(plan)
}

fn log_line(line: &str) {
    let dir = paths::home().join("Library/Logs/cleaner-app");
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("actions.log"))
    {
        let _ = writeln!(f, "{} {}", paths::now_ms(), line);
    }
}

/// Last-moment checks against the live system, independent of the scan.
fn revalidate(plan: &ActionPlan) -> Result<(), String> {
    let live = live::load();
    for s in &plan.steps {
        if !matches!(s.kind, StepKind::Trash | StepKind::Delete) {
            continue;
        }
        if !s.path.exists() {
            return Err(format!("{} no longer exists; rescan", s.path.display()));
        }
        check_target(&s.path)?;
        let key = norm_key(&s.path);
        for l in live.iter().filter(|l| l.alive) {
            let lk = norm_key(Path::new(&l.cwd));
            if lk == key || lk.starts_with(&format!("{key}/")) {
                return Err(format!(
                    "a Claude session (pid {}) is running inside {}",
                    l.pid,
                    s.path.display()
                ));
            }
        }
    }
    if let ActionRequest::RemoveWorktree { worktree, .. } = &plan.request {
        if let Some(repo) = &plan.repo {
            if let Ok(list) = git::worktree_list(repo) {
                if let Some(e) = list
                    .iter()
                    .find(|e| norm_key(&e.path) == norm_key(worktree))
                {
                    if e.locked.is_some() {
                        return Err("git has locked this worktree since the scan".into());
                    }
                }
            }
            if let Ok((d, u)) = git::status_counts(worktree) {
                if (d > 0 || u > 0) && !plan.discards_work {
                    return Err("the worktree changed since the scan; rescan and look again".into());
                }
            }
        }
    }
    Ok(())
}

pub fn execute(plan: &ActionPlan, acknowledged: bool) -> Result<ActionResult, String> {
    if !plan.blockers.is_empty() {
        return Err(format!("blocked: {}", plan.blockers.join("; ")));
    }
    if plan.discards_work && !acknowledged {
        return Err("this plan discards work; confirm explicitly".into());
    }
    revalidate(plan)?;
    let mut res = ActionResult {
        plan_id: plan.id.clone(),
        repo: plan.repo.clone(),
        done: Vec::new(),
        failed: None,
        bytes_trashed: 0,
        bytes_freed: 0,
    };
    let branch = match &plan.request {
        ActionRequest::RemoveWorktree { worktree, .. } => {
            find_branch(plan.repo.as_deref(), worktree)
        }
        _ => None,
    };
    for s in &plan.steps {
        let r: Result<(), String> = match s.kind {
            StepKind::Trash => trash::delete(&s.path).map_err(|e| e.to_string()),
            StepKind::Delete => std::fs::remove_dir_all(&s.path).map_err(|e| e.to_string()),
            StepKind::GitWorktreePrune => git::worktree_prune(&s.path).map_err(|e| e.to_string()),
            StepKind::GitBranchDelete => match &branch {
                Some(b) => git::branch_delete(&s.path, b).map_err(|e| e.to_string()),
                None => Err("branch name unknown".into()),
            },
        };
        match r {
            Ok(()) => {
                log_line(&format!("ok {:?} {} {}", s.kind, s.path.display(), s.bytes));
                match s.kind {
                    StepKind::Trash => res.bytes_trashed += s.bytes,
                    StepKind::Delete => res.bytes_freed += s.bytes,
                    _ => {}
                }
                res.done.push(s.clone());
            }
            Err(message) => {
                log_line(&format!(
                    "FAILED {:?} {} {}",
                    s.kind,
                    s.path.display(),
                    message
                ));
                res.failed = Some(FailedStep {
                    step: s.clone(),
                    message,
                });
                break;
            }
        }
    }
    Ok(res)
}

fn find_branch(repo: Option<&Path>, worktree: &Path) -> Option<String> {
    // The branch is read before the worktree is removed, from git's own list.
    let repo = repo?;
    git::worktree_list(repo)
        .ok()?
        .into_iter()
        .find(|e| norm_key(&e.path) == norm_key(worktree))
        .and_then(|e| e.branch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::rescan_repo;
    use std::process::{Command, Stdio};
    use std::sync::atomic::AtomicBool;

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

    fn report_for(repo: &Path) -> ScanReport {
        let r = rescan_repo(repo, &[], &AtomicBool::new(false)).unwrap();
        ScanReport {
            repos: vec![r],
            ..Default::default()
        }
    }

    /// Temp dirs live under $TMPDIR which is inside $HOME on macOS, so check_target passes.
    fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let t = tempfile::Builder::new()
            .prefix("cleaner-test-")
            .tempdir_in(paths::home().join("Library/Caches"))
            .unwrap();
        let repo = std::fs::canonicalize(t.path()).unwrap().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        std::fs::write(repo.join("package.json"), "{}").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "one"]);
        let wt = repo.join(".claude/worktrees/w1");
        git(
            &repo,
            &["worktree", "add", "-q", "-b", "feat", wt.to_str().unwrap()],
        );
        std::fs::create_dir_all(wt.join("node_modules/x")).unwrap();
        std::fs::write(wt.join("node_modules/x/i.js"), vec![b'a'; 50_000]).unwrap();
        (t, repo, wt)
    }

    #[test]
    fn prune_then_remove_clean_worktree_permanently() {
        let (_t, repo, wt) = fixture();
        let report = report_for(&repo);
        let plan = preview(
            ActionRequest::PruneArtifacts {
                worktree: wt.clone(),
                kinds: vec![ArtifactKind::NodeModules],
                permanent: true,
            },
            &report,
        )
        .unwrap();
        assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
        assert_eq!(plan.steps.len(), 1);
        let res = execute(&plan, false).unwrap();
        assert!(res.failed.is_none());
        assert!(!wt.join("node_modules").exists());

        // Fresh worktree, no remote: branch has a commit nowhere else only if we commit; here it is clean.
        let report = report_for(&repo);
        let plan = preview(
            ActionRequest::RemoveWorktree {
                worktree: wt.clone(),
                delete_branch: true,
                permanent: true,
            },
            &report,
        )
        .unwrap();
        assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
        assert!(!plan.discards_work);
        let res = execute(&plan, false).unwrap();
        assert!(res.failed.is_none(), "{:?}", res.failed);
        assert!(!wt.exists());
        let list = git::worktree_list(&repo).unwrap();
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn dirty_worktree_requires_acknowledgement_and_main_is_blocked() {
        let (_t, repo, wt) = fixture();
        std::fs::write(wt.join("package.json"), "{\"x\":1}").unwrap();
        let report = report_for(&repo);
        let plan = preview(
            ActionRequest::RemoveWorktree {
                worktree: wt.clone(),
                delete_branch: false,
                permanent: true,
            },
            &report,
        )
        .unwrap();
        assert!(plan.discards_work);
        assert!(execute(&plan, false).is_err());
        let main = preview(
            ActionRequest::RemoveWorktree {
                worktree: repo.clone(),
                delete_branch: false,
                permanent: true,
            },
            &report,
        )
        .unwrap();
        assert!(!main.blockers.is_empty());
    }
}
