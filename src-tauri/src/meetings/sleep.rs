//! **The PC going to sleep mid-meeting** (2026-10-29, Wispr Flow's sleep
//! marker, by behaviour). Until now a recording went on across a sleep as
//! if nothing happened: the devices stopped delivering, the audio clock
//! stood still, and every line after the wake was dated by that clock —
//! minutes too early.
//!
//! - **On sleep** (Windows' suspend notification,
//!   `PowerRegisterSuspendResumeNotification`, a callback, no window): a
//!   meeting recording is paused cleanly (`meeting_end::pause_automatically`:
//!   the last chunk transcribed, the audio closed, a stop mark, no
//!   summary). One already stopping (Stop pressed, then the lid shut)
//!   finishes as asked: no line, no question.
//! - **On wake**: the transcript gets a line "Your PC was asleep for 12
//!   min" (`TranscriptSegment::asleep_marker`) in its place on the
//!   meeting's clock — after the paused recording's last line, before
//!   anything recorded since — once that recording's last chunk is in;
//!   call detection learns the calls again, and the call's app back on the
//!   microphone within 2 minutes carries on in the same note
//!   (`meeting_detect::on_wake`); else "Your PC was asleep" asks where the
//!   check-ins do (the notepad if it's on screen, the main window, the Yap
//!   bar, a Windows notification) with **Resume** and **Summarise**. The
//!   notepad's own Resume is there too.
//!
//! Network loss mid-meeting needs no line: speech is transcribed on this
//! PC, so the transcript has no gap; only a cloud model's digests wait
//! (retried after a couple of minutes, and caught up before the summary,
//! `meeting_summary.rs`), and a summary written while offline fails with
//! Retry.
//!
//! Test mode registers nothing: `e2e_sleep` pretends a sleep.

use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Emitter};

/// The paused recording's last chunk may take this long after the wake.
const DRAIN_WAIT: Duration = Duration::from_secs(120);
/// "Your PC was asleep" goes away unanswered after this long.
const ASK_FADE_MS: u64 = 10 * 60_000;

/// A meeting the sleep paused: its note, and when (unix ms).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Slept {
    note_id: u64,
    at_ms: u64,
}

