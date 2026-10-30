//! The warm speech engine the pipeline shares: one copy of the model,
//! taken out for one transcription at a time by a dictation, a meeting's
//! chunk, Upload or Run again ([`EngineCell::take`]), and the startup load
//! they wait for instead of loading a second copy.

use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::config;
use crate::dictation::stt::{self, SttAdapter};

/// The shared warm-engine slot. The pipeline owns it; the meeting recorder
/// (`meeting.rs`), Upload, Run again and a meeting transcribed again take
/// the engine out for one transcription at a time ([`EngineCell::take`])
/// and give it back, so they and dictation take turns on one copy of the
/// model instead of loading a second.
pub(crate) type EngineSlot = std::sync::Arc<EngineCell>;

/// The warm engine, and who has it out.
///
/// Until 2026-10-24 each taker took the engine out of a bare
/// `Mutex<Option<…>>`; whoever found it empty meanwhile (a dictation that
/// ended during a meeting's chunk) loaded a second copy of the model —
/// seconds, and its memory again. Now a taker that finds it out **waits**
/// for it ([`EngineCell::take`]), and one that finds nobody has it (never
/// loaded, unloaded when idle, dropped after an error) loads it *for*
/// everyone: the others wait for that load too. The lease ([`Lease`]) gives
/// it back when dropped, even when its transcription panicked (then
/// without the engine: a fresh one is loaded next).
#[derive(Default)]
pub(crate) struct EngineCell {
    engine: Mutex<Option<SttAdapter>>,
    /// Leases out now (each holding the engine, or loading one).
    out: Mutex<usize>,
    back: Condvar,
    /// The model a taker loads into an empty slot: `None` = Settings' (the
    /// warm engine's); a refine's own slot names its model.
    model: Option<String>,
}

impl EngineCell {
    /// A slot of its own for `model` (a meeting refined on a model other
    /// than the one in use), with that model's engine in it if loaded.
    pub(crate) fn for_model(model: &str, engine: Option<SttAdapter>) -> EngineSlot {
        std::sync::Arc::new(EngineCell { engine: Mutex::new(engine), model: Some(model.to_string()), ..Default::default() })
    }

    /// The slot itself (put an engine in, look at it, unload it). Never held
    /// across a transcription, but by the live preview (`try_lock`).
    pub(crate) fn lock(&self) -> std::sync::LockResult<std::sync::MutexGuard<'_, Option<SttAdapter>>> {
        self.engine.lock()
    }

    pub(crate) fn try_lock(&self) -> std::sync::TryLockResult<std::sync::MutexGuard<'_, Option<SttAdapter>>> {
        self.engine.try_lock()
    }

    /// The model a taker loads when the slot is empty (`None`: Settings').
    pub(crate) fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }

    /// Take the engine out to transcribe with (blocking): while another
    /// taker has it out — or is loading it — wait up to `wait` for it to
    /// come back. The lease holds the engine, or none when nobody had one
    /// (or the wait ran out): load one into it ([`Lease::put`]) and it
    /// becomes the warm engine when the lease gives it back.
    pub(crate) fn take(self: &std::sync::Arc<Self>, wait: Duration) -> Lease {
        let deadline = Instant::now() + wait;
        let mut out = self.out.lock().unwrap_or_else(|p| p.into_inner());
        let mut waited: Option<Instant> = None;
        loop {
            let engine = self.engine.lock().unwrap_or_else(|p| p.into_inner()).take();
            let left = deadline.saturating_duration_since(Instant::now());
            if engine.is_some() || *out == 0 || left.is_zero() {
                *out += 1;
                if let Some(since) = waited {
                    tracing::info!(ms = since.elapsed().as_millis() as u64, got = engine.is_some(), "Speech engine: waited for it to come back");
                }
                return Lease { slot: std::sync::Arc::clone(self), engine };
            }
            if waited.is_none() {
                waited = Some(Instant::now());
                tracing::info!("Speech engine: in use; waiting for it to come back");
            }
            out = self.back.wait_timeout(out, left).unwrap_or_else(|p| p.into_inner()).0;
        }
    }

    /// A lease ends: its engine back in the slot (unless a model switch put
    /// a newer one there meanwhile: then it's dropped), and the next taker
    /// woken.
    fn give_back(&self, engine: Option<SttAdapter>) {
        let mut out = self.out.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(engine) = engine {
            let mut slot = self.engine.lock().unwrap_or_else(|p| p.into_inner());
            if slot.is_none() {
                *slot = Some(engine);
            }
        }
        *out = out.saturating_sub(1);
        drop(out);
        self.back.notify_all();
    }

    /// Leases out now (tests).
    #[cfg(test)]
    fn out(&self) -> usize {
        *self.out.lock().unwrap()
    }
}

