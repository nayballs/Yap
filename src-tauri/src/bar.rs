//! The **Yap bar** — Yap's always-there pill, after Wispr Flow's Flow Bar
//! (traced 2026-10-05; `E:\Projects\references\wispr-flow\README.md`). It's
//! the `overlay` window (`overlay.rs` holds its Win32 side and the maths):
//!
//! - **Idle** it's a tiny dark pill above the taskbar (Wispr's measured
//!   40 × 8, 14 px up; `flowbar-spec.md`). Hovering it expands it into
//!   🎤 Dictate, ◉ Meeting notes and a ^ menu (Open Yap, New meeting note,
//!   Settings, Hide the bar for 1 hour, Turn off the bar). While a dictation
//!   records or transcribes it's the dictation overlay (waveform, live text);
//!   while a meeting records, a 69 × 30 recording pill (bars, stop; the timer
//!   when hovered).
//! - **Clickable only where it draws something.** The window is click-through
//!   (`WS_EX_TRANSPARENT`). The page reports its interactive rects (the pill's
//!   hover zone, cards, the menu; `bar_regions`), and a poller here watches the
//!   cursor: on a rect, the window takes clicks; off them, clicks go through
//!   again — Electron's `setIgnoreMouseEvents(true, { forward: true })`, done
//!   natively (Wispr's bar does exactly this: WS_EX_TRANSPARENT cleared only
//!   while the pointer rests on the pill). It never takes focus: `focusable:
//!   false` (WS_EX_NOACTIVATE), a click answers `WM_MOUSEACTIVATE` with
//!   MA_NOACTIVATE, and should Windows activate it anyway the focus goes
//!   straight back to the app you were in — so 🎤 dictates into that app.
//! - **Follows you.** It sits bottom-centre on the work area of the monitor
//!   with the mouse cursor (not the foreground window), moving there once the
//!   cursor has stayed on another monitor for 200 ms (Wispr: ~340 ms). It
//!   jumps; there's no slide. While a dictation records or transcribes it
//!   stays where the dictation started. The poll is cheap: `GetCursorPos`
//!   every 250 ms, every 30 ms only while the cursor is near the bar, once a
//!   second while it's hidden over a fullscreen app.
//! - **Out of the way in fullscreen.** Over a fullscreen app on its monitor
//!   (a game, a video, a slideshow) the idle pill hides
//!   (`overlay::fullscreen_on`); it always shows while recording. A card
//!   still shows over a borderless fullscreen app (Wispr's call card appears
//!   over a game), but waits out an exclusive-fullscreen one, unseen, until
//!   you alt-tab out.
//! - **Cards.** [`show_card`] / [`update_card`] / [`dismiss_card`] put cards
//!   above the pill — Yap's notices while its main window isn't focused (the
//!   call prompts of `meeting_detect.rs`, as Wispr's "Meeting detected" card;
//!   the calendar's "Design review · In 1 min" reminders from `calendar.rs`;
//!   "update ready" from `updates.rs`), instead of a Windows notification.
//!   They return `false` when the bar is off or hidden for an hour, and
//!   callers fall back to the notification.
//!
//! Settings → General → Yap bar: `bar_enabled` (on), `bar_hide_fullscreen`
//! (on); `overlay_position` puts it at the top instead. With the bar off Yap
//! behaves as before: the overlay only while dictating, notices as Windows
//! notifications.
//!
//! Test mode (`e2e::active`) never reads the real cursor or the window in
//! front: the debug-only [`bar_simulate`] moves a pretend cursor onto a
//! region (exercising the same hit-test and click-through code) and fakes a
//! fullscreen app, and [`bar_debug`] reports the window's real ex-styles.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{LazyLock, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Listener, Manager};

use crate::overlay::{self, Edge, Fullscreen, Rect, Region, Screen};

/// Snapshot for the bar page and Settings (`bar_status` + this event).
const EVENT: &str = "yap-bar";
/// The region under the pointer, `{ region }` (`null` once it left).
const EVENT_POINTER: &str = "yap-bar-pointer";
/// It just moved to another monitor (the page plays its pop-in).
const EVENT_MOVED: &str = "yap-bar-moved";
/// The bar changed a setting itself ("Turn off the bar"): `{ enabled }`,
/// for Settings' copy of the config.
const EVENT_CHANGED: &str = "yap-bar-changed";

/// Monitor follow, fullscreen and visibility checks.
const SLOW_TICK: Duration = Duration::from_millis(250);
/// Hover checks while the cursor is near the bar (or on it).
const FAST_TICK: Duration = Duration::from_millis(30);
/// Hidden over a fullscreen app (a game): a lighter watch until it's over.
const HIDDEN_TICK: Duration = Duration::from_secs(1);
/// Nothing to watch (the bar is off and no dictation shows it).
const IDLE_TICK: Duration = Duration::from_secs(60);
/// The cursor stays on another monitor this long before the bar follows, so
/// a flick across an edge doesn't drag it along (Wispr: ~340 ms).
const FOLLOW_AFTER_MS: u64 = 200;
/// "Near" the bar: within this many px of its window, the poll speeds up.
const NEAR_PX: i32 = 64;
/// CSS px of slack around a region once the pointer is on it (no flicker
/// between clickable and click-through at its edge).
const SLACK: f64 = 6.0;
/// "Hide the bar for 1 hour".
const HIDE_FOR_MS: u64 = 60 * 60_000;
/// How often the auto-hidden-taskbar check (a message to the shell) reruns.
const AUTOHIDE_EVERY_MS: u64 = 10_000;

// ---- cards -------------------------------------------------------------------------

/// A card above the pill. Showing a card with the `id` of one already up
/// replaces it.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    /// Stable key: "call", "update", …
    pub id: String,
    /// "" — a notice: the glyph, title, body, then its buttons; or "call" —
    /// Wispr's "Meeting detected" card: one row with the call app's mark,
    /// the title over a `status` line, and a light split button (`primary`,
    /// with `secondary` and `link` in its ^ menu); the ✕ sits on its corner.
    pub style: &'static str,
    /// The glyph by the title: "call" | "update" | "notes" | "timer" |
    /// "screen" | "error" | "". On a call card, "calendar" stands in for a
    /// missing app mark (a calendar meeting with no call app).
    pub icon: &'static str,
    /// The call app (`meeting_detect::APPS` id), for its mark.
    pub app: Option<&'static str>,
    pub title: String,
    pub body: String,
    /// The line under a call card's title ("Now")…
    pub status: Option<String>,
    /// …after a dot: "live" (green: happening now), "recording" (red),
    /// "soon" (amber: a calendar meeting about to start), "".
    pub dot: &'static str,
    /// The light button on the right.
    pub primary: Option<CardAction>,
    /// The quiet button beside it.
    pub secondary: Option<CardAction>,
    /// A small link on its own line under the buttons (a rarer third answer,
    /// "Don't ask for Teams").
    pub link: Option<CardAction>,
    /// Fades after this long (paused while the pointer is on it), reporting
    /// `expire_action`. `None`: it stays until answered.
    pub timeout_ms: Option<u64>,
    pub expire_action: Option<String>,
    /// What its ✕ reports (`None`: it just closes).
    pub close_action: Option<String>,
    /// What Esc reports while the card is up. The bar never has the keyboard
    /// focus, so Yap watches the key only for as long as such a card shows.
    pub escape_action: Option<String>,
    /// A live countdown on the card ("Starting notes in 7…").
    pub countdown: Option<Countdown>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CardAction {
    /// Reported to the card's handler when clicked.
    pub id: String,
    pub label: String,
}

