//! **The on-device model reading screenshots** (Snip to Chat, phase 3,
//! 2026-10-28). Qwen3.5 is a vision-language model: with its **projector**
//! (a second GGUF, `--mmproj`) llama.cpp's server takes the same
//! `image_url` parts every cloud provider does, so a screenshot asked about
//! in Chat goes to the model on this PC — free, and nothing leaves it.
//!
//! - **The projector is an optional download** ([`PROJECTORS`]: the 4B's
//!   F16 one from bartowski's repo, 641 MiB, SHA-256 pinned; the file the
//!   evaluations ran), into `llm/vision/` — beside the models, never among
//!   them (`list_models` lists only `llm/`'s own files). Settings → Language
//!   Models and Chat's "can't read images" line offer it ("Read screenshots
//!   on this PC"); `config.local_vision` turns it off without deleting it.
//! - **Loaded only while it's needed** ([`ensure`]): the sidecar starts
//!   without it; a screenshot question to the on-device model loads it —
//!   the model started with it, or **restarted** with it once no dictation
//!   or answer uses the model — and it stays until the model is freed for
//!   being idle ([`forget`]); the next load, likely a dictation's, is
//!   without it. Measured on an RTX 5070 Ti (docs/model-evals.md "The
//!   on-device model reads screenshots"): a dictation's cleanup takes the
//!   same time with it loaded (176 vs 180 ms back to back), but the model
//!   takes ~0.85 GB more on the card and **1.3 s or more longer to load**
//!   (3.1 → 4.4 s): a dictation that loads the model would wait for it.
//! - **Where it runs** ([`fits_on_card`]): on the graphics card when the
//!   model as planned (its context, MTP, the KV cache) and the projector fit
//!   in the card's room together; else on the processor
//!   (`--no-mmproj-offload`): a screenshot answer in 2–6 s instead of under
//!   one, the model and MTP untouched — the projector never pushes the model
//!   off the card. MTP drafting works with it (llamafile 0.10.6); a start
//!   with the projector never teaches Yap that MTP fails (its key is
//!   blank in `spawn_and_wait`).
//! - **A start with it that fails** ([`ensure`]): the projector isn't
//!   tried again this session ([`FAILED`]), the model comes back without
//!   it (dictation keeps its cleanup), and Chat offers "Send its text
//!   instead" as for any model that can't read images.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};

use super::{llm_dir, CURATED_MODELS};

/// A model's projector: the vision encoder llama.cpp loads with `--mmproj`.
#[derive(Debug)]
pub struct Projector {
    /// The curated model it belongs to (`CuratedLlm::id`).
    pub model: &'static str,
    pub file: &'static str,
    pub url: &'static str,
    /// Its Hugging Face LFS oid.
    pub sha256: &'static str,
    pub bytes: u64,
}

impl Projector {
    pub fn size_mb(&self) -> u64 {
        self.bytes.div_ceil(1_048_576)
    }
}

/// The projectors Yap offers, one per model it was measured with. Only the
/// 4B so far: bartowski's Qwen3.5 0.8B, 2B and 9B repos have theirs too
/// (`mmproj-Qwen_Qwen3.5-<size>-f16.gguf`, 205 / 668 / 918 MB), not yet
/// tried with Yap.
pub const PROJECTORS: &[Projector] = &[Projector {
    model: "qwen3.5-4b",
    file: "mmproj-Qwen_Qwen3.5-4B-f16.gguf",
    url: "https://huggingface.co/bartowski/Qwen_Qwen3.5-4B-GGUF/resolve/main/mmproj-Qwen_Qwen3.5-4B-f16.gguf",
    sha256: "659b59dd44b73b1cd34af6cc424669484b06dc80f4340adf8ea84ad776eef813",
    bytes: 672_423_488,
}];

/// The folder inside `llm/` the projectors live in.
pub const DIR: &str = "vision";

/// What Chat says when the on-device model can't read the screenshot after
/// all (its projector isn't there, is off, or didn't load).
pub const NOT_READY: &str = "The on-device model can't read screenshots right now";

