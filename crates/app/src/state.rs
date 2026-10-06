use cleaner_core::ScanReport;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager};

pub struct AppState {
    pub report: Mutex<Option<ScanReport>>,
    pub scanning: AtomicBool,
    pub cancel: Arc<AtomicBool>,
}

impl Default for AppState {
    fn default() -> Self {
        AppState {
            report: Mutex::new(None),
            scanning: AtomicBool::new(false),
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }
}

pub fn report_path(app: &AppHandle) -> Option<PathBuf> {
    let dir = app.path().app_data_dir().ok()?;
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("report.json"))
}

pub fn load_persisted(app: &AppHandle) -> Option<ScanReport> {
    let p = report_path(app)?;
    let text = std::fs::read_to_string(p).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn persist(app: &AppHandle, report: &ScanReport) {
    if let Some(p) = report_path(app) {
        if let Ok(text) = serde_json::to_string(report) {
            let tmp = p.with_extension("json.tmp");
            if std::fs::write(&tmp, text).is_ok() {
                let _ = std::fs::rename(tmp, p);
            }
        }
    }
}
