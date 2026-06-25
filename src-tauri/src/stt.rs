//! Speech-to-Text (STT) engine.
//!
//! Provides a multi-engine STT abstraction built on [`transcribe-rs`], which
//! wraps both whisper.cpp (Whisper GGML models, CUDA-accelerated) and ONNX
//! Runtime (Parakeet / Moonshine / SenseVoice / GigaAM / Canary / Cohere,
//! DirectML-accelerated on Windows).
//!
//! Three layers live here:
//! - A static **model registry** (`MODELS`) of the 16 supported models, ported
//!   from Handy. Each entry knows its on-disk filename (a `.bin` file or an
//!   extracted directory), download URL, SHA-256, engine type, and UI metadata.
//! - **Download / verify / extract** (`ensure_model_exists`): streams the model
//!   from `blob.handy.computer`, verifies its SHA-256, and for directory-based
//!   models unpacks the `.tar.gz` into `models/<name>/`.
//! - The **engine layer**: a real `transcribe-rs` implementation behind the
//!   `engines` feature (and `cuda` for GPU whisper), plus a stub fallback for
//!   the default build so `cargo check` stays fast and the pipeline is testable
//!   without compiling whisper.cpp / ONNX Runtime.

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

// ── STT Error ───────────────────────────────────────────────────────

/// Errors that can occur during STT operations.
#[derive(Debug)]
pub enum SttError {
    /// Model file/directory not found at the expected path.
    ModelNotFound(PathBuf),
    /// Failed to load or initialize the model.
    ModelLoadError(String),
    /// Transcription failed during inference.
    TranscriptionError(String),
    /// Audio format is invalid (wrong sample rate, etc.).
    InvalidAudio(String),
    /// Engine is not initialized or ready.
    NotReady,
    /// Model download failed.
    DownloadError(String),
}

impl std::fmt::Display for SttError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ModelNotFound(path) => write!(f, "STT model not found: {}", path.display()),
            Self::ModelLoadError(msg) => write!(f, "STT model load error: {}", msg),
            Self::TranscriptionError(msg) => write!(f, "STT transcription error: {}", msg),
            Self::InvalidAudio(msg) => write!(f, "Invalid audio: {}", msg),
            Self::NotReady => write!(f, "STT engine not ready"),
            Self::DownloadError(msg) => write!(f, "STT model download failed: {}", msg),
        }
    }
}

impl std::error::Error for SttError {}

// ── Model Registry ──────────────────────────────────────────────────

/// Which transcribe-rs engine a model runs on. Whisper models run on
/// whisper.cpp (CUDA); the rest run on ONNX Runtime (DirectML).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum EngineType {
    Whisper,
    Parakeet,
    Moonshine,
    MoonshineStreaming,
    SenseVoice,
    GigaAM,
    Canary,
    Cohere,
}

/// A statically-known downloadable model.
///
/// `filename` is the on-disk name: a `.bin` file for whisper models, or the
/// extracted directory name for ONNX (`is_directory`) models. `url` is the
/// path component appended to [`BASE_URL`] — for files it's the `.bin` name,
/// for directories it's the `.tar.gz` archive name.
struct ModelDescriptor {
    id: &'static str,
    filename: &'static str,
    url: &'static str,
    sha256: &'static str,
    is_directory: bool,
    engine_type: EngineType,
}

/// All model artifacts are served from this host (ported from Handy).
const BASE_URL: &str = "https://blob.handy.computer/";