impl CardAction {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self { id: id.into(), label: label.into() }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Countdown {
    /// When it reaches zero (unix ms). Whoever showed the card acts then.
    pub until: u64,
    /// "Starting notes in" → "Starting notes in 7…".
    pub label: String,
}

/// A card's answers: called (off the main thread) with the clicked action's
/// id, or its expire / close / escape action.
pub type OnAction = Box<dyn Fn(&AppHandle, &str) + Send + Sync + 'static>;

// ---- state -------------------------------------------------------------------------

/// What a dictation is doing (from `yap-state`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "lowercase")]
enum Dictation {
    #[default]
    Idle,
    Recording,
    Processing,
    Error,
}

impl Dictation {
    fn from_state(state: &str) -> Self {
        match state {
            "recording" => Self::Recording,
            "processing" | "processing-slow" => Self::Processing,
            "error" => Self::Error,
            _ => Self::Idle,
        }
    }
}

/// The inputs of "is the bar on screen?".
#[derive(Debug, Clone, Copy, Default)]
struct Visibility {
    /// `bar_enabled`.
    enabled: bool,
    dictation: Dictation,
    /// A meeting is recording.
    meeting: bool,
    /// "Hide the bar for 1 hour" is running.
    hidden: bool,
    /// What's fullscreen on its monitor (`None` with `bar_hide_fullscreen`
    /// off).
    fullscreen: Fullscreen,
    /// A card is up.
    cards: bool,
}

/// Whether the window is on screen: always while a dictation records,
/// transcribes or just failed (it's the hot-mic indicator, with the bar off
/// too, as the overlay always was); with the bar on, also while a meeting
/// records, and otherwise unless hidden for an hour or over a fullscreen app
/// — though a card still shows over a borderless one (Wispr's call card
/// appears over a game), just not the idle pill.
fn shows(v: Visibility) -> bool {
    if v.dictation != Dictation::Idle {
        return true;
    }
    if !v.enabled {
        return false;
    }
    if v.meeting {
        return true;
    }
    if v.hidden {
        return false;
    }
    match v.fullscreen {
        Fullscreen::None => true,
        Fullscreen::Borderless => v.cards,
        Fullscreen::Exclusive => false,
    }
}

/// Whether the bar takes cards: it's on and not hidden for an hour. Over an
/// exclusive-fullscreen app they wait, unseen, until it's over (a call
/// prompt is still there when you alt-tab out; its call ending withdraws
/// it), rather than going to the notification center.
fn takes_cards(v: Visibility) -> bool {
    v.enabled && !v.hidden
}

/// Whether a card would be on screen now (not waiting out an exclusive
/// fullscreen app): what a countdown on it needs.
fn seen_now(v: Visibility) -> bool {
    takes_cards(v) && v.fullscreen != Fullscreen::Exclusive
}

/// Whether the window should take clicks: only with the bar on and showing,
/// and only while the cursor is on a region the page reported.
fn clickable(v: Visibility, shown: bool, on_region: bool) -> bool {
    v.enabled && shown && on_region
}

/// A monitor change waiting out [`FOLLOW_AFTER_MS`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pending {
    id: isize,
    since: u64,
}

/// Where the bar should go now (`None`: stay put). `placed` is the screen it
/// was last put on, `current` that monitor read again now (`None` if it was
/// unplugged, or before the first placement), `cursor` the cursor's monitor.
/// - Never placed, or its monitor gone: the cursor's monitor, at once.
/// - The cursor on another monitor: there, once it has stayed
///   [`FOLLOW_AFTER_MS`] — unless `frozen` (a dictation is recording or
///   transcribing: the bar stays where it started).
/// - Its own monitor changed (work area, scale): re-placed there.
fn follow(
    placed: Option<&Screen>,
    current: Option<Screen>,
    cursor: Option<Screen>,
    pending: &mut Option<Pending>,
    frozen: bool,
    now: u64,
) -> Option<Screen> {
    let Some(current) = current else {
        *pending = None;
        return cursor;
    };
    match cursor {
        Some(c) if c.id != current.id && !frozen => match *pending {
            Some(p) if p.id == c.id => {
                if now.saturating_sub(p.since) >= FOLLOW_AFTER_MS {
                    *pending = None;
                    return Some(c);
                }
            }
            _ => *pending = Some(Pending { id: c.id, since: now }),
        },
        _ => *pending = None,
    }
    (placed != Some(&current)).then_some(current)
}

struct State {
    enabled: bool,
    hide_fullscreen: bool,
    edge: Edge,
    /// The dictation hotkey and meeting-notes shortcut specs, for tooltips.
    hotkey: String,
    meeting_hotkey: Option<String>,
    /// "Hide the bar for 1 hour" until (unix ms), 0 = not hidden.
    hidden_until: u64,
    dictation: Dictation,
    meeting: bool,
    /// What's fullscreen on the bar's monitor (whatever the setting).
    fullscreen: Fullscreen,
    /// What the window was last told to be.
    shown: bool,
    /// The page's interactive rects.
    regions: Vec<Region>,
    /// The window takes clicks (`WS_EX_TRANSPARENT` cleared).
    interactive: bool,
    /// The region under the cursor while interactive.
    pointer: Option<String>,
    /// The cursor is near the window (the poll speeds up).
    near: bool,
    /// The screen the bar was last placed on, and the rect it was put at.
    placed: Option<Screen>,
    rect: Option<Rect>,
    /// Place again even if the screen didn't change (the edge setting did).
    replace: bool,
    pending: Option<Pending>,
    /// When the auto-hidden-taskbar check last ran (unix ms).
    autohide_at: u64,
    cards: Vec<Card>,
    handlers: HashMap<String, OnAction>,
    /// The last foreground window that wasn't Yap's: where the focus goes
    /// back to if a click activates the bar after all.
    prev_front: isize,
    /// Test mode's pretend cursor (`bar_simulate`).
    sim_cursor: Option<(i32, i32)>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            enabled: true,
            hide_fullscreen: true,
            edge: Edge::Bottom,
            hotkey: String::new(),
            meeting_hotkey: None,
            hidden_until: 0,
            dictation: Dictation::Idle,
            meeting: false,
            fullscreen: Fullscreen::None,
            shown: false,
            regions: Vec::new(),
            interactive: false,
            pointer: None,
            near: false,
            placed: None,
            rect: None,
            replace: false,
            pending: None,
            autohide_at: 0,
            cards: Vec::new(),
            handlers: HashMap::new(),
            prev_front: 0,
            sim_cursor: None,
        }
    }
}

impl State {
    /// What's fullscreen, as far as the bar cares (`bar_hide_fullscreen`).
    fn fullscreen(&self) -> Fullscreen {
        if self.hide_fullscreen {
            self.fullscreen
        } else {
            Fullscreen::None
        }
    }

    fn visibility(&self, now: u64) -> Visibility {
        Visibility {
            enabled: self.enabled,
            dictation: self.dictation,
            meeting: self.meeting,
            hidden: self.hidden_until > now,
            fullscreen: self.fullscreen(),
            cards: !self.cards.is_empty(),
        }
    }
}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| Mutex::new(State::default()));
/// The bar takes cards right now ([`takes_cards`]), readable without the
/// lock (`meeting_detect` asks while holding its own)…
static CARDS: AtomicBool = AtomicBool::new(false);
/// …and they'd be on screen ([`seen_now`]).
static CARDS_SEEN: AtomicBool = AtomicBool::new(false);
/// Wakes the poller (a state change, new regions, a card, Esc).
static WAKE: Mutex<Option<mpsc::Sender<()>>> = Mutex::new(None);
/// Windows activated the bar after all (`WM_ACTIVATE`): give the focus back.
static ACTIVATED: AtomicBool = AtomicBool::new(false);
/// Esc was pressed while a card with an `escape_action` was up.
static ESCAPED: AtomicBool = AtomicBool::new(false);
static APP: OnceLock<AppHandle> = OnceLock::new();

