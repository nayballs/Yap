//! Yap's dictation pipeline.
//!
//! Simpler than Voice Mirror's (no TTS, no AI routing, no modes, no VAD):
//! one mic stream captures audio into a buffer while *recording* is on;
//! toggling off runs STT, applies the dictionary, and injects the text
//! into whatever window is focused. A short chime marks start/stop.

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::time::{Duration, SystemTime};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use tauri::{AppHandle, Emitter};

use crate::config::{self, YapConfig};
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
    config: RwLock<YapConfig>,
    /// Last time the engine did real work (ms since epoch). Drives the idle
    /// model-unload watcher (B1).
    last_activity: AtomicU64,
    /// Foreground window handle captured at record-start (0 = none / our own
    /// window). The transcript is pasted back into this window so focus changes
    /// during transcription don't misfire.
    target_hwnd: AtomicIsize,
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

    fn mute_while_recording(&self) -> bool {
        self.config
            .read()
            .map(|c| c.mute_while_recording)
            .unwrap_or(false)
    }

    fn output_device(&self) -> Option<String> {
        self.config.read().ok().and_then(|c| c.output_device.clone())
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

    fn toggle(self: &Arc<Self>) {
        if self.recording.load(Ordering::SeqCst) {
            self.stop_and_transcribe();
        } else {
            self.start_recording();
        }
    }

    /// Route a hotkey press/release through the configured recording mode.
    ///
    /// - `toggle` mode: act on press only (flip recording on/off). Ignore release.
    /// - `pushToTalk` mode: press starts (if idle), release stops + transcribes
    ///   (if recording).
    fn on_key(self: &Arc<Self>, pressed: bool) {
        if self.recording_mode() == "pushToTalk" {
            if pressed {
                if !self.recording.load(Ordering::SeqCst) {
                    self.start_recording();
                }
            } else if self.recording.load(Ordering::SeqCst) {
                self.stop_and_transcribe();
            }
        } else if pressed {
            self.toggle();
        }
    }

    fn start_recording(&self) {
        if let Ok(mut buf) = self.buffer.lock() {
            buf.clear();
        }
        // Capture the window the user is dictating into, before the overlay/pill
        // (or anything else) can steal focus. Restored at paste time.
        let hwnd = crate::text_injector::current_foreground().unwrap_or(0);
        self.target_hwnd.store(hwnd, Ordering::Relaxed);
        self.recording.store(true, Ordering::SeqCst);
        self.touch_activity();
        if self.mute_while_recording() {
            crate::mute::mute_system_output();
        }
        let _ = self.app.emit("yap-state", "recording");
        if self.sound_enabled() {
            crate::sound::play_start(self.audio_feedback_volume(), self.output_device().as_deref());
        }
        tracing::info!("Recording started");
    }

    /// Stop recording and discard the buffered audio (no transcription).
    fn cancel(&self) {
        if !self.recording.swap(false, Ordering::SeqCst) {
            return;
        }
        crate::mute::unmute_system_output();
        if let Ok(mut buf) = self.buffer.lock() {
            buf.clear();
        }
        let _ = self.app.emit("yap-state", "idle");
        tracing::info!("Recording cancelled (audio discarded)");
    }

    fn stop_and_transcribe(self: &Arc<Self>) {
        self.recording.store(false, Ordering::SeqCst);
        crate::mute::unmute_system_output();
        let _ = self.app.emit("yap-state", "processing");
        if self.sound_enabled() {
            crate::sound::play_stop(self.audio_feedback_volume(), self.output_device().as_deref());
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
            let _ = self.app.emit("yap-state", "idle");
            return;
        }

        // Set on any failure below so we can tell the user instead of silently
        // returning to idle (kept short so it fits the overlay capsule).
        let mut error_msg: Option<&'static str> = None;

        // Take the engine out so the mutex isn't held across the await.
        let engine = self.engine.lock().ok().and_then(|mut g| g.take());
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
                        let _ = self.app.emit("yap-state", "needs-model");
                        return;
                    }
                }
            }
        };

        // Snapshot the language/translate settings for this transcription.
        // `"auto"` (or a model that ignores it) maps to `None`.
        let (language, translate) = self
            .config
            .read()
            .map(|c| {
                let lang = if c.selected_language == "auto" {
                    None
                } else {
                    Some(c.selected_language.clone())
                };
                (lang, c.translate_to_english)
            })
            .unwrap_or((None, false));

        let outcome = tokio::task::spawn_blocking(move || {
            let result = engine.transcribe(&audio, language.as_deref(), translate);
            (engine, result)
        })
        .await;

        // Mark activity so the idle watcher counts from end-of-transcription.
        self.touch_activity();

        match outcome {
            Ok((engine, transcription)) => {
                // Put the (warm) engine back for next time.
                if let Ok(mut g) = self.engine.lock() {
                    *g = Some(engine);
                }
                match transcription {
                    Ok(text) => {
                        // Snapshot the injection- and cleanup-related config under
                        // one read lock.
                        let (
                            dict,
                            append_space,
                            auto_submit,
                            auto_submit_key,
                            restore_clipboard,
                            pp_enabled,
                            pp_base_url,
                            pp_api_key,
                            pp_model,
                            pp_prompt,
                        ) = self
                            .config
                            .read()
                            .map(|c| {
                                (
                                    c.dictionary.clone(),
                                    c.append_trailing_space,
                                    c.auto_submit,
                                    c.auto_submit_key.clone(),
                                    c.restore_clipboard,
                                    c.post_process_enabled,
                                    c.pp_base_url.clone(),
                                    c.pp_api_key.clone(),
                                    c.pp_model.clone(),
                                    c.pp_prompt.clone(),
                                )
                            })
                            .unwrap_or_else(|_| {
                                (
                                    Vec::new(),
                                    false,
                                    false,
                                    "enter".to_string(),
                                    true,
                                    false,
                                    String::new(),
                                    String::new(),
                                    String::new(),
                                    String::new(),
                                )
                            });

                        // Optional AI cleanup pass (best-effort). Runs before the
                        // dictionary so the user's corrections always have the
                        // final say. Any error/timeout falls back to the raw
                        // transcript — dictation is never blocked. The state stays
                        // `processing` for the extra latency.
                        let raw = text.trim().to_string();
                        let cleaned = if pp_enabled
                            && !pp_base_url.is_empty()
                            && !raw.is_empty()
                        {
                            match crate::llm::cleanup(
                                &raw,
                                &pp_base_url,
                                &pp_api_key,
                                &pp_model,
                                &pp_prompt,
                            )
                            .await
                            {
                                Ok(c) if !c.trim().is_empty() => c,
                                Ok(_) => raw,
                                Err(e) => {
                                    tracing::warn!("AI cleanup failed, using raw: {}", e);
                                    raw
                                }
                            }
                        } else {
                            raw
                        };

                        let mut corrected = config::apply_dictionary(cleaned.trim(), &dict);
                        if !corrected.is_empty() {
                            if append_space {
                                corrected.push(' ');
                            }
                            tracing::info!(text = %corrected, "Transcript");
                            let _ = self.app.emit("yap-transcript", corrected.clone());
                            let target = match self.target_hwnd.load(Ordering::Relaxed) {
                                0 => None,
                                h => Some(h),
                            };
                            match crate::text_injector::inject_text(
                                &corrected,
                                restore_clipboard,
                                target,
                            )
                            .await
                            {
                                Ok(()) => {
                                    if auto_submit {
                                        if let Err(e) =
                                            crate::text_injector::press_submit(&auto_submit_key).await
                                        {
                                            tracing::warn!("Auto-submit failed: {}", e);
                                        }
                                    }
                                }
                                Err(e) => tracing::warn!("Inject failed: {}", e),
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Transcription failed: {}", e);
                        error_msg = Some("Transcription failed — check your model");
                    }
                }
            }
            Err(e) => {
                tracing::error!("STT task panicked: {}", e);
                error_msg = Some("Transcription crashed — try another model");
            }
        }

        // Never fail silently: surface a brief error to the UI, otherwise drop
        // back to idle.
        match error_msg {
            Some(msg) => {
                let _ = self.app.emit("yap-error", msg);
                let _ = self.app.emit("yap-state", "error");
            }
            None => {
                let _ = self.app.emit("yap-state", "idle");
            }
        }
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
    /// works) and emits `yap-state: needs-model` until a model is downloaded.
    pub fn start(app: AppHandle, cfg: YapConfig) -> Result<Self, String> {
        let data_dir = config::data_dir();
        let engine = match stt::create_stt_engine(&data_dir, &cfg.model_size, cfg.use_gpu) {
            Ok(e) => Some(e),
            Err(SttError::ModelNotFound(_)) => {
                tracing::warn!("STT model not found — Yap needs a model download");
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
            last_activity: AtomicU64::new(now_ms()),
            target_hwnd: AtomicIsize::new(0),
        });

        spawn_idle_watcher(&shared);

        let stream = build_input_stream(&shared, cfg.input_device.as_deref())?;

        let _ = app.emit("yap-state", if has_engine { "idle" } else { "needs-model" });

        Ok(Self {
            shared,
            _stream: SendStream(stream),
        })
    }

    /// Toggle recording (called from the pill button's `toggle_recording`).
    pub fn toggle(&self) {
        self.shared.toggle();
    }

    /// Route a hotkey press/release event through the configured recording mode.
    /// Called from the input-hook listeners for both press and release.
    pub fn on_key(&self, pressed: bool) {
        self.shared.on_key(pressed);
    }

    /// Stop recording and discard the audio (no transcription).
    pub fn cancel(&self) {
        self.shared.cancel();
    }

    /// Install a freshly-created STT engine (e.g. after a model download).
    pub fn set_engine(&self, engine: SttAdapter) {
        if let Ok(mut g) = self.shared.engine.lock() {
            *g = Some(engine);
        }
        let _ = self.shared.app.emit("yap-state", "idle");
    }

    /// Replace the live config (e.g. after the user saves settings).
    pub fn update_config(&self, cfg: YapConfig) {
        if let Ok(mut c) = self.shared.config.write() {
            *c = cfg;
        }
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
    // Time-domain amplitude meter for the scrolling waveform (Claude Code
    // style): every ~30 ms emit one peak level (raw 0..1) that the pill/overlay
    // push onto a scrolling history. ~1280 samples at 16 kHz ≈ 80 ms per bar
    // for a calm, readable scroll.
    const AMP_WINDOW: usize = 1280;
    let mut amp_peak: f32 = 0.0;
    let mut amp_count: usize = 0;

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
                // Accumulate the peak over ~30 ms, then emit it as the next
                // scrolling-waveform bar. The frontend shapes (gain/curve) and
                // scrolls it.
                for &s in &resampled {
                    let a = s.abs();
                    if a > amp_peak {
                        amp_peak = a;
                    }
                }
                amp_count += resampled.len();
                if amp_count >= AMP_WINDOW {
                    let _ = cb_shared.app.emit("yap-amp", amp_peak);
                    amp_peak = 0.0;
                    amp_count = 0;
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
