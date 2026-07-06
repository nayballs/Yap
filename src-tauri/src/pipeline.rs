//! Yap's dictation pipeline.
//!
//! Simpler than Voice Mirror's (no TTS, no AI routing, no modes, no VAD):
//! one mic stream captures audio into a buffer while *recording* is on;
//! toggling off runs STT, applies the dictionary, and injects the text
//! into whatever window is focused. A short chime marks start/stop.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::time::{Duration, SystemTime};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use tauri::{AppHandle, Emitter};

use crate::config::{self, YapConfig};
use crate::stt::{self, SttAdapter, SttError};

pub(crate) const TARGET_SAMPLE_RATE: u32 = 16_000;

/// The shared warm-engine slot. The pipeline owns it; the meeting recorder
/// (`meeting.rs`) clones the Arc and takes/returns the engine per chunk, so
/// dictation and meeting transcription interleave instead of double-loading
/// the model.
pub(crate) type EngineSlot = std::sync::Arc<Mutex<Option<SttAdapter>>>;

/// How much audio (16 kHz mono samples) to keep in the rolling pre-roll ring so
/// the first word isn't clipped: speech that started a moment *before* the user
/// pressed the key is prepended to the recording. 300 ms × 16 kHz.
const PREROLL_SAMPLES: usize = (0.3 * TARGET_SAMPLE_RATE as f64) as usize;

/// Hard cap on a single recording's captured audio (16 kHz mono f32 ≈ 64 KB/s).
/// A stuck hotkey or a forgotten toggle-mode session would otherwise grow the
/// buffer without bound (~3.8 MB/min) until the process runs out of memory. At
/// 15 minutes that's ~57 MB — far longer than any real dictation — after which
/// the callback stops appending (and logs once) rather than risk an OOM.
const MAX_RECORDING_SAMPLES: usize = 15 * 60 * TARGET_SAMPLE_RATE as usize;

