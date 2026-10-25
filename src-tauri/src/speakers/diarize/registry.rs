//! The diarization models Yap can install, the runtime each one runs on,
//! where they come from (pinned by size and SHA-256) and how they're laid
//! out in `<data>/models/diarization/`:
//!
//! ```text
//! models/diarization/
//!   nemotron-3-diarization/        Nemotron-3-Diarization.q8_0.gguf, NOTICE.txt
//!   nemo-speech-0.2.0-cpu/         the C API DLLs + their ggml and MSVC
//!                                  runtime, licenses/, NOTICE.txt
//!   eres2net-voxceleb-en/          the voice-matching model (voiceprint.rs),
//!                                  NOTICE.txt
//! ```
//!
//! A model's **extras** come with it but don't decide whether it's
//! installed: Nemotron's is the voice-matching model, which a speaker model
//! installed before it existed fetches on its own (Settings' "Download voice
//! matching", `list`'s `extras_bytes`).
//!
//! Install = download each file (resuming a partial one), check its size and
//! SHA-256, unpack into `<folder>.installing/`, then rename that into place:
//! a folder is either whole or not there.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// How an artifact goes into its package folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unpack {
    /// Saved as this file name.
    File(&'static str),
    /// NeMo-Speech.cpp's release zip: its DLLs and licences
    /// ([`runtime_entry`]).
    NemoRuntimeZip,
    /// Not installable yet (a model listed as coming later).
    NotYet,
}

pub struct Artifact {
    pub url: &'static str,
    pub sha256: &'static str,
    pub size: u64,
    pub unpack: Unpack,
}

/// Files installed together into one folder.
pub struct Package {
    pub id: &'static str,
    pub folder: &'static str,
    pub artifacts: &'static [Artifact],
    /// What must be in the folder for it to count as installed.
    pub required: &'static [&'static str],
    /// Written to the folder as NOTICE.txt.
    pub notice: &'static str,
}

/// A diarization model as Settings lists it.
pub struct Model {
    pub id: &'static str,
    pub name: &'static str,
    pub vendor: &'static str,
    /// Its family, for the brand icon (`providerIcons.js`).
    pub engine: &'static str,
    pub desc: &'static str,
    /// Most voices it tells apart.
    pub speakers: u8,
    pub recommended: bool,
    /// Downloadable and wired to an engine.
    pub available: bool,
    /// Shown instead of Download while it isn't.
    pub note: Option<&'static str>,
    pub license: &'static str,
    pub license_url: &'static str,
    pub source_url: &'static str,
    pub runtime_name: &'static str,
    pub runtime_license: &'static str,
    pub runtime_license_url: &'static str,
    pub weights: &'static Package,
    pub runtime: &'static Package,
    /// Installed with it, never required by it (see the module's notes).
    pub extras: &'static [&'static Package],
    /// The engine's file in `weights`.
    pub weights_file: &'static str,
    /// NeMo-Speech.cpp's streaming geometry: "v3-offline" = 30.4 s chunks,
    /// which on a CPU costs ~2.4% of real time on 4 threads, with labels
    /// ~10 s behind the audio (the spike's §4.4–4.5).
    pub preset: &'static str,
}

/// The Hugging Face commit the spike downloaded (24 Sep 2026, "add: q8_0
/// GGUF for NeMo-Speech.cpp"); the URL below spells it out.
#[cfg(test)]
const NEMOTRON_COMMIT: &str = "f667ed73aee57d40cc39428eb768b4fd87a0a29e";

pub static NEMOTRON_WEIGHTS: Package = Package {
    id: "nemotron-3-diarization",
    folder: "nemotron-3-diarization",
    artifacts: &[Artifact {
        url: "https://huggingface.co/nvidia/Nemotron-3-Diarization/resolve/f667ed73aee57d40cc39428eb768b4fd87a0a29e/Nemotron-3-Diarization.q8_0.gguf",
        sha256: "08456d9e22cd9a323c0364d98375f3746d6e68507ebb705cd46438c534c7a3a1",
        size: 107_012_128,
        unpack: Unpack::File("Nemotron-3-Diarization.q8_0.gguf"),
    }],
    required: &["Nemotron-3-Diarization.q8_0.gguf"],
    notice: "Nemotron-3-Diarization (q8_0 GGUF)\n\
             Copyright (c) 2026 NVIDIA Corporation & Affiliates.\n\
             Downloaded from https://huggingface.co/nvidia/Nemotron-3-Diarization\n\
             at commit f667ed73aee57d40cc39428eb768b4fd87a0a29e.\n\
             Use of this model is governed by the OpenMDW License Agreement,\n\
             version 1.1: https://openmdw.ai/license/1-1/\n",
};

