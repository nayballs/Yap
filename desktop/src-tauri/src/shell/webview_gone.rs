//! **WebView2's browser process exiting** (19 Nov 2026): every Yap window
//! dies with it, while the Rust core goes on (the hotkey, a meeting's
//! recorder, the tray). WebView2 says to make the webviews again, which
//! Tauri can't, so Yap restarts — at once if the person asks, by itself as
//! soon as nothing would be lost.
//!
//! When `ProcessFailed` says the browser process exited (`page_watch.rs`,
//! every webview reports it; once is enough):
//! - the log says so, at `error`;
//! - the tray shows it (`tray.rs`): a red dot on the icon, a line in the
//!   tooltip and **Restart Yap** at the top of its menu, which restarts now;
//! - Yap waits until nothing would be lost ([`in_the_way`], pure, from
//!   [`Work`]): no dictation recording or transcribing (nor a cancelled one
//!   kept for Undo), no meeting recording (nor its audio transcribed
//!   again), no speech model downloading — the updater's own check
//!   (`updates::busy_reason`) — and no Chat turn answering, no Voice
//!   Assistant question (listening, thinking or speaking), no Code chat's
//!   Claude or agent at work and no ▶ Run command running (their Claude Code
//!   children die with Yap: the kill-on-close job), no routine running. It
//!   looks again every [`EVERY`] (5 s), as long as it takes, and says in
//!   the log what it waits for when that changes;
//! - then it restarts (`AppHandle::request_restart`: Yap's exit cleanup
//!   runs first), leaving a note in the data folder ([`MARKER`]) so the new
//!   Yap says once, quietly (a toast or a bar card, never a Windows
//!   notification): "Yap restarted its windows after WebView2 stopped",
//!   and shows the main window again if it was open (without the focus).
//!
//! **Dev builds** (`tauri dev`, [`restarts_by_itself`]) never restart by
//! themselves — a restart there leaves the dev process tree — and only the
//! tray's Restart Yap does. Debug builds in test mode: [`e2e_webview2_gone`]
//! goes through all of this without a crash (`webview2-exit.spec.js`; the
//! same spec crashes the browser process for real).

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

/// How often Yap looks again for what a restart would lose.
pub const EVERY: Duration = Duration::from_secs(5);
/// The note a restart leaves for the next Yap, in the data folder.
pub const MARKER: &str = "webview2_restart.json";
/// A note older than this is stale (a restart that never happened): not said.
const MARKER_FRESH_SECS: u64 = 10 * 60;
/// The notice's kind (`notices/policy.rs`).
pub const NOTICE: &str = "windowsRestarted";

/// What's going on that a restart would lose.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Work {
    /// A dictation recording or transcribing, or a cancelled one kept for Undo.
    pub dictation: bool,
    /// A meeting recording, or its audio being transcribed again.
    pub meeting: bool,
    /// A speech model downloading.
    pub download: bool,
    /// A Chat turn answering.
    pub chat: bool,
    /// The Voice Assistant listening, thinking or speaking.
    pub assistant: bool,
    /// A Code chat's Claude or an agent at work, or a ▶ Run command running.
    pub code: bool,
    /// A routine running.
    pub routine: bool,
}

/// **Pure: what a restart would lose now**, in words for the log; `None`:
/// nothing, so Yap may restart.
pub fn in_the_way(w: Work) -> Option<&'static str> {
    if w.dictation {
        Some("a dictation")
    } else if w.meeting {
        Some("a meeting recording")
    } else if w.download {
        Some("a model download")
    } else if w.chat {
        Some("a Chat answer")
    } else if w.assistant {
        Some("a Voice Assistant question")
    } else if w.code {
        Some("Claude at work in a Code chat")
    } else if w.routine {
        Some("a routine")
    } else {
        None
    }
}

/// **Pure: whether Yap restarts by itself** once nothing would be lost: never
/// in a dev build (`tauri dev`), where a restart leaves the dev process tree.
pub const fn restarts_by_itself(dev: bool) -> bool {
    !dev
}

