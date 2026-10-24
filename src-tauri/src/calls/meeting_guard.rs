//! Guard rails around a meeting recording (ROADMAP Phase 8, after Wispr
//! Flow's Notetaker settings), and the notices they and `capture.rs` show.
//!
//! - **Maximum recording length** (`meeting_max_minutes`: 2 hours by
//!   default, as Wispr; 0 = no limit). Five minutes before it: "Notes stop
//!   in 5 minutes" with **Keep going** (another hour), in one place: an
//!   in-app toast while the main window is focused, else a Yap bar card,
//!   else a Windows notification. At the
//!   limit the recording stops and the action plan is written. A warning
//!   always comes at least 5 minutes before the stop, also when the limit is
//!   lowered mid-meeting or the PC slept through it; raising or removing the
//!   limit takes a warning back. The limit is per recording (a Resume starts
//!   its own) and read from Settings every second, so a change applies at
//!   once.
//! - **The meeting shortcut** (`meeting_hotkey`, Win+Alt+M as Wispr):
//!   [`start_or_stop`]. While recording, it stops and writes the action
//!   plan; otherwise it takes notes on the live call (call detection's
//!   "Record notes"), or else in a new meeting note ("Meeting · 5 Oct,
//!   14:30", folder Meetings). The global hook emits `meeting-key-pressed`;
//!   Yap's own windows catch the keys in-page (WebView2 front-runs the hook
//!   while one has focus) and invoke `meeting_shortcut`.
//! - **When a call ends** (`meeting_call_end`: "ask" | "stop"): decided in
//!   `meeting_detect.rs`; [`stop_after_call`] does the stopping.
//! - **Quiet meetings** (`meeting_end_when_quiet`, on by default, as Wispr):
//!   after 10 minutes with nobody speaking on either side, "Ending notes in
//!   30 s…" counts down with **Keep going** and **Stop now**; anyone
//!   speaking cancels it, and at zero the recording stops and the action
//!   plan is written, with a notice that says why. "Speech" is what the
//!   recorder counts as talk (`meeting::activity`): audio above its silence
//!   gate on either side, or a transcribed line; a hotkey dictation's span
//!   isn't. Quiet time runs on the meeting's audio clock, so a PC asleep or
//!   a device that stopped delivering never counts as a quiet meeting.
//! - **Check-ins**: "Still in your meeting?" (Stop and summarise / Keep
//!   recording) 10 minutes after the end of the calendar meeting the note
//!   belongs to (`Note::event`), and an hour into any recording. Each is
//!   held back while anyone spoke in the last 3 minutes, never on top of
//!   the length warning (or within 10 minutes of it), asked once per
//!   recording, and goes away by itself after 2 minutes unanswered (it only
//!   asks).
//! - [`notice`]: one-off messages for these, the screen-share warning and
//!   the recorder's own warnings, in one place (`notice::place`): the main
//!   window's toast (`yap-meeting-notice`) while it's focused, else a Yap
//!   bar card or a Windows notification ([`notice_native`]), else the
//!   window if it's on screen; not twice in ten minutes (`notice.rs`). The
//!   length warning goes the same way.
//!
//! The countdown and the check-ins show where the person is looking: the
//! meeting notepad if it's on screen, else the main window if it's focused
//! (both as a toast, `yap-meeting-checkin`), else a Yap bar card, else a
//! Windows notification. The rules are pure ([`QuietWatch::step`],
//! [`pick_surface`]) and unit-tested.
//!
//! Every stop goes through [`stop_and_summarise`]. In test mode the
//! debug-only `e2e_meeting_limit` shortens the length timings and
//! `e2e_meeting_checkins` the quiet ones.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Listener, Manager};

/// Warn this long before the length limit…
const WARN_BEFORE_MS: u64 = 5 * 60_000;
/// …and "Keep going" moves it this much.
const KEEP_GOING_MS: u64 = 60 * 60_000;
/// How often the length guard looks at the clock…
const TICK: Duration = Duration::from_secs(1);
/// …and with the e2e suite's short timings.
const TEST_TICK: Duration = Duration::from_millis(100);
/// Presses of the meeting shortcut closer together than this are one press
/// (the global hook and a window's in-page fallback can't both start and
/// then stop a meeting).
const SHORTCUT_DEBOUNCE_MS: u64 = 1_000;

/// The length warning: `{ warning: LimitWarning | null }`.
const EVENT_LIMIT: &str = "yap-meeting-limit";
/// A one-off [`Notice`].
const EVENT_NOTICE: &str = "yap-meeting-notice";
/// The quiet countdown or a check-in: `{ prompt: CheckIn | null }`.
const EVENT_CHECKIN: &str = "yap-meeting-checkin";
/// Open a note in the main window's Notes view: `{ noteId, stop }` (the
/// event `meeting_detect.rs` uses for the same).
const EVENT_OPEN: &str = "yap-meeting-open-note";
/// Settings → Meetings, at "Hide Yap's meeting windows from
/// screen sharing" (`yap-settings-goto`: a section, then the card's id).
const SCREEN_SHARE_SETTINGS: &str = "meetings#screen-sharing";

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// The main window: (on screen, focused) — `notice::window_view`. Never
/// call with a lock held: window getters wait on the main thread.
fn window_view(app: &AppHandle) -> (bool, bool) {
    crate::notices::notice::window_view(app)
}

/// The meeting notepad is on screen. Never with a lock held.
fn notepad_on_screen(app: &AppHandle) -> bool {
    app.get_webview_window(crate::notepad::LABEL)
        .is_some_and(|w| w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false))
}

/// "5 minutes", "1 hour", "1 hour and 30 minutes", "10 seconds" (rounded).
fn duration_words(ms: u64) -> String {
    let plural = |n: u64, unit: &str| format!("{n} {unit}{}", if n == 1 { "" } else { "s" });
    let secs = (ms + 500) / 1000;
    if secs < 60 {
        return plural(secs.max(1), "second");
    }
    let minutes = (secs + 30) / 60;
    match (minutes / 60, minutes % 60) {
        (0, m) => plural(m, "minute"),
        (h, 0) => plural(h, "hour"),
        (h, m) => format!("{} and {}", plural(h, "hour"), plural(m, "minute")),
    }
}

// ---- notices ----------------------------------------------------------------------------

/// A one-off message about a meeting (`yap-meeting-notice`): the main
/// window shows it as a toast; [`notice_native`] posts it as a Windows
/// notification.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Notice {
    /// "screenShare" | "stopped" | "recording".
    pub kind: &'static str,
    pub title: String,
    pub body: String,
    /// The toast chip's icon: "screen" | "timer" | "call".
    pub icon: &'static str,
    /// "default" | "success".
    pub variant: &'static str,
    /// A Settings section for an **Update settings** button.
    pub settings: Option<&'static str>,
    /// The meeting note (a Windows notification's **Open note**).
    pub note_id: Option<u64>,
    /// The one window that shows it ("notepad" | "settings"); `None`: the
    /// main window, as every notice before the quiet-meeting one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surface: Option<&'static str>,
}

impl Notice {
    /// Wispr's screen-share tip, when hiding is off and a meeting starts.
    pub fn screen_share() -> Self {
        Notice {
            kind: "screenShare",
            title: "Screen sharing".to_string(),
            body: "Your meeting notes show up in screen shares and screenshots.".to_string(),
            icon: "screen",
            variant: "default",
            settings: Some(SCREEN_SHARE_SETTINGS),
            note_id: None,
            surface: None,
        }
    }

    /// The recording reached its maximum length, `elapsed_ms` in.
    fn stopped_at_limit(elapsed_ms: u64) -> Self {
        Notice {
            kind: "stopped",
            title: format!("Stopped at {}", duration_words(elapsed_ms)),
            body: "The meeting reached the maximum recording length. Yap is writing your action plan."
                .to_string(),
            icon: "timer",
            variant: "default",
            settings: None,
            note_id: None,
            surface: None,
        }
    }

    /// The quiet countdown ran out: nobody had spoken for `quiet_ms`.
    fn stopped_quiet(quiet_ms: u64, note_id: u64) -> Self {
        Notice {
            kind: "stopped",
            title: "Stopped: the meeting went quiet".to_string(),
            body: format!(
                "Nobody had spoken for {}, so Yap stopped recording and is writing your action plan.",
                duration_words(quiet_ms)
            ),
            icon: "timer",
            variant: "default",
            settings: None,
            note_id: Some(note_id),
            surface: None,
        }
    }

    /// "When a call ends: Stop and summarise automatically" (`call`: "Teams
    /// call").
    fn call_ended(call: &str) -> Self {
        Notice {
            kind: "stopped",
            title: format!("{call} ended"),
            body: "Yap stopped recording and is writing your action plan.".to_string(),
            icon: "call",
            variant: "default",
            settings: None,
            note_id: None,
            surface: None,
        }
    }

    /// The meeting shortcut started notes (`call`: "Teams call", if any).
    fn taking_notes(call: Option<&str>, shortcut: Option<&str>, note_id: u64) -> Self {
        let again = shortcut.map_or_else(
            || "Use the meeting shortcut again to stop and write the action plan.".to_string(),
            |s| format!("Press {s} again to stop and write the action plan."),
        );
        Notice {
            kind: "recording",
            title: call.map_or_else(|| "Taking notes".to_string(), |c| format!("Taking notes on your {c}")),
            body: format!("Your mic is \"You\", the call is \"Them\". {again}"),
            icon: "call",
            variant: "success",
            settings: None,
            note_id: Some(note_id),
            surface: None,
        }
    }
}

/// Show `n` in one place (`notice::place`): the main window's toast while
/// the person is looking at it, else a card on the Yap bar, else a Windows
/// notification, else the main window if it's on screen after all. A notice
/// addressed to a window (`surface`) shows there. The same notice again
/// within ten minutes isn't shown (`notice.rs`).
pub fn notice(app: &AppHandle, n: &Notice) {
    if n.surface.is_some() {
        let _ = app.emit(EVENT_NOTICE, n);
        return;
    }
    match crate::notices::notice::place(app, true) {
        crate::notices::notice::Place::Window => {
            if crate::notices::notice::admit("meeting-notice", &n.title, &n.body) {
                let _ = app.emit(EVENT_NOTICE, n);
            }
        }
        crate::notices::notice::Place::Bar | crate::notices::notice::Place::Notification => notice_native(app, n),
        crate::notices::notice::Place::Nowhere => tracing::info!(kind = n.kind, "meeting guard: nowhere to say {}", n.title),
    }
}