pub static NEMO_RUNTIME: Package = Package {
    id: "nemo-speech-0.2.0-cpu",
    folder: "nemo-speech-0.2.0-cpu",
    artifacts: &[Artifact {
        url: "https://github.com/NVIDIA/NeMo-Speech.cpp/releases/download/v0.2.0/nemo-speech-0.2.0-windows-x86_64-cpu.zip",
        sha256: "82c80451086e86194aba9af19f800da77cd66d1c28d993d0efceffd6cdb49bb8",
        size: 5_528_418,
        unpack: Unpack::NemoRuntimeZip,
    }],
    required: &[
        "nemo_speech_asr_c.dll",
        "nemo_speech_asr.dll",
        "ggml.dll",
        "ggml-base.dll",
        "ggml-cpu.dll",
    ],
    notice: "NeMo-Speech.cpp 0.2.0 (Windows x86_64 CPU build)\n\
             Copyright (c) 2026 NVIDIA Corporation & Affiliates.\n\
             Licensed under the Apache License, Version 2.0; it bundles ggml\n\
             (MIT) and the Microsoft Visual C++ runtime. The licence and the\n\
             third-party notices that came with it are in licenses/.\n\
             Downloaded from https://github.com/NVIDIA/NeMo-Speech.cpp/releases/tag/v0.2.0\n",
};

/// **Voice matching** (`speakers/voiceprint.rs`): 3D-Speaker's ERes2Net
/// (VoxCeleb, English), k2-fsa's ONNX export — the embedding the spike chose
/// (§4.1: the best of four on these meetings; it separates voices Nemotron
/// merges). Run on Yap's own ONNX Runtime, so no sherpa: one file.
pub static VOICE_MATCHING: Package = Package {
    id: "eres2net-voxceleb-en",
    folder: "eres2net-voxceleb-en",
    artifacts: &[Artifact {
        // "recongition": the release's own spelling.
        url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_eres2net_sv_en_voxceleb_16k.onnx",
        sha256: "c59158379255ad66e161679cca6af8d52d51e389e3224ab7d7a7baae295c2db5",
        size: 26_485_263,
        unpack: Unpack::File(VOICE_MATCHING_FILE),
    }],
    required: &[VOICE_MATCHING_FILE],
    notice: "3D-Speaker ERes2Net, English (VoxCeleb), k2-fsa's ONNX export.\n\
             3D-Speaker (Alibaba's speech lab), licensed under the Apache\n\
             License, Version 2.0: https://github.com/modelscope/3D-Speaker\n\
             Trained on VoxCeleb (CC BY 4.0): https://www.robots.ox.ac.uk/~vgg/data/voxceleb/\n\
             Downloaded from https://github.com/k2-fsa/sherpa-onnx/releases/tag/speaker-recongition-models\n",
};

/// The voice-matching model's file.
pub const VOICE_MATCHING_FILE: &str = "3dspeaker_speech_eres2net_sv_en_voxceleb_16k.onnx";

/// The fallback the spike recommends for calls where everyone is on a phone
/// line (Nemotron fails on all-narrowband audio): sherpa-onnx's offline
/// pipeline. Listed, pinned, not wired to an engine yet.
pub static SHERPA_WEIGHTS: Package = Package {
    id: "sherpa-pyannote-eres2net",
    folder: "sherpa-pyannote-eres2net",
    artifacts: &[
        Artifact {
            url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-segmentation-models/sherpa-onnx-pyannote-segmentation-3-0.tar.bz2",
            sha256: "24615ee884c897d9d2ba09bb4d30da6bb1b15e685065962db5b02e76e4996488",
            size: 6_958_444,
            unpack: Unpack::NotYet,
        },
        Artifact {
            // "recongition": the release's own spelling.
            url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_eres2net_sv_en_voxceleb_16k.onnx",
            sha256: "c59158379255ad66e161679cca6af8d52d51e389e3224ab7d7a7baae295c2db5",
            size: 26_485_263,
            unpack: Unpack::File("3dspeaker_speech_eres2net_sv_en_voxceleb_16k.onnx"),
        },
    ],
    required: &["segmentation.onnx", "3dspeaker_speech_eres2net_sv_en_voxceleb_16k.onnx"],
    notice: "pyannote segmentation-3.0 (sherpa-onnx's ONNX conversion), MIT, (c) 2022 CNRS.\n\
             3D-Speaker ERes2Net (VoxCeleb), Apache-2.0.\n",
};

