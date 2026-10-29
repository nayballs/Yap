//! Yap's dictation pipeline.
//!
//! Simpler than Voice Mirror's (no TTS, no AI routing, no modes, no VAD —
//! only a Voice Assistant question ends on a pause, `pause.rs`):
//! the mic (`mic.rs`, opened on demand) captures audio into a buffer while
//! *recording* is on; toggling off runs STT, applies the dictionary, and
//! injects the text into whatever window is focused. A short chime marks
//! start/stop.
//!
//! This file holds the shared state ([`Shared`]) and the [`Pipeline`]
//! handle; the stages are its children: `recording` (the shortcuts to the
//! stop), `transcribe` (`run_stt`, Upload, Run again), `delivery` (cleanup
//! to the paste), `rewrite` (the rewrite shortcut and Commands), `finish`
//! (the corrections, "press enter", snippets), `preview` (the live text),
//! `engine` (the warm engine everyone shares), `busy` (the `processing`
//! flag), `unplugged` (the mic disconnected mid-dictation), `slow` ("Taking
//! longer than usual…" and Skip cleanup) and, in debug builds, `robot`.

mod busy;
mod delivery;
mod engine;
mod finish;
mod preview;
mod recording;
mod rewrite;
#[cfg(debug_assertions)]
mod robot;
mod slow;
mod steps;
mod transcribe;
mod unplugged;

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::time::{Duration, Instant, SystemTime};

use tauri::{AppHandle, Emitter};

use crate::pasting::app_kind::AppKind;
use crate::config::{self, YapConfig};
use crate::dictation::gesture::Gesture;
use crate::dictation::mic_watch::MicTally;
use crate::dictation::stt::{self, SttAdapter, SttError};

use busy::{spawn_busy, Busy, Job};
use engine::set_model_loading;

#[cfg(debug_assertions)]
pub(crate) use busy::crash_next_dictation;
pub(crate) use engine::{lease_engine, EngineCell, EngineSlot};
pub(crate) use finish::{apply_corrections, as_said_text};
pub(crate) use rewrite::commands_model;
#[cfg(debug_assertions)]
pub(crate) use rewrite::Rewritten;
#[cfg(debug_assertions)]
pub(crate) use robot::Robot;
#[cfg(debug_assertions)]
pub(crate) use robot::stage_snip_question;

pub(crate) const TARGET_SAMPLE_RATE: u32 = 16_000;

/// How much audio (16 kHz mono samples) to keep in the rolling pre-roll ring so
/// the first word isn't clipped: speech that started a moment *before* the user
/// pressed the key is prepended to the recording. 300 ms × 16 kHz. (The
/// meeting recorder blanks this much before a dictation, too.)
pub(crate) const PREROLL_SAMPLES: usize = (0.3 * TARGET_SAMPLE_RATE as f64) as usize;

/// Dictations as the meeting recorder sees them: `count << 1 | recording`,
/// where `count` goes up by one each time a dictation starts. One atomic, so
/// `meeting.rs` reads a consistent pair from its audio callback without a
/// lock, and still notices a dictation that started and ended between two of
/// its callbacks. While one records, a meeting's "You" side gets silence
/// instead of the dictated words (and its pre-roll).
static DICTATION: AtomicU64 = AtomicU64::new(0);

/// `(dictations started so far, one recording now)`.
pub(crate) fn dictation_state() -> (u64, bool) {
    let s = DICTATION.load(Ordering::SeqCst);
    (s >> 1, s & 1 == 1)
}

/// A dictation started recording: `start_recording`, for plain dictation,
/// the edit hotkey and the wake-word path alike (all record the same way).
/// (The e2e suite's meeting player stands in for one in test mode.)
pub(crate) fn dictation_began() {
    let mut s = DICTATION.load(Ordering::SeqCst);
    loop {
        let next = (((s >> 1) + 1) << 1) | 1;
        match DICTATION.compare_exchange_weak(s, next, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => break,
            Err(now) => s = now,
        }
    }
}

/// The dictation stopped recording (transcribed or cancelled).
pub(crate) fn dictation_ended() {
    DICTATION.fetch_and(!1, Ordering::SeqCst);
}

/// The running pipeline, if there is one, for the input hook's news.
fn with_pipeline(app: &AppHandle, f: impl FnOnce(&Pipeline)) {
    use tauri::Manager;
    let Some(state) = app.try_state::<crate::AppState>() else { return };
    let guard = state.pipeline.lock();
    if let Ok(guard) = guard {
        if let Some(p) = guard.as_ref() {
            f(p);
        }
    }
}

/// The input hook: a hold-to-talk press grew into a chord (another key went
/// down at `at`, under a second after it): the person meant a shortcut, so
/// the recording it started is dropped silently (`input_hook.rs`).
pub(crate) fn key_chord(app: &AppHandle, at: u64) {
    with_pipeline(app, |p| p.shared.key_chord(at));
}

/// The input hook's stuck-key sweep let go of a hold-to-talk key whose
/// key-up it never heard: a recording it held goes on hands-free.
pub(crate) fn key_stuck(app: &AppHandle) {
    with_pipeline(app, |p| p.shared.key_lost());
}

/// **After a spoken "snip" landed in Chat** (`snip.rs` `listens_after`): a
/// Voice Assistant question starts by itself, as if its shortcut had been
/// pressed — the start chime, the capsule saying "Ask about this
/// screenshot…" — and takes the screenshot waiting in the box when it's
/// asked (`assistant.rs`). It ends as a spoken question does: either key
/// pressed (the dictation key that said "snip" too), ✓ on the capsule, or —
/// push-to-talk — a key pressed and let go; never on a pause. Esc or ✕
/// cancel it, and the screenshot stays in the box. `false`: nothing
/// started (a dictation records or transcribes, no microphone). A test run
/// asks the question it staged instead (`e2e_snip_question`).
pub(crate) fn listen_after_snip(app: &AppHandle) -> bool {
    let mut started = false;
    with_pipeline(app, |p| {
        #[cfg(debug_assertions)]
        if crate::shell::e2e::active() && !crate::shell::realtest::on() {
            started = p.robot().listen_after_snip_staged();
            return;
        }
        started = p.shared.listen_after_snip();
    });
    started
}

