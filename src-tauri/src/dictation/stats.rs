//! **Dictation stats that outlast history** (2026-10-29, after Wispr Flow's
//! Insights): one row per day the person dictated, with numbers only —
//! words, dictations, seconds spoken, words per app, the words AI cleanup
//! changed and the dictionary's fixes — in `<data>/dictation_stats.json`.
//! Never a word of what was said. Written for every dictation whether or
//! not history is kept (Settings → Privacy & data → Keep history), so
//! clearing history, keeping it 24 hours or not at all, keeps Home's and
//! Insights' numbers.
//!
//! - **Days are the PC's own** ([`days`]): its date and time zone, and its
//!   week from Windows' regional format (the first day of the week).
//! - **Words per minute** over speaking time: a dictation's audio length
//!   (`run_stt`, also on its history entry as `secs`), never wall-clock
//!   time. Dictations from before Yap timed them are left out of the speed;
//!   once [`MEASURED_DICTATIONS`] and [`MEASURED_SECS`] of speech are
//!   timed, "time saved" uses the measured speed instead of the assumed
//!   [`SPEAKING_WPM`] (always against typing at [`TYPING_WPM`]).
//! - **Streaks**: days in a row, and weeks in a row with at least one
//!   dictation (now, and the longest).
//! - **Fixes**: the words AI cleanup changed (the words as said against
//!   the text pasted, [`word_diff`] — `core/wordDiff.js`'s twin) and the
//!   dictionary's fixes (`dict_usage::fixed_in`); this month so far against
//!   the same days of last month, as a %.
//! - **What counts**: every dictation transcribed — pasted or not, into
//!   Chat or the quick note too — never an Upload (a file, not dictated),
//!   one never transcribed, a Command or the Voice Assistant. A dictation
//!   run again after it failed counts on the day it was said.
//! - **History already there** is counted the first time ([`load`]), and
//!   so are dictations an older Yap saved to history since (it shares the
//!   data folder and knows nothing of this file): `countedTo` marks how far
//!   history is counted. Milestones passed that way aren't said.
//! - **Also kept here**: the word milestone said last (`milestones.rs`),
//!   the welcome back card's return and answer, and a copy of "Keep
//!   history"'s choice (`history/keep.rs`: an older Yap's save of
//!   config.json drops it).
//!
//! Its saves go through `saving.rs` (a failed one kept in memory, tried
//! again, said once). Logs say counts, never text or app names.

pub mod days;
pub mod share;
pub mod word_diff;

pub use days::Date;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::config::DictionaryEntry;
use crate::dictation::history::HistoryEntry;

/// The file, in the data folder.
pub const FILE: &str = "dictation_stats.json";
/// After every dictation counted: Home and Insights re-pull.
pub const EVENT: &str = "yap-stats-changed";
/// Typing speed (words a minute) "time saved" compares against.
pub const TYPING_WPM: f64 = 40.0;
/// Speaking speed assumed until this person's is measured.
pub const SPEAKING_WPM: f64 = 150.0;
/// The speed is this person's own once this many dictations…
pub const MEASURED_DICTATIONS: u64 = 3;
/// …and this many seconds of speech are timed.
pub const MEASURED_SECS: f64 = 30.0;
/// Insights' activity: this many days.
const ACTIVITY_DAYS: i64 = 30;
/// Welcome back: after this many days without a dictation.
pub const WELCOME_AFTER_DAYS: i64 = 14;
/// A return (a dictation after such a gap) is welcomed on Home for this
/// many days after it.
const RETURN_SHOWN_DAYS: i64 = 2;
/// At most this many apps a day (the rest count, without their app).
const MAX_APPS: usize = 40;

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// One day's numbers.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Day {
    /// "2026-10-29", the PC's date.
    pub date: String,
    /// Words pasted (the text after cleanup).
    #[serde(default)]
    pub words: u64,
    #[serde(default)]
    pub dictations: u64,
    /// The dictations whose speaking time is known…
    #[serde(default)]
    pub timed: u64,
    /// …their words…
    #[serde(default)]
    pub timed_words: u64,
    /// …and their seconds of audio.
    #[serde(default)]
    pub speaking_secs: f64,
    /// Words by the app dictated into (its exe's name; "yap.exe": Chat, the
    /// quick note).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub apps: BTreeMap<String, u64>,
    /// Words AI cleanup changed.
    #[serde(default)]
    pub cleanup_words: u64,
    /// Dictionary words written the dictionary's way that were heard otherwise.
    #[serde(default)]
    pub dictionary_fixes: u64,
}