/// Put `n` on the Yap bar, else post it as a Windows notification (not in
/// portable mode or test runs).
pub fn notice_native(app: &AppHandle, n: &Notice) {
    #[cfg(windows)]
    if let Err(e) = native::notice(app, n) {
        tracing::info!("meeting guard: no Windows notification ({e})");
    }
    #[cfg(not(windows))]
    let _ = (app, n);
}

// ---- stopping -------------------------------------------------------------------------

/// Stop the recording and write the action plan: the length limit, a call
/// ending with "Stop and summarise automatically", and the meeting shortcut
/// all come here. It ends the meeting as **End meeting & summarise** does
/// (`meeting_end::end`: Rust writes the action plan once the last chunk is
/// in, or asks "Started by mistake?"), without bringing up any window: the
/// meeting notepad shows the progress and the result when it's open, and
/// the note has them either way. (`note_id`: only if that note is the one
/// recording.)
pub fn stop_and_summarise(app: &AppHandle, note_id: Option<u64>) {
    stop_from(app, note_id, None);
}

/// [`stop_and_summarise`], asked for in window `origin` (the meeting
/// shortcut caught by a window's in-page fallback): a question about the
/// meeting ("Started by mistake?") shows there.
fn stop_from(app: &AppHandle, note_id: Option<u64>, origin: Option<&str>) {
    if note_id.is_some() && crate::meeting::recording_note() != note_id {
        return;
    }
    if let Err(e) = crate::meeting_end::end(app, origin) {
        tracing::info!("meeting guard: nothing to stop ({e})");
    }
}

/// "When a call ends: Stop and summarise automatically": the call (`call`:
/// "Teams call") that Yap was recording into `note_id` ended.
pub fn stop_after_call(app: &AppHandle, note_id: u64, call: &str) {
    tracing::info!(note_id, call, "meeting guard: the call ended; stopping automatically");
    stop_and_summarise(app, Some(note_id));
    notice(app, &Notice::call_ended(call));
}

// ---- maximum recording length --------------------------------------------------------------

/// The length guard's timings (ms).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Timings {
    /// `None`: no limit.
    limit: Option<u64>,
    warn_before: u64,
    keep_going: u64,
}

impl Timings {
    /// From Settings' `meeting_max_minutes` (0 = no limit).
    fn from_minutes(minutes: u32) -> Self {
        Timings {
            limit: (minutes > 0).then(|| u64::from(minutes) * 60_000),
            warn_before: WARN_BEFORE_MS,
            keep_going: KEEP_GOING_MS,
        }
    }
}

/// Short timings for the e2e suite (`e2e_meeting_limit`, debug builds).
static TEST_TIMINGS: Mutex<Option<Timings>> = Mutex::new(None);

fn test_timings() -> Option<Timings> {
    *TEST_TIMINGS.lock().unwrap_or_else(|p| p.into_inner())
}

fn timings() -> Timings {
    test_timings().unwrap_or_else(|| Timings::from_minutes(crate::config::load().meeting_max_minutes))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Wait,
    /// Warn that the recording stops at `stop_at` (unix ms).
    Warn { stop_at: u64 },
    /// Take the warning back: the limit was raised or removed.
    Withdraw,
    /// The limit: stop and write the action plan.
    Stop,
}

/// What the length guard does at `now` (unix ms) for a recording that
/// started at `started`, with `extra` added by "Keep going", having
/// announced a stop at `warned` (or not yet).
fn step(now: u64, started: u64, extra: u64, warned: Option<u64>, t: &Timings) -> Step {
    let Some(limit) = t.limit else {
        return if warned.is_some() { Step::Withdraw } else { Step::Wait };
    };
    let deadline = started + limit + extra;
    match warned {
        // Never less warning than `warn_before`, even when it comes late.
        None if now + t.warn_before >= deadline => Step::Warn { stop_at: deadline.max(now + t.warn_before) },
        None => Step::Wait,
        Some(stop_at) if now >= stop_at => Step::Stop,
        Some(_) if now + t.warn_before < deadline => Step::Withdraw,
        Some(_) => Step::Wait,
    }
}

/// "Keep going" at `now`: the new `extra`, which puts the stop
/// `keep_going` after the current limit (or after now, if that's later: a
/// warning that came late, say after the limit was lowered).
fn kept_going(now: u64, started: u64, extra: u64, t: &Timings) -> u64 {
    let Some(limit) = t.limit else { return extra };
    let deadline = (started + limit + extra).max(now);
    deadline + t.keep_going - started - limit
}

/// The recording the guard rails watch.
struct Watch {
    note_id: u64,
    started: u64,
    /// Added by "Keep going".
    extra: u64,
    /// The warning is up: the recording stops at this time (unix ms).
    warned: Option<u64>,
    /// …and it's also a Windows notification.
    native: bool,
    /// Quiet meetings and check-ins.
    quiet: QuietWatch,
}

impl Watch {
    /// The length warning is up, or due within `margin`: no check-in now.
    fn length_busy(&self, now: u64, t: &Timings, margin: u64) -> bool {
        self.warned.is_some()
            || t.limit.is_some_and(|limit| self.started + limit + self.extra <= now + t.warn_before + margin)
    }
}

static WATCH: Mutex<Option<Watch>> = Mutex::new(None);
/// Bumped per recording: an older watcher thread sees it and leaves.
static GEN: AtomicU64 = AtomicU64::new(0);

fn watch_lock() -> std::sync::MutexGuard<'static, Option<Watch>> {
    WATCH.lock().unwrap_or_else(|p| p.into_inner())
}

/// The warning, for the toast and the Windows notification.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LimitWarning {
    note_id: u64,
    /// When the recording stops (unix ms).
    stop_at: u64,
    title: String,
    body: String,
    /// The main window shows it (the person is looking at it, or nothing
    /// else could); otherwise it's on the Yap bar or a Windows notification
    /// and the window doesn't repeat it (one place: `notice.rs`).
    in_app: bool,
}

impl LimitWarning {
    fn new(note_id: u64, stop_at: u64, now: u64, t: &Timings) -> Self {
        let more = if t.keep_going == 3_600_000 {
            "another hour".to_string()
        } else {
            format!("{} more", duration_words(t.keep_going))
        };
        LimitWarning {
            note_id,
            stop_at,
            title: format!("Notes stop in {}", duration_words(stop_at.saturating_sub(now))),
            body: format!(
                "Yap will stop recording and write your action plan. Keep going gives you {more}."
            ),
            in_app: true,
        }
    }
}

fn current_warning() -> Option<LimitWarning> {
    let now = now_ms();
    let t = timings();
    watch_lock().as_ref().and_then(|w| {
        w.warned.map(|stop_at| LimitWarning { in_app: !w.native, ..LimitWarning::new(w.note_id, stop_at, now, &t) })
    })
}

fn emit_limit(app: &AppHandle, warning: Option<&LimitWarning>) {
    let _ = app.emit(EVENT_LIMIT, serde_json::json!({ "warning": warning }));
}

/// Watch recording `gen` until it stops (a thread per recording): its
/// length, then whether the meeting has gone quiet or is due a check-in.
fn watch(app: AppHandle, gen: u64) {
    loop {
        let testing = test_timings().is_some() || test_quiet_timings().is_some();
        std::thread::sleep(if testing { TEST_TICK } else { TICK });
        if GEN.load(Ordering::SeqCst) != gen {
            return;
        }
        if !length_tick(&app, gen) {
            return;
        }
        quiet_tick(&app, gen);
    }
}

/// The length guard's turn; `false` once it stopped the recording (or
/// there's nothing to watch).
fn length_tick(app: &AppHandle, gen: u64) -> bool {
    let t = timings();
    let now = now_ms();
    let (next, note_id, started) = {
        let guard = watch_lock();
        let Some(w) = guard.as_ref() else { return false };
        (step(now, w.started, w.extra, w.warned, &t), w.note_id, w.started)
    };
    match next {
        Step::Wait => {}
        Step::Warn { stop_at } => warn(app, gen, note_id, stop_at, now, &t),
        Step::Withdraw => {
            tracing::info!(note_id, "meeting guard: the limit moved; warning withdrawn");
            withdraw(app);
        }
        Step::Stop => {
            tracing::info!(note_id, "meeting guard: reached the maximum recording length");
            withdraw(app);
            withdraw_check_in(app);
            GEN.fetch_add(1, Ordering::SeqCst);
            stop_and_summarise(app, Some(note_id));
            notice(app, &Notice::stopped_at_limit(now.saturating_sub(started)));
            return false;
        }
    }
    true
}

fn warn(app: &AppHandle, gen: u64, note_id: u64, stop_at: u64, now: u64, t: &Timings) {
    {
        let mut guard = watch_lock();
        match guard.as_mut() {
            Some(w) if GEN.load(Ordering::SeqCst) == gen => w.warned = Some(stop_at),
            _ => return,
        }
    }
    // One place (`notice.rs`): the main window while the person is looking
    // at it, else the Yap bar, else a Windows notification.
    let place = crate::notices::notice::place(app, true);
    let mut warning = LimitWarning::new(note_id, stop_at, now, t);
    warning.in_app = matches!(place, crate::notices::notice::Place::Window | crate::notices::notice::Place::Nowhere);
    tracing::info!(note_id, title = %warning.title, ?place, "meeting guard: length warning");
    emit_limit(app, Some(&warning));
    // The recording stopped, or Keep going landed, in between: take it back
    // rather than leave a stale warning up.
    let still = GEN.load(Ordering::SeqCst) == gen
        && watch_lock().as_ref().is_some_and(|w| w.warned == Some(stop_at));
    if !still {
        emit_limit(app, None);
        return;
    }
    if warning.in_app {
        return;
    }
    #[cfg(windows)]
    match native::limit(app, &warning) {
        Ok(()) => {
            let posted_for = |w: &mut Watch| {
                let current = w.warned == Some(stop_at);
                w.native |= current;
                current
            };
            // Answered or stopped while it was being posted: take it down.
            if !watch_lock().as_mut().is_some_and(posted_for) {
                native::remove(app, native::LIMIT);
            }
        }
        Err(e) => {
            tracing::info!("meeting guard: no Windows notification ({e})");
            // Nowhere else after all: the main window has it.
            emit_limit(app, Some(&LimitWarning { in_app: true, ..warning }));
        }
    }
}

