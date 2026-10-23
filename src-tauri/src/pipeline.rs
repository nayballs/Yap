//! Yap's dictation pipeline.
//!
//! Simpler than Voice Mirror's (no TTS, no AI routing, no modes, no VAD —
//! only a Voice Assistant question ends on a pause, `pause.rs`):
//! the mic (`mic.rs`, opened on demand) captures audio into a buffer while
//! *recording* is on; toggling off runs STT, applies the dictionary, and
//! injects the text into whatever window is focused. A short chime marks
//! start/stop.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock, Weak};
use std::time::{Duration, Instant, SystemTime};

use tauri::{AppHandle, Emitter};

use crate::app_kind::AppKind;
use crate::config::{self, YapConfig};
use crate::gesture::{Action, Gesture};
use crate::mic_watch::{DeadMicWatch, MicSilent, MicStep, MicTally};
use crate::stt::{self, SttAdapter, SttError};

pub(crate) const TARGET_SAMPLE_RATE: u32 = 16_000;

/// The shared warm-engine slot. The pipeline owns it; the meeting recorder
/// (`meeting.rs`) clones the Arc and takes/returns the engine per chunk, so
/// dictation and meeting transcription interleave instead of double-loading
/// the model.
pub(crate) type EngineSlot = std::sync::Arc<Mutex<Option<SttAdapter>>>;

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

/// The Chat view's mic asked for the next dictation (`commands::chat_dictate`;
/// taken by the recording it starts, cleared right after).
static CHAT_ARMED: AtomicBool = AtomicBool::new(false);
/// The Chat view's box has the focus, in a focused window
/// (`commands::chat_input_focus`).
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
}

/// The edit hotkey's recording is always a rewrite; the assistant shortcut's
/// is always the assistant's, even with the chat box focused (its answer
/// lands in Chat anyway); else as [`chat_target`] says. Pure.
pub(crate) fn destination(edit: bool, assistant: bool, chat_armed: bool, chat_focused: bool, own_window: bool) -> Goes {
    if edit {
        Goes::Paste
    } else if assistant {
        Goes::Assistant
    } else if chat_target(edit, chat_armed, chat_focused, own_window) {
        Goes::ChatBox
    } else {
        Goes::Paste
    }
}

/// Run the correction dictionary over a transcript, the same way for a
/// dictation, a meeting segment and an Upload: exact replacements
/// (`config::apply_dictionary`), then the fuzzy near-miss pass (`fuzzy.rs`)
/// when "Catch near-misses" is on and the model isn't Whisper. (Whisper got
/// the vocabulary as its `initial_prompt` instead: Handy's split.) Entries
/// opted out of the fuzzy pass (`DictionaryEntry::fuzzy`) are exact-only.
/// Snippets aren't corrections and are left alone (a dictation expands them
/// afterwards, [`finish_dictation`]). The dictionary words in the result
/// count as used, for the Whisper prompt's ranking (`dict_usage`).
pub(crate) fn apply_corrections(text: &str, cfg: &YapConfig) -> String {
    let corrected = corrections(text, cfg);
    crate::dict_usage::record(&crate::memory::with_names(&cfg.dictionary), &corrected);
    corrected
}

/// [`apply_corrections`] without counting the words as used (the words as
/// said, beside the cleaned text that already counted them).
fn corrections(text: &str, cfg: &YapConfig) -> String {
    let corrected = config::apply_dictionary(text, &cfg.dictionary);
    if cfg.dictionary_fuzzy && !stt::is_whisper_model(&cfg.model_size) {
        let fixed = crate::fuzzy::apply_fuzzy(&corrected, &cfg.dictionary);
        // The names Yap remembers (memory/vocab.rs), more carefully: only
        // where what was heard isn't an ordinary word itself.
        let names: Vec<_> = crate::memory::dictation_names(&cfg.dictionary).into_iter().filter(|e| e.fuzzy).collect();
        if names.is_empty() {
            fixed
        } else {
            let words = crate::memory::Words::new();
            crate::fuzzy::apply_names(&fixed, &names, &|w| words.is(w))
        }
    } else {
        corrected
    }
}

/// A dictation's words **as said** ("Use what I said", `as_said.rs`): the
/// transcript (`raw`) through the corrections, the names on screen
/// (`screen`, when the dictation read them), the "press enter" command and
/// snippets — everything [`finish_dictation_with`] does but the AI cleanup
/// before it.
pub(crate) fn as_said_text(raw: &str, cfg: &YapConfig, screen: Option<&crate::screen_terms::ScreenTerms>) -> String {
    let corrected = crate::screen_terms::correct(&corrections(raw.trim(), cfg), screen, cfg);
    let text = cfg.voice_enter.then(|| take_voice_enter(&corrected, raw)).flatten().unwrap_or(corrected);
    config::apply_snippets(&text, &cfg.dictionary)
}

/// A dictation's text after AI cleanup (`cleaned`; `raw` = the transcript
/// before it): the corrections ([`apply_corrections`]), then the "press
/// enter" voice command when `voice_enter` is on ([`take_voice_enter`]),
/// then snippets (`config::apply_snippets` — after the command is off, so
/// "my address, press enter" still gives exactly the address). Returns the
/// text to paste and whether the voice command asked for the submit key.
/// (A dictation goes through [`finish_dictation_with`]; a dictation run
/// again from its kept audio comes here, its screen long gone.)
pub(crate) fn finish_dictation(cleaned: &str, raw: &str, cfg: &YapConfig) -> (String, bool) {
    finish_dictation_with(cleaned, raw, cfg, None)
}

/// [`finish_dictation`] with the names on screen (`screen_terms`): after
/// the corrections (the dictionary and memory have the final say), a name
/// heard a little wrong is spelled as the screen shows it — before the
/// voice command and snippets, whose text is never touched.
pub(crate) fn finish_dictation_with(
    cleaned: &str,
    raw: &str,
    cfg: &YapConfig,
    screen: Option<&crate::screen_terms::ScreenTerms>,
) -> (String, bool) {
    let corrected = crate::screen_terms::correct(&apply_corrections(cleaned, cfg), screen, cfg);
    let (text, enter) = match cfg.voice_enter.then(|| take_voice_enter(&corrected, raw)).flatten() {
        Some(text) => (text, true),
        None => (corrected, false),
    };
    (config::apply_snippets(&text, &cfg.dictionary), enter)
}

/// The "press enter" voice command: a dictation that ENDS with "press enter"
/// or "press return" (any case, any punctuation around it) is sent. Returns
/// the text to paste when it was said — `text` with the command taken off,
/// along with a comma or dash before it; a full stop or question mark after
/// it moves onto the text ("Can you check, press enter?" → "Can you
/// check?"). The command only counts as the last two words, never
/// mid-sentence. When `text` (the final text) doesn't end with it but `raw`
/// (the transcript before AI cleanup) does, cleanup dropped or reworded it:
/// `text` is pasted as it is and the key still pressed. `None`: not said.
pub(crate) fn take_voice_enter(text: &str, raw: &str) -> Option<String> {
    if let Some((start, end)) = trailing_enter_command(text) {
        let kept = text[..start].trim_end_matches(|c: char| {
            c.is_whitespace() || matches!(c, ',' | ';' | ':' | '-' | '–' | '—')
        });
        let mut out = kept.to_string();
        let ends_sentence = kept.ends_with(['.', '!', '?', '…']);
        if !kept.is_empty() && !ends_sentence {
            if let Some(stop) = text[end..].chars().find(|c| matches!(c, '.' | '!' | '?')) {
                out.push(stop);
            }
        }
        return Some(out);
    }
    trailing_enter_command(raw).map(|_| text.to_string())
}

/// Where a trailing "press enter" / "press return" is in `text`: the byte
/// range from "press" to the end of the last word (only punctuation and
/// spaces may follow it).
fn trailing_enter_command(text: &str) -> Option<(usize, usize)> {
    // The last two words: runs of letters and digits.
    let mut words: Vec<(usize, usize)> = Vec::new();
    let mut start = None;
    for (i, c) in text.char_indices() {
        match (c.is_alphanumeric(), start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                words.push((s, i));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        words.push((s, text.len()));
    }
    let [.., (p0, p1), (k0, k1)] = words[..] else {
        return None;
    };
    let key = &text[k0..k1];
    (text[p0..p1].eq_ignore_ascii_case("press")
        && (key.eq_ignore_ascii_case("enter") || key.eq_ignore_ascii_case("return")))
    .then_some((p0, k1))
}

/// A dictation's own errors ("No microphone found", "Transcription
/// failed"…): the dictation overlay says them, where the dictation is, and
/// nothing else does (`notice.rs`: one notice, one place). Other errors
/// (`yap-error`) are the main window's toasts.
pub const DICTATION_ERROR: &str = "yap-dictation-error";

/// Hard cap on a single recording's captured audio (16 kHz mono f32 ≈ 64 KB/s).
/// A stuck hotkey or a forgotten toggle-mode session would otherwise grow the
/// buffer without bound (~3.8 MB/min) until the process runs out of memory. At
/// 15 minutes that's ~57 MB — far longer than any real dictation — after which
/// the callback stops appending (and logs once) rather than risk an OOM.
const MAX_RECORDING_SAMPLES: usize = 15 * 60 * TARGET_SAMPLE_RATE as usize;

/// A recording that started while the mic was still opening waits this long
/// at most for its first real audio (`mic::Sink::live`) before the start
/// chime plays anyway (a stream that never delivers: the dead-mic watchdog
/// takes it from there).
const LIVE_FALLBACK_MS: u64 = 3_000;
/// The dead-mic watchdog counts from when the stream is running; a stream
/// still being opened after this long is watched anyway.
const OPEN_PATIENCE_MS: u64 = 6_000;

/// The bar's cue that a Voice Assistant question is about to end on a pause
/// (`pause.rs`): `{leftMs}` as it starts running out, `{leftMs: null}` when
/// you speak again.
const PAUSE_CUE: &str = "yap-assistant-pause";
/// How often the pause watch looks at its verdict.
const PAUSE_POLL_MS: u64 = 50;

/// "Stop listening when I pause" for one Voice Assistant recording
/// (`pause.rs`): fed by the mic's audio, the start chime and the live
/// preview; read by its watch thread (`spawn_pause_watch`).
struct PauseSlot {
    /// The recording it's for (`rec_session`).
    session: u64,
    pause: crate::pause::Pause,
}

/// The speech model loading in the background at startup (`Pipeline::start`,
/// off the startup thread so the tray, the bar and the window come first).
/// A dictation, a meeting chunk or a Run again that needs the engine
/// meanwhile waits for it ([`wait_for_model`]) instead of loading a second
/// copy.
static MODEL_LOADING: Mutex<bool> = Mutex::new(false);
static MODEL_LOADED: Condvar = Condvar::new();

/// The startup load is still running.
pub(crate) fn model_loading() -> bool {
    *MODEL_LOADING.lock().unwrap_or_else(|p| p.into_inner())
}

fn set_model_loading(on: bool) {
    *MODEL_LOADING.lock().unwrap_or_else(|p| p.into_inner()) = on;
    if !on {
        MODEL_LOADED.notify_all();
    }
}

/// Wait (blocking) up to `max` for the startup load to finish; `true` when
/// it has (or none was running).
pub(crate) fn wait_for_model(max: Duration) -> bool {
    let guard = MODEL_LOADING.lock().unwrap_or_else(|p| p.into_inner());
    let (guard, _) = MODEL_LOADED
        .wait_timeout_while(guard, max, |loading| *loading)
        .unwrap_or_else(|p| p.into_inner());
    !*guard
}

/// Test mode only (`e2e::e2e_dictation_mic`): the mic's audio arrives as
/// digital silence, as a muted mic's does — the dead-mic watchdog's case,
/// staged on the real microphone.
#[cfg(debug_assertions)]
static TEST_SILENT_MIC: AtomicBool = AtomicBool::new(false);

#[cfg(debug_assertions)]
pub(crate) fn set_test_silent_mic(silent: bool) {
    TEST_SILENT_MIC.store(silent, Ordering::SeqCst);
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
    /// The app runs as administrator: nothing was pasted, the text was
    /// left on the clipboard (`elevated.rs`).
    pub left_on_clipboard: bool,
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
}

/// How long a cancelled dictation's audio is kept for **Undo** ("Dictation
/// cancelled · Undo" on the bar).
pub(crate) const UNDO_MS: u64 = 10_000;
/// A cancelled recording shorter than this (0.5 s at 16 kHz), or one that
/// never heard a sound, isn't offered back: nothing worth transcribing.
const UNDO_MIN_SAMPLES: usize = 8_000;

/// Undo couldn't get back to the window the dictation was for (it closed,
/// or Windows wouldn't bring it to the front): the text is on the clipboard
/// instead of in whatever has the focus now.
fn undo_window_gone() -> crate::alert::Alert {
    crate::alert::Alert {
        kind: "pasteBlocked",
        icon: "clipboard",
        title: "Your text is copied: press Ctrl+V".to_string(),
        body: "Yap couldn't get back to the window you were dictating into, so your dictation is on the clipboard instead."
            .to_string(),
        action: None,
        timeout_ms: 12_000,
    }
}

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
}

/// The latest dictation's report, for the robot's WAV dictations (which
/// deliver from `run_stt`).
#[cfg(debug_assertions)]
static LAST_DELIVERED: Mutex<Option<Delivered>> = Mutex::new(None);

/// The scrolling waveform's level meter: the peak over ~80 ms of audio.
#[derive(Default)]
struct AmpMeter {
    peak: f32,
    count: usize,
}

impl AmpMeter {
    /// Time-domain amplitude meter for the scrolling waveform (Claude Code
    /// style): one peak level (raw 0..1) per ~1280 samples at 16 kHz ≈ 80 ms
    /// per bar for a calm, readable scroll; the overlay pushes each onto a
    /// scrolling history. `Some(peak)` when a bar is due.
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
            Some(peak)
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
    plan: crate::mute::Plan,
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
    mic: crate::mic::Mic,
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
    /// The current recording is a question for the **Voice Assistant**
    /// ([`destination`], at record start): transcribed, corrected and handed
    /// to `assistant::heard` — no cleanup, wake word, paste or history.
    assistant: AtomicBool,
    /// The Voice Assistant recording's pause watch ("Stop listening when I
    /// pause"); `None` for any other recording, or with the setting off.
    pause: Mutex<Option<PauseSlot>>,
    /// Text selected in the target app, captured at edit-mode record-start. Empty
    /// (`None`) → "write mode" (generate new text from the instruction alone).
    selection: Mutex<Option<String>>,
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
    fn other_apps(&self) -> crate::mute::Plan {
        self.config
            .read()
            .map(|c| crate::mute::Plan { quiet: c.quiet_other_apps, pause: c.pause_media })
            .unwrap_or_default()
    }

    /// The stop chime (if it's on), then the recording lets go of the mic
    /// (`mic.rs`: it stays warm a while, or a Bluetooth headset's closes now,
    /// once the chime has played on it) and the other apps' sound comes
    /// back (`mute.rs`), in that order; `token` from `mute::end`.
    fn stop_chime_then_restore(self: &Arc<Self>, token: u64) {
        let shared = Arc::clone(self);
        let lease = self.mic_lease.load(Ordering::SeqCst);
        let restore: crate::sound::Played = Box::new(move || {
            let closed_bluetooth = shared.mic.release(lease);
            crate::mute::after_stop_chime_mic(token, closed_bluetooth);
        });
        if self.sound_enabled() {
            crate::sound::play_stop(self.audio_feedback_volume(), self.output_device().as_deref(), Some(restore));
        } else {
            std::thread::spawn(restore);
        }
    }

