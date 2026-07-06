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
    /// Optional per-profile **LLM override** (superwhisper-style per-mode model
    /// choice): run this profile on its own provider/model instead of the global
    /// AI-cleanup settings — e.g. "Email" on a strong cloud model while "Slack"
    /// uses the fast local sidecar. Empty `provider` = inherit the global
    /// settings (the other three fields are then ignored). Same provider ids as
    /// `pp_provider` ("ondevice" routes through the sidecar).
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub api_key: String,
}

/// A per-mode LLM configuration for one of Yap's AI **scopes** — the OpenWhispr-
/// style "Language Models" bubbles: Dictation Cleanup, Voice Agent, Note
/// Formatting, Chat. Each scope carries its own provider/model/key/prompt so a
/// heavy cloud model can drive Voice Agent while cleanup runs on the fast local
/// sidecar, etc. (OpenWhispr's `INFERENCE_SCOPES`, one config per bubble.)
///
/// **Back-compat:** the Dictation Cleanup scope is NOT stored here — it stays in
/// the top-level `pp_*` fields, which the whole cleanup pipeline already reads.
/// This map (`YapConfig::llm_scopes`) holds only the three newer scopes, keyed
/// `"voiceAgent"|"noteFormatting"|"chat"`. A missing key = the scope's feature is
/// off / unconfigured; an empty `provider` inherits the global cleanup endpoint;
/// Note Formatting falls back to the Cleanup scope (OpenWhispr `fallbackScope`).
/// The runtime for Voice Agent (edit mode) / Note Formatting / Chat is wired in
/// later steps — this struct is the storage those hang off. See `ROADMAP.md`
/// Phase 4 "Multi-mode Language Models".
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmScope {
    /// Whether this scope's feature is enabled.
    #[serde(default)]
    pub enabled: bool,
    /// Provider id ("groq"|"anthropic"|"openai"|"openrouter"|"ondevice"|"local"|
    /// "custom"). Empty = inherit the global cleanup provider/endpoint.
    #[serde(default)]
    pub provider: String,
    /// OpenAI-compatible base URL; `/chat/completions` is appended.
    #[serde(default)]
    pub base_url: String,
    /// Model id passed to the endpoint.
    #[serde(default)]
    pub model: String,
    /// Active API key for this scope (empty for local servers). Never logged.
    #[serde(default)]
    pub api_key: String,
    /// Per-provider key memory (like `pp_api_keys`): the UI swaps `api_key` from
    /// here when the provider changes so each provider remembers its own key.
    #[serde(default)]
    pub api_keys: std::collections::HashMap<String, String>,
    /// Editable prompt body for this scope. The immutable guardrails
    /// (`llm::BASE_PROMPT` and friends) are still applied by the backend.
    #[serde(default)]
    pub prompt: String,
    /// Strip the model's `<think>…</think>` reasoning blocks from the output
    /// (only meaningful for reasoning models). See `pp_disable_thinking`.
    #[serde(default)]
    pub disable_thinking: bool,
}

/// A resolved cleanup plan for one dictation: the prompt body to use, plus the
/// profile's LLM override when the matched profile carries one (`None` =
/// use the global AI-cleanup endpoint). Produced by
/// [`YapConfig::resolve_cleanup`].
pub struct CleanupPlan {
    /// The cleanup body (tone/format instructions).
    pub body: String,
    /// `(provider, base_url, api_key, model)` override from the profile.
    pub endpoint: Option<(String, String, String, String)>,
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
    /// Per-provider API keys (provider id → key), OpenWhispr-style: the Settings
    /// UI stashes/restores `pp_api_key` from this map when the provider changes,
    /// so each provider remembers its own key. The backend only ever reads the
    /// active `pp_api_key`. Same secrecy rules: never logged.
    #[serde(default)]
    pub pp_api_keys: std::collections::HashMap<String, String>,
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
    /// Strip the cleanup model's `<think>…</think>` reasoning blocks from its
    /// output (OpenWhispr's "Disable thinking output"). Only shown/relevant for
    /// reasoning models; harmless for others (they emit no think blocks).
    #[serde(default)]
    pub pp_disable_thinking: bool,
    /// GGUF filename (inside `<data>/llm/`) the on-device sidecar should load
    /// instead of the bundled default. Empty = the bundled Qwen model. Users can
    /// drop any GGUF into the folder and pick it in Settings.
    #[serde(default)]
    pub pp_local_model: String,
    /// Smart-routing scope (FluidVoice's `PromptRoutingScope`):
    /// - "all_apps" (default): the global `pp_prompt` cleans everywhere, and
    ///   `app_routes` override it for matching apps.
    /// - "selected_apps_only": cleanup runs ONLY for apps with a matching rule;
    ///   dictation into any other app is injected raw.
    #[serde(default = "default_routing_scope")]
    pub routing_scope: String,
    /// Per-app cleanup routing rules. Resolved at dictation time against the
    /// foreground app captured at record-start. See [`YapConfig::resolve_cleanup`].
    #[serde(default)]
    pub app_routes: Vec<AppRoute>,
    /// Reusable named cleanup profiles that `app_routes` bind to.
    #[serde(default)]
    pub cleanup_profiles: Vec<CleanupProfile>,

