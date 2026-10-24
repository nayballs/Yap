//! **The speech engine's device on this PC** (2026-10-22): which device each
//! installed speech model runs on — the graphics card (the ONNX models,
//! Parakeet first, through DirectML; Whisper through Vulkan) or the
//! processor — measured once per model and PC instead of assumed. The
//! graphics card is the default and stays it unless the processor is
//! **clearly** faster here for everyday dictations (3 and 11 seconds).
//!
//! **The benchmark** ([`run`]): a bundled speech clip ([`CLIP`], ~36 s of
//! Kokoro — Apache-2.0 — reading [`PASSAGE`]), cut at pauses into three
//! dictation lengths ([`LENGTHS`]: short ~3 s, typical ~10 s, long = the
//! whole clip), transcribed on each device with the call a dictation makes
//! (the language, translate and the Whisper vocabulary prompt:
//! [`Call`], from `Pipeline::transcribe_options`). Per device: the load and
//! the **first transcription after it** (the typical length; DirectML and
//! Vulkan build their graphs here) are timed and shown, never part of the
//! choice; then [`WARM_RUNS`] rounds of the three lengths, the devices
//! taking turns (other work on the PC slows each alike), every run a length
//! the engine hasn't had before (the piece plus a little more trailing
//! silence each round: a real dictation never repeats one, and a runtime
//! caching its plan per input shape would look faster on a repeated clip),
//! and each length's median kept (a warm dictation's latency). The processor's words are the reference
//! ("still correct": the graphics card's must match them, a word in twenty
//! aside). The processor isn't timed further when its first transcription
//! took [`SLOW_FACTOR`]× the graphics card's second, warm one (and over
//! [`SLOW_FLOOR_MS`]), nor tried at all for Whisper beside a graphics card
//! that's quick warm ([`skip_cpu_whisper`]); the graphics card is always
//! timed.
//!
//! **The pick** ([`pick`], pure): judged on everyday dictations — the short
//! and typical lengths ([`EVERYDAY`]); the long one is measured and shown,
//! never counted. The graphics card, unless the processor took at most
//! `1 − CPU_MARGIN` (80 %) of its time at **both** ([`clearly_faster`]) —
//! or the graphics card failed or got the words wrong. A processor that
//! wins narrowly stays free for the person's apps, and a meeting recording
//! or a busy app slows the processor path (not the graphics card's) later
//! on. Each device's time **for your dictations** ([`Run::everyday_ms`],
//! what Settings shows) weighs the two lengths by how long this person's
//! recent dictations were ([`short_share`], from history; half and half
//! with none).
//!
//! **When**: once per model and PC — on Yap's start ([`AFTER_START`]) and
//! when a model is installed or switched to ([`AFTER_CHANGE`]) if the
//! stored result doesn't fit any more (the model's files changed: a new
//! download; the graphics driver changed: [`pc_print`] has each adapter's
//! driver version; the method changed: [`METHOD`]), and on demand
//! (Settings' "Measure again"). Never during a dictation (or an Upload, or
//! the moments after one: [`QUIET_MS`]), a meeting recording, a model
//! download, or while another app keeps the processor busy
//! ([`CPU_BUSY_PCT`]): it waits for a quiet moment, and a dictation, meeting
//! or download that starts mid-way stops it (tried again
//! [`RETRY_AFTER_INTERRUPT`] later).
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
//! cost per run.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::stt::{self, Device, SttAdapter};

/// The clip every model is timed on: 16 kHz mono 16-bit PCM, ~36 s of
/// Kokoro's af_bella (Apache-2.0) reading [`PASSAGE`], a pause between
/// sentences (made by the `--ignored` `make_clip` test below).
const CLIP: &[u8] = include_bytes!("../assets/stt-speed-clip.wav");
/// What the clip says, sentence by sentence (fictional people). The first
/// is the short length, the first three the typical one.
/// (`yap.exe bench` scores each transcript's words against it.)
pub const PASSAGE: [&str; 6] = [
    "Remind me to call Priya about the budget tomorrow.",
    "Hi Imogen, could you send Sam the updated launch plan before Friday?",
    "I'd like to go through it together on Monday morning.",
    "Thanks for the notes from today's meeting. Siobhan will update the design review by Wednesday, and Sam is checking the numbers for the third quarter.",
    "Let's keep the launch on the twelfth unless the tests show anything new, and I'll send a summary to the whole team tonight.",
    "One more thing: the office is closed next Thursday for the building work, so please book a room at the library if you need one.",
];

/// The dictation lengths timed: a name and roughly how long (ms; 0 = the
/// whole clip). Each is cut at the pause nearest that length.
pub const LENGTHS: [(&str, u64); 3] = [("short", 3_000), ("typical", 10_000), ("long", 0)];
/// Which of [`LENGTHS`] Settings shows, and the first transcription uses.
pub const TYPICAL: usize = 1;
/// How far from its length a cut may move to find a pause.
const CUT_SLACK_MS: u64 = 1_500;
/// Warm runs per length and device (the median is kept).
pub const WARM_RUNS: usize = 3;
/// Silence after each piece, as a dictation has before its stop key…
pub(crate) const TAIL_MS: u64 = 300;
/// …and this much more each round, so every run is a new length.
pub(crate) const PAD_STEP_MS: u64 = 160;
/// Which of [`LENGTHS`] decide: everyday dictations (short and typical).
pub const EVERYDAY: [usize; 2] = [0, TYPICAL];
/// The processor is picked only if it takes at most `1 − CPU_MARGIN` of the
/// graphics card's time at both everyday lengths.
pub const CPU_MARGIN: f64 = 0.20;
/// How the measuring is done; a stored result from another method is
/// measured again (1: one 8-second clip; 2: three lengths, the graphics
/// card kept unless the processor is clearly faster at all three; 3:
/// everyday lengths decide, and Parakeet's DirectML path runs its decoder
/// on the processor with a fixed encoder length — `vendor/transcribe-rs`).
pub const METHOD: u32 = 3;
/// How long after Yap starts the first measurement may begin.
pub const AFTER_START: Duration = Duration::from_secs(60);
/// …and after a model is installed or switched to.
pub const AFTER_CHANGE: Duration = Duration::from_secs(15);
/// How long the pipeline must have been quiet (no recording, transcribing,
/// or a dictation in the last moments) before a measurement starts.
const QUIET_MS: u64 = 20_000;
/// Busier than this (percent, over a second), the processor isn't free for
/// a fair measurement: it waits.
pub const CPU_BUSY_PCT: u32 = 50;
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
/// The processor, when its first transcription took this many times the
/// graphics card's…
pub const SLOW_FACTOR: u64 = 5;
/// …and longer than this, isn't timed further.
pub const SLOW_FLOOR_MS: u64 = 3_000;
/// The status event (`Status`).
pub const EVENT: &str = "yap-stt-speed";

// ---- the record -------------------------------------------------------------------

/// One dictation length on one device.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Timing {
    /// How long the piece is (without the padding).
    pub clip_ms: u64,
    /// The median of its warm runs (`None`: a run failed).
    pub warm_ms: Option<u64>,
    #[serde(default)]
    pub runs_ms: Vec<u64>,
}

/// One device's timings for a model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub device: Device,
    /// Loading the model on it.
    pub load_ms: u64,
    /// Its first transcription after loading (the typical length): what the
    /// first dictation after a load pays, graph building included. Shown,
    /// never part of the choice.
    pub first_ms: u64,
    /// Warm, per length ([`LENGTHS`] order). Empty: not timed (failed, or
    /// the processor far slower at its first go).
    #[serde(default)]
    pub lengths: Vec<Timing>,
    /// Its words matched the reference's (the processor's).
    pub agrees: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Not tried: Whisper on the processor, beside a graphics card that ran
    /// it quickly ([`skip_cpu_whisper`]).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub skipped: bool,
    /// Its time for this person's dictations: the everyday lengths' warm
    /// medians weighed by [`Measured::short_share`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub everyday_ms: Option<u64>,
}

impl Run {
    fn new(device: Device) -> Self {
        Run { device, load_ms: 0, first_ms: 0, lengths: Vec::new(), agrees: false, error: None, skipped: false, everyday_ms: None }
    }

    /// The everyday lengths' warm medians weighed: `short_share` of the
    /// short one, the rest the typical one.
    pub fn everyday(&self, short_share: f64) -> Option<u64> {
        let short = self.lengths.get(EVERYDAY[0])?.warm_ms? as f64;
        let typical = self.lengths.get(EVERYDAY[1])?.warm_ms? as f64;
        Some((short * short_share + typical * (1.0 - short_share)).round() as u64)
    }
}