/// Never held while touching windows, emitting, or taking another module's
/// lock: `run_on_main_thread` runs at once when called on the main thread,
/// and window calls wait on it.
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

/// From the window procedure (`WM_ACTIVATE`): quick, never blocks.
fn on_activated() {
    ACTIVATED.store(true, Ordering::Relaxed);
    wake();
}

/// From the Esc watch's keyboard hook: quick, never blocks.
#[cfg(windows)]
fn on_escape_key() {
    ESCAPED.store(true, Ordering::Relaxed);
    wake();
}

// ---- snapshot ------------------------------------------------------------------------

/// What the bar page (and Settings) render.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    enabled: bool,
    hide_fullscreen: bool,
    /// On screen right now.
    shown: bool,
    /// "bottom" | "top" (`overlay_position`).
    edge: &'static str,
    /// "Hide the bar for 1 hour": until when (unix ms).
    hidden_until: Option<u64>,
    /// "none" | "borderless" (the idle pill hides; cards still show) |
    /// "exclusive" (everything waits), with `bar_hide_fullscreen` on.
    fullscreen: &'static str,
    /// The dictation hotkey spec ("kb:120"), for the 🎤 tooltip.
    hotkey: String,
    /// The meeting-notes shortcut spec, if one is set, for the ◉ tooltip.
    meeting_hotkey: Option<String>,
    /// `meeting::state()`: `{ recording, noteId, elapsedSecs }`.
    meeting: serde_json::Value,
    /// A live call ◉ would record ("Record this Teams call").
    call: Option<String>,
    cards: Vec<Card>,
}

pub fn status() -> Status {
    // Other modules' state first: never with this module's lock held.
    let meeting = crate::meeting::state();
    let call = crate::meeting_detect::tray_item().map(|(_, label)| label);
    let now = now_ms();
    let s = lock();
    Status {
        enabled: s.enabled,
        hide_fullscreen: s.hide_fullscreen,
        shown: s.shown,
        edge: s.edge.as_str(),
        hidden_until: (s.hidden_until > now).then_some(s.hidden_until),
        fullscreen: s.fullscreen().as_str(),
        hotkey: s.hotkey.clone(),
        meeting_hotkey: s.meeting_hotkey.clone(),
        meeting,
        call,
        cards: s.cards.clone(),
    }
}

fn emit(app: &AppHandle) {
    let _ = app.emit(EVENT, status());
}

// ---- the window ----------------------------------------------------------------------

fn window(app: &AppHandle) -> Option<tauri::WebviewWindow> {
    app.get_webview_window(overlay::LABEL)
}

/// Run `f` on the main thread (where the window lives). At once when
/// already there — so never call this with the state lock held.
fn on_main(app: &AppHandle, f: impl FnOnce(&AppHandle) + Send + 'static) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || f(&handle));
}

/// Show or hide the window. Showing never activates it (`focus: false`:
/// tao shows it with SW_SHOWNOACTIVATE) and puts it on top of the
/// always-on-top band.
fn set_shown(app: &AppHandle, shown: bool) {
    on_main(app, move |app| {
        let Some(w) = window(app) else { return };
        if shown {
            let _ = w.show();
            overlay::force_topmost(&w);
        } else {
            let _ = w.hide();
        }
    });
}

fn set_click_through(app: &AppHandle, on: bool) {
    on_main(app, move |app| {
        #[cfg(windows)]
        if let Some(Ok(hwnd)) = window(app).map(|w| w.hwnd()) {
            overlay::win::set_click_through(hwnd.0, on);
        }
        #[cfg(not(windows))]
        if let Some(w) = window(app) {
            let _ = w.set_ignore_cursor_events(on);
        }
    });
}

/// Put the window on `screen`, sized as it is now. A move to a monitor with
/// another scale makes Windows resize it (`WM_DPICHANGED`, which tao answers
/// keeping its logical size), so it's placed again with the new size.
#[cfg(windows)]
fn place(app: &AppHandle, screen: Screen, edge: Edge, moved_monitor: bool) {
    on_main(app, move |app| {
        let Some(Ok(hwnd)) = window(app).map(|w| w.hwnd()) else { return };
        let hwnd = hwnd.0;
        let mut rect = overlay::win::window_rect(hwnd);
        for _ in 0..3 {
            let Some(now) = rect else { break };
            let target = overlay::place(&screen, (now.width(), now.height()), edge);
            if target == now {
                break;
            }
            overlay::win::move_to(hwnd, target.left, target.top);
            rect = overlay::win::window_rect(hwnd);
        }
        {
            let mut s = lock();
            s.placed = Some(screen);
            s.rect = rect;
        }
        if moved_monitor {
            let _ = app.emit(EVENT_MOVED, ());
        }
    });
}

// ---- the poller ----------------------------------------------------------------------

/// The cursor: the real one, or test mode's pretend one.
fn cursor() -> Option<(i32, i32)> {
    if crate::e2e::active() {
        return lock().sim_cursor;
    }
    #[cfg(windows)]
    {
        overlay::win::cursor()
    }
    #[cfg(not(windows))]
    None
}

