//! On-device AI cleanup sidecar (llamafile).
//!
//! Embedding llama.cpp in-process is a NO-GO: `whisper-rs-sys` (via transcribe-rs)
//! and `llama-cpp-2` each statically vendor a different, ABI-incompatible ggml, so
//! linking both yields duplicate-symbol failures (see the spike). Instead we run a
//! **llamafile** as a child process — a single-file, GPU-capable, OpenAI-compatible
//! server. Yap's existing cleanup client (`llm.rs`) just POSTs to its localhost
//! URL, so the whole cleanup stack (guardrails, presets, routing) is reused with
//! ZERO changes. Fully local, no cloud.
//!
//! This module owns the process lifecycle: spawn (hidden), wait for `/health`,
//! expose the base URL, and kill on stop/exit.

use std::path::{Path, PathBuf};
use crate::privacy::net::Recorded as _;
use std::process::Child;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};

/// Provider id (in `YapConfig::pp_provider`) that selects the managed sidecar.
pub const PROVIDER_ONDEVICE: &str = "ondevice";

/// The GGUF the sidecar loads when no other is picked (`pp_local_model`
/// empty). Qwen2.5-1.5B-Instruct Q4_K_M — Apache-2.0, ~1.04 GB, strong
/// instruction-following for its size, about a second a Chat turn on a CPU.
/// The model **recommended** for a PC is [`recommended_id`]'s: Qwen3.5-4B
/// where the graphics card can run it.
pub const MODEL_FILENAME: &str = "qwen2.5-1.5b-instruct-q4_k_m.gguf";

/// Human-readable names surfaced in the Settings UI (via `local_llm_status`) so
/// the user is told exactly what gets downloaded and what runs on their machine.
pub const MODEL_DISPLAY: &str = "Qwen2.5 1.5B Instruct";
pub const ENGINE_DISPLAY: &str = "Mozilla llamafile (llama.cpp)";

/// Download source + pinned SHA-256 for the GGUF cleanup model (HuggingFace).
pub(crate) const MODEL_URL: &str =
    "https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct-GGUF/resolve/main/qwen2.5-1.5b-instruct-q4_k_m.gguf";
pub(crate) const MODEL_SHA256: &str = "6a1a2eb6d15622bf3c96857206351ba97e1af16c30d7a74ee38970e434e9407e";

/// The llamafile runtime Yap downloads: **0.10.6** (15 Sep 2026, llama.cpp
/// b10441), the full build — it bundles the GPU backends, so it works on a
/// clean machine with no dev tools, with the processor as the fallback. An
/// Actually-Portable-Executable that runs on Windows when saved with a
/// `.exe` name. The SHA-256 is the GitHub release's published digest
/// (368,094,430 bytes). Over 0.10.3 (measured on an RTX 5070 Ti, Qwen3.5-4B,
/// docs/model-evals.md "llamafile 0.10.6"): CUDA graphs (generation ~115 →
/// ~165 tokens/s, a dictation's cleanup ~300 → ~245 ms), a much smaller
/// compute buffer (~550 MiB less on the card), the June tool-call grammar
/// fixes, context checkpoints at every user message, the Qwen3.5 MTP fix.
/// Installs that have an older runtime get this one in the background
/// ([`spawn_runtime_upgrade`]).
pub const RUNTIME_VERSION: &str = "0.10.6";
pub(crate) const RUNTIME_URL: &str =
    "https://github.com/mozilla-ai/llamafile/releases/download/0.10.6/llamafile-0.10.6";
pub(crate) const RUNTIME_SHA256: &str = "d579f61dcd3a306f518e6d90e599d77793ed5f09543023d09c96ad35fcfa63f0";

/// The runtime Yap downloaded before markers ([`RUNTIME_MARKER`]): a
/// `llamafile.exe` with no marker beside it is this one (Mozilla-Ocho's
/// 0.10.3, SHA-256 `e6d4041a…`, July–October 2026).
pub const UNMARKED_RUNTIME_VERSION: &str = "0.10.3";

/// The llamafile runtime executable name on disk.
#[cfg(windows)]
pub const RUNTIME_FILENAME: &str = "llamafile.exe";
#[cfg(not(windows))]
pub const RUNTIME_FILENAME: &str = "llamafile";

/// Beside the runtime: which version it is and its SHA-256, written after
/// every verified download, so Yap knows what's on disk without hashing
/// 368 MB at every start.
pub const RUNTIME_MARKER: &str = "llamafile.json";

/// The new runtime while it downloads and waits to be swapped in, and the
/// one it replaced until the new one has started once (the way back).
#[cfg(windows)]
const RUNTIME_NEXT: &str = "llamafile-next.exe";
#[cfg(not(windows))]
const RUNTIME_NEXT: &str = "llamafile-next";
#[cfg(windows)]
const RUNTIME_PREVIOUS: &str = "llamafile-previous.exe";
#[cfg(not(windows))]
const RUNTIME_PREVIOUS: &str = "llamafile-previous";
const MARKER_PREVIOUS: &str = "llamafile-previous.json";

/// A llamafile version, "0.10.6". Ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u32, pub u32, pub u32);

impl Version {
    /// "0.10.6", "v0.10.6", "llamafile v0.10.6" (what `--version` prints).
    /// Pure.
    pub fn parse(s: &str) -> Option<Version> {
        let word = s.split_whitespace().find(|w| w.trim_start_matches('v').starts_with(|c: char| c.is_ascii_digit()))?;
        let mut parts = word.trim_start_matches('v').split('.').map(|p| p.trim().parse::<u32>());
        let v = Version(parts.next()?.ok()?, parts.next()?.ok()?, parts.next().unwrap_or(Ok(0)).ok()?);
        Some(v)
    }

    /// The runtime that brought the buffers and the MTP that Yap's memory
    /// estimate and MTP drafting are calibrated for (0.10.6).
    pub fn is_current(self) -> bool {
        self >= Version(0, 10, 6)
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// [`RUNTIME_MARKER`]'s contents: the runtime on disk, and what this PC has
/// learnt about runtimes and MTP — carried from marker to marker through
/// every swap and roll-back (never config.json: Settings' wholesale saves).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Marker {
    pub version: String,
    /// The SHA-256 the download was verified against ("" when unknown: the
    /// old runtime, put back after a new one wouldn't start).
    pub sha256: String,
    /// Swapped in, not yet started once ([`swap_in`]): a start that fails
    /// on it goes back to the previous runtime.
    #[serde(default)]
    pub unproven: bool,
    /// Runtimes (SHA-256) that downloaded fine but wouldn't run on this PC
    /// [`REFUSE_AFTER`] times: never downloaded again.
    #[serde(default)]
    pub refused: Vec<String>,
    /// Sessions in which a downloaded runtime (by SHA-256) wouldn't start,
    /// or wouldn't say its version: refused at [`REFUSE_AFTER`].
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub failures: std::collections::BTreeMap<String, u32>,
    /// "<runtime>/<GGUF file>" pairs MTP failed with here — the model
    /// wouldn't start with it, or landed (partly) on the processor
    /// ([`mtp_key`]): never with MTP again on that runtime.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mtp_failed: Vec<String>,
}

impl Marker {
    /// `self` with what this PC learnt, as `from` has it.
    fn learnt_from(mut self, from: &Marker) -> Marker {
        self.refused = from.refused.clone();
        self.failures = from.failures.clone();
        self.mtp_failed = from.mtp_failed.clone();
        self
    }
}

/// A downloaded runtime that fails in this many sessions (its first start,
/// or `--version`) is refused on this PC; until then the previous one
/// serves, and the next launch tries the new one again.
pub const REFUSE_AFTER: u32 = 3;

/// Write the marker for the runtime in `dir` (after a verified download).
/// (The model evaluations' own runtime, `eval/local.rs`: debug builds.)
#[cfg(debug_assertions)]
pub fn write_marker(dir: &Path, m: &Marker) -> std::io::Result<()> {
    write_marker_at(&dir.join(RUNTIME_MARKER), m)
}

fn read_marker_at(path: &Path) -> Option<Marker> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn write_marker_at(path: &Path, m: &Marker) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(m).map_err(std::io::Error::other)?;
    crate::config::atomic_write(path, &json).map_err(std::io::Error::other)
}

/// Change the marker in `dir` (none: the unmarked 0.10.3's, made).
fn update_marker(dir: &Path, f: impl FnOnce(&mut Marker)) -> std::io::Result<Marker> {
    let path = dir.join(RUNTIME_MARKER);
    let mut m = read_marker_at(&path)
        .unwrap_or_else(|| Marker { version: UNMARKED_RUNTIME_VERSION.to_string(), ..Default::default() });
    f(&mut m);
    write_marker_at(&path, &m)?;
    Ok(m)
}

/// Note a session in which the runtime `sha256` failed (once a session:
/// the caller's to say). Returns how many sessions now; at
/// [`REFUSE_AFTER`] it's refused.
pub fn count_failure(dir: &Path, sha256: &str) -> u32 {
    if sha256.is_empty() {
        return 0;
    }
    let mut n = 0;
    let _ = update_marker(dir, |m| {
        let c = m.failures.entry(sha256.to_ascii_lowercase()).or_insert(0);
        *c += 1;
        n = *c;
        if n >= REFUSE_AFTER && !m.refused.iter().any(|r| r.eq_ignore_ascii_case(sha256)) {
            m.refused.push(sha256.to_ascii_lowercase());
        }
    });
    n
}

/// Take back a failure [`count_failure`] noted (and the refusal it made):
/// it wasn't the runtime's fault after all.
fn uncount_failure(dir: &Path, sha256: &str) {
    let _ = update_marker(dir, |m| {
        let key = sha256.to_ascii_lowercase();
        if let Some(c) = m.failures.get_mut(&key) {
            *c = c.saturating_sub(1);
            if *c == 0 {
                m.failures.remove(&key);
            }
        }
        m.refused.retain(|r| !r.eq_ignore_ascii_case(sha256));
    });
}

/// Is the runtime `sha256` refused in `dir`'s marker?
fn refused_in(dir: &Path, sha256: &str) -> bool {
    read_marker_at(&dir.join(RUNTIME_MARKER)).is_some_and(|m| m.refused.iter().any(|r| r.eq_ignore_ascii_case(sha256)))
}

/// The runtime in `dir`: its version (the marker's; none = 0.10.3) — `None`
/// when there's no runtime there.
pub fn runtime_version_in(dir: &Path) -> Option<Version> {
    if !dir.join(RUNTIME_FILENAME).is_file() {
        return None;
    }
    let v = read_marker_at(&dir.join(RUNTIME_MARKER)).and_then(|m| Version::parse(&m.version));
    Some(v.unwrap_or_else(|| Version::parse(UNMARKED_RUNTIME_VERSION).expect("a version")))
}

/// The installed runtime's version (`None`: not installed).
pub fn runtime_version() -> Option<Version> {
    runtime_version_in(&llm_dir())
}

/// Should the runtime on disk be replaced by the one pinned (`want_sha`)?
/// Only when there is one (`exe`), its marker doesn't say it's that one, and
/// that one wasn't refused here before. Pure.
pub fn upgrade_due(exe: bool, marker: Option<&Marker>, want_sha: &str) -> bool {
    exe && !marker.is_some_and(|m| m.sha256.eq_ignore_ascii_case(want_sha) || m.refused.iter().any(|r| r.eq_ignore_ascii_case(want_sha)))
}

/// Put the downloaded runtime (`llamafile-next.exe`) in place in `dir`: the
/// current one (and its marker) kept as the previous, the new one's marker
/// written, `unproven` until it has started once. Nothing runs from `dir`
/// meanwhile (the caller stopped the sidecar). A failure puts the old one
/// back.
pub fn swap_in(dir: &Path, version: &str, sha256: &str) -> std::io::Result<()> {
    let (cur, next, prev) = (dir.join(RUNTIME_FILENAME), dir.join(RUNTIME_NEXT), dir.join(RUNTIME_PREVIOUS));
    let (marker, prev_marker) = (dir.join(RUNTIME_MARKER), dir.join(MARKER_PREVIOUS));
    if !next.is_file() {
        return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "no new runtime to swap in"));
    }
    let learnt = read_marker_at(&marker).unwrap_or_default();
    let _ = std::fs::remove_file(&prev);
    let _ = std::fs::remove_file(&prev_marker);
    let had_old = cur.is_file();
    if had_old {
        std::fs::rename(&cur, &prev)?;
    }
    if marker.is_file() {
        let _ = std::fs::rename(&marker, &prev_marker);
    }
    if let Err(e) = std::fs::rename(&next, &cur) {
        if had_old {
            let _ = std::fs::rename(&prev, &cur);
        }
        let _ = std::fs::rename(&prev_marker, &marker);
        return Err(e);
    }
    let mut m = Marker { version: version.to_string(), sha256: sha256.to_string(), unproven: true, ..Default::default() }
        .learnt_from(&learnt);
    m.refused.retain(|r| !r.eq_ignore_ascii_case(sha256));
    write_marker_at(&marker, &m)
}

/// The swapped-in runtime has started: the previous one and its marker go.
pub fn prove(dir: &Path) {
    let path = dir.join(RUNTIME_MARKER);
    if let Some(mut m) = read_marker_at(&path).filter(|m| m.unproven) {
        m.unproven = false;
        let _ = write_marker_at(&path, &m);
    }
    let _ = std::fs::remove_file(dir.join(RUNTIME_PREVIOUS));
    let _ = std::fs::remove_file(dir.join(MARKER_PREVIOUS));
}

/// The swapped-in runtime didn't start: the previous one goes back in place
/// (with its marker, or none — the unmarked 0.10.3) and the new one is kept
/// as `llamafile-next.exe`, for [`swap_in`] again (the old one won't start
/// either: the fault was elsewhere) or the next launch's try. What this PC
/// learnt goes with it — the caller counted this failure first
/// ([`count_failure`]). A new one that's gone (an antivirus took it) leaves
/// whatever waits as `llamafile-next.exe` alone. Returns the new one's
/// marker; `None` when there was no previous one to go back to.
pub fn roll_back(dir: &Path) -> std::io::Result<Option<Marker>> {
    let (cur, next, prev) = (dir.join(RUNTIME_FILENAME), dir.join(RUNTIME_NEXT), dir.join(RUNTIME_PREVIOUS));
    let (marker, prev_marker) = (dir.join(RUNTIME_MARKER), dir.join(MARKER_PREVIOUS));
    if !prev.is_file() {
        return Ok(None);
    }
    let failed = read_marker_at(&marker).unwrap_or_default();
    if cur.is_file() {
        let _ = std::fs::remove_file(&next);
        std::fs::rename(&cur, &next)?;
        if let Err(e) = std::fs::rename(&prev, &cur) {
            let _ = std::fs::rename(&next, &cur);
            return Err(e);
        }
    } else {
        std::fs::rename(&prev, &cur)?;
    }
    let mut old = read_marker_at(&prev_marker)
        .unwrap_or(Marker { version: UNMARKED_RUNTIME_VERSION.to_string(), ..Default::default() })
        .learnt_from(&failed);
    old.unproven = false;
    let _ = std::fs::remove_file(&prev_marker);
    write_marker_at(&marker, &old)?;
    Ok(Some(failed))
}

/// Put `dir` right at launch, before anything starts: a missing
/// `llamafile.exe` comes back from the previous one (a roll-back or swap
/// cut off) or from a whole `llamafile-next.exe` of the pinned runtime
/// (`next_sha` is its hash, `None` = not worth hashing: there's none, or
/// the caller didn't); a previous one kept past a proven start goes. Returns
/// what it did, for the log. Pure but for the files.
pub fn repair_runtime(dir: &Path, next_sha: Option<&str>, pinned: (&str, &str)) -> Option<&'static str> {
    let (cur, next, prev) = (dir.join(RUNTIME_FILENAME), dir.join(RUNTIME_NEXT), dir.join(RUNTIME_PREVIOUS));
    let (marker, prev_marker) = (dir.join(RUNTIME_MARKER), dir.join(MARKER_PREVIOUS));
    if !cur.is_file() {
        let learnt = read_marker_at(&marker).unwrap_or_default();
        if prev.is_file() && std::fs::rename(&prev, &cur).is_ok() {
            let old = read_marker_at(&prev_marker)
                .unwrap_or(Marker { version: UNMARKED_RUNTIME_VERSION.to_string(), ..Default::default() })
                .learnt_from(&learnt);
            let _ = write_marker_at(&marker, &Marker { unproven: false, ..old });
            let _ = std::fs::remove_file(&prev_marker);
            return Some("restored the previous runtime");
        }
        let (version, sha) = pinned;
        if next.is_file() && next_sha.is_some_and(|h| h.eq_ignore_ascii_case(sha)) {
            // The marker first: never a runtime on disk its marker doesn't
            // describe.
            let m = Marker { version: version.to_string(), sha256: sha.to_string(), unproven: false, ..Default::default() }
                .learnt_from(&learnt);
            if write_marker_at(&marker, &m).is_ok() && std::fs::rename(&next, &cur).is_ok() {
                return Some("restored the downloaded runtime");
            }
        }
        return None;
    }
    let proven = !read_marker_at(&marker).is_some_and(|m| m.unproven);
    if proven && prev.is_file() {
        let _ = std::fs::remove_file(&prev);
        let _ = std::fs::remove_file(&prev_marker);
        return Some("deleted the previous runtime");
    }
    None
}

/// Cold model-load can take a while on first start; cap the health wait.
const HEALTH_TIMEOUT_SECS: u64 = 120;
/// The wait for a second try ([`Attempt::AGAIN`]): a first run of a new
/// runtime can take minutes (an antivirus reading 368 MB), a big model on a
/// slow disk too.
const HEALTH_TIMEOUT_AGAIN_SECS: u64 = 300;

/// The context the sidecar serves when none is set (`local_context` 0; one
/// cache shared by its slots): room for the structured cleanup prompt +
/// one-shots (~2.1k tokens) plus a long dictation, and for a Chat turn's
/// tools, notes and history. What the meeting notes' budgets assume
/// (meeting_summary.rs).
pub const DEFAULT_CONTEXT: u32 = 8192;
/// The least the sidecar is started with (the cleanup's prompt alone is
/// ~2.1k tokens) — unless the model itself was trained for less.
pub const MIN_CONTEXT: u32 = 2048;
/// The most `local_context` is taken as (a header claiming more is still
/// held to this).
pub const MAX_CONTEXT: u32 = 1 << 20;
/// What the pickers offer, up to the model's own maximum (and that maximum
/// itself when it's between two of these).
pub const CONTEXT_CHOICES: [u32; 7] = [4_096, 8_192, 16_384, 32_768, 65_536, 131_072, 262_144];

/// The context the sidecar starts with: `configured` (0 = the default),
/// within [`MIN_CONTEXT`]..=[`MAX_CONTEXT`], and never more than the model
/// was trained for (`model_max`, its GGUF's `context_length`). Pure.
pub fn effective_context(configured: u32, model_max: Option<u64>) -> u32 {
    let want = if configured == 0 {
        DEFAULT_CONTEXT
    } else {
        configured.clamp(MIN_CONTEXT, MAX_CONTEXT)
    };
    match model_max {
        Some(m) if m > 0 => want.min(u32::try_from(m).unwrap_or(u32::MAX)),
        _ => want,
    }
}

/// The contexts offered for a model trained for `model_max` (unknown: up
/// to 32K), with `current` always among them. Pure.
pub fn context_choices(model_max: Option<u64>, current: u32) -> Vec<u32> {
    let max = model_max.map(|m| m.min(MAX_CONTEXT as u64) as u32);
    let top = max.unwrap_or(32_768);
    let mut v: Vec<u32> = CONTEXT_CHOICES.iter().copied().filter(|&c| c <= top).collect();
    if let Some(m) = max {
        if (MIN_CONTEXT..=CONTEXT_CHOICES[CONTEXT_CHOICES.len() - 1]).contains(&m) {
            v.push(m);
        }
    }
    v.push(current);
    v.sort_unstable();
    v.dedup();
    v
}