pub static SHERPA_RUNTIME: Package = Package {
    id: "sherpa-onnx-1.13.8",
    folder: "sherpa-onnx-1.13.8",
    artifacts: &[Artifact {
        url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.13.8/sherpa-onnx-v1.13.8-win-x64-shared-MD-Release-no-tts.tar.bz2",
        sha256: "876e6b89b8cf84a3a1b375a397507f2cfe9c227c2a945411a11a668475fcb5d3",
        size: 19_164_933,
        unpack: Unpack::NotYet,
    }],
    required: &["sherpa-onnx-c-api.dll", "onnxruntime.dll"],
    notice: "sherpa-onnx 1.13.8, Apache-2.0; ONNX Runtime 1.28.2, MIT.\n",
};

pub static MODELS: &[Model] = &[
    Model {
        id: "nemotron-3-diarization",
        name: "Nemotron-3 Diarization",
        vendor: "NVIDIA",
        engine: "Nemotron",
        desc: "Tells up to 8 voices apart as the meeting goes, and keeps track of them for hours.",
        speakers: 8,
        recommended: true,
        available: true,
        note: None,
        license: "OpenMDW-1.1",
        license_url: "https://openmdw.ai/license/1-1/",
        source_url: "https://huggingface.co/nvidia/Nemotron-3-Diarization",
        runtime_name: "NeMo-Speech.cpp 0.2.0",
        runtime_license: "Apache-2.0",
        runtime_license_url: "https://github.com/NVIDIA/NeMo-Speech.cpp",
        weights: &NEMOTRON_WEIGHTS,
        runtime: &NEMO_RUNTIME,
        extras: &[&VOICE_MATCHING],
        weights_file: "Nemotron-3-Diarization.q8_0.gguf",
        preset: "v3-offline",
    },
    Model {
        id: "sherpa-pyannote-eres2net",
        name: "pyannote + ERes2Net",
        vendor: "sherpa-onnx",
        engine: "sherpa",
        desc: "For calls where everyone dials in by phone. Runs after the meeting.",
        speakers: 8,
        recommended: false,
        available: false,
        note: Some("For phone-only calls — coming later"),
        license: "MIT + Apache-2.0",
        license_url: "https://github.com/k2-fsa/sherpa-onnx",
        source_url: "https://github.com/k2-fsa/sherpa-onnx",
        runtime_name: "sherpa-onnx 1.13.8",
        runtime_license: "Apache-2.0",
        runtime_license_url: "https://github.com/k2-fsa/sherpa-onnx",
        weights: &SHERPA_WEIGHTS,
        runtime: &SHERPA_RUNTIME,
        extras: &[],
        weights_file: "segmentation.onnx",
        preset: "",
    },
];

/// The model new installs use.
pub const DEFAULT_MODEL: &str = "nemotron-3-diarization";

pub fn find(id: &str) -> Option<&'static Model> {
    MODELS.iter().find(|m| m.id == id)
}

/// Where the diarization packages live: `<data>/models/diarization`, or in
/// a debug build `YAP_DIARIZATION_MODELS` (the e2e suite and the real-engine
/// checks point it at files already on disk).
pub fn root() -> PathBuf {
    #[cfg(debug_assertions)]
    if let Some(dir) = std::env::var_os("YAP_DIARIZATION_MODELS").filter(|v| !v.is_empty()) {
        return PathBuf::from(dir);
    }
    crate::config::data_dir().join("models").join("diarization")
}

/// An artifact as it will be fetched (a debug build's stand-ins applied).
#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    pub url: String,
    pub sha256: String,
    pub size: u64,
    pub unpack: Unpack,
}

/// The artifacts of `pkg`. A debug build reads stand-ins from the JSON file
/// `YAP_DIARIZATION_SOURCES` (`{"<package id>": [{"url", "sha256", "size"}]}`,
/// one per artifact), which is how the e2e suite downloads small fakes from
/// a local server.
pub fn sources(pkg: &Package) -> Vec<Source> {
    #[cfg_attr(not(debug_assertions), allow(unused_mut))]
    let mut out: Vec<Source> = pkg
        .artifacts
        .iter()
        .map(|a| Source {
            url: a.url.to_string(),
            sha256: a.sha256.to_string(),
            size: a.size,
            unpack: a.unpack,
        })
        .collect();
    #[cfg(debug_assertions)]
    if let Some(path) = std::env::var_os("YAP_DIARIZATION_SOURCES").filter(|v| !v.is_empty()) {
        let stand_ins = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());
        if let Some(list) = stand_ins.as_ref().and_then(|v| v.get(pkg.id)).and_then(|v| v.as_array()) {
            for (src, stand_in) in out.iter_mut().zip(list) {
                if let (Some(url), Some(sha), Some(size)) = (
                    stand_in["url"].as_str(),
                    stand_in["sha256"].as_str(),
                    stand_in["size"].as_u64(),
                ) {
                    *src = Source {
                        url: url.to_string(),
                        sha256: sha.to_ascii_lowercase(),
                        size,
                        unpack: src.unpack,
                    };
                }
            }
        }
    }
    out
}

