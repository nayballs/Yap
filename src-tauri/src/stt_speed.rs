//! **The fastest speech engine on this PC** (2026-10-22): which device each
//! installed speech model runs on — the graphics card (the ONNX models,
//! Parakeet first, through DirectML; Whisper through Vulkan) or the
//! processor — measured once per model and PC instead of assumed. On the
//! maintainer's PC the int8 Parakeet ran ~3× faster on the processor than
//! through DirectML; a laptop may well differ, so each PC decides.
//!
//! **The benchmark** ([`run`]): a short bundled speech clip ([`CLIP`], made
//! with Kokoro — Apache-2.0 — reading [`CLIP_TEXT`]) transcribed on each
//! device: its load and first transcription timed (what the first
//! dictation after a load pays), then [`TIMED_RUNS`] rounds, the devices
//! taking turns (other work on the PC slows each alike), each round a part
//! of the clip of a new length ([`RUN_LENGTHS`]: a real dictation never
//! repeats one, and DirectML pays for each new one), and the median kept
//! (a warm dictation's latency). A device whose first transcription
//! took [`SLOW_FACTOR`]× the fastest's (and over [`SLOW_FLOOR_MS`]) isn't
//! timed further: it can't win, and Whisper on the processor holds it for
//! seconds. **The pick** ([`pick`], pure): the fastest whose words match the
//! processor's (the reference implementation: "still correct"); the
//! graphics card wins a near-tie ([`TIE`]), leaving the processor to the
//! person's apps. A device that failed to load or run is out.
//!
//! **When**: once per model and PC — on Yap's start ([`AFTER_START`]) and
//! when a model is installed or switched to ([`AFTER_CHANGE`]) if the
//! stored result doesn't fit any more (the model's files changed: a new
//! download; the graphics driver changed: [`pc_print`] has each adapter's
//! driver version), and on demand (Settings' "Measure again"). Never during
//! a dictation (or an Upload, or the moments after one: [`QUIET_MS`]), a
//! meeting recording or a model download: it waits for a quiet moment, and
//! one that starts mid-way stops it (tried again [`RETRY_AFTER_INTERRUPT`]
//! later).
//!
//! **Where it's kept**: `<data>/stt_speed.json` (`Measured` per model id),
//! never config.json (Settings' wholesale saves). The numbers go to the log
//! at `info` (milliseconds and whether the words matched: never the text).
//!
//! **Using it** ([`device_for`], every engine load through
//! `stt::create_stt_engine`): Settings → Speech-to-Text → Runs on —
//! "auto" (the measured pick, the graphics card until it's measured), "gpu"
//! or "cpu" (`config.stt_device`); the old GPU switch off = the processor.
//! A new pick, or a new choice, reloads the warm engine on that device at
//! the next quiet moment ([`reload_if_needed`]).
//!
//! Test runs (`e2e.rs`): never on their own; the Measure button (and
//! `stt_speed_measure`) runs on the stub engine, with debug-only
//! `YAP_E2E_STT_SPEED` ("gpu=450,cpu=150") standing in for each device's
//! cost.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::stt::{self, Device, SttAdapter};

/// The clip every model is timed on: 16 kHz mono 16-bit PCM, ~7 s of
/// Kokoro's af_bella (Apache-2.0) reading [`CLIP_TEXT`] (made by the
/// `--ignored` `make_clips` test below).
const CLIP: &[u8] = include_bytes!("../assets/stt-speed-clip.wav");
/// What the clip says (fictional people).
#[allow(dead_code)] // (only the tests read it: `make_clips` says it)
pub const CLIP_TEXT: &str =
    "Hi Imogen, could you send Sam the updated launch plan before Friday? I'd like to go through it together on Monday morning.";

const STORE_FILE: &str = "stt_speed.json";
/// Timed runs per device after its first (the median is kept).
pub const TIMED_RUNS: usize = 3;
/// Each timed run's length, as a share of the clip: a length the engine
/// hasn't had before, as every real dictation is. (DirectML plans its graph
/// again for each new length: on the maintainer's PC Parakeet ran the same
/// 8-second clip again in ~240 ms through it, but ~475 ms at a new length —
/// a benchmark repeating one clip would pick it wrongly.)
pub const RUN_LENGTHS: [f32; TIMED_RUNS] = [0.62, 0.81, 0.93];
/// How long after Yap starts the first measurement may begin.
pub const AFTER_START: Duration = Duration::from_secs(60);
/// …and after a model is installed or switched to.
pub const AFTER_CHANGE: Duration = Duration::from_secs(15);
/// How long the pipeline must have been quiet (no recording, transcribing,
/// or a dictation in the last moments) before a measurement starts.
const QUIET_MS: u64 = 20_000;
/// How often a waiting measurement looks again.
const IDLE_POLL: Duration = Duration::from_secs(5);
/// [`IDLE_POLL`], or a quarter of a second in a test run.
fn idle_poll() -> Duration {
    if crate::e2e::active() {
        Duration::from_millis(250)
    } else {
        IDLE_POLL
    }
}
/// A measurement that never finds a quiet moment gives up (the next start
/// or model change tries again).
const GIVE_UP_WAITING: Duration = Duration::from_secs(30 * 60);
/// A measurement a dictation interrupted is tried again this much later.
const RETRY_AFTER_INTERRUPT: Duration = Duration::from_secs(120);
/// A device whose first transcription took this many times the fastest
/// one's…
pub const SLOW_FACTOR: u64 = 5;
/// …and longer than this isn't timed further.
pub const SLOW_FLOOR_MS: u64 = 3_000;
/// Within this fraction of the fastest, the graphics card wins.
pub const TIE: f64 = 0.05;
/// The status event (`Status`).
pub const EVENT: &str = "yap-stt-speed";

// ---- the record -------------------------------------------------------------------

