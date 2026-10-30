//! **`ai_usage.json`**: every AI call's tokens, per day, per model and per
//! feature, kept 90 days (the AI usage tracker, `usage.rs`). Counts only:
//! the provider and model ids, whether the model ran on this PC, the
//! feature, the calls, the prompt and answer tokens, an estimated cost in
//! millionths of a dollar (`prices.rs`) and the on-device model's reading
//! and writing time — never what was asked or answered.
//!
//! **Its own file** (not `groq_usage.json`, which an older Yap still reads
//! and writes as it always did): an older Yap sharing the data folder never
//! touches it, and its calls aren't counted here.
//!
//! **Written by a thread** every few seconds when something was added
//! (and at exit, `lib.rs shutdown_cleanup` → [`flush`]), through
//! `saving.rs` (a failed write keeps the counts, tries again, says so
//! once). Yap - Dev and the installed Yap share the data folder: what this
//! Yap added since its last write is kept apart ([`State::pending`]) and
//! added to the file as it is on disk when another Yap wrote it meanwhile,
//! so neither's counts are lost. Until `init` (unit tests, `yap.exe mcp`,
//! `eval`), nothing is written.
//!
//! The day is the PC's own (local midnight), so "today" is the person's.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime};

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use super::{Feature, Reported};

pub const FILE: &str = "ai_usage.json";
/// Days kept (today included).
pub const KEEP_DAYS: i64 = 90;
/// Emitted after each write: the pages showing usage read it again.
pub const EVENT: &str = "yap-ai-usage";
const WRITE_EVERY: Duration = Duration::from_secs(3);

fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// One day's counts for one model and feature.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub provider: String,
    pub model: String,
    /// It ran on this PC (the on-device model, a server on localhost).
    #[serde(default)]
    pub here: bool,
    pub feature: String,
    #[serde(default)]
    pub calls: u64,
    /// Calls whose answer didn't say its tokens (in `calls`, not in the
    /// token counts).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub unreported: u64,
    #[serde(default)]
    pub prompt: u64,
    #[serde(default)]
    pub answer: u64,
    /// Estimated cost at list prices, millionths of a US dollar (a cloud
    /// model with a known price).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub cost_micros: u64,
    /// The prompt tokens a llama.cpp server read (not from its cache) and
    /// how long that took, and the tokens it wrote and how long: the
    /// on-device model's speed.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub read_tokens: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub read_ms: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub gen_tokens: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub gen_ms: u64,
}

impl Row {
    /// One call's row: `day` prices it.
    pub fn of_call(provider: &str, model: &str, here: bool, feature: Feature, r: &Reported, day: &str) -> Row {
        let (prompt, answer) = match (r.prompt, r.answer) {
            (None, None) => (r.total.unwrap_or(0), 0),
            (p, a) => (p.unwrap_or(0), a.unwrap_or(0)),
        };
        let cost = if here { None } else { super::prices::cost_micros(provider, model, day, prompt, answer) };
        Row {
            provider: provider.to_string(),
            model: model.to_string(),
            here,
            feature: feature.id().to_string(),
            calls: 1,
            unreported: u64::from(!r.counted()),
            prompt,
            answer,
            cost_micros: cost.unwrap_or(0),
            read_tokens: r.speed.read_tokens,
            read_ms: r.speed.read_ms,
            gen_tokens: r.speed.gen_tokens,
            gen_ms: r.speed.gen_ms,
        }
    }

    fn same_as(&self, o: &Row) -> bool {
        self.provider == o.provider && self.model == o.model && self.here == o.here && self.feature == o.feature
    }

