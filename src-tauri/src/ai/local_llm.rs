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
//! expose the base URL, and kill on stop/exit. Beside it: [`runtime`] (the
//! llamafile on disk and its background upgrade), [`context`] (the context
//! length, and a restart when it changes), [`models`] (the curated
//! models and their downloads), [`uninstall`] (a model deleted from
//! `llm/`, never the one in use) and [`vision`] (its projector: the model
//! reading screenshots, loaded only while it's needed).

pub mod context;
pub mod models;
pub mod on_demand;
pub mod runtime;
pub mod uninstall;
pub mod vision;

use std::path::{Path, PathBuf};
use crate::privacy::net::Recorded as _;
use std::process::Child;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};

use context::{
    card_room, context_for, context_setting, context_view, needs_restart, restart_pending, restart_when_idle, runs_on_card,
    ContextView,
};
use models::CURATED_MODELS;
use runtime::{
    count_failure, prove, read_marker_at, refused_in, roll_back, runtime_version, runtime_version_in, swap_in, uncount_failure,
    update_marker, Version,
};

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

/// Cold model-load can take a while on first start; cap the health wait.
const HEALTH_TIMEOUT_SECS: u64 = 120;
/// The wait for a second try ([`Attempt::AGAIN`]): a first run of a new
/// runtime can take minutes (an antivirus reading 368 MB), a big model on a
/// slow disk too.
const HEALTH_TIMEOUT_AGAIN_SECS: u64 = 300;

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
    /// A q8_0 KV cache ([`kv_q8_helps`]): only where it makes a long
    /// context fit on the card.
    pub kv_q8: bool,
    /// **Two slots with a cache each, on the processor** ([`plan_for`]).
    pub per_slot: bool,
}