/// What a model was trained for: its GGUF's header, else the curated
/// entry's figure (a file not downloaded yet, or one being written).
pub fn trained_context(path: &Path) -> Option<u64> {
    crate::ai::gguf::model_file(path)
        .and_then(|f| f.shape.context_length)
        .or_else(|| {
            let file = path.file_name()?.to_str()?;
            CURATED_MODELS
                .iter()
                .find(|m| m.filename == file)
                .map(|m| m.max_context as u64)
        })
}

/// The context the on-device model gets with this config.
pub fn context_for(cfg: &crate::config::YapConfig) -> u32 {
    effective_context(cfg.local_context, trained_context(&active_model_path(cfg)))
}

/// The context the on-device model works with now: the running sidecar's,
/// else what it will start with. For the model chip.
pub fn context_now(cfg: &crate::config::YapConfig) -> u32 {
    serving_context().unwrap_or_else(|| context_for(cfg))
}

/// The most the 8K budgets grow (a 32K context; beyond it a small model's
/// answers over raw text get slow and no better).
pub const MAX_BUDGET_FACTOR: usize = 4;

/// How many times its 8K budgets a call may use on a model serving
/// `context` tokens: whole multiples of [`DEFAULT_CONTEXT`], 1–4. The
/// budgets leave the reply's room at 8K, so `factor × budget` + that room
/// stays inside `factor × 8K` ≤ the context. Pure.
pub fn budget_factor(context: u32) -> usize {
    ((context / DEFAULT_CONTEXT) as usize).clamp(1, MAX_BUDGET_FACTOR)
}

/// [`budget_factor`] for a call to `provider` at `base_url`: the running
/// on-device model's context, when the call really goes to it; 1 for
/// anything else (a cloud free tier counts tokens a minute; a self-hosted
/// server's window is its own; "ondevice" fallen back to the endpoint
/// underneath is that endpoint).
pub fn budget_factor_for(provider: &str, base_url: &str) -> usize {
    let on_it = provider == PROVIDER_ONDEVICE && self::base_url().is_some_and(|u| u == base_url);
    if !on_it {
        return 1;
    }
    serving_context().map(budget_factor).unwrap_or(1)
}

/// The server's **host prompt cache**, MiB: **off**. llama.cpp's default
/// (8 GB) saves every idle slot's prompt to RAM and clears the slot when
/// another request starts, then copies it back for the next request that
/// shares it. Over a model evaluation on the processor a 4B server
/// (Qwen3-4B) peaked at 6.7 GB against llama.cpp's projected 4.1 GB with it
/// (Qwen3.5-4B: 4.4 GB, 3.85 GB without), and the copy back is slow: a dictation's cleanup
/// right after a Chat turn took **1.3–1.4 s instead of 0.13 s** on the 1.5B
/// on an RTX 5070 Ti (0.7 s instead of 0.33 s on the 4B; 1.35 s instead of
/// 0.85 s on a processor), every time (`yap.exe eval --cleanup`,
/// docs/model-evals.md). Off, each slot keeps its own cache in the shared
/// context: the cleanup's prompt stays where it was, and when a long Chat
/// turn needs the room llama.cpp clears an idle slot (the next cleanup reads
/// its ~2k-token prompt again, ~0.2 s on a card). 0 = off.
pub const CACHE_RAM_MIB: u32 = 0;

/// Per-slot **context checkpoints** for hybrid models (Qwen3.5's recurrent
/// layers can't be rolled back, so llama.cpp saves their state at points of
/// the prompt: ~50 MB each for the 4B). Default 32 a slot (up to 1.6 GB a
/// slot); the cleanup needs the one at its last message, a tool loop's next
/// step the latest, so 4 a slot measured the same (cleanup 0.32–0.36 s).
pub const CTX_CHECKPOINTS: u32 = 4;

/// How a server runs a model beyond its context: the runtime's version (the
/// arguments it understands) and whether MTP drafting is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plan {
    pub runtime: Version,
    pub mtp: bool,
}

impl Plan {
    /// The memory estimate's view of it (`gguf::estimate`).
    pub fn server(self) -> crate::ai::gguf::Server {
        let current = self.runtime.is_current();
        crate::ai::gguf::Server { current, mtp: current && self.mtp }
    }
}

/// The llamafile server's arguments, for Yap's sidecar and for the model
/// evaluations (`eval/local.rs`), so both run a model the same way.
/// `ngl`: layers on the GPU (999 = all that fit; 0 = the processor).
///
/// - `--reasoning off`: no thinking, whatever the model. Qwen3.5 thinks by
///   default and, so, loops and times out on Chat (and slows every dictation's
///   cleanup); the cleanup, Chat and the meeting notes all want the answer
///   only. llama.cpp renders the template with `enable_thinking` false.
/// - `--cache-ram` / `--ctx-checkpoints`: [`CACHE_RAM_MIB`], [`CTX_CHECKPOINTS`].
/// - `-lv 4`: the log names the layers that went to the GPU
///   (`load_tensors: offloaded 33/33 layers to GPU`, [`placement_in_log`]);
///   a request adds ~25 lines and never its text.
/// - `--spec-type draft-mtp` when `plan` says MTP (only on the runtime it's
///   measured on, 0.10.6; every other argument works on 0.10.3 and 0.10.6).
pub fn server_args(model: &Path, port: u16, ngl: u32, ctx: u32, plan: Plan) -> Vec<String> {
    let mut args = base_args(model, port, ngl, ctx);
    if plan.server().mtp {
        args.extend(["--spec-type".to_string(), "draft-mtp".to_string()]);
    }
    args
}

fn base_args(model: &Path, port: u16, ngl: u32, ctx: u32) -> Vec<String> {
    vec![
        // `--server` = OpenAI-compatible HTTP API only (no browser UI, no
        // --nobrowser flag needed — and that flag is invalid in llamafile 0.10.3).
        "--server".into(),
        "--host".into(),
        "127.0.0.1".into(),
        "--port".into(),
        port.to_string(),
        "-m".into(),
        model.to_string_lossy().into_owned(),
        "-ngl".into(),
        ngl.to_string(),
        "-c".into(),
        ctx.to_string(),
        "--reasoning".into(),
        "off".into(),
        "--cache-ram".into(),
        CACHE_RAM_MIB.to_string(),
        "--ctx-checkpoints".into(),
        CTX_CHECKPOINTS.to_string(),
        "-lv".into(),
        "4".into(),
    ]
}

/// Where a model's layers went, read from the server's log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Placement {
    /// Layers on the graphics card, of `layers`.
    pub gpu_layers: u32,
    pub layers: u32,
}

impl Placement {
    /// "gpu" (all of it), "partial", or "cpu" (none of it).
    pub fn kind(&self) -> &'static str {
        if self.layers > 0 && self.gpu_layers >= self.layers {
            "gpu"
        } else if self.gpu_layers > 0 {
            "partial"
        } else {
            "cpu"
        }
    }
}

/// Where the model went once the log says it loaded (`server_main: model
/// loaded`): its offload line, or — none, as when no layer went to a GPU —
/// the processor. `None` while the log doesn't say yet. Pure.
pub fn placement_after_load(log: &str) -> Option<Placement> {
    placement_in_log(log).or_else(|| log.contains("model loaded").then_some(Placement { gpu_layers: 0, layers: 0 }))
}

/// llama.cpp's `load_tensors: offloaded 29/29 layers to GPU` in a server log
/// (`-lv 4`), as a [`Placement`]; `None` when the log doesn't say (still
/// loading, or no layer went to a GPU: then llama.cpp writes no such line,
/// [`placement_after_load`]). Pure.
pub fn placement_in_log(log: &str) -> Option<Placement> {
    let line = log.lines().rev().find(|l| l.contains("offloaded") && l.contains("layers to GPU"))?;
    let rest = line.split("offloaded").nth(1)?.trim_start();
    let (done, of) = rest.split_once('/')?;
    let of: String = of.chars().take_while(|c| c.is_ascii_digit()).collect();
    Some(Placement { gpu_layers: done.trim().parse().ok()?, layers: of.parse().ok()? })
}

/// What a server's log (`-lv 4`) says about its context and memory.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LogFacts {
    /// `llama_context: n_ctx = 8192`: the context it really runs with.
    pub n_ctx: Option<u32>,
    /// Its buffers on the graphics card (model, KV, recurrent state,
    /// compute: `… CUDA0 KV buffer size = 256.00 MiB`)…
    pub gpu_mib: Option<u64>,
    /// …and in the processor's memory (`CPU_Mapped`, `CUDA_Host`, `CPU`).
    pub host_mib: Option<u64>,
}

/// [`LogFacts`] from a server log. Pure.
pub fn log_facts(log: &str) -> LogFacts {
    let number_before = |s: &str, unit: &str| -> Option<f64> {
        let end = s.find(unit)?;
        s[..end].trim().rsplit([' ', ',', '(']).next()?.parse().ok()
    };
    let mut f = LogFacts::default();
    let (mut gpu, mut host) = (0.0f64, 0.0f64);
    let (mut any_gpu, mut any_host) = (false, false);
    for line in log.lines() {
        if let Some(rest) = line.split("llama_context:").nth(1) {
            let rest = rest.trim_start();
            if let Some(after) = rest.strip_prefix("n_ctx") {
                if after.starts_with([' ', '=']) {
                    if let Some(v) = after.split('=').nth(1).and_then(|v| v.trim().parse().ok()) {
                        f.n_ctx = Some(v);
                    }
                }
            }
        }
        if let Some(rest) = line.split("buffer size =").nth(1) {
            if let Some(v) = number_before(rest, "MiB") {
                let name = line.split("buffer size =").next().unwrap_or("");
                if name.contains("CPU") || name.contains("_Host") {
                    host += v;
                    any_host = true;
                } else {
                    gpu += v;
                    any_gpu = true;
                }
            }
        }
    }
    f.gpu_mib = any_gpu.then(|| gpu.round() as u64);
    f.host_mib = any_host.then(|| host.round() as u64);
    f
}

static CHILD: Mutex<Option<Child>> = Mutex::new(None);
static PORT: AtomicU16 = AtomicU16::new(0);
static READY: AtomicBool = AtomicBool::new(false);
/// Bumped by every [`stop`] (Yap stopping the sidecar: a model switch, the
/// Stop button, a restart, quitting): a start under way that sees it move
/// was stopped, not failed — never retried, never blamed on the runtime.
static STOP_GEN: AtomicU64 = AtomicU64::new(0);
/// Yap is quitting ([`shutdown`]): nothing starts or rolls back any more.
static CLOSING: AtomicBool = AtomicBool::new(false);
/// Yap's own restart of the sidecar under way (a new context, a new
/// runtime): an on-device cleanup meanwhile gets no endpoint — the raw
/// text — instead of the configured cloud one ([`effective_endpoint_for`]).
static PLANNED_RESTART: AtomicU32 = AtomicU32::new(0);

/// A planned restart, until dropped ([`PLANNED_RESTART`]).
struct PlannedRestart;

impl PlannedRestart {
    fn begin() -> PlannedRestart {
        PLANNED_RESTART.fetch_add(1, Ordering::SeqCst);
        PlannedRestart
    }
}

impl Drop for PlannedRestart {
    fn drop(&mut self) {
        PLANNED_RESTART.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Is Yap restarting the sidecar itself right now (a new context or
/// runtime)? Its requests wait for it or go without.
pub fn restart_under_way() -> bool {
    PLANNED_RESTART.load(Ordering::SeqCst) > 0
}
/// The running sidecar's model id (its GGUF's name) and where its layers
/// went; set once it's ready.
static SERVING: Mutex<Option<Serving>> = Mutex::new(None);
/// One start at a time (a restart, a Chat waking the model, a Settings
/// switch, a runtime swap): a second waits, then finds it up.
static STARTING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Requests to the sidecar under way (`llm::post_chat_request` holds a
/// [`Busy`] for each): a runtime swap and a context restart wait for none.
static IN_FLIGHT: AtomicU32 = AtomicU32::new(0);
/// Requests to the sidecar started, ever: the warm-up gives way when this
/// moves.
static STARTED: AtomicU64 = AtomicU64::new(0);

/// A request to the sidecar under way, until dropped.
pub struct Busy;

impl Drop for Busy {
    fn drop(&mut self) {
        IN_FLIGHT.fetch_sub(1, Ordering::SeqCst);
    }
}

/// A request to `url` is starting: when that's the running sidecar it's
/// counted until the guard drops, and a warm-up under way gives way to it.
pub fn request_starting(url: &str) -> Option<Busy> {
    let busy = hold(url)?;
    STARTED.fetch_add(1, Ordering::SeqCst);
    Some(busy)
}

/// **Work of several requests** to `url` is under way (a Chat turn's tool
/// steps, with web searches and page reads between them; a meeting's
/// digests; a notepad helper that waits out a dictation): when that's the
/// running sidecar, it counts as busy until the guard drops — between the
/// requests too — so a context restart or a runtime swap never cuts it off.
pub fn hold(url: &str) -> Option<Busy> {
    if !base_url().is_some_and(|ours| url.trim_end_matches('/') == ours) {
        return None;
    }
    IN_FLIGHT.fetch_add(1, Ordering::SeqCst);
    Some(Busy)
}

/// Requests to the sidecar under way now.
pub fn requests_in_flight() -> u32 {
    IN_FLIGHT.load(Ordering::SeqCst)
}

#[derive(Debug, Clone)]
struct Serving {
    model_id: String,
    placement: Option<Placement>,
    /// The `-c` it was started with.
    ctx: u32,
    /// The runtime and MTP it runs with.
    plan: Plan,
    /// Its model is hybrid (recurrent layers beside attention, Qwen3.5):
    /// llama.cpp resumes its cache only from checkpoints.
    hybrid: bool,
    /// Its log's facts, read once it was ready.
    facts: LogFacts,
}

/// The model id Yap sends for a GGUF: its file name without `.gguf`
/// (`qwen2.5-1.5b-instruct-q4_k_m`, `Qwen_Qwen3.5-4B-Q4_K_M`). llamafile
/// serves the one model it was started with whatever the request names, so
/// the id is for Yap: `tools::supports_tools` reads the model's family and
/// size in it (the chat's tools are for models that can use them), and the
/// answers and usage are put down to the right model.
pub fn model_id(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| MODEL_FILENAME.trim_end_matches(".gguf").to_string())
}

/// The id of the model the sidecar serves (or will, once started): the
/// running one's, else the configured one's ([`active_model_path`]).
pub fn active_model_id(cfg: &crate::config::YapConfig) -> String {
    serving_id().unwrap_or_else(|| model_id(&active_model_path(cfg)))
}

fn serving_id() -> Option<String> {
    SERVING.lock().ok()?.as_ref().map(|s| s.model_id.clone())
}

/// The id of the model the running sidecar serves, `None` while it isn't
/// up (the Chat picker restarts it on another GGUF).
pub fn serving_model_id() -> Option<String> {
    serving_id()
}

/// Where the running sidecar's model went (GPU, partly, the processor);
/// `None` while it isn't running or the log didn't say.
pub fn placement() -> Option<Placement> {
    #[cfg(debug_assertions)]
    if let Some(s) = e2e_serving() {
        return Some(s.placement);
    }
    SERVING.lock().ok()?.as_ref().and_then(|s| s.placement)
}

/// The context the running sidecar serves: what its log says it runs
/// with, else what it was started with; `None` while it isn't running.
pub fn serving_context() -> Option<u32> {
    #[cfg(debug_assertions)]
    if let Some(s) = e2e_serving() {
        return Some(s.ctx);
    }
    SERVING.lock().ok()?.as_ref().map(|s| s.facts.n_ctx.unwrap_or(s.ctx))
}

/// Whether the running sidecar at `url` serves a hybrid model (`None`: no
/// sidecar there — another server, or it isn't up).
pub fn serving_hybrid_at(url: &str) -> Option<bool> {
    if !base_url().is_some_and(|ours| url.trim_end_matches('/') == ours) {
        return None;
    }
    SERVING.lock().ok()?.as_ref().map(|s| s.hybrid)
}

/// The running sidecar's runtime and MTP (`None` while it isn't running).
pub fn serving_plan() -> Option<Plan> {
    SERVING.lock().ok()?.as_ref().map(|s| s.plan)
}

/// The running sidecar's log facts (`None` while it isn't running).
fn serving_facts() -> Option<LogFacts> {
    #[cfg(debug_assertions)]
    if let Some(s) = e2e_serving() {
        return Some(LogFacts { n_ctx: Some(s.ctx), gpu_mib: s.used_mib, host_mib: s.used_mib });
    }
    SERVING.lock().ok()?.as_ref().map(|s| s.facts)
}

/// Is the sidecar running, as Settings shows it ([`is_running`], or the e2e
/// suite's stand-in, [`e2e_local_llm_serving`])?
pub fn shown_running() -> bool {
    #[cfg(debug_assertions)]
    if e2e_serving().is_some() {
        return true;
    }
    is_running()
}

/// The model file the sidecar serves, as Settings shows it.
pub fn shown_model_file(cfg: &crate::config::YapConfig) -> String {
    #[cfg(debug_assertions)]
    if let Some(s) = e2e_serving() {
        return s.file;
    }
    active_model_path(cfg)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// A model that wants a graphics card (or any GGUF bigger than 2 GB) whose
/// layers stayed on the processor, all or some: slow, and Settings says so,
/// pointing at the 1.5B. Pure.
pub fn slow_on_this_pc(file: &str, file_mb: u64, placement: Option<Placement>) -> bool {
    let big = match CURATED_MODELS.iter().find(|m| m.filename == file) {
        Some(m) => m.needs_gpu || m.size_mb > 2_000,
        None => file_mb > 2_000,
    };
    big && placement.is_some_and(|p| p.kind() != "gpu")
}

/// The e2e suite's stand-in for a running sidecar.
#[cfg(debug_assertions)]
#[derive(Debug, Clone)]
struct E2eServing {
    file: String,
    placement: Placement,
    ctx: u32,
    /// What its "log" says it takes (MiB), until it's restarted.
    used_mib: Option<u64>,
}

#[cfg(debug_assertions)]
static E2E_SERVING: Mutex<Option<E2eServing>> = Mutex::new(None);
/// Test mode: the stand-in's "restarts" (a context change applied to it).
#[cfg(debug_assertions)]
static E2E_RESTARTS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
/// Test mode: a dictation as the restart guard sees one (no microphone).
#[cfg(debug_assertions)]
static E2E_BUSY: AtomicBool = AtomicBool::new(false);

#[cfg(debug_assertions)]
fn e2e_serving() -> Option<E2eServing> {
    if !crate::e2e::active() {
        return None;
    }
    E2E_SERVING.lock().ok()?.clone()
}

/// Test mode only: Settings shows the sidecar as running `file` with
/// `gpu_layers` of `layers` on the graphics card, at a context of `ctx`
/// (default: the configured one), taking `used_mib` as its log would say —
/// no process: nothing is routed to it, and a context change "restarts" the
/// stand-in (`e2e_local_llm_restarts`); `file` absent = as it really is.
/// For the e2e suite's "Runs on the processor" line and the context control.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_local_llm_serving(
    file: Option<String>,
    gpu_layers: Option<u32>,
    layers: Option<u32>,
    ctx: Option<u32>,
    used_mib: Option<u64>,
) -> Result<(), String> {
    if !crate::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    let ctx = ctx.unwrap_or_else(|| context_for(&crate::config::load()));
    E2E_RESTARTS.store(0, Ordering::SeqCst);
    *E2E_SERVING.lock().unwrap_or_else(|p| p.into_inner()) = file.map(|f| E2eServing {
        file: f,
        placement: Placement { gpu_layers: gpu_layers.unwrap_or(0), layers: layers.unwrap_or(33) },
        ctx,
        used_mib,
    });
    Ok(())
}

/// Test mode only: how often the stand-in was restarted for a new context.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_local_llm_restarts() -> u32 {
    E2E_RESTARTS.load(Ordering::SeqCst)
}

/// Test mode only: the restart guard sees a dictation going on (`busy`)
/// or not — a context change waits for it, as for a real one.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_local_llm_busy(busy: bool) -> Result<(), String> {
    if !crate::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    E2E_BUSY.store(busy, Ordering::SeqCst);
    Ok(())
}

/// Where the sidecar's files live: `<data_dir>/llm/`. Users can drop their own
/// GGUF models in here and pick one in Settings.
pub fn llm_dir() -> PathBuf {
    crate::config::data_dir().join("llm")
}

/// Path to the llamafile runtime executable.
pub fn runtime_path() -> PathBuf {
    llm_dir().join(RUNTIME_FILENAME)
}

/// Path to the cleanup GGUF model.
pub fn model_path() -> PathBuf {
    llm_dir().join(MODEL_FILENAME)
}

/// Both the runtime and the active model are present on disk.
pub fn is_installed(cfg: &crate::config::YapConfig) -> bool {
    runtime_path().is_file() && active_model_path(cfg).is_file()
}

/// The model the sidecar should load: the user's custom GGUF (`pp_local_model`,
/// a filename inside `llm/`) when set and present, else the bundled default.
/// A stale selection (file deleted) falls back to the default rather than
/// breaking the sidecar.
pub fn active_model_path(cfg: &crate::config::YapConfig) -> PathBuf {
    if !cfg.pp_local_model.is_empty() {
        let p = llm_dir().join(&cfg.pp_local_model);
        if p.is_file() {
            return p;
        }
    }
    model_path()
}

/// Human-readable name of the active model: the friendly name for the bundled
/// default or a curated model ("Qwen3.5 4B"), else the custom GGUF's
/// filename without extension.
pub fn active_model_display(cfg: &crate::config::YapConfig) -> String {
    let path = active_model_path(cfg);
    if path == model_path() {
        return MODEL_DISPLAY.to_string();
    }
    let file = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    if let Some(m) = CURATED_MODELS.iter().find(|m| m.filename == file) {
        return m.display.to_string();
    }
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| MODEL_DISPLAY.to_string())
}

