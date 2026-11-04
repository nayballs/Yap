//! **The on-device model's engine: llama.cpp's own builds** (2026-11-01).
//!
//! The sidecar used to run Mozilla's llamafile, pinned when Yap was built:
//! it moved only with a Yap release, and llamafile itself lags llama.cpp by
//! months (0.10.6, 15 Sep 2026, bundles llama.cpp b10441; llama.cpp was at
//! b11960 on 28 Oct) — and new models need new builds. Now the sidecar runs
//! **llama.cpp's official Windows builds** (ggml-org/llama.cpp's GitHub
//! releases: `llama-server.exe` and its DLLs, from a zip), and keeps them up
//! to date within guardrails ([`updater`]): a build at least 3 days old,
//! every file checked against the SHA-256 GitHub publishes, tried with the
//! person's own model out of sight before it's used ([`trial`]), switched to
//! only at an idle moment, and rolled back if its first real start fails.
//!
//! On disk (`<data>/llm/engine/`): a folder per build and variant
//! (`b11982-cuda13/`: the server, its DLLs, the licences), NVIDIA's CUDA
//! runtime once for every build that uses it (`cudart-<sha12>/`, put first
//! on the server's PATH), and the marker ([`MARKER`], `engine.json`: which
//! build runs, the one before it, what this PC has learnt). Never
//! config.json: an older Yap's wholesale save would drop new fields there.
//! llamafile's own files (`llm/llamafile.exe`, `llamafile.json`) are left
//! exactly as they are: an older Yap sharing the data folder (the installed
//! nightly beside a dev build) still runs it, and it's this Yap's way back
//! when no llama.cpp build will run here.
//!
//! **Which build for this PC** ([`candidates`]): on an NVIDIA card **CUDA**
//! (CUDA 13 from a Turing card and driver 580 on, else CUDA 12; its zip and
//! NVIDIA's runtime, ~590 MB) and Vulkan, both measured and CUDA kept when
//! it's at least as fast ([`trial::choose`]: on the RTX 5070 Ti 269 against
//! 150 tokens/s, a dictation's cleanup 132 against 200 ms); Vulkan for any
//! other card with memory of its own, and the fallback when CUDA won't
//! start; the processor's build (its CPU code picked at run time: AVX2,
//! AVX-512…) without a card. What's chosen is kept per card and driver
//! ([`Pick`], like `dictation/stt_speed.rs`).
//!
//! **The floor** ([`FLOOR`]): a build pinned here, verified on a PC
//! (b11982 on an RTX 5070 Ti, 29 Oct 2026), is what a first install
//! downloads (CUDA on NVIDIA) and what Yap runs offline; newer ones come
//! through the updater. **A fresh install's floor that won't start**, with
//! nothing to go back to: refused, and the floor's other builds tried in
//! turn ([`floor_order`]: CUDA → Vulkan → the processor's on NVIDIA, Vulkan
//! → the processor's elsewhere; `updater::rescue`), each failure kept in
//! the marker; none runs: said plainly ([`NO_ENGINE`]).

pub mod feed;
pub mod trial;
pub mod updater;

use std::collections::BTreeMap;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::runtime::Version;

/// The engine's folder in `llm/`.
pub const DIR: &str = "engine";
/// Which build runs and what this PC has learnt about builds, in [`DIR`].
pub const MARKER: &str = "engine.json";
/// llama.cpp's server in each build's folder.
#[cfg(windows)]
pub const SERVER_EXE: &str = "llama-server.exe";
#[cfg(not(windows))]
pub const SERVER_EXE: &str = "llama-server";
/// The sidecar's log, in [`DIR`].
pub const SERVER_LOG: &str = "server.log";
/// The trials' log, in [`DIR`].
pub const TRIAL_LOG: &str = "trial.log";
/// One engine update at a time across Yaps (a dev build and the installed
/// app share `llm/`), in [`DIR`].
pub const LOCK: &str = "engine-update.lock";
/// The downloads while they're under way, in [`DIR`].
const DOWNLOADS: &str = "downloads";

/// llamafile 0.10.6 bundles llama.cpp b10441: every build Yap takes is
/// newer, with the buffers, MTP and flags Yap's estimates are calibrated
/// for.
pub const LLAMAFILE_BUILD: Build = Build(10_441);

/// A build of llama.cpp, "b11982" (its release's tag). Ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Build(pub u32);

impl Build {
    /// "b11982" → 11982; any other tag (`v0.8.0`, `b12x`, "") → `None`. Pure.
    pub fn parse(tag: &str) -> Option<Build> {
        let n = tag.trim().strip_prefix('b')?;
        if n.is_empty() || n.len() > 7 || !n.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        n.parse().ok().map(Build)
    }
}

impl std::fmt::Display for Build {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "b{}", self.0)
    }
}

/// Which of a release's Windows builds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Variant {
    /// CUDA 13 (Turing and newer NVIDIA cards, driver 580+).
    Cuda13,
    /// CUDA 12 (older NVIDIA cards and drivers).
    Cuda12,
    /// Vulkan: any graphics card.
    Vulkan,
    /// The processor (its CPU code chosen at run time).
    Cpu,
}

impl Variant {
    /// Its id in folder names, keys and the marker: "cuda13".
    pub fn id(self) -> &'static str {
        match self {
            Variant::Cuda13 => "cuda13",
            Variant::Cuda12 => "cuda12",
            Variant::Vulkan => "vulkan",
            Variant::Cpu => "cpu",
        }
    }

    pub fn from_id(id: &str) -> Option<Variant> {
        [Variant::Cuda13, Variant::Cuda12, Variant::Vulkan, Variant::Cpu].into_iter().find(|v| v.id() == id)
    }

    /// The start of its zip's name after `llama-<build>-bin-win-`: CUDA by
    /// major version only ("cuda-13."), as llama.cpp moves the minor.
    pub fn asset_part(self) -> &'static str {
        match self {
            Variant::Cuda13 => "cuda-13.",
            Variant::Cuda12 => "cuda-12.",
            Variant::Vulkan => "vulkan",
            Variant::Cpu => "cpu",
        }
    }

    pub fn cuda(self) -> bool {
        matches!(self, Variant::Cuda13 | Variant::Cuda12)
    }

    /// For Settings ("Engine: llama.cpp b11982 (Vulkan)"): "CUDA",
    /// "Vulkan", "processor".
    pub fn label(self) -> &'static str {
        match self {
            Variant::Cuda13 | Variant::Cuda12 => "CUDA",
            Variant::Vulkan => "Vulkan",
            Variant::Cpu => "processor",
        }
    }

    /// Runs on a graphics card.
    pub fn on_card(self) -> bool {
        self != Variant::Cpu
    }
}

/// What runs the on-device model, as a server's arguments and Yap's
/// estimates see it: llamafile (its version) or a llama.cpp build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runtime {
    Llamafile(Version),
    LlamaCpp(Build, Variant),
}

impl Runtime {
    /// Brings the buffers, MTP and flags Yap's memory estimate and MTP
    /// drafting are calibrated for: llamafile 0.10.6 (llama.cpp b10441), or
    /// any llama.cpp build since.
    pub fn is_current(self) -> bool {
        match self {
            Runtime::Llamafile(v) => v.is_current(),
            Runtime::LlamaCpp(b, _) => b >= LLAMAFILE_BUILD,
        }
    }

    /// Runs on CUDA on an NVIDIA card (llamafile's own GPU path is CUDA
    /// there): what MTP and the q8_0 KV cache were measured on.
    pub fn cuda(self) -> bool {
        match self {
            Runtime::Llamafile(_) => true,
            Runtime::LlamaCpp(_, v) => v.cuda(),
        }
    }

    pub fn is_llamafile(self) -> bool {
        matches!(self, Runtime::Llamafile(_))
    }

    /// The arguments that differ between the two servers: llamafile's
    /// `--server` (its OpenAI-compatible server, no browser UI); llama.cpp's
    /// `--no-webui` (no browser UI) and `--offline` (it never fetches a
    /// model itself).
    pub fn flavour_args(self) -> &'static [&'static str] {
        match self {
            Runtime::Llamafile(_) => &["--server"],
            Runtime::LlamaCpp(..) => &["--no-webui", "--offline"],
        }
    }
}

impl std::fmt::Display for Runtime {
    /// "0.10.6" (llamafile, as markers have always said it) or
    /// "b11982-cuda13".
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Runtime::Llamafile(v) => write!(f, "{v}"),
            Runtime::LlamaCpp(b, v) => write!(f, "{b}-{}", v.id()),
        }
    }
}

// ---- the builds on disk ------------------------------------------------------------

/// A build of llama.cpp on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Installed {
    pub build: Build,
    pub variant: Variant,
    /// Its zip's SHA-256 (GitHub's digest it was checked against).
    pub sha256: String,
    /// NVIDIA's runtime folder in [`DIR`] (`cudart-<sha12>`), for CUDA.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cudart: Option<String>,
    /// `--device` for a Vulkan build on a PC with several graphics
    /// adapters: the card (llama.cpp would otherwise split the model
    /// across the card and integrated graphics).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
}

impl Installed {
    /// "b11982-cuda13": its folder's name, and what refusals and failures
    /// are kept by.
    pub fn key(&self) -> String {
        format!("{}-{}", self.build, self.variant.id())
    }

    pub fn dir(&self, engine_dir: &Path) -> PathBuf {
        engine_dir.join(self.key())
    }

    pub fn runtime(&self) -> Runtime {
        Runtime::LlamaCpp(self.build, self.variant)
    }
}