/// A dictation after [`WELCOME_AFTER_DAYS`] without one.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Return {
    /// The last day dictated before the gap.
    pub from: String,
    /// The day of the return.
    pub on: String,
}

/// `dictation_stats.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Store {
    /// A row per day dictated, oldest first.
    #[serde(default)]
    pub days: Vec<Day>,
    /// History entries up to here (unix seconds) are counted.
    #[serde(default)]
    pub counted_to: u64,
    /// The highest word milestone passed (said, or passed before Yap said them).
    #[serde(default)]
    pub milestone: u64,
    /// The last return after a gap (welcome back).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub returned: Option<Return>,
    /// The gap welcome back was answered for: its last day before it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub welcomed: Option<String>,
    /// "Keep history"'s choice, a copy of config.json's (`history/keep.rs`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub history_keep: String,
}

/// One dictation, as counted (no text).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tally {
    pub words: u64,
    /// Seconds of audio, when known.
    pub secs: Option<f64>,
    pub app: String,
    pub cleanup_words: u64,
    pub dictionary_fixes: u64,
}

/// A dictation just delivered, for [`dictated`].
pub struct Counted<'a> {
    /// The transcript as heard.
    pub raw: &'a str,
    /// The text pasted.
    pub text: &'a str,
    /// The words as said when AI cleanup changed them.
    pub said: Option<&'a str>,
    /// The app's exe name.
    pub app: &'a str,
    /// Its audio's length.
    pub secs: Option<f32>,
    pub dict: &'a [DictionaryEntry],
    /// When it was said (unix seconds), for one run again after it failed;
    /// `None`: now.
    pub at: Option<u64>,
}

impl Tally {
    /// The numbers of a dictation: its text, the words as said (when
    /// cleanup changed them), what was heard, and the dictionary.
    pub fn of(text: &str, said: Option<&str>, raw: &str, app: &str, secs: Option<f64>, dict: &[DictionaryEntry]) -> Tally {
        Tally {
            words: text.split_whitespace().count() as u64,
            secs: secs.filter(|s| s.is_finite() && *s > 0.0),
            app: app.trim().to_string(),
            cleanup_words: said.map(|s| word_diff::diff(s, text).words as u64).unwrap_or(0),
            dictionary_fixes: crate::dictionary::dict_usage::fixed_in(dict, if raw.trim().is_empty() { text } else { raw }, text),
        }
    }
}

/// Welcome back, for Home.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Welcome {
    /// The last day dictated before the gap.
    pub since: String,
    /// The gap, in days.
    pub days: i64,
    /// They've dictated again since (else they're only looking).
    pub back: bool,
}

/// A rounded number of seconds, minutes, words a minute.
fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

impl Store {
    /// The row for `date`, made if need be (rows stay in date order).
    fn day_mut(&mut self, date: Date) -> &mut Day {
        let key = days::key(date);
        let at = match self.days.binary_search_by(|d| d.date.as_str().cmp(key.as_str())) {
            Ok(i) => i,
            Err(i) => {
                self.days.insert(i, Day { date: key, ..Day::default() });
                i
            }
        };
        &mut self.days[at]
    }

    /// Count one dictation on `date`.
    pub fn add(&mut self, date: Date, t: &Tally) {
        if t.words == 0 {
            return;
        }
        let day = self.day_mut(date);
        day.words += t.words;
        day.dictations += 1;
        if let Some(secs) = t.secs {
            day.timed += 1;
            day.timed_words += t.words;
            day.speaking_secs = round1(day.speaking_secs + secs);
        }
        if !t.app.is_empty() && (day.apps.contains_key(&t.app) || day.apps.len() < MAX_APPS) {
            *day.apps.entry(t.app.clone()).or_default() += t.words;
        }
        day.cleanup_words += t.cleanup_words;
        day.dictionary_fixes += t.dictionary_fixes;
    }