/// The Chat view's mic asked for the next dictation (`dictation::commands::chat_dictate`;
/// taken by the recording it starts, cleared right after).
static CHAT_ARMED: AtomicBool = AtomicBool::new(false);
/// The Chat view's box has the focus, in a focused window
/// (`dictation::commands::chat_input_focus`).
static CHAT_FOCUSED: AtomicBool = AtomicBool::new(false);

/// The next dictation goes to the chat box (its mic button), or not any more.
pub(crate) fn arm_chat_dictation(on: bool) {
    CHAT_ARMED.store(on, Ordering::SeqCst);
}

/// The chat box got or lost the focus.
pub(crate) fn set_chat_focus(focused: bool) {
    CHAT_FOCUSED.store(focused, Ordering::SeqCst);
}

/// Whether a dictation starting now is a question for the Chat view: its mic
/// asked for it, or the box has the focus while one of Yap's own windows is
/// in front (`own_window`: the foreground window is Yap's — a hotkey there,
/// the bar's Dictate). Never in edit mode. Pure.
pub(crate) fn chat_target(edit: bool, armed: bool, focused: bool, own_window: bool) -> bool {
    !edit && (armed || (focused && own_window))
}

/// The **Voice Assistant** shortcut asked for the next recording
/// (`Pipeline::on_assistant_key`; taken by the recording it starts, cleared
/// right after the press).
static ASSISTANT_ARMED: AtomicBool = AtomicBool::new(false);

/// Where a recording starting now goes. Pure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Goes {
    /// Pasted into the app in front (a dictation, a Command, a rewrite).
    Paste,
    /// Into the Chat view's box (`chat_target`).
    ChatBox,
    /// A question for the Voice Assistant (`assistant.rs`): answered out
    /// loud, never pasted.
    Assistant,
    /// Into the quick note at its cursor (`notes/quick_note.rs`): its window
    /// in front, or its mic button. Never pasted.
    QuickNote,
}

/// The edit hotkey's recording is always a rewrite; the assistant shortcut's
/// is always the assistant's, even with the chat box focused (its answer
/// lands in Chat anyway); the Chat view's mic is the chat's; then the quick
/// note's (`note`: its window in front, or its mic — `quick_note::takes_dictation`);
/// else as [`chat_target`] says. Pure.
pub(crate) fn destination(edit: bool, assistant: bool, chat_armed: bool, chat_focused: bool, own_window: bool, note: bool) -> Goes {
    if edit {
        Goes::Paste
    } else if assistant {
        Goes::Assistant
    } else if chat_armed {
        Goes::ChatBox
    } else if note {
        Goes::QuickNote
    } else if chat_target(edit, chat_armed, chat_focused, own_window) {
        Goes::ChatBox
    } else {
        Goes::Paste
    }
}

/// A dictation's own errors ("No microphone found", "Transcription
/// failed"…): the dictation overlay says them, where the dictation is, and
/// nothing else does (`notice.rs`: one notice, one place). Other errors
/// (`yap-error`) are the main window's toasts.
pub const DICTATION_ERROR: &str = "yap-dictation-error";

/// Test mode only (`e2e::e2e_dictation_mic`): the mic's audio arrives as
/// digital silence, as a muted mic's does — the dead-mic watchdog's case,
/// staged on the real microphone.
#[cfg(debug_assertions)]
static TEST_SILENT_MIC: AtomicBool = AtomicBool::new(false);

#[cfg(debug_assertions)]
pub(crate) fn set_test_silent_mic(silent: bool) {
    TEST_SILENT_MIC.store(silent, Ordering::SeqCst);
}

/// Test mode only (`e2e_target_window`): a stand-in for the window a
/// dictation goes into, so the last paste and the rewrite shortcut's choice
/// (`as_said.rs`) can be seen in a test run. Never a real window — a test
/// run reads and pastes into nothing, there or anywhere. 0 = none: the
/// window in front, as ever.
#[cfg(debug_assertions)]
static TEST_TARGET: AtomicIsize = AtomicIsize::new(0);

/// The stand-in's handle: no window has it.
#[cfg(debug_assertions)]
const TEST_STAND_IN: isize = 0x7E57_0001;

#[cfg(debug_assertions)]
pub(crate) fn set_test_target(on: bool) {
    TEST_TARGET.store(if on { TEST_STAND_IN } else { 0 }, Ordering::SeqCst);
}

/// The stand-in window, in a plain test run (never robot mode).
#[cfg(debug_assertions)]
pub(crate) fn test_target() -> Option<isize> {
    let h = TEST_TARGET.load(Ordering::SeqCst);
    (h != 0 && crate::shell::e2e::active() && !crate::shell::realtest::on()).then_some(h)
}

/// Robot mode only (`realtest.rs`): a WAV is playing into the dictation in
/// the mic's place — the real mic's audio is dropped meanwhile, and a
/// recording may start without one.
#[cfg(debug_assertions)]
static TEST_FEED: AtomicBool = AtomicBool::new(false);

/// A robot's WAV stands in for the mic right now (never in release builds).
fn test_feed_on() -> bool {
    #[cfg(debug_assertions)]
    {
        TEST_FEED.load(Ordering::SeqCst)
    }
    #[cfg(not(debug_assertions))]
    false
}

