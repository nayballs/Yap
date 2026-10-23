//! **Yap's notice policy**: how loud each kind of notice may be, decided in
//! one place (after Wispr Flow's notification rules, by behaviour). Every
//! card on the Yap bar (`bar.rs`) and every Windows notification
//! (`win_toast.rs`) goes through it, and the modules that pick a surface ask
//! [`place`]; the main window's and the notepad's toasts follow the same
//! rules on the page (`ui/noticePolicy.js` + `ui/toast.svelte.js`).
//!
//! - **One place.** A notice shows in one place: the main window's toast
//!   while the person is looking at it (focused), else a card on the Yap
//!   bar, else a Windows notification (the bar off or hidden), else — the
//!   window on screen, nothing else possible — the toast after all.
//! - **One at a time.** The bar shows one card; the others wait, the most
//!   important first (a call before a tip), then the oldest. A card the
//!   pointer is on stays put until the pointer leaves. An offer that stays
//!   until answered ("Started by mistake?", an update) waits its turn,
//!   gives way to its equals and comes back.
//! - **Held while you dictate.** Tips and notices wait while a dictation
//!   records or transcribes and for [`SETTLE_MS`] after, and while the
//!   first-run setup is on screen; what needs an answer now (a call, a
//!   meeting's limit or check-in, an alert about the dictation itself)
//!   doesn't. An update and the coaching tips also wait out a meeting
//!   recording.
//! - **Stale waits are dropped.** A tip or notice that waited longer than
//!   its `max_wait_ms` without being seen is dropped (reported as its card's
//!   expiry), so nothing pops up a minute after the fact.
//! - **No repeats.** The same notice (kind + text) again within its
//!   cooldown isn't shown: the microphone privacy alert, a meeting notice.
//! - **Tips can be switched off** (Settings → General → Notifications,
//!   `show_tips`); alerts, questions and notices can't.
//! - **Windows notifications** get the same cooldown, and skip the banner
//!   (straight into the notification center) while a dictation records, a
//!   fullscreen app or a presentation runs, or Windows says it's quiet
//!   time — unless they need acting on now (an alert, a question). Windows
//!   applies Focus Assist / Do Not Disturb itself.
//! - **Call-prompt fatigue** ([`Fatigue`]): three "Not now"s in a row, or
//!   five prompts left to fade, and Yap offers once to stop asking; 35
//!   prompts without a meeting ever recorded and it stops by itself, saying
//!   so (with Turn back on). Kept in `<data>/notices.json`.
//!
//! The rules ([`rule`], [`pick`], [`Ledger`], [`place_for`], [`Fatigue`])
//! are pure and unit-tested.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

/// After a dictation ends, held notices wait this much longer (the paste,
/// a glance at what landed).
pub const SETTLE_MS: u64 = 1_500;

/// How much a notice matters: a higher one shows first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    /// Coaching hints, word suggestions: nice to know, fine to miss.
    Tip,
    /// A notice: something happened (an update is ready, notes stopped).
    Info,
    /// Something went wrong and the person should act (the text is on the
    /// clipboard; Windows blocks the mic).
    Alert,
    /// A question waiting for an answer, often against the clock: a call,
    /// a calendar meeting, the meeting's length limit, a check-in.
    Prompt,
}

/// How a kind of notice behaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rule {
    pub priority: Priority,
    /// Waits while a dictation records or transcribes (and [`SETTLE_MS`]),
    /// and while the first-run setup is on screen.
    pub hold_dictation: bool,
    /// Waits while a meeting records.
    pub hold_meeting: bool,
    /// The same notice (kind + text) again within this long isn't shown.
    pub cooldown_ms: u64,
    /// Never seen after waiting this long: dropped. `None`: it waits.
    pub max_wait_ms: Option<u64>,
    /// A tip "Show tips" switches off (Settings → General → Notifications).
    pub tip: bool,
    /// An offer that stays until answered ("Started by mistake?", an
    /// update): it waits its turn (never pushes the card up aside), gives
    /// way to anything as important that comes in after it, and comes back
    /// once that's answered.
    pub yields: bool,
}

impl Rule {
    /// How it ranks for the one place on screen: priority, then whether it
    /// holds its place (an offer that yields ranks under its equals).
    fn rank(&self) -> (Priority, bool) {
        (self.priority, !self.yields)
    }
}

const MIN: u64 = 60_000;