    /// A dictation just said, today: counted, a return after a gap noted.
    /// Returns the word milestone it passed, if any.
    pub fn count_now(&mut self, today: Date, t: &Tally, now: u64) -> Option<u64> {
        if t.words == 0 {
            return None;
        }
        if let Some(last) = self.last_active() {
            if (today - last).num_days() >= WELCOME_AFTER_DAYS {
                self.returned = Some(Return { from: days::key(last), on: days::key(today) });
            }
        }
        self.add(today, t);
        self.counted_to = self.counted_to.max(now);
        let crossed = crate::dictation::milestones::crossed(self.milestone, self.total_words());
        if let Some(step) = crossed {
            self.milestone = step;
        }
        crossed
    }

    /// Milestone `step` couldn't be said after all (a meeting records): no
    /// longer marked said — back to `before`, what was said until then — so
    /// the next dictation passes it again and says it. Nothing when a newer
    /// one was marked meanwhile. Whether it changed.
    pub fn unsay(&mut self, step: u64, before: u64) -> bool {
        if self.milestone != step || before >= step {
            return false;
        }
        self.milestone = before;
        true
    }

    /// History entries this file hasn't counted (all of them the first
    /// time; then those an older Yap saved): counted by the day they were
    /// said, the milestones they pass marked passed without a word.
    /// Returns how many.
    pub fn catch_up(&mut self, entries: &[HistoryEntry], dict: &[DictionaryEntry]) -> usize {
        let mut added = 0;
        let counted_to = self.counted_to;
        for e in entries.iter().filter(|e| e.ts > counted_to) {
            let t = Tally::of(&e.text, e.said.as_deref(), &e.raw, &e.app, e.secs.map(f64::from), dict);
            if t.words > 0 {
                self.add(days::local_date(e.ts), &t);
                added += 1;
            }
        }
        if let Some(last) = entries.iter().map(|e| e.ts).max() {
            self.counted_to = self.counted_to.max(last);
        }
        self.milestone = self.milestone.max(crate::dictation::milestones::passed(self.total_words()));
        added
    }

    pub fn total_words(&self) -> u64 {
        self.days.iter().map(|d| d.words).sum()
    }

    /// The last day with a dictation.
    pub fn last_active(&self) -> Option<Date> {
        self.days.iter().rev().filter(|d| d.dictations > 0).find_map(|d| days::parse(&d.date))
    }

    /// Welcome back on `today`, if it's due: just back after a gap (for
    /// [`RETURN_SHOWN_DAYS`]), or away [`WELCOME_AFTER_DAYS`] and looking
    /// now — once per gap.
    pub fn welcome(&self, today: Date) -> Option<Welcome> {
        if let Some(r) = &self.returned {
            if let (Some(from), Some(on)) = (days::parse(&r.from), days::parse(&r.on)) {
                if (today - on).num_days() < RETURN_SHOWN_DAYS && self.welcomed.as_deref() != Some(r.from.as_str()) {
                    return Some(Welcome { since: r.from.clone(), days: (on - from).num_days(), back: true });
                }
            }
        }
        let last = self.last_active()?;
        let gap = (today - last).num_days();
        let since = days::key(last);
        (gap >= WELCOME_AFTER_DAYS && self.welcomed.as_deref() != Some(since.as_str())).then_some(Welcome { since, days: gap, back: false })
    }

    /// Welcome back answered: not again for this gap.
    pub fn dismiss_welcome(&mut self, today: Date) -> bool {
        match self.welcome(today) {
            Some(w) => {
                self.welcomed = Some(w.since);
                true
            }
            None => false,
        }
    }

    /// Words a minute over the speech timed, once there's enough of it.
    pub fn measured_wpm(&self) -> Option<f64> {
        let (timed, words, secs) = self
            .days
            .iter()
            .fold((0, 0, 0.0), |(t, w, s), d| (t + d.timed, w + d.timed_words, s + d.speaking_secs));
        (timed >= MEASURED_DICTATIONS && secs >= MEASURED_SECS && words > 0).then(|| words as f64 * 60.0 / secs)
    }

    /// Minutes saved against typing: every word at [`TYPING_WPM`] against
    /// the same words at this person's measured speed, or the assumed
    /// [`SPEAKING_WPM`] until it's measured.
    pub fn minutes_saved(&self) -> f64 {
        let rate = self.measured_wpm().unwrap_or(SPEAKING_WPM);
        (self.total_words() as f64 * (1.0 / TYPING_WPM - 1.0 / rate)).max(0.0)
    }

