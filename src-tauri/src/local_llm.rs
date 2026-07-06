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

/// Model name advertised to the OpenAI-compatible endpoint. llamafile serves the
/// single GGUF it was launched with and is lenient about this value.
pub const LOCAL_MODEL: &str = "qwen2.5-1.5b-instruct";

/// The GGUF the sidecar loads (downloaded on demand). Qwen2.5-1.5B-Instruct
/// Q4_K_M — Apache-2.0, ~1.04 GB, strong instruction-following for its size.
pub const MODEL_FILENAME: &str = "qwen2.5-1.5b-instruct-q4_k_m.gguf";

/// Human-readable names surfaced in the Settings UI (via `local_llm_status`) so
/// the user is told exactly what gets downloaded and what runs on their machine.
pub const MODEL_DISPLAY: &str = "Qwen2.5 1.5B Instruct";
pub const ENGINE_DISPLAY: &str = "Mozilla llamafile (llama.cpp)";

/// Download source + pinned SHA-256 for the GGUF cleanup model (HuggingFace).
const MODEL_URL: &str =
    "https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct-GGUF/resolve/main/qwen2.5-1.5b-instruct-q4_k_m.gguf";
const MODEL_SHA256: &str = "6a1a2eb6d15622bf3c96857206351ba97e1af16c30d7a74ee38970e434e9407e";

/// Download source + pinned SHA-256 for the llamafile runtime (v0.10.3, full
/// build — bundles the GPU backends so it works on a clean machine with no dev
/// tools; CPU fallback guaranteed). It's an Actually-Portable-Executable that
/// runs on Windows when saved with a `.exe` name.
const RUNTIME_URL: &str =
    "https://github.com/Mozilla-Ocho/llamafile/releases/download/0.10.3/llamafile-0.10.3";
const RUNTIME_SHA256: &str = "e6d4041a82ca37cee15aab62e6826d7a61c6a3ea83bca68387958970df250883";

/// The llamafile runtime executable name on disk.
#[cfg(windows)]
pub const RUNTIME_FILENAME: &str = "llamafile.exe";
#[cfg(not(windows))]
pub const RUNTIME_FILENAME: &str = "llamafile";

/// Cold model-load can take a while on first start; cap the health wait.
const HEALTH_TIMEOUT_SECS: u64 = 120;

static CHILD: Mutex<Option<Child>> = Mutex::new(None);
static PORT: AtomicU16 = AtomicU16::new(0);
static READY: AtomicBool = AtomicBool::new(false);

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
            return (
                url,
                String::new(),
                LOCAL_MODEL.to_string(),
                PROVIDER_ONDEVICE.to_string(),
            );
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
    // `--server` = OpenAI-compatible HTTP API only (no browser UI, no --nobrowser
    // flag needed — and that flag is invalid in llamafile 0.10.3).
    cmd.arg("--server")
        .arg("--host")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(port.to_string())
        .arg("-m")
        .arg(&model)
        .arg("-ngl")
        .arg("999") // offload all layers to GPU; auto-falls back to CPU
        .arg("-c")
        .arg("8192") // room for the structured system prompt + one-shots (~2.1k
        // tokens) plus a long dictation and its cleaned output
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
                READY.store(true, Ordering::SeqCst);
                tracing::info!(port, "On-device cleanup sidecar ready");
                return Ok(format!("http://127.0.0.1:{}/v1", port));
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    stop();
    Err("llamafile did not become ready in time".to_string())
}

/// Kill any orphaned llamafile processes left over from a previous session.
/// The Tauri updater (and crashes / task-kill) can force-exit Yap WITHOUT running
/// the `RunEvent::Exit` handler, so `stop()` never fires and the sidecar survives.
/// Called at startup before we spawn a fresh one, so at most one ever runs.
#[cfg(windows)]
pub fn kill_orphans() {
    use std::os::windows::process::CommandExt;
    // taskkill by image name — llamafile is Yap-specific in practice, and this
    // only runs at our startup (any running instance is a leftover we own).
    let _ = std::process::Command::new("taskkill")
        .args(["/F", "/IM", RUNTIME_FILENAME])
        .creation_flags(0x0800_0000)
        .output();
}
#[cfg(not(windows))]
pub fn kill_orphans() {}

/// Stop the sidecar (kill + reap the child). Safe to call when not running.
pub fn stop() {
    READY.store(false, Ordering::SeqCst);
    PORT.store(0, Ordering::SeqCst);
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
        // A Language-Models scope (Voice Agent, etc.) pointed at the local model.
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
    /// Marked as the suggested default in the Settings model browser.
    pub recommended: bool,
    /// Model family for the browser's provider tabs + brand icon
    /// ("qwen"|"llama"|"gemma"|"mistral"|"phi").
    pub family: &'static str,
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
        recommended: false,
        family: "qwen",
    },
    CuratedLlm {
        id: "qwen2.5-1.5b",
        display: MODEL_DISPLAY,
        blurb: "Fast and accurate — the recommended default",
        filename: MODEL_FILENAME,
        url: MODEL_URL,
        sha256: MODEL_SHA256,
        size_mb: 1043,
        recommended: true,
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
        recommended: false,
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
        recommended: false,
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
        recommended: false,
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
        recommended: false,
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
        recommended: false,
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
        recommended: false,
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
        recommended: false,
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
        recommended: false,
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
        recommended: false,
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
async fn download_verified(
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

