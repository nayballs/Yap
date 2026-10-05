//! Meeting recorder — "the notepad that cleans up after your meetings"
//! (OpenWhispr `meetingRecordingStore.ts`, ported to Yap's local-first stack).
//!
//! Captures TWO audio streams while a meeting note is open:
//! - the **mic** ("You") — same device dictation uses, its own cpal stream
//! - the **system output** ("Them") — what the call plays through the
//!   speakers, via **WASAPI loopback** (cpal on Windows: build an *input*
//!   stream on an *output* device), so it works with any call app, no bot.
//!   It follows Windows' default output device: plug in a headset, or pick
//!   another output in Windows, and the loopback stream reopens on it
//!   within ~2 s (see "Following the output device" below).
//!
//! A worker cuts each source into ~15 s chunks — at the quietest moment of
//! the last few seconds (`media::quietest_frame`), so a cut lands between
//! words — and transcribes them on the SAME warm engine dictation uses
//! (`pipeline::EngineSlot`, taken per chunk and returned — never held for
//! the whole meeting, so hotkey dictation still works between chunks). If the
//! engine falls behind, the backlog is worked off in ≤30 s chunks back to
//! back; each source buffers at most 20 minutes, after which newer audio is
//! dropped and the UI says transcription is falling behind (memory stays
//! bounded however long the meeting runs).
//!
//! Segments `{source: "you"|"them", text, ts}` (ts = when that audio was
//! captured) go through the correction dictionary exactly as a dictation
//! does (`pipeline::apply_corrections`), then are emitted live
//! (`yap-meeting-segment`), persisted to the note's `transcript` as they come
//! (a crash loses at most one chunk), and handed to the rolling digests
//! (`meeting_summary.rs`) that keep the end-of-meeting action plan fast for
//! meetings of any length.
//!
//! Unlike OpenWhispr there is NO realtime-cloud path — chunks are transcribed
//! locally, so "live" means ~15 s behind, fully offline.
//!
//! Echo: with speakers instead of headphones the mic hears "Them" too. A
//! "you" chunk that only repeats the call — same words, and a loudness that
//! follows the call audio a moment later — is flagged `echo` (kept, hidden,
//! left out of summaries; see "Echo" below). A chunk where the user also
//! spoke is kept whole, so the UI still recommends headphones. Telling the
//! people on the call apart (diarization) is a later item: "Them" is
//! everyone else.
//!
//! Dictation: a hotkey dictation mid-meeting (a Slack reply, an email) is
//! the person talking to their PC, not to the meeting. While one records,
//! and for its pre-roll just before the press, the "You" side gets silence
//! instead, and the transcript a "you dictated here" marker that summaries
//! skip (see "Dictation" below). "Them" carries on.
//!
//! Quiet sides: one side heard nothing for 3 minutes while the other talked
//! (the call plays through a headset that isn't Windows' default output, or
//! the mic is muted or the wrong one): the person is told once, in Yap
//! (`yap-meeting-warning`) and, when no Yap window that shows it is in view,
//! with a Windows notification (see "Quiet sides" below).
//!
//! In e2e test runs (debug builds, `YAP_E2E=1`) no device is opened: the
//! audio comes from test files, or nowhere (`e2e::spawn_meeting_audio`),
//! which can also stage a dictation and an output-device switch; and
//! `e2e_meeting_quiet` shortens the quiet-side timings.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use tauri::{AppHandle, Emitter, Manager};

use crate::notes::TranscriptSegment;
use crate::pipeline::{resample_linear, EngineSlot, TARGET_SAMPLE_RATE};

const RATE: usize = TARGET_SAMPLE_RATE as usize;
/// How often the worker looks for finished chunks.
const DRAIN_INTERVAL_SECS: u64 = 15;
/// A chunk is cut once a source has buffered this much audio…
const CHUNK_SECS: usize = 15;
/// …at the quietest moment of the last few seconds…
const CUT_SEARCH_SECS: usize = 4;
/// …short of the newest bit, where a word may still be going on. A backlog
/// keeps it too. (At least [`DICTATION_LOOKBACK`]: a dictation's pre-roll
/// must still be buffered when the dictation starts, to be blanked.)
const CUT_MARGIN_MS: usize = 400;
/// A backlog is worked off in chunks of at most this much.
const CHUNK_MAX_SECS: usize = 30;
/// Don't transcribe less than this much audio.
const MIN_CHUNK_SECS: usize = 1;
/// Skip chunks whose peak is below this — silence (esp. loopback when nobody
/// speaks) wastes engine time and tempts whisper into hallucinations. Also
/// the level the quiet-side watch counts as hearing something.
const SILENCE_PEAK: f32 = 0.008;
/// Per-source buffer cap (~77 MB of 16 kHz f32). The worker keeps a buffer
/// near one chunk, so this is only reached when transcription can't keep up.
pub(crate) const MAX_BUFFER_SAMPLES: usize = 20 * 60 * RATE;

// ---- Dictation: keeping hotkey dictations out of "You" ----
//
// The meeting's mic stream and the dictation pipeline's are two streams on
// the same mic, so without this a Slack reply dictated mid-call lands in the
// meeting's "You" transcript too, can become a task in the action plan, and
// travels wherever the notes go. The pipeline signals each dictation with one
// atomic (`pipeline::dictation_state`: a count of dictations started, and
// whether one records now). The mic's push path reads it once per callback,
// lock-free, and while a dictation records writes silence instead of the
// audio (same length, so segment times and the echo check stay aligned).
// When it sees a new one start, it also blanks the newest audio it already
// buffered: the dictation's pre-roll (the pipeline prepends the ~300 ms
// before the press). The push that sees a dictation end is blanked too (it
// may hold its last moment). Each blanked span leaves a "you dictated here"
// marker in the transcript (a segment with `dictated: true`, dated where the
// span starts: `TranscriptSegment::dictation_marker`), which digests, the
// action plan, "What did I miss?" and the Ask context skip like echo. Plain
// dictation, the edit hotkey and the wake-word path all record through
// `start_recording`, so all count.

/// Blanked before a dictation: its pre-roll, plus 40 ms for the two
/// streams' callbacks not lining up.
const DICTATION_LOOKBACK: usize = crate::pipeline::PREROLL_SAMPLES + RATE * 40 / 1000;

/// Where hotkey dictations go silent in the mic's audio (one per mic source;
/// see "Dictation" above).
#[derive(Debug)]
struct DictationGate {
    /// `pipeline::dictation_state()`'s count when last looked.
    seen: u64,
    /// Where the blanked span started (session position), while one is open.
    open: Option<u64>,
    /// Where blanked spans started that the transcript has no marker for yet.
    unmarked: Vec<u64>,
}

impl DictationGate {
    /// A gate for a recording starting now, with the pipeline's dictation
    /// state as it is: a dictation that ended before the meeting started is
    /// none of its business; one still recording is blanked from the start.
    fn new((count, _recording): (u64, bool)) -> Self {
        DictationGate {
            seen: count,
            open: None,
            unmarked: Vec::new(),
        }
    }

    /// Before a push, with the pipeline's dictation state: whether to blank
    /// it. A dictation that began since the last push (even one that already
    /// ended: it fell between two callbacks) opens a span and blanks its
    /// pre-roll, the newest of `buffered` (which ends at session position
    /// `end`). A span that's open stays open across back-to-back dictations.
    fn before_push(&mut self, (count, recording): (u64, bool), buffered: &mut [f32], end: u64) -> bool {
        let began = count != self.seen || (recording && self.open.is_none());
        self.seen = count;
        if began && self.open.is_none() {
            let k = DICTATION_LOOKBACK.min(buffered.len());
            let n = buffered.len();
            buffered[n - k..].fill(0.0);
            let from = end - k as u64;
            self.open = Some(from);
            self.unmarked.push(from);
        }
        self.open.is_some()
    }

    /// After a push: the span closes once no dictation records.
    fn after_push(&mut self, recording: bool) {
        if !recording {
            self.open = None;
        }
    }

    /// Markers due in a chunk that ends at session position `end`: the spans
    /// that started before it.
    fn marks_before(&mut self, end: u64) -> Vec<u64> {
        let due = self.unmarked.iter().take_while(|&&from| from < end).count();
        self.unmarked.drain(..due).collect()
    }
}

// ---- Keeping both sides on one clock ----
//
// A position in a source's audio is a time in the meeting (samples since it
// started), which dates its segments and lines "You" up with "Them" for the
// echo check. That only holds if neither side skips time. WASAPI loopback
// delivers nothing while nothing plays on the device, and a device switch
// leaves a gap; so a side that has had no audio from its device while the
// other moved on half a second is filled with silence up to the other
// ([`keep_in_step`], every 200 ms on the capture thread).

/// A side whose device was silent while the other moved on this much (0.5 s)
/// has stalled: its gap is filled with silence.
const STALL_SAMPLES: u64 = RATE as u64 / 2;

/// One source's captured audio, shared by whatever captures it (a device
/// callback, the e2e player), which appends, and the worker, which takes
/// chunks off the front. Positions are samples on the session clock.
pub(crate) struct Captured {
    /// Audio the worker hasn't taken yet.
    samples: Vec<f32>,
    /// Session position of `samples[0]`.
    start: u64,
    /// Just past the last audio the device delivered (filled gaps aside).
    real_end: u64,
    /// Just past the last audio above the silence gate: the device works.
    heard: u64,
    /// How much audio above the silence gate went into the meeting.
    talk: u64,
    /// The mic's (see "Dictation" above). `None` for "Them".
    gate: Option<DictationGate>,
}

