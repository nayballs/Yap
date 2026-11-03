//! Local models for a run: Yap's own engine — one of llama.cpp's builds, or
//! llamafile — serving a GGUF on a free localhost port, started for the run
//! and stopped after (`Drop`).
//!
//! Files come from the installed Yap's `%APPDATA%\yap\llm\` when they're
//! there, **read-only** (run in place, nothing written next to them: the
//! server's log is the run's), else they're downloaded as `local_llm.rs`
//! does (SHA-256 verified) into the models folder (`--models-dir`, by
//! default a folder like `<builds>/eval-models`: next to the runs).

use std::path::{Path, PathBuf};
use std::process::Child;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::ai::local_llm::engine::{self, Build, Engine, Variant};

/// The installed app's model folder (never the run's data dir).
pub fn installed_llm_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("yap").join("llm"))
}

/// Which engine a run serves its local model with (`--engine`,
/// `--engine-variant`, `--llamafile`).
#[derive(Debug, Clone, Copy, Default)]
pub struct Choice<'a> {
    /// `--llamafile <path>`: that llamafile.
    pub llamafile: Option<&'a Path>,
    /// `--engine`: "installed" (the default: what the installed Yap runs) |
    /// "llamafile" | "floor" | "latest" (the newest 3 days old) |
    /// "b12345" | a folder with `llama-server.exe` in it.
    pub engine: Option<&'a str>,
    /// `--engine-variant cuda13|cuda12|vulkan|cpu` (default: what Yap
    /// installs first on this PC).
    pub variant: Option<&'a str>,
}

/// **The run's engine** ([`Choice`]). A llama.cpp build not on disk is
/// downloaded into `models_dir/engine/` (each file checked against
/// GitHub's SHA-256) and run from there; the installed app's are run in
/// place, read-only.
pub async fn runtime(choice: &Choice<'_>, models_dir: &Path) -> Result<Engine, String> {
    if let Some(p) = choice.llamafile {
        if !p.is_file() {
            return Err(format!("no llamafile at {}", p.display()));
        }
        let version = crate::ai::local_llm::runtime::probe_version(p)
            .await
            .or_else(|| crate::ai::local_llm::runtime::Version::parse(crate::ai::local_llm::RUNTIME_VERSION))
            .ok_or("llamafile's version")?;
        return Ok(Engine::Llamafile { exe: p.to_path_buf(), version });
    }
    let variant = match choice.variant {
        Some(v) => Variant::from_id(v).ok_or_else(|| format!("--engine-variant cuda13|cuda12|vulkan|cpu, not {v}"))?,
        None => engine::primary_variant(),
    };
    let spec = choice.engine.unwrap_or("installed").trim();
    match spec {
        "installed" => {
            // What the installed Yap runs: its llama.cpp build, else its
            // llamafile when it's the pinned one, else the floor.
            if let Some(e) = installed_llm_dir().and_then(|d| engine::active_in(&d)) {
                match &e {
                    Engine::LlamaCpp { .. } => return Ok(e),
                    Engine::Llamafile { version, .. } if Some(*version) == crate::ai::local_llm::runtime::Version::parse(crate::ai::local_llm::RUNTIME_VERSION) => return Ok(e),
                    _ => {}
                }
            }
            download_build(engine::FLOOR, variant, models_dir).await
        }
        "llamafile" => Ok(Engine::Llamafile { exe: llamafile(models_dir).await?, version: crate::ai::local_llm::runtime::Version::parse(crate::ai::local_llm::RUNTIME_VERSION).ok_or("a version")? }),
        "floor" => download_build(engine::FLOOR, variant, models_dir).await,
        "latest" => {
            let release = engine::feed::newest(&[variant], &|_| false).await?.ok_or("no build 3 days old on the feed for this variant")?;
            download_build(release.build, variant, models_dir).await
        }
        _ => {
            if let Some(b) = Build::parse(spec) {
                return download_build(b, variant, models_dir).await;
            }
            folder_engine(Path::new(spec)).await
        }
    }
}

