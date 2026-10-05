//! Guard rails around a meeting recording (ROADMAP Phase 8, after Wispr
//! Flow's Notetaker settings), and the notices they and `capture.rs` show.
//!
//! - **Maximum recording length** (`meeting_max_minutes`: 2 hours by
//!   default, as Wispr; 0 = no limit). Five minutes before it: "Notes stop
//!   in 5 minutes" with **Keep going** (another hour), as an in-app toast
//!   plus a Windows notification while the main window isn't focused. At the
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
//! - [`notice`]: one-off messages for these and for the screen-share
//!   warning (`yap-meeting-notice`, plus [`notice_native`] for a Windows
//!   notification where it matters).
//!
//! Every stop goes through [`stop_and_summarise`]. In test mode the
//! debug-only `e2e_meeting_limit` shortens the length timings.

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
/// Open a note in the main window's Notes view: `{ noteId, stop }` (the
/// event `meeting_detect.rs` uses for the same).
const EVENT_OPEN: &str = "yap-meeting-open-note";
/// Settings → General → Meetings, at "Hide Yap's meeting windows from
/// screen sharing" (`yap-settings-goto`: a section, then the card's id).
const SCREEN_SHARE_SETTINGS: &str = "general#screen-sharing";

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// The main window: (on screen, focused). Never call with a lock held:
/// window getters wait on the main thread.
fn window_view(app: &AppHandle) -> (bool, bool) {
    app.get_webview_window("settings")
        .map(|w| {
            let visible = w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false);
            (visible, visible && w.is_focused().unwrap_or(false))
        })
        .unwrap_or((false, false))
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
        }
    }
}

/// Show `n` in the main window (a toast, whether or not it's on screen).
pub fn notice(app: &AppHandle, n: &Notice) {
    let _ = app.emit(EVENT_NOTICE, n);
}

/// Post `n` as a Windows notification (not in portable mode or test runs).
pub fn notice_native(app: &AppHandle, n: &Notice) {
    #[cfg(windows)]
    if let Err(e) = native::notice(app, n) {
        tracing::info!("meeting guard: no Windows notification ({e})");
    }
    #[cfg(not(windows))]
    let _ = (app, n);
}

/// [`notice`], plus a Windows notification while the main window isn't
/// focused (the call app is in front, or Yap is in the tray).
pub fn notice_everywhere(app: &AppHandle, n: &Notice) {
    notice(app, n);
    if !window_view(app).1 {
        notice_native(app, n);
    }
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

/// The recording the length guard watches.
struct Watch {
    note_id: u64,
    started: u64,
    /// Added by "Keep going".
    extra: u64,
    /// The warning is up: the recording stops at this time (unix ms).
    warned: Option<u64>,
    /// …and it's also a Windows notification.
    native: bool,
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
        }
    }
}

fn current_warning() -> Option<LimitWarning> {
    let now = now_ms();
    let t = timings();
    watch_lock()
        .as_ref()
        .and_then(|w| w.warned.map(|stop_at| LimitWarning::new(w.note_id, stop_at, now, &t)))
}

fn emit_limit(app: &AppHandle, warning: Option<&LimitWarning>) {
    let _ = app.emit(EVENT_LIMIT, serde_json::json!({ "warning": warning }));
}

/// Watch recording `gen` until it stops (a thread per recording).
fn watch(app: AppHandle, gen: u64) {
    loop {
        std::thread::sleep(if test_timings().is_some() { TEST_TICK } else { TICK });
        if GEN.load(Ordering::SeqCst) != gen {
            return;
        }
        let t = timings();
        let now = now_ms();
        let (next, note_id, started) = {
            let guard = watch_lock();
            let Some(w) = guard.as_ref() else { return };
            (step(now, w.started, w.extra, w.warned, &t), w.note_id, w.started)
        };
        match next {
            Step::Wait => {}
            Step::Warn { stop_at } => warn(&app, gen, note_id, stop_at, now, &t),
            Step::Withdraw => {
                tracing::info!(note_id, "meeting guard: the limit moved; warning withdrawn");
                withdraw(&app);
            }
            Step::Stop => {
                tracing::info!(note_id, "meeting guard: reached the maximum recording length");
                withdraw(&app);
                GEN.fetch_add(1, Ordering::SeqCst);
                stop_and_summarise(&app, Some(note_id));
                notice(&app, &Notice::stopped_at_limit(now.saturating_sub(started)));
                return;
            }
        }
    }
}

