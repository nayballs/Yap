//! Local transcription history + derived stats.
//!
//! Every successful dictation is appended (best-effort) to `history.json` in the
//! data dir: timestamp, raw transcript, final injected text, model, and the
//! foreground app. It's **local-only** — nothing leaves the machine — and can be
//! disabled (`history_enabled`) or cleared from Settings.
//!
//! From the same table we derive a small stats dashboard (words dictated, time
//! saved vs typing, a daily streak, and a 30-day activity series). Days use the
//! same UTC day-number trick as `usage.rs`, so no date crate is needed.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Cap the on-disk history so the file can't grow without bound.
const MAX_ENTRIES: usize = 2_000;
/// Average typing speed (words/min) used for the "time saved" estimate.
const TYPING_WPM: f64 = 40.0;
/// Average speaking speed (words/min) used for the "time saved" estimate.
const SPEAKING_WPM: f64 = 150.0;
/// How many days of activity the dashboard chart shows.
const ACTIVITY_DAYS: u64 = 30;

/// One recorded dictation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// Unix seconds when the dictation was injected.
    pub ts: u64,
    /// The raw STT transcript, before cleanup/dictionary.
    pub raw: String,
    /// The final text that was actually injected (after cleanup + dictionary).
    pub text: String,
    /// The words as said, when AI cleanup changed them: the transcript
    /// through the corrections, the voice command and snippets, no
    /// cleanup (`pipeline::as_said_text`), fitted to the same place as
    /// `text`. Home's "AI edited · N changes" compares the two, and its
    /// "Paste what I said" pastes it (`as_said.rs`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub said: Option<String>,
    /// Active model id at the time.
    #[serde(default)]
    pub model: String,
    /// Foreground app (process base name, e.g. "chrome.exe"), best-effort.
    #[serde(default)]
    pub app: String,
    /// Its audio in `recent-audio/` (`recent_audio.rs`), kept a day so it
    /// can be run again; the list says whether it's still there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<String>,
    /// `None`: transcribed and pasted. "failed": never transcribed (no
    /// model, the model failed, Yap closed mid-way). "notPasted":
    /// transcribed, but the paste couldn't land.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// What went wrong, in a few words, for those.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// A dictation that never got transcribed.
pub const FAILED: &str = "failed";
/// A dictation transcribed whose paste couldn't land.
pub const NOT_PASTED: &str = "notPasted";

impl HistoryEntry {
    fn failed(&self) -> bool {
        self.status.as_deref() == Some(FAILED)
    }
}

/// In-memory cache of the on-disk history (loaded lazily on first access).
static STATE: Mutex<Option<Vec<HistoryEntry>>> = Mutex::new(None);

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn day_of(ts: u64) -> u64 {
    ts / 86_400
}

fn history_path() -> PathBuf {
    crate::config::data_dir().join("history.json")
}

fn load_from_disk() -> Vec<HistoryEntry> {
    // Unit tests never read or write the real history.
    if cfg!(test) {
        return Vec::new();
    }
    let path = history_path();
    match std::fs::read_to_string(&path) {
        Ok(s) => match serde_json::from_str(&s) {
            Ok(entries) => entries,
            Err(e) => {
                tracing::error!("history.json failed to parse: {}", e);
                crate::config::quarantine_corrupt(&path);
                Vec::new()
            }
        },
        Err(_) => Vec::new(),
    }
}

/// history.json's saves (`saving.rs`): a failed one keeps the history in
/// memory, is tried again with the next save (the next dictation), and is
/// said once — by the command that hit it ([`saved`]) or, after a
/// dictation, the notice.
pub static SAVER: crate::shell::saving::Saver = crate::shell::saving::Saver::new("history", "your dictation history", save_now);

/// `Ok` when the whole history is on disk; else why not (the caller says it).
pub fn saved() -> Result<(), crate::shell::saving::SaveError> {
    SAVER.check()
}

/// Write the whole history again (Try again).
fn save_now() -> Result<(), crate::shell::saving::SaveError> {
    {
        let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
        let entries = guard.get_or_insert_with(load_from_disk);
        save_to_disk(entries);
    }
    saved()
}