    /// The recording is over without a stop chime (cancelled, no mic): the
    /// mic and the other apps' sound go back at once.
    fn release_now(self: &Arc<Self>, token: u64) {
        let shared = Arc::clone(self);
        let lease = self.mic_lease.load(Ordering::SeqCst);
        std::thread::spawn(move || {
            let closed_bluetooth = shared.mic.release(lease);
            crate::mute::after_stop_chime_mic(token, closed_bluetooth);
        });
    }

    /// The start chime of the recording waiting for its first real audio
    /// (`pending_start`), once — from the mic's first real audio
    /// (`mic::Sink::live`), at once for a warm mic, or after
    /// [`LIVE_FALLBACK_MS`]. Then the other apps go quiet (`mute.rs`) and
    /// the bar's capsule shows the waveform ("speak now",
    /// `yap-mic-ready`).
    fn fire_pending_start(&self) {
        let session = self.pending_session.swap(0, Ordering::SeqCst);
        if session == 0 {
            return;
        }
        let Some(start) = self.pending_start.lock().ok().and_then(|mut p| p.take()) else {
            return;
        };
        if start.session != session
            || !self.recording.load(Ordering::SeqCst)
            || self.rec_session.load(Ordering::SeqCst) != session
        {
            return; // that recording is over
        }
        // "Speak now": a Voice Assistant question's pause watch counts
        // speech from here (the chime itself left out, `pause.rs`).
        if let Ok(mut p) = self.pause.lock() {
            if let Some(slot) = p.as_mut().filter(|s| s.session == session) {
                slot.pause.speak_now();
            }
        }
        let token = start.token;
        let quiet: Option<crate::sound::Played> =
            start.plan.any().then(|| Box::new(move || crate::mute::after_start_chime(token)) as crate::sound::Played);
        if self.sound_enabled() {
            crate::sound::play_start(self.audio_feedback_volume(), self.output_device().as_deref(), quiet);
        } else if let Some(quiet) = quiet {
            std::thread::spawn(quiet);
        }
        let _ = self.app.emit("yap-mic-ready", true);
    }

    /// No start chime after all (the recording ended before its mic was on).
    fn drop_pending_start(&self) {
        self.pending_session.store(0, Ordering::SeqCst);
        if let Ok(mut p) = self.pending_start.lock() {
            *p = None;
        }
    }

    /// The mic couldn't be opened for `lease`: a recording holding it that
    /// has heard nothing ends now, saying why — Windows' privacy switches,
    /// or no microphone at all. (The robot's WAV stands in for a mic.)
    fn mic_failed(self: &Arc<Self>, lease: u64) {
        // A recording the mic already gave audio (a reopen by the dead-mic
        // watchdog that failed: unplugged mid-dictation) carries on with what
        // it has; the watchdog says what's wrong.
        if test_feed_on() {
            return;
        }
        self.mic_failed_lease.store(lease, Ordering::SeqCst);
        if self.mic_lease.load(Ordering::SeqCst) == lease
            && self.recording.load(Ordering::SeqCst)
            && self.mic_tally.read().0 == 0
        {
            self.end_without_mic();
        }
    }

