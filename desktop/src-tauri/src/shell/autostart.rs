//! Launch at sign-in, as Settings → General sets it, and whether a launch
//! brings the main window up.

use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt;

use crate::config;

/// Enable or disable OS autostart via `tauri-plugin-autostart`.
/// Kept as a free function so `set_autostart` (below) can delegate here.
pub fn set_autostart_enabled(app: &AppHandle, enabled: bool) -> Result<(), String> {
    // A dev build shares the installed Yap's "Yap" Run entry: disabling would
    // delete the installed app's launch-at-login, enabling would point it at
    // the dev exe. Leave the OS setting to the installed app.
    if cfg!(debug_assertions) {
        tracing::info!(enabled, "autostart: dev build, OS setting left alone");
        return Ok(());
    }
    let manager = app.autolaunch();
    let res = if enabled {
        manager.enable()
    } else {
        manager.disable()
    };
    res.map_err(|e| format!("Failed to set autostart: {}", e))
}

/// The argument the sign-in launch carries (the autostart Run entry).
pub(crate) const AUTOSTART_ARG: &str = "--autostart";

/// Does this launch bring Yap's main window up? A launch by the person (the
/// Start menu, a shortcut, the installer's "Run Yap") does — before
/// 2026-10-22 a release build started to the tray with nothing on screen
/// but the Yap bar, so opening Yap seemed to do nothing. Not: a launch at
/// sign-in, "Start hidden", the first run (onboarding comes up instead) or
/// the updater's relaunch (`updates::init` brings the window back if it was
/// open). Pure.
#[cfg_attr(debug_assertions, allow(dead_code))] // dev builds always show Settings
pub(crate) fn launch_shows_window(at_sign_in: bool, start_hidden: bool, has_model: bool, after_update: bool) -> bool {
    !at_sign_in && !start_hidden && has_model && !after_update
}

/// Was Yap started by the sign-in entry? Its `--autostart`, or — for an
/// entry written before the argument existed, until the launch below writes
/// it again — autostart on and Windows up for under ten minutes (with
/// autostart on, a launch by hand later finds Yap running already and just
/// brings its window up through single-instance).
#[cfg_attr(debug_assertions, allow(dead_code))]
pub(crate) fn launched_at_sign_in(autostart: bool) -> bool {
    if std::env::args().any(|a| a == AUTOSTART_ARG) {
        return true;
    }
    #[cfg(windows)]
    {
        let up_ms = unsafe { windows::Win32::System::SystemInformation::GetTickCount64() };
        autostart && up_ms < 10 * 60 * 1000
    }
    #[cfg(not(windows))]
    {
        let _ = autostart;
        false
    }
}

/// Enable/disable launching Yap at OS login, and persist the choice.
#[tauri::command]
pub fn set_autostart(app: AppHandle, enabled: bool) -> Result<(), String> {
    set_autostart_enabled(&app, enabled)?;
    config::update(|c| c.autostart = enabled)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::launch_shows_window;

    #[test]
    fn a_launch_by_the_person_brings_the_window_up() {
        // (at sign-in, start hidden, a model installed, the updater's relaunch)
        assert!(launch_shows_window(false, false, true, false));
        // Sign-in and "Start hidden" stay in the tray.
        assert!(!launch_shows_window(true, false, true, false));
        assert!(!launch_shows_window(false, true, true, false));
        // The first run shows onboarding instead.
        assert!(!launch_shows_window(false, false, false, false));
        // After "Restart to update", updates.rs decides.
        assert!(!launch_shows_window(false, false, true, true));
    }
}
