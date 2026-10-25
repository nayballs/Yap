//! Speech to text: a dictation's transcription and where its text goes
//! next (`run_stt`), an Upload file in chunks, and Home's Run again.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use tauri::Emitter;

use crate::config;
use crate::dictation::stt::{self, SttError};

use super::engine::{lease_engine, model_loading, wait_for_model, EngineCell, DICTATION_ENGINE_WAIT, UPLOAD_ENGINE_WAIT};
use super::finish::{apply_corrections, as_said_text, finish_dictation};
use super::{Reran, Shared, DICTATION_ERROR};

/// Clears `capped` when a dictation's transcription is over, however it
/// ends.
struct ClearCapped(Arc<Shared>);

impl Drop for ClearCapped {
    fn drop(&mut self) {
        self.0.capped.store(false, Ordering::SeqCst);
    }
}

impl Shared {
    /// Upload / file transcription: decode → chunk → transcribe on the warm
    /// engine, with per-chunk `yap-upload-progress` events and a cancel flag.
    /// Runs with the `processing` guard held (set by `Pipeline::transcribe_file`)
    /// so a live dictation can't grab the engine mid-run. Result arrives via
    /// `yap-upload-done` / `-error` / `-cancelled` events.
    pub(super) async fn run_file_transcription(self: Arc<Self>, path: String) {
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
            tokio::task::spawn_blocking(move || crate::dictation::media::decode_to_16k_mono(&decode_path))
                .await;
        let samples = match decoded {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => return emit_err(e),
            Err(e) => return emit_err(format!("Decode task failed: {e}")),
        };
        let duration_secs = samples.len() as f32 / 16_000.0;

        // 2. The warm engine, taken out for one chunk at a time (a
        // dictation or a meeting's chunk takes its turn in between), or
        // lazily reloaded (`lease_engine`).
        let (language, translate) = self.language_settings();

        // 3. ~60 s chunks, each cut at the quietest point of its last 5 s so a
        //    boundary doesn't slice through a word (media::chunk_ranges).
        let ranges = crate::dictation::media::chunk_ranges(samples.len(), 16_000, 60, 5, &samples);
        let total = ranges.len();
        let mut pieces: Vec<String> = Vec::with_capacity(total);

        for (i, (a, b)) in ranges.into_iter().enumerate() {
            if self.upload_cancel.load(Ordering::SeqCst) {
                tracing::info!("Upload transcription cancelled");
                let _ = self.app.emit("yap-upload-cancelled", ());
                return;
            }
            let lease = match lease_engine(&self.engine, UPLOAD_ENGINE_WAIT).await {
                Ok(lease) => lease,
                Err(e) => return emit_err(format!("No STT model available: {e}")),
            };
            let chunk: Vec<f32> = samples[a..b].to_vec();
            let lang = language.clone();
            let prompt = self.dictionary_prompt();
            let outcome = tokio::task::spawn_blocking(move || {
                let result = match lease.engine() {
                    Some(engine) => engine.transcribe(&chunk, lang.as_deref(), translate, prompt.as_deref()),
                    None => Err(SttError::NotReady),
                };
                (lease, result)
            })
            .await;
            match outcome {
                Ok((lease, Ok(text))) => {
                    drop(lease);
                    let t = text.trim().to_string();
                    if !t.is_empty() {
                        pieces.push(t);
                    }
                }
                Ok((lease, Err(err))) => {
                    drop(lease);
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
            // A test run's pace (`e2e_upload_pace`): the job still going
            // when a spec comes back to the view.
            #[cfg(debug_assertions)]
            if let Some(pace) = crate::shell::e2e::upload_pace() {
                tokio::time::sleep(pace).await;
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
            crate::dictation::history::record(&raw, &text, &model, &file_name);
            let _ = self.app.emit("yap-history-changed", ());
        }
        let _ = self.app.emit("yap-transcript", text.clone());
        let _ = self.app.emit(
            "yap-upload-done",
            serde_json::json!({ "text": text, "durationSecs": duration_secs, "file": file_name }),
        );
    }
    pub(super) async fn run_stt(self: Arc<Self>, audio: Vec<f32>) {
        // A dictation stopped at its length limit is this one only, however
        // it ends (`cap.rs`).
        let _capped = ClearCapped(Arc::clone(&self));
        if audio.is_empty() {
            if self.assistant.swap(false, Ordering::SeqCst) {
                crate::chat::assistant::nothing(&self.app);
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
            let id = self.config.read().ok().and_then(|c| crate::dictation::recent_audio::keep(&audio, &c));
            if let Ok(mut a) = self.audio_id.lock() {
                *a = id;
            }
        }

        // Started before the model finished loading (`Pipeline::start` loads
        // it in the background): wait for it — the bar says "Loading the
        // speech model…" — instead of failing or loading a second copy.
        if model_loading() && self.engine.lock().map(|g| g.is_none()).unwrap_or(false) {
            tracing::info!("Dictation: waiting for the speech model to finish loading");
            let _ = self.app.emit("yap-model-loading", true);
            let _ = tokio::task::spawn_blocking(|| wait_for_model(Duration::from_secs(180))).await;
            let _ = self.app.emit("yap-model-loading", false);
        }
        // Take the engine out (the slot isn't held across the await). A
        // meeting's chunk that has it gives it back first — a second or so
        // — rather than this loading a second copy of the model.
        let slot = Arc::clone(&self.engine);
        let mut lease = match tokio::task::spawn_blocking(move || slot.take(DICTATION_ENGINE_WAIT)).await {
            Ok(lease) => lease,
            Err(e) => {
                tracing::warn!("Dictation: the speech engine couldn't be taken: {}", e);
                self.list_failed("The speech engine wasn't available");
                let _ = self.app.emit("yap-state", "idle");
                return;
            }
        };
        match lease.engine() {
            Some(_) => {}
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
                        lease.put(e);
                    }
                    Err(e) => {
                        tracing::warn!("No STT engine (model missing) — cannot transcribe: {}", e);
                        drop(lease);
                        if self.assistant.swap(false, Ordering::SeqCst) {
                            crate::chat::assistant::nothing(&self.app);
                        }
                        // Listed with its audio: run it again once a model is in.
                        self.list_failed("No speech model was ready");
                        let _ = self.app.emit("yap-state", "needs-model");
                        return;
                    }
                }
            }
        }

        // Snapshot the language/translate settings for this transcription.
        let (language, translate) = self.language_settings();

        // "Taking longer than usual…" (`slow.rs`): past this PC's usual
        // time for this much audio on this model and device, the bar says
        // so (the blocking call can't be cancelled without losing the words,
        // so it's only said). Taken back the moment it's done.
        let audio_ms = audio.len() as u64 * 1000 / super::TARGET_SAMPLE_RATE as u64;
        let (model, device) = lease
            .engine()
            .map(|e| (e.model().to_string(), e.device()))
            .unwrap_or_else(|| (String::new(), stt::Device::Gpu));
        let (after_ms, usual_ms) = crate::dictation::slow::stt_slow_after(&model, device, audio_ms);
        let watch = super::slow::SlowWatch::start(
            &self.app,
            crate::dictation::slow::Notice { stage: crate::dictation::slow::Stage::Transcribing, skip: false },
            after_ms,
            usual_ms,
        );

        let dict_prompt = self.dictation_prompt();
        let outcome = tokio::task::spawn_blocking(move || {
            // A test run's slow transcription (`e2e_dictation_timing`).
            #[cfg(debug_assertions)]
            if let Some(wait) = crate::dictation::slow::test_stt_delay() {
                std::thread::sleep(wait);
            }
            let started = std::time::Instant::now();
            let result = match lease.engine() {
                Some(engine) => engine.transcribe(&audio, language.as_deref(), translate, dict_prompt.as_deref()),
                None => Err(SttError::NotReady),
            };
            (lease, result, started.elapsed().as_millis() as u64)
        })
        .await;

        if watch.finish() {
            tracing::info!(model = %model, ?device, audio_ms, usual_ms = ?usual_ms, "Transcription took longer than usual");
        }
        let outcome = outcome.map(|(lease, result, took_ms)| {
            if result.is_ok() {
                crate::dictation::slow::record_stt(&crate::dictation::slow::stt_key(&model, device), audio_ms, took_ms);
            }
            (lease, result)
        });

        // Mark activity so the idle watcher counts from end-of-transcription.
        self.touch_activity();

        // The bar's "Cleaned up · Use what I said", for a dictation whose
        // words AI cleanup changed (`as_said.rs`).
        let mut offer: Option<u64> = None;
        match outcome {
            Ok((lease, transcription)) => {
                // The (warm) engine back for next time, now — not after the
                // cleanup below — unless a model switch (`set_engine`)
                // installed a newer one meanwhile: then that stays
                // (`EngineCell::give_back`).
                drop(lease);
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
                        self.run_agent(text.trim().to_string(), String::new(), None).await;
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
            crate::chat::assistant::nothing(&self.app);
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
    /// **Run again** (Home's feed, `recent_audio::history_rerun`): a kept
    /// dictation's `audio` transcribed afresh — on `model` (an installed
    /// one; `None` = the one in use: the warm engine, else loaded for this
    /// and dropped) — then AI cleanup as for its app, the corrections and
    /// snippets. Nothing is pasted. Returns the transcript, the text, the
    /// words as said when cleanup changed them, and the model id.
    pub(super) async fn rerun(self: &Arc<Self>, audio: Vec<f32>, model: Option<String>, app_name: &str) -> Result<Reran, String> {
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
        // The warm engine (taken out, given back), or the other model in a
        // slot of its own, dropped after.
        let slot = if warm { Arc::clone(&self.engine) } else { EngineCell::for_model(&model_id, None) };
        let lease = lease_engine(&slot, DICTATION_ENGINE_WAIT)
            .await
            .map_err(|e| format!("Couldn't load {}: {e}", stt::model_name(&model_id)))?;
        let (language, translate) = self.language_settings();
        let prompt = if stt::is_whisper_model(&model_id) { self.dictionary_prompt() } else { None };
        tracing::info!(samples = audio.len(), model = %model_id, warm, "Running a dictation again");
        let (lease, result) = tokio::task::spawn_blocking(move || {
            let r = match lease.engine() {
                Some(engine) => engine.transcribe(&audio, language.as_deref(), translate, prompt.as_deref()),
                None => Err(SttError::NotReady),
            };
            (lease, r)
        })
        .await
        .map_err(|e| format!("The transcription crashed: {e}"))?;
        drop(lease);
        self.touch_activity();
        let raw = result.map_err(|e| format!("The transcription failed: {e}"))?.trim().to_string();
        if raw.is_empty() {
            return Err("Nothing was heard in it".into());
        }
        let (cleaned, _) = self.clean_up(&raw, &crate::ai::cleanup_styles::Target::of_app(app_name), None, false).await;
        let mut c = cfg;
        c.model_size = model_id.clone();
        let (text, _) = finish_dictation(cleaned.trim(), &raw, &c);
        let text = text.trim().to_string();
        if text.is_empty() {
            return Err("Nothing was left after the cleanup".into());
        }
        // The words as said, when cleanup changed them (Home's "AI edited").
        let said = (cleaned.trim() != raw).then(|| as_said_text(&raw, &c, None)).filter(|s| s.trim() != text);
        Ok(Reran { raw, text, said, model: model_id })
    }
}