    /// End the recording that has no microphone (see [`Self::mic_failed`]).
    fn end_without_mic(self: &Arc<Self>) {
        if !self.recording.swap(false, Ordering::SeqCst) {
            return;
        }
        dictation_ended();
        self.drop_pending_start();
        let token = crate::mute::end();
        self.release_now(token);
        if let Ok(mut buf) = self.buffer.lock() {
            buf.clear();
        }
        if let Ok(mut g) = self.selection.lock() {
            *g = None;
        }
        self.edit_mode.store(false, Ordering::SeqCst);
        crate::screen_terms::forget();
        if self.assistant.swap(false, Ordering::SeqCst) {
            crate::assistant::nothing(&self.app);
        }
        // Windows' privacy switches, if they're why: that's what to fix
        // (unless the recording's start already said so).
        if let Some(blocked) = crate::mic_privacy::blocked() {
            tracing::warn!(?blocked, "Can't record — Windows' privacy settings block the microphone");
            if !self.mic_dead.swap(true, Ordering::SeqCst) {
                self.say_mic_blocked(blocked);
            }
        } else {
            tracing::warn!("Can't record — no microphone");
            let _ = self
                .app
                .emit(DICTATION_ERROR, "No microphone found — connect one and try again");
        }
        let _ = self.app.emit("yap-state", "error");
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

    /// Mic audio (16 kHz mono) as it arrives: while recording — or while one
    /// is starting (`gather`) — into the buffer (and the dead-mic watchdog's
    /// tally); while idle, into the short pre-roll ring the next recording
    /// starts with. The waveform's level goes out while recording, or idle
    /// during a mic test (onboarding).
    fn take_audio(&self, samples: &[f32], amp: &mut AmpMeter) {
        if !self.recording.load(Ordering::Relaxed) && !self.gather.load(Ordering::Relaxed) {
            if let Ok(mut pr) = self.preroll.lock() {
                pr.extend(samples.iter().copied());
                while pr.len() > PREROLL_SAMPLES {
                    pr.pop_front();
                }
            }
            if !self.mic_test.load(Ordering::Relaxed) {
                return;
            }
        } else {
            // The dead-mic watchdog's tally: how much arrived, and
            // whether any of it wasn't digital silence.
            self.mic_tally.note(samples);
            if let Ok(mut buf) = self.buffer.lock() {
                // Bound the buffer so a stuck key can't grow it without limit
                // (OOM). Past the cap we drop further audio and warn once.
                if buf.len() < MAX_RECORDING_SAMPLES {
                    buf.extend_from_slice(samples);
                    if buf.len() >= MAX_RECORDING_SAMPLES {
                        tracing::warn!(
                            "Recording hit the {}-minute cap — further audio dropped; stop the recording",
                            MAX_RECORDING_SAMPLES / (60 * TARGET_SAMPLE_RATE as usize)
                        );
                    }
                }
            }
            // A Voice Assistant question listens for its pause (`pause.rs`).
            if let Ok(mut p) = self.pause.lock() {
                if let Some(slot) = p.as_mut() {
                    slot.pause.audio(samples);
                }
            }
        }
        // The next scrolling-waveform bar (recording, or an active mic
        // test). The frontend shapes (gain/curve) and scrolls it.
        if let Some(peak) = amp.add(samples) {
            let _ = self.app.emit("yap-amp", peak);
        }
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

    fn toggle(self: &Arc<Self>, edit: bool) {
        if self.recording.load(Ordering::SeqCst) {
            self.stop_and_transcribe();
        } else {
            self.start_recording(edit);
        }
    }

    /// Route a hotkey press/release through the configured recording mode.
    ///
    /// - `toggle` mode: act on press only (flip recording on/off). Ignore release.
    /// - `pushToTalk` mode: the gestures of `gesture.rs` — hold to talk
    ///   (release stops + transcribes), a quick tap then a second press locks
    ///   it hands-free until the next press, a lone tap is thrown away with a
    ///   coaching hint.
    ///
    /// `edit` selects edit/rewrite mode (the dictation hotkey passes `false`, the
    /// edit hotkey passes `true`). `at`: when the key moved (`gesture::now_ms`,
    /// stamped by the input hook); `None` = now.
    fn on_key(self: &Arc<Self>, pressed: bool, edit: bool, at: Option<u64>) {
        let mut g = self.gesture.lock().unwrap_or_else(|p| p.into_inner());
        if self.recording_mode() != "pushToTalk" {
            g.machine.reset();
            drop(g);
            if pressed {
                self.toggle(edit);
            }
            return;
        }
        let now = at.unwrap_or_else(crate::gesture::now_ms);
        let recording = self.recording.load(Ordering::SeqCst);
        let step = if pressed {
            g.machine.press(now, recording)
        } else {
            g.machine.release(now, recording)
        };
        self.apply_gesture(&mut g, step, edit);
    }

    /// The Voice Assistant's shortcut moved ([`Pipeline::on_assistant_key`]).
    fn assistant_key(self: &Arc<Self>, pressed: bool, at: Option<u64>) {
        // Off during meetings and calls: a press doesn't start listening (a
        // press while something records still stops it, below).
        if pressed && !self.recording.load(Ordering::SeqCst) && crate::assistant::off_for_meeting(&self.app) {
            return;
        }
        ASSISTANT_ARMED.store(pressed, Ordering::SeqCst);
        self.on_key(pressed, false, at);
        // Taken by the recording if one started; never left for another.
        ASSISTANT_ARMED.store(false, Ordering::SeqCst);
    }

    /// The tap timer: a tap whose second press didn't come is thrown away.
    fn gesture_tick(self: &Arc<Self>) {
        let mut g = self.gesture.lock().unwrap_or_else(|p| p.into_inner());
        let step = g.machine.tick(crate::gesture::now_ms(), self.recording.load(Ordering::SeqCst));
        let edit = g.edit;
        self.apply_gesture(&mut g, step, edit);
    }

    /// Do what a gesture step says (with the gesture lock held, so a key
    /// event and the tap timer never act at once). `edit`: the hotkey that
    /// starts a recording here.
    fn apply_gesture(self: &Arc<Self>, g: &mut GestureSlot, step: crate::gesture::Step, edit: bool) {
        match step.action {
            Action::Start => {
                g.edit = edit;
                g.assistant = ASSISTANT_ARMED.load(Ordering::SeqCst);
                // Held: the bar's capsule shows no ✕ / ✓ until it locks
                // hands-free (`yap-dictation-locked`) — the tap's wait for a
                // second press included, so nothing flashes. Said before the
                // recording starts, so its first frame already knows; the
                // page forgets it with the recording.
                let _ = self.app.emit("yap-dictation-held", true);
                self.start_recording(edit);
                // Refused (still transcribing the last one, no mic): nothing
                // to hold, tap or lock.
                if !self.recording.load(Ordering::SeqCst) {
                    g.machine.reset();
                }
            }
            Action::Stop => self.stop_and_transcribe(),
            Action::Discard => {
                tracing::info!("Push-to-talk: a lone tap, nothing to transcribe");
                self.cancel(false);
            }
            Action::Lock => {
                tracing::info!("Push-to-talk: double-tap, recording hands-free");
                let _ = self.app.emit("yap-dictation-locked", true);
                std::thread::spawn(crate::coaching::hands_free_used);
            }
            Action::None => {}
        }
        if let Some(ms) = step.tick_in {
            let shared = Arc::clone(self);
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(ms));
                shared.gesture_tick();
            });
        }
        // Coaching is about the dictation hotkey. Off the locks: showing it
        // asks the main thread whether the window is on screen.
        if let (Some(hint), false, false) = (step.hint, g.edit, g.assistant) {
            let hotkey = self.config.read().map(|c| c.hotkey.clone()).unwrap_or_default();
            let app = self.app.clone();
            std::thread::spawn(move || crate::coaching::offer(&app, hint, &hotkey));
        }
    }

    fn start_recording(self: &Arc<Self>, edit: bool) {
        // Barge-in: a dictation starting (the hotkey, the chat's mic, the
        // bar) silences Yap's spoken reply at once, before anything records
        // — Yap's own voice must never be the next question. If it was
        // audible just now, the pre-roll (which would hold it) is left out.
        let heard_yap = crate::tts::stop_for_dictation();
        // A previous transcription is still running — ignore the start so we don't
        // spawn an overlapping `run_stt` (which would rebuild a duplicate model).
        // Processing is normally sub-second on a GPU, so this rarely bites.
        if self.processing.load(Ordering::SeqCst) {
            tracing::info!("Ignoring start — a transcription is still processing");
            return;
        }
        // The mic: still warm from a moment ago (the stream delivers, with
        // the pre-roll), or opened now in the background — the recording
        // doesn't wait for it, it keeps everything the new stream delivers.
        // A mic that can't be opened ends the recording (`mic_failed`).
        let warm = self.mic.is_live();
        let lease = self.mic.hold();
        self.mic_lease.store(lease, Ordering::SeqCst);
        // Seed the buffer with the pre-roll ring (the ~300 ms before the keypress)
        // so a word already in flight isn't clipped — unless Yap was speaking
        // in it, or the mic wasn't running (the ring is old audio then).
        let pre: Vec<f32> = if heard_yap || !warm {
            Vec::new()
        } else {
            self.preroll
                .lock()
                .map(|p| p.iter().copied().collect())
                .unwrap_or_default()
        };
        if !warm {
            if let Ok(mut p) = self.preroll.lock() {
                p.clear();
            }
        }
        if let Ok(mut buf) = self.buffer.lock() {
            buf.clear();
            buf.extend_from_slice(&pre);
            // The dead-mic watchdog's tally starts afresh with each
            // recording; from here on the mic's audio is this recording's.
            self.mic_tally.reset();
            self.gather.store(true, Ordering::SeqCst);
        }
        self.capture_target(edit);

        self.mic_dead.store(false, Ordering::SeqCst);
        let session = self.rec_session.fetch_add(1, Ordering::SeqCst) + 1;

        // A meeting recording keeps this dictation out of its "You" side.
        dictation_began();
        self.recording.store(true, Ordering::SeqCst);
        self.gather.store(false, Ordering::SeqCst);
        self.touch_activity();
        let _ = self.app.emit("yap-state", "recording");
        // The mic couldn't be opened before the recording even started.
        if self.mic_failed_lease.load(Ordering::SeqCst) == lease && self.mic_tally.read().0 == 0 {
            self.end_without_mic();
            return;
        }
        // A question for the Voice Assistant ends on a pause (before the
        // start chime: the watch counts speech from it).
        let pause_watch = self.arm_pause(session);
        // The start chime first (Yap's own sound is never silenced), then
        // the other apps go quiet / pause once it has played (`mute.rs`) —
        // once the mic is on: at once when it's warm, else at its first
        // real audio (`fire_pending_start`), so the chime says "speak now"
        // only when Yap hears you.
        let plan = self.other_apps();
        let token = crate::mute::begin(plan);
        if let Ok(mut p) = self.pending_start.lock() {
            *p = Some(PendingStart { session, token, plan });
        }
        self.pending_session.store(session, Ordering::SeqCst);
        if self.mic.is_live() {
            self.fire_pending_start();
        } else {
            let _ = self.app.emit("yap-mic-ready", false);
            let shared = Arc::clone(self);
            let _ = std::thread::Builder::new().name("dictation-mic-ready".into()).spawn(move || {
                std::thread::sleep(Duration::from_millis(LIVE_FALLBACK_MS));
                if shared.pending_session.load(Ordering::SeqCst) == session {
                    tracing::info!("Microphone: no real audio yet, starting anyway");
                    shared.fire_pending_start();
                }
            });
        }
        // Windows' privacy switches keep desktop apps off the mic: this
        // recording will hear digital silence. Said now, not after the
        // watchdog's five seconds; the recording carries on (switching it on
        // mid-dictation works), and if it ends without a sound it isn't
        // transcribed, as for a dead mic.
        // (Said once: a mic that then fails to open says no more.)
        if !test_feed_on() {
            if let Some(blocked) = crate::mic_privacy::blocked() {
                tracing::warn!(?blocked, "Recording, but Windows' privacy settings block the microphone");
                if self.recording.load(Ordering::SeqCst) && !self.mic_dead.swap(true, Ordering::SeqCst) {
                    self.say_mic_blocked(blocked);
                }
            }
        }
        // Live partials: spawn a worker that re-transcribes a bounded sliding
        // window of the buffer while recording (cost independent of recording
        // length — see `partials::PartialSession`).
        self.maybe_start_streaming();
        self.spawn_mic_watch(session);
        if pause_watch {
            self.spawn_pause_watch(session);
        }
        tracing::info!("Recording started");
    }

    /// "Stop listening when I pause" (`pause.rs`): a fresh watch for
    /// recording `session` if it's a Voice Assistant question and the
    /// setting is on, else none. Its clock starts where the buffer is (the
    /// pre-roll), as the live preview's does. `true`: watch it.
    fn arm_pause(&self, session: u64) -> bool {
        let on = self.assistant.load(Ordering::SeqCst)
            && self.config.read().map(|c| c.assistant_stop_on_pause).unwrap_or(false);
        let at = if on { self.buffer.lock().map(|b| b.len() as u64).unwrap_or(0) } else { 0 };
        if let Ok(mut p) = self.pause.lock() {
            *p = on.then(|| PauseSlot { session, pause: crate::pause::Pause::new(at) });
        }
        on
    }

    /// May a pause end the recording now? Toggle mode, or push-to-talk
    /// locked hands-free — never under a held key (letting go ends it).
    /// `g`: the gesture lock, held.
    fn hands_free(&self, g: &GestureSlot) -> bool {
        self.recording_mode() != "pushToTalk" || g.machine.hands_free()
    }

    /// The pause watch for Voice Assistant recording `session`: ends the
    /// question when `pause.rs` says so, as a press of the shortcut would
    /// (the stop chime, the transcription, the answer), and tells the bar
    /// while a pause runs out. Until the recording ends, however it ends.
    fn spawn_pause_watch(self: &Arc<Self>, session: u64) {
        let shared = Arc::clone(self);
        let spawned = std::thread::Builder::new().name("assistant-pause".into()).spawn(move || {
            use crate::pause::Verdict;
            let live = || shared.recording.load(Ordering::SeqCst) && shared.rec_session.load(Ordering::SeqCst) == session;
            let mut cue = false;
            loop {
                std::thread::sleep(Duration::from_millis(PAUSE_POLL_MS));
                if !live() {
                    break;
                }
                let verdict = shared
                    .pause
                    .lock()
                    .ok()
                    .and_then(|p| p.as_ref().filter(|s| s.session == session).map(|s| (s.pause.verdict(), s.pause.heard_ms())));
                let Some((verdict, heard_ms)) = verdict else { break };
                // Under the gesture lock, as a key's stop is: a press at
                // this very moment finds it stopped, or stops it itself.
                let g = shared.gesture.lock().unwrap_or_else(|p| p.into_inner());
                let verdict = if shared.hands_free(&g) { verdict } else { Verdict::Listen };
                match verdict {
                    Verdict::End(why) => {
                        if live() {
                            tracing::info!(?why, heard_ms, "Voice Assistant: you paused, so the question ends");
                            shared.stop_and_transcribe();
                        }
                        break;
                    }
                    Verdict::Ending { left_ms } if !cue => {
                        cue = true;
                        let _ = shared.app.emit(PAUSE_CUE, serde_json::json!({ "leftMs": left_ms }));
                    }
                    Verdict::Listen if cue => {
                        cue = false;
                        let _ = shared.app.emit(PAUSE_CUE, serde_json::json!({ "leftMs": null }));
                    }
                    _ => {}
                }
                drop(g);
            }
            if cue {
                let _ = shared.app.emit(PAUSE_CUE, serde_json::json!({ "leftMs": null }));
            }
            if let Ok(mut p) = shared.pause.lock() {
                if p.as_ref().is_some_and(|s| s.session == session) {
                    *p = None;
                }
            }
        });
        if let Err(e) = spawned {
            tracing::warn!("Voice Assistant: no pause watch ({})", e);
        }
    }

    /// The live preview read the recording's first `covered` samples as
    /// `text`: the pause watch's words (`pause.rs`).
    fn pause_words(&self, covered: usize, text: &str) {
        if let Ok(mut p) = self.pause.lock() {
            if let Some(slot) = p.as_mut() {
                slot.pause.words(covered as u64, text);
            }
        }
    }

    /// "Windows is blocking microphone access for desktop apps": in the
    /// overlay's line (the dead-mic watchdog's place), and as an alert with
    /// **Open settings** (a toast, a bar card or a Windows notification).
    fn say_mic_blocked(&self, blocked: crate::mic_privacy::Blocked) {
        let _ = self.app.emit("yap-mic-silent", Some(crate::mic_privacy::mic_silent()));
        crate::alert::show_soon(&self.app, crate::mic_privacy::alert(blocked));
    }

    /// Where the dictation starting now goes: the window in front (captured
    /// before the overlay or anything else can take the focus; the paste
    /// goes back into it), what kind of app it is, edit mode's selection,
    /// and — in the background — the text around its cursor.
    fn capture_target(self: &Arc<Self>, edit: bool) {
        // A new dictation: a cancelled one waiting for Undo is gone for good.
        if self.kept.lock().map(|mut k| k.take().is_some()).unwrap_or(false) {
            tracing::info!("A new dictation: the cancelled one can't be undone any more");
        }
        // Capture the window the user is dictating into, before the overlay
        // (or anything else) can steal focus. Restored at paste time.
        let hwnd = crate::text_injector::current_foreground().unwrap_or(0);
        self.target_hwnd.store(hwnd, Ordering::Relaxed);
        // A question for the Voice Assistant (its shortcut: answered out
        // loud, nothing pasted), or for the Chat view (its mic, or the hotkey
        // while its box has the focus: the page gets the text).
        let armed = CHAT_ARMED.swap(false, Ordering::SeqCst);
        let goes = destination(
            edit,
            ASSISTANT_ARMED.swap(false, Ordering::SeqCst),
            armed,
            CHAT_FOCUSED.load(Ordering::SeqCst),
            hwnd == 0,
        );
        let assistant = goes == Goes::Assistant;
        self.to_chat.store(goes == Goes::ChatBox, Ordering::SeqCst);
        self.assistant.store(assistant, Ordering::SeqCst);
        match goes {
            Goes::ChatBox => tracing::info!("Dictation: a question for the Chat view"),
            // Said before the "recording" state, so the capsule's first
            // frame is the assistant's.
            Goes::Assistant => crate::assistant::listening(&self.app),
            Goes::Paste => {}
        }
        // And what kind of app it is (a terminal, a chat app…): its process,
        // class and title only, for fitting the paste.
        let kind = crate::app_kind::of_window((hwnd != 0).then_some(hwnd));
        if let Ok(mut k) = self.target_kind.lock() {
            *k = kind;
        }
        let (smart_spacing, in_terminal, screen_names) = self
            .config
            .read()
            .map(|c| (c.smart_spacing, kind.with_settings(c.terminal_mode, true).is_terminal(), c.screen_names))
            .unwrap_or((false, false, false));

        // Edit/rewrite mode: grab the current selection NOW, while the target app
        // still has focus (before recording), so the spoken instruction can be
        // applied to it. Empty selection → write mode. In a terminal (terminal
        // mode on) never by Ctrl+C: with nothing selected it interrupts the
        // program running there.
        self.edit_mode.store(edit, Ordering::SeqCst);
        if edit {
            let target = if hwnd == 0 { None } else { Some(hwnd) };
            let sel = crate::selection::capture_selection(target, !in_terminal);
            if let Ok(mut g) = self.selection.lock() {
                *g = sel;
            }
        }
        // "Match the text around the cursor": read it in the background now,
        // for the paste (`text_injector::prepare_dictation`). Not for edit
        // mode, whose rewrite replaces the selection as the AI wrote it, nor
        // in a terminal with terminal mode on: its "field" is the screen —
        // nor for the assistant, which pastes nothing.
        let smart = !edit && !assistant && smart_spacing && !in_terminal;
        crate::selection::begin_dictation((hwnd != 0).then_some(hwnd), smart);
        // "Spell names the way they're written on screen": the window's
        // visible text, read in the background for its names
        // (`screen_terms.rs`) — for a dictation that's pasted, not a rewrite.
        crate::screen_terms::begin_dictation((hwnd != 0).then_some(hwnd), goes == Goes::Paste && !edit && screen_names);
    }

    /// The dead-mic watchdog for recording `session` (`mic_watch.rs`): polls
    /// the callback's tally until the mic proves itself or the recording
    /// ends; reopens the stream once if the start is all digital silence (or
    /// no audio comes at all), and says so if that didn't help.
    fn spawn_mic_watch(self: &Arc<Self>, session: u64) {
        let shared = Arc::clone(self);
        let spawned = std::thread::Builder::new()
            .name("dictation-mic-watch".into())
            .spawn(move || {
                let asked = Instant::now();
                let mut started = Instant::now();
                let mut watch = DeadMicWatch::default();
                loop {
                    std::thread::sleep(Duration::from_millis(crate::mic_watch::POLL_MS));
                    let live = shared.recording.load(Ordering::SeqCst)
                        && shared.rec_session.load(Ordering::SeqCst) == session;
                    if !live {
                        break;
                    }
                    // The mic opens on demand (`mic.rs`), a Bluetooth
                    // headset taking a second or two: its watch counts from
                    // when the stream runs (a robot's WAV needs none).
                    if !shared.mic.is_open() && !test_feed_on() && asked.elapsed() < Duration::from_millis(OPEN_PATIENCE_MS) {
                        started = Instant::now();
                        continue;
                    }
                    let (samples, heard) = shared.mic_tally.read();
                    match watch.poll(started.elapsed().as_millis() as u64, samples, heard) {
                        MicStep::Wait => {}
                        MicStep::Fine => {
                            // Sound after all, though the privacy switches
                            // said no at the start: take the warning back.
                            if shared.mic_dead.swap(false, Ordering::SeqCst) {
                                let _ = shared.app.emit("yap-mic-silent", None::<MicSilent>);
                            }
                            break;
                        }
                        MicStep::Reopen => {
                            tracing::warn!(samples, "Dictation mic: nothing but silence — reopening it");
                            if let Err(e) = shared.reopen_mic() {
                                tracing::warn!("Dictation mic: couldn't reopen it: {}", e);
                            }
                        }
                        MicStep::Report => {
                            // Already said when the recording started:
                            // Windows' privacy switches block the mic.
                            if shared.mic_dead.swap(true, Ordering::SeqCst) {
                                continue;
                            }
                            match crate::mic_privacy::blocked() {
                                // Not the mic's fault: Windows' privacy switches.
                                Some(blocked) => {
                                    tracing::warn!(?blocked, "Dictation mic: silent, and Windows' privacy settings block it");
                                    shared.say_mic_blocked(blocked);
                                }
                                None => {
                                    let silent = shared.mic_silent();
                                    tracing::warn!("Dictation mic: {}", silent.message);
                                    // The overlay says so in the waveform's place (the
                                    // recording carries on), and only there.
                                    let _ = shared.app.emit("yap-mic-silent", Some(&silent));
                                    let _ = shared.app.emit(DICTATION_ERROR, silent.message);
                                }
                            }
                        }
                        MicStep::Recovered => {
                            tracing::info!("Dictation mic: sound again");
                            shared.mic_dead.store(false, Ordering::SeqCst);
                            let _ = shared.app.emit("yap-mic-silent", None::<MicSilent>);
                            break;
                        }
                    }
                }
            });
        if let Err(e) = spawned {
            tracing::warn!("Dictation mic: no watchdog ({})", e);
        }
    }

    /// "No sound from <the mic> — check it isn't muted or unplugged", and
    /// the overlay's short form — or, when Windows' privacy switches block
    /// the mic, "Windows is blocking microphone access for desktop apps".
    fn mic_silent(&self) -> MicSilent {
        if crate::mic_privacy::blocked().is_some() {
            return crate::mic_privacy::mic_silent();
        }
        let name = self.mic_name.lock().map(|n| n.clone()).unwrap_or_default();
        MicSilent::for_mic(&name)
    }

    /// Reopen the mic stream on the configured device, mid-recording (the
    /// recording carries on into the new stream). The dead stream stops
    /// first: two running at once would interleave their audio.
    fn reopen_mic(self: &Arc<Self>) -> Result<(), String> {
        self.mic.reopen()
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
        let dict = self.config.read().ok()?.dictionary.clone();
        // The dictionary's words, then the names Yap remembers (memory.rs).
        config::dictionary_prompt(&crate::dict_usage::ranked(&crate::memory::with_names(&dict)))
    }

    /// A dictation's Whisper prompt: [`Self::dictionary_prompt`]'s words,
    /// then the names on screen, if they've been read by now
    /// (`screen_terms`; never waited for).
    fn dictation_prompt(&self) -> Option<String> {
        let dict = self.config.read().ok()?.dictionary.clone();
        let words = crate::dict_usage::ranked(&crate::memory::with_names(&dict));
        crate::screen_terms::whisper_prompt(words, crate::screen_terms::peek().as_deref())
    }

    /// If streaming partials are enabled, spawn the worker thread for this
    /// recording session (it exits on its own when `recording` flips false).
    fn maybe_start_streaming(self: &Arc<Self>) {
        let enabled = self
            .config
            .read()
            .map(|c| c.streaming_partials)
            .unwrap_or(false);
        if !enabled {
            return;
        }
        let (language, translate) = self.language_settings();
        let shared = Arc::clone(self);
        std::thread::spawn(move || stream_partials(shared, language, translate));
    }

    /// Stop recording without transcribing. `undo`: the person cancelled it
    /// (the bar's ✕, the tray's Cancel), so its audio is kept for
    /// [`UNDO_MS`] — the bar shows "Dictation cancelled · Undo" (`yap-state`
    /// "cancelled") — then dropped; a push-to-talk lone tap is just thrown
    /// away. A recording too short to hold words, or one that never heard a
    /// sound, isn't offered back.
    fn cancel(self: &Arc<Self>, undo: bool) {
        if !self.recording.swap(false, Ordering::SeqCst) {
            return;
        }
        dictation_ended();
        self.drop_pending_start();
        // No stop chime for a cancel: the mic and the other apps' sound go
        // back now.
        let token = crate::mute::end();
        self.release_now(token);
        let audio = self.buffer.lock().map(|mut b| std::mem::take(&mut *b)).unwrap_or_default();
        // The edit-mode selection/state goes with it (into the kept
        // recording, or nowhere), so it can't leak into a later session.
        let selection = self.selection.lock().ok().and_then(|mut g| g.take());
        let edit = self.edit_mode.swap(false, Ordering::SeqCst);
        // The names read off the screen are this dictation's only (an Undo
        // reads them again).
        crate::screen_terms::forget();
        // A question for the assistant that's called off isn't kept for
        // Undo (Undo pastes; the assistant never does).
        let assistant = self.assistant.swap(false, Ordering::SeqCst);
        if assistant {
            crate::assistant::nothing(&self.app);
        }
        if assistant || !(undo && audio.len() >= UNDO_MIN_SAMPLES && self.mic_tally.heard()) {
            let _ = self.app.emit("yap-state", "idle");
            tracing::info!("Recording cancelled (audio discarded)");
            return;
        }
        let id = self.kept_seq.fetch_add(1, Ordering::SeqCst) + 1;
        let kept = Kept {
            id,
            audio,
            target_hwnd: self.target_hwnd.load(Ordering::Relaxed),
            target_kind: self.target_kind.lock().map(|k| *k).unwrap_or_default(),
            edit,
            selection,
        };
        if let Ok(mut k) = self.kept.lock() {
            *k = Some(kept);
        }
        let _ = self.app.emit("yap-dictation-cancelled", serde_json::json!({ "undoMs": UNDO_MS }));
        let _ = self.app.emit("yap-state", "cancelled");
        tracing::info!("Recording cancelled (kept {} s for Undo)", UNDO_MS / 1000);
        let shared = Arc::clone(self);
        let spawned = std::thread::Builder::new().name("dictation-undo".into()).spawn(move || {
            std::thread::sleep(Duration::from_millis(UNDO_MS));
            let expired = shared
                .kept
                .lock()
                .map(|mut k| {
                    let ours = k.as_ref().is_some_and(|k| k.id == id);
                    if ours {
                        *k = None;
                    }
                    ours
                })
                .unwrap_or(false);
            if expired {
                tracing::info!("The cancelled dictation's audio is gone (Undo timed out)");
                let _ = shared.app.emit("yap-state", "idle");
            }
        });
        if spawned.is_err() {
            if let Ok(mut k) = self.kept.lock() {
                *k = None;
            }
            let _ = self.app.emit("yap-state", "idle");
        }
    }

    /// **Undo** a cancel: the kept recording is transcribed and pasted
    /// exactly as if it had been stopped — into the window it was meant for
    /// (never the one in front now, if that's another), with its own kind of
    /// app, edit mode and selection. The text around that window's cursor
    /// is read afresh: it may have changed since.
    fn undo(self: &Arc<Self>) -> Result<(), String> {
        if self.recording.load(Ordering::SeqCst) {
            return Err("A dictation is recording".into());
        }
        if self.processing.swap(true, Ordering::SeqCst) {
            return Err("Yap is busy transcribing — try again in a moment".into());
        }
        let Some(k) = self.kept.lock().ok().and_then(|mut k| k.take()) else {
            self.processing.store(false, Ordering::SeqCst);
            return Err("Nothing to undo: the cancelled dictation is gone".into());
        };
        self.target_hwnd.store(k.target_hwnd, Ordering::Relaxed);
        if let Ok(mut kind) = self.target_kind.lock() {
            *kind = k.target_kind;
        }
        self.edit_mode.store(k.edit, Ordering::SeqCst);
        if let Ok(mut g) = self.selection.lock() {
            *g = k.selection;
        }
        let (smart_spacing, in_terminal, screen_names) = self
            .config
            .read()
            .map(|c| (c.smart_spacing, k.target_kind.with_settings(c.terminal_mode, true).is_terminal(), c.screen_names))
            .unwrap_or((false, false, false));
        let target = (k.target_hwnd != 0).then_some(k.target_hwnd);
        crate::selection::begin_dictation(target, smart_spacing && !k.edit && !in_terminal);
        crate::screen_terms::begin_dictation(target, !k.edit && screen_names);
        let _ = self.app.emit("yap-state", "processing");
        tracing::info!(samples = k.audio.len(), "Undo: transcribing the cancelled dictation for its own window");
        self.undoing.store(true, Ordering::SeqCst);
        let shared = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            let flag = Arc::clone(&shared);
            shared.run_stt(k.audio).await;
            flag.undoing.store(false, Ordering::SeqCst);
            flag.processing.store(false, Ordering::SeqCst);
        });
        Ok(())
    }

    fn stop_and_transcribe(self: &Arc<Self>) {
        // Stopped once: a second stop racing the first (the shortcut pressed
        // as the Voice Assistant's pause ends it) does nothing.
        if !self.recording.swap(false, Ordering::SeqCst) {
            return;
        }
        dictation_ended();
        // Stopped before the mic was on: no start chime after the stop's.
        self.drop_pending_start();
        // The other apps' sound comes back once the stop chime has played.
        let quiet_token = crate::mute::end();
        // The dead-mic watchdog reported this recording and it never heard a
        // thing: it's all digital silence, nothing to transcribe (and Whisper
        // can make words up from silence). End on the reason instead.
        if self.mic_dead.load(Ordering::SeqCst) && !self.mic_tally.heard() {
            if let Ok(mut buf) = self.buffer.lock() {
                buf.clear();
            }
            if let Ok(mut g) = self.selection.lock() {
                *g = None;
            }
            self.edit_mode.store(false, Ordering::SeqCst);
            if self.assistant.swap(false, Ordering::SeqCst) {
                crate::assistant::nothing(&self.app);
            }
            tracing::info!("Recording stopped: the mic gave nothing but silence, not transcribed");
            let _ = self.app.emit("yap-mic-silent", Some(self.mic_silent()));
            let _ = self.app.emit("yap-state", "error");
            self.stop_chime_then_restore(quiet_token);
            return;
        }
        let _ = self.app.emit("yap-state", "processing");
        self.stop_chime_then_restore(quiet_token);
        let audio = self
            .buffer
            .lock()
            .map(|mut b| std::mem::take(&mut *b))
            .unwrap_or_default();
        tracing::info!(samples = audio.len(), "Recording stopped, transcribing");

        // Mark "processing" so a rapid re-toggle can't start an overlapping
        // transcription. Cleared once `run_stt` returns (all paths), below.
        self.processing.store(true, Ordering::SeqCst);
        let shared = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            // `run_stt` consumes its Arc; keep a second handle to clear the flag.
            let flag = Arc::clone(&shared);
            shared.run_stt(audio).await;
            flag.processing.store(false, Ordering::SeqCst);
        });
    }

    /// Is this dictation a **Command** (it says the agent's name; was "the
    /// Voice Agent's wake word")? Port of OpenWhispr's
    /// `resolveDictationAgentReachability` + `detectAgentName` gate: the
    /// Commands scope (`voiceAgent`) must be enabled with a provider (BYOK/custom providers
    /// also need a model; ondevice/self-hosted have defaults), and the
    /// transcript must address the agent by name — fuzzy-matched, so STT
    /// mis-hearings still wake it. Default name "Yap" (the tab's copy).
    fn wake_word_hit(&self, transcript: &str) -> bool {
        let Ok(c) = self.config.read() else {
            return false;
        };
        let Some(s) = c.llm_scopes.get("voiceAgent") else {
            return false;
        };
        if !s.enabled || s.provider.is_empty() {
            return false;
        }
        if !(s.provider == "ondevice" || s.provider == "local") && s.model.trim().is_empty() {
            return false;
        }
        let name = match c.agent_name.trim() {
            "" => "Yap",
            n => n,
        };
        crate::agent_detect::detect_agent_name(transcript, name)
    }

    /// Edit/rewrite-mode finish: apply the spoken `instruction` to the selection
    /// captured at record-start and paste the result back into the target window.
    /// Uses the **Commands** scope's own provider/model/prompt when that scope is
    /// enabled + configured, otherwise the global AI-cleanup endpoint; either way it
    /// needs a base URL configured. Emits the final `yap-state` itself.
    async fn run_rewrite(self: &Arc<Self>, instruction: String) {
        // Take the selection captured when the edit hotkey was pressed.
        let selection = self
            .selection
            .lock()
            .ok()
            .and_then(|mut g| g.take())
            .unwrap_or_default();
        self.run_agent(instruction, selection).await
    }

    /// Shared runner for **Commands**: apply `instruction` via the Commands
    /// scope (`voiceAgent`) (or
    /// the global cleanup endpoint) and paste the result. `selection` empty =
    /// write mode. Two callers: the edit hotkey (`run_rewrite`, selection
    /// captured at key-press) and the **wake-word path** (no selection — the
    /// whole transcript is the instruction, OpenWhispr-style).
    async fn run_agent(self: &Arc<Self>, instruction: String, selection: String) {
        // Nothing heard (the edit key tapped without speaking): go quietly back
        // to idle like an empty dictation does. Falling through used to surface
        // "Rewrite failed via <provider> — check its key", blaming the API key.
        if instruction.trim().is_empty() {
            tracing::info!("Edit mode: no speech detected, nothing to rewrite");
            let _ = self.app.emit("yap-state", "idle");
            return;
        }

        let (base_url, api_key, model, provider, body, disable_thinking, restore_clipboard) = self
            .config
            .read()
            .map(|c| {
                // Commands scope (OpenWhispr "Language Models" bubble, step 3):
                // when it's enabled and configured, run edit/rewrite on THAT scope's own
                // provider/model/prompt. Otherwise fall back to the global AI-cleanup
                // endpoint (the original edit-mode behaviour). "ondevice" routes
                // through the sidecar in both paths via effective_endpoint_for.
                let (base_url, api_key, model, provider, body, disable_thinking) =
                    match c.llm_scopes.get("voiceAgent") {
                        Some(s) if s.enabled && !s.provider.is_empty() => {
                            // Standard-provider keys are global (shared with the
                            // Cleanup tab); an empty scope key falls back to them.
                            let key = c.provider_api_key(&s.provider, &s.api_key);
                            let (b, k, m, p) = crate::local_llm::effective_endpoint_for(
                                &s.provider,
                                &s.base_url,
                                &key,
                                &s.model,
                            );
                            // The agent prompt is templated on the wake word
                            // (OpenWhispr's `{{agentName}}`); "Yap" is the
                            // unnamed default, matching the tab's copy.
                            let name = match c.agent_name.trim() {
                                "" => "Yap",
                                n => n,
                            };
                            let body = s.prompt.replace("{{agentName}}", name);
                            (b, k, m, p, body, s.disable_thinking)
                        }
                        _ => {
                            let (b, k, m, p) = crate::local_llm::effective_endpoint(&c);
                            (b, k, m, p, String::new(), c.pp_disable_thinking)
                        }
                    };
                (base_url, api_key, model, provider, body, disable_thinking, c.restore_clipboard)
            })
            .unwrap_or_default();

        if base_url.is_empty() {
            // The on-device model, down for a moment while Yap restarts it.
            let restarting = provider == crate::local_llm::PROVIDER_ONDEVICE && crate::local_llm::restart_under_way();
            tracing::warn!(restarting, "Edit mode needs an AI cleanup endpoint (none configured)");
            let _ = self.app.emit(
                DICTATION_ERROR,
                if restarting {
                    "The on-device AI model is restarting. Try again in a moment."
                } else {
                    "Set up AI cleanup to use edit mode"
                },
            );
            let _ = self.app.emit("yap-state", "error");
            return;
        }

        // Cloud providers reject keyless requests with a confusing 401 (e.g.
        // "Invalid Anthropic API Key" while the user is looking at the Groq
        // cleanup tab) — fail fast with a message that names the provider.
        const KEYED_PROVIDERS: [&str; 5] =
            ["groq", "anthropic", "openai", "gemini", "openrouter"];
        if api_key.is_empty() && KEYED_PROVIDERS.contains(&provider.as_str()) {
            tracing::warn!(provider = %provider, "Rewrite skipped: no API key for provider");
            let _ = self.app.emit(
                DICTATION_ERROR,
                format!("Commands: no {provider} API key — add one in Language Models"),
            );
            let _ = self.app.emit("yap-state", "error");
            return;
        }

        let target = match self.target_hwnd.load(Ordering::Relaxed) {
            0 => None,
            h => Some(h),
        };

        match crate::llm::rewrite(
            &instruction,
            &selection,
            &base_url,
            &api_key,
            &model,
            &provider,
            &body,
            disable_thinking,
        )
        .await
        {
            Ok(result) => {
                let mut out = result.trim().to_string();
                if out.is_empty() {
                    let _ = self.app.emit("yap-state", "idle");
                    return;
                }
                // A terminal gets the AI's text safe for it too (one line at
                // a shell); anywhere else it's pasted as the AI wrote it.
                if let AppKind::Terminal(t) = self.paste_kind() {
                    out = crate::app_kind::terminal_text(&out, t.keeps_lines());
                }
                // The log says how much, never what (the text only in Debug logging).
                tracing::info!(chars = out.chars().count(), "Rewrite");
                tracing::debug!(text = %out, "Rewrite text");
                let _ = self.app.emit("yap-transcript", out.clone());
                let strict = self.undoing.load(Ordering::SeqCst);
                let read = self.read_kind();
                match crate::text_injector::inject_text(&out, restore_clipboard, target, strict, read).await {
                    Ok(crate::text_injector::Delivery::Pasted | crate::text_injector::Delivery::Typed) => {}
                    // An app running as administrator: on the clipboard.
                    Ok(crate::text_injector::Delivery::LeftOnClipboard) => {
                        crate::alert::show_soon(&self.app, crate::elevated::alert());
                    }
                    // An Undo whose window is gone: on the clipboard.
                    Ok(crate::text_injector::Delivery::WindowGone) => {
                        crate::alert::show_soon(&self.app, undo_window_gone());
                    }
                    Err(e) => tracing::warn!("Rewrite inject failed: {}", e),
                }
                let _ = self.app.emit("yap-state", "idle");
            }
            Err(e) => {
                // Don't fall back to typing the raw instruction — that would paste
                // "make this a list" into the doc. Surface an error instead.
                tracing::warn!("Rewrite failed: {}", e);
                let _ = self.app.emit(
                    DICTATION_ERROR,
                    format!("Rewrite failed via {provider} — check its key in Language Models"),
                );
                let _ = self.app.emit("yap-state", "error");
            }
        }
    }

    /// Upload / file transcription: decode → chunk → transcribe on the warm
    /// engine, with per-chunk `yap-upload-progress` events and a cancel flag.
    /// Runs with the `processing` guard held (set by `Pipeline::transcribe_file`)
    /// so a live dictation can't grab the engine mid-run. Result arrives via
    /// `yap-upload-done` / `-error` / `-cancelled` events.
    async fn run_file_transcription(self: Arc<Self>, path: String) {
        let emit_err = |msg: String| {
            tracing::warn!("Upload transcription failed: {}", msg);
            let _ = self.app.emit("yap-upload-error", msg);
        };

        let file_name = std::path::Path::new(&path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.clone());

        let _ = self.app.emit(
            "yap-upload-progress",
            serde_json::json!({ "stage": "decoding", "percent": 0, "chunksTotal": 0, "chunksCompleted": 0 }),
        );

        // 1. Decode off the async runtime (long files take seconds).
        let decode_path = std::path::PathBuf::from(&path);
        let decoded =
            tokio::task::spawn_blocking(move || crate::media::decode_to_16k_mono(&decode_path))
                .await;
        let samples = match decoded {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => return emit_err(e),
            Err(e) => return emit_err(format!("Decode task failed: {e}")),
        };
        let duration_secs = samples.len() as f32 / 16_000.0;

        // 2. Warm engine (the startup load waited for), or lazy-reload
        // (same pattern as run_stt).
        if model_loading() {
            let _ = tokio::task::spawn_blocking(|| wait_for_model(Duration::from_secs(180))).await;
        }
        let engine = self.engine.lock().ok().and_then(|mut g| g.take());
        let mut engine = match engine {
            Some(e) => e,
            None => {
                let (model_size, use_gpu) = self
                    .config
                    .read()
                    .map(|c| (c.model_size.clone(), c.use_gpu))
                    .unwrap_or_else(|_| (String::new(), true));
                match stt::create_stt_engine(&config::data_dir(), &model_size, use_gpu) {
                    Ok(e) => {
                        tracing::info!(model = %model_size, "Reloading model for file transcription");
                        e
                    }
                    Err(e) => return emit_err(format!("No STT model available: {e}")),
                }
            }
        };

        let (language, translate) = self.language_settings();

        // 3. ~60 s chunks, each cut at the quietest point of its last 5 s so a
        //    boundary doesn't slice through a word (media::chunk_ranges).
        let ranges = crate::media::chunk_ranges(samples.len(), 16_000, 60, 5, &samples);
        let total = ranges.len();
        let mut pieces: Vec<String> = Vec::with_capacity(total);

        for (i, (a, b)) in ranges.into_iter().enumerate() {
            if self.upload_cancel.load(Ordering::SeqCst) {
                if let Ok(mut g) = self.engine.lock() {
                    if g.is_none() {
                        *g = Some(engine);
                    }
                }
                tracing::info!("Upload transcription cancelled");
                let _ = self.app.emit("yap-upload-cancelled", ());
                return;
            }
            let chunk: Vec<f32> = samples[a..b].to_vec();
            let lang = language.clone();
            let prompt = self.dictionary_prompt();
            let outcome = tokio::task::spawn_blocking(move || {
                let result = engine.transcribe(&chunk, lang.as_deref(), translate, prompt.as_deref());
                (engine, result)
            })
            .await;
            match outcome {
                Ok((e, Ok(text))) => {
                    engine = e;
                    let t = text.trim().to_string();
                    if !t.is_empty() {
                        pieces.push(t);
                    }
                }
                Ok((e, Err(err))) => {
                    if let Ok(mut g) = self.engine.lock() {
                        if g.is_none() {
                            *g = Some(e);
                        }
                    }
                    return emit_err(format!("Transcription failed: {err}"));
                }
                Err(e) => return emit_err(format!("Transcription task failed: {e}")),
            }
            self.touch_activity();
            let _ = self.app.emit(
                "yap-upload-progress",
                serde_json::json!({
                    "stage": "transcribing",
                    "percent": ((i + 1) as f64 / total as f64 * 100.0).round(),
                    "chunksTotal": total,
                    "chunksCompleted": i + 1,
                }),
            );
        }

        // Put the warm engine back (unless a model switch installed a newer one).
        if let Ok(mut g) = self.engine.lock() {
            if g.is_none() {
                *g = Some(engine);
            }
        }

        let raw = pieces.join(" ").trim().to_string();
        // The correction dictionary, as a dictation gets it. Over the whole
        // text, so a mis-hearing split across two chunks is still caught.
        let text = self
            .config
            .read()
            .map(|c| apply_corrections(&raw, &c))
            .unwrap_or_else(|_| raw.clone());
        let text = text.trim().to_string();
        if text.is_empty() {
            return emit_err("No speech detected in this file".to_string());
        }

        // Local history (best-effort, gated like dictations) — the Home feed
        // shows the upload with the file name in the app slot.
        let (history_enabled, model) = self
            .config
            .read()
            .map(|c| (c.history_enabled, c.model_size.clone()))
            .unwrap_or((false, String::new()));
        if history_enabled {
            crate::history::record(&raw, &text, &model, &file_name);
            let _ = self.app.emit("yap-history-changed", ());
        }
        let _ = self.app.emit("yap-transcript", text.clone());
        let _ = self.app.emit(
            "yap-upload-done",
            serde_json::json!({ "text": text, "durationSecs": duration_secs, "file": file_name }),
        );
    }

    /// A dictation's transcript (`text`, from the STT), the rest of the way:
    /// AI cleanup (routed per app), the dictionary, the "press enter"
    /// command and snippets, fitting to the kind of app and the text around
    /// the cursor, the paste into the window captured at record start, the
    /// submit key, history. Returns what went to the paste (the robot's
    /// report; a dictation ignores it).
    async fn deliver(self: &Arc<Self>, text: String) -> Delivered {
        // Smart routing: which app were we dictating into? The
        // foreground window was captured at record-start; resolve
        // it to a process name (e.g. "slack.exe") so per-app
        // cleanup rules can pick a matching body.
        let target_hwnd = match self.target_hwnd.load(Ordering::Relaxed) {
            0 => None,
            h => Some(h),
        };
        let target_app = crate::text_injector::app_name_for(target_hwnd);

        let (dict, append_space, auto_submit, auto_submit_key, restore_clipboard) = self
            .config
            .read()
            .map(|c| {
                (
                    c.dictionary.clone(),
                    c.append_trailing_space,
                    c.auto_submit,
                    c.auto_submit_key.clone(),
                    c.restore_clipboard,
                )
            })
            .unwrap_or_else(|_| (Vec::new(), false, false, "enter".to_string(), true));

        // Optional AI cleanup pass (best-effort, `clean_up`). Runs
        // before the dictionary so the user's corrections always have
        // the final say. The state stays `processing` meanwhile.
        let raw = text.trim().to_string();
        // The names on screen, read at record start (`screen_terms`): this
        // dictation's only, and only if the read is done. Taken once, for
        // the cleanup, the corrections and the words as said alike.
        let screen = crate::screen_terms::take();
        let cleaned = self.clean_up(&raw, target_app.as_deref(), screen.as_deref()).await;

        // The correction dictionary (exact, then fuzzy for
        // ONNX models), exactly as meetings and Upload get it,
        // then the names on screen, the "press enter" voice command
        // and snippets (dictations only).
        let (corrected, voice_enter) = self
            .config
            .read()
            .map(|c| finish_dictation_with(cleaned.trim(), &raw, &c, screen.as_deref()))
            .unwrap_or_else(|_| (config::apply_dictionary(cleaned.trim(), &dict), false));
        // …and the words as said, when cleanup changed them: what "Use
        // what I said" swaps the paste for (`as_said.rs`) — the same
        // corrections and names on screen, no cleanup.
        let said = (cleaned.trim() != raw)
            .then(|| self.config.read().ok().map(|c| as_said_text(&raw, &c, screen.as_deref())))
            .flatten()
            .filter(|s| !s.trim().is_empty() && *s != corrected);
        self.deliver_text(raw, corrected, voice_enter, said, dict, append_space, auto_submit, auto_submit_key, restore_clipboard)
            .await
    }

    /// AI cleanup of a transcript (`raw`) for `target_app`, best-effort:
    /// any error or timeout gives the transcript back, so dictation is
    /// never blocked; an empty answer is a deliberate one (filler only).
    /// `screen`: the names on screen the dictation read (`screen_terms`),
    /// for the line telling the model how they're spelled.
    async fn clean_up(&self, raw: &str, target_app: Option<&str>, screen: Option<&crate::screen_terms::ScreenTerms>) -> String {
        // Snapshot the cleanup-related config under one read lock.
        // `pp_body` is the routed cleanup body: `Some(body)` to clean
        // with, `None` to skip cleanup (selected-apps-only scope + no
        // matching rule). The endpoint comes from the matched profile's
        // LLM override when it has one, else the global settings —
        // either way "ondevice" routes through the sidecar when it's up.
        let (dict, pp_enabled, pp_base_url, pp_api_key, pp_model, pp_provider, pp_body, pp_disable_thinking) = self
            .config
            .read()
            .map(|c| {
                let plan = c.resolve_cleanup(target_app);
                let (pp_base_url, pp_api_key, pp_model, pp_provider) =
                    match plan.as_ref().and_then(|p| p.endpoint.as_ref()) {
                        Some((provider, base_url, api_key, model)) => {
                            // Profile keys fall back to the global
                            // per-provider store, like scopes do.
                            let key = c.provider_api_key(provider, api_key);
                            crate::local_llm::effective_endpoint_for(
                                provider, base_url, &key, model,
                            )
                        }
                        None => crate::local_llm::effective_endpoint(&c),
                    };
                (
                    c.dictionary.clone(),
                    c.post_process_enabled,
                    pp_base_url,
                    pp_api_key,
                    pp_model,
                    pp_provider,
                    plan.map(|p| p.body),
                    c.pp_disable_thinking,
                )
            })
            .unwrap_or_else(|_| (Vec::new(), false, String::new(), String::new(), String::new(), String::new(), None, false));

        // `pp_body` is `None` when smart routing says "skip cleanup
        // for this app" (selected-apps-only scope, unbound app).
        match (&pp_body, pp_enabled && !pp_base_url.is_empty() && !raw.is_empty()) {
            (Some(body), true) => {
                // "Write in my region's spelling" (Dictation cleanup): British "colour"
                // for en-GB, American "color" for en-US.
                let spelling = self.config.read().ok().and_then(|c| crate::about_you::cleanup_spelling(&c));
                // The names on screen the transcript seems to hold.
                let names = crate::screen_terms::cleanup_line(screen, raw, &dict);
                match crate::llm::cleanup_with_names(
                    raw,
                    &pp_base_url,
                    &pp_api_key,
                    &pp_model,
                    &pp_provider,
                    body,
                    spelling.as_deref(),
                    names.as_deref(),
                    &dict,
                    pp_disable_thinking,
                )
                .await
                {
                    // Ok(empty) is a DELIBERATE empty result (filler-only
                    // input) — keep it empty so nothing is injected. Only a
                    // real request error falls back to the raw transcript.
                    Ok(c) => c,
                    Err(e) => {
                        tracing::warn!("AI cleanup failed, using raw: {}", e);
                        raw.to_string()
                    }
                }
            }
            _ => raw.to_string(),
        }
    }

    /// The rest of [`Shared::deliver`]: a finished dictation (`corrected`,
    /// and its words as said when cleanup changed them) to the Chat view's
    /// box, or fitted and pasted, the submit key, history.
    #[allow(clippy::too_many_arguments)]
    async fn deliver_text(
        self: &Arc<Self>,
        raw: String,
        mut corrected: String,
        voice_enter: bool,
        said: Option<String>,
        dict: Vec<config::DictionaryEntry>,
        append_space: bool,
        auto_submit: bool,
        auto_submit_key: String,
        restore_clipboard: bool,
    ) -> Delivered {
        // A question for the Chat view: its page puts the text in the box
        // (and sends it, hands-free or after "press enter"); nothing is
        // pasted, no key pressed.
        if self.to_chat.swap(false, Ordering::SeqCst) {
            return self.deliver_to_chat(raw, corrected, voice_enter);
        }
        // A terminal or a chat app (`app_kind.rs`), as the
        // settings have it: how the text is fitted and which
        // key, if any, follows the paste.
        let kind = self.paste_kind();
        let submit =
            crate::app_kind::submit_key(kind, auto_submit, voice_enter, &auto_submit_key)
                .map(str::to_string);
        if voice_enter {
            tracing::info!("Voice command: press {}", submit.as_deref().unwrap_or("enter"));
        } else if auto_submit && submit.is_none() {
            tracing::info!("Auto-submit: not pressing Enter at a terminal's prompt");
        }
        // The paste keys follow the window as read, whatever the settings
        // (Git Bash takes Shift+Insert with "Safer pasting" off too).
        let read = self.read_kind();
        let mut report = Delivered {
            raw: raw.clone(),
            kind: kind.describe(),
            submit: submit.clone(),
            paste: read.paste_key().name().to_string(),
            ..Delivered::default()
        };
        if !corrected.is_empty() {
            // Fitted to the kind of app and the text around
            // the cursor (smart spacing), else the trailing
            // space as before — the words as said too, to the
            // same place.
            let said = match said {
                Some(said) => {
                    let (text, said) = crate::text_injector::prepare_dictation_as_said(&corrected, &said, append_space, &dict, kind);
                    corrected = text;
                    Some(said)
                }
                None => {
                    corrected = crate::text_injector::prepare_dictation(&corrected, append_space, &dict, kind);
                    None
                }
            };
            report.text = corrected.clone();
            report.as_said = said.clone();
            // Why the paste didn't land, for history (a failed paste is
            // listed so it can be copied or run again).
            let mut not_pasted: Option<&'static str> = None;
            // The log says how much, never what (the text only in Debug logging).
            tracing::info!(chars = corrected.chars().count(), "Transcript");
            tracing::debug!(text = %corrected, "Transcript text");
            let _ = self.app.emit("yap-transcript", corrected.clone());
            let target = match self.target_hwnd.load(Ordering::Relaxed) {
                0 => None,
                h => Some(h),
            };
            match crate::text_injector::inject_text(
                &corrected,
                restore_clipboard,
                target,
                self.undoing.load(Ordering::SeqCst),
                read,
            )
            .await
            {
                // An app running as administrator took no keys: the
                // text is on the clipboard, and the person is told
                // to paste it (no submit key, no correction watch).
                Ok(crate::text_injector::Delivery::LeftOnClipboard) => {
                    report.left_on_clipboard = true;
                    not_pasted = Some("Left on the clipboard: the app runs as administrator");
                    if submit.is_some() {
                        tracing::info!("Not pressing the submit key: the app runs as administrator");
                    }
                    crate::alert::show_soon(&self.app, crate::elevated::alert());
                }
                // An Undo whose window is gone (or won't come to the
                // front): never into another one; on the clipboard.
                Ok(crate::text_injector::Delivery::WindowGone) => {
                    report.left_on_clipboard = true;
                    report.error = Some("the dictation's window is gone".into());
                    not_pasted = Some("Left on the clipboard: its window was gone");
                    crate::alert::show_soon(&self.app, undo_window_gone());
                }
                Ok(delivery @ (crate::text_injector::Delivery::Pasted | crate::text_injector::Delivery::Typed)) => {
                    // The paste didn't land and the text was typed instead.
                    report.typed = delivery == crate::text_injector::Delivery::Typed;
                    // Once, whether auto-submit, the voice
                    // command or both asked for it.
                    if let Some(key) = &submit {
                        if let Err(e) = crate::text_injector::press_submit(key).await {
                            tracing::warn!("Auto-submit failed: {}", e);
                            report.error = Some(e);
                        }
                        // Sent: nothing left in the field to swap.
                        crate::as_said::remember(None, read, &corrected, None, restore_clipboard);
                    } else {
                        // "Use what I said" (as_said.rs): this paste can be
                        // swapped for the words as said, and the bar offers
                        // it when cleanup changed the words.
                        let seq = crate::as_said::remember(target, read, &corrected, said.as_deref(), restore_clipboard);
                        let offer = self.config.read().map(|c| c.as_said_offer).unwrap_or(false);
                        report.offer = seq.filter(|_| {
                            offer && said.as_deref().is_some_and(|s| crate::as_said::words_differ(&corrected, s))
                        });
                        // "Suggest words from your corrections":
                        // watch the field for a fix (learn.rs).
                        // Not a terminal's: its text is the
                        // program's output too.
                        let terminal =
                            self.target_kind.lock().map(|k| k.is_terminal()).unwrap_or(true);
                        let learn = !terminal
                            && self.config.read().map(|c| c.learn_words).unwrap_or(false);
                        crate::learn::after_paste(&self.app, &corrected, target, learn);
                    }
                }
                Err(e) => {
                    tracing::warn!("Inject failed: {}", e);
                    report.error = Some(e);
                    not_pasted = Some("The paste didn't go through");
                    crate::as_said::remember(None, read, &corrected, None, restore_clipboard);
                }
            }

            // Local-only history for the stats dashboard
            // (best-effort, gated by `history_enabled`), with the
            // dictation's audio (recent_audio.rs) — and why the paste
            // didn't land, if it didn't.
            let (history_enabled, model) = self
                .config
                .read()
                .map(|c| (c.history_enabled, c.model_size.clone()))
                .unwrap_or((false, String::new()));
            let audio = self.audio_id.lock().ok().and_then(|mut a| a.take());
            if history_enabled {
                let app_name =
                    crate::text_injector::app_name_for(target).unwrap_or_default();
                crate::history::record_with(
                    &raw,
                    corrected.trim(),
                    &model,
                    &app_name,
                    audio.as_deref(),
                    not_pasted.map(|_| crate::history::NOT_PASTED),
                    not_pasted,
                );
                // Its audio written (Home lists it with Run again).
                if let Some(id) = &audio {
                    crate::recent_audio::finished(id);
                }
                // After the save: `yap-transcript` went out
                // before the paste, so a view that re-pulled
                // history on it could miss this dictation.
                let _ = self.app.emit("yap-history-changed", ());
            } else if let Some(id) = audio {
                crate::recent_audio::discard(&id);
            }
        } else if voice_enter {
            // Only "press enter" was said: nothing to paste
            // (which would have refocused the dictation's
            // window), so press it only while that window
            // still has the focus — never in another one.
            let target = match self.target_hwnd.load(Ordering::Relaxed) {
                0 => None,
                h => Some(h),
            };
            let key = submit.as_deref().unwrap_or(&auto_submit_key);
            if target.is_none() || crate::text_injector::current_foreground() == target {
                if let Err(e) = crate::text_injector::press_submit(key).await {
                    tracing::warn!("Voice command submit failed: {}", e);
                    report.error = Some(e);
                }
            } else {
                tracing::info!("Voice command: the dictation's window lost the focus, not pressing");
                report.error = Some("the dictation's window lost the focus".into());
            }
            report.submit = Some(key.to_string());
        }
        #[cfg(debug_assertions)]
        if let Ok(mut last) = LAST_DELIVERED.lock() {
            *last = Some(report.clone());
        }
        report
    }

    /// A dictated question for the Chat view (`to_chat`): `yap-chat-dictation
    /// {text, send}` — the page inserts `text` at its caret and, when `send`
    /// ("Talk back hands-free", or "press enter" said at the end;
    /// `tts::sends_by_itself`), sends it as asked by voice. Kept in history
    /// like any dictation.
    fn deliver_to_chat(&self, raw: String, text: String, said_enter: bool) -> Delivered {
        let text = text.trim().to_string();
        let send = self
            .config
            .read()
            .map(|c| crate::tts::sends_by_itself(c.speak_replies, c.talk_back_hands_free, said_enter))
            .unwrap_or(said_enter);
        tracing::info!(len = text.len(), send, "Dictation: into the Chat view's box");
        let _ = self.app.emit("yap-chat-dictation", serde_json::json!({ "text": text, "send": send }));
        if !text.is_empty() {
            let _ = self.app.emit("yap-transcript", text.clone());
            let (history_enabled, model) = self
                .config
                .read()
                .map(|c| (c.history_enabled, c.model_size.clone()))
                .unwrap_or((false, String::new()));
            if history_enabled {
                let audio = self.audio_id.lock().ok().and_then(|mut a| a.take());
                crate::history::record_with(&raw, &text, &model, "yap.exe", audio.as_deref(), None, None);
                if let Some(id) = audio {
                    crate::recent_audio::finished(&id);
                }
                let _ = self.app.emit("yap-history-changed", ());
            }
        }
        let report = Delivered {
            text,
            raw,
            kind: "the Chat view".into(),
            submit: send.then(|| "enter".to_string()),
            ..Delivered::default()
        };
        #[cfg(debug_assertions)]
        if let Ok(mut last) = LAST_DELIVERED.lock() {
            *last = Some(report.clone());
        }
        report
    }

    /// A question for the **Voice Assistant**, transcribed: the correction
    /// dictionary as for any dictation (no cleanup, voice command or
    /// snippets: the AI reads it as asked), then `assistant::heard` — Chat
    /// answers and Yap reads it aloud. `false`: nothing was said.
    fn ask_assistant(&self, transcript: &str) -> bool {
        let question = self
            .config
            .read()
            .map(|c| apply_corrections(transcript, &c))
            .unwrap_or_else(|_| transcript.to_string());
        let question = question.trim().to_string();
        if question.is_empty() {
            tracing::info!("Voice Assistant: nothing heard");
            crate::assistant::nothing(&self.app);
            return false;
        }
        tracing::info!(len = question.len(), "Voice Assistant: a question");
        crate::assistant::heard(&self.app, question);
        true
    }

    async fn run_stt(self: Arc<Self>, audio: Vec<f32>) {
        if audio.is_empty() {
            if self.assistant.swap(false, Ordering::SeqCst) {
                crate::assistant::nothing(&self.app);
            }
            let _ = self.app.emit("yap-state", "idle");
            return;
        }

        // Set on any failure below so we can tell the user instead of silently
        // returning to idle (kept short so it fits the overlay capsule).
        let mut error_msg: Option<&'static str> = None;

        // Its audio, kept a day so it can be run again (recent_audio.rs) —
        // not a rewrite's instruction or a question for the assistant.
        // Written off this thread; its history entry takes it.
        if !self.edit_mode.load(Ordering::SeqCst) && !self.assistant.load(Ordering::SeqCst) {
            let id = self.config.read().ok().and_then(|c| crate::recent_audio::keep(&audio, &c));
            if let Ok(mut a) = self.audio_id.lock() {
                *a = id;
            }
        }

        // Take the engine out so the mutex isn't held across the await.
        let mut engine = self.engine.lock().ok().and_then(|mut g| g.take());
        // Started before the model finished loading (`Pipeline::start` loads
        // it in the background): wait for it — the bar says "Loading the
        // speech model…" — instead of failing or loading a second copy.
        if engine.is_none() && model_loading() {
            tracing::info!("Dictation: waiting for the speech model to finish loading");
            let _ = self.app.emit("yap-model-loading", true);
            let _ = tokio::task::spawn_blocking(|| wait_for_model(Duration::from_secs(180))).await;
            let _ = self.app.emit("yap-model-loading", false);
            engine = self.engine.lock().ok().and_then(|mut g| g.take());
        }
        let engine = match engine {
            Some(e) => e,
            None => {
                // No warm engine — either the idle watcher unloaded it (B1) or
                // a model was never loaded. Try a lazy (re)build before giving up.
                let (model_size, use_gpu) = self
                    .config
                    .read()
                    .map(|c| (c.model_size.clone(), c.use_gpu))
                    .unwrap_or_else(|_| (String::new(), true));
                let data_dir = config::data_dir();
                match stt::create_stt_engine(&data_dir, &model_size, use_gpu) {
                    Ok(e) => {
                        tracing::info!(model = %model_size, "Reloading model (was unloaded/idle)");
                        e
                    }
                    Err(e) => {
                        tracing::warn!("No STT engine (model missing) — cannot transcribe: {}", e);
                        if self.assistant.swap(false, Ordering::SeqCst) {
                            crate::assistant::nothing(&self.app);
                        }
                        // Listed with its audio: run it again once a model is in.
                        self.list_failed("No speech model was ready");
                        let _ = self.app.emit("yap-state", "needs-model");
                        return;
                    }
                }
            }
        };

        // Snapshot the language/translate settings for this transcription.
        let (language, translate) = self.language_settings();

        // Slow-transcription watchdog: if no usable GPU is present, whisper (esp.
        // large-v3) runs on CPU and can take minutes, leaving the UI stuck on
        // "processing" with no feedback. We can't cancel the blocking call without
        // losing the result, so instead we surface a distinct "processing-slow"
        // state + an actionable log line if we cross a threshold. Cancelled the
        // instant transcription finishes.
        const SLOW_TRANSCRIBE_SECS: u64 = 8;
        let done = Arc::new(AtomicBool::new(false));
        let watch_app = self.app.clone();
        let watch_done = Arc::clone(&done);
        let watch_model = self
            .config
            .read()
            .map(|c| c.model_size.clone())
            .unwrap_or_default();
        let watchdog = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(SLOW_TRANSCRIBE_SECS)).await;
            if !watch_done.load(Ordering::SeqCst) {
                tracing::warn!(
                    model = %watch_model,
                    "Transcription still running after {}s — whisper is running on CPU (no usable Vulkan GPU found), which is very slow; Parakeet V3 (ONNX/DirectML) is the fast universal default",
                    SLOW_TRANSCRIBE_SECS
                );
                let _ = watch_app.emit("yap-state", "processing-slow");
            }
        });

        let dict_prompt = self.dictation_prompt();
        let outcome = tokio::task::spawn_blocking(move || {
            let result =
                engine.transcribe(&audio, language.as_deref(), translate, dict_prompt.as_deref());
            (engine, result)
        })
        .await;

        done.store(true, Ordering::SeqCst);
        watchdog.abort();

        // Mark activity so the idle watcher counts from end-of-transcription.
        self.touch_activity();

        // The bar's "Cleaned up · Use what I said", for a dictation whose
        // words AI cleanup changed (`as_said.rs`).
        let mut offer: Option<u64> = None;
        match outcome {
            Ok((engine, transcription)) => {
                // Put the (warm) engine back for next time — but only if the
                // slot is still empty. If a model switch (`set_engine`) installed
                // a *newer* engine while we were transcribing, keep theirs and
                // drop ours, so the switch isn't silently reverted.
                if let Ok(mut g) = self.engine.lock() {
                    if g.is_none() {
                        *g = Some(engine);
                    }
                }
                match transcription {
                    // A question for the Voice Assistant: corrected, then
                    // to Chat and read aloud (assistant.rs). Nothing pasted.
                    Ok(text) if self.assistant.swap(false, Ordering::SeqCst) => {
                        if !self.ask_assistant(text.trim()) {
                            error_msg = Some("Yap didn't catch a question — try again");
                        }
                    }
                    Ok(text) if self.edit_mode.load(Ordering::SeqCst) => {
                        // Edit/rewrite mode: the transcript is an INSTRUCTION, not
                        // dictation. Apply it to the captured selection and paste
                        // the result back. Emits its own final state.
                        self.drop_kept_audio();
                        self.run_rewrite(text.trim().to_string()).await;
                        return;
                    }
                    // A Command by name (port of OpenWhispr's detectAgentName
                    // + dictation routing): a normal dictation that addresses the
                    // agent by name ("Hey {name}, …") routes the WHOLE transcript
                    // through the Commands scope in write mode instead of
                    // cleanup — the agent prompt strips the name + command from
                    // the output. Agent unreachable / no match → normal cleanup.
                    // (Not a question for the Chat view: that already goes to
                    // the AI, name or no name.)
                    Ok(text) if !self.to_chat.load(Ordering::SeqCst) && self.wake_word_hit(text.trim()) => {
                        tracing::info!("Agent's name heard — running the dictation as a Command");
                        self.drop_kept_audio();
                        self.run_agent(text.trim().to_string(), String::new()).await;
                        return;
                    }
                    Ok(text) => {
                        offer = self.deliver(text).await.offer;
                    }
                    Err(e) => {
                        tracing::error!("Transcription failed: {}", e);
                        error_msg = Some("Transcription failed — check your model");
                        // Listed with its audio: run it again (another model?).
                        self.list_failed("The transcription failed");
                    }
                }
            }
            Err(e) => {
                tracing::error!("STT task panicked: {}", e);
                error_msg = Some("Transcription crashed — try another model");
                self.list_failed("The transcription crashed");
            }
        }

        // Audio nothing took (nothing was heard): not kept.
        self.drop_kept_audio();
        // A question for the assistant that never got transcribed.
        if self.assistant.swap(false, Ordering::SeqCst) {
            crate::assistant::nothing(&self.app);
        }
        // Never fail silently: surface a brief error to the UI, otherwise drop
        // back to idle.
        match error_msg {
            Some(msg) => {
                let _ = self.app.emit(DICTATION_ERROR, msg);
                let _ = self.app.emit("yap-state", "error");
            }
            None => self.end_state(offer),
        }
    }

    /// The dictation's last state: "idle", or "cleaned" — the bar's "Cleaned
    /// up · Use what I said" — for [`as_said::OFFER_MS`] when it offers the
    /// swap (`offer`), then "idle" unless something came after it.
    fn end_state(self: &Arc<Self>, offer: Option<u64>) {
        let Some(seq) = offer else {
            let _ = self.app.emit("yap-state", "idle");
            return;
        };
        let _ = self.app.emit("yap-state", "cleaned");
        let shared = Arc::clone(self);
        let _ = std::thread::Builder::new().name("as-said-offer".into()).spawn(move || {
            std::thread::sleep(Duration::from_millis(crate::as_said::OFFER_MS));
            // Still this dictation's offer (not swapped, no newer one) and
            // nothing recording or transcribing: the capsule goes.
            let quiet = !shared.recording.load(Ordering::SeqCst) && !shared.processing.load(Ordering::SeqCst);
            if crate::as_said::offered(seq) && quiet {
                let _ = shared.app.emit("yap-state", "idle");
            }
        });
    }

    /// The dictation never got transcribed (no model, the model failed):
    /// listed in history as failed with its audio, so it can be run again.
    fn list_failed(&self, why: &str) {
        let Some(id) = self.audio_id.lock().ok().and_then(|mut a| a.take()) else {
            return;
        };
        let (history, model) = self
            .config
            .read()
            .map(|c| (c.history_enabled, c.model_size.clone()))
            .unwrap_or((false, String::new()));
        if !history {
            crate::recent_audio::discard(&id);
            return;
        }
        let target = match self.target_hwnd.load(Ordering::Relaxed) {
            0 => None,
            h => Some(h),
        };
        let app_name = crate::text_injector::app_name_for(target).unwrap_or_default();
        crate::history::record_failed(None, &model, &app_name, &id, why);
        crate::recent_audio::finished(&id);
        tracing::info!(id = %id, "A dictation that failed is in history with its audio (Run again)");
        let _ = self.app.emit("yap-history-changed", ());
    }

    /// **Run again** (Home's feed, `recent_audio::history_rerun`): a kept
    /// dictation's `audio` transcribed afresh — on `model` (an installed
    /// one; `None` = the one in use: the warm engine, else loaded for this
    /// and dropped) — then AI cleanup as for its app, the corrections and
    /// snippets. Nothing is pasted. Returns (transcript, text, model id).
    async fn rerun(self: &Arc<Self>, audio: Vec<f32>, model: Option<String>, app_name: &str) -> Result<(String, String, String), String> {
        let cfg = self.config.read().map(|c| c.clone()).map_err(|_| "Yap's settings couldn't be read")?;
        let model_id = model.filter(|m| !m.trim().is_empty()).unwrap_or_else(|| cfg.model_size.clone());
        let data_dir = config::data_dir();
        if !stt::is_model_installed(&data_dir, &model_id) {
            return Err(format!("{} isn't installed", stt::model_name(&model_id)));
        }
        let warm = model_id == cfg.model_size;
        if warm && model_loading() {
            // The startup load is this model: wait for it, not a second copy.
            let _ = tokio::task::spawn_blocking(|| wait_for_model(Duration::from_secs(180))).await;
        }
        let engine = if warm { self.engine.lock().ok().and_then(|mut g| g.take()) } else { None };
        let engine = match engine {
            Some(e) => e,
            None => stt::create_stt_engine(&data_dir, &model_id, cfg.use_gpu)
                .map_err(|e| format!("Couldn't load {}: {e}", stt::model_name(&model_id)))?,
        };
        let (language, translate) = self.language_settings();
        let prompt = if stt::is_whisper_model(&model_id) { self.dictionary_prompt() } else { None };
        tracing::info!(samples = audio.len(), model = %model_id, warm, "Running a dictation again");
        let (engine, result) = tokio::task::spawn_blocking(move || {
            let r = engine.transcribe(&audio, language.as_deref(), translate, prompt.as_deref());
            (engine, r)
        })
        .await
        .map_err(|e| format!("The transcription crashed: {e}"))?;
        if warm {
            if let Ok(mut g) = self.engine.lock() {
                if g.is_none() {
                    *g = Some(engine);
                }
            }
        }
        self.touch_activity();
        let raw = result.map_err(|e| format!("The transcription failed: {e}"))?.trim().to_string();
        if raw.is_empty() {
            return Err("Nothing was heard in it".into());
        }
        let cleaned = self.clean_up(&raw, Some(app_name).filter(|a| !a.is_empty()), None).await;
        let mut c = cfg;
        c.model_size = model_id.clone();
        let (text, _) = finish_dictation(cleaned.trim(), &raw, &c);
        let text = text.trim().to_string();
        if text.is_empty() {
            return Err("Nothing was left after the cleanup".into());
        }
        Ok((raw, text, model_id))
    }

    /// Audio kept for a dictation that ended up nowhere (a Command, nothing
    /// heard): deleted now.
    fn drop_kept_audio(&self) {
        if let Some(id) = self.audio_id.lock().ok().and_then(|mut a| a.take()) {
            crate::recent_audio::discard(&id);
        }
    }
}