    fn add(&mut self, o: &Row) {
        self.calls = self.calls.saturating_add(o.calls);
        self.unreported = self.unreported.saturating_add(o.unreported);
        self.prompt = self.prompt.saturating_add(o.prompt);
        self.answer = self.answer.saturating_add(o.answer);
        self.cost_micros = self.cost_micros.saturating_add(o.cost_micros);
        self.read_tokens = self.read_tokens.saturating_add(o.read_tokens);
        self.read_ms = self.read_ms.saturating_add(o.read_ms);
        self.gen_tokens = self.gen_tokens.saturating_add(o.gen_tokens);
        self.gen_ms = self.gen_ms.saturating_add(o.gen_ms);
    }

    pub fn tokens(&self) -> u64 {
        self.prompt.saturating_add(self.answer)
    }
}

fn one() -> u32 {
    1
}

/// The file: days ("2026-10-25") → their rows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Store {
    #[serde(default = "one")]
    pub version: u32,
    #[serde(default)]
    pub days: BTreeMap<String, Vec<Row>>,
}

impl Default for Store {
    fn default() -> Self {
        Store { version: 1, days: BTreeMap::new() }
    }
}

impl Store {
    /// Add `row` to `day` (into the row of the same model and feature).
    pub fn add(&mut self, day: &str, row: &Row) {
        let rows = self.days.entry(day.to_string()).or_default();
        match rows.iter_mut().find(|r| r.same_as(row)) {
            Some(r) => r.add(row),
            None => rows.push(row.clone()),
        }
    }

    /// Add everything in `other`.
    pub fn merge(&mut self, other: &Store) {
        for (day, rows) in &other.days {
            for row in rows {
                self.add(day, row);
            }
        }
    }

    /// Keep the last [`KEEP_DAYS`] days up to `today` (a day not written as
    /// a date goes too).
    pub fn trim(&mut self, today: NaiveDate) {
        let oldest = today - chrono::Duration::days(KEEP_DAYS - 1);
        self.days.retain(|day, _| NaiveDate::parse_from_str(day, "%Y-%m-%d").is_ok_and(|d| d >= oldest));
    }

    pub fn is_empty(&self) -> bool {
        self.days.values().all(|rows| rows.is_empty())
    }
}

/// A day's key.
pub fn day_key(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

fn today() -> NaiveDate {
    chrono::Local::now().date_naive()
}

// ---- the store in memory, and its file ------------------------------------------------------

struct State {
    /// The file as last read or written by this Yap.
    base: Store,
    /// Added since this Yap last wrote.
    pending: Store,
    /// The file's modified time when this Yap last read or wrote it (another
    /// Yap's write changes it).
    seen: Option<SystemTime>,
    loaded: bool,
}

static STATE: Mutex<State> =
    Mutex::new(State { base: Store { version: 1, days: BTreeMap::new() }, pending: Store { version: 1, days: BTreeMap::new() }, seen: None, loaded: false });
static FILE_PATH: OnceLock<PathBuf> = OnceLock::new();
static APP: OnceLock<AppHandle> = OnceLock::new();
/// One write at a time (the thread's and the exit's).
static WRITING: Mutex<()> = Mutex::new(());

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// The file (`saving::Saver::load`): set aside if it isn't the counts; one
/// Yap couldn't read is never written over until it reads
/// ([`write_pending`] reads it again, the counts kept pending meanwhile).
fn read(path: &Path) -> Store {
    SAVER.load(path)
}

fn lock() -> std::sync::MutexGuard<'static, State> {
    let mut s = STATE.lock().unwrap_or_else(|p| p.into_inner());
    if let (false, Some(path)) = (s.loaded, FILE_PATH.get()) {
        s.loaded = true;
        s.seen = modified(path);
        s.base = read(path);
    }
    s
}

/// One call's counts in (written within [`WRITE_EVERY`]).
pub fn record(provider: &str, model: &str, here: bool, feature: Feature, r: &Reported) {
    let day = day_key(today());
    let row = Row::of_call(provider, model, here, feature, r, &day);
    lock().pending.add(&day, &row);
}

