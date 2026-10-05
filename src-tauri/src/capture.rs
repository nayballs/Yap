//! Hide Yap's meeting windows from screen capture and screen sharing.
//!
//! While a meeting records, and Settings → General → Meetings → "Hide Yap's
//! meeting windows from screen sharing" is on (the default), the windows that
//! show the meeting get Windows' `WDA_EXCLUDEFROMCAPTURE` display affinity:
//! the docked notepad (`notepad`) and the recording overlay (`overlay`, which
//! shows the live transcript while you dictate). They stay on your monitor
//! but leave screenshots, screen recordings and screen shares (Teams, Zoom,
//! Meet in a browser, OBS…), which show what's behind them instead. Wispr
//! Flow does the same for its Notepad and Flow Bar ("Don't show Notepad and
//! Flow Bar in screen capture"). Windows 10 before version 2004 doesn't know
//! the flag; there Yap falls back to `WDA_MONITOR`, which shows the window
//! as a black box in a capture instead of leaving it out.
//!
//! The flag belongs to the window (its HWND), so it survives hiding and
//! showing. It's set or lifted on every `yap-meeting-state` (a recording
//! started or stopped), when the setting changes (`save_config`), and when
//! one of these windows finishes loading mid-recording (`on_page_load` in
//! lib.rs, for a window created after the recording started). Windows are
//! looked up by label; a missing one is skipped. All of it runs on the main
//! thread, so the changes apply in order without a lock.
//!
//! With the setting off, a meeting starting warns once per note, as Wispr's
//! screen-share tip does: "Your meeting notes show up in screen shares and
//! screenshots", with **Update settings** (Settings → General → Meetings).
//!
//! The debug-only `capture_affinity` command reads each window's affinity
//! back (`GetWindowDisplayAffinity`) for the e2e suite.

use std::sync::Mutex;

use tauri::{AppHandle, Listener, Manager, WebviewWindow};

/// The windows that show a meeting, by label.
pub const MEETING_WINDOWS: [&str; 2] = ["overlay", "notepad"];

/// `SetWindowDisplayAffinity` values.
const WDA_NONE: u32 = 0x00;
const WDA_MONITOR: u32 = 0x01;
const WDA_EXCLUDEFROMCAPTURE: u32 = 0x11;

/// The note the screen-share warning was last shown for (Pause → Resume
/// doesn't warn twice).
static WARNED_NOTE: Mutex<Option<u64>> = Mutex::new(None);

#[cfg(windows)]
mod win {
    use std::ffi::c_void;

    #[link(name = "user32")]
    extern "system" {
        pub fn SetWindowDisplayAffinity(hwnd: *mut c_void, affinity: u32) -> i32;
        pub fn GetWindowDisplayAffinity(hwnd: *mut c_void, affinity: *mut u32) -> i32;
    }
}

/// Whether the meeting windows should be left out of captures right now.
fn wanted() -> bool {
    crate::meeting::is_recording() && crate::config::load().meeting_hide_from_capture
}

/// A window's display affinity as Windows has it (`None`: no native window).
#[cfg(windows)]
fn read(window: &WebviewWindow) -> Option<u32> {
    let hwnd = window.hwnd().ok()?;
    let mut affinity = 0u32;
    (unsafe { win::GetWindowDisplayAffinity(hwnd.0, &mut affinity) } != 0).then_some(affinity)
}

#[cfg(not(windows))]
fn read(_window: &WebviewWindow) -> Option<u32> {
    None
}

/// Leave `window` out of captures (`hide`) or let it back in.
#[cfg(windows)]
fn set(window: &WebviewWindow, hide: bool) {
    let Ok(hwnd) = window.hwnd() else { return };
    let current = read(window).unwrap_or(WDA_NONE);
    if (current != WDA_NONE) == hide {
        return;
    }
    let label = window.label();
    unsafe {
        if !hide {
            if win::SetWindowDisplayAffinity(hwnd.0, WDA_NONE) == 0 {
                let e = std::io::Error::last_os_error();
                tracing::warn!(label, "capture: couldn't show the window in captures again ({e})");
            }
            return;
        }
        if win::SetWindowDisplayAffinity(hwnd.0, WDA_EXCLUDEFROMCAPTURE) != 0 {
            tracing::info!(label, "capture: window left out of screen captures");
            return;
        }
        // Windows 10 before 2004 doesn't know WDA_EXCLUDEFROMCAPTURE: a black
        // box in captures is the next best thing.
        let e = std::io::Error::last_os_error();
        if win::SetWindowDisplayAffinity(hwnd.0, WDA_MONITOR) != 0 {
            tracing::info!(label, "capture: window blacked out in screen captures ({e})");
        } else {
            let e2 = std::io::Error::last_os_error();
            tracing::warn!(label, "capture: couldn't hide the window from captures ({e}; {e2})");
        }
    }
}