/// The 16 supported models. The `id` values mirror `src/lib/models.js` on the
/// frontend — they're what the UI passes as `model_size`/`modelSize`.
const MODELS: &[ModelDescriptor] = &[
    // ── File-based Whisper models (.bin) ──
    ModelDescriptor {
        id: "small",
        filename: "ggml-small.bin",
        url: "ggml-small.bin",
        sha256: "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b",
        is_directory: false,
        engine_type: EngineType::Whisper,
    },
    ModelDescriptor {
        id: "medium",
        filename: "whisper-medium-q4_1.bin",
        url: "whisper-medium-q4_1.bin",
        sha256: "79283fc1f9fe12ca3248543fbd54b73292164d8df5a16e095e2bceeaaabddf57",
        is_directory: false,
        engine_type: EngineType::Whisper,
    },
    ModelDescriptor {
        id: "turbo",
        filename: "ggml-large-v3-turbo.bin",
        url: "ggml-large-v3-turbo.bin",
        sha256: "1fc70f774d38eb169993ac391eea357ef47c88757ef72ee5943879b7e8e2bc69",
        is_directory: false,
        engine_type: EngineType::Whisper,
    },
    ModelDescriptor {
        id: "large",
        filename: "ggml-large-v3-q5_0.bin",
        url: "ggml-large-v3-q5_0.bin",
        sha256: "d75795ecff3f83b5faa89d1900604ad8c780abd5739fae406de19f23ecd98ad1",
        is_directory: false,
        engine_type: EngineType::Whisper,
    },
    ModelDescriptor {
        id: "breeze-asr",
        filename: "breeze-asr-q5_k.bin",
        url: "breeze-asr-q5_k.bin",
        sha256: "8efbf0ce8a3f50fe332b7617da787fb81354b358c288b008d3bdef8359df64c6",
        is_directory: false,
        engine_type: EngineType::Whisper,
    },
    // ── Directory-based ONNX models (.tar.gz → extracted dir) ──
    ModelDescriptor {
        id: "parakeet-tdt-0.6b-v2",
        filename: "parakeet-tdt-0.6b-v2-int8",
        url: "parakeet-v2-int8.tar.gz",
        sha256: "ac9b9429984dd565b25097337a887bb7f0f8ac393573661c651f0e7d31563991",
        is_directory: true,
        engine_type: EngineType::Parakeet,
    },
    ModelDescriptor {
        id: "parakeet-tdt-0.6b-v3",
        filename: "parakeet-tdt-0.6b-v3-int8",
        url: "parakeet-v3-int8.tar.gz",
        sha256: "43d37191602727524a7d8c6da0eef11c4ba24320f5b4730f1a2497befc2efa77",
        is_directory: true,
        engine_type: EngineType::Parakeet,
    },
    ModelDescriptor {
        id: "moonshine-base",
        filename: "moonshine-base",
        url: "moonshine-base.tar.gz",
        sha256: "04bf6ab012cfceebd4ac7cf88c1b31d027bbdd3cd704649b692e2e935236b7e8",
        is_directory: true,
        engine_type: EngineType::Moonshine,
    },
    ModelDescriptor {
        id: "moonshine-tiny-streaming-en",
        filename: "moonshine-tiny-streaming-en",
        url: "moonshine-tiny-streaming-en.tar.gz",
        sha256: "465addcfca9e86117415677dfdc98b21edc53537210333a3ecdb58509a80abaf",
        is_directory: true,
        engine_type: EngineType::MoonshineStreaming,
    },
    ModelDescriptor {
        id: "moonshine-small-streaming-en",
        filename: "moonshine-small-streaming-en",
        url: "moonshine-small-streaming-en.tar.gz",
        sha256: "dbb3e1c1832bd88a4ac712f7449a136cc2c9a18c5fe33a12ed1b7cb1cfe9cdd5",
        is_directory: true,
        engine_type: EngineType::MoonshineStreaming,
    },
    ModelDescriptor {
        id: "moonshine-medium-streaming-en",
        filename: "moonshine-medium-streaming-en",
        url: "moonshine-medium-streaming-en.tar.gz",
        sha256: "07a66f3bff1c77e75a2f637e5a263928a08baae3c29c4c053fc968a9a9373d13",
        is_directory: true,
        engine_type: EngineType::MoonshineStreaming,
    },
    ModelDescriptor {
        id: "sense-voice-int8",
        filename: "sense-voice-int8",
        url: "sense-voice-int8.tar.gz",
        sha256: "171d611fe5d353a50bbb741b6f3ef42559b1565685684e9aa888ef563ba3e8a4",
        is_directory: true,
        engine_type: EngineType::SenseVoice,
    },
    ModelDescriptor {
        id: "gigaam-v3-e2e-ctc",
        filename: "giga-am-v3-int8",
        url: "giga-am-v3-int8.tar.gz",
        sha256: "d872462268430db140b69b72e0fc4b787b194c1dbe51b58de39444d55b6da45b",
        is_directory: true,
        engine_type: EngineType::GigaAM,
    },
    ModelDescriptor {
        id: "canary-180m-flash",
        filename: "canary-180m-flash",
        url: "canary-180m-flash.tar.gz",
        sha256: "6d9cfca6118b296e196eaedc1c8fa9788305a7b0f1feafdb6dc91932ab6e53f7",
        is_directory: true,
        engine_type: EngineType::Canary,
    },
    ModelDescriptor {
        id: "canary-1b-v2",
        filename: "canary-1b-v2",
        url: "canary-1b-v2.tar.gz",
        sha256: "02305b2a25f9cf3e7deaffa7f94df00efa44f442cd55c101c2cb9c000f904666",
        is_directory: true,
        engine_type: EngineType::Canary,
    },
    ModelDescriptor {
        id: "cohere-int8",
        filename: "cohere-int8",
        url: "cohere-int8.tar.gz",
        sha256: "ea2257d52434f3644574f187dcdcf666e302cd11b92866116ab8e14cd9c887f0",
        is_directory: true,
        engine_type: EngineType::Cohere,
    },
];

/// Every registry model id, recommended-first-ish (matches the frontend order
/// loosely). Used by `commands::installed_models` to know what to probe.
pub fn all_model_ids() -> Vec<&'static str> {
    MODELS.iter().map(|m| m.id).collect()
}

/// Human-friendly display name for a model id (matches the frontend list).
/// Unknown ids return the id unchanged (legacy/custom models).
pub fn model_name(id: &str) -> String {
    match id {
        "small" => "Whisper Small",
        "medium" => "Whisper Medium",
        "turbo" => "Whisper Large v3 Turbo",
        "large" => "Whisper Large v3",
        "breeze-asr" => "Breeze ASR",
        "parakeet-tdt-0.6b-v2" => "Parakeet V2",
        "parakeet-tdt-0.6b-v3" => "Parakeet V3",
        "moonshine-base" => "Moonshine Base",
        "moonshine-tiny-streaming-en" => "Moonshine V2 Tiny",
        "moonshine-small-streaming-en" => "Moonshine V2 Small",
        "moonshine-medium-streaming-en" => "Moonshine V2 Medium",
        "sense-voice-int8" => "SenseVoice",
        "gigaam-v3-e2e-ctc" => "GigaAM v3",
        "canary-180m-flash" => "Canary 180M Flash",
        "canary-1b-v2" => "Canary 1B v2",
        "cohere-int8" => "Cohere",
        other => other,
    }
    .to_string()
}

// ── Language capabilities (for the Settings UI) ─────────────────────

/// A pragmatic shared dropdown list for the broadly-multilingual models
/// (Whisper / Canary 1B / Cohere). Ported loosely from Handy's larger sets —
/// a common subset is enough for the picker. `"auto"` is added by the UI.
const COMMON_LANGUAGES: &[&str] = &[
    "en", "es", "fr", "de", "it", "pt", "nl", "pl", "ru", "uk", "zh", "ja", "ko",
    "ar", "hi", "tr",
];

