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
    /// Active model id at the time.
    #[serde(default)]
    pub model: String,
    /// Foreground app (process base name, e.g. "chrome.exe"), best-effort.
    #[serde(default)]
    pub app: String,
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
    match std::fs::read_to_string(history_path()) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

fn save_to_disk(entries: &[HistoryEntry]) {
    match serde_json::to_string(entries) {
        Ok(json) => {
            let _ = std::fs::create_dir_all(crate::config::data_dir());
            if let Err(e) = std::fs::write(history_path(), json) {
                tracing::warn!("Failed to persist history: {}", e);
            }
        }
        Err(e) => tracing::warn!("Failed to serialize history: {}", e),
    }
}

fn word_count(text: &str) -> usize {
    text.split_whitespace().count()
}

/// Record one dictation. Best-effort: errors are logged, never propagated, so a
/// dictation is never failed by history bookkeeping. Empty text is ignored.
pub fn record(raw: &str, text: &str, model: &str, app: &str) {
    if text.trim().is_empty() {
        return;
    }
    let mut guard = match STATE.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    let entries = guard.get_or_insert_with(load_from_disk);
    entries.push(HistoryEntry {
        ts: now_secs(),
        raw: raw.to_string(),
        text: text.to_string(),
        model: model.to_string(),
        app: app.to_string(),
    });
    // Trim oldest first if over the cap.
    if entries.len() > MAX_ENTRIES {
        let drop = entries.len() - MAX_ENTRIES;
        entries.drain(0..drop);
    }
    save_to_disk(entries);
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
            json!({
                "ts": e.ts,
                "raw": e.raw,
                "text": e.text,
                "model": e.model,
                "app": e.app,
                "words": word_count(&e.text),
            })
        })
        .collect();
    json!(items)
}

/// Delete all history (and the on-disk file).
pub fn clear() {
    let mut guard = match STATE.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    *guard = Some(Vec::new());
    let _ = std::fs::remove_file(history_path());
}

/// Derived dashboard stats (camelCase JSON for the Settings panel).
pub fn stats() -> Value {
    let mut guard = match STATE.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    let entries = guard.get_or_insert_with(load_from_disk);

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
}