/// One source's audio, shared by its capture and the worker.
pub(crate) type AudioBuf = Arc<Mutex<Captured>>;

impl Captured {
    fn new(gate: Option<DictationGate>) -> Self {
        Captured {
            samples: Vec::new(),
            start: 0,
            real_end: 0,
            heard: 0,
            talk: 0,
            gate,
        }
    }

    /// The mic ("You"), whose dictations go silent.
    fn mic() -> AudioBuf {
        let gate = DictationGate::new(crate::pipeline::dictation_state());
        Arc::new(Mutex::new(Captured::new(Some(gate))))
    }

    /// The call ("Them").
    fn call() -> AudioBuf {
        Arc::new(Mutex::new(Captured::new(None)))
    }

    /// Session position just past the newest audio.
    fn end(&self) -> u64 {
        self.start + self.samples.len() as u64
    }

    /// Append audio from the device. `dictation` is the pipeline's state
    /// (read for the mic only). False when the buffer was full and the audio
    /// was dropped.
    fn push(&mut self, samples: &[f32], dictation: Option<(u64, bool)>) -> bool {
        let end = self.end();
        let blank = match (self.gate.as_mut(), dictation) {
            (Some(gate), Some(state)) => gate.before_push(state, &mut self.samples, end),
            _ => false,
        };
        let fits = self.samples.len() + samples.len() <= MAX_BUFFER_SAMPLES;
        if fits {
            let n = samples.len();
            if blank {
                self.samples.resize(self.samples.len() + n, 0.0);
            } else {
                self.samples.extend_from_slice(samples);
            }
            self.real_end = end + n as u64;
            // The device hears something (a dictation counts: the mic works);
            // only what went into the meeting is talk.
            if peak(samples) >= SILENCE_PEAK {
                self.heard = self.real_end;
                if !blank {
                    self.talk += n as u64;
                }
            }
        }
        if let (Some(gate), Some((_, recording))) = (self.gate.as_mut(), dictation) {
            gate.after_push(recording);
        }
        fits
    }

    /// Fill this side's gap with silence up to `other_end` (the other side's
    /// newest audio) if its device has been silent for [`STALL_SAMPLES`] of
    /// it. As much as fits under the buffer cap.
    fn fill_if_stalled(&mut self, other_end: u64) {
        let end = self.end();
        if other_end <= end || other_end - self.real_end < STALL_SAMPLES {
            return;
        }
        let room = MAX_BUFFER_SAMPLES.saturating_sub(self.samples.len());
        let n = ((other_end - end) as usize).min(room);
        self.samples.resize(self.samples.len() + n, 0.0);
    }

    /// Take the next chunk (see [`next_cut`]); the rest stays queued.
    fn take_chunk(&mut self, final_drain: bool) -> Option<Chunk> {
        let cut = next_cut(&self.samples, final_drain)?;
        let rest = self.samples.split_off(cut);
        let samples = std::mem::replace(&mut self.samples, rest);
        let from = self.start;
        self.start += cut as u64;
        let end = self.start;
        let dictated = self
            .gate
            .as_mut()
            .map(|g| g.marks_before(end))
            .unwrap_or_default();
        Some(Chunk {
            samples,
            from,
            dictated,
        })
    }

    /// Dictation markers no chunk carried (one began right at the stop).
    fn take_unmarked(&mut self) -> Vec<u64> {
        self.gate
            .as_mut()
            .map(|g| std::mem::take(&mut g.unmarked))
            .unwrap_or_default()
    }

    /// What the quiet-side watch needs.
    fn levels(&self) -> Levels {
        Levels {
            end: self.end(),
            heard: self.heard,
            talk: self.talk,
        }
    }
}

/// A chunk taken off a source.
struct Chunk {
    samples: Vec<f32>,
    /// Session position of `samples[0]`.
    from: u64,
    /// Where dictations began that are due a marker with this chunk.
    dictated: Vec<u64>,
}

// ---- Echo: the call's audio leaking from the speakers into the mic ----
//
// On speakers (no headphones) the mic hears "Them" too, so the "You"
// transcript repeats what they said — and a summary could hand "I'll do it"
// to the wrong person. A "you" chunk is flagged as echo only when BOTH hold:
// its words mostly repeat what "them" said around then (in order), AND the
// mic's loudness follows the call audio within a short delay (sound that
// came out of the speakers). The second keeps a headphone user's own
// "yes, I'll send the budget by Friday" — which repeats the question's
// words but not its timing — from ever being taken for echo. Flagged
// segments stay in the note (hidden, left out of summaries); a chunk where
// the user also spoke is kept whole. (OpenWhispr's meetingEchoLeakDetector
// does the audio side with sample correlation and AEC; this is the light,
// offline version.)

/// Envelope frame: 20 ms.
const ENV_FRAME: usize = RATE / 50;
/// Speaker → room → mic delay searched, in frames (600 ms).
const ECHO_MAX_LAG: u64 = 30;
/// Compare at least this much audio (3 s) before calling anything echo.
const ECHO_MIN_OVERLAP: usize = 150;
/// The mic's loudness must follow the call this closely (Pearson r)…
const ECHO_MIN_CORRELATION: f32 = 0.6;
/// …and this share of its words repeat theirs, in order.
const ECHO_MIN_SHARE: f32 = 0.6;
/// Envelope history kept per source (~2 minutes).
const ENV_KEEP_FRAMES: usize = 6_000;

/// One source's loudness, 20 ms RMS frames on the session's sample clock.
#[derive(Default)]
struct Envelope {
    frames: std::collections::VecDeque<f32>,
    /// Absolute index of `frames[0]`.
    first: u64,
    /// Samples short of a whole frame, waiting for the next chunk.
    carry: Vec<f32>,
}

impl Envelope {
    /// Append the source's next audio (chunks arrive in order, no gaps).
    fn push(&mut self, samples: &[f32]) {
        self.carry.extend_from_slice(samples);
        let (frames, _) = self.carry.as_chunks::<ENV_FRAME>();
        let whole = frames.len() * ENV_FRAME;
        for frame in frames {
            let ms = frame.iter().map(|s| s * s).sum::<f32>() / ENV_FRAME as f32;
            self.frames.push_back(ms.sqrt());
        }
        self.carry.drain(..whole);
        while self.frames.len() > ENV_KEEP_FRAMES {
            self.frames.pop_front();
            self.first += 1;
        }
    }

    /// Frames `[from, to)` (absolute), clipped to what's kept: (start, frames).
    fn range(&self, from: u64, to: u64) -> (u64, Vec<f32>) {
        let end = self.first + self.frames.len() as u64;
        let (from, to) = (from.max(self.first), to.min(end));
        if from >= to {
            return (from, Vec::new());
        }
        let frames = self
            .frames
            .range((from - self.first) as usize..(to - self.first) as usize)
            .copied()
            .collect();
        (from, frames)
    }
}

fn pearson(x: &[f32], y: &[f32]) -> f32 {
    let n = x.len() as f32;
    let (mx, my) = (x.iter().sum::<f32>() / n, y.iter().sum::<f32>() / n);
    let (mut sxy, mut sxx, mut syy) = (0.0f32, 0.0f32, 0.0f32);
    for (a, b) in x.iter().zip(y) {
        sxy += (a - mx) * (b - my);
        sxx += (a - mx) * (a - mx);
        syy += (b - my) * (b - my);
    }
    if sxx <= f32::EPSILON || syy <= f32::EPSILON {
        return 0.0;
    }
    sxy / (sxx.sqrt() * syy.sqrt())
}

/// How closely the mic's envelope (`mic`, from absolute frame `mic_from`)
/// follows the call's (`call`, from `call_from`) delayed by 0..=600 ms: the
/// best Pearson r over the lags, 0 without 3 s of overlap to compare.
fn echo_correlation(mic: &[f32], mic_from: u64, call: &[f32], call_from: u64) -> f32 {
    let mut best = 0.0f32;
    for lag in 0..=ECHO_MAX_LAG {
        // mic[i] pairs with call[i - lag], absolute frame indices.
        let lo = mic_from.max(call_from + lag);
        let hi = (mic_from + mic.len() as u64).min(call_from + call.len() as u64 + lag);
        if hi <= lo || ((hi - lo) as usize) < ECHO_MIN_OVERLAP {
            continue;
        }
        let m = &mic[(lo - mic_from) as usize..(hi - mic_from) as usize];
        let c = &call[(lo - lag - call_from) as usize..(hi - lag - call_from) as usize];
        best = best.max(pearson(m, c));
    }
    best
}

/// Content words (lowercase, 3+ letters, no stopwords).
fn content_words(s: &str) -> Vec<String> {
    const STOP: [&str; 24] = [
        "the", "and", "you", "that", "this", "was", "for", "are", "with", "but", "not", "have",
        "has", "had", "its", "it's", "our", "your", "they", "them", "then", "there", "what", "yes",
    ];
    s.split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .map(|w| w.to_lowercase())
        .filter(|w| w.chars().count() >= 3 && !STOP.contains(&w.as_str()))
        .collect()
}

