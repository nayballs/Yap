//! **`yap.exe bench`** (2026-10-24): how fast Yap is on this PC, in numbers
//! anyone can reproduce and share (docs/benchmarks.md). Runs before the app
//! starts, like `yap.exe mcp`, in release builds too; no window, tray or
//! hook.
//!
//! **What it times**, per *scenario* — the graphics card, the processor
//! with all its threads, the processor held to 4 and to 2 threads (an
//! **emulated** small laptop: process affinity, one thread per physical core,
//! and every runtime told the same count — `machine.rs`), an integrated GPU
//! beside it when the PC has one, and on request DirectML on Windows'
//! software renderer (what a PC without a graphics driver gets):
//! - each installed **speech model**: its load, the first dictation after it
//!   (graphs built), then warm dictations of ~3, ~11 and ~30 s (the bundled
//!   clip of `stt_speed.rs`, cut at pauses; every run a new length, as
//!   dictations are), the median kept, and each transcript's word error rate
//!   against the script the clip reads ([`stt_speed::PASSAGE`]);
//! - **AI cleanup** on the on-device model(s) installed (Yap's own
//!   llamafile server, as the sidecar runs it, on the card or on the
//!   processor with the same thread limit), through the cleanup's own code
//!   (`llm::cleanup`); and, when Yap's settings hold a cloud provider's key,
//!   three short cloud cleanups;
//! - **cold start**: a throwaway copy of Yap on an empty data folder, to its
//!   first window on screen (`cold.rs`).
//!
//! **Read only.** Models, the llamafile runtime and the settings (for a
//! cloud key) are read from Yap's data folder (`--data`, else this exe's:
//! portable or `%APPDATA%\yap`), nothing written there: the run's own
//! folder (in the temp folder) is its data folder for anything incidental,
//! and goes at the end. Out: a table on stdout and a `.txt` and `.json`
//! report (`--out`, else the current folder). The JSON names the hardware's
//! class (processor, cores, memory, graphics) and nothing else of the PC.

mod cold;
mod machine;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::privacy::net::Recorded as _;
use crate::stt::{self, Device};
use crate::stt_speed;

pub use machine::Hardware;

/// The report's shape (`schema`): bumped when a field changes meaning.
pub const SCHEMA: u32 = 1;

const USAGE: &str = "yap.exe bench [options]   (PowerShell: yap.exe bench | Out-Host)

Times speech-to-text, AI cleanup and a cold start on this PC, and writes a
table plus a JSON report you can share (docs/benchmarks.md).

  --data <folder>        Yap's data folder to read models from (default: this Yap's)
  --models <ids>         speech models, comma-separated (default: every one installed;
                         none: no speech-to-text)
  --scenarios <ids>      gpu, cpu, cpu-4t, cpu-2t, igpu-4t, warp-4t (default: all
                         that apply but warp; cpu-Nt = the processor held to N threads)
  --rounds <n>           warm runs per length (default 3)
  --lengths <secs>       dictation lengths (default 3,11,30)
  --llm <files>          on-device models (GGUFs) for cleanup (default: Yap's installed
                         Qwen2.5 1.5B and Qwen3.5 4B; none: only the cloud)
  --no-cleanup           skip AI cleanup     --no-cloud    skip the cloud call
  --no-cold-start        skip the cold start (it opens a throwaway Yap for a few seconds)
  --whisper-cpu          also time Whisper on the processor (slow: minutes)
  --patience <minutes>   how long each run waits for other apps to leave the processor
                         quiet (default 10; a run that goes ahead busy is marked *)
  --quick                1 round, 3 and 11 s, graphics card and processor only
  --out <file|folder>    where the report goes (default: the current folder)";

/// The command line.
#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    pub data: Option<PathBuf>,
    pub models: Option<Vec<String>>,
    pub scenarios: Option<Vec<String>>,
    pub rounds: usize,
    /// Dictation lengths, ms.
    pub lengths: Vec<u64>,
    pub llms: Option<Vec<PathBuf>>,
    pub cleanup: bool,
    pub cloud: bool,
    pub cold_start: bool,
    pub whisper_cpu: bool,
    /// How long each run waits for the processor to be quiet.
    pub patience: Duration,
    pub out: Option<PathBuf>,
    pub help: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            data: None,
            models: None,
            scenarios: None,
            rounds: 3,
            lengths: vec![3_000, 11_000, 30_000],
            llms: None,
            cleanup: true,
            cloud: true,
            cold_start: true,
            whisper_cpu: false,
            patience: Duration::from_secs(10 * 60),
            out: None,
            help: false,
        }
    }
}

/// A comma-separated list ("none": an empty one).
fn list(v: &str) -> Vec<String> {
    v.split(',').map(str::trim).filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("none")).map(str::to_string).collect()
}

impl Options {
    /// Pure.
    pub fn parse(args: &[String]) -> Result<Options, String> {
        let mut o = Options::default();
        let mut it = args.iter();
        while let Some(a) = it.next() {
            let mut value = |name: &str| it.next().cloned().ok_or_else(|| format!("{name} needs a value"));
            match a.as_str() {
                "-h" | "--help" | "/?" => o.help = true,
                "--data" => o.data = Some(PathBuf::from(value(a)?)),
                "--models" => o.models = Some(list(&value(a)?)),
                "--scenarios" => o.scenarios = Some(list(&value(a)?)),
                "--rounds" => o.rounds = value(a)?.parse().ok().filter(|n| *n > 0).ok_or("--rounds takes a number over 0")?,
                "--lengths" => {
                    o.lengths = list(&value(a)?)
                        .iter()
                        .map(|s| s.parse::<f64>().ok().filter(|v| *v > 0.5 && *v <= 36.0).map(|v| (v * 1000.0) as u64))
                        .collect::<Option<Vec<_>>>()
                        .ok_or("--lengths takes seconds between 0.5 and 36, comma-separated")?
                }
                "--llm" => o.llms = Some(list(&value(a)?).into_iter().map(PathBuf::from).collect()),
                "--no-cleanup" => o.cleanup = false,
                "--no-cloud" => o.cloud = false,
                "--no-cold-start" => o.cold_start = false,
                "--whisper-cpu" => o.whisper_cpu = true,
                "--patience" => {
                    let mins: f64 = value(a)?.parse().ok().filter(|m: &f64| *m >= 0.0).ok_or("--patience takes minutes")?;
                    o.patience = Duration::from_secs_f64(mins * 60.0);
                }
                "--quick" => {
                    o.rounds = 1;
                    o.lengths = vec![3_000, 11_000];
                    o.scenarios = Some(vec!["gpu".into(), "cpu".into()]);
                }
                "--out" => o.out = Some(PathBuf::from(value(a)?)),
                other => return Err(format!("unknown option {other}")),
            }
        }
        Ok(o)
    }
}

