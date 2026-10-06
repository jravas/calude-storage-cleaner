//! cleaner-core: scanning, classification and cleanup logic.
//!
//! No Tauri dependency. The `app` crate wraps these functions as commands,
//! and `src/bin/scan.rs` exposes them as a CLI.

pub mod actions;
pub mod fsize;
pub mod git;
pub mod model;
pub mod paths;
pub mod progress;
pub mod safety;
pub mod scan;

pub use actions::{execute, preview, ActionPlan, ActionRequest, ActionResult};
pub use model::*;
pub use progress::{ProgressSink, ScanEvent};
pub use scan::{compute_totals, rescan_repo, scan, ScanOptions};

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Human-readable byte count, base 1000 like Finder.
pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1000.0 && i < UNITS.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} B")
    } else if v >= 100.0 {
        format!("{v:.0} {}", UNITS[i])
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}