/// How long a screenshot question waits for the model to load with its
/// projector: a dictation under way first, then a restart (~4–5 s).
pub const LOAD_WAIT: Duration = Duration::from_secs(90);

/// The projector loads with the model at its next start ([`ensure`]); the
/// model freed for being idle clears it ([`forget`]).
static WANTED: AtomicBool = AtomicBool::new(false);
/// It failed to load this session: not tried again until Yap restarts.
static FAILED: AtomicBool = AtomicBool::new(false);

/// The projector for the on-device model in `file` (its GGUF's name).
pub fn for_model_file(file: &str) -> Option<&'static Projector> {
    let m = CURATED_MODELS.iter().find(|m| m.filename.eq_ignore_ascii_case(file))?;
    PROJECTORS.iter().find(|p| p.model == m.id)
}

/// The projector for a curated model id.
pub fn for_curated(id: &str) -> Option<&'static Projector> {
    PROJECTORS.iter().find(|p| p.model == id)
}

/// Where projectors are kept: `<data>/llm/vision/`.
pub fn dir() -> PathBuf {
    llm_dir().join(DIR)
}

pub fn path(p: &Projector) -> PathBuf {
    dir().join(p.file)
}

/// It's downloaded (whole: a download is checked before it's put there).
pub fn installed(p: &Projector) -> bool {
    path(p).is_file()
}

/// The projector the model in `model` (its GGUF) can load: downloaded,
/// "Read screenshots on this PC" on, and not failed this session.
pub fn usable(cfg: &crate::config::YapConfig, model: &Path) -> Option<PathBuf> {
    if !cfg.local_vision || FAILED.load(Ordering::SeqCst) {
        return None;
    }
    let p = for_model_file(model.file_name()?.to_str()?)?;
    let at = path(p);
    at.is_file().then_some(at)
}

/// Does the on-device model `id` (its GGUF's name without `.gguf`, as
/// Chat sends it) read images here: its projector usable ([`usable`]).
/// Loaded or not: it loads when a screenshot goes to it ([`ensure`]).
pub fn reads_images(cfg: &crate::config::YapConfig, id: &str) -> bool {
    usable(cfg, &llm_dir().join(format!("{id}.gguf"))).is_some()
}

/// What the projector takes beside its weights when it's on the card: its
/// compute buffer for the largest picture (223 MiB for the 4B's) and a
/// margin — llama.cpp's own worst case for the 4B's is 889 MiB, the
/// weights (641) plus this.
pub const COMPUTE_MIB: u64 = 250;

/// The projector's memory (MiB), from its file's size. Pure.
pub fn mib(bytes: u64) -> u64 {
    bytes / 1_048_576 + COMPUTE_MIB
}

/// The projector on the graphics card only when the model as planned
/// (`model_mib`, `gguf::estimate`) and the projector fit in the card's
/// room together; else the processor: it never pushes the model (or its
/// MTP) off the card. Pure.
pub fn fits_on_card(model_mib: Option<u64>, projector_mib: u64, room_mib: Option<u64>) -> bool {
    match (model_mib, room_mib) {
        (Some(m), Some(room)) => m + projector_mib <= room,
        _ => false,
    }
}

/// The projector loaded with a start ([`for_start`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Load {
    pub path: PathBuf,
    pub on_card: bool,
}

/// Does this start load the projector, and where? Only when a screenshot
/// asked for it ([`ensure`]) and it's usable for `model`.
pub fn for_start(cfg: &crate::config::YapConfig, model: &Path, ctx: u32, plan: super::Plan, ngl: u32) -> Option<Load> {
    if !WANTED.load(Ordering::SeqCst) {
        return None;
    }
    let path = usable(cfg, model)?;
    let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let vram = crate::ai::gpu::this_pc().map(|g| g.vram_mib);
    let model_mib = crate::ai::gguf::model_file(model)
        .map(|f| crate::ai::gguf::estimate(&f.shape, f.bytes, ctx as u64, true, plan.server()).total_mib);
    let on_card = ngl > 0
        && super::context::runs_on_card(None, vram)
        && fits_on_card(model_mib, mib(bytes), super::context::card_room(vram));
    Some(Load { path, on_card })
}