// ---- scenarios ------------------------------------------------------------------

/// Where a run happens: a device, the processor threads it may use, the
/// graphics adapter.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Scenario {
    pub id: String,
    pub label: String,
    pub device: Device,
    /// Logical processors it may use (`None`: all of them).
    pub threads: Option<u32>,
    /// The DXGI adapter DirectML runs on (`None`: Windows' default, as Yap).
    pub adapter: Option<u32>,
    /// Not this PC as it is: a limit or another adapter stands in for a
    /// smaller PC.
    pub emulated: bool,
}

/// The scenarios for `hw` (`wanted`: by id, else every one that applies
/// but `warp-4t`). Pure.
pub fn scenarios(hw: &Hardware, wanted: Option<&[String]>) -> Vec<Scenario> {
    let mut all = Vec::new();
    // The graphics card Yap runs on: the one with the most memory of its own
    // (`gpu::speech_adapter`).
    let first = hw.gpus().max_by_key(|a| a.gpu.vram_mib);
    if let Some(g) = first {
        all.push(Scenario {
            id: "gpu".into(),
            label: format!("graphics card ({})", g.gpu.name),
            device: Device::Gpu,
            threads: None,
            adapter: (g.index != 0).then_some(g.index),
            emulated: false,
        });
    }
    all.push(Scenario {
        id: "cpu".into(),
        label: format!("processor, all {} threads", hw.threads),
        device: Device::Cpu,
        threads: None,
        adapter: None,
        emulated: false,
    });
    for n in [4u32, 2] {
        if hw.threads > n {
            all.push(Scenario {
                id: format!("cpu-{n}t"),
                label: format!("processor held to {n} threads (emulated)"),
                device: Device::Cpu,
                threads: Some(n),
                adapter: None,
                emulated: true,
            });
        }
    }
    // A small GPU (integrated graphics) that isn't the default one.
    if let Some(i) = hw.gpus().find(|a| Some(a.index) != first.map(|f| f.index) && crate::gpu::is_small(&a.gpu)) {
        all.push(Scenario {
            id: "igpu-4t".into(),
            label: format!("integrated GPU ({}) + 4 threads (emulated)", i.gpu.name),
            device: Device::Gpu,
            threads: Some(4.min(hw.threads)),
            adapter: Some(i.index),
            emulated: true,
        });
    }
    if let Some(w) = hw.adapters.iter().find(|a| a.software) {
        all.push(Scenario {
            id: "warp-4t".into(),
            label: "no graphics driver: DirectML on Windows' software renderer + 4 threads (emulated)".into(),
            device: Device::Gpu,
            threads: Some(4.min(hw.threads)),
            adapter: Some(w.index),
            emulated: true,
        });
    }
    match wanted {
        Some(ids) => all.into_iter().filter(|s| ids.contains(&s.id)).collect(),
        None => all.into_iter().filter(|s| s.id != "warp-4t").collect(),
    }
}

/// Hold the process to the scenario's threads and point the runtimes at its
/// adapter until this drops.
struct Setting {
    _limit: machine::Limit,
}

impl Setting {
    fn apply(sc: &Scenario) -> Setting {
        let limit = machine::Limit::new(sc.threads);
        stt::set_cpu_threads(sc.threads.unwrap_or(0) as usize);
        stt::set_gpu_adapter(sc.adapter);
        Setting { _limit: limit }
    }
}

impl Drop for Setting {
    fn drop(&mut self) {
        stt::set_cpu_threads(0);
        stt::set_gpu_adapter(None);
    }
}

// ---- speech to text ---------------------------------------------------------------

/// One dictation length's timings.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LengthTiming {
    /// The piece's length (its silence after it aside), seconds.
    pub secs: f32,
    /// The median warm run, ms.
    pub warm_ms: Option<u64>,
    pub runs_ms: Vec<u64>,
    /// Words wrong against the script (substituted, dropped or added) per
    /// word of it.
    pub wer: Option<f32>,
}

/// A speech model in a scenario.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechRun {
    pub model: String,
    pub model_name: String,
    pub scenario: String,
    /// "DirectML", "Vulkan" or "CPU".
    pub runtime: String,
    pub load_ms: Option<u64>,
    /// The first dictation after loading (the ~11 s length).
    pub first_ms: Option<u64>,
    pub lengths: Vec<LengthTiming>,
    /// How busy other apps kept the processor just before and just after
    /// (percent; the bench idle then).
    pub load_before: Option<u32>,
    pub load_after: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl SpeechRun {
    /// Other apps were busy around it: its numbers are slower than the PC.
    pub fn busy(&self) -> bool {
        busy(self.load_before, self.load_after)
    }

    /// The warm median at about `secs` seconds.
    pub fn warm_at(&self, secs: f32) -> Option<u64> {
        self.lengths.iter().find(|l| (l.secs - secs).abs() < 2.0).and_then(|l| l.warm_ms)
    }
}

/// A first dictation slower than this skips the warm runs (the model is
/// no use at this setting; it would hold the processor for minutes).
const FIRST_BUDGET_MS: u64 = 20_000;

/// Busier than this (percent of all processors over a second, the bench
/// itself idle), other apps would slow a run down: it waits (`--patience`).
/// (A desktop with a browser, a chat app and a dev server open idles at
/// 10–25 % on the reference PC.)
pub const QUIET_PCT: u32 = 30;

/// Was a run's PC busy (other apps, before or after it)? Pure.
pub fn busy(before: Option<u32>, after: Option<u32>) -> bool {
    before.max(after).is_some_and(|l| l >= QUIET_PCT)
}

