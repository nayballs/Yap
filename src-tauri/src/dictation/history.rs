//! Local transcription history + derived stats.
//!
//! Every successful dictation is appended (best-effort) to `history.json` in the
//! data dir: timestamp, raw transcript, final injected text, model, and the
//! foreground app. It's **local-only** — nothing leaves the machine — and can be
//! disabled (`history_enabled`) or cleared from Settings.
//!
//! The stats (Home, Insights) are counted apart, in `stats.rs`, so they
//! outlast the history; how long the history is kept is `history/keep.rs`.

pub mod keep;

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Cap the on-disk history so the file can't grow without bound.
const MAX_ENTRIES: usize = 2_000;
/// Speaking speed (words a minute) assumed for a dictation not timed.
const SPEAKING_WPM: f64 = crate::dictation::stats::SPEAKING_WPM;

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
    /// How long it was spoken for: its audio's length in seconds (since
    /// 2026-10-29; words a minute, `stats.rs`). An older Yap drops it when
    /// it saves history: the stats have it already.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secs: Option<f32>,
}

/// A dictation that never got transcribed.
pub const FAILED: &str = "failed";
/// A dictation transcribed whose paste couldn't land.
pub const NOT_PASTED: &str = "notPasted";
/// A dictation the person cancelled (the bar's ✕, Esc, the tray), kept
/// with its audio a day so it can still be transcribed (Home's
/// **Transcribe**). Not one in the stats, nor the last dictation.
pub const CANCELLED: &str = "cancelled";

impl HistoryEntry {
    /// Never transcribed: failed, or cancelled.
    pub fn failed(&self) -> bool {
        matches!(self.status.as_deref(), Some(FAILED | CANCELLED))
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
    record_dictation(Dictation { raw, text, said: None, model, app, audio, status, error, secs: None });
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
    /// Its audio's length ([`HistoryEntry::secs`]).
    pub secs: Option<f32>,
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
        secs: d.secs.filter(|s| s.is_finite() && *s > 0.0).map(|s| (s * 10.0).round() / 10.0),
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
        secs: None,
    });
}

/// A dictation the person cancelled ([`CANCELLED`]), with its audio (it
/// needs it: there's no text), dated now.
pub fn record_cancelled(model: &str, app: &str, audio: &str) {
    push(HistoryEntry {
        ts: now_secs(),
        raw: String::new(),
        text: String::new(),
        said: None,
        model: model.to_string(),
        app: app.to_string(),
        audio: Some(audio.to_string()),
        status: Some(CANCELLED.to_string()),
        error: Some("You cancelled it".to_string()),
        secs: None,
    });
}

