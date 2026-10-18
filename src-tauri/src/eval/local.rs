//! Local models for a run: Yap's own llamafile runtime serving a GGUF on a
//! free localhost port, started for the run and stopped after (`Drop`).
//!
//! Files come from the installed Yap's `%APPDATA%\yap\llm\` when they're
//! there, **read-only** (run in place, nothing written next to them: the
//! server's log and working folder are the run's), else they're downloaded
//! as `local_llm.rs` does (SHA-256 verified) into the models folder
//! (`--models-dir`, default `E:/yap-builds/eval-models` style: next to the
//! runs).

use std::path::{Path, PathBuf};
use std::process::Child;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

/// The installed app's model folder (never the run's data dir).
pub fn installed_llm_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("yap").join("llm"))
}

/// The llamafile runtime: `--llamafile`, else the installed one (read-only),
/// else downloaded into `models_dir`.
pub async fn runtime(explicit: Option<&Path>, models_dir: &Path) -> Result<PathBuf, String> {
    if let Some(p) = explicit {
        return if p.is_file() { Ok(p.to_path_buf()) } else { Err(format!("no llamafile at {}", p.display())) };
    }
    if let Some(p) = installed_llm_dir().map(|d| d.join(crate::local_llm::RUNTIME_FILENAME)).filter(|p| p.is_file()) {
        return Ok(p);
    }
    let dest = models_dir.join(crate::local_llm::RUNTIME_FILENAME);
    if !dest.is_file() {
        std::fs::create_dir_all(models_dir).map_err(|e| e.to_string())?;
        println!("Downloading the llamafile runtime into {} …", models_dir.display());
        crate::local_llm::download_verified(
            crate::local_llm::RUNTIME_URL,
            &dest,
            crate::local_llm::RUNTIME_SHA256,
            "runtime",
            None,
        )
        .await?;
    }
    Ok(dest)
}

/// The GGUF a `local:<spec>` names: "installed" (the on-device default,
/// Qwen2.5-1.5B), a curated id from `local_llm::CURATED_MODELS`
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
            crate::local_llm::download_verified(&url, &dest, &sha, "model", None).await?;
        }
        Ok::<PathBuf, String>(dest)
    };
    if spec.is_empty() || spec == "installed" || spec == "default" {
        let file = crate::local_llm::MODEL_FILENAME;
        if let Some(p) = installed(file) {
            return Ok(p);
        }
        return fetch(crate::local_llm::MODEL_URL.into(), file.into(), crate::local_llm::MODEL_SHA256.into()).await;
    }
    if let Some(m) = crate::local_llm::curated_by_id(spec) {
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
        crate::local_llm::CURATED_MODELS.iter().map(|m| m.id).collect::<Vec<_>>().join(", ")
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
}

/// Start `runtime` serving `model` as `local_llm::start` does (`--server`,
/// `-c 8192`), but with `ngl` GPU layers (0 = CPU only), the run's folder as
/// its working dir and log, and `extra` arguments. Waits for `/health`.
pub async fn start(
    runtime: &Path,
    model: &Path,
    ngl: u32,
    ctx: u32,
    extra: &[String],
    workdir: &Path,
) -> Result<Server, String> {
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .map_err(|e| format!("no free port: {e}"))?;
    let stem = model.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let log = workdir.join(format!("llamafile-{stem}.log"));
    let file = std::fs::File::create(&log).map_err(|e| e.to_string())?;
    let mut args: Vec<String> = vec![
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
    ];
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
    let pid = child.id();
    let base_url = format!("http://127.0.0.1:{port}/v1");
    let client = reqwest::Client::builder().timeout(Duration::from_secs(3)).build().map_err(|e| e.to_string())?;
    let health = format!("http://127.0.0.1:{port}/health");
    for _ in 0..600 {
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!("llamafile exited during startup ({status}); see {}", log.display()));
        }
        if client.get(&health).send().await.is_ok_and(|r| r.status().is_success()) {
            return Ok(Server {
                child: Some(child),
                pid,
                base_url,
                log,
                load_ms: started.elapsed().as_millis() as u64,
                args,
            });
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

    /// What the server's log says about the model: its buffers (MB) and
    /// what ran where.
    pub fn log_facts(&self) -> Value {
        let text = std::fs::read_to_string(&self.log).unwrap_or_default();
        let mut buffers = serde_json::Map::new();
        let mut system = String::new();
        for line in text.lines() {
            // "… CPU_Mapped model buffer size =  1059.66 MiB", "… KV buffer size = 224.00 MiB"
            if let Some(i) = line.find("buffer size =") {
                let name = line[..i].rsplit(':').next().unwrap_or("").trim().to_string();
                let mb = line[i + "buffer size =".len()..].trim().trim_end_matches("MiB").trim().to_string();
                if let Ok(v) = mb.parse::<f64>() {
                    let e = buffers.entry(name).or_insert(json!(0.0));
                    *e = json!(e.as_f64().unwrap_or(0.0) + v);
                }
            }
            if line.contains("system_info:") && system.is_empty() {
                system = line.split("system_info:").nth(1).unwrap_or("").trim().chars().take(200).collect();
            }
        }
        json!({ "buffers_mib": buffers, "system_info": system })
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
