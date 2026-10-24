//! **Cold start** for `yap.exe bench`: how long a first-time Yap takes to
//! put its window on screen. A throwaway copy of this exe (and the DLLs
//! beside it) runs from the bench's own folder in portable mode, on an empty
//! data folder and a WebView2 profile of its own, as a **bench child**
//! (`YAP_BENCH_CHILD=1`, honoured only in portable mode: no single-instance
//! hand-off to a Yap already running, no keyboard hook, no autostart or
//! window-state changes, no other process touched — lib.rs). Launched
//! twice: the first launch builds its WebView2 profile, as a new install's
//! first launch does; the second finds it.
//!
//! Timed from the spawn: **window shown** (the first-run window, "Welcome
//! to Yap", visible and not DWM-cloaked, found with `EnumWindows`) and
//! **page mounted** (its page's "mounted" line in the copy's log). Then the
//! copy is ended (it was never shown a model, so it holds nothing).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Serialize;

/// One launch.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Launch {
    /// The first-run window on screen.
    pub window_ms: Option<u64>,
    /// Its page mounted (the copy's log says so).
    pub page_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Launch {
    /// On screen with its page: the later of the two.
    pub fn ready_ms(&self) -> Option<u64> {
        Some(self.window_ms?.max(self.page_ms?))
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ColdStart {
    /// A fresh WebView2 profile (a new install's first launch).
    pub first_launch: Launch,
    /// The profile there already (every launch after).
    pub second_launch: Launch,
}

/// How long a launch may take before it counts as failed.
const PATIENCE: Duration = Duration::from_secs(45);

/// Time two launches of a copy of `exe` in `work` (a folder of the
/// bench's own, emptied first).
pub fn measure(exe: &Path, work: &Path) -> ColdStart {
    let app = work.join("app");
    let _ = std::fs::remove_dir_all(work);
    if let Err(e) = prepare(exe, &app) {
        let failed = Launch { error: Some(e), ..Default::default() };
        return ColdStart { first_launch: failed.clone(), second_launch: failed };
    }
    let first_launch = launch(&app, &work.join("webview2"));
    // WebView2's own processes go a moment after their host.
    std::thread::sleep(Duration::from_secs(2));
    let second_launch = launch(&app, &work.join("webview2"));
    std::thread::sleep(Duration::from_secs(2));
    ColdStart { first_launch, second_launch }
}

/// The copy: the exe and the DLLs beside it, the portable marker, and
/// settings that keep it to itself.
fn prepare(exe: &Path, app: &Path) -> Result<(), String> {
    let data = app.join("Data");
    std::fs::create_dir_all(&data).map_err(|e| e.to_string())?;
    std::fs::copy(exe, app.join("yap.exe")).map_err(|e| format!("couldn't copy Yap: {e}"))?;
    if let Some(dir) = exe.parent() {
        for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())?.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|x| x.eq_ignore_ascii_case("dll")) {
                let _ = std::fs::copy(&path, app.join(entry.file_name()));
            }
        }
    }
    std::fs::write(app.join("portable"), "Yap Portable Mode").map_err(|e| e.to_string())?;
    let cfg = serde_json::json!({
        "autostart": false,
        "updateChecksEnabled": false,
        "bridgeEnabled": false,
        "soundEnabled": false,
        "meetingDetection": false,
        "hotkey": "kb:135",
    });
    std::fs::write(data.join("config.json"), cfg.to_string()).map_err(|e| e.to_string())
}