/// What a dictation sent to the paste — the robot's report
/// (`realtest.rs`); a spoken dictation ignores it.
#[derive(Clone, Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Delivered {
    /// The text as it went to the paste (fitted); empty: nothing to paste.
    pub text: String,
    /// The transcript before AI cleanup.
    pub raw: String,
    /// The kind of app it went into (`AppKind::describe`).
    pub kind: String,
    /// The key pressed after it ("enter", "ctrlEnter"…), if any.
    pub submit: Option<String>,
    /// Why the paste or the key didn't happen (a refusal in robot mode).
    pub error: Option<String>,
    /// Nothing was pasted, the text was left on the clipboard (`elevated.rs`:
    /// the app runs as administrator, …)…
    pub left_on_clipboard: bool,
    /// …and why: "admin", "keysBlocked" (Windows didn't take the keys),
    /// "noTextBox" (nothing that takes text had the focus): `Left::code`.
    pub left_why: Option<&'static str>,
    /// The keys that pasted it ("Ctrl+V", "Shift+Insert", "Ctrl+Shift+V":
    /// `AppKind::paste_key`).
    pub paste: String,
    /// The paste didn't reach the field (a Win32 edit control stayed as it
    /// was), so the text was typed into it instead.
    pub typed: bool,
    /// The words as said, fitted to the same place, when AI cleanup changed
    /// them: what "Use what I said" swaps the paste for (`as_said.rs`).
    pub as_said: Option<String>,
    /// The bar offers "Use what I said" for this one (its number).
    #[serde(skip)]
    pub offer: Option<u64>,
    /// The style AI cleanup wrote it in ("email/formal",
    /// `cleanup_styles.rs`), when it ran with one.
    pub style: Option<String>,
    /// A dictation pasted in steps (stacked messages, a mention or a file
    /// picked; `pasting/compose.rs`): each step as it went.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<crate::pasting::compose::Step>,
}

/// How long a cancelled dictation's audio is kept for **Undo** ("Dictation
/// cancelled · Undo" on the bar).
pub(crate) const UNDO_MS: u64 = 10_000;
/// A cancelled recording, kept a moment for Undo: its audio and everything
/// its paste needs — the window it was for, what kind of app that is, edit
/// mode and its selection.
struct Kept {
    id: u64,
    audio: Vec<f32>,
    target_hwnd: isize,
    target_kind: AppKind,
    edit: bool,
    selection: Option<String>,
    /// Edit mode's last dictation to rewrite (nothing was selected).
    rewrite_last: Option<crate::pasting::as_said::Pasted>,
    /// Its audio in `recent_audio` and its "Cancelled" entry in history
    /// (Home's Transcribe, for a day); Undo takes both back out.
    audio_id: Option<String>,
}

/// The latest dictation's report, for the robot's WAV dictations (which
/// deliver from `run_stt`).
#[cfg(debug_assertions)]
static LAST_DELIVERED: Mutex<Option<Delivered>> = Mutex::new(None);

/// The scrolling waveform's level meter: the peak over ~80 ms of audio, on
/// a perceptual scale (`meter.rs`).
#[derive(Default)]
struct AmpMeter {
    peak: f32,
    count: usize,
    /// The scale's floor, following the mic's noise.
    floor: crate::dictation::meter::Floor,
}

impl AmpMeter {
    /// Time-domain amplitude meter for the scrolling waveform (Claude Code
    /// style): one bar per ~1280 samples at 16 kHz ≈ 80 ms for a calm,
    /// readable scroll — the peak, in decibels over the mic's noise, gently
    /// compressed (`meter::Floor::level`: 0..0.92, a whisper about a third);
    /// the overlay pushes each onto a scrolling history. `Some(level)` when
    /// a bar is due.
    fn add(&mut self, samples: &[f32]) -> Option<f32> {
        const AMP_WINDOW: usize = 1280;
        for &s in samples {
            let a = s.abs();
            if a > self.peak {
                self.peak = a;
            }
        }
        self.count += samples.len();
        if self.count >= AMP_WINDOW {
            let peak = self.peak;
            self.peak = 0.0;
            self.count = 0;
            Some(self.floor.level(peak))
        } else {
            None
        }
    }
}

/// A recording's start chime (and the other apps going quiet after it),
/// held until the mic delivers real audio: people speak at the chime, so
/// it never plays before the mic is on (`mic.rs`).
struct PendingStart {
    session: u64,
    /// `mute::begin`'s token and plan.
    token: u64,
    plan: crate::dictation::mute::Plan,
}

/// The push-to-talk gesture (`gesture.rs`) and which hotkey its recording
/// belongs to.
#[derive(Default)]
struct GestureSlot {
    machine: Gesture,
    /// The recording is the edit hotkey's (no coaching hints for it).
    edit: bool,
    /// …or the assistant shortcut's (no coaching hints for it either: they
    /// name the dictation hotkey).
    assistant: bool,
    /// Toggle mode: the press that started the recording now (when, and
    /// the recording's `rec_session`) — another key joining it within a
    /// second makes it a chord (`key_chord`).
    toggle_press: Option<(u64, u64)>,
}