impl Plan {
    /// The memory estimate's view of it (`gguf::estimate`).
    pub fn server(self) -> crate::ai::gguf::Server {
        let current = self.runtime.is_current();
        crate::ai::gguf::Server {
            current,
            mtp: current && self.mtp,
            kv_q8: current && self.kv_q8,
            per_slot: current && self.per_slot,
        }
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
    let per_slot = plan.server().per_slot;
    // Per-slot caches: the whole context for each slot (llama.cpp gives a
    // slot `-c` / `-np` without a unified cache).
    let total = if per_slot { ctx.saturating_mul(crate::ai::gguf::PER_SLOT_SLOTS as u32) } else { ctx };
    let mut args = base_args(model, port, ngl, total);
    if per_slot {
        args.extend(["-np".to_string(), crate::ai::gguf::PER_SLOT_SLOTS.to_string(), "--no-kv-unified".to_string()]);
    }
    if plan.server().mtp {
        args.extend(["--spec-type".to_string(), "draft-mtp".to_string()]);
    }
    // A matching K and V pair: CUDA's flash attention has kernels for
    // q8_0/q8_0 only (a mismatched pair falls back to the processor).
    if plan.server().kv_q8 && ngl > 0 {
        args.extend(["-ctk", "q8_0", "-ctv", "q8_0"].map(String::from));
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
    // A request's context: `n_ctx_seq` (per-slot caches give each slot
    // `n_ctx` / slots), else `n_ctx`.
    let mut per_seq: Option<u32> = None;
    for line in log.lines() {
        if let Some(rest) = line.split("llama_context:").nth(1) {
            let rest = rest.trim_start();
            let value = |after: &str| -> Option<u32> {
                after.starts_with([' ', '=']).then(|| after.split('=').nth(1).and_then(|v| v.trim().parse().ok())).flatten()
            };
            if let Some(after) = rest.strip_prefix("n_ctx_seq") {
                if let Some(v) = value(after) {
                    per_seq = Some(v);
                }
            } else if let Some(after) = rest.strip_prefix("n_ctx") {
                if let Some(v) = value(after) {
                    f.n_ctx = Some(v);
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
    // The projector's weights (its compute buffer is a line above).
    if let Some((mib, on_card)) = vision::in_log(log) {
        if on_card {
            gpu += mib as f64;
            any_gpu = true;
        } else {
            host += mib as f64;
            any_host = true;
        }
    }
    f.n_ctx = per_seq.or(f.n_ctx);
    f.gpu_mib = any_gpu.then(|| gpu.round() as u64);
    f.host_mib = any_host.then(|| host.round() as u64);
    f
}

static CHILD: Mutex<Option<Child>> = Mutex::new(None);
static PORT: AtomicU16 = AtomicU16::new(0);
/// The ports the sidecar served on this session (kept after a stop or a
/// restart): a call that resolved its address before the model was freed
/// or restarted goes to the model as it is now, never to whatever listens
/// on the old port ([`was_ours`], [`served_on`]).
static SERVED_PORTS: Mutex<Vec<u16>> = Mutex::new(Vec::new());

/// The sidecar is serving on `port` (kept for [`was_ours`]).
fn served_on(port: u16) {
    if let Ok(mut p) = SERVED_PORTS.lock() {
        if !p.contains(&port) {
            p.push(port);
            // A session's few restarts; never a long list.
            if p.len() > 32 {
                p.remove(0);
            }
        }
    }
}
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
        // The idle clock counts from the end of the last request.
        on_demand::touch();
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
///
/// The stand-by address (the model not loaded yet, [`on_demand::STANDBY_URL`])
/// counts too: the run is about to load it, and it isn't freed meanwhile.
pub fn hold(url: &str) -> Option<Busy> {
    let url = url.trim_end_matches('/');
    if !(base_url().is_some_and(|ours| url == ours) || on_demand::is_standby(url)) {
        return None;
    }
    IN_FLIGHT.fetch_add(1, Ordering::SeqCst);
    on_demand::touch();
    Some(Busy)
}

/// Is `url` an address the sidecar served on before, but not now (freed
/// while idle, restarted on another port)? A call to it goes to the model
/// as it is now — loaded again if need be (`llm::post_chat_request`).
pub fn was_ours(url: &str) -> bool {
    let url = url.trim_end_matches('/');
    if base_url().is_some_and(|now| now == url) {
        return false;
    }
    let Some(port) = url.strip_prefix("http://127.0.0.1:").and_then(|r| r.strip_suffix("/v1")).and_then(|p| p.parse::<u16>().ok()) else {
        return false;
    };
    SERVED_PORTS.lock().is_ok_and(|p| p.contains(&port))
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
    /// Its projector (it reads screenshots, [`vision`]): `Some(true)` on the
    /// graphics card, `Some(false)` on the processor, `None` not loaded.
    vision: Option<bool>,
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

/// What each model's chat format said about tools when it last ran, by
/// model id ([`template_takes_tools`]); kept after the sidecar stops.
static TEMPLATE_TOOLS: Mutex<Option<std::collections::HashMap<String, bool>>> = Mutex::new(None);

/// Does `model`'s chat format have a place for tools? llama.cpp's own word
/// for the one it serves (`GET /props`, `chat_template_caps.supports_tools`),
/// asked once it's up: `Some(false)` for one with none (Gemma 2), whose
/// engine then never shows the model Chat's tools, so it gets none
/// (`tools::supports_tools`). `None` before it has run here, or when its
/// server didn't say.
pub fn template_takes_tools(model: &str) -> Option<bool> {
    TEMPLATE_TOOLS.lock().ok()?.as_ref()?.get(model).copied()
}

/// Ask the sidecar at `port` whether its model's chat format takes tools,
/// and keep the answer under `model` ([`template_takes_tools`]).
async fn hear_template_tools(client: &reqwest::Client, port: u16, model: &str) {
    let asked = async {
        let resp = client
            .get(format!("http://127.0.0.1:{port}/props"))
            .send_for(crate::privacy::net::Purpose::OnDeviceCheck)
            .await
            .ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let v: serde_json::Value = resp.json().await.ok()?;
        template_caps_take_tools(&v)
    };
    let Some(takes) = asked.await else { return };
    if !takes {
        tracing::info!(model, "On-device model: its chat format has no place for tools; Chat answers without them");
    }
    remember_template_tools(model, takes);
}

/// Keep what `model`'s chat format said about tools.
pub(crate) fn remember_template_tools(model: &str, takes: bool) {
    if let Ok(mut g) = TEMPLATE_TOOLS.lock() {
        g.get_or_insert_with(Default::default).insert(model.to_string(), takes);
    }
}

/// `/props`'s `chat_template_caps.supports_tools`. Pure.
pub(crate) fn template_caps_take_tools(props: &serde_json::Value) -> Option<bool> {
    props["chat_template_caps"]["supports_tools"].as_bool()
}

/// The running sidecar's runtime and MTP (`None` while it isn't running).
pub fn serving_plan() -> Option<Plan> {
    SERVING.lock().ok()?.as_ref().map(|s| s.plan)
}

/// The running sidecar's projector ([`vision`]): `Some(true)` on the card,
/// `Some(false)` on the processor; `None` when it isn't running or was
/// started without it.
pub fn serving_vision() -> Option<bool> {
    SERVING.lock().ok()?.as_ref().and_then(|s| s.vision)
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
    if !crate::shell::e2e::active() {
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
    if !crate::shell::e2e::active() {
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
    if !crate::shell::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    E2E_BUSY.store(busy, Ordering::SeqCst);
    Ok(())
}

/// Test mode: the stand-in sidecar's starts ([`e2e_stand_in_start`]).
#[cfg(debug_assertions)]
static E2E_STARTS: AtomicU32 = AtomicU32::new(0);

/// Test mode: an OpenAI-compatible stand-in (the e2e suite's fake AI, its
/// `…/v1` address in `YAP_E2E_LOCAL_LLM`) serves as the sidecar — no
/// process is started.
#[cfg(debug_assertions)]
fn e2e_stand_in() -> Option<String> {
    if !crate::shell::e2e::active() {
        return None;
    }
    std::env::var("YAP_E2E_LOCAL_LLM").ok().filter(|u| !u.trim().is_empty())
}

/// Test mode: "start" the stand-in: after `YAP_E2E_LOCAL_LLM_LOAD_MS`
/// (default 400 ms, the model "loading"), the sidecar is up at its port,
/// serving the active model's id. A stop meanwhile stops it, as for a real
/// start.
#[cfg(debug_assertions)]
async fn e2e_stand_in_start(url: &str, gen: u64) -> Result<String, String> {
    let ms = std::env::var("YAP_E2E_LOCAL_LLM_LOAD_MS").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(400);
    tokio::time::sleep(Duration::from_millis(ms)).await;
    if CLOSING.load(Ordering::SeqCst) || STOP_GEN.load(Ordering::SeqCst) != gen {
        return Err(Failure::Stopped.to_string());
    }
    let port = url::Url::parse(url).ok().and_then(|u| u.port()).ok_or("the stand-in's address has no port")?;
    let cfg = crate::config::load();
    let model = active_model_path(&cfg);
    if let Ok(client) = reqwest::Client::builder().timeout(Duration::from_secs(3)).build() {
        hear_template_tools(&client, port, &model_id(&model)).await;
    }
    if let Ok(mut s) = SERVING.lock() {
        let plan = Plan { runtime: Version(0, 10, 6), mtp: false, kv_q8: false, per_slot: false };
        // Its projector as a real start would load it (asked for, usable).
        let vision = vision::for_start(&cfg, &model, context_for(&cfg), plan, 999).map(|v| v.on_card);
        *s = Some(Serving {
            model_id: model_id(&model),
            placement: None,
            ctx: context_for(&cfg),
            plan,
            hybrid: false,
            facts: LogFacts::default(),
            vision,
        });
    }
    PORT.store(port, Ordering::SeqCst);
    READY.store(true, Ordering::SeqCst);
    served_on(port);
    on_demand::touch();
    E2E_STARTS.fetch_add(1, Ordering::SeqCst);
    tracing::info!(port, "e2e: the on-device model's stand-in is up");
    Ok(format!("http://127.0.0.1:{port}/v1"))
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
/// The on-device model's name for the id a call sent ([`model_id`]: its
/// GGUF's stem): the bundled default's, a curated model's, else the id
/// itself (a GGUF of the person's own). The AI usage tracker's rows.
pub fn display_for_id(id: &str) -> String {
    if id == model_id(&model_path()) {
        return MODEL_DISPLAY.to_string();
    }
    CURATED_MODELS
        .iter()
        .find(|m| m.filename.trim_end_matches(".gguf") == id)
        .map(|m| m.display.to_string())
        .unwrap_or_else(|| id.to_string())
}

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

/// A file in `llm/` that's a model: a GGUF, never a vision projector
/// (`mmproj-…`: Yap keeps its own in `llm/vision/`, and one dropped here
/// can't be served as a model). Pure.
pub fn is_model_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".gguf") && !lower.starts_with("mmproj")
}

/// All GGUF filenames in `llm/`, sorted, bundled default first. Powers the
/// model picker in Settings.
pub fn list_models() -> Vec<String> {
    let mut models: Vec<String> = std::fs::read_dir(llm_dir())
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.path().is_file())
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|n| is_model_file(n))
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

/// A start is under way (at launch, on demand, Settings' Start, Chat
/// waking it, a restart).
pub fn starting() -> bool {
    STARTING.try_lock().is_err()
}

/// The `(base_url, api_key, model, provider)` the cleanup client should use:
/// the managed on-device sidecar when the provider is "ondevice" AND it's up;
/// when it's installed but not loaded, the stand-by address
/// ([`on_demand::STANDBY_URL`]: the call loads it); else the user's
/// configured cloud/local endpoint (not installed: worst case the raw
/// transcript, never a hang). `provider` is the id the call is attributed
/// to for the per-provider usage meter.
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
        // Not loaded (yet, or freed while idle) but here: the stand-by
        // address, which the call loads it at (`llm::post_chat_request`,
        // `on_demand::wake`) — never the endpoint configured underneath.
        // (Not in unit tests: what this PC has installed isn't theirs.)
        let cfg = if cfg!(test) { None } else { Some(crate::config::load()) };
        if let Some(cfg) = cfg.filter(|c| is_installed(c) && !CLOSING.load(Ordering::SeqCst)) {
            let id = model_id(&active_model_path(&cfg));
            return (on_demand::STANDBY_URL.to_string(), String::new(), id, PROVIDER_ONDEVICE.to_string());
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
    #[cfg(debug_assertions)]
    if let Some(url) = e2e_stand_in() {
        return e2e_stand_in_start(&url, gen).await;
    }
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
    let vendor = gpu.as_ref().map(|g| g.vendor);
    let vram = gpu.map(|g| g.vram_mib);
    let kv_q8 = runtime.is_current() && vendor == Some(NVIDIA) && kv_q8_rescues(model, ctx, vram);
    let failed = mtp_failed_here(&llm_dir(), &mtp_key(runtime, model));
    let mtp = mtp_allowed(runtime, cfg.local_mtp, vendor, failed) && mtp_fits(model, ctx, vram, kv_q8);
    let per_slot = per_slot_caches(runtime, runs_on_card(None, vram));
    Plan { runtime, mtp, kv_q8, per_slot }
}

/// **Two slots with a cache each, on the processor** (`-np 2
/// --no-kv-unified`, each slot the whole context; 2026-10-25): with
/// llama.cpp's one cache shared by 4 slots, a dictation's cleanup right
/// after a long Chat turn ran 2.0–2.6× slower on the processor
/// (Qwen2.5-1.5B, three runs: 0.80 → 1.74 s, 0.94 → 1.92 s, 1.29 → 3.14 s,
/// generation about halved; Qwen3.5-2B 1.10 → 2.88 s): its attention went
/// over the chat's cells too, and once, when the chat needed the room, the
/// cleanup's prompt was cleared and read again (556 tokens). Each slot
/// with its own cache, the cleanup reads only its own cells and keeps its
/// prompt: 1.0–1.1× (the 1.5B 0.96 → 0.98 s and 0.84 → 0.92 s, the 2B 1.03
/// → 1.17 s; docs/model-evals.md "The processor tier"). It costs the KV
/// once more (the 1.5B: +224 MiB of memory at 8K) and leaves two slots
/// instead of four; on a graphics card the shared cache was never the slow
/// part. llamafile 0.10.6 or newer (the flags). Pure.
pub fn per_slot_caches(runtime: Version, on_card: bool) -> bool {
    runtime.is_current() && !on_card
}

/// The context from which a **q8_0 KV cache** is considered (a long one).
pub const KV_Q8_FROM: u32 = 32_768;

/// **A q8_0 KV cache, only where it helps** (2026-10-25, measured on the
/// RTX 5070 Ti with Qwen3.5-4B, docs/model-evals.md "KV cache q8_0"): it
/// saves 0.4 GB on the card at 32K and 0.75 GB at 64K (more on a model with
/// attention in every layer) and the evaluations' answers were as good
/// (the suite 160/164 both ways; a 24K and a 56K needle test 6/6 both ways),
/// but Chat's generation ran ~25 % slower with it (150 → 110 tokens/s over
/// the suite). So it's on only when it makes a long context fit on the card
/// that otherwise wouldn't: a context of at least [`KV_Q8_FROM`] whose f16
/// cache doesn't fit in [`card_room`] while the q8_0 one does. Pure.
pub fn kv_q8_helps(ctx: u32, f16_mib: u64, q8_mib: u64, room_mib: Option<u64>) -> bool {
    match room_mib {
        Some(room) => ctx >= KV_Q8_FROM && f16_mib > room && q8_mib <= room,
        None => false,
    }
}

/// [`kv_q8_helps`] for `model` at `ctx` on a card of `vram_mib` (without
/// MTP: drafting is only added where there's room for it too).
pub fn kv_q8_rescues(model: &Path, ctx: u32, vram_mib: Option<u64>) -> bool {
    let Some(f) = crate::ai::gguf::model_file(model) else { return false };
    if !runs_on_card(None, vram_mib) {
        return false;
    }
    let at = |kv_q8| crate::ai::gguf::estimate(&f.shape, f.bytes, ctx as u64, true, crate::ai::gguf::Server { current: true, mtp: false, kv_q8, per_slot: false }).total_mib;
    kv_q8_helps(ctx, at(false), at(true), card_room(vram_mib))
}

/// Can `model` draft with MTP at `ctx`, and does it fit on a card of
/// `vram_mib` with it ([`card_room`]: the card's memory less what Windows,
/// the apps on screen and Yap's speech model keep there) — with the KV
/// cache as `kv_q8` says? `false` without a card of its own.
pub fn mtp_fits(model: &Path, ctx: u32, vram_mib: Option<u64>, kv_q8: bool) -> bool {
    let Some(f) = crate::ai::gguf::model_file(model) else { return false };
    if !f.shape.has_mtp() || !runs_on_card(None, vram_mib) {
        return false;
    }
    let Some(room) = card_room(vram_mib) else { return false };
    let server = crate::ai::gguf::Server { current: true, mtp: true, kv_q8, per_slot: false };
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
    // Its projector, when a screenshot asked for it (`vision::ensure`).
    let projector = vision::for_start(&cfg, &model, ctx, plan, 999);
    // A start with the projector never teaches that MTP fails here: the
    // projector may be what failed (a blank key is never noted).
    let key = if projector.is_some() { String::new() } else { mtp_key(plan.runtime, &model) };
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
    // on the processor, `placement`), thinking off, the caches capped; the
    // projector after them, on the card only where it fits beside the model.
    let mut args = server_args(&model, port, 999, ctx, plan);
    if let Some(p) = &projector {
        args.extend(vision::args(p));
    }
    cmd.args(args)
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
    crate::shell::procs::end_with_yap(&child);
    *CHILD.lock().unwrap_or_else(|p| p.into_inner()) = Some(child);
    PORT.store(port, Ordering::SeqCst);
    READY.store(false, Ordering::SeqCst);
    // A stop between the look above and the child being kept missed it.
    if stopped() {
        stop_inner();
        return Err(fail(Failure::Stopped));
    }
    tracing::info!(
        port,
        ctx,
        runtime = %plan.runtime,
        mtp = plan.server().mtp,
        projector = ?projector.as_ref().map(|p| if p.on_card { "card" } else { "processor" }),
        wait_s = attempt.wait.as_secs(),
        model = %model.display(),
        "Starting on-device cleanup sidecar"
    );

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
                // What its chat format takes: one with no place for tools
                // gets none of Chat's (`tools::supports_tools`).
                hear_template_tools(&client, port, &model_id(&model)).await;
                if stopped() {
                    return Err(fail(Failure::Stopped));
                }
                if let Ok(mut s) = SERVING.lock() {
                    let hybrid = crate::ai::gguf::model_file(&model).is_some_and(|f| f.shape.is_hybrid());
                    let vision = projector.as_ref().map(|p| p.on_card);
                    *s = Some(Serving { model_id: model_id(&model), placement, ctx, plan, hybrid, facts, vision });
                }
                READY.store(true, Ordering::SeqCst);
                served_on(port);
                on_demand::touch();
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
    for pid in crate::shell::procs::orphaned(RUNTIME_FILENAME) {
        if crate::shell::procs::kill(pid) {
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

/// At startup and after config saves: start the sidecar only with "Load
/// the on-device model when Yap starts" on (`local_model_at_launch`, off
/// by default) and something really routed to it ([`on_demand::launch_gate`],
/// [`on_demand::in_use`]); otherwise it loads when it's first needed
/// ([`on_demand`]). Best-effort — logs and moves on if it can't.
pub async fn autostart_if_configured(cfg: &crate::config::YapConfig) {
    // A save may have fixed what made the last start fail: on demand tries
    // again at once.
    on_demand::forget_failure();
    if on_demand::launch_gate(cfg.local_model_at_launch, on_demand::in_use(cfg), is_installed(cfg), is_running()) {
        tracing::info!("Loading the on-device model at launch (Load when Yap starts is on)");
        if let Err(e) = start().await {
            tracing::warn!("On-device cleanup sidecar failed to start: {}", e);
        }
        on_demand::changed();
    }
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
    let cfg = crate::shell::settings::change_config(&app, |cfg| {
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

#[cfg(test)]
mod tests {
    use super::context::DEFAULT_CONTEXT;
    use super::runtime::{upgrade_due, write_marker, Marker, REFUSE_AFTER};
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
    fn a_projector_is_never_listed_as_a_model() {
        assert!(is_model_file("Qwen_Qwen3.5-4B-Q4_K_M.gguf"));
        assert!(is_model_file("my-own.GGUF"));
        assert!(!is_model_file("mmproj-Qwen_Qwen3.5-4B-f16.gguf"));
        assert!(!is_model_file("MMPROJ-something.gguf"));
        assert!(!is_model_file("llamafile.exe"));
        assert!(!is_model_file("Qwen_Qwen3.5-4B-Q4_K_M.partial"));
    }

    #[test]
    fn the_projectors_weights_count_where_they_went() {
        let log = "I llama_kv_cache:      CUDA0 KV buffer size =   256.00 MiB\nI clip_ctx: CLIP using CUDA0 backend\nI load_hparams: model size:         641.26 MiB\nI reserve_compute_meta:      CUDA0 compute buffer size =   223.30 MiB";
        assert_eq!(log_facts(log).gpu_mib, Some(256 + 641 + 223));
        let cpu = "I llama_kv_cache:      CUDA0 KV buffer size =   256.00 MiB\nI clip_ctx: CLIP using CPU backend\nI load_hparams: model size:         641.26 MiB\nI reserve_compute_meta:        CPU compute buffer size =   223.30 MiB";
        let f = log_facts(cpu);
        assert_eq!((f.gpu_mib, f.host_mib), (Some(256), Some(641 + 223)));
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

    pub(super) const NEW: Version = Version(0, 10, 6);
    pub(super) const OLD: Version = Version(0, 10, 3);

    #[test]
    fn the_server_runs_without_thinking_and_with_its_caches_capped() {
        let args = server_args(Path::new("m.gguf"), 5000, 999, DEFAULT_CONTEXT, Plan { runtime: NEW, mtp: false, kv_q8: false, per_slot: false });
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
        let mtp = server_args(Path::new("m.gguf"), 5000, 999, DEFAULT_CONTEXT, Plan { runtime: NEW, mtp: true, kv_q8: false, per_slot: false });
        assert_eq!(&mtp[mtp.len() - 2..], ["--spec-type", "draft-mtp"]);
        let old = server_args(Path::new("m.gguf"), 5000, 999, DEFAULT_CONTEXT, Plan { runtime: OLD, mtp: true, kv_q8: false, per_slot: false });
        assert_eq!(old, args);
    }

    /// A folder of its own under the temp dir, emptied first.
    pub(super) fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("yap-runtime-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    pub(super) fn read(dir: &Path, file: &str) -> String {
        std::fs::read_to_string(dir.join(file)).unwrap_or_default()
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
    fn mtp_only_for_a_model_that_can_draft_on_a_card_with_room() {
        let dir = scratch("mtp");
        let qwen35 = dir.join("Qwen_Qwen3.5-4B-Q4_K_M.gguf");
        std::fs::write(&qwen35, crate::ai::gguf::tests::qwen35_4b(1_000).bytes()).unwrap();
        let qwen25 = dir.join(MODEL_FILENAME);
        std::fs::write(&qwen25, crate::ai::gguf::tests::qwen25_1_5b(1_000).bytes()).unwrap();
        // A 16 GB card: room for it (the stand-in file is small; the KV,
        // recurrent state and compute aren't).
        assert!(mtp_fits(&qwen35, 8_192, Some(16_303), false));
        // No prediction layer, no card of its own, or no room: off.
        assert!(!mtp_fits(&qwen25, 8_192, Some(16_303), false));
        assert!(!mtp_fits(&qwen35, 8_192, None, false));
        assert!(!mtp_fits(&qwen35, 8_192, Some(512), false), "integrated graphics");
        assert!(!mtp_fits(&qwen35, 8_192, Some(2_600), false), "no room beside Windows and speech");
        // At 256K the KV alone is 8 GiB: not on a 12 GB card with MTP.
        assert!(!mtp_fits(&qwen35, 262_144, Some(12_282), false));
        // The plan: only on 0.10.6, only with the setting on.
        let cfg = crate::config::YapConfig::default();
        assert!(cfg.local_mtp, "on by default");
        assert!(!plan_for(&cfg, &qwen35, 8_192, OLD).server().mtp);
        assert!(!plan_for(&crate::config::YapConfig { local_mtp: false, ..cfg.clone() }, &qwen35, 8_192, NEW).mtp);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_q8_kv_cache_only_where_it_makes_a_long_context_fit() {
        // Pure: 32K+, the f16 cache over the card's room and the q8_0 one in it.
        assert!(kv_q8_helps(65_536, 9_000, 8_200, Some(8_500)));
        assert!(!kv_q8_helps(65_536, 8_000, 7_300, Some(8_500)), "it fits anyway: f16 is faster");
        assert!(!kv_q8_helps(65_536, 12_000, 9_000, Some(8_500)), "it wouldn't fit either way");
        assert!(!kv_q8_helps(16_384, 9_000, 8_200, Some(8_500)), "a short context");
        assert!(!kv_q8_helps(65_536, 9_000, 8_200, None), "no card");
        // On a model file: Qwen3.5-4B's KV at 256K is 8 GiB (4.25 GiB as q8_0).
        let dir = scratch("kvq8");
        let qwen35 = dir.join("Qwen_Qwen3.5-4B-Q4_K_M.gguf");
        std::fs::write(&qwen35, crate::ai::gguf::tests::qwen35_4b(1_000).bytes()).unwrap();
        assert!(!kv_q8_rescues(&qwen35, 262_144, Some(16_303)), "fits a 16 GB card as f16");
        assert!(kv_q8_rescues(&qwen35, 262_144, Some(10_240)), "a 10 GB card: only as q8_0");
        assert!(!kv_q8_rescues(&qwen35, 262_144, Some(6_144)), "a 6 GB card: neither");
        assert!(!kv_q8_rescues(&qwen35, 8_192, Some(4_096)), "a short context never");
        assert!(!kv_q8_rescues(&qwen35, 262_144, None));
        // The arguments: both K and V (a matching pair), on the card only.
        let q8 = Plan { runtime: NEW, mtp: false, kv_q8: true, per_slot: false };
        let args = server_args(Path::new("m.gguf"), 5000, 999, 262_144, q8);
        assert_eq!(&args[args.len() - 4..], ["-ctk", "q8_0", "-ctv", "q8_0"]);
        assert!(!server_args(Path::new("m.gguf"), 5000, 0, 262_144, q8).iter().any(|a| a == "-ctk"), "never on the processor");
        assert!(!server_args(Path::new("m.gguf"), 5000, 999, 262_144, Plan { runtime: OLD, ..q8 }).iter().any(|a| a == "-ctk"));
        assert!(q8.server().kv_q8 && !Plan { runtime: OLD, ..q8 }.server().kv_q8);
        let _ = std::fs::remove_dir_all(&dir);
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
    fn on_the_processor_each_of_two_slots_has_a_cache_of_its_own() {
        // The decision: the processor, the runtime that has the flags.
        assert!(per_slot_caches(NEW, false));
        assert!(!per_slot_caches(NEW, true), "a graphics card keeps the shared cache");
        assert!(!per_slot_caches(OLD, false));
        // The arguments: two slots, no unified cache, the whole context each.
        let p = Plan { runtime: NEW, mtp: false, kv_q8: false, per_slot: true };
        let args = server_args(Path::new("m.gguf"), 5000, 0, 8_192, p);
        let after = |flag: &str| args.iter().position(|a| a == flag).map(|i| args[i + 1].as_str());
        assert_eq!(after("-c"), Some("16384"));
        assert_eq!(after("-np"), Some("2"));
        assert!(args.iter().any(|a| a == "--no-kv-unified"));
        assert_eq!(server_args(Path::new("m.gguf"), 5000, 0, 8_192, Plan { runtime: OLD, ..p }), base_args(Path::new("m.gguf"), 5000, 0, 8_192));
        // What a slot gets is the context the server reports: no restart loop
        // over "16384 isn't 8192".
        let log = "0.00.419.239 I llama_context: n_ctx                 = 16384\n0.00.419.239 I llama_context: n_ctx_seq             = 8192\n0.00.419.242 I llama_context: n_ctx_seq (8192) < n_ctx_train (32768) -- the full capacity of the model will not be utilized\n";
        assert_eq!(log_facts(log).n_ctx, Some(8_192));
        assert!(!context::needs_restart(log_facts(log).n_ctx, 8_192));
        // The estimate: the KV twice, the recurrent state for two slots.
        let shape = crate::ai::gguf::tests::qwen25_1_5b(151_936);
        let s = crate::ai::gguf::read_shape(std::io::Cursor::new(shape.bytes())).unwrap();
        let one = crate::ai::gguf::estimate(&s, 1_000_000_000, 8_192, false, Plan { per_slot: false, ..p }.server());
        let two = crate::ai::gguf::estimate(&s, 1_000_000_000, 8_192, false, p.server());
        assert_eq!(two.kv_mib, 2 * one.kv_mib);
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

}