/// The file from now on, and the thread that writes it. Called once from
/// `lib.rs` setup.
pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
    if FILE_PATH.set(crate::config::data_dir().join(FILE)).is_err() {
        return;
    }
    drop(lock()); // read it now, off the AI call's path
    let _ = std::thread::Builder::new().name("yap-ai-usage".into()).spawn(|| loop {
        std::thread::sleep(WRITE_EVERY);
        flush();
    });
}

/// ai_usage.json's saves (`saving.rs`): a failed one keeps the counts in
/// memory and is tried again with the next.
pub static SAVER: crate::shell::saving::Saver = crate::shell::saving::Saver::new("aiUsage", "your AI usage history", save_now);

fn save_now() -> Result<(), crate::shell::saving::SaveError> {
    write_pending(true);
    SAVER.check()
}

/// Write what was added, if anything (the thread, and at exit).
pub fn flush() {
    write_pending(false);
}

fn write_pending(always: bool) {
    let Some(path) = FILE_PATH.get() else { return };
    let _one = WRITING.lock().unwrap_or_else(|p| p.into_inner());
    let (base, pending, seen) = {
        let s = lock();
        if s.pending.is_empty() && !always {
            return;
        }
        (s.base.clone(), s.pending.clone(), s.seen)
    };
    let base = if SAVER.unread(path) {
        // A file that couldn't be read: its counts are the base once it
        // reads; till then nothing's written (the counts stay pending).
        match SAVER.reread(path, crate::shell::saving::json) {
            Some(disk) => disk,
            None => return,
        }
    } else if modified(path) != seen {
        // Another Yap wrote the file since: its counts are the base now.
        read(path)
    } else {
        base
    };
    let mut next = base;
    next.merge(&pending);
    next.trim(today());
    if SAVER.write_json(path, &next, false).is_err() {
        // Kept: the next write tries again (saving.rs says so once).
        return;
    }
    {
        let mut s = lock();
        // What was added while writing stays pending.
        let mut after = std::mem::take(&mut s.pending);
        subtract(&mut after, &pending);
        s.pending = after;
        s.base = next;
        s.seen = modified(path);
    }
    if let Some(app) = APP.get() {
        let _ = app.emit(EVENT, ());
    }
}

/// Reset app (`privacy/reset.rs`): every count gone — the file and what
/// this Yap added since its last write, so the thread's next write starts
/// from nothing.
pub fn reset() {
    let _one = WRITING.lock().unwrap_or_else(|p| p.into_inner());
    let mut s = lock();
    s.base = Store { version: 1, days: BTreeMap::new() };
    s.pending = Store { version: 1, days: BTreeMap::new() };
    if let Some(path) = FILE_PATH.get() {
        match std::fs::remove_file(path) {
            Ok(()) => SAVER.nothing_to_save(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => SAVER.nothing_to_save(),
            // Held by another program: an empty file over it (or said).
            Err(_) => {
                let _ = SAVER.write_json(path, &s.base, false);
            }
        }
        s.seen = modified(path);
    }
    drop(s);
    if let Some(app) = APP.get() {
        let _ = app.emit(EVENT, ());
    }
}

/// `from` less what `written` holds (the pending counts a write took):
/// a row left with no calls goes. Pure.
fn subtract(from: &mut Store, written: &Store) {
    for (day, rows) in &written.days {
        let Some(have) = from.days.get_mut(day) else { continue };
        for w in rows {
            if let Some(r) = have.iter_mut().find(|r| r.same_as(w)) {
                r.calls = r.calls.saturating_sub(w.calls);
                r.unreported = r.unreported.saturating_sub(w.unreported);
                r.prompt = r.prompt.saturating_sub(w.prompt);
                r.answer = r.answer.saturating_sub(w.answer);
                r.cost_micros = r.cost_micros.saturating_sub(w.cost_micros);
                r.read_tokens = r.read_tokens.saturating_sub(w.read_tokens);
                r.read_ms = r.read_ms.saturating_sub(w.read_ms);
                r.gen_tokens = r.gen_tokens.saturating_sub(w.gen_tokens);
                r.gen_ms = r.gen_ms.saturating_sub(w.gen_ms);
            }
        }
        have.retain(|r| r.calls > 0);
    }
    from.days.retain(|_, rows| !rows.is_empty());
}

/// Everything counted: the file (read again if another Yap wrote it) and
/// what this Yap added since.
pub fn current() -> Store {
    let mut s = lock();
    if let Some(path) = FILE_PATH.get() {
        let on_disk = modified(path);
        if on_disk != s.seen {
            s.base = read(path);
            s.seen = on_disk;
        }
    }
    let mut all = s.base.clone();
    all.merge(&s.pending);
    all
}

// ---- the summary ---------------------------------------------------------------------------

/// One model's use over a stretch of days.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelUse {
    pub provider: String,
    pub model: String,
    /// The on-device model's name ("Qwen3.5 4B"); "" for any other.
    pub name: String,
    pub here: bool,
    pub tokens: u64,
    pub prompt: u64,
    pub answer: u64,
    pub calls: u64,
    /// The estimate, millionths of a dollar: `None` when there's no price
    /// for the model (or it ran on this PC).
    pub cost_micros: Option<u64>,
    /// How fast it wrote (tokens a second), when its server said.
    pub per_second: Option<f64>,
}