/// The selectable language codes for a model id (empty = no language selection).
///
/// Mirrors Handy's per-model capability: Whisper exposes the common multilingual
/// set; SenseVoice and Canary 180M have small fixed sets; Canary 1B and Cohere
/// use the common set. Parakeet (incl. V3), Moonshine*, and GigaAM are fixed /
/// not user-selectable, so they return an empty list.
pub fn model_supported_languages(id: &str) -> Vec<&'static str> {
    match id {
        // Whisper family — broadly multilingual.
        "small" | "medium" | "large" | "turbo" | "breeze-asr" => COMMON_LANGUAGES.to_vec(),
        // SenseVoice — fixed five.
        "sense-voice-int8" => vec!["zh", "en", "ja", "ko", "yue"],
        // Canary 180M Flash — four European languages.
        "canary-180m-flash" => vec!["en", "de", "es", "fr"],
        // Canary 1B / Cohere — broadly multilingual.
        "canary-1b-v2" | "cohere-int8" => COMMON_LANGUAGES.to_vec(),
        // Parakeet (incl. V3), Moonshine*, GigaAM → not selectable.
        _ => Vec::new(),
    }
}

/// Whether a model lets the user pick the spoken language.
pub fn model_supports_language(id: &str) -> bool {
    !model_supported_languages(id).is_empty()
}

/// Whether a model can translate the transcription to English.
///
/// Per Handy: Whisper (except the turbo / breeze variants) and both Canary
/// models. Everything else transcribes in the source language only.
pub fn model_supports_translate(id: &str) -> bool {
    matches!(
        id,
        "small" | "medium" | "large" | "canary-180m-flash" | "canary-1b-v2"
    )
}

/// A model resolved to its on-disk artifact + how to load/download it.
struct ResolvedModel {
    /// On-disk name under `models/`: a `.bin` file or an extracted directory.
    filename: String,
    /// Full download URL, or `None` for legacy/custom models with no source.
    url: Option<String>,
    /// Expected SHA-256, or `None` to skip verification (legacy/custom).
    sha256: Option<String>,
    is_directory: bool,
    engine_type: EngineType,
}

fn find_model(id: &str) -> Option<&'static ModelDescriptor> {
    MODELS.iter().find(|m| m.id == id)
}

/// Resolve a model id to its on-disk artifact and load parameters.
///
/// Registry ids resolve directly. Unknown ids are treated as **legacy/custom
/// Whisper** models so old configs (e.g. `large-v3`) and user-dropped `.bin`
/// files keep working: a few known legacy names map to their historical
/// filenames, anything else falls back to `<id>.bin`.
fn resolve_model(id: &str) -> ResolvedModel {
    if let Some(d) = find_model(id) {
        return ResolvedModel {
            filename: d.filename.to_string(),
            url: Some(format!("{}{}", BASE_URL, d.url)),
            sha256: Some(d.sha256.to_string()),
            is_directory: d.is_directory,
            engine_type: d.engine_type,
        };
    }

    // Legacy ids from Blip's previous (whisper-only) registry — file-based.
    let legacy_filename = match id {
        "large-v3" => Some("ggml-large-v3.bin"),
        "large-v3-turbo" => Some("ggml-large-v3-turbo-q5_0.bin"),
        "base" => Some("ggml-base.en.bin"),
        "tiny" => Some("ggml-tiny.en.bin"),
        _ => None,
    };
    let filename = legacy_filename
        .map(str::to_string)
        .unwrap_or_else(|| format!("{}.bin", id));

    ResolvedModel {
        filename,
        url: None,
        sha256: None,
        is_directory: false,
        engine_type: EngineType::Whisper,
    }
}

/// The on-disk filename (file or directory) for a model id.
pub fn model_filename(id: &str) -> String {
    resolve_model(id).filename
}

/// Whether a model's artifact already exists on disk.
///
/// File-based models check for the `.bin` file; directory-based models check
/// for the extracted directory.
pub fn is_model_installed(data_dir: &Path, id: &str) -> bool {
    let resolved = resolve_model(id);
    let path = data_dir.join("models").join(&resolved.filename);
    if resolved.is_directory {
        path.is_dir()
    } else {
        path.is_file()
    }
}

// ── Model Download / Verify / Extract ───────────────────────────────

/// Progress event emitted during model download.
///
/// `model_size` is the model **id** (serialized as `modelSize`); the frontend
/// matches it against the model it's downloading.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SttDownloadProgress {
    pub model_size: String,
    pub percent: u8,
    pub downloaded_mb: f64,
    pub total_mb: f64,
}