/// Why a build didn't pass its trial here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Why {
    DidntStart,
    NoAnswer,
    ToolCall,
    Slower,
    Vision,
    /// Its files didn't download or check out.
    Download,
}

/// What happened last, for Settings' line ([`event_line`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Event {
    /// Switched to `to` after its trial; `ratio`: its generation speed
    /// against the engine before (1.0 = the same).
    #[serde(rename_all = "camelCase")]
    Updated { to: Build, from: Option<Build>, ratio: Option<f64>, on_card: bool, at: u64 },
    /// `build` was tried here and didn't pass.
    #[serde(rename_all = "camelCase")]
    Refused { build: Build, why: Why, ratio: Option<f64>, staying: Option<Build>, at: u64 },
    /// Switched, but its first start here failed: back on `to` (`None`:
    /// llamafile).
    #[serde(rename_all = "camelCase")]
    WentBack { from: Build, to: Option<Build>, at: u64 },
    /// The person went back (Settings).
    #[serde(rename_all = "camelCase")]
    BackByHand { from: Build, to: Option<Build>, at: u64 },
    /// The floor's builds in `tried` didn't start here: `variant` runs
    /// instead ([`floor_order`]).
    #[serde(rename_all = "camelCase")]
    Fallback { build: Build, variant: Variant, tried: Vec<Variant>, at: u64 },
    /// None of the floor's builds (`tried`) started on this PC, with these
    /// graphics adapters and drivers (`print`: a new driver tries again).
    #[serde(rename_all = "camelCase")]
    NoEngine { build: Build, tried: Vec<Variant>, print: String, at: u64 },
}

/// CUDA or Vulkan on this card, measured once per card and driver.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pick {
    /// The graphics adapters and drivers it was measured on
    /// (`stt_speed::pc_print`): a new driver measures again.
    pub print: String,
    pub variant: Variant,
    /// Each variant's generation speed (tokens/s) when measured.
    #[serde(default)]
    pub speeds: BTreeMap<String, f64>,
    pub build: Build,
    pub at: u64,
}

/// [`MARKER`]'s contents.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Marker {
    /// The build the sidecar runs (`None`: llamafile, or nothing yet).
    #[serde(default)]
    pub current: Option<Installed>,
    /// The one before it, kept for going back (`None` with llamafile on
    /// disk: back to llamafile).
    #[serde(default)]
    pub previous: Option<Installed>,
    /// Switched to, not yet started by the sidecar once: a start that fails
    /// on it goes back ([`roll_back`]).
    #[serde(default)]
    pub unproven: bool,
    /// A build whose first real start failed: tried again at the next
    /// launch ([`retry_at_launch`]) until it's refused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry: Option<Installed>,
    /// Builds ([`Installed::key`]) never taken again here.
    #[serde(default)]
    pub refused: Vec<String>,
    /// Sessions in which a switched-to build wouldn't start, by key:
    /// refused at [`REFUSE_AFTER`].
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub failures: BTreeMap<String, u32>,
    /// "<build>-<variant>/<GGUF file>" pairs MTP failed with here
    /// (`local_llm::mtp_key`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mtp_failed: Vec<String>,
    /// "Keep the engine up to date" turned off (Settings). Here, never in
    /// config.json: an older Yap's save would drop a new field there.
    #[serde(default)]
    pub paused: bool,
    /// When the releases were last looked at (unix seconds), and the newest
    /// build that could be taken then.
    #[serde(default)]
    pub checked_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest: Option<Build>,
    /// When it last switched to a newer build (unix seconds).
    #[serde(default)]
    pub updated_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<Event>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pick: Option<Pick>,
}

/// A build that fails its first real start in this many sessions is
/// refused here (as llamafile's runtimes, `runtime::REFUSE_AFTER`).
pub const REFUSE_AFTER: u32 = 3;

impl Marker {
    pub fn is_refused(&self, key: &str) -> bool {
        self.refused.iter().any(|r| r == key)
    }

    pub fn refuse(&mut self, key: &str) {
        if !self.is_refused(key) {
            self.refused.push(key.to_string());
        }
        // A long life of refusals: the oldest go (they're older builds,
        // never offered again anyway — only newer ones are taken).
        if self.refused.len() > 64 {
            self.refused.remove(0);
        }
    }

    /// The keys whose folders stay ([`tidy`]).
    pub fn kept(&self) -> Vec<String> {
        [&self.current, &self.previous, &self.retry].into_iter().flatten().map(Installed::key).collect()
    }

    /// The CUDA runtime folders the kept builds use.
    pub fn kept_cudart(&self) -> Vec<String> {
        [&self.current, &self.previous, &self.retry].into_iter().flatten().filter_map(|i| i.cudart.clone()).collect()
    }
}

/// The engine folder: `<llm>/engine`.
pub fn engine_dir_in(llm: &Path) -> PathBuf {
    llm.join(DIR)
}

/// Yap's engine folder.
pub fn engine_dir() -> PathBuf {
    engine_dir_in(&super::llm_dir())
}