/// One device's timings for a model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub device: Device,
    /// Loading the model on it.
    pub load_ms: u64,
    /// Its first transcription after loading.
    pub first_ms: u64,
    /// The median of its timed runs (`None`: not timed — failed, or too
    /// slow to win).
    pub warm_ms: Option<u64>,
    #[serde(default)]
    pub runs_ms: Vec<u64>,
    /// Its words matched the reference's (the processor's).
    pub agrees: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Not tried: Whisper on the processor, beside a graphics card that ran
    /// it quickly ([`skip_cpu_whisper`]).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub skipped: bool,
}

/// A model measured on this PC.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Measured {
    /// The model's files when measured (`stt::model_fingerprint`).
    pub model_print: String,
    /// The PC's graphics adapters and drivers when measured ([`pc_print`]).
    pub pc_print: String,
    /// When (unix seconds).
    pub at: u64,
    /// How long the clip is.
    pub clip_ms: u64,
    pub runs: Vec<Run>,
    pub pick: Device,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Store {
    #[serde(default)]
    models: BTreeMap<String, Measured>,
}

// ---- the pure parts ---------------------------------------------------------------

/// A transcript's words, as compared: lower case, letters and digits.
fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '\'')
        .map(|w| w.trim_matches('\'').to_lowercase())
        .filter(|w| !w.is_empty())
        .collect()
}

/// Whether `text` says what `reference` says (case and punctuation aside);
/// with no reference, whether it says anything at all.
pub fn agrees(reference: Option<&str>, text: &str) -> bool {
    match reference {
        Some(r) => words(r) == words(text),
        None => !words(text).is_empty(),
    }
}

/// The median of some timings (the upper one of an even count).
pub fn median(ms: &[u64]) -> Option<u64> {
    let mut v = ms.to_vec();
    v.sort_unstable();
    v.get(v.len() / 2).copied()
}

/// Which devices aren't worth timing further: a first transcription
/// [`SLOW_FACTOR`]× the fastest's and over [`SLOW_FLOOR_MS`].
pub fn too_slow(first_ms: u64, fastest_first_ms: u64) -> bool {
    first_ms > SLOW_FLOOR_MS && first_ms > fastest_first_ms.saturating_mul(SLOW_FACTOR)
}

/// Whisper on the processor isn't tried when the graphics card's first
/// transcription took no more than [`SLOW_FLOOR_MS`]: whisper.cpp on the
/// processor (4 threads) needs far longer for its 30-second window — on the
/// maintainer's PC large-v3 took minutes against Vulkan's 2.5 s, holding
/// the processor all that time. A slow graphics card (an old integrated
/// one) still has it tried.
pub fn skip_cpu_whisper(gpu_first_ms: u64) -> bool {
    gpu_first_ms <= SLOW_FLOOR_MS
}

/// The device to run on: the fastest that ran, was timed and got the words
/// right; the graphics card when it's within [`TIE`] of that. `None` when
/// none qualifies.
pub fn pick(runs: &[Run]) -> Option<Device> {
    let ok: Vec<(&Run, u64)> = runs
        .iter()
        .filter(|r| r.error.is_none() && r.agrees)
        .filter_map(|r| r.warm_ms.map(|ms| (r, ms)))
        .collect();
    let fastest = ok.iter().map(|(_, ms)| *ms).min()?;
    if let Some((gpu, ms)) = ok.iter().find(|(r, _)| r.device == Device::Gpu) {
        if *ms as f64 <= fastest as f64 * (1.0 + TIE) {
            return Some(gpu.device);
        }
    }
    ok.iter().min_by_key(|(_, ms)| *ms).map(|(r, _)| r.device)
}

/// Where a model runs, from Settings' "Runs on" (`setting`: "auto" | "gpu" |
/// "cpu"), the old GPU switch and the measured pick (graphics card until
/// measured, as before).
pub fn device_choice(setting: &str, use_gpu: bool, measured: Option<Device>) -> Device {
    match setting {
        "cpu" => Device::Cpu,
        "gpu" => Device::Gpu,
        _ if !use_gpu => Device::Cpu,
        _ => measured.unwrap_or(Device::Gpu),
    }
}

/// Whether a stored measurement still fits: the same model files on the
/// same graphics hardware and drivers.
fn fits(m: &Measured, model_print: &str, pc_print: &str) -> bool {
    m.model_print == model_print && m.pc_print == pc_print
}

// ---- state ------------------------------------------------------------------------

struct Inner {
    store: Option<Store>,
    /// Settings' "Runs on" and the GPU switch (`sync`).
    setting: String,
    use_gpu: bool,
    /// The model being measured now.
    running: Option<String>,
    /// A measurement is waiting to start (`schedule`).
    waiting: bool,
    /// Why the last measurement didn't finish, for Settings.
    error: Option<String>,
}

static STATE: Mutex<Inner> = Mutex::new(Inner {
    store: None,
    setting: String::new(),
    use_gpu: true,
    running: None,
    waiting: false,
    error: None,
});

/// Never held while loading a model, transcribing, emitting or taking
/// another module's lock.
fn lock() -> MutexGuard<'static, Inner> {
    STATE.lock().unwrap_or_else(|p| p.into_inner())
}

fn store_path() -> PathBuf {
    crate::config::data_dir().join(STORE_FILE)
}

fn load_store() -> Store {
    // Unit tests never read or write the real data folder.
    if cfg!(test) {
        return Store::default();
    }
    let path = store_path();
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_else(|e| {
            tracing::warn!("stt_speed.json failed to parse ({e}); measuring again");
            crate::config::quarantine_corrupt(&path);
            Store::default()
        }),
        Err(_) => Store::default(),
    }
}