/// All GGUF filenames in `llm/`, sorted, bundled default first. Powers the
/// model picker in Settings.
pub fn list_models() -> Vec<String> {
    let mut models: Vec<String> = std::fs::read_dir(llm_dir())
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.path().is_file())
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|n| n.to_ascii_lowercase().ends_with(".gguf"))
                .collect()
        })
        .unwrap_or_default();
    models.sort_by_key(|n| (n != MODEL_FILENAME, n.to_ascii_lowercase()));
    models
}

/// The base URL of the running sidecar (`http://127.0.0.1:<port>/v1`), or `None`
/// when it isn't up and ready.
pub fn base_url() -> Option<String> {
    if READY.load(Ordering::SeqCst) {
        let port = PORT.load(Ordering::SeqCst);
        if port != 0 {
            return Some(format!("http://127.0.0.1:{}/v1", port));
        }
    }
    None
}

/// Is the sidecar up and serving?
pub fn is_running() -> bool {
    base_url().is_some()
}

/// The `(base_url, api_key, model, provider)` the cleanup client should use:
/// the managed on-device sidecar when the provider is "ondevice" AND it's up,
/// else the user's configured cloud/local endpoint. Falling back keeps cleanup
/// working even if the sidecar failed to start (worst case: raw transcript,
/// never a hang). `provider` is the id the call is attributed to for the
/// per-provider usage meter.
pub fn effective_endpoint(cfg: &crate::config::YapConfig) -> (String, String, String, String) {
    effective_endpoint_for(
        &cfg.pp_provider,
        &cfg.pp_base_url,
        &cfg.pp_api_key,
        &cfg.pp_model,
    )
}

/// Same as [`effective_endpoint`] but for explicit values — used by per-profile
/// LLM overrides, where the provider comes from the matched `CleanupProfile`
/// instead of the global config. "ondevice" routes through the sidecar when
/// it's up, exactly like the global path.
pub fn effective_endpoint_for(
    provider: &str,
    base_url: &str,
    api_key: &str,
    model: &str,
) -> (String, String, String, String) {
    if provider == PROVIDER_ONDEVICE {
        if let Some(url) = self::base_url() {
            // The GGUF it serves, by name: `supports_tools` reads the model's
            // family and size in it (it used to say the 1.5B whatever ran).
            let id = serving_id().unwrap_or_else(|| model_id(&model_path()));
            return (url, String::new(), id, PROVIDER_ONDEVICE.to_string());
        }
        // Down for a moment because Yap is restarting it (a new context or
        // runtime): no endpoint — a dictation keeps its raw text — never the
        // cloud endpoint someone on the on-device model left configured.
        if restart_under_way() {
            return (String::new(), String::new(), model_id(&model_path()), PROVIDER_ONDEVICE.to_string());
        }
    }
    (
        base_url.to_string(),
        api_key.to_string(),
        model.to_string(),
        provider.to_string(),
    )
}

/// Pick a free localhost TCP port for the server.
fn find_free_port() -> Option<u16> {
    std::net::TcpListener::bind("127.0.0.1:0")
        .ok()
        .and_then(|l| l.local_addr().ok())
        .map(|addr| addr.port())
}

/// Has the child process already exited? (missing child counts as exited.)
fn child_exited() -> bool {
    match CHILD.lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
        Some(child) => matches!(child.try_wait(), Ok(Some(_))),
        None => true,
    }
}

/// Start the llamafile server for the installed model and wait until `/health`
/// reports ready. Idempotent: returns the existing URL if already running. Errors
/// (missing files, launch failure, never-ready) leave the sidecar stopped.
pub async fn start() -> Result<String, String> {
    if let Some(url) = base_url() {
        return Ok(url);
    }
    // One start at a time: a second caller waits, then finds it up.
    let _one = STARTING.lock().await;
    if let Some(url) = base_url() {
        return Ok(url);
    }
    start_locked().await
}

/// One way back per session from a runtime that won't start ([`start_locked`]).
static WENT_BACK: AtomicBool = AtomicBool::new(false);
/// One failure counted per session for a runtime that won't say its version
/// ([`upgrade_runtime`]).
static PROBE_COUNTED: AtomicBool = AtomicBool::new(false);

/// How one try at starting the sidecar goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Attempt {
    /// MTP as [`plan_for`] decides (`false`: never).
    pub mtp: bool,
    /// How long to wait for `/health`.
    pub wait: Duration,
}

impl Attempt {
    pub const FIRST: Attempt = Attempt { mtp: true, wait: Duration::from_secs(HEALTH_TIMEOUT_SECS) };
    /// After a failed start: without MTP, and longer to load.
    pub const AGAIN: Attempt = Attempt { mtp: false, wait: Duration::from_secs(HEALTH_TIMEOUT_AGAIN_SECS) };
}

/// Why a start didn't get the sidecar up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// Yap stopped it meanwhile ([`stop`]), or is quitting: not a failure.
    Stopped,
    /// Something isn't there (the runtime, the model, a free port): not the
    /// runtime's fault, and trying again won't help.
    Missing(String),
    /// It wouldn't launch (an antivirus holding a new exe back).
    Launch(String),
    /// It exited while loading.
    Exited,
    /// It wasn't ready in time.
    Timeout,
}

impl Failure {
    /// Worth another try, and — on a new runtime — the runtime's to answer for.
    pub fn counts(&self) -> bool {
        matches!(self, Failure::Launch(_) | Failure::Exited | Failure::Timeout)
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failure::Stopped => write!(f, "the on-device model was stopped while it started"),
            Failure::Missing(why) | Failure::Launch(why) => write!(f, "{why}"),
            Failure::Exited => write!(f, "llamafile exited during startup"),
            Failure::Timeout => write!(f, "llamafile did not become ready in time"),
        }
    }
}

/// A sidecar that came up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Started {
    pub url: String,
    /// It runs with MTP…
    pub mtp: bool,
    /// …and its model didn't all go to the graphics card.
    pub off_card: bool,
    /// Its [`mtp_key`].
    pub key: String,
}

/// A start that failed, and whether it tried MTP.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failed {
    pub why: Failure,
    pub mtp: bool,
    pub key: String,
}

/// [`start`] under its lock.
async fn start_locked() -> Result<String, String> {
    // A stop from now on — by anyone — makes this start "stopped".
    let gen = STOP_GEN.load(Ordering::SeqCst);
    start_with(&llm_dir(), &WENT_BACK, &|| CLOSING.load(Ordering::SeqCst), |a| spawn_and_wait(a, gen)).await
}

/// **A start, and what it does when the sidecar won't come up** (`spawn`:
/// one try, [`spawn_and_wait`] — a stand-in in the unit tests):
/// - stopped by Yap meanwhile (a model switch, the Stop button, a restart,
///   quitting), or something missing: that's the answer, nothing tried
///   again and nothing blamed;
/// - else **once more, without MTP and with longer to load**
///   ([`Attempt::AGAIN`]) — a first run of a new runtime past two minutes
///   (an antivirus), or MTP that crashes it. Up that way after a try with
///   MTP: MTP is off for this model on this runtime here, for good
///   ([`note_mtp_failed`]);
/// - still not up, on a runtime swapped in and not yet started once: the
///   **previous runtime goes back** in place and starts instead (once a
///   session, `went_back`), the new one kept as `llamafile-next.exe` for
///   the next launch — refused for good after [`REFUSE_AFTER`] sessions
///   like this ([`count_failure`]). When the previous one won't start
///   either, the fault isn't the runtime's: the new one goes back in and
///   the failure isn't counted.
///
/// Up: a new runtime is proven (the previous one deleted); one that runs
/// MTP with its model partly on the processor starts again without it,
/// and MTP stays off for that model on that runtime.
pub async fn start_with<F, Fut>(
    dir: &Path,
    went_back: &AtomicBool,
    closing: &(dyn Fn() -> bool + Sync),
    mut spawn: F,
) -> Result<String, String>
where
    F: FnMut(Attempt) -> Fut,
    Fut: std::future::Future<Output = Result<Started, Failed>>,
{
    let unproven = read_marker_at(&dir.join(RUNTIME_MARKER)).is_some_and(|m| m.unproven);
    let first = match spawn(Attempt::FIRST).await {
        Ok(s) => return up(dir, unproven, s, &mut spawn).await,
        Err(f) => f,
    };
    if !first.why.counts() || closing() {
        return Err(first.why.to_string());
    }
    tracing::warn!(mtp = first.mtp, "The on-device model didn't start ({}); trying once more without MTP", first.why);
    match spawn(Attempt::AGAIN).await {
        Ok(s) => {
            if first.mtp {
                note_mtp_failed(dir, &first.key);
            }
            return up(dir, unproven, s, &mut spawn).await;
        }
        Err(again) if !again.why.counts() || closing() => return Err(again.why.to_string()),
        Err(_) => {}
    }
    if !unproven || went_back.swap(true, Ordering::SeqCst) {
        return Err(first.why.to_string());
    }
    let sha = read_marker_at(&dir.join(RUNTIME_MARKER)).map(|m| m.sha256).unwrap_or_default();
    let tries = count_failure(dir, &sha);
    tracing::warn!(tries, "The new llamafile runtime didn't start ({}); going back to the previous one", first.why);
    let failed = match roll_back(dir) {
        Ok(Some(m)) => m,
        Ok(None) => return Err(first.why.to_string()),
        Err(e) => {
            tracing::warn!("Couldn't put the previous llamafile runtime back: {e}");
            return Err(first.why.to_string());
        }
    };
    match spawn(Attempt::AGAIN).await {
        Ok(s) => {
            if refused_in(dir, &failed.sha256) {
                let _ = std::fs::remove_file(dir.join(RUNTIME_NEXT));
                tracing::warn!(version = %failed.version, "The previous llamafile runtime runs again; the new one won't be downloaded again");
            } else {
                tracing::warn!(version = %failed.version, "The previous llamafile runtime runs again; the new one is tried again at the next launch");
            }
            Ok(s.url)
        }
        // Stopped meanwhile: the previous runtime stays, the failure counted.
        Err(again) if !again.why.counts() || closing() => Err(first.why.to_string()),
        Err(again) => {
            // Neither starts: the model (or the PC) is at fault, not the
            // new runtime, which goes back in.
            tracing::warn!("The previous llamafile runtime didn't start either ({}); keeping the new one", again.why);
            match swap_in(dir, &failed.version, &failed.sha256) {
                Ok(()) => uncount_failure(dir, &failed.sha256),
                Err(e) => tracing::warn!("Couldn't put the new llamafile runtime back: {e}"),
            }
            Err(first.why.to_string())
        }
    }
}

/// The sidecar is up ([`start_with`]).
async fn up<F, Fut>(dir: &Path, unproven: bool, s: Started, spawn: &mut F) -> Result<String, String>
where
    F: FnMut(Attempt) -> Fut,
    Fut: std::future::Future<Output = Result<Started, Failed>>,
{
    if unproven {
        prove(dir);
        tracing::info!(version = %runtime_version_in(dir).map(|v| v.to_string()).unwrap_or_default(), "The new llamafile runtime started; the previous one is deleted");
    }
    // MTP is for a model all on the graphics card: one that landed (partly)
    // on the processor starts again without it — and stays without it.
    if s.mtp && s.off_card {
        tracing::warn!("The on-device model didn't all go to the graphics card: starting it again without MTP");
        note_mtp_failed(dir, &s.key);
        return spawn(Attempt::AGAIN).await.map(|s| s.url).map_err(|f| f.why.to_string());
    }
    Ok(s.url)
}

/// "<runtime>/<GGUF file>": what MTP failing is remembered by
/// ([`Marker::mtp_failed`]) — a newer runtime gets to try it again.
pub fn mtp_key(runtime: Version, model: &Path) -> String {
    let file = model.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    format!("{runtime}/{file}")
}

/// MTP failed this session ([`note_mtp_failed`]; also when the marker
/// can't be written).
static MTP_FAILED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// MTP failed with `key` here: never again (this session, and in the
/// marker for the next ones).
fn note_mtp_failed(dir: &Path, key: &str) {
    if key.is_empty() {
        return;
    }
    if let Ok(mut g) = MTP_FAILED.lock() {
        if !g.iter().any(|k| k == key) {
            g.push(key.to_string());
        }
    }
    if dir.join(RUNTIME_MARKER).is_file() {
        let _ = update_marker(dir, |m| {
            if !m.mtp_failed.iter().any(|k| k == key) {
                m.mtp_failed.push(key.to_string());
            }
        });
    }
    tracing::info!(key, "MTP is off for this model on this runtime from now on");
}

/// Did MTP fail with `key` on this PC (this session, or the marker says)?
fn mtp_failed_here(dir: &Path, key: &str) -> bool {
    MTP_FAILED.lock().is_ok_and(|g| g.iter().any(|k| k == key))
        || read_marker_at(&dir.join(RUNTIME_MARKER)).is_some_and(|m| m.mtp_failed.iter().any(|k| k == key))
}

/// NVIDIA's PCI vendor id: the cards MTP was measured on (CUDA).
pub const NVIDIA: u32 = 0x10DE;

/// May the sidecar draft with MTP, before the memory sums ([`mtp_fits`])?
/// llamafile 0.10.6 or newer, the setting on, an **NVIDIA** card (MTP was
/// measured on CUDA only; AMD and Intel cards run llama.cpp's Vulkan
/// backend, where nobody has measured it), and it hasn't failed with this
/// model here. Pure.
pub fn mtp_allowed(runtime: Version, setting: bool, vendor: Option<u32>, failed_here: bool) -> bool {
    runtime.is_current() && setting && vendor == Some(NVIDIA) && !failed_here
}

/// How the sidecar runs `model` at `ctx` on `runtime` ([`Plan`]): **MTP
/// drafting** (`--spec-type draft-mtp`) wherever it can be — llamafile
/// 0.10.6 or newer (on 0.10.3 it kept fewer drafts and cost 2.15 GB), an
/// NVIDIA card, `config.local_mtp` on, not failed with this model here
/// ([`mtp_allowed`]), a model with the prediction layer
/// (`gguf::Shape::has_mtp`: Qwen3.5), the whole model on the card with room
/// for it at this context ([`mtp_fits`]). Never on the processor. Measured
/// on an RTX 5070 Ti (docs/model-evals.md "Local AI, phase 1"): the 4B's
/// generation ~165 → ~255 tokens/s, greedy outputs unchanged, ~+0.85 GB on
/// the card.
pub fn plan_for(cfg: &crate::config::YapConfig, model: &Path, ctx: u32, runtime: Version) -> Plan {
    let gpu = crate::ai::gpu::this_pc();
    let failed = mtp_failed_here(&llm_dir(), &mtp_key(runtime, model));
    let mtp = mtp_allowed(runtime, cfg.local_mtp, gpu.as_ref().map(|g| g.vendor), failed)
        && mtp_fits(model, ctx, gpu.map(|g| g.vram_mib));
    Plan { runtime, mtp }
}

/// Can `model` draft with MTP at `ctx`, and does it fit on a card of
/// `vram_mib` with it ([`card_room`]: the card's memory less what Windows,
/// the apps on screen and Yap's speech model keep there)? `false` without a
/// card of its own.
pub fn mtp_fits(model: &Path, ctx: u32, vram_mib: Option<u64>) -> bool {
    let Some(f) = crate::ai::gguf::model_file(model) else { return false };
    if !f.shape.has_mtp() || !runs_on_card(None, vram_mib) {
        return false;
    }
    let Some(room) = card_room(vram_mib) else { return false };
    let server = crate::ai::gguf::Server { current: true, mtp: true };
    crate::ai::gguf::estimate(&f.shape, f.bytes, ctx as u64, true, server).total_mib <= room
}