/// The engine taken out of its slot ([`EngineCell::take`]); back when dropped.
pub(crate) struct Lease {
    slot: EngineSlot,
    engine: Option<SttAdapter>,
}

impl Lease {
    /// The engine, if the slot had one (or one was put in).
    pub(crate) fn engine(&self) -> Option<&SttAdapter> {
        self.engine.as_ref()
    }

    /// A freshly loaded engine: the warm one once given back.
    pub(crate) fn put(&mut self, engine: SttAdapter) {
        self.engine = Some(engine);
    }

    /// The engine failed: it isn't given back, so the next taker loads a
    /// fresh one (a graphics card lost after sleep, a driver reset).
    pub(crate) fn discard(&mut self) {
        self.engine = None;
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        // Dropped by a panic in its transcription: not that engine again.
        let engine = if std::thread::panicking() { None } else { self.engine.take() };
        self.slot.give_back(engine);
    }
}

/// How long an Upload's chunk waits for the warm engine (see
/// [`DICTATION_ENGINE_WAIT`]).
pub(super) const UPLOAD_ENGINE_WAIT: Duration = Duration::from_secs(60);

/// The engine of `slot` for one transcription, waiting up to `wait` while
/// another taker has it ([`EngineCell::take`]); with nobody holding one, the
/// slot's model (`EngineCell::model`, else Settings') is loaded into the
/// lease — the warm engine once it's given back. Never while the startup
/// load is still going: that one is waited for.
pub(crate) async fn lease_engine(slot: &EngineSlot, wait: Duration) -> Result<Lease, String> {
    if slot.model().is_none() && model_loading() && slot.lock().map(|g| g.is_none()).unwrap_or(false) {
        let _ = tokio::task::spawn_blocking(|| wait_for_model(Duration::from_secs(180))).await;
    }
    let taking = Arc::clone(slot);
    let mut lease = tokio::task::spawn_blocking(move || taking.take(wait))
        .await
        .map_err(|e| format!("The speech engine couldn't be taken: {e}"))?;
    if lease.engine().is_none() {
        let cfg = config::load();
        let model = slot.model().map_or_else(|| cfg.model_size.clone(), str::to_string);
        let use_gpu = cfg.use_gpu;
        let (loaded, back) = tokio::task::spawn_blocking(move || {
            let loaded = stt::create_stt_engine(&config::data_dir(), &model, use_gpu);
            (loaded, lease)
        })
        .await
        .map_err(|e| format!("The speech model's load crashed: {e}"))?;
        lease = back;
        let engine = loaded.map_err(|e| e.to_string())?;
        tracing::info!(model = %engine.model(), "Speech model loaded (none was warm)");
        lease.put(engine);
    }
    Ok(lease)
}

/// How long a dictation waits for the warm engine while another taker has
/// it out (a meeting's chunk: a second or so on a graphics card) before it
/// loads a copy of its own after all.
pub(super) const DICTATION_ENGINE_WAIT: Duration = Duration::from_secs(20);

/// The speech model loading in the background at startup (`Pipeline::start`,
/// off the startup thread so the tray, the bar and the window come first).
/// A dictation, a meeting chunk or a Run again that needs the engine
/// meanwhile waits for it ([`wait_for_model`]) instead of loading a second
/// copy.
static MODEL_LOADING: Mutex<bool> = Mutex::new(false);
static MODEL_LOADED: Condvar = Condvar::new();

/// The startup load is still running.
pub(crate) fn model_loading() -> bool {
    *MODEL_LOADING.lock().unwrap_or_else(|p| p.into_inner())
}

pub(super) fn set_model_loading(on: bool) {
    *MODEL_LOADING.lock().unwrap_or_else(|p| p.into_inner()) = on;
    if !on {
        MODEL_LOADED.notify_all();
    }
}

/// Wait (blocking) up to `max` for the startup load to finish; `true` when
/// it has (or none was running).
pub(crate) fn wait_for_model(max: Duration) -> bool {
    let guard = MODEL_LOADING.lock().unwrap_or_else(|p| p.into_inner());
    let (guard, _) = MODEL_LOADED
        .wait_timeout_while(guard, max, |loading| *loading)
        .unwrap_or_else(|p| p.into_inner());
    !*guard
}

#[cfg(test)]
mod tests {
    #[cfg(not(feature = "engines"))]
    use super::*;

    // ---- the shared engine (EngineCell) ----

    /// A stub engine (the default build loads nothing from disk).
    #[cfg(not(feature = "engines"))]
    fn engine() -> SttAdapter {
        stt::create_stt_engine_on(std::path::Path::new("."), "parakeet-tdt-0.6b-v3", stt::Device::Cpu).unwrap()
    }

