//! How much each dictionary word gets used, so the Whisper vocabulary prompt
//! leads with the words you actually say (Wispr Flow ranks its vocabulary the
//! same way: recently used first, then most used).
//!
//! `<data>/dictionary_usage.json` maps a word (the entry's `to` spelling,
//! lowercased) to `{uses, lastUsed}` (unix seconds). It's its own store, never
//! config.json: Settings auto-saves its whole copy of the config, which would
//! put old counts back. Written atomically, a corrupt file quarantined
//! (`config::atomic_write` / `quarantine_corrupt`), like every Yap store.
//!
//! A word counts as used when it's in the corrected text of a dictation, a
//! meeting line or an Upload (`pipeline::apply_corrections` calls [`record`]):
//! that's whenever its exact or near-miss correction fired, and also when the
//! recognizer spelled it right by itself (with a Whisper model the prompt
//! usually sees to that, so the correction never fires). Best-effort
//! throughout: a failure is logged, never in a dictation's way.

use crate::config::DictionaryEntry;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

/// One word's record.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WordUse {
    pub uses: u32,
    /// Unix seconds.
    pub last_used: u64,
}

/// Word (lowercased `to`) → its record.
pub type Usage = BTreeMap<String, WordUse>;

const DAY: u64 = 24 * 60 * 60;

/// The store, loaded on first use. Only this process writes it (a dev build
/// beside the installed app: the last writer wins, which is fine for counts).
static STORE: Mutex<Option<Usage>> = Mutex::new(None);