pub fn read_marker(engine_dir: &Path) -> Marker {
    std::fs::read_to_string(engine_dir.join(MARKER)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn write_marker(engine_dir: &Path, m: &Marker) -> std::io::Result<()> {
    std::fs::create_dir_all(engine_dir)?;
    let json = serde_json::to_string_pretty(m).map_err(std::io::Error::other)?;
    crate::config::atomic_write(&engine_dir.join(MARKER), &json).map_err(std::io::Error::other)
}

/// Change the marker in one step (read, `f`, write); returns it as written.
pub fn update_marker(engine_dir: &Path, f: impl FnOnce(&mut Marker)) -> std::io::Result<Marker> {
    let mut m = read_marker(engine_dir);
    f(&mut m);
    write_marker(engine_dir, &m)?;
    Ok(m)
}

/// Seconds since 1970 (the marker's clock).
pub fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

// ---- what runs ----------------------------------------------------------------------

/// A server program for the on-device model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Engine {
    /// Mozilla's llamafile (`llm/llamafile.exe`).
    Llamafile { exe: PathBuf, version: Version },
    /// One of llama.cpp's own builds (`llm/engine/<key>/llama-server.exe`).
    LlamaCpp { dir: PathBuf, installed: Installed, cudart: Option<PathBuf> },
}

impl Engine {
    /// A llama.cpp build in `engine_dir`.
    pub fn llama_cpp(engine_dir: &Path, installed: &Installed) -> Engine {
        Engine::LlamaCpp {
            dir: installed.dir(engine_dir),
            installed: installed.clone(),
            cudart: installed.cudart.as_ref().map(|c| engine_dir.join(c)),
        }
    }

    pub fn runtime(&self) -> Runtime {
        match self {
            Engine::Llamafile { version, .. } => Runtime::Llamafile(*version),
            Engine::LlamaCpp { installed, .. } => installed.runtime(),
        }
    }

    pub fn exe(&self) -> PathBuf {
        match self {
            Engine::Llamafile { exe, .. } => exe.clone(),
            Engine::LlamaCpp { dir, .. } => dir.join(SERVER_EXE),
        }
    }

    /// Its program is on disk.
    pub fn present(&self) -> bool {
        self.exe().is_file()
    }

    /// The build, for llama.cpp.
    pub fn build(&self) -> Option<Build> {
        match self {
            Engine::Llamafile { .. } => None,
            Engine::LlamaCpp { installed, .. } => Some(installed.build),
        }
    }

    /// "llamafile 0.10.6" | "llama.cpp b11982".
    pub fn label(&self) -> String {
        match self {
            Engine::Llamafile { version, .. } => format!("llamafile {version}"),
            Engine::LlamaCpp { installed, .. } => format!("llama.cpp {}", installed.build),
        }
    }

    /// The command that starts its server, before the server's own
    /// arguments ([`super::server_args`]): a hidden window, and for a
    /// llama.cpp build its own folder as the working one (ggml looks for its
    /// backends beside the exe **and in the working folder**: another
    /// build's folder there would load that build's), NVIDIA's runtime
    /// first on the PATH (the DLLs `ggml-cuda.dll` needs), and its card
    /// pinned (`--device`) where it has one.
    pub fn command(&self) -> std::process::Command {
        let mut cmd = std::process::Command::new(self.exe());
        if let Engine::LlamaCpp { dir, installed, cudart } = self {
            cmd.current_dir(dir);
            if let Some(cudart) = cudart {
                let path = std::env::var_os("PATH").unwrap_or_default();
                let mut parts = vec![cudart.clone()];
                parts.extend(std::env::split_paths(&path));
                if let Ok(joined) = std::env::join_paths(parts) {
                    cmd.env("PATH", joined);
                }
            }
            if let Some(device) = &installed.device {
                cmd.args(["--device", device]);
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        cmd
    }

    /// The sidecar's log: llamafile's where it always was, a llama.cpp
    /// build's in the engine folder.
    pub fn log_path(&self, llm: &Path) -> PathBuf {
        match self {
            Engine::Llamafile { .. } => llm.join("llamafile.log"),
            Engine::LlamaCpp { .. } => engine_dir_in(llm).join(SERVER_LOG),
        }
    }
}

/// llamafile in `llm`, when it's there.
pub fn llamafile_in(llm: &Path) -> Option<Engine> {
    let exe = llm.join(super::RUNTIME_FILENAME);
    let version = super::runtime::runtime_version_in(llm)?;
    Some(Engine::Llamafile { exe, version })
}

/// **The engine the sidecar runs** from `llm`: the marker's llama.cpp
/// build when its server is there, else llamafile when it's there.
pub fn active_in(llm: &Path) -> Option<Engine> {
    let edir = engine_dir_in(llm);
    if let Some(current) = read_marker(&edir).current {
        let e = Engine::llama_cpp(&edir, &current);
        if e.present() {
            return Some(e);
        }
    }
    llamafile_in(llm)
}

/// [`active_in`] Yap's `llm/`.
pub fn active() -> Option<Engine> {
    active_in(&super::llm_dir())
}

/// What a server would run as now, for estimates and plans: the active
/// engine's, else what an install gets (the floor for this PC).
pub fn active_runtime() -> Runtime {
    active().map(|e| e.runtime()).unwrap_or_else(|| Runtime::LlamaCpp(FLOOR, primary_variant()))
}

/// Is a llama.cpp build the active engine in `llm`?
pub fn llama_cpp_active_in(llm: &Path) -> bool {
    matches!(active_in(llm), Some(Engine::LlamaCpp { .. }))
}

// ---- which build for this PC ---------------------------------------------------------

/// What an NVIDIA card's driver says (`nvidia-smi`): its compute capability
/// and the driver's version.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CudaInfo {
    pub compute: Option<(u32, u32)>,
    pub driver: Option<f32>,
}

/// CUDA 13 or 12 for an NVIDIA card, or neither (too old a driver): CUDA
/// 13 needs a Turing card or newer (compute 7.5+) and driver 580+ — with
/// the compute capability unknown, driver 590+ (which dropped the older
/// cards) says it; CUDA 12 needs driver 528+, or the driver unknown. Pure.
pub fn cuda_flavour(info: CudaInfo) -> Option<Variant> {
    let modern = info.compute.map(|c| c >= (7, 5));
    let driver = info.driver;
    let cuda13 = match (modern, driver) {
        (Some(true), Some(d)) => d >= 580.0,
        (None, Some(d)) => d >= 590.0,
        _ => false,
    };
    if cuda13 {
        return Some(Variant::Cuda13);
    }
    match driver {
        Some(d) if d < 528.0 => None,
        _ => Some(Variant::Cuda12),
    }
}

/// `nvidia-smi --query-gpu=compute_cap,driver_version --format=csv,noheader`'s
/// first line: "12.0, 617.14". Pure.
pub fn parse_nvidia_smi(out: &str) -> CudaInfo {
    let line = out.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    let mut parts = line.split(',').map(str::trim);
    let compute = parts.next().and_then(|c| {
        let (a, b) = c.split_once('.')?;
        Some((a.parse().ok()?, b.parse().ok()?))
    });
    let driver = parts.next().and_then(|d| d.parse::<f32>().ok());
    CudaInfo { compute, driver }
}

/// What the PC has, for choosing builds.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Hardware {
    /// The best card's PCI vendor and memory of its own (`gpu::this_pc`).
    pub vendor: Option<u32>,
    pub vram_mib: u64,
    pub cuda: CudaInfo,
}

/// The builds worth trying on a PC, best first. Pure:
/// - an NVIDIA card: **CUDA** ([`cuda_flavour`]: its zip and NVIDIA's
///   runtime, ~590 MB), then Vulkan — both measured, CUDA kept when it's at
///   least as fast (`trial::choose`; 2026-11-01, the maintainer: on the RTX
///   5070 Ti CUDA generates 269 tokens/s to Vulkan's 150 and cleans up a
///   dictation in 132 ms to Vulkan's 200 — dictation comes first);
/// - any other card with memory of its own: Vulkan;
/// - none, or a small one (integrated graphics): the processor.
pub fn candidates(hw: &Hardware) -> Vec<Variant> {
    let card = hw.vendor.is_some() && hw.vram_mib >= crate::ai::gpu::SMALL_GPU_MIB;
    if !card {
        return vec![Variant::Cpu];
    }
    let mut v = Vec::new();
    if hw.vendor == Some(super::NVIDIA) {
        v.extend(cuda_flavour(hw.cuda));
    }
    v.push(Variant::Vulkan);
    v
}

/// **The floor's builds a PC falls back through** when one won't start,
/// in order: its candidates, then the processor's — every PC runs that
/// (CUDA → Vulkan → processor on NVIDIA, Vulkan → processor elsewhere).
/// Pure.
pub fn floor_order(candidates: &[Variant]) -> Vec<Variant> {
    let mut v = candidates.to_vec();
    if !v.contains(&Variant::Cpu) {
        v.push(Variant::Cpu);
    }
    v
}

/// The floor's build of `variant`, by key ("b11982-cuda13").
pub fn floor_key(variant: Variant) -> String {
    format!("{FLOOR}-{}", variant.id())
}

/// The next of the floor's builds to try here: the first in `order` not
/// refused. Pure.
pub fn next_floor(m: &Marker, order: &[Variant]) -> Option<Variant> {
    order.iter().copied().find(|v| !m.is_refused(&floor_key(*v)))
}

/// **None of the engine's builds runs on this PC**: nothing current, no
/// llamafile, and every one of the floor's builds in `order` refused (it
/// didn't start or answer here). Pure.
pub fn none_runs(m: &Marker, order: &[Variant], llamafile: bool) -> bool {
    !llamafile && m.current.is_none() && !order.is_empty() && order.iter().all(|v| m.is_refused(&floor_key(*v)))
}

/// [`none_runs`] for this PC's `llm/`.
pub fn none_runs_here() -> bool {
    let llm = super::llm_dir();
    let edir = engine_dir_in(&llm);
    if !edir.join(MARKER).is_file() {
        return false;
    }
    none_runs(&read_marker(&edir), &floor_order(&candidates(&hardware())), llamafile_in(&llm).is_some())
}

/// What a start says when none of the engine's builds runs here: Settings'
/// Load now, Chat's "didn't start", the dictation's one notice.
pub const NO_ENGINE: &str = "The on-device engine won't start on this PC: Yap tried every build of llama.cpp it has for it. Settings → Language Models has Try again.";

/// A failed start's log that blames the model, not the engine (a GGUF this
/// llama.cpp can't load): no build is refused for it. Pure.
pub fn log_blames_model(log: &str) -> bool {
    let l = log.to_ascii_lowercase();
    l.contains("error loading model") || l.contains("unknown model architecture")
}

/// **A build that never started here, with nothing to go back to** (no
/// build before it, no llamafile — a fresh install's floor): refused, and
/// the marker left with nothing current, so the floor's next build is tried
/// (`updater::rescue`). Only a build not yet proven. Pure.
pub fn first_start_failed(m: &mut Marker) -> Option<Installed> {
    if !m.unproven {
        return None;
    }
    let failed = m.current.take()?;
    m.refuse(&failed.key());
    m.unproven = false;
    m.retry = None;
    Some(failed)
}

/// Which of `candidates` to download and try now: the one picked for this
/// card and driver (`pick`), else all of them (one only: that one). Pure.
pub fn to_try(candidates: &[Variant], pick: Option<&Pick>, print: &str) -> Vec<Variant> {
    if let Some(p) = pick.filter(|p| p.print == print && candidates.contains(&p.variant)) {
        return vec![p.variant];
    }
    candidates.to_vec()
}

/// This PC's hardware (cached; in a test run the stand-in card, and no
/// `nvidia-smi`).
pub fn hardware() -> Hardware {
    static CACHE: std::sync::OnceLock<Hardware> = std::sync::OnceLock::new();
    let gpu = crate::ai::gpu::this_pc();
    if crate::shell::e2e::active() {
        return Hardware { vendor: gpu.as_ref().map(|g| g.vendor), vram_mib: gpu.map(|g| g.vram_mib).unwrap_or(0), cuda: CudaInfo::default() };
    }
    CACHE
        .get_or_init(|| {
            let vendor = gpu.as_ref().map(|g| g.vendor);
            let cuda = if vendor == Some(super::NVIDIA) { nvidia_smi() } else { CudaInfo::default() };
            Hardware { vendor, vram_mib: gpu.map(|g| g.vram_mib).unwrap_or(0), cuda }
        })
        .clone()
}

/// What `nvidia-smi` (installed with NVIDIA's driver) says, within 10 s.
fn nvidia_smi() -> CudaInfo {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        let exe = root.join("System32").join("nvidia-smi.exe");
        if !exe.is_file() {
            return CudaInfo::default();
        }
        let child = std::process::Command::new(exe)
            .args(["--query-gpu=compute_cap,driver_version", "--format=csv,noheader"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .creation_flags(0x0800_0000)
            .spawn();
        let Ok(child) = child else { return CudaInfo::default() };
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(child.wait_with_output());
        });
        match rx.recv_timeout(std::time::Duration::from_secs(10)) {
            Ok(Ok(out)) => {
                let info = parse_nvidia_smi(&String::from_utf8_lossy(&out.stdout));
                tracing::info!(compute = ?info.compute, driver = ?info.driver, "On-device engine: NVIDIA's driver");
                info
            }
            _ => CudaInfo::default(),
        }
    }
    #[cfg(not(windows))]
    {
        CudaInfo::default()
    }
}

/// The build a first install gets on this PC.
pub fn primary_variant() -> Variant {
    candidates(&hardware()).first().copied().unwrap_or(Variant::Cpu)
}

// ---- the floor ------------------------------------------------------------------------