/// Take the warning down (answered, moot, or the recording stopped).
fn withdraw(app: &AppHandle) {
    let (had, native) = {
        let mut guard = watch_lock();
        match guard.as_mut() {
            Some(w) => (w.warned.take().is_some(), std::mem::take(&mut w.native)),
            None => (false, false),
        }
    };
    if had {
        emit_limit(app, None);
    }
    #[cfg(windows)]
    if native {
        native::remove(app, native::LIMIT);
    }
    #[cfg(not(windows))]
    let _ = native;
}

/// "Keep going" (the toast or the Windows notification): the limit moves
/// out by an hour from the current one.
pub fn keep_going(app: &AppHandle) -> Result<(), String> {
    let t = timings();
    let now = now_ms();
    let native = {
        let mut guard = watch_lock();
        let Some(w) = guard.as_mut() else {
            return Err("No meeting is being recorded".to_string());
        };
        if w.warned.is_none() {
            return Ok(()); // answered already, on the other surface
        }
        w.extra = kept_going(now, w.started, w.extra, &t);
        w.warned = None;
        // The person just said the meeting goes on: no check-in after this.
        w.quiet.checked_in();
        tracing::info!(note_id = w.note_id, extra_ms = w.extra, "meeting guard: keep going");
        std::mem::take(&mut w.native)
    };
    emit_limit(app, None);
    #[cfg(windows)]
    if native {
        native::remove(app, native::LIMIT);
    }
    #[cfg(not(windows))]
    let _ = native;
    Ok(())
}

/// A recording started or stopped (`yap-meeting-state`).
fn on_meeting_state(app: &AppHandle, payload: &str) {
    let state: serde_json::Value = serde_json::from_str(payload).unwrap_or_default();
    let recording = state["recording"].as_bool() == Some(true);
    let gen = GEN.fetch_add(1, Ordering::SeqCst) + 1;
    match state["noteId"].as_u64().filter(|_| recording) {
        Some(note_id) => {
            let elapsed = state["elapsedSecs"].as_u64().unwrap_or(0) * 1000;
            let previous = watch_lock().replace(Watch {
                note_id,
                started: now_ms().saturating_sub(elapsed),
                extra: 0,
                warned: None,
                native: false,
                quiet: QuietWatch::default(),
            });
            if previous.is_some_and(|w| w.warned.is_some()) {
                emit_limit(app, None);
            }
            withdraw_check_in(app);
            let handle = app.clone();
            let spawned = std::thread::Builder::new()
                .name("meeting-length".into())
                .spawn(move || watch(handle, gen));
            if let Err(e) = spawned {
                tracing::warn!("meeting guard: couldn't watch the recording's length ({e})");
            }
        }
        None => {
            withdraw(app);
            withdraw_check_in(app);
            *watch_lock() = None;
        }
    }
}

// ---- quiet meetings and check-ins ------------------------------------------------------------

/// After this long with nobody speaking (ms of meeting audio)…
const QUIET_END_MS: u64 = 10 * 60_000;
/// …"Ending notes in 30 s…" counts down this long.
const QUIET_COUNTDOWN_MS: u64 = 30_000;
/// A check-in waits until nobody has spoken for this long…
const RECENT_SPEECH_MS: u64 = 3 * 60_000;
/// …and comes this long after the calendar meeting's end…
const AFTER_END_MS: u64 = 10 * 60_000;
/// …or this far into a recording.
const CHECK_IN_AT_MS: u64 = 60 * 60_000;
/// A check-in nobody answers goes away after this long (it only asks).
const CHECK_IN_FADE_MS: u64 = 2 * 60_000;
/// No check-in while the length warning is up or due within this long.
const LENGTH_MARGIN_MS: u64 = 10 * 60_000;
/// A calendar "meeting" this long or longer (a day blocked out) gets no
/// check-in at its end.
const LONGEST_EVENT_SECS: u64 = 24 * 60 * 60;
/// The bar card and the Windows notification of the countdown / check-ins.
const CHECKIN_TAG: &str = "meeting-checkin";

/// The quiet guard's timings (ms).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct QuietTimings {
    /// Nobody speaking this long (meeting audio) starts the countdown.
    quiet: u64,
    countdown: u64,
    /// A check-in waits for this long without speech.
    recent: u64,
    after_end: u64,
    check_in_at: u64,
    fade: u64,
    length_margin: u64,
}

impl QuietTimings {
    const REAL: QuietTimings = QuietTimings {
        quiet: QUIET_END_MS,
        countdown: QUIET_COUNTDOWN_MS,
        recent: RECENT_SPEECH_MS,
        after_end: AFTER_END_MS,
        check_in_at: CHECK_IN_AT_MS,
        fade: CHECK_IN_FADE_MS,
        length_margin: LENGTH_MARGIN_MS,
    };
}

/// Short timings for the e2e suite (`e2e_meeting_checkins`, debug builds).
static TEST_QUIET: Mutex<Option<QuietTimings>> = Mutex::new(None);

fn test_quiet_timings() -> Option<QuietTimings> {
    *TEST_QUIET.lock().unwrap_or_else(|p| p.into_inner())
}

fn quiet_timings() -> QuietTimings {
    test_quiet_timings().unwrap_or(QuietTimings::REAL)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CheckInKind {
    /// The calendar meeting the note belongs to ended a while ago.
    Calendar,
    /// An hour into the recording.
    Hour,
}

/// What the quiet guard looks at, once a tick.
#[derive(Debug, Clone, Copy)]
struct Look {
    /// Unix ms.
    now: u64,
    /// When this recording started (unix ms).
    started: u64,
    /// The meeting's audio clock and talk counter (`meeting::activity`).
    clock: u64,
    talk: u64,
    /// "End notes when the meeting goes quiet".
    end_when_quiet: bool,
    /// The end of the calendar meeting the note belongs to (unix ms).
    event_end: Option<u64>,
    /// The length warning is up or due soon ([`Watch::length_busy`]).
    length_busy: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuietStep {
    Wait,
    /// Nobody has spoken for a while: count down to `until` (unix ms). A
    /// check-in that's up gives way to it.
    Count { until: u64 },
    /// Someone spoke (or the setting went off): the countdown is off.
    Cancel,
    /// The countdown ran out: stop and write the action plan.
    End,
    /// Ask "Still in your meeting?".
    Ask(CheckInKind),
    /// The check-in goes away unanswered: 2 minutes passed, or the length
    /// warning came.
    Fade,
}

/// One recording's quiet guard: how long nobody has spoken, the countdown,
/// and the check-ins.
#[derive(Debug, Default)]
struct QuietWatch {
    /// The talk counter last seen (`None`: not looked yet).
    talk: Option<u64>,
    /// The audio clock at the last speech (or the recording's start, or a
    /// Keep going).
    last_speech: u64,
    /// The audio clock last seen.
    clock: u64,
    /// The countdown is up: it ends at this time (unix ms).
    countdown: Option<u64>,
    /// A check-in is up: asked at this time (unix ms).
    check_in: Option<u64>,
    asked_calendar: bool,
    asked_hour: bool,
}

impl QuietWatch {
    /// How long nobody has spoken (ms of meeting audio).
    fn quiet_for(&self) -> u64 {
        self.clock.saturating_sub(self.last_speech)
    }

    fn step(&mut self, k: &Look, t: &QuietTimings) -> QuietStep {
        let spoke = self.talk.is_some_and(|before| before != k.talk);
        // The quiet count starts with the recording, and again at every word.
        if self.talk.is_none() || spoke {
            self.last_speech = k.clock;
        }
        self.talk = Some(k.talk);
        self.clock = k.clock;
        let quiet = self.quiet_for();

        if let Some(until) = self.countdown {
            if spoke || !k.end_when_quiet {
                self.countdown = None;
                self.last_speech = k.clock;
                return QuietStep::Cancel;
            }
            if k.now >= until {
                // (Should the stop not happen, another full quiet stretch
                // before asking again.)
                self.countdown = None;
                self.last_speech = k.clock;
                return QuietStep::End;
            }
            return QuietStep::Wait;
        }
        if k.end_when_quiet && quiet >= t.quiet {
            let until = k.now + t.countdown;
            self.countdown = Some(until);
            self.check_in = None;
            return QuietStep::Count { until };
        }
        if let Some(asked) = self.check_in {
            if k.length_busy || k.now >= asked + t.fade {
                self.check_in = None;
                return QuietStep::Fade;
            }
            return QuietStep::Wait;
        }
        // A check-in waits while anyone spoke lately, and never comes on
        // top of the length warning.
        if k.length_busy || quiet < t.recent {
            return QuietStep::Wait;
        }
        let calendar_due = k.event_end.is_some_and(|end| {
            let at = end + t.after_end;
            k.started < at && k.now >= at
        });
        if !self.asked_calendar && calendar_due {
            self.asked_calendar = true;
            self.check_in = Some(k.now);
            return QuietStep::Ask(CheckInKind::Calendar);
        }
        if !self.asked_hour && k.now >= k.started + t.check_in_at {
            self.asked_hour = true;
            self.check_in = Some(k.now);
            return QuietStep::Ask(CheckInKind::Hour);
        }
        QuietStep::Wait
    }

    /// "Keep going" / "Keep recording": the prompt's gone, and the quiet
    /// count starts again from now.
    fn kept(&mut self) {
        self.countdown = None;
        self.check_in = None;
        self.last_speech = self.clock;
    }

    /// The length warning's Keep going: as good as a check-in.
    fn checked_in(&mut self) {
        self.asked_calendar = true;
        self.asked_hour = true;
        if self.check_in.take().is_some() {
            self.last_speech = self.clock;
        }
    }
}

/// Where a countdown or check-in shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Surface {
    /// The meeting notepad's toasts (it's on screen).
    Notepad,
    /// The main window's toasts (it's focused).
    Main,
    Bar,
    /// A Windows notification.
    Windows,
}

impl Surface {
    /// The name the windows know it by (`CheckIn::surface`, `Notice::surface`).
    fn label(self) -> &'static str {
        match self {
            Surface::Notepad => crate::notepad::LABEL,
            Surface::Main => "settings",
            Surface::Bar => "bar",
            Surface::Windows => "windows",
        }
    }
}