/// A model measured on this PC.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Measured {
    /// [`METHOD`] when measured (0: before it was recorded).
    #[serde(default)]
    pub method: u32,
    /// The model's files when measured (`stt::model_fingerprint`).
    pub model_print: String,
    /// The PC's graphics adapters and drivers when measured ([`pc_print`]).
    pub pc_print: String,
    /// When (unix seconds).
    pub at: u64,
    /// How busy the processor was just before (percent), if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_load: Option<u32>,
    /// The share of this person's recent dictations nearer the short length
    /// than the typical one ([`short_share`]).
    #[serde(default = "half")]
    pub short_share: f64,
    pub runs: Vec<Run>,
    pub pick: Device,
}

fn half() -> f64 {
    0.5
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Store {
    #[serde(default)]
    models: BTreeMap<String, Measured>,
}

/// What a dictation passes the engine besides its audio.
#[derive(Debug, Clone, Default)]
pub struct Call {
    pub language: Option<String>,
    pub translate: bool,
    /// The Whisper vocabulary prompt (the dictionary, the names Yap knows).
    pub prompt: Option<String>,
}

// ---- the pure parts ---------------------------------------------------------------

/// A transcript's words, as compared: lower case, letters and digits.
pub(crate) fn words(text: &str) -> Vec<String> {
    text.replace('\u{2019}', "'")
        .split(|c: char| !c.is_alphanumeric() && c != '\'')
        .map(|w| w.trim_matches('\'').to_lowercase())
        .filter(|w| !w.is_empty())
        .collect()
}

/// Words added, dropped or changed to turn `a` into `b`.
pub(crate) fn word_edits(a: &[String], b: &[String]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, wa) in a.iter().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, wb) in b.iter().enumerate() {
            cur[j + 1] = (prev[j] + usize::from(wa != wb)).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        prev = cur;
    }
    prev[b.len()]
}

/// Whether `text` says what `reference` says (case and punctuation aside,
/// a word in twenty — at least one — may differ: the devices round
/// differently); with no reference, whether it says anything at all.
pub fn agrees(reference: Option<&str>, text: &str) -> bool {
    let t = words(text);
    match reference {
        Some(r) => {
            let r = words(r);
            r.is_empty() == t.is_empty() && word_edits(&r, &t) <= (r.len() / 20).max(1)
        }
        None => !t.is_empty(),
    }
}

/// The median of some timings (the upper one of an even count).
pub fn median(ms: &[u64]) -> Option<u64> {
    let mut v = ms.to_vec();
    v.sort_unstable();
    v.get(v.len() / 2).copied()
}

/// The processor isn't worth timing further when its first transcription
/// took [`SLOW_FACTOR`]× the graphics card's second (warm) one and over
/// [`SLOW_FLOOR_MS`]. (The graphics card is always timed, and never judged
/// by its first run, which builds its graph.)
pub fn too_slow(cpu_first_ms: u64, gpu_warm_ms: u64) -> bool {
    cpu_first_ms > SLOW_FLOOR_MS && cpu_first_ms > gpu_warm_ms.saturating_mul(SLOW_FACTOR)
}

/// Whisper on the processor isn't tried when the graphics card's second
/// (warm) transcription took no more than [`SLOW_FLOOR_MS`]: whisper.cpp on
/// the processor (4 threads) needs far longer for its 30-second window — on
/// the maintainer's PC large-v3 took 16 minutes for 11 s of speech against
/// Vulkan's 0.9 s warm (8.3 s for its first run, the shaders being built),
/// holding the processor all that time. A slow graphics card (an old
/// integrated one) still has it tried.
pub fn skip_cpu_whisper(gpu_warm_ms: u64) -> bool {
    gpu_warm_ms <= SLOW_FLOOR_MS
}

/// A length's warm median, if it was timed.
fn warm(r: &Run, length: usize) -> Option<u64> {
    r.lengths.get(length).and_then(|t| t.warm_ms)
}

/// Ran, got the words right and has a warm median at the everyday lengths.
fn timed(r: &Run) -> bool {
    r.error.is_none() && r.agrees && EVERYDAY.iter().all(|&l| warm(r, l).is_some())
}

/// The processor took at most `1 − CPU_MARGIN` of the graphics card's time
/// at both everyday lengths (the long one doesn't count).
pub fn clearly_faster(cpu: &Run, gpu: &Run) -> bool {
    EVERYDAY.iter().all(|&l| match (warm(cpu, l), warm(gpu, l)) {
        (Some(c), Some(g)) => c as f64 <= g as f64 * (1.0 - CPU_MARGIN),
        _ => false,
    })
}

/// The share of `secs` (dictations' spoken lengths) nearer the short length
/// than the typical one (`boundary`: halfway between them, seconds); half
/// and half with none.
pub fn short_share_of(secs: &[f32], boundary: f32) -> f64 {
    if secs.is_empty() {
        return 0.5;
    }
    secs.iter().filter(|s| **s < boundary).count() as f64 / secs.len() as f64
}

/// [`short_share_of`] this person's latest 200 dictations (history; none in
/// a test run's first moments).
fn short_share(boundary: f32) -> f64 {
    short_share_of(&crate::history::recent_dictation_secs(200), boundary)
}

/// The device to run on: the graphics card, unless the processor is
/// [`clearly_faster`] or the graphics card didn't run, wasn't timed or got
/// the words wrong. `None` when neither qualifies.
pub fn pick(runs: &[Run]) -> Option<Device> {
    let gpu = runs.iter().find(|r| r.device == Device::Gpu && timed(r));
    let cpu = runs.iter().find(|r| r.device == Device::Cpu && timed(r));
    match (gpu, cpu) {
        (Some(g), Some(c)) => Some(if clearly_faster(c, g) { Device::Cpu } else { Device::Gpu }),
        (Some(_), None) => Some(Device::Gpu),
        (None, Some(_)) => Some(Device::Cpu),
        (None, None) => None,
    }
}

/// Where a model runs, from Settings' "Runs on" (`setting`: "auto" | "gpu" |
/// "cpu"), the old GPU switch and the measured pick; until it's measured,
/// `unmeasured` (`hardware::speech_device_before_measuring`: the graphics
/// card when there's one that isn't small, else the processor).
pub fn device_choice(setting: &str, use_gpu: bool, measured: Option<Device>, unmeasured: Device) -> Device {
    match setting {
        "cpu" => Device::Cpu,
        "gpu" => Device::Gpu,
        _ if !use_gpu => Device::Cpu,
        _ => measured.unwrap_or(unmeasured),
    }
}

/// Whether a stored measurement still fits: measured this way, of the same
/// model files on the same graphics hardware and drivers.
fn fits(m: &Measured, model_print: &str, pc_print: &str) -> bool {
    m.method == METHOD && m.model_print == model_print && m.pc_print == pc_print
}

/// Where each of [`LENGTHS`] ends in `audio` (16 kHz): at the quietest
/// 20 ms near its length, the whole clip for the last.
pub fn cuts(audio: &[f32]) -> Vec<usize> {
    let per_ms = (crate::pipeline::TARGET_SAMPLE_RATE / 1000) as usize;
    LENGTHS
        .iter()
        .map(|&(_, ms)| {
            if ms == 0 {
                return audio.len();
            }
            let at = ms as usize * per_ms;
            let slack = CUT_SLACK_MS as usize * per_ms;
            let from = at.saturating_sub(slack).min(audio.len());
            let to = (at + slack).min(audio.len());
            crate::media::quietest_frame(audio, from, to, crate::pipeline::TARGET_SAMPLE_RATE as usize).0.clamp(1, audio.len())
        })
        .collect()
}

/// A piece with `pad_ms` of silence after it (a new length, the same
/// words).
pub(crate) fn padded(piece: &[f32], pad_ms: u64) -> Vec<f32> {
    let mut out = piece.to_vec();
    out.resize(piece.len() + pad_ms as usize * (crate::pipeline::TARGET_SAMPLE_RATE / 1000) as usize, 0.0);
    out
}