/// Whether `pkg` is installed under `root`: its folder holds every required
/// file, and each file saved as-is has its pinned size.
pub fn installed(root: &Path, pkg: &Package) -> bool {
    let dir = root.join(pkg.folder);
    if !pkg.required.iter().all(|f| dir.join(f).is_file()) {
        return false;
    }
    sources(pkg).iter().all(|s| match s.unpack {
        Unpack::File(name) => std::fs::metadata(dir.join(name)).is_ok_and(|m| m.len() == s.size),
        _ => true,
    })
}

/// A model and its runtime are both there.
pub fn model_installed(root: &Path, model: &Model) -> bool {
    model.available && installed(root, model.weights) && installed(root, model.runtime)
}

/// The voice-matching model's file, if it's installed (`voiceprint.rs`).
pub fn voice_matching_file() -> Option<PathBuf> {
    let root = root();
    installed(&root, &VOICE_MATCHING).then(|| root.join(VOICE_MATCHING.folder).join(VOICE_MATCHING_FILE))
}

/// What `yap.exe diarize` needs to run a model.
#[derive(Debug, Clone)]
pub struct EngineFiles {
    pub runtime_dir: PathBuf,
    pub model_path: PathBuf,
    pub preset: &'static str,
    pub name: &'static str,
}

/// The engine files of an installed model (`id`, else the default).
pub fn engine_files(id: &str) -> Option<EngineFiles> {
    let model = find(id).or_else(|| find(DEFAULT_MODEL))?;
    let root = root();
    model_installed(&root, model).then(|| EngineFiles {
        runtime_dir: root.join(model.runtime.folder),
        model_path: root.join(model.weights.folder).join(model.weights_file),
        preset: model.preset,
        name: model.name,
    })
}

/// A model as Settings shows it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub vendor: &'static str,
    pub engine: &'static str,
    pub desc: &'static str,
    pub speakers: u8,
    pub recommended: bool,
    pub available: bool,
    pub note: Option<&'static str>,
    /// What a download fetches now (the runtime too, unless it's there; its
    /// extras too, when they're missing).
    pub download_bytes: u64,
    /// Its extras (voice matching) are missing: what fetching them takes
    /// (0: none missing).
    pub extras_bytes: u64,
    /// The model and its runtime, on disk.
    pub size_bytes: u64,
    pub installed: bool,
    pub license: &'static str,
    pub license_url: &'static str,
    pub source_url: &'static str,
    pub runtime_name: &'static str,
    pub runtime_license: &'static str,
    pub runtime_license_url: &'static str,
}

pub fn list(root: &Path) -> Vec<ModelInfo> {
    let size = |pkg: &Package| sources(pkg).iter().map(|s| s.size).sum::<u64>();
    MODELS
        .iter()
        .map(|m| {
            let mut download_bytes = 0;
            for pkg in [m.weights, m.runtime] {
                if !installed(root, pkg) {
                    download_bytes += size(pkg);
                }
            }
            let extras_bytes: u64 = m.extras.iter().filter(|p| !installed(root, p)).map(|p| size(p)).sum();
            download_bytes += extras_bytes;
            ModelInfo {
                id: m.id,
                name: m.name,
                vendor: m.vendor,
                engine: m.engine,
                desc: m.desc,
                speakers: m.speakers,
                recommended: m.recommended,
                available: m.available,
                note: m.note,
                download_bytes,
                extras_bytes,
                size_bytes: size(m.weights) + size(m.runtime) + m.extras.iter().map(|p| size(p)).sum::<u64>(),
                installed: model_installed(root, m),
                license: m.license,
                license_url: m.license_url,
                source_url: m.source_url,
                runtime_name: m.runtime_name,
                runtime_license: m.runtime_license,
                runtime_license_url: m.runtime_license_url,
            }
        })
        .collect()
}