fn save_store(store: &Store) {
    if cfg!(test) {
        return;
    }
    match serde_json::to_string_pretty(store) {
        Ok(json) => {
            let _ = std::fs::create_dir_all(crate::config::data_dir());
            if let Err(e) = crate::config::atomic_write(&store_path(), &json) {
                tracing::warn!("Couldn't save the speech engine timings: {e}");
            }
        }
        Err(e) => tracing::warn!("Couldn't write the speech engine timings: {e}"),
    }
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Settings were loaded or saved: "Runs on" and the GPU switch.
pub fn sync(cfg: &crate::config::YapConfig) {
    let mut s = lock();
    s.setting = cfg.stt_device.clone();
    s.use_gpu = cfg.use_gpu;
}

/// The measurement stored for `model_id` that still fits this PC and the
/// model's files.
fn stored(model_id: &str) -> Option<Measured> {
    let model_print = stt::model_fingerprint(&crate::config::data_dir(), model_id)?;
    let pc = pc_print();
    let mut s = lock();
    let store = s.store.get_or_insert_with(load_store);
    store.models.get(model_id).filter(|m| fits(m, &model_print, &pc)).cloned()
}

/// The device `model_id` loads on now (`stt::create_stt_engine`).
pub fn device_for(model_id: &str, use_gpu: bool) -> Device {
    let (setting, synced_gpu) = {
        let s = lock();
        (s.setting.clone(), s.use_gpu)
    };
    let measured = if setting == "auto" || setting.is_empty() {
        stored(model_id).map(|m| m.pick)
    } else {
        None
    };
    device_choice(&setting, use_gpu && synced_gpu, measured)
}

// ---- this PC ----------------------------------------------------------------------

/// The PC's graphics adapters as a measurement depends on them: each one's
/// vendor and device ids, name and driver version (DXGI). A new driver (or
/// card) makes every stored measurement stale. Read once per run.
pub fn pc_print() -> String {
    static PRINT: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    PRINT
        .get_or_init(|| {
            if crate::e2e::active() {
                return "e2e".into();
            }
            adapters_print()
        })
        .clone()
}

#[cfg(windows)]
fn adapters_print() -> String {
    use windows::core::Interface;
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIDevice, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE};
    let mut out = Vec::new();
    // SAFETY: COM calls on a factory this function owns; each description
    // is copied out before its adapter is released.
    unsafe {
        let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else {
            return String::new();
        };
        let mut i = 0u32;
        while let Ok(adapter) = factory.EnumAdapters1(i) {
            i += 1;
            let Ok(desc) = adapter.GetDesc1() else { continue };
            if desc.Flags & (DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0 {
                continue;
            }
            let len = desc.Description.iter().position(|&c| c == 0).unwrap_or(desc.Description.len());
            let name = String::from_utf16_lossy(&desc.Description[..len]);
            // The user-mode driver's version (a.b.c.d packed in 64 bits).
            let driver = adapter
                .CheckInterfaceSupport(&IDXGIDevice::IID)
                .map(|v| {
                    let v = v as u64;
                    format!("{}.{}.{}.{}", v >> 48, (v >> 32) & 0xFFFF, (v >> 16) & 0xFFFF, v & 0xFFFF)
                })
                .unwrap_or_default();
            out.push(format!("{:04x}:{:04x} {} {}", desc.VendorId, desc.DeviceId, name.trim(), driver));
            if i > 16 {
                break;
            }
        }
    }
    out.join("; ")
}

#[cfg(not(windows))]
fn adapters_print() -> String {
    String::new()
}

/// Whether there's a graphics card to try (an integrated one counts).
fn has_gpu() -> bool {
    crate::e2e::active() || crate::gpu::this_pc().is_some()
}

// ---- measuring --------------------------------------------------------------------

/// The bundled clip as 16 kHz mono samples.
pub fn clip() -> Vec<f32> {
    let pcm = crate::tts::parse_wav(CLIP).expect("the bundled speech clip is a WAV");
    let mono: Vec<f32> = if pcm.channels > 1 {
        pcm.samples
            .chunks(pcm.channels as usize)
            .map(|c| c.iter().sum::<f32>() / c.len() as f32)
            .collect()
    } else {
        pcm.samples
    };
    crate::pipeline::resample_linear(&mono, pcm.sample_rate, crate::pipeline::TARGET_SAMPLE_RATE)
}

/// Why a measurement stopped short.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stopped {
    /// Something started that mustn't share the PC with it (a dictation…).
    Interrupted(&'static str),
    /// Nothing could run (the model isn't installed…).
    Failed(String),
}

/// Debug test runs (`YAP_E2E_STT_SPEED` = "gpu=450,cpu=150"): what each
/// device's run costs on the stub engine.
fn fake_cost(device: Device) -> Option<u64> {
    #[cfg(debug_assertions)]
    if crate::e2e::active() {
        let spec = std::env::var("YAP_E2E_STT_SPEED").ok()?;
        return spec.split(',').find_map(|part| {
            let (k, v) = part.split_once('=')?;
            (k.trim() == device.as_str()).then(|| v.trim().parse().ok()).flatten()
        });
    }
    let _ = device;
    None
}

fn transcribe_timed(engine: &SttAdapter, audio: &[f32]) -> (u64, Result<String, String>) {
    let started = Instant::now();
    let out = engine.transcribe(audio, None, false, None).map_err(|e| e.to_string());
    if let Some(ms) = fake_cost(engine.device()) {
        std::thread::sleep(Duration::from_millis(ms));
    }
    (started.elapsed().as_millis() as u64, out)
}

/// Time `model_id` on each of `devices` with `audio` (see the module
/// comment): loads, first transcriptions, then [`TIMED_RUNS`] interleaved
/// rounds. `busy` is asked before every step: `Some(why)` stops it.
pub fn measure(
    data_dir: &Path,
    model_id: &str,
    devices: &[Device],
    audio: &[f32],
    busy: &dyn Fn() -> Option<&'static str>,
) -> Result<Vec<Run>, Stopped> {
    let check = || busy().map_or(Ok(()), |why| Err(Stopped::Interrupted(why)));
    let mut runs: Vec<Run> = Vec::new();
    let mut engines: Vec<Option<SttAdapter>> = Vec::new();
    let mut texts: Vec<Option<String>> = Vec::new();
    for &device in devices {
        check()?;
        if device == Device::Cpu && stt::is_whisper_model(model_id) {
            let gpu_first = runs.iter().zip(&engines).find(|(r, e)| r.device == Device::Gpu && e.is_some()).map(|(r, _)| r.first_ms);
            if gpu_first.is_some_and(skip_cpu_whisper) {
                tracing::info!(model = model_id, "Speech engine timing: Whisper on the processor not tried (the graphics card is quick)");
                runs.push(Run { device, load_ms: 0, first_ms: 0, warm_ms: None, runs_ms: Vec::new(), agrees: false, error: None, skipped: true });
                engines.push(None);
                texts.push(None);
                continue;
            }
        }
        let started = Instant::now();
        let loaded = stt::create_stt_engine_on(data_dir, model_id, device);
        let load_ms = started.elapsed().as_millis() as u64;
        let mut run = Run { device, load_ms, first_ms: 0, warm_ms: None, runs_ms: Vec::new(), agrees: false, error: None, skipped: false };
        let engine = match loaded {
            Ok(e) => e,
            Err(stt::SttError::ModelNotFound(_)) => return Err(Stopped::Failed("The model isn't installed".into())),
            Err(e) => {
                tracing::warn!(model = model_id, device = device.as_str(), "Speech engine timing: couldn't load: {e}");
                run.error = Some(format!("Couldn't load: {e}"));
                runs.push(run);
                engines.push(None);
                texts.push(None);
                continue;
            }
        };
        check()?;
        let (ms, out) = transcribe_timed(&engine, audio);
        run.first_ms = ms;
        match out {
            Ok(text) => {
                texts.push(Some(text));
                engines.push(Some(engine));
            }
            Err(e) => {
                run.error = Some(format!("Couldn't transcribe: {e}"));
                texts.push(None);
                engines.push(None);
            }
        }
        runs.push(run);
    }
    if engines.iter().all(Option::is_none) {
        let why = runs.iter().find_map(|r| r.error.clone()).unwrap_or_else(|| "Nothing ran".into());
        return Err(Stopped::Failed(why));
    }
    // Not worth timing: far slower at the first go than the fastest.
    let fastest = runs.iter().zip(&engines).filter(|(_, e)| e.is_some()).map(|(r, _)| r.first_ms).min().unwrap_or(0);
    for (run, engine) in runs.iter().zip(engines.iter_mut()) {
        if engine.is_some() && too_slow(run.first_ms, fastest) {
            tracing::info!(model = model_id, device = run.device.as_str(), first_ms = run.first_ms, fastest, "Speech engine timing: too slow to win, not timed further");
            *engine = None;
        }
    }
    // The timed rounds, the devices taking turns, each round a new length.
    for share in RUN_LENGTHS {
        let piece = &audio[..((audio.len() as f32 * share) as usize).clamp(1, audio.len())];
        for (i, engine) in engines.iter().enumerate() {
            let Some(engine) = engine else { continue };
            check()?;
            let (ms, out) = transcribe_timed(engine, piece);
            match out {
                Ok(_) => runs[i].runs_ms.push(ms),
                Err(e) => runs[i].error = Some(format!("Couldn't transcribe: {e}")),
            }
        }
    }
    drop(engines);
    // The processor's words are the reference ("still correct").
    let reference = devices
        .iter()
        .position(|d| *d == Device::Cpu)
        .and_then(|i| texts.get(i).cloned().flatten());
    for (run, text) in runs.iter_mut().zip(&texts) {
        run.warm_ms = if run.error.is_none() { median(&run.runs_ms) } else { None };
        run.agrees = text.as_deref().is_some_and(|t| agrees(reference.as_deref(), t));
    }
    Ok(runs)
}

// ---- running it -------------------------------------------------------------------

/// Why the PC isn't free for a measurement now, if it isn't.
fn busy(app: &AppHandle) -> Option<&'static str> {
    if crate::meeting::is_recording() {
        return Some("a meeting is recording");
    }
    if crate::commands::model_download_in_flight() {
        return Some("a model is downloading");
    }
    let state = app.try_state::<crate::AppState>()?;
    let guard = state.pipeline.lock().ok()?;
    let p = guard.as_ref()?;
    // (A test run doesn't wait for the quiet after its start.)
    let quiet_ms = if crate::e2e::active() { 0 } else { QUIET_MS };
    if p.is_busy() {
        Some("a dictation is running")
    } else if p.idle_ms() < quiet_ms {
        Some("a dictation just ran")
    } else {
        None
    }
}