/// Launch the runtime on the configured model and wait for `/health`; then
/// read where the model went, and warm it up ([`warm_up`], in the
/// background). One try ([`start_with`] decides what's next): `gen` is
/// [`STOP_GEN`] when the start began — a stop since makes it
/// [`Failure::Stopped`]. A sidecar already running (MTP's restart off the
/// processor) is stopped first.
async fn spawn_and_wait(attempt: Attempt, gen: u64) -> Result<Started, Failed> {
    let exe = runtime_path();
    // Load whichever GGUF the user picked (custom models live in the same dir);
    // falls back to the bundled default if the selection is gone.
    let cfg = crate::config::load();
    let model = active_model_path(&cfg);
    // The context set in the pickers, within what the model was trained for.
    let ctx = context_for(&cfg);
    let mut plan = plan_for(&cfg, &model, ctx, runtime_version().unwrap_or(Version(0, 10, 3)));
    plan.mtp &= attempt.mtp;
    let key = mtp_key(plan.runtime, &model);
    let fail = |why: Failure| Failed { why, mtp: plan.server().mtp, key: key.clone() };
    let stopped = || CLOSING.load(Ordering::SeqCst) || STOP_GEN.load(Ordering::SeqCst) != gen;
    if stopped() {
        return Err(fail(Failure::Stopped));
    }
    if !exe.is_file() {
        return Err(fail(Failure::Missing(format!("llamafile runtime not installed: {}", exe.display()))));
    }
    if !model.is_file() {
        return Err(fail(Failure::Missing(format!("cleanup model not installed: {}", model.display()))));
    }
    stop_inner();

    let port = find_free_port().ok_or_else(|| fail(Failure::Missing("no free localhost port".to_string())))?;

    // Capture the server's output to a log so startup failures are diagnosable
    // (an invalid `--nobrowser` flag silently killing it is exactly how we'd
    // otherwise be blind). Best-effort — falls back to null if the file won't open.
    let log = std::fs::File::create(llm_dir().join("llamafile.log")).ok();
    let to_stdio = |f: Option<std::fs::File>| {
        f.map(std::process::Stdio::from)
            .unwrap_or_else(std::process::Stdio::null)
    };

    let mut cmd = std::process::Command::new(&exe);
    // All layers on the GPU (999; llama.cpp fits what it can, the rest stays
    // on the processor, `placement`), thinking off, the caches capped.
    cmd.args(server_args(&model, port, 999, ctx, plan))
        .stdin(std::process::Stdio::null())
        .stdout(to_stdio(log.as_ref().and_then(|f| f.try_clone().ok())))
        .stderr(to_stdio(log.and_then(|f| f.try_clone().ok())));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW — no console popup
    }

    let child = cmd
        .spawn()
        .map_err(|e| fail(Failure::Launch(format!("failed to launch llamafile: {}", e))))?;
    // The model never keeps running after Yap: a crash or End task ends it too.
    crate::procs::end_with_yap(&child);
    *CHILD.lock().unwrap_or_else(|p| p.into_inner()) = Some(child);
    PORT.store(port, Ordering::SeqCst);
    READY.store(false, Ordering::SeqCst);
    // A stop between the look above and the child being kept missed it.
    if stopped() {
        stop_inner();
        return Err(fail(Failure::Stopped));
    }
    tracing::info!(port, ctx, runtime = %plan.runtime, mtp = plan.server().mtp, wait_s = attempt.wait.as_secs(), model = %model.display(), "Starting on-device cleanup sidecar");

    // Poll /health until the model has loaded (or the process dies / we time out).
    let health = format!("http://127.0.0.1:{}/health", port);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .map_err(|e| fail(Failure::Missing(e.to_string())))?;

    let t0 = Instant::now();
    while t0.elapsed() < attempt.wait {
        // Stopped by Yap meanwhile (`stop` killed it): not a failure.
        if stopped() {
            return Err(fail(Failure::Stopped));
        }
        if child_exited() {
            stop_inner();
            return Err(fail(Failure::Exited));
        }
        if let Ok(resp) = client.get(&health).send_for(crate::privacy::net::Purpose::OnDeviceCheck).await {
            if resp.status().is_success() {
                // Where its layers went: a big model on the processor is slow,
                // and Settings says so. The log may lag a moment behind.
                let mut placement = None;
                let mut facts = LogFacts::default();
                for _ in 0..10 {
                    let log = std::fs::read_to_string(llm_dir().join("llamafile.log")).unwrap_or_default();
                    placement = placement_after_load(&log);
                    facts = log_facts(&log);
                    if placement.is_some() {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(300)).await;
                }
                if stopped() {
                    return Err(fail(Failure::Stopped));
                }
                if let Ok(mut s) = SERVING.lock() {
                    let hybrid = crate::ai::gguf::model_file(&model).is_some_and(|f| f.shape.is_hybrid());
                    *s = Some(Serving { model_id: model_id(&model), placement, ctx, plan, hybrid, facts });
                }
                READY.store(true, Ordering::SeqCst);
                tracing::info!(
                    port,
                    model = %model_id(&model),
                    on = placement.map(|p| p.kind()).unwrap_or("?"),
                    gpu_layers = placement.map(|p| p.gpu_layers).unwrap_or(0),
                    layers = placement.map(|p| p.layers).unwrap_or(0),
                    ctx = facts.n_ctx.unwrap_or(ctx),
                    gpu_mib = facts.gpu_mib.unwrap_or(0),
                    host_mib = facts.host_mib.unwrap_or(0),
                    "On-device cleanup sidecar ready"
                );
                let url = format!("http://127.0.0.1:{}/v1", port);
                // The first request after loading is slow: one small cleanup
                // now, cancelled by the first real request.
                let (u, id) = (url.clone(), model_id(&model));
                tauri::async_runtime::spawn(async move {
                    let w = warm_up(&u, &id).await;
                    tracing::info!(ms = w.ms, cancelled = w.cancelled, error = w.error.as_deref().unwrap_or(""), "On-device model warmed up");
                });
                let off_card = placement.is_some_and(|p| p.kind() != "gpu");
                return Ok(Started { url, mtp: plan.server().mtp, off_card, key });
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    if stopped() {
        return Err(fail(Failure::Stopped));
    }
    stop_inner();
    Err(fail(Failure::Timeout))
}

/// A short dictation for the warm-up (made up; never pasted anywhere).
const WARM_UP_TEXT: &str = "so um I think we should uh move the team meeting to Thursday afternoon";

/// What a warm-up did.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct WarmUp {
    pub ms: u64,
    /// A real request came first, so it gave way.
    pub cancelled: bool,
    pub error: Option<String>,
}

/// **Warm the model up** once it's loaded: the first request after loading
/// is slow — CUDA graphs captured, kernels loaded, the cleanup's ~2k-token
/// prompt read cold (on llamafile 0.10.6, 2.8 s once where a dictation's
/// cleanup takes 0.25 s; docs/model-evals.md). One cleanup-sized request
/// — the cleanup's own prompt as Settings has it, a short made-up
/// dictation, a few tokens of answer — does that, and leaves the prompt in
/// a slot for the first real dictation. It never stands in a real request's
/// way: the first one to start ([`request_starting`]) cancels it (closing
/// the connection stops it in llama.cpp). Not through `llm.rs`: it isn't a
/// request anyone waits for, and it isn't counted as one.
pub async fn warm_up(url: &str, model: &str) -> WarmUp {
    let cfg = crate::config::load();
    let spelling = crate::ai::about_you::cleanup_spelling(&cfg);
    let split = crate::ai::llm::splits_dictation(url, model);
    let messages = crate::ai::llm::cleanup_messages(WARM_UP_TEXT, &cfg.pp_prompt, spelling.as_deref(), &cfg.dictionary, split);
    let body = serde_json::json!({
        "model": model,
        "stream": false,
        "messages": messages,
        "temperature": 0.2,
        "max_tokens": 8,
    });
    let t0 = Instant::now();
    let started = STARTED.load(Ordering::SeqCst);
    let client = match reqwest::Client::builder().timeout(Duration::from_secs(60)).build() {
        Ok(c) => c,
        Err(e) => return WarmUp { error: Some(e.to_string()), ..Default::default() },
    };
    let send = client.post(format!("{}/chat/completions", url.trim_end_matches('/'))).json(&body).send_for(crate::privacy::net::Purpose::OnDeviceWarmUp);
    let gave_way = async {
        while STARTED.load(Ordering::SeqCst) == started {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    };
    let mut out = WarmUp::default();
    tokio::select! {
        r = send => match r {
            Ok(resp) if resp.status().is_success() => {
                let _ = resp.bytes().await;
            }
            Ok(resp) => out.error = Some(format!("HTTP {}", resp.status().as_u16())),
            Err(e) => out.error = Some(e.to_string()),
        },
        _ = gave_way => out.cancelled = true,
    }
    out.ms = t0.elapsed().as_millis() as u64;
    out
}

/// The sidecar's URL: as it is when it's up, after the start already under
/// way (never a second process), else after starting it. For callers that
/// need the model now (the AI Chat on the on-device model).
pub async fn ensure_started() -> Result<String, String> {
    // Every start holds `STARTING` (its tries, a way back included): waiting
    // for it is waiting for the start under way.
    start().await
}

/// Kill any orphaned llamafile processes left over from a previous session.
/// Crashes and task-kill end Yap WITHOUT running the `RunEvent::Exit` handler
/// (so did the Tauri updater before updates.rs ran `shutdown_cleanup` from its
/// pre-install hook), so `stop()` never fires and the sidecar survives.
/// Called at startup before we spawn a fresh one. Only sidecars whose parent
/// Yap is gone count: a dev build must not kill the installed app's live one.
#[cfg(windows)]
pub fn kill_orphans() {
    for pid in crate::procs::orphaned(RUNTIME_FILENAME) {
        if crate::procs::kill(pid) {
            tracing::info!("Killed orphaned cleanup sidecar (pid {})", pid);
        }
    }
}
#[cfg(not(windows))]
pub fn kill_orphans() {}

/// Stop the sidecar (kill + reap the child). Safe to call when not running.
/// Yap's own stop: a start under way ends as stopped ([`STOP_GEN`]), never
/// tried again or blamed on the runtime.
pub fn stop() {
    STOP_GEN.fetch_add(1, Ordering::SeqCst);
    stop_inner();
}

/// Yap is quitting: stop the sidecar, and start nothing (nor go back to a
/// previous runtime) from now on. From every way out (`shutdown_cleanup`).
pub fn shutdown() {
    CLOSING.store(true, Ordering::SeqCst);
    stop();
}

/// Kill + reap the child, as part of a start ([`spawn_and_wait`]).
fn stop_inner() {
    READY.store(false, Ordering::SeqCst);
    PORT.store(0, Ordering::SeqCst);
    if let Ok(mut s) = SERVING.lock() {
        *s = None;
    }
    // Poisoned or not, the child is ended: a skipped kill would leave the
    // model running while the next start launches another.
    let mut guard = CHILD.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(mut child) = guard.take() {
        let _ = child.kill();
        let _ = child.wait();
        tracing::info!("On-device cleanup sidecar stopped");
    }
}

/// Whether any config path can route cleanup to the on-device sidecar: the
/// global provider, or a per-profile LLM override.
fn ondevice_selected(cfg: &crate::config::YapConfig) -> bool {
    cfg.pp_provider == PROVIDER_ONDEVICE
        || cfg
            .cleanup_profiles
            .iter()
            .any(|p| p.provider == PROVIDER_ONDEVICE)
        // A Language-Models scope (Commands, etc.) pointed at the local model.
        || cfg
            .llm_scopes
            .values()
            .any(|s| s.enabled && s.provider == PROVIDER_ONDEVICE)
}

/// If on-device cleanup is selected anywhere (globally or by a profile
/// override) and the files are installed, start the sidecar (best-effort —
/// logs and moves on if it can't). Called at startup and after config saves.
pub async fn autostart_if_configured(cfg: &crate::config::YapConfig) {
    if cfg.post_process_enabled && ondevice_selected(cfg) && is_installed(cfg) && !is_running() {
        if let Err(e) = start().await {
            tracing::warn!("On-device cleanup sidecar failed to start: {}", e);
        }
    }
}

// ---- the context: restarting on a change, never mid-dictation --------------

/// Does a sidecar serving `serving` (None = not running) need a restart
/// to give the model `wanted`? A stopped one starts with it anyway. Pure.
pub fn needs_restart(serving: Option<u32>, wanted: u32) -> bool {
    serving.is_some_and(|c| c != wanted)
}

/// A pending restart's next move, each time it looks at the pipeline. Pure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartStep {
    /// A dictation is recording or being transcribed and cleaned up (on
    /// this very model, maybe): look again shortly.
    Wait,
    /// It just finished: give the paste a moment, then look again.
    Settle,
    Go,
}

pub fn restart_step(busy: bool, waited: bool, settled: bool) -> RestartStep {
    if busy {
        RestartStep::Wait
    } else if waited && !settled {
        RestartStep::Settle
    } else {
        RestartStep::Go
    }
}

/// How often a pending restart looks at the pipeline, and how long it lets
/// a finished dictation settle (its paste, a quick follow-up).
const RESTART_TICK: Duration = Duration::from_millis(250);
const RESTART_SETTLE: Duration = Duration::from_millis(1_500);

/// Wait until `busy()` says no dictation is going on ([`restart_step`]).
/// `false` = `current()` said a newer request took over. Never times out:
/// a dictation ends.
pub async fn wait_until_idle(
    busy: impl Fn() -> bool,
    current: impl Fn() -> bool,
    tick: Duration,
    settle: Duration,
) -> bool {
    let (mut waited, mut settled) = (false, false);
    loop {
        if !current() {
            return false;
        }
        match restart_step(busy(), waited, settled) {
            RestartStep::Wait => {
                waited = true;
                settled = false;
                tokio::time::sleep(tick).await;
            }
            RestartStep::Settle => {
                settled = true;
                tokio::time::sleep(settle).await;
            }
            RestartStep::Go => return true,
        }
    }
}

/// Bumped by every restart request; the latest one clears "pending".
static RESTART_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static RESTART_PENDING: AtomicBool = AtomicBool::new(false);
/// One restart at a time (a newer one waits, then compares again).
static RESTARTING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// A context change waiting for a dictation to end, or under way.
pub fn restart_pending() -> bool {
    RESTART_PENDING.load(Ordering::SeqCst)
}

/// A dictation recording, or being transcribed and cleaned up (the
/// cleanup may be on this very model).
fn pipeline_busy(app: &AppHandle) -> bool {
    use tauri::Manager;
    #[cfg(debug_assertions)]
    if crate::e2e::active() && E2E_BUSY.load(Ordering::SeqCst) {
        return true;
    }
    if crate::dictation::pipeline::dictation_state().1 {
        return true;
    }
    app.try_state::<crate::AppState>()
        .and_then(|s| s.pipeline.lock().ok().map(|g| g.as_ref().is_some_and(|p| p.is_busy())))
        .unwrap_or(false)
}

/// The sidecar is in use: a dictation (above), or a request to it under way
/// — a Chat answer, a meeting digest, the Ask bar ([`request_starting`]).
fn sidecar_busy(app: &AppHandle) -> bool {
    pipeline_busy(app) || requests_in_flight() > 0
}

/// Restart the sidecar on the context the config now asks for, once no
/// dictation is going on and no request to it is under way (never under
/// one: its cleanup or a Chat answer would lose the model mid-answer,
/// [`sidecar_busy`]). The model switch's path: stop, start. Emits
/// `yap-local-llm-changed` when done (the pickers re-read), and
/// `yap-chat-model-changed {scope: null}` so the chips do.
pub fn restart_when_idle(app: AppHandle) {
    let gen = RESTART_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    RESTART_PENDING.store(true, Ordering::SeqCst);
    let _ = app.emit("yap-local-llm-changed", serde_json::json!({ "pending": true }));
    tauri::async_runtime::spawn(async move {
        let latest = || RESTART_GEN.load(Ordering::SeqCst) == gen;
        if sidecar_busy(&app) {
            tracing::info!("On-device model: the new context waits for the dictation or answer to finish");
        }
        if !wait_until_idle(|| sidecar_busy(&app), latest, RESTART_TICK, RESTART_SETTLE).await {
            return; // a newer change took over
        }
        let result = {
            let _one = RESTARTING.lock().await;
            let wanted = context_for(&crate::config::load());
            restart_on(wanted).await
        };
        if let Err(e) = &result {
            tracing::warn!("On-device model didn't restart with its new context: {e}");
        }
        if latest() {
            RESTART_PENDING.store(false, Ordering::SeqCst);
        }
        let _ = app.emit(
            "yap-local-llm-changed",
            serde_json::json!({ "pending": restart_pending(), "error": result.err() }),
        );
        let _ = app.emit("yap-chat-model-changed", serde_json::json!({ "scope": null }));
    });
}

/// Stop and start the sidecar when it serves another context than `wanted`.
async fn restart_on(wanted: u32) -> Result<(), String> {
    #[cfg(debug_assertions)]
    if crate::e2e::active() {
        // The stand-in "restarts"; a test run never starts a real model.
        if let Ok(mut g) = E2E_SERVING.lock() {
            if let Some(s) = g.as_mut().filter(|s| s.ctx != wanted) {
                s.ctx = wanted;
                // A new log: what it takes isn't known until it's measured.
                s.used_mib = None;
                E2E_RESTARTS.fetch_add(1, Ordering::SeqCst);
            }
        }
        return Ok(());
    }
    if !needs_restart(serving_context(), wanted) {
        return Ok(());
    }
    tracing::info!(ctx = wanted, "Restarting the on-device model with its new context");
    let _restarting = PlannedRestart::begin();
    stop();
    start().await.map(|_| ())
}

// ---- the runtime: a newer llamafile, in the background ----------------------

/// When the background upgrade first looks (after launch: the sidecar
/// starts and the first dictations come first), and how long it waits
/// after a "not now" (metered, not in use, a failed download), doubling up
/// to the longest.
const UPGRADE_FIRST_LOOK: Duration = Duration::from_secs(90);
const UPGRADE_RETRY: Duration = Duration::from_secs(30 * 60);
const UPGRADE_RETRY_MAX: Duration = Duration::from_secs(6 * 60 * 60);

/// What one look at the runtime came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpgradeStep {
    /// The runtime on disk is the one pinned (or none is installed).
    Done,
    /// Not now, and why ("a metered connection", a download error…).
    Later(String),
}

/// The upgrade under way, for Settings and the log: "downloading" |
/// "waiting" (for the sidecar to be idle) | "" (none).
static UPGRADE_PHASE: Mutex<&'static str> = Mutex::new("");

fn set_upgrade_phase(p: &'static str) {
    if let Ok(mut g) = UPGRADE_PHASE.lock() {
        *g = p;
    }
}

/// The background upgrade's phase now ("" when there's none).
pub fn upgrade_phase() -> &'static str {
    UPGRADE_PHASE.lock().map(|g| *g).unwrap_or("")
}

/// **Upgrade an older runtime in the background** (installs from before 22
/// Oct 2026 have 0.10.3, which `ensure_runtime` never replaced: it only
/// downloads a missing one). Looks 90 s after launch, then — after a "not
/// now" — every 30 min, doubling to 6 h:
/// - only while the on-device model is in use (no 368 MB for nothing), and
///   not on a metered connection (`updates::metered`);
/// - downloads the pinned runtime to `llamafile-next.exe` (SHA-256 verified;
///   a failed download is retried later, the reason logged) while the old
///   one keeps serving, and checks it runs here (`--version`);
/// - swaps it in once the sidecar is idle — no dictation, no request to it
///   under way, no meeting recording or being summarised — stopping it
///   first and starting it again on the new runtime ([`start_locked`]: a
///   new runtime that won't start gives way to the previous one, which is
///   kept until the new one has started once).
///
/// The new runtime runs as `llamafile.exe` like the old one, so the orphan
/// sweep (`kill_orphans`) and the job object (`procs::end_with_yap`) cover
/// it. A test run only upgrades from its own stand-in (`YAP_LLM_SOURCES`),
/// never runs it, and looks after `YAP_E2E_RUNTIME_UPGRADE_MS` (debug).
pub fn spawn_runtime_upgrade(app: AppHandle) {
    let Some(first) = first_look() else { return };
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(first).await;
        let mut wait = UPGRADE_RETRY;
        loop {
            match upgrade_runtime(&app).await {
                UpgradeStep::Done => {
                    set_upgrade_phase("");
                    return;
                }
                UpgradeStep::Later(why) => {
                    set_upgrade_phase("");
                    tracing::info!(retry_min = wait.as_secs() / 60, "llamafile upgrade: not now ({why})");
                    tokio::time::sleep(wait).await;
                    wait = (wait * 2).min(UPGRADE_RETRY_MAX);
                }
            }
        }
    });
}

/// When the upgrade first looks; `None` = never (a test run that didn't
/// ask for it with `YAP_E2E_RUNTIME_UPGRADE_MS`, debug builds).
fn first_look() -> Option<Duration> {
    if crate::e2e::active() {
        #[cfg(debug_assertions)]
        if let Some(ms) = std::env::var("YAP_E2E_RUNTIME_UPGRADE_MS").ok().and_then(|v| v.trim().parse().ok()) {
            return Some(Duration::from_millis(ms));
        }
        return None;
    }
    Some(UPGRADE_FIRST_LOOK)
}

/// A meeting recording or being summarised (its digests and action plan
/// go to the on-device model one call after another).
fn meeting_busy() -> bool {
    crate::meetings::meeting::is_recording() || crate::meetings::meeting_end::summary_running()
}

/// The runtime's size (the pinned download's): what a download and a swap
/// need on the disk.
const RUNTIME_BYTES: u64 = 368_094_430;

/// Free space an upgrade wants before it downloads: the new runtime beside
/// the old one (and the old one kept until the new one has started), plus
/// room to spare.
const UPGRADE_ROOM: u64 = 2 * RUNTIME_BYTES + 256 * 1_048_576;