fn save_to_disk(entries: &[HistoryEntry]) {
    if cfg!(test) {
        return;
    }
    let _ = SAVER.write_json(&history_path(), entries, false);
}

fn word_count(text: &str) -> usize {
    text.split_whitespace().count()
}

/// Record one dictation. Best-effort: errors are logged, never propagated, so a
/// dictation is never failed by history bookkeeping. Empty text is ignored.
pub fn record(raw: &str, text: &str, model: &str, app: &str) {
    record_with(raw, text, model, app, None, None, None);
}

/// [`record`] with the dictation's audio (`recent_audio.rs`) and, when its
/// paste couldn't land, why (`status` [`NOT_PASTED`], `error`).
pub fn record_with(
    raw: &str,
    text: &str,
    model: &str,
    app: &str,
    audio: Option<&str>,
    status: Option<&str>,
    error: Option<&str>,
) {
    record_dictation(Dictation { raw, text, said: None, model, app, audio, status, error });
}

/// One dictation for [`record_dictation`].
pub struct Dictation<'a> {
    pub raw: &'a str,
    pub text: &'a str,
    /// Its words as said, when AI cleanup changed them ([`HistoryEntry::said`]).
    pub said: Option<&'a str>,
    pub model: &'a str,
    pub app: &'a str,
    pub audio: Option<&'a str>,
    pub status: Option<&'a str>,
    pub error: Option<&'a str>,
}

/// Record a dictation; returns its timestamp (its id beside its text), or
/// `None` when there was nothing to record (empty text).
pub fn record_dictation(d: Dictation<'_>) -> Option<u64> {
    if d.text.trim().is_empty() {
        return None;
    }
    let ts = now_secs();
    push(HistoryEntry {
        ts,
        raw: d.raw.to_string(),
        text: d.text.to_string(),
        said: said_beside(d.said, d.text),
        model: d.model.to_string(),
        app: d.app.to_string(),
        audio: d.audio.map(str::to_string),
        status: d.status.map(str::to_string),
        error: d.error.map(str::to_string),
    });
    Some(ts)
}

/// The words as said worth keeping beside `text`: trimmed, and only when
/// they're something other than `text`.
fn said_beside(said: Option<&str>, text: &str) -> Option<String> {
    said.map(str::trim).filter(|s| !s.is_empty() && *s != text.trim()).map(str::to_string)
}

/// A dictation that never got transcribed ([`FAILED`]): listed with its
/// audio so it can be run again. `ts` (unix seconds) is when it was
/// recorded (`None`: now). Needs its audio: without it there's nothing to
/// list.
pub fn record_failed(ts: Option<u64>, model: &str, app: &str, audio: &str, error: &str) {
    push(HistoryEntry {
        ts: ts.unwrap_or_else(now_secs),
        raw: String::new(),
        text: String::new(),
        said: None,
        model: model.to_string(),
        app: app.to_string(),
        audio: Some(audio.to_string()),
        status: Some(FAILED.to_string()),
        error: Some(error.to_string()),
    });
}

fn push(entry: HistoryEntry) {
    let mut guard = match STATE.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    let entries = guard.get_or_insert_with(load_from_disk);
    // Kept in time order (a failed one from before a crash is dated when it
    // was recorded).
    let at = entries.iter().rposition(|e| e.ts <= entry.ts).map_or(0, |i| i + 1);
    entries.insert(at, entry);
    // Trim oldest first if over the cap.
    if entries.len() > MAX_ENTRIES {
        let drop = entries.len() - MAX_ENTRIES;
        entries.drain(0..drop);
    }
    save_to_disk(entries);
}

/// The entry at `ts` whose audio is `audio`, as Home lists it (for Run again).
pub fn find_audio(ts: u64, audio: &str) -> Option<HistoryEntry> {
    let mut guard = match STATE.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    let entries = guard.get_or_insert_with(load_from_disk);
    entries.iter().find(|e| e.ts == ts && e.audio.as_deref() == Some(audio)).cloned()
}