/// Wait up to `patience` for other apps to leave the processor quiet; how
/// busy it was when the run went ahead.
fn wait_quiet(patience: Duration) -> Option<u32> {
    use std::io::Write;
    let started = Instant::now();
    let mut said = false;
    loop {
        match stt_speed::cpu_load(Duration::from_secs(1)) {
            Some(load) if load >= QUIET_PCT && started.elapsed() < patience => {
                if !said {
                    print!("(other apps keep the processor {load} % busy: waiting) ");
                    let _ = std::io::stdout().flush();
                    said = true;
                }
                std::thread::sleep(Duration::from_secs(4));
            }
            load => return load,
        }
    }
}

/// Where `audio` (16 kHz) ends for each of `lengths_ms`: at the quietest
/// 20 ms within 1.5 s of it, the whole clip past its end. Pure.
pub fn cut_points(audio: &[f32], lengths_ms: &[u64]) -> Vec<usize> {
    let per_ms = 16usize;
    lengths_ms
        .iter()
        .map(|&ms| {
            let at = ms as usize * per_ms;
            if at >= audio.len() {
                return audio.len();
            }
            let slack = 1_500 * per_ms;
            let from = at.saturating_sub(slack);
            let to = (at + slack).min(audio.len());
            crate::media::quietest_frame(audio, from, to, 16_000).0.clamp(1, audio.len())
        })
        .collect()
}

/// The word error rate of `text` against the script's start it says: the
/// fewest edits over the passage's sentence prefixes, per word of that
/// prefix. Pure.
pub fn wer(text: &str) -> Option<f32> {
    let said = stt_speed::words(text);
    (1..=stt_speed::PASSAGE.len())
        .map(|k| {
            let script = stt_speed::words(&stt_speed::PASSAGE[..k].join(" "));
            (stt_speed::word_edits(&script, &said), script.len())
        })
        .min_by(|a, b| (a.0 as f32 / a.1 as f32).total_cmp(&(b.0 as f32 / b.1 as f32)))
        .map(|(edits, n)| edits as f32 / n.max(1) as f32)
}

/// Whisper models over this size aren't run on the processor unless asked
/// (`--whisper-cpu`): minutes per dictation.
const WHISPER_CPU_MAX_BYTES: u64 = 600 * 1_048_576;

fn whisper_too_big(data: &Path, model: &str) -> bool {
    stt::is_whisper_model(model)
        && std::fs::metadata(data.join("models").join(stt::model_filename(model))).map(|m| m.len()).unwrap_or(0) > WHISPER_CPU_MAX_BYTES
}

fn time_speech(data: &Path, model: &str, sc: &Scenario, pieces: &[Vec<f32>], rounds: usize, o: &Options) -> SpeechRun {
    let mut run = SpeechRun {
        model: model.to_string(),
        model_name: stt::model_name(model),
        scenario: sc.id.clone(),
        runtime: sc.device.runtime(model).to_string(),
        load_ms: None,
        first_ms: None,
        lengths: Vec::new(),
        load_before: None,
        load_after: None,
        skipped: None,
        error: None,
    };
    if sc.device == Device::Cpu && !o.whisper_cpu && whisper_too_big(data, model) {
        run.skipped = Some("a large Whisper model on the processor takes minutes (--whisper-cpu to time it)".into());
        return run;
    }
    if sc.device == Device::Gpu && sc.adapter.is_some() && stt::is_whisper_model(model) {
        run.skipped = Some("Whisper's Vulkan device isn't chosen by adapter here".into());
        return run;
    }
    run.load_before = wait_quiet(o.patience);
    timed_speech(&mut run, data, model, sc, pieces, rounds);
    run.load_after = stt_speed::cpu_load(Duration::from_secs(1));
    run
}

fn timed_speech(run: &mut SpeechRun, data: &Path, model: &str, sc: &Scenario, pieces: &[Vec<f32>], rounds: usize) {
    let _setting = Setting::apply(sc);
    let call = stt_speed::Call::default();
    let started = Instant::now();
    let engine = match stt::create_stt_engine_on(data, model, sc.device) {
        Ok(e) => e,
        Err(e) => {
            run.error = Some(format!("couldn't load: {e}"));
            return;
        }
    };
    run.load_ms = Some(started.elapsed().as_millis() as u64);
    let typical = pieces.len().min(2) - 1;
    let (ms, out) = stt_speed::transcribe_timed(&engine, &stt_speed::padded(&pieces[typical], stt_speed::TAIL_MS), &call);
    run.first_ms = Some(ms);
    if let Err(e) = out {
        run.error = Some(format!("couldn't transcribe: {e}"));
        return;
    }
    if ms > FIRST_BUDGET_MS {
        run.skipped = Some(format!("{:.0} s for one {:.0}-second dictation: not timed further", ms as f32 / 1000.0, pieces[typical].len() as f32 / 16_000.0));
        return;
    }
    let mut times: Vec<Vec<u64>> = vec![Vec::new(); pieces.len()];
    let mut texts: Vec<Option<String>> = vec![None; pieces.len()];
    for round in 0..rounds {
        for (li, piece) in pieces.iter().enumerate() {
            let audio = stt_speed::padded(piece, stt_speed::TAIL_MS + (round as u64 + 1) * stt_speed::PAD_STEP_MS);
            let (ms, out) = stt_speed::transcribe_timed(&engine, &audio, &call);
            match out {
                Ok(t) => {
                    times[li].push(ms);
                    texts[li].get_or_insert(t);
                }
                Err(e) => run.error = Some(format!("couldn't transcribe: {e}")),
            }
        }
    }
    run.lengths = pieces
        .iter()
        .zip(times)
        .zip(texts)
        .map(|((piece, runs), text)| LengthTiming {
            secs: (piece.len() as f32 / 1_600.0).round() / 10.0,
            warm_ms: stt_speed::median(&runs),
            runs_ms: runs,
            wer: text.as_deref().and_then(wer),
        })
        .collect();
}

// ---- AI cleanup -------------------------------------------------------------------

