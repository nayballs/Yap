//! Yap configuration: stored as JSON in the app data dir.
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

/// A reusable, named cleanup profile (FluidVoice's `DictationPromptProfile`): a
/// library entry that per-app routing rules bind to, so one body can serve many
/// apps and is edited in one place. The immutable `llm::BASE_PROMPT` guardrails
/// are always prepended, exactly as for the global body.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupProfile {
    /// Stable id referenced by `AppRoute.profile_id`.
    pub id: String,
    /// Display name shown in the profile picker.
    pub name: String,
    /// The cleanup body (tone/format instructions).
    pub prompt: String,
}

/// A per-app cleanup routing rule ("smart routing", ported in spirit from
/// FluidVoice's app-prompt bindings). When the foreground app at record-start
/// matches `app` (process base name, e.g. "slack.exe"), the cleanup pass uses
/// the bound profile's body instead of the global `pp_prompt`. FluidVoice keys
/// its bindings on macOS bundle identifiers; on Windows we key on the process
/// exe name (what `text_injector::app_name_for` already returns for history).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppRoute {
    /// Process base name to match, e.g. "slack.exe". Matched case-insensitively.
    pub app: String,
    /// Friendly display label for the Settings list (defaults to `app`).
    #[serde(default)]
    pub label: String,
    /// Id of the bound `CleanupProfile`. Empty → fall back to `prompt` (legacy)
    /// or the global default.
    #[serde(default)]
    pub profile_id: String,
    /// Legacy inline body from before named profiles existed. Kept for
    /// back-compat; the frontend migrates these into profiles on load.
    #[serde(default)]
    pub prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct YapConfig {
    /// Global hotkey for the input hook. Format: "kb:VKEY" or "mouse:ID".
    /// Default kb:120 = F9.
    #[serde(default = "default_hotkey")]
    pub hotkey: String,
    /// Optional second hotkey for **edit/rewrite mode**: capture the selected
    /// text, then treat your spoken words as an instruction to rewrite it (or, if
    /// nothing is selected, to write new text). Empty = unbound (opt-in). Same
    /// "kb:VKEY"/"mouse:ID" format as `hotkey`.
    #[serde(default)]
    pub edit_hotkey: String,
    /// Active model id from the STT registry (e.g. "parakeet-tdt-0.6b-v3",
    /// "small"). Field name kept as `model_size` for config back-compat.
    #[serde(default = "default_model_size")]
    pub model_size: String,
    /// Use GPU acceleration (whisper → Vulkan, ONNX → DirectML; any GPU).
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
    /// Launch Yap at OS login.
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
    /// Automatically check GitHub Releases for a newer Yap on launch.
    #[serde(default = "default_true")]
    pub update_checks_enabled: bool,

    // ---- AI cleanup (post-processing) ----
    /// Run the transcript through an LLM cleanup pass before injecting it.
    /// Off by default; any error/timeout falls back to the raw transcript.
    #[serde(default)]
    pub post_process_enabled: bool,
    /// UI preset id ("groq"|"openai"|"openrouter"|"local"|"custom"). Drives the
    /// base-URL default in the settings UI; the backend only uses `pp_base_url`.
    #[serde(default = "default_pp_provider")]
    pub pp_provider: String,
    /// OpenAI-compatible base URL, e.g. "https://api.groq.com/openai/v1" or
    /// "http://localhost:11434/v1". `/chat/completions` is appended.
    #[serde(default = "default_pp_base_url")]
    pub pp_base_url: String,
    /// API key for the cleanup endpoint (empty for local servers). Stored in the
    /// local config.json; REDACTED in any logs — never logged.
    #[serde(default)]
    pub pp_api_key: String,
    /// Model id passed to the cleanup endpoint.
    #[serde(default = "default_pp_model")]
    pub pp_model: String,
    /// Editable cleanup "body": the tone/format instructions the user can tweak.
    /// The immutable guardrails (don't answer the transcript, output-only, etc.)
    /// live in `llm::BASE_PROMPT` and are always prepended — so editing this can
    /// never break refusal behaviour. A cleanup *preset* just fills this in.
    #[serde(default = "default_pp_prompt")]
    pub pp_prompt: String,
    /// Which cleanup preset the body came from: "default"|"email"|"notes"|
    /// "slack"|"code"|"custom". Persisted only so the Settings dropdown remembers
    /// the selection; the backend always uses `pp_prompt` as the body.
    #[serde(default = "default_pp_preset")]
    pub pp_preset: String,
    /// Smart-routing scope (FluidVoice's `PromptRoutingScope`):
    /// - "all_apps" (default): the global `pp_prompt` cleans everywhere, and
    ///   `app_routes` override it for matching apps.
    /// - "selected_apps_only": cleanup runs ONLY for apps with a matching rule;
    ///   dictation into any other app is injected raw.
    #[serde(default = "default_routing_scope")]
    pub routing_scope: String,
    /// Per-app cleanup routing rules. Resolved at dictation time against the
    /// foreground app captured at record-start. See [`YapConfig::resolve_cleanup_body`].
    #[serde(default)]
    pub app_routes: Vec<AppRoute>,
    /// Reusable named cleanup profiles that `app_routes` bind to.
    #[serde(default)]
    pub cleanup_profiles: Vec<CleanupProfile>,

    /// Show live partial transcripts in the overlay while you speak. Opt-in
    /// (off by default): re-transcribes the growing buffer on a timer, which adds
    /// GPU load. The final transcript on stop is always authoritative.
    #[serde(default)]
    pub streaming_partials: bool,

    /// Keep a local transcription history (powers the stats dashboard). Stored
    /// only on this machine; can be cleared from Settings. On by default.
    #[serde(default = "default_true")]
    pub history_enabled: bool,
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
fn default_pp_provider() -> String {
    "groq".into()
}
fn default_pp_base_url() -> String {
    "https://api.groq.com/openai/v1".into()
}
fn default_pp_model() -> String {
    "llama-3.1-8b-instant".into()
}
fn default_pp_prompt() -> String {
    // The "Default" preset body. Behaviour/tone only — the guardrails live in
    // `llm::BASE_PROMPT`. Keep in sync with PP_PRESETS.default in Settings.svelte.
    "Remove filler words (um, uh, er, like, you know). Fix capitalization, punctuation, and obvious grammar. Resolve spoken self-corrections (e.g. \"go to the store, no wait, the bank\" → \"go to the bank\"). Keep the result faithful and natural — don't over-format.".into()
}
fn default_pp_preset() -> String {
    "default".into()
}
fn default_routing_scope() -> String {
    "all_apps".into()
}

impl Default for YapConfig {
    fn default() -> Self {
        Self {
            hotkey: default_hotkey(),
            edit_hotkey: String::new(),
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
            update_checks_enabled: true,
            post_process_enabled: false,
            pp_provider: default_pp_provider(),
            pp_base_url: default_pp_base_url(),
            pp_api_key: String::new(),
            pp_model: default_pp_model(),
            pp_prompt: default_pp_prompt(),
            pp_preset: default_pp_preset(),
            routing_scope: default_routing_scope(),
            app_routes: Vec::new(),
            cleanup_profiles: Vec::new(),
            streaming_partials: false,
            history_enabled: true,
        }
    }
}

/// Yap's data directory.
///
/// In **portable mode** this is `<exe_dir>/Data/` (set up by the installer and
/// detected by [`crate::portable`]). Otherwise it's `%APPDATA%/yap/` (or the
/// platform equivalent) — left unchanged so existing installs are unaffected.
pub fn data_dir() -> PathBuf {
    if let Some(dir) = crate::portable::data_dir() {
        return dir.clone();
    }
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("yap")
}

fn config_path() -> PathBuf {
    data_dir().join("config.json")
}

/// Load config from disk, falling back to defaults.
pub fn load() -> YapConfig {
    match std::fs::read_to_string(config_path()) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => YapConfig::default(),
    }
}