/// Run again: the entry with `audio` gets the new transcript (`raw`, `text`,
/// its words as said when cleanup changed them, `model`) and is no longer a
/// failure. `false` when it's gone.
pub fn rerun_result(audio: &str, raw: &str, text: &str, said: Option<&str>, model: &str) -> bool {
    let mut guard = match STATE.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    let entries = guard.get_or_insert_with(load_from_disk);
    let Some(e) = entries.iter_mut().find(|e| e.audio.as_deref() == Some(audio)) else {
        return false;
    };
    e.raw = raw.to_string();
    e.text = text.to_string();
    e.said = said_beside(said, text);
    e.model = model.to_string();
    e.status = None;
    e.error = None;
    save_to_disk(entries);
    true
}

/// Recent entries, newest first, capped at `limit`.
pub fn list(limit: usize) -> Value {
    let mut guard = match STATE.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    let entries = guard.get_or_insert_with(load_from_disk);
    let items: Vec<Value> = entries
        .iter()
        .rev()
        .take(limit)
        .map(|e| {
            let mut item = json!({
                "ts": e.ts,
                "raw": e.raw,
                "text": e.text,
                "model": e.model,
                "app": e.app,
                "words": word_count(&e.text),
            });
            // Its words as said, when AI cleanup changed them (Home's "AI
            // edited", "Paste what I said").
            if let Some(said) = &e.said {
                item["said"] = json!(said);
            }
            // Its audio, while it's kept (Run again, Play).
            if let Some(id) = e.audio.as_deref().filter(|id| crate::dictation::recent_audio::has(id)) {
                item["audio"] = json!(id);
            }
            if let Some(status) = &e.status {
                item["status"] = json!(status);
                item["error"] = json!(e.error.clone().unwrap_or_default());
            }
            item
        })
        .collect();
    json!(items)
}

/// The newest dictation's final text — not an Upload's (those record the
/// file's name where a dictation records the app's exe, or nothing).
/// "Paste last transcript" falls back to it, e.g. after a restart.
pub fn latest_dictation() -> Option<String> {
    let mut guard = match STATE.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    let entries = guard.get_or_insert_with(load_from_disk);
    entries
        .iter()
        .rev()
        .find(|e| is_dictation_app(&e.app) && !e.text.trim().is_empty())
        .map(|e| e.text.clone())
}

/// Every dictation's final text since `since` (unix secs), oldest first —
/// not an Upload's, nor one never transcribed ("Type that often? Save it as
/// a snippet", `dictionary/snippet_offer.rs`).
pub fn dictation_texts_since(since: u64) -> Vec<(u64, String)> {
    let mut guard = match STATE.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    let entries = guard.get_or_insert_with(load_from_disk);
    entries
        .iter()
        .filter(|e| e.ts >= since && !e.failed() && is_dictation_app(&e.app) && !e.text.trim().is_empty())
        .map(|e| (e.ts, e.text.clone()))
        .collect()
}

/// The newest dictation's transcript before cleanup ([`latest_dictation`]'s
/// `raw`): "Paste last transcript (as said)" after a restart.
pub fn latest_dictation_raw() -> Option<String> {
    let mut guard = match STATE.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    let entries = guard.get_or_insert_with(load_from_disk);
    entries
        .iter()
        .rev()
        .find(|e| is_dictation_app(&e.app) && !e.text.trim().is_empty())
        .map(|e| if e.raw.trim().is_empty() { e.text.clone() } else { e.raw.clone() })
}

/// How long each of the latest `limit` dictations was spoken for (seconds),
/// estimated from its words at [`SPEAKING_WPM`], newest first — uploads and
/// dictations that never transcribed aside (`stt_speed.rs`: how this person's
/// dictations weigh the speed check's lengths).
pub fn recent_dictation_secs(limit: usize) -> Vec<f32> {
    let mut guard = match STATE.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    let entries = guard.get_or_insert_with(load_from_disk);
    entries
        .iter()
        .rev()
        .filter(|e| is_dictation_app(&e.app) && e.status.as_deref() != Some(FAILED))
        .map(|e| if e.raw.trim().is_empty() { &e.text } else { &e.raw })
        .map(|t| t.split_whitespace().count() as f32 * 60.0 / SPEAKING_WPM as f32)
        .filter(|secs| *secs > 0.0)
        .take(limit)
        .collect()
}