/// The server's arguments for it. Pure.
pub fn args(load: &Load) -> Vec<String> {
    let mut a = vec!["--mmproj".to_string(), load.path.to_string_lossy().into_owned()];
    if !load.on_card {
        a.push("--no-mmproj-offload".to_string());
    }
    a
}

/// The model was freed for being idle: its next load (a dictation's,
/// likely) is without the projector.
pub fn forget() {
    WANTED.store(false, Ordering::SeqCst);
}

/// Is the projector wanted at the next start? (Test mode's view.)
#[cfg(debug_assertions)]
pub fn wanted() -> bool {
    WANTED.load(Ordering::SeqCst)
}

/// It failed to load this session (not tried again until Yap restarts).
pub fn failed() -> bool {
    FAILED.load(Ordering::SeqCst)
}

/// **The on-device model, ready to read a screenshot**: loaded with its
/// projector — started with it, or restarted with it once no dictation or
/// answer uses the model (never under one) — before a screenshot question
/// goes to it. A dictation meanwhile waits for the model as for any load
/// (its endpoint is the stand-by address while it's down). An error when
/// the projector isn't usable, or it doesn't load in [`LOAD_WAIT`]; a
/// start with it that fails puts the model back without it (dictation's
/// cleanup never goes without the model because of the projector).
pub async fn ensure(app: &AppHandle) -> Result<(), String> {
    #[cfg(debug_assertions)]
    if let Ok(mut n) = E2E_ENSURES.lock() {
        *n += 1;
    }
    let cfg = crate::config::load();
    let model = super::active_model_path(&cfg);
    if usable(&cfg, &model).is_none() {
        return Err(NOT_READY.to_string());
    }
    WANTED.store(true, Ordering::SeqCst);
    let deadline = Instant::now() + LOAD_WAIT;
    let left = || deadline.saturating_duration_since(Instant::now());
    // A start under way (a dictation's, maybe without it): let it finish.
    while super::starting() || super::on_demand::loading() {
        if left().is_zero() {
            return Err(format!("{NOT_READY}: the model is still loading"));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let was_running = super::is_running();
    if was_running && super::serving_vision().is_none() {
        // Restarted with it, once nothing uses the model; one restart at a
        // time (a context change waits for this one, and this for it).
        let _one = super::context::RESTARTING.lock().await;
        let idle = super::context::wait_until_idle(
            || super::context::sidecar_busy(app),
            || !left().is_zero(),
            super::context::RESTART_TICK,
            super::context::RESTART_SETTLE,
        )
        .await;
        if !idle {
            return Err(format!("{NOT_READY}: the model was busy"));
        }
        if super::is_running() && super::serving_vision().is_none() {
            tracing::info!("On-device model: restarting it with its screenshot reader");
            super::stop();
        }
    }
    let t0 = Instant::now();
    let woke = super::on_demand::wake(left(), "screenshot").await;
    match woke {
        Ok(_) if super::serving_vision().is_some() => {
            tracing::info!(ms = t0.elapsed().as_millis() as u64, on_card = super::serving_vision(), "On-device model: reading screenshots");
            Ok(())
        }
        Ok(_) => Err(NOT_READY.to_string()),
        Err(e) => {
            // It didn't load with the projector: not again this session,
            // and the model back as it was, for dictation.
            if !super::is_running() && !super::starting() && !super::on_demand::loading() {
                tracing::warn!("On-device model: it didn't start with its screenshot reader; going on without it ({e})");
                FAILED.store(true, Ordering::SeqCst);
                WANTED.store(false, Ordering::SeqCst);
                super::on_demand::forget_failure();
                if was_running {
                    super::on_demand::load_soon(&cfg, "screenshot-failed");
                }
                let _ = app.emit("yap-local-llm-changed", serde_json::json!({}));
            }
            Err(format!("{NOT_READY}: {e}"))
        }
    }
}

/// Test mode: how often a screenshot asked for the projector ([`ensure`]).
#[cfg(debug_assertions)]
static E2E_ENSURES: std::sync::Mutex<u32> = std::sync::Mutex::new(0);

/// What Settings and Chat show of the active model's projector
/// (`local_llm_status().vision`); `null` when the model has none Yap offers.
pub fn status(cfg: &crate::config::YapConfig) -> serde_json::Value {
    let model = super::active_model_path(cfg);
    let Some(p) = model.file_name().and_then(|f| f.to_str()).and_then(for_model_file) else {
        return serde_json::Value::Null;
    };
    let display = CURATED_MODELS.iter().find(|m| m.id == p.model).map(|m| m.display).unwrap_or(p.model);
    // Where it would go now: the model as it would start, and the card's room.
    let ctx = super::context::context_for(cfg);
    let runtime = super::runtime::runtime_version().unwrap_or_else(|| super::runtime::Version::parse(super::RUNTIME_VERSION).expect("a version"));
    let plan = super::plan_for(cfg, &model, ctx, runtime);
    let vram = crate::ai::gpu::this_pc().map(|g| g.vram_mib);
    let model_mib = crate::ai::gguf::model_file(&model)
        .map(|f| crate::ai::gguf::estimate(&f.shape, f.bytes, ctx as u64, true, plan.server()).total_mib);
    let card = super::context::runs_on_card(None, vram);
    let on_card = card && fits_on_card(model_mib, mib(p.bytes), super::context::card_room(vram));
    serde_json::json!({
        "model": p.model,
        "modelName": display,
        "sizeMb": p.size_mb(),
        "installed": installed(p),
        "on": cfg.local_vision,
        // Loaded with the running model: true on the card, false on the
        // processor; null when it isn't loaded.
        "loaded": super::serving_vision(),
        // Where it would go: the card, else the processor — beside a model
        // on a card that has no room for it too (`card`), or like the model.
        "onCard": on_card,
        "card": card,
        "failed": failed(),
        "cancelId": cancel_id(p),
    })
}

/// What cancels a projector's download (`download_cancel`).
pub fn cancel_id(p: &Projector) -> String {
    format!("llm-vision:{}", p.model)
}

/// Progress event payload for a projector's download
/// (`local-vision-download-progress`).
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct VisionProgress {
    model: &'static str,
    percent: u8,
    #[serde(rename = "downloaded_mb")]
    downloaded_mb: f64,
    #[serde(rename = "total_mb")]
    total_mb: f64,
    bytes_per_sec: u64,
    eta_secs: Option<u64>,
    cancel_id: String,
}

/// **Download the active model's projector** ("Read screenshots on this
/// PC"): through the shared helper (resume, speed, Cancel), checked against
/// its SHA-256, into `llm/vision/`; then the setting on. A test run
/// downloads only from its own servers (`YAP_LLM_SOURCES`, key
/// `vision:<curated id>`).
pub async fn install(app: &AppHandle) -> Result<(), String> {
    let cfg = crate::config::load();
    let model = super::active_model_path(&cfg);
    let p = model
        .file_name()
        .and_then(|f| f.to_str())
        .and_then(for_model_file)
        .ok_or_else(|| "This model can't read screenshots on this PC: Qwen3.5 4B can".to_string())?;
    if !installed(p) {
        let ticket = crate::shell::download::Ticket::begin(&cancel_id(p)).ok_or_else(|| "The screenshot reader is already downloading".to_string())?;
        std::fs::create_dir_all(dir()).map_err(|e| format!("Couldn't make the folder for it: {e}"))?;
        let (url, sha) = super::models::source(&format!("vision:{}", p.model), p.url, p.sha256);
        if crate::shell::e2e::active() && !super::models::allowed_in_test_run(&url) {
            return Err("Test runs never download models from the internet".to_string());
        }
        let dest = path(p);
        let partial = dest.with_extension("partial");
        tracing::info!(model = p.model, size_mb = p.size_mb(), "Downloading the on-device model's screenshot reader");
        let report = |pr: crate::shell::download::Progress| {
            let _ = app.emit(
                "local-vision-download-progress",
                VisionProgress {
                    model: p.model,
                    percent: pr.percent(),
                    downloaded_mb: pr.downloaded as f64 / 1_048_576.0,
                    total_mb: pr.total as f64 / 1_048_576.0,
                    bytes_per_sec: pr.bytes_per_sec,
                    eta_secs: pr.eta_secs,
                    cancel_id: cancel_id(p),
                },
            );
        };
        crate::shell::download::fetch(
            crate::shell::download::Fetch { url: &url, partial: &partial, size: None, what: crate::privacy::net::Download::OnDeviceAi, ticket: &ticket },
            &report,
        )
        .await?;
        super::models::verify_into(&partial, &dest, &sha, "screenshot reader").await?;
        tracing::info!(model = p.model, "The screenshot reader is downloaded");
    }
    FAILED.store(false, Ordering::SeqCst);
    crate::shell::settings::change_config(app, |c| {
        c.local_vision = true;
        Ok(())
    })?;
    let _ = app.emit("yap-local-llm-changed", serde_json::json!({}));
    let _ = app.emit("yap-chat-model-changed", serde_json::json!({ "scope": null }));
    Ok(())
}

/// **Remove the active model's projector** (its file goes): a model running
/// with it is freed first when nothing uses it (Windows won't delete a file
/// the engine holds), else "in use right now".
pub fn remove(app: &AppHandle, curated: Option<&str>) -> Result<(), String> {
    let cfg = crate::config::load();
    let p = match curated {
        Some(id) => for_curated(id),
        None => super::active_model_path(&cfg).file_name().and_then(|f| f.to_str()).and_then(for_model_file),
    }
    .ok_or_else(|| "That model has no screenshot reader".to_string())?;
    {
        let no_start = super::STARTING.try_lock().ok();
        if super::serving_vision().is_some() {
            if no_start.is_none() || super::requests_in_flight() > 0 || super::context::sidecar_busy(app) {
                return Err("The screenshot reader is in use right now. Try again in a moment.".to_string());
            }
            super::stop();
        }
        match std::fs::remove_file(path(p)) {
            Ok(()) => tracing::info!(model = p.model, "Removed the on-device model's screenshot reader"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                tracing::warn!(kind = ?e.kind(), code = e.raw_os_error(), "Couldn't remove the screenshot reader");
                return Err("Windows wouldn't delete the screenshot reader. Try again in a moment.".to_string());
            }
        }
    }
    WANTED.store(false, Ordering::SeqCst);
    let _ = app.emit("yap-local-llm-changed", serde_json::json!({}));
    let _ = app.emit("yap-chat-model-changed", serde_json::json!({ "scope": null }));
    Ok(())
}

/// A model's projector goes with it (its uninstall): best effort, never
/// an error of the uninstall's.
pub fn remove_with_model(file: &str) {
    if let Some(p) = for_model_file(file) {
        if std::fs::remove_file(path(p)).is_ok() {
            tracing::info!(model = p.model, "Removed the uninstalled model's screenshot reader");
        }
    }
}

/// "Read screenshots on this PC", set from Settings or Chat's line.
pub fn set_on(app: &AppHandle, on: bool) -> Result<(), String> {
    crate::shell::settings::change_config(app, |c| {
        c.local_vision = on;
        Ok(())
    })?;
    if on {
        FAILED.store(false, Ordering::SeqCst);
    } else {
        WANTED.store(false, Ordering::SeqCst);
    }
    let _ = app.emit("yap-local-llm-changed", serde_json::json!({}));
    let _ = app.emit("yap-chat-model-changed", serde_json::json!({ "scope": null }));
    Ok(())
}

/// The projector's lines in a server log (`-lv 4`): its weights' size and
/// whether they went to a graphics card ("CLIP using CUDA0 backend") —
/// its compute buffer is a `… buffer size` line `log_facts` counts already.
/// `(MiB, on the card)`; `None` without one. Pure.
pub fn in_log(log: &str) -> Option<(u64, bool)> {
    let backend = log.lines().find_map(|l| l.split("CLIP using ").nth(1))?;
    let on_card = !backend.trim_start().starts_with("CPU");
    let size = log
        .lines()
        .filter(|l| l.contains("load_hparams:") && l.contains("model size:"))
        .find_map(|l| l.split("model size:").nth(1)?.split_whitespace().next()?.parse::<f64>().ok())?;
    Some((size.round() as u64, on_card))
}

/// Test mode only: how often screenshots asked for the projector, whether
/// it's wanted at the next start, and whether the running model has it.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_local_vision() -> Result<serde_json::Value, String> {
    if !crate::shell::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    Ok(serde_json::json!({
        "ensures": E2E_ENSURES.lock().map(|n| *n).unwrap_or(0),
        "wanted": wanted(),
        "loaded": super::serving_vision(),
        "failed": FAILED.load(Ordering::SeqCst),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_4b_has_its_projector_and_no_other_model_does_yet() {
        let p = for_model_file("Qwen_Qwen3.5-4B-Q4_K_M.gguf").expect("the 4B's");
        assert_eq!(p.model, "qwen3.5-4b");
        assert_eq!(p.sha256.len(), 64);
        assert!(p.url.ends_with(p.file));
        assert!(p.file.starts_with("mmproj-"));
        assert_eq!(p.size_mb(), 642);
        // Every projector belongs to a curated model.
        for p in PROJECTORS {
            assert!(CURATED_MODELS.iter().any(|m| m.id == p.model), "{}", p.model);
        }
        assert!(for_model_file("qwen2.5-1.5b-instruct-q4_k_m.gguf").is_none());
        assert!(for_model_file("Qwen_Qwen3.5-9B-Q4_K_M.gguf").is_none());
        assert!(for_model_file("my-own.gguf").is_none());
        assert!(for_curated("qwen3.5-4b").is_some());
    }

    #[test]
    fn it_goes_on_the_card_only_beside_the_model_never_instead() {
        // The 4B with MTP at 8K (~4.15 GB) and the projector (~0.9 GB) on a
        // 16 GB card's room.
        assert!(fits_on_card(Some(4_153), mib(672_423_488), Some(14_255)));
        assert_eq!(mib(672_423_488), 891);
        // A 6 GB card: the model fits, the projector beside it doesn't.
        assert!(!fits_on_card(Some(4_153), 891, Some(4_096)));
        assert!(fits_on_card(Some(3_000), 891, Some(4_096)));
        // No card, or a model Yap can't read: the processor.
        assert!(!fits_on_card(Some(1_000), 891, None));
        assert!(!fits_on_card(None, 891, Some(14_255)));
    }

    #[test]
    fn its_arguments_put_it_where_it_fits() {
        let on = Load { path: PathBuf::from(r"C:\llm\vision\mmproj.gguf"), on_card: true };
        assert_eq!(args(&on), ["--mmproj", r"C:\llm\vision\mmproj.gguf"]);
        let off = Load { on_card: false, ..on };
        assert_eq!(args(&off), ["--mmproj", r"C:\llm\vision\mmproj.gguf", "--no-mmproj-offload"]);
    }

    #[test]
    fn the_log_says_where_it_went() {
        let card = "I clip_ctx: CLIP using CUDA0 backend\nI load_hparams: model size:         641.26 MiB\nI reserve_compute_meta:      CUDA0 compute buffer size =   223.30 MiB";
        assert_eq!(in_log(card), Some((641, true)));
        let cpu = "I clip_ctx: CLIP using CPU backend\nI load_hparams: model size:         641.26 MiB";
        assert_eq!(in_log(cpu), Some((641, false)));
        assert_eq!(in_log("I load_tensors: offloaded 33/33 layers to GPU"), None);
    }

    #[test]
    fn it_reads_images_only_when_downloaded_and_on() {
        // Nothing is downloaded for a model without a projector, whatever
        // the setting.
        let cfg = crate::config::YapConfig::default();
        assert!(cfg.local_vision, "on by default: downloading it is the choice");
        assert!(!reads_images(&cfg, "qwen2.5-1.5b-instruct-q4_k_m"));
        let off = crate::config::YapConfig { local_vision: false, ..Default::default() };
        assert!(usable(&off, Path::new("Qwen_Qwen3.5-4B-Q4_K_M.gguf")).is_none());
    }
}