/// One feature's use over a stretch of days.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeatureUse {
    pub feature: String,
    pub tokens: u64,
    pub calls: u64,
}

/// A stretch of days, added up.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Totals {
    pub days: i64,
    pub tokens: u64,
    pub prompt: u64,
    pub answer: u64,
    pub calls: u64,
    /// Calls that didn't say their tokens.
    pub unreported: u64,
    /// Tokens on this PC, and elsewhere.
    pub here: u64,
    pub cloud: u64,
    /// The cloud calls' estimated cost (models with a price).
    pub cost_micros: u64,
    /// Cloud tokens with no price known (not in the estimate).
    pub unpriced: u64,
    /// Most tokens first.
    pub models: Vec<ModelUse>,
    pub features: Vec<FeatureUse>,
}

/// The on-device model's writing speed on a day.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DaySpeed {
    pub day: String,
    /// Tokens written a second, over the day's calls.
    pub per_second: f64,
    /// Tokens read a second (the prompt), when said.
    pub read_per_second: Option<f64>,
    pub tokens: u64,
}

/// What the pages get ([`super::ai_usage_summary`]).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub today: Totals,
    /// The last 7 days, today included.
    pub week: Totals,
    /// The last 30 days.
    pub month: Totals,
    /// The on-device models' speed, by day (the last 30 days, oldest first).
    pub speed: Vec<DaySpeed>,
    pub prices_as_of: &'static str,
    pub price_sources: Vec<PriceSource>,
    pub days_kept: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PriceSource {
    pub name: &'static str,
    pub url: &'static str,
}