fn poller(app: AppHandle, rx: mpsc::Receiver<()>) {
    let mut last_slow = 0u64;
    loop {
        let timeout = {
            let s = lock();
            let watching = s.enabled || s.dictation != Dictation::Idle;
            if !watching {
                IDLE_TICK
            } else if s.interactive || s.near {
                FAST_TICK
            } else if !s.shown && s.fullscreen() != Fullscreen::None {
                HIDDEN_TICK
            } else {
                SLOW_TICK
            }
        };
        match rx.recv_timeout(timeout) {
            Ok(()) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        while rx.try_recv().is_ok() {} // one pass covers a burst of wake-ups
        if ACTIVATED.swap(false, Ordering::Relaxed) {
            give_focus_back();
        }
        if ESCAPED.swap(false, Ordering::Relaxed) {
            escape(&app);
        }
        let now = now_ms();
        let cursor = cursor();
        if now.saturating_sub(last_slow) >= SLOW_TICK.as_millis() as u64 {
            last_slow = now;
            #[cfg(windows)]
            {
                follow_cursor(&app, cursor, now);
                check_fullscreen(&app);
            }
            apply(&app);
        }
        hover(&app, cursor);
    }
}

/// Follow the cursor's monitor (see [`follow`]).
#[cfg(windows)]
fn follow_cursor(app: &AppHandle, cursor: Option<(i32, i32)>, now: u64) {
    use overlay::win;
    let (placed, frozen, edge, replace, autohide_due) = {
        let s = lock();
        let watching = s.enabled || s.dictation != Dictation::Idle;
        if !watching {
            return;
        }
        (
            s.placed,
            s.dictation == Dictation::Recording || s.dictation == Dictation::Processing,
            s.edge,
            s.replace,
            now.saturating_sub(s.autohide_at) >= AUTOHIDE_EVERY_MS,
        )
    };
    // No cursor (the lock screen or a UAC prompt has the input): stay put.
    // Test mode without a pretend cursor: the primary monitor.
    let point = match cursor {
        Some(p) => p,
        None if crate::e2e::active() => (0, 0),
        None => return,
    };
    // The cursor's monitor (its auto-hidden taskbars are read only if the bar
    // goes there, below).
    let cursor_screen = win::screen_at(point, false);
    // The bar's own monitor again: its auto-hidden taskbars are re-checked
    // only now and then (a message to the shell), else carried over.
    let current = placed.and_then(|p| {
        win::screen(p.id, autohide_due).map(|mut s| {
            if !autohide_due {
                s.autohide_bottom = p.autohide_bottom;
                s.autohide_top = p.autohide_top;
            }
            s
        })
    });
    let target = {
        let mut s = lock();
        if autohide_due {
            s.autohide_at = now;
        }
        s.replace = false;
        let mut pending = s.pending;
        let target = follow(placed.as_ref(), current, cursor_screen, &mut pending, frozen, now);
        s.pending = pending;
        target.or(if replace { current.or(cursor_screen) } else { None })
    };
    if let Some(screen) = target {
        let moved = placed.is_some_and(|p| p.id != screen.id);
        // Going to another monitor: read its auto-hidden taskbars now.
        let screen = if moved || placed.is_none() { win::screen(screen.id, true).unwrap_or(screen) } else { screen };
        place(app, screen, edge, moved);
    }
}

/// What's fullscreen on the bar's monitor? (Not read in test mode, where
/// `bar_simulate` stands in.)
#[cfg(windows)]
fn check_fullscreen(app: &AppHandle) {
    if crate::e2e::active() {
        return;
    }
    let (wanted, screen, before) = {
        let s = lock();
        (s.hide_fullscreen && s.enabled, s.placed, s.fullscreen)
    };
    let foreign = overlay::win::foreign_foreground();
    let fullscreen = match (wanted, screen) {
        (true, Some(screen)) => {
            let front = overlay::win::front();
            // Exclusive mode or a slideshow is asked about only when the
            // window in front covers the bar's monitor (cheaper, and a game
            // on another monitor leaves this one's bar alone).
            let covers = front.as_ref().is_some_and(|f| f.rect.covers(&screen.monitor));
            let quns = if covers { overlay::win::notification_state() } else { 0 };
            overlay::fullscreen_on(front.as_ref(), quns, &screen)
        }
        _ => Fullscreen::None,
    };
    {
        let mut s = lock();
        s.fullscreen = fullscreen;
        if foreign != 0 {
            s.prev_front = foreign;
        }
    }
    if fullscreen != before {
        tracing::info!(fullscreen = fullscreen.as_str(), "bar: fullscreen");
        emit(app);
    }
}

/// Show or hide the window as [`shows`] says, and keep [`CARDS`] current.
fn apply(app: &AppHandle) {
    let now = now_ms();
    let (change, takes, seen) = {
        let mut s = lock();
        let v = s.visibility(now);
        let want = shows(v);
        let change = (want != s.shown).then(|| {
            s.shown = want;
            if !want {
                s.interactive = false;
                s.pointer = None;
            }
            want
        });
        (change, takes_cards(v), seen_now(v))
    };
    let has_window = window(app).is_some();
    CARDS.store(takes && has_window, Ordering::Relaxed);
    CARDS_SEEN.store(seen && has_window, Ordering::Relaxed);
    sync_escape();
    if let Some(shown) = change {
        tracing::info!(shown, "bar: {}", if shown { "shown" } else { "hidden" });
        if !shown {
            set_click_through(app, true);
        }
        set_shown(app, shown);
        emit(app);
    }
}

/// Clickable while the cursor is on a region, click-through otherwise.
fn hover(app: &AppHandle, cursor: Option<(i32, i32)>) {
    #[cfg(windows)]
    let geometry = window(app)
        .and_then(|w| w.hwnd().ok())
        .and_then(|h| overlay::win::window_rect(h.0).map(|r| (r, overlay::win::window_scale(h.0))));
    #[cfg(not(windows))]
    let geometry: Option<(Rect, f64)> = None;
    let Some((rect, scale)) = geometry else { return };
    let (toggle, pointer, became_interactive) = {
        let mut s = lock();
        s.near = cursor.is_some_and(|(x, y)| rect.inflate(NEAR_PX).contains(x, y));
        let slack = if s.interactive { SLACK } else { 0.0 };
        let hit = cursor
            .and_then(|c| overlay::region_at(&s.regions, c, (rect.left, rect.top), scale, slack))
            .map(|r| r.id.clone());
        let want = clickable(s.visibility(now_ms()), s.shown, hit.is_some());
        let toggle = (want != s.interactive).then_some(want);
        s.interactive = want;
        let region = if want { hit } else { None };
        let pointer = (region != s.pointer).then(|| region.clone());
        s.pointer = region;
        (toggle, pointer, toggle == Some(true))
    };
    if became_interactive {
        // Where the focus goes back to, should a click activate the bar.
        #[cfg(windows)]
        if !crate::e2e::active() {
            let fg = overlay::win::foreign_foreground();
            if fg != 0 {
                lock().prev_front = fg;
            }
        }
    }
    if let Some(interactive) = toggle {
        set_click_through(app, !interactive);
    }
    if let Some(region) = pointer {
        let _ = app.emit(EVENT_POINTER, serde_json::json!({ "region": region }));
    }
}

/// The bar got activated after all (a WebView2 quirk): the app you were in
/// gets the focus back, so typing and pasting carry on there.
fn give_focus_back() {
    #[cfg(windows)]
    {
        if crate::e2e::active() || !overlay::win::bar_in_front() {
            return;
        }
        let prev = lock().prev_front;
        tracing::info!("bar: activated by a click; giving the focus back");
        overlay::win::give_back_focus(prev);
    }
}

/// Esc while a card with an `escape_action` is up: the newest such card.
fn escape(app: &AppHandle) {
    let target = {
        let s = lock();
        s.cards
            .iter()
            .rev()
            .find_map(|c| c.escape_action.clone().map(|a| (c.id.clone(), a)))
    };
    if let Some((id, action)) = target {
        tracing::info!(card = %id, "bar: Esc");
        card_action(app, &id, &action);
    }
}

// ---- public API ---------------------------------------------------------------------

/// Start the bar (app setup, main thread): make the overlay window the bar
/// window, then the poller.
pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
    let cfg = crate::config::load();
    {
        let mut s = lock();
        read_config(&mut s, &cfg);
        s.meeting = crate::meeting::is_recording();
    }
    if let Some(w) = window(app) {
        #[cfg(windows)]
        if let Ok(hwnd) = w.hwnd() {
            overlay::win::install(hwnd.0, on_activated);
        }
        // As before the bar: click-through and on top (tao's flags; the
        // ex-style itself is kept by `overlay::win`). Its one size, in
        // logical px — it never resizes with what it shows.
        let _ = w.set_ignore_cursor_events(true);
        let _ = w.set_always_on_top(true);
        let _ = w.set_size(tauri::LogicalSize::new(overlay::WIDTH, overlay::HEIGHT));
    }
    let (tx, rx) = mpsc::channel();
    let _ = tx.send(());
    *WAKE.lock().unwrap_or_else(|p| p.into_inner()) = Some(tx);
    let handle = app.clone();
    if let Err(e) = std::thread::Builder::new()
        .name("yap-bar".into())
        .spawn(move || poller(handle, rx))
    {
        tracing::warn!("bar: couldn't start ({e})");
    }
    // A meeting starting or stopping (the recording pill), a call coming or
    // going (◉'s tooltip).
    let h = app.clone();
    app.listen("yap-meeting-state", move |event| {
        let recording = serde_json::from_str::<serde_json::Value>(event.payload())
            .ok()
            .and_then(|v| v["recording"].as_bool())
            .unwrap_or(false);
        lock().meeting = recording;
        emit(&h);
        wake();
    });
    let h = app.clone();
    app.listen("yap-meeting-detect", move |_| emit(&h));
    tracing::info!(enabled = cfg.bar_enabled, "bar: started");
}

