//! The curated on-device models (and the one recommended for this PC),
//! and installing them and the runtime: verified downloads, with
//! progress for Settings.

use std::path::Path;
use std::time::Duration;

use tauri::{AppHandle, Emitter};

use crate::privacy::net::Recorded as _;

use super::runtime::{free_bytes, next_is_whole, read_marker_at, room_for, write_marker_at, Marker, UpgradeLock};
use super::{
    llm_dir, model_path, ondevice_selected, runtime_path, MODEL_DISPLAY, MODEL_FILENAME, MODEL_SHA256, MODEL_URL, RUNTIME_MARKER,
    RUNTIME_NEXT, RUNTIME_SHA256, RUNTIME_URL, RUNTIME_VERSION,
};

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

/// Ensure the llamafile runtime is on disk (SHA-verified download if missing),
/// with its marker ([`RUNTIME_MARKER`]). An older one already there stays
/// until the background upgrade replaces it ([`spawn_runtime_upgrade`]).
/// `model`: the curated id the install is for (its progress names it).
pub(crate) async fn ensure_runtime(model: &str, app: Option<&AppHandle>) -> Result<(), String> {
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
            download_for(&url, &next, &sha, "runtime", model, app).await?;
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
    download_for(url, dest, expected_sha, stage, "", app).await
}

/// How long a download may go without a byte (or a server without an
/// answer) before it's given up on: a stalled connection never leaves an
/// install or the background upgrade at "downloading".
const DOWNLOAD_STALL: Duration = Duration::from_secs(60);

/// Room left on the disk beside a download, past its own size.
const DOWNLOAD_SPARE: u64 = 64 * 1_048_576;

/// A verified download, its progress naming the curated model `model`.
/// Whatever goes wrong — the server, a stalled connection, a full disk, a
/// bad hash — the partial file goes too.
pub(super) async fn download_for(
    url: &str,
    dest: &Path,
    expected_sha: &str,
    stage: &'static str,
    model: &str,
    app: Option<&AppHandle>,
) -> Result<(), String> {
    if crate::shell::e2e::active() && !allowed_in_test_run(url) {
        return Err("Test runs never download models from the internet".to_string());
    }
    let partial = dest.with_extension("partial");
    tracing::info!(url, dest = %dest.display(), stage, "Downloading on-device cleanup asset");
    let result = fetch_to(url, dest, &partial, expected_sha, stage, model, app).await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&partial).await;
    }
    result
}

/// [`download_for`]'s work: `url` → `partial`, verified, renamed to `dest`.
async fn fetch_to(
    url: &str,
    dest: &Path,
    partial: &Path,
    expected_sha: &str,
    stage: &'static str,
    model: &str,
    app: Option<&AppHandle>,
) -> Result<(), String> {
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("request failed: {}", e))?;
    let resp = tokio::time::timeout(DOWNLOAD_STALL, client.get(url).send_for(crate::privacy::net::Purpose::Download(crate::privacy::net::Download::OnDeviceAi)))
        .await
        .map_err(|_| "the server didn't answer".to_string())?
        .map_err(|e| format!("request failed: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {} from {}", resp.status(), url));
    }
    let total = resp.content_length();
    if let Some(total) = total {
        let free = dest.parent().and_then(free_bytes);
        if !room_for(free, total + DOWNLOAD_SPARE) {
            return Err(format!(
                "not enough disk space: it needs {} MB, {} MB are free",
                (total + DOWNLOAD_SPARE) / 1_048_576,
                free.unwrap_or(0) / 1_048_576
            ));
        }
    }

    let mut file = tokio::fs::File::create(partial)
        .await
        .map_err(|e| format!("create temp file: {}", e))?;
    let mut downloaded: u64 = 0;
    let mut last_pct: u8 = 0;
    let mut stream = resp.bytes_stream();
    loop {
        let chunk = match tokio::time::timeout(DOWNLOAD_STALL, stream.next()).await {
            Ok(Some(chunk)) => chunk.map_err(|e| format!("download stream error: {}", e))?,
            Ok(None) => break,
            Err(_) => return Err("the download stalled".to_string()),
        };
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
    let verify_path = partial.to_path_buf();
    let expected = expected_sha.to_string();
    let ok = tokio::task::spawn_blocking(move || match compute_sha256(&verify_path) {
        Ok(actual) => actual.eq_ignore_ascii_case(&expected),
        Err(_) => false,
    })
    .await
    .map_err(|e| format!("sha task panicked: {}", e))?;
    if !ok {
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
