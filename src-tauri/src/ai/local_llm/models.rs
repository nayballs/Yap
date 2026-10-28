//! The curated on-device models (and the one recommended for this PC),
//! and installing them and the runtime: verified downloads, with
//! progress for Settings.

use std::path::Path;

use tauri::{AppHandle, Emitter};

use super::runtime::{next_is_whole, read_marker_at, write_marker_at, Marker, UpgradeLock};
use super::{
    llm_dir, model_path, ondevice_selected, runtime_path, MODEL_DISPLAY, MODEL_FILENAME, MODEL_SHA256, MODEL_URL, RUNTIME_MARKER,
    RUNTIME_NEXT, RUNTIME_SHA256, RUNTIME_URL, RUNTIME_VERSION,
};

/// Progress event payload for the on-device install (`local-llm-download-progress`).
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct DownloadProgress {
    /// "runtime" (the llamafile engine) or "model" (the GGUF).
    stage: &'static str,
    /// The curated id the download is for ("" for the bundled default's
    /// install): the Chat picker shows the bar in that model's row.
    model: String,
    percent: u8,
    #[serde(rename = "downloaded_mb")]
    downloaded_mb: f64,
    #[serde(rename = "total_mb")]
    total_mb: f64,
    /// The speed over the last few seconds and the time left at it
    /// (`shell/download.rs`).
    bytes_per_sec: u64,
    eta_secs: Option<u64>,
    /// What Cancel cancels (`download_cancel {id}`).
    cancel_id: String,
}

/// The id a curated model's install is cancelled by (`download_cancel`);
/// its engine's download, when it brings one, is part of it.
pub fn cancel_id(model: &str) -> String {
    format!("llm:{}", if model.is_empty() { DEFAULT_MODEL_ID } else { model })
}

