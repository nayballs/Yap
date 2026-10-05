//! Call detection — Yap notices a call starting (Teams, Zoom, Google Meet,
//! Slack huddles, Discord, Webex, …), offers to take notes, and offers to wrap
//! up when the call ends. A port of OpenWhispr's `meetingDetectionEngine.js`
//! (with its `audioActivityDetector.js` / `meetingProcessDetector.js`) onto
//! Windows' own record of which app is using the microphone.
//!
//! **Signal.** Windows keeps a per-app microphone record — the data behind the
//! taskbar's mic indicator and Settings → Privacy → Microphone — under
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\
//! ConsentStore\microphone`: one subkey per packaged app (new Teams is
//! `MSTeams_8wekyb3d8bbwe`) plus `NonPackaged\<exe path, '\' spelled '#'>` for
//! desktop apps, each with `LastUsedTimeStart` / `LastUsedTimeStop` FILETIMEs.
//! A stop of 0 means "on the mic right now". Only a known call app counts
//! ([`APPS`]); a browser counts when one of its windows shows a meeting (Meet,
//! Teams, Zoom … in the title) and keeps that call while the person switches
//! tabs. Dictation apps, games, OBS and Yap itself (it holds the mic for its
//! pre-roll, dictation and the meeting recorder) never match. OpenWhispr runs
//! a helper exe on WASAPI capture sessions and prompts for any app's mic use
//! instead; on a typical machine most mic users are not calls.
//!
//! **Cost.** A thread blocks in `RegNotifyChangeKeyValue` on that key, so
//! nothing runs until some app takes or lets go of the mic; a scan then reads
//! the values of known apps' subkeys only. While a call is starting, live or
//! ending, the detector rescans every 2 s as well. Fully local, no network.
//!
//! **Debounce** (OpenWhispr: mic busy ≥ 2 s, a 5-minute cooldown after a
//! dismissal, prompts held while the user dictates and shown 2.5 s after):
//! - a call starts once its app has held the mic for 5 s (20 s for chat apps,
//!   whose voice messages use the mic too);
//! - it ends once the app has let go for 15 s, so a device switch or a
//!   rejoin is a gap, not an ending;
//! - "Not now" quiets that app's calls for 5 minutes, and prompts wait while
//!   a dictation records or transcribes.
//!
//! **Asking** — once per call, only while Yap isn't recording a meeting
//! already: an in-app toast when the main window is on screen, a Windows
//! notification with Yap's logo otherwise. A window that is open but not
//! focused (the call app is in front) gets both; answering one withdraws the
//! other, and focusing the window moves a pending prompt into it. "Record
//! notes" creates a meeting note ("Teams call · 5 Oct, 14:30", Meetings
//! folder) and starts the meeting recorder. Nothing records without that click.
//!
//! **Ending.** When a call Yap is recording ends: "Stop and summarise?"
//! OpenWhispr never stops a meeting recording on its own (its calendar
//! "meeting ended" event isn't acted on), and the mic signal can't tell a
//! finished call from one moved to a phone, a breakout room or a rejoin, so
//! Yap asks instead of stopping. Stopping opens the note and stops it there,
//! which runs the usual Meeting Notes summary.
//!
//! Test mode (`e2e::active`) reads no registry and posts no Windows
//! notifications; the debug-only [`meeting_detect_simulate`] drives the flow.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{LazyLock, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

/// A call starts once its app has held the mic this long…
const START_AFTER_MS: u64 = 5_000;
/// …or this long for a chat app (its voice messages use the mic too).
const CHAT_START_AFTER_MS: u64 = 20_000;
/// A call ends once its app has let go of the mic for this long.
const END_AFTER_MS: u64 = 15_000;
/// "Not now" quiets that app's next calls for this long (OpenWhispr: 5 min).
const SNOOZE_MS: u64 = 5 * 60_000;
/// A held-back prompt shows this long after a dictation (OpenWhispr: 2.5 s).
const AFTER_DICTATION_MS: u64 = 2_500;
/// Rescans while a call is starting, live or ending, or a prompt is up.
const ACTIVE_TICK: Duration = Duration::from_secs(2);
/// Rescans otherwise — a safety net under the registry change notifications…
const IDLE_TICK: Duration = Duration::from_secs(60);
/// …or the cadence without them (the watch couldn't start).
const POLL_TICK: Duration = Duration::from_secs(5);
/// A "Stop and summarise" the Notes view didn't carry out in this time is
/// done here, so the recording stops as asked.
const STOP_FALLBACK: Duration = Duration::from_secs(8);

/// Snapshot of calls and the pending prompt (`meeting_detect_status`).
const EVENT: &str = "yap-meeting-detect";
/// Open a note in the main window: `{ noteId, stop }` (`stop`: stop its
/// recording there, which runs the summary).
const EVENT_OPEN: &str = "yap-meeting-open-note";

// ---- call apps -------------------------------------------------------------

/// How a browser window's title names a meeting service (lowercased).
enum Title {
    Starts(&'static str),
    Has(&'static str),
}
use Title::{Has, Starts};

/// A call app Yap recognises.
pub struct App {
    pub id: &'static str,
    /// For the prompt and the note title: "Teams", "Google Meet".
    pub label: &'static str,
    /// "call" ("huddle" for Slack).
    pub noun: &'static str,
    /// Desktop exes (lowercase file names), as in the NonPackaged keys.
    exes: &'static [&'static str],
    /// Packaged apps: package names (the family name minus its publisher hash).
    packages: &'static [&'static str],
    /// The web version, recognised in a browser window's title.
    titles: &'static [Title],
    /// A chat app: its voice messages use the mic too, so wait longer.
    chat: bool,
}

/// Order matters for browser titles: the first match names the call.
pub static APPS: &[App] = &[
    App {
        id: "teams",
        label: "Teams",
        noun: "call",
        exes: &["ms-teams.exe", "teams.exe"],
        packages: &["MSTeams", "MicrosoftTeams"],
        titles: &[Has("microsoft teams")],
        chat: false,
    },
    App {
        id: "zoom",
        label: "Zoom",
        noun: "call",
        exes: &["zoom.exe"],
        packages: &[],
        titles: &[Has("zoom meeting"), Has("zoom webinar"), Starts("zoom")],
        chat: false,
    },
    App {
        id: "meet",
        label: "Google Meet",
        noun: "call",
        exes: &[],
        packages: &[],
        titles: &[Starts("meet - "), Starts("meet \u{2013} "), Has("google meet")],
        chat: false,
    },
    App {
        id: "webex",
        label: "Webex",
        noun: "call",
        exes: &["webex.exe", "ciscocollabhost.exe", "atmgr.exe", "webexmeetingsapp.exe"],
        packages: &[],
        titles: &[Has("webex")],
        chat: false,
    },
    App {
        id: "slack",
        label: "Slack",
        noun: "huddle",
        exes: &["slack.exe"],
        packages: &["91750D7E.Slack"],
        titles: &[Has("slack")],
        chat: true,
    },
    App {
        id: "discord",
        label: "Discord",
        noun: "call",
        exes: &["discord.exe", "discordptb.exe", "discordcanary.exe"],
        packages: &[],
        titles: &[Has("discord")],
        chat: false,
    },
    App {
        id: "goto",
        label: "GoTo Meeting",
        noun: "call",
        exes: &["goto.exe", "g2mcomm.exe"],
        packages: &[],
        titles: &[Has("goto meeting"), Has("gotomeeting")],
        chat: false,
    },
    App {
        id: "whereby",
        label: "Whereby",
        noun: "call",
        exes: &[],
        packages: &[],
        titles: &[Has("whereby")],
        chat: false,
    },
    App {
        id: "jitsi",
        label: "Jitsi Meet",
        noun: "call",
        exes: &[],
        packages: &[],
        titles: &[Has("jitsi")],
        chat: false,
    },
    App {
        id: "whatsapp",
        label: "WhatsApp",
        noun: "call",
        exes: &["whatsapp.exe"],
        packages: &["5319275A.WhatsAppDesktop"],
        titles: &[Has("whatsapp")],
        chat: true,
    },
    App {
        id: "signal",
        label: "Signal",
        noun: "call",
        exes: &["signal.exe"],
        packages: &[],
        titles: &[],
        chat: true,
    },
    App {
        id: "telegram",
        label: "Telegram",
        noun: "call",
        exes: &["telegram.exe"],
        packages: &["TelegramMessengerLLP.TelegramDesktop"],
        titles: &[],
        chat: true,
    },
];