/// Persist config to disk.
pub fn save(cfg: &YapConfig) -> Result<(), String> {
    std::fs::create_dir_all(data_dir()).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    std::fs::write(config_path(), json).map_err(|e| e.to_string())
}

impl YapConfig {
    /// Decide which cleanup body to use for a dictation whose target app is
    /// `process` (process base name, e.g. "slack.exe"), applying the smart-routing
    /// rules. This mirrors FluidVoice's `promptResolution` precedence, minus the
    /// macOS-only pieces (modes, bundle ids):
    ///
    /// 1. An `app_routes` rule matching `process` → that rule's body.
    /// 2. Otherwise, if scope is "selected_apps_only" → `None` (skip cleanup).
    /// 3. Otherwise ("all_apps") → the global `pp_prompt` body.
    ///
    /// `None` means "inject the raw transcript"; the caller still checks
    /// `post_process_enabled`/base-URL before running any cleanup at all.
    pub fn resolve_cleanup_body(&self, process: Option<&str>) -> Option<String> {
        if let Some(proc) = process {
            let proc = proc.trim();
            if let Some(route) = self
                .app_routes
                .iter()
                .find(|r| !r.app.trim().is_empty() && r.app.trim().eq_ignore_ascii_case(proc))
            {
                // Bound app: prefer its profile, then any legacy inline body, then
                // the global default (a bound app is always cleaned — mirrors
                // FluidVoice's promptID==nil → "force default").
                if !route.profile_id.is_empty() {
                    if let Some(p) = self
                        .cleanup_profiles
                        .iter()
                        .find(|p| p.id == route.profile_id)
                    {
                        return Some(p.prompt.clone());
                    }
                }
                if !route.prompt.trim().is_empty() {
                    return Some(route.prompt.clone());
                }
                return Some(self.pp_prompt.clone());
            }
        }
        if self.routing_scope == "selected_apps_only" {
            None
        } else {
            Some(self.pp_prompt.clone())
        }
    }
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
