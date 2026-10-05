//! Meeting recorder — "the notepad that cleans up after your meetings"
//! (OpenWhispr `meetingRecordingStore.ts`, ported to Yap's local-first stack).
//!
//! Captures TWO audio streams while a meeting note is open:
//! - the **mic** ("You") — same device dictation uses, its own cpal stream
//! - the **system output** ("Them") — what the call plays through the
//!   speakers, via **WASAPI loopback** (cpal on Windows: build an *input*
//!   stream on an *output* device), so it works with any call app, no bot
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
//! captured) are emitted live (`yap-meeting-segment`), persisted to the
//! note's `transcript` as they come (a crash loses at most one chunk), and
//! handed to the rolling digests (`meeting_summary.rs`) that keep the
//! end-of-meeting action plan fast for meetings of any length.
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
//! In e2e test runs (debug builds, `YAP_E2E=1`) no device is opened: the
//! audio comes from test files, or nowhere (`e2e::spawn_meeting_audio`).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use tauri::{AppHandle, Emitter};

use crate::notes::TranscriptSegment;
use crate::pipeline::{resample_linear, EngineSlot, TARGET_SAMPLE_RATE};

const RATE: usize = TARGET_SAMPLE_RATE as usize;
/// How often the worker looks for finished chunks.
const DRAIN_INTERVAL_SECS: u64 = 15;
/// A chunk is cut once a source has buffered this much audio…
const CHUNK_SECS: usize = 15;
/// …at the quietest moment of the last few seconds…
const CUT_SEARCH_SECS: usize = 4;
/// …short of the newest bit, where a word may still be going on.
const CUT_MARGIN_MS: usize = 300;
/// A backlog is worked off in chunks of at most this much.
const CHUNK_MAX_SECS: usize = 30;
/// Don't transcribe less than this much audio.
const MIN_CHUNK_SECS: usize = 1;
/// Skip chunks whose peak is below this — silence (esp. loopback when nobody
/// speaks) wastes engine time and tempts whisper into hallucinations.
const SILENCE_PEAK: f32 = 0.008;
/// Per-source buffer cap (~77 MB of 16 kHz f32). The worker keeps a buffer
/// near one chunk, so this is only reached when transcription can't keep up.
pub(crate) const MAX_BUFFER_SAMPLES: usize = 20 * 60 * RATE;