/// **The floor**: the build a first install downloads and Yap runs offline
/// — **b11982** (29 Oct 2026), verified on an RTX 5070 Ti with Qwen3.5-4B
/// (docs/model-evals.md "llama.cpp's own builds", 2026-11-01): the everyday
/// scorecard 5/7 on Vulkan, CUDA 13.4 and llamafile 0.10.6 alike;
/// generation Vulkan 150, CUDA 269 (MTP), llamafile 225 (MTP) tokens/s; a
/// dictation's cleanup 200 / 132 / 156 ms; tool calls, Think's fields and
/// the projector as on llamafile. Newer builds come through the [`updater`].
pub const FLOOR: Build = Build(11_982);

/// The floor's release on GitHub, as `scripts/check-pins.mjs` reads pins
/// (the weekly check of the downloads pinned in the code; llama.cpp's floor
/// is only "behind" 30 days past upstream, as the updater moves past it).
pub const FLOOR_RELEASE: &str = "https://github.com/ggml-org/llama.cpp/releases/download/b11982/";

/// A file on a release, as pinned or as the releases feed lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub sha256: String,
    pub size: u64,
    /// Where it comes from when not llama.cpp's release ([`asset_url`]): a
    /// test run's stand-in only.
    pub url: Option<String>,
}

impl Asset {
    fn pinned(name: &str, sha256: &str, size: u64) -> Asset {
        Asset { name: name.into(), sha256: sha256.into(), size, url: None }
    }
}

/// The floor's files for `variant`: its zip, and for CUDA NVIDIA's runtime
/// (SHA-256 = GitHub's digests on the b11982 release).
pub fn floor_assets(variant: Variant) -> (Asset, Option<Asset>) {
    match variant {
        Variant::Cuda13 => (
            Asset::pinned("llama-b11982-bin-win-cuda-13.4-x64.zip", "aebd94feb10c3f13a770e8f78a0c75d21c4e2701c1f7823e866613f1aa2f82ab", 164_987_252),
            Some(Asset::pinned("cudart-llama-bin-win-cuda-13.4-x64.zip", "738f8c251ac22b70c3ae6f83a10cf222725df0395246a2cf58f32bdb85fbe668", 423_535_356)),
        ),
        Variant::Cuda12 => (
            Asset::pinned("llama-b11982-bin-win-cuda-12.4-x64.zip", "92e3e2694d50b70130cf5410e597f741467e0edaddd543398c2156e62bc1f60e", 279_763_799),
            Some(Asset::pinned("cudart-llama-bin-win-cuda-12.4-x64.zip", "8c79a9b226de4b3cacfd1f83d24f962d0773be79f1e7b75c6af4ded7e32ae1d6", 391_443_627)),
        ),
        Variant::Vulkan => {
            (Asset::pinned("llama-b11982-bin-win-vulkan-x64.zip", "455c401e3a599ea08b5b1263bd994bf035880b8b558454d44a6cd0c3af480bfc", 37_346_367), None)
        }
        Variant::Cpu => (Asset::pinned("llama-b11982-bin-win-cpu-x64.zip", "20cd898fafc269657df8de946b06ef37dcf6b783dfa43d28ac6d187bb6ebfe1e", 24_408_458), None),
    }
}

/// What a first install downloads on this PC (bytes).
pub fn floor_bytes_here() -> u64 {
    let (zip, cudart) = floor_assets(primary_variant());
    zip.size + cudart.map(|c| c.size).unwrap_or(0)
}

/// Where a release's file is downloaded from: ggml-org/llama.cpp's
/// GitHub releases, built here from the tag and the file's name — never an
/// address read from the feed — or in a test run its stand-in
/// (`YAP_E2E_ENGINE_FEED`'s server, debug builds).
pub fn asset_url(build: Build, name: &str) -> String {
    #[cfg(debug_assertions)]
    if let Some(base) = feed::stand_in_base() {
        return format!("{base}/download/{build}/{name}");
    }
    if build == FLOOR {
        return format!("{FLOOR_RELEASE}{name}");
    }
    format!("https://github.com/ggml-org/llama.cpp/releases/download/{build}/{name}")
}

// ---- installing a build ------------------------------------------------------------

/// The file a zip entry is unpacked as: its name without folders when it's
/// the server, a DLL or a licence (the other tools stay out); `None`
/// otherwise, or for a name that isn't plain. Pure.
pub fn unpacked_name(entry: &str) -> Option<String> {
    let name = entry.rsplit(['/', '\\']).next()?.trim();
    if name.is_empty() || name.contains("..") || name.contains(':') || name.starts_with('.') {
        return None;
    }
    let lower = name.to_ascii_lowercase();
    let wanted = lower == SERVER_EXE || lower.ends_with(".dll") || lower.starts_with("license");
    wanted.then(|| name.to_string())
}

/// Unpack the wanted files of `zip` into `dest` (made fresh), streamed
/// (NVIDIA's cuBLAS is ~500 MB). Two entries with the same name, or a zip
/// with no server when `server` says there must be one, is an error.
/// Returns how many files it wrote.
pub fn unpack(zip: &Path, dest: &Path, server: bool) -> Result<usize, String> {
    let file = std::fs::File::open(zip).map_err(|e| format!("open {}: {e}", zip.display()))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("read the engine's zip: {e}"))?;
    let _ = std::fs::remove_dir_all(dest);
    std::fs::create_dir_all(dest).map_err(|e| format!("create {}: {e}", dest.display()))?;
    let mut seen = std::collections::HashSet::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| format!("the engine's zip, entry {i}: {e}"))?;
        if entry.is_dir() {
            continue;
        }
        // Never a path out of the folder (an absolute or `..` name).
        let Some(enclosed) = entry.enclosed_name() else { continue };
        let Some(name) = unpacked_name(&enclosed.to_string_lossy()) else { continue };
        if !seen.insert(name.to_ascii_lowercase()) {
            return Err(format!("the engine's zip has two files called {name}"));
        }
        let mut out = std::fs::File::create(dest.join(&name)).map_err(|e| format!("write {name}: {e}"))?;
        let mut buf = vec![0u8; 1 << 20];
        loop {
            let n = entry.read(&mut buf).map_err(|e| format!("unpack {name}: {e}"))?;
            if n == 0 {
                break;
            }
            std::io::Write::write_all(&mut out, &buf[..n]).map_err(|e| format!("write {name}: {e}"))?;
        }
    }
    if server && !seen.contains(SERVER_EXE) {
        return Err("the engine's zip has no llama-server".to_string());
    }
    if seen.is_empty() {
        return Err("the engine's zip had nothing Yap uses".to_string());
    }
    Ok(seen.len())
}

/// The CUDA runtime's folder name for a cudart zip's digest.
pub fn cudart_dir_name(sha256: &str) -> String {
    format!("cudart-{}", &sha256[..sha256.len().min(12)])
}

/// Download one file of a release into `engine/downloads/`, checked
/// against `asset.sha256` (a bad one deleted), and return its path.
async fn fetch_checked(
    edir: &Path,
    build: Build,
    asset: &Asset,
    done_before: u64,
    report: &(dyn Fn(crate::shell::download::Progress) + Sync),
    overall: &crate::shell::download::Overall,
    ticket: &crate::shell::download::Ticket,
) -> Result<PathBuf, String> {
    let url = asset.url.clone().unwrap_or_else(|| asset_url(build, &asset.name));
    // (A model evaluation runs in test mode too, and does download.)
    if crate::shell::e2e::active() && !crate::ai::eval::active() && !super::models::allowed_in_test_run(&url) {
        return Err("Test runs never download the engine from the internet".to_string());
    }
    let downloads = edir.join(DOWNLOADS);
    std::fs::create_dir_all(&downloads).map_err(|e| format!("Couldn't save the engine: {e}"))?;
    let dest = downloads.join(&asset.name);
    let partial = dest.with_extension("partial");
    tracing::info!(build = %build, file = %asset.name, mb = asset.size / 1_048_576, "On-device engine: downloading");
    let on_progress = |p: crate::shell::download::Progress| report(overall.at(done_before + p.downloaded));
    crate::shell::download::fetch(
        crate::shell::download::Fetch {
            url: &url,
            partial: &partial,
            size: (asset.size > 0).then_some(asset.size),
            what: crate::privacy::net::Download::OnDeviceEngine,
            ticket,
        },
        &on_progress,
    )
    .await?;
    super::models::verify_into(&partial, &dest, &asset.sha256, "engine").await?;
    Ok(dest)
}

