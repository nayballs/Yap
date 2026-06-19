//! Blip's dictation pipeline.
//!
//! Simpler than Voice Mirror's (no TTS, no AI routing, no modes, no VAD):
//! one mic stream captures audio into a buffer while *recording* is on;
//! toggling off runs STT, applies the dictionary, and injects the text
//! into whatever window is focused. A short chime marks start/stop.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use tauri::{AppHandle, Emitter};

use crate::config::{self, BlipConfig};
use crate::stt::{self, SttAdapter, SttError};

const TARGET_SAMPLE_RATE: u32 = 16_000;

/// cpal's `Stream` is `!Send` on some platforms; we only hold it alive.
struct SendStream(#[allow(dead_code)] cpal::Stream);
// SAFETY: the stream is only kept alive and dropped; cpal manages its own
// internal threading and we never touch it from another thread.
unsafe impl Send for SendStream {}

/// State shared between the audio callback, the hotkey toggle, and STT.
struct Shared {
    recording: AtomicBool,
    buffer: Mutex<Vec<f32>>,
    engine: Mutex<Option<SttAdapter>>,
    app: AppHandle,
    config: RwLock<BlipConfig>,
}

impl Shared {
    fn sound_enabled(&self) -> bool {
        self.config.read().map(|c| c.sound_enabled).unwrap_or(true)
    }

    fn toggle(self: &Arc<Self>) {
        if self.recording.load(Ordering::SeqCst) {
            self.stop_and_transcribe();
        } else {
            self.start_recording();
        }
    }

    fn start_recording(&self) {
        if let Ok(mut buf) = self.buffer.lock() {
            buf.clear();
        }
        self.recording.store(true, Ordering::SeqCst);
        let _ = self.app.emit("blip-state", "recording");
        if self.sound_enabled() {
            crate::sound::play_start();
        }
        tracing::info!("Recording started");
    }

    fn stop_and_transcribe(self: &Arc<Self>) {
        self.recording.store(false, Ordering::SeqCst);
        let _ = self.app.emit("blip-state", "processing");
        if self.sound_enabled() {
            crate::sound::play_stop();
        }
        let audio = self
            .buffer
            .lock()
            .map(|mut b| std::mem::take(&mut *b))
            .unwrap_or_default();
        tracing::info!(samples = audio.len(), "Recording stopped, transcribing");

        let shared = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            shared.run_stt(audio).await;
        });
    }

    async fn run_stt(self: Arc<Self>, audio: Vec<f32>) {
        if audio.is_empty() {
            let _ = self.app.emit("blip-state", "idle");
            return;
        }

        // Take the engine out so the mutex isn't held across the await.
        let engine = self.engine.lock().ok().and_then(|mut g| g.take());
        let Some(engine) = engine else {
            tracing::warn!("No STT engine (model missing) — cannot transcribe");
            let _ = self.app.emit("blip-state", "needs-model");
            return;
        };

        let outcome = tokio::task::spawn_blocking(move || {
            let result = engine.transcribe(&audio);
            (engine, result)
        })
        .await;

        match outcome {
            Ok((engine, transcription)) => {
                // Put the (warm) engine back for next time.
                if let Ok(mut g) = self.engine.lock() {
                    *g = Some(engine);
                }
                match transcription {
                    Ok(text) => {
                        let dict = self
                            .config
                            .read()
                            .map(|c| c.dictionary.clone())
                            .unwrap_or_default();
                        let corrected = config::apply_dictionary(text.trim(), &dict);
                        if !corrected.is_empty() {
                            tracing::info!(text = %corrected, "Transcript");
                            let _ = self.app.emit("blip-transcript", corrected.clone());
                            if let Err(e) = crate::text_injector::inject_text(&corrected).await {
                                tracing::warn!("Inject failed: {}", e);
                            }
                        }
                    }
                    Err(e) => tracing::error!("Transcription failed: {}", e),
                }
            }
            Err(e) => tracing::error!("STT task panicked: {}", e),
        }

        let _ = self.app.emit("blip-state", "idle");
    }
}

/// The running pipeline. Owns the mic stream (kept alive) + shared state.
pub struct Pipeline {
    shared: Arc<Shared>,
    _stream: SendStream,
}

