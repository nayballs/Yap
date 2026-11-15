//! **Routines** (2026-11-15, the Claude app's scheduled routines, run on
//! this PC): Skills that run by themselves. A routine is a Skill
//! (`chat/skills.rs`), optional words to run it with, a trigger and a
//! switch; it runs only while Yap runs, never in a cloud.
//!
//! - **Triggers** ([`Trigger`], when each is due: `schedule.rs`, pure): a
//!   schedule (every day or on weekdays at a time, or once a week on a day
//!   and time); after a meeting ends (any, only meetings in a folder, or
//!   with someone from outside the person's own email domains — the
//!   calendar's folder rules' companies, `calendar/filing/companies.rs`),
//!   once its summary is written ([`meeting_ending`], [`summary_done`]);
//!   or a few minutes before each meeting on the calendar (its title the
//!   Skill's words unless the routine has its own).
//! - **Where a run goes** ([`Dest`]): **its own chat** — one per routine, made
//!   at its first run and named after the Skill, every later run there (the
//!   list marks it: [`chat_marks`]; deleted, the next run makes another;
//!   archived, a run brings it back) — or, for a meeting's, **with the
//!   meeting's note** (a new Ask thread on it, as its Ask box keeps one:
//!   `note_chats.rs`), for a Skill that runs on one meeting. Then **one quiet
//!   notice** on the Yap bar (`run.rs`: kind "routine" in `notices/policy.rs`,
//!   held over a dictation and a meeting, gone unseen after ten minutes;
//!   never with Yap's main window in front, nor over its own chat).
//! - **Missed runs** (`schedule.rs` `due`): the PC off or asleep, Yap not
//!   running — the latest missed time runs once when Yap finds it up to
//!   three hours late, else it's skipped and the runs list says so.
//! - **Guards** (`run.rs`): one at a time (one worker); never during a
//!   dictation or while a meeting records (a meeting's own waits for its
//!   end — it's after it anyway); [`schedule::DAILY_CAP`] runs a day; on
//!   the chat's default model (Chat's scope), every call counted as
//!   "Routines" (`ai/usage.rs`); never a phone's (nothing of a phone's starts
//!   one); Chat's tools for a Skill, less anything that acts on the screen or
//!   apps with nobody there ([`unattended`]: no look at the screen, no
//!   clicks or typing on pages, no opening apps, no agents or connectors, no
//!   cards that ask).
//! - **Settings → Skills → Routines** (`settings/RoutinesSection.svelte`):
//!   each routine's trigger and switch, Run now, and the recent runs — when,
//!   the result's chat or note, failed and why, skipped and why.
//!
//! **Saved** in `routines.json` ([`SAVER`], `shell/saving.rs`): a new file an
//! older Yap never opens, so nothing of a routine lives in a store an older
//! Yap rewrites (its chat is marked from here, never in chats.json). Unit
//! tests keep a store per test thread. Logs: ids, built-ins' commands and
//! counts, never the words.

pub mod schedule;
mod run;

#[cfg(not(test))]
use std::sync::Mutex;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use schedule::Days;

/// Routines at most.
pub const MAX_ROUTINES: usize = 20;
/// The runs the list keeps, newest first.
const RUNS_KEPT: usize = 50;
/// A routine's own words with its Skill, at most.
const WORDS_MAX: usize = 200;
/// A folder name, at most.
const FOLDER_MAX: usize = 80;
/// Every window's routines and runs changed (Settings' list follows).
pub const EVENT: &str = "yap-routines-changed";
/// A run put something in a chat or a note: `{ routine, conversationId?,
/// noteId? }` (Chat's list reads itself again).
pub const RAN: &str = "yap-routine-ran";
/// The run's routine's Skill was deleted.
pub const SKILL_GONE: &str = "Its Skill isn't there any more. Pick another for this routine.";

/// When a routine runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Trigger {
    /// A time of day, every day, on weekdays or once a week.
    Schedule {
        #[serde(default)]
        days: Days,
        /// For a weekly one: 0 = Monday … 6 = Sunday.
        #[serde(default)]
        weekday: u8,
        /// "09:00", the PC's own time.
        time: String,
    },
    /// After a meeting ends, once its summary is written.
    #[serde(rename = "meetingEnd")]
    MeetingEnd {
        /// Only meetings in this folder ("": any).
        #[serde(default)]
        folder: String,
        /// Only meetings with someone from outside the person's own email
        /// domains.
        #[serde(default)]
        outside: bool,
    },
    /// A few minutes before each meeting on the calendar.
    #[serde(rename = "beforeEvent")]
    BeforeEvent {
        #[serde(default = "ten")]
        minutes: u32,
    },
}

fn ten() -> u32 {
    10
}

impl Default for Trigger {
    fn default() -> Self {
        Trigger::Schedule { days: Days::Weekdays, weekday: 0, time: "09:00".into() }
    }
}

impl Trigger {
    /// A schedule's days and time, pure.
    pub fn schedule(&self) -> Option<schedule::Schedule> {
        match self {
            Trigger::Schedule { days, weekday, time } => {
                let (hour, minute) = schedule::parse_time(time)?;
                Some(schedule::Schedule { days: *days, weekday: *weekday, hour, minute })
            }
            _ => None,
        }
    }

    /// The trigger's kind, for the logs.
    fn kind(&self) -> &'static str {
        match self {
            Trigger::Schedule { .. } => "schedule",
            Trigger::MeetingEnd { .. } => "meetingEnd",
            Trigger::BeforeEvent { .. } => "beforeEvent",
        }
    }
}

/// Where a run's answer goes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Dest {
    /// The routine's own chat.
    #[default]
    Chat,
    /// With the meeting's note (a meeting's routine, a Skill on one meeting).
    Note,
}