/// **Install a build**: its zip (and NVIDIA's runtime, once for every
/// build that uses it) downloaded through `shell/download.rs` (resume,
/// Cancel), each checked against its SHA-256, unpacked into
/// `engine/<key>/` (the server, its DLLs, the licences) — through a
/// `.unpacking` folder, so a half-made one is never taken for a build. A
/// build already there is kept. For a Vulkan build its card is pinned when
/// it sees more than one adapter ([`pin_device`]).
pub async fn install(
    edir: &Path,
    build: Build,
    variant: Variant,
    zip: &Asset,
    cudart: Option<&Asset>,
    report: &(dyn Fn(crate::shell::download::Progress) + Sync),
    ticket: &crate::shell::download::Ticket,
) -> Result<Installed, String> {
    let mut installed = Installed { build, variant, sha256: zip.sha256.clone(), cudart: None, device: None };
    let dir = installed.dir(edir);
    let total = zip.size + cudart.map(|c| c.size).unwrap_or(0);
    let overall = crate::shell::download::Overall::new(total);
    let mut done = 0u64;
    if let Some(c) = cudart {
        let name = cudart_dir_name(&c.sha256);
        let cdir = edir.join(&name);
        if !has_cudart(&cdir) {
            let file = fetch_checked(edir, build, c, done, report, &overall, ticket).await?;
            let tmp = edir.join(format!("{name}.unpacking"));
            let (f, t) = (file.clone(), tmp.clone());
            tokio::task::spawn_blocking(move || unpack(&f, &t, false)).await.map_err(|e| e.to_string())??;
            let _ = std::fs::remove_dir_all(&cdir);
            std::fs::rename(&tmp, &cdir).map_err(|e| format!("Couldn't put the CUDA runtime in place: {e}"))?;
            let _ = std::fs::remove_file(&file);
        }
        done += c.size;
        installed.cudart = Some(name);
    }
    if !dir.join(SERVER_EXE).is_file() {
        let file = fetch_checked(edir, build, zip, done, report, &overall, ticket).await?;
        let tmp = edir.join(format!("{}.unpacking", installed.key()));
        let (f, t) = (file.clone(), tmp.clone());
        tokio::task::spawn_blocking(move || unpack(&f, &t, true)).await.map_err(|e| e.to_string())??;
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::rename(&tmp, &dir).map_err(|e| format!("Couldn't put the engine in place: {e}"))?;
        let _ = std::fs::remove_file(&file);
    }
    report(overall.at(total));
    if variant == Variant::Vulkan && !crate::shell::e2e::active() {
        installed.device = pin_device(&Engine::llama_cpp(edir, &installed)).await;
    }
    tracing::info!(build = %build, variant = variant.id(), device = installed.device.as_deref().unwrap_or(""), "On-device engine: installed");
    Ok(installed)
}

fn has_cudart(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .map(|rd| rd.filter_map(|e| e.ok()).any(|e| e.file_name().to_string_lossy().to_ascii_lowercase().starts_with("cudart64_")))
        .unwrap_or(false)
}

/// `llama-server --list-devices`' devices: ("Vulkan0", "NVIDIA GeForce RTX
/// 5070 Ti"). Pure.
pub fn parse_devices(out: &str) -> Vec<(String, String)> {
    out.lines()
        .filter_map(|l| {
            let l = l.trim();
            let (id, rest) = l.split_once(':')?;
            if id.is_empty() || id.contains(' ') || !id.ends_with(|c: char| c.is_ascii_digit()) {
                return None;
            }
            let name = rest.trim().split(" (").next().unwrap_or("").trim();
            (!name.is_empty()).then(|| (id.to_string(), name.to_string()))
        })
        .collect()
}

/// The device to pin among `devices` for the card `card` (DXGI's name):
/// `None` with one device (nothing to choose), else the one named like the
/// card, else none (llama.cpp's own choice). Pure.
pub fn device_for(devices: &[(String, String)], card: Option<&str>) -> Option<String> {
    if devices.len() < 2 {
        return None;
    }
    let card = card?.trim().to_ascii_lowercase();
    devices.iter().find(|(_, name)| name.to_ascii_lowercase() == card).map(|(id, _)| id.clone())
}