/// Ensure a model's artifact exists on disk, downloading it if needed.
///
/// Streams the model from `blob.handy.computer` to a `.partial` file (emitting
/// `stt-download-progress`), verifies its SHA-256, then either renames the file
/// into place (file-based) or extracts the `.tar.gz` into `models/<name>/`
/// (directory-based). Returns the final artifact path.
///
/// Resume (HTTP Range) is intentionally not implemented yet — a fresh download
/// runs each time the artifact is missing.
pub async fn ensure_model_exists(
    data_dir: &Path,
    model_id: &str,
    app_handle: Option<&AppHandle>,
) -> Result<PathBuf, SttError> {
    let resolved = resolve_model(model_id);
    let models_dir = data_dir.join("models");
    let final_path = models_dir.join(&resolved.filename);

    // Already present?
    let present = if resolved.is_directory {
        final_path.is_dir()
    } else {
        final_path.is_file()
    };
    if present {
        tracing::info!(path = %final_path.display(), "Model already present");
        return Ok(final_path);
    }

    let url = resolved.url.clone().ok_or_else(|| {
        SttError::DownloadError(format!("No download URL for model '{}'", model_id))
    })?;

    tokio::fs::create_dir_all(&models_dir)
        .await
        .map_err(|e| SttError::DownloadError(format!("Failed to create models dir: {}", e)))?;

    // Download to "<filename>.partial" (the partial holds the raw .bin or the
    // raw .tar.gz, depending on model kind).
    let partial_path = models_dir.join(format!("{}.partial", &resolved.filename));

    tracing::info!(url = %url, dest = %partial_path.display(), model = %model_id, "Downloading model");

    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| SttError::DownloadError(format!("HTTP request failed: {}", e)))?;

    if !resp.status().is_success() {
        return Err(SttError::DownloadError(format!(
            "HTTP {} from {}",
            resp.status(),
            url
        )));
    }

    let total_size = resp.content_length();

    let mut file = tokio::fs::File::create(&partial_path)
        .await
        .map_err(|e| SttError::DownloadError(format!("Failed to create temp file: {}", e)))?;

    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

    let mut downloaded: u64 = 0;
    let mut last_progress: u8 = 0;
    let mut stream = resp.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk =
            chunk.map_err(|e| SttError::DownloadError(format!("Download stream error: {}", e)))?;
        file.write_all(&chunk)
            .await
            .map_err(|e| SttError::DownloadError(format!("Write error: {}", e)))?;
        downloaded += chunk.len() as u64;

        if let Some(total) = total_size {
            let pct = ((downloaded as f64 / total as f64) * 100.0) as u8;
            if pct >= last_progress + 5 {
                last_progress = pct;
                let downloaded_mb = downloaded as f64 / 1_048_576.0;
                let total_mb = total as f64 / 1_048_576.0;
                tracing::info!(
                    "Downloading {} model... {}% ({:.1} MB / {:.1} MB)",
                    model_id, pct, downloaded_mb, total_mb
                );
                if let Some(handle) = app_handle {
                    let _ = handle.emit(
                        "stt-download-progress",
                        SttDownloadProgress {
                            model_size: model_id.to_string(),
                            percent: pct,
                            downloaded_mb,
                            total_mb,
                        },
                    );
                }
            }
        }
    }

    file.flush()
        .await
        .map_err(|e| SttError::DownloadError(format!("Flush error: {}", e)))?;
    drop(file);

    // Verify SHA-256 on a blocking thread (files can be ~1.6 GB).
    if let Some(expected) = resolved.sha256.clone() {
        let verify_path = partial_path.clone();
        let model_id_owned = model_id.to_string();
        let ok = tokio::task::spawn_blocking(move || {
            match compute_sha256(&verify_path) {
                Ok(actual) => {
                    if actual == expected {
                        true
                    } else {
                        tracing::warn!(
                            model = %model_id_owned,
                            expected = %expected,
                            actual = %actual,
                            "SHA-256 mismatch"
                        );
                        false
                    }
                }
                Err(e) => {
                    tracing::warn!(model = %model_id_owned, "SHA-256 read error: {}", e);
                    false
                }
            }
        })
        .await
        .map_err(|e| SttError::DownloadError(format!("SHA-256 task panicked: {}", e)))?;

        if !ok {
            let _ = tokio::fs::remove_file(&partial_path).await;
            return Err(SttError::DownloadError(format!(
                "Download verification failed for '{}': file is corrupt. Please retry.",
                model_id
            )));
        }
        tracing::info!(model = %model_id, "SHA-256 verified");
    }

    if resolved.is_directory {
        extract_archive(&partial_path, &final_path, model_id)?;
        let _ = tokio::fs::remove_file(&partial_path).await;
    } else {
        tokio::fs::rename(&partial_path, &final_path)
            .await
            .map_err(|e| SttError::DownloadError(format!("Rename failed: {}", e)))?;
    }

    tracing::info!(path = %final_path.display(), model = %model_id, "Model ready");
    Ok(final_path)
}

/// Extract a `.tar.gz` archive into `final_dir`.
///
/// Unpacks into a temporary `<name>.extracting/` dir, then if the archive
/// contained a single nested directory promotes that to `final_dir`, otherwise
/// renames the temp dir itself. Mirrors Handy's atomic-extraction approach.
fn extract_archive(
    archive_path: &Path,
    final_dir: &Path,
    model_id: &str,
) -> Result<(), SttError> {
    use flate2::read::GzDecoder;
    use std::fs::{self, File};
    use tar::Archive;

    let parent = final_dir
        .parent()
        .ok_or_else(|| SttError::DownloadError("models dir has no parent".into()))?;
    let file_name = final_dir
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| SttError::DownloadError("bad model dir name".into()))?;
    let temp_extract_dir = parent.join(format!("{}.extracting", file_name));

    fn io_err(what: &str, e: std::io::Error) -> SttError {
        SttError::DownloadError(format!("{}: {}", what, e))
    }

    if temp_extract_dir.exists() {
        let _ = fs::remove_dir_all(&temp_extract_dir);
    }
    fs::create_dir_all(&temp_extract_dir).map_err(|e| io_err("create extract dir", e))?;

    tracing::info!(model = %model_id, "Extracting archive");
    let tar_gz = File::open(archive_path).map_err(|e| io_err("open archive", e))?;
    let mut archive = Archive::new(GzDecoder::new(tar_gz));
    if let Err(e) = archive.unpack(&temp_extract_dir) {
        let _ = fs::remove_dir_all(&temp_extract_dir);
        let _ = fs::remove_file(archive_path);
        return Err(SttError::DownloadError(format!(
            "Failed to extract archive: {}",
            e
        )));
    }

    // Find the directories the archive unpacked. We count ONLY directories and
    // ignore stray files (these macOS-built tarballs include AppleDouble `._*`
    // junk at the top level): if there's exactly one real directory, the archive
    // was wrapped in a single folder — promote that folder's contents to
    // `final_dir`. Otherwise the files sit at the top level, so use the temp dir.
    let nested_dirs: Vec<PathBuf> = fs::read_dir(&temp_extract_dir)
        .map_err(|e| io_err("read extract dir", e))?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
        .map(|e| e.path())
        .filter(|p| {
            // Skip AppleDouble directory shadows just in case.
            p.file_name()
                .and_then(|s| s.to_str())
                .map(|n| !n.starts_with("._"))
                .unwrap_or(true)
        })
        .collect();

    if final_dir.exists() {
        let _ = fs::remove_dir_all(final_dir);
    }

    if nested_dirs.len() == 1 {
        fs::rename(&nested_dirs[0], final_dir).map_err(|e| io_err("move extracted dir", e))?;
        let _ = fs::remove_dir_all(&temp_extract_dir);
    } else {
        fs::rename(&temp_extract_dir, final_dir).map_err(|e| io_err("rename extract dir", e))?;
    }

    tracing::info!(model = %model_id, dir = %final_dir.display(), "Archive extracted");
    Ok(())
}