/// A calendar meeting a "before" routine ran for (its key, its start).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventDone {
    pub key: String,
    pub start: i64,
}

/// A routine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Routine {
    /// "r<ms>" ("" for a new one, given on save).
    #[serde(default)]
    pub id: String,
    /// The Skill's id ("builtin:standup", "s1730…").
    pub skill: String,
    /// Words to run it with ("/project-update Atlas"), as typed after a command.
    #[serde(default)]
    pub words: String,
    #[serde(default)]
    pub trigger: Trigger,
    #[serde(default = "yes")]
    pub on: bool,
    #[serde(default)]
    pub to: Dest,
    /// Its own chat (0: none yet).
    #[serde(default)]
    pub chat: u64,
    /// A schedule's last time handled — run or skipped (unix seconds). Made,
    /// changed or switched on: that moment, so no time before it runs.
    #[serde(default)]
    pub handled: i64,
    /// The calendar meetings a "before" routine ran for.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<EventDone>,
    #[serde(default)]
    pub created_ms: u64,
    #[serde(default)]
    pub updated_ms: u64,
}

fn yes() -> bool {
    true
}

/// What started a run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Cause {
    /// Its time came.
    #[default]
    Schedule,
    /// Its time was missed (the PC off or asleep): run late, or skipped.
    Missed,
    /// Settings' Run now.
    Now,
    /// A meeting ended.
    Meeting,
    /// A calendar meeting is about to start.
    Event,
}

/// Where a run is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum State {
    /// Queued, or waiting out a dictation or a meeting (`why` says which).
    #[default]
    Waiting,
    Running,
    Done,
    Failed,
    Skipped,
}

/// One run, as the runs list shows it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    /// Unique, from the clock (ms).
    pub id: u64,
    pub routine: String,
    /// The Skill's command and name then.
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub cause: Cause,
    /// A schedule's time it was for; a calendar meeting's start.
    #[serde(default)]
    pub slot: i64,
    /// When it was queued, started and ended (unix seconds; 0: not yet / never).
    #[serde(default)]
    pub queued: i64,
    #[serde(default)]
    pub started: i64,
    #[serde(default)]
    pub ended: i64,
    #[serde(default)]
    pub state: State,
    /// Why it failed, was skipped, or waits.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub why: String,
    /// Its answer: in this chat…
    #[serde(default)]
    pub chat: u64,
    /// …or with this meeting's note (and its thread there).
    #[serde(default)]
    pub note: u64,
    #[serde(default)]
    pub thread: u64,
    /// The meeting it was about ("Supplier call").
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub about: String,
}

/// What routines.json holds.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Store {
    #[serde(default)]
    pub routines: Vec<Routine>,
    /// Newest first, at most [`RUNS_KEPT`].
    #[serde(default)]
    pub runs: Vec<Run>,
}

// ---- the store --------------------------------------------------------------------------------

/// routines.json's saves.
pub static SAVER: crate::shell::saving::Saver = crate::shell::saving::Saver::new("routines", "your routines", save_now);

fn save_now() -> Result<(), crate::shell::saving::SaveError> {
    with_store(|s| save(s));
    SAVER.check()
}

#[cfg(not(test))]
static STATE: Mutex<Option<Store>> = Mutex::new(None);

#[cfg(not(test))]
fn path() -> std::path::PathBuf {
    crate::config::data_dir().join("routines.json")
}

#[cfg(not(test))]
fn with_store<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let store = guard.get_or_insert_with(|| SAVER.load(&path()));
    if SAVER.any_unread() && SAVER.refresh(&path(), store) {
        save(store);
    }
    f(store)
}

#[cfg(not(test))]
fn save(s: &Store) {
    if crate::ai::eval::active() {
        return;
    }
    let _ = SAVER.write_json(&path(), s, true);
}

#[cfg(test)]
thread_local! {
    static TEST_STORE: std::cell::RefCell<Store> = std::cell::RefCell::new(Store::default());
}

#[cfg(test)]
fn with_store<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    TEST_STORE.with(|s| f(&mut s.borrow_mut()))
}

#[cfg(test)]
fn save(_: &Store) {}

/// Every routine and run as they are (Download your data).
pub fn snapshot() -> Store {
    with_store(|s| s.clone())
}

/// The app, once started (its windows hear the changes).
static APP: OnceLock<AppHandle> = OnceLock::new();

fn changed() {
    if let Some(app) = APP.get() {
        let _ = app.emit(EVENT, ());
    }
}

fn now_secs() -> i64 {
    chrono::Utc::now().timestamp()
}

fn now_ms() -> u64 {
    chrono::Utc::now().timestamp_millis().max(0) as u64
}

// ---- making and changing them ---------------------------------------------------------------