/// Browsers (lowercase exe names): they count when a window shows a meeting.
const BROWSERS: &[&str] = &[
    "chrome.exe", "msedge.exe", "firefox.exe", "brave.exe", "opera.exe", "vivaldi.exe",
    "arc.exe", "zen.exe", "floorp.exe", "librewolf.exe", "waterfox.exe", "chromium.exe",
];
/// Packaged browsers: (package name, the exe their windows belong to).
const BROWSER_PACKAGES: &[(&str, &str)] = &[("TheBrowserCompany.Arc", "arc.exe")];

fn app_by_id(id: &str) -> Option<&'static App> {
    APPS.iter().find(|a| a.id == id)
}

/// A microphone user, as the consent store names it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Source {
    /// A desktop app: its exe file name, lowercased.
    Exe(String),
    /// A packaged app: its package name.
    Package(String),
}

/// A `NonPackaged` subkey is the exe's full path with `\` spelled `#`.
fn exe_source(key: &str) -> Source {
    Source::Exe(key.rsplit('#').next().unwrap_or(key).to_ascii_lowercase())
}

/// A packaged app's subkey is its family name: `MSTeams_8wekyb3d8bbwe`.
fn package_source(key: &str) -> Source {
    Source::Package(key.split('_').next().unwrap_or(key).to_string())
}

/// What a microphone user is to call detection.
#[derive(Debug, PartialEq, Eq)]
enum Kind {
    Call(&'static str),
    /// A browser, by the exe its windows belong to.
    Browser(String),
}

fn classify(src: &Source) -> Option<Kind> {
    match src {
        Source::Exe(exe) => APPS
            .iter()
            .find(|a| a.exes.contains(&exe.as_str()))
            .map(|a| Kind::Call(a.id))
            .or_else(|| BROWSERS.contains(&exe.as_str()).then(|| Kind::Browser(exe.clone()))),
        Source::Package(name) => APPS
            .iter()
            .find(|a| a.packages.iter().any(|p| p.eq_ignore_ascii_case(name)))
            .map(|a| Kind::Call(a.id))
            .or_else(|| {
                BROWSER_PACKAGES
                    .iter()
                    .find(|(p, _)| p.eq_ignore_ascii_case(name))
                    .map(|(_, exe)| Kind::Browser(exe.to_string()))
            }),
    }
}

/// The meeting service a browser window shows, from its title.
fn app_in_title(title: &str) -> Option<&'static App> {
    let t = title.to_lowercase();
    APPS.iter().find(|a| {
        a.titles.iter().any(|m| match m {
            Starts(prefix) => t.starts_with(prefix),
            Has(needle) => t.contains(needle),
        })
    })
}

/// On the mic right now: `LastUsedTimeStop` is 0 — unless the start predates
/// this boot (an app that died mid-call before a restart can leave that).
fn on_mic(start: u64, stop: u64, boot: u64) -> bool {
    start != 0 && stop == 0 && start >= boot
}