/// **One upgrade at a time across Yaps**: a dev build and the installed app
/// share `llm/`. A lock file holding the owner's process id, taken with
/// create-new; one whose owner isn't a running Yap any more is stale and
/// taken over. Released (deleted) when dropped.
pub struct UpgradeLock(PathBuf);

/// The upgrade lock's file in `llm/`.
const UPGRADE_LOCK: &str = "llamafile-upgrade.lock";

impl UpgradeLock {
    /// `None`: another running Yap holds it (or the folder can't be written).
    pub fn take(dir: &Path) -> Option<UpgradeLock> {
        Self::take_unless(dir, |pid| pid != std::process::id() && running_yap(pid))
    }

    /// [`take`](Self::take), `held(pid)` saying whether the pid in an
    /// existing lock still holds it (the unit tests' stand-in).
    fn take_unless(dir: &Path, held: impl Fn(u32) -> bool) -> Option<UpgradeLock> {
        use std::io::Write;
        let path = dir.join(UPGRADE_LOCK);
        for _ in 0..2 {
            match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut f) => {
                    let _ = write!(f, "{}", std::process::id());
                    return Some(UpgradeLock(path));
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    let owner = std::fs::read_to_string(&path).ok().and_then(|s| s.trim().parse::<u32>().ok());
                    if owner.is_some_and(&held) {
                        return None;
                    }
                    // Stale (its Yap is gone), or ours from before.
                    let _ = std::fs::remove_file(&path);
                }
                Err(_) => return None,
            }
        }
        None
    }
}

impl Drop for UpgradeLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[cfg(windows)]
fn running_yap(pid: u32) -> bool {
    crate::procs::is_running_yap(pid)
}
#[cfg(not(windows))]
fn running_yap(_pid: u32) -> bool {
    false
}

/// Another running Yap's sidecar (a dev build beside the installed app),
/// running the runtime in this `llm/`: a swap waits for it to stop.
#[cfg(windows)]
fn other_yaps_sidecar() -> bool {
    crate::procs::sidecars_of_other_yaps(&llm_dir(), RUNTIME_FILENAME) > 0
}
#[cfg(not(windows))]
fn other_yaps_sidecar() -> bool {
    false
}

/// Free bytes on the disk `dir` is on (`None`: unknown).
#[cfg(windows)]
pub fn free_bytes(dir: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let mut free = 0u64;
    // SAFETY: a NUL-terminated path, an out pointer that lives through the call.
    unsafe { GetDiskFreeSpaceExW(windows::core::PCWSTR(wide.as_ptr()), Some(&mut free), None, None) }.ok()?;
    Some(free)
}
#[cfg(not(windows))]
pub fn free_bytes(_dir: &Path) -> Option<u64> {
    None
}

/// Enough room on the disk for `need` bytes? Unknown: yes. Pure.
pub fn room_for(free: Option<u64>, need: u64) -> bool {
    free.is_none_or(|f| f >= need)
}

/// The `llamafile-next.exe` this session already hashed and found whole:
/// its size and time, so a later look doesn't read 368 MB again.
static NEXT_CHECKED: Mutex<Option<(u64, std::time::SystemTime)>> = Mutex::new(None);

fn file_stamp(path: &Path) -> Option<(u64, std::time::SystemTime)> {
    let m = std::fs::metadata(path).ok()?;
    Some((m.len(), m.modified().ok()?))
}

/// Is `next` the whole pinned runtime (`sha`)? Hashed once a session.
async fn next_is_whole(next: &Path, sha: &str) -> bool {
    let Some(stamp) = file_stamp(next) else { return false };
    if NEXT_CHECKED.lock().is_ok_and(|g| *g == Some(stamp)) {
        return true;
    }
    let p = next.to_path_buf();
    let ok = tokio::task::spawn_blocking(move || compute_sha256(&p)).await.ok().and_then(Result::ok).is_some_and(|h| h.eq_ignore_ascii_case(sha));
    if ok {
        if let Ok(mut g) = NEXT_CHECKED.lock() {
            *g = Some(stamp);
        }
    }
    ok
}

/// **At launch, before the sidecar starts**: a runtime cut off mid-swap or
/// taken by an antivirus comes back ([`repair_runtime`]), a previous one
/// kept past a proven start goes, and a download that was cut off leaves
/// no partial file. Not while another Yap holds the upgrade lock.
pub async fn tidy_runtime() {
    let dir = llm_dir();
    if !dir.is_dir() {
        return;
    }
    let Some(_lock) = UpgradeLock::take(&dir) else { return };
    let _ = std::fs::remove_file(dir.join(RUNTIME_NEXT).with_extension("partial"));
    let (_, sha) = source("runtime", RUNTIME_URL, RUNTIME_SHA256);
    let next = dir.join(RUNTIME_NEXT);
    let next_sha = if !dir.join(RUNTIME_FILENAME).is_file() && next.is_file() {
        tokio::task::spawn_blocking(move || compute_sha256(&next)).await.ok().and_then(Result::ok)
    } else {
        None
    };
    if let Some(did) = repair_runtime(&dir, next_sha.as_deref(), (RUNTIME_VERSION, &sha)) {
        tracing::info!("llamafile runtime: {did}");
    }
}

/// Wait until `idle()` (then a settle, and idle again), then take the
/// restart lock — never waiting for it: a context restart under way isn't
/// held up — and the start lock, looking again once it's had to wait for
/// that one: a request that began meanwhile (a Chat turn, a dictation) sends
/// it back to waiting. Returns both locks, held.
async fn idle_with_locks(
    restarting: &'static tokio::sync::Mutex<()>,
    starting: &'static tokio::sync::Mutex<()>,
    idle: impl Fn() -> bool,
    tick: Duration,
    settle: Duration,
) -> (tokio::sync::MutexGuard<'static, ()>, tokio::sync::MutexGuard<'static, ()>) {
    loop {
        wait_until_idle(|| !idle(), || true, tick, settle).await;
        tokio::time::sleep(settle).await;
        if let Ok(restart) = restarting.try_lock() {
            if idle() {
                let one = starting.lock().await;
                if idle() {
                    return (restart, one);
                }
            }
        }
        tokio::time::sleep(tick).await;
    }
}

/// One look: download the pinned runtime if it's due, then swap it in.
pub async fn upgrade_runtime(app: &AppHandle) -> UpgradeStep {
    let dir = llm_dir();
    let (url, sha) = source("runtime", RUNTIME_URL, RUNTIME_SHA256);
    // This session went back from it: the next launch tries it again.
    if WENT_BACK.load(Ordering::SeqCst) {
        return UpgradeStep::Done;
    }
    let marker = read_marker_at(&dir.join(RUNTIME_MARKER));
    if !upgrade_due(dir.join(RUNTIME_FILENAME).is_file(), marker.as_ref(), &sha) {
        return UpgradeStep::Done;
    }
    let test_run = crate::e2e::active();
    if test_run && !allowed_in_test_run(&url) {
        return UpgradeStep::Done; // a test run without a stand-in runtime
    }
    let cfg = crate::config::load();
    if !in_use(&cfg) && !is_running() {
        return UpgradeStep::Later("the on-device model isn't in use".into());
    }
    if !test_run && crate::updates::metered() {
        return UpgradeStep::Later("a metered connection".into());
    }
    let Some(_lock) = UpgradeLock::take(&dir) else {
        return UpgradeStep::Later("another Yap is upgrading it".into());
    };
    // Under the lock: the other Yap may have done it meanwhile.
    let marker = read_marker_at(&dir.join(RUNTIME_MARKER));
    if !upgrade_due(dir.join(RUNTIME_FILENAME).is_file(), marker.as_ref(), &sha) {
        return UpgradeStep::Done;
    }
    let next = dir.join(RUNTIME_NEXT);
    // Downloaded before (Yap closed before the swap, or a way back kept it):
    // kept if it's whole.
    if !(next.is_file() && next_is_whole(&next, &sha).await) {
        let free = free_bytes(&dir);
        if !room_for(free, UPGRADE_ROOM) {
            return UpgradeStep::Later(format!(
                "only {} MB free on the disk; it needs {} MB",
                free.unwrap_or(0) / 1_048_576,
                UPGRADE_ROOM / 1_048_576
            ));
        }
        set_upgrade_phase("downloading");
        tracing::info!(from = %marker.as_ref().map(|m| m.version.clone()).unwrap_or_else(|| UNMARKED_RUNTIME_VERSION.into()), to = RUNTIME_VERSION, "llamafile upgrade: downloading the new runtime in the background");
        if let Err(e) = download_for(&url, &next, &sha, "runtime", "", None).await {
            return UpgradeStep::Later(format!("the download failed: {e}"));
        }
        if let (Some(stamp), Ok(mut g)) = (file_stamp(&next), NEXT_CHECKED.lock()) {
            *g = Some(stamp);
        }
    }
    // It runs on this PC (an antivirus can hold a new exe back). One that
    // won't, session after session, is refused and deleted.
    if !test_run {
        let why = match probe_version(&next).await {
            Some(v) if v.to_string() == RUNTIME_VERSION => None,
            Some(v) => Some(format!("the download says it's {v}, not {RUNTIME_VERSION}")),
            None => Some("the new runtime didn't run (--version)".to_string()),
        };
        if let Some(why) = why {
            if !PROBE_COUNTED.swap(true, Ordering::SeqCst) && count_failure(&dir, &sha) >= REFUSE_AFTER {
                let _ = std::fs::remove_file(&next);
                tracing::warn!("llamafile upgrade: {why}, again; this runtime won't be downloaded again");
                return UpgradeStep::Done;
            }
            return UpgradeStep::Later(why);
        }
    }
    // Swapped in once nothing uses the sidecar (idle, then idle again after
    // a settle), never under a context restart — which isn't held up
    // meanwhile: the locks are only taken once it's idle — and never under
    // a request that began while the start lock was awaited.
    set_upgrade_phase("waiting");
    let idle = || !(sidecar_busy(app) || meeting_busy());
    let (_restart, _one) = idle_with_locks(&RESTARTING, &STARTING, idle, RESTART_TICK, RESTART_SETTLE).await;
    // Another Yap's sidecar runs the runtime here: not under it.
    if !test_run && other_yaps_sidecar() {
        return UpgradeStep::Later("another Yap is using the on-device model".into());
    }
    let result = {
        let _restarting = PlannedRestart::begin();
        let was_running = is_running();
        if was_running {
            stop();
        }
        match swap_in(&dir, RUNTIME_VERSION, &sha) {
            Ok(()) => {
                tracing::info!(version = RUNTIME_VERSION, restart = was_running, "llamafile upgrade: the new runtime is in place");
                if was_running {
                    start_locked().await.map(|_| ())
                } else {
                    Ok(())
                }
            }
            Err(e) => {
                tracing::warn!("llamafile upgrade: couldn't swap the runtime in: {e}");
                if was_running {
                    let _ = start_locked().await;
                }
                Err(e.to_string())
            }
        }
    };
    let _ = app.emit("yap-local-llm-changed", serde_json::json!({ "pending": restart_pending(), "error": result.as_ref().err() }));
    let _ = app.emit("yap-chat-model-changed", serde_json::json!({ "scope": null }));
    match result {
        Ok(()) => UpgradeStep::Done,
        Err(e) => UpgradeStep::Later(e),
    }
}

/// What `exe --version` says it is ("llamafile v0.10.6"), within a minute
/// (a fresh download's first run waits for the antivirus); `None` when it
/// didn't run or said nothing readable. Tied to Yap like the sidecar.
pub async fn probe_version(exe: &Path) -> Option<Version> {
    let exe = exe.to_path_buf();
    let run = tokio::task::spawn_blocking(move || {
        let mut cmd = std::process::Command::new(&exe);
        cmd.arg("--version")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let child = cmd.spawn().ok()?;
        crate::procs::end_with_yap(&child);
        let out = child.wait_with_output().ok()?;
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        Version::parse(&text)
    });
    tokio::time::timeout(Duration::from_secs(60), run).await.ok()?.ok()?
}

/// One context the pickers offer, with what it takes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextOption {
    pub tokens: u32,
    /// ≈ MiB the model takes at it ([`crate::ai::gguf::estimate`]); `None`
    /// when its file can't be read.
    pub estimate_mib: Option<u64>,
    /// More than the graphics card has room for: layers would go to the
    /// processor (slow).
    pub over: bool,
}

/// The on-device model's context, as the pickers show it
/// (`local_llm_status().context`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextView {
    /// What the active model gets (the setting, within its maximum).
    pub tokens: u32,
    /// `local_context` (0 = the default).
    pub configured: u32,
    pub default: u32,
    /// What the active model was trained for.
    pub max: Option<u64>,
    /// The running sidecar's context (`None`: not running).
    pub serving: Option<u32>,
    /// A change waiting for a dictation to end, or the restart under way.
    pub pending: bool,
    /// The estimates are of the graphics card's memory (else the PC's).
    pub on_card: bool,
    /// The graphics card's room for the model: its own memory less what
    /// Windows, the apps on screen and Yap's speech model keep there.
    pub room_mib: Option<u64>,
    /// What the running model takes, from its log.
    pub used_mib: Option<u64>,
    pub options: Vec<ContextOption>,
}

/// What Windows, the apps on screen and Yap's speech model keep on the
/// card (gpu.rs' sums: the speech model ~1–1.6 GB, Windows ~0.5–1 GB). Not
/// llama.cpp's "MiB free": on Windows (WDDM) CUDA reports the card as if
/// nothing else used it (15,037 of 16,275 MiB free beside a running 4B).
const CARD_RESERVE_MIB: u64 = 2_048;

/// The card's room for the on-device model: its own memory less
/// [`CARD_RESERVE_MIB`]. Pure.
pub fn card_room(vram_mib: Option<u64>) -> Option<u64> {
    vram_mib.map(|v| v.saturating_sub(CARD_RESERVE_MIB))
}

/// Does the model go to the graphics card? Its log says when it's running;
/// else any card with memory of its own (llamafile offloads to it). Pure.
pub fn runs_on_card(placement: Option<Placement>, vram_mib: Option<u64>) -> bool {
    match placement {
        Some(p) => p.kind() != "cpu",
        None => vram_mib.is_some_and(|v| v >= 1_024),
    }
}

/// The context view for the active model ([`ContextView`]).
pub fn context_view(cfg: &crate::config::YapConfig) -> ContextView {
    let path = active_model_path(cfg);
    let file = crate::ai::gguf::model_file(&path);
    let max = trained_context(&path);
    let tokens = effective_context(cfg.local_context, max);
    let running = shown_running();
    let placement = running.then(placement).flatten();
    let facts = running.then(serving_facts).flatten();
    let gpu = crate::ai::gpu::this_pc();
    let vram = gpu.as_ref().map(|g| g.vram_mib);
    let on_card = runs_on_card(placement, vram);
    let room = on_card.then(|| card_room(vram)).flatten();
    // The runtime on disk (its buffers differ), or the one an install gets.
    let runtime = runtime_version().unwrap_or_else(|| Version::parse(RUNTIME_VERSION).expect("a version"));
    let options = context_choices(max, tokens)
        .into_iter()
        .map(|c| {
            let server = plan_for(cfg, &path, c, runtime).server();
            let estimate_mib = file
                .as_ref()
                .map(|f| crate::ai::gguf::estimate(&f.shape, f.bytes, c as u64, on_card, server).total_mib);
            ContextOption {
                tokens: c,
                estimate_mib,
                over: matches!((estimate_mib, room), (Some(e), Some(r)) if e > r),
            }
        })
        .collect();
    ContextView {
        tokens,
        configured: cfg.local_context,
        default: DEFAULT_CONTEXT,
        max,
        serving: running.then(serving_context).flatten(),
        pending: restart_pending(),
        on_card,
        room_mib: room,
        used_mib: facts.and_then(|f| if on_card { f.gpu_mib } else { f.host_mib }),
        options,
    }
}

/// A context as `local_context` keeps it: 0 for the default, else within
/// [`MIN_CONTEXT`]..=[`MAX_CONTEXT`]. Pure.
pub fn context_setting(tokens: u32) -> Result<u32, String> {
    if tokens == 0 || tokens == DEFAULT_CONTEXT {
        return Ok(0);
    }
    if !(MIN_CONTEXT..=MAX_CONTEXT).contains(&tokens) {
        return Err(format!(
            "A context of {tokens} tokens isn't offered: pick one from {}K to {}K",
            MIN_CONTEXT / 1024,
            MAX_CONTEXT / 1024
        ));
    }
    Ok(tokens)
}

/// The on-device model's context (the Chat picker's On this PC pane,
/// Settings → Language Models → Local): saved the way Settings saves
/// (`save_config`), `yap-local-context-changed {localContext}` so Settings'
/// copy adopts it, and a running sidecar restarted on it once no dictation
/// is going on ([`restart_when_idle`]).
#[tauri::command]
pub fn local_llm_set_context(app: AppHandle, tokens: u32) -> Result<ContextView, String> {
    let setting = context_setting(tokens)?;
    // In one step: never over a change saved meanwhile.
    let mut before = 0;
    let cfg = crate::commands::change_config(&app, |cfg| {
        before = context_for(cfg);
        cfg.local_context = setting;
        Ok(())
    })?;
    let _ = app.emit("yap-local-context-changed", serde_json::json!({ "localContext": setting }));
    let after = context_for(&cfg);
    tracing::info!(from = before, to = after, "On-device model context set");
    if shown_running() && (needs_restart(serving_context(), after) || restart_pending()) {
        restart_when_idle(app);
    }
    Ok(context_view(&cfg))
}

/// Progress event payload for the on-device install (`local-llm-download-progress`).
#[derive(serde::Serialize, Clone)]
struct DownloadProgress {
    /// "runtime" (the llamafile engine) or "model" (the GGUF).
    stage: &'static str,
    /// The curated id the download is for ("" for the bundled default's
    /// install): the Chat picker shows the bar in that model's row.
    model: String,
    percent: u8,
    downloaded_mb: f64,
    total_mb: f64,
}

/// Where a download comes from: `url` + `sha256` as pinned, or in a debug
/// build a stand-in from the JSON file `YAP_LLM_SOURCES` (`{"runtime" |
/// "<curated id>": {"url", "sha256"}}`): how the e2e suite downloads small
/// fakes from a local server (`e2e/support/local-llm.js`).
fn source(key: &str, url: &str, sha256: &str) -> (String, String) {
    #[cfg(debug_assertions)]
    if let Some(path) = std::env::var_os("YAP_LLM_SOURCES").filter(|v| !v.is_empty()) {
        let stand_ins = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());
        if let Some(s) = stand_ins.as_ref().and_then(|v| v.get(key)) {
            if let (Some(u), Some(h)) = (s["url"].as_str(), s["sha256"].as_str()) {
                return (u.to_string(), h.to_ascii_lowercase());
            }
        }
    }
    let _ = key;
    (url.to_string(), sha256.to_string())
}

/// A test run downloads only from its own servers (127.0.0.1), never a
/// real model from the internet. Pure.
fn allowed_in_test_run(url: &str) -> bool {
    url::Url::parse(url).is_ok_and(|u| u.host_str() == Some("127.0.0.1"))
}

/// A curated, SHA-pinned local cleanup model offered in onboarding/Settings.
/// (Any other GGUF dropped into `llm/` still works via `pp_local_model` — this
/// list is just the managed, one-click download set.)
pub struct CuratedLlm {
    pub id: &'static str,
    pub display: &'static str,
    /// One-line pitch shown in the picker.
    pub blurb: &'static str,
    pub filename: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub size_mb: u32,
    /// The bundled default: the model an empty `pp_local_model` means (the
    /// 1.5B), recommended where the PC has no graphics card for a bigger one.
    pub default: bool,
    /// Recommended on a PC whose graphics card can hold it
    /// (`gpu::can_run_big_model`): too slow on a processor.
    pub needs_gpu: bool,
    /// Model family for the browser's provider tabs + brand icon
    /// ("qwen"|"llama"|"gemma"|"mistral"|"phi").
    pub family: &'static str,
    /// The context it was trained for (its GGUF's `context_length`; the
    /// file's own header wins once it's downloaded, `trained_context`).
    pub max_context: u32,
}