/// The rule for a kind of notice: a bar card's id, a Windows notification's
/// tag, an alert's kind.
pub fn rule(kind: &str) -> Rule {
    let prompt = Rule {
        priority: Priority::Prompt,
        hold_dictation: false,
        hold_meeting: false,
        cooldown_ms: 0,
        max_wait_ms: None,
        tip: false,
        yields: false,
    };
    let info = Rule {
        priority: Priority::Info,
        hold_dictation: true,
        hold_meeting: false,
        cooldown_ms: 0,
        max_wait_ms: Some(MIN),
        tip: false,
        yields: false,
    };
    let tip = Rule { priority: Priority::Tip, max_wait_ms: Some(10_000), ..info };
    let alert = Rule { priority: Priority::Alert, hold_dictation: false, max_wait_ms: None, ..info };
    match kind {
        // Questions against the clock. (Call prompts wait out a dictation in
        // meeting_detect.rs themselves; the rest can't wait.)
        "call" | "calendar" | "meeting-limit" | "meeting-checkin" => prompt,
        // After a meeting: it can wait for a dictation to finish, and gives
        // way to a call or a meeting card.
        "meeting-mistake" => Rule { hold_dictation: true, yields: true, ..prompt },
        // About the dictation that's running: said now, and once in ten
        // minutes (the overlay's line says it on every press).
        "micBlocked" => Rule { cooldown_ms: 10 * MIN, ..alert },
        // The dictation's text is on the clipboard: every time (it's the
        // only way to know it didn't land), never held back.
        "pasteBlocked" => alert,
        // "Stop asking about calls?": after a prompt, never during a
        // dictation.
        "calls-fatigue" => Rule { hold_dictation: true, max_wait_ms: None, ..alert },
        // "Yap closed during a meeting" (meeting_audio.rs), at the next
        // start: not over a dictation or a meeting going on, and as long as
        // it takes (the note says it too).
        "meetingRecovery" => Rule { hold_dictation: true, hold_meeting: true, max_wait_ms: None, ..alert },
        // The Voice Assistant's answer (assistant.rs): the person just asked
        // for it, so it shows at once — over a meeting too (that's when it
        // isn't read aloud) — and steps aside only for a question against
        // the clock (a call), coming back after. It goes with its voice.
        "assistant" => Rule { hold_meeting: false, max_wait_ms: None, ..alert },
        // The Voice Assistant has no AI model, or its model failed: right
        // after its own question.
        "assistantNoModel" | "assistantFailed" => alert,
        // "The Voice Assistant is off during meetings": the answer to a press
        // made during one, so it shows over the meeting, every press.
        "assistantMeetingOff" => Rule { hold_meeting: false, ..alert },
        // Ready to install: waits for a dictation or a meeting to end, as
        // long as it takes.
        "update" => Rule { hold_meeting: true, max_wait_ms: None, yields: true, ..info },
        // "Stopped at 2 hours", "Yap can't hear the call"…: not twice in ten
        // minutes.
        "meeting-notice" => Rule { cooldown_ms: 10 * MIN, ..info },
        // Coaching: about dictating, so never during a meeting either; one
        // of the tips "Show tips" switches off.
        "hint" => Rule { hold_meeting: true, tip: true, ..tip },
        // A word to add: may wait out one dictation. ("Suggest words from
        // your corrections" is its own switch.)
        "word-suggestion" => Rule { max_wait_ms: Some(MIN), ..tip },
        "word-added" => Rule { max_wait_ms: Some(10_000), ..info },
        _ => info,
    }
}

/// What's going on now, as far as notices care.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Moment {
    pub now: u64,
    /// A dictation records or transcribes, or ended under [`SETTLE_MS`] ago
    /// — or the first-run setup is on screen.
    pub dictating: bool,
    /// A meeting records.
    pub meeting: bool,
    /// The person switched tips off.
    pub no_tips: bool,
}

/// Whether a notice under `r` waits right now.
pub fn held(r: &Rule, m: &Moment) -> bool {
    (r.hold_dictation && m.dictating) || (r.hold_meeting && m.meeting)
}

/// Whether a notice under `r` isn't wanted at all: a tip, with tips off.
pub fn muted(r: &Rule, m: &Moment) -> bool {
    r.tip && m.no_tips
}

/// A notice waiting for (or on) the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Waiting<'a> {
    pub kind: &'a str,
    /// When it was asked for (unix ms).
    pub since: u64,
    /// It has been on screen (a notice someone saw is never stale).
    pub seen: bool,
}

/// [`pick`]'s answer: indexes into its list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Pick {
    /// The one to show now.
    pub front: Option<usize>,
    /// Waited too long unseen (or muted): drop these.
    pub stale: Vec<usize>,
}