// ---- start/end debounce --------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// On the mic, not long enough to count yet.
    Starting { since: u64 },
    /// A call. `quiet_since`: when its app let go of the mic (it may be back).
    Live { since: u64, quiet_since: Option<u64> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Edge {
    /// A call started; `since`: when its app took the mic (ms).
    Started { app: &'static str, since: u64 },
    Ended { app: &'static str },
}

/// Per call app: on the mic long enough to be a call; off it long enough to
/// have ended.
#[derive(Default)]
struct Tracker {
    apps: HashMap<&'static str, Phase>,
}

impl Tracker {
    /// The call apps on the mic at `now` (ms) → the calls that started or ended.
    fn update(&mut self, now: u64, on_mic: &[&'static App]) -> Vec<Edge> {
        let mut edges = Vec::new();
        for app in on_mic {
            let phase = self.apps.entry(app.id).or_insert(Phase::Starting { since: now });
            match *phase {
                Phase::Starting { since } => {
                    let wait = if app.chat { CHAT_START_AFTER_MS } else { START_AFTER_MS };
                    if now.saturating_sub(since) >= wait {
                        *phase = Phase::Live { since, quiet_since: None };
                        edges.push(Edge::Started { app: app.id, since });
                    }
                }
                Phase::Live { since, .. } => *phase = Phase::Live { since, quiet_since: None },
            }
        }
        let mut gone = Vec::new();
        for (id, phase) in self.apps.iter_mut() {
            if on_mic.iter().any(|a| a.id == *id) {
                continue;
            }
            match *phase {
                Phase::Starting { .. } => gone.push((*id, false)),
                Phase::Live { since, quiet_since } => {
                    let quiet = quiet_since.unwrap_or(now);
                    if now.saturating_sub(quiet) >= END_AFTER_MS {
                        gone.push((*id, true));
                    } else {
                        *phase = Phase::Live { since, quiet_since: Some(quiet) };
                    }
                }
            }
        }
        for (id, was_call) in gone {
            self.apps.remove(id);
            if was_call {
                edges.push(Edge::Ended { app: id });
            }
        }
        edges
    }

    /// Start or end a call at once (the debug-only simulation).
    fn force(&mut self, app: &'static str, live: bool, now: u64) -> Option<Edge> {
        match (live, self.apps.get(app).copied()) {
            (true, Some(Phase::Live { since, .. })) => {
                self.apps.insert(app, Phase::Live { since, quiet_since: None });
                None
            }
            (true, _) => {
                self.apps.insert(app, Phase::Live { since: now, quiet_since: None });
                Some(Edge::Started { app, since: now })
            }
            (false, Some(Phase::Live { .. })) => {
                self.apps.remove(app);
                Some(Edge::Ended { app })
            }
            (false, _) => {
                self.apps.remove(app);
                None
            }
        }
    }

    /// Watching a call closely (starting, live or ending)?
    fn busy(&self) -> bool {
        !self.apps.is_empty()
    }
}

// ---- state ---------------------------------------------------------------------------

/// A call in progress.
struct Call {
    id: u64,
    app: &'static App,
    /// When its app took the mic (ms).
    since: u64,
    /// The meeting note its prompt started recording into.
    note_id: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum PromptKind {
    /// "Teams call detected — Record notes?"
    Start,
    /// "Teams call ended — Stop and summarise?"
    End,
}

#[derive(Clone)]
struct Prompt {
    id: u64,
    kind: PromptKind,
    call_id: u64,
    app: &'static App,
    /// End prompts: the note being recorded.
    note_id: Option<u64>,
    /// Shown as the in-app toast (the main window is, or came, on screen).
    in_app: bool,
    /// Posted as a Windows notification.
    native: bool,
}

struct State {
    enabled: bool,
    tracker: Tracker,
    /// Browser exe on the mic → the meeting one of its windows showed.
    browser_calls: HashMap<String, &'static App>,
    /// Apps the debug-only simulation holds "on the mic".
    simulated: HashSet<&'static str>,
    calls: Vec<Call>,
    prompt: Option<Prompt>,
    /// Prompts already answered (an answer can arrive from both surfaces).
    answered: Vec<u64>,
    /// The meeting note a "Record notes" started: a later call while it's
    /// still recording (a rejoin) is asked about at its end too.
    our_note: Option<u64>,
    /// A call waiting for its start prompt (held back while dictating).
    due: Option<u64>,
    /// App id → until when "Not now" quiets its calls (ms).
    snoozed: HashMap<&'static str, u64>,
    /// When a dictation last recorded or transcribed (ms).
    last_dictation: u64,
    seq: u64,
}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| {
    Mutex::new(State {
        enabled: false,
        tracker: Tracker::default(),
        browser_calls: HashMap::new(),
        simulated: HashSet::new(),
        calls: Vec::new(),
        prompt: None,
        answered: Vec::new(),
        our_note: None,
        due: None,
        snoozed: HashMap::new(),
        last_dictation: 0,
        seq: 0,
    })
});
/// Wakes the detector thread (the registry watch, settings, the simulation).
static WAKE: Mutex<Option<mpsc::Sender<()>>> = Mutex::new(None);
/// The registry watch is running (else the detector polls).
static WATCHING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static STARTED: OnceLock<()> = OnceLock::new();

/// Never held while touching windows or WinRT: window queries wait on the
/// main thread, which may itself be waiting for this lock.
fn lock() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|p| p.into_inner())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn wake() {
    if let Some(tx) = WAKE.lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
        let _ = tx.send(());
    }
}

// ---- snapshot ------------------------------------------------------------------------

/// What the main window renders (camelCase, `meeting_detect_status` + the
/// `yap-meeting-detect` event).
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    enabled: bool,
    calls: Vec<CallView>,
    prompt: Option<PromptView>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CallView {
    id: u64,
    app: &'static str,
    label: &'static str,
    noun: &'static str,
    /// Unix seconds.
    since: u64,
    note_id: Option<u64>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PromptView {
    id: u64,
    kind: PromptKind,
    call_id: u64,
    app: &'static str,
    title: String,
    body: &'static str,
    accept: &'static str,
    decline: &'static str,
    note_id: Option<u64>,
    /// Show it as the in-app toast.
    in_app: bool,
}

/// One wording for both surfaces (the in-app toast, the Windows notification).
struct Wording {
    title: String,
    body: &'static str,
    accept: &'static str,
    decline: &'static str,
}

fn wording(kind: PromptKind, app: &App) -> Wording {
    match kind {
        PromptKind::Start => Wording {
            title: format!("{} {} detected", app.label, app.noun),
            body: "Record notes? Let people know you're taking notes.",
            accept: "Record notes",
            decline: "Not now",
        },
        PromptKind::End => Wording {
            title: format!("{} {} ended", app.label, app.noun),
            body: "Stop recording and summarise your notes?",
            accept: "Stop and summarise",
            decline: "Keep recording",
        },
    }
}

fn status_of(s: &State) -> Status {
    Status {
        enabled: s.enabled,
        calls: s
            .calls
            .iter()
            .map(|c| CallView {
                id: c.id,
                app: c.app.id,
                label: c.app.label,
                noun: c.app.noun,
                since: c.since / 1000,
                note_id: c.note_id,
            })
            .collect(),
        prompt: s.prompt.as_ref().map(|p| {
            let w = wording(p.kind, p.app);
            PromptView {
                id: p.id,
                kind: p.kind,
                call_id: p.call_id,
                app: p.app.id,
                title: w.title,
                body: w.body,
                accept: w.accept,
                decline: w.decline,
                note_id: p.note_id,
                in_app: p.in_app,
            }
        }),
    }
}

pub fn status() -> Status {
    status_of(&lock())
}

fn emit(app: &AppHandle) {
    let _ = app.emit(EVENT, status());
}

// ---- side effects (run after the lock is released) ----------------------------------------

/// What a decision under the lock asks for.
#[derive(Default)]
struct Todo {
    /// Take the call notification out of the notification center.
    remove_native: bool,
    /// Post this prompt as a Windows notification.
    post_native: Option<Prompt>,
    emit: bool,
}

fn run(app: &AppHandle, todo: Todo) {
    #[cfg(windows)]
    {
        if todo.remove_native {
            native::remove(app);
        }
        if let Some(p) = todo.post_native {
            match native::post_prompt(app, &p) {
                Ok(()) => {
                    if let Some(cur) = lock().prompt.as_mut().filter(|cur| cur.id == p.id) {
                        cur.native = true;
                    }
                }
                Err(e) => tracing::info!("meeting detect: no Windows notification ({e})"),
            }
        }
    }
    #[cfg(not(windows))]
    let _ = (todo.remove_native, todo.post_native);
    if todo.emit {
        emit(app);
    }
}

/// Retire the pending prompt (answered, moot, or its call ended).
fn withdraw(s: &mut State, todo: &mut Todo) {
    if let Some(p) = s.prompt.take() {
        todo.remove_native |= p.native;
        todo.emit = true;
    }
}

/// The main window: (on screen, focused). Never call with the lock held.
fn window_view(app: &AppHandle) -> (bool, bool) {
    app.get_webview_window("settings")
        .map(|w| {
            let visible = w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false);
            (visible, visible && w.is_focused().unwrap_or(false))
        })
        .unwrap_or((false, false))
}

/// A dictation is recording or transcribing. Locks the pipeline, so never
/// from a `yap-state` listener (see `updates::busy_reason`).
fn dictating(app: &AppHandle) -> bool {
    let Some(state) = app.try_state::<crate::AppState>() else {
        return false;
    };
    let Ok(guard) = state.pipeline.lock() else {
        return false;
    };
    guard.as_ref().is_some_and(|p| p.is_busy())
}

/// Put up a prompt (replacing whatever was up): the in-app toast when the
/// main window is on screen, a Windows notification unless it's focused.
fn show_prompt(
    s: &mut State,
    todo: &mut Todo,
    view: (bool, bool),
    kind: PromptKind,
    call_id: u64,
    app: &'static App,
    note_id: Option<u64>,
) {
    withdraw(s, todo);
    s.seq += 1;
    let (visible, focused) = view;
    let prompt = Prompt { id: s.seq, kind, call_id, app, note_id, in_app: visible, native: false };
    if !focused {
        todo.post_native = Some(prompt.clone());
    }
    tracing::info!(app = app.id, ?kind, in_app = visible, "meeting detect: prompt");
    s.prompt = Some(prompt);
    todo.emit = true;
}

// ---- calls starting and ending ------------------------------------------------------------

/// The note Yap is recording `call` into, if any: the one its prompt started,
/// or a recording started (give or take a minute) after the call did.
fn recording_of(call: &Call) -> Option<u64> {
    let st = crate::meeting::state();
    if st["recording"].as_bool() != Some(true) {
        return None;
    }
    let note = st["noteId"].as_u64()?;
    if call.note_id == Some(note) {
        return Some(note);
    }
    let started = now_ms().saturating_sub(st["elapsedSecs"].as_u64().unwrap_or(0) * 1000);
    (started + 60_000 >= call.since).then_some(note)
}

fn on_edges(app: &AppHandle, edges: Vec<Edge>) {
    if edges.is_empty() {
        return;
    }
    let view = window_view(app);
    let mut todo = Todo::default();
    {
        let mut s = lock();
        for edge in edges {
            match edge {
                Edge::Started { app: id, since } => call_started(&mut s, &mut todo, id, since),
                Edge::Ended { app: id } => call_ended(&mut s, &mut todo, view, id),
            }
        }
    }
    run(app, todo);
}

fn call_started(s: &mut State, todo: &mut Todo, id: &'static str, since: u64) {
    let Some(app) = app_by_id(id) else { return };
    s.seq += 1;
    let call_id = s.seq;
    // Still recording the note an earlier call's "Record notes" started (a
    // rejoin, or "Keep recording"): this call carries it on, and a pending
    // "…ended — Stop and summarise?" for the same app is moot.
    let recording_note = crate::meeting::state()["noteId"].as_u64();
    let carried = recording_note.filter(|note| s.our_note == Some(*note));
    let rejoin = s.prompt.as_ref().is_some_and(|p| p.kind == PromptKind::End && p.app.id == id);
    if carried.is_some() && rejoin {
        withdraw(s, todo);
    }
    s.calls.push(Call { id: call_id, app, since, note_id: carried });
    todo.emit = true;
    tracing::info!(app = id, carried = carried.is_some(), "meeting detect: call started");
    let snoozed = s.snoozed.get(id).is_some_and(|until| now_ms() < *until);
    if recording_note.is_none() && !snoozed {
        s.due = Some(call_id);
    }
}

fn call_ended(s: &mut State, todo: &mut Todo, view: (bool, bool), id: &str) {
    let Some(pos) = s.calls.iter().position(|c| c.app.id == id) else { return };
    let call = s.calls.remove(pos);
    todo.emit = true;
    tracing::info!(app = id, "meeting detect: call ended");
    if s.due == Some(call.id) {
        s.due = None;
    }
    if let Some(note) = recording_of(&call) {
        show_prompt(s, todo, view, PromptKind::End, call.id, call.app, Some(note));
    } else if s.prompt.as_ref().is_some_and(|p| p.call_id == call.id) {
        withdraw(s, todo);
    }
}

/// Housekeeping after every scan: hold prompts while dictating, show a due
/// one, retire prompts that became moot.
fn tick(app: &AppHandle) {
    let busy = dictating(app);
    let recording = crate::meeting::is_recording();
    let wants_view = {
        let s = lock();
        s.enabled && s.due.is_some() && s.prompt.is_none()
    };
    let view = if wants_view { window_view(app) } else { (false, false) };
    let now = now_ms();
    let mut todo = Todo::default();
    {
        let mut s = lock();
        if !s.enabled {
            return;
        }
        if busy {
            s.last_dictation = now;
        }
        // Moot: recording started some other way, or the recording stopped.
        let moot = s.prompt.as_ref().is_some_and(|p| match p.kind {
            PromptKind::Start => recording,
            PromptKind::End => !recording,
        });
        if moot {
            withdraw(&mut s, &mut todo);
        }
        if recording {
            s.due = None;
        }
        if let Some(call_id) = s.due {
            let quiet = now.saturating_sub(s.last_dictation) >= AFTER_DICTATION_MS;
            if wants_view && s.prompt.is_none() && quiet {
                s.due = None;
                if let Some(app) = s.calls.iter().find(|c| c.id == call_id).map(|c| c.app) {
                    show_prompt(&mut s, &mut todo, view, PromptKind::Start, call_id, app, None);
                }
            }
        }
    }
    run(app, todo);
}

// ---- the detector thread ------------------------------------------------------------

/// Start call detection (app setup): the detector thread and, outside test
/// mode, the registry watch that wakes it.
pub fn init(app: &AppHandle) {
    if STARTED.set(()).is_err() {
        return;
    }
    let enabled = crate::config::load().meeting_detection;
    lock().enabled = enabled;
    let (tx, rx) = mpsc::channel();
    let _ = tx.send(()); // a first scan: a call may be under way already
    *WAKE.lock().unwrap_or_else(|p| p.into_inner()) = Some(tx.clone());
    let handle = app.clone();
    let spawned = std::thread::Builder::new()
        .name("meeting-detect".into())
        .spawn(move || detector(handle, rx));
    if let Err(e) = spawned {
        tracing::warn!("meeting detect: couldn't start ({e})");
        return;
    }
    #[cfg(windows)]
    if !crate::e2e::active() {
        let _ = std::thread::Builder::new()
            .name("meeting-detect-watch".into())
            .spawn(move || registry::watch(tx));
    }
    #[cfg(not(windows))]
    drop(tx);
    tracing::info!(enabled, "meeting detect: started");
}

fn detector(app: AppHandle, rx: mpsc::Receiver<()>) {
    loop {
        // Test mode reads no registry, so there's nothing to poll for.
        let polling =
            !WATCHING.load(std::sync::atomic::Ordering::Relaxed) && !crate::e2e::active();
        let timeout = {
            let s = lock();
            if s.enabled && (s.tracker.busy() || s.due.is_some() || s.prompt.is_some()) {
                ACTIVE_TICK
            } else if s.enabled && polling {
                POLL_TICK
            } else {
                IDLE_TICK
            }
        };
        match rx.recv_timeout(timeout) {
            Ok(()) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        while rx.try_recv().is_ok() {} // one scan covers a burst of changes
        if !lock().enabled {
            continue;
        }
        scan(&app);
        tick(&app);
    }
}

/// Read who's on the mic, name the calls, and feed the debounce.
fn scan(app: &AppHandle) {
    #[cfg(windows)]
    let users = if crate::e2e::active() { Vec::new() } else { registry::mic_users() };
    #[cfg(not(windows))]
    let users: Vec<Source> = Vec::new();

    let kinds: Vec<Kind> = users.iter().filter_map(classify).collect();
    let mut on_mic: Vec<&'static App> = Vec::new();
    let mut unnamed: Vec<String> = Vec::new(); // browsers on the mic, no meeting seen yet
    let browsers_on_mic: Vec<String> = kinds
        .iter()
        .filter_map(|k| match k {
            Kind::Browser(exe) => Some(exe.clone()),
            Kind::Call(_) => None,
        })
        .collect();
    {
        let mut s = lock();
        s.browser_calls.retain(|exe, _| browsers_on_mic.contains(exe));
        for kind in &kinds {
            match kind {
                Kind::Call(id) => on_mic.extend(app_by_id(id)),
                Kind::Browser(exe) => match s.browser_calls.get(exe) {
                    Some(app) => on_mic.push(app),
                    None if !unnamed.contains(exe) => unnamed.push(exe.clone()),
                    None => {}
                },
            }
        }
        on_mic.extend(s.simulated.iter().filter_map(|id| app_by_id(id)));
    }

    #[cfg(windows)]
    let named: Vec<(String, &'static App)> = if unnamed.is_empty() {
        Vec::new()
    } else {
        let mut found: Vec<(String, &'static App)> = Vec::new();
        for (exe, title) in windows_titles::of(&unnamed) {
            if found.iter().all(|(e, _)| *e != exe) {
                if let Some(app) = app_in_title(&title) {
                    found.push((exe, app));
                }
            }
        }
        found
    };
    #[cfg(not(windows))]
    let named: Vec<(String, &'static App)> = {
        let _ = &unnamed;
        Vec::new()
    };

    let edges = {
        let mut s = lock();
        for (exe, app) in named {
            tracing::info!(browser = %exe, app = app.id, "meeting detect: meeting in a browser");
            s.browser_calls.insert(exe, app);
            on_mic.push(app);
        }
        on_mic.sort_by_key(|a| a.id);
        on_mic.dedup_by_key(|a| a.id);
        s.tracker.update(now_ms(), &on_mic)
    };
    on_edges(app, edges);
}

/// The Settings toggle ("Detect calls and offer to take notes") changed.
pub fn sync(app: &AppHandle, enabled: bool) {
    let mut todo = Todo::default();
    {
        let mut s = lock();
        if s.enabled == enabled {
            return;
        }
        s.enabled = enabled;
        if !enabled {
            // A recording Yap started keeps going; it just won't be asked about.
            withdraw(&mut s, &mut todo);
            s.tracker = Tracker::default();
            s.browser_calls.clear();
            s.simulated.clear();
            s.calls.clear();
            s.due = None;
        }
        todo.emit = true;
    }
    tracing::info!(enabled, "meeting detect: setting changed");
    run(app, todo);
    wake();
}

/// The main window came up or was clicked: a pending prompt moves into it
/// (its Windows notification is withdrawn). A window event, not a page one:
/// WebView2 reports `visible` even while the window is hidden.
pub fn on_main_window_focused(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut todo = Todo::default();
        {
            let mut s = lock();
            if let Some(p) = s.prompt.as_mut() {
                if p.native || !p.in_app {
                    todo.remove_native = p.native;
                    p.native = false;
                    p.in_app = true;
                    todo.emit = true;
                }
            }
        }
        run(&app, todo);
    });
}

// ---- answers -------------------------------------------------------------------------

/// The person answered prompt `prompt_id` (the in-app toast or the Windows
/// notification): "record" | "dismiss" (start prompts), "stop" | "keep" (end
/// prompts). "record" returns the new note's id. Blocking (the recorder takes
/// up to a few seconds to start): run it off the main thread.
pub fn respond(app: &AppHandle, prompt_id: u64, action: &str) -> Result<Option<u64>, String> {
    let mut todo = Todo::default();
    let prompt = {
        let mut s = lock();
        // Answered already, on the other surface.
        if s.answered.contains(&prompt_id) {
            return Ok(None);
        }
        let current = s.prompt.as_ref().is_some_and(|p| p.id == prompt_id);
        if current {
            s.answered.push(prompt_id);
            if s.answered.len() > 16 {
                s.answered.remove(0);
            }
        }
        let prompt = if current { s.prompt.clone() } else { None };
        if current {
            withdraw(&mut s, &mut todo);
        }
        if let Some(p) = prompt.as_ref().filter(|_| action == "dismiss") {
            s.snoozed.insert(p.app.id, now_ms() + SNOOZE_MS);
        }
        prompt
    };
    run(app, todo);
    tracing::info!(prompt_id, action, current = prompt.is_some(), "meeting detect: answered");
    match (action, prompt) {
        ("record", Some(p)) if p.kind == PromptKind::Start => record(app, p.call_id).map(Some),
        ("record", _) => Err("That call has ended.".to_string()),
        ("stop", Some(p)) if p.kind == PromptKind::End => {
            stop_and_summarise(app, p.note_id);
            Ok(None)
        }
        ("dismiss" | "keep" | "stop", _) => Ok(None),
        (other, _) => Err(format!("Unknown answer: {other}")),
    }
}

/// "Record notes": a meeting note for the call, recording into it at once.
fn record(app: &AppHandle, call_id: u64) -> Result<u64, String> {
    let call_app = lock()
        .calls
        .iter()
        .find(|c| c.id == call_id)
        .map(|c| c.app)
        .ok_or("That call has ended.")?;
    if crate::meeting::is_recording() {
        return Err("Yap is already recording a meeting.".to_string());
    }
    crate::notes::folder_create("Meetings");
    let title = note_title(call_app, local_now());
    let note = crate::notes::create(&title, "", "meeting", "Meetings");
    crate::notes::mark_meeting(note.id)?;
    let started = crate::commands::meeting_start(app.clone(), app.state::<crate::AppState>(), note.id);
    if let Err(e) = started {
        crate::notes::delete(note.id);
        let _ = app.emit("yap-notes-changed", ());
        tracing::warn!("meeting detect: couldn't start recording: {e}");
        return Err(e);
    }
    {
        let mut s = lock();
        s.our_note = Some(note.id);
        if let Some(call) = s.calls.iter_mut().find(|c| c.id == call_id) {
            call.note_id = Some(note.id);
        }
    }
    let _ = app.emit("yap-notes-changed", ());
    tracing::info!(note_id = note.id, app = call_app.id, "meeting detect: recording the call");
    if window_view(app).0 {
        let _ = app.emit(EVENT_OPEN, serde_json::json!({ "noteId": note.id, "stop": false }));
    } else {
        #[cfg(windows)]
        native::recording(app, call_app, note.id);
    }
    emit(app);
    Ok(note.id)
}

/// "Stop and summarise": open the note and stop it there, so the Notes view
/// runs its Meeting Notes summary as for any recording.
fn stop_and_summarise(app: &AppHandle, note_id: Option<u64>) {
    let _ = crate::commands::show_settings(app);
    let _ = app.emit(EVENT_OPEN, serde_json::json!({ "noteId": note_id, "stop": true }));
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(STOP_FALLBACK).await;
        let st = crate::meeting::state();
        let same = note_id.is_none() || st["noteId"].as_u64() == note_id;
        if st["recording"].as_bool() == Some(true) && same {
            tracing::warn!("meeting detect: the Notes view didn't stop the recording; stopping it here");
            let _ = crate::meeting::stop();
        }
    });
}