    /// Home's and Insights' numbers (camelCase JSON), on `today` with weeks
    /// starting on `first`.
    pub fn summary(&self, today: Date, first: chrono::Weekday) -> Value {
        let in_span = |d: &Day, (from, to): (Date, Date)| days::parse(&d.date).is_some_and(|x| x >= from && x <= to);
        let sum = |f: &dyn Fn(&Day) -> u64| self.days.iter().map(f).sum::<u64>();
        let active: BTreeSet<Date> = self.days.iter().filter(|d| d.dictations > 0).filter_map(|d| days::parse(&d.date)).collect();
        let today_row = self.days.iter().find(|d| d.date == days::key(today));
        let (week_streak, longest_week_streak) = days::week_streaks(&active, today, first);

        // The last 30 days, oldest first.
        let activity: Vec<Value> = (0..ACTIVITY_DAYS)
            .rev()
            .map(|back| {
                let date = days::key(today - chrono::Duration::days(back));
                let words = self.days.iter().find(|d| d.date == date).map_or(0, |d| d.words);
                json!({ "date": date, "words": words })
            })
            .collect();

        // Fixes: all of them, and this month so far against the same days of last month.
        let fixes = |d: &Day| d.cleanup_words + d.dictionary_fixes;
        let (this_span, last_span) = days::month_spans(today);
        let this_month: u64 = self.days.iter().filter(|d| in_span(d, this_span)).map(fixes).sum();
        let last_month: u64 = self.days.iter().filter(|d| in_span(d, last_span)).map(fixes).sum();
        let change_pct = (last_month > 0).then(|| ((this_month as f64 - last_month as f64) * 100.0 / last_month as f64).round());

        // Where you dictate: words per app, the most first.
        let mut apps: BTreeMap<&str, u64> = BTreeMap::new();
        for d in &self.days {
            for (app, words) in &d.apps {
                *apps.entry(app.as_str()).or_default() += words;
            }
        }
        let mut top: Vec<(&str, u64)> = apps.into_iter().collect();
        top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        let top_apps: Vec<Value> = top.into_iter().take(20).map(|(app, words)| json!({ "app": app, "words": words })).collect();

        let wpm = self.measured_wpm();
        let timed = sum(&|d: &Day| d.timed);
        let speaking_secs: f64 = self.days.iter().map(|d| d.speaking_secs).sum();
        let minutes_saved = self.minutes_saved();
        json!({
            "totalTranscriptions": sum(&|d: &Day| d.dictations),
            "totalWords": self.total_words(),
            "today": {
                "transcriptions": today_row.map_or(0, |d| d.dictations),
                "words": today_row.map_or(0, |d| d.words),
            },
            "since": self.days.first().map(|d| d.date.clone()),
            "streakDays": days::day_streak(&active, today),
            "weekStreak": week_streak,
            "longestWeekStreak": longest_week_streak,
            "activity": activity,
            "wpm": wpm.map(|w| w.round()),
            "timedDictations": timed,
            "speakingMinutes": round1(speaking_secs / 60.0),
            "timeSavedMinutes": minutes_saved,
            "timeSaved": {
                "minutes": minutes_saved,
                "measured": wpm.is_some(),
                "speakingWpm": wpm.unwrap_or(SPEAKING_WPM).round(),
                "typingWpm": TYPING_WPM,
            },
            "fixes": {
                "cleanupWords": sum(&|d: &Day| d.cleanup_words),
                "dictionaryFixes": sum(&|d: &Day| d.dictionary_fixes),
                "thisMonth": this_month,
                "lastMonth": last_month,
                "changePct": change_pct,
            },
            "topApps": top_apps,
            "nextMilestone": crate::dictation::milestones::next_after(self.milestone),
        })
    }
}

// ---- the store on disk ------------------------------------------------------------------

static STORE: Mutex<Option<Store>> = Mutex::new(None);

/// dictation_stats.json's saves (`saving.rs`): a failed one keeps the stats
/// in memory, is tried again with the next save, and is said once.
pub static SAVER: crate::shell::saving::Saver = crate::shell::saving::Saver::new("dictationStats", "your dictation stats", save_now);