fn warn(app: &AppHandle, gen: u64, note_id: u64, stop_at: u64, now: u64, t: &Timings) {
    {
        let mut guard = watch_lock();
        match guard.as_mut() {
            Some(w) if GEN.load(Ordering::SeqCst) == gen => w.warned = Some(stop_at),
            _ => return,
        }
    }
    let warning = LimitWarning::new(note_id, stop_at, now, t);
    tracing::info!(note_id, title = %warning.title, "meeting guard: length warning");
    emit_limit(app, Some(&warning));
    // The recording stopped, or Keep going landed, in between: take it back
    // rather than leave a stale warning up.
    let still = GEN.load(Ordering::SeqCst) == gen
        && watch_lock().as_ref().is_some_and(|w| w.warned == Some(stop_at));
    if !still {
        emit_limit(app, None);
        return;
    }
    if window_view(app).1 {
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
        Err(e) => tracing::info!("meeting guard: no Windows notification ({e})"),
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
            });
            if previous.is_some_and(|w| w.warned.is_some()) {
                emit_limit(app, None);
            }
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
            *watch_lock() = None;
        }
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
    if let Some((call_id, call)) = crate::meeting_detect::latest_call() {
        tracing::info!(call_id, "meeting shortcut: record the call");
        crate::meeting_detect::on_tray_record(app, call_id);
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
                notice(app, &n);
            } else {
                notice_native(app, &n);
            }
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
    let title = crate::meeting_detect::meeting_note_title();
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
/// `crate::win_toast`): the length warning with **Keep going**, and notices
/// (the screen-share tip with **Update settings**, "Taking notes" with
/// **Open note**).
#[cfg(windows)]
mod native {
    use std::collections::HashMap;
    use std::sync::{LazyLock, Mutex};

    use tauri::{AppHandle, Emitter};
    use windows::UI::Notifications::ToastNotification;

    use super::{LimitWarning, Notice, EVENT_OPEN};
    use crate::win_toast::{esc, logo_xml};

    pub const LIMIT: &str = "meeting-limit";
    const NOTICE: &str = "meeting-notice";

    /// The live toasts by tag, kept so their buttons work from the
    /// notification center.
    static LIVE: LazyLock<Mutex<HashMap<&'static str, ToastNotification>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));

    fn post(app: &AppHandle, tag: &'static str, xml: &str) -> Result<(), String> {
        if !crate::win_toast::allowed() {
            return Err("off in portable mode and test runs".to_string());
        }
        let toast = crate::win_toast::post(app, tag, xml, None, activated)?;
        LIVE.lock().unwrap_or_else(|p| p.into_inner()).insert(tag, toast);
        Ok(())
    }

    pub fn remove(app: &AppHandle, tag: &'static str) {
        if LIVE.lock().unwrap_or_else(|p| p.into_inner()).remove(tag).is_some() {
            crate::win_toast::remove(app, tag);
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
        post(app, LIMIT, &limit_xml(w, &logo_xml()))
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
        post(app, NOTICE, &notice_xml(n, &logo_xml()))
    }

    /// The meeting shortcut couldn't start notes: say why.
    pub fn failed(app: &AppHandle, message: &str) -> Result<(), String> {
        let xml = format!(
            "<toast launch=\"guard:show\"><visual><binding template=\"ToastGeneric\">\
             <text>Couldn't start meeting notes</text><text>{}</text>{}</binding></visual>\
             <audio silent=\"true\"/></toast>",
            esc(message),
            logo_xml(),
        );
        post(app, NOTICE, &xml)
    }

    /// A click: `guard:keep`, `guard:settings:<section>`, `guard:open:<note
    /// id>`, or the body of a plain notice (`guard:show`).
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
        assert_eq!(n.settings, Some("general#screen-sharing"));
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

    #[cfg(windows)]
    #[test]
    fn notification_xml_has_the_right_buttons() {
        let t = two_hours();
        let w = LimitWarning::new(7, 2 * HOUR, 2 * HOUR - 5 * MIN, &t);
        let xml = native::limit_xml(&w, "");
        assert!(xml.contains("<text>Notes stop in 5 minutes</text>"));
        assert!(xml.contains("content=\"Keep going\" arguments=\"guard:keep\""));
        assert!(xml.contains("launch=\"guard:open:7\""));
        crate::win_toast::build("meeting-limit", &xml, false).unwrap();
        let xml = native::notice_xml(&Notice::screen_share(), "");
        assert!(xml.contains("content=\"Update settings\" arguments=\"guard:settings:general#screen-sharing\""));
        crate::win_toast::build("meeting-notice", &xml, false).unwrap();
        let xml = native::notice_xml(&Notice::taking_notes(None, None, 4), "");
        assert!(xml.contains("content=\"Open note\" arguments=\"guard:open:4\""));
        crate::win_toast::build("meeting-notice", &xml, false).unwrap();
    }
}