/// For a Vulkan build: the card's device when the build sees several
/// (integrated graphics beside the card), so the model goes on the card
/// alone.
async fn pin_device(engine: &Engine) -> Option<String> {
    let mut cmd = engine.command();
    cmd.arg("--list-devices").stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    let run = tokio::task::spawn_blocking(move || {
        let child = cmd.spawn().ok()?;
        crate::shell::procs::end_with_yap(&child);
        let out = child.wait_with_output().ok()?;
        Some(format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
    });
    let text = tokio::time::timeout(std::time::Duration::from_secs(60), run).await.ok()?.ok()??;
    let card = crate::ai::gpu::this_pc().map(|g| g.name);
    device_for(&parse_devices(&text), card.as_deref())
}

// ---- switching, going back --------------------------------------------------------------

/// Switch to `new` (its trial passed): the build before it kept for going
/// back, `new` unproven until the sidecar has started on it once. Pure.
pub fn switch_to(m: &mut Marker, new: Installed, ratio: Option<f64>, now: u64) {
    let from = m.current.take();
    let on_card = new.variant.on_card();
    let to = new.build;
    m.previous = from.clone();
    m.current = Some(new);
    m.unproven = true;
    m.retry = None;
    m.updated_at = now;
    m.event = Some(Event::Updated { to, from: from.map(|f| f.build), ratio, on_card, at: now });
}

/// Count a session in which `key` wouldn't start; refused at
/// [`REFUSE_AFTER`]. Returns the count. Pure.
pub fn count_failure(m: &mut Marker, key: &str) -> u32 {
    let n = {
        let c = m.failures.entry(key.to_string()).or_insert(0);
        *c += 1;
        *c
    };
    if n >= REFUSE_AFTER {
        m.refuse(key);
    }
    n
}

/// **The switched-to build didn't start** (twice): the one before goes
/// back in (`None`: llamafile), the failure counted, the failed one kept to
/// try again at the next launch until it's refused. `None` when there's
/// nothing to go back from (not unproven). Pure.
pub fn roll_back(m: &mut Marker, now: u64) -> Option<Installed> {
    if !m.unproven {
        return None;
    }
    let failed = m.current.take()?;
    count_failure(m, &failed.key());
    m.current = m.previous.take();
    m.unproven = false;
    m.retry = (!m.is_refused(&failed.key())).then(|| failed.clone());
    m.event = Some(Event::WentBack { from: failed.build, to: m.current.as_ref().map(|c| c.build), at: now });
    Some(failed)
}

/// The one before didn't start either: the fault isn't the new build's —
/// it goes back in, unproven, the failure taken back. Pure.
pub fn put_back(m: &mut Marker, failed: Installed, ratio_event: Option<Event>) {
    let key = failed.key();
    if let Some(c) = m.failures.get_mut(&key) {
        *c = c.saturating_sub(1);
        if *c == 0 {
            m.failures.remove(&key);
        }
    }
    m.refused.retain(|r| r != &key);
    m.previous = m.current.take();
    m.current = Some(failed);
    m.unproven = true;
    m.retry = None;
    m.event = ratio_event;
}

/// The sidecar started on the current build: proven (its folder no longer
/// a candidate for going back automatically). Pure.
pub fn prove(m: &mut Marker) -> bool {
    std::mem::replace(&mut m.unproven, false)
}

/// At launch: a build kept to try again after a failed first start goes
/// back in (unproven), unless it was refused meanwhile. Pure.
pub fn retry_at_launch(m: &mut Marker) -> bool {
    let Some(r) = m.retry.take() else { return false };
    if m.is_refused(&r.key()) || m.current.as_ref().is_some_and(|c| c.key() == r.key()) {
        return false;
    }
    m.previous = m.current.take();
    m.current = Some(r);
    m.unproven = true;
    true
}

/// Try the floor's builds again (Settings' Try again, or a new graphics
/// driver after none of them started): their refusals and the "none
/// started" line go. Pure.
pub fn forget_floor_refusals(m: &mut Marker) {
    let prefix = format!("{FLOOR}-");
    m.refused.retain(|r| !r.starts_with(&prefix));
    m.failures.retain(|k, _| !k.starts_with(&prefix));
    if matches!(m.event, Some(Event::NoEngine { .. })) {
        m.event = None;
    }
}

/// At launch, after none of the floor's builds started: a new graphics
/// driver (or card) since (`print`) tries them again. Pure.
pub fn new_driver_retry(m: &mut Marker, print: &str) -> bool {
    match &m.event {
        Some(Event::NoEngine { print: then, .. }) if then != print => {
            forget_floor_refusals(m);
            true
        }
        _ => false,
    }
}

/// What going back by hand goes back to: the build before, else llamafile
/// (when it's on disk), else nothing. Pure.
pub fn back_target(m: &Marker, llamafile: Option<Version>) -> Option<String> {
    m.current.as_ref()?;
    match (&m.previous, llamafile) {
        (Some(p), _) => Some(p.build.to_string()),
        (None, Some(v)) => Some(format!("llamafile {v}")),
        (None, None) => None,
    }
}

/// **Go back by hand** (Settings): to the build before, or to llamafile;
/// the build left is refused, so it isn't taken again (a newer one is).
/// Pure.
pub fn go_back(m: &mut Marker, llamafile: bool, now: u64) -> Result<(), String> {
    let left = m.current.clone().ok_or("Yap isn't on a llama.cpp build")?;
    match m.previous.take() {
        Some(p) => m.current = Some(p),
        None if llamafile => m.current = None,
        None => return Err("There's no earlier engine to go back to".to_string()),
    }
    m.refuse(&left.key());
    m.unproven = false;
    m.retry = None;
    m.event = Some(Event::BackByHand { from: left.build, to: m.current.as_ref().map(|c| c.build), at: now });
    Ok(())
}

/// Delete what no build kept needs in `edir`: other builds' folders, CUDA
/// runtimes none of them uses, half-unpacked folders. A folder in use (a
/// server another Yap runs from it) stays for the next tidy. Returns what
/// went.
pub fn tidy(edir: &Path, m: &Marker) -> Vec<String> {
    let kept = m.kept();
    let kept_cudart = m.kept_cudart();
    let mut gone = Vec::new();
    let Ok(rd) = std::fs::read_dir(edir) else { return gone };
    for e in rd.filter_map(|e| e.ok()) {
        let name = e.file_name().to_string_lossy().into_owned();
        if !e.path().is_dir() {
            continue;
        }
        let build_dir = name.split_once('-').is_some_and(|(b, v)| Build::parse(b).is_some() && Variant::from_id(v).is_some());
        let stale = name.ends_with(".unpacking")
            || (build_dir && !kept.contains(&name))
            || (name.starts_with("cudart-") && !kept_cudart.contains(&name));
        if stale && std::fs::remove_dir_all(e.path()).is_ok() {
            gone.push(name);
        }
    }
    gone
}

/// **At launch, before the sidecar starts**: a build kept to try again goes
/// back in, leftovers go. Not while another Yap holds the lock.
pub fn tidy_at_launch() {
    let edir = engine_dir();
    if !edir.is_dir() {
        return;
    }
    let Some(_lock) = super::runtime::UpgradeLock::take_named(&edir, LOCK) else { return };
    let print = crate::dictation::stt_speed::pc_print();
    let m = update_marker(&edir, |m| {
        if retry_at_launch(m) {
            tracing::info!("On-device engine: trying the build that didn't start last time again");
        }
        if new_driver_retry(m, &print) {
            tracing::info!("On-device engine: a new graphics driver since none of its builds started; trying them again");
        }
    });
    if let Ok(m) = m {
        let gone = tidy(&edir, &m);
        if !gone.is_empty() {
            tracing::info!(folders = gone.len(), "On-device engine: tidied old builds");
        }
    }
}

// ---- Settings' line ---------------------------------------------------------------------

/// What "staying on" names: the build, or llamafile.
fn staying_name(b: Option<Build>) -> String {
    b.map(|b| b.to_string()).unwrap_or_else(|| "llamafile".to_string())
}

/// **What happened last, in Settings' words.** Pure.
pub fn event_line(e: &Event) -> String {
    match e {
        Event::Updated { to, from, ratio, on_card, .. } => {
            let verb = if from.is_some() { "Updated to" } else { "Moved to llama.cpp" };
            let place = if *on_card { "on your card" } else { "on this PC" };
            let how = match ratio {
                Some(r) if *r >= 1.03 => format!(" · faster {place}"),
                Some(r) if *r > 0.97 => " · as fast as before".to_string(),
                Some(_) => " · a little slower, within Yap's limit".to_string(),
                None => String::new(),
            };
            format!("{verb} {to}{how}")
        }
        Event::Refused { build, why, ratio, staying, .. } => {
            let what = match why {
                Why::DidntStart => "didn't start here".to_string(),
                Why::NoAnswer => "didn't answer here".to_string(),
                Why::ToolCall => "couldn't use tools here".to_string(),
                Why::Vision => "couldn't read screenshots here".to_string(),
                Why::Download => "didn't download".to_string(),
                Why::Slower => match ratio {
                    Some(r) => format!("was slower here ({}% of the speed)", (r * 100.0).round() as u32),
                    None => "was slower here".to_string(),
                },
            };
            format!("{build} {what}: staying on {}", staying_name(*staying))
        }
        Event::WentBack { from, to, .. } => format!("{from} didn't start: back on {}", staying_name(*to)),
        Event::BackByHand { from, to, .. } => format!("Went back to {} · {from} won't be installed again", staying_name(*to)),
        Event::Fallback { variant, tried, .. } => {
            let on = match variant {
                Variant::Cpu => "the processor: answers will be slower".to_string(),
                v => v.label().to_string(),
            };
            if tried.is_empty() {
                format!("The engine runs on {on} here")
            } else {
                format!("{} didn't start here, so the engine runs on {on}", names(tried))
            }
        }
        Event::NoEngine { tried, .. } => format!(
            "None of the engine's builds started on this PC ({} tried). Updating the graphics driver may help; then Try again.",
            names(tried)
        ),
    }
}

/// "CUDA, Vulkan and the processor's build". Pure.
fn names(tried: &[Variant]) -> String {
    let words: Vec<&str> = tried.iter().map(|v| if *v == Variant::Cpu { "the processor's build" } else { v.label() }).collect();
    match words.as_slice() {
        [] => "No build".to_string(),
        [one] => capitalise(one),
        [rest @ .., last] => capitalise(&format!("{} and {last}", rest.join(", "))),
    }
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// Each [`status`] numbered as it's made: Check now answers with one made
/// before its look runs, and the look's own `yap-local-engine` events can
/// reach the page first, so Settings keeps whichever is newest (`rev`).
static STATUS_REV: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Settings' engine line and its controls (`local_llm_status().engine`).
pub fn status() -> serde_json::Value {
    let rev = STATUS_REV.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let llm = super::llm_dir();
    let edir = engine_dir_in(&llm);
    let m = read_marker(&edir);
    let active = active_in(&llm);
    let llamafile = super::runtime::runtime_version_in(&llm);
    let build = active.as_ref().and_then(Engine::build);
    let phase = updater::phase();
    let up_to_date = match (build, m.latest) {
        (Some(b), Some(l)) => b >= l,
        _ => false,
    };
    serde_json::json!({
        // "llama.cpp b11982" | "llamafile 0.10.6" | null (nothing installed).
        "label": active.as_ref().map(Engine::label),
        "kind": match &active { Some(Engine::LlamaCpp { .. }) => "llamacpp", Some(Engine::Llamafile { .. }) => "llamafile", None => "none" },
        "build": build.map(|b| b.to_string()),
        "variant": active.as_ref().and_then(|e| match e { Engine::LlamaCpp { installed, .. } => Some(installed.variant.label()), _ => None }),
        "latest": m.latest.map(|b| b.to_string()),
        "checkedAt": (m.checked_at > 0).then_some(m.checked_at),
        "upToDate": up_to_date,
        "auto": !m.paused,
        "phase": phase.0,
        "percent": phase.1,
        // A download's size (CUDA's ~590 MB with NVIDIA's runtime).
        "downloadMb": updater::download_mb(),
        "candidate": updater::candidate().map(|b| b.to_string()),
        // None of its builds runs here (said plainly, with Try again), or
        // another of the floor's builds is being set up after one didn't start.
        "failed": active.is_none() && none_runs_here(),
        "rescuing": updater::rescuing(),
        "line": m.event.as_ref().map(event_line),
        "backTo": back_target(&m, llamafile),
        "error": updater::last_error(),
        "rev": rev,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::local_llm::tests::scratch;

    fn inst(build: u32, variant: Variant) -> Installed {
        Installed { build: Build(build), variant, sha256: format!("{build:064}"), cudart: None, device: None }
    }

    #[test]
    fn builds_read_from_their_tags_and_order() {
        assert_eq!(Build::parse("b11982"), Some(Build(11_982)));
        assert_eq!(Build::parse(" b9 "), Some(Build(9)));
        assert_eq!(Build::parse("v0.8.0"), None, "not a build's tag");
        assert_eq!(Build::parse("b"), None);
        assert_eq!(Build::parse("b12x"), None);
        assert_eq!(Build::parse("b-1"), None);
        assert_eq!(Build::parse("b123456789"), None, "absurd");
        assert!(Build(11_960) < Build(11_982) && Build(9_999) < Build(10_000));
        assert_eq!(Build(11_982).to_string(), "b11982");
        assert!(FLOOR > LLAMAFILE_BUILD);
        assert_eq!(serde_json::to_string(&Build(5)).unwrap(), "5");
    }

    #[test]
    fn the_runtime_says_what_a_server_takes() {
        let lf = Runtime::Llamafile(Version(0, 10, 6));
        let old = Runtime::Llamafile(Version(0, 10, 3));
        let cpp = Runtime::LlamaCpp(FLOOR, Variant::Cuda13);
        assert!(lf.is_current() && cpp.is_current() && !old.is_current());
        assert!(!Runtime::LlamaCpp(Build(9_000), Variant::Cpu).is_current(), "older than llamafile's own");
        assert_eq!(lf.flavour_args(), ["--server"]);
        assert_eq!(cpp.flavour_args(), ["--no-webui", "--offline"]);
        assert_eq!(lf.to_string(), "0.10.6", "llamafile's MTP keys stay as they were");
        assert_eq!(cpp.to_string(), "b11982-cuda13");
        assert!(lf.cuda() && cpp.cuda() && !Runtime::LlamaCpp(FLOOR, Variant::Vulkan).cuda());
    }

    #[test]
    fn the_build_for_a_pc_cuda_on_nvidia_vulkan_on_other_cards_else_the_processor() {
        let nv = |cc: Option<(u32, u32)>, d: Option<f32>| Hardware { vendor: Some(0x10DE), vram_mib: 16_303, cuda: CudaInfo { compute: cc, driver: d } };
        // On NVIDIA, CUDA first, then Vulkan.
        assert_eq!(candidates(&nv(Some((12, 0)), Some(617.14))), vec![Variant::Cuda13, Variant::Vulkan]);
        assert_eq!(candidates(&nv(Some((6, 1)), Some(580.97))), vec![Variant::Cuda12, Variant::Vulkan], "a Pascal card: CUDA 13 dropped it");
        assert_eq!(candidates(&nv(Some((8, 6)), Some(560.0))), vec![Variant::Cuda12, Variant::Vulkan], "a driver too old for CUDA 13");
        assert_eq!(candidates(&nv(None, Some(595.0))), vec![Variant::Cuda13, Variant::Vulkan], "590+ dropped the older cards");
        assert_eq!(candidates(&nv(None, None)), vec![Variant::Cuda12, Variant::Vulkan], "nvidia-smi missing: the widest CUDA");
        assert_eq!(candidates(&nv(Some((7, 5)), Some(470.0))), vec![Variant::Vulkan], "no CUDA on so old a driver");
        let amd = Hardware { vendor: Some(0x1002), vram_mib: 8_192, cuda: CudaInfo::default() };
        assert_eq!(candidates(&amd), vec![Variant::Vulkan]);
        let igpu = Hardware { vendor: Some(0x1002), vram_mib: 2_048, cuda: CudaInfo::default() };
        assert_eq!(candidates(&igpu), vec![Variant::Cpu], "integrated graphics");
        assert_eq!(candidates(&Hardware::default()), vec![Variant::Cpu]);
        // nvidia-smi's line.
        assert_eq!(parse_nvidia_smi("12.0, 617.14\r\n"), CudaInfo { compute: Some((12, 0)), driver: Some(617.14) });
        assert_eq!(parse_nvidia_smi(""), CudaInfo::default());
        assert_eq!(parse_nvidia_smi("[N/A], 580.88").compute, None);
    }

    #[test]
    fn cuda_and_vulkan_are_measured_once_per_card_and_driver() {
        let both = [Variant::Cuda13, Variant::Vulkan];
        assert_eq!(to_try(&both, None, "card 1"), both.to_vec(), "never measured: both");
        let pick = Pick { print: "card 1".into(), variant: Variant::Vulkan, speeds: BTreeMap::new(), build: FLOOR, at: 1 };
        assert_eq!(to_try(&both, Some(&pick), "card 1"), vec![Variant::Vulkan]);
        assert_eq!(to_try(&both, Some(&pick), "card 1, new driver"), both.to_vec(), "a new driver: again");
        assert_eq!(to_try(&[Variant::Cpu], Some(&pick), "card 1"), vec![Variant::Cpu], "the pick isn't a candidate any more");
    }

    #[test]
    fn a_fresh_install_falls_back_through_the_floors_builds_and_says_plainly_when_none_runs() {
        let nv = Hardware { vendor: Some(0x10DE), vram_mib: 16_303, cuda: CudaInfo { compute: Some((12, 0)), driver: Some(617.14) } };
        let amd = Hardware { vendor: Some(0x1002), vram_mib: 8_192, cuda: CudaInfo::default() };
        // The order: CUDA → Vulkan → the processor's on NVIDIA; Vulkan → the processor's elsewhere.
        let order = floor_order(&candidates(&nv));
        assert_eq!(order, vec![Variant::Cuda13, Variant::Vulkan, Variant::Cpu]);
        assert_eq!(floor_order(&candidates(&amd)), vec![Variant::Vulkan, Variant::Cpu]);
        assert_eq!(floor_order(&candidates(&Hardware::default())), vec![Variant::Cpu]);
        assert_eq!(floor_key(Variant::Cuda13), "b11982-cuda13");
        // A fresh install's floor (CUDA) that never started: refused, nothing current.
        let mut m = Marker { current: Some(inst(FLOOR.0, Variant::Cuda13)), unproven: true, ..Default::default() };
        assert_eq!(next_floor(&m, &order), Some(Variant::Cuda13));
        assert_eq!(first_start_failed(&mut m).map(|i| i.variant), Some(Variant::Cuda13));
        assert!(m.current.is_none() && !m.unproven && m.is_refused("b11982-cuda13"));
        assert_eq!(next_floor(&m, &order), Some(Variant::Vulkan), "the next one, never CUDA again");
        assert!(!none_runs(&m, &order, false));
        // A proven build isn't refused by a failed start (a transient fault).
        let mut proven = Marker { current: Some(inst(FLOOR.0, Variant::Vulkan)), ..Default::default() };
        assert_eq!(first_start_failed(&mut proven), None);
        assert!(proven.current.is_some() && proven.refused.is_empty());
        // Vulkan and the processor's refused too: none runs — unless llamafile's here.
        m.refuse("b11982-vulkan");
        assert_eq!(next_floor(&m, &order), Some(Variant::Cpu));
        m.refuse("b11982-cpu");
        assert_eq!(next_floor(&m, &order), None);
        assert!(none_runs(&m, &order, false));
        assert!(!none_runs(&m, &order, true), "llamafile still runs");
        assert!(!none_runs(&Marker::default(), &order, false), "a PC that never had an engine");
        // A newer build's refusal is the updater's, not the floor's.
        let mut newer = Marker::default();
        newer.refuse("b12000-cuda13");
        assert_eq!(next_floor(&newer, &order), Some(Variant::Cuda13));
        // Said plainly; Try again, or a new driver, forgets the floor's refusals only.
        m.event = Some(Event::NoEngine { build: FLOOR, tried: order.clone(), print: "card, driver 1".into(), at: 1 });
        assert_eq!(
            event_line(m.event.as_ref().unwrap()),
            "None of the engine's builds started on this PC (CUDA, Vulkan and the processor's build tried). Updating the graphics driver may help; then Try again."
        );
        m.refuse("b12000-vulkan");
        assert!(!new_driver_retry(&mut m.clone(), "card, driver 1"), "the same driver: nothing");
        assert!(new_driver_retry(&mut m, "card, driver 2"));
        assert_eq!(m.refused, vec!["b12000-vulkan".to_string()]);
        assert!(m.event.is_none());
        assert_eq!(next_floor(&m, &order), Some(Variant::Cuda13));
        // The log that blames the model, not the engine.
        assert!(log_blames_model("llama_model_load: error loading model: unknown model architecture: 'spark2_5'"));
        assert!(!log_blames_model("ggml_cuda_init: failed to initialize CUDA: no CUDA-capable device is detected"));
        // The fallback's line.
        let fell = |variant, tried: Vec<Variant>| event_line(&Event::Fallback { build: FLOOR, variant, tried, at: 1 });
        assert_eq!(fell(Variant::Vulkan, vec![Variant::Cuda13]), "CUDA didn't start here, so the engine runs on Vulkan");
        assert_eq!(fell(Variant::Cpu, vec![Variant::Cuda13, Variant::Vulkan]), "CUDA and Vulkan didn't start here, so the engine runs on the processor: answers will be slower");
        assert_eq!(fell(Variant::Cuda13, vec![]), "The engine runs on CUDA here", "Try again's first build passed");
    }

    #[test]
    fn the_floor_is_pinned_for_every_variant() {
        for v in [Variant::Cuda13, Variant::Cuda12, Variant::Vulkan, Variant::Cpu] {
            let (zip, cudart) = floor_assets(v);
            assert!(zip.name.starts_with(&format!("llama-{FLOOR}-bin-win-{}", v.asset_part())), "{}", zip.name);
            assert_eq!(zip.sha256.len(), 64);
            assert!(zip.size > 1_000_000);
            assert_eq!(cudart.is_some(), v.cuda());
            if let Some(c) = cudart {
                assert!(c.name.starts_with(&format!("cudart-llama-bin-win-{}", v.asset_part())));
                assert_eq!(c.sha256.len(), 64);
            }
        }
        assert_eq!(asset_url(FLOOR, "llama-b11982-bin-win-cpu-x64.zip"), format!("{FLOOR_RELEASE}llama-b11982-bin-win-cpu-x64.zip"));
        assert!(FLOOR_RELEASE.ends_with(&format!("/{FLOOR}/")), "the pin the weekly check reads is the floor");
    }

    #[test]
    fn only_the_server_its_dlls_and_licences_are_unpacked() {
        assert_eq!(unpacked_name("llama-server.exe").as_deref(), Some("llama-server.exe"));
        assert_eq!(unpacked_name("ggml-cuda.dll").as_deref(), Some("ggml-cuda.dll"));
        assert_eq!(unpacked_name("build/bin/ggml-base.dll").as_deref(), Some("ggml-base.dll"), "folders flattened");
        assert_eq!(unpacked_name("LICENSE-LLVM-OpenMP").as_deref(), Some("LICENSE-LLVM-OpenMP"));
        assert_eq!(unpacked_name("llama-cli.exe"), None, "the other tools stay out");
        assert_eq!(unpacked_name("llama-connect.exe"), None);
        assert_eq!(unpacked_name("dir/"), None);
        assert_eq!(unpacked_name("..dll"), None);
        assert_eq!(unpacked_name("C:evil.dll"), None);
        // A real zip: the server and DLLs out, the rest left in it.
        let dir = scratch("engine-unpack");
        let zip_path = dir.join("b.zip");
        {
            let f = std::fs::File::create(&zip_path).unwrap();
            let mut z = zip::ZipWriter::new(f);
            let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            for (name, body) in [("llama-server.exe", "server"), ("ggml.dll", "dll"), ("llama-cli.exe", "cli"), ("LICENSE-x", "lic")] {
                z.start_file(name, opts).unwrap();
                std::io::Write::write_all(&mut z, body.as_bytes()).unwrap();
            }
            z.finish().unwrap();
        }
        let out = dir.join("out");
        assert_eq!(unpack(&zip_path, &out, true).unwrap(), 3);
        assert_eq!(std::fs::read_to_string(out.join(SERVER_EXE)).unwrap(), "server");
        assert!(!out.join("llama-cli.exe").exists());
        // No server in a build's zip: refused.
        let cudart = dir.join("c.zip");
        {
            let f = std::fs::File::create(&cudart).unwrap();
            let mut z = zip::ZipWriter::new(f);
            z.start_file("cudart64_13.dll", zip::write::SimpleFileOptions::default()).unwrap();
            std::io::Write::write_all(&mut z, b"rt").unwrap();
            z.finish().unwrap();
        }
        assert!(unpack(&cudart, &dir.join("x"), true).is_err());
        assert_eq!(unpack(&cudart, &dir.join("rt"), false).unwrap(), 1);
        assert!(has_cudart(&dir.join("rt")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_vulkan_build_is_pinned_to_the_card_when_it_sees_integrated_graphics_too() {
        let out = "ggml_vulkan: Found 2 Vulkan devices:\nAvailable devices:\n  Vulkan0: NVIDIA GeForce RTX 5070 Ti (15995 MiB, 15227 MiB free)\n  Vulkan1: AMD Radeon(TM) Graphics (48756 MiB, 46319 MiB free)\n";
        let d = parse_devices(out);
        assert_eq!(d, vec![("Vulkan0".into(), "NVIDIA GeForce RTX 5070 Ti".into()), ("Vulkan1".into(), "AMD Radeon(TM) Graphics".into())]);
        assert_eq!(device_for(&d, Some("NVIDIA GeForce RTX 5070 Ti")).as_deref(), Some("Vulkan0"));
        assert_eq!(device_for(&d, Some("Some other card")), None);
        assert_eq!(device_for(&d[..1], Some("NVIDIA GeForce RTX 5070 Ti")), None, "one device: nothing to pin");
        assert!(parse_devices("Available devices:\n  (none)\n").is_empty());
    }

    #[test]
    fn a_switch_keeps_the_way_back_and_a_failed_first_start_goes_back() {
        let mut m = Marker { current: Some(inst(11_960, Variant::Cuda13)), ..Default::default() };
        switch_to(&mut m, inst(11_980, Variant::Cuda13), Some(1.08), 100);
        assert_eq!(m.current.as_ref().unwrap().build, Build(11_980));
        assert_eq!(m.previous.as_ref().unwrap().build, Build(11_960));
        assert!(m.unproven);
        assert_eq!(event_line(m.event.as_ref().unwrap()), "Updated to b11980 · faster on your card");
        // Its first start failed: back on b11960, kept to try again.
        let failed = roll_back(&mut m, 200).unwrap();
        assert_eq!(failed.build, Build(11_980));
        assert_eq!(m.current.as_ref().unwrap().build, Build(11_960));
        assert!(m.previous.is_none() && !m.unproven);
        assert_eq!(m.retry.as_ref().unwrap().build, Build(11_980));
        assert_eq!(m.failures.get("b11980-cuda13"), Some(&1));
        assert_eq!(event_line(m.event.as_ref().unwrap()), "b11980 didn't start: back on b11960");
        // The next launch tries it again.
        assert!(retry_at_launch(&mut m));
        assert!(m.unproven && m.current.as_ref().unwrap().build == Build(11_980));
        assert_eq!(m.previous.as_ref().unwrap().build, Build(11_960));
        // Three sessions down: refused, never again.
        roll_back(&mut m, 300).unwrap();
        assert!(retry_at_launch(&mut m));
        roll_back(&mut m, 400).unwrap();
        assert!(m.is_refused("b11980-cuda13"));
        assert!(m.retry.is_none());
        assert!(!retry_at_launch(&mut m));
        // Proven builds have no way back automatically.
        let mut p = Marker { current: Some(inst(1, Variant::Cpu)), ..Default::default() };
        assert!(roll_back(&mut p, 1).is_none());
    }

    #[test]
    fn when_the_one_before_wont_start_either_the_new_build_goes_back_in_uncounted() {
        let mut m = Marker { current: Some(inst(11_960, Variant::Cpu)), ..Default::default() };
        switch_to(&mut m, inst(11_980, Variant::Cpu), Some(1.0), 1);
        let event = m.event.clone();
        let failed = roll_back(&mut m, 2).unwrap();
        put_back(&mut m, failed, event);
        assert_eq!(m.current.as_ref().unwrap().build, Build(11_980));
        assert_eq!(m.previous.as_ref().unwrap().build, Build(11_960));
        assert!(m.unproven && m.failures.is_empty() && m.refused.is_empty() && m.retry.is_none());
        // A migration from llamafile: the way back is llamafile (no previous).
        let mut lf = Marker::default();
        switch_to(&mut lf, inst(11_982, Variant::Cuda13), Some(1.15), 1);
        assert_eq!(event_line(lf.event.as_ref().unwrap()), "Moved to llama.cpp b11982 · faster on your card");
        roll_back(&mut lf, 2).unwrap();
        assert!(lf.current.is_none(), "llamafile runs again");
        assert_eq!(event_line(lf.event.as_ref().unwrap()), "b11982 didn't start: back on llamafile");
    }

    #[test]
    fn going_back_by_hand_refuses_the_build_left() {
        let mut m = Marker { current: Some(inst(11_960, Variant::Cuda13)), ..Default::default() };
        switch_to(&mut m, inst(11_980, Variant::Cuda13), Some(1.0), 1);
        assert_eq!(back_target(&m, Some(Version(0, 10, 6))).as_deref(), Some("b11960"));
        go_back(&mut m, true, 2).unwrap();
        assert_eq!(m.current.as_ref().unwrap().build, Build(11_960));
        assert!(m.is_refused("b11980-cuda13"));
        assert_eq!(event_line(m.event.as_ref().unwrap()), "Went back to b11960 · b11980 won't be installed again");
        // Then to llamafile, when it's on disk; else nowhere.
        assert_eq!(back_target(&m, Some(Version(0, 10, 6))).as_deref(), Some("llamafile 0.10.6"));
        assert_eq!(back_target(&m, None), None);
        assert!(go_back(&mut m.clone(), false, 3).is_err());
        go_back(&mut m, true, 3).unwrap();
        assert!(m.current.is_none());
        assert_eq!(event_line(m.event.as_ref().unwrap()), "Went back to llamafile · b11960 won't be installed again");
        assert_eq!(back_target(&m, Some(Version(0, 10, 6))), None, "on llamafile: nothing further back");
    }

    #[test]
    fn settings_says_what_happened_in_plain_words() {
        let refused = |why, ratio| Event::Refused { build: Build(11_990), why, ratio, staying: Some(Build(11_960)), at: 1 };
        assert_eq!(event_line(&refused(Why::DidntStart, None)), "b11990 didn't start here: staying on b11960");
        assert_eq!(event_line(&refused(Why::ToolCall, None)), "b11990 couldn't use tools here: staying on b11960");
        assert_eq!(event_line(&refused(Why::Slower, Some(0.72))), "b11990 was slower here (72% of the speed): staying on b11960");
        assert_eq!(event_line(&refused(Why::Vision, None)), "b11990 couldn't read screenshots here: staying on b11960");
        let lf = Event::Refused { build: Build(11_982), why: Why::NoAnswer, ratio: None, staying: None, at: 1 };
        assert_eq!(event_line(&lf), "b11982 didn't answer here: staying on llamafile");
        let up = |ratio, on_card| Event::Updated { to: Build(11_980), from: Some(Build(11_960)), ratio, on_card, at: 1 };
        assert_eq!(event_line(&up(Some(1.0), true)), "Updated to b11980 · as fast as before");
        assert_eq!(event_line(&up(Some(0.9), true)), "Updated to b11980 · a little slower, within Yap's limit");
        assert_eq!(event_line(&up(Some(1.2), false)), "Updated to b11980 · faster on this PC");
        // The marker reads back what it wrote; an empty history isn't written.
        let m = Marker { event: Some(up(Some(1.1), true)), ..Default::default() };
        let json = serde_json::to_string(&m).unwrap();
        assert!(!json.contains("failures") && !json.contains("mtp_failed") && !json.contains("retry"), "{json}");
        assert_eq!(serde_json::from_str::<Marker>(&json).unwrap(), m);
        assert_eq!(serde_json::from_str::<Marker>("{}").unwrap(), Marker::default(), "an empty marker reads");
    }

    #[test]
    fn the_active_engine_is_the_markers_build_else_llamafile() {
        let llm = scratch("engine-active");
        assert_eq!(active_in(&llm), None);
        std::fs::write(llm.join(super::super::RUNTIME_FILENAME), "lf").unwrap();
        assert!(matches!(active_in(&llm), Some(Engine::Llamafile { .. })));
        assert!(!llama_cpp_active_in(&llm));
        let edir = engine_dir_in(&llm);
        let b = inst(11_982, Variant::Cpu);
        write_marker(&edir, &Marker { current: Some(b.clone()), ..Default::default() }).unwrap();
        // Its folder isn't there (yet): llamafile still.
        assert!(matches!(active_in(&llm), Some(Engine::Llamafile { .. })));
        std::fs::create_dir_all(b.dir(&edir)).unwrap();
        std::fs::write(b.dir(&edir).join(SERVER_EXE), "srv").unwrap();
        let e = active_in(&llm).unwrap();
        assert_eq!(e.label(), "llama.cpp b11982");
        assert_eq!(e.runtime(), Runtime::LlamaCpp(Build(11_982), Variant::Cpu));
        assert_eq!(e.log_path(&llm), edir.join(SERVER_LOG));
        assert!(llama_cpp_active_in(&llm));
        // Tidy keeps the current, previous and retry folders, and nothing else.
        for d in ["b11960-cpu", "b11990-cpu.unpacking", "cudart-abc", "downloads"] {
            std::fs::create_dir_all(edir.join(d)).unwrap();
        }
        let gone = tidy(&edir, &read_marker(&edir));
        assert!(gone.contains(&"b11960-cpu".to_string()) && gone.contains(&"b11990-cpu.unpacking".to_string()) && gone.contains(&"cudart-abc".to_string()));
        assert!(b.dir(&edir).is_dir() && edir.join("downloads").is_dir(), "the current build and the downloads stay");
        let _ = std::fs::remove_dir_all(&llm);
    }
}
