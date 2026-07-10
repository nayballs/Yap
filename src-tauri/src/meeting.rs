//! Meeting recorder — "the notepad that cleans up after your meetings"
//! (OpenWhispr `meetingRecordingStore.ts`, ported to Yap's local-first stack).
//!
//! Captures TWO audio streams while a meeting note is open:
//! - the **mic** ("You") — same device dictation uses, its own cpal stream
//! - the **system output** ("Them") — what the call plays through the
//!   speakers, via **WASAPI loopback** (cpal on Windows: build an *input*
//!   stream on an *output* device)
//!
//! A worker drains each source every ~15 s and transcribes the chunk on the
//! SAME warm engine dictation uses (`pipeline::EngineSlot`, taken per chunk and
//! returned — never held for the whole meeting, so hotkey dictation still
//! works between chunks). Segments `{source: "you"|"them", text, ts}` are
//! emitted live (`yap-meeting-segment`) and persisted to the note's
//! `transcript` every drain. On stop, the UI runs the meeting enhancement
//! (llm::MEETING_NOTE_BASE_PROMPT via `note_enhance`).
//!
//! Unlike OpenWhispr there is NO realtime-cloud path — chunks are transcribed
//! locally, so "live" means ~15 s behind, fully offline.
//!
//! Echo caveat (v1, same as their `oneOnOneAttendee` fast-path): with speakers
//! instead of headphones, the mic hears "Them" too — the UI recommends
//! headphones for clean separation. Speaker diarization is a later item.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use tauri::{AppHandle, Emitter};

use crate::pipeline::{resample_linear, EngineSlot, TARGET_SAMPLE_RATE};

/// Drain-and-transcribe cadence.
const DRAIN_INTERVAL_SECS: u64 = 15;
/// Don't bother transcribing less than this much audio (except the final drain).
const MIN_CHUNK_SECS: usize = 2;
/// Skip chunks whose peak is below this — silence (esp. loopback when nobody
/// speaks) wastes engine time and tempts whisper into hallucinations.
const SILENCE_PEAK: f32 = 0.008;
/// Hard cap per source so a forgotten recording can't eat RAM without bound
/// (~4 h at 16 kHz f32 ≈ 900 MB across both sources).
const MAX_BUFFER_SAMPLES: usize = 4 * 60 * 60 * TARGET_SAMPLE_RATE as usize;

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

/// Build a capture stream into `buf`. `loopback` selects the default OUTPUT
/// device (WASAPI loopback) instead of an input device.
fn build_capture_stream(
    buf: Arc<Mutex<Vec<f32>>>,
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
                if let Ok(mut b) = buf.lock() {
                    if b.len() < MAX_BUFFER_SAMPLES {
                        b.extend_from_slice(&resampled);
                    }
                }
            },
            |err| tracing::warn!("Meeting capture stream error: {}", err),
            None,
        )
        .map_err(|e| format!("Failed to build capture stream: {e}"))
}

/// Take everything buffered for one source (a "chunk").
fn drain(buf: &Arc<Mutex<Vec<f32>>>) -> Vec<f32> {
    buf.lock()
        .map(|mut b| std::mem::take(&mut *b))
        .unwrap_or_default()
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

/// Transcribe one drained chunk on the shared warm engine (take → transcribe →
/// put back; lazily reloads the model if the idle watcher dropped it).
async fn transcribe_chunk(engine_slot: &EngineSlot, samples: Vec<f32>) -> Option<String> {
    if samples.len() < MIN_CHUNK_SECS * TARGET_SAMPLE_RATE as usize / 2
        || peak(&samples) < SILENCE_PEAK
    {
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

/// Start recording into `note_id`. Fails if a session is already running or
/// the streams can't be built. Emits `yap-meeting-state`.
pub fn start(app: AppHandle, engine_slot: EngineSlot, note_id: u64) -> Result<(), String> {
    {
        let guard = SESSION.lock().unwrap_or_else(|p| p.into_inner());
        if guard.is_some() {
            return Err("A meeting is already being recorded".to_string());
        }
    }

    let mic_buf: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
    let sys_buf: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
    let stop = Arc::new(AtomicBool::new(false));

    // cpal streams are !Send — they live on a dedicated thread that builds
    // them, reports the result, then parks until stop.
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), String>>();
    {
        let mic_buf = Arc::clone(&mic_buf);
        let sys_buf = Arc::clone(&sys_buf);
        let stop = Arc::clone(&stop);
        let input_device = crate::config::load().input_device.clone();
        std::thread::Builder::new()
            .name("meeting-capture".into())
            .spawn(move || {
                let mic = build_capture_stream(mic_buf, input_device, false);
                let sys = build_capture_stream(sys_buf, None, true);
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
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
                drop(mic);
                drop(sys);
                tracing::info!("Meeting capture streams stopped");
            })
            .map_err(|e| format!("Failed to spawn capture thread: {e}"))?;
    }
    // Wait for the streams to come up (or fail) before claiming success.
    ready_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .map_err(|_| "Capture thread didn't start in time".to_string())??;

    let session = Session {
        note_id,
        started_ms: now_ms(),
        stop: Arc::clone(&stop),
    };
    {
        let mut guard = SESSION.lock().unwrap_or_else(|p| p.into_inner());
        *guard = Some(session);
    }
    ACTIVE_NOTE.store(note_id, Ordering::SeqCst);
    let _ = app.emit("yap-meeting-state", state());
    tracing::info!(note_id, "Meeting recording started");

    // The drain/transcribe worker.
    tauri::async_runtime::spawn(async move {
        loop {
            // Sleep in small steps so stop is picked up quickly.
            for _ in 0..(DRAIN_INTERVAL_SECS * 5) {
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
            let stopping = stop.load(Ordering::SeqCst);

            for (buf, source) in [(&mic_buf, "you"), (&sys_buf, "them")] {
                let samples = drain(buf);
                // Mid-meeting, hold small chunks back for the next tick (put
                // them back); on the final drain transcribe whatever exists.
                if !stopping
                    && samples.len() < MIN_CHUNK_SECS * TARGET_SAMPLE_RATE as usize
                {
                    if !samples.is_empty() {
                        if let Ok(mut b) = buf.lock() {
                            let mut restored = samples;
                            restored.extend_from_slice(&b);
                            *b = restored;
                        }
                    }
                    continue;
                }
                let ts = now_ms() / 1000;
                if let Some(text) = transcribe_chunk(&engine_slot, samples).await {
                    let seg = crate::notes::TranscriptSegment {
                        source: source.to_string(),
                        text,
                        ts,
                    };
                    let _ = crate::notes::append_transcript(note_id, &[seg.clone()]);
                    let _ = app.emit("yap-meeting-segment", serde_json::json!(seg));
                }
            }

            if stopping {
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
