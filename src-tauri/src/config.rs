//! Blip configuration: stored as JSON in the app data dir.
//!
//! Deliberately tiny — a dictation pill only needs a hotkey, a model,
//! GPU toggle, the sound cue, and the correction dictionary.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A single transcription correction: replace `from` with `to`.
/// Post-processing fix for words the STT model mishears
/// (e.g. "Power to Keep" -> "Parakeet"). Model-agnostic.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryEntry {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlipConfig {
    /// Global hotkey for the input hook. Format: "kb:VKEY" or "mouse:ID".
    /// Default kb:120 = F9.
    #[serde(default = "default_hotkey")]
    pub hotkey: String,
    /// Whisper model size (e.g. "large-v3", "base").
    #[serde(default = "default_model_size")]
    pub model_size: String,
    /// Use CUDA GPU acceleration.
    #[serde(default = "default_true")]
    pub use_gpu: bool,
    /// Preferred input device name (None = system default).
    #[serde(default)]
    pub input_device: Option<String>,
    /// Play a chime when recording starts/stops.
    #[serde(default = "default_true")]
    pub sound_enabled: bool,
    /// Transcription corrections.
    #[serde(default)]
    pub dictionary: Vec<DictionaryEntry>,
}

fn default_hotkey() -> String {
    "kb:120".into()
}
fn default_model_size() -> String {
    "large-v3".into()
}
fn default_true() -> bool {
    true
}

impl Default for BlipConfig {
    fn default() -> Self {
        Self {
            hotkey: default_hotkey(),
            model_size: default_model_size(),
            use_gpu: true,
            input_device: None,
            sound_enabled: true,
            dictionary: Vec::new(),
        }
    }
}

/// Blip's data directory: `%APPDATA%/blip/` (or platform equivalent).
pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("blip")
}

fn config_path() -> PathBuf {
    data_dir().join("config.json")
}

/// Load config from disk, falling back to defaults.
pub fn load() -> BlipConfig {
    match std::fs::read_to_string(config_path()) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => BlipConfig::default(),
    }
}

/// Persist config to disk.
pub fn save(cfg: &BlipConfig) -> Result<(), String> {
    std::fs::create_dir_all(data_dir()).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    std::fs::write(config_path(), json).map_err(|e| e.to_string())
}

/// Apply dictionary corrections to a transcription.
///
/// Case-insensitive literal replacement. Uses a replacement closure so
/// `$` in the target text is never treated as a regex backreference.
pub fn apply_dictionary(text: &str, dict: &[DictionaryEntry]) -> String {
    let mut out = text.to_string();
    for entry in dict {
        let from = entry.from.trim();
        if from.is_empty() {
            continue;
        }
        let pattern = format!("(?i){}", regex::escape(from));
        if let Ok(re) = regex::Regex::new(&pattern) {
            let to = entry.to.clone();
            out = re
                .replace_all(&out, |_: &regex::Captures| to.clone())
                .into_owned();
        }
    }
    out
}
