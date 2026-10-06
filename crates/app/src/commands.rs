use crate::state::{self, AppState};
use cleaner_core::{
    ActionPlan, ActionRequest, ActionResult, ProgressSink, Repo, ScanEvent, ScanOptions, ScanReport,
};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter, Manager, State};

struct EmitSink(AppHandle);

impl ProgressSink for EmitSink {
    fn on(&self, ev: ScanEvent) {
        match ev {
            ScanEvent::Progress { phase, current } => {
                let _ = self.0.emit(
                    "scan-progress",
                    serde_json::json!({ "phase": phase, "current": current }),
                );
            }
            ScanEvent::Repo { repo } => {
                let _ = self.0.emit("scan-repo", *repo);
            }
        }
    }
}

#[tauri::command]
pub fn ping() -> String {
    format!("cleaner-core {}", cleaner_core::version())
}

#[tauri::command]
pub fn get_report(app: AppHandle, st: State<AppState>) -> Option<ScanReport> {
    let mut guard = st.report.lock().unwrap();
    if guard.is_none() {
        *guard = state::load_persisted(&app);
    }
    guard.clone()
}

#[tauri::command]
pub fn start_scan(
    app: AppHandle,
    st: State<AppState>,
    roots: Option<Vec<String>>,
) -> Result<(), String> {
    if st
        .scanning
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("a scan is already running".into());
    }
    st.cancel.store(false, Ordering::SeqCst);
    let cancel = st.cancel.clone();
    let mut opts = ScanOptions::default();
    if let Some(r) = roots {
        if !r.is_empty() {
            opts.roots = r.into_iter().map(PathBuf::from).collect();
        }
    }
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let sink = EmitSink(handle.clone());
        let report = cleaner_core::scan(&opts, &sink, &cancel);
        let cancelled = cancel.load(Ordering::SeqCst);
        let st = handle.state::<AppState>();
        if !cancelled {
            state::persist(&handle, &report);
            *st.report.lock().unwrap() = Some(report.clone());
            let _ = handle.emit("scan-done", report);
        } else {
            let _ = handle.emit("scan-cancelled", ());
        }
        st.scanning.store(false, Ordering::SeqCst);
    });
    Ok(())
}

#[tauri::command]
pub fn cancel_scan(st: State<AppState>) {
    st.cancel.store(true, Ordering::SeqCst);
}

/// Re-scan one repo and splice it into the stored report. Returns the new report.
#[tauri::command]
pub fn refresh_repo(
    app: AppHandle,
    st: State<AppState>,
    path: String,
) -> Result<ScanReport, String> {
    let repo_path = PathBuf::from(&path);
    let transcripts = st
        .report
        .lock()
        .unwrap()
        .as_ref()
        .map(|r| r.transcript_dirs.clone())
        .unwrap_or_default();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let fresh: Repo =
        cleaner_core::rescan_repo(&repo_path, &transcripts, &cancel).map_err(|e| e.message)?;
    let mut guard = st.report.lock().unwrap();
    let report = guard.as_mut().ok_or("no report yet")?;
    match report.repos.iter_mut().find(|r| r.path == fresh.path) {
        Some(slot) => *slot = fresh,
        None => report.repos.push(fresh),
    }
    report
        .repos
        .sort_by_key(|r| std::cmp::Reverse(r.total_bytes));
    report.totals =
        cleaner_core::compute_totals(&report.repos, &report.transcript_dirs, &report.buckets);
    state::persist(&app, report);
    Ok(report.clone())
}

#[tauri::command]
pub fn reveal(path: String) -> Result<(), String> {
    tauri_plugin_opener::reveal_item_in_dir(path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn open_trash() -> Result<(), String> {
    let trash = cleaner_core::paths::home().join(".Trash");
    tauri_plugin_opener::open_path(trash, None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn preview_action(st: State<AppState>, request: ActionRequest) -> Result<ActionPlan, String> {
    let guard = st.report.lock().unwrap();
    let report = guard.as_ref().ok_or("no scan yet")?;
    cleaner_core::preview(request, report)
}

#[tauri::command]
pub fn run_action(
    st: State<AppState>,
    plan: ActionPlan,
    acknowledged: bool,
) -> Result<ActionResult, String> {
    if st.scanning.load(Ordering::SeqCst) {
        return Err("wait for the scan to finish".into());
    }
    let res = cleaner_core::execute(&plan, acknowledged)?;
    // Drop forgotten transcript dirs from the stored report right away.
    if let ActionRequest::ForgetTranscripts { .. } = plan.request {
        let mut guard = st.report.lock().unwrap();
        if let Some(r) = guard.as_mut() {
            let done: Vec<PathBuf> = res.done.iter().map(|s| s.path.clone()).collect();
            r.transcript_dirs.retain(|t| !done.contains(&t.path));
            r.totals = cleaner_core::compute_totals(&r.repos, &r.transcript_dirs, &r.buckets);
        }
    }
    Ok(res)
}