/// Where the person is looking: the meeting notepad if it's on screen, else
/// the main window if it's focused, else the Yap bar if it takes cards
/// (`bar`: for a countdown, one that would be seen now), else a Windows
/// notification.
fn pick_surface(notepad: bool, main_focused: bool, bar: bool) -> Surface {
    if notepad {
        Surface::Notepad
    } else if main_focused {
        Surface::Main
    } else if bar {
        Surface::Bar
    } else {
        Surface::Windows
    }
}

/// [`pick_surface`] now. Window getters: never with a lock held. (A test
/// run says for itself whether the main window counts as focused:
/// `notice::window_view`.)
fn surface_now(app: &AppHandle, countdown: bool) -> Surface {
    let notepad = notepad_on_screen(app);
    let main_focused = window_view(app).1;
    let bar = if countdown { crate::bar::cards_seen() } else { crate::bar::cards_available() };
    pick_surface(notepad, main_focused, bar)
}

/// A prompt's answer: what clicking it reports, and its label.
#[derive(Debug, Clone, Serialize)]
pub struct Answer {
    id: &'static str,
    label: &'static str,
}

/// The countdown or a check-in, as the windows show it (`yap-meeting-checkin`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckIn {
    id: u64,
    note_id: u64,
    /// "quiet" (the countdown) | "calendar" | "hour".
    kind: &'static str,
    title: String,
    body: String,
    /// When the countdown ends (unix ms): "Ending notes in 28 s…".
    until: Option<u64>,
    /// Where it shows: "notepad" | "settings" (that window's toast) |
    /// "bar" | "windows".
    surface: &'static str,
    /// The button on the right; the quiet one beside it.
    primary: Answer,
    secondary: Answer,
    /// The countdown's why ("Nobody has spoken for 10 minutes"): the bar
    /// card's title under its headline, the Windows notification's body.
    #[serde(skip)]
    reason: String,
}

const KEEP_GOING: Answer = Answer { id: "keep", label: "Keep going" };
const STOP_NOW: Answer = Answer { id: "stop", label: "Stop now" };
const STOP_AND_SUMMARISE: Answer = Answer { id: "stop", label: "Stop and summarise" };
const KEEP_RECORDING: Answer = Answer { id: "keep", label: "Keep recording" };

/// "30 s", "1 min 5 s" (rounded up): the countdown's time left.
fn seconds_words(ms: u64) -> String {
    let secs = ms.div_ceil(1000).max(1);
    match (secs / 60, secs % 60) {
        (0, s) => format!("{s} s"),
        (m, 0) => format!("{m} min"),
        (m, s) => format!("{m} min {s} s"),
    }
}

impl CheckIn {
    /// "Ending notes in 30 s…": nobody has spoken for `quiet_ms`.
    fn countdown(id: u64, note_id: u64, until: u64, now: u64, quiet_ms: u64, surface: Surface) -> Self {
        let reason = format!("Nobody has spoken for {}", duration_words(quiet_ms));
        CheckIn {
            id,
            note_id,
            kind: "quiet",
            title: format!("Ending notes in {}…", seconds_words(until.saturating_sub(now))),
            body: format!("{reason}. Yap will stop recording and write your action plan."),
            until: Some(until),
            surface: surface.label(),
            primary: KEEP_GOING,
            secondary: STOP_NOW,
            reason,
        }
    }

    /// "Still in your meeting?" (`event`: the calendar meeting's title and
    /// end, unix ms, for a calendar check-in).
    fn still_there(
        id: u64,
        note_id: u64,
        kind: CheckInKind,
        look: &Look,
        quiet_ms: u64,
        event: Option<(&str, u64)>,
        surface: Surface,
    ) -> Self {
        let (kind, body) = match (kind, event) {
            (CheckInKind::Calendar, Some((title, end))) => (
                "calendar",
                format!(
                    "\u{201c}{}\u{201d} was due to end {} ago. Yap is still taking notes.",
                    title.trim(),
                    duration_words(look.now.saturating_sub(end))
                ),
            ),
            (CheckInKind::Calendar, None) => ("calendar", "Your calendar meeting is over. Yap is still taking notes.".into()),
            (CheckInKind::Hour, _) => (
                "hour",
                format!(
                    "Yap has been taking notes for {}, and nobody has spoken for {}.",
                    duration_words(look.now.saturating_sub(look.started)),
                    duration_words(quiet_ms)
                ),
            ),
        };
        CheckIn {
            id,
            note_id,
            kind,
            title: "Still in your meeting?".to_string(),
            body,
            until: None,
            surface: surface.label(),
            primary: STOP_AND_SUMMARISE,
            secondary: KEEP_RECORDING,
            reason: String::new(),
        }
    }
}

/// The countdown or check-in that's up.
static CHECK_IN: Mutex<Option<CheckIn>> = Mutex::new(None);
static CHECK_IN_SEQ: AtomicU64 = AtomicU64::new(0);

fn check_in_lock() -> std::sync::MutexGuard<'static, Option<CheckIn>> {
    CHECK_IN.lock().unwrap_or_else(|p| p.into_inner())
}

fn emit_check_in(app: &AppHandle, prompt: Option<&CheckIn>) {
    let _ = app.emit(EVENT_CHECKIN, serde_json::json!({ "prompt": prompt }));
}

/// Put `p` up where it says (replacing whatever was up).
fn show_check_in(app: &AppHandle, p: CheckIn) {
    let previous = check_in_lock().replace(p.clone());
    tracing::info!(note_id = p.note_id, kind = p.kind, surface = p.surface, "meeting guard: {}", p.title);
    // The windows keep the prompt either way (a reload asks for it); only
    // the one it names shows it.
    emit_check_in(app, Some(&p));
    #[cfg(windows)]
    {
        let elsewhere = previous.as_ref().is_some_and(|old| old.surface != p.surface);
        if elsewhere {
            native::remove(app, native::CHECKIN);
        }
        if matches!(p.surface, "bar" | "windows") {
            if let Err(e) = native::check_in(app, &p, p.surface == "bar") {
                tracing::info!("meeting guard: no Windows notification ({e})");
            }
        }
    }
    #[cfg(not(windows))]
    let _ = previous;
}

/// Take the countdown or check-in down (answered, moot, the recording
/// stopped). Whether one was up.
fn withdraw_check_in(app: &AppHandle) -> bool {
    let Some(p) = check_in_lock().take() else {
        return false;
    };
    emit_check_in(app, None);
    #[cfg(windows)]
    if matches!(p.surface, "bar" | "windows") {
        native::remove(app, native::CHECKIN);
    }
    #[cfg(not(windows))]
    let _ = p;
    true
}

/// A notice on the surface a countdown was on (the reason it stopped).
fn notice_on(app: &AppHandle, surface: &str, mut n: Notice) {
    match surface {
        "notepad" => n.surface = Some(crate::notepad::LABEL),
        "settings" => n.surface = Some("settings"),
        _ => {
            // The bar, else a Windows notification.
            notice_native(app, &n);
            return;
        }
    }
    notice(app, &n);
}

/// The quiet guard's turn (see "Quiet meetings" at the top).
fn quiet_tick(app: &AppHandle, gen: u64) {
    let Some(activity) = crate::meeting::activity() else {
        return;
    };
    let t = quiet_timings();
    let cfg = crate::config::load();
    let length = test_timings().unwrap_or_else(|| Timings::from_minutes(cfg.meeting_max_minutes));
    let event = crate::notes::event_of(activity.note_id)
        .filter(|e| e.end > e.start && e.end - e.start < LONGEST_EVENT_SECS);
    let now = now_ms();
    let (step, look, quiet_ms) = {
        let mut guard = watch_lock();
        let Some(w) = guard.as_mut() else { return };
        if w.note_id != activity.note_id || GEN.load(Ordering::SeqCst) != gen {
            return;
        }
        let look = Look {
            now,
            started: w.started,
            clock: activity.clock_ms,
            talk: activity.talk,
            end_when_quiet: cfg.meeting_end_when_quiet,
            event_end: event.as_ref().map(|e| e.end * 1000),
            length_busy: w.length_busy(now, &length, t.length_margin),
        };
        let step = w.quiet.step(&look, &t);
        (step, look, w.quiet.quiet_for())
    };
    let note_id = activity.note_id;
    match step {
        QuietStep::Wait => {}
        QuietStep::Count { until } => {
            let surface = surface_now(app, true);
            let id = CHECK_IN_SEQ.fetch_add(1, Ordering::SeqCst) + 1;
            show_check_in(app, CheckIn::countdown(id, note_id, until, now, quiet_ms, surface));
        }
        QuietStep::Cancel => {
            tracing::info!(note_id, "meeting guard: someone spoke; the countdown is off");
            withdraw_check_in(app);
        }
        QuietStep::End => {
            let surface = check_in_lock().as_ref().map_or("windows", |p| p.surface);
            tracing::info!(note_id, quiet_ms, "meeting guard: the meeting went quiet; stopping");
            withdraw_check_in(app);
            GEN.fetch_add(1, Ordering::SeqCst);
            stop_and_summarise(app, Some(note_id));
            notice_on(app, surface, Notice::stopped_quiet(t.quiet, note_id));
        }
        QuietStep::Ask(kind) => {
            let surface = surface_now(app, false);
            let id = CHECK_IN_SEQ.fetch_add(1, Ordering::SeqCst) + 1;
            let event = event.as_ref().map(|e| (e.title.as_str(), e.end * 1000));
            show_check_in(app, CheckIn::still_there(id, note_id, kind, &look, quiet_ms, event, surface));
        }
        QuietStep::Fade => {
            tracing::info!(note_id, "meeting guard: the check-in went unanswered (or the length warning came)");
            withdraw_check_in(app);
        }
    }
}