/// Dictations as speech-to-text writes them (fictional people), cleaned
/// one after another.
pub const DICTATIONS: [&str; 4] = [
    "so um can you send the the report to Priya by Friday",
    "hi Sam just checking in on the launch plan uh do we still have the meeting on Tuesday at three",
    "remind me to book a room at the library for next Thursday no wait Wednesday",
    "the numbers for the third quarter look good but we need to double check the the travel costs before we send it to Imogen",
];

/// Cleanup on one model in one scenario.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupRun {
    /// The GGUF's name, or the cloud model.
    pub model: String,
    /// "local" or the cloud provider ("groq"…).
    pub provider: String,
    pub scenario: String,
    /// The server up and answering (local), ms.
    pub load_ms: Option<u64>,
    /// The first cleanup (after the sidecar's warm-up; for the cloud, a new
    /// connection).
    pub first_ms: Option<u64>,
    /// The median of the rest.
    pub median_ms: Option<u64>,
    pub runs_ms: Vec<u64>,
    /// Other apps' load on the processor around it (as [`SpeechRun`]).
    pub load_before: Option<u32>,
    pub load_after: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl CleanupRun {
    fn new(model: &str, provider: &str, scenario: &str) -> CleanupRun {
        CleanupRun {
            model: model.into(),
            provider: provider.into(),
            scenario: scenario.into(),
            load_ms: None,
            first_ms: None,
            median_ms: None,
            runs_ms: Vec::new(),
            load_before: None,
            load_after: None,
            skipped: None,
            error: None,
        }
    }

    /// Other apps were busy around it (local runs only: a cloud call isn't
    /// the processor's).
    pub fn busy(&self) -> bool {
        self.provider == "local" && busy(self.load_before, self.load_after)
    }

    fn finish(&mut self, times: Vec<u64>) {
        self.first_ms = times.first().copied();
        self.median_ms = stt_speed::median(times.get(1..).unwrap_or(&[]));
        self.runs_ms = times;
    }
}

/// A cleanup slower than this ends the scenario's run (dictation cleanup
/// gives up at 20 s and pastes the raw text).
const CLEANUP_BUDGET_MS: u64 = 20_000;

async fn cleanup_once(text: &str, base_url: &str, key: &str, model: &str, provider: &str) -> Result<u64, String> {
    let cfg = crate::config::YapConfig::default();
    let spelling = crate::about_you::cleanup_spelling(&cfg);
    let t0 = Instant::now();
    crate::llm::cleanup(text, base_url, key, model, provider, &cfg.pp_prompt, spelling.as_deref(), &[], false).await?;
    Ok(t0.elapsed().as_millis() as u64)
}

/// The on-device model `gguf` on llamafile `runtime` in scenario `sc`, as
/// Yap's sidecar runs it (`local_llm::server_args`; on the processor `-ngl
/// 0`, and `-t`/`-tb` the scenario's threads).
async fn time_local_cleanup(runtime: &Path, gguf: &Path, sc: &Scenario, work: &Path) -> CleanupRun {
    let id = crate::local_llm::model_id(gguf);
    let mut run = CleanupRun::new(&id, "local", &sc.id);
    let port = match std::net::TcpListener::bind("127.0.0.1:0").and_then(|l| l.local_addr()) {
        Ok(a) => a.port(),
        Err(e) => {
            run.error = Some(format!("no free port: {e}"));
            return run;
        }
    };
    let version = crate::local_llm::runtime_version_in(runtime.parent().unwrap_or(Path::new(".")))
        .or_else(|| crate::local_llm::Version::parse(crate::local_llm::RUNTIME_VERSION))
        .unwrap_or(crate::local_llm::Version(0, 10, 6));
    let ctx = crate::local_llm::DEFAULT_CONTEXT;
    let on_gpu = sc.device == Device::Gpu;
    let plan = if on_gpu {
        crate::local_llm::plan_for(&crate::config::YapConfig::default(), gguf, ctx, version)
    } else {
        crate::local_llm::Plan { runtime: version, mtp: false }
    };
    let mut args = crate::local_llm::server_args(gguf, port, if on_gpu { 999 } else { 0 }, ctx, plan);
    if let Some(n) = sc.threads {
        args.extend(["-t".into(), n.to_string(), "-tb".into(), n.to_string()]);
    }
    let log = work.join(format!("llamafile-{id}-{}.log", sc.id));
    let Ok(file) = std::fs::File::create(&log) else {
        run.error = Some("couldn't write the server's log".into());
        return run;
    };
    let mut cmd = std::process::Command::new(runtime);
    cmd.args(&args).current_dir(work).stdin(std::process::Stdio::null());
    if let Ok(f) = file.try_clone() {
        cmd.stdout(f);
    }
    cmd.stderr(file);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    // The server inherits the scenario's processor limit.
    let started = Instant::now();
    let spawned = {
        let _limit = machine::Limit::new(sc.threads);
        cmd.spawn()
    };
    let mut child = match spawned {
        Ok(c) => c,
        Err(e) => {
            run.error = Some(format!("couldn't start llamafile: {e}"));
            return run;
        }
    };
    crate::procs::end_with_yap(&child);
    let base = format!("http://127.0.0.1:{port}/v1");
    let client = reqwest::Client::builder().timeout(Duration::from_secs(3)).build().expect("an HTTP client");
    let mut up = false;
    while started.elapsed() < Duration::from_secs(300) {
        if let Ok(Some(status)) = child.try_wait() {
            run.error = Some(format!("llamafile exited during startup ({status})"));
            break;
        }
        if client.get(format!("http://127.0.0.1:{port}/health")).send_for(crate::privacy::net::Purpose::OnDeviceCheck).await.is_ok_and(|r| r.status().is_success()) {
            up = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    if up {
        run.load_ms = Some(started.elapsed().as_millis() as u64);
        let _ = crate::local_llm::warm_up(&base, &id).await;
        let mut times = Vec::new();
        'rounds: for _ in 0..2 {
            for text in DICTATIONS {
                match cleanup_once(text, &base, "", &id, crate::local_llm::PROVIDER_ONDEVICE).await {
                    Ok(ms) => {
                        times.push(ms);
                        if ms > CLEANUP_BUDGET_MS {
                            run.skipped = Some("slower than the 20 s a dictation waits: not timed further".into());
                            break 'rounds;
                        }
                    }
                    Err(e) => {
                        run.error = Some(e);
                        break 'rounds;
                    }
                }
            }
        }
        run.finish(times);
    } else if run.error.is_none() {
        run.error = Some("llamafile wasn't ready after 5 minutes".into());
    }
    let _ = child.kill();
    let _ = child.wait();
    run
}

/// A cloud endpoint from Yap's settings: the cleanup's own when it's a
/// cloud one with a key, else Groq when a Groq key is saved. Pure.
pub fn cloud_endpoint(cfg: &crate::config::YapConfig) -> Option<(String, String, String, String)> {
    let local = |p: &str| p == crate::local_llm::PROVIDER_ONDEVICE || p == "local" || p == "custom";
    if !local(&cfg.pp_provider) && !cfg.pp_api_key.trim().is_empty() && !cfg.pp_model.trim().is_empty() {
        return Some((cfg.pp_base_url.clone(), cfg.pp_api_key.clone(), cfg.pp_model.clone(), cfg.pp_provider.clone()));
    }
    let groq = cfg.pp_api_keys.get("groq").filter(|k| !k.trim().is_empty())?;
    Some(("https://api.groq.com/openai/v1".into(), groq.clone(), "openai/gpt-oss-20b".into(), "groq".into()))
}

async fn time_cloud_cleanup(cfg: Option<&crate::config::YapConfig>) -> CleanupRun {
    let Some((base, key, model, provider)) = cfg.and_then(cloud_endpoint) else {
        let mut run = CleanupRun::new("", "cloud", "cloud");
        run.skipped = Some("no cloud provider's key in Yap's settings".into());
        return run;
    };
    let mut run = CleanupRun::new(&model, &provider, "cloud");
    let mut times = Vec::new();
    for text in &DICTATIONS[..3] {
        match cleanup_once(text, &base, &key, &model, &provider).await {
            Ok(ms) => times.push(ms),
            Err(e) => {
                run.error = Some(e);
                break;
            }
        }
    }
    run.finish(times);
    run
}

// ---- the report -------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub schema: u32,
    pub yap: String,
    /// The real speech engines (a build without them times a stand-in).
    pub engines: bool,
    /// When, UTC.
    pub at: String,
    pub hardware: Hardware,
    /// How busy the processor was just before, percent.
    pub cpu_load_before: Option<u32>,
    pub scenarios: Vec<Scenario>,
    pub speech: Vec<SpeechRun>,
    pub cleanup: Vec<CleanupRun>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cold_start: Option<cold::ColdStart>,
    /// What Yap picks for this PC (`hardware.rs`).
    pub picks: crate::hardware::Advice,
}

