//! Calendar: Yap reads the person's own calendar straight from this PC, to
//! list upcoming meetings (the Meetings view), give a heads-up before each
//! one, and give a meeting note its title, attendees and invite. Wispr
//! Flow's calendar, local-first: there's no Yap server in between.
//!
//! **Connections** (Settings → Connectors, at most [`vault::SLOTS`]):
//! - Google Calendar in one click: installed-app OAuth with PKCE and two
//!   read-only scopes ([`google`]);
//! - any calendar by its private iCal (ICS) link: Outlook's published
//!   calendar, Google's secret address, iCloud, Fastmail… ([`ics`]).
//!
//! Their secrets (the refresh token, the link) live in Windows Credential
//! Manager ([`vault`]); `calendar.json` keeps the connections' names, the
//! next week of meetings, the reminder answers and the "Connect your
//! calendar" nudge. Event data never leaves the PC.
//!
//! **Sync**: every [`SYNC_EVERY`] seconds while Yap runs, straight after the
//! PC wakes, and on "Sync calendar"; the next 7 days, recurring meetings
//! expanded, time zones resolved, Wispr's filters applied ([`model`]).
//!
//! **Reminders** ("Notify before scheduled meetings start", `meeting_reminder`:
//! right before = 15 s, 1 or 2 minutes, never): a card, "Design review · In
//! 1 min", with **Join & take notes**, **Start notes**, snooze and ✕, up until
//! 5 minutes after the start. Recording another meeting's notes, it offers
//! to **switch** notes instead (back-to-back meetings). The card goes on
//! screen through [`present_card`] alone.
//!
//! **Into meeting notes**: whichever way a recording starts during a meeting
//! (the card, the Meetings view, call detection's "Record notes", the Notes
//! view, the meeting shortcut), [`on_meeting_started`] ties the note to the
//! event: its title (over a placeholder), the attendees (the names the
//! action plan gives tasks to), and the invite's description as context.
//! Call detection asks [`claims_call`] first, so a call during a meeting
//! whose card is up, or was answered, isn't asked about twice; a call during
//! the next meeting while recording the last one offers to switch
//! ([`on_call_started`]).
//!
//! **Join codes** (`codes.rs`, `call_windows.rs`): a call's window says
//! which meeting it is (a Meet tab's code, Zoom's or Teams' meeting in a
//! browser's address), read once as the call starts ([`read_call_code`]);
//! a code match beats time and service (`model::event_for_call`).
//!
//! **Day to day**: the next meeting in the tray's menu and tooltip
//! ([`tray_item`], "Next: Weekly sync in 12 min"; kept current by the
//! scheduler's own turns, no extra wake-ups), recurring meetings' notes
//! filed in a folder of their own ([`filing`]), and the Meetings view's
//! end of the day ([`calendar_day`]: today's meetings, their open tasks,
//! the ones without notes).
//!
//! **Briefs** ("Prepare a brief before meetings", `meeting_brief`, on): from
//! [`BRIEF_AHEAD`] before a meeting, what to know going in — last time with
//! these people, their open items, what memory knows, the invite's agenda,
//! every line citing its note, and two or three bullets from the meeting's
//! AI model on top ([`brief`]). Kept per meeting in calendar.json and made
//! again when what it was made from changes; shown on the reminder card, in
//! the Meetings view and in the meeting's notepad.

pub(crate) mod brief;
mod call_windows;
mod codes;
pub(crate) mod filing;
mod google;
mod ics;
mod links;
mod model;
pub(crate) mod tz;
mod vault;

use std::collections::{BTreeMap, HashMap, HashSet};
use crate::privacy::net::Recorded as _;
use std::sync::{LazyLock, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Listener, Manager};

pub(crate) use model::{Busy, Event};
#[cfg(any(test, debug_assertions))] // the model evaluations' calendar
pub(crate) use model::Attendee;

/// The snapshot ([`Snapshot`]), on every change.
const EVENT: &str = "yap-calendar";
/// A connection was made: `{ kind, label, meetings }` (meetings: how many of
/// its meetings are on now or in the next 7 days).
const EVENT_CONNECTED: &str = "yap-calendar-connected";
/// Something the page didn't directly ask for went wrong (a Google sign-in
/// finishing in the background): the message.
const EVENT_ERROR: &str = "yap-calendar-error";
/// Open a note in Notes: `{ noteId, stop }` (call detection's event).
const EVENT_OPEN: &str = "yap-meeting-open-note";
/// A meeting's brief was made or finished (`brief::Brief`).
const EVENT_BRIEF: &str = "yap-meeting-brief";

/// Sync this often while Yap runs (Wispr: every 10–15 minutes).
const SYNC_EVERY: i64 = 12 * 60;
/// Meetings kept: from this long ago (one running over, today's)…
const LOOK_BACK: i64 = 12 * 3600;
/// …to this far ahead (Wispr's "next 7 days").
const LOOK_AHEAD: i64 = 7 * 86_400;
/// The Meetings view shows at most this many (Wispr: up to 50).
const MAX_SHOWN: usize = 50;
/// A feed bigger than this isn't read (a decade of history is a few MB).
const MAX_FEED_BYTES: usize = 32 * 1024 * 1024;
/// "Snooze" brings the card back after this long (Wispr: 2 minutes).
const SNOOZE_SECS: i64 = 2 * 60;
/// Reminder answers are kept this long (they only matter around the start).
const ANSWER_KEEP: i64 = 2 * 86_400;
/// The scheduler looks at the clock at least this often (and notices a PC
/// that slept, when the wall clock jumps further).
const MAX_SLEEP: u64 = 30;
/// The calendar's card on the Yap bar (one at a time).
const BAR_CARD: &str = "calendar";
/// While a card is up the scheduler turns this often (seconds), so the bar
/// card's "In 1 min" stays true and the card follows you out of the window.
const CARD_TICK: i64 = 5;
/// A meeting's brief is made this long before it starts (in time for the
/// reminder card, with a small model on a processor)…
const BRIEF_AHEAD: i64 = 15 * 60;
/// …kept until this long after it started…
const BRIEF_KEEP: i64 = 2 * 86_400;
/// …at most this many of them…
const BRIEF_MAX: usize = 40;
/// …and one whose summary failed is tried again by itself after this long
/// (asked for, at once).
const BRIEF_RETRY: i64 = 2 * 60;

const NO_GOOGLE: &str = "Google sign-in is in the installed Yap. In this build, use a private iCal link under Other calendar.";
const TOO_MANY: &str = "You've connected as many calendars as Yap holds. Disconnect one first.";

// ---- the store (calendar.json) -------------------------------------------------------------

/// A connected calendar. Its secret is in Credential Manager, under its slot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Connection {
    /// The slot ("1"…"8"), which also names its secret.
    id: String,
    /// "google" | "outlook" | "ics" (a link from the Outlook card, or any other).
    kind: String,
    /// The account ("imogen@example.com") or the calendar's name.
    label: String,
    /// Where it comes from, for a link: its host, never the link itself.
    #[serde(default)]
    detail: String,
    #[serde(default)]
    added_ts: i64,
    #[serde(default)]
    synced_ts: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    /// Google ended the access: only connecting again helps.
    #[serde(default)]
    reconnect: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Nudge {
    /// "Not now" on "Connect your calendar": never again.
    #[serde(default)]
    dismissed: bool,
    /// The after-a-meeting nudge has been shown (it's shown once).
    #[serde(default)]
    shown_after_meeting: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Store {
    #[serde(default)]
    connections: Vec<Connection>,
    #[serde(default)]
    events: Vec<Event>,
    /// Reminders answered, by `model::answer_key`: when (unix seconds).
    #[serde(default)]
    answered: BTreeMap<String, i64>,
    #[serde(default)]
    nudge: Nudge,
    /// Meetings' briefs, by event key.
    #[serde(default)]
    briefs: BTreeMap<String, brief::Brief>,
    /// Busy time of your own (events nobody else is on): only for Chat's
    /// calendar tool ("am I free at 3?"), never a reminder or a note.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    busy: Vec<Busy>,
}

fn store_path() -> std::path::PathBuf {
    crate::config::data_dir().join("calendar.json")
}

fn load_store() -> Store {
    let path = store_path();
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_else(|e| {
            tracing::error!("calendar.json failed to parse: {e}");
            crate::config::quarantine_corrupt(&path);
            Store::default()
        }),
        Err(_) => Store::default(),
    }
}

/// calendar.json's saves (`saving.rs`): a failed one keeps the calendar in
/// memory, is tried again with the next save (the next sync), and is said
/// once — by the command that hit it ([`saved`]) or the notice. File I/O
/// only, so it's fine under the runtime's lock.
pub static SAVER: crate::shell::saving::Saver = crate::shell::saving::Saver::new("calendar", "your calendar", save_now);

/// `Ok` when the calendar is on disk; else why not (the caller says it).
/// Never under the runtime's lock.
pub fn saved() -> Result<(), crate::shell::saving::SaveError> {
    SAVER.check()
}

/// Write the store again (Try again). Only once `init` loaded it: never an
/// empty one over calendar.json.
fn save_now() -> Result<(), crate::shell::saving::SaveError> {
    if let Some(s) = lock_started() {
        save_store(&s.store);
    }
    saved()
}

fn save_store(store: &Store) {
    let _ = SAVER.write_json(&store_path(), store, false);
}

// ---- runtime state -----------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum CardKind {
    /// "Design review · In 1 min": take notes?
    Remind,
    /// Recording another meeting: switch notes to this one?
    Switch,
}

/// Where a card shows besides the main window's toast (which follows the
/// snapshot, so it's there whenever the window is).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Surface {
    /// Nowhere else: the main window was focused.
    #[default]
    InApp,
    /// A card on the Yap bar (`bar::show_card`).
    Bar,
    /// A Windows notification: the bar is off or hidden.
    Notification,
}

/// The card on screen.
#[derive(Debug, Clone)]
struct Card {
    id: u64,
    kind: CardKind,
    event: Event,
    surface: Surface,
    /// The bar card's status line as last shown ("In 1 min · with Tanay").
    bar_status: String,
}

#[derive(Default)]
struct Runtime {
    /// `init` ran: the store is loaded and the scheduler going.
    started: bool,
    store: Store,
    syncing: bool,
    /// When the next sync is due (unix seconds).
    next_sync: i64,
    /// A Google sign-in in the browser, by attempt number (0 = none).
    google_attempt: u64,
    google_waiting: bool,
    /// Access tokens by connection: (token, expires at).
    access: HashMap<String, (String, i64)>,
    card: Option<Card>,
    /// Snoozed reminders: until when (unix seconds).
    snoozed: HashMap<String, i64>,
    /// A call started during this meeting while another one records: offer
    /// to switch notes to it (its key).
    switch_to: Option<String>,
    seq: u64,
    /// Test runs: the links Yap would have opened.
    opened: Vec<String>,
    /// The brief whose summary is being written (one at a time)…
    brief_job: Option<String>,
    /// …the ones waiting their turn…
    brief_waiting: Vec<String>,
    /// …and ones not tried again by themselves before (unix seconds).
    brief_retry: HashMap<String, i64>,
    /// Notes, memory or settings changed: the due briefs may be stale.
    briefs_dirty: bool,
    /// "Prepare a brief before meetings" as the pages last heard it.
    briefs_on: Option<bool>,
}

static STATE: LazyLock<Mutex<Runtime>> = LazyLock::new(|| Mutex::new(Runtime::default()));
/// Wakes the scheduler (settings saved, a sync or an answer to act on).
static WAKE: LazyLock<tokio::sync::Notify> = LazyLock::new(tokio::sync::Notify::new);

/// Never held while touching windows, WinRT, the network or the notes
/// store's own lock: window getters wait on the main thread.
fn lock() -> MutexGuard<'static, Runtime> {
    STATE.lock().unwrap_or_else(|p| p.into_inner())
}

/// The state, once `init` has run (never in unit tests of other modules,
/// so they never read the real calendar.json).
fn lock_started() -> Option<MutexGuard<'static, Runtime>> {
    let s = lock();
    s.started.then_some(s)
}