/// Where an entry of NeMo-Speech.cpp's release zip goes in the runtime
/// folder: the DLLs from `bin/` (not the CLI, nor the translation, speech
/// synthesis or LLM libraries diarization doesn't load) at the top, its
/// licences under `licenses/`, nothing else.
pub fn runtime_entry(name: &str) -> Option<String> {
    let parts: Vec<&str> = name.split('/').collect();
    // Every name sits under one top folder ("nemo-speech-0.2.0-windows-…/").
    let rest = parts.get(1..)?;
    if rest.iter().any(|p| p.is_empty() || *p == "." || *p == ".." || p.contains(['\\', ':'])) {
        return None;
    }
    const SKIP: &[&str] = &["llama.dll", "nemo_speech_nmt.dll", "nemo_speech_nmt_c.dll", "nemo_speech_tts.dll"];
    match rest {
        ["bin", file] => {
            let lower = file.to_ascii_lowercase();
            (lower.ends_with(".dll") && !SKIP.contains(&lower.as_str())).then(|| file.to_string())
        }
        ["share", "licenses", "nemo-speech", tail @ ..] if !tail.is_empty() => {
            Some(format!("licenses/{}", tail.join("/")))
        }
        _ => None,
    }
}

/// Unpack the runtime zip into `dest`. Returns how many files it wrote.
fn extract_runtime(zip_path: &Path, dest: &Path) -> Result<usize, String> {
    let file = std::fs::File::open(zip_path).map_err(|e| format!("open the runtime: {e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("read the runtime zip: {e}"))?;
    let mut written = 0;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| format!("runtime zip entry {i}: {e}"))?;
        if entry.is_dir() {
            continue;
        }
        let Some(rel) = runtime_entry(entry.name()) else {
            continue;
        };
        let out_path = dest.join(&rel);
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
        }
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut bytes).map_err(|e| format!("unpack {rel}: {e}"))?;
        std::fs::write(&out_path, &bytes).map_err(|e| format!("write {rel}: {e}"))?;
        written += 1;
    }
    Ok(written)
}

/// Download progress, for `diarization-download-progress`.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub id: String,
    /// "runtime" | "model": what's downloading now.
    pub stage: &'static str,
    pub percent: u8,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    /// The speed and the time left (`shell/download.rs`).
    #[serde(default)]
    pub bytes_per_sec: u64,
    pub eta_secs: Option<u64>,
    /// What Cancel cancels (`download_cancel`).
    pub cancel_id: String,
}

/// The id a speaker model's download is cancelled by.
pub fn cancel_id(id: &str) -> String {
    format!("diarization:{id}")
}

/// A test run fetches only from this PC (the e2e suite's local server).
fn allowed(url: &str) -> bool {
    if !crate::shell::e2e::active() {
        return true;
    }
    ["http://127.0.0.1:", "http://localhost:", "http://[::1]:"]
        .iter()
        .any(|p| url.starts_with(p))
}

