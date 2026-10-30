//! The live preview while recording: a bounded sliding window of the
//! audio transcribed again every half second or so (`partials.rs`),
//! never in the final pass's way.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::Emitter;

use crate::config;
use crate::dictation::stt;

use super::engine::model_loading;
use super::Shared;

impl Shared {
    /// If streaming partials are enabled, spawn the worker thread for
    /// recording `session` (it exits on its own when that recording ends).
    pub(super) fn maybe_start_streaming(self: &Arc<Self>, session: u64) {
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
        std::thread::spawn(move || stream_partials(shared, session, language, translate));
    }

    /// Recording `session` is still the one recording.
    fn still_recording(&self, session: u64) -> bool {
        self.recording.load(Ordering::SeqCst) && self.rec_session.load(Ordering::SeqCst) == session
    }
}

/// Streaming-partials worker. Runs on its own thread for one recording
/// session and exits when that recording ends — not merely when `recording`
/// flips false: a cancel and a new press within one of its ticks used to
/// leave it running into the new recording beside the new one's worker,
/// showing the old dictation's words in the new one's preview.
///
/// Each tick it transcribes only a bounded **sliding window** of the buffer
/// (`partials::PartialSession`): audio older than the window is frozen as
/// committed text, so per-tick cost is independent of recording length. It
/// never blocks the authoritative final pass: it grabs the engine with
/// `try_lock` and skips the tick if the engine is busy or absent (taken by a
/// meeting chunk; the final pass only takes it after `recording` flips false).
fn stream_partials(shared: Arc<Shared>, recording: u64, language: Option<String>, translate: bool) {
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

    let mut session = crate::dictation::partials::PartialSession::default();
    let mut last_emitted = String::new();
    let mut interval = BASE_INTERVAL;
    loop {
        std::thread::sleep(interval);
        if !shared.still_recording(recording) {
            break;
        }

        // Snapshot this tick's window under the buffer lock — only the bounded
        // window is cloned, never the whole recording.
        let (win, rel_cut, abs_cut) = {
            let buf = match shared.buffer.lock() {
                Ok(b) => b,
                Err(_) => break,
            };
            let len = buf.len();
            if len.saturating_sub(session.window_start) < STREAM_MIN_SAMPLES {
                continue;
            }
            match session.plan(len, &buf) {
                crate::dictation::partials::TickPlan::Normal => {
                    (buf[session.window_start..len].to_vec(), None, 0usize)
                }
                crate::dictation::partials::TickPlan::Advance { cut } => (
                    buf[session.window_start..len].to_vec(),
                    Some(cut - session.window_start),
                    cut,
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
                    if !shared.still_recording(recording) {
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
        if !display.is_empty() && display != last_emitted {
            // Recording may have stopped while we were transcribing — don't emit
            // a stale partial over the authoritative final transcript.
            if !shared.still_recording(recording) {
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
    // Out with a meeting's chunk (or being loaded by another taker): it comes
    // back in a moment. Loading here would put a second copy of the model on
    // the graphics card — what `EngineCell` lends the engine in turns to
    // avoid — and the worker polls for it meanwhile anyway.
    if shared.engine.is_out() {
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