fn ms(v: Option<u64>) -> String {
    v.map_or("—".into(), |v| v.to_string())
}

impl Report {
    /// The table printed and written beside the JSON.
    pub fn table(&self) -> String {
        use std::fmt::Write;
        let mut s = String::new();
        let hw = &self.hardware;
        let _ = writeln!(s, "Yap {} benchmark, {} (UTC){}", self.yap, self.at, if self.engines { "" } else { " — STUB ENGINE, numbers mean nothing" });
        let gpus: Vec<String> = hw.gpus().map(|a| format!("{} ({:.1} GB)", a.gpu.name, a.gpu.vram_mib as f32 / 1024.0)).collect();
        let _ = writeln!(
            s,
            "PC: {} ({} cores, {} threads), {} GB memory; graphics: {}",
            hw.cpu,
            hw.cores,
            hw.threads,
            hw.ram_gb,
            if gpus.is_empty() { "none".into() } else { gpus.join(", ") }
        );
        if let Some(load) = self.cpu_load_before {
            let _ = writeln!(s, "Processor {load} % busy before it started.");
        }
        let _ = writeln!(s, "\nSpeech to text: ms per dictation (warm median), word error rate against the script");
        let lens: Vec<f32> = self.speech.iter().find(|r| !r.lengths.is_empty()).map(|r| r.lengths.iter().map(|l| l.secs).collect()).unwrap_or_default();
        let mut head = format!("{:<22} {:<44} {:>6} {:>6}", "model", "where", "load", "first");
        for l in &lens {
            let _ = write!(head, " {:>7}", format!("{l:.0} s"));
        }
        let _ = writeln!(s, "{head}  {:>5}", "WER");
        for r in &self.speech {
            let label = self.scenarios.iter().find(|x| x.id == r.scenario).map(|x| x.label.as_str()).unwrap_or(&r.scenario);
            let name = if r.busy() { format!("{}*", r.model_name) } else { r.model_name.clone() };
            let mut line = format!("{:<22} {:<44} {:>6} {:>6}", name, truncate(&format!("{} · {label}", r.runtime), 44), ms(r.load_ms), ms(r.first_ms));
            for l in &lens {
                let _ = write!(line, " {:>7}", ms(r.warm_at(*l)));
            }
            let wer: Vec<f32> = r.lengths.iter().filter_map(|l| l.wer).collect();
            let wer = if wer.is_empty() { "—".into() } else { format!("{:.1}%", 100.0 * wer.iter().sum::<f32>() / wer.len() as f32) };
            let _ = write!(line, "  {wer:>5}");
            if let Some(why) = r.skipped.as_ref().or(r.error.as_ref()) {
                let _ = write!(line, "  ({why})");
            }
            let _ = writeln!(s, "{line}");
        }
        if !self.cleanup.is_empty() {
            let _ = writeln!(s, "\nAI cleanup: ms per dictation (first after loading, then the median)");
            let _ = writeln!(s, "{:<34} {:<44} {:>7} {:>6} {:>7}", "model", "where", "load", "first", "median");
            for r in &self.cleanup {
                let label = if r.provider == "local" {
                    self.scenarios.iter().find(|x| x.id == r.scenario).map(|x| x.label.clone()).unwrap_or(r.scenario.clone())
                } else {
                    format!("cloud ({})", r.provider)
                };
                let model = if r.busy() { format!("{}*", truncate(&r.model, 33)) } else { truncate(&r.model, 34) };
                let mut line = format!("{:<34} {:<44} {:>7} {:>6} {:>7}", model, truncate(&label, 44), ms(r.load_ms), ms(r.first_ms), ms(r.median_ms));
                if let Some(why) = r.skipped.as_ref().or(r.error.as_ref()) {
                    let _ = write!(line, "  ({})", truncate(why, 90));
                }
                let _ = writeln!(s, "{line}");
            }
        }
        if let Some(c) = &self.cold_start {
            let launch = |l: &cold::Launch| match (l.ready_ms(), &l.error) {
                (Some(ms), _) => format!("{ms} ms (window {} ms, page {} ms)", ms_opt(l.window_ms), ms_opt(l.page_ms)),
                (None, Some(e)) => format!("failed: {e}"),
                (None, None) => "—".into(),
            };
            let _ = writeln!(s, "\nCold start to the first-run window on screen: first launch {}; second launch {}", launch(&c.first_launch), launch(&c.second_launch));
        }
        if self.speech.iter().any(SpeechRun::busy) || self.cleanup.iter().any(CleanupRun::busy) {
            let _ = writeln!(s, "\n* Other apps kept the processor {QUIET_PCT} % or more busy around this run: slower than this PC is.");
        }
        let p = &self.picks;
        let _ = writeln!(s, "\nYap's picks for this PC: {}", p.summary);
        s
    }
}