/// State shared between the audio callback, the hotkey toggle, and STT.
struct Shared {
    recording: AtomicBool,
    buffer: Mutex<Vec<f32>>,
    /// Rolling ring of the most recent ~300 ms of mic audio, maintained while
    /// *idle* — only while the mic is warm (`mic.rs`) — so `start_recording`
    /// can seed the buffer with the moment before the keypress (anti
    /// first-word-clipping). Capped at `PREROLL_SAMPLES`.
    preroll: Mutex<VecDeque<f32>>,
    /// A recording is starting (`start_recording`, before `recording`): the
    /// mic's audio goes straight into the buffer, all of it — with a cold
    /// mic it's the first audio the new stream delivers.
    gather: AtomicBool,
    /// The dictation's microphone, opened on demand (`mic.rs`).
    mic: crate::dictation::mic::Mic,
    /// The recording's hold on the mic (`Mic::hold`), released after its
    /// stop chime.
    mic_lease: AtomicU64,
    /// The mic couldn't be opened for this lease (a recording that hadn't
    /// started yet ends at once when it does).
    mic_failed_lease: AtomicU64,
    /// The waveform's level meter for the mic's callback.
    amp: Mutex<AmpMeter>,
    /// The recording waiting for its first real audio to play the start
    /// chime (its session; 0: none), and its chime.
    pending_session: AtomicU64,
    pending_start: Mutex<Option<PendingStart>>,
    /// Itself, for the mic's news (`mic::Sink`, called with `&Shared`).
    me: Weak<Shared>,
    engine: EngineSlot,
    app: AppHandle,
    config: RwLock<YapConfig>,
    /// Last time the engine did real work (ms since epoch). Drives the idle
    /// model-unload watcher (B1).
    last_activity: AtomicU64,
    /// Foreground window handle captured at record-start (0 = none / our own
    /// window). The transcript is pasted back into this window so focus changes
    /// during transcription don't misfire.
    target_hwnd: AtomicIsize,
    /// What kind of app `target_hwnd` is (a terminal, a chat app…; read at
    /// record start, `app_kind::of_window`), for fitting the paste.
    target_kind: Mutex<AppKind>,
    /// Whether the current recording session is **edit/rewrite mode** (the spoken
    /// words are an instruction to rewrite `selection`) rather than dictation.
    /// Set at record-start, read in `run_stt`.
    edit_mode: AtomicBool,
    /// The current dictation is a question for the **Chat** view's box
    /// ([`chat_target`], decided at record start): its text goes to the page
    /// (`yap-chat-dictation`) instead of a paste, never to the wake word.
    to_chat: AtomicBool,
    /// The current dictation is for the **quick note** ([`destination`], at
    /// record start): its text goes to the quick note's page
    /// (`yap-quick-note-dictation`) at its cursor, never a paste or the wake
    /// word. Kept through a cancel, so Undo lands there too.
    to_note: AtomicBool,
    /// The current recording is a question for the **Voice Assistant**
    /// ([`destination`], at record start): transcribed, corrected and handed
    /// to `assistant::heard` — no cleanup, wake word, paste or history.
    assistant: AtomicBool,
    /// Text selected in the target app, captured at edit-mode record-start. Empty
    /// (`None`) → "write mode" (generate new text from the instruction alone)
    /// — unless `rewrite_last`.
    selection: Mutex<Option<String>>,
    /// Edit mode with nothing selected, right after a paste into the same
    /// window (`as_said::rewrite_target`, at record start): the instruction
    /// rewrites that paste — the last dictation — in place.
    rewrite_last: Mutex<Option<crate::pasting::as_said::Pasted>>,
    /// True from `stop_and_transcribe` until `run_stt` finishes. Blocks starting a
    /// new recording while one is still transcribing — a rapid re-toggle would
    /// otherwise spawn a second `run_stt` that finds the engine taken and rebuilds
    /// a *duplicate* model into VRAM.
    processing: AtomicBool,
    /// Cancel flag for an in-flight Upload file transcription (checked between
    /// chunks — a running chunk finishes, then the run stops).
    upload_cancel: AtomicBool,
    /// Mic-test mode (onboarding "speak and see the waves react"): when set, the
    /// audio callback emits `yap-amp` while *idle* too, so a level meter works
    /// without recording. Normally amp is only emitted during a recording.
    /// It holds the mic open meanwhile (`Mic::test`).
    mic_test: AtomicBool,
    /// Push-to-talk's hold / tap / double-tap state. Locked while acting on
    /// a step, so key events and the tap timer take turns.
    gesture: Mutex<GestureSlot>,
    /// The current recording's live audio, for the dead-mic watchdog
    /// (`mic_watch.rs`), tallied by the audio callback.
    mic_tally: MicTally,
    /// The watchdog told the user this recording's mic gives nothing.
    mic_dead: AtomicBool,
    /// Bumped by every `start_recording`: a recording's watcher stops when
    /// a newer one starts.
    rec_session: AtomicU64,
    /// The name of the device the mic stream captures from.
    mic_name: Mutex<String>,
    /// The last cancelled recording, for Undo (dropped after [`UNDO_MS`],
    /// or when another dictation starts).
    kept: Mutex<Option<Kept>>,
    /// Numbers the kept recordings, so an old expiry timer leaves a newer
    /// one alone.
    kept_seq: AtomicU64,
    /// The dictation being delivered is an Undo: its paste goes back to its
    /// own window or nowhere (`inject_text`'s `strict`).
    undoing: AtomicBool,
    /// The transcribing dictation's audio, kept a day to run it again
    /// (`recent_audio.rs`): taken by its history entry, else dropped.
    audio_id: Mutex<Option<String>>,
    /// The dictation ended because its microphone was disconnected
    /// (`unplugged.rs`): its paste goes to its own window or the clipboard,
    /// never another, and the person is told once, by what became of it.
    /// Cleared when the next one starts.
    unplugged: Mutex<Option<unplugged::Lost>>,
    /// The bar's **Skip cleanup**, while a dictation's AI cleanup takes
    /// longer than usual (`pipeline/slow.rs`).
    skip_cleanup: tokio::sync::Notify,
    /// The dictation being transcribed was stopped at its length limit
    /// (`cap.rs`): its paste goes back to its own window or onto the
    /// clipboard (as an Undo's), and an alert offers Recover text.
    capped: AtomicBool,
}

