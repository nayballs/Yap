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

use std::path::PathBuf;
use std::process::Child;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::Mutex;
use std::time::Duration;

/// Provider id (in `YapConfig::pp_provider`) that selects the managed sidecar.
pub const PROVIDER_ONDEVICE: &str = "ondevice";

/// Model name advertised to the OpenAI-compatible endpoint. llamafile serves the
/// single GGUF it was launched with and is lenient about this value.
pub const LOCAL_MODEL: &str = "qwen2.5-1.5b-instruct";

/// The GGUF the sidecar loads (downloaded on demand). Qwen2.5-1.5B-Instruct
/// Q4_K_M — Apache-2.0, ~1.1 GB, strong instruction-following for its size.
pub const MODEL_FILENAME: &str = "qwen2.5-1.5b-instruct-q4_k_m.gguf";

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

/// Where the sidecar's files live: `<data_dir>/llm/`.
fn llm_dir() -> PathBuf {
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

/// Both the runtime and the model are present on disk.
pub fn is_installed() -> bool {
    runtime_path().is_file() && model_path().is_file()
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

/// The `(base_url, api_key, model)` the cleanup client should use: the managed
/// on-device sidecar when the provider is "ondevice" AND it's up, else the user's
/// configured cloud/local endpoint. Falling back keeps cleanup working even if the
/// sidecar failed to start (worst case: raw transcript, never a hang).
pub fn effective_endpoint(cfg: &crate::config::YapConfig) -> (String, String, String) {
    if cfg.pp_provider == PROVIDER_ONDEVICE {
        if let Some(url) = base_url() {
            return (url, String::new(), LOCAL_MODEL.to_string());
        }
    }
    (
        cfg.pp_base_url.clone(),
        cfg.pp_api_key.clone(),
        cfg.pp_model.clone(),
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
    let model = model_path();
    if !exe.is_file() {
        return Err(format!("llamafile runtime not installed: {}", exe.display()));
    }
    if !model.is_file() {
        return Err(format!("cleanup model not installed: {}", model.display()));
    }

    let port = find_free_port().ok_or_else(|| "no free localhost port".to_string())?;

    let mut cmd = std::process::Command::new(&exe);
    cmd.arg("--server")
        .arg("--nobrowser")
        .arg("--host")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(port.to_string())
        .arg("-m")
        .arg(&model)
        .arg("-ngl")
        .arg("999") // offload all layers to GPU; llamafile falls back to CPU
        .arg("-c")
        .arg("2048") // small context — cleanup inputs are short
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
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

/// If the user has selected on-device cleanup and the files are installed, start
/// the sidecar (best-effort — logs and moves on if it can't). Called at startup.
pub async fn autostart_if_configured(cfg: &crate::config::YapConfig) {
    if cfg.post_process_enabled
        && cfg.pp_provider == PROVIDER_ONDEVICE
        && is_installed()
        && !is_running()
    {
        if let Err(e) = start().await {
            tracing::warn!("On-device cleanup sidecar failed to start: {}", e);
        }
    }
}