fn wake() {
    WAKE.notify_one();
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// "Notify before scheduled meetings start", in seconds (`None`: never).
fn reminder_lead(setting: &str) -> Option<i64> {
    match setting {
        "never" => None,
        "1min" => Some(60),
        "2min" => Some(120),
        _ => Some(15),
    }
}

// ---- the snapshot ------------------------------------------------------------------------

/// What the pages render (`calendar_status` + the `yap-calendar` event).
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    connections: Vec<ConnectionView>,
    /// Meetings that haven't ended, soonest first (at most [`MAX_SHOWN`]).
    events: Vec<EventView>,
    syncing: bool,
    google: GoogleView,
    card: Option<CardView>,
    nudge: NudgeView,
    max_connections: u8,
    /// "Prepare a brief before meetings" is on.
    briefs: bool,
    /// Busy time of your own in the next day, times only (no titles): the
    /// Meetings view's "45 min free" between meetings leaves it out.
    busy: Vec<BusyView>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BusyView {
    start: i64,
    end: i64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectionView {
    id: String,
    kind: String,
    label: String,
    detail: String,
    synced_ts: i64,
    error: Option<String>,
    reconnect: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct EventView {
    key: String,
    title: String,
    start: i64,
    end: i64,
    attendees: Vec<String>,
    /// "Tanay Kothari, Priya Shah +2".
    with: String,
    service: Option<String>,
    /// "Teams", "Google Meet"…
    service_label: Option<&'static str>,
    has_link: bool,
    /// "Maybe".
    tentative: bool,
    /// Overlaps another meeting.
    conflict: bool,
    /// The note made for it, if any.
    note_id: Option<u64>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct GoogleView {
    /// This build has an OAuth client.
    available: bool,
    /// A sign-in is waiting in the browser.
    waiting: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CardView {
    id: u64,
    kind: CardKind,
    /// Shown as the main window's toast (the person is looking at it).
    /// Otherwise it's on the Yap bar or a Windows notification, and the
    /// window doesn't repeat it (one place: `notice.rs`).
    in_app: bool,
    key: String,
    title: String,
    start: i64,
    end: i64,
    with: String,
    service_label: Option<&'static str>,
    has_link: bool,
    /// Its meeting's brief, once made (briefs on).
    brief: Option<brief::Brief>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct NudgeView {
    /// The one-time nudge after a meeting.
    after_meeting: bool,
}

fn event_view(e: &Event, conflicts: &HashSet<String>, notes: &HashMap<String, u64>) -> EventView {
    EventView {
        key: e.key.clone(),
        title: e.display_title(),
        start: e.start,
        end: e.end,
        attendees: e.attendees.iter().map(|a| a.name.clone()).collect(),
        with: e.with_line(),
        service: e.service.clone(),
        service_label: e.service_label(),
        has_link: e.join_url.is_some(),
        tentative: e.tentative,
        conflict: conflicts.contains(&e.key),
        note_id: notes.get(&e.key).copied(),
    }
}

fn snapshot_of(s: &Runtime, notes: &HashMap<String, u64>, now: i64, briefs: bool) -> Snapshot {
    let mut upcoming: Vec<&Event> = s.store.events.iter().filter(|e| e.end > now && e.start < now + LOOK_AHEAD).collect();
    upcoming.sort_by_key(|e| (e.start, e.key.clone()));
    upcoming.truncate(MAX_SHOWN);
    let owned: Vec<Event> = upcoming.iter().map(|e| (*e).clone()).collect();
    let conflicts = model::conflicts(&owned);
    let connected = !s.store.connections.is_empty();
    Snapshot {
        connections: s
            .store
            .connections
            .iter()
            .map(|c| ConnectionView {
                id: c.id.clone(),
                kind: c.kind.clone(),
                label: c.label.clone(),
                detail: c.detail.clone(),
                synced_ts: c.synced_ts,
                error: c.error.clone(),
                reconnect: c.reconnect,
            })
            .collect(),
        events: upcoming.iter().map(|e| event_view(e, &conflicts, notes)).collect(),
        syncing: s.syncing,
        google: GoogleView { available: google::client().is_some(), waiting: s.google_waiting },
        card: s.card.as_ref().map(|c| CardView {
            id: c.id,
            kind: c.kind,
            in_app: c.surface == Surface::InApp,
            key: c.event.key.clone(),
            title: c.event.display_title(),
            start: c.event.start,
            end: c.event.end,
            with: c.event.with_line(),
            service_label: c.event.service_label(),
            has_link: c.event.join_url.is_some(),
            brief: s.store.briefs.get(&c.event.key).filter(|_| briefs).cloned(),
        }),
        nudge: NudgeView {
            // A tip: not with tips switched off (Settings → Notifications).
            after_meeting: !connected
                && !s.store.nudge.dismissed
                && !s.store.nudge.shown_after_meeting
                && !crate::notices::notice::tips_off(),
        },
        max_connections: vault::SLOTS,
        briefs,
        busy: s
            .store
            .busy
            .iter()
            .filter(|b| b.end > now && b.start < now + 86_400)
            .map(|b| BusyView { start: b.start, end: b.end })
            .collect(),
    }
}

/// The notes made for events: event key → note id.
fn note_links() -> HashMap<String, u64> {
    crate::notes::event_links().into_iter().map(|(id, key)| (key, id)).collect()
}

pub fn status() -> Snapshot {
    let notes = note_links();
    let briefs = crate::config::load().meeting_brief;
    snapshot_of(&lock(), &notes, now_secs(), briefs)
}

fn emit(app: &AppHandle) {
    let _ = app.emit(EVENT, status());
}

// ---- for Chat's calendar tool --------------------------------------------------------------

/// What Chat's calendar tool reads (`chat/tools/calendar.rs`): the meetings
/// and busy time Yap keeps (from [`LOOK_BACK`] ago to [`LOOK_AHEAD`] on), as
/// they are now: no sync, no network, nothing touched under the lock.
#[derive(Debug, Clone, Default)]
pub struct Agenda {
    /// A calendar is connected.
    pub connected: bool,
    pub events: Vec<Event>,
    pub busy: Vec<Busy>,
    /// The notes made for meetings: event key → note id.
    pub notes: HashMap<String, u64>,
    /// The last good sync of any calendar (unix seconds; 0 = never).
    pub synced_ts: i64,
}

/// A calendar is connected (Chat offers its calendar tool). Cheap: no copy.
pub fn connected() -> bool {
    #[cfg(debug_assertions)]
    if let Some(a) = SEEDED.lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
        return a.connected;
    }
    lock_started().is_some_and(|s| !s.store.connections.is_empty())
}

/// The calendar as Chat sees it ([`Agenda`]). Before `init` (a unit test,
/// a model evaluation): nothing connected, unless an evaluation seeded one.
pub fn agenda() -> Agenda {
    #[cfg(debug_assertions)]
    if let Some(a) = SEEDED.lock().unwrap_or_else(|p| p.into_inner()).clone() {
        return a;
    }
    let (connected, events, busy, synced_ts) = match lock_started() {
        Some(s) => (
            !s.store.connections.is_empty(),
            s.store.events.clone(),
            s.store.busy.clone(),
            s.store.connections.iter().map(|c| c.synced_ts).max().unwrap_or(0),
        ),
        None => (false, Vec::new(), Vec::new(), 0),
    };
    // The notes store, never under the calendar's lock.
    let notes = if events.is_empty() { HashMap::new() } else { note_links() };
    Agenda { connected, events, busy, notes, synced_ts }
}

/// A model evaluation's calendar (`eval/scenario.rs` `seed.calendar`), in
/// place of the real one; `None` takes it away. Debug builds.
#[cfg(debug_assertions)]
static SEEDED: Mutex<Option<Agenda>> = Mutex::new(None);

#[cfg(debug_assertions)]
pub fn seed(agenda: Option<Agenda>) {
    *SEEDED.lock().unwrap_or_else(|p| p.into_inner()) = agenda;
}

/// Meetings in `events` that overlap another of them (by key).
pub(crate) fn conflicts_of(events: &[Event]) -> HashSet<String> {
    model::conflicts(events)
}

/// How far back and ahead the calendar is kept (seconds): what Chat's
/// calendar tool can see.
pub const KEPT_BACK: i64 = LOOK_BACK;
pub const KEPT_AHEAD: i64 = LOOK_AHEAD;

// ---- startup and the scheduler -------------------------------------------------------------

/// Load calendar.json and start syncing and reminding (app setup).
pub fn init(app: &AppHandle) {
    {
        let mut s = lock();
        if s.started {
            return;
        }
        s.store = load_store();
        let now = now_secs();
        s.store.answered.retain(|_, at| *at > now - ANSWER_KEEP);
        s.store.events.retain(|e| e.end > now - LOOK_BACK);
        s.store.busy.retain(|b| b.end > now - LOOK_BACK);
        prune_briefs(&mut s.store, now);
        // A summary a quit left half-written: the brief is made again when
        // it's due or asked for.
        for b in s.store.briefs.values_mut() {
            b.writing = false;
        }
        s.started = true;
        // A first sync straight away (it's cheap, and the cache may be old).
        s.next_sync = now;
    }
    // A recording starting or stopping changes which card is due.
    app.listen("yap-meeting-state", |_| wake());
    // What a brief is made from changed: the due ones are looked at again at
    // the scheduler's next turn (not at once: a recording changes its note
    // often).
    for event in ["yap-notes-changed", "yap-note-changed", "yap-memory-changed"] {
        app.listen(event, |_| briefs_changed());
    }
    let handle = app.clone();
    tauri::async_runtime::spawn(async move { scheduler(handle).await });
    tracing::info!("calendar: started");
}

async fn scheduler(app: AppHandle) {
    let mut last_wall = now_secs();
    loop {
        let now = now_secs();
        // The loop wakes at least every MAX_SLEEP seconds, so a bigger jump
        // in the wall clock means the PC slept: catch up at once.
        let slept = now - last_wall > 3 * MAX_SLEEP as i64;
        last_wall = now;
        let due = {
            let mut s = lock();
            if slept {
                s.next_sync = now;
            }
            !s.store.connections.is_empty() && !s.syncing && now >= s.next_sync
        };
        if due {
            sync_all(&app).await;
        }
        tick_card(&app);
        tick_briefs(&app);
        tick_tray(&app);
        let wait = next_wake(now_secs());
        tokio::select! {
            _ = WAKE.notified() => {}
            _ = tokio::time::sleep(Duration::from_secs(wait)) => {}
        }
    }
}

/// Seconds until the scheduler has something to do: a sync, a reminder
/// coming due, a card running out or a snooze ending; at most [`MAX_SLEEP`].
fn next_wake(now: i64) -> u64 {
    let cfg = crate::config::load();
    let lead = reminder_lead(&cfg.meeting_reminder);
    let s = lock();
    let mut next = now + MAX_SLEEP as i64;
    if !s.store.connections.is_empty() {
        next = next.min(s.next_sync);
    }
    if let Some(t) = lead.and_then(|lead| model::next_reminder_at(&s.store.events, now, lead, &s.snoozed)) {
        next = next.min(t);
    }
    if let Some(card) = &s.card {
        next = next.min(card.event.start + model::REMINDER_AFTER_SECS).min(now + CARD_TICK);
    }
    if cfg.meeting_brief {
        if let Some(t) = s.store.events.iter().map(|e| e.start - BRIEF_AHEAD).filter(|t| *t > now).min() {
            next = next.min(t);
        }
    }
    (next - now).clamp(1, MAX_SLEEP as i64) as u64
}

/// Settings were saved: the reminder lead may have changed, and briefs been
/// switched on or off or given another model.
pub fn on_config_saved() {
    briefs_changed();
    wake();
}

/// The main window got the focus: a card on the bar (or in the
/// notification center) moves into it.
pub fn on_main_window_focused() {
    wake();
}

// ---- syncing ------------------------------------------------------------------------------

static HTTP: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(format!("Yap/{} (Windows)", env!("CARGO_PKG_VERSION")))
        .build()
        .unwrap_or_default()
});

/// Fetch every connection and replace its meetings. A connection that fails
/// keeps the meetings it had, and says why.
async fn sync_all(app: &AppHandle) {
    let connections = {
        let mut s = lock();
        if s.syncing {
            return;
        }
        s.syncing = true;
        s.store.connections.clone()
    };
    emit(app);
    let now = now_secs();
    let (from, to) = (now - LOOK_BACK, now + LOOK_AHEAD + 3600);
    let mut results = Vec::new();
    for conn in &connections {
        let result = fetch(conn, from, to).await;
        if let Err(e) = &result {
            tracing::info!(connection = %conn.id, kind = %conn.kind, "calendar: sync failed: {}", e.message);
        }
        results.push((conn.id.clone(), conn.added_ts, result));
    }
    {
        let mut s = lock();
        for (id, added_ts, result) in results {
            // The calendar fetched, not another connected into its slot
            // since (its meetings would show under the new one).
            let Some(conn) = s.store.connections.iter_mut().find(|c| c.id == id && c.added_ts == added_ts) else { continue };
            match result {
                Ok(fetched) => {
                    conn.synced_ts = now;
                    conn.error = None;
                    conn.reconnect = false;
                    s.store.events.retain(|e| e.connection != id);
                    s.store.events.extend(fetched.events);
                    s.store.busy.retain(|b| b.connection != id);
                    s.store.busy.extend(fetched.busy);
                }
                Err(e) => {
                    conn.error = Some(e.message);
                    conn.reconnect |= e.reconnect;
                }
            }
        }
        s.store.events.sort_by_key(|e| (e.start, e.key.clone()));
        s.store.busy.sort_by_key(|b| (b.start, b.key.clone()));
        s.store.answered.retain(|_, at| *at > now - ANSWER_KEEP);
        s.syncing = false;
        // A calendar connected while this sync ran gets its own straight away.
        let added = s.store.connections.iter().any(|c| !connections.iter().any(|o| o.id == c.id && o.added_ts == c.added_ts));
        s.next_sync = if added { now_secs() } else { now_secs() + SYNC_EVERY };
        save_store(&s.store);
        tracing::info!(events = s.store.events.len(), "calendar: synced");
    }
    emit(app);
    wake();
}

/// Why a fetch failed: what to show, and whether only reconnecting helps.
#[derive(Debug)]
struct FetchError {
    message: String,
    reconnect: bool,
}

impl From<String> for FetchError {
    fn from(message: String) -> Self {
        FetchError { message, reconnect: false }
    }
}

/// A calendar's meetings, and the busy time of your own beside them.
#[derive(Debug, Default)]
struct Fetched {
    events: Vec<Event>,
    busy: Vec<Busy>,
}

impl Fetched {
    /// Drafts from a source, sorted ([`model::sort`]); one key once.
    fn from_drafts(drafts: impl IntoIterator<Item = model::Draft>) -> Fetched {
        let mut seen = HashSet::new();
        let mut out = Fetched::default();
        for d in drafts {
            if !seen.insert(d.key.clone()) {
                continue;
            }
            match model::sort(d) {
                model::Kept::Meeting(e) => out.events.push(e),
                model::Kept::Busy(b) => out.busy.push(b),
                model::Kept::Neither => {}
            }
        }
        out
    }
}

async fn fetch(conn: &Connection, from: i64, to: i64) -> Result<Fetched, FetchError> {
    match conn.kind.as_str() {
        "google" => fetch_google(conn, from, to).await,
        _ => {
            let link = vault::load(&conn.id).ok_or_else(|| "Yap lost this calendar's link. Disconnect it and add it again.".to_string())?;
            let bytes = fetch_feed(&link).await?;
            let id = conn.id.clone();
            let parsed = tauri::async_runtime::spawn_blocking(move || events_from_feed(&id, &bytes, from, to))
                .await
                .map_err(|e| e.to_string())??;
            Ok(parsed.0)
        }
    }
}

/// A private iCal link as Yap fetches it: `webcal://` read as `https://`,
/// HTTPS only (plain HTTP just for this PC's own servers).
fn normalize_link(raw: &str) -> Result<String, String> {
    let raw = raw.trim();
    let fixed = match raw.split_once("://") {
        Some((scheme, rest)) if scheme.eq_ignore_ascii_case("webcal") || scheme.eq_ignore_ascii_case("webcals") => {
            format!("https://{rest}")
        }
        _ => raw.to_string(),
    };
    let url = url::Url::parse(&fixed).map_err(|_| "That doesn't look like a link. Paste the whole address.".to_string())?;
    let host = url.host_str().unwrap_or("").to_ascii_lowercase();
    let local = matches!(host.as_str(), "localhost" | "127.0.0.1" | "[::1]");
    match url.scheme() {
        "https" if !host.is_empty() => Ok(url.to_string()),
        "http" if local => Ok(url.to_string()),
        "http" => Err("Yap only reads calendar links over HTTPS. Use the https:// (or webcal://) address.".into()),
        _ => Err("That doesn't look like a calendar link. It should start with https:// or webcal://.".into()),
    }
}

/// The link's host, the only part of it Yap ever shows.
fn link_host(link: &str) -> String {
    url::Url::parse(link).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default()
}

async fn fetch_feed(link: &str) -> Result<Vec<u8>, String> {
    let resp = HTTP
        .get(link)
        .header(reqwest::header::ACCEPT, "text/calendar, text/plain;q=0.8, */*;q=0.5")
        .send_for(crate::privacy::net::Purpose::Calendar("iCal link"))
        .await
        .map_err(|_| "Couldn't reach the calendar. Yap tries again in a few minutes.".to_string())?;
    match resp.status().as_u16() {
        200..=299 => {}
        404 | 410 => {
            return Err("That calendar link doesn't work any more (it may have been reset). Add the new one.".into())
        }
        401 | 403 => return Err("The calendar server turned the link down. Check it's the private (secret) address.".into()),
        code => return Err(format!("The calendar server had a problem (error {code}). Yap tries again in a few minutes.")),
    }
    let mut bytes = Vec::new();
    let mut stream = resp;
    while let Some(chunk) = stream.chunk().await.map_err(|_| "The calendar stopped halfway. Yap tries again in a few minutes.".to_string())? {
        bytes.extend_from_slice(&chunk);
        if bytes.len() > MAX_FEED_BYTES {
            return Err("That calendar is too big for Yap to read.".into());
        }
    }
    Ok(bytes)
}

/// A feed's meetings (and busy time) between `from` and `to`, and the
/// calendar's name.
fn events_from_feed(connection: &str, bytes: &[u8], from: i64, to: i64) -> Result<(Fetched, Option<String>), String> {
    let calendar = ics::parse_bytes(bytes)
        .ok_or("That address didn't return a calendar. Check you copied the iCal (.ics) link.")?;
    let resolver = tz::Resolver::new(&calendar, tz::Zone::Local);
    let me = model::self_addresses(&calendar);
    let fetched = Fetched::from_drafts(ics::occurrences(&calendar, &resolver, from, to).iter().map(|o| model::draft_from_ics(connection, o, &me)));
    Ok((fetched, ics::calendar_name(&calendar)))
}

/// A Google connection's secret: the refresh token, the client it belongs
/// to and the scopes granted.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleSecret {
    refresh_token: String,
    client_id: String,
    #[serde(default)]
    scope: String,
}

async fn fetch_google(conn: &Connection, from: i64, to: i64) -> Result<Fetched, FetchError> {
    let client = google::client().ok_or_else(|| NO_GOOGLE.to_string())?;
    let secret: GoogleSecret = vault::load(&conn.id)
        .and_then(|s| serde_json::from_str(&s).ok())
        .ok_or(FetchError { message: "Yap lost this calendar's key. Connect it again.".into(), reconnect: true })?;
    if secret.client_id != client.id {
        return Err(FetchError { message: "This calendar was connected from another build of Yap. Connect it again.".into(), reconnect: true });
    }
    let can_list = secret.scope.split_whitespace().any(|s| s.ends_with("calendar.calendarlist.readonly"));
    let mut retried = false;
    loop {
        let access = access_token(conn, &client, &secret).await?;
        let result = async {
            let (_, calendars) = google::calendars(&access, can_list).await?;
            let mut items = Vec::new();
            for calendar in calendars {
                items.extend(google::events(&access, &calendar, from, to).await?);
            }
            Ok::<_, google::Failure>(items)
        }
        .await;
        match result {
            Ok(items) => {
                return Ok(Fetched::from_drafts(items.iter().filter_map(|item| google::draft_from_item(&conn.id, item))));
            }
            Err(google::Failure::Unauthorized) if !retried => {
                // An access token Google no longer takes: drop it, try once more.
                lock().access.remove(&conn.id);
                retried = true;
            }
            Err(f) => return Err(google_error(f)),
        }
    }
}

fn google_error(f: google::Failure) -> FetchError {
    let reconnect = matches!(f, google::Failure::Revoked | google::Failure::Unauthorized);
    FetchError { message: f.message(), reconnect }
}

/// A current access token for `conn` (kept in memory only), refreshed when
/// it's about to run out.
async fn access_token(conn: &Connection, client: &google::Client, secret: &GoogleSecret) -> Result<String, FetchError> {
    let now = now_secs();
    if let Some((token, until)) = lock().access.get(&conn.id) {
        if *until > now + 60 {
            return Ok(token.clone());
        }
    }
    let tokens = google::refresh(client, &secret.refresh_token).await.map_err(google_error)?;
    lock().access.insert(conn.id.clone(), (tokens.access.clone(), now + tokens.expires_in));
    Ok(tokens.access)
}

/// A free connection slot.
fn free_slot(s: &Runtime) -> Option<String> {
    (1..=vault::SLOTS).map(|n| n.to_string()).find(|id| !s.store.connections.iter().any(|c| c.id == *id))
}

/// How many of connection `id`'s meetings are on now or in the next 7 days
/// ("Calendar connected · 6 meetings in the next 7 days").
fn upcoming_count(events: &[Event], id: &str, now: i64) -> usize {
    events.iter().filter(|e| e.connection == id && e.end > now && e.start < now + LOOK_AHEAD).count()
}

// ---- connecting -------------------------------------------------------------------------------

/// Open `link` in the browser. Test runs never do: they keep it, for the
/// suite to check (`calendar_e2e_opened`).
fn open_link(app: &AppHandle, link: &str) -> Result<(), String> {
    if crate::shell::e2e::active() {
        tracing::info!("e2e: would open a calendar link");
        lock().opened.push(link.to_string());
        return Ok(());
    }
    use tauri_plugin_opener::OpenerExt;
    app.opener().open_url(link, None::<&str>).map_err(|e| format!("Couldn't open your browser: {e}"))
}

/// Connect Google Calendar: the consent page opens in the browser, and the
/// rest happens when Google hands back (`yap-calendar-connected`, or
/// `yap-calendar-error`).
#[tauri::command]
pub async fn calendar_connect_google(app: AppHandle) -> Result<(), String> {
    let client = google::client().ok_or(NO_GOOGLE)?;
    let sign_in = google::begin(&client)?;
    let attempt = {
        let mut s = lock();
        let has_google = s.store.connections.iter().any(|c| c.kind == "google");
        if !has_google && free_slot(&s).is_none() {
            return Err(TOO_MANY.into());
        }
        s.google_attempt += 1;
        s.google_waiting = true;
        s.google_attempt
    };
    if let Err(e) = open_link(&app, &sign_in.url) {
        lock().google_waiting = false;
        emit(&app);
        return Err(e);
    }
    tracing::info!("calendar: Google sign-in started");
    emit(&app);
    let handle = app.clone();
    tauri::async_runtime::spawn(async move { finish_google(handle, client, sign_in, attempt).await });
    Ok(())
}

/// Stop waiting for the Google sign-in.
#[tauri::command]
pub fn calendar_cancel_google(app: AppHandle) {
    {
        let mut s = lock();
        s.google_attempt += 1;
        s.google_waiting = false;
    }
    emit(&app);
}

async fn finish_google(app: AppHandle, client: google::Client, sign_in: google::SignIn, attempt: u64) {
    let current = move || lock().google_attempt == attempt;
    // Wait for the browser (off the async threads), as long as this attempt
    // is the current one and hasn't timed out.
    let waited = tauri::async_runtime::spawn_blocking(move || {
        let deadline = std::time::Instant::now() + google::SIGN_IN_TTL;
        loop {
            match sign_in.handback.recv_timeout(Duration::from_millis(500)) {
                Ok(h) => return (Some(h), sign_in),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if !current() || std::time::Instant::now() > deadline {
                        return (None, sign_in);
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return (None, sign_in),
            }
        }
    })
    .await;
    let Ok((handback, sign_in)) = waited else { return };
    let outcome = match handback {
        Some(google::Handback::Code(code, reply)) => {
            let (verifier, redirect_uri) = (sign_in.verifier.clone(), sign_in.redirect_uri.clone());
            let result = connect_google(&client, &code, &verifier, &redirect_uri).await;
            // The browser tab says how it went; the meetings load after.
            let _ = reply.send(result.clone().map(|(_, label)| label));
            if result.is_ok() {
                sync_all(&app).await;
            }
            result.map(Some)
        }
        Some(google::Handback::Denied(error)) => Err(if error == "access_denied" {
            "You didn't let Yap see your calendar, so it isn't connected.".to_string()
        } else {
            format!("Google didn't connect the calendar ({error}).")
        }),
        None => Ok(None), // cancelled, replaced or timed out
    };
    drop(sign_in);
    let still_current = {
        let mut s = lock();
        let current = s.google_attempt == attempt;
        if current {
            s.google_waiting = false;
        }
        current
    };
    match outcome {
        Ok(Some((id, label))) => {
            let meetings = upcoming_count(&lock().store.events, &id, now_secs());
            let _ = app.emit(EVENT_CONNECTED, json!({ "kind": "google", "label": label, "meetings": meetings }));
        }
        Err(e) if still_current => {
            tracing::info!("calendar: Google sign-in failed: {e}");
            let _ = app.emit(EVENT_ERROR, e);
        }
        _ => {}
    }
    emit(&app);
}

/// The code from the browser → tokens → the account → a connection (the
/// caller syncs it). Returns its slot and the account's address.
async fn connect_google(
    client: &google::Client,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<(String, String), String> {
    let tokens = google::exchange(client, code, verifier, redirect_uri)
        .await
        .map_err(|f| match f {
            google::Failure::Offline => "Couldn't reach Google to finish connecting. Try again.".to_string(),
            _ => "Google didn't accept the sign-in. Try connecting again.".to_string(),
        })?;
    if !tokens.can_read_events() {
        return Err("Yap needs to see the events on your calendars. Connect again and leave that box ticked.".into());
    }
    let refresh = tokens.refresh.clone().ok_or("Google didn't give Yap lasting access. Try connecting again.")?;
    let (account, _) = google::calendars(&tokens.access, tokens.can_list_calendars())
        .await
        .map_err(|f| f.message())?;
    let label = if account.is_empty() { "Google Calendar".to_string() } else { account };
    let secret = serde_json::to_string(&GoogleSecret { refresh_token: refresh, client_id: client.id.clone(), scope: tokens.scope.clone() })
        .map_err(|e| e.to_string())?;
    let now = now_secs();
    let id = {
        let mut s = lock();
        // The same account again: it takes over its old slot.
        let existing = s.store.connections.iter().find(|c| c.kind == "google" && c.label.eq_ignore_ascii_case(&label)).map(|c| c.id.clone());
        let id = existing.or_else(|| free_slot(&s)).ok_or(TOO_MANY)?;
        vault::save(&id, &secret)?;
        s.access.insert(id.clone(), (tokens.access.clone(), now + tokens.expires_in));
        s.store.connections.retain(|c| c.id != id);
        s.store.connections.push(Connection {
            id: id.clone(),
            kind: "google".into(),
            label: label.clone(),
            detail: "Google Calendar".into(),
            added_ts: now,
            synced_ts: 0,
            error: None,
            reconnect: false,
        });
        s.next_sync = now;
        save_store(&s.store);
        id
    };
    tracing::info!(connection = %id, "calendar: Google Calendar connected");
    Ok((id, label))
}

/// Add a calendar by its private iCal link (`kind`: "outlook" from the
/// Outlook card, else "ics"). The link is fetched once first, so a wrong one
/// is caught here, in the form. Returns what was connected, as the
/// `yap-calendar-connected` event says it: `{ kind, label, meetings }`.
#[tauri::command]
pub async fn calendar_add_link(app: AppHandle, link: String, kind: Option<String>) -> Result<Value, String> {
    let link = normalize_link(&link)?;
    let kind = if kind.as_deref() == Some("outlook") { "outlook" } else { "ics" };
    let (id, already) = {
        let s = lock();
        let ids: Vec<String> = s.store.connections.iter().filter(|c| c.kind != "google").map(|c| c.id.clone()).collect();
        (free_slot(&s), ids)
    };
    if already.iter().any(|other| vault::load(other).as_deref() == Some(link.as_str())) {
        return Err("That calendar is already connected.".into());
    }
    let id = id.ok_or(TOO_MANY)?;
    let bytes = fetch_feed(&link).await?;
    let now = now_secs();
    let (from, to) = (now - LOOK_BACK, now + LOOK_AHEAD + 3600);
    let slot = id.clone();
    let (fetched, name) = tauri::async_runtime::spawn_blocking(move || events_from_feed(&slot, &bytes, from, to))
        .await
        .map_err(|e| e.to_string())??;
    vault::save(&id, &link)?;
    let host = link_host(&link);
    let label = match name.filter(|n| !n.eq_ignore_ascii_case("calendar")) {
        Some(name) => name,
        None if kind == "outlook" => "Outlook calendar".to_string(),
        None => "Calendar".to_string(),
    };
    let meetings = {
        let mut s = lock();
        s.store.connections.push(Connection {
            id: id.clone(),
            kind: kind.to_string(),
            label: label.clone(),
            detail: host,
            added_ts: now,
            synced_ts: now,
            error: None,
            reconnect: false,
        });
        s.store.events.retain(|e| e.connection != id);
        s.store.events.extend(fetched.events);
        s.store.events.sort_by_key(|e| (e.start, e.key.clone()));
        s.store.busy.retain(|b| b.connection != id);
        s.store.busy.extend(fetched.busy);
        s.store.busy.sort_by_key(|b| (b.start, b.key.clone()));
        s.next_sync = now + SYNC_EVERY;
        save_store(&s.store);
        upcoming_count(&s.store.events, &id, now)
    };
    tracing::info!(connection = %id, kind, meetings, "calendar: calendar link added");
    let connected = json!({ "kind": kind, "label": label, "meetings": meetings });
    let _ = app.emit(EVENT_CONNECTED, connected.clone());
    emit(&app);
    wake();
    // Connected, but calendar.json couldn't be written: said by the page
    // ("Couldn't save your calendar: …", Try again), never as if it saved.
    saved()?;
    Ok(connected)
}

/// Disconnect a calendar: its meetings, its secret and (Google) Yap's
/// access go. Notes made from its meetings keep what they copied.
#[tauri::command]
pub async fn calendar_disconnect(app: AppHandle, id: String) -> Result<(), String> {
    let conn = {
        let mut s = lock();
        let pos = s.store.connections.iter().position(|c| c.id == id).ok_or("That calendar isn't connected.")?;
        let conn = s.store.connections.remove(pos);
        s.store.events.retain(|e| e.connection != id);
        s.store.busy.retain(|b| b.connection != id);
        s.access.remove(&id);
        save_store(&s.store);
        conn
    };
    // Its secret goes before Google is told: the slot is free already, and a
    // calendar connected into it while the revoke waits on Google (up to its
    // timeout) saves its own secret there, which a delete after the wait
    // took (the 2026-10-23 review).
    let secret = (conn.kind == "google")
        .then(|| vault::load(&id).and_then(|s| serde_json::from_str::<GoogleSecret>(&s).ok()))
        .flatten();
    vault::delete(&id);
    if let Some(secret) = secret {
        google::revoke(&secret.refresh_token).await;
    }
    tracing::info!(connection = %id, kind = %conn.kind, "calendar: disconnected");
    emit(&app);
    wake();
    // Disconnected, but calendar.json still lists it until Yap can save:
    // the page says so, with Try again.
    saved()?;
    Ok(())
}

/// Sync now ("Sync calendar"); returns once it's done.
#[tauri::command]
pub async fn calendar_sync(app: AppHandle) -> Snapshot {
    if lock_started().is_some_and(|s| !s.store.connections.is_empty()) {
        sync_all(&app).await;
    }
    status()
}

#[tauri::command]
pub fn calendar_status() -> Snapshot {
    status()
}

// ---- meetings into notes ----------------------------------------------------------------------

fn note_event(e: &Event) -> crate::notes::NoteEvent {
    crate::notes::NoteEvent {
        key: e.key.clone(),
        title: e.display_title(),
        start: e.start.max(0) as u64,
        end: e.end.max(0) as u64,
        description: e.description.clone(),
        service: e.service.clone().unwrap_or_default(),
    }
}

/// Everyone invited but you (no rooms, no declines), as (name, email): the
/// note's attendees, and for a 1:1 call the guess for "Them".
fn attendees_of(e: &Event) -> Vec<(String, String)> {
    e.attendees.iter().map(|a| (a.name.clone(), a.email.clone())).collect()
}

/// The note for `event`: the one made before, or a new meeting note with its
/// title and attendees (dated to the meeting when made ahead of it).
fn note_for(event: &Event, ahead: bool) -> Result<(u64, bool), String> {
    if let Some(id) = crate::notes::find_by_event(&event.key) {
        return Ok((id, false));
    }
    // Its series' folder ("Always file Weekly sync in Design"), else Meetings.
    let folder = filing::folder_for(&event.key).unwrap_or_else(|| {
        crate::notes::folder_create(crate::notes::MEETINGS);
        crate::notes::MEETINGS.to_string()
    });
    let note = crate::notes::create(&event.display_title(), "", "calendar", &folder);
    let date = ahead.then_some(event.start.max(0) as u64);
    crate::notes::link_event(note.id, note_event(event), None, &attendees_of(event), date)?;
    Ok((note.id, true))
}

fn event_by_key(key: &str) -> Result<Event, String> {
    lock().store.events.iter().find(|e| e.key == key).cloned().ok_or_else(|| "That meeting isn't on your calendar any more.".to_string())
}

/// Note the answer to `event`'s reminder (and take its card down, at the
/// next tick).
fn answer(event: &Event) {
    let mut s = lock();
    s.store.answered.insert(model::answer_key(event), now_secs());
    s.snoozed.remove(&model::answer_key(event));
    if s.switch_to.as_deref() == Some(event.key.as_str()) {
        s.switch_to = None;
    }
    save_store(&s.store);
    drop(s);
    wake();
}

/// The main window opens `note_id` in Notes if it's on screen; otherwise a
/// Windows notification says notes are being taken.
fn show_note(app: &AppHandle, note_id: u64, title: &str) {
    if window_view(app).0 {
        let _ = app.emit(EVENT_OPEN, json!({ "noteId": note_id, "stop": false }));
    } else {
        let n = crate::calls::meeting_guard::Notice {
            kind: "recording",
            title: format!("Taking notes on {title}"),
            body: "Yap is recording. Let people know you're taking notes.".to_string(),
            icon: "call",
            variant: "success",
            settings: None,
            note_id: Some(note_id),
            surface: None,
        };
        crate::calls::meeting_guard::notice_native(app, &n);
    }
}

/// Take notes on `key` (and join it first, `join`): its note, recording.
fn start_event(app: &AppHandle, key: &str, join: bool) -> Result<u64, String> {
    let event = event_by_key(key)?;
    if let Some(recording) = crate::meetings::meeting::recording_note() {
        if crate::notes::find_by_event(key) == Some(recording) {
            if join {
                join_link(app, &event)?;
            }
            show_note(app, recording, &event.display_title());
            return Ok(recording);
        }
        return Err("Yap is already recording a meeting. End it first, or switch notes to this one.".into());
    }
    if join {
        join_link(app, &event)?;
    }
    let (note_id, made) = note_for(&event, false)?;
    let started = crate::meetings::commands::meeting_start(app.clone(), app.state::<crate::AppState>(), note_id);
    if let Err(e) = started {
        if made {
            crate::notes::delete(note_id);
        }
        let _ = app.emit("yap-notes-changed", ());
        return Err(e);
    }
    answer(&event);
    let _ = app.emit("yap-notes-changed", ());
    tracing::info!(note_id, join, "calendar: taking notes on a meeting");
    show_note(app, note_id, &event.display_title());
    Ok(note_id)
}

/// Back-to-back meetings: end the one recording (`meeting_end` writes its
/// action plan, as for any meeting that ends), then take notes on `key`.
async fn switch_to_event(app: &AppHandle, key: &str, join: bool) -> Result<u64, String> {
    let event = event_by_key(key)?;
    if join {
        join_link(app, &event)?;
    }
    if let Some(current) = crate::meetings::meeting::recording_note() {
        if crate::notes::find_by_event(key) == Some(current) {
            return Ok(current);
        }
        tracing::info!(note_id = current, "calendar: switching notes to the next meeting");
        crate::meetings::meeting_end::end(app, Some("calendar"))?;
        // The recorder transcribes its last chunk before it lets go.
        for _ in 0..240 {
            if crate::meetings::meeting::recording_note().is_none() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        if crate::meetings::meeting::recording_note().is_some() {
            return Err("The last meeting's notes are still finishing. Try again in a moment.".into());
        }
    }
    let (handle, key) = (app.clone(), key.to_string());
    tauri::async_runtime::spawn_blocking(move || start_event(&handle, &key, false))
        .await
        .map_err(|e| e.to_string())?
}

/// Open `event`'s join link.
fn join_link(app: &AppHandle, event: &Event) -> Result<(), String> {
    let link = event.join_url.as_deref().ok_or("This meeting has no link to join.")?;
    open_link(app, link)
}

/// A row in the Meetings view: "open" (its note: the draft made ahead of
/// the meeting, or the one recorded), "start", "joinStart", "join" (open the
/// link only), "switch" or "joinSwitch". Returns the note's id.
#[tauri::command]
pub async fn calendar_event(app: AppHandle, key: String, action: String) -> Result<Option<u64>, String> {
    match action.as_str() {
        "open" => {
            let event = event_by_key(&key)?;
            let ahead = event.start > now_secs();
            let (note_id, made) = tauri::async_runtime::spawn_blocking(move || note_for(&event, ahead))
                .await
                .map_err(|e| e.to_string())??;
            if made {
                let _ = app.emit("yap-notes-changed", ());
            }
            emit(&app);
            Ok(Some(note_id))
        }
        "join" => {
            let event = event_by_key(&key)?;
            join_link(&app, &event)?;
            // Joined without notes: call detection won't ask about it either.
            answer(&event);
            emit(&app);
            Ok(None)
        }
        "start" | "joinStart" => {
            let join = action == "joinStart";
            let handle = app.clone();
            let id = tauri::async_runtime::spawn_blocking(move || start_event(&handle, &key, join))
                .await
                .map_err(|e| e.to_string())??;
            emit(&app);
            Ok(Some(id))
        }
        "switch" | "joinSwitch" => {
            let id = switch_to_event(&app, &key, action == "joinSwitch").await?;
            emit(&app);
            Ok(Some(id))
        }
        other => Err(format!("Unknown action: {other}")),
    }
}

/// A recording started into `note_id`, whichever way (`meetings::commands::meeting_start`).
/// During a meeting, the note becomes that meeting's: its title (over a
/// placeholder), attendees and invite. A note already made from the
/// calendar just marks its meeting's reminder answered.
pub fn on_meeting_started(app: &AppHandle, note_id: u64) {
    if lock_started().is_none() {
        return;
    }
    let Some(note) = crate::notes::get(note_id) else { return };
    let now = now_secs();
    if let Some(linked) = &note.event {
        let event = lock().store.events.iter().find(|e| e.key == linked.key).cloned();
        if let Some(event) = event {
            answer(&event);
        }
        return;
    }
    // A call in progress says which meeting it is (its service).
    let calls: Vec<String> = serde_json::to_value(crate::calls::meeting_detect::status())
        .ok()
        .and_then(|v| v.get("calls").cloned())
        .and_then(|c| serde_json::from_value::<Vec<Value>>(c).ok())
        .unwrap_or_default()
        .iter()
        .filter_map(|c| c.get("app").and_then(Value::as_str).map(str::to_string))
        .collect();
    // …and its join code, read from its window as it started.
    let codes: Vec<Option<String>> = calls.iter().map(|app_id| call_code(app_id)).collect();
    let (event, by_code) = {
        let s = lock();
        let found = calls
            .iter()
            .zip(&codes)
            .rev()
            .find_map(|(app_id, code)| model::event_for_call(&s.store.events, now, Some(app_id), code.as_deref()))
            // Any meeting on now — but still never one whose code the call
            // showed is another's.
            .or_else(|| model::event_for_call(&s.store.events, now, None, codes.iter().rev().flatten().next().map(String::as_str)))
            .cloned();
        let by_code = found.as_ref().is_some_and(|e| codes.iter().flatten().any(|c| e.codes.contains(c)));
        (found, by_code)
    };
    let Some(event) = event else { return };
    // Resuming an older meeting's notes isn't this meeting (a code match
    // reaches further either side).
    if let Some(first) = note.transcript.first().map(|s| s.ts as i64) {
        let (early, late) = if by_code { (model::CODE_EARLY_SECS, model::CODE_LATE_SECS) } else { (model::EARLY_SECS, 0) };
        if first < event.start - early || first > event.end + late {
            return;
        }
    }
    // Over a title Yap made up ("Teams call · 5 Oct, 14:30"): the meeting's
    // real name, which the AI meeting title then leaves alone.
    let made_up = crate::notes::title_open_to_ai(&note) || model::is_placeholder_title(&note.title);
    let title = made_up.then(|| event.display_title());
    if let Err(e) = crate::notes::link_event(note_id, note_event(&event), title, &attendees_of(&event), None) {
        tracing::warn!("calendar: couldn't tie the note to its meeting: {e}");
        return;
    }
    answer(&event);
    tracing::info!(note_id, by_code, "calendar: a recording started during a meeting; note tied to it");
    // Its series' folder, when there's a rule for it.
    filing::file_linked(note_id, &event.key);
    // Every window showing the note (the notepad, Notes) follows.
    crate::notes::commands::note_changed(app, note_id, "calendar");
    let _ = app.emit("yap-notes-changed", ());
    emit(app);
}

/// Call detection, about to ask "Teams call detected — Record notes?" (under
/// its own lock: no windows, no other locks but this one). `true` when the
/// call belongs to a meeting whose card is up or was answered: the calendar
/// asked already. A snoozed card comes back instead.
pub fn claims_call(app_id: &str) -> bool {
    let code = call_code(app_id);
    let Some(mut s) = lock_started() else { return false };
    let now = now_secs();
    let Some(event) = model::event_for_call(&s.store.events, now, Some(app_id), code.as_deref()).cloned() else {
        return false;
    };
    let key = model::answer_key(&event);
    let showing = s.card.as_ref().is_some_and(|c| c.event.key == event.key);
    let answered = s.store.answered.contains_key(&key);
    let snoozed = s.snoozed.remove(&key).is_some();
    drop(s);
    if snoozed {
        wake();
    }
    let claimed = showing || answered || snoozed;
    if claimed {
        tracing::info!(app = app_id, "calendar: the call is a meeting the calendar asked about already");
    }
    claimed
}

/// Call detection, about to carry a call of `app_id` on in note `note_id`
/// (it came back soon after Yap stopped that note's recording by itself, or
/// the PC slept): whether the calendar says the call is another meeting
/// (`model::another_meeting`: the next of two back to back on the same app,
/// or the note's meeting long over). Then it's asked about instead. Reads
/// the notes: never under call detection's lock.
pub fn call_is_another_meeting(app_id: &str, note_id: u64) -> bool {
    let Some(linked) = crate::notes::event_of(note_id) else { return false };
    let code = call_code(app_id);
    let now = now_secs();
    let end = linked.end as i64;
    let another = match lock_started() {
        Some(s) => model::another_meeting(&s.store.events, now, Some(app_id), code.as_deref(), &linked.key, end),
        None => model::another_meeting(&[], now, Some(app_id), code.as_deref(), &linked.key, end),
    };
    if another {
        tracing::info!(app = app_id, note_id, "calendar: the call that came back is another meeting");
    }
    another
}

/// A call started (after call detection's lock is released). Recording an
/// earlier meeting's notes while this call belongs to the next meeting:
/// offer to switch notes (back-to-back meetings). Whether it did (call
/// detection's own "Switch to …?" then stays quiet).
pub fn on_call_started(app_id: &str) -> bool {
    if lock_started().is_none() {
        return false;
    }
    let Some(recording) = crate::meetings::meeting::recording_note() else { return false };
    let recording_key = crate::notes::get(recording).and_then(|n| n.event).map(|e| e.key);
    let code = call_code(app_id);
    let mut s = lock();
    let now = now_secs();
    let Some(next) = model::event_for_call(&s.store.events, now, Some(app_id), code.as_deref()).cloned() else { return false };
    if recording_key.as_deref() == Some(next.key.as_str()) {
        return false; // the same meeting: a rejoin
    }
    if let Some(previous) = recording_key.as_deref().and_then(|k| s.store.events.iter().find(|e| e.key == k)) {
        if previous.start >= next.start {
            return false; // only ever forward, to the next meeting
        }
    }
    if s.store.answered.contains_key(&model::answer_key(&next)) {
        return false;
    }
    tracing::info!(app = app_id, "calendar: a call during the next meeting; offering to switch notes");
    s.switch_to = Some(next.key);
    drop(s);
    wake();
    true
}

// ---- join codes ---------------------------------------------------------------------------------

/// Join codes read from calls' windows as each started ([`read_call_code`]),
/// by call app: (code, when read). Its own small lock: [`claims_call`] reads
/// it under call detection's.
static CALL_CODES: LazyLock<Mutex<HashMap<String, (String, i64)>>> = LazyLock::new(Default::default);
/// A code read this long ago is another call's.
const CALL_CODE_KEEP: i64 = 6 * 3600;

/// A call of `app_id` started (call detection, before it takes its own
/// lock): read its join code from its windows (`call_windows.rs`) for the
/// matching that follows ([`claims_call`], [`on_call_started`],
/// [`on_meeting_started`]). Only while some meeting kept has a code to
/// match. Touches windows and UI Automation: never under a lock, never on
/// the main thread.
pub fn read_call_code(app_id: &str) {
    let wanted = lock_started().is_some_and(|s| s.store.events.iter().any(|e| e.codes.iter().any(|c| codes::matches_app(c, app_id))));
    let code = if wanted { call_windows::read(app_id) } else { None };
    let mut kept = CALL_CODES.lock().unwrap_or_else(|p| p.into_inner());
    match code {
        Some(code) => {
            kept.insert(app_id.to_string(), (code, now_secs()));
        }
        None => {
            kept.remove(app_id);
        }
    }
}

/// The join code `app_id`'s call showed as it started, if any.
fn call_code(app_id: &str) -> Option<String> {
    let kept = CALL_CODES.lock().unwrap_or_else(|p| p.into_inner());
    kept.get(app_id).filter(|(_, at)| now_secs() - at < CALL_CODE_KEEP).map(|(code, _)| code.clone())
}

/// Test runs: call app `appId`'s window shows `title` (and, in a browser,
/// `address`); none takes it away. Read as the next simulated call starts.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn calendar_e2e_call_window(app_id: String, title: Option<String>, address: Option<String>) -> Result<(), String> {
    if !crate::shell::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    let shown = (title.is_some() || address.is_some()).then(|| (title.unwrap_or_default(), address.unwrap_or_default()));
    call_windows::e2e_set(&app_id, shown);
    Ok(())
}

// ---- the next meeting in the tray --------------------------------------------------------------

/// The tray menu item's id.
pub const TRAY_ID: &str = "calendar_next";

/// What the tray says about the next meeting.
#[derive(Debug, Clone, PartialEq)]
struct TrayNext {
    key: String,
    /// The menu item: "Next: Weekly sync in 12 min", "Now: Design review ·
    /// Take notes".
    label: String,
    /// The tooltip's line: the label without what a click does.
    tip: String,
    /// A click starts notes on it (it's on, or about to be, and nothing
    /// records); otherwise a click opens its note.
    starts: bool,
}

/// "Weekly sync", cut at `max` characters with "…".
fn short(title: &str, max: usize) -> String {
    if title.chars().count() <= max {
        return title.to_string();
    }
    let cut: String = title.chars().take(max - 1).collect();
    format!("{}\u{2026}", cut.trim_end())
}

/// The tray's line for meeting `e` at `now` (pure; local times in `tz`):
/// "Next: Weekly sync in 12 min" (an hour away at most), "… at 14:30"
/// (later today), "… tomorrow at 09:00", "… on Thu at 09:00"; "Now: Design
/// review" once it's on; "· Take notes" while a click starts notes (from
/// [`model::EARLY_SECS`] before the start, nothing recording); "·
/// Recording" on the meeting being recorded.
fn tray_line<Tz: chrono::TimeZone>(e: &Event, now: i64, recording_this: bool, recording_any: bool, tz: &Tz) -> TrayNext
where
    Tz::Offset: std::fmt::Display,
{
    let title = short(&e.display_title(), 40);
    let at = |t: i64| tz.timestamp_opt(t, 0).single();
    let tip = if e.start <= now {
        format!("Now: {title}")
    } else {
        let diff = e.start - now;
        let when = match (at(now), at(e.start)) {
            _ if diff < 3_600 => format!("in {} min", ((diff + 30) / 60).max(1)),
            (Some(n), Some(s)) if n.date_naive() == s.date_naive() => format!("at {}", s.format("%H:%M")),
            (Some(n), Some(s)) if n.date_naive().succ_opt() == Some(s.date_naive()) => format!("tomorrow at {}", s.format("%H:%M")),
            (_, Some(s)) => format!("on {} at {}", s.format("%a"), s.format("%H:%M")),
            _ => format!("in {} h", diff / 3_600),
        };
        format!("Next: {title} {when}")
    };
    let starts = !recording_any && model::is_current(e, now);
    let label = if recording_this {
        format!("{tip} \u{b7} Recording")
    } else if starts {
        format!("{tip} \u{b7} Take notes")
    } else {
        tip.clone()
    };
    TrayNext { key: e.key.clone(), label, tip, starts }
}

/// The next meeting as the tray shows it now (`None`: no calendar, or
/// nothing in the next 7 days). Takes the notes store's and the recorder's
/// locks, then the calendar's: never under one of them.
fn tray_next(now: i64) -> Option<TrayNext> {
    let recording = crate::meetings::meeting::recording_note();
    let recording_key = recording.and_then(crate::notes::event_of).map(|e| e.key);
    let event = {
        let s = lock_started()?;
        if s.store.connections.is_empty() {
            return None;
        }
        model::next_meeting(&s.store.events, now, recording_key.as_deref())?.clone()
    };
    let this = recording_key.as_deref() == Some(event.key.as_str());
    Some(tray_line(&event, now, this, recording.is_some(), &chrono::Local))
}

/// The tray menu's next meeting: ([`TRAY_ID`], "Next: Weekly sync in 12
/// min"), right under the version line (`shell/tray.rs`).
pub fn tray_item() -> Option<(String, String)> {
    tray_next(now_secs()).map(|n| (TRAY_ID.to_string(), n.label))
}

/// The tray tooltip's line about the next meeting.
pub fn tray_tooltip() -> Option<String> {
    tray_next(now_secs()).map(|n| n.tip)
}

/// The line the tray was last given; a change refreshes it.
static TRAY_LAST: Mutex<Option<String>> = Mutex::new(None);

/// Each scheduler turn (at least every [`MAX_SLEEP`] seconds, which it
/// does anyway: no extra wake-ups): the tray follows the countdown, a sync,
/// a recording starting or stopping.
fn tick_tray(app: &AppHandle) {
    let label = tray_next(now_secs()).map(|n| n.label);
    let mut last = TRAY_LAST.lock().unwrap_or_else(|p| p.into_inner());
    if *last != label {
        *last = label;
        drop(last);
        crate::shell::tray::refresh(app);
    }
}

/// The tray's next meeting clicked: notes on it when it's on (or about to
/// be) and nothing records, else its note opens in the main window (made
/// ahead of the meeting if need be; the one recording, if it's this one's).
/// Blocking: off the main thread.
pub fn on_tray_next(app: &AppHandle) {
    let Some(next) = tray_next(now_secs()) else { return };
    tracing::info!(starts = next.starts, "calendar: the tray's next meeting clicked");
    let result = if next.starts { start_event(app, &next.key, false).map(|_| ()) } else { open_event_note(app, &next.key) };
    emit(app);
    if let Err(e) = result {
        let _ = crate::shell::window::show_settings(app);
        let _ = app.emit("yap-error", format!("Couldn't open the meeting's note. {e}"));
    }
}

/// Meeting `key`'s note in the main window's Notes: the one recording it,
/// else the one made for it (made now, ahead of the meeting if it's still
/// to come).
fn open_event_note(app: &AppHandle, key: &str) -> Result<(), String> {
    let event = event_by_key(key)?;
    let recording = crate::meetings::meeting::recording_note().filter(|r| crate::notes::find_by_event(key) == Some(*r));
    let note_id = match recording {
        Some(id) => id,
        None => {
            let (id, made) = note_for(&event, event.start > now_secs())?;
            if made {
                let _ = app.emit("yap-notes-changed", ());
            }
            id
        }
    };
    crate::shell::window::show_settings(app)?;
    let _ = app.emit(EVENT_OPEN, json!({ "noteId": note_id, "stop": false }));
    Ok(())
}

// ---- the end of the day -------------------------------------------------------------------------

/// The Meetings view's "Done for today" (after today's last meeting):
/// today's meetings (local day) that have ended, earliest first, each with
/// its note (`noteId`, `hasNotes`: anything typed, said or summarised) and
/// its open tasks (the summary's and those added, not done or archived).
/// `pending`: meetings still to come today (the view waits for none).
#[tauri::command]
pub fn calendar_day() -> Value {
    use chrono::TimeZone;
    let now = now_secs();
    let (today, tomorrow) = {
        let local = chrono::Local.timestamp_opt(now, 0).single().map(|t| t.date_naive());
        let midnight = |d: chrono::NaiveDate| {
            d.and_hms_opt(0, 0, 0).and_then(|t| chrono::Local.from_local_datetime(&t).earliest()).map(|t| t.timestamp())
        };
        match local {
            Some(d) => (midnight(d).unwrap_or(now - 86_400), d.succ_opt().and_then(midnight).unwrap_or(now + 86_400)),
            None => (now - 86_400, now + 86_400),
        }
    };
    let events: Vec<Event> = match lock_started() {
        Some(s) => s.store.events.iter().filter(|e| e.start >= today && e.start < tomorrow).cloned().collect(),
        None => Vec::new(),
    };
    let pending = events.iter().filter(|e| e.end > now).count();
    let store = crate::notes::task_store::snapshot();
    let mut done: Vec<&Event> = events.iter().filter(|e| e.end <= now).collect();
    done.sort_by_key(|e| (e.start, e.key.clone()));
    let meetings: Vec<Value> = done
        .iter()
        .map(|e| {
            let note = crate::notes::find_by_event(&e.key).and_then(crate::notes::get);
            let has_notes = note.as_ref().is_some_and(|n| {
                !n.content.trim().is_empty() || !n.enhanced_content.trim().is_empty() || n.transcript.iter().any(|s| !s.echo)
            });
            let tasks: Vec<Value> = note
                .as_ref()
                .map(|n| crate::notes::tasks::note_tasks(n, &store).tasks)
                .unwrap_or_default()
                .into_iter()
                .filter(|t| !t.task.done && !t.archived)
                .map(|t| serde_json::to_value(t).unwrap_or(Value::Null))
                .collect();
            json!({
                "key": e.key,
                "title": e.display_title(),
                "start": e.start,
                "end": e.end,
                "noteId": note.as_ref().map(|n| n.id),
                "hasNotes": has_notes,
                "tasks": tasks,
            })
        })
        .collect();
    json!({ "meetings": meetings, "pending": pending })
}

// ---- the reminder card ------------------------------------------------------------------------

/// The main window: (on screen, focused) — `notice::window_view` (a test
/// run says for itself whether the person is looking at it). Never with a
/// lock held.
fn window_view(app: &AppHandle) -> (bool, bool) {
    crate::notices::notice::window_view(app)
}

/// What a card is waiting for, after a turn of [`tick_card`].
enum CardChange {
    Nothing,
    /// Take this card down, and maybe put that one up.
    Replace(Option<Box<Card>>, Option<Box<Card>>),
    /// The card up stays: put it on the bar (or a notification) after all —
    /// the main window lost the focus, or the bar went away under it.
    Present(Box<Card>),
    /// The card up stays: off the bar (or out of the notification center)
    /// and into the main window, which has the focus now.
    MoveIn(Box<Card>),
    /// The card up stays: its status line on the bar reads this now.
    Status(String, &'static str),
}

/// Decide which card should be up, and make it so.
fn tick_card(app: &AppHandle) {
    let now = now_secs();
    let lead = reminder_lead(&crate::config::load().meeting_reminder);
    let recording = crate::meetings::meeting::recording_note();
    let recording_key = recording.and_then(crate::notes::get).and_then(|n| n.event).map(|e| e.key);
    let focused = window_view(app).1;
    let change = {
        let Some(mut s) = lock_started() else { return };
        s.snoozed.retain(|_, until| *until > now - 3600);
        let answered: HashSet<String> = s.store.answered.keys().cloned().collect();
        // A switch a call asked for, while it's still current and unanswered.
        if s.switch_to.as_ref().is_some_and(|key| {
            recording.is_none()
                || !s.store.events.iter().any(|e| e.key == *key && model::is_current(e, now) && !answered.contains(&model::answer_key(e)))
        }) {
            s.switch_to = None;
        }
        let forced = s.switch_to.as_ref().and_then(|key| s.store.events.iter().find(|e| e.key == *key)).cloned();
        let due = forced.or_else(|| {
            lead.and_then(|lead| {
                model::due_reminder(&s.store.events, now, lead, &answered, &s.snoozed, recording_key.as_deref()).cloned()
            })
        });
        let kind_for = |e: &Event| match recording {
            Some(_) if recording_key.as_deref() != Some(e.key.as_str()) => CardKind::Switch,
            _ => CardKind::Remind,
        };
        // One card at a time: the due one is the earliest, so a card stays
        // up until it's answered or runs out, and the next waits its turn.
        let same = |c: &Card, e: &Event| c.event.key == e.key && c.event.start == e.start && c.kind == kind_for(e);
        let new_card = |s: &mut Runtime, event: Event| {
            s.seq += 1;
            let card = Card { id: s.seq, kind: kind_for(&event), event, surface: Surface::InApp, bar_status: String::new() };
            s.card = Some(card.clone());
            card
        };
        match (s.card.clone(), due) {
            (Some(card), Some(event)) if same(&card, &event) => match card.surface {
                Surface::InApp if !focused && (crate::bar::cards_available() || native_possible()) => {
                    CardChange::Present(Box::new(card))
                }
                Surface::Bar | Surface::Notification if focused => {
                    if let Some(c) = s.card.as_mut() {
                        c.surface = Surface::InApp;
                    }
                    CardChange::MoveIn(Box::new(card))
                }
                Surface::Bar if !crate::bar::cards_available() => CardChange::Present(Box::new(card)),
                Surface::Bar => {
                    let (status, dot) = bar_status(&card, now);
                    if status == card.bar_status {
                        CardChange::Nothing
                    } else {
                        if let Some(c) = s.card.as_mut() {
                            c.bar_status = status.clone();
                        }
                        CardChange::Status(status, dot)
                    }
                }
                _ => CardChange::Nothing,
            },
            (Some(old), Some(event)) => CardChange::Replace(Some(Box::new(old)), Some(Box::new(new_card(&mut s, event)))),
            (Some(old), None) => {
                s.card = None;
                CardChange::Replace(Some(Box::new(old)), None)
            }
            (None, Some(event)) => CardChange::Replace(None, Some(Box::new(new_card(&mut s, event)))),
            (None, None) => CardChange::Nothing,
        }
    };
    match change {
        CardChange::Nothing => {}
        CardChange::Replace(down, up) => {
            if let Some(old) = down {
                withdraw_card(app, &old);
            }
            if let Some(card) = up {
                present_card(app, &card, focused);
            }
        }
        CardChange::Present(card) => {
            // Off a bar that went away (turned off, hidden for an hour).
            crate::bar::dismiss_card(app, BAR_CARD);
            present_card(app, &card, focused);
        }
        CardChange::MoveIn(card) => {
            tracing::info!(kind = ?card.kind, "calendar: the card moves into the main window");
            withdraw_card(app, &card);
        }
        CardChange::Status(status, dot) => {
            crate::bar::update_card(app, BAR_CARD, |c| {
                c.status = Some(status);
                c.dot = dot;
            });
        }
    }
}

/// **The one place a calendar card goes on screen** — one place at a time
/// (`notice.rs`): the main window's in-app toast while the person is
/// looking at it (the snapshot's `inApp`, `calendar.svelte.js`), else the
/// **Yap bar** (`bar::show_card`, a "Meeting detected"-style call card),
/// else — the bar off or hidden for an hour — a Windows notification, as
/// call detection's prompts do; nothing else possible, the in-app toast.
fn present_card(app: &AppHandle, card: &Card, focused: bool) {
    tracing::info!(kind = ?card.kind, focused, "calendar: meeting card");
    let now = now_secs();
    let mut on_bar = bar_card(card, now);
    on_bar.brief = card_brief(&card.event.key);
    let surface = if focused {
        Surface::InApp
    } else if crate::bar::show_card(app, on_bar, Box::new(answer_from)) {
        Surface::Bar
    } else {
        native_post(app, card)
    };
    let mut s = lock();
    match s.card.as_mut().filter(|c| c.id == card.id) {
        Some(c) => {
            c.surface = surface;
            c.bar_status = bar_status(card, now).0;
            drop(s);
            emit(app);
        }
        // Answered while it was going up: take it down.
        None => {
            drop(s);
            withdraw_card(app, &Card { surface, ..card.clone() });
        }
    }
}

/// Whether a card could go out as a Windows notification at all (not in
/// portable mode or a test run, Windows only). Lock-free.
fn native_possible() -> bool {
    #[cfg(windows)]
    {
        crate::notices::win_toast::allowed()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// The card as a Windows notification (Windows only; never in portable mode
/// or a test run): where it went.
fn native_post(app: &AppHandle, card: &Card) -> Surface {
    #[cfg(windows)]
    match native::post(app, card) {
        Ok(()) => return Surface::Notification,
        Err(e) => tracing::info!("calendar: no Windows notification ({e})"),
    }
    #[cfg(not(windows))]
    let _ = (app, card);
    Surface::InApp
}

/// Take `card` off screen (answered, run out, or replaced).
fn withdraw_card(app: &AppHandle, card: &Card) {
    emit(app);
    match card.surface {
        Surface::Bar => crate::bar::dismiss_card(app, BAR_CARD),
        #[cfg(windows)]
        Surface::Notification => native::remove(app),
        _ => {}
    }
}

/// The bar card's mark: the meeting's call app, as call detection names it
/// (others get a calendar).
fn bar_mark(service: &str) -> Option<&'static str> {
    Some(match service {
        "teams" => "teams",
        "meet" => "meet",
        "zoom" => "zoom",
        "webex" => "webex",
        "slack" => "slack",
        "goto" => "goto",
        "whereby" => "whereby",
        "jitsi" => "jitsi",
        _ => return None,
    })
}

/// "In 1 min", "Starting now", "Started 3 min ago" (the in-app toast's
/// `whenText`).
fn when_text(start: i64, now: i64) -> String {
    let diff = start - now;
    if diff >= 90 {
        format!("In {} min", (diff + 30) / 60)
    } else if diff >= 30 {
        "In 1 min".into()
    } else if diff > 0 {
        "Starting now".into()
    } else if -diff >= 60 {
        format!("Started {} min ago", -diff / 60)
    } else {
        "Started just now".into()
    }
}

/// The bar card's status line and its dot: "● In 1 min · with Tanay +1"
/// (amber before the start, green once it's on), or for a switch, "● Next
/// meeting · In 1 min".
fn bar_status(card: &Card, now: i64) -> (String, &'static str) {
    let when = when_text(card.event.start, now);
    let dot = if card.event.start > now { "soon" } else { "live" };
    let line = match card.kind {
        CardKind::Switch => format!("Next meeting \u{b7} {when}"),
        CardKind::Remind => {
            let first = |name: &str| name.split_whitespace().next().unwrap_or(name).to_string();
            match card.event.attendees.as_slice() {
                [] => when,
                [one] => format!("{when} \u{b7} with {}", first(&one.name)),
                [one, rest @ ..] => format!("{when} \u{b7} with {} +{}", first(&one.name), rest.len()),
            }
        }
    };
    (line, dot)
}

/// The card on the Yap bar, as call detection's "Meeting detected" card:
/// the call app's mark (else a calendar), the meeting over "● In 1 min ·
/// with Tanay +1", a split button **Join & take notes** whose ^ menu holds
/// **Start notes** and **Snooze 2 min**, and ✕ = dismiss. It stays until
/// answered or 5 minutes after the start (Rust takes it down); Esc is left
/// to the app in front.
fn bar_card(card: &Card, now: i64) -> crate::bar::Card {
    use crate::bar::CardAction;
    let arg = |answer: &str| format!("calendar:{answer}:{}", card.id);
    let link = card.event.join_url.is_some();
    let (primary, secondary) = match (card.kind, link) {
        (CardKind::Remind, true) => ("Join & take notes", Some("Start notes")),
        (CardKind::Remind, false) => ("Start notes", None),
        (CardKind::Switch, true) => ("Join & switch notes", Some("Switch notes")),
        (CardKind::Switch, false) => ("Switch notes", None),
    };
    let (status, dot) = bar_status(card, now);
    crate::bar::Card {
        id: BAR_CARD.into(),
        style: "call",
        icon: "calendar",
        app: card.event.service.as_deref().and_then(bar_mark),
        title: card.event.display_title(),
        body: match card.kind {
            CardKind::Switch => "Switch your notes to this meeting?".into(),
            CardKind::Remind => card.event.with_line(),
        },
        status: Some(status),
        dot,
        primary: Some(CardAction::new(arg(if link { "join" } else { "start" }), primary)),
        secondary: secondary.map(|label| CardAction::new(arg("start"), label)),
        link: Some(CardAction::new(arg("snooze"), "Snooze 2 min")),
        close_action: Some(arg("dismiss")),
        ..Default::default()
    }
}

/// An answer from the bar card or the Windows notification:
/// `calendar:<answer>:<card id>` (anything else brings up the main window).
/// A start that fails says why in the main window.
fn answer_from(app: &AppHandle, arg: &str) {
    tracing::info!(arg, "calendar: card answered outside the window");
    let app = app.clone();
    let arg = arg.to_string();
    tauri::async_runtime::spawn(async move {
        let mut parts = arg.splitn(3, ':');
        match (parts.next(), parts.next(), parts.next().and_then(|n| n.parse::<u64>().ok())) {
            (Some("calendar"), Some(answer), Some(id)) => {
                if let Err(e) = respond(&app, id, answer).await {
                    let _ = crate::shell::window::show_settings(&app);
                    let _ = app.emit("yap-error", format!("Couldn't start meeting notes. {e}"));
                }
            }
            _ => {
                let _ = crate::shell::window::show_settings(&app);
            }
        }
    });
}

/// The person answered card `id`: "join" (join the meeting and take notes,
/// or switch notes to it), "start" (notes, or switch, without joining),
/// "snooze" or "dismiss". Returns the note's id when notes started.
pub async fn respond(app: &AppHandle, id: u64, action: &str) -> Result<Option<u64>, String> {
    let card = {
        let mut s = lock();
        match s.card.take() {
            Some(card) if card.id == id => card,
            other => {
                // Answered already, on the other surface.
                s.card = other;
                return Ok(None);
            }
        }
    };
    withdraw_card(app, &card);
    tracing::info!(action, kind = ?card.kind, "calendar: card answered");
    let event = card.event;
    match action {
        "snooze" => {
            lock().snoozed.insert(model::answer_key(&event), now_secs() + SNOOZE_SECS);
            wake();
            Ok(None)
        }
        "dismiss" => {
            answer(&event);
            Ok(None)
        }
        "join" | "start" => {
            answer(&event);
            let join = action == "join" && event.join_url.is_some();
            let result = match card.kind {
                CardKind::Switch => switch_to_event(app, &event.key, join).await,
                CardKind::Remind => {
                    let handle = app.clone();
                    let key = event.key.clone();
                    tauri::async_runtime::spawn_blocking(move || start_event(&handle, &key, join))
                        .await
                        .map_err(|e| e.to_string())?
                }
            };
            emit(app);
            result.map(Some)
        }
        other => Err(format!("Unknown answer: {other}")),
    }
}

/// Answer the card (the in-app toast; see [`respond`]).
#[tauri::command]
pub async fn calendar_card(app: AppHandle, id: u64, action: String) -> Result<Option<u64>, String> {
    respond(&app, id, &action).await
}

/// The "Connect your calendar" nudge: "dismiss" (Not now: never again) or
/// "shown" (the one-time after-a-meeting nudge went up).
#[tauri::command]
pub fn calendar_nudge(app: AppHandle, action: String) {
    {
        let mut s = lock();
        match action.as_str() {
            "dismiss" => s.store.nudge.dismissed = true,
            "shown" => s.store.nudge.shown_after_meeting = true,
            _ => return,
        }
        save_store(&s.store);
    }
    emit(&app);
}

// ---- briefs -------------------------------------------------------------------------------------

/// Notes, memory or settings changed: the due briefs are looked at again.
fn briefs_changed() {
    lock().briefs_dirty = true;
}

/// Reset app (`privacy/reset.rs`): every pre-meeting brief gone (they
/// quote notes and memory); the calendars and their meetings stay, and a
/// due brief is made again from what's left. File I/O only under the lock.
pub fn forget_briefs() -> usize {
    let mut s = lock();
    if !s.started || s.store.briefs.is_empty() {
        return 0;
    }
    let gone = s.store.briefs.len();
    s.store.briefs.clear();
    s.briefs_dirty = true;
    save_store(&s.store);
    gone
}

/// Briefs of meetings long over go, and the oldest past [`BRIEF_MAX`].
fn prune_briefs(store: &mut Store, now: i64) {
    store.briefs.retain(|_, b| b.start > now - BRIEF_KEEP);
    while store.briefs.len() > BRIEF_MAX {
        let Some(oldest) = store.briefs.iter().min_by_key(|(_, b)| b.start).map(|(k, _)| k.clone()) else { break };
        store.briefs.remove(&oldest);
    }
}

/// The meeting as a brief needs it.
fn brief_meeting(e: &Event) -> brief::Meeting {
    brief::Meeting {
        key: e.key.clone(),
        title: e.display_title(),
        start: e.start,
        attendees: e.attendees.iter().map(|a| a.name.clone()).collect(),
        description: e.description.clone(),
    }
}

/// Where a brief's "In short" comes from: the meeting's own model (the Note
/// Formatting scope, else cleanup's), or why there's none.
enum BriefAi {
    Model(crate::meetings::meeting_summary::Endpoint),
    Plain(&'static str),
}

impl BriefAi {
    fn of(cfg: &crate::config::YapConfig) -> BriefAi {
        match crate::meetings::meeting_summary::resolve_endpoint(cfg) {
            // A brief never starts a stopped on-device model.
            Ok(ep) if ep.provider == crate::ai::local_llm::PROVIDER_ONDEVICE && !crate::ai::local_llm::is_running() => {
                BriefAi::Plain("The on-device model isn't running, so this is the plain brief.")
            }
            // A small model on this PC got too much wrong (model-evals.md).
            Ok(ep) if !brief::writes_summary(ep.is_local(), &ep.model) => {
                BriefAi::Plain("A short summary on top needs a bigger AI model (4B or more, or a cloud model).")
            }
            Ok(ep) => BriefAi::Model(ep),
            Err(_) => BriefAi::Plain("Set up an AI model in Language Models for a short summary on top."),
        }
    }

    /// What the fingerprint knows of it: another model makes the brief again.
    fn label(&self) -> String {
        match self {
            BriefAi::Model(ep) => format!("{}:{}", ep.provider, ep.model),
            BriefAi::Plain(_) => "plain".to_string(),
        }
    }
}

/// The card's brief for the Yap bar, as the page draws it (briefs on).
fn card_brief(key: &str) -> Option<Value> {
    if !crate::config::load().meeting_brief {
        return None;
    }
    lock().store.briefs.get(key).and_then(|b| serde_json::to_value(b).ok())
}

/// Keep `b` and show it: the `yap-meeting-brief` event (the Meetings view,
/// the notepad), the snapshot (the in-app card) and, when its meeting's card
/// is on the Yap bar, the card there.
fn store_brief(app: &AppHandle, b: &brief::Brief) {
    let on_bar = {
        let mut s = lock();
        s.store.briefs.insert(b.key.clone(), b.clone());
        prune_briefs(&mut s.store, now_secs());
        save_store(&s.store);
        s.card.as_ref().is_some_and(|c| c.event.key == b.key && c.surface == Surface::Bar)
    };
    let _ = app.emit(EVENT_BRIEF, b);
    emit(app);
    if on_bar {
        let value = serde_json::to_value(b).ok();
        crate::bar::update_card(app, BAR_CARD, |c| c.brief = value);
    }
}

/// The brief for meeting `key`: the one kept, when nothing it's made from
/// has changed (`force`: made again anyway); else made now — Yap's own part
/// at once, and "In short" in the background on the meeting's model (the
/// brief kept meanwhile is `writing`; `yap-meeting-brief` brings the
/// finished one). One summary is written at a time; others wait their turn.
/// Blocking: it reads the notes.
fn prepare_brief(app: &AppHandle, key: &str, force: bool) -> Result<brief::Brief, String> {
    let cfg = crate::config::load();
    if !cfg.meeting_brief {
        return Err("Briefs are off (Settings → Meetings).".into());
    }
    let meeting = brief_meeting(&event_by_key(key)?);
    let now = now_secs();
    let facts = brief::gather(&meeting, &crate::notes::all(), &crate::memory::items(), now);
    let ai = BriefAi::of(&cfg);
    let fingerprint = facts.fingerprint(&ai.label());
    {
        let s = lock();
        let busy = s.brief_job.as_deref() == Some(key) || s.brief_waiting.iter().any(|k| k == key);
        if let Some(b) = s.store.briefs.get(key) {
            if busy || (!force && b.fingerprint == fingerprint && !b.writing) {
                return Ok(b.clone());
            }
        }
    }
    let wants_ai = matches!(ai, BriefAi::Model(_)) && facts.worth_a_call();
    let note = match &ai {
        BriefAi::Plain(why) if facts.worth_a_call() => why,
        _ => "",
    };
    let mut b = brief::assemble(&facts, vec![], false, note, now);
    b.fingerprint = fingerprint.clone();
    b.writing = wants_ai;
    let start = wants_ai && {
        let mut s = lock();
        if s.brief_job.is_none() {
            s.brief_job = Some(key.to_string());
            true
        } else {
            s.brief_waiting.retain(|k| k != key);
            s.brief_waiting.push(key.to_string());
            false
        }
    };
    tracing::info!(
        sources = b.sources.len(),
        open = b.open_count(),
        first = b.first,
        words = b.words,
        summary = wants_ai,
        "calendar: brief made"
    );
    tracing::debug!("calendar: brief:\n{}", brief::plain_text(&b));
    store_brief(app, &b);
    if let (true, BriefAi::Model(ep)) = (start, ai) {
        let app = app.clone();
        tauri::async_runtime::spawn(async move { write_brief(app, facts, ep, fingerprint).await });
    }
    Ok(b)
}

/// Write `facts`' "In short" on the meeting's model (beside any dictation),
/// keep the finished brief, then start the next one waiting.
async fn write_brief(app: AppHandle, facts: brief::Facts, ep: crate::meetings::meeting_summary::Endpoint, fingerprint: String) {
    let key = facts.meeting.key.clone();
    let started = std::time::Instant::now();
    let result = brief::write(Some(&app), &facts, &ep).await;
    let now = now_secs();
    let b = match result {
        Ok((checked, _)) => {
            tracing::info!(
                kept = checked.lines.len(),
                uncited = checked.uncited,
                ungrounded = checked.ungrounded,
                swapped = checked.swapped,
                ms = started.elapsed().as_millis() as u64,
                "calendar: brief's summary written"
            );
            let mut b = brief::assemble(&facts, checked.lines, true, "", now);
            b.fingerprint = fingerprint;
            b
        }
        Err(e) => {
            tracing::info!("calendar: brief's summary not written ({e})");
            lock().brief_retry.insert(key.clone(), now + BRIEF_RETRY);
            // The plain brief, made again when asked for or after the wait.
            brief::assemble(&facts, vec![], false, "The AI couldn't write a summary this time, so this is the plain brief.", now)
        }
    };
    tracing::debug!("calendar: brief:\n{}", brief::plain_text(&b));
    store_brief(&app, &b);
    let next = {
        let mut s = lock();
        s.brief_job = None;
        (!s.brief_waiting.is_empty()).then(|| s.brief_waiting.remove(0))
    };
    if let Some(next) = next {
        let handle = app.clone();
        let _ = tauri::async_runtime::spawn_blocking(move || prepare_brief(&handle, &next, false)).await;
    }
}

/// The scheduler's turn: make the briefs of meetings starting within
/// [`BRIEF_AHEAD`] (or on now) that have none, and look again at those whose
/// notes or memory changed since.
fn tick_briefs(app: &AppHandle) {
    let on = crate::config::load().meeting_brief;
    // Switched on or off in Settings: the pages show the Brief buttons, or not.
    let switched = lock_started().is_some_and(|mut s| s.briefs_on.replace(on).is_some_and(|was| was != on));
    if switched {
        emit(app);
    }
    if !on {
        return;
    }
    let now = now_secs();
    let due: Vec<String> = {
        let Some(mut s) = lock_started() else { return };
        if s.brief_job.is_some() {
            return;
        }
        let dirty = std::mem::take(&mut s.briefs_dirty);
        s.brief_retry.retain(|_, until| *until > now);
        s.store
            .events
            .iter()
            .filter(|e| e.start - BRIEF_AHEAD <= now && now < e.start + model::REMINDER_AFTER_SECS && now < e.end)
            .filter(|e| !s.brief_retry.contains_key(&e.key))
            .filter(|e| dirty || s.store.briefs.get(&e.key).is_none_or(|b| b.writing || b.fingerprint.is_empty()))
            .map(|e| e.key.clone())
            .collect()
    };
    if due.is_empty() {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        for key in due {
            if let Err(e) = prepare_brief(&app, &key, false) {
                tracing::info!("calendar: no brief ({e})");
            }
        }
    });
}

/// A meeting's brief (the Meetings view): the one kept, or made now; with
/// `refresh`, made again. Its "In short" may follow (`yap-meeting-brief`).
#[tauri::command]
pub async fn calendar_brief(app: AppHandle, key: String, refresh: Option<bool>) -> Result<brief::Brief, String> {
    tauri::async_runtime::spawn_blocking(move || prepare_brief(&app, &key, refresh.unwrap_or(false)))
        .await
        .map_err(|e| e.to_string())?
}

/// The brief of the meeting note `note_id` was made for (the notepad): the
/// one kept, else — its meeting still on the calendar — made now. `None`
/// with briefs off, or for a note no calendar meeting made.
#[tauri::command]
pub async fn calendar_brief_for_note(app: AppHandle, note_id: u64) -> Result<Option<brief::Brief>, String> {
    if !crate::config::load().meeting_brief {
        return Ok(None);
    }
    let Some(key) = crate::notes::event_of(note_id).map(|e| e.key) else { return Ok(None) };
    let (kept, known) = {
        let s = lock();
        (s.store.briefs.get(&key).cloned(), s.store.events.iter().any(|e| e.key == key))
    };
    if kept.is_some() || !known {
        return Ok(kept);
    }
    tauri::async_runtime::spawn_blocking(move || prepare_brief(&app, &key, false).ok())
        .await
        .map_err(|e| e.to_string())
}

/// The kept brief of the meeting note `note_id` was made for, as plain
/// lines for a helper's context (the notepad's "Make me sound smart"):
/// never made here. `None` with briefs off, or without one.
pub fn brief_lines_for_note(note_id: u64) -> Option<String> {
    if !crate::config::load().meeting_brief {
        return None;
    }
    let key = crate::notes::event_of(note_id)?.key;
    let kept = lock().store.briefs.get(&key).cloned()?;
    Some(brief::plain(&kept)).filter(|t| !t.trim().is_empty())
}

/// A brief's citation `n` opened (any window): its note in the main window's
/// Notes, Settings → Memory, or — the invite — the meeting's own note (made
/// ahead if need be).
#[tauri::command]
pub async fn calendar_brief_open(app: AppHandle, key: String, n: usize) -> Result<(), String> {
    let source = lock()
        .store
        .briefs
        .get(&key)
        .and_then(|b| b.sources.iter().find(|s| s.n == n).cloned())
        .ok_or("That brief has changed. Open it again.")?;
    let note_id = match source.kind.as_str() {
        "note" => source.note_id,
        "invite" => {
            let event = event_by_key(&key)?;
            let ahead = event.start > now_secs();
            let (id, made) = tauri::async_runtime::spawn_blocking(move || note_for(&event, ahead))
                .await
                .map_err(|e| e.to_string())??;
            if made {
                let _ = app.emit("yap-notes-changed", ());
            }
            Some(id)
        }
        _ => None,
    };
    crate::shell::window::show_settings(&app)?;
    match note_id {
        Some(id) => {
            let _ = app.emit(EVENT_OPEN, json!({ "noteId": id, "stop": false }));
        }
        None => {
            let _ = app.emit("yap-open-settings", "memory");
        }
    }
    Ok(())
}

// ---- past meeting notes -------------------------------------------------------------------------

/// Meeting notes for the Meetings view, newest meeting first, matching every
/// word of `query` (title, attendees, notes, action plan, transcript).
/// Drafts made ahead of a meeting still to come are left out (the meeting
/// is listed above them).
#[tauri::command]
pub fn calendar_meeting_notes(query: Option<String>) -> Vec<Value> {
    let words: Vec<String> = query.unwrap_or_default().to_lowercase().split_whitespace().map(str::to_string).collect();
    let now = now_secs().max(0) as u64;
    let recording = crate::meetings::meeting::recording_note();
    let mut out: Vec<(u64, Value)> = crate::notes::all()
        .into_iter()
        .filter(|n| n.note_type == "meeting" || n.event.is_some())
        .filter(|n| !(n.transcript.is_empty() && n.event.as_ref().is_some_and(|e| e.start > now)))
        .filter(|n| {
            if words.is_empty() {
                return true;
            }
            let mut hay = format!("{}\n{}\n{}\n{}", n.title, n.participants.join(" "), n.content, n.enhanced_content);
            for seg in n.transcript.iter().filter(|s| !s.echo) {
                hay.push('\n');
                hay.push_str(&seg.text);
            }
            let hay = hay.to_lowercase();
            words.iter().all(|w| hay.contains(w.as_str()))
        })
        .map(|n| {
            let when = n.event.as_ref().map(|e| e.start).or_else(|| n.transcript.first().map(|s| s.ts)).unwrap_or(n.created_ts);
            let source = if n.enhanced_content.trim().is_empty() { &n.content } else { &n.enhanced_content };
            let preview: String = source.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(140).collect();
            let value = json!({
                "id": n.id,
                "title": n.title,
                "when": when,
                "participants": n.participants,
                "preview": preview,
                "hasPlan": !n.enhanced_content.trim().is_empty(),
                // Something to summarise ("Generate summary").
                "hasTalk": n.transcript.iter().any(|s| !s.echo),
                "recording": recording == Some(n.id),
                // Its folder, its series' name and that series' folder rule
                // (the menu's "File this meeting in…", "Always file…").
                "filing": filing::view_of(&n),
            });
            (when, value)
        })
        .collect();
    out.sort_by_key(|a| std::cmp::Reverse(a.0));
    out.into_iter().take(200).map(|(_, v)| v).collect()
}

/// The invite's description, as context for a meeting's action plan and its
/// Ask bar (`meeting_summary`, `notes::commands::note_ask`): `None` without one.
pub fn invite_context(note: &crate::notes::Note) -> Option<String> {
    let event = note.event.as_ref().filter(|e| !e.description.trim().is_empty())?;
    Some(format!("From the calendar invite \"{}\":\n{}\n", event.title, event.description.trim()))
}

// ---- test mode ------------------------------------------------------------------------------------

/// Test mode only: the links Yap would have opened in the browser (meeting
/// links, Google's consent page), oldest first.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn calendar_e2e_opened() -> Result<Vec<String>, String> {
    if !crate::shell::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    Ok(lock().opened.clone())
}

// ---- Windows notifications ---------------------------------------------------------------------

/// The card as a Windows notification (silent, Yap's logo, its answers as
/// buttons), for while the main window isn't focused.
#[cfg(windows)]
mod native {
    use std::sync::Mutex;

    use tauri::AppHandle;
    use windows::UI::Notifications::ToastNotification;

    use super::{Card, CardKind};
    use crate::notices::win_toast::{esc, logo_xml};

    const TAG: &str = "calendar";

    /// The live toast, kept so its buttons work from the notification center.
    static CURRENT: Mutex<Option<ToastNotification>> = Mutex::new(None);

    /// "Starts at 14:30 · Teams · with Tanay Kothari, Priya Shah".
    fn body(card: &Card) -> String {
        use chrono::TimeZone;
        let at = chrono::Local
            .timestamp_opt(card.event.start, 0)
            .single()
            .map(|t| t.format("%H:%M").to_string())
            .unwrap_or_default();
        let mut parts = vec![if card.event.start > super::now_secs() { format!("Starts at {at}") } else { format!("Started at {at}") }];
        if let Some(service) = card.event.service_label() {
            parts.push(service.to_string());
        }
        let with = card.event.with_line();
        if !with.is_empty() {
            parts.push(format!("with {with}"));
        }
        let lead = match card.kind {
            CardKind::Switch => "Switch your notes to this meeting? ",
            CardKind::Remind => "",
        };
        format!("{lead}{}", parts.join(" \u{b7} "))
    }

    pub fn xml(card: &Card, logo: &str) -> String {
        let link = card.event.join_url.is_some();
        let (primary, secondary) = match (card.kind, link) {
            (CardKind::Remind, true) => ("Join &amp; take notes", Some("Start notes")),
            (CardKind::Remind, false) => ("Start notes", None),
            (CardKind::Switch, true) => ("Join &amp; switch notes", Some("Switch notes")),
            (CardKind::Switch, false) => ("Switch notes", None),
        };
        let id = card.id;
        let primary_action = if link { "join" } else { "start" };
        let secondary = secondary.map_or(String::new(), |label| {
            format!("<action content=\"{label}\" arguments=\"calendar:start:{id}\" activationType=\"foreground\"/>")
        });
        format!(
            "<toast launch=\"calendar:show\" duration=\"long\"><visual><binding template=\"ToastGeneric\">\
             <text>{}</text><text>{}</text>{logo}</binding></visual><actions>\
             <action content=\"{primary}\" arguments=\"calendar:{primary_action}:{id}\" activationType=\"foreground\"/>\
             {secondary}\
             <action content=\"Snooze\" arguments=\"calendar:snooze:{id}\" activationType=\"foreground\"/>\
             </actions><audio silent=\"true\"/></toast>",
            esc(&card.event.display_title()),
            esc(&body(card)),
        )
    }

    pub fn post(app: &AppHandle, card: &Card) -> Result<(), String> {
        if !crate::notices::win_toast::allowed() {
            return Err("off in portable mode and test runs".to_string());
        }
        let toast = crate::notices::win_toast::post(app, TAG, &xml(card, &logo_xml()), None, activated)?;
        *CURRENT.lock().unwrap_or_else(|p| p.into_inner()) = Some(toast);
        Ok(())
    }

    pub fn remove(app: &AppHandle) {
        if CURRENT.lock().unwrap_or_else(|p| p.into_inner()).take().is_some() {
            crate::notices::win_toast::remove(app, TAG);
        }
    }

    /// A click: `calendar:<answer>:<card id>`, or the body (`calendar:show`).
    fn activated(app: &AppHandle, arg: &str) {
        super::answer_from(app, arg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_are_https_or_webcal() {
        assert_eq!(
            normalize_link(" webcal://p01-caldav.icloud.com/published/2/abc ").unwrap(),
            "https://p01-caldav.icloud.com/published/2/abc"
        );
        assert!(normalize_link("https://outlook.office365.com/owa/calendar/x/y/calendar.ics").is_ok());
        assert!(normalize_link("http://calendar.example.com/feed.ics").is_err());
        assert!(normalize_link("http://127.0.0.1:8080/feed.ics").is_ok());
        assert!(normalize_link("ftp://example.com/feed.ics").is_err());
        assert!(normalize_link("not a link").is_err());
        assert_eq!(link_host("https://calendar.google.com/calendar/ical/x/private-y/basic.ics"), "calendar.google.com");
    }

    #[test]
    fn reminder_leads() {
        assert_eq!(reminder_lead("15s"), Some(15));
        assert_eq!(reminder_lead("1min"), Some(60));
        assert_eq!(reminder_lead("2min"), Some(120));
        assert_eq!(reminder_lead("never"), None);
        assert_eq!(reminder_lead("anything else"), Some(15));
        assert_eq!(crate::config::YapConfig::default().meeting_reminder, "15s");
    }

    #[test]
    fn a_feed_becomes_meetings_with_wisprs_filters() {
        let feed = b"BEGIN:VCALENDAR\r\nX-WR-CALNAME:imogen@example.com\r\n\
            BEGIN:VEVENT\r\nUID:a\r\nSUMMARY:Design review\r\nDTSTART:20261005T140000Z\r\nDTEND:20261005T143000Z\r\n\
            ATTENDEE;CN=Tanay Kothari:mailto:tanay@example.com\r\nATTENDEE;CN=Imogen:mailto:imogen@example.com\r\nEND:VEVENT\r\n\
            BEGIN:VEVENT\r\nUID:b\r\nSUMMARY:Focus\r\nDTSTART:20261005T150000Z\r\nDTEND:20261005T160000Z\r\nEND:VEVENT\r\n\
            BEGIN:VEVENT\r\nUID:c\r\nSUMMARY:Offsite\r\nDTSTART;VALUE=DATE:20261005\r\nDTEND;VALUE=DATE:20261006\r\n\
            ATTENDEE:mailto:tanay@example.com\r\nEND:VEVENT\r\n\
            BEGIN:VEVENT\r\nUID:d\r\nSUMMARY:Workshop\r\nDTSTART:20261005T080000Z\r\nDTEND:20261005T150000Z\r\n\
            ATTENDEE:mailto:tanay@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let (fetched, name) = events_from_feed("1", feed, 1_791_000_000, 1_792_000_000).unwrap();
        assert_eq!(name.as_deref(), Some("imogen@example.com"));
        assert_eq!(fetched.events.iter().map(|e| e.title.as_str()).collect::<Vec<_>>(), ["Design review"]);
        // Focus time (nobody else) and the 7-hour workshop: busy, not meetings.
        // (The all-day offsite never gets this far.)
        let mut busy: Vec<&str> = fetched.busy.iter().map(|b| b.title.as_str()).collect();
        busy.sort_unstable();
        assert_eq!(busy, ["Focus", "Workshop"]);
        assert!(events_from_feed("1", b"<html>nope</html>", 0, 1).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn the_card_as_a_windows_notification() {
        let event: Event = serde_json::from_value(json!({
            "key": "1:design", "connection": "1", "title": "Design review & Q4",
            "start": now_secs() + 60, "end": now_secs() + 1_860,
            "attendees": [{ "name": "Tanay Kothari" }, { "name": "Priya Shah" }],
            "joinUrl": "https://teams.microsoft.com/l/meetup-join/19%3a1", "service": "teams",
        }))
        .unwrap();
        let card = Card { id: 7, kind: CardKind::Remind, event, surface: Surface::InApp, bar_status: String::new() };
        let xml = native::xml(&card, "");
        assert!(xml.contains("<text>Design review &amp; Q4</text>"));
        assert!(xml.contains("Teams \u{b7} with Tanay Kothari, Priya Shah"));
        // Join & take notes, Start notes, Snooze.
        assert_eq!(xml.matches("<action ").count(), 3);
        assert!(xml.contains("content=\"Join &amp; take notes\" arguments=\"calendar:join:7\""));
        assert!(xml.contains("content=\"Start notes\" arguments=\"calendar:start:7\""));
        assert!(xml.contains("arguments=\"calendar:snooze:7\""));
        // WinRT takes it as a toast (built, never shown).
        crate::notices::win_toast::build("calendar", &xml, false).unwrap();
        // Without a link, and as a switch: no join, and the question.
        let mut plain = card.clone();
        plain.kind = CardKind::Switch;
        plain.event.join_url = None;
        let xml = native::xml(&plain, "");
        assert_eq!(xml.matches("<action ").count(), 2);
        assert!(xml.contains("content=\"Switch notes\" arguments=\"calendar:start:7\""));
        assert!(xml.contains("Switch your notes to this meeting?"));
        crate::notices::win_toast::build("calendar", &xml, false).unwrap();
    }

    #[test]
    fn the_card_on_the_yap_bar() {
        let now = 1_791_200_000;
        let event: Event = serde_json::from_value(json!({
            "key": "1:design", "connection": "1", "title": "Design review",
            "start": now + 60, "end": now + 1_860,
            "attendees": [{ "name": "Tanay Kothari" }, { "name": "Priya Shah" }],
            "joinUrl": "https://teams.microsoft.com/l/meetup-join/19%3a1", "service": "teams",
        }))
        .unwrap();
        let card = Card { id: 7, kind: CardKind::Remind, event, surface: Surface::InApp, bar_status: String::new() };
        let bar = bar_card(&card, now);
        // Call detection's card: the app's mark, the meeting over "● In 1 min".
        assert_eq!((bar.id.as_str(), bar.style, bar.app), (BAR_CARD, "call", Some("teams")));
        assert_eq!(bar.title, "Design review");
        assert_eq!((bar.status.as_deref(), bar.dot), (Some("In 1 min \u{b7} with Tanay +1"), "soon"));
        let action = |a: &Option<crate::bar::CardAction>| a.as_ref().map(|a| (a.id.clone(), a.label.clone()));
        assert_eq!(action(&bar.primary), Some(("calendar:join:7".into(), "Join & take notes".into())));
        assert_eq!(action(&bar.secondary), Some(("calendar:start:7".into(), "Start notes".into())));
        assert_eq!(action(&bar.link), Some(("calendar:snooze:7".into(), "Snooze 2 min".into())));
        assert_eq!(bar.close_action.as_deref(), Some("calendar:dismiss:7"));
        // It stays until answered (Rust takes it down 5 minutes in), and Esc
        // belongs to the app in front.
        assert_eq!((bar.timeout_ms, bar.escape_action.as_deref()), (None, None));
        // The line keeps time: on, then running.
        assert_eq!(bar_status(&card, now + 50), ("Starting now \u{b7} with Tanay +1".to_string(), "soon"));
        assert_eq!(bar_status(&card, now + 240), ("Started 3 min ago \u{b7} with Tanay +1".to_string(), "live"));
        // A switch without a link, on a service without a mark.
        let mut next = card.clone();
        next.kind = CardKind::Switch;
        next.event.join_url = None;
        next.event.service = Some("chime".into());
        let bar = bar_card(&next, now - 200);
        assert_eq!(bar.app, None);
        assert_eq!((bar.status.as_deref(), bar.dot), (Some("Next meeting \u{b7} In 4 min"), "soon"));
        assert_eq!(action(&bar.primary), Some(("calendar:start:7".into(), "Switch notes".into())));
        assert_eq!(bar.secondary.as_ref().map(|a| a.id.as_str()), None);
        assert_eq!(when_text(now + 150, now), "In 3 min");
        assert_eq!(when_text(now - 30, now), "Started just now");
    }

    #[test]
    fn a_new_connection_counts_its_meetings_in_the_next_7_days() {
        let now = 1_791_200_000;
        let event = |key: &str, connection: &str, start: i64, end: i64| -> Event {
            serde_json::from_value(json!({ "key": key, "connection": connection, "start": start, "end": end })).unwrap()
        };
        let events = [
            event("1:on-now", "1", now - 600, now + 1_200),
            event("1:over", "1", now - 7_200, now - 3_600),
            event("1:tomorrow", "1", now + 86_400, now + 88_200),
            event("1:too-far", "1", now + LOOK_AHEAD + 60, now + LOOK_AHEAD + 1_860),
            event("2:other", "2", now + 3_600, now + 5_400),
        ];
        // "Calendar connected · 2 meetings in the next 7 days": the one on
        // now and tomorrow's, not one that's over, a week away or another
        // calendar's.
        assert_eq!(upcoming_count(&events, "1", now), 2);
        assert_eq!(upcoming_count(&events, "2", now), 1);
        assert_eq!(upcoming_count(&events, "3", now), 0);
    }

    #[test]
    fn the_trays_next_meeting_line() {
        let now = 1_791_187_200; // Mon 5 Oct 2026, 08:00 UTC
        let ev = |start: i64, end: i64| -> Event {
            serde_json::from_value(json!({ "key": "1:w", "connection": "1", "title": "Weekly sync", "start": start, "end": end })).unwrap()
        };
        let line = |e: &Event, this: bool, any: bool| tray_line(e, now, this, any, &chrono::Utc);
        // A countdown within the hour; a click opens its note.
        let next = line(&ev(now + 720, now + 2_520), false, false);
        assert_eq!((next.label.as_str(), next.tip.as_str(), next.starts), ("Next: Weekly sync in 12 min", "Next: Weekly sync in 12 min", false));
        // From 10 minutes before, a click starts notes.
        let soon = line(&ev(now + 240, now + 2_040), false, false);
        assert_eq!((soon.label.as_str(), soon.tip.as_str(), soon.starts), ("Next: Weekly sync in 4 min \u{b7} Take notes", "Next: Weekly sync in 4 min", true));
        // Later today, tomorrow, later in the week.
        assert_eq!(line(&ev(now + 23_400, now + 25_200), false, false).label, "Next: Weekly sync at 14:30");
        assert_eq!(line(&ev(now + 90_000, now + 91_800), false, false).label, "Next: Weekly sync tomorrow at 09:00");
        assert_eq!(line(&ev(now + 176_400, now + 178_200), false, false).label, "Next: Weekly sync on Wed at 09:00");
        // On now: take notes, unless another meeting records; the one
        // recording says so.
        let on = ev(now - 60, now + 1_740);
        assert_eq!(line(&on, false, false).label, "Now: Weekly sync \u{b7} Take notes");
        let busy = line(&on, false, true);
        assert_eq!((busy.label.as_str(), busy.starts), ("Now: Weekly sync", false));
        let this = line(&on, true, true);
        assert_eq!((this.label.as_str(), this.tip.as_str(), this.starts), ("Now: Weekly sync \u{b7} Recording", "Now: Weekly sync", false));
        // A long name is cut.
        let mut long = ev(now + 720, now + 2_520);
        long.title = "Quarterly business review with the regional leadership team".into();
        assert_eq!(line(&long, false, false).label, "Next: Quarterly business review with the regi\u{2026} in 12 min");
    }

    #[test]
    fn invites_become_context() {
        let mut note: crate::notes::Note = serde_json::from_value(json!({
            "id": 1, "title": "Design review", "createdTs": 1, "updatedTs": 1,
        }))
        .unwrap();
        assert!(invite_context(&note).is_none());
        note.event = Some(crate::notes::NoteEvent {
            key: "1:a".into(),
            title: "Design review".into(),
            start: 1,
            end: 2,
            description: "Agenda: the Q4 plan".into(),
            service: String::new(),
        });
        assert_eq!(invite_context(&note).unwrap(), "From the calendar invite \"Design review\":\nAgenda: the Q4 plan\n");
    }
}