/// A routine as saved: `r` checked and tidied against `skills` (its Skill
/// is one of them) and `folders` (a meeting's folder is one of them).
/// `Err`: what's wrong, in words. Pure.
pub fn cleaned(mut r: Routine, skills: &[crate::chat::skills::Skill], folders: &[String]) -> Result<Routine, String> {
    let skill = skills.iter().find(|s| s.id == r.skill).ok_or("Pick a Skill for it to run.")?;
    r.words = r.words.split_whitespace().collect::<Vec<_>>().join(" ");
    if r.words.chars().count() > WORDS_MAX {
        return Err(format!("Its words can be {WORDS_MAX} characters at most."));
    }
    match &mut r.trigger {
        Trigger::Schedule { days, weekday, time } => {
            let (h, m) = schedule::parse_time(time).ok_or("Give it a time of day, like 09:00.")?;
            *time = format!("{h:02}:{m:02}");
            if *days == Days::Weekly && *weekday > 6 {
                return Err("Pick the day of the week it runs on.".into());
            }
            if *days != Days::Weekly {
                *weekday = 0;
            }
        }
        Trigger::MeetingEnd { folder, .. } => {
            *folder = folder.trim().to_string();
            if folder.chars().count() > FOLDER_MAX {
                return Err("That folder's name is too long.".into());
            }
            if !folder.is_empty() {
                match folders.iter().find(|f| f.eq_ignore_ascii_case(folder)) {
                    Some(f) => *folder = f.clone(),
                    None => return Err(format!("There's no folder called {folder}.")),
                }
            }
        }
        Trigger::BeforeEvent { minutes } => {
            if !(1..=schedule::LEAD_MAX).contains(minutes) {
                return Err(format!("It runs between 1 and {} minutes before a meeting.", schedule::LEAD_MAX));
            }
        }
    }
    // With the meeting's note: a meeting's routine whose Skill runs on one
    // meeting; anything else goes to its own chat.
    if r.to == Dest::Note && (!matches!(r.trigger, Trigger::MeetingEnd { .. }) || !skill.runs_on.on_one()) {
        r.to = Dest::Chat;
    }
    Ok(r)
}

/// Save routine `r` (new: no id; else a change) at `now`: checked
/// ([`cleaned`]); a new one, or one whose trigger changed or that's
/// switched on again, counts from now (no time before it runs). Returns it
/// as saved.
pub fn save_routine(r: Routine, now: i64, ms: u64) -> Result<Routine, String> {
    let skills = crate::chat::skills::all();
    let folders = crate::notes::folders();
    let mut r = cleaned(r, &skills, &folders)?;
    with_store(|st| {
        if r.id.is_empty() {
            if st.routines.len() >= MAX_ROUTINES {
                return Err(format!("You can have {MAX_ROUTINES} routines at most. Delete one first."));
            }
            r.id = format!("r{ms}");
            while st.routines.iter().any(|o| o.id == r.id) {
                r.id = format!("r{}", ms + st.routines.len() as u64 + 1);
            }
            r.created_ms = ms;
            r.chat = 0;
            r.handled = now;
            r.events.clear();
        } else {
            let old = st.routines.iter().find(|o| o.id == r.id).ok_or("That routine isn't there any more.")?;
            r.created_ms = old.created_ms;
            // Its chat stays its own (Rust's to keep, never the page's).
            r.chat = old.chat;
            if old.trigger != r.trigger || (r.on && !old.on) {
                r.handled = now;
                r.events.clear();
            } else {
                r.handled = old.handled;
                r.events = old.events.clone();
            }
        }
        r.updated_ms = ms;
        match st.routines.iter_mut().find(|o| o.id == r.id) {
            Some(o) => *o = r.clone(),
            None => st.routines.push(r.clone()),
        }
        save(st);
        Ok(r)
    })
}

/// Delete routine `id` (its runs stay in the list; its chat stays a chat).
pub fn delete_routine(id: &str) -> Result<(), String> {
    with_store(|st| -> Result<(), String> {
        let before = st.routines.len();
        st.routines.retain(|r| r.id != id);
        if st.routines.len() == before {
            return Err("That routine isn't there any more.".into());
        }
        save(st);
        Ok(())
    })?;
    run::forget_queued(id);
    Ok(())
}

/// Routine `id` switched on or off at `now` (on again: counts from now).
pub fn set_on(id: &str, on: bool, now: i64) -> Result<(), String> {
    with_store(|st| {
        let r = st.routines.iter_mut().find(|r| r.id == id).ok_or("That routine isn't there any more.")?;
        if on && !r.on {
            r.handled = now;
            r.events.clear();
        }
        r.on = on;
        save(st);
        Ok(())
    })
}

/// Conversations deleted (Chat's Delete, Reset): a routine whose chat it
/// was makes a new one at its next run — a later conversation may get the
/// id, and mustn't become a routine's.
pub fn chats_deleted(ids: &[u64]) {
    if ids.is_empty() {
        return;
    }
    let touched = with_store(|st| {
        let mut touched = false;
        for r in st.routines.iter_mut().filter(|r| r.chat != 0 && ids.contains(&r.chat)) {
            r.chat = 0;
            touched = true;
        }
        for run in st.runs.iter_mut().filter(|run| run.chat != 0 && ids.contains(&run.chat)) {
            run.chat = 0;
            touched = true;
        }
        if touched {
            save(st);
        }
        touched
    });
    if touched {
        changed();
    }
}

/// Every chat deleted (Delete some of your data → Chats): no routine has one.
pub fn reset_chats() {
    let every: Vec<u64> = with_store(|st| st.routines.iter().map(|r| r.chat).chain(st.runs.iter().map(|r| r.chat)).filter(|&c| c != 0).collect());
    chats_deleted(&every);
}

/// **The routines' chats, marked in Chat's list** (`chats_list`): each
/// listed conversation that's a routine's gets `routine` — its Skill's
/// `/command`.
pub fn chat_marks(list: &mut serde_json::Value) {
    let marks: Vec<(u64, String)> = with_store(|st| st.routines.iter().filter(|r| r.chat != 0).map(|r| (r.chat, r.skill.clone())).collect());
    if marks.is_empty() {
        return;
    }
    let skills = crate::chat::skills::all();
    let Some(items) = list.as_array_mut() else { return };
    for item in items {
        let Some(id) = item["id"].as_u64() else { continue };
        if let Some((_, skill)) = marks.iter().find(|(c, _)| *c == id) {
            let command = skills.iter().find(|s| &s.id == skill).map(|s| s.command.clone()).unwrap_or_default();
            item["routine"] = serde_json::Value::String(command);
        }
    }
}