fn ms_of(samples: usize) -> u64 {
    samples as u64 * 1000 / u64::from(crate::pipeline::TARGET_SAMPLE_RATE)
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

const STORE_FILE: &str = "stt_speed.json";

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
    device_choice(&setting, use_gpu && synced_gpu, measured, crate::hardware::speech_device_here())
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

/// Whether there's a graphics card to try (an integrated one counts). A
/// test run has one unless its stand-in card says none (`YAP_E2E_GPU`).
fn has_gpu() -> bool {
    #[cfg(debug_assertions)]
    if crate::e2e::active() && std::env::var_os("YAP_E2E_GPU").is_none() {
        return true;
    }
    crate::gpu::this_pc().is_some()
}

/// How busy the processor is (percent of all cores) over `over`, from
/// Windows' idle and busy times. `None` in a test run or if unreadable.
pub fn cpu_load(over: Duration) -> Option<u32> {
    if crate::e2e::active() {
        return None;
    }
    let (idle0, total0) = system_times()?;
    std::thread::sleep(over);
    let (idle1, total1) = system_times()?;
    let total = total1.checked_sub(total0).filter(|t| *t > 0)?;
    let idle = idle1.saturating_sub(idle0).min(total);
    Some((100 - idle * 100 / total) as u32)
}

/// (idle, kernel + user) in 100 ns units since boot.
#[cfg(windows)]
fn system_times() -> Option<(u64, u64)> {
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::Threading::GetSystemTimes;
    let (mut idle, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default());
    // SAFETY: three out-pointers to locals.
    unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)) }.ok()?;
    let n = |t: FILETIME| (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime);
    // (Kernel time includes the idle time.)
    Some((n(idle), n(kernel) + n(user)))
}

