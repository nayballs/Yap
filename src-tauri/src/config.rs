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
    /// Active model id from the STT registry (e.g. "parakeet-tdt-0.6b-v3",
    /// "small"). Field name kept as `model_size` for config back-compat.
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
    /// Pill size multiplier (1.0 = default). Clamped 0.6..=1.6 when applied.
    #[serde(default = "default_scale")]
    pub pill_scale: f64,
    /// Transcription corrections.
    #[serde(default)]
    pub dictionary: Vec<DictionaryEntry>,

    /// Recording mode: "toggle" (press to start, press again to stop) or
    /// "pushToTalk" (hold to record, release to stop).
    #[serde(default = "default_recording_mode")]
    pub recording_mode: String,
    /// Mute system output audio while recording (Windows).
    #[serde(default)]
    pub mute_while_recording: bool,
    /// Append a single space after each injected transcription.
    #[serde(default)]
    pub append_trailing_space: bool,
    /// After injecting text, simulate pressing Enter (auto-submit).
    #[serde(default)]
    pub auto_submit: bool,
    /// Restore the user's previous clipboard contents after pasting.
    #[serde(default = "default_true")]
    pub restore_clipboard: bool,
    /// Don't show any window on launch (the pill still appears).
    #[serde(default)]
    pub start_hidden: bool,
    /// Show the system-tray icon.
    #[serde(default = "default_true")]
    pub show_tray_icon: bool,
    /// Launch Blip at OS login.
    #[serde(default)]
    pub autostart: bool,
    /// Chime volume (0.0–1.0).
    #[serde(default = "default_audio_feedback_volume")]
    pub audio_feedback_volume: f32,
    /// Show the always-on-top pill window. **Hidden by default** — the
    /// bottom-center overlay provides on-speak feedback; the pill is opt-in.
    /// Dictation works regardless via the hotkey.
    #[serde(default)]
    pub show_pill: bool,
    /// Show the floating bottom-center "transcribing" overlay while dictating.
    #[serde(default = "default_true")]
    pub show_overlay: bool,

    /// Transcription language. `"auto"` = auto-detect; otherwise a language code
    /// (e.g. "en", "fr"). Only applied by models that support language selection.
    #[serde(default = "default_selected_language")]
    pub selected_language: String,
    /// Translate the transcription to English (Whisper / Canary only).
    #[serde(default)]
    pub translate_to_english: bool,
    /// Unload the STT model after this much idle time to free VRAM, lazily
    /// reloading on the next dictation. One of "never", "1min", "5min",
    /// "15min", "30min".
    #[serde(default = "default_model_unload_timeout")]
    pub model_unload_timeout: String,
    /// Preferred output device for the start/stop chimes (None = system default).
    #[serde(default)]
    pub output_device: Option<String>,
    /// Where the transcribing overlay appears: "bottom" or "top".
    #[serde(default = "default_overlay_position")]
    pub overlay_position: String,
    /// Which key auto-submit presses after pasting: "enter", "ctrlEnter",
    /// or "shiftEnter".
    #[serde(default = "default_auto_submit_key")]
    pub auto_submit_key: String,
}

fn default_scale() -> f64 {
    1.0
}

fn default_hotkey() -> String {
    "kb:120".into()
}
fn default_model_size() -> String {
    "parakeet-tdt-0.6b-v3".into()
}
fn default_true() -> bool {
    true
}
fn default_recording_mode() -> String {
    "toggle".into()
}
fn default_audio_feedback_volume() -> f32 {
    1.0
}
fn default_selected_language() -> String {
    "auto".into()
}
fn default_model_unload_timeout() -> String {
    "never".into()
}
fn default_overlay_position() -> String {
    "bottom".into()
}
fn default_auto_submit_key() -> String {
    "enter".into()
}

impl Default for BlipConfig {
    fn default() -> Self {
        Self {
            hotkey: default_hotkey(),
            model_size: default_model_size(),
            use_gpu: true,
            input_device: None,
            sound_enabled: true,
            pill_scale: 1.0,
            dictionary: Vec::new(),
            recording_mode: default_recording_mode(),
            mute_while_recording: false,
            append_trailing_space: false,
            auto_submit: false,
            restore_clipboard: true,
            start_hidden: false,
            show_tray_icon: true,
            autostart: false,
            audio_feedback_volume: default_audio_feedback_volume(),
            show_pill: false,
            show_overlay: true,
            selected_language: default_selected_language(),
            translate_to_english: false,
            model_unload_timeout: default_model_unload_timeout(),
            output_device: None,
            overlay_position: default_overlay_position(),
            auto_submit_key: default_auto_submit_key(),
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