/// An answer to the countdown or a check-in (`id`: which one; an answer to
/// one that's gone does nothing): "keep" (Keep going / Keep recording, or
/// its ✕) or "stop" (Stop now / Stop and summarise).
pub fn check_in_answer(app: &AppHandle, id: u64, answer: &str) -> Result<(), String> {
    let note_id = {
        let guard = check_in_lock();
        match guard.as_ref() {
            Some(p) if p.id == id => p.note_id,
            _ => return Ok(()), // answered already, on another surface
        }
    };
    tracing::info!(note_id, choice = answer, "meeting guard: check-in answered");
    match answer {
        "keep" => {
            if let Some(w) = watch_lock().as_mut() {
                w.quiet.kept();
            }
            withdraw_check_in(app);
            Ok(())
        }
        "stop" => {
            withdraw_check_in(app);
            stop_and_summarise(app, Some(note_id));
            Ok(())
        }
        other => Err(format!("Unknown answer: {other}")),
    }
}

// ---- the meeting shortcut ---------------------------------------------------------------

/// When the meeting shortcut was last pressed (unix ms).
static LAST_SHORTCUT: AtomicU64 = AtomicU64::new(0);

/// "Win + Alt + M" for a `kb:` spec; `None` for mouse buttons, modifier
/// chords and keys without a short name.
fn shortcut_label(spec: &str) -> Option<String> {
    let mut parts: Vec<String> = spec.strip_prefix("kb:")?.split('+').map(str::to_ascii_lowercase).collect();
    let vk: u32 = parts.pop()?.parse().ok()?;
    let key = match vk {
        48..=57 | 65..=90 => char::from_u32(vk)?.to_string(),
        112..=135 => format!("F{}", vk - 111),
        32 => "Space".to_string(),
        _ => return None,
    };
    let held = |names: &[&str]| parts.iter().any(|p| names.contains(&p.as_str()));
    let mut out: Vec<String> = [
        (&["win", "super", "meta", "cmd", "command"][..], "Win"),
        (&["ctrl", "control"][..], "Ctrl"),
        (&["alt", "option"][..], "Alt"),
        (&["shift"][..], "Shift"),
    ]
    .iter()
    .filter(|(names, _)| held(names))
    .map(|(_, label)| label.to_string())
    .collect();
    out.push(key);
    Some(out.join(" + "))
}

/// The meeting shortcut, or anything else that starts or stops meeting
/// notes in one go. Recording: stop and write the action plan. A call live:
/// take notes on it (call detection's "Record notes"). Otherwise: a new
/// meeting note ("Meeting · 5 Oct, 14:30", folder Meetings), recording.
/// Blocking (the recorder takes a moment to start), so off the main thread.
pub fn start_or_stop(app: &AppHandle) {
    start_or_stop_from(app, None);
}

/// [`start_or_stop`], pressed in window `origin` (its in-page fallback).
fn start_or_stop_from(app: &AppHandle, origin: Option<&str>) {
    let now = now_ms();
    if now.saturating_sub(LAST_SHORTCUT.swap(now, Ordering::SeqCst)) < SHORTCUT_DEBOUNCE_MS {
        tracing::info!("meeting shortcut: pressed again straight away; ignored");
        return;
    }
    if let Some(note_id) = crate::meeting::recording_note() {
        tracing::info!(note_id, "meeting shortcut: stop and summarise");
        stop_from(app, Some(note_id), origin);
        return;
    }
    let shortcut = shortcut_label(&crate::config::load().meeting_hotkey);
    if let Some((call_id, call)) = crate::calls::meeting_detect::latest_call() {
        tracing::info!(call_id, "meeting shortcut: record the call");
        crate::calls::meeting_detect::on_tray_record(app, call_id);
        if let Some(note_id) = crate::meeting::recording_note() {
            if window_view(app).0 {
                notice(app, &Notice::taking_notes(Some(&call), shortcut.as_deref(), note_id));
            }
        }
        return;
    }
    match new_meeting_note(app) {
        Ok(note_id) => {
            tracing::info!(note_id, "meeting shortcut: taking notes in a new meeting note");
            let n = Notice::taking_notes(None, shortcut.as_deref(), note_id);
            if window_view(app).0 {
                let _ = app.emit(EVENT_OPEN, serde_json::json!({ "noteId": note_id, "stop": false }));
            }
            notice(app, &n);
        }
        Err(e) => {
            tracing::warn!("meeting shortcut: couldn't start ({e})");
            #[cfg(windows)]
            if !window_view(app).0 && native::failed(app, &e).is_ok() {
                return;
            }
            let _ = crate::commands::show_settings(app);
            let _ = app.emit("yap-error", format!("Couldn't start meeting notes. {e}"));
        }
    }
}

/// A meeting note of its own, recording (the shortcut with no call). On
/// failure the note is gone again.
fn new_meeting_note(app: &AppHandle) -> Result<u64, String> {
    crate::notes::folder_create("Meetings");
    let title = crate::calls::meeting_detect::meeting_note_title();
    let note = crate::notes::create(&title, "", "meeting", "Meetings");
    crate::notes::mark_meeting(note.id)?;
    let started = crate::commands::meeting_start(app.clone(), app.state::<crate::AppState>(), note.id);
    if let Err(e) = started {
        crate::notes::delete(note.id);
        let _ = app.emit("yap-notes-changed", ());
        return Err(e);
    }
    let _ = app.emit("yap-notes-changed", ());
    Ok(note.id)
}

/// Follow the meeting recorder and the shortcut's global hotkey (app setup).
pub fn init(app: &AppHandle) {
    let handle = app.clone();
    app.listen("yap-meeting-state", move |event| on_meeting_state(&handle, event.payload()));
    let handle = app.clone();
    app.listen("meeting-key-pressed", move |_| {
        let app = handle.clone();
        tauri::async_runtime::spawn_blocking(move || start_or_stop(&app));
    });
}

// ---- commands -----------------------------------------------------------------------------

/// The meeting shortcut from a window's in-page fallback (see
/// [`start_or_stop`]); `origin` = that window's label.
#[tauri::command]
pub async fn meeting_shortcut(app: AppHandle, origin: Option<String>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || start_or_stop_from(&app, origin.as_deref()))
        .await
        .map_err(|e| e.to_string())
}

/// "Keep going" on the length warning.
#[tauri::command]
pub fn meeting_keep_going(app: AppHandle) -> Result<(), String> {
    keep_going(&app)
}

/// The length warning that's up, if any: `{ warning }` (as `yap-meeting-limit`).
#[tauri::command]
pub fn meeting_limit_status() -> serde_json::Value {
    serde_json::json!({ "warning": current_warning() })
}

/// An answer to the quiet countdown or a check-in (see [`check_in_answer`]).
#[tauri::command]
pub fn meeting_checkin_respond(app: AppHandle, id: u64, answer: String) -> Result<(), String> {
    check_in_answer(&app, id, &answer)
}

/// The countdown or check-in that's up, if any: `{ prompt }` (as
/// `yap-meeting-checkin`), for a window that loads while one is.
#[tauri::command]
pub fn meeting_checkin_status() -> serde_json::Value {
    serde_json::json!({ "prompt": check_in_lock().clone() })
}

/// Test mode only: shorter quiet-meeting and check-in timings (ms), so the
/// e2e suite needn't wait for hours — `quietMs` of meeting audio with nobody
/// speaking before the countdown, `countdownMs`, `recentMs` without speech
/// before a check-in, `afterEndMs` after a calendar meeting's end, `hourMs`
/// into a recording, `fadeMs` for an unanswered check-in, `marginMs` before
/// the length warning; each left out keeps its real value. The meeting test
/// player's mic delivers silence meanwhile, as a real one does, so the
/// audio clock runs (`e2e::set_live_mic`). No arguments at all: the real
/// timings, and the player as before.
#[cfg(debug_assertions)]
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn e2e_meeting_checkins(
    quiet_ms: Option<u64>,
    countdown_ms: Option<u64>,
    recent_ms: Option<u64>,
    after_end_ms: Option<u64>,
    hour_ms: Option<u64>,
    fade_ms: Option<u64>,
    margin_ms: Option<u64>,
) -> Result<(), String> {
    if !crate::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    let any = [quiet_ms, countdown_ms, recent_ms, after_end_ms, hour_ms, fade_ms, margin_ms]
        .iter()
        .any(Option::is_some);
    let r = QuietTimings::REAL;
    let timings = any.then(|| QuietTimings {
        quiet: quiet_ms.unwrap_or(r.quiet),
        countdown: countdown_ms.unwrap_or(r.countdown),
        recent: recent_ms.unwrap_or(r.recent),
        after_end: after_end_ms.unwrap_or(r.after_end),
        check_in_at: hour_ms.unwrap_or(r.check_in_at),
        fade: fade_ms.unwrap_or(r.fade),
        length_margin: margin_ms.unwrap_or(r.length_margin),
    });
    tracing::info!(?timings, "e2e: quiet-meeting timings");
    *TEST_QUIET.lock().unwrap_or_else(|p| p.into_inner()) = timings;
    crate::e2e::set_live_mic(any);
    Ok(())
}

/// Test mode only: shorter length timings (ms) so the e2e suite needn't
/// wait hours — `limitMs` (none: no limit), `warnMs` before it, `keepGoingMs`
/// per "Keep going". No arguments at all: back to Settings' limit.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_meeting_limit(
    limit_ms: Option<u64>,
    warn_ms: Option<u64>,
    keep_going_ms: Option<u64>,
) -> Result<(), String> {
    if !crate::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    let timings = (limit_ms.is_some() || warn_ms.is_some() || keep_going_ms.is_some()).then(|| Timings {
        limit: limit_ms,
        warn_before: warn_ms.unwrap_or(WARN_BEFORE_MS),
        keep_going: keep_going_ms.unwrap_or(KEEP_GOING_MS),
    });
    tracing::info!(?timings, "e2e: meeting length timings");
    *TEST_TIMINGS.lock().unwrap_or_else(|p| p.into_inner()) = timings;
    Ok(())
}

// ---- Windows notifications -------------------------------------------------------------------

/// The guard rails' Windows notifications (silent, Yap's logo; see
/// `crate::notices::win_toast`): the length warning with **Keep going**, and notices
/// (the screen-share tip with **Update settings**, "Taking notes" with
/// **Open note**).
#[cfg(windows)]
mod native {
    use std::collections::HashMap;
    use std::sync::{LazyLock, Mutex};

    use tauri::{AppHandle, Emitter};
    use windows::UI::Notifications::ToastNotification;

    use super::{CheckIn, LimitWarning, Notice, EVENT_OPEN};
    use crate::notices::win_toast::{esc, logo_xml};