// ---- the runs' records ------------------------------------------------------------------------

/// Record `run` (a new one first; one already there changed in place),
/// keeping the newest [`RUNS_KEPT`]. Pure but for the store.
fn record_in(st: &mut Store, run: Run) {
    match st.runs.iter_mut().find(|r| r.id == run.id) {
        Some(r) => *r = run,
        None => {
            st.runs.insert(0, run);
            st.runs.truncate(RUNS_KEPT);
        }
    }
}

/// A run's id: the clock's ms, never one already listed.
fn new_run_id(st: &Store) -> u64 {
    let mut id = now_ms();
    while st.runs.iter().any(|r| r.id == id) {
        id += 1;
    }
    id
}

/// Runs Yap left waiting or running when it closed: failed, said so.
fn close_left_over(st: &mut Store, now: i64) -> bool {
    let mut any = false;
    for run in st.runs.iter_mut().filter(|r| matches!(r.state, State::Waiting | State::Running)) {
        run.state = State::Failed;
        run.why = "Yap closed before it finished.".into();
        run.ended = now;
        any = true;
    }
    any
}

// ---- what's due --------------------------------------------------------------------------------

/// What a tick found for one routine.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Found {
    /// Queue a run.
    Run { routine: String, cause: Cause, slot: i64, words: String, about: String },
    /// Skipped (missed by more than a few hours): recorded, never run.
    Skip { routine: String, slot: i64 },
}

/// **What's due at `now`** among `routines` (switched on), given the
/// calendar's upcoming meetings: a schedule's latest time since it was last
/// handled (on time, late or skipped: `schedule::due`), and each meeting a
/// "before" routine hasn't run for. Moves each routine's marks
/// (`handled`, `events`) past what it found; returns what to queue or
/// record. Pure.
pub(crate) fn due_now<Tz: chrono::TimeZone>(routines: &mut [Routine], events: &[schedule::Upcoming], now: i64, tz: &Tz) -> Vec<Found> {
    let mut out = Vec::new();
    for r in routines.iter_mut().filter(|r| r.on) {
        match &r.trigger {
            Trigger::Schedule { .. } => {
                let Some(s) = r.trigger.schedule() else { continue };
                match schedule::due(&s, r.handled, now, tz) {
                    schedule::Due::Not => {}
                    schedule::Due::Run { slot, late } => {
                        r.handled = slot;
                        let cause = if late { Cause::Missed } else { Cause::Schedule };
                        out.push(Found::Run { routine: r.id.clone(), cause, slot, words: r.words.clone(), about: String::new() });
                    }
                    schedule::Due::Skip { slot } => {
                        r.handled = slot;
                        out.push(Found::Skip { routine: r.id.clone(), slot });
                    }
                }
            }
            Trigger::BeforeEvent { minutes } => {
                let done: Vec<String> = r.events.iter().map(|e| e.key.clone()).collect();
                for e in schedule::events_due(events, *minutes, now, &done) {
                    r.events.push(EventDone { key: e.key.to_string(), start: e.start });
                    // The meeting's title, unless the routine has words of its own.
                    let words = if r.words.is_empty() { e.title.to_string() } else { r.words.clone() };
                    out.push(Found::Run { routine: r.id.clone(), cause: Cause::Event, slot: e.start, words, about: e.title.to_string() });
                }
                // (Meetings that began over a day ago are forgotten.)
                r.events.retain(|e| e.start > now - 86_400);
                let len = r.events.len();
                if len > 50 {
                    r.events.drain(..len - 50);
                }
            }
            Trigger::MeetingEnd { .. } => {}
        }
    }
    out
}

/// A tick of the scheduler at `now`: what's due queued, what was missed by
/// too long recorded as skipped. The calendar is read as it is (no sync, no
/// network: `calendar::agenda`), never under the store's lock.
fn tick(now: i64) {
    let any_before = with_store(|st| st.routines.iter().any(|r| r.on && matches!(r.trigger, Trigger::BeforeEvent { .. })));
    let agenda = if any_before { crate::calendar::agenda() } else { crate::calendar::Agenda::default() };
    let titles: Vec<String> = agenda.events.iter().map(|e| e.display_title()).collect();
    let upcoming: Vec<schedule::Upcoming> = agenda
        .events
        .iter()
        .zip(titles.iter())
        .map(|(e, title)| schedule::Upcoming { key: &e.key, title, start: e.start, end: e.end, meeting: !e.attendees.is_empty() || e.join_url.is_some() })
        .collect();
    let found = with_store(|st| {
        let before = st.routines.clone();
        let found = due_now(&mut st.routines, &upcoming, now, &chrono::Local);
        if st.routines != before {
            save(st);
        }
        found
    });
    for f in found {
        match f {
            Found::Run { routine, cause, slot, words, about } => run::queue(&routine, cause, slot, 0, words, about),
            Found::Skip { routine, slot } => skipped(&routine, slot),
        }
    }
}

/// A schedule's time missed by more than a few hours: recorded, not run.
fn skipped(routine: &str, slot: i64) {
    let Some(r) = with_store(|st| st.routines.iter().find(|r| r.id == routine).cloned()) else { return };
    let (command, name) = skill_names(&r.skill);
    let when = chrono::DateTime::from_timestamp(slot, 0).map(|t| t.with_timezone(&chrono::Local).format("%H:%M").to_string()).unwrap_or_default();
    tracing::info!(routine, "Routines: a missed time skipped (over the grace)");
    with_store(|st| {
        let id = new_run_id(st);
        let now = now_secs();
        record_in(
            st,
            Run {
                id,
                routine: routine.to_string(),
                command,
                name,
                cause: Cause::Missed,
                slot,
                queued: now,
                ended: now,
                state: State::Skipped,
                why: format!("Missed at {when}: Yap wasn't running, and that was over {} hours ago.", schedule::GRACE_SECS / 3600),
                ..Default::default()
            },
        );
        save(st);
    });
    changed();
}