/// Install model `id` (and its runtime, if missing) under `root`. A cancel
/// (`ticket`) or a failure leaves no half-made folder; a partial download
/// that failed stays, to resume (a cancelled one goes).
pub async fn install(root: &Path, id: &str, ticket: &crate::shell::download::Ticket, on_progress: &(dyn Fn(Progress) + Send + Sync)) -> Result<(), String> {
    let model = find(id).ok_or_else(|| format!("No diarization model '{id}'"))?;
    if !model.available {
        return Err(format!("{} isn't available yet", model.name));
    }
    let todo: Vec<(&'static str, &Package)> = [("runtime", model.runtime), ("model", model.weights)]
        .into_iter()
        .chain(model.extras.iter().map(|p| ("voices", *p)))
        .filter(|(_, pkg)| !installed(root, pkg))
        .collect();
    let total: u64 = todo.iter().flat_map(|(_, pkg)| sources(pkg)).map(|s| s.size).sum();
    let overall = crate::shell::download::Overall::new(total);
    std::fs::create_dir_all(root).map_err(|e| format!("create {}: {e}", root.display()))?;
    let mut done: u64 = 0;
    for (stage, pkg) in todo {
        tracing::info!(package = pkg.id, "Diarization: installing");
        let staging = root.join(format!("{}.installing", pkg.folder));
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(&staging).map_err(|e| format!("create {}: {e}", staging.display()))?;
        let fail = |e: String| {
            let _ = std::fs::remove_dir_all(&staging);
            e
        };
        for (i, src) in sources(pkg).into_iter().enumerate() {
            if !allowed(&src.url) {
                return Err(fail("Test runs never download from the internet".to_string()));
            }
            let partial = root.join(format!("{}.{i}.partial", pkg.folder));
            let base = done;
            let report = |p: crate::shell::download::Progress| {
                let so_far = base + p.downloaded;
                let p = overall.at(so_far);
                on_progress(Progress {
                    id: id.to_string(),
                    stage,
                    percent: if total == 0 { 100 } else { p.percent() },
                    downloaded_bytes: so_far,
                    total_bytes: total,
                    bytes_per_sec: p.bytes_per_sec,
                    eta_secs: p.eta_secs,
                    cancel_id: cancel_id(id),
                });
            };
            download(&src, &partial, ticket, &report, crate::privacy::net::Download::SpeakerModel).await.map_err(fail)?;
            verify(&partial, &src).await.map_err(fail)?;
            match src.unpack {
                Unpack::File(name) => {
                    std::fs::rename(&partial, staging.join(name)).map_err(|e| format!("move {name}: {e}"))?;
                }
                Unpack::NemoRuntimeZip => {
                    let (zip, dest) = (partial.clone(), staging.clone());
                    let n = tokio::task::spawn_blocking(move || extract_runtime(&zip, &dest))
                        .await
                        .map_err(|e| format!("unpack task: {e}"))??;
                    tracing::info!(package = pkg.id, files = n, "Diarization: runtime unpacked");
                    let _ = std::fs::remove_file(&partial);
                }
                Unpack::NotYet => return Err(format!("{} can't be installed yet", pkg.id)),
            }
            done += src.size;
        }
        std::fs::write(staging.join("NOTICE.txt"), pkg.notice).map_err(|e| format!("write NOTICE.txt: {e}"))?;
        if !pkg.required.iter().all(|f| staging.join(f).is_file()) {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(format!("{} is missing files after unpacking", pkg.id));
        }
        let dest = root.join(pkg.folder);
        if dest.exists() {
            std::fs::remove_dir_all(&dest).map_err(|e| format!("replace {}: {e}", dest.display()))?;
        }
        std::fs::rename(&staging, &dest).map_err(|e| format!("install {}: {e}", pkg.id))?;
        tracing::info!(package = pkg.id, "Diarization: installed");
    }
    let p = overall.at(total);
    on_progress(Progress {
        id: id.to_string(),
        stage: "model",
        percent: 100,
        downloaded_bytes: total,
        total_bytes: total,
        bytes_per_sec: p.bytes_per_sec,
        eta_secs: Some(0),
        cancel_id: cancel_id(id),
    });
    Ok(())
}

/// Stream `src` to `partial` through the shared helper
/// (`shell/download.rs`): resuming what an earlier try left, stopped by
/// `ticket` (the partial file goes then). `what`: the download as the
/// network receipts name it (`net.rs`).
pub(crate) async fn download(
    src: &Source,
    partial: &Path,
    ticket: &crate::shell::download::Ticket,
    report: &(dyn Fn(crate::shell::download::Progress) + Sync),
    what: crate::privacy::net::Download,
) -> Result<(), String> {
    crate::shell::download::fetch(
        crate::shell::download::Fetch { url: &src.url, partial, size: Some(src.size), what, ticket },
        report,
    )
    .await
    .map(|_| ())
}

/// Size and SHA-256, or the partial goes.
pub(crate) async fn verify(partial: &Path, src: &Source) -> Result<(), String> {
    let path = partial.to_path_buf();
    let expected = src.sha256.clone();
    let size = src.size;
    let ok = tokio::task::spawn_blocking(move || {
        let len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        len == size && sha256(&path).is_ok_and(|actual| actual == expected)
    })
    .await
    .map_err(|e| format!("checksum task: {e}"))?;
    if !ok {
        let _ = tokio::fs::remove_file(partial).await;
        tracing::warn!(url = %host(&src.url), "Model download failed its SHA-256 check");
        return Err("The download was corrupt (its checksum didn't match). Try again.".to_string());
    }
    Ok(())
}

fn host(url: &str) -> &str {
    url.split("//").nth(1).and_then(|r| r.split('/').next()).unwrap_or(url)
}

pub fn sha256(path: &Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Delete model `id`, and its runtime when no other installed model uses it.
pub fn delete(root: &Path, id: &str) -> Result<(), String> {
    let model = find(id).ok_or_else(|| format!("No diarization model '{id}'"))?;
    let remove = |pkg: &Package| -> Result<(), String> {
        let dir = root.join(pkg.folder);
        if dir.exists() {
            std::fs::remove_dir_all(&dir).map_err(|e| format!("Couldn't delete {}: {e}", pkg.id))?;
        }
        for i in 0..pkg.artifacts.len() {
            let _ = std::fs::remove_file(root.join(format!("{}.{i}.partial", pkg.folder)));
        }
        Ok(())
    };
    remove(model.weights)?;
    for extra in model.extras {
        remove(extra)?;
    }
    let shared = MODELS
        .iter()
        .any(|m| m.id != id && std::ptr::eq(m.runtime, model.runtime) && installed(root, m.weights));
    if !shared {
        remove(model.runtime)?;
    }
    tracing::info!(model = id, "Diarization: model deleted");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_registry_is_pinned() {
        let mut ids = Vec::new();
        for m in MODELS {
            assert!(!ids.contains(&m.id), "duplicate {}", m.id);
            ids.push(m.id);
            for pkg in [m.weights, m.runtime].into_iter().chain(m.extras.iter().copied()) {
                assert!(!pkg.artifacts.is_empty());
                for a in pkg.artifacts {
                    assert_eq!(a.sha256.len(), 64, "{}", a.url);
                    assert!(a.sha256.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
                    assert!(a.size > 0 && a.url.starts_with("https://"));
                    // Only the sources the spike used.
                    assert!(
                        a.url.starts_with("https://huggingface.co/nvidia/")
                            || a.url.starts_with("https://github.com/NVIDIA/NeMo-Speech.cpp/releases/download/")
                            || a.url.starts_with("https://github.com/k2-fsa/sherpa-onnx/releases/download/"),
                        "{}",
                        a.url
                    );
                }
            }
        }
        let nemotron = find(DEFAULT_MODEL).unwrap();
        assert!(nemotron.available && nemotron.recommended);
        assert!(nemotron.weights.artifacts[0].url.contains(NEMOTRON_COMMIT), "pinned to the spike's commit");
        assert_eq!(nemotron.weights.artifacts[0].unpack, Unpack::File(nemotron.weights_file));
        assert_eq!(nemotron.preset, "v3-offline");
        // The fallback is listed but can't be installed.
        let sherpa = find("sherpa-pyannote-eres2net").unwrap();
        assert!(!sherpa.available && sherpa.note.is_some());
    }

    #[test]
    fn runtime_zip_keeps_the_dlls_and_licences() {
        let top = "nemo-speech-0.2.0-windows-x86_64-cpu";
        let e = |rest: &str| runtime_entry(&format!("{top}/{rest}"));
        assert_eq!(e("bin/nemo_speech_asr_c.dll").as_deref(), Some("nemo_speech_asr_c.dll"));
        assert_eq!(e("bin/ggml-cpu.dll").as_deref(), Some("ggml-cpu.dll"));
        assert_eq!(e("bin/msvcp140_2.dll").as_deref(), Some("msvcp140_2.dll"));
        assert_eq!(e("share/licenses/nemo-speech/LICENSE").as_deref(), Some("licenses/LICENSE"));
        assert_eq!(
            e("share/licenses/nemo-speech/third_party/ggml/LICENSE").as_deref(),
            Some("licenses/third_party/ggml/LICENSE")
        );
        for skipped in [
            "bin/nemo-speech.exe",
            "bin/llama.dll",
            "bin/nemo_speech_tts.dll",
            "bin/nemo_speech_nmt_c.dll",
            "lib/nemo_speech_asr_c.lib",
            "include/nemo_speech/diar.h",
            "share/doc/nemo-speech/README.md",
            "share/licenses/nemo-speech/",
        ] {
            assert_eq!(e(skipped), None, "{skipped}");
        }
        // Nothing escapes the folder.
        assert_eq!(e("bin/../../evil.dll"), None);
        assert_eq!(e("share/licenses/nemo-speech/../../../x"), None);
        assert_eq!(runtime_entry("ggml.dll"), None, "no top folder");
        assert_eq!(e("bin/c:evil.dll"), None);
        assert_eq!(e("bin\\x.dll"), None);
    }

    fn temp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("yap-diar-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn installed_means_every_file_at_its_pinned_size() {
        let root = temp_root("installed");
        let model = find(DEFAULT_MODEL).unwrap();
        assert!(!model_installed(&root, model));
        let rt = root.join(model.runtime.folder);
        std::fs::create_dir_all(&rt).unwrap();
        for f in model.runtime.required {
            std::fs::write(rt.join(f), b"dll").unwrap();
        }
        assert!(installed(&root, model.runtime));
        let w = root.join(model.weights.folder);
        std::fs::create_dir_all(&w).unwrap();
        std::fs::write(w.join(model.weights_file), b"too short").unwrap();
        assert!(!installed(&root, model.weights), "a truncated model isn't installed");
        assert!(!model_installed(&root, model));
        let info = list(&root);
        let row = info.iter().find(|m| m.id == DEFAULT_MODEL).unwrap();
        assert!(!row.installed);
        assert_eq!(row.download_bytes, 107_012_128 + 26_485_263, "the runtime is there: the model (and voice matching) to fetch");
        assert_eq!(row.extras_bytes, 26_485_263);
        assert_eq!(row.size_bytes, 107_012_128 + 5_528_418 + 26_485_263);

        // Installed without voice matching (a model from before it): still
        // installed; the extra is what's left to fetch.
        std::fs::write(w.join(model.weights_file), vec![0u8; 107_012_128]).unwrap();
        assert!(model_installed(&root, model));
        let row = list(&root).into_iter().find(|m| m.id == DEFAULT_MODEL).unwrap();
        assert!(row.installed);
        assert_eq!((row.download_bytes, row.extras_bytes), (26_485_263, 26_485_263));
        let vm = root.join(VOICE_MATCHING.folder);
        std::fs::create_dir_all(&vm).unwrap();
        std::fs::write(vm.join(VOICE_MATCHING_FILE), vec![0u8; 26_485_263]).unwrap();
        assert!(installed(&root, &VOICE_MATCHING));
        let row = list(&root).into_iter().find(|m| m.id == DEFAULT_MODEL).unwrap();
        assert_eq!((row.download_bytes, row.extras_bytes), (0, 0));

        // Deleting the model takes its runtime and extras too (nothing else uses them).
        delete(&root, DEFAULT_MODEL).unwrap();
        assert!(!rt.exists() && !w.exists() && !vm.exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn extracting_a_runtime_zip() {
        use std::io::Write;
        let root = temp_root("zip");
        let zip_path = root.join("rt.zip");
        {
            let f = std::fs::File::create(&zip_path).unwrap();
            let mut z = zip::ZipWriter::new(f);
            let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            for (name, body) in [
                ("rt/bin/ggml.dll", "ggml"),
                ("rt/bin/nemo-speech.exe", "cli"),
                ("rt/share/licenses/nemo-speech/LICENSE", "Apache"),
                ("rt/include/x.h", "h"),
            ] {
                z.start_file(name, opts).unwrap();
                z.write_all(body.as_bytes()).unwrap();
            }
            z.finish().unwrap();
        }
        let dest = root.join("out");
        std::fs::create_dir_all(&dest).unwrap();
        assert_eq!(extract_runtime(&zip_path, &dest).unwrap(), 2);
        assert_eq!(std::fs::read_to_string(dest.join("ggml.dll")).unwrap(), "ggml");
        assert_eq!(std::fs::read_to_string(dest.join("licenses/LICENSE")).unwrap(), "Apache");
        assert!(!dest.join("nemo-speech.exe").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn checksums() {
        let root = temp_root("sha");
        let p = root.join("abc");
        std::fs::write(&p, b"abc").unwrap();
        assert_eq!(sha256(&p).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The real download, once, from NVIDIA's release and Hugging Face into a
    /// temp folder (`YAP_DIARIZATION_DOWNLOAD_DIR`, else the system temp):
    /// `cargo test real_download -- --ignored --nocapture`. ~112 MB.
    #[test]
    #[ignore]
    fn real_download_from_the_pinned_sources() {
        let root = std::env::var_os("YAP_DIARIZATION_DOWNLOAD_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| temp_root("download"));
        let rt = tokio::runtime::Runtime::new().unwrap();
        let last = std::sync::Mutex::new(0u8);
        rt.block_on(install(&root, DEFAULT_MODEL, &crate::shell::download::Ticket::unlisted(), &|p: Progress| {
            let mut l = last.lock().unwrap();
            if p.percent >= *l + 10 || p.percent == 100 {
                *l = p.percent;
                eprintln!("{} {}% ({} / {} bytes)", p.stage, p.percent, p.downloaded_bytes, p.total_bytes);
            }
        }))
        .unwrap();
        let model = find(DEFAULT_MODEL).unwrap();
        assert!(model_installed(&root, model));
        let gguf = root.join(model.weights.folder).join(model.weights_file);
        assert_eq!(sha256(&gguf).unwrap(), model.weights.artifacts[0].sha256);
        for f in ["LICENSE", "NOTICE", "THIRD_PARTY_NOTICES.md"] {
            assert!(root.join(model.runtime.folder).join("licenses").join(f).is_file(), "{f}");
        }
        eprintln!("installed into {}", root.display());
    }
}