// ---- the note title ---------------------------------------------------------------------

/// (day, month 1–12, hour, minute), local time.
fn local_now() -> (u16, u16, u16, u16) {
    #[cfg(windows)]
    {
        let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
        (t.wDay, t.wMonth, t.wHour, t.wMinute)
    }
    #[cfg(not(windows))]
    {
        // UTC — Yap ships on Windows; this only keeps other targets building.
        let secs = now_ms() / 1000;
        let (days, rem) = (secs / 86_400, secs % 86_400);
        // Howard Hinnant's civil_from_days.
        let z = days as i64 + 719_468;
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = (doy - (153 * mp + 2) / 5 + 1) as u16;
        let month = if mp < 10 { mp + 3 } else { mp - 9 } as u16;
        (day, month, (rem / 3600) as u16, (rem % 3600 / 60) as u16)
    }
}

/// "Teams call · 5 Oct, 14:30".
fn note_title(app: &App, (day, month, hour, minute): (u16, u16, u16, u16)) -> String {
    const MONTHS: [&str; 12] =
        ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let mon = MONTHS[usize::from(month.clamp(1, 12) - 1)];
    format!("{} {} \u{b7} {day} {mon}, {hour:02}:{minute:02}", app.label, app.noun)
}

// ---- commands ------------------------------------------------------------------------------