fn read_config(s: &mut State, cfg: &crate::config::YapConfig) -> bool {
    let edge = Edge::from_config(&cfg.overlay_position);
    // Read loosely: the meeting-notes shortcut may not exist in this build.
    let meeting_hotkey = serde_json::to_value(cfg)
        .ok()
        .and_then(|v| v.get("meetingHotkey").and_then(|h| h.as_str()).map(str::to_string))
        .filter(|h| !h.is_empty());
    let changed = s.enabled != cfg.bar_enabled
        || s.hide_fullscreen != cfg.bar_hide_fullscreen
        || s.edge != edge
        || s.hotkey != cfg.hotkey
        || s.meeting_hotkey != meeting_hotkey;
    s.replace |= s.edge != edge;
    s.enabled = cfg.bar_enabled;
    s.hide_fullscreen = cfg.bar_hide_fullscreen;
    s.edge = edge;
    s.hotkey = cfg.hotkey.clone();
    s.meeting_hotkey = meeting_hotkey;
    changed
}

/// Settings were saved (`save_config`): apply the bar settings at once.
pub fn sync(app: &AppHandle, cfg: &crate::config::YapConfig) {
    let changed = read_config(&mut lock(), cfg);
    if changed {
        tracing::info!(enabled = cfg.bar_enabled, fullscreen = cfg.bar_hide_fullscreen, "bar: settings changed");
        apply(app);
        emit(app);
        wake();
    }
}

/// A dictation changed state (`yap-state`). Starting one puts the bar on
/// the cursor's monitor at once, where it stays until the dictation is over.
/// Called from the `yap-state` listener: it never locks the pipeline.
pub fn on_pipeline_state(app: &AppHandle, state: &str) {
    let d = Dictation::from_state(state);
    let starting = {
        let mut s = lock();
        let starting = d == Dictation::Recording && s.dictation != Dictation::Recording;
        s.dictation = d;
        if starting {
            s.pending = None;
        }
        starting
    };
    #[cfg(windows)]
    if starting {
        let (placed, edge) = {
            let s = lock();
            (s.placed, s.edge)
        };
        if let Some(screen) = cursor().and_then(|c| overlay::win::screen_at(c, false)) {
            if placed.map(|p| p.id) != Some(screen.id) {
                let screen = overlay::win::screen(screen.id, true).unwrap_or(screen);
                place(app, screen, edge, placed.is_some());
            }
        }
    }
    #[cfg(not(windows))]
    let _ = starting;
    apply(app);
    wake();
}

/// The bar takes cards right now (it's on and not hidden for an hour; over
/// an exclusive-fullscreen app they wait to be seen). Lock-free, so callers
/// may hold their own locks.
pub fn cards_available() -> bool {
    CARDS.load(Ordering::Relaxed)
}

/// A card shown now would be seen now (not waiting out an exclusive
/// fullscreen app): what a countdown needs. Lock-free.
pub fn cards_seen() -> bool {
    CARDS_SEEN.load(Ordering::Relaxed)
}

/// Whether the card `id` is up and on screen.
pub fn card_on_screen(id: &str) -> bool {
    let s = lock();
    s.shown && s.cards.iter().any(|c| c.id == id) && cards_seen()
}

/// Show `card` above the pill (replacing one with the same id); its answers
/// go to `on_action`. `false` when the bar is off or hidden: the caller
/// falls back to a Windows notification.
pub fn show_card(app: &AppHandle, card: Card, on_action: OnAction) -> bool {
    if !cards_available() {
        return false;
    }
    tracing::info!(card = %card.id, seen = cards_seen(), "bar: card");
    {
        let mut s = lock();
        s.cards.retain(|c| c.id != card.id);
        s.handlers.insert(card.id.clone(), on_action);
        s.cards.push(card);
    }
    after_cards_changed(app);
    on_main(app, |app| {
        if let Some(w) = window(app) {
            overlay::force_topmost(&w);
        }
    });
    true
}

/// Change a card that's up. `false` if it isn't.
pub fn update_card(app: &AppHandle, id: &str, f: impl FnOnce(&mut Card)) -> bool {
    let found = {
        let mut s = lock();
        match s.cards.iter_mut().find(|c| c.id == id) {
            Some(card) => {
                f(card);
                true
            }
            None => false,
        }
    };
    if found {
        after_cards_changed(app);
    }
    found
}

/// Take the card `id` down (no answer is reported).
pub fn dismiss_card(app: &AppHandle, id: &str) {
    let removed = {
        let mut s = lock();
        let before = s.cards.len();
        s.cards.retain(|c| c.id != id);
        s.handlers.remove(id);
        s.cards.len() != before
    };
    if removed {
        after_cards_changed(app);
    }
}

fn after_cards_changed(app: &AppHandle) {
    // A card can bring the window up (over a borderless fullscreen app) or
    // its going take it down again.
    apply(app);
    emit(app);
    wake();
}

/// Watch Esc only while a card that offers it is on screen (never while it
/// waits out a game in exclusive fullscreen, where Esc is the game's).
fn sync_escape() {
    let wanted = {
        let s = lock();
        s.shown && cards_seen() && s.cards.iter().any(|c| c.escape_action.is_some())
    };
    #[cfg(windows)]
    if wanted && !crate::e2e::active() {
        escape_watch::start(on_escape_key);
    } else {
        escape_watch::stop();
    }
    #[cfg(not(windows))]
    let _ = wanted;
}

/// A card's button (or its timeout, ✕ or Esc): the card goes, then its
/// handler gets `action` (none for an empty one). Blocking handlers are
/// fine: this runs off the main thread.
fn card_action(app: &AppHandle, id: &str, action: &str) {
    let handler = {
        let mut s = lock();
        let before = s.cards.len();
        s.cards.retain(|c| c.id != id);
        let handler = s.handlers.remove(id);
        (s.cards.len() != before).then_some(handler).flatten()
    };
    after_cards_changed(app);
    if let (Some(handler), false) = (handler, action.is_empty()) {
        handler(app, action);
    }
}

// ---- actions -----------------------------------------------------------------------

/// "New meeting note" (the menu): a meeting note to prepare, not recording
/// yet, opened in the main window's Notes view.
fn new_meeting_note(app: &AppHandle) -> Result<(), String> {
    crate::notes::folder_create("Meetings");
    let title = crate::meeting_detect::meeting_note_title();
    let note = crate::notes::create(&title, "", "meeting", "Meetings");
    crate::notes::mark_meeting(note.id)?;
    let _ = app.emit("yap-notes-changed", ());
    crate::commands::show_settings(app)?;
    let _ = app.emit("yap-meeting-open-note", serde_json::json!({ "noteId": note.id, "stop": false }));
    Ok(())
}