/// Measure the active model in a while (`after`), if it has no measurement
/// that fits. Test runs: only on demand (or with `YAP_E2E_STT_SPEED`).
pub fn schedule(app: &AppHandle, after: Duration) {
    if crate::e2e::active() && fake_cost(Device::Cpu).is_none() {
        return;
    }
    {
        let mut s = lock();
        if s.waiting || s.running.is_some() {
            return;
        }
        s.waiting = true;
    }
    let app = app.clone();
    let spawned = std::thread::Builder::new().name("stt-speed".into()).spawn(move || {
        std::thread::sleep(after);
        lock().waiting = false;
        let model = crate::config::load().model_size;
        let _ = run(&app, &model, false);
    });
    if spawned.is_err() {
        lock().waiting = false;
    }
}

/// Measure `model_id` now (Settings' "Measure again"), in the background.
pub fn measure_now(app: &AppHandle) {
    let app = app.clone();
    let _ = std::thread::Builder::new().name("stt-speed".into()).spawn(move || {
        let model = crate::config::load().model_size;
        let _ = run(&app, &model, true);
    });
}

/// Measure `model_id` (unless a fitting measurement exists and `!force`),
/// once the PC is quiet; store the result and move the warm engine onto
/// the pick. Blocking: run it on its own thread.
pub fn run(app: &AppHandle, model_id: &str, force: bool) -> Result<Measured, String> {
    let data_dir = crate::config::data_dir();
    let model_print = stt::model_fingerprint(&data_dir, model_id).ok_or("The model isn't installed")?;
    if !force {
        if let Some(m) = stored(model_id) {
            return Ok(m);
        }
    }
    {
        let mut s = lock();
        if s.running.is_some() {
            return Err("Already measuring".into());
        }
        s.running = Some(model_id.to_string());
        s.error = None;
    }
    let outcome = run_measured(app, &data_dir, model_id, &model_print);
    {
        let mut s = lock();
        s.running = None;
        s.error = outcome.as_ref().err().cloned();
    }
    emit_status(app);
    if outcome.is_ok() {
        reload_if_needed(app);
    }
    outcome
}