/// Compute the SHA-256 hex digest of a file (64 KB chunks for large models).
fn compute_sha256(path: &Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;

    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

// ── STT Engine Trait ────────────────────────────────────────────────

/// Common trait for all Speech-to-Text engines.
///
/// Implementations must be Send + Sync so the warm engine can be moved into a
/// blocking task and shared across the pipeline.
pub trait SttEngine: Send + Sync {
    /// Transcribe 16kHz mono f32 audio to text.
    ///
    /// `language` is the BCP-47-ish code to force (e.g. "en", "fr"), or `None`
    /// for auto-detect. `translate` requests translation to English. Both are
    /// applied per-engine — engines without language/translate support ignore
    /// them (see [`model_supports_language`] / [`model_supports_translate`]).
    fn transcribe(
        &self,
        audio: &[f32],
        language: Option<&str>,
        translate: bool,
    ) -> Result<String, SttError>;

    /// Process a streaming audio chunk and return a partial transcript when
    /// enough audio has accumulated. Default: no partials (returns `None`).
    fn transcribe_streaming(&self, _audio_chunk: &[f32]) -> Result<Option<String>, SttError> {
        Ok(None)
    }

    /// Engine name for display/logging.
    fn name(&self) -> &str;

    /// Whether the engine is ready to process audio.
    fn is_ready(&self) -> bool;
}

/// Skip audio shorter than this (100ms at 16kHz) — too short to transcribe
/// and a common source of hallucinated output on silence.
const MIN_SAMPLES: usize = 1_600;

fn engine_type_name(engine_type: EngineType) -> &'static str {
    match engine_type {
        EngineType::Whisper => "whisper",
        EngineType::Parakeet => "parakeet",
        EngineType::Moonshine => "moonshine",
        EngineType::MoonshineStreaming => "moonshine-streaming",
        EngineType::SenseVoice => "sense-voice",
        EngineType::GigaAM => "gigaam",
        EngineType::Canary => "canary",
        EngineType::Cohere => "cohere",
    }
}

// ── Real engine (transcribe-rs) ─────────────────────────────────────

#[cfg(feature = "engines")]
mod engine_real {
    use super::*;
    use std::sync::Mutex;
    use transcribe_rs::{
        onnx::{
            canary::CanaryModel,
            cohere::CohereModel,
            gigaam::GigaAMModel,
            moonshine::{MoonshineModel, MoonshineVariant, StreamingModel},
            parakeet::{ParakeetModel, ParakeetParams, TimestampGranularity},
            sense_voice::{SenseVoiceModel, SenseVoiceParams},
            Quantization,
        },
        whisper_cpp::{WhisperEngine, WhisperInferenceParams},
        SpeechModel, TranscribeOptions,
    };

    /// The loaded transcribe-rs model, kept warm and reused across calls.
    enum LoadedEngine {
        Whisper(WhisperEngine),
        Parakeet(ParakeetModel),
        Moonshine(MoonshineModel),
        MoonshineStreaming(StreamingModel),
        SenseVoice(SenseVoiceModel),
        GigaAM(GigaAMModel),
        Canary(CanaryModel),
        Cohere(CohereModel),
    }

    /// A loaded multi-engine STT model.
    ///
    /// The inner model is held behind a `Mutex` because the transcribe-rs
    /// inference calls take `&mut self`, while our pipeline shares the engine
    /// via `&self` (it's taken out, run on a blocking task, and put back warm).
    pub struct RealEngine {
        inner: Mutex<LoadedEngine>,
        name: &'static str,
    }

    impl RealEngine {
        /// Load `model_path` (a `.bin` file for Whisper, an extracted directory
        /// for ONNX models) using the engine implied by `engine_type`.
        ///
        /// Whisper reads the global accelerator atomics set by
        /// [`super::apply_accelerator_settings`]; ONNX models load at INT8.
        pub fn load(model_path: &Path, engine_type: EngineType) -> Result<Self, SttError> {
            if !model_path.exists() {
                return Err(SttError::ModelNotFound(model_path.to_path_buf()));
            }

            let load_err = |e: transcribe_rs::TranscribeError| {
                SttError::ModelLoadError(format!("Failed to load {} model: {}", engine_type_name(engine_type), e))
            };

            let loaded = match engine_type {
                EngineType::Whisper => {
                    LoadedEngine::Whisper(WhisperEngine::load(model_path).map_err(load_err)?)
                }
                EngineType::Parakeet => LoadedEngine::Parakeet(
                    ParakeetModel::load(model_path, &Quantization::Int8).map_err(load_err)?,
                ),
                EngineType::Moonshine => LoadedEngine::Moonshine(
                    MoonshineModel::load(model_path, MoonshineVariant::Base, &Quantization::default())
                        .map_err(load_err)?,
                ),
                EngineType::MoonshineStreaming => LoadedEngine::MoonshineStreaming(
                    StreamingModel::load(model_path, 0, &Quantization::default()).map_err(load_err)?,
                ),
                EngineType::SenseVoice => LoadedEngine::SenseVoice(
                    SenseVoiceModel::load(model_path, &Quantization::Int8).map_err(load_err)?,
                ),
                EngineType::GigaAM => LoadedEngine::GigaAM(
                    GigaAMModel::load(model_path, &Quantization::Int8).map_err(load_err)?,
                ),
                EngineType::Canary => LoadedEngine::Canary(
                    CanaryModel::load(model_path, &Quantization::Int8).map_err(load_err)?,
                ),
                EngineType::Cohere => LoadedEngine::Cohere(
                    CohereModel::load(model_path, &Quantization::Int8).map_err(load_err)?,
                ),
            };

            tracing::info!(
                model_path = %model_path.display(),
                engine = engine_type_name(engine_type),
                "STT engine loaded (transcribe-rs)"
            );

            Ok(Self {
                inner: Mutex::new(loaded),
                name: engine_type_name(engine_type),
            })
        }
    }

