//! Push-to-talk coaching hints (Wispr Flow's, by behaviour): a lone tap of
//! the dictation hotkey, or a hold of a minute or more, earns a short tip
//! (`gesture::Hint`). Each shows at most [`MAX_SHOWS`] times, never within
//! [`MIN_GAP_MS`] of another, and none at all once double-tap has been used
//! (they're about learning it). Two more share the budget: a press that
//! grew into a chord ("Hold only {key} to dictate", either recording mode;
//! not silenced by double-tap) and a dictation of ten minutes or more
//! ("Long dictation? Try meeting notes", naming the meeting shortcut: shown
//! once, ever — [`max_shows`]).
//!
//! Where: one place (`notice::place`): a toast in the main window while the
//! person is looking at it, else a card on the Yap bar, else the window's
//! toast if it's on screen. Neither (the bar off or hidden for an hour, the
//! window in the tray): nothing, and nothing counted — no Windows
//! notification for a tip. "Show tips" off (Settings → General →
//! Notifications): none at all. On the bar a hint waits out a dictation or
//! a meeting, and is dropped if it can't show within 10 s (`notice.rs`). The hotkey's name is
//! written by the page, with `lib/core/hotkeys.js` (`formatHotkeySpec`): the
//! texts say `{key}` and carry the spec.
//!
//! The counts live in `<data>/hints.json`, not config.json: Settings keeps
//! its own copy of the config and auto-saves it, which would write old
//! counts back.

use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::dictation::gesture::Hint;

/// Each hint shows this many times at most.
pub const MAX_SHOWS: u32 = 3;
/// …and never within this long of the last one (either hint).
pub const MIN_GAP_MS: u64 = 10 * 60_000;
/// The main window's event for a hint toast.
const EVENT: &str = "yap-hint";
/// The bar card's id (one hint at a time).
const CARD: &str = "hint";
/// How long a hint card stays up.
const CARD_MS: u64 = 10_000;

/// What `hints.json` remembers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Record {
    /// Times the lone-tap hint showed.
    pub tap: u32,
    /// Times the long-hold hint showed.
    pub long_hold: u32,
    /// Times the "hold only {key}" hint showed (a press that grew into a
    /// chord, either recording mode).
    pub chord: u32,
    /// Times the "try meeting notes" hint showed (a ten-minute dictation).
    pub long_dictation: u32,
    /// When a hint last showed (unix ms).
    pub last_shown_at: u64,
    /// Double-tap has locked a recording at least once: no more hints
    /// about it (the tap and long-hold ones).
    pub hands_free_used: bool,
    /// Feature tips shown (`tips.rs`): each kind's when (unix ms), once
    /// each. An older Yap drops this when it saves a hint: a tip may then
    /// show once more.
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub features: std::collections::BTreeMap<String, u64>,
    /// The Yap bar's first-use pointers shown (`bar/pointer.rs`): each
    /// kind's when (unix ms).
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub pointers: std::collections::BTreeMap<String, u64>,
    /// A dictation has finished at least once: no first-use pointers.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub dictated: bool,
}

/// How many times `hint` shows at most: the meeting-notes suggestion once,
/// ever; the others [`MAX_SHOWS`].
pub fn max_shows(hint: Hint) -> u32 {
    match hint {
        Hint::LongDictation => 1,
        _ => MAX_SHOWS,
    }
}

/// The hints that teach double-tap (none once it's been used).
fn about_double_tap(hint: Hint) -> bool {
    matches!(hint, Hint::Tap | Hint::LongHold)
}

fn count(rec: &Record, hint: Hint) -> u32 {
    match hint {
        Hint::Tap => rec.tap,
        Hint::LongHold => rec.long_hold,
        Hint::ChordHold | Hint::ChordPress => rec.chord,
        Hint::LongDictation => rec.long_dictation,
    }
}

/// May `hint` show at `now` (unix ms)?
pub fn eligible(rec: &Record, hint: Hint, now: u64) -> bool {
    if rec.hands_free_used && about_double_tap(hint) {
        return false;
    }
    count(rec, hint) < max_shows(hint) && (rec.last_shown_at == 0 || now.saturating_sub(rec.last_shown_at) >= MIN_GAP_MS)
}

/// `rec` after `hint` showed at `now`.
pub fn shown(mut rec: Record, hint: Hint, now: u64) -> Record {
    match hint {
        Hint::Tap => rec.tap += 1,
        Hint::LongHold => rec.long_hold += 1,
        Hint::ChordHold | Hint::ChordPress => rec.chord += 1,
        Hint::LongDictation => rec.long_dictation += 1,
    }
    rec.last_shown_at = now;
    rec
}