#[cfg(not(windows))]
fn set(_window: &WebviewWindow, _hide: bool) {}

/// Apply the wanted state to every meeting window that exists (main thread).
fn apply(app: &AppHandle) {
    let hide = wanted();
    for label in MEETING_WINDOWS {
        if let Some(window) = app.get_webview_window(label) {
            set(&window, hide);
        }
    }
}

/// Bring the meeting windows in line with the recording and the setting.
/// Safe from any thread: the work runs on the main thread, in order.
pub fn sync(app: &AppHandle) {
    let handle = app.clone();
    if let Err(e) = app.run_on_main_thread(move || apply(&handle)) {
        tracing::warn!("capture: couldn't reach the main thread ({e})");
    }
}

/// One window, e.g. right after it was created or loaded: hidden from
/// captures if a meeting records with the setting on (a no-op for windows
/// that don't show meetings).
pub fn sync_window(window: &WebviewWindow) {
    if !MEETING_WINDOWS.contains(&window.label()) {
        return;
    }
    let w = window.clone();
    if let Err(e) = window.run_on_main_thread(move || set(&w, wanted())) {
        tracing::warn!("capture: couldn't reach the main thread ({e})");
    }
}

/// A recording started or stopped (`yap-meeting-state`): apply, and with
/// the setting off, warn when a meeting starts.
fn on_meeting_state(app: &AppHandle, payload: &str) {
    sync(app);
    let state: serde_json::Value = serde_json::from_str(payload).unwrap_or_default();
    if state["recording"].as_bool() != Some(true) {
        return;
    }
    let Some(note) = state["noteId"].as_u64() else { return };
    if crate::config::load().meeting_hide_from_capture {
        return;
    }
    {
        let mut warned = WARNED_NOTE.lock().unwrap_or_else(|p| p.into_inner());
        if *warned == Some(note) {
            return;
        }
        *warned = Some(note);
    }
    tracing::info!(note, "capture: meeting windows show in screen shares (setting off); warning");
    // In the window, plus a Windows notification while it isn't focused
    // (the meeting may have started from the tray or the shortcut).
    crate::meeting_guard::notice_everywhere(app, &crate::meeting_guard::Notice::screen_share());
}

/// Follow the meeting recorder (app setup).
pub fn init(app: &AppHandle) {
    let handle = app.clone();
    app.listen("yap-meeting-state", move |event| on_meeting_state(&handle, event.payload()));
}

/// Debug builds only: every window's display affinity as Windows reports it
/// (`{ hidden, windows: { overlay: 17, notepad: null, … } }`: 0 none, 1
/// monitor only, 17 excluded from capture, null no such window), for the
/// e2e suite. `hidden`: whether the meeting windows should be hidden now.
#[tauri::command]
pub fn capture_affinity(app: AppHandle) -> Result<serde_json::Value, String> {
    if !cfg!(debug_assertions) {
        return Err("Only in debug builds".to_string());
    }
    let mut windows = serde_json::Map::new();
    for label in MEETING_WINDOWS.iter().chain(&["settings", "onboarding"]) {
        let affinity = app.get_webview_window(label).and_then(|w| read(&w));
        windows.insert(label.to_string(), serde_json::json!(affinity));
    }
    Ok(serde_json::json!({ "hidden": wanted(), "windows": windows }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn affinity_values_match_windows() {
        // winuser.h: WDA_NONE 0, WDA_MONITOR 1, WDA_EXCLUDEFROMCAPTURE 0x11.
        assert_eq!((WDA_NONE, WDA_MONITOR, WDA_EXCLUDEFROMCAPTURE), (0, 1, 17));
        assert_eq!(MEETING_WINDOWS, ["overlay", "notepad"]);
    }

    #[test]
    fn hiding_is_on_by_default_and_off_without_a_recording() {
        assert!(crate::config::YapConfig::default().meeting_hide_from_capture);
        // A config saved before the setting existed keeps it on.
        let cfg: crate::config::YapConfig = serde_json::from_str("{}").unwrap();
        assert!(cfg.meeting_hide_from_capture);
        // Nothing records in a unit test.
        assert!(!wanted());
    }
}