/// The on-device model that runs Chat's tools well where the graphics card
/// can hold it: Qwen3.5-4B, thinking off (`server_args`). Measured with the
/// model evaluations (docs/model-evals.md): 92–94% of the scenarios over two
/// passes on an RTX 5070 Ti at ~1.3 s a turn, where the 1.5B gets 79% and
/// can't act on notes; on a processor, 25 s a turn.
pub const GPU_MODEL_ID: &str = "qwen3.5-4b";
/// The default's id in [`CURATED_MODELS`].
pub const DEFAULT_MODEL_ID: &str = "qwen2.5-1.5b";

/// The model recommended for this PC: [`GPU_MODEL_ID`] when its graphics
/// card can run it, else the 1.5B default. Pure.
pub fn recommended_id(gpu_capable: bool) -> &'static str {
    if gpu_capable {
        GPU_MODEL_ID
    } else {
        DEFAULT_MODEL_ID
    }
}

/// The bigger model to offer someone on the default (once, in Settings,
/// never downloaded by itself): on a PC that can run it, while the on-device
/// model is in use, on the 1.5B, and the offer wasn't turned down. Pure.
pub fn upgrade_offer(gpu_capable: bool, in_use: bool, active_file: &str, dismissed: bool) -> Option<&'static CuratedLlm> {
    if !gpu_capable || !in_use || dismissed || active_file != MODEL_FILENAME {
        return None;
    }
    curated_by_id(GPU_MODEL_ID)
}

/// Is the on-device model what Chat or the cleanup uses (globally — Chat
/// inherits the cleanup's endpoint —, a profile, or a Language-Models scope)?
pub fn in_use(cfg: &crate::config::YapConfig) -> bool {
    ondevice_selected(cfg)
}

/// Grouped by family, smallest→largest within a family. SHA-256s are the
/// HuggingFace `lfs.oid` values (verified July 2026 from each repo's LFS
/// pointer); all are real, ungated Q4_K_M GGUFs.
pub const CURATED_MODELS: &[CuratedLlm] = &[
    // ── Qwen ─────────────────────────────────────────────────────────────
    CuratedLlm {
        id: "qwen2.5-0.5b",
        display: "Qwen2.5 0.5B Instruct",
        blurb: "Tiny — instant even on a laptop",
        filename: "Qwen2.5-0.5B-Instruct-Q4_K_M.gguf",
        url: "https://huggingface.co/bartowski/Qwen2.5-0.5B-Instruct-GGUF/resolve/main/Qwen2.5-0.5B-Instruct-Q4_K_M.gguf",
        sha256: "6eb923e7d26e9cea28811e1a8e852009b21242fb157b26149d3b188f3a8c8653",
        size_mb: 398,
        default: false,
        needs_gpu: false,
        family: "qwen",
        max_context: 32_768,
    },
    CuratedLlm {
        id: "qwen2.5-1.5b",
        display: MODEL_DISPLAY,
        blurb: "Fast and light: quick even without a graphics card",
        filename: MODEL_FILENAME,
        url: MODEL_URL,
        sha256: MODEL_SHA256,
        size_mb: 1043,
        default: true,
        needs_gpu: false,
        family: "qwen",
        max_context: 32_768,
    },
    CuratedLlm {
        id: "qwen2.5-3b",
        display: "Qwen2.5 3B Instruct",
        blurb: "The default's bigger sibling — noticeably smarter",
        filename: "qwen2.5-3b-instruct-q4_k_m.gguf",
        url: "https://huggingface.co/Qwen/Qwen2.5-3B-Instruct-GGUF/resolve/main/qwen2.5-3b-instruct-q4_k_m.gguf",
        sha256: "626b4a6678b86442240e33df819e00132d3ba7dddfe1cdc4fbb18e0a9615c62d",
        size_mb: 2105,
        default: false,
        needs_gpu: false,
        family: "qwen",
        max_context: 32_768,
    },
    CuratedLlm {
        id: GPU_MODEL_ID,
        display: "Qwen3.5 4B",
        blurb: "Uses Chat's tools well. Wants a graphics card",
        filename: "Qwen_Qwen3.5-4B-Q4_K_M.gguf",
        // bartowski's Q4_K_M of Qwen/Qwen3.5-4B (Apache-2.0, ungated); the
        // SHA-256 is its LFS oid, and the file the evaluations ran.
        url: "https://huggingface.co/bartowski/Qwen_Qwen3.5-4B-GGUF/resolve/main/Qwen_Qwen3.5-4B-Q4_K_M.gguf",
        sha256: "13c16f426047e2de38cd075bdade4a7bcbc8c774384876f677740cda65f8a983",
        size_mb: 2873,
        default: false,
        needs_gpu: true,
        family: "qwen",
        max_context: 262_144,
    },
    CuratedLlm {
        id: "qwen2.5-7b",
        display: "Qwen2.5 7B Instruct",
        blurb: "Big Qwen — best local cleanup quality",
        filename: "Qwen2.5-7B-Instruct-Q4_K_M.gguf",
        url: "https://huggingface.co/bartowski/Qwen2.5-7B-Instruct-GGUF/resolve/main/Qwen2.5-7B-Instruct-Q4_K_M.gguf",
        sha256: "65b8fcd92af6b4fefa935c625d1ac27ea29dcb6ee14589c55a8f115ceaaa1423",
        size_mb: 4683,
        default: false,
        needs_gpu: false,
        family: "qwen",
        max_context: 32_768,
    },
    // ── Meta Llama ───────────────────────────────────────────────────────
    CuratedLlm {
        id: "llama-3.2-1b",
        display: "Llama 3.2 1B Instruct",
        blurb: "Smallest + fastest — great on modest PCs",
        filename: "Llama-3.2-1B-Instruct-Q4_K_M.gguf",
        url: "https://huggingface.co/bartowski/Llama-3.2-1B-Instruct-GGUF/resolve/main/Llama-3.2-1B-Instruct-Q4_K_M.gguf",
        sha256: "6f85a640a97cf2bf5b8e764087b1e83da0fdb51d7c9fab7d0fece9385611df83",
        size_mb: 808,
        default: false,
        needs_gpu: false,
        family: "llama",
        max_context: 131_072,
    },
    CuratedLlm {
        id: "llama-3.2-3b",
        display: "Llama 3.2 3B Instruct",
        blurb: "Stronger rewrites, still quick on a GPU",
        filename: "Llama-3.2-3B-Instruct-Q4_K_M.gguf",
        url: "https://huggingface.co/bartowski/Llama-3.2-3B-Instruct-GGUF/resolve/main/Llama-3.2-3B-Instruct-Q4_K_M.gguf",
        sha256: "6c1a2b41161032677be168d354123594c0e6e67d2b9227c84f296ad037c728ff",
        size_mb: 2019,
        default: false,
        needs_gpu: false,
        family: "llama",
        max_context: 131_072,
    },
    CuratedLlm {
        id: "llama-3.1-8b",
        display: "Llama 3.1 8B Instruct",
        blurb: "Meta's 8B — strong and well-rounded",
        filename: "Meta-Llama-3.1-8B-Instruct-Q4_K_M.gguf",
        url: "https://huggingface.co/bartowski/Meta-Llama-3.1-8B-Instruct-GGUF/resolve/main/Meta-Llama-3.1-8B-Instruct-Q4_K_M.gguf",
        sha256: "7b064f5842bf9532c91456deda288a1b672397a54fa729aa665952863033557c",
        size_mb: 4921,
        default: false,
        needs_gpu: false,
        family: "llama",
        max_context: 131_072,
    },
    // ── Gemma (Google) ───────────────────────────────────────────────────
    CuratedLlm {
        id: "gemma-2-2b",
        display: "Gemma 2 2B IT",
        blurb: "Google's small model — polished, natural tone",
        filename: "gemma-2-2b-it-Q4_K_M.gguf",
        url: "https://huggingface.co/bartowski/gemma-2-2b-it-GGUF/resolve/main/gemma-2-2b-it-Q4_K_M.gguf",
        sha256: "e0aee85060f168f0f2d8473d7ea41ce2f3230c1bc1374847505ea599288a7787",
        size_mb: 1709,
        default: false,
        needs_gpu: false,
        family: "gemma",
        max_context: 8_192,
    },
    CuratedLlm {
        id: "gemma-2-9b",
        display: "Gemma 2 9B IT",
        blurb: "Google's 9B — polished, high quality",
        filename: "gemma-2-9b-it-Q4_K_M.gguf",
        url: "https://huggingface.co/bartowski/gemma-2-9b-it-GGUF/resolve/main/gemma-2-9b-it-Q4_K_M.gguf",
        sha256: "13b2a7b4115bbd0900162edcebe476da1ba1fc24e718e8b40d32f6e300f56dfe",
        size_mb: 5761,
        default: false,
        needs_gpu: false,
        family: "gemma",
        max_context: 8_192,
    },
    // ── Mistral ──────────────────────────────────────────────────────────
    CuratedLlm {
        id: "mistral-7b-v0.3",
        display: "Mistral 7B Instruct v0.3",
        blurb: "Mistral's classic 7B — a solid all-rounder",
        filename: "Mistral-7B-Instruct-v0.3-Q4_K_M.gguf",
        url: "https://huggingface.co/bartowski/Mistral-7B-Instruct-v0.3-GGUF/resolve/main/Mistral-7B-Instruct-v0.3-Q4_K_M.gguf",
        sha256: "1270d22c0fbb3d092fb725d4d96c457b7b687a5f5a715abe1e818da303e562b6",
        size_mb: 4373,
        default: false,
        needs_gpu: false,
        family: "mistral",
        max_context: 32_768,
    },
    // ── Phi (Microsoft) ──────────────────────────────────────────────────
    CuratedLlm {
        id: "phi-3.5-mini",
        display: "Phi-3.5 Mini Instruct",
        blurb: "Microsoft's 3.8B — strong quality for its size",
        filename: "Phi-3.5-mini-instruct-Q4_K_M.gguf",
        url: "https://huggingface.co/bartowski/Phi-3.5-mini-instruct-GGUF/resolve/main/Phi-3.5-mini-instruct-Q4_K_M.gguf",
        sha256: "e4165e3a71af97f1b4820da61079826d8752a2088e313af0c7d346796c38eff5",
        size_mb: 2393,
        default: false,
        needs_gpu: false,
        family: "phi",
        max_context: 131_072,
    },
];

pub fn curated_by_id(id: &str) -> Option<&'static CuratedLlm> {
    CURATED_MODELS.iter().find(|m| m.id == id)
}

/// Ensure the llamafile runtime is on disk (SHA-verified download if missing),
/// with its marker ([`RUNTIME_MARKER`]). An older one already there stays
/// until the background upgrade replaces it ([`spawn_runtime_upgrade`]).
/// `model`: the curated id the install is for (its progress names it).
async fn ensure_runtime(model: &str, app: Option<&AppHandle>) -> Result<(), String> {
    tokio::fs::create_dir_all(llm_dir())
        .await
        .map_err(|e| format!("failed to create llm dir: {}", e))?;

    if !runtime_path().is_file() {
        let dir = llm_dir();
        let _lock = UpgradeLock::take(&dir)
            .ok_or_else(|| "Another Yap is setting up the on-device model's engine. Try again in a minute.".to_string())?;
        if runtime_path().is_file() {
            return Ok(()); // the other Yap just did
        }
        let (url, sha) = source("runtime", RUNTIME_URL, RUNTIME_SHA256);
        // Beside it first (or kept from an upgrade), then the marker, then
        // into place: never a runtime on disk its marker doesn't describe.
        let next = dir.join(RUNTIME_NEXT);
        if !(next.is_file() && next_is_whole(&next, &sha).await) {
            download_for(&url, &next, &sha, "runtime", model, app).await?;
        }
        let learnt = read_marker_at(&dir.join(RUNTIME_MARKER)).unwrap_or_default();
        let marker = Marker { version: RUNTIME_VERSION.to_string(), sha256: sha, unproven: false, ..Default::default() }.learnt_from(&learnt);
        write_marker_at(&dir.join(RUNTIME_MARKER), &marker).map_err(|e| format!("Couldn't write the engine's marker: {e}"))?;
        std::fs::rename(&next, runtime_path()).map_err(|e| format!("Couldn't put the engine in place: {e}"))?;
        // On Unix the runtime needs the executable bit (Windows infers from .exe).
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = std::fs::metadata(runtime_path()) {
                let mut perms = meta.permissions();
                perms.set_mode(0o755);
                let _ = std::fs::set_permissions(runtime_path(), perms);
            }
        }
    }
    Ok(())
}

/// Download the runtime + the default model (whichever are missing), each
/// SHA-verified, into `<data_dir>/llm/`. Idempotent — already-present files are
/// skipped. Emits `local-llm-download-progress` so the UI can show a bar per stage.
pub async fn install(app: Option<&AppHandle>) -> Result<(), String> {
    ensure_runtime("", app).await?;
    if !model_path().is_file() {
        let (url, sha) = source(DEFAULT_MODEL_ID, MODEL_URL, MODEL_SHA256);
        download_for(&url, &model_path(), &sha, "model", "", app).await?;
    }
    Ok(())
}

/// Download the runtime + a specific **curated** model by id. Returns the
/// curated entry so the caller can point `pp_local_model` at its filename.
pub async fn install_curated(
    id: &str,
    app: Option<&AppHandle>,
) -> Result<&'static CuratedLlm, String> {
    let m = curated_by_id(id).ok_or_else(|| format!("unknown cleanup model id: {}", id))?;
    ensure_runtime(m.id, app).await?;
    let dest = llm_dir().join(m.filename);
    if !dest.is_file() {
        let (url, sha) = source(m.id, m.url, m.sha256);
        download_for(&url, &dest, &sha, "model", m.id, app).await?;
    }
    Ok(m)
}

/// Stream `url` → `dest.partial`, emit progress, verify SHA-256, then rename into
/// place. A verification failure deletes the partial and errors (safe: a corrupt
/// download can never be used). (The model evaluations' downloads,
/// `eval/local.rs`: debug builds.)
#[cfg(debug_assertions)]
pub(crate) async fn download_verified(
    url: &str,
    dest: &Path,
    expected_sha: &str,
    stage: &'static str,
    app: Option<&AppHandle>,
) -> Result<(), String> {
    download_for(url, dest, expected_sha, stage, "", app).await
}

/// How long a download may go without a byte (or a server without an
/// answer) before it's given up on: a stalled connection never leaves an
/// install or the background upgrade at "downloading".
const DOWNLOAD_STALL: Duration = Duration::from_secs(60);

/// Room left on the disk beside a download, past its own size.
const DOWNLOAD_SPARE: u64 = 64 * 1_048_576;

/// A verified download, its progress naming the curated model `model`.
/// Whatever goes wrong — the server, a stalled connection, a full disk, a
/// bad hash — the partial file goes too.
async fn download_for(
    url: &str,
    dest: &Path,
    expected_sha: &str,
    stage: &'static str,
    model: &str,
    app: Option<&AppHandle>,
) -> Result<(), String> {
    if crate::e2e::active() && !allowed_in_test_run(url) {
        return Err("Test runs never download models from the internet".to_string());
    }
    let partial = dest.with_extension("partial");
    tracing::info!(url, dest = %dest.display(), stage, "Downloading on-device cleanup asset");
    let result = fetch_to(url, dest, &partial, expected_sha, stage, model, app).await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&partial).await;
    }
    result
}

/// [`download_for`]'s work: `url` → `partial`, verified, renamed to `dest`.
async fn fetch_to(
    url: &str,
    dest: &Path,
    partial: &Path,
    expected_sha: &str,
    stage: &'static str,
    model: &str,
    app: Option<&AppHandle>,
) -> Result<(), String> {
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("request failed: {}", e))?;
    let resp = tokio::time::timeout(DOWNLOAD_STALL, client.get(url).send_for(crate::privacy::net::Purpose::Download(crate::privacy::net::Download::OnDeviceAi)))
        .await
        .map_err(|_| "the server didn't answer".to_string())?
        .map_err(|e| format!("request failed: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {} from {}", resp.status(), url));
    }
    let total = resp.content_length();
    if let Some(total) = total {
        let free = dest.parent().and_then(free_bytes);
        if !room_for(free, total + DOWNLOAD_SPARE) {
            return Err(format!(
                "not enough disk space: it needs {} MB, {} MB are free",
                (total + DOWNLOAD_SPARE) / 1_048_576,
                free.unwrap_or(0) / 1_048_576
            ));
        }
    }

    let mut file = tokio::fs::File::create(partial)
        .await
        .map_err(|e| format!("create temp file: {}", e))?;
    let mut downloaded: u64 = 0;
    let mut last_pct: u8 = 0;
    let mut stream = resp.bytes_stream();
    loop {
        let chunk = match tokio::time::timeout(DOWNLOAD_STALL, stream.next()).await {
            Ok(Some(chunk)) => chunk.map_err(|e| format!("download stream error: {}", e))?,
            Ok(None) => break,
            Err(_) => return Err("the download stalled".to_string()),
        };
        file.write_all(&chunk).await.map_err(|e| format!("write: {}", e))?;
        downloaded += chunk.len() as u64;
        if let (Some(total), Some(app)) = (total, app) {
            let pct = ((downloaded as f64 / total as f64) * 100.0) as u8;
            if pct >= last_pct + 2 {
                last_pct = pct;
                let _ = app.emit(
                    "local-llm-download-progress",
                    DownloadProgress {
                        stage,
                        model: model.to_string(),
                        percent: pct,
                        downloaded_mb: downloaded as f64 / 1_048_576.0,
                        total_mb: total as f64 / 1_048_576.0,
                    },
                );
            }
        }
    }
    file.flush().await.map_err(|e| format!("flush: {}", e))?;
    drop(file);

    // Verify SHA-256 on a blocking thread (files are hundreds of MB to ~1 GB).
    let verify_path = partial.to_path_buf();
    let expected = expected_sha.to_string();
    let ok = tokio::task::spawn_blocking(move || match compute_sha256(&verify_path) {
        Ok(actual) => actual.eq_ignore_ascii_case(&expected),
        Err(_) => false,
    })
    .await
    .map_err(|e| format!("sha task panicked: {}", e))?;
    if !ok {
        return Err(format!("{} verification failed (corrupt download) — retry", stage));
    }

    tokio::fs::rename(partial, dest)
        .await
        .map_err(|e| format!("rename: {}", e))?;
    Ok(())
}