/// Where a download comes from: `url` + `sha256` as pinned, or in a debug
/// build a stand-in from the JSON file `YAP_LLM_SOURCES` (`{"runtime" |
/// "<curated id>": {"url", "sha256"}}`): how the e2e suite downloads small
/// fakes from a local server (`e2e/support/local-llm.js`).
pub(crate) fn source(key: &str, url: &str, sha256: &str) -> (String, String) {
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
pub(crate) fn allowed_in_test_run(url: &str) -> bool {
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
    /// ("qwen"|"granite"|"gemma"|"llama"|"mistral"|"phi").
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
/// HuggingFace `lfs.oid` values, read from each repo's file list; all are
/// ungated Q4_K_M GGUFs whose architecture the pinned llamafile serves.
/// Refreshed 2026-10-27 (yap-private's local-ai-research.md, "Addendum"):
/// the 2024 models (Qwen2.5 0.5B/3B/7B, Llama 3.2 1B/3B, Llama 3.1 8B,
/// Gemma 2 2B/9B, Mistral 7B v0.3, Phi-3.5 Mini) left the list for newer
/// ones measured with the model evaluations (docs/model-evals.md); a file
/// of theirs already downloaded is still listed and served as the person's
/// own GGUF.
pub const CURATED_MODELS: &[CuratedLlm] = &[
    // ── Qwen ─────────────────────────────────────────────────────────────
    CuratedLlm {
        id: "qwen3.5-0.8b",
        display: "Qwen3.5 0.8B",
        blurb: "Tiny and instant, even on a laptop",
        filename: "Qwen_Qwen3.5-0.8B-Q4_K_M.gguf",
        url: "https://huggingface.co/bartowski/Qwen_Qwen3.5-0.8B-GGUF/resolve/main/Qwen_Qwen3.5-0.8B-Q4_K_M.gguf",
        sha256: "fb044e93939a70469c905781334f5de1e6c8b608ced6cbc8c9249bd4127d9526",
        size_mb: 553,
        default: false,
        needs_gpu: false,
        family: "qwen",
        max_context: 262_144,
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
        id: "qwen3.5-2b",
        display: "Qwen3.5 2B",
        blurb: "Newer and smarter than the default, still fine without a graphics card",
        filename: "Qwen_Qwen3.5-2B-Q4_K_M.gguf",
        url: "https://huggingface.co/bartowski/Qwen_Qwen3.5-2B-GGUF/resolve/main/Qwen_Qwen3.5-2B-Q4_K_M.gguf",
        sha256: "57a1085840f497d764a7fc5d346922dbde961efb54cc792ea81d694fd846a1d8",
        size_mb: 1332,
        default: false,
        needs_gpu: false,
        family: "qwen",
        max_context: 262_144,
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
        id: "qwen3.5-9b",
        display: "Qwen3.5 9B",
        blurb: "A bigger Qwen, a little slower. Wants a 12 GB graphics card",
        // The file the evaluations ran: 204/210 at 1.2 s a Chat turn on an
        // RTX 5070 Ti (the 4B: 206/210, 1.1 s), but 4 false memories from an
        // import; not better than the 4B, which stays recommended (27 Oct 2026).
        filename: "Qwen_Qwen3.5-9B-Q4_K_M.gguf",
        url: "https://huggingface.co/bartowski/Qwen_Qwen3.5-9B-GGUF/resolve/main/Qwen_Qwen3.5-9B-Q4_K_M.gguf",
        sha256: "d784ce9eda1a5a7b51e8f705a9e6310844bf4f173654d115823c775fdea56d43",
        size_mb: 5884,
        default: false,
        needs_gpu: true,
        family: "qwen",
        max_context: 262_144,
    },
    // ── Gemma (Google) ───────────────────────────────────────────────────
    CuratedLlm {
        id: "gemma-4-12b",
        display: "Gemma 4 12B",
        blurb: "Google's Gemma, 12B. Wants a 12 GB graphics card",
        // LM Studio's Q4_K_M of google/gemma-4-12B-it (Apache-2.0; bartowski
        // has none): 200/210 in the model evaluations at 1.9 s a Chat turn,
        // cleanup in 0.41 s (27 Oct 2026). The E4B (93 %, notes not
        // created when asked) isn't offered.
        filename: "gemma-4-12B-it-Q4_K_M.gguf",
        url: "https://huggingface.co/lmstudio-community/gemma-4-12B-it-GGUF/resolve/main/gemma-4-12B-it-Q4_K_M.gguf",
        sha256: "95d83ba36642b1f385fb906b5962a71763361be3bac930a709945f72d97473f8",
        size_mb: 7039,
        default: false,
        needs_gpu: true,
        family: "gemma",
        max_context: 262_144,
    },
    // ── IBM Granite ──────────────────────────────────────────────────────
    CuratedLlm {
        id: "granite-4.1-8b",
        display: "Granite 4.1 8B",
        blurb: "IBM's 8B, an alternative to Qwen. Wants a graphics card",
        // The file the evaluations ran: 200/210 at 2.2 s a Chat turn (27 Oct
        // 2026), cleanup in 195 ms (21 Oct). Dense, so a long context takes
        // far more memory than Qwen3.5's.
        filename: "ibm-granite_granite-4.1-8b-Q4_K_M.gguf",
        url: "https://huggingface.co/bartowski/ibm-granite_granite-4.1-8b-GGUF/resolve/main/ibm-granite_granite-4.1-8b-Q4_K_M.gguf",
        sha256: "5f42811b446eeddf8ca50a287e0c49abafb11f0ca8ed23606fd758720d2a351f",
        size_mb: 5283,
        default: false,
        needs_gpu: true,
        family: "granite",
        max_context: 131_072,
    },
];

pub fn curated_by_id(id: &str) -> Option<&'static CuratedLlm> {
    CURATED_MODELS.iter().find(|m| m.id == id)
}

/// Ensure the llamafile runtime is on disk (SHA-verified download if missing),
/// with its marker ([`RUNTIME_MARKER`]). An older one already there stays
/// until the background upgrade replaces it ([`spawn_runtime_upgrade`]).
/// `model`: the curated id the install is for (its progress names it);
/// `ticket`: what cancels it.
pub(crate) async fn ensure_runtime(model: &str, app: Option<&AppHandle>, ticket: &crate::shell::download::Ticket) -> Result<(), String> {
    tokio::fs::create_dir_all(llm_dir())
        .await
        .map_err(|e| format!("failed to create llm dir: {}", e))?;

    if !runtime_path().is_file() {
        let dir = llm_dir();
        let _lock = UpgradeLock::take(&dir)
            .ok_or_else(|| "Another Yap is setting up the on-device model's engine. Try again in a minute.".to_string())?;
        if runtime_path().is_file() {
            return Ok(()); // the other Yap just did
        }
        let (url, sha) = source("runtime", RUNTIME_URL, RUNTIME_SHA256);
        // Beside it first (or kept from an upgrade), then the marker, then
        // into place: never a runtime on disk its marker doesn't describe.
        let next = dir.join(RUNTIME_NEXT);
        if !(next.is_file() && next_is_whole(&next, &sha).await) {
            download_for(&url, &next, &sha, "runtime", model, app, ticket).await?;
        }
        let learnt = read_marker_at(&dir.join(RUNTIME_MARKER)).unwrap_or_default();
        let marker = Marker { version: RUNTIME_VERSION.to_string(), sha256: sha, unproven: false, ..Default::default() }.learnt_from(&learnt);
        write_marker_at(&dir.join(RUNTIME_MARKER), &marker).map_err(|e| format!("Couldn't write the engine's marker: {e}"))?;
        std::fs::rename(&next, runtime_path()).map_err(|e| format!("Couldn't put the engine in place: {e}"))?;
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
    let ticket = crate::shell::download::Ticket::begin(&cancel_id(""))
        .ok_or_else(|| "That model is already downloading".to_string())?;
    ensure_runtime("", app, &ticket).await?;
    if !model_path().is_file() {
        let (url, sha) = source(DEFAULT_MODEL_ID, MODEL_URL, MODEL_SHA256);
        download_for(&url, &model_path(), &sha, "model", "", app, &ticket).await?;
    }
    Ok(())
}

/// Download the runtime + a specific **curated** model by id. Returns the
/// curated entry so the caller can point `pp_local_model` at its filename.
/// Cancelled by [`cancel_id`] (`download_cancel`).
pub async fn install_curated(
    id: &str,
    app: Option<&AppHandle>,
) -> Result<&'static CuratedLlm, String> {
    let m = curated_by_id(id).ok_or_else(|| format!("unknown cleanup model id: {}", id))?;
    let ticket = crate::shell::download::Ticket::begin(&cancel_id(m.id))
        .ok_or_else(|| format!("{} is already downloading", m.display))?;
    ensure_runtime(m.id, app, &ticket).await?;
    let dest = llm_dir().join(m.filename);
    if !dest.is_file() {
        let (url, sha) = source(m.id, m.url, m.sha256);
        download_for(&url, &dest, &sha, "model", m.id, app, &ticket).await?;
    }
    Ok(m)
}

/// Stream `url` → `dest.partial`, emit progress, verify SHA-256, then rename into
/// place. A verification failure deletes the partial and errors (safe: a corrupt
/// download can never be used). (The model evaluations' downloads,
/// `eval/local.rs`: debug builds.)
#[cfg(debug_assertions)]
pub(crate) async fn download_verified(
    url: &str,
    dest: &Path,
    expected_sha: &str,
    stage: &'static str,
    app: Option<&AppHandle>,
) -> Result<(), String> {
    download_for(url, dest, expected_sha, stage, "", app, &crate::shell::download::Ticket::unlisted()).await
}

/// A verified download, its progress naming the curated model `model`,
/// through the shared helper (`shell/download.rs`): it resumes a partial
/// file an earlier try left, gives up on a server or connection silent for
/// a minute (keeping the partial file, to resume), and a cancel (`ticket`)
/// takes the partial file away. A bad hash deletes it too: a corrupt
/// download can never be used.
#[allow(clippy::too_many_arguments)]
pub(super) async fn download_for(
    url: &str,
    dest: &Path,
    expected_sha: &str,
    stage: &'static str,
    model: &str,
    app: Option<&AppHandle>,
    ticket: &crate::shell::download::Ticket,
) -> Result<(), String> {
    if crate::shell::e2e::active() && !allowed_in_test_run(url) {
        return Err("Test runs never download models from the internet".to_string());
    }
    let partial = dest.with_extension("partial");
    tracing::info!(url, dest = %dest.display(), stage, "Downloading on-device cleanup asset");
    let report = |p: crate::shell::download::Progress| {
        if let Some(app) = app {
            let _ = app.emit(
                "local-llm-download-progress",
                DownloadProgress {
                    stage,
                    model: model.to_string(),
                    percent: p.percent(),
                    downloaded_mb: p.downloaded as f64 / 1_048_576.0,
                    total_mb: p.total as f64 / 1_048_576.0,
                    bytes_per_sec: p.bytes_per_sec,
                    eta_secs: p.eta_secs,
                    cancel_id: cancel_id(model),
                },
            );
        }
    };
    crate::shell::download::fetch(
        crate::shell::download::Fetch { url, partial: &partial, size: None, what: crate::privacy::net::Download::OnDeviceAi, ticket },
        &report,
    )
    .await?;
    verify_into(&partial, dest, expected_sha, stage).await
}

/// The whole file checked against its SHA-256, then put in place (a bad
/// one deleted).
pub(super) async fn verify_into(partial: &Path, dest: &Path, expected_sha: &str, stage: &'static str) -> Result<(), String> {
    // Verify SHA-256 on a blocking thread (files are hundreds of MB to ~1 GB).
    let verify_path = partial.to_path_buf();
    let expected = expected_sha.to_string();
    let ok = tokio::task::spawn_blocking(move || match compute_sha256(&verify_path) {
        Ok(actual) => actual.eq_ignore_ascii_case(&expected),
        Err(_) => false,
    })
    .await
    .map_err(|e| format!("sha task panicked: {}", e))?;
    if !ok {
        let _ = tokio::fs::remove_file(partial).await;
        return Err(format!("{} verification failed (corrupt download) — retry", stage));
    }

    tokio::fs::rename(partial, dest)
        .await
        .map_err(|e| format!("rename: {}", e))?;
    Ok(())
}

/// SHA-256 of a file, streamed so large models don't load into memory.
pub(super) fn compute_sha256(path: &Path) -> std::io::Result<String> {
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
}