/// What the bar's buttons and menu do (`bar_action`).
fn act(app: &AppHandle, action: &str) -> Result<(), String> {
    match action {
        // 🎤: the focus first goes back to the app you're in (should the
        // click have activated the bar), so the dictation pastes there.
        "dictate" => {
            give_focus_back();
            let state = app.state::<crate::AppState>();
            let guard = state.pipeline.lock().map_err(|_| "pipeline unavailable")?;
            guard.as_ref().ok_or("pipeline not started")?.toggle();
        }
        // ◉: what the meeting shortcut does — take notes on the call Yap
        // noticed, else in a new meeting note; while one records, stop it
        // and write the action plan.
        "meeting" => {
            give_focus_back();
            crate::meeting_guard::start_or_stop(app);
        }
        // ■ on the recording pill: end the meeting, which writes the action
        // plan in Rust (or asks "Started by mistake?").
        "stop-meeting" => crate::meeting_end::end(app, Some(overlay::LABEL))?,
        // The recording pill: its notes, in the meeting notepad.
        "open-note" => match crate::meeting::recording_note() {
            Some(note_id) => crate::notepad::open(app, note_id),
            None => crate::commands::show_settings(app)?,
        },
        "new-note" => new_meeting_note(app)?,
        "open" => crate::commands::show_settings(app)?,
        "settings" => {
            crate::commands::show_settings(app)?;
            let _ = app.emit("yap-open-settings", "general#bar");
        }
        "hide" => {
            lock().hidden_until = now_ms() + HIDE_FOR_MS;
            tracing::info!("bar: hidden for an hour");
            apply(app);
            emit(app);
        }
        "show" => {
            lock().hidden_until = 0;
            apply(app);
            emit(app);
        }
        "off" => {
            let mut cfg = crate::config::load();
            cfg.bar_enabled = false;
            crate::config::save(&cfg)?;
            sync(app, &cfg);
            let _ = app.emit(EVENT_CHANGED, serde_json::json!({ "enabled": false }));
            tracing::info!("bar: turned off from its menu");
        }
        other => return Err(format!("Unknown bar action: {other}")),
    }
    wake();
    Ok(())
}

// ---- commands -----------------------------------------------------------------------

/// The bar's snapshot (see [`Status`]).
#[tauri::command]
pub fn bar_status() -> Status {
    status()
}

/// The page's interactive rects (CSS px from the window's top-left): the
/// pill's hover zone, cards, the menu. Sent on every layout change.
#[tauri::command]
pub fn bar_regions(regions: Vec<Region>) {
    let mut s = lock();
    if s.regions != regions {
        s.regions = regions;
        drop(s);
        wake();
    }
}

/// The page saw the pointer leave every region while the window was
/// clickable (it was over a transparent part): click-through again now,
/// rather than at the next poll.
#[tauri::command]
pub fn bar_pointer_left(app: AppHandle) {
    let was = {
        let mut s = lock();
        let was = s.interactive;
        s.interactive = false;
        s.pointer = None;
        was
    };
    if was {
        set_click_through(&app, true);
        let _ = app.emit(EVENT_POINTER, serde_json::json!({ "region": null }));
    }
}

/// A bar button or menu item: "dictate" | "meeting" | "stop-meeting" |
/// "open-note" | "new-note" | "open" | "settings" | "hide" | "show" | "off".
#[tauri::command]
pub async fn bar_action(app: AppHandle, action: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || act(&app, &action))
        .await
        .map_err(|e| e.to_string())?
}

/// A card's button, ✕, timeout or countdown: `action` ("" for none).
#[tauri::command]
pub async fn bar_card_action(app: AppHandle, id: String, action: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        card_action(&app, &id, &action);
        give_focus_back();
    })
    .await
    .map_err(|e| e.to_string())
}

/// Debug builds only (the e2e suite): pretend the cursor is on a region
/// (`pointer`: a region id, or "away"), that an app is fullscreen
/// (`fullscreen`: "none" | "borderless" | "exclusive"), that Esc was
/// pressed, or put up a demo card (`card`: "call" | "update" | "info").
#[tauri::command]
pub async fn bar_simulate(
    app: AppHandle,
    pointer: Option<String>,
    fullscreen: Option<String>,
    escape: Option<bool>,
    card: Option<String>,
) -> Result<(), String> {
    if !cfg!(debug_assertions) || !crate::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    if let Some(target) = pointer {
        let point = simulated_point(&app, &target)?;
        lock().sim_cursor = point;
    }
    if let Some(kind) = fullscreen {
        lock().fullscreen = Fullscreen::from_name(&kind);
        apply(&app);
        emit(&app);
    }
    if escape == Some(true) {
        ESCAPED.store(true, Ordering::Relaxed);
    }
    if let Some(kind) = card {
        let shown = show_card(&app, demo_card(&kind), Box::new(|app, action| {
            tracing::info!(action, "bar: demo card answered");
            let _ = app.emit("yap-bar-demo-answer", action);
        }));
        if !shown {
            return Err("The bar can't take cards right now".to_string());
        }
    }
    wake();
    Ok(())
}

/// A screen point on region `target` (its centre), or well away from the
/// bar for "away".
fn simulated_point(app: &AppHandle, target: &str) -> Result<Option<(i32, i32)>, String> {
    #[cfg(windows)]
    {
        let hwnd = window(app).and_then(|w| w.hwnd().ok()).ok_or("no bar window")?.0;
        let rect = overlay::win::window_rect(hwnd).ok_or("no bar rect")?;
        if target == "away" {
            return Ok(Some((rect.left - 400, rect.top - 400)));
        }
        let scale = overlay::win::window_scale(hwnd);
        let s = lock();
        let r = s.regions.iter().find(|r| r.id == target).ok_or_else(|| format!("no region {target}"))?;
        let x = rect.left + ((r.x + r.w / 2.0) * scale) as i32;
        let y = rect.top + ((r.y + r.h / 2.0) * scale) as i32;
        Ok(Some((x, y)))
    }
    #[cfg(not(windows))]
    {
        let _ = (app, target);
        Err("Windows only".to_string())
    }
}

fn demo_card(kind: &str) -> Card {
    match kind {
        "call" => Card {
            id: "demo".into(),
            style: "call",
            icon: "call",
            app: Some("discord"),
            title: "Discord call detected".into(),
            status: Some("Now".into()),
            dot: "live",
            primary: Some(CardAction::new("record", "Record notes")),
            secondary: Some(CardAction::new("dismiss", "Not now")),
            link: Some(CardAction::new("never", "Don't ask for Discord")),
            close_action: Some("dismiss".into()),
            ..Card::default()
        },
        "update" => Card {
            id: "demo".into(),
            icon: "update",
            title: "Yap 0.2.0 is ready".into(),
            body: "Restart Yap to finish updating. It only takes a few seconds.".into(),
            primary: Some(CardAction::new("install", "Restart to update")),
            secondary: Some(CardAction::new("", "Later")),
            ..Card::default()
        },
        _ => Card {
            id: "demo".into(),
            icon: "notes",
            title: "Taking notes on your Teams call".into(),
            body: "Let people know you're taking notes. Yap offers to stop and summarise when it ends.".into(),
            primary: Some(CardAction::new("open", "Open note")),
            timeout_ms: Some(8_000),
            ..Card::default()
        },
    }
}