fn run_measured(app: &AppHandle, data_dir: &Path, model_id: &str, model_print: &str) -> Result<Measured, String> {
    // A quiet moment first.
    let waited = Instant::now();
    while let Some(why) = busy(app) {
        if waited.elapsed() > GIVE_UP_WAITING {
            tracing::info!("Speech engine timing: no quiet moment ({why}); trying another time");
            return Err(format!("Waiting for a quiet moment ({why})"));
        }
        std::thread::sleep(idle_poll());
    }
    emit_status(app);
    let devices: Vec<Device> = if has_gpu() { vec![Device::Gpu, Device::Cpu] } else { vec![Device::Cpu] };
    let audio = clip();
    let clip_ms = (audio.len() as u64 * 1000) / u64::from(crate::pipeline::TARGET_SAMPLE_RATE);
    tracing::info!(model = model_id, devices = devices.len(), clip_ms, "Speech engine timing: measuring");
    let runs = match measure(data_dir, model_id, &devices, &audio, &|| busy(app)) {
        Ok(runs) => runs,
        Err(Stopped::Interrupted(why)) => {
            tracing::info!("Speech engine timing: stopped ({why}); trying again later");
            lock().waiting = false;
            schedule(app, RETRY_AFTER_INTERRUPT);
            return Err(format!("Stopped: {why}"));
        }
        Err(Stopped::Failed(why)) => {
            tracing::warn!("Speech engine timing: {why}");
            return Err(why);
        }
    };
    let default = if has_gpu() { Device::Gpu } else { Device::Cpu };
    let pick = pick(&runs).unwrap_or(default);
    for r in &runs {
        tracing::info!(
            model = model_id,
            device = r.device.runtime(model_id),
            load_ms = r.load_ms,
            first_ms = r.first_ms,
            warm_ms = ?r.warm_ms,
            runs = r.runs_ms.len(),
            agrees = r.agrees,
            failed = r.error.is_some(),
            "Speech engine timed"
        );
    }
    tracing::info!(model = model_id, pick = pick.runtime(model_id), "Speech engine: the fastest on this PC");
    let measured = Measured {
        model_print: model_print.to_string(),
        pc_print: pc_print(),
        at: now_secs(),
        clip_ms,
        runs,
        pick,
    };
    {
        let mut s = lock();
        let store = s.store.get_or_insert_with(load_store);
        store.models.insert(model_id.to_string(), measured.clone());
        save_store(store);
    }
    Ok(measured)
}

/// The warm engine isn't on the device its model should run on now (a new
/// pick, or "Runs on" changed): load it there and swap it in, at a quiet
/// moment, in the background.
pub fn reload_if_needed(app: &AppHandle) {
    let app = app.clone();
    let _ = std::thread::Builder::new().name("stt-speed-reload".into()).spawn(move || {
        let Some((model, device)) = engine_info(&app) else { return };
        let use_gpu = crate::config::load().use_gpu;
        let want = device_for(&model, use_gpu);
        if want == device {
            return;
        }
        let waited = Instant::now();
        while busy(&app).is_some() {
            if waited.elapsed() > GIVE_UP_WAITING {
                return;
            }
            std::thread::sleep(idle_poll());
        }
        match stt::create_stt_engine_on(&crate::config::data_dir(), &model, want) {
            Ok(engine) => {
                // Still the model in use, and still not on that device?
                if engine_info(&app).is_some_and(|(m, d)| m == model && d != want) {
                    let state = app.state::<crate::AppState>();
                    let guard = state.pipeline.lock();
                    if let Some(p) = guard.as_ref().ok().and_then(|g| g.as_ref()) {
                        p.replace_engine(engine);
                        tracing::info!(model = %model, device = want.runtime(&model), "Speech engine moved to the device chosen for it");
                    }
                }
            }
            Err(e) => tracing::warn!(model = %model, "Couldn't load the speech engine on the other device: {e}"),
        }
        emit_status(&app);
    });
}

/// The warm engine's model and device, if one is loaded.
fn engine_info(app: &AppHandle) -> Option<(String, Device)> {
    let state = app.try_state::<crate::AppState>()?;
    let guard = state.pipeline.lock().ok()?;
    guard.as_ref()?.engine_info()
}

// ---- Settings ---------------------------------------------------------------------

/// What Settings → Speech-to-Text → Runs on shows.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub model: String,
    pub model_name: String,
    /// "whisper" | "onnx": which runtimes the devices mean (Vulkan or
    /// DirectML on the graphics card).
    pub family: &'static str,
    /// "auto" | "gpu" | "cpu".
    pub setting: String,
    /// Where it runs now (the warm engine, else where it would load).
    pub device: Device,
    pub has_gpu: bool,
    pub measuring: bool,
    /// The measurement that fits now, if any.
    pub measured: Option<Measured>,
    /// Why the last measurement didn't finish.
    pub error: Option<String>,
}