/// The hint's title and body; `{key}` stands for the hotkey's name (the
/// meeting shortcut's, for [`Hint::LongDictation`]).
pub fn text(hint: Hint) -> (&'static str, &'static str) {
    match hint {
        Hint::Tap => (
            "Hold {key} while you talk",
            "Push-to-talk records while the key is down. Or double-tap {key} to keep recording hands-free, and press it once more to stop.",
        ),
        Hint::LongHold => (
            "Double-tap {key} for hands-free",
            "No need to hold the key through a long dictation: double-tap {key} and Yap keeps recording until you press it again.",
        ),
        Hint::ChordHold => (
            "Hold only {key} to dictate",
            "Another key went down with {key} within a second, so Yap took it for a shortcut and didn't keep the recording. To dictate, hold {key} on its own.",
        ),
        Hint::ChordPress => (
            "Press only {key} to dictate",
            "Another key went down with {key} within a second, so Yap took it for a shortcut and didn't keep the recording. To dictate, press {key} on its own.",
        ),
        Hint::LongDictation => (
            "Long dictation? Try meeting notes",
            "Meeting notes keep going past a dictation's 15-minute limit, and summarise what was said. Press {key} to start them next time.",
        ),
    }
}

/// [`Hint::LongDictation`]'s body when the meeting shortcut is unbound.
const LONG_DICTATION_NO_KEY: &str =
    "Meeting notes keep going past a dictation's 15-minute limit, and summarise what was said. Start them from Meetings next time.";

/// What the main window gets (`yap-hint`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HintToast {
    /// "tap" | "longHold" | "chord" | "longDictation".
    pub kind: &'static str,
    pub title: &'static str,
    pub body: &'static str,
    /// The dictation hotkey's spec, named in place of `{key}`.
    pub hotkey: String,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ---- hints.json ---------------------------------------------------------------------------

static RECORD: Mutex<Option<Record>> = Mutex::new(None);

fn record_path() -> std::path::PathBuf {
    crate::config::data_dir().join("hints.json")
}

/// The cached record, read from disk the first time.
fn loaded(slot: &mut Option<Record>) -> &mut Record {
    slot.get_or_insert_with(|| {
        let path = record_path();
        match std::fs::read_to_string(&path) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_else(|_| {
                crate::config::quarantine_corrupt(&path);
                Record::default()
            }),
            Err(_) => Record::default(),
        }
    })
}

/// hints.json as it is now (feature tips and the bar's pointers keep their
/// counts here too).
pub(crate) fn record() -> Record {
    loaded(&mut RECORD.lock().unwrap_or_else(|p| p.into_inner())).clone()
}

/// Change the record and save it, under the one lock (a hint shown and
/// double-tap used at once both stick).
pub(crate) fn update_record(f: impl FnOnce(Record) -> Record) {
    let mut guard = RECORD.lock().unwrap_or_else(|p| p.into_inner());
    let rec = f(loaded(&mut guard).clone());
    let _ = SAVER.write_json(&record_path(), &rec, true);
    *guard = Some(rec);
}

/// hints.json's saves (`saving.rs`): a failed one keeps the record in
/// memory and is tried again with the next save; only logged (a hint shown
/// once more is all it costs).
pub static SAVER: crate::shell::saving::Saver = crate::shell::saving::Saver::quiet("hints", "your tips' progress", save_now);

/// Write the record again (Try again).
fn save_now() -> Result<(), crate::shell::saving::SaveError> {
    if let Some(rec) = RECORD.lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
        let _ = SAVER.write_json(&record_path(), rec, true);
    }
    SAVER.check()
}

// ---- showing -------------------------------------------------------------------------------

/// Show `hint` if it's due, about the hotkey `hotkey` (a spec, "kb:120": the
/// dictation hotkey; the meeting shortcut for [`Hint::LongDictation`]).
/// Blocking (window queries): run it off the pipeline's locks, on a thread
/// of its own.
pub fn offer(app: &AppHandle, hint: Hint, hotkey: &str) {
    let now = now_ms();
    if crate::notices::notice::tips_off() {
        tracing::debug!(?hint, "coaching: tips are switched off");
        return;
    }
    if !eligible(&record(), hint, now) {
        tracing::debug!(?hint, "coaching: not now");
        return;
    }
    let (title, mut body) = text(hint);
    if hint == Hint::LongDictation && hotkey.trim().is_empty() {
        body = LONG_DICTATION_NO_KEY;
    }
    // One place (`notice.rs`): the main window while the person is looking
    // at it, else the Yap bar; never a Windows notification for a tip.
    let place = crate::notices::notice::place_tip(app, "hint");
    let in_window = place == crate::notices::notice::Place::Window;
    let shown_now = if in_window {
        let kind = match hint {
            Hint::Tap => "tap",
            Hint::LongHold => "longHold",
            Hint::ChordHold | Hint::ChordPress => "chord",
            Hint::LongDictation => "longDictation",
        };
        app.emit(EVENT, HintToast { kind, title, body, hotkey: hotkey.to_string() }).is_ok()
    } else if place == crate::notices::notice::Place::Bar {
        let card = crate::bar::Card {
            id: CARD.into(),
            icon: "tip",
            title: title.into(),
            body: body.into(),
            hotkey: Some(hotkey.to_string()),
            timeout_ms: Some(CARD_MS),
            ..Default::default()
        };
        crate::bar::show_card(app, card, Box::new(|_, _| {}))
    } else {
        false
    };
    if shown_now {
        tracing::info!(?hint, in_window, "coaching: hint shown");
        update_record(|r| shown(r, hint, now));
    } else {
        tracing::debug!(?hint, "coaching: nowhere to show it");
    }
}