/// Which notice shows now. `current` is the one on screen (it keeps its
/// place against equals, so nothing flickers), `pinned` says the pointer is
/// on it (it stays, whatever came in since).
pub fn pick(list: &[Waiting], current: Option<usize>, pinned: bool, m: &Moment) -> Pick {
    let mut out = Pick::default();
    for (i, w) in list.iter().enumerate() {
        let r = rule(w.kind);
        let waited_out = !w.seen && r.max_wait_ms.is_some_and(|max| m.now.saturating_sub(w.since) > max);
        if waited_out || (!w.seen && muted(&r, m)) {
            out.stale.push(i);
        }
    }
    if let Some(c) = current.filter(|c| pinned && *c < list.len() && !out.stale.contains(c)) {
        out.front = Some(c);
        return out;
    }
    let eligible = |i: usize| !out.stale.contains(&i) && !held(&rule(list[i].kind), m);
    // Higher rank (priority; an offer that yields under its equals), then
    // the oldest.
    let best = |patient_too: bool, skip: Option<usize>| {
        (0..list.len())
            .filter(|&i| eligible(i) && Some(i) != skip && (patient_too || !rule(list[i].kind).yields))
            .max_by_key(|&i| (rule(list[i].kind).rank(), std::cmp::Reverse(list[i].since)))
    };
    out.front = match current.filter(|&c| c < list.len() && eligible(c)) {
        // The one up keeps its place unless something that matters more
        // comes in (an offer waits its turn, and gives way to its equals).
        Some(c) => {
            let up = rule(list[c].kind);
            match best(false, Some(c)) {
                Some(i) if rule(list[i].kind).rank() > up.rank() => Some(i),
                Some(i) if up.yields && rule(list[i].kind).priority >= up.priority => Some(i),
                _ => Some(c),
            }
        }
        None => best(true, None),
    };
    out
}

/// A notice's text, for "the same notice again".
pub fn fingerprint(title: &str, body: &str) -> u64 {
    // FNV-1a: stable across runs, no dependencies.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in title.bytes().chain([0u8]).chain(body.bytes()) {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// When each kind last went out, and with what text: the cooldowns.
#[derive(Debug, Default)]
pub struct Ledger {
    last: HashMap<String, (u64, u64)>,
}

impl Ledger {
    /// May `kind` with text `fp` go out at `now`? Records it if so.
    pub fn admit(&mut self, kind: &str, fp: u64, now: u64) -> bool {
        let cooldown = rule(kind.trim_start_matches("native:")).cooldown_ms;
        if cooldown > 0 {
            if let Some(&(at, last_fp)) = self.last.get(kind) {
                if last_fp == fp && now.saturating_sub(at) < cooldown {
                    return false;
                }
            }
        }
        self.last.insert(kind.to_string(), (now, fp));
        true
    }
}

// ---- where a notice goes ----------------------------------------------------------------------

/// The one place a notice shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// A toast in the main window.
    Window,
    /// A card on the Yap bar.
    Bar,
    /// A Windows notification.
    Notification,
    /// Nowhere (the log has it).
    Nowhere,
}

/// Where a notice goes: the main window while the person is looking at it
/// (`focused`), else the Yap bar if it takes cards, else a Windows
/// notification if this notice may be one, else the main window if it's on
/// screen (`visible`) after all.
pub fn place_for(focused: bool, visible: bool, bar: bool, notification: bool) -> Place {
    if focused {
        Place::Window
    } else if bar {
        Place::Bar
    } else if notification {
        Place::Notification
    } else if visible {
        Place::Window
    } else {
        Place::Nowhere
    }
}

/// A test run's stand-in for "the main window has the focus" (a test run
/// never focuses windows): 0 = while it's on screen (the default: the person
/// is looking at Yap), 1 = never (they're in another app).
static TEST_FOCUS: AtomicU8 = AtomicU8::new(0);

/// The main window: (on screen, focused). Window getters wait on the main
/// thread: never call this with a lock held.
pub fn window_view(app: &AppHandle) -> (bool, bool) {
    app.get_webview_window("settings")
        .map(|w| {
            let visible = w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false);
            let focused = if crate::e2e::active() {
                TEST_FOCUS.load(Ordering::SeqCst) == 0
            } else {
                w.is_focused().unwrap_or(false)
            };
            (visible, visible && focused)
        })
        .unwrap_or((false, false))
}

/// Whether Yap may post Windows notifications at all (not portable, not a
/// test run, Windows only).
pub fn notifications_allowed() -> bool {
    #[cfg(windows)]
    {
        crate::win_toast::allowed()
    }
    #[cfg(not(windows))]
    false
}

/// Where a notice goes now ([`place_for`]); `notification`: this notice may
/// be a Windows notification (a tip never is). Window getters: never with a
/// lock held.
pub fn place(app: &AppHandle, notification: bool) -> Place {
    let (visible, focused) = window_view(app);
    place_for(focused, visible, crate::bar::cards_available(), notification && notifications_allowed())
}

