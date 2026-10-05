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
//! Echo caveat (same as their `oneOnOneAttendee` fast-path): with speakers
//! instead of headphones, the mic hears "Them" too — the UI recommends
//! headphones for clean separation. Speaker diarization is a later item.
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
        let result =
            engine.transcribe(&samples, language.as_deref(), translate, dict_prompt.as_deref());
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
        // (source, buffer, samples handed to the engine so far — which dates
        // the next chunk)
        let mut sources = [("you", mic_buf, 0u64), ("them", sys_buf, 0u64)];
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
            backlog = false;

            let mut batch: Vec<TranscriptSegment> = Vec::new();
            for (source, buf, taken) in sources.iter_mut() {
                let Some(chunk) = take_chunk(buf, stopping) else {
                    continue;
                };
                let start_ms = started_ms + *taken * 1000 / RATE as u64;
                *taken += chunk.len() as u64;
                if let Some(text) = transcribe_chunk(&engine_slot, chunk).await {
                    batch.push(TranscriptSegment {
                        source: source.to_string(),
                        text,
                        ts: start_ms / 1000,
                        echo: false,
                    });
                }
                let left = buffered(buf);
                backlog |= left >= CHUNK_SECS * RATE || (stopping && left > 0);
            }
            batch.sort_by_key(|s| s.ts);
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
        assert!((13 * RATE..13 * RATE + RATE / 10).contains(&cut), "cut at {cut}");
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