    pub const LIMIT: &str = "meeting-limit";
    const NOTICE: &str = "meeting-notice";
    pub const CHECKIN: &str = super::CHECKIN_TAG;

    /// The live toasts by tag, kept so their buttons work from the
    /// notification center.
    static LIVE: LazyLock<Mutex<HashMap<&'static str, ToastNotification>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));

    fn post(app: &AppHandle, tag: &'static str, xml: &str) -> Result<(), String> {
        if !crate::notices::win_toast::allowed() {
            return Err("off in portable mode and test runs".to_string());
        }
        let toast = crate::notices::win_toast::post(app, tag, xml, None, activated)?;
        LIVE.lock().unwrap_or_else(|p| p.into_inner()).insert(tag, toast);
        Ok(())
    }

    /// On the Yap bar while it's on screen (`crate::bar`), instead of a
    /// Windows notification: the same buttons, back through [`activated`].
    fn bar_card(app: &AppHandle, card: crate::bar::Card) -> bool {
        crate::bar::show_card(app, card, Box::new(activated))
    }

    pub fn remove(app: &AppHandle, tag: &'static str) {
        crate::bar::dismiss_card(app, tag);
        if LIVE.lock().unwrap_or_else(|p| p.into_inner()).remove(tag).is_some() {
            crate::notices::win_toast::remove(app, tag);
        }
    }

    /// "Notes stop in 5 minutes" with **Keep going**; the body opens the note.
    pub fn limit_xml(w: &LimitWarning, logo: &str) -> String {
        format!(
            "<toast launch=\"guard:open:{}\"><visual><binding template=\"ToastGeneric\">\
             <text>{}</text><text>{}</text>{logo}</binding></visual><actions>\
             <action content=\"Keep going\" arguments=\"guard:keep\" activationType=\"foreground\"/>\
             </actions><audio silent=\"true\"/></toast>",
            w.note_id,
            esc(&w.title),
            esc(&w.body),
        )
    }

    pub fn limit(app: &AppHandle, w: &LimitWarning) -> Result<(), String> {
        let card = crate::bar::Card {
            id: LIMIT.into(),
            icon: "timer",
            title: w.title.clone(),
            body: w.body.clone(),
            primary: Some(crate::bar::CardAction::new("guard:keep", "Keep going")),
            secondary: Some(crate::bar::CardAction::new(format!("guard:open:{}", w.note_id), "Open note")),
            ..Default::default()
        };
        if bar_card(app, card) {
            return Ok(());
        }
        post(app, LIMIT, &limit_xml(w, &logo_xml()))
    }

    /// The countdown or a check-in as a Windows notification: its two
    /// answers as buttons, the body opening the note. The countdown's title
    /// can't count down there: "Ending notes in 30 seconds".
    pub fn check_in_xml(p: &CheckIn, now: u64, logo: &str) -> String {
        let (title, body) = match p.until {
            Some(until) => (
                format!("Ending notes in {}", super::duration_words(until.saturating_sub(now))),
                format!("{}. Keep going to carry on recording.", p.reason),
            ),
            None => (p.title.clone(), p.body.clone()),
        };
        format!(
            "<toast launch=\"guard:open:{note}\"><visual><binding template=\"ToastGeneric\">\
             <text>{}</text><text>{}</text>{logo}</binding></visual><actions>\
             <action content=\"{}\" arguments=\"guard:checkin:{}:{id}\" activationType=\"foreground\"/>\
             <action content=\"{}\" arguments=\"guard:checkin:{}:{id}\" activationType=\"foreground\"/>\
             </actions><audio silent=\"true\"/></toast>",
            esc(&title),
            esc(&body),
            esc(p.primary.label),
            p.primary.id,
            esc(p.secondary.label),
            p.secondary.id,
            note = p.note_id,
            id = p.id,
        )
    }

    /// The countdown or a check-in on the Yap bar (`bar_first`) or, with the
    /// bar off or hidden, as a Windows notification. The countdown's card
    /// reads at a glance: "Ending notes in 28 s…" as its headline, the
    /// seconds in a ring; its ✕ only hides it (the countdown carries on, as
    /// the length warning's does). A check-in's ✕ is Keep recording.
    pub fn check_in(app: &AppHandle, p: &CheckIn, bar_first: bool) -> Result<(), String> {
        let arg = |answer: &str| format!("guard:checkin:{answer}:{}", p.id);
        let card = match p.until {
            Some(until) => crate::bar::Card {
                id: CHECKIN.into(),
                icon: "timer",
                title: p.reason.clone(),
                body: "Yap will stop recording and write your action plan.".into(),
                primary: Some(crate::bar::CardAction::new(arg(p.primary.id), p.primary.label)),
                secondary: Some(crate::bar::CardAction::new(arg(p.secondary.id), p.secondary.label)),
                countdown: Some(crate::bar::Countdown {
                    done: Some("Ending notes…".into()),
                    headline: true,
                    ..crate::bar::Countdown::new(until, "Ending notes in")
                }),
                ..Default::default()
            },
            None => crate::bar::Card {
                id: CHECKIN.into(),
                icon: "timer",
                title: p.title.clone(),
                body: p.body.clone(),
                primary: Some(crate::bar::CardAction::new(arg(p.primary.id), p.primary.label)),
                secondary: Some(crate::bar::CardAction::new(arg(p.secondary.id), p.secondary.label)),
                close_action: Some(arg("keep")),
                ..Default::default()
            },
        };
        if bar_first && bar_card(app, card) {
            return Ok(());
        }
        post(app, CHECKIN, &check_in_xml(p, super::now_ms(), &logo_xml()))
    }

    /// A notice, with **Update settings** or **Open note** when it has one.
    pub fn notice_xml(n: &Notice, logo: &str) -> String {
        let (launch, action) = match (n.settings, n.note_id) {
            (Some(section), _) => (
                format!("guard:settings:{}", esc(section)),
                format!(
                    "<actions><action content=\"Update settings\" arguments=\"guard:settings:{}\" activationType=\"foreground\"/></actions>",
                    esc(section)
                ),
            ),
            (None, Some(id)) => (
                format!("guard:open:{id}"),
                format!("<actions><action content=\"Open note\" arguments=\"guard:open:{id}\" activationType=\"foreground\"/></actions>"),
            ),
            (None, None) => ("guard:show".to_string(), String::new()),
        };
        format!(
            "<toast launch=\"{launch}\"><visual><binding template=\"ToastGeneric\">\
             <text>{}</text><text>{}</text>{logo}</binding></visual>{action}\
             <audio silent=\"true\"/></toast>",
            esc(&n.title),
            esc(&n.body),
        )
    }

    pub fn notice(app: &AppHandle, n: &Notice) -> Result<(), String> {
        let action = match (n.settings, n.note_id) {
            (Some(section), _) => Some(crate::bar::CardAction::new(format!("guard:settings:{section}"), "Update settings")),
            (None, Some(id)) => Some(crate::bar::CardAction::new(format!("guard:open:{id}"), "Open note")),
            (None, None) => None,
        };
        let card = crate::bar::Card {
            id: NOTICE.into(),
            icon: n.icon,
            title: n.title.clone(),
            body: n.body.clone(),
            primary: action,
            timeout_ms: Some(10_000),
            ..Default::default()
        };
        if bar_card(app, card) {
            return Ok(());
        }
        post(app, NOTICE, &notice_xml(n, &logo_xml()))
    }

    /// The meeting shortcut couldn't start notes: say why.
    pub fn failed(app: &AppHandle, message: &str) -> Result<(), String> {
        let card = crate::bar::Card {
            id: NOTICE.into(),
            icon: "error",
            title: "Couldn't start meeting notes".into(),
            body: message.into(),
            timeout_ms: Some(10_000),
            ..Default::default()
        };
        if bar_card(app, card) {
            return Ok(());
        }
        let xml = format!(
            "<toast launch=\"guard:show\"><visual><binding template=\"ToastGeneric\">\
             <text>Couldn't start meeting notes</text><text>{}</text>{}</binding></visual>\
             <audio silent=\"true\"/></toast>",
            esc(message),
            logo_xml(),
        );
        post(app, NOTICE, &xml)
    }