// ---- the moment, kept current from the app's events -------------------------------------------

static DICTATING: AtomicBool = AtomicBool::new(false);
/// When the last dictation ended (unix ms): held notices wait [`SETTLE_MS`].
static ENDED_AT: AtomicU64 = AtomicU64::new(0);
static MEETING: AtomicBool = AtomicBool::new(false);
static ONBOARDING: AtomicBool = AtomicBool::new(false);
static NO_TIPS: AtomicBool = AtomicBool::new(false);
static LEDGER: Mutex<Option<Ledger>> = Mutex::new(None);

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// The pipeline's `yap-state` (lib.rs).
pub fn on_pipeline_state(state: &str) {
    let busy = matches!(state, "recording" | "processing" | "processing-slow");
    if DICTATING.swap(busy, Ordering::SeqCst) && !busy {
        ENDED_AT.store(now_ms(), Ordering::SeqCst);
    }
}

/// A meeting recording started or stopped.
pub fn on_meeting(recording: bool) {
    MEETING.store(recording, Ordering::SeqCst);
}

/// The first-run setup came on screen or went: tips and notices wait it out.
pub fn on_onboarding(shown: bool) {
    ONBOARDING.store(shown, Ordering::SeqCst);
}

/// Settings were loaded or saved: `show_tips`.
pub fn sync(cfg: &crate::config::YapConfig) {
    NO_TIPS.store(!cfg.show_tips, Ordering::SeqCst);
}

/// Tips are switched off (Settings → General → Notifications).
pub fn tips_off() -> bool {
    NO_TIPS.load(Ordering::SeqCst)
}

/// Now. Lock-free.
pub fn moment() -> Moment {
    let now = now_ms();
    let settling = now.saturating_sub(ENDED_AT.load(Ordering::SeqCst)) < SETTLE_MS;
    Moment {
        now,
        dictating: DICTATING.load(Ordering::SeqCst) || settling || ONBOARDING.load(Ordering::SeqCst),
        meeting: MEETING.load(Ordering::SeqCst),
        no_tips: NO_TIPS.load(Ordering::SeqCst),
    }
}

/// The cooldown check for `kind` with this text, recorded if it passes.
pub fn admit(kind: &str, title: &str, body: &str) -> bool {
    let mut guard = LEDGER.lock().unwrap_or_else(|p| p.into_inner());
    let ok = guard.get_or_insert_with(Ledger::default).admit(kind, fingerprint(title, body), now_ms());
    if !ok {
        tracing::info!(kind, "notice: the same notice again within its cooldown; not shown");
    }
    ok
}

/// `SHQueryUserNotificationState` says the person shouldn't be interrupted
/// now: a fullscreen app or game, a presentation, the quiet first hour after
/// setup, the screen locked.
fn windows_busy() -> bool {
    #[cfg(windows)]
    {
        if crate::e2e::active() {
            return false;
        }
        // 5 = QUNS_ACCEPTS_NOTIFICATIONS; 0 = the query failed.
        !matches!(crate::overlay::win::notification_state(), 0 | 5)
    }
    #[cfg(not(windows))]
    false
}

/// A Windows notification for `kind` goes out without a banner (straight
/// into the notification center): while a dictation records or Windows
/// says the person is busy, unless it needs acting on now (an alert, a
/// question).
pub fn native_quietly(kind: &str) -> bool {
    quietly(&rule(kind), &moment(), windows_busy())
}

fn quietly(r: &Rule, m: &Moment, busy: bool) -> bool {
    r.priority < Priority::Alert && (m.dictating || busy)
}

// ---- call-prompt fatigue ----------------------------------------------------------------------

/// "Not now"s in a row before Yap offers to stop asking about calls.
pub const CLOSED_IN_A_ROW: u32 = 3;
/// Prompts left to fade in a row (none closed in between) before it offers.
pub const IGNORED_IN_A_ROW: u32 = 5;
/// Prompts in all, with no meeting ever recorded, before it stops by itself.
pub const PROMPTS_WITHOUT_A_MEETING: u32 = 35;

/// How the person answered a call prompt, as fatigue counts it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// Record notes, Don't ask for this app, the auto-start ran: anything
    /// but "not now".
    Engaged,
    /// Not now, or ✕.
    Closed,
    /// Left to fade.
    Ignored,
}

/// What fatigue says to do after an answer or a prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Nothing,
    /// Offer once to stop asking (`ignored`: they've been letting prompts
    /// fade, rather than saying Not now).
    OfferToStop { ignored: bool },
    /// Stop asking (call detection off), and say so with Turn back on.
    SwitchOff,
}