fn ms_opt(v: Option<u64>) -> String {
    ms(v)
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n - 1).collect::<String>() + "…"
    }
}

// ---- running it -------------------------------------------------------------------

/// Point stdout at the console the bench was started from (a release
/// build is a windowed exe and gets none of its own). Under a pipe
/// (`| Out-Host`) the pipe is kept.
fn attach_console() {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        extern "system" {
            fn AttachConsole(process: u32) -> i32;
            fn GetStdHandle(which: u32) -> isize;
        }
        const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
        const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
        // SAFETY: plain kernel32 calls without pointers.
        unsafe {
            let out = GetStdHandle(STD_OUTPUT_HANDLE);
            if out == 0 || out == -1 {
                AttachConsole(ATTACH_PARENT_PROCESS);
            }
        }
    }
}

/// The installed speech models, in the registry's order.
fn installed(data: &Path) -> Vec<String> {
    stt::all_model_ids().into_iter().filter(|id| stt::is_model_installed(data, id)).map(str::to_string).collect()
}

/// The on-device models to time: `--llm`, else Yap's installed 1.5B and 4B.
fn local_models(data: &Path, o: &Options) -> Vec<PathBuf> {
    if let Some(l) = &o.llms {
        return l.clone();
    }
    let llm = data.join("llm");
    [crate::local_llm::DEFAULT_MODEL_ID, crate::local_llm::GPU_MODEL_ID]
        .iter()
        .filter_map(|id| crate::local_llm::curated_by_id(id))
        .map(|m| llm.join(m.filename))
        .filter(|p| p.is_file())
        .collect()
}

/// `yap.exe bench …` (main.rs). The process's exit code.
pub fn main(args: Vec<String>) -> i32 {
    attach_console();
    let o = match Options::parse(&args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}\n\n{USAGE}");
            return 2;
        }
    };
    if o.help {
        println!("{USAGE}");
        return 0;
    }
    // Yap's data folder is read only; the run's own folder takes anything
    // incidental (a usage count, a settings default) and goes at the end.
    let data = o.data.clone().unwrap_or_else(crate::portable::would_use);
    let work = std::env::temp_dir().join(format!("yap-bench-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&work);
    crate::portable::use_data_dir(work.join("data"));
    let peeked = crate::config::peek(&data);
    let code = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt.block_on(run(&o, &data, &work, peeked.as_ref())),
        Err(e) => {
            eprintln!("couldn't start: {e}");
            1
        }
    };
    let _ = std::fs::remove_dir_all(&work);
    code
}