/// Debug builds only: the bar window as Windows sees it — its real
/// ex-styles, where it is, and what a click on the pill would reach.
#[tauri::command]
pub fn bar_debug(app: AppHandle) -> Result<serde_json::Value, String> {
    if !cfg!(debug_assertions) {
        return Err("Only in debug builds".to_string());
    }
    #[cfg(windows)]
    {
        use overlay::*;
        let hwnd = window(&app).and_then(|w| w.hwnd().ok()).ok_or("no bar window")?.0;
        let style = win::ex_style(hwnd);
        let rect = win::window_rect(hwnd).ok_or("no bar rect")?;
        let scale = win::window_scale(hwnd);
        let s = lock();
        let pill = s.regions.iter().find(|r| r.id == "pill").map(|r| {
            (
                rect.left + ((r.x + r.w / 2.0) * scale) as i32,
                rect.top + ((r.y + r.h / 2.0) * scale) as i32,
            )
        });
        let reaches_bar = pill.map(|p| win::window_at(p) == hwnd as isize);
        drop(s);
        // Windows' own activation record (tao follows WM_ACTIVATE): the bar
        // must never be the active window; the main window only when it
        // was really activated.
        let bar_active = window(&app).is_some_and(|w| w.is_focused().unwrap_or(false));
        let main_active = app
            .get_webview_window("settings")
            .is_some_and(|w| w.is_focused().unwrap_or(false));
        let thread_active = win::thread_active(hwnd);
        let s = lock();
        Ok(serde_json::json!({
            "barActive": bar_active,
            "mainActive": main_active,
            // The active window of Yap's UI thread (foreground or not).
            "threadActiveIsBar": thread_active == hwnd as isize,
            "threadActive": thread_active,
            "hwnd": hwnd as isize,
            "pid": std::process::id(),
            "shown": s.shown,
            "interactive": s.interactive,
            "pointer": s.pointer,
            "exStyle": {
                "transparent": style & WS_EX_TRANSPARENT != 0,
                "noActivate": style & WS_EX_NOACTIVATE != 0,
                "toolWindow": style & WS_EX_TOOLWINDOW != 0,
                "layered": style & WS_EX_LAYERED != 0,
                "topmost": style & WS_EX_TOPMOST != 0,
                "appWindow": style & WS_EX_APPWINDOW != 0,
            },
            "rect": rect,
            "scale": scale,
            "screen": s.placed.map(|p| serde_json::json!({ "monitor": p.monitor, "work": p.work })),
            "regions": s.regions,
            "pillPoint": pill,
            "clickOnPillReachesBar": reaches_bar,
            "barInFront": win::bar_in_front(),
        }))
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        Err("Windows only".to_string())
    }
}

// ---- Esc -------------------------------------------------------------------------------

/// Esc for a card that offers it (a countdown's "cancel"): the bar never has
/// the keyboard focus, so while such a card is up Yap watches the key with a
/// low-level keyboard hook of its own. It only looks (Esc still reaches the
/// app you're in), and it's gone as soon as the card is.
#[cfg(windows)]
mod escape_watch {
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::OnceLock;

    #[repr(C)]
    struct KbdLlHook {
        vk_code: u32,
        scan_code: u32,
        flags: u32,
        time: u32,
        extra: usize,
    }

    #[repr(C)]
    struct Point {
        x: i32,
        y: i32,
    }

    /// Laid out as `input_hook`'s MSG (one `GetMessageW` declaration).
    #[repr(C)]
    struct Msg {
        hwnd: isize,
        message: u32,
        wparam: usize,
        lparam: isize,
        time: u32,
        pt: Point,
    }

    type HookProc = unsafe extern "system" fn(i32, usize, isize) -> isize;

    #[link(name = "user32")]
    extern "system" {
        fn SetWindowsHookExW(id: i32, proc_: HookProc, module: isize, thread: u32) -> isize;
        fn CallNextHookEx(hook: isize, code: i32, wparam: usize, lparam: isize) -> isize;
        fn UnhookWindowsHookEx(hook: isize) -> i32;
        fn GetMessageW(msg: *mut Msg, hwnd: isize, min: u32, max: u32) -> i32;
        fn PostThreadMessageW(thread: u32, msg: u32, wparam: usize, lparam: isize) -> i32;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentThreadId() -> u32;
        fn GetModuleHandleW(name: *const u16) -> isize;
    }

    const WH_KEYBOARD_LL: i32 = 13;
    const WM_KEYDOWN: usize = 0x0100;
    const WM_SYSKEYDOWN: usize = 0x0104;
    const WM_QUIT: u32 = 0x0012;
    const VK_ESCAPE: u32 = 0x1B;

    static WANTED: AtomicBool = AtomicBool::new(false);
    static RUNNING: AtomicBool = AtomicBool::new(false);
    static THREAD: AtomicU32 = AtomicU32::new(0);
    static ON_ESC: OnceLock<fn()> = OnceLock::new();

    unsafe extern "system" fn hook(code: i32, wparam: usize, lparam: isize) -> isize {
        if code >= 0 && (wparam == WM_KEYDOWN || wparam == WM_SYSKEYDOWN) {
            let info = &*(lparam as *const KbdLlHook);
            if info.vk_code == VK_ESCAPE {
                if let Some(f) = ON_ESC.get() {
                    f();
                }
            }
        }
        CallNextHookEx(0, code, wparam, lparam)
    }

    pub fn start(on_esc: fn()) {
        let _ = ON_ESC.set(on_esc);
        WANTED.store(true, Ordering::SeqCst);
        if RUNNING.swap(true, Ordering::SeqCst) {
            return;
        }
        let spawned = std::thread::Builder::new().name("yap-bar-esc".into()).spawn(|| {
            unsafe {
                THREAD.store(GetCurrentThreadId(), Ordering::SeqCst);
                let handle = if WANTED.load(Ordering::SeqCst) {
                    SetWindowsHookExW(WH_KEYBOARD_LL, hook, GetModuleHandleW(std::ptr::null()), 0)
                } else {
                    0
                };
                if handle != 0 {
                    tracing::info!("bar: watching Esc");
                    // The hook runs inside this thread's message wait; a
                    // WM_QUIT from `stop` ends it.
                    let mut msg: Msg = std::mem::zeroed();
                    loop {
                        if !WANTED.load(Ordering::SeqCst) || GetMessageW(&mut msg, 0, 0, 0) <= 0 {
                            break;
                        }
                    }
                    UnhookWindowsHookEx(handle);
                }
                THREAD.store(0, Ordering::SeqCst);
            }
            RUNNING.store(false, Ordering::SeqCst);
            // Wanted again while this one was winding down.
            if WANTED.load(Ordering::SeqCst) {
                if let Some(f) = ON_ESC.get() {
                    start(*f);
                }
            }
        });
        if spawned.is_err() {
            RUNNING.store(false, Ordering::SeqCst);
        }
    }