/// cpal's `Stream` is `!Send` on some platforms; we only hold it alive.
struct SendStream(#[allow(dead_code)] cpal::Stream);
// SAFETY: the stream is only kept alive and dropped; cpal manages its own
// internal threading and we never touch it from another thread.
unsafe impl Send for SendStream {}

/// State shared between the audio callback, the hotkey toggle, and STT.
struct Shared {
    recording: AtomicBool,
    buffer: Mutex<Vec<f32>>,
    /// Rolling ring of the most recent ~300 ms of mic audio, maintained while
    /// *idle* so `start_recording` can seed the buffer with the moment before the
    /// keypress (anti first-word-clipping). Capped at `PREROLL_SAMPLES`.
    preroll: Mutex<VecDeque<f32>>,
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
    /// Whether the current recording session is **edit/rewrite mode** (the spoken
    /// words are an instruction to rewrite `selection`) rather than dictation.
    /// Set at record-start, read in `run_stt`.
    edit_mode: AtomicBool,
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
    mic_test: AtomicBool,
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
    /// - `pushToTalk` mode: press starts (if idle), release stops + transcribes
    ///   (if recording).
    ///
    /// `edit` selects edit/rewrite mode (the dictation hotkey passes `false`, the
    /// edit hotkey passes `true`).
    fn on_key(self: &Arc<Self>, pressed: bool, edit: bool) {
        if self.recording_mode() == "pushToTalk" {
            if pressed {
                if !self.recording.load(Ordering::SeqCst) {
                    self.start_recording(edit);
                }
            } else if self.recording.load(Ordering::SeqCst) {
                self.stop_and_transcribe();
            }
        } else if pressed {
            self.toggle(edit);
        }
    }

    fn start_recording(self: &Arc<Self>, edit: bool) {
        // A previous transcription is still running — ignore the start so we don't
        // spawn an overlapping `run_stt` (which would rebuild a duplicate model).
        // Processing is normally sub-second on a GPU, so this rarely bites.
        if self.processing.load(Ordering::SeqCst) {
            tracing::info!("Ignoring start — a transcription is still processing");
            return;
        }
        // Seed the buffer with the pre-roll ring (the ~300 ms before the keypress)
        // so a word already in flight isn't clipped.
        let pre: Vec<f32> = self
            .preroll
            .lock()
            .map(|p| p.iter().copied().collect())
            .unwrap_or_default();
        if let Ok(mut buf) = self.buffer.lock() {
            buf.clear();
            buf.extend_from_slice(&pre);
        }
        // Capture the window the user is dictating into, before the overlay/pill
        // (or anything else) can steal focus. Restored at paste time.
        let hwnd = crate::text_injector::current_foreground().unwrap_or(0);
        self.target_hwnd.store(hwnd, Ordering::Relaxed);

        // Edit/rewrite mode: grab the current selection NOW, while the target app
        // still has focus (before recording), so the spoken instruction can be
        // applied to it. Empty selection → write mode.
        self.edit_mode.store(edit, Ordering::SeqCst);
        if edit {
            let target = if hwnd == 0 { None } else { Some(hwnd) };
            let sel = crate::selection::capture_selection(target);
            if let Ok(mut g) = self.selection.lock() {
                *g = sel;
            }
        }

        self.recording.store(true, Ordering::SeqCst);
        self.touch_activity();
        if self.mute_while_recording() {
            crate::mute::mute_system_output();
        }
        let _ = self.app.emit("yap-state", "recording");
        if self.sound_enabled() {
            crate::sound::play_start(self.audio_feedback_volume(), self.output_device().as_deref());
        }
        // Opt-in live partials: spawn a worker that re-transcribes the growing
        // buffer while recording. Off by default (extra GPU load).
        self.maybe_start_streaming();
        tracing::info!("Recording started");
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

    /// Stop recording and discard the buffered audio (no transcription).
    fn cancel(&self) {
        if !self.recording.swap(false, Ordering::SeqCst) {
            return;
        }
        crate::mute::unmute_system_output();
        if let Ok(mut buf) = self.buffer.lock() {
            buf.clear();
        }
        // Drop any edit-mode selection/state so it can't leak into a later session.
        if let Ok(mut g) = self.selection.lock() {
            *g = None;
        }
        self.edit_mode.store(false, Ordering::SeqCst);
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

    /// Should this dictation wake the Voice Agent? Port of OpenWhispr's
    /// `resolveDictationAgentReachability` + `detectAgentName` gate: the
    /// Voice-Agent scope must be enabled with a provider (BYOK/custom providers
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
    /// Uses the **Voice Agent** scope's own provider/model/prompt when that scope is
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

    /// Shared agent runner: apply `instruction` via the Voice-Agent scope (or
    /// the global cleanup endpoint) and paste the result. `selection` empty =
    /// write mode. Two callers: the edit hotkey (`run_rewrite`, selection
    /// captured at key-press) and the **wake-word path** (no selection — the
    /// whole transcript is the instruction, OpenWhispr-style).
    async fn run_agent(self: &Arc<Self>, instruction: String, selection: String) {
        let (base_url, api_key, model, provider, body, disable_thinking, restore_clipboard) = self
            .config
            .read()
            .map(|c| {
                // Voice Agent scope (OpenWhispr "Language Models" bubble, step 3):
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
            tracing::warn!("Edit mode needs an AI cleanup endpoint (none configured)");
            let _ = self
                .app
                .emit("yap-error", "Set up AI cleanup to use edit mode");
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
                "yap-error",
                format!("Voice Agent: no {provider} API key — add one in Language Models"),
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
                let out = result.trim().to_string();
                if out.is_empty() {
                    let _ = self.app.emit("yap-state", "idle");
                    return;
                }
                tracing::info!(text = %out, "Rewrite");
                let _ = self.app.emit("yap-transcript", out.clone());
                if let Err(e) =
                    crate::text_injector::inject_text(&out, restore_clipboard, target).await
                {
                    tracing::warn!("Rewrite inject failed: {}", e);
                }
                let _ = self.app.emit("yap-state", "idle");
            }
            Err(e) => {
                // Don't fall back to typing the raw instruction — that would paste
                // "make this a list" into the doc. Surface an error instead.
                tracing::warn!("Rewrite failed: {}", e);
                let _ = self.app.emit(
                    "yap-error",
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

        // 2. Warm engine, or lazy-reload (same pattern as run_stt).
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
            let outcome = tokio::task::spawn_blocking(move || {
                let result = engine.transcribe(&chunk, lang.as_deref(), translate);
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

        let text = pieces.join(" ").trim().to_string();
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
            crate::history::record(&text, &text, &model, &file_name);
        }
        let _ = self.app.emit("yap-transcript", text.clone());
        let _ = self.app.emit(
            "yap-upload-done",
            serde_json::json!({ "text": text, "durationSecs": duration_secs, "file": file_name }),
        );
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

        let outcome = tokio::task::spawn_blocking(move || {
            let result = engine.transcribe(&audio, language.as_deref(), translate);
            (engine, result)
        })
        .await;

        done.store(true, Ordering::SeqCst);
        watchdog.abort();

        // Mark activity so the idle watcher counts from end-of-transcription.
        self.touch_activity();

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
                    Ok(text) if self.edit_mode.load(Ordering::SeqCst) => {
                        // Edit/rewrite mode: the transcript is an INSTRUCTION, not
                        // dictation. Apply it to the captured selection and paste
                        // the result back. Emits its own final state.
                        self.run_rewrite(text.trim().to_string()).await;
                        return;
                    }
                    // Wake-word Voice Agent (port of OpenWhispr's detectAgentName
                    // + dictation routing): a normal dictation that addresses the
                    // agent by name ("Hey {name}, …") routes the WHOLE transcript
                    // through the Voice-Agent scope in write mode instead of
                    // cleanup — the agent prompt strips the name + command from
                    // the output. Agent unreachable / no match → normal cleanup.
                    Ok(text) if self.wake_word_hit(text.trim()) => {
                        tracing::info!("Wake word detected — routing dictation to the Voice Agent");
                        self.run_agent(text.trim().to_string(), String::new()).await;
                        return;
                    }
                    Ok(text) => {
                        // Smart routing: which app were we dictating into? The
                        // foreground window was captured at record-start; resolve
                        // it to a process name (e.g. "slack.exe") so per-app
                        // cleanup rules can pick a matching body.
                        let target_hwnd = match self.target_hwnd.load(Ordering::Relaxed) {
                            0 => None,
                            h => Some(h),
                        };
                        let target_app = crate::text_injector::app_name_for(target_hwnd);

                        // Snapshot the injection- and cleanup-related config under
                        // one read lock. `pp_body` is the routed cleanup body:
                        // `Some(body)` to clean with, `None` to skip cleanup
                        // (selected-apps-only scope + no matching rule). The
                        // endpoint comes from the matched profile's LLM override
                        // when it has one, else the global settings — either way
                        // "ondevice" routes through the sidecar when it's up.
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
                            pp_provider,
                            pp_body,
                            pp_disable_thinking,
                        ) = self
                            .config
                            .read()
                            .map(|c| {
                                let plan = c.resolve_cleanup(target_app.as_deref());
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
                                    c.append_trailing_space,
                                    c.auto_submit,
                                    c.auto_submit_key.clone(),
                                    c.restore_clipboard,
                                    c.post_process_enabled,
                                    pp_base_url,
                                    pp_api_key,
                                    pp_model,
                                    pp_provider,
                                    plan.map(|p| p.body),
                                    c.pp_disable_thinking,
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
                                    None,
                                    false,
                                )
                            });

                        // Optional AI cleanup pass (best-effort). Runs before the
                        // dictionary so the user's corrections always have the
                        // final say. Any error/timeout falls back to the raw
                        // transcript — dictation is never blocked. The state stays
                        // `processing` for the extra latency.
                        let raw = text.trim().to_string();
                        // `pp_body` is `None` when smart routing says "skip cleanup
                        // for this app" (selected-apps-only scope, unbound app).
                        let cleaned = match (&pp_body, pp_enabled && !pp_base_url.is_empty() && !raw.is_empty())
                        {
                            (Some(body), true) => {
                                match crate::llm::cleanup(
                                    &raw,
                                    &pp_base_url,
                                    &pp_api_key,
                                    &pp_model,
                                    &pp_provider,
                                    body,
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
                                        raw.clone()
                                    }
                                }
                            }
                            _ => raw.clone(),
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

                            // Local-only history for the stats dashboard
                            // (best-effort, gated by `history_enabled`).
                            let (history_enabled, model) = self
                                .config
                                .read()
                                .map(|c| (c.history_enabled, c.model_size.clone()))
                                .unwrap_or((false, String::new()));
                            if history_enabled {
                                let app_name =
                                    crate::text_injector::app_name_for(target).unwrap_or_default();
                                crate::history::record(
                                    &raw,
                                    corrected.trim(),
                                    &model,
                                    &app_name,
                                );
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
            preroll: Mutex::new(VecDeque::with_capacity(PREROLL_SAMPLES + 1)),
            engine: std::sync::Arc::new(Mutex::new(engine)),
            app: app.clone(),
            config: RwLock::new(cfg.clone()),
            last_activity: AtomicU64::new(now_ms()),
            target_hwnd: AtomicIsize::new(0),
            edit_mode: AtomicBool::new(false),
            selection: Mutex::new(None),
            processing: AtomicBool::new(false),
            upload_cancel: AtomicBool::new(false),
            mic_test: AtomicBool::new(false),
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
        self.shared.toggle(false);
    }

    /// Route a **dictation** hotkey press/release through the recording mode.
    /// Called from the input-hook listeners for both press and release.
    pub fn on_key(&self, pressed: bool) {
        self.shared.on_key(pressed, false);
    }

    /// Route an **edit/rewrite** hotkey press/release. Same as `on_key` but the
    /// session captures the selection and rewrites it from the spoken instruction.
    pub fn on_edit_key(&self, pressed: bool) {
        self.shared.on_key(pressed, true);
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
        if let Ok(mut c) = self.shared.config.write() {
            *c = cfg;
        }
    }

    /// Enable/disable mic-test mode: while on, the audio callback emits
    /// `yap-amp` levels even when idle (onboarding's "see the waves react").
    pub fn set_mic_test(&self, on: bool) {
        self.shared.mic_test.store(on, Ordering::Relaxed);
    }

    /// Swap the capture stream to a different input device **live** (no app
    /// restart). Builds the new stream first, so on failure the old stream keeps
    /// running and an error is returned.
    pub fn set_input_device(&mut self, device: Option<&str>) -> Result<(), String> {
        let stream = build_input_stream(&self.shared, device)?;
        self._stream = SendStream(stream); // dropping the old stream stops it
        tracing::info!(device = device.unwrap_or("default"), "Input device switched");
        Ok(())
    }
}

/// Streaming-partials worker (opt-in). Runs on its own thread for one recording
/// session and exits when `recording` flips false.
///
/// Every `INTERVAL`, it re-transcribes the *whole* growing buffer on the warm
/// engine and emits a de-flickered partial (`yap-partial`). It never blocks the
/// authoritative final pass: it grabs the engine with `try_lock` and skips the
/// tick if the engine is busy or has been taken for the final transcription.
fn stream_partials(shared: Arc<Shared>, language: Option<String>, translate: bool) {
    const INTERVAL: Duration = Duration::from_millis(500);
    // Don't bother until there's at least ~0.5 s of audio (avoids hallucinated
    // output on tiny snippets — mirrors the engine's own MIN_SAMPLES guard).
    const STREAM_MIN_SAMPLES: usize = 8_000;

    let mut last = String::new();
    loop {
        std::thread::sleep(INTERVAL);
        if !shared.recording.load(Ordering::SeqCst) {
            break;
        }
        let buf = match shared.buffer.lock() {
            Ok(b) => b.clone(),
            Err(_) => break,
        };
        if buf.len() < STREAM_MIN_SAMPLES {
            continue;
        }

        // Re-entrancy/contention guard: only transcribe if the engine is free.
        let text = {
            let guard = match shared.engine.try_lock() {
                Ok(g) => g,
                Err(_) => continue, // a transcription is already running — skip
            };
            match guard.as_ref() {
                Some(engine) => {
                    if !shared.recording.load(Ordering::SeqCst) {
                        break;
                    }
                    match engine.transcribe(&buf, language.as_deref(), translate) {
                        Ok(t) => t,
                        Err(e) => {
                            tracing::debug!("partial transcribe skipped: {}", e);
                            continue;
                        }
                    }
                }
                None => continue, // engine taken for the final pass — wind down
            }
        };

        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        let stable = smart_diff(&last, trimmed);
        if stable != last {
            last = stable.clone();
            // Recording may have stopped while we were transcribing — don't emit a
            // stale partial over the authoritative final transcript.
            if !shared.recording.load(Ordering::SeqCst) {
                break;
            }
            let _ = shared.app.emit("yap-partial", stable);
        }
    }
}

/// De-flicker successive full-transcript partials (FluidVoice `smartDiffUpdate`).
///
/// Keeps the stable longest-common **word** prefix of the previous emit and
/// appends the new tail, so the displayed text grows smoothly instead of
/// re-rendering wholesale. If the new transcript diverges from the previous one
/// by more than half its words, it's replaced outright (the decode changed its
/// mind). Word comparison ignores case and surrounding punctuation.
fn smart_diff(prev: &str, next: &str) -> String {
    let norm = |w: &str| {
        w.trim_matches(|c: char| !c.is_alphanumeric())
            .to_ascii_lowercase()
    };
    let pw: Vec<&str> = prev.split_whitespace().collect();
    let nw: Vec<&str> = next.split_whitespace().collect();

    let mut common = 0;
    while common < pw.len() && common < nw.len() && norm(pw[common]) == norm(nw[common]) {
        common += 1;
    }

    if !pw.is_empty() && (common as f32 / pw.len() as f32) >= 0.5 {
        let mut out: Vec<&str> = pw[..common].to_vec();
        out.extend_from_slice(&nw[common..]);
        out.join(" ")
    } else {
        next.to_string()
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
                // While idle, keep a short rolling pre-roll ring (no buffering) so
                // the next recording starts a moment before the key. Amp is only
                // emitted while idle if a mic test (onboarding) asked for levels.
                if !cb_shared.recording.load(Ordering::Relaxed) {
                    if let Ok(mut pr) = cb_shared.preroll.lock() {
                        pr.extend(resampled.iter().copied());
                        while pr.len() > PREROLL_SAMPLES {
                            pr.pop_front();
                        }
                    }
                    if !cb_shared.mic_test.load(Ordering::Relaxed) {
                        return;
                    }
                } else if let Ok(mut buf) = cb_shared.buffer.lock() {
                    // Bound the buffer so a stuck key can't grow it without limit
                    // (OOM). Past the cap we drop further audio and warn once.
                    if buf.len() < MAX_RECORDING_SAMPLES {
                        buf.extend_from_slice(&resampled);
                        if buf.len() >= MAX_RECORDING_SAMPLES {
                            tracing::warn!(
                                "Recording hit the {}-minute cap — further audio dropped; stop the recording",
                                MAX_RECORDING_SAMPLES / (60 * TARGET_SAMPLE_RATE as usize)
                            );
                        }
                    }
                }
                // Accumulate the peak over ~30 ms, then emit it as the next
                // scrolling-waveform bar (recording, or an active mic test). The
                // frontend shapes (gain/curve) and scrolls it.
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

#[cfg(test)]
mod tests {
    use super::smart_diff;

    #[test]
    fn growing_transcript_keeps_prefix_and_appends() {
        let out = smart_diff("the meeting is", "the meeting is at three");
        assert_eq!(out, "the meeting is at three");
    }

    #[test]
    fn from_empty_takes_next() {
        assert_eq!(smart_diff("", "hello world"), "hello world");
    }

    #[test]
    fn case_and_punctuation_insensitive_prefix() {
        // Prev tail lacked punctuation; new decode added it — prefix still stable.
        let out = smart_diff("lets go to the", "Let's go to the bank.");
        assert!(out.ends_with("bank."));
    }

    #[test]
    fn large_divergence_replaces_wholesale() {
        let out = smart_diff("alpha beta gamma delta", "totally different words here");
        assert_eq!(out, "totally different words here");
    }
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