/// What's going on now: the same calls the updater, the bar and the Agent
/// panel go by. (Locks the pipeline's mutex: never from a `yap-state`
/// listener.)
fn work_now(app: &AppHandle) -> Work {
    use crate::updates::Busy;
    let busy = crate::updates::busy_reason(app);
    Work {
        dictation: busy == Some(Busy::Dictation),
        meeting: busy == Some(Busy::Meeting),
        download: busy == Some(Busy::ModelDownload),
        chat: !crate::chat::stream::answering_ids().is_empty(),
        assistant: crate::chat::assistant::assistant_phase().phase != crate::chat::assistant::Phase::Idle,
        code: !crate::agents::store::live_ids().is_empty() || !crate::agents::run_block::running_chats().is_empty(),
        routine: crate::chat::routines::running(),
    }
}

static GONE: AtomicBool = AtomicBool::new(false);
/// The main window was on screen when the browser process went.
static SHOW_MAIN: AtomicBool = AtomicBool::new(false);

/// **Every window is gone** (WebView2's browser process exited).
pub fn is_gone() -> bool {
    GONE.load(Ordering::Relaxed)
}

/// WebView2's browser process exited (`page_watch.rs`; each webview says
/// so, the first one counts): logged, the tray's Restart Yap, and a restart
/// by itself once nothing would be lost.
pub fn gone(app: &AppHandle, reason: &'static str, exit_code: i32) {
    if GONE.swap(true, Ordering::SeqCst) {
        return;
    }
    let main_shown = app.get_webview_window("settings").and_then(|w| w.is_visible().ok()).unwrap_or(false);
    SHOW_MAIN.store(main_shown, Ordering::Relaxed);
    let by_itself = restarts_by_itself(tauri::is_dev());
    tracing::error!(
        reason,
        exit_code,
        by_itself,
        "WebView2's browser process exited: every Yap window is gone; Restart Yap offered in the tray"
    );
    crate::shell::tray::windows_gone(app);
    let app = app.clone();
    let spawned = std::thread::Builder::new().name("webview2-restart".into()).spawn(move || wait_then_restart(&app, by_itself));
    if let Err(e) = spawned {
        tracing::warn!("Couldn't wait to restart Yap after WebView2 stopped: {e}");
    }
}

/// Look every [`EVERY`] until nothing would be lost, then restart (a dev
/// build only says so).
fn wait_then_restart(app: &AppHandle, by_itself: bool) {
    let mut said: Option<Option<&'static str>> = None;
    loop {
        let why = in_the_way(work_now(app));
        if said != Some(why) {
            match why {
                Some(what) => tracing::info!(waiting_for = what, "Not restarting Yap yet: {what} would be lost"),
                None if by_itself => tracing::info!("Nothing would be lost: restarting Yap"),
                None => tracing::warn!("Nothing would be lost, but a dev build doesn't restart by itself: Restart Yap is in the tray"),
            }
            said = Some(why);
        }
        if why.is_none() {
            if by_itself {
                restart(app, By::Itself);
            }
            return;
        }
        std::thread::sleep(EVERY);
    }
}

/// Who restarted Yap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum By {
    /// Yap, once nothing would be lost.
    Itself,
    /// The person, from the tray's Restart Yap.
    Tray,
}