/// A Skill's command and name, by its id ("" for one deleted).
fn skill_names(id: &str) -> (String, String) {
    crate::chat::skills::all().into_iter().find(|s| s.id == id).map(|s| (s.command, s.name)).unwrap_or_default()
}

// ---- after a meeting ---------------------------------------------------------------------------

/// Meetings that ended and whose summary is being written: their "after"
/// routines run once it is ([`summary_done`]).
static ENDING: std::sync::Mutex<Vec<u64>> = std::sync::Mutex::new(Vec::new());

/// **Meeting `note_id` ended** (`meetings/end.rs`, not one started by
/// mistake): its "after" routines wait for its summary.
pub fn meeting_ending(note_id: u64) {
    let mut v = ENDING.lock().unwrap_or_else(|p| p.into_inner());
    if !v.contains(&note_id) {
        v.push(note_id);
    }
}

/// **The summary of meeting `note_id` was written, or couldn't be**
/// (`meetings/end.rs`): if the meeting just ended, each "after" routine it
/// matches is queued (a summary written again later runs none).
pub fn summary_done(note_id: u64) {
    let ended = {
        let mut v = ENDING.lock().unwrap_or_else(|p| p.into_inner());
        let had = v.contains(&note_id);
        v.retain(|n| *n != note_id);
        had
    };
    if !ended {
        return;
    }
    let after: Vec<Routine> = with_store(|st| st.routines.iter().filter(|r| r.on && matches!(r.trigger, Trigger::MeetingEnd { .. })).cloned().collect());
    if after.is_empty() {
        return;
    }
    let Some(note) = crate::notes::get(note_id) else { return };
    // The meeting's companies besides the person's own (only when a routine asks).
    let outside = if after.iter().any(|r| matches!(r.trigger, Trigger::MeetingEnd { outside: true, .. })) {
        crate::calendar::filing::companies::outside_of(&note)
    } else {
        Vec::new()
    };
    let about = crate::search::chunks::note_title(&note);
    for r in after {
        if let Trigger::MeetingEnd { folder, outside: only_outside } = &r.trigger {
            if schedule::meeting_matches(folder, *only_outside, &note.folder, &outside) {
                tracing::info!(routine = %r.id, note_id, "Routines: a meeting's routine queued");
                run::queue(&r.id, Cause::Meeting, note.created_ts as i64, note_id, r.words.clone(), about.clone());
            }
        }
    }
}

// ---- the turn's limits -------------------------------------------------------------------------

tokio::task_local! {
    /// A routine's run: nobody's there to see the screen or answer a card.
    static UNATTENDED: ();
}

/// Run `f` as a routine's run ([`unattended`] then says so).
pub(crate) async fn as_routine<F: std::future::Future>(f: F) -> F::Output {
    UNATTENDED.scope((), f).await
}

/// **The turn running here is a routine's** (`chat/turn/gates.rs`): no look
/// at the screen, no acting on pages, no opening apps, no agents, connectors
/// or cards that ask — nobody's there.
pub fn unattended() -> bool {
    UNATTENDED.try_with(|_| ()).is_ok()
}

// ---- starting --------------------------------------------------------------------------------

/// How long after Yap starts the first tick comes (the windows up, the
/// model warming).
const START_DELAY: std::time::Duration = std::time::Duration::from_secs(20);
/// How often the scheduler looks.
const TICK: std::time::Duration = std::time::Duration::from_secs(30);

/// **Start with the app**: a run Yap left waiting or running is failed and
/// said so; then the scheduler ticks (missed runs found at the first tick),
/// and the worker runs what's queued, one at a time. Not in a model
/// evaluation or a benchmark.
pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
    let now = now_secs();
    if with_store(|st| {
        let any = close_left_over(st, now);
        if any {
            save(st);
        }
        any
    }) {
        tracing::info!("Routines: runs left unfinished when Yap closed, marked failed");
    }
    let worker = app.clone();
    tauri::async_runtime::spawn(run::worker(worker));
    tauri::async_runtime::spawn(async {
        tokio::time::sleep(START_DELAY).await;
        loop {
            tick(now_secs());
            tokio::time::sleep(TICK).await;
        }
    });
}

// ---- commands ------------------------------------------------------------------------------------

/// A routine as Settings lists it: the routine, its Skill's command and
/// name (`skillGone`: deleted), when it runs next (unix seconds: a schedule,
/// or the next meeting on the calendar for a "before" one), and whether it's
/// waiting or running now.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Listed {
    #[serde(flatten)]
    pub routine: Routine,
    pub command: String,
    pub name: String,
    pub skill_gone: bool,
    /// Its Skill runs on one meeting (a meeting's may go with its note).
    pub on_one: bool,
    pub next: Option<i64>,
    pub busy: bool,
}

/// Settings' list: the routines, the recent runs, and today's count
/// against the cap.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    pub routines: Vec<Listed>,
    pub runs: Vec<Run>,
    pub today: usize,
    pub cap: usize,
    pub max: usize,
}