pub fn status(app: &AppHandle) -> Status {
    let cfg = crate::config::load();
    let model = cfg.model_size.clone();
    let measured = stored(&model);
    let device = engine_info(app)
        .filter(|(m, _)| *m == model)
        .map(|(_, d)| d)
        .unwrap_or_else(|| device_for(&model, cfg.use_gpu));
    let s = lock();
    Status {
        model_name: stt::model_name(&model),
        family: if stt::is_whisper_model(&model) { "whisper" } else { "onnx" },
        setting: if s.setting.is_empty() { "auto".into() } else { s.setting.clone() },
        device,
        has_gpu: has_gpu(),
        measuring: s.running.is_some(),
        measured,
        error: s.error.clone(),
        model,
    }
}

fn emit_status(app: &AppHandle) {
    let _ = app.emit(EVENT, status(app));
}

/// Settings → Speech-to-Text → Runs on: what's measured, and where the
/// model runs.
#[tauri::command]
pub fn stt_speed_status(app: AppHandle) -> Status {
    status(&app)
}

/// "Measure again": time the active model on each device now (in the
/// background; `yap-stt-speed` says when it's done).
#[tauri::command]
pub fn stt_speed_measure(app: AppHandle) -> Result<(), String> {
    if lock().running.is_some() {
        return Err("Already measuring".into());
    }
    measure_now(&app);
    // Say "measuring" at once (the run says it again when it starts).
    let mut st = status(&app);
    st.measuring = true;
    let _ = app.emit(EVENT, st);
    Ok(())
}