/// Current wall-clock time in milliseconds since the Unix epoch.
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Map a `model_unload_timeout` config string to a duration. `"never"` (and any
/// unknown value) means "don't unload" → `None`.
fn unload_timeout_duration(s: &str) -> Option<Duration> {
    match s {
        "1min" => Some(Duration::from_secs(60)),
        "5min" => Some(Duration::from_secs(5 * 60)),
        "15min" => Some(Duration::from_secs(15 * 60)),
        "30min" => Some(Duration::from_secs(30 * 60)),
        _ => None,
    }
}

impl Shared {
    fn sound_enabled(&self) -> bool {
        self.config.read().map(|c| c.sound_enabled).unwrap_or(true)
    }

    fn audio_feedback_volume(&self) -> f32 {
        self.config
            .read()
            .map(|c| c.audio_feedback_volume)
            .unwrap_or(1.0)
    }

    fn recording_mode(&self) -> String {
        self.config
            .read()
            .map(|c| c.recording_mode.clone())
            .unwrap_or_else(|_| "toggle".into())
    }

    /// What happens to other apps' sound while dictating (`mute.rs`).
    fn other_apps(&self) -> crate::dictation::mute::Plan {
        self.config
            .read()
            .map(|c| crate::dictation::mute::Plan { quiet: c.quiet_other_apps, pause: c.pause_media })
            .unwrap_or_default()
    }

    fn output_device(&self) -> Option<String> {
        self.config.read().ok().and_then(|c| c.output_device.clone())
    }

    /// The kind of app this recording goes into, as read at record start —
    /// what the paste keys follow (`AppKind::paste_key`: a terminal's are
    /// its own whatever the settings say).
    fn read_kind(&self) -> AppKind {
        self.target_kind.lock().map(|k| *k).unwrap_or_default()
    }

    /// The kind of app this recording goes into (read at record start), as
    /// the settings have Yap treat it (`AppKind::with_settings`).
    fn paste_kind(&self) -> AppKind {
        let kind = self.read_kind();
        self.config
            .read()
            .map(|c| kind.with_settings(c.terminal_mode, c.chat_no_full_stop))
            .unwrap_or_default()
    }

    /// Reset the idle timer to "now".
    fn touch_activity(&self) {
        self.last_activity.store(now_ms(), Ordering::Relaxed);
    }

    /// Idle-unload check, called periodically by the watcher thread.
    ///
    /// Drops the warm engine (freeing VRAM) once it's been idle longer than the
    /// configured timeout. Never unloads while recording; the next dictation
    /// lazily reloads the model in `run_stt`.
    fn maybe_unload_idle(&self) {
        let timeout = match self
            .config
            .read()
            .ok()
            .and_then(|c| unload_timeout_duration(&c.model_unload_timeout))
        {
            Some(t) => t,
            None => return, // "never"
        };

        // Keep the timer fresh while recording so we never unload mid-session.
        if self.recording.load(Ordering::SeqCst) {
            self.touch_activity();
            return;
        }

        let idle_ms = now_ms().saturating_sub(self.last_activity.load(Ordering::Relaxed));
        if idle_ms < timeout.as_millis() as u64 {
            return;
        }

        if let Ok(mut g) = self.engine.lock() {
            if g.is_some() {
                *g = None;
                tracing::info!(idle_secs = idle_ms / 1000, "Model unloaded due to inactivity");
            }
        }
    }

    /// Snapshot the language/translate settings for a transcription.
    /// `"auto"` (or a model that ignores it) maps to `None`.
    fn language_settings(&self) -> (Option<String>, bool) {
        self.config
            .read()
            .map(|c| {
                let lang = if c.selected_language == "auto" {
                    None
                } else {
                    Some(c.selected_language.clone())
                };
                (lang, c.translate_to_english)
            })
            .unwrap_or((None, false))
    }

    /// The dictionary vocabulary as a Whisper `initial_prompt` bias (None when
    /// the dictionary is empty), the words used most lately first
    /// (`dict_usage::ranked`). Snapshotted per transcription like
    /// `language_settings`; only Whisper engines consume it.
    fn dictionary_prompt(&self) -> Option<String> {
        let (dict, name) = self.config.read().ok().map(|c| (c.dictionary.clone(), c.about_name.clone()))?;
        // The dictionary's words, then your name and the names Yap
        // remembers (memory.rs).
        config::dictionary_prompt(&crate::dictionary::dict_usage::ranked(&crate::memory::with_names(&dict, &name)))
    }

    /// A dictation's Whisper prompt: [`Self::dictionary_prompt`]'s words,
    /// then the names on screen, if they've been read by now
    /// (`screen_terms`; never waited for).
    fn dictation_prompt(&self) -> Option<String> {
        let (dict, name) = self.config.read().ok().map(|c| (c.dictionary.clone(), c.about_name.clone()))?;
        let words = crate::dictionary::dict_usage::ranked(&crate::memory::with_names(&dict, &name));
        crate::dictionary::screen_terms::whisper_prompt(words, crate::dictionary::screen_terms::peek().as_deref())
    }

}

// ---- A transcription's `processing` flag ---------------------------------------

/// The running pipeline: the shared state, with the mic (`mic.rs`) and the
/// warm speech engine in it.
pub struct Pipeline {
    shared: Arc<Shared>,
}

impl Pipeline {
    /// Start the pipeline; the speech model loads in the background
    /// (`model_loading`), the mic opens when something needs it (`mic.rs`;
    /// at once with "Keep the microphone ready").
    ///
    /// A missing model is tolerated: the pipeline still runs (so the hotkey
    /// works) and emits `yap-state: needs-model` until a model is downloaded.
    /// So is a missing microphone (none plugged in, a Bluetooth headset that
    /// connects after login, a CI runner): models, Upload and Settings work
    /// without one, and every recording tries to open it again.
    pub fn start(app: AppHandle, cfg: YapConfig) -> Self {
        let data_dir = config::data_dir();
        // (The stub build has every model: it never needs one.)
        let has_model = !cfg!(feature = "engines") || stt::is_model_installed(&data_dir, &cfg.model_size);

        let shared = Arc::new_cyclic(|me: &Weak<Shared>| Shared {
            recording: AtomicBool::new(false),
            buffer: Mutex::new(Vec::new()),
            preroll: Mutex::new(VecDeque::with_capacity(PREROLL_SAMPLES + 1)),
            gather: AtomicBool::new(false),
            mic: crate::dictation::mic::Mic::start(me.clone() as Weak<dyn crate::dictation::mic::Sink>),
            mic_lease: AtomicU64::new(0),
            mic_failed_lease: AtomicU64::new(0),
            amp: Mutex::new(AmpMeter::default()),
            pending_session: AtomicU64::new(0),
            pending_start: Mutex::new(None),
            me: me.clone(),
            engine: EngineSlot::default(),
            app: app.clone(),
            config: RwLock::new(cfg.clone()),
            last_activity: AtomicU64::new(now_ms()),
            target_hwnd: AtomicIsize::new(0),
            target_kind: Mutex::new(AppKind::Other),
            edit_mode: AtomicBool::new(false),
            to_chat: AtomicBool::new(false),
            to_note: AtomicBool::new(false),
            assistant: AtomicBool::new(false),
            selection: Mutex::new(None),
            rewrite_last: Mutex::new(None),
            processing: AtomicBool::new(false),
            upload_cancel: AtomicBool::new(false),
            mic_test: AtomicBool::new(false),
            gesture: Mutex::new(GestureSlot::default()),
            mic_tally: MicTally::default(),
            mic_dead: AtomicBool::new(false),
            rec_session: AtomicU64::new(0),
            mic_name: Mutex::new(String::new()),
            kept: Mutex::new(None),
            kept_seq: AtomicU64::new(0),
            undoing: AtomicBool::new(false),
            audio_id: Mutex::new(None),
            unplugged: Mutex::new(None),
            skip_cleanup: tokio::sync::Notify::new(),
            capped: AtomicBool::new(false),
        });

        spawn_idle_watcher(&shared);

        // The mic: closed until something needs it, unless "Keep the
        // microphone ready" (then open now, as Yap always was).
        shared.mic.settings(cfg.keep_mic_ready, cfg.mic_order.clone());
        // If Windows' privacy switches keep desktop apps off the mic, the log
        // says so now (a recording says it to the person).
        if let Some(blocked) = crate::dictation::mic_privacy::blocked() {
            tracing::warn!(?blocked, "Windows' privacy settings block the microphone for desktop apps");
        }

        let _ = app.emit("yap-state", if has_model { "idle" } else { "needs-model" });

        // The speech model, off the startup thread: the tray, the bar and the
        // window come up meanwhile, and a dictation started before it's
        // ready waits for it ("Loading the speech model…", `run_stt`).
        if has_model {
            set_model_loading(true);
            let loader = Arc::clone(&shared);
            let model = cfg.model_size.clone();
            let use_gpu = cfg.use_gpu;
            let spawned = std::thread::Builder::new().name("stt-load".into()).spawn(move || {
                // Test mode: a slow load, to see a dictation wait for it.
                #[cfg(debug_assertions)]
                if crate::shell::e2e::active() {
                    if let Some(ms) = std::env::var("YAP_E2E_MODEL_LOAD_MS").ok().and_then(|v| v.trim().parse::<u64>().ok()) {
                        tracing::info!(ms, "e2e: the speech model loads slowly");
                        std::thread::sleep(Duration::from_millis(ms));
                    }
                }
                let started = Instant::now();
                let loaded = stt::create_stt_engine(&config::data_dir(), &model, use_gpu);
                let ok = loaded.is_ok();
                match loaded {
                    Ok(e) => {
                        tracing::info!(model = %model, ms = started.elapsed().as_millis() as u64, "Speech model loaded in the background");
                        // A model switch meanwhile (`set_engine`) wins.
                        if let Ok(mut g) = loader.engine.lock() {
                            if g.is_none() {
                                *g = Some(e);
                            }
                        }
                        loader.touch_activity();
                    }
                    Err(SttError::ModelNotFound(_)) => {
                        tracing::warn!("STT model not found — Yap needs a model download");
                    }
                    Err(e) => tracing::error!("STT engine init failed: {}", e),
                }
                set_model_loading(false);
                // Nothing to wait for: a dictation that would have needs the
                // model (`run_stt` says so).
                let quiet = !loader.recording.load(Ordering::SeqCst) && !loader.processing.load(Ordering::SeqCst);
                let engine_in = loader.engine.lock().map(|g| g.is_some()).unwrap_or(false);
                if quiet && !ok && !engine_in {
                    let _ = loader.app.emit("yap-state", "needs-model");
                }
            });
            if let Err(e) = spawned {
                set_model_loading(false);
                tracing::error!("No thread to load the speech model on: {}", e);
            }
        } else {
            tracing::warn!("STT model not found — Yap needs a model download");
        }

        Self { shared }
    }

    /// Toggle recording (the `toggle_recording` command).
    pub fn toggle(&self) {
        self.shared.toggle(false);
    }

    /// The Chat view's mic: start a dictation whose text goes to the chat
    /// box (or stop the one recording).
    pub fn toggle_for_chat(&self) {
        let starting = !self.shared.recording.load(Ordering::SeqCst);
        arm_chat_dictation(starting);
        self.toggle();
        // Taken by the recording if it started; never left for another one.
        arm_chat_dictation(false);
    }

    /// The quick note's mic: start a dictation whose text goes into the
    /// note (or stop the one recording).
    pub fn toggle_for_quick_note(&self) {
        let starting = !self.shared.recording.load(Ordering::SeqCst);
        crate::notes::quick_note::arm(starting);
        self.toggle();
        crate::notes::quick_note::arm(false);
    }

    /// Route a **dictation** hotkey press/release through the recording mode.
    /// Called from the input-hook listeners for both press and release (`at`:
    /// the hook's timestamp, `gesture::now_ms`) and from the windows' in-page
    /// fallback (`dictation_key`, `None` = now).
    pub fn on_key(&self, pressed: bool, at: Option<u64>) {
        self.shared.on_key(pressed, false, at);
    }

    /// Route an **edit/rewrite** hotkey press/release. Same as `on_key` but the
    /// session captures the selection and rewrites it from the spoken instruction.
    pub fn on_edit_key(&self, pressed: bool, at: Option<u64>) {
        self.shared.on_key(pressed, true, at);
    }

    /// Route a **Voice Assistant** shortcut press/release (the hook, or a Yap
    /// window's in-page fallback): the same recording mode and gestures as
    /// the dictation key, but the recording a press starts is a question for
    /// Yap's Chat, answered out loud (`assistant.rs`). A press while Yap is
    /// speaking stops the voice first (`start_recording`'s barge-in); a
    /// press while any recording runs stops it, as the other keys do.
    pub fn on_assistant_key(&self, pressed: bool, at: Option<u64>) {
        self.shared.assistant_key(pressed, at);
    }

    /// Route a **language shortcut**'s press/release (My languages,
    /// `languages.rs`; the hook's `language-key`, or a Yap window's in-page
    /// fallback): the dictation key's recording mode and gestures, the
    /// recording a press starts in the shortcut's language (and translated,
    /// when it says so). A press while a recording runs stops it, as the
    /// other keys do.
    pub fn on_language_key(&self, id: &str, pressed: bool, at: Option<u64>) {
        crate::dictation::languages::arm(pressed.then_some(id));
        self.shared.on_key(pressed, false, at);
        // Taken by the recording if one started; never left for another.
        crate::dictation::languages::arm(None);
    }

    /// Cancel the recording (the bar's ✕, the tray's Cancel): nothing is
    /// transcribed, and its audio is kept a moment for **Undo**.
    pub fn cancel(&self) {
        self.shared.cancel(true);
    }

    /// Stop the recording and paste it, as a second press of the hotkey does
    /// (the bar's ✓). Into the window the dictation started in.
    pub fn stop(&self) {
        if self.shared.recording.load(Ordering::SeqCst) {
            self.shared.stop_and_transcribe();
        }
    }

    /// **Undo** the last cancel: transcribe its kept audio and paste it into
    /// the window it was for. Errors once the audio is gone.
    pub fn undo(&self) -> Result<(), String> {
        self.shared.undo()
    }

    /// **Skip cleanup** (the bar, while a dictation's AI cleanup takes
    /// longer than usual): the words as transcribed are pasted now.
    pub fn skip_cleanup(&self) {
        self.shared.skip_cleanup_now();
    }

    /// Recording, or transcribing a dictation / an Upload file — the updater
    /// waits for this to clear before restarting Yap.
    pub fn is_busy(&self) -> bool {
        self.shared.recording.load(Ordering::SeqCst)
            || self.shared.processing.load(Ordering::SeqCst)
    }

    /// A cancelled dictation is waiting for Undo (the updater doesn't
    /// restart Yap under it: its "idle" afterwards lets it go ahead).
    pub fn holds_undo(&self) -> bool {
        self.shared.kept.lock().map(|k| k.is_some()).unwrap_or(false)
    }

    /// Install a freshly-created STT engine (e.g. after a model download).
    /// A model new to this PC gets its devices timed soon (`stt_speed`).
    pub fn set_engine(&self, engine: SttAdapter) {
        if let Ok(mut g) = self.shared.engine.lock() {
            *g = Some(engine);
        }
        let _ = self.shared.app.emit("yap-state", "idle");
        crate::dictation::stt_speed::schedule(&self.shared.app, crate::dictation::stt_speed::AFTER_CHANGE);
    }

    /// The same model on another device (`stt_speed`: a new pick, or "Runs
    /// on" changed), swapped in without a word.
    pub(crate) fn replace_engine(&self, engine: SttAdapter) {
        if let Ok(mut g) = self.shared.engine.lock() {
            *g = Some(engine);
        }
    }

    /// The warm engine's model and device, if one is loaded.
    pub(crate) fn engine_info(&self) -> Option<(String, stt::Device)> {
        let g = self.shared.engine.lock().ok()?;
        g.as_ref().map(|e| (e.model().to_string(), e.device()))
    }

    /// What a dictation passes the engine besides its audio — the language,
    /// translate, and the Whisper vocabulary prompt — so the speech-engine
    /// timing makes the same call (`stt_speed`).
    pub(crate) fn transcribe_options(&self) -> (Option<String>, bool, Option<String>) {
        let (language, translate) = self.shared.language_settings();
        (language, translate, self.shared.dictionary_prompt())
    }

    /// How long since a dictation last did anything (ms): the speech-engine
    /// timing waits for a quiet moment.
    pub(crate) fn idle_ms(&self) -> u64 {
        now_ms().saturating_sub(self.shared.last_activity.load(Ordering::Relaxed))
    }

    /// Clone of the shared warm-engine slot for the meeting recorder (see
    /// `EngineSlot`).
    pub(crate) fn engine_slot(&self) -> EngineSlot {
        std::sync::Arc::clone(&self.shared.engine)
    }

    /// Start an Upload file transcription (async — progress + result arrive via
    /// `yap-upload-*` events). Refuses while a dictation is recording, and holds
    /// the `processing` guard for the whole run so the hotkey can't start a
    /// dictation that would fight over the engine.
    pub fn transcribe_file(&self, path: String) -> Result<(), String> {
        let shared = Arc::clone(&self.shared);
        if shared.recording.load(Ordering::SeqCst) {
            return Err("Recording in progress — stop dictating first".into());
        }
        let Some(busy) = Busy::take(&shared, Job::Upload) else {
            return Err("Yap is busy transcribing — try again in a moment".into());
        };
        shared.upload_cancel.store(false, Ordering::SeqCst);
        spawn_busy(busy, async move { shared.run_file_transcription(path).await });
        Ok(())
    }

    /// Cancel an in-flight Upload transcription (takes effect between chunks).
    pub fn cancel_file_transcription(&self) {
        self.shared.upload_cancel.store(true, Ordering::SeqCst);
    }

    /// Replace the live config (e.g. after the user saves settings).
    pub fn update_config(&self, cfg: YapConfig) {
        // "Keep the microphone ready" and the microphones' order, for the mic.
        self.shared.mic.settings(cfg.keep_mic_ready, cfg.mic_order.clone());
        if let Ok(mut c) = self.shared.config.write() {
            *c = cfg;
        }
    }

    /// Enable/disable mic-test mode: while on, the mic is open and the audio
    /// callback emits `yap-amp` levels even when idle (onboarding's "see the
    /// waves react").
    pub fn set_mic_test(&self, on: bool) {
        self.shared.mic_test.store(on, Ordering::Relaxed);
        self.shared.mic.test(on);
    }

    /// The mic's state (`mic.rs`): open, live, Bluetooth, how the last open
    /// went — robot mode's `realtest_mic`.
    #[cfg(debug_assertions)]
    pub(crate) fn mic_status(&self) -> crate::dictation::mic::Status {
        self.shared.mic.status()
    }

    /// Robot mode's way into the pipeline (`realtest.rs`).
    #[cfg(debug_assertions)]
    pub(crate) fn robot(&self) -> Robot {
        Robot(Arc::clone(&self.shared))
    }

    /// Home's Run again (`recent_audio::history_rerun`).
    pub(crate) fn rerunner(&self) -> Rerun {
        Rerun(Arc::clone(&self.shared))
    }

    /// Saved transforms (`transforms.rs`).
    pub(crate) fn transformer(&self) -> Transformer {
        Transformer(Arc::clone(&self.shared))
    }
}

/// Runs a saved transform (`transforms.rs`) never beside a dictation: it
/// holds the `processing` guard meanwhile, so no dictation starts (or
/// pastes) while a transform reads, asks and pastes, and the updater waits.
pub(crate) struct Transformer(Arc<Shared>);

impl Transformer {
    /// The live settings.
    pub(crate) fn config(&self) -> YapConfig {
        self.0.config.read().map(|c| c.clone()).unwrap_or_default()
    }

    /// `work` with the `processing` guard held (let go however it ends; a
    /// panic in it says so on the bar). `Err`: a dictation records or
    /// transcribes.
    pub(crate) fn hold<T>(&self, work: impl FnOnce(&YapConfig) -> T) -> Result<T, &'static str> {
        let shared = &self.0;
        let busy = (!shared.recording.load(Ordering::SeqCst)).then(|| Busy::take(shared, Job::Transform)).flatten();
        let Some(busy) = busy else {
            return Err("Yap is busy with a dictation — try again in a moment");
        };
        let cfg = self.config();
        let out = work(&cfg);
        busy.done();
        Ok(out)
    }
}