/// A build in `models_dir/engine/` (downloaded and checked when it isn't there).
async fn download_build(build: Build, variant: Variant, models_dir: &Path) -> Result<Engine, String> {
    let edir = models_dir.join("engine");
    let (zip, cudart) = if build == engine::FLOOR {
        engine::floor_assets(variant)
    } else {
        let release = engine::feed::release(build).await?;
        engine::feed::pick_assets(&release, variant).ok_or_else(|| format!("{build} has no {} build for Windows", variant.id()))?
    };
    println!("The engine: llama.cpp {build} ({}) in {} …", variant.id(), edir.display());
    let installed = engine::install(&edir, build, variant, &zip, cudart.as_ref(), &|_| {}, &crate::shell::download::Ticket::unlisted()).await?;
    Ok(Engine::llama_cpp(&edir, &installed))
}

/// A folder holding a llama.cpp build (`llama-server.exe` and its DLLs):
/// its build from `--version`, CUDA, Vulkan or the processor by its
/// backends, NVIDIA's runtime in the folder or a `cudart…` folder beside it.
async fn folder_engine(dir: &Path) -> Result<Engine, String> {
    if !dir.join(engine::SERVER_EXE).is_file() {
        return Err(format!("--engine: no {} in {} (give installed, llamafile, floor, latest, a build like b11982, or a folder)", engine::SERVER_EXE, dir.display()));
    }
    let has = |d: &Path, prefix: &str| {
        std::fs::read_dir(d).map(|rd| rd.filter_map(|e| e.ok()).any(|e| e.file_name().to_string_lossy().to_ascii_lowercase().starts_with(prefix))).unwrap_or(false)
    };
    let variant = if has(dir, "ggml-cuda") {
        Variant::Cuda13
    } else if has(dir, "ggml-vulkan") {
        Variant::Vulkan
    } else {
        Variant::Cpu
    };
    let exe = dir.join(engine::SERVER_EXE);
    let out = tokio::task::spawn_blocking(move || std::process::Command::new(exe).arg("--version").output()).await.map_err(|e| e.to_string())?.map_err(|e| e.to_string())?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let build = text
        .split("(build ")
        .nth(1)
        .and_then(|r| r.split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|n| n.parse().ok())
        .map(Build)
        .ok_or("llama-server didn't say its build")?;
    let cudart = if variant.cuda() && !has(dir, "cudart64_") {
        dir.parent().and_then(|p| std::fs::read_dir(p).ok()).and_then(|rd| {
            rd.filter_map(|e| e.ok()).map(|e| e.path()).find(|p| p.is_dir() && p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("cudart")) && has(p, "cudart64_"))
        })
    } else {
        None
    };
    let installed = engine::Installed { build, variant, sha256: String::new(), cudart: None, device: None };
    Ok(Engine::LlamaCpp { dir: dir.to_path_buf(), installed, cudart })
}

/// The llamafile runtime: the installed one (read-only) when it's the
/// version Yap pins (its marker says so), else the pinned one downloaded
/// into `models_dir` (verified, with its marker).
async fn llamafile(models_dir: &Path) -> Result<PathBuf, String> {
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
/// ("qwen3.5-9b"), a path, or an https URL with `#sha256=<hex>`. Installed
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

/// The projector for `model` (`local_llm::vision::PROJECTORS`), for
/// `--vision`: the installed app's (`llm/vision/`, read-only) when it's
/// there, else downloaded (verified) into `models_dir`. `None`: the model
/// has none Yap offers.
pub async fn projector(model: &Path, models_dir: &Path) -> Result<Option<PathBuf>, String> {
    let Some(p) = model.file_name().and_then(|f| f.to_str()).and_then(crate::ai::local_llm::vision::for_model_file) else {
        return Ok(None);
    };
    if let Some(at) = installed_llm_dir().map(|d| d.join(crate::ai::local_llm::vision::DIR).join(p.file)).filter(|p| p.is_file()) {
        return Ok(Some(at));
    }
    let dest = models_dir.join(p.file);
    if !dest.is_file() {
        std::fs::create_dir_all(models_dir).map_err(|e| e.to_string())?;
        println!("Downloading {} into {} …", p.file, models_dir.display());
        crate::ai::local_llm::models::download_verified(p.url, &dest, p.sha256, "projector", None).await?;
    }
    Ok(Some(dest))
}

/// A running server (llama.cpp's or llamafile's).
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
    /// Its key, as the sidecar has one (`local_llm::key`): the run's
    /// requests carry it; forgotten when the server stops.
    key: Option<crate::ai::local_llm::key::Issued>,
}

