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

mod google;
mod ics;
mod links;
mod model;
mod tz;
mod vault;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{LazyLock, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Listener, Manager};

use model::Event;

/// The snapshot ([`Snapshot`]), on every change.
const EVENT: &str = "yap-calendar";
/// A connection was made: `{ kind, label }`.
const EVENT_CONNECTED: &str = "yap-calendar-connected";
/// Something the page didn't directly ask for went wrong (a Google sign-in
/// finishing in the background): the message.
const EVENT_ERROR: &str = "yap-calendar-error";
/// Open a note in Notes: `{ noteId, stop }` (call detection's event).
const EVENT_OPEN: &str = "yap-meeting-open-note";

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

const NO_GOOGLE: &str = "This build of Yap can't connect to Google yet. Add your Google Calendar's secret iCal address under Other calendar instead.";
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
    /// The account ("nathan@gmail.com") or the calendar's name.
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

fn save_store(store: &Store) {
    match serde_json::to_string(store) {
        Ok(json) => {
            let _ = std::fs::create_dir_all(crate::config::data_dir());
            if let Err(e) = crate::config::atomic_write(&store_path(), &json) {
                tracing::warn!("calendar: couldn't save calendar.json: {e}");
            }
        }
        Err(e) => tracing::warn!("calendar: couldn't serialize the store: {e}"),
    }
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
    key: String,
    title: String,
    start: i64,
    end: i64,
    with: String,
    service_label: Option<&'static str>,
    has_link: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct NudgeView {
    /// The Meetings view's "Connect your calendar" card.
    hub: bool,
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

fn snapshot_of(s: &Runtime, notes: &HashMap<String, u64>, now: i64) -> Snapshot {
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
            key: c.event.key.clone(),
            title: c.event.display_title(),
            start: c.event.start,
            end: c.event.end,
            with: c.event.with_line(),
            service_label: c.event.service_label(),
            has_link: c.event.join_url.is_some(),
        }),
        nudge: NudgeView {
            hub: !connected && !s.store.nudge.dismissed,
            after_meeting: !connected && !s.store.nudge.dismissed && !s.store.nudge.shown_after_meeting,
        },
        max_connections: vault::SLOTS,
    }
}

/// The notes made for events: event key → note id.
fn note_links() -> HashMap<String, u64> {
    crate::notes::event_links().into_iter().map(|(id, key)| (key, id)).collect()
}

pub fn status() -> Snapshot {
    let notes = note_links();
    snapshot_of(&lock(), &notes, now_secs())
}

fn emit(app: &AppHandle) {
    let _ = app.emit(EVENT, status());
}

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
        s.started = true;
        // A first sync straight away (it's cheap, and the cache may be old).
        s.next_sync = now;
    }
    // A recording starting or stopping changes which card is due.
    app.listen("yap-meeting-state", |_| wake());
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
    let lead = reminder_lead(&crate::config::load().meeting_reminder);
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
    (next - now).clamp(1, MAX_SLEEP as i64) as u64
}