/// One source's audio, shared by its capture callback and the worker.
pub(crate) type AudioBuf = Arc<Mutex<Vec<f32>>>;

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
pub(crate) fn push_audio(buf: &AudioBuf, samples: &[f32], overflow: &AtomicBool) {
    if let Ok(mut b) = buf.lock() {
        if b.len() + samples.len() <= MAX_BUFFER_SAMPLES {
            b.extend_from_slice(samples);
        } else {
            overflow.store(true, Ordering::Relaxed);
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

/// Open the mic + loopback streams on their own thread (cpal streams are
/// !Send: the thread builds them, reports the result, then parks until stop).
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
            let mic = build_capture_stream(mic_buf, Arc::clone(&overflow), input_device, false);
            let sys = build_capture_stream(sys_buf, overflow, None, true);
            let (mic, sys) = match (mic, sys) {
                (Ok(m), Ok(s)) => (m, s),
                (Err(e), _) | (_, Err(e)) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
            if let Err(e) = mic.play().and_then(|_| sys.play()) {
                let _ = ready_tx.send(Err(format!("Failed to start capture: {e}")));
                return;
            }
            let _ = ready_tx.send(Ok(()));
            while !stop.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(200));
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
/// newest audio (in a backlog, before `CHUNK_MAX_SECS`). The final drain
/// takes whatever is left, in pieces of at most `CHUNK_MAX_SECS`.
fn next_cut(samples: &[f32], final_drain: bool) -> Option<usize> {
    let len = samples.len();
    let max = CHUNK_MAX_SECS * RATE;
    if len == 0 || (!final_drain && len < CHUNK_SECS * RATE) {
        return None;
    }
    if final_drain && len <= max {
        return Some(len);
    }
    let hi = if len > max {
        max
    } else {
        len - CUT_MARGIN_MS * RATE / 1000
    };
    let lo = hi
        .saturating_sub(CUT_SEARCH_SECS * RATE)
        .max(MIN_CHUNK_SECS * RATE);
    if lo >= hi {
        return Some(hi);
    }
    Some(crate::media::quietest_frame(samples, lo, hi, RATE).0)
}

/// Take the next chunk off `buf` (see [`next_cut`]); the rest stays queued.
fn take_chunk(buf: &AudioBuf, final_drain: bool) -> Option<Vec<f32>> {
    let mut b = buf.lock().ok()?;
    let cut = next_cut(&b, final_drain)?;
    let rest = b.split_off(cut);
    Some(std::mem::replace(&mut *b, rest))
}

fn buffered(buf: &AudioBuf) -> usize {
    buf.lock().map(|b| b.len()).unwrap_or(0)
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

/// Transcribe one chunk on the shared warm engine (take → transcribe →
/// put back; lazily reloads the model if the idle watcher dropped it).
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
    let dict_prompt = crate::config::dictionary_prompt(&cfg.dictionary);

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
                    let t = text.trim().to_string();
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
    /// Samples handed to the engine so far: dates the next chunk.
    taken: u64,
    loudness: Envelope,
}

impl Source {
    fn new(buf: AudioBuf) -> Self {
        Source {
            buf,
            taken: 0,
            loudness: Envelope::default(),
        }
    }

    /// The next chunk, transcribed, or `None` (nothing ready, or silence).
    async fn next(&mut self, engine_slot: &EngineSlot, final_drain: bool) -> Option<Heard> {
        let chunk = take_chunk(&self.buf, final_drain)?;
        let from = self.taken;
        self.taken += chunk.len() as u64;
        self.loudness.push(&chunk);
        let onset = from + speech_onset(&chunk) as u64;
        let text = transcribe_chunk(engine_slot, chunk).await?;
        Some(Heard {
            from,
            to: self.taken,
            onset,
            text,
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
    text: String,
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

    let mic_buf: AudioBuf = Arc::new(Mutex::new(Vec::new()));
    let sys_buf: AudioBuf = Arc::new(Mutex::new(Vec::new()));
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
        let mut theirs: std::collections::VecDeque<(u64, u64, String)> = Default::default();
        let tick = drain_interval();
        let mut backlog = false;
        let mut warned = false;
        loop {
            // Wait for the next tick (in small steps, so stop is picked up
            // quickly) unless there's a backlog to work off.
            if !backlog {
                let steps = (tick.as_millis() / 200).max(1);
                for _ in 0..steps {
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
            }
            let stopping = stop.load(Ordering::SeqCst);

            // "Them" first, so the echo check can hold "you" up against it.
            // Segments are dated (and ordered) by where their speech starts.
            let mut batch: Vec<(u64, TranscriptSegment)> = Vec::new();
            let at = |pos: u64| (started_ms + pos * 1000 / RATE as u64) / 1000;
            if let Some(h) = them.next(&engine_slot, stopping).await {
                let seg = TranscriptSegment {
                    source: "them".to_string(),
                    text: h.text.clone(),
                    ts: at(h.onset),
                    echo: false,
                };
                batch.push((h.onset, seg));
                theirs.push_back((h.from, h.to, h.text));
                while theirs.len() > 4 {
                    theirs.pop_front();
                }
            }
            if let Some(h) = you.next(&engine_slot, stopping).await {
                let (mic, call) = (&you.loudness, &them.loudness);
                let echo = sounds_like_echo(mic, call, h.from, h.to, &h.text, &theirs);
                let seg = TranscriptSegment {
                    source: "you".to_string(),
                    text: h.text,
                    ts: at(h.onset),
                    echo,
                };
                batch.push((h.onset, seg));
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

    #[test]
    fn a_backlog_is_cut_into_bounded_chunks() {
        let buf: AudioBuf = Arc::new(Mutex::new(tone(95.0)));
        let mut chunks = Vec::new();
        while let Some(c) = take_chunk(&buf, true) {
            assert!(c.len() <= CHUNK_MAX_SECS * RATE);
            chunks.push(c.len());
        }
        assert_eq!(chunks.iter().sum::<usize>(), 95 * RATE);
        assert!(chunks.len() >= 4);
        assert_eq!(buffered(&buf), 0);
        // Mid-meeting, the remainder after a cut stays queued.
        let buf: AudioBuf = Arc::new(Mutex::new(tone(20.0)));
        let first = take_chunk(&buf, false).unwrap().len();
        assert!((15 * RATE..20 * RATE).contains(&first));
        assert_eq!(first + buffered(&buf), 20 * RATE);
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
        let buf: AudioBuf = Arc::new(Mutex::new(vec![0.0; MAX_BUFFER_SAMPLES - 10]));
        let overflow = AtomicBool::new(false);
        push_audio(&buf, &[0.1; 10], &overflow);
        assert!(!overflow.load(Ordering::Relaxed));
        push_audio(&buf, &[0.1; 10], &overflow);
        assert!(overflow.load(Ordering::Relaxed));
        assert_eq!(buffered(&buf), MAX_BUFFER_SAMPLES);
    }
}