/// The next time routine `r` runs, from `now` (`events`: the calendar's
/// meetings' starts, for a "before" one). Pure.
fn next_of<Tz: chrono::TimeZone>(r: &Routine, now: i64, tz: &Tz, starts: &[(i64, bool)]) -> Option<i64> {
    if !r.on {
        return None;
    }
    match &r.trigger {
        Trigger::Schedule { .. } => r.trigger.schedule().and_then(|s| schedule::next_slot(&s, now, tz)),
        Trigger::BeforeEvent { minutes } => {
            let lead = i64::from(*minutes) * 60;
            starts.iter().filter(|(start, meeting)| *meeting && start - lead > now).map(|(start, _)| start - lead).min()
        }
        Trigger::MeetingEnd { .. } => None,
    }
}

/// Every routine, the recent runs, today's count.
#[tauri::command]
pub fn routines_list() -> Listing {
    let now = now_secs();
    let skills = crate::chat::skills::all();
    let store = snapshot();
    let starts: Vec<(i64, bool)> = if store.routines.iter().any(|r| matches!(r.trigger, Trigger::BeforeEvent { .. })) {
        crate::calendar::agenda().events.iter().map(|e| (e.start, !e.attendees.is_empty() || e.join_url.is_some())).collect()
    } else {
        Vec::new()
    };
    let busy = run::busy_ids();
    let routines = store
        .routines
        .iter()
        .map(|r| {
            let skill = skills.iter().find(|s| s.id == r.skill);
            Listed {
                command: skill.map(|s| s.command.clone()).unwrap_or_default(),
                name: skill.map(|s| s.name.clone()).unwrap_or_default(),
                skill_gone: skill.is_none(),
                on_one: skill.is_some_and(|s| s.runs_on.on_one()),
                next: next_of(r, now, &chrono::Local, &starts),
                busy: busy.contains(&r.id),
                routine: r.clone(),
            }
        })
        .collect();
    let today = schedule::runs_today(store.runs.iter().map(|r| r.started), now, &chrono::Local);
    Listing { routines, runs: store.runs, today, cap: schedule::DAILY_CAP, max: MAX_ROUTINES }
}

/// Save a routine (new, or a change).
#[tauri::command]
pub fn routine_save(routine: Routine) -> Result<Routine, String> {
    let saved = save_routine(routine, now_secs(), now_ms())?;
    tracing::info!(id = %saved.id, trigger = saved.trigger.kind(), on = saved.on, "Routines: saved");
    changed();
    SAVER.check().map_err(|e| e.to_string())?;
    Ok(saved)
}

/// Delete a routine.
#[tauri::command]
pub fn routine_delete(id: String) -> Result<(), String> {
    delete_routine(&id)?;
    tracing::info!(id = %id, "Routines: deleted");
    changed();
    SAVER.check().map_err(|e| e.to_string())?;
    Ok(())
}

/// Switch a routine on or off.
#[tauri::command]
pub fn routine_set_on(id: String, on: bool) -> Result<(), String> {
    set_on(&id, on, now_secs())?;
    tracing::info!(id = %id, on, "Routines: switched");
    changed();
    SAVER.check().map_err(|e| e.to_string())?;
    Ok(())
}

/// **Run now** (Settings): queued at once, behind any run going, under the
/// same guards — the dictation, a meeting, today's cap. A meeting's routine
/// runs on the latest meeting; a "before" one on the next meeting on the
/// calendar (else as it is, with its own words).
#[tauri::command]
pub fn routine_run_now(id: String) -> Result<(), String> {
    let r = with_store(|st| st.routines.iter().find(|r| r.id == id).cloned()).ok_or("That routine isn't there any more.")?;
    if run::busy_ids().contains(&r.id) {
        return Err("It's running already.".into());
    }
    let now = now_secs();
    let (note, slot, words, about) = match &r.trigger {
        Trigger::MeetingEnd { .. } => {
            let latest = crate::notes::all().into_iter().filter(|n| n.note_type == "meeting" && !n.transcript.is_empty()).max_by_key(|n| n.created_ts);
            match latest {
                Some(n) => (n.id, n.created_ts as i64, r.words.clone(), crate::search::chunks::note_title(&n)),
                None => return Err("There's no meeting yet for it to run on.".into()),
            }
        }
        Trigger::BeforeEvent { .. } => {
            let agenda = crate::calendar::agenda();
            let next = agenda.events.iter().filter(|e| e.start > now && (!e.attendees.is_empty() || e.join_url.is_some())).min_by_key(|e| e.start);
            match next {
                Some(e) => (0, e.start, if r.words.is_empty() { e.display_title() } else { r.words.clone() }, e.display_title()),
                None => (0, 0, r.words.clone(), String::new()),
            }
        }
        Trigger::Schedule { .. } => (0, 0, r.words.clone(), String::new()),
    };
    tracing::info!(id = %r.id, "Routines: Run now");
    run::queue(&r.id, Cause::Now, slot, note, words, about);
    Ok(())
}