fn path() -> PathBuf {
    crate::config::data_dir().join(FILE)
}

/// The store as saved, with history it hasn't counted yet counted (and
/// saved). Unit tests never touch the data folder.
fn load() -> Store {
    if cfg!(test) {
        return Store::default();
    }
    let path = path();
    let mut store = match std::fs::read_to_string(&path) {
        Ok(s) => match serde_json::from_str::<Store>(&s) {
            Ok(store) => store,
            Err(e) => {
                tracing::error!("{FILE} failed to parse: {e}");
                crate::config::quarantine_corrupt(&path);
                Store::default()
            }
        },
        Err(_) => Store::default(),
    };
    let first = !path.is_file();
    let entries = crate::dictation::history::counted_since(store.counted_to);
    let dict = crate::config::load().dictionary;
    let added = store.catch_up(&entries, &dict);
    if added > 0 || first {
        tracing::info!(added, days = store.days.len(), first, "Dictation stats: history counted");
        save(&store);
    }
    store
}

fn save(store: &Store) {
    if cfg!(test) {
        return;
    }
    let _ = SAVER.write_json(&path(), store, false);
}

/// Write the stats again (Try again).
fn save_now() -> Result<(), crate::shell::saving::SaveError> {
    if let Some(store) = STORE.lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
        save(store);
    }
    SAVER.check()
}

fn with_store<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    let mut guard = STORE.lock().unwrap_or_else(|p| p.into_inner());
    let store = guard.get_or_insert_with(load);
    f(store)
}

/// At start, before any dictation: the store loaded, history it hasn't
/// counted counted (so a dictation is never counted twice).
pub fn init() {
    with_store(|_| ());
}

/// A dictation delivered: counted (whether or not history keeps it), and
/// its milestone said if it passed one.
pub fn dictated(app: &AppHandle, c: Counted<'_>) {
    let t = Tally::of(c.text, c.said, c.raw, c.app, c.secs.map(f64::from), c.dict);
    if t.words == 0 {
        return;
    }
    let now = now_secs();
    let crossed = with_store(|s| {
        let before = s.milestone;
        let crossed = match c.at {
            None => s.count_now(days::today(), &t, now),
            // Run again on one that failed: on the day it was said.
            Some(at) => {
                s.add(days::local_date(at), &t);
                None
            }
        };
        save(s);
        crossed.map(|step| (step, before, s.minutes_saved()))
    });
    tracing::debug!(words = t.words, timed = t.secs.is_some(), "Dictation stats: counted");
    // Home and Insights re-pull (history off, nothing else says so).
    let _ = app.emit(EVENT, ());
    if let Some((step, before, minutes)) = crossed {
        crate::dictation::milestones::celebrate(app, step, before, minutes);
    }
}

/// Milestone `step` wasn't said after all (a meeting records): marked
/// unsaid again, back to `before`, so the next dictation says it
/// (`milestones::celebrate`).
pub fn milestone_unsaid(step: u64, before: u64) {
    with_store(|s| {
        if s.unsay(step, before) {
            save(s);
        }
    });
}

/// Home's and Insights' numbers.
pub fn summary() -> Value {
    with_store(|s| s.summary(days::today(), days::first_weekday()))
}

/// Welcome back, if it's due today.
pub fn welcome() -> Option<Welcome> {
    with_store(|s| s.welcome(days::today()))
}

/// "Keep history"'s choice as last copied here ("" if never).
pub fn keep_choice() -> String {
    with_store(|s| s.history_keep.clone())
}

/// Copy "Keep history"'s choice here (saved only when it changed).
pub fn set_keep_choice(keep: &str) {
    with_store(|s| {
        if s.history_keep != keep {
            s.history_keep = keep.to_string();
            save(s);
        }
    });
}

/// The store as it is, for Download your data (`privacy/export.rs`).
pub fn snapshot() -> Value {
    with_store(|s| serde_json::to_value(&*s).unwrap_or(Value::Null))
}