    /// Per-mode LLM config for the AI **scopes** beyond cleanup (OpenWhispr's
    /// Language-Model bubbles), keyed `"voiceAgent"|"noteFormatting"|"chat"`.
    /// (Step-1 storage for the multi-mode Language Models feature.) The
    /// Dictation Cleanup scope stays in the `pp_*` fields above for back-compat.
    /// See [`LlmScope`] and `ROADMAP.md` Phase 4 "Multi-mode Language Models".
    #[serde(default)]
    pub llm_scopes: std::collections::HashMap<String, LlmScope>,
    /// The **Voice Agent** wake word (OpenWhispr "Agent Name"): speak this name
    /// during dictation to address the agent — it executes your spoken command
    /// instead of transcribing it verbatim. Empty = no wake word set (the agent
    /// then only runs via the edit/rewrite hotkey). Also added to the dictionary
    /// so the STT spells it right. See the Voice Agent bubble in Settings.
    /// (Persisted by the Voice Agent tab's Agent Name field.)
    #[serde(default)]
    pub agent_name: String,

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
    // The "Default" preset body, shown (with `llm::BASE_PROMPT` on top) in the
    // Prompt Studio View. Structured OpenWhispr-style so the rules are VISIBLE.
    // The same rules are ALSO enforced at runtime via `llm::cleanup`'s framing +
    // one-shot examples (kept for small-model reliability), so editing/deleting
    // this body can't break cleanup — it degrades gracefully to the guardrails.
    // MUST stay byte-identical to PP_PRESETS.default.body + the FIELD_DEFAULTS
    // fallback in Settings.svelte (the "Default vs Modified" check compares them).
    "Clean up the transcript using these rules.\n\nRULES:\n- Remove filler words (um, uh, er, like, you know, basically) unless meaningful.\n- Fix grammar, spelling, punctuation, and capitalization; break up run-on sentences.\n- Remove false starts, stutters, and accidental repetitions.\n- Correct obvious speech-to-text transcription errors from context.\n- Preserve the speaker's voice, tone, vocabulary, and intent.\n- Preserve technical terms, proper nouns, names, and jargon exactly as spoken — never \"correct\" them.\n\nSelf-corrections (\"wait no\", \"I meant\", \"scratch that\"): keep only the corrected version. \"Actually\" used for emphasis is NOT a correction.\nSpoken punctuation (\"period\", \"comma\", \"new line\"): convert to symbols. Use context to distinguish commands from literal mentions.\nNumbers & dates: standard written forms (January 15, 2026 / $300 / 5:30 PM). Small conversational numbers can stay as words.\nBroken phrases: reconstruct the speaker's likely intent from context. Never output a polished sentence that says nothing coherent.\nFormatting: bullets, numbered lists, or paragraph breaks only when they genuinely improve readability. Don't over-format.\n\nOUTPUT:\n- Output ONLY the cleaned text. Nothing else.\n- No commentary, labels, explanations, or preamble.\n- No questions. No suggestions. No added content.\n- Empty or filler-only input = empty output.".into()
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
            pp_api_keys: std::collections::HashMap::new(),
            pp_model: default_pp_model(),
            pp_prompt: default_pp_prompt(),
            pp_preset: default_pp_preset(),
            pp_disable_thinking: false,
            pp_local_model: String::new(),
            routing_scope: default_routing_scope(),
            app_routes: Vec::new(),
            cleanup_profiles: Vec::new(),
            llm_scopes: std::collections::HashMap::new(),
            agent_name: String::new(),
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
    let mut cfg = match std::fs::read_to_string(config_path()) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => YapConfig::default(),
    };
    // Migration: users still on the "default" cleanup preset get the current
    // default body (so prompt improvements land without a "Modified" badge). A
    // hand-customised body carries `pp_preset != "default"`, so it's preserved.
    if cfg.pp_preset == "default" {
        cfg.pp_prompt = default_pp_prompt();
    }
    cfg
}

/// Persist config to disk.
pub fn save(cfg: &YapConfig) -> Result<(), String> {
    std::fs::create_dir_all(data_dir()).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    std::fs::write(config_path(), json).map_err(|e| e.to_string())
}

impl YapConfig {
    /// Resolve the API key for `provider` given a scope-local key. Standard
    /// cloud-provider keys are GLOBAL — one key per provider shared by every
    /// scope, exactly like OpenWhispr's `openai_api_key` etc. — so an empty
    /// scope key falls back to the per-provider store (`pp_api_keys`) and then
    /// to the active cleanup key when it's the same provider. Fixes rewrites
    /// firing with an empty key while the Cleanup tab held one for that provider.
    pub fn provider_api_key(&self, provider: &str, scope_key: &str) -> String {
        if !scope_key.is_empty() {
            return scope_key.to_string();
        }
        if let Some(k) = self.pp_api_keys.get(provider) {
            if !k.is_empty() {
                return k.clone();
            }
        }
        if self.pp_provider == provider && !self.pp_api_key.is_empty() {
            return self.pp_api_key.clone();
        }
        String::new()
    }