/// **Open a run's result**: the main window on its chat, or on the meeting's
/// note (Settings' runs list, the bar's Open).
#[tauri::command]
pub fn routine_open(app: AppHandle, chat: Option<u64>, note: Option<u64>) -> Result<(), String> {
    run::open(&app, chat.unwrap_or(0), note.unwrap_or(0));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::skills::{RunsOn, Skill};
    use chrono::{TimeZone, Utc};

    fn skills() -> Vec<Skill> {
        let mut v = crate::chat::skills::builtins();
        v.push(Skill { id: "s1".into(), name: "Client recap".into(), command: "client-recap".into(), instructions: "Recap it.".into(), runs_on: RunsOn::Meeting, ..Default::default() });
        v
    }
    fn folders() -> Vec<String> {
        vec!["Personal".into(), "Meetings".into(), "Clients".into()]
    }
    fn routine(skill: &str, trigger: Trigger) -> Routine {
        Routine { id: String::new(), skill: skill.into(), words: String::new(), trigger, on: true, to: Dest::Chat, chat: 0, handled: 0, events: Vec::new(), created_ms: 0, updated_ms: 0 }
    }
    fn at(day: u32, h: u32, m: u32) -> i64 {
        Utc.with_ymd_and_hms(2026, 11, day, h, m, 0).unwrap().timestamp()
    }
    fn daily(time: &str) -> Trigger {
        Trigger::Schedule { days: Days::Daily, weekday: 0, time: time.into() }
    }

    #[test]
    fn a_routine_is_checked_and_tidied() {
        let ok = cleaned(routine("builtin:standup", daily("9:05")), &skills(), &folders()).unwrap();
        assert_eq!(ok.trigger, daily("09:05"));
        assert_eq!(cleaned(routine("builtin:nope", daily("09:00")), &skills(), &folders()), Err("Pick a Skill for it to run.".to_string()));
        assert!(cleaned(routine("builtin:standup", daily("25:00")), &skills(), &folders()).is_err());
        assert!(cleaned(routine("builtin:standup", Trigger::BeforeEvent { minutes: 0 }), &skills(), &folders()).is_err());
        assert!(cleaned(routine("builtin:standup", Trigger::BeforeEvent { minutes: 61 }), &skills(), &folders()).is_err());
        // A folder by its own spelling; one that isn't there is said.
        let f = cleaned(routine("s1", Trigger::MeetingEnd { folder: " clients ".into(), outside: false }), &skills(), &folders()).unwrap();
        assert_eq!(f.trigger, Trigger::MeetingEnd { folder: "Clients".into(), outside: false });
        assert!(cleaned(routine("s1", Trigger::MeetingEnd { folder: "Nowhere".into(), outside: false }), &skills(), &folders()).unwrap_err().contains("Nowhere"));
        // With the note: only a meeting's, and a Skill on one meeting.
        let mut r = routine("s1", Trigger::MeetingEnd { folder: String::new(), outside: true });
        r.to = Dest::Note;
        assert_eq!(cleaned(r.clone(), &skills(), &folders()).unwrap().to, Dest::Note);
        r.skill = "builtin:todos".into();
        assert_eq!(cleaned(r.clone(), &skills(), &folders()).unwrap().to, Dest::Chat, "/todos runs across meetings");
        r.skill = "s1".into();
        r.trigger = daily("09:00");
        assert_eq!(cleaned(r, &skills(), &folders()).unwrap().to, Dest::Chat, "a schedule has no meeting");
        // Words: one line, not too long.
        let mut w = routine("builtin:project-update", daily("09:00"));
        w.words = "  the   Atlas\nlaunch ".into();
        assert_eq!(cleaned(w.clone(), &skills(), &folders()).unwrap().words, "the Atlas launch");
        w.words = "x".repeat(WORDS_MAX + 1);
        assert!(cleaned(w, &skills(), &folders()).is_err());
    }

    #[test]
    fn the_scheduler_queues_on_time_late_or_skips_and_moves_its_marks() {
        let mut rs = vec![
            Routine { id: "a".into(), handled: at(16, 8, 0), ..routine("builtin:standup", daily("09:00")) },
            Routine { id: "b".into(), handled: at(15, 9, 0), ..routine("builtin:todos", daily("09:00")) },
            Routine { id: "off".into(), on: false, handled: at(10, 0, 0), ..routine("builtin:todos", daily("09:00")) },
        ];
        // 09:01 on the 16th: "a" on time; "b" was handled on the 15th — the 16th's is on time too.
        let found = due_now(&mut rs, &[], at(16, 9, 1), &Utc);
        assert_eq!(
            found,
            vec![
                Found::Run { routine: "a".into(), cause: Cause::Schedule, slot: at(16, 9, 0), words: String::new(), about: String::new() },
                Found::Run { routine: "b".into(), cause: Cause::Schedule, slot: at(16, 9, 0), words: String::new(), about: String::new() },
            ]
        );
        assert_eq!((rs[0].handled, rs[1].handled, rs[2].handled), (at(16, 9, 0), at(16, 9, 0), at(10, 0, 0)), "a switched-off one is left alone");
        // The same tick again: nothing.
        assert!(due_now(&mut rs, &[], at(16, 9, 2), &Utc).is_empty());
        // The PC off overnight until 11:00 on the 17th: late, once each.
        let late = due_now(&mut rs, &[], at(17, 11, 0), &Utc);
        assert!(late.iter().all(|f| matches!(f, Found::Run { cause: Cause::Missed, slot, .. } if *slot == at(17, 9, 0))) && late.len() == 2);
        // Off until 15:00 on the 18th: skipped.
        let skipped = due_now(&mut rs, &[], at(18, 15, 0), &Utc);
        assert_eq!(skipped, vec![Found::Skip { routine: "a".into(), slot: at(18, 9, 0) }, Found::Skip { routine: "b".into(), slot: at(18, 9, 0) }]);
        assert!(due_now(&mut rs, &[], at(18, 16, 0), &Utc).is_empty());
    }

    #[test]
    fn before_a_meeting_once_with_its_title_as_the_words() {
        let mut rs = vec![
            Routine { id: "prep".into(), ..routine("builtin:prep", Trigger::BeforeEvent { minutes: 10 }) },
            Routine { id: "own".into(), words: "the launch".into(), ..routine("builtin:prep", Trigger::BeforeEvent { minutes: 15 }) },
        ];
        let events = [schedule::Upcoming { key: "c:1", title: "Weekly sync", start: at(16, 10, 0), end: at(16, 10, 30), meeting: true }];
        assert!(due_now(&mut rs, &events, at(16, 9, 40), &Utc).is_empty());
        let found = due_now(&mut rs, &events, at(16, 9, 50), &Utc);
        assert_eq!(
            found,
            vec![
                Found::Run { routine: "prep".into(), cause: Cause::Event, slot: at(16, 10, 0), words: "Weekly sync".into(), about: "Weekly sync".into() },
                Found::Run { routine: "own".into(), cause: Cause::Event, slot: at(16, 10, 0), words: "the launch".into(), about: "Weekly sync".into() },
            ]
        );
        assert!(due_now(&mut rs, &events, at(16, 9, 55), &Utc).is_empty(), "once each");
        // A day on, the meeting is forgotten.
        due_now(&mut rs, &[], at(17, 11, 0), &Utc);
        assert!(rs.iter().all(|r| r.events.is_empty()));
    }

    #[test]
    fn save_counts_from_now_keeps_its_chat_and_caps_the_count() {
        crate::chat::skills::builtins();
        let saved = save_routine(routine("builtin:standup", daily("09:00")), at(16, 8, 0), 1).unwrap();
        assert_eq!((saved.id.as_str(), saved.handled, saved.chat), ("r1", at(16, 8, 0), 0));
        // Its chat is Rust's: the page can't set it.
        with_store(|st| st.routines[0].chat = 42);
        let mut changed = saved.clone();
        changed.chat = 7;
        changed.words = "today".into();
        let again = save_routine(changed.clone(), at(16, 12, 0), 2).unwrap();
        assert_eq!((again.chat, again.handled), (42, at(16, 8, 0)), "the same trigger: its mark stays");
        changed.trigger = daily("10:00");
        assert_eq!(save_routine(changed, at(16, 12, 0), 3).unwrap().handled, at(16, 12, 0), "a new time counts from now");
        // Off, then on: from now.
        set_on("r1", false, at(16, 13, 0)).unwrap();
        set_on("r1", true, at(16, 14, 0)).unwrap();
        assert_eq!(snapshot().routines[0].handled, at(16, 14, 0));
        // Its chat deleted: no longer its.
        chats_deleted(&[42]);
        assert_eq!(snapshot().routines[0].chat, 0);
        for i in 1..MAX_ROUTINES {
            save_routine(routine("builtin:todos", daily("09:00")), at(16, 8, 0), 100 + i as u64).unwrap();
        }
        assert!(save_routine(routine("builtin:todos", daily("09:00")), at(16, 8, 0), 999).unwrap_err().contains("at most"));
        delete_routine("r1").unwrap();
        assert!(delete_routine("r1").is_err());
    }

    #[test]
    fn the_runs_list_keeps_the_newest_and_closes_what_yap_left() {
        let mut st = Store::default();
        for i in 0..(RUNS_KEPT as u64 + 5) {
            record_in(&mut st, Run { id: i, state: State::Done, ..Default::default() });
        }
        assert_eq!(st.runs.len(), RUNS_KEPT);
        assert_eq!(st.runs[0].id, RUNS_KEPT as u64 + 4, "newest first");
        record_in(&mut st, Run { id: RUNS_KEPT as u64 + 4, state: State::Running, ..Default::default() });
        assert_eq!(st.runs.len(), RUNS_KEPT, "changed in place");
        assert!(close_left_over(&mut st, 5));
        assert_eq!((st.runs[0].state, st.runs[0].why.as_str()), (State::Failed, "Yap closed before it finished."));
        assert!(!close_left_over(&mut st, 6));
    }

    #[test]
    fn the_file_reads_back_and_an_older_shape_still_reads() {
        let mut r = routine("builtin:standup", Trigger::Schedule { days: Days::Weekly, weekday: 4, time: "16:00".into() });
        r.id = "r1".into();
        let st = Store { routines: vec![r.clone()], runs: vec![Run { id: 1, routine: "r1".into(), state: State::Done, chat: 3, ..Default::default() }] };
        let json = serde_json::to_string(&st).unwrap();
        assert!(json.contains(r#""kind":"schedule""#) && json.contains(r#""days":"weekly""#), "{json}");
        assert_eq!(serde_json::from_str::<Store>(&json).unwrap(), st);
        // A routine with only what's needed reads with the defaults.
        let bare: Routine = serde_json::from_str(r#"{"skill":"builtin:prep","trigger":{"kind":"beforeEvent"}}"#).unwrap();
        assert_eq!((bare.on, bare.to, bare.trigger), (true, Dest::Chat, Trigger::BeforeEvent { minutes: 10 }));
        let meeting: Trigger = serde_json::from_str(r#"{"kind":"meetingEnd","outside":true}"#).unwrap();
        assert_eq!(meeting, Trigger::MeetingEnd { folder: String::new(), outside: true });
    }

    #[test]
    fn next_says_when_it_runs() {
        let r = routine("builtin:standup", daily("09:00"));
        assert_eq!(next_of(&r, at(16, 10, 0), &Utc, &[]), Some(at(17, 9, 0)));
        let off = Routine { on: false, ..r };
        assert_eq!(next_of(&off, at(16, 10, 0), &Utc, &[]), None);
        let before = routine("builtin:prep", Trigger::BeforeEvent { minutes: 10 });
        let starts = [(at(16, 11, 0), true), (at(16, 10, 30), false), (at(16, 10, 5), true)];
        assert_eq!(next_of(&before, at(16, 10, 0), &Utc, &starts), Some(at(16, 10, 50)), "the 10:05 one's lead has begun; a block isn't a meeting");
        assert_eq!(next_of(&routine("s1", Trigger::MeetingEnd { folder: String::new(), outside: false }), at(16, 10, 0), &Utc, &starts), None);
    }
}