// ---- tests ------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn run(device: Device, warm: Option<u64>, agrees: bool) -> Run {
        Run { device, load_ms: 100, first_ms: 200, warm_ms: warm, runs_ms: vec![], agrees, error: None, skipped: false }
    }

    #[test]
    fn the_fastest_device_that_got_the_words_right_wins() {
        // This PC's Parakeet: the processor ~3× faster.
        assert_eq!(pick(&[run(Device::Gpu, Some(458), true), run(Device::Cpu, Some(148), true)]), Some(Device::Cpu));
        // A laptop where the graphics card wins.
        assert_eq!(pick(&[run(Device::Gpu, Some(90), true), run(Device::Cpu, Some(400), true)]), Some(Device::Gpu));
        // Faster but wrong (different words) doesn't count.
        assert_eq!(pick(&[run(Device::Gpu, Some(50), false), run(Device::Cpu, Some(400), true)]), Some(Device::Cpu));
        // A device that failed, or wasn't timed (too slow), is out.
        let mut failed = run(Device::Cpu, Some(10), true);
        failed.error = Some("Couldn't load".into());
        assert_eq!(pick(&[run(Device::Gpu, Some(300), true), failed]), Some(Device::Gpu));
        assert_eq!(pick(&[run(Device::Gpu, Some(300), true), run(Device::Cpu, None, true)]), Some(Device::Gpu));
        // Nothing qualifies.
        assert_eq!(pick(&[run(Device::Gpu, None, true)]), None);
        assert_eq!(pick(&[]), None);
    }

    #[test]
    fn the_graphics_card_wins_a_near_tie() {
        assert_eq!(pick(&[run(Device::Cpu, Some(100), true), run(Device::Gpu, Some(104), true)]), Some(Device::Gpu));
        assert_eq!(pick(&[run(Device::Cpu, Some(100), true), run(Device::Gpu, Some(110), true)]), Some(Device::Cpu));
    }

    #[test]
    fn the_words_are_compared_without_case_or_punctuation() {
        assert!(agrees(Some("Hi Imogen, could you send Sam the plan?"), "hi imogen could you send sam the plan"));
        assert!(agrees(Some("I'd like it."), "I'd like it"));
        assert!(!agrees(Some("Hi Imogen, could you send Sam the plan?"), "Hi Imogen, could you send Pam the plan?"));
        // No reference (the processor didn't run): any words will do.
        assert!(agrees(None, "something"));
        assert!(!agrees(None, " … "));
    }

    #[test]
    fn a_device_far_slower_at_its_first_go_is_not_timed_further() {
        // Whisper on the processor against Vulkan.
        assert!(too_slow(6_000, 600));
        // Slow, but not by enough, or quick anyway.
        assert!(!too_slow(2_400, 400));
        assert!(!too_slow(4_000, 1_000));
        assert!(!too_slow(300, 20));
    }

    #[test]
    fn whisper_on_the_processor_is_tried_only_beside_a_slow_graphics_card() {
        assert!(skip_cpu_whisper(2_523));
        assert!(!skip_cpu_whisper(4_800));
    }

    #[test]
    fn the_median_of_the_timed_runs() {
        assert_eq!(median(&[300, 100, 200]), Some(200));
        assert_eq!(median(&[100, 400]), Some(400));
        assert_eq!(median(&[]), None);
    }

    #[test]
    fn runs_on_follows_the_setting_then_the_measurement() {
        assert_eq!(device_choice("cpu", true, Some(Device::Gpu)), Device::Cpu);
        assert_eq!(device_choice("gpu", true, Some(Device::Cpu)), Device::Gpu);
        assert_eq!(device_choice("auto", true, Some(Device::Cpu)), Device::Cpu);
        // Not measured yet: the graphics card, as before.
        assert_eq!(device_choice("auto", true, None), Device::Gpu);
        assert_eq!(device_choice("", true, None), Device::Gpu);
        // The old GPU switch off: the processor.
        assert_eq!(device_choice("auto", false, Some(Device::Gpu)), Device::Cpu);
    }

    #[test]
    fn a_measurement_fits_the_same_model_files_on_the_same_drivers_only() {
        let m = Measured {
            model_print: "670523:1782000000".into(),
            pc_print: "10de:2c05 NVIDIA GeForce RTX 5070 Ti 32.0.15.8180".into(),
            at: 0,
            clip_ms: 7_000,
            runs: vec![],
            pick: Device::Cpu,
        };
        assert!(fits(&m, "670523:1782000000", "10de:2c05 NVIDIA GeForce RTX 5070 Ti 32.0.15.8180"));
        // A new download, or a new driver.
        assert!(!fits(&m, "670524:1782000999", "10de:2c05 NVIDIA GeForce RTX 5070 Ti 32.0.15.8180"));
        assert!(!fits(&m, "670523:1782000000", "10de:2c05 NVIDIA GeForce RTX 5070 Ti 32.0.15.9001"));
    }

    #[test]
    fn the_bundled_clip_is_a_few_seconds_of_speech() {
        let audio = clip();
        let secs = audio.len() as f32 / 16_000.0;
        assert!((4.0..12.0).contains(&secs), "{secs} s");
        assert!(audio.iter().any(|s| s.abs() > 0.1), "silent");
    }

    #[test]
    fn the_stub_engine_is_timed_on_each_device_and_a_dictation_stops_it() {
        // The stub engine loads from a model path it never reads, so a
        // folder with the model's name stands in.
        let dir = std::env::temp_dir().join(format!("yap-stt-speed-{}", std::process::id()));
        let model = dir.join("models").join(stt::model_filename("parakeet-tdt-0.6b-v3"));
        std::fs::create_dir_all(&model).unwrap();
        std::fs::write(model.join("encoder-model.int8.onnx"), b"x").unwrap();
        let audio = clip();
        let runs = measure(&dir, "parakeet-tdt-0.6b-v3", &[Device::Gpu, Device::Cpu], &audio, &|| None).unwrap();
        assert_eq!(runs.len(), 2);
        for r in &runs {
            assert!(r.error.is_none(), "{r:?}");
            assert_eq!(r.runs_ms.len(), TIMED_RUNS);
            assert!(r.warm_ms.is_some());
            assert!(r.agrees, "the stub says the same on both");
        }
        assert!(pick(&runs).is_some());
        // A dictation starting stops it.
        let interrupted = measure(&dir, "parakeet-tdt-0.6b-v3", &[Device::Gpu], &audio, &|| Some("a dictation is running"));
        assert_eq!(interrupted, Err(Stopped::Interrupted("a dictation is running")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The bundled clip and the benchmark's dictation-length clips, made
    /// with Kokoro (Apache-2.0, af_bella) on this PC:
    /// `YAP_TTS_MODELS=<a copy of models/tts> YAP_SPEED_CLIPS=<dir> cargo test
    /// --features engines stt_speed::tests::make_clips -- --ignored --nocapture`.
    /// Writes `assets/stt-speed-clip.wav` and `<dir>/{short,medium,long}.wav`.
    #[cfg(feature = "engines")]
    #[test]
    #[ignore]
    fn make_clips() {
        let out = PathBuf::from(std::env::var_os("YAP_SPEED_CLIPS").expect("YAP_SPEED_CLIPS"));
        std::fs::create_dir_all(&out).unwrap();
        let mut voice = crate::tts::kokoro::open_real("af_bella", 1.0).expect("Kokoro installed under YAP_TTS_MODELS");
        let mut say = |text: &str, path: &Path| {
            let pcm = voice.synthesize(text).unwrap();
            let audio = crate::pipeline::resample_linear(&pcm.samples, pcm.sample_rate, 16_000);
            // A little silence either side, as a dictation's pre-roll has.
            let pad = vec![0f32; 4_800];
            let audio: Vec<f32> = pad.iter().chain(audio.iter()).chain(pad.iter()).copied().collect();
            write_wav(path, &audio);
            println!("{}: {:.2} s", path.display(), audio.len() as f32 / 16_000.0);
        };
        say(CLIP_TEXT, &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets").join("stt-speed-clip.wav"));
        for (name, text) in BENCH_CLIPS {
            say(text, &out.join(format!("{name}.wav")));
            std::fs::write(out.join(format!("{name}.txt")), text).unwrap();
        }
    }

    /// The benchmark's dictation-length clips (fictional people).
    #[cfg(feature = "engines")]
    const BENCH_CLIPS: [(&str, &str); 3] = [
        ("short", "Remind me to call Priya about the budget tomorrow."),
        ("medium", CLIP_TEXT),
        (
            "long",
            "Thanks for the notes from today's meeting. Siobhan will update the design review by Wednesday, and Sam is checking the numbers for the third quarter. Let's keep the launch on the twelfth unless the tests show anything new, and I'll send a summary to the whole team tonight.",
        ),
    ];

    #[cfg(feature = "engines")]
    fn write_wav(path: &Path, audio: &[f32]) {
        let data: Vec<u8> = audio.iter().flat_map(|s| ((s.clamp(-1.0, 1.0) * 32_767.0) as i16).to_le_bytes()).collect();
        let mut w = Vec::new();
        w.extend_from_slice(b"RIFF");
        w.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        w.extend_from_slice(b"WAVEfmt ");
        w.extend_from_slice(&16u32.to_le_bytes());
        w.extend_from_slice(&1u16.to_le_bytes());
        w.extend_from_slice(&1u16.to_le_bytes());
        w.extend_from_slice(&16_000u32.to_le_bytes());
        w.extend_from_slice(&32_000u32.to_le_bytes());
        w.extend_from_slice(&2u16.to_le_bytes());
        w.extend_from_slice(&16u16.to_le_bytes());
        w.extend_from_slice(b"data");
        w.extend_from_slice(&(data.len() as u32).to_le_bytes());
        w.extend_from_slice(&data);
        std::fs::write(path, w).unwrap();
    }

    #[cfg(feature = "engines")]
    fn read_clip(path: &Path) -> Vec<f32> {
        let pcm = crate::tts::parse_wav(&std::fs::read(path).unwrap()).unwrap();
        crate::pipeline::resample_linear(&pcm.samples, pcm.sample_rate, 16_000)
    }

    /// The numbers for the report: each model on each device, with the
    /// dictation-length clips, the devices interleaved and medians kept
    /// (other work on the PC slows each alike). `YAP_SPEED_DATA` = a data
    /// folder whose `models/` holds copies of the models (never the real
    /// one), `YAP_SPEED_CLIPS` = `make_clips`' folder, `YAP_SPEED_MODELS` =
    /// ids, `,`-separated; `YAP_SPEED_ROUNDS` (default 7).
    /// `cargo test --features engines stt_speed::tests::bench_this_pc -- --ignored --nocapture --test-threads=1`
    #[cfg(feature = "engines")]
    #[test]
    #[ignore]
    fn bench_this_pc() {
        let data = PathBuf::from(std::env::var_os("YAP_SPEED_DATA").expect("YAP_SPEED_DATA"));
        let clips_dir = PathBuf::from(std::env::var_os("YAP_SPEED_CLIPS").expect("YAP_SPEED_CLIPS"));
        let models = std::env::var("YAP_SPEED_MODELS").unwrap_or_else(|_| "parakeet-tdt-0.6b-v3,parakeet-tdt-0.6b-v2,large".into());
        let rounds: usize = std::env::var("YAP_SPEED_ROUNDS").ok().and_then(|r| r.parse().ok()).unwrap_or(7);
        let clips: Vec<(String, Vec<f32>)> = ["short", "medium", "long"]
            .iter()
            .map(|n| (n.to_string(), read_clip(&clips_dir.join(format!("{n}.wav")))))
            .collect();
        println!("PC: {}", pc_print());
        for model in models.split(',').map(str::trim).filter(|m| !m.is_empty()) {
            println!("\n== {model} ==");
            // What Yap itself would decide, on the bundled clip.
            match measure(&data, model, &[Device::Gpu, Device::Cpu], &clip(), &|| None) {
                Ok(runs) => {
                    for r in &runs {
                        println!(
                            "  in-app benchmark {:>8}: load {} ms, first {} ms, warm {:?} ms ({:?}), agrees {}{}",
                            r.device.runtime(model),
                            r.load_ms,
                            r.first_ms,
                            r.warm_ms,
                            r.runs_ms,
                            r.agrees,
                            r.error.as_deref().map(|e| format!(", {e}")).unwrap_or_default()
                        );
                    }
                    println!("  → pick: {:?}", pick(&runs).map(|d| d.runtime(model)));
                }
                Err(e) => println!("  in-app benchmark failed: {e:?}"),
            }
            // Each device: load + first dictation (the "first dictation"
            // latency), then interleaved warm runs on each clip — a device
            // far slower at its first go (too_slow) only gets that.
            let mut engines = Vec::new();
            let mut firsts = Vec::new();
            for device in [Device::Gpu, Device::Cpu] {
                if device == Device::Cpu && stt::is_whisper_model(model) && firsts.first().copied().is_some_and(skip_cpu_whisper) {
                    println!("  {:>8}: not tried (Whisper beside a quick graphics card)", device.runtime(model));
                    continue;
                }
                let t = Instant::now();
                let e = stt::create_stt_engine_on(&data, model, device).expect("load");
                let load = t.elapsed().as_millis();
                let t = Instant::now();
                let text = e.transcribe(&clips[0].1, None, false, None).unwrap();
                let first = t.elapsed().as_millis() as u64;
                println!("  {:>8}: load {load} ms, first dictation (short clip) {first} ms → {} words", device.runtime(model), words(&text).len());
                firsts.push(first);
                engines.push((device, e));
            }
            let fastest = firsts.iter().copied().min().unwrap_or(0);
            let engines: Vec<(Device, SttAdapter)> = engines
                .into_iter()
                .zip(firsts)
                .filter_map(|((d, e), first)| {
                    if too_slow(first, fastest) {
                        println!("  {:>8}: too slow to time further", d.runtime(model));
                        None
                    } else {
                        Some((d, e))
                    }
                })
                .collect();
            let mut times: BTreeMap<(String, &'static str), Vec<u64>> = BTreeMap::new();
            let mut texts: BTreeMap<(String, &'static str), String> = BTreeMap::new();
            for _ in 0..rounds {
                for (name, audio) in &clips {
                    for (device, e) in &engines {
                        let t = Instant::now();
                        let text = e.transcribe(audio, None, false, None).unwrap();
                        times.entry((name.clone(), device.runtime(model))).or_default().push(t.elapsed().as_millis() as u64);
                        texts.insert((name.clone(), device.runtime(model)), text);
                    }
                }
            }
            for (name, audio) in &clips {
                let secs = audio.len() as f32 / 16_000.0;
                let mut line = format!("  {name:>6} ({secs:.1} s):");
                for (device, _) in &engines {
                    let key = (name.clone(), device.runtime(model));
                    let v = &times[&key];
                    let mut sorted = v.clone();
                    sorted.sort_unstable();
                    line.push_str(&format!(
                        "  {} median {} ms (min {}, max {})",
                        device.runtime(model),
                        median(v).unwrap(),
                        sorted[0],
                        sorted[sorted.len() - 1]
                    ));
                }
                if engines.len() == 2 {
                    let same = texts[&(name.clone(), "CPU")] == texts[&(name.clone(), Device::Gpu.runtime(model))];
                    line.push_str(&format!("  same text: {same}"));
                }
                println!("{line}");
            }
        }
    }
}