/// Calls in progress and the pending prompt.
#[tauri::command]
pub fn meeting_detect_status() -> Status {
    status()
}

/// Answer a prompt (see [`respond`]).
#[tauri::command]
pub async fn meeting_detect_respond(
    app: AppHandle,
    prompt_id: u64,
    action: String,
) -> Result<Option<u64>, String> {
    tauri::async_runtime::spawn_blocking(move || respond(&app, prompt_id, &action))
        .await
        .map_err(|e| e.to_string())?
}

/// Debug builds only: pretend call app `app_id` ("teams", "zoom", …) took
/// (`active`) or let go of the mic, skipping the debounce — the e2e suite's
/// stand-in for a real call.
#[tauri::command]
pub async fn meeting_detect_simulate(app: AppHandle, app_id: String, active: bool) -> Result<(), String> {
    if !cfg!(debug_assertions) {
        return Err("Call simulation is only in debug builds".to_string());
    }
    let call_app = app_by_id(&app_id).ok_or_else(|| format!("Unknown call app: {app_id}"))?;
    let edge = {
        let mut s = lock();
        if !s.enabled {
            return Ok(());
        }
        if active {
            s.simulated.insert(call_app.id);
        } else {
            s.simulated.remove(call_app.id);
        }
        s.tracker.force(call_app.id, active, now_ms())
    };
    tracing::info!(app = call_app.id, active, "meeting detect: simulated");
    on_edges(&app, edge.into_iter().collect());
    tick(&app);
    Ok(())
}