fn is_dictation_app(app: &str) -> bool {
    app.trim().is_empty() || app.trim().to_ascii_lowercase().ends_with(".exe")
}

/// The entry Home lists at `ts` with `text` (the pair it deletes by, too).
pub fn find(ts: u64, text: &str) -> Option<HistoryEntry> {
    let mut guard = match STATE.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    let entries = guard.get_or_insert_with(load_from_disk);
    entries.iter().rev().find(|e| e.ts == ts && e.text == text).cloned()
}

/// One entry by timestamp (the local-API bridge's transcription id; first
/// match wins on the rare same-second collision).
pub fn get_by_ts(ts: u64) -> Option<HistoryEntry> {
    let mut guard = match STATE.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    let entries = guard.get_or_insert_with(load_from_disk);
    entries.iter().find(|e| e.ts == ts).cloned()
}

/// Delete one entry by timestamp alone (bridge delete route). Returns whether
/// anything was removed.
pub fn delete_by_ts(ts: u64) -> bool {
    crate::pasting::text_injector::forget_last_transcript();
    let mut guard = match STATE.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    let entries = guard.get_or_insert_with(load_from_disk);
    if let Some(pos) = entries.iter().position(|e| e.ts == ts) {
        let gone = entries.remove(pos);
        save_to_disk(entries);
        // Its audio goes with it.
        if let Some(id) = gone.audio {
            crate::dictation::recent_audio::discard(&id);
        }
        true
    } else {
        false
    }
}

/// Delete one entry, matched by timestamp + final text (the pair is unique
/// enough for a per-item delete from the Home feed; first match wins).
pub fn delete(ts: u64, text: &str) {
    crate::pasting::text_injector::forget_last_transcript();
    let mut guard = match STATE.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    let entries = guard.get_or_insert_with(load_from_disk);
    if let Some(pos) = entries.iter().position(|e| e.ts == ts && e.text == text) {
        let gone = entries.remove(pos);
        save_to_disk(entries);
        // Its audio goes with it.
        if let Some(id) = gone.audio {
            crate::dictation::recent_audio::discard(&id);
        }
    }
}