    /// A click: `guard:keep`, `guard:checkin:<keep|stop>:<id>`,
    /// `guard:settings:<section>`, `guard:open:<note id>`, or the body of a
    /// plain notice (`guard:show`).
    fn activated(app: &AppHandle, arg: &str) {
        tracing::info!(arg, "meeting guard: Windows notification clicked");
        let app = app.clone();
        let arg = arg.to_string();
        tauri::async_runtime::spawn_blocking(move || {
            if arg == "guard:keep" {
                if let Err(e) = super::keep_going(&app) {
                    tracing::info!("meeting guard: keep going: {e}");
                }
                return;
            }
            if let Some(rest) = arg.strip_prefix("guard:checkin:") {
                let (answer, id) = rest.split_once(':').unwrap_or((rest, ""));
                if let Ok(id) = id.parse::<u64>() {
                    if let Err(e) = super::check_in_answer(&app, id, answer) {
                        tracing::info!("meeting guard: check-in: {e}");
                    }
                }
                return;
            }
            let _ = crate::commands::show_settings(&app);
            if let Some(section) = arg.strip_prefix("guard:settings:") {
                let _ = app.emit("yap-open-settings", section);
            } else if let Some(id) = arg.strip_prefix("guard:open:").and_then(|n| n.parse::<u64>().ok()) {
                let _ = app.emit(EVENT_OPEN, serde_json::json!({ "noteId": id, "stop": false }));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: u64 = 60_000;
    const HOUR: u64 = 60 * MIN;

    fn two_hours() -> Timings {
        Timings::from_minutes(120)
    }

    #[test]
    fn the_limit_comes_from_settings() {
        assert_eq!(two_hours().limit, Some(2 * HOUR));
        assert_eq!(Timings::from_minutes(0).limit, None, "0 = no limit");
        let cfg = crate::config::YapConfig::default();
        assert_eq!(cfg.meeting_max_minutes, 120, "2 hours by default, as Wispr");
        assert_eq!(cfg.meeting_call_end, "ask");
        let from_empty: crate::config::YapConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(from_empty.meeting_max_minutes, 120);
    }

    #[test]
    fn warns_five_minutes_before_then_stops() {
        let t = two_hours();
        let start = 1_000_000;
        assert_eq!(step(start + HOUR, start, 0, None, &t), Step::Wait);
        assert_eq!(step(start + 2 * HOUR - 6 * MIN, start, 0, None, &t), Step::Wait);
        let at = start + 2 * HOUR - 5 * MIN;
        assert_eq!(step(at, start, 0, None, &t), Step::Warn { stop_at: start + 2 * HOUR });
        let warned = Some(start + 2 * HOUR);
        assert_eq!(step(at + MIN, start, 0, warned, &t), Step::Wait);
        assert_eq!(step(start + 2 * HOUR, start, 0, warned, &t), Step::Stop);
    }

    #[test]
    fn a_late_warning_still_gives_five_minutes() {
        // The limit lowered to 1 hour at 1:30 in, or the PC slept through it.
        let t = Timings::from_minutes(60);
        let start = 0;
        let now = 90 * MIN;
        assert_eq!(step(now, start, 0, None, &t), Step::Warn { stop_at: now + 5 * MIN });
        assert_eq!(step(now + 4 * MIN, start, 0, Some(now + 5 * MIN), &t), Step::Wait);
        assert_eq!(step(now + 5 * MIN, start, 0, Some(now + 5 * MIN), &t), Step::Stop);
    }

    #[test]
    fn raising_or_removing_the_limit_takes_the_warning_back() {
        let start = 0;
        let now = 2 * HOUR - 4 * MIN;
        let warned = Some(2 * HOUR);
        assert_eq!(step(now, start, 0, warned, &Timings::from_minutes(180)), Step::Withdraw);
        assert_eq!(step(now, start, 0, warned, &Timings::from_minutes(0)), Step::Withdraw);
        assert_eq!(step(now, start, 0, None, &Timings::from_minutes(0)), Step::Wait);
    }

    #[test]
    fn keep_going_adds_an_hour() {
        let t = two_hours();
        let start = 0;
        let now = 2 * HOUR - 5 * MIN;
        let extra = kept_going(now, start, 0, &t);
        assert_eq!(extra, HOUR);
        // The next warning is five minutes before 3:00.
        assert_eq!(step(now + MIN, start, extra, None, &t), Step::Wait);
        assert_eq!(step(3 * HOUR - 5 * MIN, start, extra, None, &t), Step::Warn { stop_at: 3 * HOUR });
        // After a late warning, from now rather than from a limit long past.
        let late = Timings::from_minutes(60);
        let now = 90 * MIN;
        let extra = kept_going(now, start, 0, &late);
        assert_eq!(start + HOUR + extra, now + HOUR);
        assert_eq!(step(now + MIN, start, extra, None, &late), Step::Wait);
    }

    #[test]
    fn the_warning_says_when_and_what_happens() {
        let t = two_hours();
        let w = LimitWarning::new(7, 2 * HOUR, 2 * HOUR - 5 * MIN, &t);
        assert_eq!(w.title, "Notes stop in 5 minutes");
        assert!(w.body.contains("write your action plan"));
        assert!(w.body.contains("Keep going gives you another hour"));
        let test = Timings { limit: Some(6_000), warn_before: 4_000, keep_going: 6_000 };
        assert_eq!(LimitWarning::new(7, 6_000, 2_000, &test).title, "Notes stop in 4 seconds");
    }

    #[test]
    fn durations_in_words() {
        assert_eq!(duration_words(5 * MIN), "5 minutes");
        assert_eq!(duration_words(5 * MIN - 400), "5 minutes");
        assert_eq!(duration_words(MIN), "1 minute");
        assert_eq!(duration_words(2 * HOUR), "2 hours");
        assert_eq!(duration_words(HOUR), "1 hour");
        assert_eq!(duration_words(HOUR + 30 * MIN), "1 hour and 30 minutes");
        assert_eq!(duration_words(1_000), "1 second");
        assert_eq!(duration_words(11_200), "11 seconds");
        assert_eq!(duration_words(0), "1 second");
    }

    #[test]
    fn shortcut_labels() {
        assert_eq!(shortcut_label("kb:alt+win+77").as_deref(), Some("Win + Alt + M"));
        assert_eq!(
            shortcut_label(&crate::config::YapConfig::default().meeting_hotkey).as_deref(),
            Some("Win + Alt + M")
        );
        assert_eq!(shortcut_label("kb:ctrl+alt+77").as_deref(), Some("Ctrl + Alt + M"));
        assert_eq!(shortcut_label("kb:ctrl+shift+32").as_deref(), Some("Ctrl + Shift + Space"));
        assert_eq!(shortcut_label("kb:120").as_deref(), Some("F9"));
        assert_eq!(shortcut_label("mouse:4"), None);
        assert_eq!(shortcut_label("mods:ctrl+alt"), None);
        assert_eq!(shortcut_label(""), None);
    }

    #[test]
    fn notices_say_what_happened() {
        let n = Notice::screen_share();
        assert_eq!(n.body, "Your meeting notes show up in screen shares and screenshots.");
        // Settings → General, at the switch (Settings.svelte's `settings-screen-sharing`).
        assert_eq!(n.settings, Some("meetings#screen-sharing"));
        assert_eq!(Notice::call_ended("Teams call").title, "Teams call ended");
        assert_eq!(Notice::stopped_at_limit(2 * HOUR).title, "Stopped at 2 hours");
        let n = Notice::taking_notes(None, Some("Win + Alt + M"), 3);
        assert_eq!(n.title, "Taking notes");
        assert!(n.body.ends_with("Press Win + Alt + M again to stop and write the action plan."));
        assert_eq!(Notice::taking_notes(Some("Zoom call"), None, 3).title, "Taking notes on your Zoom call");
        // camelCase for the page.
        let json = serde_json::to_value(Notice::screen_share()).unwrap();
        assert_eq!(json["kind"], "screenShare");
        assert_eq!(json["noteId"], serde_json::Value::Null);
    }

    // ---- quiet meetings and check-ins ----

    const SEC: u64 = 1_000;
    const T: QuietTimings = QuietTimings::REAL;
    const START: u64 = 1_700_000_000_000;

    /// A look `at` ms into a recording whose audio clock is `at` too (audio
    /// keeps arriving), with the talk counter at `talk`.
    fn look(at: u64, talk: u64) -> Look {
        Look {
            now: START + at,
            started: START,
            clock: at,
            talk,
            end_when_quiet: true,
            event_end: None,
            length_busy: false,
        }
    }

    /// Step `w` once a second from `from` to `to` ms (inclusive), the talk
    /// counter fixed; the steps that weren't Wait, with when.
    fn run(w: &mut QuietWatch, from: u64, to: u64, talk: u64, f: impl Fn(Look) -> Look) -> Vec<(u64, QuietStep)> {
        run_with(&T, w, from, to, talk, f)
    }

    fn run_with(
        t: &QuietTimings,
        w: &mut QuietWatch,
        from: u64,
        to: u64,
        talk: u64,
        f: impl Fn(Look) -> Look,
    ) -> Vec<(u64, QuietStep)> {
        let mut out = Vec::new();
        let mut at = from;
        while at <= to {
            let step = w.step(&f(look(at, talk)), t);
            if step != QuietStep::Wait {
                out.push((at, step));
            }
            at += SEC;
        }
        out
    }

    #[test]
    fn ten_quiet_minutes_count_down_then_end() {
        let mut w = QuietWatch::default();
        let steps = run(&mut w, 0, 10 * MIN + 31 * SEC, 0, |l| l);
        assert_eq!(
            steps,
            vec![
                (10 * MIN, QuietStep::Count { until: START + 10 * MIN + 30 * SEC }),
                (10 * MIN + 30 * SEC, QuietStep::End),
            ],
            "counted from the recording's start; 30 s later it ends"
        );
    }

    #[test]
    fn anyone_speaking_puts_the_count_back_and_cancels_a_countdown() {
        let mut w = QuietWatch::default();
        // Someone talks at 4 minutes: ten quiet minutes from there.
        assert!(run(&mut w, 0, 4 * MIN, 0, |l| l).is_empty());
        assert!(run(&mut w, 4 * MIN + SEC, 14 * MIN, 1, |l| l).is_empty());
        let steps = run(&mut w, 14 * MIN + SEC, 14 * MIN + SEC, 1, |l| l);
        assert_eq!(steps, vec![(14 * MIN + SEC, QuietStep::Count { until: START + 14 * MIN + 31 * SEC })]);
        // A word mid-countdown: off, and ten more minutes before the next.
        let steps = run(&mut w, 14 * MIN + 10 * SEC, 14 * MIN + 10 * SEC, 2, |l| l);
        assert_eq!(steps, vec![(14 * MIN + 10 * SEC, QuietStep::Cancel)]);
        assert!(run(&mut w, 14 * MIN + 11 * SEC, 24 * MIN + 9 * SEC, 2, |l| l).is_empty());
        assert!(matches!(w.step(&look(24 * MIN + 10 * SEC, 2), &T), QuietStep::Count { .. }));
    }

    #[test]
    fn quiet_time_runs_on_the_meetings_audio_clock() {
        // The PC slept (or the device died): the wall clock ran on, the
        // audio clock didn't. No countdown.
        let mut w = QuietWatch::default();
        let frozen = |l: Look| Look { clock: (5 * MIN).min(l.clock), ..l };
        assert!(run(&mut w, 0, 40 * MIN, 0, frozen).is_empty());
    }

    #[test]
    fn keep_going_buys_another_ten_minutes_and_the_setting_turns_it_off() {
        let mut w = QuietWatch::default();
        run(&mut w, 0, 10 * MIN, 0, |l| l);
        assert!(w.countdown.is_some());
        w.kept();
        assert!(w.countdown.is_none());
        assert!(run(&mut w, 10 * MIN + SEC, 20 * MIN - SEC, 0, |l| l).is_empty());
        assert!(matches!(w.step(&look(20 * MIN, 0), &T), QuietStep::Count { .. }));
        // Switched off mid-countdown: cancelled, and nothing after.
        let off = |l: Look| Look { end_when_quiet: false, ..l };
        assert_eq!(w.step(&off(look(20 * MIN + SEC, 0)), &T), QuietStep::Cancel);
        assert!(run(&mut w, 20 * MIN + 2 * SEC, 50 * MIN, 0, off).is_empty());
    }

    #[test]
    fn the_hour_check_in_waits_for_three_quiet_minutes_and_comes_once() {
        let mut w = QuietWatch::default();
        let talking = |at: u64| at / (2 * MIN); // a new word every 2 minutes
        let mut at = 0;
        while at <= 70 * MIN {
            let step = w.step(&look(at, talking(at)), &T);
            assert!(!matches!(step, QuietStep::Ask(_)), "asked at {at} while people talked");
            at += SEC;
        }
        // The talk stops at 70:00 (last word then): asked 3 minutes later.
        let steps = run(&mut w, 70 * MIN + SEC, 74 * MIN, talking(70 * MIN), |l| l);
        assert_eq!(steps, vec![(73 * MIN, QuietStep::Ask(CheckInKind::Hour))]);
        // Unanswered, it goes after 2 minutes; never again this recording.
        let steps = run(&mut w, 74 * MIN + SEC, 76 * MIN, talking(70 * MIN), |l| l);
        assert_eq!(steps, vec![(75 * MIN, QuietStep::Fade)]);
        assert!(!run(&mut w, 76 * MIN + SEC, 79 * MIN, talking(70 * MIN), |l| l)
            .iter()
            .any(|(_, s)| matches!(s, QuietStep::Ask(_))));
    }

    #[test]
    fn the_calendar_check_in_comes_ten_minutes_after_the_meetings_end() {
        let ends = START + 30 * MIN;
        let booked = |l: Look| Look { event_end: Some(ends), ..l };
        let mut w = QuietWatch::default();
        // Quiet from 35:00 on (the last word then).
        assert!(run(&mut w, 0, 35 * MIN, 0, |l| booked(Look { talk: l.now - START, ..l })).is_empty());
        let steps = run(&mut w, 35 * MIN + SEC, 40 * MIN, 35 * MIN, booked);
        assert_eq!(steps, vec![(40 * MIN, QuietStep::Ask(CheckInKind::Calendar))]);
        // Keep recording: gone, and the quiet count starts again.
        w.kept();
        assert_eq!(w.quiet_for(), 0);
        // A recording started after that point gets no calendar check-in.
        let mut late = QuietWatch::default();
        let after = |l: Look| Look { started: ends + 11 * MIN, now: ends + 11 * MIN + (l.now - START), ..booked(l) };
        assert!(!run(&mut late, 0, 9 * MIN, 0, after).iter().any(|(_, s)| matches!(s, QuietStep::Ask(_))));
    }

    #[test]
    fn check_ins_never_come_on_top_of_the_length_warning() {
        let busy = |l: Look| Look { length_busy: true, ..l };
        let mut w = QuietWatch::default();
        let quiet_hour = |l: Look| Look { talk: (l.now - START).min(58 * MIN), ..l };
        assert!(run(&mut w, 0, 65 * MIN, 0, |l| busy(quiet_hour(l))).iter().all(|(_, s)| !matches!(s, QuietStep::Ask(_))));
        // The warning answered (busy no more): the check-in may come…
        let mut w = QuietWatch::default();
        run(&mut w, 0, 61 * MIN, 0, quiet_hour);
        assert!(w.check_in.is_some());
        // …and the length warning arriving takes it down.
        assert_eq!(w.step(&busy(quiet_hour(look(61 * MIN + SEC, 0))), &T), QuietStep::Fade);
        // "Keep going" on the length warning counts as a check-in.
        let mut w = QuietWatch::default();
        w.checked_in();
        assert!(run(&mut w, 0, 65 * MIN, 0, quiet_hour).iter().all(|(_, s)| !matches!(s, QuietStep::Ask(_))));
    }

    #[test]
    fn the_countdown_replaces_a_check_in_that_is_up() {
        // (With the real timings a check-in has faded long before: it needs
        // 3 quiet minutes and goes 2 minutes later.)
        let slow_fade = QuietTimings { fade: 30 * MIN, ..T };
        let mut w = QuietWatch::default();
        let steps = run_with(&slow_fade, &mut w, 0, 60 * MIN, 0, |l| Look { talk: (l.now - START).min(55 * MIN), ..l });
        assert_eq!(steps, vec![(60 * MIN, QuietStep::Ask(CheckInKind::Hour))]);
        let steps = run_with(&slow_fade, &mut w, 60 * MIN + SEC, 65 * MIN, 55 * MIN, |l| l);
        assert_eq!(steps, vec![(65 * MIN, QuietStep::Count { until: START + 65 * MIN + 30 * SEC })]);
        assert!(w.check_in.is_none());
    }

    #[test]
    fn the_length_guard_is_busy_near_its_warning() {
        let t = Timings::from_minutes(120);
        let w = Watch { note_id: 1, started: 0, extra: 0, warned: None, native: false, quiet: QuietWatch::default() };
        assert!(!w.length_busy(60 * MIN, &t, LENGTH_MARGIN_MS));
        assert!(!w.length_busy(104 * MIN, &t, LENGTH_MARGIN_MS));
        assert!(w.length_busy(105 * MIN, &t, LENGTH_MARGIN_MS), "the warning's due at 1:55");
        // A one-hour limit: no hour check-in at all.
        assert!(w.length_busy(60 * MIN, &Timings::from_minutes(60), LENGTH_MARGIN_MS));
        assert!(!w.length_busy(60 * MIN, &Timings::from_minutes(0), LENGTH_MARGIN_MS));
        let warned = Watch { warned: Some(2 * HOUR), ..w };
        assert!(warned.length_busy(0, &t, LENGTH_MARGIN_MS));
    }

    #[test]
    fn prompts_show_where_the_person_is_looking() {
        assert_eq!(pick_surface(true, true, true), Surface::Notepad);
        assert_eq!(pick_surface(false, true, true), Surface::Main);
        assert_eq!(pick_surface(false, false, true), Surface::Bar);
        assert_eq!(pick_surface(false, false, false), Surface::Windows);
        assert_eq!(Surface::Notepad.label(), "notepad");
        assert_eq!(Surface::Main.label(), "settings");
    }

    #[test]
    fn prompts_say_what_and_why() {
        let p = CheckIn::countdown(3, 7, START + 30 * SEC, START, 10 * MIN, Surface::Bar);
        assert_eq!(p.title, "Ending notes in 30 s…");
        assert_eq!(p.body, "Nobody has spoken for 10 minutes. Yap will stop recording and write your action plan.");
        assert_eq!((p.primary.label, p.secondary.label), ("Keep going", "Stop now"));
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["noteId"], 7);
        assert_eq!(v["until"], START + 30 * SEC);
        assert_eq!(v["surface"], "bar");
        assert!(v.get("reason").is_none());

        let l = look(62 * MIN, 0);
        let p = CheckIn::still_there(4, 7, CheckInKind::Hour, &l, 3 * MIN, None, Surface::Notepad);
        assert_eq!(p.title, "Still in your meeting?");
        assert_eq!(p.body, "Yap has been taking notes for 1 hour and 2 minutes, and nobody has spoken for 3 minutes.");
        assert_eq!((p.primary.id, p.secondary.id), ("stop", "keep"));
        let ended = START + 50 * MIN;
        let p = CheckIn::still_there(5, 7, CheckInKind::Calendar, &l, 3 * MIN, Some((" Design review ", ended)), Surface::Main);
        assert_eq!(p.kind, "calendar");
        assert_eq!(p.body, "\u{201c}Design review\u{201d} was due to end 12 minutes ago. Yap is still taking notes.");

        let n = Notice::stopped_quiet(10 * MIN, 7);
        assert_eq!(n.title, "Stopped: the meeting went quiet");
        assert!(n.body.starts_with("Nobody had spoken for 10 minutes"));
        assert_eq!(seconds_words(29_100), "30 s");
        assert_eq!(seconds_words(65_000), "1 min 5 s");
    }

    #[test]
    fn the_quiet_setting_is_on_by_default() {
        assert!(crate::config::YapConfig::default().meeting_end_when_quiet);
        let from_empty: crate::config::YapConfig = serde_json::from_str("{}").unwrap();
        assert!(from_empty.meeting_end_when_quiet);
    }

    #[cfg(windows)]
    #[test]
    fn notification_xml_has_the_right_buttons() {
        let t = two_hours();
        let w = LimitWarning::new(7, 2 * HOUR, 2 * HOUR - 5 * MIN, &t);
        let xml = native::limit_xml(&w, "");
        assert!(xml.contains("<text>Notes stop in 5 minutes</text>"));
        assert!(xml.contains("content=\"Keep going\" arguments=\"guard:keep\""));
        assert!(xml.contains("launch=\"guard:open:7\""));
        crate::notices::win_toast::build("meeting-limit", &xml, false).unwrap();
        let xml = native::notice_xml(&Notice::screen_share(), "");
        assert!(xml.contains("content=\"Update settings\" arguments=\"guard:settings:meetings#screen-sharing\""));
        crate::notices::win_toast::build("meeting-notice", &xml, false).unwrap();
        let xml = native::notice_xml(&Notice::taking_notes(None, None, 4), "");
        assert!(xml.contains("content=\"Open note\" arguments=\"guard:open:4\""));
        crate::notices::win_toast::build("meeting-notice", &xml, false).unwrap();
        let p = CheckIn::countdown(9, 4, START + 30 * SEC, START, 10 * MIN, Surface::Windows);
        let xml = native::check_in_xml(&p, START, "");
        assert!(xml.contains("<text>Ending notes in 30 seconds</text>"), "{xml}");
        assert!(xml.contains("<text>Nobody has spoken for 10 minutes. Keep going to carry on recording.</text>"));
        assert!(xml.contains("content=\"Keep going\" arguments=\"guard:checkin:keep:9\""));
        assert!(xml.contains("content=\"Stop now\" arguments=\"guard:checkin:stop:9\""));
        crate::notices::win_toast::build("meeting-checkin", &xml, false).unwrap();
        let p = CheckIn::still_there(10, 4, CheckInKind::Hour, &look(HOUR, 0), 3 * MIN, None, Surface::Windows);
        let xml = native::check_in_xml(&p, START, "");
        assert!(xml.contains("<text>Still in your meeting?</text>"));
        assert!(xml.contains("content=\"Stop and summarise\" arguments=\"guard:checkin:stop:10\""));
        crate::notices::win_toast::build("meeting-checkin", &xml, false).unwrap();
    }
}