    pub fn stop() {
        if !WANTED.swap(false, Ordering::SeqCst) {
            return;
        }
        let thread = THREAD.load(Ordering::SeqCst);
        if thread != 0 {
            unsafe {
                PostThreadMessageW(thread, WM_QUIT, 0, 0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(id: isize, left: i32) -> Screen {
        Screen {
            id,
            monitor: Rect::new(left, 0, left + 2560, 1440),
            work: Rect::new(left, 0, left + 2560, 1392),
            scale: 1.0,
            autohide_bottom: 0,
            autohide_top: 0,
        }
    }

    #[test]
    fn goes_to_the_cursor_first() {
        let mut pending = None;
        // Never placed: straight to the cursor's monitor.
        assert_eq!(follow(None, None, Some(screen(2, 2560)), &mut pending, false, 0), Some(screen(2, 2560)));
        // Placed, cursor on the same monitor (anywhere on it): stays put.
        let a = screen(1, 0);
        assert_eq!(follow(Some(&a), Some(a), Some(a), &mut pending, false, 10), None);
        assert_eq!(pending, None);
    }

    #[test]
    fn follows_the_cursor_to_another_monitor_after_a_moment() {
        let (a, b) = (screen(1, 0), screen(2, 2560));
        let mut pending = None;
        assert_eq!(follow(Some(&a), Some(a), Some(b), &mut pending, false, 1_000), None);
        assert_eq!(pending, Some(Pending { id: 2, since: 1_000 }));
        assert_eq!(follow(Some(&a), Some(a), Some(b), &mut pending, false, 1_100), None);
        assert_eq!(follow(Some(&a), Some(a), Some(b), &mut pending, false, 1_200), Some(b));
        assert_eq!(pending, None);
    }

    #[test]
    fn a_flick_across_the_edge_doesnt_drag_it_along() {
        let (a, b) = (screen(1, 0), screen(2, 2560));
        let mut pending = None;
        assert_eq!(follow(Some(&a), Some(a), Some(b), &mut pending, false, 0), None);
        // Back on its own monitor before 200 ms: the pending move is dropped…
        assert_eq!(follow(Some(&a), Some(a), Some(a), &mut pending, false, 150), None);
        assert_eq!(pending, None);
        // …so crossing again starts a fresh wait.
        assert_eq!(follow(Some(&a), Some(a), Some(b), &mut pending, false, 300), None);
        assert_eq!(follow(Some(&a), Some(a), Some(b), &mut pending, false, 450), None);
        assert_eq!(follow(Some(&a), Some(a), Some(b), &mut pending, false, 500), Some(b));
    }

    #[test]
    fn stays_where_a_dictation_started() {
        let (a, b) = (screen(1, 0), screen(2, 2560));
        let mut pending = None;
        for t in [0, 300, 5_000] {
            assert_eq!(follow(Some(&a), Some(a), Some(b), &mut pending, true, t), None);
        }
        assert_eq!(pending, None);
        // Once it's over, it follows again (after the usual moment).
        assert_eq!(follow(Some(&a), Some(a), Some(b), &mut pending, false, 6_000), None);
        assert_eq!(follow(Some(&a), Some(a), Some(b), &mut pending, false, 6_250), Some(b));
    }

    #[test]
    fn its_monitor_going_away_moves_it_at_once() {
        let b = screen(2, 2560);
        let mut pending = Some(Pending { id: 3, since: 0 });
        // Unplugged (even mid-dictation): to the cursor's monitor now.
        assert_eq!(follow(Some(&screen(1, 0)), None, Some(b), &mut pending, true, 10), Some(b));
        assert_eq!(pending, None);
    }

    #[test]
    fn its_own_monitor_changing_re_places_it() {
        let a = screen(1, 0);
        let mut moved_taskbar = a;
        moved_taskbar.work = Rect::new(0, 0, 2560, 1380);
        let mut pending = None;
        assert_eq!(follow(Some(&a), Some(moved_taskbar), Some(moved_taskbar), &mut pending, false, 0), Some(moved_taskbar));
        // Its scale changing (display settings) too, even mid-dictation.
        let mut scaled = a;
        scaled.scale = 1.25;
        assert_eq!(follow(Some(&a), Some(scaled), Some(screen(2, 2560)), &mut pending, true, 0), Some(scaled));
    }

    fn vis() -> Visibility {
        Visibility { enabled: true, ..Visibility::default() }
    }

    #[test]
    fn shows_when_it_should() {
        // On and idle: shown, and takes cards, seen at once.
        assert!(shows(vis()));
        assert!(takes_cards(vis()) && seen_now(vis()));
        // Hidden for an hour: hidden, no cards (they go to Windows)…
        let hidden = Visibility { hidden: true, ..vis() };
        assert!(!shows(hidden));
        assert!(!takes_cards(hidden));
        assert!(!shows(Visibility { cards: true, ..hidden }));
        // …over any fullscreen app the idle pill hides…
        for fullscreen in [Fullscreen::Borderless, Fullscreen::Exclusive] {
            let v = Visibility { fullscreen, ..vis() };
            assert!(!shows(v), "{fullscreen:?}");
            // …but always while recording: a dictation or a meeting.
            for v in [Visibility { hidden: true, ..v }, v] {
                assert!(shows(Visibility { dictation: Dictation::Recording, ..v }));
                assert!(shows(Visibility { meeting: true, ..v }));
            }
        }
        // Off: only while a dictation shows (today's overlay), never cards,
        // not for a meeting.
        let off = Visibility { enabled: false, ..vis() };
        assert!(!shows(off));
        assert!(!takes_cards(off));
        assert!(!shows(Visibility { meeting: true, ..off }));
        assert!(!shows(Visibility { cards: true, ..off }));
        for d in [Dictation::Recording, Dictation::Processing, Dictation::Error] {
            assert!(shows(Visibility { dictation: d, ..off }));
        }
    }

    #[test]
    fn cards_over_fullscreen_apps() {
        // Over a borderless game or video: a card shows (the window comes up
        // for it, without the idle pill), and goes with it.
        let borderless = Visibility { fullscreen: Fullscreen::Borderless, ..vis() };
        assert!(takes_cards(borderless) && seen_now(borderless));
        assert!(shows(Visibility { cards: true, ..borderless }));
        assert!(!shows(borderless));
        // Over exclusive fullscreen (or a slideshow): taken, but it waits,
        // unseen, until that's over (no countdown can run meanwhile).
        let exclusive = Visibility { fullscreen: Fullscreen::Exclusive, cards: true, ..vis() };
        assert!(takes_cards(exclusive));
        assert!(!seen_now(exclusive));
        assert!(!shows(exclusive));
        assert!(shows(Visibility { fullscreen: Fullscreen::None, ..exclusive }));
    }

    #[test]
    fn takes_clicks_only_on_a_region_of_a_bar_that_is_on() {
        assert!(clickable(vis(), true, true));
        assert!(!clickable(vis(), true, false), "off the regions: click-through");
        assert!(!clickable(vis(), false, true), "hidden");
        // The bar off (the overlay showing for a dictation): never.
        assert!(!clickable(Visibility { enabled: false, ..vis() }, true, true));
    }

    #[test]
    fn dictation_states() {
        assert_eq!(Dictation::from_state("recording"), Dictation::Recording);
        assert_eq!(Dictation::from_state("processing-slow"), Dictation::Processing);
        assert_eq!(Dictation::from_state("error"), Dictation::Error);
        assert_eq!(Dictation::from_state("needs-model"), Dictation::Idle);
        assert_eq!(Dictation::from_state("idle"), Dictation::Idle);
    }

    #[test]
    fn card_shape_for_the_page() {
        let card = Card {
            id: "call".into(),
            icon: "call",
            title: "Teams call detected".into(),
            primary: Some(CardAction::new("meeting:record:3", "Record notes")),
            countdown: Some(Countdown { until: 99, label: "Starting notes in".into() }),
            timeout_ms: Some(30_000),
            ..Card::default()
        };
        let v = serde_json::to_value(&card).unwrap();
        assert_eq!(v["primary"]["id"], "meeting:record:3");
        assert_eq!(v["timeoutMs"], 30_000);
        assert_eq!(v["countdown"]["until"], 99);
        assert!(v["secondary"].is_null());
    }
}