/// The last `days` days up to `today`, added up. `name`: the on-device
/// model's name for its id. Pure.
pub fn totals(store: &Store, today: NaiveDate, days: i64, name: &dyn Fn(&str) -> String) -> Totals {
    let first = day_key(today - chrono::Duration::days(days - 1));
    let last = day_key(today);
    let mut t = Totals { days, ..Default::default() };
    let mut models: Vec<(Row, bool)> = Vec::new();
    let mut features: BTreeMap<String, FeatureUse> = BTreeMap::new();
    for (_, rows) in store.days.range(first..=last) {
        for r in rows {
            let tokens = r.tokens();
            t.tokens += tokens;
            t.prompt += r.prompt;
            t.answer += r.answer;
            t.calls += r.calls;
            t.unreported += r.unreported;
            let priced = !r.here && super::prices::has_price(&r.provider, &r.model);
            if r.here {
                t.here += tokens;
            } else {
                t.cloud += tokens;
                if priced {
                    t.cost_micros += r.cost_micros;
                } else {
                    t.unpriced += tokens;
                }
            }
            match models.iter_mut().find(|(m, _)| m.provider == r.provider && m.model == r.model && m.here == r.here) {
                Some((m, _)) => m.add(r),
                None => models.push((Row { feature: String::new(), ..r.clone() }, priced)),
            }
            let f = features.entry(r.feature.clone()).or_insert_with(|| FeatureUse { feature: r.feature.clone(), ..Default::default() });
            f.tokens += tokens;
            f.calls += r.calls;
        }
    }
    t.models = models
        .into_iter()
        .map(|(m, priced)| ModelUse {
            name: if m.provider == crate::ai::local_llm::PROVIDER_ONDEVICE { name(&m.model) } else { String::new() },
            tokens: m.tokens(),
            per_second: (m.gen_ms > 0 && m.gen_tokens > 0).then(|| m.gen_tokens as f64 * 1000.0 / m.gen_ms as f64),
            cost_micros: priced.then_some(m.cost_micros),
            provider: m.provider,
            model: m.model,
            here: m.here,
            prompt: m.prompt,
            answer: m.answer,
            calls: m.calls,
        })
        .collect();
    t.models.sort_by(|a, b| b.tokens.cmp(&a.tokens).then(b.calls.cmp(&a.calls)));
    t.features = features.into_values().collect();
    t.features.sort_by(|a, b| b.tokens.cmp(&a.tokens).then(b.calls.cmp(&a.calls)));
    t
}

/// The models on this PC's writing speed, by day, over the last `days`
/// days. Pure.
pub fn speed(store: &Store, today: NaiveDate, days: i64) -> Vec<DaySpeed> {
    let first = day_key(today - chrono::Duration::days(days - 1));
    let last = day_key(today);
    store
        .days
        .range(first..=last)
        .filter_map(|(day, rows)| {
            let (gt, gm, rt, rm) = rows.iter().filter(|r| r.here).fold((0u64, 0u64, 0u64, 0u64), |(a, b, c, d), r| {
                (a + r.gen_tokens, b + r.gen_ms, c + r.read_tokens, d + r.read_ms)
            });
            (gt > 0 && gm > 0).then(|| DaySpeed {
                day: day.clone(),
                per_second: gt as f64 * 1000.0 / gm as f64,
                read_per_second: (rt > 0 && rm > 0).then(|| rt as f64 * 1000.0 / rm as f64),
                tokens: gt,
            })
        })
        .collect()
}

/// The summary of `store` on `today`. Pure.
pub fn summarize(store: &Store, today: NaiveDate, name: &dyn Fn(&str) -> String) -> Summary {
    Summary {
        today: totals(store, today, 1, name),
        week: totals(store, today, 7, name),
        month: totals(store, today, 30, name),
        speed: speed(store, today, 30),
        prices_as_of: super::prices::PRICES_AS_OF,
        price_sources: super::prices::SOURCES.iter().map(|(name, url)| PriceSource { name, url }).collect(),
        days_kept: KEEP_DAYS,
    }
}