/// The note a restart leaves for the next Yap.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Marker {
    /// Unix seconds.
    at: u64,
    by: By,
    /// The main window was open: shown again (without the focus).
    show_main: bool,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Set for the Yap a restart starts (it inherits this process's
/// environment): a test run's restarted Yap has no harness on its stdin
/// (`e2e::start`).
const RESTARTED_ENV: &str = "YAP_RESTARTED_AFTER_WEBVIEW2";

/// This Yap was started by [`restart`].
pub fn restarted() -> bool {
    std::env::var(RESTARTED_ENV).is_ok_and(|v| v == "1")
}

/// **Restart Yap now** (the tray's Restart Yap, or by itself): the note for
/// the next Yap left first.
pub fn restart(app: &AppHandle, by: By) {
    let marker = Marker { at: now_secs(), by, show_main: SHOW_MAIN.load(Ordering::Relaxed) };
    let path = crate::config::data_dir().join(MARKER);
    match serde_json::to_vec(&marker).map_err(|e| e.to_string()).and_then(|b| std::fs::write(&path, b).map_err(|e| e.to_string())) {
        Ok(()) => {}
        Err(e) => tracing::warn!("Couldn't leave the restart's note: {e}"),
    }
    tracing::warn!(by = ?by, "Restarting Yap: its windows' WebView2 stopped");
    std::env::set_var(RESTARTED_ENV, "1");
    app.request_restart();
}

/// At start (setup): a restart's note left by the last Yap said once,
/// quietly, once the windows are up — and the main window shown again if it
/// was open.
pub fn init(app: &AppHandle) {
    let path = crate::config::data_dir().join(MARKER);
    let Ok(bytes) = std::fs::read(&path) else { return };
    let _ = std::fs::remove_file(&path);
    let Ok(marker) = serde_json::from_slice::<Marker>(&bytes) else { return };
    if now_secs().saturating_sub(marker.at) > MARKER_FRESH_SECS {
        return;
    }
    tracing::info!(by = ?marker.by, "Yap restarted after WebView2 stopped");
    let app = app.clone();
    let _ = std::thread::Builder::new().name("webview2-restarted".into()).spawn(move || {
        // (The windows' pages are up by then.)
        std::thread::sleep(Duration::from_secs(4));
        if marker.show_main {
            if let Some(w) = app.get_webview_window("settings") {
                crate::shell::window::show_quietly(&w);
            }
        }
        let body = match marker.by {
            By::Itself => "WebView2, which draws Yap's windows, stopped. Yap waited until nothing was in progress, so nothing was lost.",
            By::Tray => "WebView2, which draws Yap's windows, stopped, and you restarted Yap from the tray.",
        };
        crate::notices::alert::confirm(
            &app,
            crate::notices::alert::Alert {
                kind: NOTICE,
                icon: "settings",
                title: "Yap restarted its windows after WebView2 stopped".into(),
                body: body.into(),
                action: None,
                timeout_ms: 10_000,
            },
        );
    });
}

/// Debug builds in test mode: WebView2's browser process "exits" without a
/// crash — what Yap does then (the tray, the wait, the restart), with the
/// windows still there to look at (`webview2-exit.spec.js`).
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_webview2_gone(app: AppHandle) -> Result<(), String> {
    if !crate::shell::e2e::active() {
        return Err("Only in test mode.".into());
    }
    gone(&app, "simulated", 0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_in_progress_is_the_only_time_yap_restarts_by_itself() {
        assert_eq!(in_the_way(Work::default()), None);
        let each = [
            (Work { dictation: true, ..Default::default() }, "a dictation"),
            (Work { meeting: true, ..Default::default() }, "a meeting recording"),
            (Work { download: true, ..Default::default() }, "a model download"),
            (Work { chat: true, ..Default::default() }, "a Chat answer"),
            (Work { assistant: true, ..Default::default() }, "a Voice Assistant question"),
            (Work { code: true, ..Default::default() }, "Claude at work in a Code chat"),
            (Work { routine: true, ..Default::default() }, "a routine"),
        ];
        for (work, words) in each {
            assert_eq!(in_the_way(work), Some(words), "{work:?}");
        }
        // Several at once: the one that matters most is said.
        let all = Work { dictation: true, meeting: true, download: true, chat: true, assistant: true, code: true, routine: true };
        assert_eq!(in_the_way(all), Some("a dictation"));
        assert_eq!(in_the_way(Work { chat: true, routine: true, ..Default::default() }), Some("a Chat answer"));
    }

    #[test]
    fn a_dev_build_never_restarts_by_itself() {
        assert!(!restarts_by_itself(true));
        assert!(restarts_by_itself(false));
    }

    #[test]
    fn the_note_reads_back() {
        let m = Marker { at: 1_795_000_000, by: By::Tray, show_main: true };
        let json = serde_json::to_string(&m).unwrap();
        assert_eq!(json, r#"{"at":1795000000,"by":"tray","showMain":true}"#);
        let back: Marker = serde_json::from_str(&json).unwrap();
        assert_eq!(back.by, By::Tray);
        assert!(back.show_main);
    }
}
