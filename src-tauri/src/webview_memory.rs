//! Hidden webviews hold less memory.
//!
//! Yap's windows are made at launch and live hidden most of the day: the main
//! window (Yap sits in the tray), onboarding (after the first run) and the
//! meeting notepad (between meetings) — each with a WebView2 renderer process
//! of its own. While one of them is hidden or minimised, its webview gets
//! WebView2's **low memory target** (`ICoreWebView2_19::
//! put_MemoryUsageTargetLevel(LOW)`, what Microsoft suggests for an app that
//! isn't in use): the runtime trims the renderer's working set and caches.
//! Back on screen, the normal target. Scripts keep running either way:
//! events, timers and listeners are untouched — unlike `TrySuspend`, which
//! Yap's hidden pages can't afford (Settings' auto-save and hotkey copy, the
//! notepad following a recording, the toasts deciding where a notice goes).
//! The Yap bar and the split preview are left alone (the bar is on screen
//! whenever it's on; the preview is tiny and short-lived).
//!
//! One watcher thread reads each window's visibility every few seconds and
//! changes the target only when it changes ([`target`], pure): one place,
//! whatever showed or hid the window (the tray, close = hide, a notice, the
//! notepad docking for a meeting, the updater's relaunch).

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Manager};

/// How often the watcher looks.
const POLL: Duration = Duration::from_secs(3);
/// Windows the watcher leaves at WebView2's default.
const LEFT_ALONE: &[&str] = &["overlay", "split-preview"];

/// A webview's memory target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Target {
    Normal,
    Low,
}

/// Low while the window is off screen: hidden, or minimised to the taskbar.
pub fn target(visible: bool, minimized: bool) -> Target {
    if visible && !minimized {
        Target::Normal
    } else {
        Target::Low
    }
}

/// The target each window was last given (for the e2e suite and the log).
static GIVEN: Mutex<BTreeMap<String, Target>> = Mutex::new(BTreeMap::new());

/// Start the watcher (after setup: the hidden webviews have had their
/// DWM-cloaked first show by then).
pub fn init(app: &AppHandle) {
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("webview-memory".into())
        .spawn(move || {
            // The pages load first.
            std::thread::sleep(Duration::from_secs(5));
            watch(&app)
        });
    if let Err(e) = spawned {
        tracing::warn!("Couldn't start the webview memory watcher: {e}");
    }
}

fn watch(app: &AppHandle) -> ! {
    loop {
        for (label, window) in app.webview_windows() {
            if LEFT_ALONE.contains(&label.as_str()) {
                continue;
            }
            let want = target(
                window.is_visible().unwrap_or(true),
                window.is_minimized().unwrap_or(false),
            );
            let given = GIVEN.lock().unwrap_or_else(|p| p.into_inner()).get(&label).copied();
            if given != Some(want) {
                apply(&window, want);
                GIVEN.lock().unwrap_or_else(|p| p.into_inner()).insert(label, want);
            }
        }
        std::thread::sleep(POLL);
    }
}

#[cfg(windows)]
fn apply(window: &tauri::WebviewWindow, want: Target) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2, ICoreWebView2_19, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW,
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
    };
    use windows::core::Interface;

    let label = window.label().to_string();
    let result = window.with_webview(move |platform| {
        let level = match want {
            Target::Low => COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW,
            Target::Normal => COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
        };
        let set = || -> windows::core::Result<()> {
            let core: ICoreWebView2 = unsafe { platform.controller().CoreWebView2()? };
            // ICoreWebView2_19: WebView2 Runtime 114 and later.
            let core19: ICoreWebView2_19 = core.cast()?;
            unsafe { core19.SetMemoryUsageTargetLevel(level) }
        };
        match set() {
            Ok(()) => tracing::debug!(window = %label, ?want, "webview memory target"),
            Err(e) => tracing::debug!(window = %label, "webview memory target not set: {e}"),
        }
    });
    if let Err(e) = result {
        tracing::debug!("Couldn't reach a webview for its memory target: {e}");
    }
}

#[cfg(not(windows))]
fn apply(_window: &tauri::WebviewWindow, _want: Target) {}

/// Debug builds: the target each window was last given, `{ label: "low" |
/// "normal" }`.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_webview_memory() -> BTreeMap<String, Target> {
    GIVEN.lock().unwrap_or_else(|p| p.into_inner()).clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_window_on_screen_keeps_the_normal_target() {
        assert_eq!(target(true, false), Target::Normal);
        assert_eq!(target(false, false), Target::Low);
        assert_eq!(target(true, true), Target::Low);
        assert_eq!(target(false, true), Target::Low);
    }

    #[test]
    fn the_bar_and_the_preview_are_left_alone() {
        assert!(LEFT_ALONE.contains(&"overlay"));
        assert!(LEFT_ALONE.contains(&"split-preview"));
        assert!(!LEFT_ALONE.contains(&"settings"));
        assert!(!LEFT_ALONE.contains(&"notepad"));
    }
}