/// SHA-256 of a file, streamed so large models don't load into memory.
fn compute_sha256(path: &Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_says_where_the_layers_went() {
        let gpu = "0.00.461.604 I load_tensors: offloading 27 repeating layers to GPU\n0.00.461.605 I load_tensors: offloaded 29/29 layers to GPU\n";
        let p = placement_in_log(gpu).unwrap();
        assert_eq!((p.gpu_layers, p.layers, p.kind()), (29, 29, "gpu"));
        let partial = placement_in_log("I load_tensors: offloaded 20/33 layers to GPU").unwrap();
        assert_eq!(partial.kind(), "partial");
        let cpu = placement_in_log("I load_tensors: offloaded 0/33 layers to GPU").unwrap();
        assert_eq!(cpu.kind(), "cpu");
        assert_eq!(placement_in_log("I srv server_main: model loaded"), None);
        // No line once loaded: no layer went to a GPU (llama.cpp only writes
        // it when some did); still loading: unknown.
        assert_eq!(placement_after_load("I srv server_main: model loaded").map(|p| p.kind()), Some("cpu"));
        assert_eq!(placement_after_load("I srv load_model: loading model"), None);
        assert_eq!(placement_after_load(gpu).map(|p| p.kind()), Some("gpu"));
    }

    #[test]
    fn a_big_model_on_the_processor_is_slow_and_the_small_one_never() {
        let cpu = Some(Placement { gpu_layers: 0, layers: 33 });
        let gpu = Some(Placement { gpu_layers: 33, layers: 33 });
        let some = Some(Placement { gpu_layers: 12, layers: 33 });
        assert!(slow_on_this_pc("Qwen_Qwen3.5-4B-Q4_K_M.gguf", 2873, cpu));
        assert!(slow_on_this_pc("Qwen_Qwen3.5-4B-Q4_K_M.gguf", 2873, some));
        assert!(!slow_on_this_pc("Qwen_Qwen3.5-4B-Q4_K_M.gguf", 2873, gpu));
        assert!(!slow_on_this_pc(MODEL_FILENAME, 1065, cpu));
        // A GGUF of the person's own: by its size.
        assert!(slow_on_this_pc("my-8b.gguf", 4_700, cpu));
        assert!(!slow_on_this_pc("tiny.gguf", 400, cpu));
        assert!(!slow_on_this_pc("Qwen_Qwen3.5-4B-Q4_K_M.gguf", 2873, None));
    }

    #[test]
    fn the_four_b_is_recommended_where_the_card_can_run_it() {
        assert_eq!(recommended_id(true), GPU_MODEL_ID);
        assert_eq!(recommended_id(false), DEFAULT_MODEL_ID);
        let big = curated_by_id(GPU_MODEL_ID).unwrap();
        assert!(big.needs_gpu && !big.default);
        assert_eq!(big.sha256.len(), 64);
        assert!(big.url.ends_with(big.filename));
        // Exactly one default, and it's the bundled file.
        let defaults: Vec<_> = CURATED_MODELS.iter().filter(|m| m.default).collect();
        assert_eq!(defaults.len(), 1);
        assert_eq!((defaults[0].id, defaults[0].filename), (DEFAULT_MODEL_ID, MODEL_FILENAME));
        // Ids are unique.
        let mut ids: Vec<_> = CURATED_MODELS.iter().map(|m| m.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), CURATED_MODELS.len());
    }

    #[test]
    fn the_upgrade_is_offered_once_to_the_default_on_a_capable_pc() {
        assert_eq!(upgrade_offer(true, true, MODEL_FILENAME, false).map(|m| m.id), Some(GPU_MODEL_ID));
        assert!(upgrade_offer(false, true, MODEL_FILENAME, false).is_none(), "no card for it");
        assert!(upgrade_offer(true, false, MODEL_FILENAME, false).is_none(), "on-device not in use");
        assert!(upgrade_offer(true, true, MODEL_FILENAME, true).is_none(), "turned down");
        assert!(upgrade_offer(true, true, "Qwen_Qwen3.5-4B-Q4_K_M.gguf", false).is_none(), "already on it");
        assert!(upgrade_offer(true, true, "my-own.gguf", false).is_none(), "a model of their own");
    }

    #[test]
    fn a_test_run_downloads_only_from_its_own_servers() {
        assert!(allowed_in_test_run("http://127.0.0.1:5123/llm/qwen3.5-4b.gguf"));
        assert!(!allowed_in_test_run(MODEL_URL));
        assert!(!allowed_in_test_run(RUNTIME_URL));
        assert!(!allowed_in_test_run("http://localhost:5123/llm/x.gguf"));
        assert!(!allowed_in_test_run("not a web address"));
        // Without stand-ins: the pinned source.
        assert_eq!(
            source("no-such-id", MODEL_URL, MODEL_SHA256),
            (MODEL_URL.to_string(), MODEL_SHA256.to_string())
        );
    }

    const NEW: Version = Version(0, 10, 6);
    const OLD: Version = Version(0, 10, 3);

    #[test]
    fn the_server_runs_without_thinking_and_with_its_caches_capped() {
        let args = server_args(Path::new("m.gguf"), 5000, 999, DEFAULT_CONTEXT, Plan { runtime: NEW, mtp: false });
        let after = |flag: &str| args.iter().position(|a| a == flag).map(|i| args[i + 1].as_str());
        assert_eq!(after("--reasoning"), Some("off"));
        assert_eq!(after("--cache-ram"), Some("0"));
        assert_eq!(after("--ctx-checkpoints"), Some("4"));
        assert_eq!(after("-ngl"), Some("999"));
        assert_eq!(after("-c"), Some("8192"));
        assert_eq!(after("-lv"), Some("4"));
        assert_eq!(after("--port"), Some("5000"));
        assert!(args.iter().any(|a| a == "--server"));
        assert!(!args.iter().any(|a| a == "--spec-type"), "no MTP unless the plan says");
        // MTP drafting: on the runtime it's measured on, never on 0.10.3
        // (whatever the plan says), whose other arguments are the same.
        let mtp = server_args(Path::new("m.gguf"), 5000, 999, DEFAULT_CONTEXT, Plan { runtime: NEW, mtp: true });
        assert_eq!(&mtp[mtp.len() - 2..], ["--spec-type", "draft-mtp"]);
        let old = server_args(Path::new("m.gguf"), 5000, 999, DEFAULT_CONTEXT, Plan { runtime: OLD, mtp: true });
        assert_eq!(old, args);
    }

    /// A folder of its own under the temp dir, emptied first.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("yap-runtime-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn read(dir: &Path, file: &str) -> String {
        std::fs::read_to_string(dir.join(file)).unwrap_or_default()
    }

    #[test]
    fn an_old_runtime_is_upgraded_once_and_a_refused_one_never() {
        let current = Marker { version: "0.10.6".into(), sha256: RUNTIME_SHA256.into(), ..Default::default() };
        // No runtime: nothing to upgrade (an install downloads the new one).
        assert!(!upgrade_due(false, None, RUNTIME_SHA256));
        // No marker: the 0.10.3 every install had before markers.
        assert!(upgrade_due(true, None, RUNTIME_SHA256));
        assert!(!upgrade_due(true, Some(&current), RUNTIME_SHA256));
        assert!(!upgrade_due(true, Some(&current), &RUNTIME_SHA256.to_uppercase()));
        // A newer pin later: due again.
        assert!(upgrade_due(true, Some(&current), "ab12"));
        // One that wouldn't start here: never again.
        let refused = Marker { version: "0.10.3".into(), refused: vec![RUNTIME_SHA256.into()], ..Default::default() };
        assert!(!upgrade_due(true, Some(&refused), RUNTIME_SHA256));
        // The version on disk: the marker's, else 0.10.3; none without a runtime.
        let dir = scratch("version");
        assert_eq!(runtime_version_in(&dir), None);
        std::fs::write(dir.join(RUNTIME_FILENAME), "old").unwrap();
        assert_eq!(runtime_version_in(&dir), Some(OLD));
        write_marker(&dir, &current).unwrap();
        assert_eq!(runtime_version_in(&dir), Some(NEW));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_new_runtime_swaps_in_and_the_old_one_stays_until_it_has_started() {
        let dir = scratch("swap");
        std::fs::write(dir.join(RUNTIME_FILENAME), "old").unwrap();
        // Nothing downloaded: nothing changes.
        assert!(swap_in(&dir, "0.10.6", "aa").is_err());
        assert_eq!(read(&dir, RUNTIME_FILENAME), "old");
        std::fs::write(dir.join(RUNTIME_NEXT), "new").unwrap();
        swap_in(&dir, "0.10.6", "aa").unwrap();
        assert_eq!((read(&dir, RUNTIME_FILENAME), read(&dir, RUNTIME_PREVIOUS)), ("new".into(), "old".into()));
        assert!(!dir.join(RUNTIME_NEXT).exists());
        let m = read_marker_at(&dir.join(RUNTIME_MARKER)).unwrap();
        assert_eq!((m.version.as_str(), m.sha256.as_str(), m.unproven), ("0.10.6", "aa", true));
        // It started: the old one goes.
        prove(&dir);
        assert!(!dir.join(RUNTIME_PREVIOUS).exists());
        assert!(!read_marker_at(&dir.join(RUNTIME_MARKER)).unwrap().unproven);
        // Nothing to go back to now.
        assert_eq!(roll_back(&dir).unwrap(), None);
        assert_eq!(read(&dir, RUNTIME_FILENAME), "new");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_new_runtime_that_wont_start_gives_way_to_the_old_one() {
        let dir = scratch("rollback");
        std::fs::write(dir.join(RUNTIME_FILENAME), "old").unwrap();
        std::fs::write(dir.join(RUNTIME_NEXT), "new").unwrap();
        swap_in(&dir, "0.10.6", "aa").unwrap();
        // It didn't start (counted once): the old one (unmarked: 0.10.3) is
        // back, the new one kept aside for the next launch — not refused yet.
        assert_eq!(count_failure(&dir, "aa"), 1);
        let failed = roll_back(&dir).unwrap().unwrap();
        assert_eq!((failed.version.as_str(), failed.sha256.as_str()), ("0.10.6", "aa"));
        assert_eq!((read(&dir, RUNTIME_FILENAME), read(&dir, RUNTIME_NEXT)), ("old".into(), "new".into()));
        assert!(!dir.join(RUNTIME_PREVIOUS).exists());
        let m = read_marker_at(&dir.join(RUNTIME_MARKER)).unwrap();
        assert_eq!((m.version.as_str(), m.unproven), (UNMARKED_RUNTIME_VERSION, false));
        assert_eq!((m.failures.get("aa"), m.refused.is_empty()), (Some(&1), true), "what was learnt goes with it");
        assert_eq!(runtime_version_in(&dir), Some(OLD));
        assert!(upgrade_due(true, Some(&m), "aa"), "tried again at the next launch");
        // The old one didn't start either: not the runtime's fault — the new
        // one goes back in, the failure not counted.
        swap_in(&dir, &failed.version, &failed.sha256).unwrap();
        uncount_failure(&dir, "aa");
        assert_eq!((read(&dir, RUNTIME_FILENAME), read(&dir, RUNTIME_PREVIOUS)), ("new".into(), "old".into()));
        let m = read_marker_at(&dir.join(RUNTIME_MARKER)).unwrap();
        assert!(m.unproven && m.refused.is_empty() && m.failures.is_empty());
        // Three sessions it failed in: refused, never downloaded again.
        for n in 1..=REFUSE_AFTER {
            assert_eq!(count_failure(&dir, "aa"), n);
        }
        let m = read_marker_at(&dir.join(RUNTIME_MARKER)).unwrap();
        assert_eq!(m.refused, vec!["aa".to_string()]);
        roll_back(&dir).unwrap().unwrap();
        let m = read_marker_at(&dir.join(RUNTIME_MARKER)).unwrap();
        assert!(!upgrade_due(true, Some(&m), "aa"), "never downloaded again");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The review's finding (2026-10-22): a roll-back with the new runtime
    /// gone (an antivirus took it) deleted the waiting `llamafile-next.exe`
    /// and then failed to rename, leaving no runtime at all.
    #[test]
    fn a_roll_back_with_the_new_runtime_gone_still_puts_the_old_one_back() {
        let dir = scratch("rollback-gone");
        std::fs::write(dir.join(RUNTIME_FILENAME), "old").unwrap();
        std::fs::write(dir.join(RUNTIME_NEXT), "new").unwrap();
        swap_in(&dir, "0.10.6", "aa").unwrap();
        std::fs::remove_file(dir.join(RUNTIME_FILENAME)).unwrap();
        std::fs::write(dir.join(RUNTIME_NEXT), "waiting").unwrap();
        roll_back(&dir).unwrap().unwrap();
        assert_eq!((read(&dir, RUNTIME_FILENAME), read(&dir, RUNTIME_NEXT)), ("old".into(), "waiting".into()));
        assert!(!dir.join(RUNTIME_PREVIOUS).exists());
        assert_eq!(runtime_version_in(&dir), Some(OLD));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_runtime_comes_back_at_launch() {
        let pinned = ("0.10.6", "aa");
        // Cut off mid-swap or mid-roll-back (or an antivirus took the new
        // one): the previous one comes back.
        let dir = scratch("repair-prev");
        std::fs::write(dir.join(RUNTIME_FILENAME), "old").unwrap();
        std::fs::write(dir.join(RUNTIME_NEXT), "new").unwrap();
        swap_in(&dir, "0.10.6", "aa").unwrap();
        std::fs::remove_file(dir.join(RUNTIME_FILENAME)).unwrap();
        assert_eq!(repair_runtime(&dir, None, pinned), Some("restored the previous runtime"));
        assert_eq!(read(&dir, RUNTIME_FILENAME), "old");
        assert_eq!(runtime_version_in(&dir), Some(OLD));
        assert!(!read_marker_at(&dir.join(RUNTIME_MARKER)).unwrap().unproven);
        assert_eq!(repair_runtime(&dir, None, pinned), None, "nothing more to do");
        let _ = std::fs::remove_dir_all(&dir);
        // No previous one, a whole download waiting: it goes in, its marker first.
        let dir = scratch("repair-next");
        std::fs::write(dir.join(RUNTIME_NEXT), "new").unwrap();
        assert_eq!(repair_runtime(&dir, Some("bb"), pinned), None, "not the pinned runtime");
        assert!(!dir.join(RUNTIME_FILENAME).exists());
        assert_eq!(repair_runtime(&dir, Some("AA"), pinned), Some("restored the downloaded runtime"));
        assert_eq!(read(&dir, RUNTIME_FILENAME), "new");
        assert_eq!(runtime_version_in(&dir), Some(NEW));
        let _ = std::fs::remove_dir_all(&dir);
        // A previous one kept past a proven start goes; not before.
        let dir = scratch("repair-tidy");
        std::fs::write(dir.join(RUNTIME_FILENAME), "old").unwrap();
        std::fs::write(dir.join(RUNTIME_NEXT), "new").unwrap();
        swap_in(&dir, "0.10.6", "aa").unwrap();
        assert_eq!(repair_runtime(&dir, None, pinned), None, "unproven: the way back stays");
        assert!(dir.join(RUNTIME_PREVIOUS).exists());
        update_marker(&dir, |m| m.unproven = false).unwrap();
        assert_eq!(repair_runtime(&dir, None, pinned), Some("deleted the previous runtime"));
        assert!(!dir.join(RUNTIME_PREVIOUS).exists() && !dir.join(MARKER_PREVIOUS).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_old_marker_reads_and_an_empty_history_isnt_written() {
        let old: Marker = serde_json::from_str(r#"{"version":"0.10.6","sha256":"aa","unproven":false,"refused":[]}"#).unwrap();
        assert!(old.failures.is_empty() && old.mtp_failed.is_empty());
        let json = serde_json::to_string(&old).unwrap();
        assert!(!json.contains("failures") && !json.contains("mtp_failed"), "{json}");
    }

    const KEY: &str = "0.10.6/start-test.gguf";

    fn up_ok(mtp: bool, off_card: bool) -> Result<Started, Failed> {
        Ok(Started { url: "http://127.0.0.1:1/v1".into(), mtp, off_card, key: KEY.into() })
    }

    fn down(why: Failure, mtp: bool) -> Result<Started, Failed> {
        Err(Failed { why, mtp, key: KEY.into() })
    }

    /// [`start_with`] over `script` (one result a try), in `dir`: what it
    /// came to and the tries it made.
    async fn start_scripted(dir: &Path, went_back: &AtomicBool, closing: bool, script: Vec<Result<Started, Failed>>) -> (Result<String, String>, Vec<Attempt>) {
        let mut script = script.into_iter();
        let mut tried = Vec::new();
        let out = start_with(dir, went_back, &|| closing, |a| {
            tried.push(a);
            std::future::ready(script.next().expect("a scripted try"))
        })
        .await;
        (out, tried)
    }

    /// A runtime just swapped in, not started yet.
    fn swapped(name: &str) -> PathBuf {
        let dir = scratch(name);
        std::fs::write(dir.join(RUNTIME_FILENAME), "old").unwrap();
        std::fs::write(dir.join(RUNTIME_NEXT), "new").unwrap();
        swap_in(&dir, "0.10.6", "aa").unwrap();
        dir
    }

    /// The review's first finding (2026-10-22): Yap stopping the sidecar
    /// while a new runtime started counted as the runtime failing — it went
    /// back and refused it for good. Stopped, missing or quitting: nothing
    /// tried again, nothing blamed.
    #[tokio::test]
    async fn a_start_yap_stopped_is_never_blamed_on_the_runtime() {
        let dir = swapped("start-stopped");
        let went_back = AtomicBool::new(false);
        let (out, tried) = start_scripted(&dir, &went_back, false, vec![down(Failure::Stopped, true)]).await;
        assert!(out.is_err());
        assert_eq!(tried, vec![Attempt::FIRST]);
        // Stopped on the second try too.
        let (_, tried) = start_scripted(&dir, &went_back, false, vec![down(Failure::Exited, false), down(Failure::Stopped, false)]).await;
        assert_eq!(tried.len(), 2);
        // Missing files; Yap quitting.
        let (_, tried) = start_scripted(&dir, &went_back, false, vec![down(Failure::Missing("no model".into()), false)]).await;
        assert_eq!(tried.len(), 1);
        let (_, tried) = start_scripted(&dir, &went_back, true, vec![down(Failure::Exited, true)]).await;
        assert_eq!(tried.len(), 1);
        // The new runtime is still in place, still to be proven, not counted.
        assert_eq!(read(&dir, RUNTIME_FILENAME), "new");
        let m = read_marker_at(&dir.join(RUNTIME_MARKER)).unwrap();
        assert!(m.unproven && m.failures.is_empty() && m.refused.is_empty());
        assert!(!went_back.load(Ordering::SeqCst));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The review's second finding: a start with MTP that failed never
    /// tried without it. Now it does, longer, and MTP stays off for that
    /// model on that runtime here.
    #[tokio::test]
    async fn a_start_that_fails_with_mtp_tries_again_without_it() {
        let dir = swapped("start-mtp");
        let went_back = AtomicBool::new(false);
        let (out, tried) = start_scripted(&dir, &went_back, false, vec![down(Failure::Exited, true), up_ok(false, false)]).await;
        assert_eq!(out.as_deref(), Ok("http://127.0.0.1:1/v1"));
        assert_eq!(tried, vec![Attempt::FIRST, Attempt::AGAIN]);
        assert!(!Attempt::AGAIN.mtp && Attempt::AGAIN.wait > Attempt::FIRST.wait);
        let m = read_marker_at(&dir.join(RUNTIME_MARKER)).unwrap();
        assert_eq!(m.mtp_failed, vec![KEY.to_string()]);
        assert!(mtp_failed_here(&dir, KEY));
        // It started: proven, the previous one gone.
        assert!(!m.unproven && !dir.join(RUNTIME_PREVIOUS).exists());
        // A slow first start without MTP: up on the second try, MTP not blamed.
        let dir2 = swapped("start-slow");
        let (out, _) = start_scripted(&dir2, &went_back, false, vec![down(Failure::Timeout, false), up_ok(false, false)]).await;
        assert!(out.is_ok());
        assert!(read_marker_at(&dir2.join(RUNTIME_MARKER)).unwrap().mtp_failed.is_empty());
        // Up with MTP but partly on the processor: again without it, and
        // without it from then on (the marker, not only this session).
        let dir3 = swapped("start-off-card");
        let (out, tried) = start_scripted(&dir3, &went_back, false, vec![up_ok(true, true), up_ok(false, false)]).await;
        assert!(out.is_ok());
        assert_eq!(tried, vec![Attempt::FIRST, Attempt::AGAIN]);
        assert_eq!(read_marker_at(&dir3.join(RUNTIME_MARKER)).unwrap().mtp_failed, vec![KEY.to_string()]);
        for d in [dir, dir2, dir3] {
            let _ = std::fs::remove_dir_all(&d);
        }
    }

    /// Twice down on the new runtime: the previous one goes back and starts;
    /// the new one waits for the next launch, refused after three.
    #[tokio::test]
    async fn a_new_runtime_down_twice_goes_back_and_is_refused_after_three_sessions() {
        let dir = swapped("start-back");
        for session in 1..=REFUSE_AFTER {
            let went_back = AtomicBool::new(false);
            let (out, tried) =
                start_scripted(&dir, &went_back, false, vec![down(Failure::Exited, true), down(Failure::Exited, false), up_ok(false, false)]).await;
            assert!(out.is_ok(), "the previous one serves");
            assert_eq!(tried, vec![Attempt::FIRST, Attempt::AGAIN, Attempt::AGAIN]);
            assert!(went_back.load(Ordering::SeqCst));
            assert_eq!(read(&dir, RUNTIME_FILENAME), "old");
            let m = read_marker_at(&dir.join(RUNTIME_MARKER)).unwrap();
            assert_eq!(m.failures.get("aa"), Some(&session));
            if session < REFUSE_AFTER {
                assert!(m.refused.is_empty() && dir.join(RUNTIME_NEXT).is_file(), "kept for the next launch");
                assert!(upgrade_due(true, Some(&m), "aa"));
                swap_in(&dir, "0.10.6", "aa").unwrap(); // the next launch's upgrade
            } else {
                assert_eq!(m.refused, vec!["aa".to_string()]);
                assert!(!dir.join(RUNTIME_NEXT).exists(), "refused: deleted");
                assert!(!upgrade_due(true, Some(&m), "aa"));
            }
        }
        // Once a session: a second failure the same session isn't another way back.
        let dir2 = swapped("start-once");
        let went_back = AtomicBool::new(true);
        let (out, tried) = start_scripted(&dir2, &went_back, false, vec![down(Failure::Exited, false), down(Failure::Exited, false)]).await;
        assert!(out.is_err() && tried.len() == 2);
        assert_eq!(read(&dir2, RUNTIME_FILENAME), "new");
        for d in [dir, dir2] {
            let _ = std::fs::remove_dir_all(&d);
        }
    }

    /// Neither runtime starts: the model or the PC is at fault — the new one
    /// goes back in and the failure isn't counted.
    #[tokio::test]
    async fn when_neither_runtime_starts_the_new_one_stays_uncounted() {
        let dir = swapped("start-neither");
        let went_back = AtomicBool::new(false);
        let (out, tried) =
            start_scripted(&dir, &went_back, false, vec![down(Failure::Timeout, false), down(Failure::Timeout, false), down(Failure::Exited, false)]).await;
        assert!(out.is_err());
        assert_eq!(tried.len(), 3);
        assert_eq!((read(&dir, RUNTIME_FILENAME), read(&dir, RUNTIME_PREVIOUS)), ("new".into(), "old".into()));
        let m = read_marker_at(&dir.join(RUNTIME_MARKER)).unwrap();
        assert!(m.unproven && m.failures.is_empty() && m.refused.is_empty());
        // A proven runtime that won't start: tried twice, nothing else.
        let proven = scratch("start-proven");
        std::fs::write(proven.join(RUNTIME_FILENAME), "it").unwrap();
        let (_, tried) = start_scripted(&proven, &AtomicBool::new(false), false, vec![down(Failure::Exited, false), down(Failure::Exited, false)]).await;
        assert_eq!(tried.len(), 2);
        for d in [dir, proven] {
            let _ = std::fs::remove_dir_all(&d);
        }
    }

    #[test]
    fn mtp_on_nvidia_cards_only_and_never_where_it_failed() {
        assert!(mtp_allowed(NEW, true, Some(NVIDIA), false));
        assert!(!mtp_allowed(NEW, true, Some(0x1002), false), "AMD: Vulkan, not measured");
        assert!(!mtp_allowed(NEW, true, Some(0x8086), false), "Intel");
        assert!(!mtp_allowed(NEW, true, None, false), "no card");
        assert!(!mtp_allowed(OLD, true, Some(NVIDIA), false));
        assert!(!mtp_allowed(NEW, false, Some(NVIDIA), false), "turned off");
        assert!(!mtp_allowed(NEW, true, Some(NVIDIA), true), "failed with it here");
        assert_eq!(mtp_key(NEW, Path::new("C:/x/llm/Qwen_Qwen3.5-4B-Q4_K_M.gguf")), "0.10.6/Qwen_Qwen3.5-4B-Q4_K_M.gguf");
        let dir = scratch("mtp-failed");
        write_marker(&dir, &Marker { version: "0.10.6".into(), mtp_failed: vec!["0.10.6/a.gguf".into()], ..Default::default() }).unwrap();
        assert!(mtp_failed_here(&dir, "0.10.6/a.gguf"));
        assert!(!mtp_failed_here(&dir, "0.10.7/a.gguf"), "a newer runtime tries again");
        assert!(!mtp_failed_here(&dir, "0.10.6/b.gguf"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn one_upgrade_at_a_time_across_yaps() {
        let dir = scratch("upgrade-lock");
        let held = UpgradeLock::take_unless(&dir, |_| true).expect("free");
        assert!(UpgradeLock::take_unless(&dir, |_| true).is_none(), "another running Yap holds it");
        drop(held);
        assert!(!dir.join(UPGRADE_LOCK).exists(), "released");
        // A lock left by a Yap that's gone: taken over.
        std::fs::write(dir.join(UPGRADE_LOCK), "4000000").unwrap();
        let _ours = UpgradeLock::take_unless(&dir, |_| false).expect("stale");
        assert_eq!(read(&dir, UPGRADE_LOCK), std::process::id().to_string());
        // The disk: room for the new runtime beside the old one, plus some.
        assert!(room_for(None, UPGRADE_ROOM), "unknown: go");
        assert!(room_for(Some(UPGRADE_ROOM), UPGRADE_ROOM));
        assert!(!room_for(Some(RUNTIME_BYTES), UPGRADE_ROOM));
        assert!(free_bytes(&dir).is_some_and(|f| f > 0) || cfg!(not(windows)));
        // Not the sidecar's address: nothing held.
        assert!(hold("http://127.0.0.1:9/v1").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The review's fourth finding: the swap waited for idle, then for the
    /// start lock — and a request that began meanwhile was cut off. Now it
    /// looks again once it has the lock, and goes back to waiting.
    #[tokio::test]
    async fn the_swap_backs_off_when_a_request_began_while_it_waited_for_the_lock() {
        use std::sync::Arc;
        static R: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
        static S: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
        let busy = Arc::new(AtomicBool::new(false));
        let done = Arc::new(AtomicBool::new(false));
        let start_under_way = S.lock().await;
        let task = {
            let (busy, done) = (busy.clone(), done.clone());
            tokio::spawn(async move {
                let _locks = idle_with_locks(&R, &S, || !busy.load(Ordering::SeqCst), Duration::from_millis(1), Duration::from_millis(1)).await;
                done.store(true, Ordering::SeqCst);
            })
        };
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!done.load(Ordering::SeqCst), "waiting for the start lock");
        // A Chat turn begins while it waits; then the start lock comes free.
        busy.store(true, Ordering::SeqCst);
        drop(start_under_way);
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!done.load(Ordering::SeqCst), "it backed off");
        busy.store(false, Ordering::SeqCst);
        task.await.unwrap();
        assert!(done.load(Ordering::SeqCst));
    }

    #[test]
    fn mtp_only_for_a_model_that_can_draft_on_a_card_with_room() {
        let dir = scratch("mtp");
        let qwen35 = dir.join("Qwen_Qwen3.5-4B-Q4_K_M.gguf");
        std::fs::write(&qwen35, crate::ai::gguf::tests::qwen35_4b(1_000).bytes()).unwrap();
        let qwen25 = dir.join(MODEL_FILENAME);
        std::fs::write(&qwen25, crate::ai::gguf::tests::qwen25_1_5b(1_000).bytes()).unwrap();
        // A 16 GB card: room for it (the stand-in file is small; the KV,
        // recurrent state and compute aren't).
        assert!(mtp_fits(&qwen35, 8_192, Some(16_303)));
        // No prediction layer, no card of its own, or no room: off.
        assert!(!mtp_fits(&qwen25, 8_192, Some(16_303)));
        assert!(!mtp_fits(&qwen35, 8_192, None));
        assert!(!mtp_fits(&qwen35, 8_192, Some(512)), "integrated graphics");
        assert!(!mtp_fits(&qwen35, 8_192, Some(2_600)), "no room beside Windows and speech");
        // At 256K the KV alone is 8 GiB: not on a 12 GB card with MTP.
        assert!(!mtp_fits(&qwen35, 262_144, Some(12_282)));
        // The plan: only on 0.10.6, only with the setting on.
        let cfg = crate::config::YapConfig::default();
        assert!(cfg.local_mtp, "on by default");
        assert!(!plan_for(&cfg, &qwen35, 8_192, OLD).server().mtp);
        assert!(!plan_for(&crate::config::YapConfig { local_mtp: false, ..cfg.clone() }, &qwen35, 8_192, NEW).mtp);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn versions_read_and_order() {
        assert_eq!(Version::parse("llamafile v0.10.6"), Some(NEW));
        assert_eq!(Version::parse("0.10.3"), Some(OLD));
        assert_eq!(Version::parse("v0.11"), Some(Version(0, 11, 0)));
        assert_eq!(Version::parse("llamafile"), None);
        assert_eq!(Version::parse(RUNTIME_VERSION), Some(NEW));
        assert!(NEW.is_current() && Version(0, 11, 0).is_current());
        assert!(!OLD.is_current() && !Version(0, 10, 5).is_current());
        assert_eq!(NEW.to_string(), "0.10.6");
        // The pinned runtime: mozilla-ai's release, its digest.
        assert!(RUNTIME_URL.starts_with("https://github.com/mozilla-ai/llamafile/releases/download/0.10.6/"));
        assert_eq!(RUNTIME_SHA256.len(), 64);
    }

    #[test]
    fn the_model_id_is_the_files_name() {
        assert_eq!(model_id(Path::new("C:/x/llm/Qwen_Qwen3.5-4B-Q4_K_M.gguf")), "Qwen_Qwen3.5-4B-Q4_K_M");
        assert_eq!(model_id(&model_path()), "qwen2.5-1.5b-instruct-q4_k_m");
        // The default's id gets no tools; the 4B's does.
        assert!(!crate::chat::tools::supports_tools(PROVIDER_ONDEVICE, &model_id(&model_path())));
        assert!(crate::chat::tools::supports_tools(PROVIDER_ONDEVICE, "Qwen_Qwen3.5-4B-Q4_K_M"));
    }

    #[test]
    fn the_context_is_the_setting_within_what_the_model_was_trained_for() {
        // Not set: today's 8K.
        assert_eq!(effective_context(0, Some(262_144)), 8_192);
        assert_eq!(effective_context(0, None), DEFAULT_CONTEXT);
        assert_eq!(effective_context(32_768, Some(262_144)), 32_768);
        // Never more than the model's maximum (Gemma 2: 8K; the 1.5B: 32K)…
        assert_eq!(effective_context(32_768, Some(8_192)), 8_192);
        assert_eq!(effective_context(131_072, Some(32_768)), 32_768);
        // …nor under the floor, unless the model can't do more.
        assert_eq!(effective_context(512, Some(32_768)), MIN_CONTEXT);
        assert_eq!(effective_context(0, Some(1_024)), 1_024);
        assert_eq!(effective_context(u32::MAX, None), MAX_CONTEXT);
        // A header's absurd claim is held to the ceiling too.
        assert_eq!(effective_context(u32::MAX, Some(u64::MAX)), MAX_CONTEXT);
        // How the setting is kept: 0 for the default.
        assert_eq!(context_setting(8_192), Ok(0));
        assert_eq!(context_setting(0), Ok(0));
        assert_eq!(context_setting(32_768), Ok(32_768));
        assert!(context_setting(100).is_err());
        assert!(context_setting(MAX_CONTEXT + 1).is_err());
    }

    #[test]
    fn the_choices_stop_at_the_models_maximum() {
        assert_eq!(context_choices(Some(32_768), 8_192), vec![4_096, 8_192, 16_384, 32_768]);
        assert_eq!(context_choices(Some(262_144), 8_192), CONTEXT_CHOICES.to_vec());
        assert_eq!(context_choices(Some(8_192), 8_192), vec![4_096, 8_192]);
        // A maximum between two choices is offered itself (Qwen3's 40K).
        assert_eq!(context_choices(Some(40_960), 8_192), vec![4_096, 8_192, 16_384, 32_768, 40_960]);
        // 1M models stop at 256K; an unknown maximum at 32K.
        assert_eq!(*context_choices(Some(1_048_576), 8_192).last().unwrap(), 262_144);
        assert_eq!(*context_choices(None, 8_192).last().unwrap(), 32_768);
        // The current one is always there.
        assert!(context_choices(Some(32_768), 24_576).contains(&24_576));
        // Every curated model has its maximum.
        assert!(CURATED_MODELS.iter().all(|m| m.max_context >= 8_192));
        assert_eq!(curated_by_id(GPU_MODEL_ID).unwrap().max_context, 262_144);
    }

    #[test]
    fn a_curated_file_not_readable_yet_has_the_curated_maximum() {
        assert_eq!(trained_context(Path::new("C:/nowhere/Qwen_Qwen3.5-4B-Q4_K_M.gguf")), Some(262_144));
        assert_eq!(trained_context(Path::new("C:/nowhere/gemma-2-2b-it-Q4_K_M.gguf")), Some(8_192));
        assert_eq!(trained_context(Path::new("C:/nowhere/someone-elses.gguf")), None);
        // A real header wins.
        let dir = std::env::temp_dir().join(format!("yap-ctx-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("gemma-2-2b-it-Q4_K_M.gguf");
        std::fs::write(&p, crate::ai::gguf::tests::qwen25_1_5b(10).bytes()).unwrap();
        assert_eq!(trained_context(&p), Some(32_768));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_log_says_the_context_and_the_memory() {
        let log = "\
0.00.001.307 I   - CPU     : CPU (63092 MiB, 63092 MiB free)
0.00.048.473 I   - CUDA0   : NVIDIA GeForce RTX 5070 Ti (16275 MiB, 15037 MiB free)
0.00.810.830 I load_tensors:   CPU_Mapped model buffer size =   497.31 MiB
0.00.810.831 I load_tensors:        CUDA0 model buffer size =  2862.99 MiB
0.01.685.153 I llama_context: n_seq_max     = 4
0.01.685.153 I llama_context: n_ctx         = 32768
0.01.685.153 I llama_context: n_ctx_seq     = 32768
0.01.685.157 W llama_context: n_ctx_seq (32768) < n_ctx_train (262144) -- the full capacity of the model will not be utilized
0.01.685.703 I llama_context:  CUDA_Host  output buffer size =     3.79 MiB
0.01.686.092 I llama_kv_cache:      CUDA0 KV buffer size =  1024.00 MiB
0.01.691.975 I llama_memory_recurrent:      CUDA0 RS buffer size =   201.00 MiB
0.01.697.662 I sched_reserve:      CUDA0 compute buffer size =   519.00 MiB
0.01.697.667 I sched_reserve:  CUDA_Host compute buffer size =    74.02 MiB
0.01.789.140 I slot   load_model: id  0 | task -1 | new slot, n_ctx = 32768
";
        let f = log_facts(log);
        assert_eq!(f.n_ctx, Some(32_768));
        assert_eq!(f.gpu_mib, Some(2_863 + 1_024 + 201 + 519));
        assert_eq!(f.host_mib, Some(497 + 4 + 74));
        // A processor-only run: nothing on a card.
        let cpu = log_facts("I load_tensors:   CPU_Mapped model buffer size =  1059.89 MiB\nI llama_kv_cache:        CPU KV buffer size =   224.00 MiB\n");
        assert_eq!((cpu.gpu_mib, cpu.host_mib), (None, Some(1_284)));
        assert_eq!(log_facts(""), LogFacts::default());
    }

    #[test]
    fn the_card_has_room_for_its_memory_less_what_windows_and_speech_keep() {
        assert_eq!(card_room(Some(15_995)), Some(15_995 - 2_048));
        // A "6 GB" card: the 4B at 8K (~3.8 GB) fits, at 32K (~4.6) it doesn't.
        assert_eq!(card_room(Some(5_966)), Some(3_918));
        assert_eq!(card_room(Some(1_000)), Some(0));
        assert_eq!(card_room(None), None);
        // Where the estimates are of: the card, unless the log says the
        // processor, or there's no card with memory of its own.
        let cpu = Some(Placement { gpu_layers: 0, layers: 34 });
        let some = Some(Placement { gpu_layers: 20, layers: 34 });
        assert!(!runs_on_card(cpu, Some(15_995)));
        assert!(runs_on_card(some, Some(15_995)));
        assert!(runs_on_card(None, Some(8_176)));
        assert!(!runs_on_card(None, Some(512)), "integrated graphics");
        assert!(!runs_on_card(None, None));
    }

    #[test]
    fn a_bigger_context_scales_the_budgets_by_whole_8k_steps() {
        assert_eq!(budget_factor(8_192), 1);
        assert_eq!(budget_factor(4_096), 1, "never below the 8K budgets' floor");
        assert_eq!(budget_factor(12_288), 1);
        assert_eq!(budget_factor(16_384), 2);
        assert_eq!(budget_factor(32_768), 4);
        assert_eq!(budget_factor(262_144), MAX_BUDGET_FACTOR);
        // Not the on-device model (or it's down): the 8K budgets.
        assert_eq!(budget_factor_for("groq", "https://api.groq.com/openai/v1"), 1);
        assert_eq!(budget_factor_for(PROVIDER_ONDEVICE, "https://api.groq.com/openai/v1"), 1);
    }

    #[test]
    fn a_new_context_restarts_a_running_model_only() {
        assert!(needs_restart(Some(8_192), 32_768));
        assert!(!needs_restart(Some(32_768), 32_768));
        assert!(!needs_restart(None, 32_768), "a stopped one starts with it anyway");
        // Never while a dictation records or is cleaned up; settle after one.
        assert_eq!(restart_step(true, false, false), RestartStep::Wait);
        assert_eq!(restart_step(true, true, true), RestartStep::Wait);
        assert_eq!(restart_step(false, false, false), RestartStep::Go);
        assert_eq!(restart_step(false, true, false), RestartStep::Settle);
        assert_eq!(restart_step(false, true, true), RestartStep::Go);
    }

    #[tokio::test]
    async fn the_restart_waits_for_the_dictation_to_end() {
        use std::sync::atomic::AtomicU32;
        use std::sync::Arc;
        // A dictation records for the first 5 looks; the restart may only go
        // after it, and after the settle.
        let looks = Arc::new(AtomicU32::new(0));
        let l = looks.clone();
        let busy = move || l.fetch_add(1, Ordering::SeqCst) < 5;
        let went = wait_until_idle(busy, || true, Duration::from_millis(1), Duration::from_millis(1)).await;
        assert!(went);
        // 5 busy looks, the one that saw it idle (settle), the one after.
        assert_eq!(looks.load(Ordering::SeqCst), 7);
        // Idle from the start: at once.
        let looks = Arc::new(AtomicU32::new(0));
        let l = looks.clone();
        assert!(wait_until_idle(move || { l.fetch_add(1, Ordering::SeqCst); false }, || true, Duration::from_millis(1), Duration::from_millis(1)).await);
        assert_eq!(looks.load(Ordering::SeqCst), 1);
        // A newer change takes over: this one gives up, never restarting.
        let newer = Arc::new(AtomicBool::new(false));
        let n = newer.clone();
        let busy = move || {
            n.store(true, Ordering::SeqCst);
            true
        };
        let latest = {
            let n = newer.clone();
            move || !n.load(Ordering::SeqCst)
        };
        assert!(!wait_until_idle(busy, latest, Duration::from_millis(1), Duration::from_millis(1)).await);
    }
}