#[cfg(not(windows))]
fn system_times() -> Option<(u64, u64)> {
    None
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

/// One transcription, as a dictation makes it (`run_stt`: the engine's
/// `transcribe` with the language, translate and vocabulary prompt), timed.
pub fn transcribe_timed(engine: &SttAdapter, audio: &[f32], call: &Call) -> (u64, Result<String, String>) {
    let started = Instant::now();
    let out = engine
        .transcribe(audio, call.language.as_deref(), call.translate, call.prompt.as_deref())
        .map_err(|e| e.to_string());
    if let Some(ms) = fake_cost(engine.device()) {
        std::thread::sleep(Duration::from_millis(ms));
    }
    (started.elapsed().as_millis() as u64, out)
}

/// Time `model_id` on each of `devices` with `audio` cut into [`LENGTHS`]
/// (see the module comment): loads and first transcriptions, then
/// [`WARM_RUNS`] interleaved rounds of every length. `busy` is asked before
/// every step: `Some(why)` stops it.
pub fn measure(
    data_dir: &Path,
    model_id: &str,
    devices: &[Device],
    audio: &[f32],
    call: &Call,
    busy: &dyn Fn() -> Option<&'static str>,
) -> Result<Vec<Run>, Stopped> {
    let check = || busy().map_or(Ok(()), |why| Err(Stopped::Interrupted(why)));
    let pieces: Vec<&[f32]> = cuts(audio).into_iter().map(|end| &audio[..end]).collect();
    let mut runs: Vec<Run> = Vec::new();
    let mut engines: Vec<Option<SttAdapter>> = Vec::new();
    // Each device's words: its first transcription, then each length's.
    let mut firsts: Vec<Option<String>> = Vec::new();
    let mut texts: Vec<Vec<Option<String>>> = Vec::new();
    // The graphics card's second run (its graph built), which the processor
    // is judged against before it's timed — never its first.
    let mut gpu_warm: Option<u64> = None;
    for &device in devices {
        check()?;
        texts.push(vec![None; pieces.len()]);
        let mut run = Run::new(device);
        if device == Device::Cpu && stt::is_whisper_model(model_id) && gpu_warm.is_some_and(skip_cpu_whisper) {
            tracing::info!(model = model_id, "Speech engine timing: Whisper on the processor not tried (the graphics card is quick)");
            run.skipped = true;
            runs.push(run);
            engines.push(None);
            firsts.push(None);
            continue;
        }
        let started = Instant::now();
        let loaded = stt::create_stt_engine_on(data_dir, model_id, device);
        run.load_ms = started.elapsed().as_millis() as u64;
        let engine = match loaded {
            Ok(e) => e,
            Err(stt::SttError::ModelNotFound(_)) => return Err(Stopped::Failed("The model isn't installed".into())),
            Err(e) => {
                tracing::warn!(model = model_id, device = device.as_str(), "Speech engine timing: couldn't load: {e}");
                run.error = Some(format!("Couldn't load: {e}"));
                runs.push(run);
                engines.push(None);
                firsts.push(None);
                continue;
            }
        };
        check()?;
        // The first dictation after a load: the typical length.
        let (ms, out) = transcribe_timed(&engine, &padded(pieces[TYPICAL], TAIL_MS), call);
        run.first_ms = ms;
        match out {
            Ok(text) => {
                if device == Device::Gpu {
                    check()?;
                    let (ms, out) = transcribe_timed(&engine, &padded(pieces[TYPICAL], TAIL_MS + PAD_STEP_MS / 2), call);
                    gpu_warm = out.is_ok().then_some(ms);
                }
                firsts.push(Some(text));
                engines.push(Some(engine));
            }
            Err(e) => {
                run.error = Some(format!("Couldn't transcribe: {e}"));
                firsts.push(None);
                engines.push(None);
            }
        }
        runs.push(run);
    }
    if engines.iter().all(Option::is_none) {
        let why = runs.iter().find_map(|r| r.error.clone()).unwrap_or_else(|| "Nothing ran".into());
        return Err(Stopped::Failed(why));
    }
    // The processor far slower at its first go than the graphics card warm:
    // not timed further (it can't win, and holds the processor for seconds).
    if let Some(gpu_warm) = gpu_warm {
        for (run, engine) in runs.iter().zip(engines.iter_mut()) {
            if run.device == Device::Cpu && engine.is_some() && too_slow(run.first_ms, gpu_warm) {
                tracing::info!(model = model_id, first_ms = run.first_ms, gpu_warm, "Speech engine timing: the processor is far slower, not timed further");
                *engine = None;
            }
        }
    }
    // The warm rounds: every length, the devices taking turns, each run a
    // length the engine hasn't had (a little more trailing silence a round).
    let mut times: Vec<Vec<Vec<u64>>> = vec![vec![Vec::new(); pieces.len()]; runs.len()];
    for round in 0..WARM_RUNS {
        for (li, piece) in pieces.iter().enumerate() {
            let audio = padded(piece, TAIL_MS + (round as u64 + 1) * PAD_STEP_MS);
            for (i, engine) in engines.iter().enumerate() {
                let Some(engine) = engine else { continue };
                if runs[i].error.is_some() {
                    continue;
                }
                check()?;
                let (ms, out) = transcribe_timed(engine, &audio, call);
                match out {
                    Ok(text) => {
                        times[i][li].push(ms);
                        texts[i][li].get_or_insert(text);
                    }
                    Err(e) => runs[i].error = Some(format!("Couldn't transcribe: {e}")),
                }
            }
        }
    }
    let was_timed: Vec<bool> = engines.iter().map(Option::is_some).collect();
    drop(engines);
    // The processor's words are the reference ("still correct").
    let cpu = runs.iter().position(|r| r.device == Device::Cpu);
    for (i, run) in runs.iter_mut().enumerate() {
        let reference = |li: Option<usize>| -> Option<String> {
            let c = cpu.filter(|c| *c != i)?;
            match li {
                Some(li) => texts[c][li].clone(),
                None => firsts[c].clone(),
            }
        };
        // Compared at every length both have, else on the first
        // transcriptions; a device with no words at all doesn't agree.
        let mut compared = false;
        let mut agree = true;
        for (li, text) in texts[i].iter().enumerate() {
            if let Some(text) = text {
                compared = true;
                agree &= agrees(reference(Some(li)).as_deref(), text);
            }
        }
        if !compared {
            match &firsts[i] {
                Some(text) => agree = agrees(reference(None).as_deref(), text),
                None => agree = false,
            }
        }
        run.agrees = agree;
        if was_timed[i] {
            let failed = run.error.is_some();
            run.lengths = pieces
                .iter()
                .zip(&times[i])
                .map(|(piece, ms)| Timing {
                    clip_ms: ms_of(piece.len()),
                    warm_ms: if failed { None } else { median(ms) },
                    runs_ms: ms.clone(),
                })
                .collect();
        }
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

/// What a dictation passes the engine now (the pipeline's own settings).
fn dictation_call(app: &AppHandle) -> Call {
    let from_pipeline = app.try_state::<crate::AppState>().and_then(|state| {
        let guard = state.pipeline.lock().ok()?;
        guard.as_ref().map(|p| p.transcribe_options())
    });
    match from_pipeline {
        Some((language, translate, prompt)) => Call { language, translate, prompt },
        None => Call::default(),
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

/// A quiet moment: nothing of Yap's in the way, and the processor not kept
/// busy by another app (which would make it look slower than it is).
/// Returns how busy the processor was.
fn wait_for_quiet(app: &AppHandle) -> Result<Option<u32>, String> {
    let waited = Instant::now();
    loop {
        let why = match busy(app) {
            Some(why) => Some(why),
            None => match cpu_load(Duration::from_secs(1)) {
                Some(load) if load >= CPU_BUSY_PCT => Some("another app is keeping the processor busy"),
                load => return Ok(load),
            },
        };
        if let Some(why) = why {
            if waited.elapsed() > GIVE_UP_WAITING {
                tracing::info!("Speech engine timing: no quiet moment ({why}); trying another time");
                return Err(format!("Waiting for a quiet moment ({why})"));
            }
        }
        std::thread::sleep(idle_poll());
    }
}

fn run_measured(app: &AppHandle, data_dir: &Path, model_id: &str, model_print: &str) -> Result<Measured, String> {
    let cpu_load = wait_for_quiet(app)?;
    emit_status(app);
    let devices: Vec<Device> = if has_gpu() { vec![Device::Gpu, Device::Cpu] } else { vec![Device::Cpu] };
    let audio = clip();
    let call = dictation_call(app);
    tracing::info!(
        model = model_id,
        devices = devices.len(),
        clip_ms = ms_of(audio.len()),
        cpu_load = ?cpu_load,
        "Speech engine timing: measuring"
    );
    let mut runs = match measure(data_dir, model_id, &devices, &audio, &call, &|| busy(app)) {
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
    // Each device's time for this person's dictations (Settings' line).
    let boundary = cuts(&audio)[..2].iter().map(|&e| ms_of(e) as f32 / 1000.0).sum::<f32>() / 2.0;
    let short_share = short_share(boundary);
    for r in &mut runs {
        r.everyday_ms = r.everyday(short_share);
    }
    for r in &runs {
        tracing::info!(
            model = model_id,
            device = r.device.runtime(model_id),
            load_ms = r.load_ms,
            first_ms = r.first_ms,
            agrees = r.agrees,
            skipped = r.skipped,
            failed = r.error.is_some(),
            everyday_ms = ?r.everyday_ms,
            "Speech engine timed (first transcription after loading; everyday dictations)"
        );
        for (t, (name, _)) in r.lengths.iter().zip(LENGTHS) {
            tracing::info!(
                model = model_id,
                device = r.device.runtime(model_id),
                length = name,
                clip_ms = t.clip_ms,
                warm_ms = ?t.warm_ms,
                runs = ?t.runs_ms,
                "Speech engine timed (warm)"
            );
        }
    }
    tracing::info!(model = model_id, pick = pick.runtime(model_id), short_share, "Speech engine: the device for this model on this PC");
    let measured = Measured {
        method: METHOD,
        model_print: model_print.to_string(),
        pc_print: pc_print(),
        at: now_secs(),
        cpu_load,
        short_share,
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
    /// The processor must take at least this share less time than the
    /// graphics card at both everyday lengths to be picked (0.2).
    pub cpu_margin: f64,
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
        cpu_margin: CPU_MARGIN,
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

    /// A device timed at the three lengths.
    fn run(device: Device, warm: [u64; 3], agrees: bool) -> Run {
        Run {
            device,
            load_ms: 100,
            first_ms: 200,
            lengths: warm
                .iter()
                .zip([3_000, 10_000, 40_000])
                .map(|(ms, clip_ms)| Timing { clip_ms, warm_ms: Some(*ms), runs_ms: vec![*ms; 3] })
                .collect(),
            agrees,
            error: None,
            skipped: false,
            everyday_ms: None,
        }
    }

    #[test]
    fn everyday_dictations_decide_and_the_processor_must_be_clearly_faster_at_both() {
        // This PC before the DirectML fix: the processor 70 % and 37 % faster
        // at 3 and 11 s, slower at 36 s — the long one doesn't count.
        assert_eq!(pick(&[run(Device::Gpu, [441, 630, 1244], true), run(Device::Cpu, [131, 396, 1620], true)]), Some(Device::Cpu));
        // Faster, but not by 20 % at one everyday length: the graphics card.
        assert_eq!(pick(&[run(Device::Gpu, [300, 450, 900], true), run(Device::Cpu, [80, 400, 300], true)]), Some(Device::Gpu));
        // 15 % faster at both: not clearly.
        assert_eq!(pick(&[run(Device::Gpu, [100, 200, 400], true), run(Device::Cpu, [85, 170, 300], true)]), Some(Device::Gpu));
        // Exactly 20 % less time counts.
        assert_eq!(pick(&[run(Device::Gpu, [100, 200, 400], true), run(Device::Cpu, [80, 160, 9_000], true)]), Some(Device::Cpu));
        // The graphics card faster: of course.
        assert_eq!(pick(&[run(Device::Gpu, [60, 90, 200], true), run(Device::Cpu, [80, 150, 900], true)]), Some(Device::Gpu));
        // A long length that failed doesn't matter.
        let mut no_long = run(Device::Cpu, [80, 150, 0], true);
        no_long.lengths[2].warm_ms = None;
        assert_eq!(pick(&[run(Device::Gpu, [300, 450, 900], true), no_long]), Some(Device::Cpu));
    }

    #[test]
    fn the_time_for_your_dictations_weighs_the_two_everyday_lengths_by_your_history() {
        let cpu = run(Device::Cpu, [100, 400, 1_600], true);
        assert_eq!(cpu.everyday(0.5), Some(250));
        assert_eq!(cpu.everyday(1.0), Some(100));
        assert_eq!(cpu.everyday(0.25), Some(325));
        // Spoken lengths nearer 3 s than 11 s count as short ones.
        assert_eq!(short_share_of(&[2.0, 4.0, 9.0, 20.0], 7.0), 0.5);
        assert_eq!(short_share_of(&[2.0, 3.0, 5.0], 7.0), 1.0);
        // No history: half and half.
        assert_eq!(short_share_of(&[], 7.0), 0.5);
        let mut untimed = cpu.clone();
        untimed.lengths.clear();
        assert_eq!(untimed.everyday(0.5), None);
    }

    #[test]
    fn the_first_transcription_after_loading_never_counts_against_the_graphics_card() {
        let mut gpu = run(Device::Gpu, [90, 120, 300], true);
        gpu.first_ms = 2_400; // DirectML building its graph
        let cpu = run(Device::Cpu, [80, 110, 400], true);
        assert_eq!(pick(&[gpu, cpu]), Some(Device::Gpu));
        // Nor is the graphics card ever dropped for it.
        assert!(!too_slow(200, 2_400));
        assert!(too_slow(6_000, 600));
    }

    #[test]
    fn a_device_that_failed_or_got_the_words_wrong_is_out() {
        // The graphics card's words wrong: the processor.
        assert_eq!(pick(&[run(Device::Gpu, [50, 60, 100], false), run(Device::Cpu, [80, 150, 300], true)]), Some(Device::Cpu));
        // The processor failed, or wasn't timed (too slow, Whisper skipped).
        let mut failed = run(Device::Cpu, [10, 10, 10], true);
        failed.error = Some("Couldn't load".into());
        assert_eq!(pick(&[run(Device::Gpu, [300, 450, 900], true), failed]), Some(Device::Gpu));
        let mut untimed = run(Device::Cpu, [10, 10, 10], true);
        untimed.lengths.clear();
        assert_eq!(pick(&[run(Device::Gpu, [300, 450, 900], true), untimed]), Some(Device::Gpu));
        // An everyday length that failed mid-way.
        let mut partial = run(Device::Gpu, [300, 450, 900], true);
        partial.lengths[1].warm_ms = None;
        assert_eq!(pick(&[partial, run(Device::Cpu, [280, 440, 880], true)]), Some(Device::Cpu));
        // Nothing qualifies.
        assert_eq!(pick(&[run(Device::Gpu, [1, 1, 1], false)]), None);
        assert_eq!(pick(&[]), None);
    }

    #[test]
    fn the_words_are_compared_without_case_or_punctuation_a_word_in_twenty_aside() {
        assert!(agrees(Some("Hi Imogen, could you send Sam the plan?"), "hi imogen could you send sam the plan"));
        assert!(agrees(Some("I'd like it."), "I\u{2019}d like it"));
        // One word in a short sentence may differ (the devices round
        // differently); two may not.
        assert!(agrees(Some("Hi Imogen, could you send Sam the plan?"), "Hi Imogen, could you send Pam the plan?"));
        assert!(!agrees(Some("Hi Imogen, could you send Sam the plan?"), "Hi Imogen, would you send Pam the plan?"));
        assert!(!agrees(Some("Hi Imogen, could you send Sam the plan?"), ""));
        // No reference (the processor didn't run): any words will do.
        assert!(agrees(None, "something"));
        assert!(!agrees(None, " … "));
        assert_eq!(word_edits(&words("a b c d"), &words("a x c")), 2);
    }

    #[test]
    fn whisper_on_the_processor_is_tried_only_beside_a_slow_graphics_card() {
        // This PC's large-v3: 8.3 s at first (Vulkan building its shaders),
        // 0.9 s warm — judged by the warm run.
        assert!(skip_cpu_whisper(886));
        assert!(!skip_cpu_whisper(4_800));
    }

    #[test]
    fn the_median_of_the_warm_runs() {
        assert_eq!(median(&[300, 100, 200]), Some(200));
        assert_eq!(median(&[100, 400]), Some(400));
        assert_eq!(median(&[]), None);
    }

    #[test]
    fn runs_on_follows_the_setting_then_the_measurement() {
        assert_eq!(device_choice("cpu", true, Some(Device::Gpu), Device::Gpu), Device::Cpu);
        assert_eq!(device_choice("gpu", true, Some(Device::Cpu), Device::Cpu), Device::Gpu);
        assert_eq!(device_choice("auto", true, Some(Device::Cpu), Device::Gpu), Device::Cpu);
        // Not measured yet: the graphics card where there's a real one…
        assert_eq!(device_choice("auto", true, None, Device::Gpu), Device::Gpu);
        assert_eq!(device_choice("", true, None, Device::Gpu), Device::Gpu);
        // …else the processor (no driver, or integrated graphics:
        // `hardware::speech_device_before_measuring`).
        assert_eq!(device_choice("auto", true, None, Device::Cpu), Device::Cpu);
        // The old GPU switch off: the processor.
        assert_eq!(device_choice("auto", false, Some(Device::Gpu), Device::Gpu), Device::Cpu);
    }

    #[test]
    fn a_measurement_fits_the_same_method_model_files_and_drivers_only() {
        let pc = "10de:2c05 NVIDIA GeForce RTX 5070 Ti 32.0.15.8180";
        let m = Measured {
            method: METHOD,
            model_print: "670523:1782000000".into(),
            pc_print: pc.into(),
            at: 0,
            cpu_load: Some(4),
            short_share: 0.5,
            runs: vec![],
            pick: Device::Gpu,
        };
        assert!(fits(&m, "670523:1782000000", pc));
        // A new download, or a new driver.
        assert!(!fits(&m, "670524:1782000999", pc));
        assert!(!fits(&m, "670523:1782000000", "10de:2c05 NVIDIA GeForce RTX 5070 Ti 32.0.15.9001"));
        // Measured the old way (one 8-second clip): measured again.
        let old: Measured = serde_json::from_str(&format!(
            r#"{{"modelPrint":"670523:1782000000","pcPrint":"{pc}","at":1,"clipMs":8230,
               "runs":[{{"device":"cpu","loadMs":1,"firstMs":2,"warmMs":148,"runsMs":[148],"agrees":true}}],"pick":"cpu"}}"#
        ))
        .unwrap();
        assert_eq!(old.method, 0);
        assert_eq!(old.short_share, 0.5);
        assert!(!fits(&old, "670523:1782000000", pc));
        // …or judged at every length on the old DirectML path (method 2).
        let mut two = m.clone();
        two.method = 2;
        assert!(!fits(&two, "670523:1782000000", pc));
    }

    #[test]
    fn the_bundled_clip_cuts_into_three_dictation_lengths_at_pauses() {
        let audio = clip();
        let secs = audio.len() as f32 / 16_000.0;
        assert!((30.0..60.0).contains(&secs), "{secs} s");
        assert!(audio.iter().any(|s| s.abs() > 0.1), "silent");
        let ends = cuts(&audio);
        assert_eq!(ends.len(), 3);
        let short = ends[0] as f32 / 16_000.0;
        let typical = ends[1] as f32 / 16_000.0;
        assert!((1.5..4.5).contains(&short), "short {short} s");
        assert!((8.5..11.5).contains(&typical), "typical {typical} s");
        assert_eq!(ends[2], audio.len());
        // Each cut in a pause: the 20 ms around it all but silent.
        for &end in &ends[..2] {
            let around = &audio[end - 160..end + 160];
            assert!(around.iter().all(|s| s.abs() < 0.01), "cut at {end} isn't in a pause");
        }
    }

    #[test]
    fn every_warm_run_is_a_new_length() {
        let piece = vec![0.5f32; 16_000];
        let a = padded(&piece, PAD_STEP_MS);
        let b = padded(&piece, 2 * PAD_STEP_MS);
        assert_eq!(a.len(), 16_000 + 2_560);
        assert_ne!(a.len(), b.len());
        assert_eq!(&a[..16_000], &piece[..]);
        assert!(a[16_000..].iter().all(|s| *s == 0.0));
    }

    #[test]
    fn the_stub_engine_is_timed_on_each_device_at_each_length_and_a_dictation_stops_it() {
        // The stub engine loads from a model path it never reads, so a
        // folder with the model's name stands in.
        let dir = std::env::temp_dir().join(format!("yap-stt-speed-{}", std::process::id()));
        let model = dir.join("models").join(stt::model_filename("parakeet-tdt-0.6b-v3"));
        std::fs::create_dir_all(&model).unwrap();
        std::fs::write(model.join("encoder-model.int8.onnx"), b"x").unwrap();
        let audio = clip();
        let call = Call::default();
        let runs = measure(&dir, "parakeet-tdt-0.6b-v3", &[Device::Gpu, Device::Cpu], &audio, &call, &|| None).unwrap();
        assert_eq!(runs.len(), 2);
        for r in &runs {
            assert!(r.error.is_none(), "{r:?}");
            assert_eq!(r.lengths.len(), LENGTHS.len());
            for t in &r.lengths {
                assert_eq!(t.runs_ms.len(), WARM_RUNS);
                assert!(t.warm_ms.is_some());
            }
            assert!(r.lengths[0].clip_ms < r.lengths[1].clip_ms && r.lengths[1].clip_ms < r.lengths[2].clip_ms);
            assert!(r.agrees, "the stub says the same on both");
        }
        assert!(pick(&runs).is_some());
        // A dictation starting stops it.
        let interrupted = measure(&dir, "parakeet-tdt-0.6b-v3", &[Device::Gpu], &audio, &call, &|| Some("a dictation is running"));
        assert_eq!(interrupted, Err(Stopped::Interrupted("a dictation is running")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_processor_load_reads_as_a_percentage() {
        if let Some(load) = cpu_load(Duration::from_millis(200)) {
            assert!(load <= 100);
        }
    }

    /// The bundled clip, made with Kokoro (Apache-2.0, af_bella) on this PC:
    /// `YAP_TTS_MODELS=<a copy of models/tts> cargo test --features engines
    /// stt_speed::tests::make_clip -- --ignored --nocapture`. Each sentence
    /// of [`PASSAGE`] said on its own, a pause between them, a little
    /// silence either side (as a dictation's pre-roll has).
    #[cfg(feature = "engines")]
    #[test]
    #[ignore]
    fn make_clip() {
        let mut voice = crate::tts::kokoro::open_real("af_bella", 1.0).expect("Kokoro installed under YAP_TTS_MODELS");
        let edge = vec![0f32; 4_800];
        let pause = vec![0f32; 6_400];
        let mut audio = edge.clone();
        for (i, sentence) in PASSAGE.iter().enumerate() {
            let pcm = voice.synthesize(sentence).unwrap();
            audio.extend(crate::pipeline::resample_linear(&pcm.samples, pcm.sample_rate, 16_000));
            audio.extend(if i + 1 < PASSAGE.len() { &pause } else { &edge });
            println!("after sentence {}: {:.2} s", i + 1, audio.len() as f32 / 16_000.0);
        }
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets").join("stt-speed-clip.wav");
        write_wav(&path, &audio);
        let ends: Vec<String> = cuts(&audio).iter().map(|e| format!("{:.2} s", *e as f32 / 16_000.0)).collect();
        println!("{}: {:.2} s, cut at {ends:?}", path.display(), audio.len() as f32 / 16_000.0);
    }

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

    /// The numbers for the report: Yap's own benchmark (what it picks),
    /// then a closer look — per device the load and first dictation, and
    /// per length the warm median at a new length (what a dictation pays)
    /// and at the same length again (DirectML's graph reused), the devices
    /// interleaved (other work on the PC slows each alike). `YAP_SPEED_DATA`
    /// = a data folder whose `models/` holds copies of the models (never the
    /// real one), `YAP_SPEED_MODELS` = ids, `,`-separated;
    /// `YAP_SPEED_ROUNDS` (default 7).
    /// `cargo test --release --features engines stt_speed::tests::bench_this_pc -- --ignored --nocapture --test-threads=1`
    #[cfg(feature = "engines")]
    #[test]
    #[ignore]
    fn bench_this_pc() {
        let data = PathBuf::from(std::env::var_os("YAP_SPEED_DATA").expect("YAP_SPEED_DATA"));
        let models = std::env::var("YAP_SPEED_MODELS").unwrap_or_else(|_| "parakeet-tdt-0.6b-v3,parakeet-tdt-0.6b-v2,large".into());
        let rounds: usize = std::env::var("YAP_SPEED_ROUNDS").ok().and_then(|r| r.parse().ok()).unwrap_or(7);
        let audio = clip();
        let pieces: Vec<Vec<f32>> = cuts(&audio).into_iter().map(|e| audio[..e].to_vec()).collect();
        let call = Call::default();
        println!("PC: {}", pc_print());
        println!("Processor load before: {:?} %", cpu_load(Duration::from_secs(1)));
        for model in models.split(',').map(str::trim).filter(|m| !m.is_empty()) {
            println!("\n== {model} ==");
            // What Yap itself decides.
            match measure(&data, model, &[Device::Gpu, Device::Cpu], &audio, &call, &|| None) {
                Ok(runs) => {
                    for r in &runs {
                        let warm: Vec<String> =
                            r.lengths.iter().map(|t| format!("{:.1} s: {:?} ms {:?}", t.clip_ms as f32 / 1000.0, t.warm_ms, t.runs_ms)).collect();
                        println!(
                            "  Yap's benchmark {:>8}: load {} ms, first {} ms, warm [{}], agrees {}{}{}",
                            r.device.runtime(model),
                            r.load_ms,
                            r.first_ms,
                            warm.join("; "),
                            r.agrees,
                            if r.skipped { ", skipped" } else { "" },
                            r.error.as_deref().map(|e| format!(", {e}")).unwrap_or_default()
                        );
                    }
                    let everyday: Vec<String> = runs.iter().map(|r| format!("{} {:?} ms", r.device.runtime(model), r.everyday(0.5))).collect();
                    println!("  → Automatic picks: {:?} (everyday dictations, half and half: {})", pick(&runs).map(|d| d.runtime(model)), everyday.join(", "));
                }
                Err(e) => println!("  Yap's benchmark failed: {e:?}"),
            }
            // The closer look.
            let mut engines: Vec<(Device, SttAdapter)> = Vec::new();
            let mut gpu_warm = None;
            for device in [Device::Gpu, Device::Cpu] {
                if device == Device::Cpu && stt::is_whisper_model(model) && gpu_warm.is_some_and(skip_cpu_whisper) {
                    println!("  {:>8}: not tried (Whisper beside a quick graphics card)", device.runtime(model));
                    continue;
                }
                let t = Instant::now();
                let e = stt::create_stt_engine_on(&data, model, device).expect("load");
                let load = t.elapsed().as_millis() as u64;
                let (first, text) = transcribe_timed(&e, &padded(&pieces[TYPICAL], TAIL_MS), &call);
                println!(
                    "  {:>8}: load {load} ms, first dictation after loading ({:.1} s) {first} ms ({} words) → first dictation {} ms",
                    device.runtime(model),
                    pieces[TYPICAL].len() as f32 / 16_000.0,
                    text.map(|t| words(&t).len()).unwrap_or(0),
                    load + first
                );
                if device == Device::Gpu {
                    let (warm, _) = transcribe_timed(&e, &padded(&pieces[TYPICAL], TAIL_MS + PAD_STEP_MS / 2), &call);
                    println!("  {:>8}: its second run {warm} ms", device.runtime(model));
                    gpu_warm = Some(warm);
                } else if gpu_warm.is_some_and(|g| too_slow(first, g)) {
                    println!("  {:>8}: far slower, not timed further", device.runtime(model));
                    continue;
                }
                engines.push((device, e));
            }
            // (length, device) → new-length runs, same-length-again runs.
            let mut new: BTreeMap<(usize, &str), Vec<u64>> = BTreeMap::new();
            let mut again: BTreeMap<(usize, &str), Vec<u64>> = BTreeMap::new();
            let mut texts: BTreeMap<(usize, &str), String> = BTreeMap::new();
            for round in 0..rounds {
                for (li, piece) in pieces.iter().enumerate() {
                    let audio = padded(piece, TAIL_MS + (round as u64 + 1) * PAD_STEP_MS);
                    for (device, e) in &engines {
                        let (ms, out) = transcribe_timed(e, &audio, &call);
                        new.entry((li, device.as_str())).or_default().push(ms);
                        texts.entry((li, device.as_str())).or_insert_with(|| out.unwrap_or_default());
                        let (ms, _) = transcribe_timed(e, &audio, &call);
                        again.entry((li, device.as_str())).or_default().push(ms);
                    }
                }
            }
            let stat = |v: &Vec<u64>| {
                let mut s = v.clone();
                s.sort_unstable();
                format!("{} ms ({}–{})", median(v).unwrap(), s[0], s[s.len() - 1])
            };
            for (li, piece) in pieces.iter().enumerate() {
                let mut line = format!("  {:>7} {:>5.1} s:", LENGTHS[li].0, piece.len() as f32 / 16_000.0);
                for (device, _) in &engines {
                    line.push_str(&format!(
                        "  {} new length {}, same again {}",
                        device.runtime(model),
                        stat(&new[&(li, device.as_str())]),
                        stat(&again[&(li, device.as_str())])
                    ));
                }
                if engines.len() == 2 {
                    let g = median(&new[&(li, "gpu")]).unwrap() as f64;
                    let c = median(&new[&(li, "cpu")]).unwrap() as f64;
                    line.push_str(&format!(
                        "  | processor {:+.0} % time, words agree {}, same text {}",
                        (c / g - 1.0) * 100.0,
                        agrees(Some(&texts[&(li, "cpu")]), &texts[&(li, "gpu")]),
                        texts[&(li, "cpu")] == texts[&(li, "gpu")]
                    ));
                }
                println!("{line}");
            }
        }
    }

    /// Where the time goes in one Parakeet transcription, stage by stage —
    /// the preprocessor (`nemo128.onnx`), the encoder, and the decoder loop
    /// (one `decoder_joint` run per frame and per token, as transcribe-rs
    /// runs it) — each on DirectML or the processor, and what a split costs.
    /// The same steps transcribe-rs takes, with ort directly. `YAP_SPEED_DATA`
    /// as for `bench_this_pc`; `YAP_SPEED_MODELS` (default v3);
    /// `YAP_SPEED_ROUNDS` (default 5).
    /// `cargo test --release --features engines stt_speed::tests::profile_parakeet -- --ignored --nocapture --test-threads=1`
    #[cfg(feature = "engines")]
    #[test]
    #[ignore]
    fn profile_parakeet() {
        use ort::session::builder::GraphOptimizationLevel;
        use ort::session::{Session, SessionInputValue};
        use ort::value::Tensor;

        fn open(path: &Path, gpu: bool, threads: Option<usize>) -> Session {
            open_with(path, gpu, threads, &[])
        }
        fn open_with(path: &Path, gpu: bool, threads: Option<usize>, entries: &[(&str, &str)]) -> Session {
            let e = |e: ort::Error<ort::session::builder::SessionBuilder>| e.to_string();
            let mut b = Session::builder().unwrap().with_optimization_level(GraphOptimizationLevel::Level3).map_err(e).unwrap();
            for (k, v) in entries {
                b = b.with_config_entry(k, v).map_err(e).unwrap();
            }
            if let Some(n) = threads {
                b = b.with_intra_threads(n).map_err(e).unwrap();
            }
            b = b.with_parallel_execution(!gpu).map_err(e).unwrap();
            if gpu {
                b = b.with_memory_pattern(false).map_err(e).unwrap();
                b = b.with_execution_providers([ort::ep::DirectML::default().build(), ort::ep::CPU::default().build()]).map_err(e).unwrap();
            } else {
                b = b.with_execution_providers([ort::ep::CPU::default().build()]).map_err(e).unwrap();
            }
            b.commit_from_file(path).unwrap()
        }
        fn input<T: ort::value::PrimitiveTensorElementType + std::fmt::Debug + 'static>(name: &str, v: Tensor<T>) -> (String, SessionInputValue<'static>) {
            (name.to_string(), v.into())
        }
        #[derive(Default, Clone, Copy)]
        struct Times {
            prep: u64,
            enc: u64,
            dec: u64,
            runs: u64,
        }
        struct Model {
            vocab: usize,
            blank: i32,
            states: [Vec<i64>; 2],
        }
        /// One transcription: the token ids and each stage's time (µs).
        fn run(m: &Model, prep: &mut Session, enc: &mut Session, dec: &mut Session, audio: &[f32]) -> (Vec<i32>, Times) {
            run_padded(m, prep, enc, dec, audio, 0)
        }
        /// …with the waveform zero-padded to `pad_to` samples (its length
        /// still the audio's).
        fn run_padded(m: &Model, prep: &mut Session, enc: &mut Session, dec: &mut Session, audio: &[f32], pad_to: usize) -> (Vec<i32>, Times) {
            let mut t = Times::default();
            // transcribe-rs puts 250 ms of silence before the audio.
            let mut wave = vec![0f32; 4_000];
            wave.extend_from_slice(audio);
            let n = wave.len() as i64;
            if wave.len() < pad_to {
                wave.resize(pad_to, 0.0);
            }
            let total = wave.len() as i64;
            let started = Instant::now();
            let (features, fshape, flen) = {
                let out = prep
                    .run(vec![input("waveforms", Tensor::from_array(([1i64, total], wave)).unwrap()), input("waveforms_lens", Tensor::from_array(([1i64], vec![n])).unwrap())])
                    .unwrap();
                let (shape, f) = out["features"].try_extract_tensor::<f32>().unwrap();
                let (_, l) = out["features_lens"].try_extract_tensor::<i64>().unwrap();
                (f.to_vec(), shape.to_vec(), l.to_vec())
            };
            t.prep = started.elapsed().as_micros() as u64;
            let started = Instant::now();
            let (encoded, eshape, elen) = {
                let out = enc
                    .run(vec![input("audio_signal", Tensor::from_array((fshape, features)).unwrap()), input("length", Tensor::from_array(([1i64], flen)).unwrap())])
                    .unwrap();
                let (shape, e) = out["outputs"].try_extract_tensor::<f32>().unwrap();
                let (_, l) = out["encoded_lengths"].try_extract_tensor::<i64>().unwrap();
                (e.to_vec(), shape.to_vec(), l[0] as usize)
            };
            t.enc = started.elapsed().as_micros() as u64;
            // [1, D, T]: frame t is every T-th value from t.
            let (d, frames) = (eshape[1] as usize, eshape[2] as usize);
            let started = Instant::now();
            let size = |s: &[i64]| s.iter().map(|&x| x.max(1) as usize).product::<usize>();
            let mut state = [vec![0f32; size(&m.states[0])], vec![0f32; size(&m.states[1])]];
            let shape = |s: &[i64]| vec![s[0], 1, s[2]];
            let (mut tokens, mut frame, mut emitted) = (Vec::new(), 0usize, 0usize);
            while frame < elen.min(frames) {
                let column: Vec<f32> = (0..d).map(|k| encoded[k * frames + frame]).collect();
                let target = tokens.last().copied().unwrap_or(m.blank);
                let out = dec
                    .run(vec![
                        input("encoder_outputs", Tensor::from_array(([1i64, d as i64, 1], column)).unwrap()),
                        input("targets", Tensor::from_array(([1i64, 1], vec![target])).unwrap()),
                        input("target_length", Tensor::from_array(([1i64], vec![1i32])).unwrap()),
                        input("input_states_1", Tensor::from_array((shape(&m.states[0]), state[0].clone())).unwrap()),
                        input("input_states_2", Tensor::from_array((shape(&m.states[1]), state[1].clone())).unwrap()),
                    ])
                    .unwrap();
                t.runs += 1;
                let (_, logits) = out["outputs"].try_extract_tensor::<f32>().unwrap();
                let logits = &logits[..m.vocab.min(logits.len())];
                let token = logits
                    .iter()
                    .enumerate()
                    .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                    .map(|(i, _)| i as i32)
                    .unwrap_or(m.blank);
                if token != m.blank {
                    state = [
                        out["output_states_1"].try_extract_tensor::<f32>().unwrap().1.to_vec(),
                        out["output_states_2"].try_extract_tensor::<f32>().unwrap().1.to_vec(),
                    ];
                    tokens.push(token);
                    emitted += 1;
                }
                if token == m.blank || emitted == 10 {
                    frame += 1;
                    emitted = 0;
                }
            }
            t.dec = started.elapsed().as_micros() as u64;
            (tokens, t)
        }

        let data = PathBuf::from(std::env::var_os("YAP_SPEED_DATA").expect("YAP_SPEED_DATA"));
        let models = std::env::var("YAP_SPEED_MODELS").unwrap_or_else(|_| "parakeet-tdt-0.6b-v3".into());
        let rounds: usize = std::env::var("YAP_SPEED_ROUNDS").ok().and_then(|r| r.parse().ok()).unwrap_or(5);
        let audio = clip();
        let pieces: Vec<Vec<f32>> = cuts(&audio).into_iter().map(|e| audio[..e].to_vec()).collect();
        println!("Processor load before: {:?} %", cpu_load(Duration::from_secs(1)));
        for model in models.split(',').map(str::trim).filter(|m| !m.is_empty()) {
            let dir = data.join("models").join(stt::model_filename(model));
            let vocab = std::fs::read_to_string(dir.join("vocab.txt")).unwrap();
            let blank = vocab.lines().position(|l| l.starts_with("<blk>")).unwrap() as i32;
            let mut sessions: BTreeMap<&str, Session> = BTreeMap::new();
            for (name, file, gpu, threads) in [
                ("prep-gpu", "nemo128.onnx", true, None),
                ("prep-cpu", "nemo128.onnx", false, None),
                ("enc-gpu", "encoder-model.int8.onnx", true, None),
                ("enc-cpu", "encoder-model.int8.onnx", false, None),
                ("dec-gpu", "decoder_joint-model.int8.onnx", true, None),
                ("dec-cpu", "decoder_joint-model.int8.onnx", false, None),
                ("dec-cpu1", "decoder_joint-model.int8.onnx", false, Some(1)),
            ] {
                let t = Instant::now();
                sessions.insert(name, open(&dir.join(file), gpu, threads));
                println!("  {name}: loaded in {} ms", t.elapsed().as_millis());
            }
            let state_shape = |s: &Session, name: &str| -> Vec<i64> {
                let i = s.inputs().iter().find(|i| i.name() == name).unwrap();
                i.dtype().tensor_shape().unwrap().to_vec()
            };
            let m = Model {
                vocab: vocab.lines().count(),
                blank,
                states: [state_shape(&sessions["dec-cpu"], "input_states_1"), state_shape(&sessions["dec-cpu"], "input_states_2")],
            };
            // The words: the old DirectML path (everything on DirectML, each
            // length as it is) against the new one (the encoder alone on
            // DirectML at its planned length, as vendor/transcribe-rs runs it)
            // and the processor.
            if std::env::var_os("YAP_PROFILE_SAME").is_some() {
                let words: Vec<&str> = vocab.lines().map(|l| l.split(' ').next().unwrap_or("")).collect();
                let text = |tokens: &[i32]| -> String {
                    tokens.iter().map(|&t| words.get(t as usize).copied().unwrap_or("?")).collect::<String>().replace('\u{2581}', " ").trim().to_string()
                };
                let mut planned = open(&dir.join("encoder-model.int8.onnx"), true, None);
                let [mut pg, mut pc, mut eg, mut ec, mut dg, mut dc] =
                    ["prep-gpu", "prep-cpu", "enc-gpu", "enc-cpu", "dec-gpu", "dec-cpu"].map(|k| sessions.remove(k).unwrap());
                let bucket = 16_000 * 30 + 4_000;
                let _ = run_padded(&m, &mut pc, &mut planned, &mut dc, &[0.0; 16_000], bucket);
                for round in 0..3u64 {
                    for (li, piece) in pieces.iter().enumerate() {
                        let audio = padded(piece, TAIL_MS + round * PAD_STEP_MS);
                        let old = run(&m, &mut pg, &mut eg, &mut dg, &audio).0;
                        let new = run_padded(&m, &mut pc, &mut planned, &mut dc, &audio, bucket).0;
                        let cpu = run(&m, &mut pc, &mut ec, &mut dc, &audio).0;
                        println!(
                            "  {model} {} round {round}: new = old {}, new = processor {}, old = processor {}",
                            LENGTHS[li].0,
                            new == old,
                            new == cpu,
                            old == cpu
                        );
                        if new != old || old != cpu {
                            println!("    old:       {}\n    new:       {}\n    processor: {}", text(&old), text(&new), text(&cpu));
                        }
                    }
                }
                continue;
            }
            // Fixed lengths: the waveform padded to a bucket, so DirectML's
            // encoder always gets the shape it built its graph for; and graph
            // fusion off.
            if std::env::var_os("YAP_PROFILE_BUCKETS").is_some() {
                let [mut ps, mut cs, mut ds] = ["prep-cpu", "enc-cpu", "dec-cpu"].map(|k| sessions.remove(k).unwrap());
                let reference: Vec<Vec<i32>> =
                    pieces.iter().map(|p| run(&m, &mut ps, &mut cs, &mut ds, &padded(p, TAIL_MS)).0).collect();
                let enc_path = dir.join("encoder-model.int8.onnx");
                for secs in [12usize, 16, 20, 30, 40] {
                    let pad_to = 4_000 + secs * 16_000;
                    let mut es = open(&enc_path, true, None);
                    let t = Instant::now();
                    let _ = run_padded(&m, &mut ps, &mut es, &mut ds, &pieces[0], pad_to);
                    let first = t.elapsed().as_millis();
                    let mut line = format!("  bucket {secs:>2} s: first {first} ms;");
                    for (li, piece) in pieces.iter().enumerate() {
                        if piece.len() + 4_000 + 16_000 > pad_to {
                            continue;
                        }
                        let mut times = Vec::new();
                        let mut same = true;
                        for round in 0..5u64 {
                            let (tokens, tm) = run_padded(&m, &mut ps, &mut es, &mut ds, &padded(piece, TAIL_MS + (round + 1) * PAD_STEP_MS), pad_to);
                            times.push(tm.enc / 1000);
                            if round == 0 {
                                same = tokens == reference[li];
                            }
                        }
                        line.push_str(&format!(" {} encoder {:?} ms, same tokens {same};", LENGTHS[li].0, times));
                    }
                    println!("{line}");
                }
                let mut es = open_with(&enc_path, true, None, &[("ep.dml.disable_graph_fusion", "1")]);
                for (label, pad) in [("A", 100u64), ("A", 100), ("B", 200), ("B", 200), ("C", 300), ("A", 100)] {
                    let (_, t) = run(&m, &mut ps, &mut es, &mut ds, &padded(&pieces[TYPICAL], pad));
                    println!("  fusion off, shape {label}: encoder {} ms", t.enc / 1000);
                }
                return;
            }
            // How DirectML's encoder takes lengths it has had, and new ones.
            if std::env::var_os("YAP_PROFILE_SHAPES").is_some() {
                let [mut ps, mut es, mut ds] = ["prep-cpu", "enc-gpu", "dec-cpu"].map(|k| sessions.remove(k).unwrap());
                for (label, pad) in [("A", 100), ("A", 100), ("A", 100), ("B", 200), ("B", 200), ("A", 100), ("C", 300), ("C", 300), ("B", 200), ("A", 100), ("D", 400), ("A", 100)] {
                    let (_, t) = run(&m, &mut ps, &mut es, &mut ds, &padded(&pieces[TYPICAL], pad));
                    println!("  shape {label}: encoder {} ms", t.enc / 1000);
                }
                for (label, pad) in [("A", 100), ("A", 100)] {
                    std::thread::sleep(Duration::from_millis(1500));
                    let (_, t) = run(&m, &mut ps, &mut es, &mut ds, &padded(&pieces[TYPICAL], pad));
                    println!("  shape {label} after 1.5 s idle: encoder {} ms", t.enc / 1000);
                }
                for k in 0..4 {
                    let mut cs = sessions.remove("enc-cpu").unwrap();
                    let _ = run(&m, &mut ps, &mut cs, &mut ds, &padded(&pieces[TYPICAL], 100));
                    sessions.insert("enc-cpu", cs);
                    let (_, t) = run(&m, &mut ps, &mut es, &mut ds, &padded(&pieces[TYPICAL], 100));
                    println!("  shape A after a processor encoder run ({k}): encoder {} ms", t.enc / 1000);
                }
                sessions.insert("prep-cpu", ps);
                sessions.insert("enc-gpu", es);
                sessions.insert("dec-cpu", ds);
                return;
            }
            // (label, preprocessor, encoder, decoder)
            let configs: [(&str, &str, &str, &str); 5] = [
                ("all DirectML (today)", "prep-gpu", "enc-gpu", "dec-gpu"),
                ("all processor", "prep-cpu", "enc-cpu", "dec-cpu"),
                ("encoder DirectML, rest processor", "prep-cpu", "enc-gpu", "dec-cpu"),
                ("encoder DirectML, decoder 1 thread", "prep-cpu", "enc-gpu", "dec-cpu1"),
                ("prep+encoder DirectML, decoder processor", "prep-gpu", "enc-gpu", "dec-cpu"),
            ];
            // First runs (graph building) apart.
            for (label, p, e, d) in configs {
                let [mut ps, mut es, mut ds] = [p, e, d].map(|k| sessions.remove(k).unwrap());
                let t = Instant::now();
                let (_, times) = run(&m, &mut ps, &mut es, &mut ds, &padded(&pieces[TYPICAL], TAIL_MS));
                println!(
                    "  first run, {label}: {} ms (prep {} / enc {} / dec {} ms, {} decoder runs)",
                    t.elapsed().as_millis(),
                    times.prep / 1000,
                    times.enc / 1000,
                    times.dec / 1000,
                    times.runs
                );
                sessions.insert(p, ps);
                sessions.insert(e, es);
                sessions.insert(d, ds);
            }
            let mut reference: Vec<Vec<i32>> = vec![Vec::new(); pieces.len()];
            let mut all: BTreeMap<(usize, &str), Vec<(u64, Times)>> = BTreeMap::new();
            for round in 0..rounds {
                for (li, piece) in pieces.iter().enumerate() {
                    let audio = padded(piece, TAIL_MS + (round as u64 + 1) * PAD_STEP_MS);
                    for (label, p, e, d) in configs {
                        let [mut ps, mut es, mut ds] = [p, e, d].map(|k| sessions.remove(k).unwrap());
                        let t = Instant::now();
                        let (tokens, times) = run(&m, &mut ps, &mut es, &mut ds, &audio);
                        let total = t.elapsed().as_micros() as u64;
                        sessions.insert(p, ps);
                        sessions.insert(e, es);
                        sessions.insert(d, ds);
                        if round == 0 {
                            if reference[li].is_empty() {
                                reference[li] = tokens.clone();
                            } else if reference[li] != tokens {
                                println!("  ! {label} at {} gave other tokens", LENGTHS[li].0);
                            }
                        }
                        all.entry((li, label)).or_default().push((total, times));
                    }
                }
            }
            println!("\n== {model}: median ms (preprocessor / encoder / decoder loop, decoder runs) ==");
            for (li, piece) in pieces.iter().enumerate() {
                println!("  {} ({:.1} s, {} tokens):", LENGTHS[li].0, piece.len() as f32 / 16_000.0, reference[li].len());
                for (label, ..) in configs {
                    let v = &all[&(li, label)];
                    let med = |f: &dyn Fn(&(u64, Times)) -> u64| median(&v.iter().map(f).collect::<Vec<_>>()).unwrap();
                    let runs = v[0].1.runs;
                    let dec = med(&|x| x.1.dec);
                    println!(
                        "    {label:>42}: {:>5.0} ms  = {:>4.0} / {:>5.0} / {:>5.0}  ({runs} runs, {:.2} ms each)",
                        med(&|x| x.0) as f64 / 1000.0,
                        med(&|x| x.1.prep) as f64 / 1000.0,
                        med(&|x| x.1.enc) as f64 / 1000.0,
                        dec as f64 / 1000.0,
                        dec as f64 / 1000.0 / runs.max(1) as f64
                    );
                    println!("{:>48} encoder runs: {:?}", "", v.iter().map(|x| x.1.enc / 1000).collect::<Vec<_>>());
                }
            }
        }
    }
}