/// Share of `you`'s content words that repeat `them`'s, in order (longest
/// common subsequence); 0 for fewer than 4 words (too little to judge).
fn echoed_share(you: &str, them: &str) -> f32 {
    let (a, b) = (content_words(you), content_words(them));
    if a.len() < 4 || b.is_empty() {
        return 0.0;
    }
    let mut prev = vec![0usize; b.len() + 1];
    for x in &a {
        let mut cur = vec![0usize; b.len() + 1];
        for (j, y) in b.iter().enumerate() {
            cur[j + 1] = if x == y {
                prev[j] + 1
            } else {
                prev[j + 1].max(cur[j])
            };
        }
        prev = cur;
    }
    prev[b.len()] as f32 / a.len() as f32
}

/// The echo verdict for a "you" chunk (see above).
fn is_echo(correlation: f32, share: f32) -> bool {
    correlation >= ECHO_MIN_CORRELATION && share >= ECHO_MIN_SHARE
}

/// The active session — the capture thread and drain worker hold the buffers
/// and streams; this only carries what stop/state need.
struct Session {
    note_id: u64,
    started_ms: u64,
    stop: Arc<AtomicBool>,
}

static SESSION: Mutex<Option<Session>> = Mutex::new(None);
/// Note id of the active session (0 = none).
static ACTIVE_NOTE: AtomicU64 = AtomicU64::new(0);

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Whether a meeting is being recorded (the updater won't restart mid-meeting).
pub fn is_recording() -> bool {
    SESSION.lock().unwrap_or_else(|p| p.into_inner()).is_some()
}

/// The note a meeting is being recorded into, if any.
pub fn recording_note() -> Option<u64> {
    match ACTIVE_NOTE.load(Ordering::SeqCst) {
        0 => None,
        id => Some(id),
    }
}

/// Session state for the UI: `{ recording, noteId, elapsedSecs }`.
pub fn state() -> serde_json::Value {
    let guard = SESSION.lock().unwrap_or_else(|p| p.into_inner());
    match guard.as_ref() {
        Some(s) => serde_json::json!({
            "recording": true,
            "noteId": s.note_id,
            "elapsedSecs": (now_ms().saturating_sub(s.started_ms)) / 1000,
        }),
        None => serde_json::json!({ "recording": false }),
    }
}

/// Append captured audio, unless the buffer is full (then flag `overflow`).
/// The one push path: the device callbacks and the e2e player both come
/// here. For the mic, it reads the pipeline's dictation signal (one atomic
/// load) and blanks dictations (see "Dictation" above).
pub(crate) fn push_audio(buf: &AudioBuf, samples: &[f32], overflow: &AtomicBool) {
    if let Ok(mut b) = buf.lock() {
        let dictation = b.gate.is_some().then(crate::pipeline::dictation_state);
        if !b.push(samples, dictation) {
            overflow.store(true, Ordering::Relaxed);
        }
    }
}

/// Keep both sides on one clock: a side whose device stalled (nothing
/// playing, a device switch) is filled with silence up to the other side
/// (see "Keeping both sides on one clock" above). Called every 200 ms on the
/// capture thread, and every tick by the e2e player. Locks one side at a
/// time.
pub(crate) fn keep_in_step(a: &AudioBuf, b: &AudioBuf) {
    let end = |buf: &AudioBuf| buf.lock().map(|c| c.end()).ok();
    if let (Some(a_end), Some(b_end)) = (end(a), end(b)) {
        if let Ok(mut c) = a.lock() {
            c.fill_if_stalled(b_end);
        }
        if let Ok(mut c) = b.lock() {
            c.fill_if_stalled(a_end);
        }
    }
}

// ---- Following the output device ----
//
// "Them" is a loopback stream on Windows' default output device. If the
// person switches output mid-meeting (plugs in a headset, picks another
// device in Windows), the call moves to the new default and the old stream
// hears nothing. The capture thread checks the default output's endpoint id
// every 2 s and, when it changed, reopens the loopback stream on the new one
// (the old one stops first, so the two never interleave); the gap in between
// is filled with silence. A call app set to play on a device that isn't the
// default is never heard this way: the quiet-side watch says so.

/// How often the capture thread checks Windows' default output.
const OUTPUT_POLL: Duration = Duration::from_secs(2);

/// Which output device "Them" listens to, and when to move: plain
/// bookkeeping, unit-tested without devices (the e2e player's simulated
/// switch drives it too).
#[derive(Debug, Default)]
pub(crate) struct OutputFollower {
    /// The default output (its endpoint id) the loopback stream was opened
    /// on; `None` if Yap couldn't tell.
    current: Option<String>,
    /// A device whose stream wouldn't open (logged once, tried every poll).
    failed: Option<String>,
}

#[derive(Debug, PartialEq)]
pub(crate) enum Follow {
    Stay,
    /// Reopen the loopback stream on this device (endpoint id).
    Switch(String),
}

impl OutputFollower {
    /// The stream was opened on device `id` (`None`: couldn't tell which).
    pub(crate) fn listening_to(id: Option<String>) -> Self {
        OutputFollower {
            current: id,
            failed: None,
        }
    }

    /// Windows' default output now (`None`: there's none, or it couldn't be
    /// read: keep what there is).
    pub(crate) fn poll(&self, default: Option<String>) -> Follow {
        match default {
            Some(id) if self.current.as_ref() != Some(&id) => Follow::Switch(id),
            _ => Follow::Stay,
        }
    }

    /// The stream now listens to `id`.
    pub(crate) fn opened(&mut self, id: String) {
        self.current = Some(id);
        self.failed = None;
    }

    /// `id`'s stream wouldn't open: the old one carries on and the next poll
    /// tries again. Whether that's news (log it once).
    pub(crate) fn failed(&mut self, id: &str) -> bool {
        let news = self.failed.as_deref() != Some(id);
        self.failed = Some(id.to_string());
        news
    }
}

/// Log that "Them" moved to the new default output (`device`: its name).
pub(crate) fn followed_output(device: &str) {
    tracing::info!(
        device,
        "Meeting: Windows' default output changed; \"Them\" now follows it"
    );
}

/// Reads Windows' default output device: its endpoint id, which tells two
/// devices with the same name apart. Lives on the capture thread.
#[cfg(windows)]
mod default_output {
    use windows::Win32::Media::Audio::{eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
        COINIT_MULTITHREADED,
    };

    pub struct DefaultOutput {
        enumerator: Option<IMMDeviceEnumerator>,
        /// COM was initialized here (and is undone on drop). Not when cpal
        /// already set the thread up its own way (RPC_E_CHANGED_MODE): COM
        /// works either way.
        com: bool,
    }

    impl DefaultOutput {
        pub fn new() -> Self {
            // SAFETY: plain COM calls on this thread; every pointer comes
            // from COM and is released by its wrapper.
            unsafe {
                let com = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
                let enumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok();
                DefaultOutput { enumerator, com }
            }
        }

        /// The default output's endpoint id (the "console" role, as cpal's
        /// `default_output_device`), or `None` without one.
        pub fn id(&self) -> Option<String> {
            let enumerator = self.enumerator.as_ref()?;
            // SAFETY: COM calls on the thread that made the enumerator; the
            // id string is COM-allocated and freed once, after copying it.
            unsafe {
                let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
                let id = device.GetId().ok()?;
                let text = id.to_string().ok();
                CoTaskMemFree(Some(id.0 as *const _));
                text
            }
        }
    }

    impl Drop for DefaultOutput {
        fn drop(&mut self) {
            self.enumerator = None;
            if self.com {
                // SAFETY: balances this thread's successful CoInitializeEx.
                unsafe { CoUninitialize() };
            }
        }
    }
}

/// Elsewhere: the default output's name, from cpal.
#[cfg(not(windows))]
mod default_output {
    use cpal::traits::{DeviceTrait, HostTrait};

    pub struct DefaultOutput;

    impl DefaultOutput {
        pub fn new() -> Self {
            DefaultOutput
        }

        pub fn id(&self) -> Option<String> {
            cpal::default_host().default_output_device()?.name().ok()
        }
    }
}

/// Build a capture stream into `buf`. `loopback` selects the default OUTPUT
/// device (WASAPI loopback) instead of an input device.
fn build_capture_stream(
    buf: AudioBuf,
    overflow: Arc<AtomicBool>,
    input_device_name: Option<String>,
    loopback: bool,
) -> Result<cpal::Stream, String> {
    let host = cpal::default_host();
    let device = if loopback {
        host.default_output_device()
            .ok_or("No default output device for loopback capture")?
    } else {
        match input_device_name.as_deref() {
            Some(name) => host
                .input_devices()
                .map_err(|e| format!("Failed to enumerate input devices: {e}"))?
                .find(|d| d.name().map(|n| n == name).unwrap_or(false))
                .ok_or_else(|| format!("Input device not found: {name}"))?,
            None => host
                .default_input_device()
                .ok_or("No default input device available")?,
        }
    };

    // Loopback streams are configured from the OUTPUT device's default config.
    let default_config = if loopback {
        device
            .default_output_config()
            .map_err(|e| format!("Failed to get output config for loopback: {e}"))?
    } else {
        device
            .default_input_config()
            .map_err(|e| format!("Failed to get input config: {e}"))?
    };
    let native_rate = default_config.sample_rate().0;
    let channels = default_config.channels();
    tracing::info!(
        device = %device.name().unwrap_or_else(|_| "unknown".into()),
        native_rate,
        channels,
        loopback,
        "Meeting capture stream"
    );

    let stream_config = cpal::StreamConfig {
        channels,
        sample_rate: cpal::SampleRate(native_rate),
        buffer_size: cpal::BufferSize::Default,
    };
    let needs_resample = native_rate != TARGET_SAMPLE_RATE;
    let needs_downmix = channels > 1;

    device
        .build_input_stream(
            &stream_config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                let mono = if needs_downmix {
                    let ch = channels as usize;
                    data.chunks_exact(ch)
                        .map(|frame| frame.iter().sum::<f32>() / ch as f32)
                        .collect::<Vec<f32>>()
                } else {
                    data.to_vec()
                };
                let resampled = if needs_resample {
                    resample_linear(&mono, native_rate, TARGET_SAMPLE_RATE)
                } else {
                    mono
                };
                push_audio(&buf, &resampled, &overflow);
            },
            |err| tracing::warn!("Meeting capture stream error: {}", err),
            None,
        )
        .map_err(|e| format!("Failed to build capture stream: {e}"))
}