/// Call prompts' fatigue counters (Wispr Flow's rules, by behaviour).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Fatigue {
    /// "Not now" / ✕ in a row (a prompt left to fade doesn't break it).
    pub closed: u32,
    /// Prompts left to fade in a row, none closed in between.
    pub ignored: u32,
    /// Prompts shown while no meeting was ever recorded.
    pub shown: u32,
    /// "Stop asking about calls?" was offered (once ever).
    pub offered: bool,
    /// Yap switched call detection off by itself (once ever).
    pub switched_off: bool,
}

impl Fatigue {
    /// After an answer. Someone who has recorded a meeting with Yap is
    /// never offered anything (they know the feature; "Don't ask for X" is
    /// on every prompt).
    pub fn answered(mut self, a: Answer, ever_recorded: bool) -> (Self, Verdict) {
        match a {
            Answer::Engaged => {
                self.closed = 0;
                self.ignored = 0;
                self.shown = 0;
            }
            Answer::Closed => {
                self.closed += 1;
                self.ignored = 0;
            }
            Answer::Ignored => self.ignored += 1,
        }
        if ever_recorded || self.offered || a == Answer::Engaged {
            return (self, Verdict::Nothing);
        }
        if self.closed >= CLOSED_IN_A_ROW || self.ignored >= IGNORED_IN_A_ROW {
            self.offered = true;
            let ignored = self.closed < CLOSED_IN_A_ROW;
            return (self, Verdict::OfferToStop { ignored });
        }
        (self, Verdict::Nothing)
    }

    /// After a prompt was shown.
    pub fn prompted(mut self, ever_recorded: bool) -> (Self, Verdict) {
        if ever_recorded || self.switched_off {
            return (self, Verdict::Nothing);
        }
        self.shown += 1;
        if self.shown >= PROMPTS_WITHOUT_A_MEETING {
            self.switched_off = true;
            return (self, Verdict::SwitchOff);
        }
        (self, Verdict::Nothing)
    }
}

/// What `<data>/notices.json` keeps.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Store {
    calls: Fatigue,
}

static STORE: Mutex<Option<Store>> = Mutex::new(None);

fn store_path() -> std::path::PathBuf {
    crate::config::data_dir().join("notices.json")
}

/// Change the call-prompt counters under the store's lock (read, change,
/// write in one step) and return what `f` returns.
pub fn with_call_fatigue<T>(f: impl FnOnce(Fatigue) -> (Fatigue, T)) -> T {
    let mut guard = STORE.lock().unwrap_or_else(|p| p.into_inner());
    let store = guard.get_or_insert_with(|| {
        if cfg!(test) {
            return Store::default();
        }
        let path = store_path();
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                tracing::warn!("notices.json unreadable ({e}); starting afresh");
                crate::config::quarantine_corrupt(&path);
                Store::default()
            }),
            Err(_) => Store::default(),
        }
    });
    let (next, out) = f(store.calls.clone());
    if next != store.calls {
        store.calls = next;
        if !cfg!(test) {
            match serde_json::to_string_pretty(&*store) {
                Ok(json) => {
                    if let Err(e) = crate::config::atomic_write(&store_path(), &json) {
                        tracing::warn!("notices: couldn't save notices.json: {e}");
                    }
                }
                Err(e) => tracing::warn!("notices: couldn't serialize notices.json: {e}"),
            }
        }
    }
    out
}

// ---- test mode --------------------------------------------------------------------------------

/// Test mode only: forget every cooldown and fatigue count, and look at the
/// main window again, so each e2e test starts fresh (the suite's `main`
/// fixture calls it before every test).
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_notice_reset() -> Result<(), String> {
    if !crate::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    *LEDGER.lock().unwrap_or_else(|p| p.into_inner()) = None;
    with_call_fatigue(|_| (Fatigue::default(), ()));
    TEST_FOCUS.store(0, Ordering::SeqCst);
    Ok(())
}

/// Test mode only: whether the person counts as looking at the main window
/// while it's on screen (`focused`, the default) or as being in another app
/// (`false`: notices go to the Yap bar).
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_focus(app: AppHandle, focused: bool) -> Result<(), String> {
    if !crate::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    TEST_FOCUS.store(if focused { 0 } else { 1 }, Ordering::SeqCst);
    // As the real focus change does: a call prompt moves into the window,
    // and the calendar looks again at once (its card follows you either
    // way).
    if focused {
        crate::meeting_detect::on_main_window_focused(&app);
    }
    crate::calendar::on_main_window_focused();
    Ok(())
}