/// Runs a kept dictation's audio again ([`Pipeline::rerunner`]), never
/// beside a dictation: it holds the `processing` guard meanwhile.
pub(crate) struct Rerun(Arc<Shared>);

/// What [`Rerun::run`] made of a kept dictation.
pub(crate) struct Reran {
    /// The transcript.
    pub raw: String,
    /// After AI cleanup, the corrections and snippets.
    pub text: String,
    /// The words as said, when cleanup changed them (`as_said_text`).
    pub said: Option<String>,
    /// The model id that ran it.
    pub model: String,
}

impl Rerun {
    /// See `Shared::rerun`.
    pub(crate) async fn run(&self, audio: Vec<f32>, model: Option<String>, app_name: &str) -> Result<Reran, String> {
        let shared = &self.0;
        let busy = (!shared.recording.load(Ordering::SeqCst)).then(|| Busy::take(shared, Job::Rerun)).flatten();
        let Some(busy) = busy else {
            return Err("Yap is busy with a dictation — try again in a moment".into());
        };
        let out = shared.rerun(audio, model, app_name).await;
        busy.done();
        out
    }
}

/// Spawn the idle model-unload watcher (B1).
///
/// Ticks every 10s and unloads the warm engine once it's been idle longer than
/// the configured `model_unload_timeout` (freeing VRAM). Holds only a `Weak`
/// reference, so it exits on its own once the pipeline is dropped.
fn spawn_idle_watcher(shared: &Arc<Shared>) {
    let weak: Weak<Shared> = Arc::downgrade(shared);
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(10));
        match weak.upgrade() {
            Some(shared) => shared.maybe_unload_idle(),
            None => break, // pipeline dropped
        }
    });
}

