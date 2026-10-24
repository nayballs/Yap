//! Launch at sign-in, as Settings → General sets it.

use tauri::AppHandle;

use crate::config;

/// Enable/disable launching Yap at OS login, and persist the choice.
#[tauri::command]
pub fn set_autostart(app: AppHandle, enabled: bool) -> Result<(), String> {
    crate::set_autostart_enabled(&app, enabled)?;
    config::update(|c| c.autostart = enabled)?;
    Ok(())
}
