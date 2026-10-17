//! Push-to-talk coaching hints (Wispr Flow's, by behaviour): a lone tap of
//! the dictation hotkey, or a hold of a minute or more, earns a short tip
//! (`gesture::Hint`). Each shows at most [`MAX_SHOWS`] times, never within
//! [`MIN_GAP_MS`] of another, and none at all once double-tap has been used
//! (they're about learning it).
//!
//! Where: a toast in the main window while it's on screen (as an update's
//! announcement does), else a card on the Yap bar. Neither (the bar off or
//! hidden for an hour, the window in the tray): nothing, and nothing
//! counted — no Windows notification for a tip. The hotkey's name is
//! written by the page, with `lib/hotkeys.js` (`formatHotkeySpec`): the
//! texts say `{key}` and carry the spec.
//!
//! The counts live in `<data>/hints.json`, not config.json: Settings keeps
//! its own copy of the config and auto-saves it, which would write old
//! counts back.

use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::gesture::Hint;

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
    /// When a hint last showed (unix ms).
    pub last_shown_at: u64,
    /// Double-tap has locked a recording at least once: no more hints.
    pub hands_free_used: bool,
}

/// May `hint` show at `now` (unix ms)?
pub fn eligible(rec: &Record, hint: Hint, now: u64) -> bool {
    if rec.hands_free_used {
        return false;
    }
    let shown = match hint {
        Hint::Tap => rec.tap,
        Hint::LongHold => rec.long_hold,
    };
    shown < MAX_SHOWS && (rec.last_shown_at == 0 || now.saturating_sub(rec.last_shown_at) >= MIN_GAP_MS)
}

/// `rec` after `hint` showed at `now`.
pub fn shown(mut rec: Record, hint: Hint, now: u64) -> Record {
    match hint {
        Hint::Tap => rec.tap += 1,
        Hint::LongHold => rec.long_hold += 1,
    }
    rec.last_shown_at = now;
    rec
}

/// The hint's title and body; `{key}` stands for the hotkey's name.
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
    }
}

/// What the main window gets (`yap-hint`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HintToast {
    /// "tap" | "longHold".
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

fn record() -> Record {
    loaded(&mut RECORD.lock().unwrap_or_else(|p| p.into_inner())).clone()
}

/// Change the record and save it, under the one lock (a hint shown and
/// double-tap used at once both stick).
fn update_record(f: impl FnOnce(Record) -> Record) {
    let mut guard = RECORD.lock().unwrap_or_else(|p| p.into_inner());
    let rec = f(loaded(&mut guard).clone());
    match serde_json::to_string_pretty(&rec) {
        Ok(json) => {
            let _ = std::fs::create_dir_all(crate::config::data_dir());
            if let Err(e) = crate::config::atomic_write(&record_path(), &json) {
                tracing::warn!("coaching: couldn't save hints.json: {}", e);
            }
        }
        Err(e) => tracing::warn!("coaching: couldn't serialize hints.json: {}", e),
    }
    *guard = Some(rec);
}

// ---- showing -------------------------------------------------------------------------------

/// The main window is on screen (not hidden, not minimized). Waits on the
/// main thread: never call with a lock held.
fn main_window_visible(app: &AppHandle) -> bool {
    app.get_webview_window("settings")
        .is_some_and(|w| w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false))
}

/// Show `hint` if it's due, about the dictation hotkey `hotkey` (a spec,
/// "kb:120"). Blocking (window queries): run it off the pipeline's locks,
/// on a thread of its own.
pub fn offer(app: &AppHandle, hint: Hint, hotkey: &str) {
    let now = now_ms();
    if !eligible(&record(), hint, now) {
        tracing::debug!(?hint, "coaching: not now");
        return;
    }
    let (title, body) = text(hint);
    let in_window = main_window_visible(app);
    let shown_now = if in_window {
        let kind = match hint {
            Hint::Tap => "tap",
            Hint::LongHold => "longHold",
        };
        app.emit(EVENT, HintToast { kind, title, body, hotkey: hotkey.to_string() }).is_ok()
    } else {
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
    if !crate::e2e::active() {
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
        for hint in [Hint::Tap, Hint::LongHold] {
            let (title, body) = text(hint);
            assert!(title.contains("{key}") && body.contains("{key}"), "{hint:?}");
        }
    }
}