/// Simple linear resampler from one rate to another. Shared with `media.rs`
/// (audio-file decode front-end).
pub(crate) fn resample_linear(input: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if input.is_empty() || from_rate == to_rate {
        return input.to_vec();
    }
    let ratio = from_rate as f64 / to_rate as f64;
    let out_len = ((input.len() as f64) / ratio).floor() as usize;
    let mut output = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src_idx = i as f64 * ratio;
        let idx0 = src_idx.floor() as usize;
        let idx1 = (idx0 + 1).min(input.len() - 1);
        let frac = (src_idx - idx0 as f64) as f32;
        output.push(input[idx0] * (1.0 - frac) + input[idx1] * frac);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn which_dictations_are_questions_for_the_chat() {
        // The chat's mic: wherever the focus is.
        assert!(chat_target(false, true, false, false));
        // The hotkey (or the bar) while the chat box has the focus in Yap's window.
        assert!(chat_target(false, false, true, true));
        // The box was focused, but another app is in front (a stale focus, the global hook).
        assert!(!chat_target(false, false, true, false));
        // Yap in front, the box not focused (Notes, Settings…).
        assert!(!chat_target(false, false, false, true));
        // Edit mode rewrites a selection, never a chat question.
        assert!(!chat_target(true, true, true, true));
    }

    #[test]
    fn the_assistant_shortcuts_recordings_are_the_assistants() {
        // Wherever the focus is, the chat box's included: the answer lands
        // in Chat anyway, and is read aloud.
        assert_eq!(destination(false, true, false, false, false, false), Goes::Assistant);
        assert_eq!(destination(false, true, true, true, true, true), Goes::Assistant);
        // The edit hotkey's recording stays a rewrite.
        assert_eq!(destination(true, true, false, false, false, false), Goes::Paste);
        assert_eq!(destination(true, false, false, false, true, true), Goes::Paste);
        // Without the shortcut: as before.
        assert_eq!(destination(false, false, true, false, false, false), Goes::ChatBox);
        assert_eq!(destination(false, false, false, true, true, false), Goes::ChatBox);
        assert_eq!(destination(false, false, false, true, false, false), Goes::Paste);
        assert_eq!(destination(false, false, false, false, false, false), Goes::Paste);
    }

    #[test]
    fn a_dictation_with_the_quick_note_in_front_is_the_notes() {
        // Its window in front (or its mic): the note's, never a paste.
        assert_eq!(destination(false, false, false, false, true, true), Goes::QuickNote);
        // A chat box focused a moment ago in the main window: still the note's.
        assert_eq!(destination(false, false, false, true, true, true), Goes::QuickNote);
        // The Chat view's own mic, the assistant's shortcut and a rewrite win.
        assert_eq!(destination(false, false, true, false, true, true), Goes::ChatBox);
        assert_eq!(destination(false, true, false, false, true, true), Goes::Assistant);
        assert_eq!(destination(true, false, false, false, true, true), Goes::Paste);
    }

    #[test]
    fn the_dictation_signal_counts_dictations_and_says_which_is_live() {
        // One test owns the global (others don't touch it).
        let (before, _) = dictation_state();
        dictation_began();
        assert_eq!(dictation_state(), (before + 1, true));
        dictation_ended();
        assert_eq!(dictation_state(), (before + 1, false));
        // Ending twice (a cancel after a stop) changes nothing.
        dictation_ended();
        assert_eq!(dictation_state(), (before + 1, false));
        dictation_began();
        dictation_began(); // never happens, but stays one live dictation
        assert_eq!(dictation_state(), (before + 3, true));
        dictation_ended();
        assert_eq!(dictation_state(), (before + 3, false));
    }
}