/// Test mode only: put a notice of `kind` up through the real policy — a
/// card on the Yap bar (`surface` "bar"), or the main window's toast
/// ("window", `level` = the page's kind: "tip" | "info" | "alert") — or
/// pretend a dictation is running (`dictating`, as the pipeline's
/// `yap-state` says it) for the hold-backs. Returns whether it was taken.
#[cfg(debug_assertions)]
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn e2e_notice(
    app: AppHandle,
    surface: Option<String>,
    kind: Option<String>,
    title: Option<String>,
    body: Option<String>,
    level: Option<String>,
    timeout_ms: Option<u64>,
    dictating: Option<bool>,
) -> Result<bool, String> {
    use tauri::Emitter;
    if !crate::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    if let Some(on) = dictating {
        let state = if on { "recording" } else { "idle" };
        app.emit("yap-state", state).map_err(|e| e.to_string())?;
        return Ok(true);
    }
    let kind: &'static str = Box::leak(kind.unwrap_or_else(|| "e2e".into()).into_boxed_str());
    let title = title.unwrap_or_default();
    let body = body.unwrap_or_default();
    match surface.as_deref() {
        Some("window") => {
            if !admit(kind, &title, &body) {
                return Ok(false);
            }
            app.emit(
                "yap-alert",
                serde_json::json!({
                    "kind": kind, "icon": "", "title": title, "body": body, "action": null,
                    "timeoutMs": timeout_ms.unwrap_or(10_000), "level": level.unwrap_or_else(|| "alert".into()),
                }),
            )
            .map_err(|e| e.to_string())?;
            Ok(true)
        }
        _ => {
            let card = crate::bar::Card {
                id: kind.into(),
                icon: "tip",
                title,
                body,
                timeout_ms: timeout_ms.or(Some(60_000)),
                ..Default::default()
            };
            Ok(crate::bar::show_card(&app, card, Box::new(|_, _| {})))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: u64 = 1_700_000_000_000;

    fn m(dictating: bool, meeting: bool) -> Moment {
        Moment { now: T, dictating, meeting, no_tips: false }
    }

    fn w(kind: &str, ago: u64) -> Waiting<'_> {
        Waiting { kind, since: T - ago, seen: false }
    }

    #[test]
    fn a_question_outranks_an_alert_outranks_a_notice_outranks_a_tip() {
        assert!(rule("call").priority > rule("micBlocked").priority);
        assert!(rule("micBlocked").priority > rule("update").priority);
        assert!(rule("update").priority > rule("hint").priority);
        assert_eq!(rule("something-new").priority, Priority::Info);
    }

    #[test]
    fn one_at_a_time_the_most_important_first() {
        let list = [w("hint", 5_000), w("update", 4_000), w("call", 1_000)];
        assert_eq!(pick(&list, None, false, &m(false, false)).front, Some(2));
        // Without the call: the update, then the hint.
        assert_eq!(pick(&list[..2], None, false, &m(false, false)).front, Some(1));
        assert_eq!(pick(&list[..1], None, false, &m(false, false)).front, Some(0));
        assert_eq!(pick(&[], None, false, &m(false, false)), Pick::default());
    }

    #[test]
    fn equals_queue_oldest_first_and_the_one_up_keeps_its_place() {
        let list = [w("meeting-notice", 1_000), w("word-added", 3_000)];
        assert_eq!(pick(&list, None, false, &m(false, false)).front, Some(1));
        // The newer one is already on screen: it stays.
        assert_eq!(pick(&list, Some(0), false, &m(false, false)).front, Some(0));
    }

    #[test]
    fn a_question_pushes_a_tip_back_unless_the_pointer_is_on_it() {
        let list = [w("word-suggestion", 3_000), w("call", 0)];
        assert_eq!(pick(&list, Some(0), false, &m(false, false)).front, Some(1));
        assert_eq!(pick(&list, Some(0), true, &m(false, false)).front, Some(0));
    }

    #[test]
    fn an_offer_left_up_gives_way_to_a_call_and_comes_back_after() {
        let list = [w("meeting-mistake", 20_000), w("call", 0)];
        assert_eq!(pick(&list, Some(0), false, &m(false, false)).front, Some(1));
        assert_eq!(pick(&list[..1], None, false, &m(false, false)).front, Some(0));
        // …but not to a tip or a notice.
        let list = [w("meeting-mistake", 20_000), w("meeting-notice", 0)];
        assert_eq!(pick(&list, Some(0), false, &m(false, false)).front, Some(0));
        // An update gives way to another notice.
        let list = [w("update", 20_000), w("meeting-notice", 0)];
        assert_eq!(pick(&list, Some(0), false, &m(false, false)).front, Some(1));
    }

    #[test]
    fn an_offer_waits_its_turn_behind_the_card_up_then_goes_first() {
        // "Stopped: the meeting went quiet" is up; "Started by mistake?"
        // comes in: the notice is read first.
        let list = [w("meeting-notice", 2_000), w("meeting-mistake", 0), w("hint", 1_000)];
        assert_eq!(pick(&list, Some(0), false, &m(false, false)).front, Some(0));
        // The notice gone: the question before the tip.
        assert_eq!(pick(&list[1..], None, false, &m(false, false)).front, Some(0));
    }

    #[test]
    fn tips_and_notices_wait_out_a_dictation_questions_and_alerts_dont() {
        let dictating = m(true, false);
        for kind in ["hint", "word-suggestion", "update", "meeting-notice", "word-added", "meeting-mistake", "calls-fatigue"] {
            assert_eq!(pick(&[w(kind, 0)], None, false, &dictating).front, None, "{kind}");
            assert_eq!(pick(&[w(kind, 0)], None, false, &m(false, false)).front, Some(0), "{kind}");
        }
        for kind in ["call", "calendar", "meeting-limit", "meeting-checkin", "micBlocked", "pasteBlocked"] {
            assert_eq!(pick(&[w(kind, 0)], None, false, &dictating).front, Some(0), "{kind}");
        }
        // A held card on screen goes back to waiting while you dictate.
        assert_eq!(pick(&[w("update", 0)], Some(0), false, &dictating).front, None);
    }

    #[test]
    fn the_voice_assistants_answer_shows_at_once_but_a_call_goes_first() {
        // Right after its own question (the settle after a dictation), and
        // over a meeting, where it isn't read aloud.
        for moment in [m(true, false), m(false, true), m(true, true)] {
            assert_eq!(pick(&[w("assistant", 0)], None, false, &moment).front, Some(0));
            assert_eq!(pick(&[w("assistantNoModel", 0)], None, false, &moment).front, Some(0));
            assert_eq!(pick(&[w("assistantFailed", 0)], None, false, &moment).front, Some(0));
        }
        // It never goes stale while it waits behind a call…
        let list = [w("assistant", 10 * MIN), w("call", 0)];
        let p = pick(&list, Some(0), false, &m(false, false));
        assert_eq!(p.front, Some(1));
        assert!(p.stale.is_empty());
        // …and keeps its place against a tip or a notice.
        let list = [w("assistant", 1_000), w("hint", 0), w("meeting-notice", 0)];
        assert_eq!(pick(&list, Some(0), false, &m(false, false)).front, Some(0));
        assert!(!rule("assistant").tip);
    }

    #[test]
    fn an_update_and_a_hint_wait_out_a_meeting() {
        let meeting = m(false, true);
        assert_eq!(pick(&[w("update", 0)], None, false, &meeting).front, None);
        assert_eq!(pick(&[w("hint", 0)], None, false, &meeting).front, None);
        assert_eq!(pick(&[w("meeting-limit", 0)], None, false, &meeting).front, Some(0));
        assert_eq!(pick(&[w("meeting-notice", 0)], None, false, &meeting).front, Some(0));
    }

    #[test]
    fn a_stale_wait_is_dropped_but_never_one_that_was_seen() {
        let list = [w("hint", 11_000), w("update", 10 * MIN), w("call", 10 * MIN)];
        let p = pick(&list, None, false, &m(true, false));
        assert_eq!(p.stale, vec![0]);
        assert_eq!(p.front, Some(2));
        let seen = [Waiting { seen: true, ..w("hint", 31_000) }];
        assert!(pick(&seen, None, false, &m(false, false)).stale.is_empty());
        let notice = [w("meeting-notice", MIN + 1)];
        assert_eq!(pick(&notice, None, false, &m(false, false)).stale, vec![0]);
        // A word suggestion may wait out a dictation of under a minute.
        assert!(pick(&[w("word-suggestion", 50_000)], None, false, &m(true, false)).stale.is_empty());
    }

    #[test]
    fn with_tips_off_a_tip_is_dropped_and_nothing_else() {
        let off = Moment { no_tips: true, ..m(false, false) };
        let list = [w("hint", 0), w("word-suggestion", 0), w("update", 0)];
        let p = pick(&list, None, false, &off);
        assert_eq!(p.stale, vec![0]);
        assert_eq!(p.front, Some(2));
        assert!(!muted(&rule("micBlocked"), &off));
    }

    #[test]
    fn the_same_notice_again_within_its_cooldown_is_dropped() {
        let mut l = Ledger::default();
        let fp = fingerprint("Windows is blocking the microphone", "…");
        assert!(l.admit("micBlocked", fp, T));
        assert!(!l.admit("micBlocked", fp, T + 60_000));
        assert!(l.admit("micBlocked", fp, T + 10 * MIN));
        // Different text is a different notice.
        assert!(l.admit("micBlocked", fingerprint("Windows is blocking", "other"), T + 10 * MIN + 1));
        // Kinds without a cooldown always go.
        let paste = fingerprint("Your text is copied: press Ctrl+V", "");
        assert!(l.admit("pasteBlocked", paste, T));
        assert!(l.admit("pasteBlocked", paste, T + 1));
        // A Windows notification keeps its own count, under its kind's rule.
        assert!(l.admit("native:meeting-notice", fp, T));
        assert!(!l.admit("native:meeting-notice", fp, T + 1));
    }

    #[test]
    fn one_place_where_the_person_is_looking() {
        // Looking at Yap: its window.
        assert_eq!(place_for(true, true, true, true), Place::Window);
        // In another app: the bar, else a Windows notification…
        assert_eq!(place_for(false, true, true, true), Place::Bar);
        assert_eq!(place_for(false, true, false, true), Place::Notification);
        // …else the window if it's on screen after all.
        assert_eq!(place_for(false, true, false, false), Place::Window);
        assert_eq!(place_for(false, false, false, false), Place::Nowhere);
        assert_eq!(place_for(false, false, true, false), Place::Bar);
    }

    #[test]
    fn a_windows_notification_skips_the_banner_while_dictating_or_busy_unless_it_needs_acting_on() {
        assert!(quietly(&rule("update"), &m(true, false), false));
        assert!(quietly(&rule("meeting-notice"), &m(false, false), true));
        assert!(!quietly(&rule("update"), &m(false, false), false));
        assert!(!quietly(&rule("call"), &m(true, false), true));
        assert!(!quietly(&rule("micBlocked"), &m(true, false), true));
    }

    #[test]
    fn three_not_nows_in_a_row_offer_once_to_stop_asking() {
        let f = Fatigue::default();
        let (f, v) = f.answered(Answer::Closed, false);
        assert_eq!(v, Verdict::Nothing);
        // A prompt left to fade doesn't break the streak.
        let (f, _) = f.answered(Answer::Ignored, false);
        let (f, v) = f.answered(Answer::Closed, false);
        assert_eq!(v, Verdict::Nothing);
        let (f, v) = f.answered(Answer::Closed, false);
        assert_eq!(v, Verdict::OfferToStop { ignored: false });
        // Once ever.
        let (f, v) = f.answered(Answer::Closed, false);
        assert_eq!(v, Verdict::Nothing);
        assert!(f.offered);
    }

    #[test]
    fn five_prompts_left_to_fade_offer_too_and_a_yes_resets_the_streaks() {
        let mut f = Fatigue::default();
        for _ in 0..4 {
            let (next, v) = f.answered(Answer::Ignored, false);
            assert_eq!(v, Verdict::Nothing);
            f = next;
        }
        // A "Not now" in between starts the fading count again.
        let (f2, _) = f.clone().answered(Answer::Closed, false);
        assert_eq!(f2.ignored, 0);
        let (_, v) = f.clone().answered(Answer::Ignored, false);
        assert_eq!(v, Verdict::OfferToStop { ignored: true });
        let (f, v) = f.answered(Answer::Engaged, false);
        assert_eq!(v, Verdict::Nothing);
        assert_eq!((f.closed, f.ignored, f.shown), (0, 0, 0));
    }

    #[test]
    fn someone_who_records_meetings_is_never_offered_anything() {
        let mut f = Fatigue::default();
        for _ in 0..10 {
            let (next, v) = f.answered(Answer::Closed, true);
            assert_eq!(v, Verdict::Nothing);
            let (next, v) = next.prompted(true);
            assert_eq!(v, Verdict::Nothing);
            f = next;
        }
    }

    #[test]
    fn thirty_five_prompts_without_a_meeting_switch_detection_off_once() {
        let mut f = Fatigue::default();
        for i in 1..PROMPTS_WITHOUT_A_MEETING {
            let (next, v) = f.prompted(false);
            assert_eq!(v, Verdict::Nothing, "prompt {i}");
            f = next;
        }
        let (f, v) = f.prompted(false);
        assert_eq!(v, Verdict::SwitchOff);
        let (_, v) = f.prompted(false);
        assert_eq!(v, Verdict::Nothing);
    }

    #[test]
    fn fingerprints_tell_title_from_body() {
        assert_ne!(fingerprint("ab", "c"), fingerprint("a", "bc"));
        assert_eq!(fingerprint("a", "b"), fingerprint("a", "b"));
    }
}