/// What's counted, summed up for the pages.
pub fn summary() -> Summary {
    let name = |id: &str| {
        let search = crate::search::embedder::model();
        if id == search.tag {
            search.name.to_string()
        } else {
            crate::ai::local_llm::display_for_id(id)
        }
    };
    summarize(&current(), today(), &name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::usage::Speed;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn reported(prompt: u64, answer: u64) -> Reported {
        Reported { prompt: Some(prompt), answer: Some(answer), total: Some(prompt + answer), speed: Speed::default() }
    }

    fn call(provider: &str, model: &str, here: bool, feature: Feature, r: Reported, day: &str) -> Row {
        Row::of_call(provider, model, here, feature, &r, day)
    }

    #[test]
    fn calls_of_one_model_and_feature_add_up_in_a_day() {
        let mut s = Store::default();
        let day = "2026-10-25";
        s.add(day, &call("groq", "openai/gpt-oss-20b", false, Feature::Cleanup, reported(1_000, 200), day));
        s.add(day, &call("groq", "openai/gpt-oss-20b", false, Feature::Cleanup, reported(500, 100), day));
        s.add(day, &call("groq", "openai/gpt-oss-20b", false, Feature::Chat, reported(3_000, 300), day));
        // A call that said nothing: counted as a call, no tokens.
        let silent = Reported::default();
        s.add(day, &call("custom", "my-model", false, Feature::Chat, silent, day));
        let rows = &s.days[day];
        assert_eq!(rows.len(), 3);
        let cleanup = rows.iter().find(|r| r.feature == "cleanup").unwrap();
        assert_eq!((cleanup.calls, cleanup.prompt, cleanup.answer), (2, 1_500, 300));
        // Each call at $0.075/M prompt + $0.30/M answer, rounded to the
        // millionth: 75 + 60, then 37.5 + 30.
        assert_eq!(cleanup.cost_micros, 135 + 68);
        let custom = rows.iter().find(|r| r.provider == "custom").unwrap();
        assert_eq!((custom.calls, custom.unreported, custom.tokens(), custom.cost_micros), (1, 1, 0, 0));
        // The file keeps no text, only these counts.
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"2026-10-25\"") && json.contains("\"costMicros\""));
    }

    #[test]
    fn a_new_day_starts_its_own_counts_and_90_days_are_kept() {
        let mut s = Store::default();
        let r = |day: &str| call("ondevice", "Qwen_Qwen3.5-4B-Q4_K_M", true, Feature::Chat, reported(100, 10), day);
        s.add("2026-10-24", &r("2026-10-24"));
        s.add("2026-10-25", &r("2026-10-25"));
        assert_eq!(s.days.len(), 2);
        assert_eq!(s.days["2026-10-25"][0].calls, 1, "yesterday's counts stay yesterday's");
        // The 90th day back stays, the 91st goes; a key that isn't a date goes.
        s.add("2026-07-28", &r("2026-07-28"));
        s.add("2026-07-27", &r("2026-07-27"));
        s.days.insert("someday".into(), vec![r("2026-10-25")]);
        s.trim(d("2026-10-25"));
        assert!(s.days.contains_key("2026-07-28"));
        assert!(!s.days.contains_key("2026-07-27"));
        assert!(!s.days.contains_key("someday"));
        assert_eq!(s.days.len(), 3);
    }

    #[test]
    fn another_yaps_counts_and_this_ones_are_both_kept() {
        // What this Yap wrote, another Yap's write on top, and this Yap's
        // calls since: the file gets them all.
        let day = "2026-10-25";
        let a = call("groq", "openai/gpt-oss-20b", false, Feature::Cleanup, reported(100, 10), day);
        let mut disk = Store::default();
        disk.add(day, &a);
        disk.add(day, &a); // the other Yap's
        let mut pending = Store::default();
        pending.add(day, &a);
        let mut next = disk.clone();
        next.merge(&pending);
        assert_eq!(next.days[day][0].calls, 3);
        // A write took `pending`; a call that came meanwhile stays pending.
        let mut now = pending.clone();
        now.add(day, &a);
        subtract(&mut now, &pending);
        assert_eq!(now.days[day][0].calls, 1);
        let all = now.clone();
        subtract(&mut now, &all);
        assert!(now.is_empty() && now.days.is_empty());
        // An older file's unknown fields don't stop it reading; a missing
        // version is 1.
        let old: Store = serde_json::from_str(r#"{"days":{"2026-10-25":[{"provider":"groq","model":"m","feature":"chat","calls":2,"prompt":5,"answer":1,"later":true}]}}"#).unwrap();
        assert_eq!((old.version, old.days[day][0].calls, old.days[day][0].here), (1, 2, false));
    }

    #[test]
    fn the_summary_says_today_the_week_and_the_month_by_model_and_feature() {
        let mut s = Store::default();
        let add = |s: &mut Store, day: &str, provider: &str, model: &str, here: bool, f: Feature, p: u64, a: u64| {
            s.add(day, &call(provider, model, here, f, reported(p, a), day));
        };
        add(&mut s, "2026-10-25", "groq", "openai/gpt-oss-20b", false, Feature::Cleanup, 1_000, 200);
        add(&mut s, "2026-10-25", "ondevice", "Qwen_Qwen3.5-4B-Q4_K_M", true, Feature::Chat, 4_000, 400);
        add(&mut s, "2026-10-21", "openrouter", "acme/brainy-9", false, Feature::Chat, 2_000, 100);
        add(&mut s, "2026-10-10", "anthropic", "claude-haiku-5-5", false, Feature::Meetings, 10_000, 1_000);
        // Out of every window.
        add(&mut s, "2026-08-01", "groq", "openai/gpt-oss-20b", false, Feature::Cleanup, 9_999, 9);
        let name = |id: &str| if id == "Qwen_Qwen3.5-4B-Q4_K_M" { "Qwen3.5 4B".to_string() } else { id.to_string() };
        let sum = summarize(&s, d("2026-10-25"), &name);

        assert_eq!((sum.today.tokens, sum.today.here, sum.today.cloud), (5_600, 4_400, 1_200));
        assert_eq!(sum.today.cost_micros, 135);
        assert_eq!(sum.today.models[0].name, "Qwen3.5 4B");
        assert_eq!(sum.today.models[0].cost_micros, None, "on this PC: free, no estimate");
        assert_eq!(sum.today.models[1].cost_micros, Some(135));
        assert_eq!(sum.today.features.iter().map(|f| f.feature.as_str()).collect::<Vec<_>>(), ["chat", "cleanup"]);

        // The week has OpenRouter's (no price known: counted apart).
        assert_eq!(sum.week.tokens, 7_700);
        assert_eq!((sum.week.unpriced, sum.week.cost_micros), (2_100, 135));
        let brainy = sum.week.models.iter().find(|m| m.model == "acme/brainy-9").unwrap();
        assert_eq!(brainy.cost_micros, None);
        // The month has Haiku 5.5's meeting summary: 10,000 × $0.10/M + 1,000 × $0.50/M.
        assert_eq!(sum.month.tokens, 18_700);
        assert_eq!(sum.month.cost_micros, 135 + 1_500);
        assert_eq!(sum.month.features[0].feature, "meetings");
        assert_eq!(sum.days_kept, 90);
    }

    #[test]
    fn the_on_device_speed_is_its_tokens_over_its_time_each_day() {
        let mut s = Store::default();
        let fast = Reported { prompt: Some(1_000), answer: Some(240), total: None, speed: Speed { read_tokens: 1_000, read_ms: 500, gen_tokens: 240, gen_ms: 2_000 } };
        let slow = Reported { speed: Speed { read_tokens: 0, read_ms: 0, gen_tokens: 60, gen_ms: 2_000 }, ..fast };
        s.add("2026-10-24", &Row::of_call("ondevice", "q", true, Feature::Chat, &fast, "2026-10-24"));
        s.add("2026-10-25", &Row::of_call("ondevice", "q", true, Feature::Chat, &fast, "2026-10-25"));
        s.add("2026-10-25", &Row::of_call("ondevice", "q", true, Feature::Cleanup, &slow, "2026-10-25"));
        // A cloud model's speed isn't the PC's.
        s.add("2026-10-25", &Row::of_call("groq", "g", false, Feature::Chat, &fast, "2026-10-25"));
        let sp = speed(&s, d("2026-10-25"), 30);
        assert_eq!(sp.len(), 2);
        assert_eq!(sp[0].per_second, 120.0);
        assert_eq!(sp[0].read_per_second, Some(2_000.0));
        // 300 tokens in 4 s.
        assert_eq!(sp[1].per_second, 75.0);
        let t = totals(&s, d("2026-10-25"), 1, &|id: &str| id.to_string());
        assert_eq!(t.models.iter().find(|m| m.here).unwrap().per_second, Some(75.0));
    }
}