/// Open the loopback stream ("Them") on Windows' current default output and
/// start it.
fn open_loopback(buf: &AudioBuf, overflow: &Arc<AtomicBool>) -> Result<cpal::Stream, String> {
    let stream = build_capture_stream(Arc::clone(buf), Arc::clone(overflow), None, true)?;
    stream
        .play()
        .map_err(|e| format!("Failed to start capture: {e}"))?;
    Ok(stream)
}

/// The default output's name, for the log.
fn default_output_name() -> String {
    cpal::default_host()
        .default_output_device()
        .and_then(|d| d.name().ok())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Open the mic + loopback streams on their own thread (cpal streams are
/// !Send: the thread builds them and reports the result). Until stop it then
/// keeps both sides on one clock (every 200 ms) and follows Windows' default
/// output (every 2 s; see "Following the output device" above).
fn spawn_device_capture(
    mic_buf: AudioBuf,
    sys_buf: AudioBuf,
    stop: Arc<AtomicBool>,
    overflow: Arc<AtomicBool>,
) -> Result<(), String> {
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), String>>();
    let input_device = crate::config::load().input_device.clone();
    std::thread::Builder::new()
        .name("meeting-capture".into())
        .spawn(move || {
            let mic = build_capture_stream(Arc::clone(&mic_buf), Arc::clone(&overflow), input_device, false);
            let mic = match mic.and_then(|m| {
                m.play()
                    .map(|_| m)
                    .map_err(|e| format!("Failed to start capture: {e}"))
            }) {
                Ok(m) => m,
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
            let devices = default_output::DefaultOutput::new();
            let mut output = OutputFollower::listening_to(devices.id());
            let mut sys = match open_loopback(&sys_buf, &overflow) {
                Ok(s) => Some(s),
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
            let _ = ready_tx.send(Ok(()));
            let mut polled = Instant::now();
            while !stop.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(200));
                keep_in_step(&mic_buf, &sys_buf);
                if polled.elapsed() < OUTPUT_POLL {
                    continue;
                }
                polled = Instant::now();
                if let Follow::Switch(id) = output.poll(devices.id()) {
                    // The old stream stops first, so the two never interleave.
                    drop(sys.take());
                    match open_loopback(&sys_buf, &overflow) {
                        Ok(s) => {
                            sys = Some(s);
                            output.opened(id);
                            followed_output(&default_output_name());
                        }
                        Err(e) => {
                            if output.failed(&id) {
                                tracing::warn!("Meeting: couldn't follow the new output device: {}", e);
                            }
                        }
                    }
                    // The gap is silence.
                    keep_in_step(&mic_buf, &sys_buf);
                }
            }
            drop(mic);
            drop(sys);
            tracing::info!("Meeting capture streams stopped");
        })
        .map_err(|e| format!("Failed to spawn capture thread: {e}"))?;
    // Wait for the streams to come up (or fail) before claiming success.
    ready_rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| "Capture thread didn't start in time".to_string())?
}

/// Where to cut the next chunk off a source's buffered `samples`, if one is
/// ready: at the quietest moment of the last `CUT_SEARCH_SECS` before the
/// newest audio (in a backlog, before `CHUNK_MAX_SECS`), always leaving the
/// newest `CUT_MARGIN_MS` buffered. The final drain takes whatever is left,
/// in pieces of at most `CHUNK_MAX_SECS`.
fn next_cut(samples: &[f32], final_drain: bool) -> Option<usize> {
    let len = samples.len();
    let max = CHUNK_MAX_SECS * RATE;
    if len == 0 || (!final_drain && len < CHUNK_SECS * RATE) {
        return None;
    }
    if final_drain && len <= max {
        return Some(len);
    }
    let hi = if final_drain {
        max
    } else {
        max.min(len - CUT_MARGIN_MS * RATE / 1000)
    };
    let lo = hi
        .saturating_sub(CUT_SEARCH_SECS * RATE)
        .max(MIN_CHUNK_SECS * RATE);
    if lo >= hi {
        return Some(hi);
    }
    Some(crate::media::quietest_frame(samples, lo, hi, RATE).0)
}

fn buffered(buf: &AudioBuf) -> usize {
    buf.lock().map(|b| b.samples.len()).unwrap_or(0)
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

/// The meeting's attendees (from its calendar invite, or typed in) ahead of
/// the dictionary's spellings, for the Whisper prompt: names come out spelled
/// as the invite has them.
fn with_attendees(dictionary: &[crate::config::DictionaryEntry]) -> Vec<crate::config::DictionaryEntry> {
    let names = recording_note()
        .and_then(crate::notes::get)
        .map(|n| n.participants)
        .unwrap_or_default();
    names
        .into_iter()
        .map(|to| crate::config::DictionaryEntry { from: String::new(), to, fuzzy: false })
        .chain(dictionary.iter().cloned())
        .collect()
}

/// Transcribe one chunk on the shared warm engine (take → transcribe →
/// put back; lazily reloads the model if the idle watcher dropped it), then
/// run the correction dictionary over it as a dictation's text gets it
/// (`pipeline::apply_corrections`: exact, then fuzzy for ONNX models). Both
/// sides go through here, so the echo check compares corrected text with
/// corrected text.
async fn transcribe_chunk(engine_slot: &EngineSlot, samples: Vec<f32>) -> Option<String> {
    if samples.len() < MIN_CHUNK_SECS * RATE || peak(&samples) < SILENCE_PEAK {
        return None;
    }
    let engine = engine_slot.lock().ok().and_then(|mut g| g.take());
    let engine = match engine {
        Some(e) => e,
        None => {
            let cfg = crate::config::load();
            match crate::stt::create_stt_engine(
                &crate::config::data_dir(),
                &cfg.model_size,
                cfg.use_gpu,
            ) {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!("Meeting: no STT engine available: {}", e);
                    return None;
                }
            }
        }
    };

    let cfg = crate::config::load();
    let language = if cfg.selected_language == "auto" {
        None
    } else {
        Some(cfg.selected_language.clone())
    };
    let translate = cfg.translate_to_english;
    let dict_prompt = crate::config::dictionary_prompt(&with_attendees(&cfg.dictionary));

    let slot = Arc::clone(engine_slot);
    let outcome = tokio::task::spawn_blocking(move || {
        let result = engine.transcribe(
            &samples,
            language.as_deref(),
            translate,
            dict_prompt.as_deref(),
        );
        (engine, result)
    })
    .await;

    match outcome {
        Ok((engine, result)) => {
            if let Ok(mut g) = slot.lock() {
                if g.is_none() {
                    *g = Some(engine);
                }
            }
            match result {
                Ok(text) => {
                    let t = crate::pipeline::apply_corrections(text.trim(), &cfg);
                    let t = t.trim().to_string();
                    if t.is_empty() {
                        None
                    } else {
                        Some(t)
                    }
                }
                Err(e) => {
                    tracing::warn!("Meeting chunk transcription failed: {}", e);
                    None
                }
            }
        }
        Err(e) => {
            tracing::warn!("Meeting chunk task failed: {}", e);
            None
        }
    }
}

/// One captured source in the worker.
struct Source {
    buf: AudioBuf,
    loudness: Envelope,
}

impl Source {
    fn new(buf: AudioBuf) -> Self {
        Source {
            buf,
            loudness: Envelope::default(),
        }
    }

    /// The next chunk, transcribed, or `None` when none is ready.
    async fn next(&mut self, engine_slot: &EngineSlot, final_drain: bool) -> Option<Heard> {
        // (The lock is let go before the transcription is awaited.)
        let chunk = self.buf.lock().ok()?.take_chunk(final_drain)?;
        let from = chunk.from;
        let to = from + chunk.samples.len() as u64;
        self.loudness.push(&chunk.samples);
        let onset = from + speech_onset(&chunk.samples) as u64;
        let text = transcribe_chunk(engine_slot, chunk.samples).await;
        Some(Heard {
            from,
            to,
            onset,
            text,
            dictated: chunk.dictated,
        })
    }
}

/// A transcribed chunk; positions in samples on the session clock.
struct Heard {
    from: u64,
    to: u64,
    /// Where the speech in it starts: dates the segment, and orders "you"
    /// and "them" chunks that were cut at the same moment.
    onset: u64,
    /// `None`: silence (a dictation blanked, say), or nothing came of it.
    text: Option<String>,
    /// Where dictations began that are due a "you dictated here" marker.
    dictated: Vec<u64>,
}

