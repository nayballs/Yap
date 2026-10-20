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
use std::process::Child;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::Mutex;
use std::time::Duration;

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

/// Download source + pinned SHA-256 for the llamafile runtime (v0.10.3, full
/// build — bundles the GPU backends so it works on a clean machine with no dev
/// tools; CPU fallback guaranteed). It's an Actually-Portable-Executable that
/// runs on Windows when saved with a `.exe` name.
pub(crate) const RUNTIME_URL: &str =
    "https://github.com/Mozilla-Ocho/llamafile/releases/download/0.10.3/llamafile-0.10.3";
pub(crate) const RUNTIME_SHA256: &str = "e6d4041a82ca37cee15aab62e6826d7a61c6a3ea83bca68387958970df250883";

/// The llamafile runtime executable name on disk.
#[cfg(windows)]
pub const RUNTIME_FILENAME: &str = "llamafile.exe";
#[cfg(not(windows))]
pub const RUNTIME_FILENAME: &str = "llamafile";

/// Cold model-load can take a while on first start; cap the health wait.
const HEALTH_TIMEOUT_SECS: u64 = 120;

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
    crate::gguf::model_file(path)
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
pub fn server_args(model: &Path, port: u16, ngl: u32, ctx: u32) -> Vec<String> {
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
/// The running sidecar's model id (its GGUF's name) and where its layers
/// went; set once it's ready.
static SERVING: Mutex<Option<Serving>> = Mutex::new(None);
/// One start at a time (a restart, a Chat waking the model, a Settings
/// switch): a second waits, then finds it up.
static STARTING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Debug, Clone)]
struct Serving {
    model_id: String,
    placement: Option<Placement>,
    /// The `-c` it was started with.
    ctx: u32,
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
    match CHILD.lock() {
        Ok(mut guard) => match guard.as_mut() {
            Some(child) => matches!(child.try_wait(), Ok(Some(_))),
            None => true,
        },
        Err(_) => true,
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
    let exe = runtime_path();
    // Load whichever GGUF the user picked (custom models live in the same dir);
    // falls back to the bundled default if the selection is gone.
    let cfg = crate::config::load();
    let model = active_model_path(&cfg);
    // The context set in the pickers, within what the model was trained for.
    let ctx = context_for(&cfg);
    if !exe.is_file() {
        return Err(format!("llamafile runtime not installed: {}", exe.display()));
    }
    if !model.is_file() {
        return Err(format!("cleanup model not installed: {}", model.display()));
    }

    let port = find_free_port().ok_or_else(|| "no free localhost port".to_string())?;

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
    cmd.args(server_args(&model, port, 999, ctx))
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
        .map_err(|e| format!("failed to launch llamafile: {}", e))?;
    // The model never keeps running after Yap: a crash or End task ends it too.
    crate::procs::end_with_yap(&child);
    *CHILD.lock().unwrap() = Some(child);
    PORT.store(port, Ordering::SeqCst);
    READY.store(false, Ordering::SeqCst);
    tracing::info!(port, ctx, model = %model.display(), "Starting on-device cleanup sidecar");

    // Poll /health until the model has loaded (or the process dies / we time out).
    let health = format!("http://127.0.0.1:{}/health", port);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .map_err(|e| e.to_string())?;

    for _ in 0..HEALTH_TIMEOUT_SECS {
        if child_exited() {
            stop();
            return Err("llamafile exited during startup".to_string());
        }
        if let Ok(resp) = client.get(&health).send().await {
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
                if let Ok(mut s) = SERVING.lock() {
                    *s = Some(Serving { model_id: model_id(&model), placement, ctx, facts });
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
                return Ok(format!("http://127.0.0.1:{}/v1", port));
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    stop();
    Err("llamafile did not become ready in time".to_string())
}

/// The sidecar's URL: as it is when it's up, after the start already under
/// way (never a second process), else after starting it. For callers that
/// need the model now (the AI Chat on the on-device model).
pub async fn ensure_started() -> Result<String, String> {
    if let Some(url) = base_url() {
        return Ok(url);
    }
    if PORT.load(Ordering::SeqCst) != 0 && !child_exited() {
        for _ in 0..HEALTH_TIMEOUT_SECS {
            tokio::time::sleep(Duration::from_secs(1)).await;
            if let Some(url) = base_url() {
                return Ok(url);
            }
            if PORT.load(Ordering::SeqCst) == 0 {
                break;
            }
        }
        return base_url().ok_or_else(|| "llamafile did not become ready in time".to_string());
    }
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
pub fn stop() {
    READY.store(false, Ordering::SeqCst);
    PORT.store(0, Ordering::SeqCst);
    if let Ok(mut s) = SERVING.lock() {
        *s = None;
    }
    if let Ok(mut guard) = CHILD.lock() {
        if let Some(mut child) = guard.take() {
            let _ = child.kill();
            let _ = child.wait();
            tracing::info!("On-device cleanup sidecar stopped");
        }
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
    if crate::pipeline::dictation_state().1 {
        return true;
    }
    app.try_state::<crate::AppState>()
        .and_then(|s| s.pipeline.lock().ok().map(|g| g.as_ref().is_some_and(|p| p.is_busy())))
        .unwrap_or(false)
}

/// Restart the sidecar on the context the config now asks for, once no
/// dictation is going on (never under one: its cleanup would lose the
/// model mid-answer). The model switch's path: stop, start. Emits
/// `yap-local-llm-changed` when done (the pickers re-read), and
/// `yap-chat-model-changed {scope: null}` so the chips do.
pub fn restart_when_idle(app: AppHandle) {
    let gen = RESTART_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    RESTART_PENDING.store(true, Ordering::SeqCst);
    let _ = app.emit("yap-local-llm-changed", serde_json::json!({ "pending": true }));
    tauri::async_runtime::spawn(async move {
        let latest = || RESTART_GEN.load(Ordering::SeqCst) == gen;
        if pipeline_busy(&app) {
            tracing::info!("On-device model: the new context waits for the dictation to finish");
        }
        if !wait_until_idle(|| pipeline_busy(&app), latest, RESTART_TICK, RESTART_SETTLE).await {
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
    stop();
    start().await.map(|_| ())
}

/// One context the pickers offer, with what it takes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextOption {
    pub tokens: u32,
    /// ≈ MiB the model takes at it ([`crate::gguf::estimate`]); `None`
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
    let file = crate::gguf::model_file(&path);
    let max = trained_context(&path);
    let tokens = effective_context(cfg.local_context, max);
    let running = shown_running();
    let placement = running.then(placement).flatten();
    let facts = running.then(serving_facts).flatten();
    let gpu = crate::gpu::this_pc();
    let vram = gpu.as_ref().map(|g| g.vram_mib);
    let on_card = runs_on_card(placement, vram);
    let room = on_card.then(|| card_room(vram)).flatten();
    let options = context_choices(max, tokens)
        .into_iter()
        .map(|c| {
            let estimate_mib = file
                .as_ref()
                .map(|f| crate::gguf::estimate(&f.shape, f.bytes, c as u64, on_card).total_mib);
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
pub fn local_llm_set_context(
    app: AppHandle,
    state: tauri::State<'_, crate::AppState>,
    tokens: u32,
) -> Result<ContextView, String> {
    let setting = context_setting(tokens)?;
    let mut cfg = crate::config::load();
    let before = context_for(&cfg);
    cfg.local_context = setting;
    crate::commands::save_config(app.clone(), state, cfg.clone())?;
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

/// Ensure the llamafile runtime is on disk (SHA-verified download if missing).
/// `model`: the curated id the install is for (its progress names it).
async fn ensure_runtime(model: &str, app: Option<&AppHandle>) -> Result<(), String> {
    tokio::fs::create_dir_all(llm_dir())
        .await
        .map_err(|e| format!("failed to create llm dir: {}", e))?;

    if !runtime_path().is_file() {
        let (url, sha) = source("runtime", RUNTIME_URL, RUNTIME_SHA256);
        download_for(&url, &runtime_path(), &sha, "runtime", model, app).await?;
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
/// download can never be used).
pub(crate) async fn download_verified(
    url: &str,
    dest: &Path,
    expected_sha: &str,
    stage: &'static str,
    app: Option<&AppHandle>,
) -> Result<(), String> {
    download_for(url, dest, expected_sha, stage, "", app).await
}

/// [`download_verified`], its progress naming the curated model `model`.
async fn download_for(
    url: &str,
    dest: &Path,
    expected_sha: &str,
    stage: &'static str,
    model: &str,
    app: Option<&AppHandle>,
) -> Result<(), String> {
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

    if crate::e2e::active() && !allowed_in_test_run(url) {
        return Err("Test runs never download models from the internet".to_string());
    }
    let partial = dest.with_extension("partial");
    tracing::info!(url, dest = %dest.display(), stage, "Downloading on-device cleanup asset");

    let client = reqwest::Client::new();
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("request failed: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {} from {}", resp.status(), url));
    }
    let total = resp.content_length();

    let mut file = tokio::fs::File::create(&partial)
        .await
        .map_err(|e| format!("create temp file: {}", e))?;
    let mut downloaded: u64 = 0;
    let mut last_pct: u8 = 0;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("download stream error: {}", e))?;
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
    let verify_path = partial.clone();
    let expected = expected_sha.to_string();
    let ok = tokio::task::spawn_blocking(move || match compute_sha256(&verify_path) {
        Ok(actual) => actual == expected,
        Err(_) => false,
    })
    .await
    .map_err(|e| format!("sha task panicked: {}", e))?;
    if !ok {
        let _ = tokio::fs::remove_file(&partial).await;
        return Err(format!("{} verification failed (corrupt download) — retry", stage));
    }

    tokio::fs::rename(&partial, dest)
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

    #[test]
    fn the_server_runs_without_thinking_and_with_its_caches_capped() {
        let args = server_args(Path::new("m.gguf"), 5000, 999, DEFAULT_CONTEXT);
        let after = |flag: &str| args.iter().position(|a| a == flag).map(|i| args[i + 1].as_str());
        assert_eq!(after("--reasoning"), Some("off"));
        assert_eq!(after("--cache-ram"), Some("0"));
        assert_eq!(after("--ctx-checkpoints"), Some("4"));
        assert_eq!(after("-ngl"), Some("999"));
        assert_eq!(after("-c"), Some("8192"));
        assert_eq!(after("-lv"), Some("4"));
        assert_eq!(after("--port"), Some("5000"));
        assert!(args.iter().any(|a| a == "--server"));
    }

    #[test]
    fn the_model_id_is_the_files_name() {
        assert_eq!(model_id(Path::new("C:/x/llm/Qwen_Qwen3.5-4B-Q4_K_M.gguf")), "Qwen_Qwen3.5-4B-Q4_K_M");
        assert_eq!(model_id(&model_path()), "qwen2.5-1.5b-instruct-q4_k_m");
        // The default's id gets no tools; the 4B's does.
        assert!(!crate::tools::supports_tools(PROVIDER_ONDEVICE, &model_id(&model_path())));
        assert!(crate::tools::supports_tools(PROVIDER_ONDEVICE, "Qwen_Qwen3.5-4B-Q4_K_M"));
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
        std::fs::write(&p, crate::gguf::tests::qwen25_1_5b(10).bytes()).unwrap();
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

