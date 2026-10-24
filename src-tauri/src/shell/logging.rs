//! The log: where it is, and the pages' own lines written to it.

use crate::config;

/// Where logs live + the most recent log file (the one to attach to a bug
/// report). Yap always writes a daily-rolling file at `info`; Debug mode
/// raises Yap's own crate to `debug`.
#[tauri::command]
pub fn log_info() -> serde_json::Value {
    let dir = config::data_dir().join("logs");
    let newest = std::fs::read_dir(&dir)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_file())
        .max_by_key(|e| {
            e.metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
        })
        .map(|e| e.path().to_string_lossy().to_string());
    serde_json::json!({
        "dir": dir.to_string_lossy(),
        "file": newest,
    })
}

/// Open the logs folder in the system file manager.
#[tauri::command]
pub fn open_logs_folder() -> Result<(), String> {
    let dir = config::data_dir().join("logs");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    #[cfg(windows)]
    let opener = "explorer";
    #[cfg(target_os = "macos")]
    let opener = "open";
    #[cfg(all(unix, not(target_os = "macos")))]
    let opener = "xdg-open";
    std::process::Command::new(opener)
        .arg(&dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Frontend → log-file bridge: webviews call this to land diagnostics in the
/// same rolling `yap.log` as the backend (webview consoles are invisible in
/// normal runs, which made the onboarding event-delivery bug nearly
/// undebuggable — see 2026-07-05).
#[tauri::command]
pub fn frontend_log(msg: String) {
    tracing::info!("[web] {}", msg);
}