// ---- Windows ---------------------------------------------------------------------------------

/// The consent store: who's on the mic, and a wake-up whenever that changes.
#[cfg(windows)]
mod registry {
    use std::sync::atomic::Ordering;
    use std::sync::mpsc::Sender;

    use windows::core::{HSTRING, PWSTR};
    use windows::Win32::Foundation::{ERROR_NO_MORE_ITEMS, ERROR_SUCCESS};
    use windows::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegGetValueW, RegNotifyChangeKeyValue, RegOpenKeyExW, HKEY,
        HKEY_CURRENT_USER, KEY_NOTIFY, KEY_READ, REG_NOTIFY_CHANGE_LAST_SET,
        REG_NOTIFY_CHANGE_NAME, REG_SAM_FLAGS, RRF_RT_REG_QWORD,
    };

    use super::{classify, exe_source, on_mic, package_source, Source};

    const MIC: &str =
        r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone";

    struct Key(HKEY);

    impl Drop for Key {
        fn drop(&mut self) {
            unsafe {
                let _ = RegCloseKey(self.0);
            }
        }
    }

    fn open(parent: HKEY, path: &str, access: REG_SAM_FLAGS) -> Option<Key> {
        let mut key = HKEY::default();
        let r = unsafe { RegOpenKeyExW(parent, &HSTRING::from(path), None, access, &mut key) };
        (r == ERROR_SUCCESS).then_some(Key(key))
    }

    fn subkeys(key: &Key) -> Vec<String> {
        let mut out = Vec::new();
        for i in 0.. {
            let mut buf = [0u16; 512];
            let mut len = buf.len() as u32;
            let r = unsafe {
                RegEnumKeyExW(key.0, i, Some(PWSTR(buf.as_mut_ptr())), &mut len, None, None, None, None)
            };
            if r == ERROR_SUCCESS {
                out.push(String::from_utf16_lossy(&buf[..len as usize]));
            } else if r == ERROR_NO_MORE_ITEMS || i > 10_000 {
                break;
            } // else a name too long for any exe path: skip it
        }
        out
    }

    /// A QWORD value of subkey `sub` (0 if it's missing).
    fn qword(key: &Key, sub: &str, value: &str) -> u64 {
        let mut v = 0u64;
        let mut size = std::mem::size_of::<u64>() as u32;
        let r = unsafe {
            RegGetValueW(
                key.0,
                &HSTRING::from(sub),
                &HSTRING::from(value),
                RRF_RT_REG_QWORD,
                None,
                Some(&mut v as *mut u64 as *mut core::ffi::c_void),
                Some(&mut size),
            )
        };
        if r == ERROR_SUCCESS {
            v
        } else {
            0
        }
    }

    /// When this boot started, as a FILETIME.
    fn boot_filetime() -> u64 {
        let uptime_ms = unsafe { windows::Win32::System::SystemInformation::GetTickCount64() };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64 / 100)
            .unwrap_or(0)
            + 116_444_736_000_000_000;
        now.saturating_sub(uptime_ms.saturating_mul(10_000))
    }

    /// The known call apps and browsers on the mic right now. Only their
    /// subkeys' values are read.
    pub fn mic_users() -> Vec<Source> {
        let Some(mic) = open(HKEY_CURRENT_USER, MIC, KEY_READ) else {
            return Vec::new();
        };
        let boot = boot_filetime();
        let mut out = Vec::new();
        let mut check = |key: &Key, sub: &str, src: Source| {
            if classify(&src).is_some()
                && on_mic(qword(key, sub, "LastUsedTimeStart"), qword(key, sub, "LastUsedTimeStop"), boot)
            {
                out.push(src);
            }
        };
        for sub in subkeys(&mic) {
            if !sub.eq_ignore_ascii_case("NonPackaged") {
                check(&mic, &sub, package_source(&sub));
            }
        }
        if let Some(desktop) = open(mic.0, "NonPackaged", KEY_READ) {
            for sub in subkeys(&desktop) {
                check(&desktop, &sub, exe_source(&sub));
            }
        }
        out
    }

    /// Every in-use entry, call app or not (exe/package names only) — the
    /// read-only machine check in the tests.
    #[cfg(test)]
    pub fn all_on_mic() -> Vec<Source> {
        let Some(mic) = open(HKEY_CURRENT_USER, MIC, KEY_READ) else {
            return Vec::new();
        };
        let boot = boot_filetime();
        let mut out = Vec::new();
        for sub in subkeys(&mic) {
            if !sub.eq_ignore_ascii_case("NonPackaged")
                && on_mic(qword(&mic, &sub, "LastUsedTimeStart"), qword(&mic, &sub, "LastUsedTimeStop"), boot)
            {
                out.push(package_source(&sub));
            }
        }
        if let Some(desktop) = open(mic.0, "NonPackaged", KEY_READ) {
            for sub in subkeys(&desktop) {
                if on_mic(qword(&desktop, &sub, "LastUsedTimeStart"), qword(&desktop, &sub, "LastUsedTimeStop"), boot) {
                    out.push(exe_source(&sub));
                }
            }
        }
        out
    }

    /// Block until something under the microphone key changes (an app took
    /// or let go of the mic), wake the detector, repeat. Yap's own mic use
    /// wakes it too; the scan ignores it.
    pub fn watch(tx: Sender<()>) {
        let Some(mic) = open(HKEY_CURRENT_USER, MIC, KEY_NOTIFY) else {
            tracing::warn!("meeting detect: can't watch the microphone record; polling instead");
            return;
        };
        super::WATCHING.store(true, Ordering::Relaxed);
        loop {
            let r = unsafe {
                RegNotifyChangeKeyValue(
                    mic.0,
                    true,
                    REG_NOTIFY_CHANGE_LAST_SET | REG_NOTIFY_CHANGE_NAME,
                    None,
                    false,
                )
            };
            if r != ERROR_SUCCESS {
                tracing::warn!("meeting detect: microphone watch failed ({}); polling instead", r.0);
                break;
            }
            if tx.send(()).is_err() {
                break;
            }
        }
        super::WATCHING.store(false, Ordering::Relaxed);
    }
}

/// Visible top-level windows' titles, for the meeting a browser shows.
#[cfg(windows)]
mod windows_titles {
    use std::ffi::c_void;

    #[link(name = "user32")]
    extern "system" {
        fn EnumWindows(callback: unsafe extern "system" fn(*mut c_void, isize) -> i32, lparam: isize) -> i32;
        fn IsWindowVisible(hwnd: *mut c_void) -> i32;
        fn GetWindowTextW(hwnd: *mut c_void, text: *mut u16, max: i32) -> i32;
    }