/// Delete all history (and the on-disk file), and the recent audio with it.
pub fn clear() {
    crate::pasting::text_injector::forget_last_transcript();
    let mut guard = match STATE.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    *guard = Some(Vec::new());
    if !cfg!(test) {
        match std::fs::remove_file(history_path()) {
            Ok(()) => SAVER.nothing_to_save(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => SAVER.nothing_to_save(),
            // Held by another program: an empty history over it, else the
            // failure is said ([`saved`]) and the next save tries again.
            Err(_) => save_to_disk(&[]),
        }
    }
    drop(guard);
    crate::dictation::recent_audio::clear();
}

/// Derived dashboard stats (camelCase JSON for the Settings panel).
pub fn stats() -> Value {
    let mut guard = match STATE.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    let all = guard.get_or_insert_with(load_from_disk);
    // A dictation that never got transcribed isn't one in the stats.
    let entries: Vec<&HistoryEntry> = all.iter().filter(|e| !e.failed()).collect();

    let today = day_of(now_secs());
    let total = entries.len() as u64;
    let mut total_words: u64 = 0;
    let mut today_words: u64 = 0;
    let mut today_count: u64 = 0;

    // Words per day-number, for the streak + activity series.
    use std::collections::HashMap;
    let mut by_day: HashMap<u64, u64> = HashMap::new();

    for e in entries.iter() {
        let w = word_count(&e.text) as u64;
        total_words += w;
        let d = day_of(e.ts);
        *by_day.entry(d).or_insert(0) += w;
        if d == today {
            today_words += w;
            today_count += 1;
        }
    }

    // Streak: consecutive days with activity, counting back from today (or
    // yesterday, so the streak survives a day you haven't dictated *yet*).
    let mut streak: u64 = 0;
    let mut cursor = if by_day.contains_key(&today) {
        today
    } else {
        today.saturating_sub(1)
    };
    while by_day.contains_key(&cursor) {
        streak += 1;
        if cursor == 0 {
            break;
        }
        cursor -= 1;
    }

    // Activity series: the last ACTIVITY_DAYS days, oldest→newest.
    let start = today.saturating_sub(ACTIVITY_DAYS - 1);
    let activity: Vec<Value> = (start..=today)
        .map(|d| json!({ "day": d, "words": by_day.get(&d).copied().unwrap_or(0) }))
        .collect();

    let time_saved_minutes =
        (total_words as f64) * (1.0 / TYPING_WPM - 1.0 / SPEAKING_WPM);

    json!({
        "totalTranscriptions": total,
        "totalWords": total_words,
        "timeSavedMinutes": time_saved_minutes.max(0.0),
        "today": { "transcriptions": today_count, "words": today_words },
        "streakDays": streak,
        "activity": activity,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_count_counts_whitespace_separated() {
        assert_eq!(word_count("hello there  friend"), 3);
        assert_eq!(word_count("   "), 0);
    }

    #[test]
    fn day_of_buckets_by_utc_day() {
        assert_eq!(day_of(0), 0);
        assert_eq!(day_of(86_399), 0);
        assert_eq!(day_of(86_400), 1);
    }

    /// (In unit tests the history lives in memory only.)
    #[test]
    fn a_failed_dictation_is_listed_in_its_place_and_run_again_fixes_it() {
        let now = now_secs();
        record_with("hello there", "Hello there.", "parakeet-tdt-0.6b-v3", "notepad.exe", Some("t-ok"), None, None);
        // From before a crash: dated when it was recorded, so before it.
        record_failed(Some(now - 600), "parakeet-tdt-0.6b-v3", "notepad.exe", "t-failed", "Yap closed before it was transcribed");
        let failed = find_audio(now - 600, "t-failed").expect("listed");
        assert_eq!(failed.status.as_deref(), Some(FAILED));
        assert!(failed.text.is_empty());
        {
            let guard = STATE.lock().unwrap();
            let entries = guard.as_ref().unwrap();
            let at = |id: &str| entries.iter().position(|e| e.audio.as_deref() == Some(id)).unwrap();
            assert!(at("t-failed") < at("t-ok"), "in time order");
        }
        // A failed one isn't the last dictation to paste.
        assert_ne!(latest_dictation().as_deref(), Some(""));
        assert!(rerun_result("t-failed", "um the plan is ready", "The plan is ready.", Some("um the plan is ready"), "large"));
        let fixed = find_audio(now - 600, "t-failed").unwrap();
        assert_eq!((fixed.status, fixed.text.as_str(), fixed.model.as_str()), (None, "The plan is ready.", "large"));
        assert_eq!(fixed.said.as_deref(), Some("um the plan is ready"));
        assert!(!rerun_result("t-gone", "x", "x", None, "large"));
    }

    #[test]
    fn the_words_as_said_are_kept_only_when_they_differ() {
        let ts = record_dictation(Dictation {
            raw: "um send it to sam",
            text: "Send it to Sam.",
            said: Some(" um send it to sam "),
            model: "parakeet-tdt-0.6b-v3",
            app: "slack.exe",
            audio: None,
            status: None,
            error: None,
        })
        .expect("recorded");
        let e = find(ts, "Send it to Sam.").expect("found by its time and text");
        assert_eq!(e.said.as_deref(), Some("um send it to sam"));
        // Listed for Home.
        let listed = list(50);
        let item = listed.as_array().unwrap().iter().find(|i| i["text"] == "Send it to Sam.").unwrap();
        assert_eq!(item["said"], "um send it to sam");
        // The same words: nothing to keep.
        assert_eq!(said_beside(Some("Same. "), "Same."), None);
        assert_eq!(said_beside(Some("  "), "Same."), None);
        // Nothing to record.
        assert!(record_dictation(Dictation { raw: "", text: " ", said: None, model: "", app: "", audio: None, status: None, error: None }).is_none());
    }
}