static SLEPT: Mutex<Option<Slept>> = Mutex::new(None);

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// "12 min", "1 h 5 min", "40 s": how long the PC slept (`ms`), as the
/// transcript says it (`notepadText.js` `asleepWords` the same).
pub fn span_words(ms: u64) -> String {
    if ms < 60_000 {
        return format!("{} s", ms.div_ceil(1000).max(1));
    }
    let minutes = (ms + 30_000) / 60_000;
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

/// Where the line goes on the meeting's clock (unix seconds): when the PC
/// went to sleep, or just after the paused recording's last line if a
/// clock that ran a little ahead dated it later.
pub fn marker_ts(slept_at_ms: u64, last_line_ts: Option<u64>) -> u64 {
    (slept_at_ms / 1000).max(last_line_ts.unwrap_or(0))
}

/// Start with the app: Windows says when the PC sleeps and wakes.
pub fn init(app: &AppHandle) {
    if crate::shell::e2e::active() {
        return; // The suite pretends one: `e2e_sleep`.
    }
    #[cfg(windows)]
    power::register(app);
    #[cfg(not(windows))]
    let _ = app;
}

/// The PC is going to sleep: a meeting recording pauses cleanly.
pub fn suspending() {
    let Some(note_id) = crate::meetings::meeting::recording_note() else {
        return;
    };
    // Already ending or pausing (Stop and summarise, then the lid shut at
    // once): its last chunk is on its way, and it finishes as asked. Paused
    // here instead, an ended meeting got no summary, a "Your PC was asleep"
    // line and Resume, and its call's app back on the mic after the wake
    // carried on in it.
    if crate::meetings::meeting::stopping() {
        tracing::info!(note_id, "Sleep: the PC is going to sleep as a meeting's notes finish; left to finish as asked");
        return;
    }
    tracing::info!(note_id, "Sleep: the PC is going to sleep mid-meeting; pausing the notes");
    *SLEPT.lock().unwrap_or_else(|p| p.into_inner()) = Some(Slept { note_id, at_ms: now_ms() });
    if let Err(e) = crate::meetings::meeting_end::pause_automatically() {
        tracing::info!("Sleep: nothing to pause ({e})");
        *SLEPT.lock().unwrap_or_else(|p| p.into_inner()) = None;
    }
}

/// The PC woke (`asleep_ms`: how long it slept; `None`: since it went to
/// sleep). The line in the transcript, and Resume.
pub fn woke(app: &AppHandle, asleep_ms: Option<u64>) {
    let slept = SLEPT.lock().unwrap_or_else(|p| p.into_inner()).take();
    crate::calls::meeting_detect::on_wake(app, slept.map(|s| s.note_id));
    let Some(slept) = slept else { return };
    let ms = asleep_ms.unwrap_or_else(|| now_ms().saturating_sub(slept.at_ms));
    tracing::info!(note_id = slept.note_id, secs = ms / 1000, "Sleep: the PC woke; marking the gap");
    let app = app.clone();
    let spawned = std::thread::Builder::new().name("sleep-gap".into()).spawn(move || {
        // The paused recording's last chunk first (it may still be on its
        // way): the line goes after it.
        let until = Instant::now() + DRAIN_WAIT;
        while crate::meetings::meeting::recording_note() == Some(slept.note_id) && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(200));
        }
        mark(&app, slept, ms);
        // Back on the call already (`meeting_detect::on_wake`): nothing to ask.
        if crate::meetings::meeting::recording_note() == Some(slept.note_id) {
            return;
        }
        ask_resume(&app, slept.note_id, ms);
    });
    if let Err(e) = spawned {
        tracing::warn!("Sleep: couldn't mark the gap ({e})");
    }
}

/// "Your PC was asleep for 12 min" into the note, in its place.
fn mark(app: &AppHandle, slept: Slept, ms: u64) {
    let Some(note) = crate::notes::get(slept.note_id) else { return };
    let last = note.transcript.iter().filter(|s| s.ts <= slept.at_ms / 1000 + 60).map(|s| s.ts).max();
    let line = crate::notes::TranscriptSegment::asleep_marker(marker_ts(slept.at_ms, last), ms);
    match crate::notes::insert_transcript(slept.note_id, &[line]) {
        Ok(()) => {
            // The Notes view's open note, and the notepad (which reloads on
            // any change to the notes).
            let _ = app.emit("yap-meeting-transcript", serde_json::json!({ "noteId": slept.note_id }));
            let _ = app.emit("yap-notes-changed", ());
        }
        Err(e) => tracing::warn!(note_id = slept.note_id, "Sleep: the gap wasn't marked: {e}"),
    }
}

/// "Your PC was asleep": Resume, or Summarise.
fn ask_resume(app: &AppHandle, note_id: u64, ms: u64) {
    use crate::calls::meeting_guard::{About, Answer, Question};
    crate::calls::meeting_guard::ask(
        app,
        Question {
            note_id,
            kind: "asleep",
            icon: "notes",
            title: "Your PC was asleep".to_string(),
            body: format!(
                "Yap paused your meeting notes for the {} it slept. Resume to carry on in the same note.",
                span_words(ms)
            ),
            primary: Answer::new("resume", "Resume"),
            secondary: Answer::new("summarise", "Summarise"),
            about: About::Asleep,
            fade_ms: ASK_FADE_MS,
        },
    );
}