/// Offset of the first 20 ms frame of speech in `samples`: the first at a
/// fifth of the loudest frame's level (and above room noise), else 0.
fn speech_onset(samples: &[f32]) -> usize {
    let rms = |f: &[f32]| (f.iter().map(|s| s * s).sum::<f32>() / f.len() as f32).sqrt();
    let (frames, _) = samples.as_chunks::<ENV_FRAME>();
    let loudest = frames.iter().map(|f| rms(f)).fold(0.0f32, f32::max);
    let threshold = (loudest * 0.2).max(0.004);
    frames
        .iter()
        .position(|f| rms(f) >= threshold)
        .map(|i| i * ENV_FRAME)
        .unwrap_or(0)
}

/// Was this "you" chunk (`from..to`, saying `text`) the call coming through
/// the speakers? See the echo notes at the top.
fn sounds_like_echo(
    mic: &Envelope,
    call: &Envelope,
    from: u64,
    to: u64,
    text: &str,
    theirs: &std::collections::VecDeque<(u64, u64, String)>,
) -> bool {
    let frame = ENV_FRAME as u64;
    let (a, b) = (from / frame, to / frame);
    let (mic_from, mic_frames) = mic.range(a, b);
    let (call_from, call_frames) = call.range(a.saturating_sub(ECHO_MAX_LAG), b);
    let correlation = echo_correlation(&mic_frames, mic_from, &call_frames, call_from);
    let slack = RATE as u64;
    let said: Vec<&str> = theirs
        .iter()
        .filter(|(f, t, _)| *t + slack >= from && *f <= to + slack)
        .map(|(_, _, text)| text.as_str())
        .collect();
    let share = echoed_share(text, &said.join(" "));
    let echo = is_echo(correlation, share);
    tracing::debug!(
        correlation,
        share,
        echo,
        "Meeting: echo check on a mic chunk"
    );
    if echo {
        tracing::info!(
            correlation,
            share,
            "Meeting: a mic chunk was the call coming through the speakers; hidden"
        );
    }
    echo
}

// ---- Quiet sides ----
//
// Teams and Zoom can play a call on a device of their own choosing; when
// that isn't Windows' default output, loopback never hears it, "Them" stays
// silent for the whole meeting and the action plan misses everyone else.
// Likewise a muted mic, or the wrong one, leaves "You" silent. The worker
// watches both sides (every 200 ms): one that has heard nothing above the
// silence gate for 3 minutes of the meeting while the other talked at least
// 10 s of it gets one warning, until it's heard again. Both quiet is a
// break, not a fault. A dictation counts as the mic being heard (it works),
// not as talk.

/// A side quiet this long while the other talks gets a warning…
const QUIET_SECS: u64 = 3 * 60;
/// …if the other side talked at least this much meanwhile.
const QUIET_TALK_SECS: u64 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    You,
    Them,
}

/// A source as the quiet-side watch sees it (samples, session clock).
#[derive(Clone, Copy, Debug, Default)]
struct Levels {
    /// Its newest audio.
    end: u64,
    /// Just past the last audio above the silence gate.
    heard: u64,
    /// Audio above the silence gate that went into the meeting.
    talk: u64,
}

/// The quiet-side watch's timings, in samples.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct QuietTimings {
    quiet: u64,
    talk: u64,
}

impl QuietTimings {
    const REAL: QuietTimings = QuietTimings {
        quiet: QUIET_SECS * RATE as u64,
        talk: QUIET_TALK_SECS * RATE as u64,
    };
}

/// Short timings for the e2e suite (`e2e_meeting_quiet`, debug builds).
static TEST_QUIET: Mutex<Option<QuietTimings>> = Mutex::new(None);

fn quiet_timings() -> QuietTimings {
    TEST_QUIET
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .unwrap_or(QuietTimings::REAL)
}

/// Test mode only (`e2e::e2e_meeting_quiet`): the quiet-side watch's
/// timings in seconds of meeting audio, (quiet, talk); `None`: the real ones.
#[cfg(debug_assertions)]
pub(crate) fn set_test_quiet_timings(secs: Option<(f32, f32)>) {
    let samples = |s: f32| (s.max(0.0) * RATE as f32) as u64;
    *TEST_QUIET.lock().unwrap_or_else(|p| p.into_inner()) = secs.map(|(quiet, talk)| QuietTimings {
        quiet: samples(quiet),
        talk: samples(talk),
    });
}

/// One side's quiet stretch.
#[derive(Debug, Default)]
struct Episode {
    /// The side's `heard` when the stretch began.
    heard: u64,
    /// The other side's `talk` then.
    other_talk: u64,
    warned: bool,
}

/// Watches for one side going quiet while the other talks.
#[derive(Debug, Default)]
struct QuietWatch {
    you: Episode,
    them: Episode,
}

impl QuietWatch {
    /// The side to warn about now, if any: once per quiet stretch.
    fn check(&mut self, you: Levels, them: Levels, t: QuietTimings) -> Option<Side> {
        let now = you.end.max(them.end);
        let sides = [
            (Side::Them, &mut self.them, them, you),
            (Side::You, &mut self.you, you, them),
        ];
        for (side, stretch, mine, other) in sides {
            if mine.heard != stretch.heard {
                // Heard again: a new stretch starts here.
                *stretch = Episode {
                    heard: mine.heard,
                    other_talk: other.talk,
                    warned: false,
                };
            }
            let quiet_for = now.saturating_sub(stretch.heard);
            let talked = other.talk.saturating_sub(stretch.other_talk);
            if !stretch.warned && quiet_for >= t.quiet && talked >= t.talk {
                stretch.warned = true;
                return Some(side);
            }
        }
        None
    }
}

/// "3 minutes", "1 minute", "6 seconds" (`samples` of meeting audio).
fn span_words(samples: u64) -> String {
    let rate = RATE as u64;
    let secs = (samples + rate / 2) / rate;
    let plural = |n: u64, unit: &str| format!("{n} {unit}{}", if n == 1 { "" } else { "s" });
    if secs >= 60 {
        plural((secs + 30) / 60, "minute")
    } else {
        plural(secs.max(1), "second")
    }
}

/// What the person is told about a side quiet for `quiet` samples (`call`:
/// the call Yap detected, "Teams call", if any).
fn quiet_message(side: Side, quiet: u64, call: Option<&str>) -> String {
    let span = span_words(quiet);
    match (side, call) {
        (Side::Them, Some(call)) => format!(
            "Yap hasn't heard your {call} for {span}. If it plays through a headset, make that Windows' default output device."
        ),
        (Side::Them, None) => format!(
            "Yap hasn't heard the call for {span}. If your call plays through a headset, make it Windows' default output device."
        ),
        (Side::You, _) => format!(
            "Yap hasn't heard your mic for {span}. If you're talking, check it isn't muted, and pick the right mic in Settings \u{2192} General."
        ),
    }
}

/// Watch the two sides (see "Quiet sides" above); tell the person about one
/// that went quiet: `yap-meeting-warning`, as the main window and the
/// notepad show it, plus a Windows notification when neither is in view.
fn watch_quiet(app: &AppHandle, note_id: u64, watch: &mut QuietWatch, you: &AudioBuf, them: &AudioBuf) {
    let levels = |buf: &AudioBuf| buf.lock().map(|c| c.levels()).unwrap_or_default();
    let t = quiet_timings();
    let Some(side) = watch.check(levels(you), levels(them), t) else {
        return;
    };
    let call = crate::meeting_detect::latest_call().map(|(_, name)| name);
    let message = quiet_message(side, t.quiet, call.as_deref());
    tracing::info!(note_id, ?side, "Meeting: one side was quiet while the other talked; telling the person");
    let _ = app.emit("yap-meeting-warning", &message);
    notify_quiet(app, side, message);
}

/// The quiet-side warning as a Windows notification as well, when neither
/// the meeting notepad (docked beside the call) is on screen nor the main
/// window focused: the person is in their call app, where an in-app toast
/// would go unseen until the meeting is over. (None in test runs or portable
/// mode.)
fn notify_quiet(app: &AppHandle, side: Side, message: String) {
    let app = app.clone();
    // Window getters wait on the main thread: not from the worker's task.
    std::thread::spawn(move || {
        let notepad_up = app.get_webview_window(crate::notepad::LABEL).is_some_and(|w| {
            w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false)
        });
        let main_focused = app
            .get_webview_window("settings")
            .is_some_and(|w| w.is_focused().unwrap_or(false));
        if notepad_up || main_focused {
            return;
        }
        let (title, settings) = match side {
            Side::Them => ("Yap can't hear the call", None),
            Side::You => ("Yap can't hear your mic", Some("general")),
        };
        crate::meeting_guard::notice_native(
            &app,
            &crate::meeting_guard::Notice {
                kind: "quiet",
                title: title.to_string(),
                body: message,
                icon: "call",
                variant: "default",
                settings,
                note_id: None,
            },
        );
    });
}

/// New transcript segments for the meeting in `note_id`: persist them, show
/// them, and let the rolling digests know. The recorder's worker and the
/// e2e test feed (`e2e::e2e_meeting_feed`) both come through here.
pub(crate) fn ingest(
    app: &AppHandle,
    note_id: u64,
    segments: Vec<TranscriptSegment>,
) -> Result<(), String> {
    if segments.is_empty() {
        return Ok(());
    }
    crate::notes::append_transcript(note_id, &segments)?;
    for seg in &segments {
        let _ = app.emit("yap-meeting-segment", serde_json::json!(seg));
    }
    crate::meeting_summary::kick(app, note_id);
    Ok(())
}