    impl SttEngine for RealEngine {
        fn transcribe(
            &self,
            audio: &[f32],
            language: Option<&str>,
            translate: bool,
        ) -> Result<String, SttError> {
            if audio.is_empty() {
                return Ok(String::new());
            }
            if audio.len() < MIN_SAMPLES {
                tracing::debug!(samples = audio.len(), "Audio too short, skipping");
                return Ok(String::new());
            }

            let duration_secs = audio.len() as f64 / 16_000.0;
            tracing::info!(
                samples = audio.len(),
                duration_secs = format!("{:.2}", duration_secs),
                engine = self.name,
                "Running transcription"
            );

            let mut engine = self
                .inner
                .lock()
                .map_err(|e| SttError::TranscriptionError(format!("engine mutex poisoned: {}", e)))?;

            let tx_err = |e: transcribe_rs::TranscribeError| {
                SttError::TranscriptionError(format!("Transcription failed: {}", e))
            };

            // Apply language + translate per engine, mirroring Handy's
            // `managers/transcription.rs`. `language == None` means auto-detect;
            // engines that don't support selection ignore it. `"zh-Hans"` /
            // `"zh-Hant"` collapse to `"zh"` for the engines that expect it.
            let result = match &mut *engine {
                LoadedEngine::Whisper(e) => {
                    let lang = language.map(|l| match l {
                        "zh-Hans" | "zh-Hant" => "zh".to_string(),
                        other => other.to_string(),
                    });
                    let params = WhisperInferenceParams {
                        language: lang,
                        translate,
                        ..Default::default()
                    };
                    e.transcribe_with(audio, &params).map_err(tx_err)?
                }
                LoadedEngine::Parakeet(e) => {
                    let params = ParakeetParams {
                        timestamp_granularity: Some(TimestampGranularity::Segment),
                        ..Default::default()
                    };
                    e.transcribe_with(audio, &params).map_err(tx_err)?
                }
                LoadedEngine::Moonshine(e) => {
                    e.transcribe(audio, &TranscribeOptions::default()).map_err(tx_err)?
                }
                LoadedEngine::MoonshineStreaming(e) => {
                    e.transcribe(audio, &TranscribeOptions::default()).map_err(tx_err)?
                }
                LoadedEngine::SenseVoice(e) => {
                    let lang = match language {
                        Some("zh") | Some("zh-Hans") | Some("zh-Hant") => Some("zh".to_string()),
                        Some("en") => Some("en".to_string()),
                        Some("ja") => Some("ja".to_string()),
                        Some("ko") => Some("ko".to_string()),
                        Some("yue") => Some("yue".to_string()),
                        _ => None,
                    };
                    let params = SenseVoiceParams {
                        language: lang,
                        use_itn: Some(true),
                    };
                    e.transcribe_with(audio, &params).map_err(tx_err)?
                }
                LoadedEngine::GigaAM(e) => {
                    e.transcribe(audio, &TranscribeOptions::default()).map_err(tx_err)?
                }
                LoadedEngine::Canary(e) => {
                    let options = TranscribeOptions {
                        language: language.map(|l| l.to_string()),
                        translate,
                        ..Default::default()
                    };
                    e.transcribe(audio, &options).map_err(tx_err)?
                }
                LoadedEngine::Cohere(e) => {
                    let lang = language.map(|l| match l {
                        "zh-Hans" | "zh-Hant" => "zh".to_string(),
                        other => other.to_string(),
                    });
                    let options = TranscribeOptions {
                        language: lang,
                        ..Default::default()
                    };
                    e.transcribe(audio, &options).map_err(tx_err)?
                }
            };

            let text = result.text.trim().to_string();
            tracing::info!(text_len = text.len(), "Transcription complete");
            Ok(text)
        }

        fn name(&self) -> &str {
            self.name
        }

        fn is_ready(&self) -> bool {
            true
        }
    }
}

// ── Stub engine (default build) ─────────────────────────────────────

#[cfg(not(feature = "engines"))]
mod engine_stub {
    use super::*;

    /// Stub STT engine used when the `engines` feature is disabled.
    ///
    /// Returns placeholder text so the full pipeline (hotkey → capture →
    /// inject) can be exercised without compiling whisper.cpp / ONNX Runtime.
    pub struct StubEngine {
        engine: &'static str,
    }

    impl StubEngine {
        pub fn load(model_path: &Path, engine_type: EngineType) -> Result<Self, SttError> {
            tracing::info!(
                model_path = %model_path.display(),
                engine = engine_type_name(engine_type),
                "STT engine created (STUB — no real inference)"
            );
            Ok(Self {
                engine: engine_type_name(engine_type),
            })
        }
    }