/// Double-tap locked a recording: the hints have done their job.
pub fn hands_free_used() {
    if record().hands_free_used {
        return;
    }
    update_record(|mut r| {
        r.hands_free_used = true;
        r
    });
}

/// Test mode only: forget every hint shown (and double-tap used), so each
/// e2e test starts with all of them due.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_coaching_reset() -> Result<(), String> {
    if !crate::shell::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    update_record(|_| Record::default());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: u64 = 1_700_000_000_000;

    #[test]
    fn a_fresh_record_shows_either_hint() {
        let r = Record::default();
        assert!(eligible(&r, Hint::Tap, T0));
        assert!(eligible(&r, Hint::LongHold, T0));
    }

    #[test]
    fn hints_keep_ten_minutes_apart_and_stop_after_three() {
        let mut r = Record::default();
        let mut t = T0;
        for n in 1..=MAX_SHOWS {
            assert!(eligible(&r, Hint::Tap, t), "show {n}");
            r = shown(r, Hint::Tap, t);
            assert_eq!(r.tap, n);
            // Not again, nor the other hint, within ten minutes.
            assert!(!eligible(&r, Hint::Tap, t + MIN_GAP_MS - 1));
            assert!(!eligible(&r, Hint::LongHold, t + 60_000));
            t += MIN_GAP_MS;
        }
        // Three taps shown: no more, however long it's been…
        assert!(!eligible(&r, Hint::Tap, t + 30 * 24 * 3_600_000));
        // …but the long-hold hint has its own three.
        assert!(eligible(&r, Hint::LongHold, t));
    }

    #[test]
    fn no_hints_once_double_tap_has_been_used() {
        let r = Record { hands_free_used: true, ..Record::default() };
        assert!(!eligible(&r, Hint::Tap, T0));
        assert!(!eligible(&r, Hint::LongHold, T0));
    }

    #[test]
    fn a_clock_set_back_waits_rather_than_repeating() {
        let r = shown(Record::default(), Hint::Tap, T0);
        assert!(!eligible(&r, Hint::Tap, T0 - 3_600_000));
    }

    #[test]
    fn the_store_reads_back_and_tolerates_missing_fields() {
        let r = shown(Record::default(), Hint::LongHold, T0);
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"longHold\":1"), "{json}");
        assert_eq!(serde_json::from_str::<Record>(&json).unwrap(), r);
        let old: Record = serde_json::from_str(r#"{"tap":2}"#).unwrap();
        assert_eq!(old, Record { tap: 2, ..Record::default() });
    }

    #[test]
    fn the_texts_name_the_key() {
        for hint in [Hint::Tap, Hint::LongHold, Hint::ChordHold, Hint::ChordPress] {
            let (title, body) = text(hint);
            assert!(title.contains("{key}") && body.contains("{key}"), "{hint:?}");
        }
        // The meeting-notes one names the meeting shortcut in its body, and
        // has words for none.
        assert!(text(Hint::LongDictation).1.contains("{key}"));
        assert!(!LONG_DICTATION_NO_KEY.contains("{key}"));
    }

    #[test]
    fn the_chord_hint_shares_the_budget_but_not_double_taps_silence() {
        // Double-tap learned: tap and long-hold hints stop, the chord one
        // doesn't (it's about a different mistake).
        let r = Record { hands_free_used: true, ..Record::default() };
        assert!(eligible(&r, Hint::ChordHold, T0));
        assert!(eligible(&r, Hint::LongDictation, T0));
        // Both chord texts count as one hint: three at most, ten minutes apart.
        let mut r = Record::default();
        let mut t = T0;
        for hint in [Hint::ChordHold, Hint::ChordPress, Hint::ChordHold] {
            assert!(eligible(&r, hint, t));
            r = shown(r, hint, t);
            assert!(!eligible(&r, Hint::Tap, t + 1), "the gap holds for every hint");
            t += MIN_GAP_MS;
        }
        assert_eq!(r.chord, 3);
        assert!(!eligible(&r, Hint::ChordPress, t));
        assert!(eligible(&r, Hint::Tap, t));
    }

    #[test]
    fn the_meeting_notes_suggestion_shows_once_ever() {
        let r = shown(Record::default(), Hint::LongDictation, T0);
        assert_eq!(r.long_dictation, 1);
        assert!(!eligible(&r, Hint::LongDictation, T0 + 365 * 24 * 3_600_000));
        // An old hints.json without the new counts reads as none shown.
        let old: Record = serde_json::from_str(r#"{"tap":1,"handsFreeUsed":true}"#).unwrap();
        assert_eq!((old.chord, old.long_dictation), (0, 0));
    }
}