/// The entry with audio `audio` taken out again (a cancelled dictation the
/// person took back with Undo: it's transcribed as a new one). `false` when
/// none has it. Its audio is the caller's to drop.
pub fn forget_audio(audio: &str) -> bool {
    let mut guard = match STATE.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    let entries = guard.get_or_insert_with(load_from_disk);
    let Some(pos) = entries.iter().position(|e| e.audio.as_deref() == Some(audio)) else {
        return false;
    };
    entries.remove(pos);
    save_to_disk(entries);
    true
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

/// Every entry as kept, oldest first (Download your data, `privacy/export.rs`).
pub fn all() -> Vec<HistoryEntry> {
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    guard.get_or_insert_with(load_from_disk).clone()
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
            // Its audio, while it's kept (Run again, Play), and whether
            // it's kept past its day (Keep).
            if let Some(id) = e.audio.as_deref().filter(|id| crate::dictation::recent_audio::has(id)) {
                item["audio"] = json!(id);
                if crate::dictation::recent_audio::is_kept(id) {
                    item["keep"] = json!(true);
                }
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

/// How long each of the latest `limit` dictations was spoken for (seconds):
/// its audio's length, or (from before Yap kept it) estimated from its words
/// at [`SPEAKING_WPM`], newest first — uploads and dictations that never
/// transcribed aside (`stt_speed.rs`: how this person's dictations weigh the
/// speed check's lengths).
pub fn recent_dictation_secs(limit: usize) -> Vec<f32> {
    let mut guard = match STATE.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    let entries = guard.get_or_insert_with(load_from_disk);
    entries
        .iter()
        .rev()
        .filter(|e| is_dictation_app(&e.app) && !e.failed())
        .map(|e| {
            e.secs.unwrap_or_else(|| {
                let t = if e.raw.trim().is_empty() { &e.text } else { &e.raw };
                t.split_whitespace().count() as f32 * 60.0 / SPEAKING_WPM as f32
            })
        })
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

/// The dictations newer than `since` (unix seconds) the stats count
/// (`stats.rs`): transcribed, with text, not an Upload's. Oldest first.
pub fn counted_since(since: u64) -> Vec<HistoryEntry> {
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let entries = guard.get_or_insert_with(load_from_disk);
    entries
        .iter()
        .filter(|e| e.ts > since && !e.failed() && is_dictation_app(&e.app) && !e.text.trim().is_empty())
        .cloned()
        .collect()
}

/// How many dictations history keeps (transcribed ones), and how many
/// entries are from before `cutoff` (unix seconds) — failed and cancelled
/// ones too, as [`forget_before`] deletes them all: Settings → Privacy &
/// data ("24 hours" asks first about those).
pub fn counts(cutoff: u64) -> (usize, usize) {
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let entries = guard.get_or_insert_with(load_from_disk);
    entries
        .iter()
        .fold((0, 0), |(all, older), e| (all + usize::from(!e.failed()), older + usize::from(e.ts < cutoff)))
}

/// Every entry from before `cutoff` (unix seconds) deleted, with its audio
/// (kept or not): "Keep history: 24 hours" (`keep.rs`). Returns how many.
pub fn forget_before(cutoff: u64) -> usize {
    let gone: Vec<HistoryEntry> = {
        let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
        let entries = guard.get_or_insert_with(load_from_disk);
        let (old, kept): (Vec<HistoryEntry>, Vec<HistoryEntry>) = std::mem::take(entries).into_iter().partition(|e| e.ts < cutoff);
        *entries = kept;
        if !old.is_empty() {
            save_to_disk(entries);
        }
        old
    };
    for id in gone.iter().filter_map(|e| e.audio.as_deref()) {
        crate::dictation::recent_audio::discard(id);
    }
    gone.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_count_counts_whitespace_separated() {
        assert_eq!(word_count("hello there  friend"), 3);
        assert_eq!(word_count("   "), 0);
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
    fn a_cancelled_dictation_is_listed_until_transcribed_or_taken_back() {
        let before = now_secs();
        record_cancelled("parakeet-tdt-0.6b-v3", "notepad.exe", "t-cancelled");
        let e = (before..=now_secs()).find_map(|ts| find_audio(ts, "t-cancelled")).expect("listed");
        assert_eq!(e.status.as_deref(), Some(CANCELLED));
        assert!(e.failed(), "not in the stats");
        assert!(e.text.is_empty());
        // Transcribe (Run again's path): a dictation like any other.
        assert!(rerun_result("t-cancelled", "the draft", "The draft.", None, "large"));
        assert!(!find_audio(e.ts, "t-cancelled").unwrap().failed());
        // Undo took one back: it leaves the list.
        record_cancelled("parakeet-tdt-0.6b-v3", "notepad.exe", "t-undone");
        assert!(forget_audio("t-undone"));
        assert!(!forget_audio("t-undone"));
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
            secs: Some(2.04),
        })
        .expect("recorded");
        let e = find(ts, "Send it to Sam.").expect("found by its time and text");
        assert_eq!(e.said.as_deref(), Some("um send it to sam"));
        // How long it was said for, to a tenth of a second.
        assert_eq!(e.secs, Some(2.0));
        // Listed for Home.
        let listed = list(50);
        let item = listed.as_array().unwrap().iter().find(|i| i["text"] == "Send it to Sam.").unwrap();
        assert_eq!(item["said"], "um send it to sam");
        // The same words: nothing to keep.
        assert_eq!(said_beside(Some("Same. "), "Same."), None);
        assert_eq!(said_beside(Some("  "), "Same."), None);
        // Nothing to record.
        assert!(record_dictation(Dictation { raw: "", text: " ", said: None, model: "", app: "", audio: None, status: None, error: None, secs: None }).is_none());
    }
}
