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
        .arg("2048") // small context — cleanup inputs are short
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

/// Progress event payload for the on-device install (`local-llm-download-progress`).
#[derive(serde::Serialize, Clone)]
struct DownloadProgress {
    /// "runtime" (the llamafile engine) or "model" (the GGUF).
    stage: &'static str,
    percent: u8,
    downloaded_mb: f64,
    total_mb: f64,
}

/// Download the runtime + model (whichever are missing), each SHA-verified, into
/// `<data_dir>/llm/`. Idempotent — already-present files are skipped. Emits
/// `local-llm-download-progress` so the UI can show a bar per stage.
pub async fn install(app: Option<&AppHandle>) -> Result<(), String> {
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
    if !model_path().is_file() {
        download_verified(MODEL_URL, &model_path(), MODEL_SHA256, "model", app).await?;
    }
    Ok(())
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