    impl SttEngine for StubEngine {
        fn transcribe(
            &self,
            audio: &[f32],
            _language: Option<&str>,
            _translate: bool,
        ) -> Result<String, SttError> {
            if audio.is_empty() {
                return Ok(String::new());
            }
            if audio.len() < MIN_SAMPLES {
                return Err(SttError::InvalidAudio(format!(
                    "Audio too short: {} samples ({:.1}ms). Need at least 100ms.",
                    audio.len(),
                    audio.len() as f64 / 16.0
                )));
            }
            let duration_secs = audio.len() as f64 / 16_000.0;
            tracing::info!(
                samples = audio.len(),
                duration_secs = format!("{:.2}", duration_secs),
                engine = self.engine,
                "StubEngine.transcribe() called"
            );
            Ok(format!(
                "[STT stub: received {:.1}s of audio, engine={}]",
                duration_secs, self.engine
            ))
        }

        fn name(&self) -> &str {
            "stub"
        }

        fn is_ready(&self) -> bool {
            true
        }
    }
}

// ── Active engine selection ─────────────────────────────────────────

#[cfg(feature = "engines")]
use engine_real::RealEngine as ActiveEngine;

#[cfg(not(feature = "engines"))]
use engine_stub::StubEngine as ActiveEngine;

/// Wrapper over the active (real or stub) engine that the pipeline holds warm.
pub struct SttAdapter {
    engine: ActiveEngine,
}

impl SttAdapter {
    /// Transcribe audio using the underlying engine. `language` (`None` =
    /// auto-detect) and `translate` are applied per engine; see
    /// [`SttEngine::transcribe`].
    pub fn transcribe(
        &self,
        audio: &[f32],
        language: Option<&str>,
        translate: bool,
    ) -> Result<String, SttError> {
        self.engine.transcribe(audio, language, translate)
    }

    /// Process a streaming audio chunk (returns a partial transcript or `None`).
    pub fn transcribe_streaming(&self, audio_chunk: &[f32]) -> Result<Option<String>, SttError> {
        self.engine.transcribe_streaming(audio_chunk)
    }

    /// Engine name (e.g. "whisper", "parakeet", "stub").
    pub fn name(&self) -> &str {
        self.engine.name()
    }

    /// Whether the engine is ready.
    pub fn is_ready(&self) -> bool {
        self.engine.is_ready()
    }
}

/// Build an STT engine for `model_id`, resolving its artifact under `data_dir`.
///
/// Errors with [`SttError::ModelNotFound`] if the artifact isn't on disk yet
/// (the caller downloads first via [`ensure_model_exists`]).
///
/// `use_gpu` is currently advisory: GPU selection is driven by the global
/// transcribe-rs accelerator atomics set in [`apply_accelerator_settings`].
pub fn create_stt_engine(
    data_dir: &Path,
    model_id: &str,
    _use_gpu: bool,
) -> Result<SttAdapter, SttError> {
    let resolved = resolve_model(model_id);
    let model_path = data_dir.join("models").join(&resolved.filename);
    let engine = ActiveEngine::load(&model_path, resolved.engine_type)?;
    Ok(SttAdapter { engine })
}

// ── Accelerator setup ───────────────────────────────────────────────

/// Apply Blip's fixed accelerator policy to the transcribe-rs global atomics:
/// **whisper → CUDA** (Auto picks the CUDA build), **ONNX → DirectML**.
///
/// Must be called once on startup, before any model loads. No-op (compiled
/// away) in the stub build.
#[cfg(feature = "engines")]
pub fn apply_accelerator_settings(use_gpu: bool) {
    use transcribe_rs::accel;

    let whisper = if use_gpu {
        accel::WhisperAccelerator::Auto
    } else {
        accel::WhisperAccelerator::CpuOnly
    };
    accel::set_whisper_accelerator(whisper);

    // ONNX always targets DirectML on Windows (GPU via DX12, no CUDA-version
    // matching). transcribe-rs falls back to CPU if DirectML is unavailable.
    accel::set_ort_accelerator(accel::OrtAccelerator::DirectMl);

    tracing::info!(use_gpu, "transcribe-rs accelerators set (whisper=Auto/Cpu, ort=DirectMl)");
}

#[cfg(not(feature = "engines"))]
pub fn apply_accelerator_settings(_use_gpu: bool) {}

/// `ort` drops a 0-byte `DirectML.dll` next to our binary on every build. On
/// Windows the executable's own directory is searched before System32, so that
/// empty stub *shadows* the real ~10 MB DirectML and makes every ONNX model
/// fail to run. Delete the stub on startup (before any model loads) so the
/// loader falls back to the real system DirectML. No-op in the stub build or
/// off Windows.
#[cfg(all(feature = "engines", target_os = "windows"))]
pub fn fix_directml_stub() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let Some(dml) = exe.parent().map(|d| d.join("DirectML.dll")) else {
        return;
    };
    if std::fs::metadata(&dml).map(|m| m.len() == 0).unwrap_or(false) {
        match std::fs::remove_file(&dml) {
            Ok(_) => tracing::info!("Removed 0-byte DirectML.dll stub; using system DirectML"),
            Err(e) => tracing::warn!("Could not remove 0-byte DirectML.dll stub: {}", e),
        }
    }
}