/// How a run's server runs the model (`local_llm::Plan`): the engine's
/// runtime, and MTP as Yap's sidecar would decide on this PC — `mtp`
/// overrides (still only on CUDA, where it's measured); never on the
/// processor (`ngl` 0).
pub async fn plan(runtime: &Engine, model: &Path, ngl: u32, ctx: u32, mtp: Option<bool>) -> crate::ai::local_llm::Plan {
    let rt = runtime.runtime();
    let yap = crate::ai::local_llm::plan_for(&crate::config::load(), model, ctx, rt);
    let on = ngl > 0 && rt.cuda() && mtp.unwrap_or(yap.mtp);
    crate::ai::local_llm::Plan { runtime: rt, mtp: on, kv_q8: ngl > 0 && yap.kv_q8, per_slot: ngl == 0 && rt.is_current() }
}

/// Start `runtime` serving `model` with `local_llm::server_args` — as Yap's
/// sidecar runs (thinking off, the caches capped, `-lv 4`, MTP as `plan`
/// says) — but with `ngl` GPU layers (0 = CPU only), the log in the run's
/// folder (llamafile's working folder too; a llama.cpp build runs in its own,
/// `Engine::command`), and `extra` arguments after Yap's (a later flag wins:
/// `--reasoning auto`, `--cache-ram 8192` measure the old way). Waits for
/// `/health`, then warms the model up as the sidecar does (`warm`).
#[allow(clippy::too_many_arguments)]
pub async fn start(
    runtime: &Engine,
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
    let log = workdir.join(format!("server-{stem}.log"));
    let file = std::fs::File::create(&log).map_err(|e| e.to_string())?;
    let mut args = crate::ai::local_llm::server_args(model, port, ngl, ctx, plan);
    args.extend(extra.iter().cloned());
    let mut cmd = runtime.command();
    if runtime.runtime().is_llamafile() {
        cmd.current_dir(workdir);
    }
    cmd.args(&args).stdin(std::process::Stdio::null()).stdout(file.try_clone().map_err(|e| e.to_string())?).stderr(file);
    let key = crate::ai::local_llm::key::issue(port)?;
    key.give(&mut cmd);
    let started = Instant::now();
    let mut child = cmd.spawn().map_err(|e| format!("couldn't start {}: {e}", runtime.label()))?;
    // Ended with this process however it ends (a run stopped mid-way, End task).
    crate::shell::procs::end_with_yap(&child);
    let pid = child.id();
    let base_url = format!("http://127.0.0.1:{port}/v1");
    let client = reqwest::Client::builder().timeout(Duration::from_secs(3)).build().map_err(|e| e.to_string())?;
    let health = format!("http://127.0.0.1:{port}/health");
    for _ in 0..600 {
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!("{} exited during startup ({status}); see {}", runtime.label(), log.display()));
        }
        if client.get(&health).send().await.is_ok_and(|r| r.status().is_success()) {
            let load_ms = started.elapsed().as_millis() as u64;
            let warm_up = if warm { Some(crate::ai::local_llm::warm_up(&base_url, &stem).await) } else { None };
            return Ok(Server { child: Some(child), pid, base_url, log, load_ms, args, plan, warm_up, key: Some(key) });
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let _ = child.kill();
    let _ = child.wait();
    Err(format!("{} wasn't ready after 5 minutes; see {}", runtime.label(), log.display()))
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
        self.key.take();
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}