/// The drain cadence (an e2e run can play its test audio faster).
fn drain_interval() -> Duration {
    #[cfg(debug_assertions)]
    if crate::e2e::active() {
        return Duration::from_secs_f32(DRAIN_INTERVAL_SECS as f32 / crate::e2e::meeting_speed());
    }
    Duration::from_secs(DRAIN_INTERVAL_SECS)
}

/// Start recording into `note_id`. Fails if a session is already running or
/// the streams can't be built. Emits `yap-meeting-state`.
pub fn start(app: AppHandle, engine_slot: EngineSlot, note_id: u64) -> Result<(), String> {
    {
        let guard = SESSION.lock().unwrap_or_else(|p| p.into_inner());
        if guard.is_some() {
            return Err("A meeting is already being recorded".to_string());
        }
    }

    let mic_buf = Captured::mic();
    let sys_buf = Captured::call();
    let stop = Arc::new(AtomicBool::new(false));
    let overflow = Arc::new(AtomicBool::new(false));

    let bufs = (Arc::clone(&mic_buf), Arc::clone(&sys_buf));
    let flags = (Arc::clone(&stop), Arc::clone(&overflow));
    #[cfg(debug_assertions)]
    let capture = if crate::e2e::active() {
        crate::e2e::spawn_meeting_audio(bufs.0, bufs.1, flags.0, flags.1)
    } else {
        spawn_device_capture(bufs.0, bufs.1, flags.0, flags.1)
    };
    #[cfg(not(debug_assertions))]
    let capture = spawn_device_capture(bufs.0, bufs.1, flags.0, flags.1);
    capture?;

    let started_ms = now_ms();
    let session = Session {
        note_id,
        started_ms,
        stop: Arc::clone(&stop),
    };
    {
        let mut guard = SESSION.lock().unwrap_or_else(|p| p.into_inner());
        *guard = Some(session);
    }
    ACTIVE_NOTE.store(note_id, Ordering::SeqCst);
    let _ = app.emit("yap-meeting-state", state());
    tracing::info!(note_id, "Meeting recording started");

    // The chunk/transcribe worker.
    tauri::async_runtime::spawn(async move {
        let mut you = Source::new(mic_buf);
        let mut them = Source::new(sys_buf);
        // What "them" said lately, for the echo check: (from, to, text) in
        // samples on the session clock.
        let mut theirs: VecDeque<(u64, u64, String)> = Default::default();
        let mut quiet = QuietWatch::default();
        let tick = drain_interval();
        let mut backlog = false;
        let mut warned = false;
        // A session position as unix seconds.
        let at = move |pos: u64| (started_ms + pos * 1000 / RATE as u64) / 1000;
        loop {
            // Wait for the next tick (in small steps, so stop is picked up
            // quickly) unless there's a backlog to work off. Each step also
            // looks for a side gone quiet.
            if !backlog {
                let steps = (tick.as_millis() / 200).max(1);
                for _ in 0..steps {
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    watch_quiet(&app, note_id, &mut quiet, &you.buf, &them.buf);
                }
            } else {
                watch_quiet(&app, note_id, &mut quiet, &you.buf, &them.buf);
            }
            let stopping = stop.load(Ordering::SeqCst);

            // "Them" first, so the echo check can hold "you" up against it.
            // Segments are dated (and ordered) by where their speech starts;
            // a dictation marker by where the dictation began.
            let mut batch: Vec<(u64, TranscriptSegment)> = Vec::new();
            if let Some(h) = them.next(&engine_slot, stopping).await {
                if let Some(text) = h.text {
                    let seg = TranscriptSegment {
                        source: "them".to_string(),
                        text: text.clone(),
                        ts: at(h.onset),
                        echo: false,
                        dictated: false,
                    };
                    batch.push((h.onset, seg));
                    theirs.push_back((h.from, h.to, text));
                    while theirs.len() > 4 {
                        theirs.pop_front();
                    }
                }
            }
            if let Some(h) = you.next(&engine_slot, stopping).await {
                for from in h.dictated {
                    tracing::info!(note_id, "Meeting: a dictation was left out of \"You\"");
                    batch.push((from, TranscriptSegment::dictation_marker(at(from))));
                }
                if let Some(text) = h.text {
                    let (mic, call) = (&you.loudness, &them.loudness);
                    let echo = sounds_like_echo(mic, call, h.from, h.to, &text, &theirs);
                    let seg = TranscriptSegment {
                        source: "you".to_string(),
                        text,
                        ts: at(h.onset),
                        echo,
                        dictated: false,
                    };
                    batch.push((h.onset, seg));
                }
            }
            backlog = [&you, &them].iter().any(|s| {
                let left = buffered(&s.buf);
                left >= CHUNK_SECS * RATE || (stopping && left > 0)
            });
            batch.sort_by_key(|(onset, _)| *onset);
            let batch = batch.into_iter().map(|(_, seg)| seg).collect();
            if let Err(e) = ingest(&app, note_id, batch) {
                tracing::warn!("Meeting segments not saved: {}", e);
            }
            if !warned && overflow.load(Ordering::Relaxed) {
                warned = true;
                tracing::warn!("Meeting transcription is falling behind; audio was dropped");
                let _ = app.emit(
                    "yap-meeting-warning",
                    "Transcription is falling behind, so some audio was skipped. A faster speech model (Settings → Speech-to-Text) keeps up better.",
                );
            }

            if stopping && !backlog {
                break;
            }
        }
        // A dictation no chunk carried a marker for (one began right at the
        // stop, with no audio after it).
        let left = you.buf.lock().map(|mut b| b.take_unmarked()).unwrap_or_default();
        if !left.is_empty() {
            let markers = left.into_iter().map(|from| TranscriptSegment::dictation_marker(at(from)));
            if let Err(e) = ingest(&app, note_id, markers.collect()) {
                tracing::warn!("Meeting segments not saved: {}", e);
            }
        }

        ACTIVE_NOTE.store(0, Ordering::SeqCst);
        {
            let mut guard = SESSION.lock().unwrap_or_else(|p| p.into_inner());
            *guard = None;
        }
        let _ = app.emit("yap-meeting-state", state());
        tracing::info!(note_id, "Meeting recording finished");
    });

    Ok(())
}

