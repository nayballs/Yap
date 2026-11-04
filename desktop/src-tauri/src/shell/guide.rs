//! **Getting started** (2026-10-29): what the first-run setup and the
//! main window's "Set up meeting notes" checklist remember, in
//! `<data>/onboarding.json` (its own file, so an older Yap sharing the data
//! folder never drops it: it doesn't know the file and leaves it alone).
//!
//! - **The calendar step** (`calendarStep`): "connected" or "skipped" —
//!   the setup never shows it again either way (nor once a calendar is
//!   connected some other way: `calendar::connected`).
//! - **The typing test** (`typingWpm`, `speakingWpm`, `typingAt`): your
//!   typing speed, and how fast you read its line aloud, as numbers of
//!   words a minute — nothing typed or said is kept.
//! - **The checklist** (`checklistDismissed`): connect a calendar, record a
//!   first meeting (or try one), add Yap to an AI app (MCP) — each ticked by
//!   itself as it's done ([`meeting_setup_status`]); hidden once dismissed or
//!   all done.
//!
//! A reset (`privacy/reset.rs`) keeps it with Yap's own bookkeeping.
//!
//! Test mode shows the checklist only once a spec asks (`e2e_guide_reset`),
//! so its "Record your first meeting" never stands beside the Record button
//! another spec means.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// What `onboarding.json` keeps.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Record {
    /// The setup's calendar step: "" (not yet), "connected" or "skipped".
    pub calendar_step: String,
    /// The typing test's result, in words a minute.
    pub typing_wpm: Option<u32>,
    /// Its line read aloud, in words a minute (none: not read).
    pub speaking_wpm: Option<u32>,
    /// When it was taken (unix ms).
    pub typing_at: u64,
    /// The "Set up meeting notes" checklist was dismissed.
    pub checklist_dismissed: bool,
}

/// The fastest typing speed kept (a test finished in a blink is a mistake).
pub const MAX_WPM: u32 = 250;

/// A calendar step's answer, as saved. Pure.
pub fn calendar_answer(state: &str) -> Option<&'static str> {
    match state {
        "connected" => Some("connected"),
        "skipped" => Some("skipped"),
        _ => None,
    }
}

/// Whether the setup still shows its calendar step: never once it was
/// answered, nor with a calendar connected. Pure.
pub fn calendar_step_due(rec: &Record, connected: bool) -> bool {
    rec.calendar_step.is_empty() && !connected
}

/// The checklist's items, done or not.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Checklist {
    pub calendar: bool,
    pub meeting: bool,
    pub mcp: bool,
    pub dismissed: bool,
    /// Shown in the sidebar: not dismissed and not all done.
    pub shown: bool,
}

/// The checklist from what's done. Pure.
pub fn checklist(calendar: bool, meeting: bool, mcp: bool, dismissed: bool) -> Checklist {
    Checklist { calendar, meeting, mcp, dismissed, shown: !dismissed && !(calendar && meeting && mcp) }
}

/// Test mode: the checklist shown (`e2e_guide_reset` turns it on).
static IN_TESTS: AtomicBool = AtomicBool::new(false);

// ---- onboarding.json ----------------------------------------------------------------------------

static RECORD: Mutex<Option<Record>> = Mutex::new(None);

fn path() -> std::path::PathBuf {
    crate::config::data_dir().join("onboarding.json")
}

/// onboarding.json, read the first time (`saving::Saver::loaded`: set aside
/// if it isn't the record; one Yap couldn't read is never written over, and
/// read again here until it reads). Unit tests never touch the data folder.
fn loaded(slot: &mut Option<Record>) -> &mut Record {
    if cfg!(test) {
        return slot.get_or_insert_with(Record::default);
    }
    SAVER.loaded(slot, &path(), true)
}

/// onboarding.json as it is now.
pub fn record() -> Record {
    loaded(&mut RECORD.lock().unwrap_or_else(|p| p.into_inner())).clone()
}

fn update(f: impl FnOnce(&mut Record)) {
    let mut guard = RECORD.lock().unwrap_or_else(|p| p.into_inner());
    let rec = loaded(&mut guard);
    f(rec);
    if !cfg!(test) {
        let _ = SAVER.write_json(&path(), &*rec, true);
    }
}

/// onboarding.json's saves (`saving.rs`): only logged (a step shown once
/// more is all a lost save costs).
pub static SAVER: crate::shell::saving::Saver = crate::shell::saving::Saver::quiet("onboarding", "your setup's progress", save_now);