fn path() -> PathBuf {
    crate::config::data_dir().join("dictionary_usage.json")
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn load(path: &std::path::Path) -> Usage {
    match std::fs::read_to_string(path) {
        Ok(s) => match serde_json::from_str(&s) {
            Ok(u) => u,
            Err(e) => {
                tracing::warn!("dictionary_usage.json unreadable: {}", e);
                crate::config::quarantine_corrupt(path);
                Usage::new()
            }
        },
        Err(_) => Usage::new(),
    }
}

fn with_store<R>(f: impl FnOnce(&mut Usage) -> R) -> Option<R> {
    let mut guard = STORE.lock().ok()?;
    let store = guard.get_or_insert_with(|| load(&path()));
    Some(f(store))
}

/// The key a dictionary entry is counted under.
fn key(entry: &DictionaryEntry) -> String {
    entry.to.trim().to_lowercase()
}

/// The dictionary words (keys) that appear in `text`, whole words, any case.
/// Snippets aren't vocabulary and never count.
pub fn used_in(dict: &[DictionaryEntry], text: &str) -> Vec<String> {
    let mut used: Vec<String> = Vec::new();
    for entry in dict.iter().filter(|e| !e.snippet) {
        let k = key(entry);
        if k.is_empty() || used.contains(&k) {
            continue;
        }
        if crate::config::term_regex(&entry.to).is_some_and(|re| re.is_match(text)) {
            used.push(k);
        }
    }
    used
}

/// Count one more use of each word in `words`, at `now`.
fn bump(usage: &mut Usage, words: &[String], now: u64) {
    for w in words {
        let e = usage.entry(w.clone()).or_default();
        e.uses = e.uses.saturating_add(1);
        e.last_used = e.last_used.max(now);
    }
}

/// Note the dictionary words used in a corrected `text` and save the store
/// when any were. Best-effort; never fails the caller. (Unit tests never
/// touch the real data dir: a no-op there.)
pub fn record(dict: &[DictionaryEntry], text: &str) {
    if cfg!(test) || dict.is_empty() {
        return;
    }
    let words = used_in(dict, text);
    if words.is_empty() {
        return;
    }
    with_store(|usage| {
        bump(usage, &words, now_secs());
        let _ = SAVER.write_json(&path(), usage, true);
    });
}

/// dictionary_usage.json's saves (`saving.rs`): a failed one keeps the
/// counts in memory, is tried again with the next save, and is said once.
pub static SAVER: crate::shell::saving::Saver = crate::shell::saving::Saver::new("dictionaryUsage", "your dictionary's word counts", save_now);

/// Write the counts again (Try again).
fn save_now() -> Result<(), crate::shell::saving::SaveError> {
    if let Ok(guard) = STORE.lock() {
        if let Some(usage) = guard.as_ref() {
            let _ = SAVER.write_json(&path(), usage, true);
        }
    }
    SAVER.check()
}

/// The dictionary in vocabulary order: words used in the last day first,
/// then the last 14 days, then the rest; within each, the most used first;
/// then the order you added them. Pure (see [`ranked`]).
pub fn rank(dict: &[DictionaryEntry], usage: &Usage, now: u64) -> Vec<DictionaryEntry> {
    let mut order: Vec<(usize, &DictionaryEntry)> = dict.iter().enumerate().collect();
    order.sort_by_key(|(i, e)| {
        let u = usage.get(&key(e)).copied().unwrap_or_default();
        let age = now.saturating_sub(u.last_used);
        let tier = match u.uses {
            0 => 2,
            _ if age <= DAY => 0,
            _ if age <= 14 * DAY => 1,
            _ => 2,
        };
        (tier, std::cmp::Reverse(u.uses), *i)
    });
    order.into_iter().map(|(_, e)| e.clone()).collect()
}

/// [`rank`] against the saved usage, now. What every Whisper prompt is built
/// from (`config::dictionary_prompt(&dict_usage::ranked(..))`).
pub fn ranked(dict: &[DictionaryEntry]) -> Vec<DictionaryEntry> {
    if dict.len() < 2 {
        return dict.to_vec();
    }
    let usage = with_store(|u| u.clone()).unwrap_or_default();
    rank(dict, &usage, now_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(from: &str, to: &str) -> DictionaryEntry {
        DictionaryEntry { from: from.into(), to: to.into(), fuzzy: true, snippet: false }
    }

    fn tos(dict: &[DictionaryEntry]) -> Vec<&str> {
        dict.iter().map(|e| e.to.as_str()).collect()
    }

    const NOW: u64 = 1_800_000_000;

    #[test]
    fn recent_use_beats_count_which_beats_the_order_added() {
        let dict = vec![
            entry("", "Never"),
            entry("", "OldButBusy"),
            entry("", "LastWeek"),
            entry("", "Yesterday"),
            entry("", "AnHourAgo"),
            entry("", "AlsoNever"),
            entry("", "LastWeekBusy"),
        ];
        let mut usage = Usage::new();
        let mut set = |w: &str, uses, ago| {
            usage.insert(w.to_lowercase(), WordUse { uses, last_used: NOW - ago });
        };
        set("OldButBusy", 90, 60 * DAY);
        set("LastWeek", 2, 7 * DAY);
        set("LastWeekBusy", 5, 7 * DAY);
        set("Yesterday", 1, DAY - 1);
        set("AnHourAgo", 3, 3600);
        assert_eq!(
            tos(&rank(&dict, &usage, NOW)),
            ["AnHourAgo", "Yesterday", "LastWeekBusy", "LastWeek", "OldButBusy", "Never", "AlsoNever"]
        );
        // Nothing used: the order you added them, untouched.
        assert_eq!(tos(&rank(&dict, &Usage::new(), NOW)), tos(&dict));
    }

    #[test]
    fn ranking_ignores_case_and_a_clock_that_went_back() {
        let dict = vec![entry("", "first"), entry("", "Kubernetes")];
        let mut usage = Usage::new();
        // Saved "in the future" (the clock was changed): still recent.
        usage.insert("kubernetes".into(), WordUse { uses: 1, last_used: NOW + 500 });
        assert_eq!(tos(&rank(&dict, &usage, NOW)), ["Kubernetes", "first"]);
    }

    #[test]
    fn the_ranked_prompt_keeps_the_words_you_use_inside_the_cap() {
        let mut dict: Vec<DictionaryEntry> =
            (0..120).map(|i| entry("", &format!("Filler{i:03}"))).collect();
        dict.push(entry("cuber netties", "Kubernetes"));
        // Added last, so unranked it falls off the 600-char prompt…
        assert!(!crate::config::dictionary_prompt(&dict).unwrap().contains("Kubernetes"));
        // …but once used it leads.
        let mut usage = Usage::new();
        bump(&mut usage, &used_in(&dict, "deploy to Kubernetes"), NOW);
        let prompt = crate::config::dictionary_prompt(&rank(&dict, &usage, NOW)).unwrap();
        assert!(prompt.starts_with("Kubernetes, Filler000"), "{prompt}");
    }

    #[test]
    fn a_use_is_the_word_in_the_corrected_text() {
        let dict = vec![
            entry("jaison", "JSON"),
            entry("ai", "AI"),
            entry("x", "json"), // same word twice: counted once
            DictionaryEntry { snippet: true, ..entry("brb", "Be right back") },
        ];
        assert_eq!(used_in(&dict, "the JSON is from the AI"), ["json", "ai"]);
        // Whole words only, and a snippet's text isn't vocabulary.
        assert!(used_in(&dict, "rain on jsonl, be right back").is_empty());
    }

    #[test]
    fn bumping_counts_and_dates_each_use() {
        let mut usage = Usage::new();
        bump(&mut usage, &["json".into()], NOW);
        bump(&mut usage, &["json".into(), "ai".into()], NOW + 10);
        assert_eq!(usage["json"], WordUse { uses: 2, last_used: NOW + 10 });
        assert_eq!(usage["ai"], WordUse { uses: 1, last_used: NOW + 10 });
        // The file's shape.
        let json = serde_json::to_value(&usage).unwrap();
        assert_eq!(json["json"]["lastUsed"], NOW + 10);
    }

    #[test]
    fn a_corrupt_store_is_set_aside_and_starts_empty() {
        let dir = std::env::temp_dir().join(format!("yap-dict-usage-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("dictionary_usage.json");
        std::fs::write(&p, "{ not json").unwrap();
        assert!(load(&p).is_empty());
        assert!(!p.exists(), "quarantined, not overwritten");
        let kept = std::fs::read_dir(&dir).unwrap().count();
        assert_eq!(kept, 1);
        assert!(load(&dir.join("missing.json")).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