/// Settings were saved: the reminder lead may have changed.
pub fn on_config_saved() {
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
        results.push((conn.id.clone(), result));
    }
    {
        let mut s = lock();
        for (id, result) in results {
            let Some(conn) = s.store.connections.iter_mut().find(|c| c.id == id) else { continue };
            match result {
                Ok(events) => {
                    conn.synced_ts = now;
                    conn.error = None;
                    conn.reconnect = false;
                    s.store.events.retain(|e| e.connection != id);
                    s.store.events.extend(events);
                }
                Err(e) => {
                    conn.error = Some(e.message);
                    conn.reconnect |= e.reconnect;
                }
            }
        }
        s.store.events.sort_by_key(|e| (e.start, e.key.clone()));
        s.store.answered.retain(|_, at| *at > now - ANSWER_KEEP);
        s.syncing = false;
        // A calendar connected while this sync ran gets its own straight away.
        let added = s.store.connections.iter().any(|c| !connections.iter().any(|o| o.id == c.id));
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

async fn fetch(conn: &Connection, from: i64, to: i64) -> Result<Vec<Event>, FetchError> {
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
        .send()
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

/// A feed's meetings between `from` and `to`, and the calendar's name.
fn events_from_feed(connection: &str, bytes: &[u8], from: i64, to: i64) -> Result<(Vec<Event>, Option<String>), String> {
    let calendar = ics::parse_bytes(bytes)
        .ok_or("That address didn't return a calendar. Check you copied the iCal (.ics) link.")?;
    let resolver = tz::Resolver::new(&calendar, tz::Zone::Local);
    let me = model::self_addresses(&calendar);
    let mut seen = HashSet::new();
    let events = ics::occurrences(&calendar, &resolver, from, to)
        .iter()
        .filter_map(|o| model::finish(model::draft_from_ics(connection, o, &me)))
        .filter(|e| seen.insert(e.key.clone()))
        .collect();
    Ok((events, ics::calendar_name(&calendar)))
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

async fn fetch_google(conn: &Connection, from: i64, to: i64) -> Result<Vec<Event>, FetchError> {
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
                let mut seen = HashSet::new();
                return Ok(items
                    .iter()
                    .filter_map(|item| google::draft_from_item(&conn.id, item))
                    .filter_map(model::finish)
                    .filter(|e| seen.insert(e.key.clone()))
                    .collect());
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

// ---- connecting -------------------------------------------------------------------------------

/// Open `link` in the browser. Test runs never do: they keep it, for the
/// suite to check (`calendar_e2e_opened`).
fn open_link(app: &AppHandle, link: &str) -> Result<(), String> {
    if crate::e2e::active() {
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
            let _ = reply.send(result.clone());
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
        Ok(Some(label)) => {
            let _ = app.emit(EVENT_CONNECTED, json!({ "kind": "google", "label": label }));
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
/// caller syncs it). Returns the account's address.
async fn connect_google(
    client: &google::Client,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<String, String> {
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
    Ok(label)
}

/// Add a calendar by its private iCal link (`kind`: "outlook" from the
/// Outlook card, else "ics"). The link is fetched once first, so a wrong one
/// is caught here, in the form.
#[tauri::command]
pub async fn calendar_add_link(app: AppHandle, link: String, kind: Option<String>) -> Result<(), String> {
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
    let (events, name) = tauri::async_runtime::spawn_blocking(move || events_from_feed(&slot, &bytes, from, to))
        .await
        .map_err(|e| e.to_string())??;
    vault::save(&id, &link)?;
    let host = link_host(&link);
    let label = match name.filter(|n| !n.eq_ignore_ascii_case("calendar")) {
        Some(name) => name,
        None if kind == "outlook" => "Outlook calendar".to_string(),
        None => "Calendar".to_string(),
    };
    {
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
        s.store.events.extend(events);
        s.store.events.sort_by_key(|e| (e.start, e.key.clone()));
        s.next_sync = now + SYNC_EVERY;
        save_store(&s.store);
    }
    tracing::info!(connection = %id, kind, "calendar: calendar link added");
    let _ = app.emit(EVENT_CONNECTED, json!({ "kind": kind, "label": label }));
    emit(&app);
    wake();
    Ok(())
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
        s.access.remove(&id);
        save_store(&s.store);
        conn
    };
    if conn.kind == "google" {
        if let Some(secret) = vault::load(&id).and_then(|s| serde_json::from_str::<GoogleSecret>(&s).ok()) {
            google::revoke(&secret.refresh_token).await;
        }
    }
    vault::delete(&id);
    tracing::info!(connection = %id, kind = %conn.kind, "calendar: disconnected");
    emit(&app);
    wake();
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

fn attendee_names(e: &Event) -> Vec<String> {
    e.attendees.iter().map(|a| a.name.clone()).collect()
}

/// The note for `event`: the one made before, or a new meeting note with its
/// title and attendees (dated to the meeting when made ahead of it).
fn note_for(event: &Event, ahead: bool) -> Result<(u64, bool), String> {
    if let Some(id) = crate::notes::find_by_event(&event.key) {
        return Ok((id, false));
    }
    crate::notes::folder_create("Meetings");
    let note = crate::notes::create(&event.display_title(), "", "calendar", "Meetings");
    let date = ahead.then_some(event.start.max(0) as u64);
    crate::notes::link_event(note.id, note_event(event), None, &attendee_names(event), date)?;
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
        let n = crate::meeting_guard::Notice {
            kind: "recording",
            title: format!("Taking notes on {title}"),
            body: "Yap is recording. Let people know you're taking notes.".to_string(),
            icon: "call",
            variant: "success",
            settings: None,
            note_id: Some(note_id),
        };
        crate::meeting_guard::notice_native(app, &n);
    }
}

/// Take notes on `key` (and join it first, `join`): its note, recording.
fn start_event(app: &AppHandle, key: &str, join: bool) -> Result<u64, String> {
    let event = event_by_key(key)?;
    if let Some(recording) = crate::meeting::recording_note() {
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
    let started = crate::commands::meeting_start(app.clone(), app.state::<crate::AppState>(), note_id);
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
    if let Some(current) = crate::meeting::recording_note() {
        if crate::notes::find_by_event(key) == Some(current) {
            return Ok(current);
        }
        tracing::info!(note_id = current, "calendar: switching notes to the next meeting");
        crate::meeting_end::end(app, Some("calendar"))?;
        // The recorder transcribes its last chunk before it lets go.
        for _ in 0..240 {
            if crate::meeting::recording_note().is_none() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        if crate::meeting::recording_note().is_some() {
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

/// A recording started into `note_id`, whichever way (`commands::meeting_start`).
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
    let calls: Vec<String> = serde_json::to_value(crate::meeting_detect::status())
        .ok()
        .and_then(|v| v.get("calls").cloned())
        .and_then(|c| serde_json::from_value::<Vec<Value>>(c).ok())
        .unwrap_or_default()
        .iter()
        .filter_map(|c| c.get("app").and_then(Value::as_str).map(str::to_string))
        .collect();
    let event = {
        let s = lock();
        calls
            .iter()
            .rev()
            .find_map(|app_id| model::event_at(&s.store.events, now, Some(app_id)))
            .or_else(|| model::event_at(&s.store.events, now, None))
            .cloned()
    };
    let Some(event) = event else { return };
    // Resuming an older meeting's notes isn't this meeting.
    if let Some(first) = note.transcript.first().map(|s| s.ts as i64) {
        if first < event.start - model::EARLY_SECS || first > event.end {
            return;
        }
    }
    // Over a title Yap made up ("Teams call · 5 Oct, 14:30"): the meeting's
    // real name, which the AI meeting title then leaves alone.
    let made_up = crate::notes::title_open_to_ai(&note) || model::is_placeholder_title(&note.title);
    let title = made_up.then(|| event.display_title());
    if let Err(e) = crate::notes::link_event(note_id, note_event(&event), title, &attendee_names(&event), None) {
        tracing::warn!("calendar: couldn't tie the note to its meeting: {e}");
        return;
    }
    answer(&event);
    tracing::info!(note_id, "calendar: a recording started during a meeting; note tied to it");
    // Every window showing the note (the notepad, Notes) follows.
    crate::commands::note_changed(app, note_id, "calendar");
    let _ = app.emit("yap-notes-changed", ());
    emit(app);
}

/// Call detection, about to ask "Teams call detected — Record notes?" (under
/// its own lock: no windows, no other locks but this one). `true` when the
/// call belongs to a meeting whose card is up or was answered: the calendar
/// asked already. A snoozed card comes back instead.
pub fn claims_call(app_id: &str) -> bool {
    let Some(mut s) = lock_started() else { return false };
    let now = now_secs();
    let Some(event) = model::event_at(&s.store.events, now, Some(app_id)).cloned() else {
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

/// A call started (after call detection's lock is released). Recording an
/// earlier meeting's notes while this call belongs to the next meeting:
/// offer to switch notes (back-to-back meetings).
pub fn on_call_started(app_id: &str) {
    if lock_started().is_none() {
        return;
    }
    let Some(recording) = crate::meeting::recording_note() else { return };
    let recording_key = crate::notes::get(recording).and_then(|n| n.event).map(|e| e.key);
    let mut s = lock();
    let now = now_secs();
    let Some(next) = model::event_at(&s.store.events, now, Some(app_id)).cloned() else { return };
    if recording_key.as_deref() == Some(next.key.as_str()) {
        return; // the same meeting: a rejoin
    }
    if let Some(previous) = recording_key.as_deref().and_then(|k| s.store.events.iter().find(|e| e.key == k)) {
        if previous.start >= next.start {
            return; // only ever forward, to the next meeting
        }
    }
    if s.store.answered.contains_key(&model::answer_key(&next)) {
        return;
    }
    tracing::info!(app = app_id, "calendar: a call during the next meeting; offering to switch notes");
    s.switch_to = Some(next.key);
    drop(s);
    wake();
}

// ---- the reminder card ------------------------------------------------------------------------

/// The main window: (on screen, focused). Never with a lock held. A test
/// run never focuses its windows, so one the desktop activates behind the
/// suite's back doesn't count as focused (as in `meeting_detect`).
fn window_view(app: &AppHandle) -> (bool, bool) {
    app.get_webview_window("settings")
        .map(|w| {
            let visible = w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false);
            let focused = !crate::e2e::active() && w.is_focused().unwrap_or(false);
            (visible, visible && focused)
        })
        .unwrap_or((false, false))
}

/// What a card is waiting for, after a turn of [`tick_card`].
enum CardChange {
    Nothing,
    /// Take this card down, and maybe put that one up.
    Replace(Option<Box<Card>>, Option<Box<Card>>),
    /// The card up stays: put it on the bar (or a notification) after all —
    /// the main window lost the focus, or the bar went away under it.
    Present(Box<Card>),
    /// The card up stays: its status line on the bar reads this now.
    Status(String, &'static str),
}

/// Decide which card should be up, and make it so.
fn tick_card(app: &AppHandle) {
    let now = now_secs();
    let lead = reminder_lead(&crate::config::load().meeting_reminder);
    let recording = crate::meeting::recording_note();
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
        CardChange::Status(status, dot) => {
            crate::bar::update_card(app, BAR_CARD, |c| {
                c.status = Some(status);
                c.dot = dot;
            });
        }
    }
}

/// **The one place a calendar card goes on screen.** The main window's
/// in-app toast follows the snapshot (`calendar.svelte.js`), and while the
/// window isn't focused (the call app is in front, Yap is in the tray) the
/// card goes on the **Yap bar** (`bar::show_card`, a "Meeting detected"-style
/// call card), else — the bar off or hidden for an hour — a Windows
/// notification: as call detection's prompts do.
fn present_card(app: &AppHandle, card: &Card, focused: bool) {
    tracing::info!(kind = ?card.kind, focused, "calendar: meeting card");
    emit(app);
    let now = now_secs();
    let surface = if focused {
        Surface::InApp
    } else if crate::bar::show_card(app, bar_card(card, now), Box::new(answer_from)) {
        Surface::Bar
    } else {
        native_post(app, card)
    };
    let mut s = lock();
    match s.card.as_mut().filter(|c| c.id == card.id) {
        Some(c) => {
            c.surface = surface;
            c.bar_status = bar_status(card, now).0;
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
        crate::win_toast::allowed()
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
                    let _ = crate::commands::show_settings(&app);
                    let _ = app.emit("yap-error", format!("Couldn't start meeting notes. {e}"));
                }
            }
            _ => {
                let _ = crate::commands::show_settings(&app);
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

// ---- past meeting notes -------------------------------------------------------------------------

/// Meeting notes for the Meetings view, newest meeting first, matching every
/// word of `query` (title, attendees, notes, action plan, transcript).
/// Drafts made ahead of a meeting still to come are left out (the meeting
/// is listed above them).
#[tauri::command]
pub fn calendar_meeting_notes(query: Option<String>) -> Vec<Value> {
    let words: Vec<String> = query.unwrap_or_default().to_lowercase().split_whitespace().map(str::to_string).collect();
    let now = now_secs().max(0) as u64;
    let recording = crate::meeting::recording_note();
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
                "recording": recording == Some(n.id),
            });
            (when, value)
        })
        .collect();
    out.sort_by_key(|a| std::cmp::Reverse(a.0));
    out.into_iter().take(200).map(|(_, v)| v).collect()
}

/// The invite's description, as context for a meeting's action plan and its
/// Ask bar (`meeting_summary`, `commands::note_ask`): `None` without one.
pub fn invite_context(note: &crate::notes::Note) -> Option<String> {
    let event = note.event.as_ref().filter(|e| !e.description.trim().is_empty())?;
    Some(format!("From the calendar invite \"{}\":\n{}\n", event.title, event.description.trim()))
}

/// Characters of each meeting given to a question from the Meetings view.
const ASK_NOTE_CHARS: usize = 2_400;

/// When a meeting note's meeting was: the event's start, else its first
/// transcript line, else when the note was made.
fn meeting_time(n: &crate::notes::Note) -> u64 {
    n.event.as_ref().map(|e| e.start).or_else(|| n.transcript.first().map(|s| s.ts)).unwrap_or(n.created_ts)
}

/// Context for a question asked from the Meetings view's Ask bar
/// (`commands::chat_send` with scope "meetings"): the three latest meetings
/// and up to three more that match the question, newest first, each as a
/// `<note>` block with its date and attendees, the action plan or else the
/// typed notes and the transcript (a long one as its digests), bounded so a
/// small local model still fits.
pub fn meetings_context(query: &str) -> String {
    use chrono::TimeZone;
    let all = crate::notes::all();
    let mut meetings: Vec<&crate::notes::Note> = all
        .iter()
        .filter(|n| n.note_type == "meeting")
        .filter(|n| !n.transcript.is_empty() || !n.enhanced_content.trim().is_empty() || !n.content.trim().is_empty())
        .collect();
    meetings.sort_by_key(|n| std::cmp::Reverse(meeting_time(n)));
    let mut picked: Vec<&crate::notes::Note> = meetings.iter().take(3).copied().collect();
    for (_, hit) in crate::tools::search_notes(query, 20) {
        if picked.len() >= 6 {
            break;
        }
        if let Some(n) = meetings.iter().find(|n| n.id == hit.id) {
            if !picked.iter().any(|p| p.id == n.id) {
                picked.push(n);
            }
        }
    }
    picked.sort_by_key(|n| std::cmp::Reverse(meeting_time(n)));
    picked
        .iter()
        .map(|n| {
            let mut body = String::new();
            if let Some(invite) = invite_context(n) {
                body.push_str(&invite);
            }
            if !n.enhanced_content.trim().is_empty() {
                body.push_str(n.enhanced_content.trim());
            } else {
                if !n.content.trim().is_empty() {
                    body.push_str(n.content.trim());
                    body.push('\n');
                }
                if !n.transcript.is_empty() {
                    body.push_str(&crate::meeting_summary::ask_context(n, 600));
                }
            }
            let body: String = body.chars().take(ASK_NOTE_CHARS).collect();
            let date = chrono::Local
                .timestamp_opt(meeting_time(n) as i64, 0)
                .single()
                .map(|t| t.format("%a %-d %b %Y, %H:%M").to_string())
                .unwrap_or_default();
            let quote = |s: &str| s.replace('"', "'");
            format!(
                "<note id=\"{}\" title=\"{}\" date=\"{}\" attendees=\"{}\">\n{}\n</note>",
                n.id,
                quote(if n.title.trim().is_empty() { "Untitled meeting" } else { n.title.trim() }),
                date,
                quote(&n.participants.join(", ")),
                body.trim()
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

// ---- test mode ------------------------------------------------------------------------------------

/// Test mode only: the links Yap would have opened in the browser (meeting
/// links, Google's consent page), oldest first.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn calendar_e2e_opened() -> Result<Vec<String>, String> {
    if !crate::e2e::active() {
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
    use crate::win_toast::{esc, logo_xml};

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
        if !crate::win_toast::allowed() {
            return Err("off in portable mode and test runs".to_string());
        }
        let toast = crate::win_toast::post(app, TAG, &xml(card, &logo_xml()), None, activated)?;
        *CURRENT.lock().unwrap_or_else(|p| p.into_inner()) = Some(toast);
        Ok(())
    }

    pub fn remove(app: &AppHandle) {
        if CURRENT.lock().unwrap_or_else(|p| p.into_inner()).take().is_some() {
            crate::win_toast::remove(app, TAG);
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
        let feed = b"BEGIN:VCALENDAR\r\nX-WR-CALNAME:nathan@example.com\r\n\
            BEGIN:VEVENT\r\nUID:a\r\nSUMMARY:Design review\r\nDTSTART:20261005T140000Z\r\nDTEND:20261005T143000Z\r\n\
            ATTENDEE;CN=Tanay Kothari:mailto:tanay@example.com\r\nATTENDEE;CN=Nathan:mailto:nathan@example.com\r\nEND:VEVENT\r\n\
            BEGIN:VEVENT\r\nUID:b\r\nSUMMARY:Focus\r\nDTSTART:20261005T150000Z\r\nDTEND:20261005T160000Z\r\nEND:VEVENT\r\n\
            BEGIN:VEVENT\r\nUID:c\r\nSUMMARY:Offsite\r\nDTSTART;VALUE=DATE:20261005\r\nDTEND;VALUE=DATE:20261006\r\n\
            ATTENDEE:mailto:tanay@example.com\r\nEND:VEVENT\r\n\
            BEGIN:VEVENT\r\nUID:d\r\nSUMMARY:Workshop\r\nDTSTART:20261005T080000Z\r\nDTEND:20261005T150000Z\r\n\
            ATTENDEE:mailto:tanay@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let (events, name) = events_from_feed("1", feed, 1_791_000_000, 1_792_000_000).unwrap();
        assert_eq!(name.as_deref(), Some("nathan@example.com"));
        assert_eq!(events.iter().map(|e| e.title.as_str()).collect::<Vec<_>>(), ["Design review"]);
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
        crate::win_toast::build("calendar", &xml, false).unwrap();
        // Without a link, and as a switch: no join, and the question.
        let mut plain = card.clone();
        plain.kind = CardKind::Switch;
        plain.event.join_url = None;
        let xml = native::xml(&plain, "");
        assert_eq!(xml.matches("<action ").count(), 2);
        assert!(xml.contains("content=\"Switch notes\" arguments=\"calendar:start:7\""));
        assert!(xml.contains("Switch your notes to this meeting?"));
        crate::win_toast::build("calendar", &xml, false).unwrap();
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