/// Forget every number (Reset app, `privacy/reset.rs`): history there now
/// counts as counted, so it isn't counted again; "Keep history"'s copy
/// stays (config.json keeps the choice).
pub fn clear() {
    let mut guard = STORE.lock().unwrap_or_else(|p| p.into_inner());
    let keep = guard.as_ref().map(|s| s.history_keep.clone()).unwrap_or_default();
    // Everything in history now counts as counted: a reset starts from nothing.
    let store = Store { counted_to: now_secs(), history_keep: keep, ..Store::default() };
    save(&store);
    *guard = Some(store);
    tracing::info!("Dictation stats: cleared");
}

// ---- commands -----------------------------------------------------------------------------

/// Home's welcome back card answered (✕, or its button): not again for this gap.
#[tauri::command]
pub fn welcome_back_dismiss() -> Result<(), String> {
    let changed = with_store(|s| {
        let changed = s.dismiss_welcome(days::today());
        if changed {
            save(s);
        }
        changed
    });
    tracing::info!(changed, "Welcome back: answered");
    Ok(SAVER.check()?)
}

/// Test mode only: the stats set to `store` (`dictation_stats.json`'s
/// shape; its `countedTo` now when left out, so history isn't counted
/// again), or cleared (`None`) — each test starts from the numbers it
/// needs. Home and Insights re-pull.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_stats_set(app: AppHandle, store: Option<Value>) -> Result<(), String> {
    if !crate::shell::e2e::active() {
        return Err("Only in e2e test mode".into());
    }
    match store {
        None => clear(),
        Some(v) => {
            let mut s: Store = serde_json::from_value(v).map_err(|e| e.to_string())?;
            if s.counted_to == 0 {
                s.counted_to = now_secs();
            }
            save(&s);
            *STORE.lock().unwrap_or_else(|p| p.into_inner()) = Some(s);
        }
    }
    let _ = app.emit(EVENT, ());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Weekday;

    fn d(s: &str) -> Date {
        days::parse(s).unwrap()
    }
    fn tally(words: u64, secs: Option<f64>, app: &str) -> Tally {
        Tally { words, secs, app: app.into(), cleanup_words: 0, dictionary_fixes: 0 }
    }
    fn entry(ts: u64, text: &str, app: &str) -> HistoryEntry {
        HistoryEntry {
            ts,
            raw: text.to_lowercase(),
            text: text.into(),
            said: None,
            model: String::new(),
            app: app.into(),
            audio: None,
            status: None,
            error: None,
            secs: None,
        }
    }

    #[test]
    fn a_day_keeps_numbers_only_in_date_order() {
        let mut s = Store::default();
        s.add(d("2026-10-29"), &tally(10, Some(4.0), "slack.exe"));
        s.add(d("2026-10-27"), &tally(5, None, "code.exe"));
        s.add(d("2026-10-29"), &Tally { cleanup_words: 2, dictionary_fixes: 1, ..tally(6, Some(2.04), "slack.exe") });
        s.add(d("2026-10-29"), &tally(0, Some(9.0), "x.exe")); // nothing said: nothing counted
        assert_eq!(s.days.iter().map(|d| d.date.as_str()).collect::<Vec<_>>(), ["2026-10-27", "2026-10-29"]);
        let day = &s.days[1];
        assert_eq!((day.words, day.dictations, day.timed, day.timed_words), (16, 2, 2, 16));
        assert!((day.speaking_secs - 6.0).abs() < 1e-9);
        assert_eq!(day.apps.get("slack.exe"), Some(&16));
        assert_eq!((day.cleanup_words, day.dictionary_fixes), (2, 1));
        // On disk: numbers and app names, never a word said.
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"speakingSecs\":6.0") && json.contains("\"slack.exe\":16"), "{json}");
    }

    #[test]
    fn words_a_minute_come_from_speaking_time_once_there_is_enough() {
        let mut s = Store::default();
        // Two timed dictations: not enough yet; the assumed speed counts.
        s.add(d("2026-10-29"), &tally(30, Some(15.0), "a.exe"));
        s.add(d("2026-10-29"), &tally(30, Some(15.0), "a.exe"));
        assert_eq!(s.measured_wpm(), None);
        let assumed = 60.0 * (1.0 / TYPING_WPM - 1.0 / SPEAKING_WPM);
        assert!((s.minutes_saved() - assumed).abs() < 1e-9);
        // An old, untimed one is left out of the speed.
        s.add(d("2026-10-28"), &tally(500, None, "a.exe"));
        s.add(d("2026-10-29"), &tally(30, Some(15.0), "a.exe"));
        assert_eq!(s.measured_wpm(), Some(120.0));
        // Time saved: every word, at the measured speed.
        let saved = 590.0 * (1.0 / TYPING_WPM - 1.0 / 120.0);
        assert!((s.minutes_saved() - saved).abs() < 1e-9);
        let sum = s.summary(d("2026-10-29"), Weekday::Mon);
        assert_eq!(sum["wpm"], 120.0);
        assert_eq!(sum["timeSaved"]["measured"], true);
        assert_eq!(sum["timeSaved"]["speakingWpm"], 120.0);
        assert_eq!(sum["timedDictations"], 3);
        // Slower than typing: nothing saved, never less.
        let mut slow = Store::default();
        for _ in 0..3 {
            slow.add(d("2026-10-29"), &tally(5, Some(20.0), "a.exe"));
        }
        assert_eq!(slow.minutes_saved(), 0.0);
    }

    #[test]
    fn the_summary_has_streaks_activity_fixes_and_apps() {
        let mut s = Store::default();
        for (date, words) in [("2026-09-02", 50), ("2026-09-20", 10), ("2026-10-12", 40), ("2026-10-27", 20), ("2026-10-28", 30)] {
            s.add(d(date), &Tally { cleanup_words: words / 10, dictionary_fixes: 1, ..tally(words, None, "slack.exe") });
        }
        s.add(d("2026-10-28"), &tally(5, None, "code.exe"));
        let sum = s.summary(d("2026-10-29"), Weekday::Mon);
        assert_eq!(sum["totalWords"], 155);
        assert_eq!(sum["totalTranscriptions"], 6);
        assert_eq!(sum["today"]["words"], 0);
        assert_eq!(sum["since"], "2026-09-02");
        assert_eq!(sum["streakDays"], 2, "yesterday and the day before");
        // Weeks of 12, 19 (none), 26 Oct: now 1; Sept's two weeks apart.
        assert_eq!((sum["weekStreak"].as_u64(), sum["longestWeekStreak"].as_u64()), (Some(1), Some(1)));
        let activity = sum["activity"].as_array().unwrap();
        assert_eq!(activity.len(), 30);
        assert_eq!(activity[29], json!({ "date": "2026-10-29", "words": 0 }));
        assert_eq!(activity[28], json!({ "date": "2026-10-28", "words": 35 }));
        // Fixes: 1–29 Oct (4+2+3 + 3 dictionary) against 1–29 Sept (5+1 + 2).
        assert_eq!(sum["fixes"]["thisMonth"], 12);
        assert_eq!(sum["fixes"]["lastMonth"], 8);
        assert_eq!(sum["fixes"]["changePct"], 50.0);
        assert_eq!(sum["fixes"]["cleanupWords"], 15);
        assert_eq!(sum["fixes"]["dictionaryFixes"], 5);
        assert_eq!(sum["topApps"], json!([{ "app": "slack.exe", "words": 150 }, { "app": "code.exe", "words": 5 }]));
        assert_eq!(sum["wpm"], Value::Null);
        assert_eq!(sum["nextMilestone"], 100);
        // Nothing last month: no %.
        let mut fresh = Store::default();
        fresh.add(d("2026-10-02"), &Tally { cleanup_words: 3, ..tally(9, None, "") });
        assert_eq!(fresh.summary(d("2026-10-29"), Weekday::Mon)["fixes"]["changePct"], Value::Null);
    }

    #[test]
    fn history_already_there_is_counted_once_and_its_milestones_kept_quiet() {
        let mut s = Store::default();
        let ts = 1_790_000_000; // 2026-09-21
        let mut e1 = entry(ts, "One two three", "notepad.exe");
        e1.secs = Some(1.5);
        e1.said = Some("um one two three".into());
        let mut failed = entry(ts + 20, "", "notepad.exe");
        failed.status = Some("failed".into());
        let big = entry(ts + 30, &vec!["word"; 120].join(" "), "slack.exe");
        // Uploads and failures never reach here (`history::counted_since`), but
        // an empty text is never counted either.
        assert_eq!(s.catch_up(&[e1.clone(), failed, big], &[]), 2);
        assert_eq!(s.total_words(), 123);
        assert_eq!(s.counted_to, ts + 30);
        assert_eq!(s.milestone, 100, "passed before it could be said");
        let day = &s.days[0];
        assert_eq!((day.timed, day.speaking_secs, day.cleanup_words), (1, 1.5, 1));
        // Again: nothing new. An older Yap's newer entry: counted.
        assert_eq!(s.catch_up(&[e1.clone()], &[]), 0);
        assert_eq!(s.catch_up(&[e1, entry(ts + 90, "Four five", "code.exe")], &[]), 1);
        assert_eq!(s.total_words(), 125);
    }

    #[test]
    fn a_dictation_now_passes_a_milestone_once() {
        let mut s = Store::default();
        let today = d("2026-10-29");
        assert_eq!(s.count_now(today, &tally(60, None, "a.exe"), 10), None);
        assert_eq!(s.count_now(today, &tally(50, None, "a.exe"), 20), Some(100));
        assert_eq!(s.counted_to, 20);
        assert_eq!(s.count_now(today, &tally(50, None, "a.exe"), 30), None);
        assert_eq!(s.count_now(today, &tally(900, None, "a.exe"), 40), Some(1_000), "past 500 and 1,000: the higher");
        assert_eq!(s.milestone, 1_000);
        assert_eq!(s.count_now(today, &tally(0, None, "a.exe"), 50), None);
    }

    /// A milestone passed during a meeting was marked said as it was
    /// counted, then waited unseen on the bar and was dropped (or showed
    /// over the meeting): never said. Unsaid, the next dictation says it.
    #[test]
    fn a_milestone_that_could_not_be_said_is_said_by_the_next_dictation() {
        let mut s = Store::default();
        let today = d("2026-10-29");
        assert_eq!(s.count_now(today, &tally(60, None, "a.exe"), 10), None);
        assert_eq!(s.count_now(today, &tally(50, None, "a.exe"), 20), Some(100));
        // A meeting records: not said.
        assert!(s.unsay(100, 0));
        assert_eq!(s.milestone, 0);
        assert_eq!(s.count_now(today, &tally(5, None, "a.exe"), 30), Some(100), "the next dictation passes it again");
        // A newer one marked meanwhile stays.
        assert_eq!(s.count_now(today, &tally(900, None, "a.exe"), 40), Some(1_000));
        assert!(!s.unsay(100, 0));
        assert_eq!(s.milestone, 1_000);
    }

    #[test]
    fn welcome_back_once_per_gap() {
        let mut s = Store::default();
        assert_eq!(s.welcome(d("2026-10-29")), None, "never dictated: onboarding's job");
        s.add(d("2026-10-01"), &tally(10, None, "a.exe"));
        assert_eq!(s.welcome(d("2026-10-14")), None, "13 days");
        let away = s.welcome(d("2026-10-15")).expect("14 days away");
        assert_eq!(away, Welcome { since: "2026-10-01".into(), days: 14, back: false });
        // Answered: not again for this gap, even after they're back.
        assert!(s.dismiss_welcome(d("2026-10-16")));
        assert_eq!(s.welcome(d("2026-10-16")), None);
        s.count_now(d("2026-10-20"), &tally(5, None, "a.exe"), 1);
        assert_eq!(s.welcome(d("2026-10-20")), None);

        // Back before they looked: welcomed for two days after the return.
        s.count_now(d("2026-11-10"), &tally(5, None, "a.exe"), 2);
        let back = s.welcome(d("2026-11-11")).expect("just back");
        assert_eq!(back, Welcome { since: "2026-10-20".into(), days: 21, back: true });
        assert_eq!(s.welcome(d("2026-11-12")), None, "the return is old news");
        assert!(!s.dismiss_welcome(d("2026-11-12")));
    }

    #[test]
    fn the_file_reads_back_and_reads_what_a_later_yap_may_leave_out() {
        let mut s = Store { milestone: 500, history_keep: "day".into(), counted_to: 9, ..Store::default() };
        s.add(d("2026-10-29"), &tally(3, Some(1.2), "a.exe"));
        let back: Store = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
        let bare: Store = serde_json::from_str(r#"{"days":[{"date":"2026-10-29","words":4}]}"#).unwrap();
        assert_eq!((bare.total_words(), bare.days[0].dictations, bare.milestone), (4, 0, 0));
    }
}