fn save_now() -> Result<(), crate::shell::saving::SaveError> {
    let mut guard = RECORD.lock().unwrap_or_else(|p| p.into_inner());
    if guard.is_some() {
        let _ = SAVER.write_json(&path(), &*loaded(&mut guard), true);
    }
    drop(guard);
    SAVER.check()
}

// ---- commands -----------------------------------------------------------------------------------

/// What the setup and the main window need: the record, and whether the
/// calendar step is still due.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    #[serde(flatten)]
    pub record: Record,
    pub calendar_step_due: bool,
}

#[tauri::command]
pub fn guide_status() -> Status {
    let record = record();
    let calendar_step_due = calendar_step_due(&record, crate::calendar::connected());
    Status { record, calendar_step_due }
}

/// The setup's calendar step was answered: "connected" | "skipped".
#[tauri::command]
pub fn guide_calendar_step(state: String) -> Result<(), String> {
    let answer = calendar_answer(&state).ok_or_else(|| format!("No such answer: {state}"))?;
    tracing::info!(answer, "guide: the calendar step answered");
    update(|r| r.calendar_step = answer.to_string());
    Ok(())
}

/// The fastest speaking speed kept.
pub const MAX_SPEAKING_WPM: u32 = 400;

/// The typing test's result (words a minute), and its line read aloud
/// (`speaking`, once said): kept as numbers only. A typing speed without a
/// spoken one keeps the spoken one from before.
#[tauri::command]
pub fn guide_typing(wpm: u32, speaking: Option<u32>) -> Result<u32, String> {
    if wpm == 0 || wpm > MAX_WPM || speaking.is_some_and(|s| s == 0 || s > MAX_SPEAKING_WPM) {
        return Err("That speed doesn't look right".to_string());
    }
    tracing::info!(wpm, ?speaking, "guide: typing test taken");
    let now = crate::notices::notice::now_ms();
    update(|r| {
        r.typing_wpm = Some(wpm);
        if speaking.is_some() {
            r.speaking_wpm = speaking;
        }
        r.typing_at = now;
    });
    Ok(wpm)
}

/// "Set up meeting notes": each item done or not, and whether it shows.
#[tauri::command]
pub async fn meeting_setup_status() -> Checklist {
    tauri::async_runtime::spawn_blocking(|| {
        let calendar = crate::calendar::connected();
        let meeting = crate::notes::all().iter().any(|n| n.note_type == "meeting");
        let mcp = crate::integrations::mcp_clients::added_anywhere();
        let mut list = checklist(calendar, meeting, mcp, record().checklist_dismissed);
        list.shown &= !crate::shell::e2e::active() || IN_TESTS.load(Ordering::Relaxed);
        list
    })
    .await
    .unwrap_or_default()
}

/// The checklist's ✕: hidden for good.
#[tauri::command]
pub fn meeting_setup_dismiss() {
    tracing::info!("guide: the meeting notes checklist dismissed");
    update(|r| r.checklist_dismissed = true);
}

/// Test mode only: onboarding.json forgotten (every step due again).
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_guide_reset() -> Result<(), String> {
    if !crate::shell::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    IN_TESTS.store(true, Ordering::Relaxed);
    update(|r| *r = Record::default());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_calendar_step_shows_until_answered_or_connected() {
        let fresh = Record::default();
        assert!(calendar_step_due(&fresh, false));
        assert!(!calendar_step_due(&fresh, true));
        for answer in ["connected", "skipped"] {
            let r = Record { calendar_step: calendar_answer(answer).unwrap().into(), ..Record::default() };
            assert!(!calendar_step_due(&r, false), "{answer}");
        }
        assert_eq!(calendar_answer("maybe"), None);
    }

    #[test]
    fn the_checklist_ticks_itself_and_goes_once_done_or_dismissed() {
        let c = checklist(true, false, false, false);
        assert!(c.calendar && !c.meeting && c.shown);
        assert!(!checklist(true, true, true, false).shown);
        assert!(!checklist(false, false, false, true).shown);
    }

    #[test]
    fn the_file_reads_back_and_an_unknown_field_is_harmless() {
        let r = Record { calendar_step: "skipped".into(), typing_wpm: Some(48), speaking_wpm: Some(150), typing_at: 5, checklist_dismissed: true };
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"typingWpm\":48") && json.contains("\"calendarStep\":\"skipped\""), "{json}");
        assert_eq!(serde_json::from_str::<Record>(&json).unwrap(), r);
        let newer: Record = serde_json::from_str(r#"{"typingWpm":40,"somethingLater":true}"#).unwrap();
        assert_eq!(newer.typing_wpm, Some(40));
    }
}