/// The mic's audio and news (`mic.rs`) into the pipeline.
impl crate::mic::Sink for Shared {
    fn audio(&self, samples: &[f32]) {
        // The e2e suite's "muted mic", and the robot's WAV playing in the
        // mic's place (debug builds only).
        #[cfg(debug_assertions)]
        let zeros;
        #[cfg(debug_assertions)]
        let samples = if TEST_SILENT_MIC.load(Ordering::Relaxed) {
            zeros = vec![0.0; samples.len()];
            &zeros[..]
        } else {
            samples
        };
        if test_feed_on() {
            return;
        }
        if let Ok(mut amp) = self.amp.lock() {
            self.take_audio(samples, &mut amp);
        }
    }

    fn live(&self) {
        self.fire_pending_start();
    }

    fn opened(&self, device: &str) {
        if let Ok(mut name) = self.mic_name.lock() {
            *name = device.to_string();
        }
        // The pre-roll is this stream's from now on, never a minute-old
        // stream's.
        if !self.recording.load(Ordering::SeqCst) && !self.gather.load(Ordering::SeqCst) {
            if let Ok(mut p) = self.preroll.lock() {
                p.clear();
            }
        }
    }

    fn failed(&self, _error: &str, dictation: Option<u64>) {
        // (The mic's thread: `Weak` gave it `&Shared`; the recording's end
        // needs the `Arc`.)
        if let Some(lease) = dictation {
            if let Some(shared) = self.me.upgrade() {
                shared.mic_failed(lease);
            }
        }
    }
}

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
            mic: crate::mic::Mic::start(me.clone() as Weak<dyn crate::mic::Sink>),
            mic_lease: AtomicU64::new(0),
            mic_failed_lease: AtomicU64::new(0),
            amp: Mutex::new(AmpMeter::default()),
            pending_session: AtomicU64::new(0),
            pending_start: Mutex::new(None),
            me: me.clone(),
            engine: std::sync::Arc::new(Mutex::new(None)),
            app: app.clone(),
            config: RwLock::new(cfg.clone()),
            last_activity: AtomicU64::new(now_ms()),
            target_hwnd: AtomicIsize::new(0),
            target_kind: Mutex::new(AppKind::Other),
            edit_mode: AtomicBool::new(false),
            to_chat: AtomicBool::new(false),
            assistant: AtomicBool::new(false),
            pause: Mutex::new(None),
            selection: Mutex::new(None),
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
        });

        spawn_idle_watcher(&shared);

        // The mic: closed until something needs it, unless "Keep the
        // microphone ready" (then open now, as Yap always was).
        shared.mic.settings(cfg.keep_mic_ready, cfg.input_device.clone());
        // If Windows' privacy switches keep desktop apps off the mic, the log
        // says so now (a recording says it to the person).
        if let Some(blocked) = crate::mic_privacy::blocked() {
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
                if crate::e2e::active() {
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
        crate::stt_speed::schedule(&self.shared.app, crate::stt_speed::AFTER_CHANGE);
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
        if shared.processing.swap(true, Ordering::SeqCst) {
            return Err("Yap is busy transcribing — try again in a moment".into());
        }
        shared.upload_cancel.store(false, Ordering::SeqCst);
        tauri::async_runtime::spawn(async move {
            let flag = Arc::clone(&shared);
            shared.run_file_transcription(path).await;
            flag.processing.store(false, Ordering::SeqCst);
        });
        Ok(())
    }

    /// Cancel an in-flight Upload transcription (takes effect between chunks).
    pub fn cancel_file_transcription(&self) {
        self.shared.upload_cancel.store(true, Ordering::SeqCst);
    }

    /// Replace the live config (e.g. after the user saves settings).
    pub fn update_config(&self, cfg: YapConfig) {
        // "Keep the microphone ready" and the microphone, for the mic.
        self.shared.mic.settings(cfg.keep_mic_ready, cfg.input_device.clone());
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

    /// Swap the capture stream to a different input device **live** (no app
    /// restart) when one is running — the new stream is built first, so on
    /// failure the old one keeps running and an error is returned — else
    /// check the device is there for the next recording.
    pub fn set_input_device(&self, device: Option<&str>) -> Result<(), String> {
        self.shared.mic.switch(device.map(str::to_string))?;
        tracing::info!(device = device.unwrap_or("default"), "Input device switched");
        Ok(())
    }

    /// The mic's state (`mic.rs`): open, live, Bluetooth, how the last open
    /// went — robot mode's `realtest_mic`.
    #[cfg(debug_assertions)]
    pub(crate) fn mic_status(&self) -> crate::mic::Status {
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
}

/// Runs a kept dictation's audio again ([`Pipeline::rerunner`]), never
/// beside a dictation: it holds the `processing` guard meanwhile.
pub(crate) struct Rerun(Arc<Shared>);

impl Rerun {
    /// (transcript, text, model id) — see `Shared::rerun`.
    pub(crate) async fn run(&self, audio: Vec<f32>, model: Option<String>, app_name: &str) -> Result<(String, String, String), String> {
        let shared = &self.0;
        if shared.recording.load(Ordering::SeqCst) || shared.processing.swap(true, Ordering::SeqCst) {
            return Err("Yap is busy with a dictation — try again in a moment".into());
        }
        let out = shared.rerun(audio, model, app_name).await;
        shared.processing.store(false, Ordering::SeqCst);
        out
    }
}

/// Robot mode's dictations (`realtest.rs`, debug builds only): without a
/// hotkey, and from a transcript or a WAV instead of the microphone.
/// Where they paste is up to the guard in `realtest.rs`, as for any
/// dictation in robot mode.
#[cfg(debug_assertions)]
pub(crate) struct Robot(Arc<Shared>);

/// How [`Robot::ask_assistant_audio`] presses the assistant shortcut.
#[cfg(debug_assertions)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Press {
    /// Pressed once to start (and again at the end, if it's still on).
    Toggle,
    /// Held down for the whole audio, let go at its end (push-to-talk).
    Hold,
    /// A double-tap: locked hands-free (push-to-talk), pressed again at the
    /// end if it's still on.
    HandsFree,
}

/// What [`Robot::ask_assistant_audio`] saw.
#[cfg(debug_assertions)]
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AskedAloud {
    /// The recording ended before its audio did, no key pressed: a pause
    /// ended it.
    pub ended_by_itself: bool,
    /// How much of the audio was played before it ended (ms).
    pub listened_ms: u64,
}

/// The robot's WAV plays in the mic's place until this drops.
#[cfg(debug_assertions)]
struct FeedOn;

#[cfg(debug_assertions)]
impl FeedOn {
    fn start() -> FeedOn {
        TEST_FEED.store(true, Ordering::SeqCst);
        FeedOn
    }
}

#[cfg(debug_assertions)]
impl Drop for FeedOn {
    fn drop(&mut self) {
        TEST_FEED.store(false, Ordering::SeqCst);
    }
}

#[cfg(debug_assertions)]
impl Robot {
    const BUSY: &'static str = "Yap is busy with another dictation";

    /// The report of the latest dictation that went to the paste (an Undo's,
    /// say), taken; `None` while none has since this was last asked.
    pub(crate) fn take_delivered(&self) -> Option<Delivered> {
        LAST_DELIVERED.lock().ok().and_then(|mut last| last.take())
    }

    /// `text` as if the STT had just heard it: the window in front becomes
    /// the target and the text around its cursor is read, as at a
    /// recording's start (waiting for that read, as a few seconds of talk
    /// would), then the dictation's own steps from cleanup to the paste.
    pub(crate) async fn dictate_text(&self, text: String) -> Result<Delivered, String> {
        let shared = &self.0;
        if shared.recording.load(Ordering::SeqCst) || shared.processing.swap(true, Ordering::SeqCst) {
            return Err(Self::BUSY.into());
        }
        shared.capture_target(false);
        crate::selection::wait_for_context(Duration::from_millis(1_500));
        crate::screen_terms::wait_for_terms(Duration::from_millis(3_000));
        let _ = shared.app.emit("yap-state", "processing");
        tracing::info!(len = text.len(), "e2e: robot dictation from a transcript");
        let report = shared.deliver(text).await;
        shared.processing.store(false, Ordering::SeqCst);
        shared.end_state(report.offer);
        Ok(report)
    }

    /// [`Robot::dictate_text`] as the Chat view's mic would start it: into
    /// the chat box (`yap-chat-dictation`), never pasted. (Test mode's
    /// `commands::e2e_chat_dictation`.)
    pub(crate) async fn dictate_to_chat(&self, text: String) -> Result<Delivered, String> {
        arm_chat_dictation(true);
        let out = self.dictate_text(text).await;
        arm_chat_dictation(false);
        out
    }

    /// A question for the **Voice Assistant** without a microphone (test
    /// mode's `assistant::e2e_assistant_ask`): what a press of its shortcut
    /// does — the barge-in, the target, "listening" on the capsule for
    /// `listen_ms` (a dictation as the meeting recorder and the speaker see
    /// it) — then `text` as the transcript, from the dictionary on.
    pub(crate) async fn ask_assistant(&self, text: String, listen_ms: u64) -> Result<(), String> {
        let shared = &self.0;
        // As the shortcut: off during meetings and calls.
        if !shared.recording.load(Ordering::SeqCst) && crate::assistant::off_for_meeting(&shared.app) {
            return Err("The Voice Assistant is off during meetings".into());
        }
        if shared.recording.load(Ordering::SeqCst) || shared.processing.swap(true, Ordering::SeqCst) {
            return Err(Self::BUSY.into());
        }
        crate::tts::stop_for_dictation();
        ASSISTANT_ARMED.store(true, Ordering::SeqCst);
        shared.capture_target(false);
        ASSISTANT_ARMED.store(false, Ordering::SeqCst);
        dictation_began();
        let _ = shared.app.emit("yap-state", "recording");
        tokio::time::sleep(Duration::from_millis(listen_ms)).await;
        dictation_ended();
        let _ = shared.app.emit("yap-state", "processing");
        tracing::info!(len = text.len(), "e2e: a question for the Voice Assistant from a transcript");
        let asked = shared.assistant.swap(false, Ordering::SeqCst) && shared.ask_assistant(text.trim());
        shared.processing.store(false, Ordering::SeqCst);
        let _ = shared.app.emit("yap-state", "idle");
        if asked {
            Ok(())
        } else {
            crate::assistant::nothing(&shared.app);
            Err("Nothing to ask".into())
        }
    }

    /// A question to the **Voice Assistant** from `samples` (16 kHz mono)
    /// played in the mic's place in real time, its shortcut pressed as
    /// `press` says through the real key path (the recording mode and
    /// push-to-talk's gestures as configured): the start, "speak now", the
    /// live preview, the pause watch, and at the end of the audio the
    /// shortcut again if the recording is still on — then the STT and the
    /// question to Chat, as ever. Test mode's `e2e::e2e_assistant_audio`.
    pub(crate) async fn ask_assistant_audio(&self, samples: Vec<f32>, press: Press) -> Result<AskedAloud, String> {
        let shared = Arc::clone(&self.0);
        if shared.recording.load(Ordering::SeqCst) || shared.processing.load(Ordering::SeqCst) {
            return Err(Self::BUSY.into());
        }
        if crate::assistant::off_for_meeting(&shared.app) {
            return Err("The Voice Assistant is off during meetings".into());
        }
        let _feed = FeedOn::start();
        if let Ok(mut pre) = shared.preroll.lock() {
            pre.clear();
        }
        let key = |pressed: bool| shared.assistant_key(pressed, None);
        key(true);
        if !shared.recording.load(Ordering::SeqCst) {
            return Err("The recording didn't start (see the log)".into());
        }
        let session = shared.rec_session.load(Ordering::SeqCst);
        match press {
            // A toggle's release does nothing; a hold stays down.
            Press::Toggle => key(false),
            Press::Hold => {}
            // A quick tap, then a second press: locked hands-free.
            Press::HandsFree => {
                tokio::time::sleep(Duration::from_millis(80)).await;
                key(false);
                tokio::time::sleep(Duration::from_millis(120)).await;
                key(true);
                tokio::time::sleep(Duration::from_millis(80)).await;
                key(false);
            }
        }
        tracing::info!(secs = samples.len() / TARGET_SAMPLE_RATE as usize, ?press, "e2e: a question for the Voice Assistant from audio");
        let feeder = Arc::clone(&shared);
        let played = tokio::task::spawn_blocking(move || {
            let mut amp = AmpMeter::default();
            let started = Instant::now();
            let chunk = TARGET_SAMPLE_RATE as usize / 10;
            for (i, part) in samples.chunks(chunk).enumerate() {
                let due = Duration::from_millis(100 * i as u64);
                if let Some(wait) = due.checked_sub(started.elapsed()) {
                    std::thread::sleep(wait);
                }
                if !feeder.recording.load(Ordering::SeqCst) || feeder.rec_session.load(Ordering::SeqCst) != session {
                    return 100 * i as u64;
                }
                feeder.take_audio(part, &mut amp);
                if i == 0 {
                    feeder.fire_pending_start();
                }
            }
            (samples.len() / (TARGET_SAMPLE_RATE as usize / 1000)) as u64
        })
        .await
        .map_err(|e| format!("The audio player failed: {e}"))?;
        // Still listening at the end of the audio: the shortcut ends it.
        let ended_by_itself = !(shared.recording.load(Ordering::SeqCst) && shared.rec_session.load(Ordering::SeqCst) == session);
        if !ended_by_itself {
            match press {
                Press::Hold => key(false),
                Press::Toggle | Press::HandsFree => {
                    key(true);
                    key(false);
                }
            }
        }
        let deadline = Instant::now() + Duration::from_secs(120);
        while shared.processing.load(Ordering::SeqCst) || shared.recording.load(Ordering::SeqCst) {
            if Instant::now() > deadline {
                return Err("The question wasn't transcribed within 2 minutes".into());
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        Ok(AskedAloud { ended_by_itself, listened_ms: played })
    }

    /// `samples` (16 kHz mono) played into the dictation in real time, in
    /// the mic's place, between a real start and a real stop: the target
    /// captured at the start, the live preview, the dead-mic watchdog, the
    /// STT, then the dictation's own steps to the paste. `None`: nothing was
    /// transcribed (a silent "mic", say). `cancel`: the recording is
    /// cancelled instead of stopped, as the bar's ✕ does (kept for Undo).
    pub(crate) async fn dictate_audio(&self, samples: Vec<f32>, cancel: bool) -> Result<Option<Delivered>, String> {
        let shared = Arc::clone(&self.0);
        if shared.recording.load(Ordering::SeqCst) || shared.processing.load(Ordering::SeqCst) {
            return Err(Self::BUSY.into());
        }
        if let Ok(mut last) = LAST_DELIVERED.lock() {
            *last = None;
        }
        let _feed = FeedOn::start();
        // No room audio from before the WAV in the recording.
        if let Ok(mut pre) = shared.preroll.lock() {
            pre.clear();
        }
        tracing::info!(secs = samples.len() / TARGET_SAMPLE_RATE as usize, "e2e: robot dictation from a WAV");
        shared.start_recording(false);
        if !shared.recording.load(Ordering::SeqCst) {
            return Err("The recording didn't start (see the log)".into());
        }
        let session = shared.rec_session.load(Ordering::SeqCst);
        let feeder = Arc::clone(&shared);
        tokio::task::spawn_blocking(move || {
            let mut amp = AmpMeter::default();
            let started = Instant::now();
            // 100 ms of audio every 100 ms, as a mic delivers it.
            for (i, chunk) in samples.chunks(TARGET_SAMPLE_RATE as usize / 10).enumerate() {
                let due = Duration::from_millis(100 * i as u64);
                if let Some(wait) = due.checked_sub(started.elapsed()) {
                    std::thread::sleep(wait);
                }
                if !feeder.recording.load(Ordering::SeqCst) || feeder.rec_session.load(Ordering::SeqCst) != session {
                    break;
                }
                feeder.take_audio(chunk, &mut amp);
                // The WAV is the mic here: its first audio says "speak now".
                if i == 0 {
                    feeder.fire_pending_start();
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        })
        .await
        .map_err(|e| format!("The WAV player failed: {e}"))?;
        // Stop as the hotkey would (or cancel, as the bar's ✕), if it's
        // still this recording.
        if shared.recording.load(Ordering::SeqCst) && shared.rec_session.load(Ordering::SeqCst) == session {
            if cancel {
                shared.cancel(true);
                return Ok(None);
            }
            shared.stop_and_transcribe();
        }
        // The transcription and the paste.
        let deadline = Instant::now() + Duration::from_secs(300);
        while shared.processing.load(Ordering::SeqCst) || shared.recording.load(Ordering::SeqCst) {
            if Instant::now() > deadline {
                return Err("The dictation didn't finish within 5 minutes".into());
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        Ok(LAST_DELIVERED.lock().ok().and_then(|mut last| last.take()))
    }
}

/// Streaming-partials worker. Runs on its own thread for one recording session
/// and exits when `recording` flips false.
///
/// Each tick it transcribes only a bounded **sliding window** of the buffer
/// (`partials::PartialSession`): audio older than the window is frozen as
/// committed text, so per-tick cost is independent of recording length. It
/// never blocks the authoritative final pass: it grabs the engine with
/// `try_lock` and skips the tick if the engine is busy or absent (taken by a
/// meeting chunk; the final pass only takes it after `recording` flips false).
fn stream_partials(shared: Arc<Shared>, language: Option<String>, translate: bool) {
    const BASE_INTERVAL: Duration = Duration::from_millis(500);
    const MAX_INTERVAL: Duration = Duration::from_secs(3);
    // Don't bother until the window has at least ~0.5 s of audio (avoids
    // hallucinated output on tiny snippets — mirrors the engine's own guard).
    const STREAM_MIN_SAMPLES: usize = 8_000;

    // Dictionary vocabulary bias (Whisper models only) — snapshotted once per
    // session like `language`/`translate`.
    let dict_prompt = shared.dictionary_prompt();

    // If the idle watcher unloaded the model, warm-load it now — otherwise a
    // session started after an idle unload silently produces no partials at
    // all (only the final pass lazily reloads).
    ensure_partial_engine(&shared);

    // Transcription outcome of one tick (built while holding the engine).
    enum Out {
        Normal(String),
        Advance { commit: String, tail: String, cut: usize },
    }

    let mut session = crate::partials::PartialSession::default();
    let mut last_emitted = String::new();
    let mut interval = BASE_INTERVAL;
    loop {
        std::thread::sleep(interval);
        if !shared.recording.load(Ordering::SeqCst) {
            break;
        }

        // Snapshot this tick's window under the buffer lock — only the bounded
        // window is cloned, never the whole recording.
        let (win, rel_cut, abs_cut, len) = {
            let buf = match shared.buffer.lock() {
                Ok(b) => b,
                Err(_) => break,
            };
            let len = buf.len();
            if len.saturating_sub(session.window_start) < STREAM_MIN_SAMPLES {
                continue;
            }
            match session.plan(len, &buf) {
                crate::partials::TickPlan::Normal => {
                    (buf[session.window_start..len].to_vec(), None, 0usize, len)
                }
                crate::partials::TickPlan::Advance { cut } => (
                    buf[session.window_start..len].to_vec(),
                    Some(cut - session.window_start),
                    cut,
                    len,
                ),
            }
        };

        // Re-entrancy/contention guard: only transcribe if the engine is free.
        let started = Instant::now();
        let out = {
            let guard = match shared.engine.try_lock() {
                Ok(g) => g,
                Err(_) => continue, // a transcription is already running — skip
            };
            match guard.as_ref() {
                Some(engine) => {
                    if !shared.recording.load(Ordering::SeqCst) {
                        break;
                    }
                    // On an advance, both halves are transcribed under one
                    // engine hold and emitted once, so the display never
                    // shrinks at the seam.
                    let res = match rel_cut {
                        None => engine
                            .transcribe(&win, language.as_deref(), translate, dict_prompt.as_deref())
                            .map(Out::Normal),
                        Some(c) => {
                            engine
                                .transcribe(&win[..c], language.as_deref(), translate, dict_prompt.as_deref())
                                .and_then(|commit| {
                                    engine
                                        .transcribe(&win[c..], language.as_deref(), translate, dict_prompt.as_deref())
                                        .map(|tail| Out::Advance {
                                            commit,
                                            tail,
                                            cut: abs_cut,
                                        })
                                })
                        }
                    };
                    match res {
                        Ok(o) => o,
                        Err(e) => {
                            tracing::debug!("partial transcribe skipped: {}", e);
                            continue;
                        }
                    }
                }
                // Engine absent: a meeting chunk has it (it comes back — keep
                // polling). If the final pass took it, `recording` is already
                // false and the loop-top check exits next tick.
                None => continue,
            }
        };

        match out {
            Out::Normal(text) => session.apply_normal(&text),
            Out::Advance { commit, tail, cut } => session.apply_advance(&commit, &tail, cut),
        }
        shared.touch_activity();

        // Self-throttle when transcription is slow (e.g. CPU fallback): back
        // off the tick so the preview never saturates the machine.
        let elapsed = started.elapsed();
        interval = if elapsed > Duration::from_secs(1) {
            (elapsed * 2).min(MAX_INTERVAL)
        } else {
            BASE_INTERVAL
        };

        let display = session.display();
        // A Voice Assistant question's pause watch reads the words too: what
        // the first `len` samples said (`pause.rs`).
        shared.pause_words(len, &display);
        if !display.is_empty() && display != last_emitted {
            // Recording may have stopped while we were transcribing — don't emit
            // a stale partial over the authoritative final transcript.
            if !shared.recording.load(Ordering::SeqCst) {
                break;
            }
            last_emitted = display.clone();
            let _ = shared.app.emit("yap-partial", display);
        }
    }
}

/// Warm-load the shared engine for the partials worker if the slot is empty
/// (e.g. after an idle unload). Best-effort: on failure the worker simply runs
/// partial-less and the final pass surfaces `needs-model` as before.
fn ensure_partial_engine(shared: &Arc<Shared>) {
    // The startup load will put it there (the final pass waits for it).
    if model_loading() {
        return;
    }
    let empty = shared
        .engine
        .try_lock()
        .map(|g| g.is_none())
        .unwrap_or(false);
    if !empty {
        return;
    }
    let (model_size, use_gpu) = shared
        .config
        .read()
        .map(|c| (c.model_size.clone(), c.use_gpu))
        .unwrap_or_else(|_| (String::new(), true));
    match stt::create_stt_engine(&config::data_dir(), &model_size, use_gpu) {
        Ok(e) => {
            // Place back only if still empty — a model switch or lazy reload
            // that landed meanwhile wins (same pattern as run_stt's put-back).
            if let Ok(mut g) = shared.engine.lock() {
                if g.is_none() {
                    tracing::info!(model = %model_size, "Warm-loaded model for the live preview");
                    *g = Some(e);
                }
            }
        }
        Err(e) => tracing::debug!("partials warm-load skipped: {}", e),
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
    use crate::config::DictionaryEntry;

    fn entry(from: &str, to: &str, fuzzy: bool) -> DictionaryEntry {
        DictionaryEntry {
            from: from.to_string(),
            to: to.to_string(),
            fuzzy,
            snippet: false,
        }
    }

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
        assert_eq!(destination(false, true, false, false, false), Goes::Assistant);
        assert_eq!(destination(false, true, true, true, true), Goes::Assistant);
        // The edit hotkey's recording stays a rewrite.
        assert_eq!(destination(true, true, false, false, false), Goes::Paste);
        // Without the shortcut: as before.
        assert_eq!(destination(false, false, true, false, false), Goes::ChatBox);
        assert_eq!(destination(false, false, false, true, true), Goes::ChatBox);
        assert_eq!(destination(false, false, false, true, false), Goes::Paste);
        assert_eq!(destination(false, false, false, false, false), Goes::Paste);
    }

    #[test]
    fn press_enter_at_the_end_is_taken_off() {
        let t = |text: &str| take_voice_enter(text, text);
        assert_eq!(t("Sounds good press enter").as_deref(), Some("Sounds good"));
        assert_eq!(t("Sounds good, press enter.").as_deref(), Some("Sounds good."));
        assert_eq!(t("Sounds good. Press Enter.").as_deref(), Some("Sounds good."));
        assert_eq!(t("Can you check this — PRESS RETURN?").as_deref(), Some("Can you check this?"));
        assert_eq!(t("Done!\npress-enter").as_deref(), Some("Done!"));
        assert_eq!(t("He said \"hi\", press enter").as_deref(), Some("He said \"hi\""));
        // Nothing but the command: nothing to paste, still a press.
        assert_eq!(t("Press enter.").as_deref(), Some(""));
        assert_eq!(t("press return").as_deref(), Some(""));
    }

    #[test]
    fn press_enter_only_counts_as_the_last_words() {
        let t = |text: &str| take_voice_enter(text, text);
        assert_eq!(t("press enter and wait for the prompt"), None);
        assert_eq!(t("I pressed enter"), None);
        assert_eq!(t("press the enter key"), None);
        assert_eq!(t("express enter"), None);
        assert_eq!(t("press enters"), None);
        assert_eq!(t("enter"), None);
        assert_eq!(t(""), None);
    }

    #[test]
    fn press_enter_dropped_by_cleanup_is_found_in_the_raw_transcript() {
        // Cleanup took the command out: the text stays as cleanup left it.
        assert_eq!(
            take_voice_enter("Sounds good.", "sounds good press enter").as_deref(),
            Some("Sounds good.")
        );
        // Said nowhere: nothing.
        assert_eq!(take_voice_enter("Sounds good.", "sounds good"), None);
    }

    #[test]
    fn a_dictation_gets_corrections_then_the_voice_command_then_snippets() {
        let mut dict = vec![entry("jaison", "JSON", true)];
        dict.push(DictionaryEntry { snippet: true, ..entry("my address", "1 Long Lane\nLeeds", true) });
        let mut c = cfg("parakeet-tdt-0.6b-v3", true, dict);
        // A snippet of several lines ending the dictation ends where its own
        // text does (no "Norwich.").
        assert_eq!(
            finish_dictation("The jaison is at my address, press enter.", "x", &c),
            ("The JSON is at 1 Long Lane\nLeeds".to_string(), true)
        );
        // The command off first, so the snippet is all that's left: exactly its text.
        assert_eq!(
            finish_dictation("My address, press enter.", "x", &c),
            ("1 Long Lane\nLeeds".to_string(), true)
        );
        assert_eq!(finish_dictation("Hi.", "hi", &c), ("Hi.".to_string(), false));
        // Switched off: the words stay and nothing is pressed.
        c.voice_enter = false;
        assert_eq!(
            finish_dictation("Hi, press enter.", "hi press enter", &c),
            ("Hi, press enter.".to_string(), false)
        );
    }

    #[test]
    fn corrections_leave_snippets_to_the_dictation() {
        // Meetings and Upload run apply_corrections alone: a snippet's phrase
        // stays as said there.
        let snip = DictionaryEntry { snippet: true, ..entry("my address", "1 Long Lane", true) };
        let c = cfg("parakeet-tdt-0.6b-v3", true, vec![snip]);
        assert_eq!(apply_corrections("at my address", &c), "at my address");
    }

    fn cfg(model: &str, fuzzy: bool, dictionary: Vec<DictionaryEntry>) -> YapConfig {
        YapConfig {
            model_size: model.to_string(),
            dictionary_fuzzy: fuzzy,
            dictionary,
            ..YapConfig::default()
        }
    }

    #[test]
    fn corrections_are_exact_then_fuzzy_for_onnx_models() {
        let dict = vec![entry("jaison", "JSON", true), entry("chat gpt", "ChatGPT", true)];
        let parakeet = cfg("parakeet-tdt-0.6b-v3", true, dict.clone());
        // Exact (case-insensitive, whole words) and a near-miss ("jaisen").
        assert_eq!(
            apply_corrections("ask Chat GPT about the jaisen file", &parakeet),
            "ask ChatGPT about the JSON file"
        );
        assert_eq!(apply_corrections("the jaison file", &parakeet), "the JSON file");
        // Whisper had the words as its prompt: exact replacements only.
        let whisper = cfg("turbo", true, dict.clone());
        assert_eq!(
            apply_corrections("the jaisen and jaison files", &whisper),
            "the jaisen and JSON files"
        );
        // "Catch near-misses" off: exact only.
        let off = cfg("parakeet-tdt-0.6b-v3", false, dict);
        assert_eq!(apply_corrections("the jaisen file", &off), "the jaisen file");
    }

    #[test]
    fn corrections_respect_the_per_entry_opt_out() {
        let c = cfg("parakeet-tdt-0.6b-v3", true, vec![entry("json", "JSON", false)]);
        assert_eq!(apply_corrections("Dear Jason, the json is attached", &c), "Dear Jason, the JSON is attached");
    }

    #[test]
    fn no_dictionary_leaves_the_text_alone() {
        let c = cfg("parakeet-tdt-0.6b-v3", true, Vec::new());
        assert_eq!(apply_corrections("  two  spaces\tand a tab ", &c), "  two  spaces\tand a tab ");
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
