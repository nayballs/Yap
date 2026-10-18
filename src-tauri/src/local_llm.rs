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

/// The context the sidecar serves (shared by its slots): room for the
/// structured cleanup prompt + one-shots (~2.1k tokens) plus a long
/// dictation, and for a Chat turn's tools, notes and history.
pub const CONTEXT_TOKENS: u32 = 8192;

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

static CHILD: Mutex<Option<Child>> = Mutex::new(None);
static PORT: AtomicU16 = AtomicU16::new(0);
static READY: AtomicBool = AtomicBool::new(false);
/// The running sidecar's model id (its GGUF's name) and where its layers
/// went; set once it's ready.
static SERVING: Mutex<Option<Serving>> = Mutex::new(None);

#[derive(Debug, Clone)]
struct Serving {
    model_id: String,
    placement: Option<Placement>,
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

/// Where the running sidecar's model went (GPU, partly, the processor);
/// `None` while it isn't running or the log didn't say.
pub fn placement() -> Option<Placement> {
    #[cfg(debug_assertions)]
    if let Some((_, p)) = e2e_serving() {
        return Some(p);
    }
    SERVING.lock().ok()?.as_ref().and_then(|s| s.placement)
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
    if let Some((file, _)) = e2e_serving() {
        return file;
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

#[cfg(debug_assertions)]
static E2E_SERVING: Mutex<Option<(String, Placement)>> = Mutex::new(None);

#[cfg(debug_assertions)]
fn e2e_serving() -> Option<(String, Placement)> {
    if !crate::e2e::active() {
        return None;
    }
    E2E_SERVING.lock().ok()?.clone()
}

/// Test mode only: Settings shows the sidecar as running `file` with
/// `gpu_layers` of `layers` on the graphics card (no process: nothing is
/// routed to it); `file` absent = as it really is. For the e2e suite's
/// "Runs on the processor" line.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_local_llm_serving(file: Option<String>, gpu_layers: Option<u32>, layers: Option<u32>) -> Result<(), String> {
    if !crate::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    *E2E_SERVING.lock().unwrap_or_else(|p| p.into_inner()) = file.map(|f| {
        (f, Placement { gpu_layers: gpu_layers.unwrap_or(0), layers: layers.unwrap_or(33) })
    });
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
/// default, else the custom GGUF's filename without extension.
pub fn active_model_display(cfg: &crate::config::YapConfig) -> String {
    let path = active_model_path(cfg);
    if path == model_path() {
        return MODEL_DISPLAY.to_string();
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
    let exe = runtime_path();
    // Load whichever GGUF the user picked (custom models live in the same dir);
    // falls back to the bundled default if the selection is gone.
    let model = active_model_path(&crate::config::load());
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
    cmd.args(server_args(&model, port, 999, CONTEXT_TOKENS))
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
    tracing::info!(port, model = %model.display(), "Starting on-device cleanup sidecar");

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
                for _ in 0..10 {
                    let log = std::fs::read_to_string(llm_dir().join("llamafile.log")).unwrap_or_default();
                    placement = placement_after_load(&log);
                    if placement.is_some() {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(300)).await;
                }
                if let Ok(mut s) = SERVING.lock() {
                    *s = Some(Serving { model_id: model_id(&model), placement });
                }
                READY.store(true, Ordering::SeqCst);
                tracing::info!(
                    port,
                    model = %model_id(&model),
                    on = placement.map(|p| p.kind()).unwrap_or("?"),
                    gpu_layers = placement.map(|p| p.gpu_layers).unwrap_or(0),
                    layers = placement.map(|p| p.layers).unwrap_or(0),
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

/// Progress event payload for the on-device install (`local-llm-download-progress`).
#[derive(serde::Serialize, Clone)]
struct DownloadProgress {
    /// "runtime" (the llamafile engine) or "model" (the GGUF).
    stage: &'static str,
    percent: u8,
    downloaded_mb: f64,
    total_mb: f64,
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
    },
    CuratedLlm {
        id: GPU_MODEL_ID,
        display: "Qwen3.5 4B",
        blurb: "Uses Chat's tools well: your notes, apps and the web. Wants a graphics card",
        filename: "Qwen_Qwen3.5-4B-Q4_K_M.gguf",
        // bartowski's Q4_K_M of Qwen/Qwen3.5-4B (Apache-2.0, ungated); the
        // SHA-256 is its LFS oid, and the file the evaluations ran.
        url: "https://huggingface.co/bartowski/Qwen_Qwen3.5-4B-GGUF/resolve/main/Qwen_Qwen3.5-4B-Q4_K_M.gguf",
        sha256: "13c16f426047e2de38cd075bdade4a7bcbc8c774384876f677740cda65f8a983",
        size_mb: 2873,
        default: false,
        needs_gpu: true,
        family: "qwen",
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
    },
];

pub fn curated_by_id(id: &str) -> Option<&'static CuratedLlm> {
    CURATED_MODELS.iter().find(|m| m.id == id)
}

/// Ensure the llamafile runtime is on disk (SHA-verified download if missing).
async fn ensure_runtime(app: Option<&AppHandle>) -> Result<(), String> {
    tokio::fs::create_dir_all(llm_dir())
        .await
        .map_err(|e| format!("failed to create llm dir: {}", e))?;

    if !runtime_path().is_file() {
        download_verified(RUNTIME_URL, &runtime_path(), RUNTIME_SHA256, "runtime", app).await?;
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
    ensure_runtime(app).await?;
    if !model_path().is_file() {
        download_verified(MODEL_URL, &model_path(), MODEL_SHA256, "model", app).await?;
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
    ensure_runtime(app).await?;
    let dest = llm_dir().join(m.filename);
    if !dest.is_file() {
        download_verified(m.url, &dest, m.sha256, "model", app).await?;
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
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

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
    fn the_server_runs_without_thinking_and_with_its_caches_capped() {
        let args = server_args(Path::new("m.gguf"), 5000, 999, CONTEXT_TOKENS);
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
}