/// Test mode only: the PC "sleeps" now and "wakes" `asleepMs` later (at
/// once: the line says that long).
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_sleep(app: AppHandle, asleep_ms: u64) -> Result<(), String> {
    if !crate::shell::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    tracing::info!(asleep_ms, "e2e: the PC sleeps");
    suspending();
    woke(&app, Some(asleep_ms));
    Ok(())
}

/// Windows' suspend and resume notifications, on a callback (no window).
#[cfg(windows)]
mod power {
    use std::ffi::c_void;
    use std::sync::mpsc::{self, Sender};
    use std::sync::Mutex;

    use tauri::AppHandle;

    const DEVICE_NOTIFY_CALLBACK: u32 = 2;
    const PBT_APMSUSPEND: u32 = 0x4;
    const PBT_APMRESUMEAUTOMATIC: u32 = 0x12;

    /// DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS.
    #[repr(C)]
    struct Subscribe {
        callback: unsafe extern "system" fn(*mut c_void, u32, *mut c_void) -> u32,
        context: *mut c_void,
    }

    #[link(name = "powrprof", kind = "raw-dylib")]
    extern "system" {
        fn PowerRegisterSuspendResumeNotification(flags: u32, recipient: *mut c_void, handle: *mut *mut c_void) -> u32;
    }

    /// `true`: going to sleep; `false`: awake again.
    static TX: Mutex<Option<Sender<bool>>> = Mutex::new(None);

    unsafe extern "system" fn on_power(_context: *mut c_void, kind: u32, _setting: *mut c_void) -> u32 {
        let event = match kind {
            PBT_APMSUSPEND => Some(true),
            PBT_APMRESUMEAUTOMATIC => Some(false),
            _ => None,
        };
        if let Some(e) = event {
            if let Some(tx) = TX.lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
                let _ = tx.send(e);
            }
        }
        0
    }

    pub fn register(app: &AppHandle) {
        let (tx, rx) = mpsc::channel::<bool>();
        *TX.lock().unwrap_or_else(|p| p.into_inner()) = Some(tx);
        // Lives as long as Yap: Windows keeps the pointer.
        let params: &'static mut Subscribe = Box::leak(Box::new(Subscribe { callback: on_power, context: std::ptr::null_mut() }));
        let mut handle: *mut c_void = std::ptr::null_mut();
        // SAFETY: `params` is valid for the life of the process; the
        // callback only sends on a channel.
        let r = unsafe {
            PowerRegisterSuspendResumeNotification(DEVICE_NOTIFY_CALLBACK, params as *mut Subscribe as *mut c_void, &mut handle)
        };
        if r != 0 {
            tracing::warn!(code = r, "Sleep: Windows won't say when the PC sleeps");
            return;
        }
        let app = app.clone();
        let spawned = std::thread::Builder::new().name("sleep".into()).spawn(move || {
            for asleep in rx {
                if asleep {
                    super::suspending();
                } else {
                    super::woke(&app, None);
                }
            }
        });
        if let Err(e) = spawned {
            tracing::warn!("Sleep: couldn't follow the PC's sleep ({e})");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_says_how_long_in_plain_words() {
        assert_eq!(span_words(40_000), "40 s");
        assert_eq!(span_words(200), "1 s");
        assert_eq!(span_words(60_000), "1 min");
        assert_eq!(span_words(12 * 60_000 + 20_000), "12 min");
        assert_eq!(span_words(60 * 60_000), "1 h");
        assert_eq!(span_words(65 * 60_000 + 10_000), "1 h 5 min");
    }

    #[test]
    fn the_line_goes_where_the_pc_went_to_sleep() {
        // When it went to sleep (unix seconds)…
        assert_eq!(marker_ts(1_700_000_123_456, Some(1_700_000_100)), 1_700_000_123);
        // …or after the paused recording's last line, dated a moment later.
        assert_eq!(marker_ts(1_700_000_123_456, Some(1_700_000_124)), 1_700_000_124);
        assert_eq!(marker_ts(1_700_000_123_456, None), 1_700_000_123);
    }
}