    unsafe extern "system" fn collect(hwnd: *mut c_void, lparam: isize) -> i32 {
        let list = &mut *(lparam as *mut Vec<isize>);
        if IsWindowVisible(hwnd) != 0 {
            list.push(hwnd as isize);
        }
        1
    }

    /// (exe, title) of the visible windows belonging to `exes` (lowercase).
    pub fn of(exes: &[String]) -> Vec<(String, String)> {
        let mut hwnds: Vec<isize> = Vec::new();
        unsafe {
            EnumWindows(collect, &mut hwnds as *mut Vec<isize> as isize);
        }
        let mut out = Vec::new();
        for hwnd in hwnds {
            let mut buf = [0u16; 512];
            let len = unsafe { GetWindowTextW(hwnd as *mut c_void, buf.as_mut_ptr(), buf.len() as i32) };
            if len <= 0 {
                continue;
            }
            let Some(exe) = crate::text_injector::app_name_for(Some(hwnd)) else { continue };
            let exe = exe.to_ascii_lowercase();
            if exes.contains(&exe) {
                out.push((exe, String::from_utf16_lossy(&buf[..len as usize])));
            }
        }
        out
    }
}

/// The prompts as Windows notifications (silent, with Yap's logo; see
/// `crate::win_toast`), for when the main window isn't in front.
#[cfg(windows)]
mod native {
    use std::sync::Mutex;

    use tauri::{AppHandle, Emitter};
    use windows::UI::Notifications::ToastNotification;

    use super::{wording, App, Prompt, PromptKind, EVENT_OPEN};
    use crate::win_toast::{esc, logo_xml};

    const TAG: &str = "call";

    /// The live toast, kept so its buttons keep working from the
    /// notification center.
    static CURRENT: Mutex<Option<ToastNotification>> = Mutex::new(None);

    fn post(app: &AppHandle, xml: &str) -> Result<(), String> {
        if !crate::win_toast::allowed() {
            return Err("off in portable mode and test runs".to_string());
        }
        let toast = crate::win_toast::post(app, TAG, xml, None, activated)?;
        *CURRENT.lock().unwrap_or_else(|p| p.into_inner()) = Some(toast);
        Ok(())
    }

    pub fn prompt_xml(p: &Prompt, logo: &str) -> String {
        let w = wording(p.kind, p.app);
        let (accept, decline) = match p.kind {
            PromptKind::Start => ("record", "dismiss"),
            PromptKind::End => ("stop", "keep"),
        };
        format!(
            "<toast launch=\"meeting:show\"><visual><binding template=\"ToastGeneric\">\
             <text>{}</text><text>{}</text>{logo}</binding></visual><actions>\
             <action content=\"{}\" arguments=\"meeting:{accept}:{id}\" activationType=\"foreground\"/>\
             <action content=\"{}\" arguments=\"meeting:{decline}:{id}\" activationType=\"foreground\"/>\
             </actions><audio silent=\"true\"/></toast>",
            esc(&w.title),
            esc(w.body),
            esc(w.accept),
            esc(w.decline),
            id = p.id,
        )
    }

    pub fn post_prompt(app: &AppHandle, p: &Prompt) -> Result<(), String> {
        post(app, &prompt_xml(p, &logo_xml()))
    }

    /// "Record notes" from the notification center: say it's recording.
    pub fn recording(app: &AppHandle, call: &App, note_id: u64) {
        let xml = format!(
            "<toast launch=\"meeting:open:{note_id}\"><visual><binding template=\"ToastGeneric\">\
             <text>{}</text><text>Yap offers to stop and summarise when it ends.</text>{}\
             </binding></visual><actions>\
             <action content=\"Open note\" arguments=\"meeting:open:{note_id}\" activationType=\"foreground\"/>\
             </actions><audio silent=\"true\"/></toast>",
            esc(&format!("Taking notes on your {} {}", call.label, call.noun)),
            logo_xml(),
        );
        if let Err(e) = post(app, &xml) {
            tracing::info!("meeting detect: no Windows notification ({e})");
        }
    }

    fn failed(app: &AppHandle, message: &str) {
        let xml = format!(
            "<toast launch=\"meeting:show\"><visual><binding template=\"ToastGeneric\">\
             <text>Couldn't record the call</text><text>{}</text>{}</binding></visual>\
             <audio silent=\"true\"/></toast>",
            esc(message),
            logo_xml(),
        );
        let _ = post(app, &xml);
    }

    /// Take the call notification out of the notification center.
    pub fn remove(app: &AppHandle) {
        if CURRENT.lock().unwrap_or_else(|p| p.into_inner()).take().is_some() {
            crate::win_toast::remove(app, TAG);
        }
    }