fn launch(app: &Path, webview2: &Path) -> Launch {
    let mut cmd = std::process::Command::new(app.join("yap.exe"));
    cmd.current_dir(app)
        .env("YAP_BENCH_CHILD", "1")
        .env("WEBVIEW2_USER_DATA_FOLDER", webview2)
        .env_remove("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let started = Instant::now();
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return Launch { error: Some(format!("couldn't start the copy: {e}")), ..Default::default() },
    };
    crate::procs::end_with_yap(&child);
    let pid = child.id();
    let logs = app.join("Data").join("logs");
    // (The second launch's log already has the first one's line.)
    let seen = pages_mounted(&logs);
    let mut out = Launch::default();
    while started.elapsed() < PATIENCE && (out.window_ms.is_none() || out.page_ms.is_none()) {
        if let Ok(Some(status)) = child.try_wait() {
            out.error = Some(format!("the copy exited ({status})"));
            break;
        }
        if out.window_ms.is_none() && first_run_window_shown(pid) {
            out.window_ms = Some(started.elapsed().as_millis() as u64);
        }
        if out.page_ms.is_none() && pages_mounted(&logs) > seen {
            out.page_ms = Some(started.elapsed().as_millis() as u64);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if out.error.is_none() && (out.window_ms.is_none() || out.page_ms.is_none()) {
        out.error = Some(format!("not on screen after {} s", PATIENCE.as_secs()));
    }
    let _ = child.kill();
    let _ = child.wait();
    out
}

/// How many times the first-run page has said it mounted
/// (Onboarding.svelte's `flog`) in the copy's log.
fn pages_mounted(logs: &Path) -> usize {
    let Ok(dir) = std::fs::read_dir(logs) else { return 0 };
    dir.flatten()
        .map(|e| e.path())
        .filter(|p: &PathBuf| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("yap.log")))
        .map(|p| std::fs::read_to_string(p).map(|s| s.matches("[onboarding] mounted").count()).unwrap_or(0))
        .sum()
}

/// Is a window of `pid` titled "Welcome to Yap" visible and not cloaked?
#[cfg(windows)]
fn first_run_window_shown(pid: u32) -> bool {
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible};
    struct Search {
        pid: u32,
        found: bool,
    }
    unsafe extern "system" fn each(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
        // SAFETY: `lparam` is the `Search` below, alive for EnumWindows.
        let search = unsafe { &mut *(lparam.0 as *mut Search) };
        let mut owner = 0u32;
        // SAFETY: plain reads of a window handle EnumWindows gave.
        unsafe {
            GetWindowThreadProcessId(hwnd, Some(&mut owner));
            if owner != search.pid || !IsWindowVisible(hwnd).as_bool() {
                return true.into();
            }
            let mut cloaked = 0u32;
            let _ = DwmGetWindowAttribute(
                hwnd,
                DWMWA_CLOAKED,
                &mut cloaked as *mut u32 as *mut core::ffi::c_void,
                std::mem::size_of::<u32>() as u32,
            );
            let mut title = [0u16; 128];
            let n = GetWindowTextW(hwnd, &mut title) as usize;
            if cloaked == 0 && String::from_utf16_lossy(&title[..n]) == "Welcome to Yap" {
                search.found = true;
                return false.into();
            }
        }
        true.into()
    }
    let mut search = Search { pid, found: false };
    // SAFETY: the callback only reads windows and writes `search`.
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut search as *mut Search as isize));
    }
    search.found
}

#[cfg(not(windows))]
fn first_run_window_shown(_pid: u32) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_launch_is_ready_when_its_window_and_its_page_both_are() {
        let l = Launch { window_ms: Some(900), page_ms: Some(640), error: None };
        assert_eq!(l.ready_ms(), Some(900));
        assert_eq!(Launch { window_ms: Some(900), page_ms: None, error: None }.ready_ms(), None);
        let v = serde_json::to_value(ColdStart { first_launch: l, second_launch: Launch::default() }).unwrap();
        assert_eq!(v["firstLaunch"]["windowMs"], 900);
        assert_eq!(v["firstLaunch"]["pageMs"], 640);
        assert!(v["secondLaunch"].get("error").is_none());
    }

    #[test]
    fn each_launch_waits_for_a_mounted_line_of_its_own() {
        let dir = std::env::temp_dir().join(format!("yap-cold-logs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(pages_mounted(&dir), 0);
        std::fs::write(dir.join("yap.log.2026-10-24"), "x INFO [web] [onboarding] mounted; visibilityState=visible\n").unwrap();
        assert_eq!(pages_mounted(&dir), 1);
        std::fs::write(dir.join("yap.log.2026-10-24"), "a [onboarding] mounted\nb\nc [onboarding] mounted\n").unwrap();
        std::fs::write(dir.join("other.txt"), "[onboarding] mounted").unwrap();
        assert_eq!(pages_mounted(&dir), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_throwaway_copy_is_portable_with_settings_of_its_own() {
        let dir = std::env::temp_dir().join(format!("yap-cold-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // A stand-in exe and a DLL beside it.
        let from = dir.join("build");
        std::fs::create_dir_all(&from).unwrap();
        std::fs::write(from.join("yap-test.exe"), b"exe").unwrap();
        std::fs::write(from.join("helper.dll"), b"dll").unwrap();
        std::fs::write(from.join("notes.txt"), b"not copied").unwrap();
        let app = dir.join("app");
        prepare(&from.join("yap-test.exe"), &app).unwrap();
        assert_eq!(std::fs::read_to_string(app.join("portable")).unwrap(), "Yap Portable Mode");
        assert_eq!(std::fs::read(app.join("yap.exe")).unwrap(), b"exe");
        assert!(app.join("helper.dll").is_file());
        assert!(!app.join("notes.txt").exists());
        let cfg: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(app.join("Data").join("config.json")).unwrap()).unwrap();
        assert_eq!(cfg["autostart"], false);
        assert_eq!(cfg["updateChecksEnabled"], false);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
