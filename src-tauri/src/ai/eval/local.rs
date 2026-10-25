//! Local models for a run: Yap's own llamafile runtime serving a GGUF on a
//! free localhost port, started for the run and stopped after (`Drop`).
//!
//! Files come from the installed Yap's `%APPDATA%\yap\llm\` when they're
//! there, **read-only** (run in place, nothing written next to them: the
//! server's log and working folder are the run's), else they're downloaded
//! as `local_llm.rs` does (SHA-256 verified) into the models folder
//! (`--models-dir`, by default a folder like `<builds>/eval-models`: next to the
//! runs).

use std::path::{Path, PathBuf};
use std::process::Child;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

/// The installed app's model folder (never the run's data dir).
pub fn installed_llm_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("yap").join("llm"))
}

/// The llamafile runtime: `--llamafile`, else the installed one (read-only)
/// when it's the version Yap pins now (its marker says so), else the pinned
/// one downloaded into `models_dir` (verified, with its marker).
pub async fn runtime(explicit: Option<&Path>, models_dir: &Path) -> Result<PathBuf, String> {
    if let Some(p) = explicit {
        return if p.is_file() { Ok(p.to_path_buf()) } else { Err(format!("no llamafile at {}", p.display())) };
    }
    let pinned = crate::ai::local_llm::runtime::Version::parse(crate::ai::local_llm::RUNTIME_VERSION);
    let current = |dir: &Path| crate::ai::local_llm::runtime::runtime_version_in(dir).filter(|v| Some(*v) == pinned).is_some();
    if let Some(dir) = installed_llm_dir().filter(|d| current(d)) {
        return Ok(dir.join(crate::ai::local_llm::RUNTIME_FILENAME));
    }
    let dest = models_dir.join(crate::ai::local_llm::RUNTIME_FILENAME);
    if !current(models_dir) {
        std::fs::create_dir_all(models_dir).map_err(|e| e.to_string())?;
        println!("Downloading the llamafile runtime {} into {} …", crate::ai::local_llm::RUNTIME_VERSION, models_dir.display());
        let _ = std::fs::remove_file(&dest);
        crate::ai::local_llm::models::download_verified(
            crate::ai::local_llm::RUNTIME_URL,
            &dest,
            crate::ai::local_llm::RUNTIME_SHA256,
            "runtime",
            None,
        )
        .await?;
        crate::ai::local_llm::runtime::write_marker(
            models_dir,
            &crate::ai::local_llm::runtime::Marker {
                version: crate::ai::local_llm::RUNTIME_VERSION.to_string(),
                sha256: crate::ai::local_llm::RUNTIME_SHA256.to_string(),
                ..Default::default()
            },
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(dest)
}

/// The GGUF a `local:<spec>` names: "installed" (the on-device default,
/// Qwen2.5-1.5B), a curated id from `local_llm::models::CURATED_MODELS`
/// ("qwen2.5-3b"), a path, or an https URL with `#sha256=<hex>`. Installed
/// files are used where they are; anything else is downloaded (verified)
/// into `models_dir`.
pub async fn model(spec: &str, models_dir: &Path) -> Result<PathBuf, String> {
    let spec = spec.trim();
    let installed = |file: &str| installed_llm_dir().map(|d| d.join(file)).filter(|p| p.is_file());
    let fetch = |url: String, file: String, sha: String| async move {
        let dest = models_dir.join(&file);
        if !dest.is_file() {
            std::fs::create_dir_all(models_dir).map_err(|e| e.to_string())?;
            println!("Downloading {file} into {} …", models_dir.display());
            crate::ai::local_llm::models::download_verified(&url, &dest, &sha, "model", None).await?;
        }
        Ok::<PathBuf, String>(dest)
    };
    if spec.is_empty() || spec == "installed" || spec == "default" {
        let file = crate::ai::local_llm::MODEL_FILENAME;
        if let Some(p) = installed(file) {
            return Ok(p);
        }
        return fetch(crate::ai::local_llm::MODEL_URL.into(), file.into(), crate::ai::local_llm::MODEL_SHA256.into()).await;
    }
    if let Some(m) = crate::ai::local_llm::models::curated_by_id(spec) {
        if let Some(p) = installed(m.filename) {
            return Ok(p);
        }
        return fetch(m.url.into(), m.filename.into(), m.sha256.into()).await;
    }
    if spec.starts_with("https://") {
        let (url, sha) = spec.split_once("#sha256=").ok_or("a model URL needs #sha256=<hex> (its Hugging Face LFS oid)")?;
        let file = url.rsplit('/').next().unwrap_or("model.gguf").to_string();
        return fetch(url.into(), file, sha.trim().to_lowercase()).await;
    }
    let p = PathBuf::from(spec);
    if p.is_file() {
        return Ok(p);
    }
    if let Some(p) = installed(spec).or_else(|| Some(models_dir.join(spec)).filter(|p| p.is_file())) {
        return Ok(p);
    }
    Err(format!(
        "no GGUF \"{spec}\": give a path, \"installed\", a curated id ({}), or an https URL#sha256=…",
        crate::ai::local_llm::models::CURATED_MODELS.iter().map(|m| m.id).collect::<Vec<_>>().join(", ")
    ))
}

/// A running llamafile server.
pub struct Server {
    child: Option<Child>,
    pub pid: u32,
    pub base_url: String,
    pub log: PathBuf,
    pub load_ms: u64,
    pub args: Vec<String>,
    pub plan: crate::ai::local_llm::Plan,
    /// The warm-up Yap's sidecar sends once loaded (`local_llm::warm_up`),
    /// unless the run said `--no-warm-up`.
    pub warm_up: Option<crate::ai::local_llm::WarmUp>,
}

/// How a run's server runs the model (`local_llm::Plan`): the runtime's
/// version (`--version`, else the one Yap pins), and MTP as Yap's sidecar
/// would decide on this PC — `mtp` overrides; never on the processor
/// (`ngl` 0).
pub async fn plan(runtime: &Path, model: &Path, ngl: u32, ctx: u32, mtp: Option<bool>) -> crate::ai::local_llm::Plan {
    let version = crate::ai::local_llm::runtime::probe_version(runtime)
        .await
        .or_else(|| crate::ai::local_llm::runtime::Version::parse(crate::ai::local_llm::RUNTIME_VERSION))
        .expect("a version");
    let yap = crate::ai::local_llm::plan_for(&crate::config::load(), model, ctx, version);
    let on = ngl > 0 && mtp.unwrap_or(yap.mtp);
    crate::ai::local_llm::Plan { runtime: version, mtp: on, kv_q8: ngl > 0 && yap.kv_q8, per_slot: ngl == 0 && version.is_current() }
}

/// Start `runtime` serving `model` with `local_llm::server_args` — as Yap's
/// sidecar runs (thinking off, the caches capped, `-lv 4`, MTP as `plan`
/// says) — but with `ngl` GPU layers (0 = CPU only), the run's folder as its
/// working dir and log, and `extra` arguments after Yap's (a later flag wins:
/// `--reasoning auto`, `--cache-ram 8192` measure the old way). Waits for
/// `/health`, then warms the model up as the sidecar does (`warm`).
#[allow(clippy::too_many_arguments)]
pub async fn start(
    runtime: &Path,
    model: &Path,
    ngl: u32,
    ctx: u32,
    plan: crate::ai::local_llm::Plan,
    extra: &[String],
    workdir: &Path,
    warm: bool,
) -> Result<Server, String> {
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .map_err(|e| format!("no free port: {e}"))?;
    let stem = model.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let log = workdir.join(format!("llamafile-{stem}.log"));
    let file = std::fs::File::create(&log).map_err(|e| e.to_string())?;
    let mut args = crate::ai::local_llm::server_args(model, port, ngl, ctx, plan);
    args.extend(extra.iter().cloned());
    let mut cmd = std::process::Command::new(runtime);
    cmd.args(&args)
        .current_dir(workdir)
        .stdin(std::process::Stdio::null())
        .stdout(file.try_clone().map_err(|e| e.to_string())?)
        .stderr(file);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let started = Instant::now();
    let mut child = cmd.spawn().map_err(|e| format!("couldn't start llamafile: {e}"))?;
    // Ended with this process however it ends (a run stopped mid-way, End task).
    crate::shell::procs::end_with_yap(&child);
    let pid = child.id();
    let base_url = format!("http://127.0.0.1:{port}/v1");
    let client = reqwest::Client::builder().timeout(Duration::from_secs(3)).build().map_err(|e| e.to_string())?;
    let health = format!("http://127.0.0.1:{port}/health");
    for _ in 0..600 {
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!("llamafile exited during startup ({status}); see {}", log.display()));
        }
        if client.get(&health).send().await.is_ok_and(|r| r.status().is_success()) {
            let load_ms = started.elapsed().as_millis() as u64;
            let warm_up = if warm { Some(crate::ai::local_llm::warm_up(&base_url, &stem).await) } else { None };
            return Ok(Server { child: Some(child), pid, base_url, log, load_ms, args, plan, warm_up });
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let _ = child.kill();
    let _ = child.wait();
    Err(format!("llamafile wasn't ready after 5 minutes; see {}", log.display()))
}

impl Server {
    /// The server's peak memory (working set), MB — Windows only.
    pub fn peak_ram_mb(&self) -> Option<u64> {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let out = std::process::Command::new("powershell")
                .args(["-NoProfile", "-Command", &format!("(Get-Process -Id {}).PeakWorkingSet64", self.pid)])
                .creation_flags(0x0800_0000)
                .output()
                .ok()?;
            let bytes: u64 = String::from_utf8_lossy(&out.stdout).trim().parse().ok()?;
            Some(bytes / 1_048_576)
        }
        #[cfg(not(windows))]
        None
    }

    /// What the server's log says about memory and the CPU: llama.cpp's
    /// projected host memory (the model + its context), the largest its
    /// prompt cache grew (idle prompts kept in RAM, up to `--cache-ram`,
    /// 8 GB by default), and its system line.
    pub fn log_facts(&self) -> Value {
        let text = std::fs::read_to_string(&self.log).unwrap_or_default();
        let mut projected: Option<f64> = None;
        let mut cache: f64 = 0.0;
        let mut gpu_mib: f64 = 0.0;
        let mut system = String::new();
        let number_before = |s: &str, unit: &str| -> Option<f64> {
            let end = s.find(unit)?;
            s[..end].trim().rsplit([' ', ',']).next()?.parse().ok()
        };
        for line in text.lines() {
            // "common_params_fit_impl: projected to use 4102 MiB of host memory"
            if let Some(rest) = line.split("projected to use").nth(1) {
                projected = projected.or_else(|| number_before(rest, "MiB"));
            }
            // "cache state: 9 prompts, 2730.745 MiB (limits: …)"
            if let Some(rest) = line.split("cache state:").nth(1) {
                if let Some(v) = number_before(rest, "MiB") {
                    cache = cache.max(v);
                }
            }
            if line.contains("system_info:") && system.is_empty() {
                system = line.split("system_info:").nth(1).unwrap_or("").trim().chars().take(200).collect();
            }
            // "load_tensors: CUDA0 model buffer size = 934.70 MiB" and the KV
            // and compute buffers (`-lv 4`): what the model takes on the card.
            if line.contains("buffer size =") && !line.contains("CPU") && !line.contains("_Host") {
                if let Some(v) = line.split("buffer size =").nth(1).and_then(|r| number_before(r, "MiB")) {
                    gpu_mib += v;
                }
            }
        }
        let placement = crate::ai::local_llm::placement_in_log(&text);
        json!({
            "projected_host_mib": projected,
            "prompt_cache_peak_mib": cache.round(),
            "system_info": system,
            "gpu_layers": placement.map(|p| p.gpu_layers),
            "layers": placement.map(|p| p.layers),
            "gpu_buffers_mib": gpu_mib.round(),
        })
    }

    pub fn stop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}