    /// Decide the cleanup **plan** (body + optional per-profile LLM override) for
    /// a dictation whose target app is `process` (process base name, e.g.
    /// "slack.exe"), applying the smart-routing rules. This mirrors FluidVoice's
    /// `promptResolution` precedence, minus the macOS-only pieces (modes, bundle
    /// ids):
    ///
    /// 1. An `app_routes` rule matching `process` → that rule's profile (body +
    ///    its LLM override, if the profile names a provider) or legacy body.
    /// 2. Otherwise, if scope is "selected_apps_only" → `None` (skip cleanup).
    /// 3. Otherwise ("all_apps") → the global `pp_prompt` body, global endpoint.
    ///
    /// `None` means "inject the raw transcript"; the caller still checks
    /// `post_process_enabled`/base-URL before running any cleanup at all.
    pub fn resolve_cleanup(&self, process: Option<&str>) -> Option<CleanupPlan> {
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
                        // Per-profile LLM override: active iff the profile names
                        // a provider; otherwise inherit the global endpoint.
                        let endpoint = if p.provider.trim().is_empty() {
                            None
                        } else {
                            Some((
                                p.provider.clone(),
                                p.base_url.clone(),
                                p.api_key.clone(),
                                p.model.clone(),
                            ))
                        };
                        return Some(CleanupPlan {
                            body: p.prompt.clone(),
                            endpoint,
                        });
                    }
                }
                if !route.prompt.trim().is_empty() {
                    return Some(CleanupPlan {
                        body: route.prompt.clone(),
                        endpoint: None,
                    });
                }
                return Some(CleanupPlan {
                    body: self.pp_prompt.clone(),
                    endpoint: None,
                });
            }
        }
        if self.routing_scope == "selected_apps_only" {
            None
        } else {
            Some(CleanupPlan {
                body: self.pp_prompt.clone(),
                endpoint: None,
            })
        }
    }
}

/// Apply dictionary corrections to a transcription.
///
/// Case-insensitive, WHOLE-WORD replacement. Word boundaries (`\b`) are added
/// only where the term starts/ends with a word character, so `ai`→`AI` fires on
/// "ai" but NOT inside "rain", while terms with punctuation (e.g. `.md`) still
/// match. Uses a replacement closure so `$` in the target is never treated as a
/// regex backreference.
pub fn apply_dictionary(text: &str, dict: &[DictionaryEntry]) -> String {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let mut out = text.to_string();
    for entry in dict {
        let from = entry.from.trim();
        if from.is_empty() {
            continue;
        }
        let lead = if from.chars().next().is_some_and(is_word) { "\\b" } else { "" };
        let trail = if from.chars().last().is_some_and(is_word) { "\\b" } else { "" };
        let pattern = format!("(?i){}{}{}", lead, regex::escape(from), trail);
        if let Ok(re) = regex::Regex::new(&pattern) {
            let to = entry.to.clone();
            out = re
                .replace_all(&out, |_: &regex::Captures| to.clone())
                .into_owned();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(from: &str, to: &str) -> DictionaryEntry {
        DictionaryEntry {
            from: from.to_string(),
            to: to.to_string(),
        }
    }

    #[test]
    fn dictionary_matches_whole_words_only() {
        let dict = vec![entry("ai", "AI")];
        assert_eq!(apply_dictionary("ai is cool", &dict), "AI is cool");
        assert_eq!(apply_dictionary("AI is cool", &dict), "AI is cool");
        // The bug this fixes: must NOT rewrite inside another word.
        assert_eq!(apply_dictionary("rain again", &dict), "rain again");
    }

    #[test]
    fn dictionary_handles_terms_with_punctuation() {
        let dict = vec![entry("cloud.md", "Claude.md")];
        assert_eq!(
            apply_dictionary("open cloud.md now", &dict),
            "open Claude.md now"
        );
    }

    #[test]
    fn dictionary_target_dollar_is_literal() {
        // `$` in the replacement must not be treated as a regex backreference.
        let dict = vec![entry("price", "$5")];
        assert_eq!(apply_dictionary("the price", &dict), "the $5");
    }

    #[test]
    fn provider_api_key_falls_back_to_the_global_store() {
        let mut cfg = YapConfig::default();
        cfg.pp_provider = "groq".into();
        cfg.pp_api_key = "gsk_active".into();
        cfg.pp_api_keys
            .insert("anthropic".into(), "sk-ant-stored".into());

        // scope key wins when set
        assert_eq!(cfg.provider_api_key("anthropic", "sk-scope"), "sk-scope");
        // empty scope key → per-provider store
        assert_eq!(cfg.provider_api_key("anthropic", ""), "sk-ant-stored");
        // empty scope key + empty store slot, but the provider is the active
        // cleanup provider → the active cleanup key (the user's Groq-tab key)
        assert_eq!(cfg.provider_api_key("groq", ""), "gsk_active");
        // nothing anywhere → empty (caller fails fast with a named error)
        assert_eq!(cfg.provider_api_key("openai", ""), "");
    }
}
