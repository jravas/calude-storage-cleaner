#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod state;

use tauri::Manager;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(state::AppState::default())
        .setup(|app| {
            let handle = app.handle().clone();
            if let Some(r) = state::load_persisted(&handle) {
                *handle.state::<state::AppState>().report.lock().unwrap() = Some(r);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::ping,
            commands::get_report,
            commands::start_scan,
            commands::cancel_scan,
            commands::refresh_repo,
            commands::preview_action,
            commands::run_action,
            commands::reveal,
            commands::open_trash,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Claude Storage Cleaner");
}