impl Pipeline {
    /// Start audio capture and (best-effort) load the STT engine.
    ///
    /// A missing model is tolerated: the pipeline still runs (so the hotkey
    /// works) and emits `blip-state: needs-model` until a model is downloaded.
    pub fn start(app: AppHandle, cfg: BlipConfig) -> Result<Self, String> {
        let data_dir = config::data_dir();
        let engine = match stt::create_stt_engine(
            "whisper-local",
            &data_dir,
            Some(&cfg.model_size),
            cfg.use_gpu,
        ) {
            Ok(e) => Some(e),
            Err(SttError::ModelNotFound(_)) => {
                tracing::warn!("Whisper model not found — Blip needs a model download");
                None
            }
            Err(e) => {
                tracing::error!("STT engine init failed: {}", e);
                None
            }
        };

        let has_engine = engine.is_some();

        let shared = Arc::new(Shared {
            recording: AtomicBool::new(false),
            buffer: Mutex::new(Vec::new()),
            engine: Mutex::new(engine),
            app: app.clone(),
            config: RwLock::new(cfg.clone()),
        });

        let stream = build_input_stream(&shared, cfg.input_device.as_deref())?;

        let _ = app.emit("blip-state", if has_engine { "idle" } else { "needs-model" });

        Ok(Self {
            shared,
            _stream: SendStream(stream),
        })
    }

    /// Toggle recording (called from the global hotkey).
    pub fn toggle(&self) {
        self.shared.toggle();
    }

    /// Install a freshly-created STT engine (e.g. after a model download).
    pub fn set_engine(&self, engine: SttAdapter) {
        if let Ok(mut g) = self.shared.engine.lock() {
            *g = Some(engine);
        }
        let _ = self.shared.app.emit("blip-state", "idle");
    }

    /// Replace the live config (e.g. after the user saves settings).
    pub fn update_config(&self, cfg: BlipConfig) {
        if let Ok(mut c) = self.shared.config.write() {
            *c = cfg;
        }
    }
}

/// Build and start the mic input stream.
fn build_input_stream(shared: &Arc<Shared>, device_name: Option<&str>) -> Result<cpal::Stream, String> {
    let host = cpal::default_host();
    let device = match device_name {
        Some(name) => host
            .input_devices()
            .map_err(|e| format!("Failed to enumerate input devices: {}", e))?
            .find(|d| d.name().map(|n| n == name).unwrap_or(false))
            .ok_or_else(|| format!("Input device not found: {}", name))?,
        None => host
            .default_input_device()
            .ok_or_else(|| "No default input device available".to_string())?,
    };

    let dev_name = device.name().unwrap_or_else(|_| "unknown".into());
    let default_config = device
        .default_input_config()
        .map_err(|e| format!("Failed to get default input config: {}", e))?;
    let native_rate = default_config.sample_rate().0;
    let channels = default_config.channels();
    tracing::info!(device = %dev_name, native_rate, channels, "Audio input config");

    let stream_config = cpal::StreamConfig {
        channels,
        sample_rate: cpal::SampleRate(native_rate),
        buffer_size: cpal::BufferSize::Default,
    };
    let needs_resample = native_rate != TARGET_SAMPLE_RATE;
    let needs_downmix = channels > 1;

    let cb_shared = Arc::clone(shared);
    let mut level_counter: usize = 0;

    let stream = device
        .build_input_stream(
            &stream_config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                if !cb_shared.recording.load(Ordering::Relaxed) {
                    return;
                }
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
                if let Ok(mut buf) = cb_shared.buffer.lock() {
                    buf.extend_from_slice(&resampled);
                }
                // Emit a normalized level for the pill's waveform (throttled).
                level_counter = level_counter.wrapping_add(1);
                if level_counter % 3 == 0 && !resampled.is_empty() {
                    let rms = (resampled.iter().map(|s| s * s).sum::<f32>()
                        / resampled.len() as f32)
                        .sqrt();
                    let level = (rms * 10.0).min(1.0);
                    let _ = cb_shared.app.emit("blip-level", level);
                }
            },
            move |err| tracing::error!("Audio input stream error: {}", err),
            None,
        )
        .map_err(|e| format!("Failed to build input stream: {}", e))?;

    stream
        .play()
        .map_err(|e| format!("Failed to start input stream: {}", e))?;
    tracing::info!("Audio capture started");
    Ok(stream)
}

/// Simple linear resampler from one rate to another.
fn resample_linear(input: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
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