/// Signal the active session to stop. The worker does a final drain, persists,
/// clears the session, and emits the final `yap-meeting-state` — the UI waits
/// for that event before running the meeting enhancement.
pub fn stop() -> Result<(), String> {
    let guard = SESSION.lock().unwrap_or_else(|p| p.into_inner());
    match guard.as_ref() {
        Some(s) => {
            s.stop.store(true, Ordering::SeqCst);
            Ok(())
        }
        None => Err("No meeting is being recorded".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(secs: f32) -> Vec<f32> {
        (0..(secs * RATE as f32) as usize)
            .map(|i| 0.3 * (i as f32 * 0.07).sin())
            .collect()
    }

    #[test]
    fn waits_for_a_full_chunk() {
        assert_eq!(next_cut(&tone(10.0), false), None);
        assert_eq!(next_cut(&[], true), None);
        // The final drain takes a short remainder whole.
        assert_eq!(next_cut(&tone(10.0), true), Some(10 * RATE));
    }

    #[test]
    fn cuts_in_the_pause_before_the_newest_audio() {
        // 16 s of "speech" with a pause at 13.0–13.1 s.
        let mut s = tone(16.0);
        for x in &mut s[13 * RATE..13 * RATE + RATE / 10] {
            *x = 0.0005;
        }
        let cut = next_cut(&s, false).unwrap();
        assert!(
            (13 * RATE..13 * RATE + RATE / 10).contains(&cut),
            "cut at {cut}"
        );
    }

    /// A source (no dictation gate) with `samples` already buffered.
    fn buffered_audio(samples: Vec<f32>) -> AudioBuf {
        let mut c = Captured::new(None);
        c.samples = samples;
        Arc::new(Mutex::new(c))
    }

    fn take(buf: &AudioBuf, final_drain: bool) -> Option<Chunk> {
        buf.lock().unwrap().take_chunk(final_drain)
    }

    #[test]
    fn a_backlog_is_cut_into_bounded_chunks() {
        let buf = buffered_audio(tone(95.0));
        let mut chunks = Vec::new();
        let mut at = 0;
        while let Some(c) = take(&buf, true) {
            assert!(c.samples.len() <= CHUNK_MAX_SECS * RATE);
            assert_eq!(c.from, at, "chunks follow each other on the session clock");
            at += c.samples.len() as u64;
            chunks.push(c.samples.len());
        }
        assert_eq!(chunks.iter().sum::<usize>(), 95 * RATE);
        assert!(chunks.len() >= 4);
        assert_eq!(buffered(&buf), 0);
        // Mid-meeting, the remainder after a cut stays queued.
        let buf = buffered_audio(tone(20.0));
        let first = take(&buf, false).unwrap().samples.len();
        assert!((15 * RATE..20 * RATE).contains(&first));
        assert_eq!(first + buffered(&buf), 20 * RATE);
    }

    // A dictation's pre-roll has to still be buffered when it starts.
    const _: () = assert!(CUT_MARGIN_MS * RATE / 1000 >= DICTATION_LOOKBACK);

    #[test]
    fn the_newest_audio_stays_buffered_after_any_cut() {
        // Mid-meeting cuts, a backlog's included, leave the newest 400 ms.
        for tenths in (150..=450).step_by(7) {
            let s = tone(tenths as f32 / 10.0);
            let cut = next_cut(&s, false).unwrap();
            assert!(s.len() - cut >= DICTATION_LOOKBACK, "{tenths}: cut at {cut}");
            assert!(cut <= CHUNK_MAX_SECS * RATE);
        }
    }

    // ---- dictation ----

    /// The mic, with the pipeline's dictation state when the meeting started.
    fn mic(state: (u64, bool)) -> Captured {
        Captured::new(Some(DictationGate::new(state)))
    }

    /// Push `audio` in 10 ms callbacks, each reading dictation state `state`.
    fn feed(c: &mut Captured, audio: &[f32], state: (u64, bool)) {
        for piece in audio.chunks(RATE / 100) {
            assert!(c.push(piece, Some(state)));
        }
    }

    fn silent(s: &[f32]) -> bool {
        !s.is_empty() && s.iter().all(|x| *x == 0.0)
    }

    fn loud(s: &[f32]) -> bool {
        peak(s) > 0.2
    }

    fn unmarked(c: &Captured) -> Vec<u64> {
        c.gate.as_ref().unwrap().unmarked.clone()
    }

    /// One 10 ms callback.
    const CALLBACK: usize = RATE / 100;

    #[test]
    fn a_dictation_goes_silent_with_its_pre_roll() {
        let mut c = mic((0, false));
        feed(&mut c, &tone(2.0), (0, false)); // the meeting
        feed(&mut c, &tone(1.0), (1, true)); // a dictation
        feed(&mut c, &tone(1.0), (1, false)); // the meeting again
        let from = 2 * RATE - DICTATION_LOOKBACK;
        // …to where a callback saw it end (it may hold its last moment).
        let to = 3 * RATE + CALLBACK;
        assert!(loud(&c.samples[from - RATE / 10..from]), "the meeting right up to the pre-roll");
        assert!(silent(&c.samples[from..to]));
        assert!(loud(&c.samples[to..to + RATE / 10]), "the meeting right after");
        // Silence, not a gap: the timeline keeps its length.
        assert_eq!(c.samples.len(), 4 * RATE);
        assert_eq!(unmarked(&c), vec![from as u64]);
    }

    #[test]
    fn a_dictation_across_a_chunk_cut_is_marked_once() {
        let mut c = mic((0, false));
        feed(&mut c, &tone(14.0), (0, false));
        feed(&mut c, &tone(4.0), (1, true));
        feed(&mut c, &tone(2.0), (1, false));
        let from = (14 * RATE - DICTATION_LOOKBACK) as u64;
        // The cut lands in the (silent) dictation: it crosses into the next chunk.
        let first = c.take_chunk(false).unwrap();
        let cut = first.samples.len();
        assert!((from as usize) < cut && cut < 18 * RATE, "cut at {cut}");
        assert_eq!(first.dictated, vec![from], "marked with the chunk it starts in");
        assert!(silent(&first.samples[from as usize..]));
        let rest = c.take_chunk(true).unwrap();
        assert_eq!(rest.from, cut as u64);
        assert!(rest.dictated.is_empty(), "…and only once");
        let blank = 18 * RATE + CALLBACK - cut;
        assert!(silent(&rest.samples[..blank]));
        assert!(loud(&rest.samples[blank..]));
        assert!(c.take_unmarked().is_empty());
    }

    #[test]
    fn a_marker_waits_for_the_chunk_its_dictation_starts_in() {
        let mut c = mic((0, false));
        feed(&mut c, &tone(16.0), (0, false));
        let first = c.take_chunk(false).unwrap();
        assert!(first.dictated.is_empty());
        let kept_from = first.samples.len() as u64;
        // Two dictations, the second cancelled at once (a callback later).
        feed(&mut c, &tone(1.0), (1, true));
        feed(&mut c, &tone(2.0), (1, false));
        feed(&mut c, &tone(0.01), (2, true));
        feed(&mut c, &tone(2.0), (2, false));
        let one = (16 * RATE - DICTATION_LOOKBACK) as u64;
        let two = (19 * RATE - DICTATION_LOOKBACK) as u64;
        assert!(one >= kept_from, "the first one's pre-roll was still buffered");
        let rest = c.take_chunk(true).unwrap();
        assert_eq!(rest.dictated, vec![one, two]);
        let at = |pos: u64| (pos - kept_from) as usize;
        // The cancelled one: its pre-roll, its callback and the one that saw it end.
        assert!(silent(&rest.samples[at(two)..at(two) + DICTATION_LOOKBACK + 2 * CALLBACK]));
        assert!(loud(&rest.samples[at(two) + DICTATION_LOOKBACK + 2 * CALLBACK..]));
    }

    #[test]
    fn a_dictation_between_two_callbacks_is_still_caught() {
        // It began and ended before the mic's next callback (the count moved
        // on, nothing records): its pre-roll and that callback go silent.
        let mut c = mic((0, false));
        feed(&mut c, &tone(1.0), (0, false));
        feed(&mut c, &tone(1.0), (1, false));
        let from = RATE - DICTATION_LOOKBACK;
        assert!(silent(&c.samples[from..RATE + CALLBACK]));
        assert!(loud(&c.samples[RATE + CALLBACK..]));
        assert_eq!(unmarked(&c), vec![from as u64]);
        // Two back to back between callbacks: one span, one marker.
        let mut c = mic((0, false));
        feed(&mut c, &tone(1.0), (0, false));
        feed(&mut c, &tone(1.0), (1, true));
        feed(&mut c, &tone(1.0), (3, true));
        feed(&mut c, &tone(1.0), (3, false));
        assert!(silent(&c.samples[RATE - DICTATION_LOOKBACK..3 * RATE + CALLBACK]));
        assert_eq!(unmarked(&c).len(), 1);
    }

    #[test]
    fn only_dictations_during_the_meeting_count() {
        // One still recording when the meeting started: silent from the start.
        let mut c = mic((5, true));
        feed(&mut c, &tone(1.0), (5, true));
        feed(&mut c, &tone(1.0), (5, false));
        assert!(silent(&c.samples[..RATE + CALLBACK]));
        assert!(loud(&c.samples[RATE + CALLBACK..]));
        assert_eq!(unmarked(&c), vec![0]);
        // One that ended before it started: none of its business.
        let mut c = mic((5, false));
        feed(&mut c, &tone(1.0), (5, false));
        assert!(loud(&c.samples[..RATE / 10]) && loud(&c.samples[RATE - RATE / 10..]));
        assert!(unmarked(&c).is_empty());
        // "Them" is never touched.
        let mut c = Captured::new(None);
        assert!(c.push(&tone(1.0), Some((9, true))));
        assert!(loud(&c.samples[..RATE / 10]));
    }

    #[test]
    fn levels_count_what_the_meeting_heard() {
        let mut c = mic((0, false));
        feed(&mut c, &tone(1.0), (0, false));
        feed(&mut c, &vec![0.0; RATE], (0, false));
        let l = c.levels();
        assert_eq!((l.end, l.heard, l.talk), (2 * RATE as u64, RATE as u64, RATE as u64));
        // A dictation: the mic works (heard), but that isn't meeting talk.
        feed(&mut c, &tone(1.0), (1, true));
        let l = c.levels();
        assert_eq!((l.heard, l.talk), (3 * RATE as u64, RATE as u64));
    }

    // ---- one clock ----

    #[test]
    fn a_stalled_side_is_filled_with_silence_up_to_the_other() {
        let (you, them) = (buffered_audio(Vec::new()), buffered_audio(Vec::new()));
        let overflow = AtomicBool::new(false);
        let end = |b: &AudioBuf| b.lock().unwrap().end();
        push_audio(&you, &tone(2.0), &overflow);
        push_audio(&them, &tone(2.0), &overflow);
        // A hiccup shorter than half a second isn't a stall.
        push_audio(&you, &tone(0.3), &overflow);
        keep_in_step(&you, &them);
        assert_eq!(end(&them), 2 * RATE as u64);
        // Longer (nothing playing, a device switch): silence up to "You".
        push_audio(&you, &tone(0.3), &overflow);
        keep_in_step(&you, &them);
        assert_eq!(end(&them), end(&you));
        assert!(silent(&them.lock().unwrap().samples[2 * RATE..]));
        // Its device delivers again: in step from there.
        push_audio(&them, &tone(1.0), &overflow);
        push_audio(&you, &tone(1.0), &overflow);
        assert_eq!(end(&them), end(&you));
        // The mic gets the same when it's the one that stalls.
        push_audio(&them, &tone(1.0), &overflow);
        keep_in_step(&you, &them);
        assert_eq!(end(&you), end(&them));
        // Filled silence isn't hearing anything.
        assert_eq!(you.lock().unwrap().levels().heard, (2 * RATE + 2 * 4_800 + RATE) as u64);
        assert!(!overflow.load(Ordering::Relaxed));
    }

    // ---- following the output device ----

    #[test]
    fn following_the_default_output() {
        let mut f = OutputFollower::listening_to(Some("speakers".into()));
        assert_eq!(f.poll(Some("speakers".into())), Follow::Stay);
        // No output at all, or it couldn't be read: keep what there is.
        assert_eq!(f.poll(None), Follow::Stay);
        // A headset plugged in (Windows makes it the default): follow it.
        assert_eq!(f.poll(Some("headset".into())), Follow::Switch("headset".into()));
        // Its stream wouldn't open: said once, tried again every poll.
        assert!(f.failed("headset"));
        assert!(!f.failed("headset"));
        assert_eq!(f.poll(Some("headset".into())), Follow::Switch("headset".into()));
        f.opened("headset".into());
        assert_eq!(f.poll(Some("headset".into())), Follow::Stay);
        // Unplugged again: back to the speakers.
        assert_eq!(f.poll(Some("speakers".into())), Follow::Switch("speakers".into()));
        assert!(f.failed("speakers"), "a new failure is news");
        // Opened without knowing the device: the first one read is followed.
        let f = OutputFollower::listening_to(None);
        assert_eq!(f.poll(Some("speakers".into())), Follow::Switch("speakers".into()));
    }

    // ---- quiet sides ----

    const R: u64 = RATE as u64;

    fn lv(end: u64, heard: u64, talk: u64) -> Levels {
        Levels { end: end * R, heard: heard * R, talk: talk * R }
    }

    #[test]
    fn the_call_quiet_while_you_talk_is_said_once_per_stretch() {
        let t = QuietTimings::REAL;
        let mut w = QuietWatch::default();
        // Two minutes of you talking, nothing from the call: not yet.
        assert_eq!(w.check(lv(120, 119, 60), lv(120, 0, 0), t), None);
        // Three: say so.
        assert_eq!(w.check(lv(181, 180, 90), lv(181, 0, 0), t), Some(Side::Them));
        // Once per quiet stretch.
        assert_eq!(w.check(lv(400, 399, 200), lv(400, 0, 0), t), None);
        // The call heard again starts a new stretch, which can warn again.
        assert_eq!(w.check(lv(410, 409, 205), lv(410, 405, 1), t), None);
        assert_eq!(w.check(lv(590, 589, 290), lv(590, 405, 1), t), Some(Side::Them));
    }

    #[test]
    fn the_mic_quiet_while_they_talk_is_said_too() {
        let t = QuietTimings::REAL;
        let mut w = QuietWatch::default();
        assert_eq!(w.check(lv(185, 0, 0), lv(185, 184, 100), t), Some(Side::You));
        // A mic that went away (its audio stalled) still counts the meeting's time.
        let mut w = QuietWatch::default();
        assert_eq!(w.check(lv(30, 29, 20), lv(30, 29, 20), t), None);
        assert_eq!(w.check(lv(30, 29, 20), lv(250, 249, 150), t), Some(Side::You));
    }

    #[test]
    fn a_quiet_meeting_or_a_cough_is_not_a_fault() {
        let t = QuietTimings::REAL;
        let mut w = QuietWatch::default();
        // Both quiet: a break.
        assert_eq!(w.check(lv(600, 10, 8), lv(600, 12, 9), t), None);
        // The other side barely made a sound meanwhile.
        let mut w = QuietWatch::default();
        assert_eq!(w.check(lv(300, 200, 3), lv(300, 0, 0), t), None);
        // Short test timings work the same.
        let short = QuietTimings { quiet: 6 * R, talk: 2 * R };
        let mut w = QuietWatch::default();
        assert_eq!(w.check(lv(5, 5, 5), lv(5, 0, 0), short), None);
        assert_eq!(w.check(lv(6, 6, 6), lv(6, 0, 0), short), Some(Side::Them));
    }

    #[test]
    fn quiet_warnings_say_what_to_do() {
        let three = QuietTimings::REAL.quiet;
        assert_eq!(
            quiet_message(Side::Them, three, None),
            "Yap hasn't heard the call for 3 minutes. If your call plays through a headset, make it Windows' default output device."
        );
        assert_eq!(
            quiet_message(Side::Them, three, Some("Teams call")),
            "Yap hasn't heard your Teams call for 3 minutes. If it plays through a headset, make that Windows' default output device."
        );
        let mic = quiet_message(Side::You, three, Some("Teams call"));
        assert!(mic.starts_with("Yap hasn't heard your mic for 3 minutes."));
        assert!(mic.contains("Settings \u{2192} General"));
        assert!(quiet_message(Side::Them, 6 * R, None).contains("for 6 seconds."));
        assert_eq!(span_words(60 * R), "1 minute");
        assert_eq!(span_words(R), "1 second");
    }

    /// A speech-like loudness pattern: syllables on and off, `seed`-shaped.
    fn talk_audio(secs: f32, seed: u32) -> Vec<f32> {
        let n = (secs * RATE as f32) as usize;
        (0..n)
            .map(|i| {
                let syllable = (i / 2_400) as u32; // 150 ms
                let hash = syllable
                    .wrapping_add(seed.wrapping_mul(1_000_003))
                    .wrapping_mul(2_654_435_761);
                let on = !(hash >> 16).is_multiple_of(3);
                let amp = if on { 0.3 } else { 0.004 };
                amp * (i as f32 * 0.21).sin()
            })
            .collect()
    }

    #[test]
    fn speech_onset_skips_the_leading_silence() {
        let mut s = vec![0.001f32; 2 * RATE]; // 2 s of room noise…
        s.extend(tone(1.0)); // …then speech
        let onset = speech_onset(&s);
        assert!(
            (2 * RATE - ENV_FRAME..=2 * RATE).contains(&onset),
            "{onset}"
        );
        assert_eq!(speech_onset(&tone(1.0)), 0);
        assert_eq!(speech_onset(&[0.0; 100]), 0);
    }

    #[test]
    fn envelope_frames_line_up_across_chunks() {
        let audio = talk_audio(3.0, 1);
        let mut whole = Envelope::default();
        whole.push(&audio);
        let mut pieces = Envelope::default();
        for chunk in audio.chunks(1_234) {
            pieces.push(chunk);
        }
        assert_eq!(whole.frames, pieces.frames);
        assert_eq!(whole.range(10, 20).1.len(), 10);
        assert_eq!(whole.range(140, 999).1.len(), 10);
        assert!(whole.range(500, 600).1.is_empty());
    }

    #[test]
    fn speaker_bleed_follows_the_call_with_a_delay() {
        let call = talk_audio(10.0, 7);
        // The mic hears the call 120 ms late and quieter, plus room noise.
        let delay = RATE * 12 / 100;
        let mut mic: Vec<f32> = vec![0.0; delay];
        mic.extend(call.iter().map(|s| s * 0.3));
        mic.truncate(call.len());
        for (i, s) in mic.iter_mut().enumerate() {
            *s += 0.002 * (i as f32 * 1.7).sin();
        }
        let (mut c, mut m) = (Envelope::default(), Envelope::default());
        c.push(&call);
        m.push(&mic);
        let (cf, cv) = c.range(0, 500);
        let (mf, mv) = m.range(0, 500);
        assert!(echo_correlation(&mv, mf, &cv, cf) > 0.9);
        // The user talking on their own (headphones) doesn't follow it.
        let mut own = Envelope::default();
        own.push(&talk_audio(10.0, 3));
        let (of, ov) = own.range(0, 500);
        assert!(echo_correlation(&ov, of, &cv, cf) < ECHO_MIN_CORRELATION);
        // Too little to compare: no verdict.
        assert_eq!(echo_correlation(&mv[..100], mf, &cv, cf), 0.0);
    }

    #[test]
    fn echoed_text_repeats_theirs_in_order() {
        let them =
            "Can you send the revised budget to finance by Friday? We also need the venue booked.";
        // Bleed, transcribed a little differently.
        assert!(
            echoed_share(
                "can you send the revised budget to finance by friday we also need a venue booked",
                them
            ) > 0.9
        );
        // The user's own answer repeats some words, but adds their own.
        let reply =
            "Sure, happy to do that, I'll get the numbers over to finance after lunch today";
        assert!(echoed_share(reply, them) < ECHO_MIN_SHARE);
        // Too short to judge.
        assert_eq!(echoed_share("Friday budget", them), 0.0);
    }

    #[test]
    fn echo_needs_both_the_words_and_the_timing() {
        // A headphone user confirming the task: same words, own timing.
        assert!(!is_echo(0.2, 0.83));
        // Speakers, a chunk that's only the call: flagged.
        assert!(is_echo(0.85, 0.9));
        // Speakers, the user also talking: kept.
        assert!(!is_echo(0.7, 0.45));
    }

    #[test]
    fn a_full_buffer_drops_new_audio_and_says_so() {
        let buf = buffered_audio(vec![0.0; MAX_BUFFER_SAMPLES - 10]);
        let overflow = AtomicBool::new(false);
        push_audio(&buf, &[0.1; 10], &overflow);
        assert!(!overflow.load(Ordering::Relaxed));
        push_audio(&buf, &[0.1; 10], &overflow);
        assert!(overflow.load(Ordering::Relaxed));
        assert_eq!(buffered(&buf), MAX_BUFFER_SAMPLES);
        // Dropped audio doesn't move the clock; filling stops at the cap.
        let mut c = buf.lock().unwrap();
        assert_eq!(c.end(), MAX_BUFFER_SAMPLES as u64);
        c.fill_if_stalled(MAX_BUFFER_SAMPLES as u64 + R);
        assert_eq!(c.samples.len(), MAX_BUFFER_SAMPLES);
    }
}
