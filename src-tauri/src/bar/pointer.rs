//! **First-use pointers on the Yap bar** (2026-10-29; Wispr Flow's, by
//! behaviour): for someone who has never dictated, a small pointer over the
//! idle pill says "Click a text box, then press F9" ("…then hold F9 and
//! talk" in push-to-talk), and over the first dictation's capsule "Talk,
//! then press F9 again" ("…then let go"). Each shows once, and both are
//! gone after the first dictation.
//!
//! Rust says which are still due ([`status`], in the bar's `Status`); the
//! page shows one when the bar is idle or recording with no card up, and
//! tells Rust it has ([`bar_pointer_shown`]), which counts it in
//! `<data>/hints.json` beside the push-to-talk hints (`coaching.rs`). A
//! dictation finished ([`dictated`]) ends them, as does any dictation in the
//! history from before. "Show tips" off: none. Not a region: the bar stays
//! click-through around it, and it never takes the focus.
//!
//! Test mode shows none unless a spec asks (`e2e_bar_pointer`).

use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;

/// The pointer over the idle pill.
pub const IDLE: &str = "idle";
/// The pointer over the first dictation's capsule.
pub const RECORDING: &str = "recording";

/// Which pointers are still due, and how the dictation key works.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Due {
    pub idle: bool,
    pub recording: bool,
    /// Push-to-talk: hold the key ("…hold F9 and talk", "…let go").
    pub hold: bool,
}

/// The pointers due for a person whose hints.json is `rec`, `ever` meaning
/// a dictation is in their history from before. `None`: none at all. Pure.
pub fn due_from(rec: &crate::dictation::coaching::Record, ever: bool, hold: bool) -> Option<Due> {
    if rec.dictated || ever {
        return None;
    }
    let d = Due { idle: !rec.pointers.contains_key(IDLE), recording: !rec.pointers.contains_key(RECORDING), hold };
    (d.idle || d.recording).then_some(d)
}

/// Push-to-talk, from the config (`sync`).
static HOLD: AtomicBool = AtomicBool::new(false);
/// A dictation is in the history from before: never any pointer.
static EVER: AtomicBool = AtomicBool::new(false);
static EVER_READ: AtomicBool = AtomicBool::new(false);

#[cfg(debug_assertions)]
static E2E_ON: AtomicBool = AtomicBool::new(false);

/// The settings the pointers follow (the bar reads its config).
pub fn sync(cfg: &crate::config::YapConfig) {
    HOLD.store(cfg.recording_mode == "pushToTalk", Ordering::SeqCst);
}

/// Has a dictation ever finished, by the history? Read once (then kept).
fn ever_dictated() -> bool {
    if !EVER_READ.swap(true, Ordering::SeqCst) {
        let any = !crate::dictation::history::all().is_empty();
        EVER.store(any, Ordering::SeqCst);
    }
    EVER.load(Ordering::SeqCst)
}

/// The pointers due now, for the bar's status. Never with the bar's lock
/// held (it reads hints.json and, once, the history).
pub fn status() -> Option<Due> {
    #[cfg(debug_assertions)]
    if crate::shell::e2e::active() {
        if !E2E_ON.load(Ordering::SeqCst) {
            return None;
        }
        return due_from(&crate::dictation::coaching::record(), false, HOLD.load(Ordering::SeqCst));
    }
    if crate::notices::notice::tips_off() || !crate::notices::kinds::on("bar-pointer") {
        return None;
    }
    let rec = crate::dictation::coaching::record();
    if rec.dictated {
        return None;
    }
    due_from(&rec, ever_dictated(), HOLD.load(Ordering::SeqCst))
}

/// A dictation finished: no more pointers, ever.
pub fn dictated() {
    if crate::dictation::coaching::record().dictated {
        return;
    }
    crate::dictation::coaching::update_record(|mut r| {
        r.dictated = true;
        r
    });
}

/// The page showed a pointer (`kind`: "idle" | "recording"): counted, so
/// it shows once.
#[tauri::command]
pub fn bar_pointer_shown(kind: String) -> Result<(), String> {
    let kind = match kind.as_str() {
        IDLE => IDLE,
        RECORDING => RECORDING,
        other => return Err(format!("No such pointer: {other}")),
    };
    tracing::info!(kind, "bar: first-use pointer shown");
    let now = crate::notices::notice::now_ms();
    crate::dictation::coaching::update_record(|mut r| {
        r.pointers.entry(kind.to_string()).or_insert(now);
        r
    });
    Ok(())
}

/// Test mode only: show the pointers (`on`) as for someone who has never
/// dictated — those shown and the first dictation forgotten.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_bar_pointer(app: tauri::AppHandle, on: bool) -> Result<Option<Due>, String> {
    if !crate::shell::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    E2E_ON.store(on, Ordering::SeqCst);
    crate::dictation::coaching::update_record(|mut r| {
        r.pointers.clear();
        r.dictated = false;
        r
    });
    crate::bar::refresh(&app);
    Ok(status())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictation::coaching::Record;

    #[test]
    fn both_pointers_until_each_has_shown_and_none_once_dictated() {
        let fresh = Record::default();
        assert_eq!(due_from(&fresh, false, false), Some(Due { idle: true, recording: true, hold: false }));
        assert_eq!(due_from(&fresh, false, true).map(|d| d.hold), Some(true));
        let mut r = Record::default();
        r.pointers.insert(IDLE.into(), 1);
        assert_eq!(due_from(&r, false, false), Some(Due { idle: false, recording: true, hold: false }));
        r.pointers.insert(RECORDING.into(), 2);
        assert_eq!(due_from(&r, false, false), None);
        // A first dictation, or one in the history from before: none.
        assert_eq!(due_from(&Record { dictated: true, ..Record::default() }, false, false), None);
        assert_eq!(due_from(&fresh, true, false), None);
    }

    #[test]
    fn an_older_hints_file_reads_as_never_pointed() {
        let old: Record = serde_json::from_str(r#"{"tap":2,"lastShownAt":5}"#).unwrap();
        assert!(old.pointers.is_empty() && !old.dictated);
        let mut r = Record::default();
        r.pointers.insert(IDLE.into(), 7);
        r.dictated = true;
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"pointers\":{\"idle\":7}") && json.contains("\"dictated\":true"), "{json}");
    }
}
