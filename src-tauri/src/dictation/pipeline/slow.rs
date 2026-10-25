//! "Taking longer than usual…" (`dictation/slow.rs`) for the dictation in
//! hand: the watch beside its transcription and beside its AI cleanup, and
//! the bar's **Skip cleanup** (the words as transcribed, pasted now).

use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};

use crate::dictation::slow::{self, Notice, Stage};

use super::Shared;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Watch {
    Running,
    Slow,
    Done,
}

/// The watch on one stage: past its time it tells the bar (`yap-slow`
/// `{stage, skip}` + `yap-state` "processing-slow"); [`SlowWatch::finish`]
/// takes it back. Both under one lock, so the bar never hears "slow" after
/// "done".
pub(super) struct SlowWatch {
    state: Arc<Mutex<Watch>>,
    task: tokio::task::JoinHandle<()>,
    app: AppHandle,
}

impl SlowWatch {
    /// Watch a stage starting now: slow after `after_ms` (its usual time
    /// `usual`, for the log).
    pub(super) fn start(app: &AppHandle, notice: Notice, after_ms: u64, usual: Option<u64>) -> SlowWatch {
        let state = Arc::new(Mutex::new(Watch::Running));
        let (watched, bar) = (Arc::clone(&state), app.clone());
        let task = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(after_ms)).await;
            let mut s = watched.lock().unwrap_or_else(|p| p.into_inner());
            if *s == Watch::Running {
                *s = Watch::Slow;
                tracing::info!(stage = ?notice.stage, after_ms, usual_ms = ?usual, "Dictation: taking longer than usual");
                let _ = bar.emit(slow::EVENT, Some(notice));
                let _ = bar.emit("yap-state", "processing-slow");
            }
        });
        SlowWatch { state, task, app: app.clone() }
    }

    /// The stage is over: the bar goes back to plain "processing" if it
    /// was told it's slow. Returns whether it was.
    pub(super) fn finish(&self) -> bool {
        self.task.abort();
        let mut s = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let was = *s == Watch::Slow;
        *s = Watch::Done;
        if was {
            let _ = self.app.emit(slow::EVENT, None::<Notice>);
            let _ = self.app.emit("yap-state", "processing");
        }
        was
    }
}

impl Drop for SlowWatch {
    fn drop(&mut self) {
        self.task.abort();
        *self.state.lock().unwrap_or_else(|p| p.into_inner()) = Watch::Done;
    }
}

impl Shared {
    /// A dictation's AI cleanup (`call`, by `key`'s model: `slow::cleanup_key`)
    /// of `chars` characters, watched: past its usual time the bar says so
    /// and offers **Skip cleanup**. Skipped: `None` — the caller goes on
    /// with the words as transcribed (the request is dropped). A finished
    /// cleanup's time is kept for the next one's usual.
    pub(super) async fn watched_cleanup<F, T, E>(&self, call: F, key: &str, chars: u64) -> Option<Result<T, E>>
    where
        F: Future<Output = Result<T, E>>,
    {
        let (after, usual) = slow::cleanup_slow_after(key, chars);
        let watch = SlowWatch::start(&self.app, Notice { stage: Stage::Cleanup, skip: true }, after, usual);
        // Made before the call starts: a Skip from now on reaches it.
        let skip = self.skip_cleanup.notified();
        let started = Instant::now();
        let out = tokio::select! {
            r = call => Some(r),
            _ = skip => None,
        };
        let ms = started.elapsed().as_millis() as u64;
        watch.finish();
        match &out {
            Some(Ok(_)) => slow::record_cleanup(key, chars, ms),
            Some(Err(_)) => {}
            None => tracing::info!(ms, "AI cleanup skipped from the bar: pasting the words as transcribed"),
        }
        out
    }

    /// The bar's **Skip cleanup**: the cleanup running now (if any) gives
    /// way to the words as transcribed. Nothing waiting: nothing happens.
    pub(super) fn skip_cleanup_now(&self) {
        tracing::info!("Skip cleanup asked");
        self.skip_cleanup.notify_waiters();
    }
}