    /// A click: `meeting:<answer>:<prompt id>`, `meeting:open:<note id>`, or
    /// the body (`meeting:show`).
    fn activated(app: &AppHandle, arg: &str) {
        tracing::info!(arg, "meeting detect: Windows notification clicked");
        let app = app.clone();
        let arg = arg.to_string();
        tauri::async_runtime::spawn_blocking(move || {
            let mut parts = arg.splitn(3, ':');
            let (scheme, verb, id) = (parts.next(), parts.next(), parts.next().and_then(|n| n.parse::<u64>().ok()));
            match (scheme, verb, id) {
                (Some("meeting"), Some("open"), Some(note_id)) => {
                    let _ = crate::commands::show_settings(&app);
                    let _ = app.emit(EVENT_OPEN, serde_json::json!({ "noteId": note_id, "stop": false }));
                }
                (Some("meeting"), Some(answer), Some(prompt_id)) => {
                    if let Err(e) = super::respond(&app, prompt_id, answer) {
                        failed(&app, &e);
                    }
                }
                // The body: open Yap; a pending prompt moves into the window.
                _ => {
                    let _ = crate::commands::show_settings(&app);
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(id: &str) -> &'static App {
        app_by_id(id).unwrap()
    }

    #[test]
    fn names_call_apps_from_consent_store_keys() {
        let teams = package_source("MSTeams_8wekyb3d8bbwe");
        assert_eq!(classify(&teams), Some(Kind::Call("teams")));
        let discord = exe_source("C:#Users#me#AppData#Local#Discord#app-1.0.9260#Discord.exe");
        assert_eq!(discord, Source::Exe("discord.exe".to_string()));
        assert_eq!(classify(&discord), Some(Kind::Call("discord")));
        assert_eq!(classify(&exe_source("C:#Users#me#AppData#Roaming#Zoom#bin#Zoom.exe")), Some(Kind::Call("zoom")));
        assert_eq!(
            classify(&package_source("5319275A.WhatsAppDesktop_cv1g1gvanyjgm")),
            Some(Kind::Call("whatsapp"))
        );
        let brave = exe_source("C:#Users#me#AppData#Local#BraveSoftware#Brave-Browser#Application#brave.exe");
        assert_eq!(classify(&brave), Some(Kind::Browser("brave.exe".to_string())));
        assert_eq!(
            classify(&package_source("TheBrowserCompany.Arc_ttt1ap7aakyb4")),
            Some(Kind::Browser("arc.exe".to_string()))
        );
    }

    #[test]
    fn yap_and_other_mic_users_are_never_calls() {
        for key in [
            "D:#Hobby Project#Yap#yap.exe",
            "E:#Projects#Yap#src-tauri#target#debug#yap.exe",
            "C:#Users#me#AppData#Local#WisprFlow#app-1.6.447#Wispr Flow.exe",
            "E:#Users#me#AppData#Local#Handy#handy.exe",
            "D:#Program Files#obs-studio#bin#64bit#obs64.exe",
            "D:#Program Files (x86)#Overwatch#_retail_#Overwatch.exe",
            "C:#Program Files (x86)#Microsoft#EdgeWebView#Application#msedgewebview2.exe",
        ] {
            assert_eq!(classify(&exe_source(key)), None, "{key}");
        }
        for family in ["Claude_pzs8sxrjxfjjc", "windows.immersivecontrolpanel_cw5n1h2txyewy"] {
            assert_eq!(classify(&package_source(family)), None, "{family}");
        }
    }

    #[test]
    fn names_the_meeting_in_a_browser_title() {
        let named = |t: &str| app_in_title(t).map(|a| a.id);
        assert_eq!(named("Meet - abc-defg-hij - Google Chrome"), Some("meet"));
        assert_eq!(named("Meet \u{2013} Weekly sync — Mozilla Firefox"), Some("meet"));
        assert_eq!(named("Weekly sync | Microsoft Teams - Work - Microsoft\u{200b} Edge"), Some("teams"));
        assert_eq!(named("Zoom Meeting - Google Chrome"), Some("zoom"));
        assert_eq!(named("general (Channel) - Acme - Slack - Brave"), Some("slack"));
        assert_eq!(named("Cisco Webex Meetings - Google Chrome"), Some("webex"));
        assert_eq!(named("YouTube - Google Chrome"), None);
        assert_eq!(named("Inbox (3) - me@example.com - Gmail - Google Chrome"), None);
    }

    #[test]
    fn on_the_mic_means_stopped_never_since_this_boot() {
        let boot = 1_000;
        assert!(on_mic(1_500, 0, boot));
        assert!(!on_mic(1_500, 1_600, boot)); // let go
        assert!(!on_mic(900, 0, boot)); // left over from before a restart
        assert!(!on_mic(0, 0, boot)); // never used
    }

    #[test]
    fn a_call_starts_after_five_seconds_on_the_mic() {
        let mut t = Tracker::default();
        let teams = app("teams");
        assert!(t.update(0, &[teams]).is_empty());
        assert!(t.update(4_000, &[teams]).is_empty());
        assert_eq!(t.update(5_000, &[teams]), vec![Edge::Started { app: "teams", since: 0 }]);
        assert!(t.update(7_000, &[teams]).is_empty()); // once
    }

    #[test]
    fn a_short_mic_use_is_no_call() {
        let mut t = Tracker::default();
        let zoom = app("zoom");
        t.update(0, &[zoom]);
        assert!(t.update(3_000, &[]).is_empty());
        assert!(!t.busy());
        // Back on the mic: the clock starts over.
        assert!(t.update(4_000, &[zoom]).is_empty());
        assert!(t.update(8_000, &[zoom]).is_empty());
        assert_eq!(t.update(9_000, &[zoom]).len(), 1);
    }

    #[test]
    fn chat_apps_wait_longer() {
        let mut t = Tracker::default();
        let slack = app("slack");
        t.update(0, &[slack]);
        assert!(t.update(10_000, &[slack]).is_empty()); // a voice clip
        assert_eq!(t.update(20_000, &[slack]), vec![Edge::Started { app: "slack", since: 0 }]);
    }

    #[test]
    fn a_gap_is_not_an_ending() {
        let mut t = Tracker::default();
        let meet = app("meet");
        t.update(0, &[meet]);
        t.update(5_000, &[meet]);
        // A headset switch: off the mic for 10 s, then back.
        assert!(t.update(60_000, &[]).is_empty());
        assert!(t.update(70_000, &[]).is_empty());
        assert!(t.update(71_000, &[meet]).is_empty());
        // Gone for good: ends 15 s after letting go.
        assert!(t.update(100_000, &[]).is_empty());
        assert!(t.update(114_000, &[]).is_empty());
        assert_eq!(t.update(115_000, &[]), vec![Edge::Ended { app: "meet" }]);
        assert!(!t.busy());
    }

    #[test]
    fn simulated_calls_skip_the_debounce() {
        let mut t = Tracker::default();
        assert_eq!(t.force("teams", true, 10), Some(Edge::Started { app: "teams", since: 10 }));
        assert_eq!(t.force("teams", true, 20), None);
        assert_eq!(t.force("teams", false, 30), Some(Edge::Ended { app: "teams" }));
        assert_eq!(t.force("teams", false, 40), None);
    }

    #[test]
    fn note_titles() {
        assert_eq!(note_title(app("teams"), (5, 10, 14, 30)), "Teams call \u{b7} 5 Oct, 14:30");
        assert_eq!(note_title(app("slack"), (31, 12, 9, 5)), "Slack huddle \u{b7} 31 Dec, 09:05");
        assert_eq!(note_title(app("meet"), (1, 1, 0, 0)), "Google Meet call \u{b7} 1 Jan, 00:00");
    }

    #[test]
    fn prompt_wording() {
        let start = wording(PromptKind::Start, app("teams"));
        assert_eq!(start.title, "Teams call detected");
        assert_eq!((start.accept, start.decline), ("Record notes", "Not now"));
        assert!(start.body.contains("Let people know you're taking notes"));
        let end = wording(PromptKind::End, app("slack"));
        assert_eq!(end.title, "Slack huddle ended");
        assert_eq!((end.accept, end.decline), ("Stop and summarise", "Keep recording"));
    }

    #[cfg(windows)]
    #[test]
    fn notification_xml_names_the_answers() {
        let p = Prompt {
            id: 7,
            kind: PromptKind::Start,
            call_id: 3,
            app: app("teams"),
            note_id: None,
            in_app: false,
            native: false,
        };
        let xml = native::prompt_xml(&p, "");
        assert!(xml.contains("<text>Teams call detected</text>"));
        assert!(xml.contains("arguments=\"meeting:record:7\""));
        assert!(xml.contains("arguments=\"meeting:dismiss:7\""));
        assert!(xml.contains("<audio silent=\"true\"/>"));
        // WinRT takes it as a toast (built, never shown).
        let doc = windows::Data::Xml::Dom::XmlDocument::new().unwrap();
        doc.LoadXml(&windows::core::HSTRING::from(xml)).unwrap();
        windows::UI::Notifications::ToastNotification::CreateToastNotification(&doc).unwrap();
        let end = Prompt { kind: PromptKind::End, note_id: Some(1), ..p };
        let xml = native::prompt_xml(&end, "");
        assert!(xml.contains("arguments=\"meeting:stop:7\"") && xml.contains("arguments=\"meeting:keep:7\""));
    }

    /// Read-only look at this machine's microphone record:
    /// `cargo test meeting_detect -- --ignored --nocapture`.
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn this_machine() {
        let everyone = registry::all_on_mic();
        println!("on the mic now (any app): {everyone:?}");
        let calls = registry::mic_users();
        println!("call apps / browsers on the mic: {calls:?}");
        for src in &calls {
            println!("  {src:?} -> {:?}", classify(src));
        }
        let browsers: Vec<String> = BROWSERS.iter().map(|b| b.to_string()).collect();
        let named: Vec<&str> = windows_titles::of(&browsers)
            .iter()
            .filter_map(|(_, t)| app_in_title(t))
            .map(|a| a.id)
            .collect();
        println!("meetings showing in browser windows: {named:?}");
        assert!(everyone.iter().all(|s| !matches!(classify(s), Some(Kind::Call(_))) || calls.contains(s)));
    }
}