#[cfg(not(all(feature = "engines", target_os = "windows")))]
pub fn fix_directml_stub() {}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_has_16_models() {
        assert_eq!(MODELS.len(), 16);
    }

    /// End-to-end ONNX path check: downloads the smallest ONNX model
    /// (Moonshine V2 Tiny, ~31MB), loads it via ort/DirectML, and runs
    /// inference on 1s of silence. Proves the ONNX Runtime is actually
    /// linked + the DirectML provider resolves. Network + heavy → ignored.
    /// Run: cargo test --features cuda --lib onnx_moonshine -- --ignored --nocapture
    #[cfg(feature = "engines")]
    #[test]
    #[ignore = "downloads ~55MB and initializes ONNX Runtime; run manually"]
    fn onnx_moonshine_base_loads_and_transcribes() {
        // Moonshine Base = non-streaming ONNX model, ~55MB. We feed ~3s of a
        // real (non-degenerate) sine tone rather than pure silence, since a
        // zero-energy buffer can collapse an intermediate tensor to length 0
        // and trip DirectML's Slice op. We only assert it runs without error
        // (the transcript of a tone is meaningless).
        let data_dir = crate::config::data_dir();
        apply_accelerator_settings(true);
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            ensure_model_exists(&data_dir, "moonshine-base", None)
                .await
                .expect("download/extract moonshine-base");
        });
        let engine = create_stt_engine(&data_dir, "moonshine-base", true)
            .expect("load moonshine-base via ort/DirectML");
        // 3 seconds of a quiet 220 Hz tone at 16 kHz.
        let sr = 16_000usize;
        let audio: Vec<f32> = (0..sr * 3)
            .map(|i| 0.1 * (std::f32::consts::TAU * 220.0 * i as f32 / sr as f32).sin())
            .collect();
        let text = engine
            .transcribe(&audio, None, false)
            .expect("transcribe on ONNX engine should not error");
        eprintln!("ONNX smoke transcript: {:?}", text);
    }

    #[test]
    fn registry_ids_are_unique() {
        let mut ids: Vec<&str> = MODELS.iter().map(|m| m.id).collect();
        ids.sort_unstable();
        let count = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), count, "duplicate model id in registry");
    }

    #[test]
    fn recommended_default_is_parakeet_v3() {
        let m = find_model("parakeet-tdt-0.6b-v3").expect("default model present");
        assert!(m.is_directory);
        assert_eq!(m.engine_type, EngineType::Parakeet);
    }

    #[test]
    fn resolve_file_based_model() {
        let r = resolve_model("small");
        assert_eq!(r.filename, "ggml-small.bin");
        assert!(!r.is_directory);
        assert_eq!(r.engine_type, EngineType::Whisper);
        assert!(r.url.as_deref().unwrap().ends_with("ggml-small.bin"));
        assert!(r.sha256.is_some());
    }

    #[test]
    fn resolve_directory_based_model() {
        let r = resolve_model("gigaam-v3-e2e-ctc");
        assert_eq!(r.filename, "giga-am-v3-int8"); // dir name differs from id
        assert!(r.is_directory);
        assert_eq!(r.engine_type, EngineType::GigaAM);
        assert!(r.url.as_deref().unwrap().ends_with("giga-am-v3-int8.tar.gz"));
    }

    #[test]
    fn resolve_legacy_id_keeps_old_filename() {
        let r = resolve_model("large-v3");
        assert_eq!(r.filename, "ggml-large-v3.bin");
        assert!(!r.is_directory);
        assert_eq!(r.engine_type, EngineType::Whisper);
        assert!(r.url.is_none(), "legacy ids have no registry URL");
    }

    #[test]
    fn resolve_unknown_id_falls_back_to_bin() {
        let r = resolve_model("my-custom-model");
        assert_eq!(r.filename, "my-custom-model.bin");
        assert!(!r.is_directory);
        assert_eq!(r.engine_type, EngineType::Whisper);
    }

    #[test]
    fn url_uses_base_host() {
        let r = resolve_model("parakeet-tdt-0.6b-v3");
        assert_eq!(
            r.url.unwrap(),
            "https://blob.handy.computer/parakeet-v3-int8.tar.gz"
        );
    }

    #[test]
    fn all_model_ids_matches_registry() {
        assert_eq!(all_model_ids().len(), MODELS.len());
    }

    // The real engine errors when the artifact is missing; the stub loads
    // regardless (so the pipeline still runs without a model), so this only
    // holds with `engines` enabled.
    #[cfg(feature = "engines")]
    #[test]
    fn missing_model_errors() {
        let data_dir = std::env::temp_dir().join("blip-stt-test-missing");
        let result = create_stt_engine(&data_dir, "parakeet-tdt-0.6b-v3", false);
        assert!(result.is_err());
    }

    // Stub-only behavior checks.
    #[cfg(not(feature = "engines"))]
    mod stub_tests {
        use super::*;

        #[test]
        fn stub_transcribe_returns_placeholder() {
            let engine = ActiveEngine::load(Path::new("models/whatever"), EngineType::Parakeet)
                .unwrap();
            let adapter = SttAdapter { engine };
            let audio = vec![0.1f32; 16_000];
            let out = adapter.transcribe(&audio, None, false).unwrap();
            assert!(out.contains("STT stub"));
            assert!(adapter.is_ready());
            assert_eq!(adapter.name(), "stub");
        }

        #[test]
        fn stub_empty_audio_is_empty() {
            let engine =
                ActiveEngine::load(Path::new("models/whatever"), EngineType::Whisper).unwrap();
            let adapter = SttAdapter { engine };
            assert!(adapter.transcribe(&[], None, false).unwrap().is_empty());
        }

        #[test]
        fn stub_short_audio_errors() {
            let engine =
                ActiveEngine::load(Path::new("models/whatever"), EngineType::Whisper).unwrap();
            let adapter = SttAdapter { engine };
            assert!(adapter.transcribe(&[0.1f32; 100], None, false).is_err());
        }
    }

    #[test]
    fn stt_error_display() {
        assert!(SttError::ModelNotFound(PathBuf::from("/x")).to_string().contains("not found"));
        assert!(SttError::ModelLoadError("m".into()).to_string().contains("load error"));
        assert!(SttError::TranscriptionError("t".into()).to_string().contains("transcription error"));
        assert!(SttError::InvalidAudio("a".into()).to_string().contains("Invalid audio"));
        assert!(SttError::NotReady.to_string().contains("not ready"));
        assert!(SttError::DownloadError("d".into()).to_string().contains("download failed"));
    }
}