    /// A meeting's chunk has the engine out while a dictation ends: the
    /// dictation waits for it instead of loading a second copy of the
    /// model (until 2026-10-24 it found the slot empty and loaded one).
    #[cfg(not(feature = "engines"))]
    #[test]
    fn a_dictation_waits_for_the_engine_a_meeting_chunk_has_out() {
        let slot = EngineSlot::default();
        *slot.lock().unwrap() = Some(engine());
        let chunk = slot.take(Duration::ZERO);
        assert!(chunk.engine().is_some());
        assert!(slot.lock().unwrap().is_none(), "out of the slot while it transcribes");
        let (tx, rx) = std::sync::mpsc::channel();
        // Timed from here, not from inside the thread: under load the thread
        // can start late, and its own clock then showed less than the wait.
        let started = Instant::now();
        let dictation = {
            let slot = Arc::clone(&slot);
            std::thread::spawn(move || {
                let lease = slot.take(Duration::from_secs(5));
                tx.send(()).unwrap();
                (lease.engine().is_some(), started.elapsed())
            })
        };
        // The chunk transcribes for a while; the dictation hasn't got it.
        assert!(rx.recv_timeout(Duration::from_millis(150)).is_err(), "the dictation waits");
        drop(chunk);
        let (got, waited) = dictation.join().unwrap();
        assert!(got, "the dictation got the warm engine, no second copy");
        assert!(waited >= Duration::from_millis(150));
        assert!(slot.lock().unwrap().is_some(), "back in the slot");
        assert_eq!(slot.out(), 0);
    }

    /// Nobody has it (unloaded when idle): the first taker loads it for
    /// everyone — the others wait for that load, then share it.
    #[cfg(not(feature = "engines"))]
    #[test]
    fn one_taker_loads_the_model_and_the_others_wait_for_it() {
        let slot = EngineSlot::default();
        let mut loading = slot.take(Duration::ZERO);
        assert!(loading.engine().is_none(), "nobody had one: this taker loads it");
        let waiter = {
            let slot = Arc::clone(&slot);
            std::thread::spawn(move || slot.take(Duration::from_secs(5)).engine().is_some())
        };
        std::thread::sleep(Duration::from_millis(100));
        loading.put(engine());
        drop(loading);
        assert!(waiter.join().unwrap(), "the waiter got the one that was loaded");
        // A load that fails lets the waiters go at once: each tries its own.
        let mut gone = slot.take(Duration::ZERO);
        gone.discard();
        drop(gone);
        let failed = slot.take(Duration::ZERO);
        assert!(failed.engine().is_none());
        let waiter = {
            let slot = Arc::clone(&slot);
            std::thread::spawn(move || {
                let started = Instant::now();
                (slot.take(Duration::from_secs(5)).engine().is_some(), started.elapsed())
            })
        };
        std::thread::sleep(Duration::from_millis(50));
        drop(failed);
        let (got, waited) = waiter.join().unwrap();
        assert!(!got, "nothing to share: it loads its own");
        assert!(waited < Duration::from_secs(2), "woken by the failed load, not the timeout");
        assert_eq!(slot.out(), 0);
    }

    /// An engine that failed (or whose transcription panicked) isn't given
    /// back: the next taker loads a fresh one. A model switch meanwhile wins.
    #[cfg(not(feature = "engines"))]
    #[test]
    fn a_failed_engine_is_dropped_and_a_newer_one_wins() {
        let slot = EngineSlot::default();
        *slot.lock().unwrap() = Some(engine());
        let mut lease = slot.take(Duration::ZERO);
        lease.discard();
        drop(lease);
        assert!(slot.lock().unwrap().is_none());
        assert_eq!(slot.out(), 0);
        // A panic while it has the engine.
        *slot.lock().unwrap() = Some(engine());
        let crashing = Arc::clone(&slot);
        let crashed = std::thread::spawn(move || {
            let _lease = crashing.take(Duration::ZERO);
            panic!("a crash in the engine");
        })
        .join();
        assert!(crashed.is_err());
        assert!(slot.lock().unwrap().is_none(), "not that engine again");
        assert_eq!(slot.out(), 0, "and nobody waits for it");
        // A model switch while one was out: the newer engine stays.
        *slot.lock().unwrap() = Some(engine());
        let old = slot.take(Duration::ZERO);
        *slot.lock().unwrap() = Some(stt::create_stt_engine_on(std::path::Path::new("."), "turbo", stt::Device::Cpu).unwrap());
        drop(old);
        assert_eq!(slot.lock().unwrap().as_ref().map(|e| e.model().to_string()).as_deref(), Some("turbo"));
    }
}