async fn run(o: &Options, data: &Path, work: &Path, cfg: Option<&crate::config::YapConfig>) -> i32 {
    let hw = Hardware::this_pc();
    let scenarios = scenarios(&hw, o.scenarios.as_deref());
    let models = o.models.clone().unwrap_or_else(|| installed(data));
    println!("Yap {} benchmark — reading models from {} (read only)", env!("CARGO_PKG_VERSION"), data.display());
    if !cfg!(feature = "engines") {
        println!("⚠ This build has no speech engines (a stub): its numbers mean nothing.");
    }
    if models.is_empty() && o.models.is_none() {
        println!("No speech model installed there: speech-to-text is skipped (--data, or install one in Yap).");
    }
    let cpu_load_before = stt_speed::cpu_load(Duration::from_secs(1));
    if let Some(load) = cpu_load_before.filter(|l| *l >= stt_speed::CPU_BUSY_PCT) {
        println!("⚠ The processor is {load} % busy: close what you can for fair numbers.");
    }
    stt::fix_directml_stub();
    stt::apply_accelerator_settings(true);
    let audio = stt_speed::clip();
    let pieces: Vec<Vec<f32>> = cut_points(&audio, &o.lengths).into_iter().map(|end| audio[..end].to_vec()).collect();

    let mut speech = Vec::new();
    for sc in &scenarios {
        for model in &models {
            print!("  {} · {} … ", stt::model_name(model), sc.label);
            use std::io::Write;
            let _ = std::io::stdout().flush();
            let r = tokio::task::block_in_place(|| time_speech(data, model, sc, &pieces, o.rounds, o));
            println!(
                "{}",
                r.skipped.clone().or(r.error.clone()).unwrap_or_else(|| {
                    r.lengths.iter().map(|l| format!("{:.0} s {} ms", l.secs, ms(l.warm_ms))).collect::<Vec<_>>().join(", ")
                })
            );
            speech.push(r);
        }
    }

    let mut cleanup = Vec::new();
    if o.cleanup {
        let runtime = data.join("llm").join(crate::local_llm::RUNTIME_FILENAME);
        let ggufs = local_models(data, o);
        if o.llms.is_none() && (!runtime.is_file() || ggufs.is_empty()) {
            println!("No on-device AI model installed: local cleanup is skipped.");
        }
        for gguf in ggufs.iter().filter(|_| runtime.is_file()) {
            // (The integrated GPU and software rendering are speech-only:
            // llamafile picks its own GPU.)
            for sc in scenarios.iter().filter(|s| s.adapter.is_none()) {
                let name = crate::local_llm::model_id(gguf);
                print!("  cleanup · {name} · {} … ", sc.label);
                use std::io::Write;
                let _ = std::io::stdout().flush();
                let before = tokio::task::block_in_place(|| wait_quiet(o.patience));
                let mut r = time_local_cleanup(&runtime, gguf, sc, work).await;
                r.load_before = before;
                r.load_after = tokio::task::block_in_place(|| stt_speed::cpu_load(Duration::from_secs(1)));
                println!("{}", r.skipped.clone().or(r.error.clone()).unwrap_or_else(|| format!("median {} ms", ms(r.median_ms))));
                cleanup.push(r);
            }
        }
        if o.cloud {
            let r = time_cloud_cleanup(cfg).await;
            println!("  cleanup · cloud … {}", r.skipped.clone().or(r.error.clone()).unwrap_or_else(|| format!("median {} ms", ms(r.median_ms))));
            cleanup.push(r);
        }
    }

    let cold_start = if o.cold_start {
        println!("  cold start (a throwaway Yap opens for a few seconds) …");
        match std::env::current_exe() {
            Ok(exe) => Some(tokio::task::block_in_place(|| cold::measure(&exe, &work.join("cold")))),
            Err(_) => None,
        }
    } else {
        None
    };

    let picks = crate::hardware::advise(&crate::hardware::Machine::from_hardware(&hw));
    let report = Report {
        schema: SCHEMA,
        yap: env!("CARGO_PKG_VERSION").to_string(),
        engines: cfg!(feature = "engines"),
        at: chrono::Utc::now().format("%Y-%m-%d %H:%M").to_string(),
        hardware: hw,
        cpu_load_before,
        scenarios,
        speech,
        cleanup,
        cold_start,
        picks,
    };
    let table = report.table();
    println!("\n{table}");
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M").to_string();
    let (json_path, txt_path) = match &o.out {
        Some(p) if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("json")) => (p.clone(), p.with_extension("txt")),
        Some(dir) => (dir.join(format!("yap-bench-{stamp}.json")), dir.join(format!("yap-bench-{stamp}.txt"))),
        None => (PathBuf::from(format!("yap-bench-{stamp}.json")), PathBuf::from(format!("yap-bench-{stamp}.txt"))),
    };
    if let Some(dir) = json_path.parent().filter(|d| !d.as_os_str().is_empty()) {
        let _ = std::fs::create_dir_all(dir);
    }
    let json = serde_json::to_string_pretty(&report).unwrap_or_default();
    match std::fs::write(&json_path, json).and_then(|_| std::fs::write(&txt_path, &table)) {
        Ok(()) => {
            println!("Written: {}\n         {}", json_path.display(), txt_path.display());
            println!("Share the .json (it names your processor, memory and graphics, nothing else): see docs/benchmarks.md.");
            0
        }
        Err(e) => {
            eprintln!("Couldn't write the report: {e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpu::{Adapter, Gpu};

    fn hw(threads: u32, adapters: Vec<Adapter>) -> Hardware {
        Hardware { cpu: "Test CPU".into(), cores: threads / 2, threads, ram_gb: 16, adapters }
    }

    fn adapter(index: u32, name: &str, mib: u64, software: bool) -> Adapter {
        Adapter { index, gpu: Gpu { name: name.into(), vram_mib: mib, vendor: if software { 0x1414 } else { 0x10DE } }, software }
    }

    #[test]
    fn the_options_read_as_written() {
        let args = |s: &str| s.split_whitespace().map(str::to_string).collect::<Vec<_>>();
        assert_eq!(Options::parse(&[]).unwrap(), Options::default());
        let o = Options::parse(&args("--models parakeet-tdt-0.6b-v3,small --scenarios cpu,cpu-2t --rounds 5 --lengths 3,11 --no-cloud --out x.json")).unwrap();
        assert_eq!(o.models.as_deref(), Some(&["parakeet-tdt-0.6b-v3".to_string(), "small".to_string()][..]));
        assert_eq!(o.scenarios.as_deref(), Some(&["cpu".to_string(), "cpu-2t".to_string()][..]));
        assert_eq!((o.rounds, o.lengths.clone(), o.cloud, o.cleanup), (5, vec![3_000, 11_000], false, true));
        assert_eq!(o.out, Some(PathBuf::from("x.json")));
        let q = Options::parse(&args("--quick")).unwrap();
        assert_eq!((q.rounds, q.lengths.len()), (1, 2));
        assert!(Options::parse(&args("--rounds 0")).is_err());
        assert!(Options::parse(&args("--lengths 90")).is_err());
        assert!(Options::parse(&args("--frobnicate")).is_err());
        assert!(Options::parse(&args("--models")).is_err());
    }

    #[test]
    fn scenarios_follow_the_hardware() {
        // A desktop: card first, an integrated GPU, Windows' software renderer.
        let desktop = hw(
            16,
            vec![adapter(0, "NVIDIA GeForce RTX 5070 Ti", 15_907, false), adapter(1, "AMD Radeon(TM) Graphics", 2_048, false), adapter(2, "Microsoft Basic Render Driver", 0, true)],
        );
        let ids: Vec<String> = scenarios(&desktop, None).into_iter().map(|s| s.id).collect();
        assert_eq!(ids, ["gpu", "cpu", "cpu-4t", "cpu-2t", "igpu-4t"]);
        let igpu = scenarios(&desktop, Some(&["igpu-4t".into(), "warp-4t".into()]));
        assert_eq!(igpu.iter().map(|s| (s.adapter, s.threads, s.emulated)).collect::<Vec<_>>(), [(Some(1), Some(4), true), (Some(2), Some(4), true)]);
        // A laptop whose integrated GPU DXGI lists first: the card is the one.
        let hybrid = hw(12, vec![adapter(0, "Intel(R) Iris(R) Xe Graphics", 128, false), adapter(1, "NVIDIA GeForce RTX 4050 Laptop GPU", 5_921, false)]);
        let gpu = &scenarios(&hybrid, None)[0];
        assert_eq!((gpu.id.as_str(), gpu.adapter), ("gpu", Some(1)));
        assert_eq!(scenarios(&hybrid, None).last().map(|s| (s.id.clone(), s.adapter)), Some(("igpu-4t".into(), Some(0))));
        // A laptop with only integrated graphics and 4 threads.
        let laptop = hw(4, vec![adapter(0, "Intel(R) UHD Graphics", 128, false), adapter(1, "Microsoft Basic Render Driver", 0, true)]);
        let ids: Vec<String> = scenarios(&laptop, None).into_iter().map(|s| s.id).collect();
        assert_eq!(ids, ["gpu", "cpu", "cpu-2t"]);
        // No graphics driver: no "gpu" at all.
        let bare = hw(8, vec![adapter(0, "Microsoft Basic Render Driver", 0, true)]);
        let ids: Vec<String> = scenarios(&bare, None).into_iter().map(|s| s.id).collect();
        assert_eq!(ids, ["cpu", "cpu-4t", "cpu-2t"]);
    }

    #[test]
    fn transcripts_are_scored_against_the_scripts_start() {
        assert_eq!(wer(stt_speed::PASSAGE[0]), Some(0.0));
        let three = stt_speed::PASSAGE[..3].join(" ");
        assert_eq!(wer(&three), Some(0.0));
        // One word wrong in the first sentence's nine.
        let w = wer("Remind me to call Priya about the budget today.").unwrap();
        assert!((w - 1.0 / 9.0).abs() < 1e-6, "{w}");
        assert_eq!(wer(""), Some(1.0));
    }

    #[test]
    fn the_clip_cuts_near_three_eleven_and_thirty_seconds_at_pauses() {
        let audio = stt_speed::clip();
        let ends = cut_points(&audio, &[3_000, 11_000, 30_000, 99_000]);
        let secs: Vec<f32> = ends.iter().map(|e| *e as f32 / 16_000.0).collect();
        assert!((1.5..4.5).contains(&secs[0]), "{secs:?}");
        assert!((9.5..12.5).contains(&secs[1]), "{secs:?}");
        assert!((28.5..31.5).contains(&secs[2]), "{secs:?}");
        assert_eq!(ends[3], audio.len());
    }

    #[test]
    fn a_cloud_key_is_found_where_yap_keeps_it() {
        let mut cfg = crate::config::YapConfig { pp_provider: "ondevice".into(), ..Default::default() };
        assert_eq!(cloud_endpoint(&cfg), None);
        cfg.pp_api_keys.insert("groq".into(), "gsk_test".into());
        assert_eq!(cloud_endpoint(&cfg).map(|e| e.3), Some("groq".into()));
        cfg.pp_provider = "openai".into();
        cfg.pp_base_url = "https://api.openai.com/v1".into();
        cfg.pp_api_key = "sk-test".into();
        cfg.pp_model = "gpt-5-mini".into();
        let e = cloud_endpoint(&cfg).unwrap();
        assert_eq!((e.2.as_str(), e.3.as_str()), ("gpt-5-mini", "openai"));
    }

    #[test]
    fn the_report_has_the_shape_the_docs_describe() {
        let report = Report {
            schema: SCHEMA,
            yap: "0.1.1".into(),
            engines: true,
            at: "2026-10-24 12:00".into(),
            hardware: hw(8, vec![adapter(0, "Intel(R) UHD Graphics", 128, false)]),
            cpu_load_before: Some(3),
            scenarios: scenarios(&hw(8, vec![]), None),
            speech: vec![SpeechRun {
                model: "parakeet-tdt-0.6b-v3".into(),
                model_name: "Parakeet V3".into(),
                scenario: "cpu".into(),
                runtime: "CPU".into(),
                load_ms: Some(900),
                first_ms: Some(400),
                lengths: vec![LengthTiming { secs: 3.1, warm_ms: Some(120), runs_ms: vec![120, 118, 125], wer: Some(0.0) }],
                load_before: Some(4),
                load_after: Some(30),
                skipped: None,
                error: None,
            }],
            cleanup: vec![CleanupRun::new("qwen2.5-1.5b-instruct-q4_k_m", "local", "cpu")],
            cold_start: Some(cold::ColdStart::default()),
            picks: crate::hardware::advise(&crate::hardware::Machine { gpu: None, threads: 8, ram_mib: 16_384 }),
        };
        let v = serde_json::to_value(&report).unwrap();
        for key in ["schema", "yap", "engines", "at", "hardware", "cpuLoadBefore", "scenarios", "speech", "cleanup", "coldStart", "picks"] {
            assert!(v.get(key).is_some(), "no {key}");
        }
        for key in ["cpu", "cores", "threads", "ramGb", "adapters"] {
            assert!(v["hardware"].get(key).is_some(), "no hardware.{key}");
        }
        assert_eq!(v["hardware"]["adapters"][0]["name"], "Intel(R) UHD Graphics");
        assert!(v["hardware"].get("driver").is_none());
        let s = &v["speech"][0];
        for key in ["model", "modelName", "scenario", "runtime", "loadMs", "firstMs", "lengths"] {
            assert!(s.get(key).is_some(), "no speech.{key}");
        }
        assert_eq!(s["lengths"][0]["warmMs"], 120);
        assert_eq!(v["scenarios"][1]["threads"], 4);
        assert_eq!(v["scenarios"][1]["emulated"], true);
        for key in ["model", "provider", "scenario", "firstMs", "medianMs", "runsMs"] {
            assert!(v["cleanup"][0].get(key).is_some(), "no cleanup.{key}");
        }
        assert!(v["coldStart"].get("firstLaunch").is_some());
        assert_eq!((s["loadBefore"].as_u64(), s["loadAfter"].as_u64()), (Some(4), Some(30)));
        let table = report.table();
        assert!(table.contains("Parakeet V3*") && table.contains("120"), "{table}");
        assert!(table.contains("Other apps kept the processor"), "{table}");
    }

    #[test]
    fn a_run_is_busy_when_other_apps_were_before_or_after_it() {
        assert!(!busy(Some(3), Some(12)));
        assert!(busy(Some(3), Some(QUIET_PCT)));
        assert!(busy(Some(80), None));
        assert!(!busy(None, None));
    }
}
