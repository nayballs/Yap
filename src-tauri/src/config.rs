//! Yap configuration: stored as JSON in the app data dir.
//!
//! Deliberately tiny — a dictation tool only needs a hotkey, a model,
//! GPU toggle, the sound cue, and the correction dictionary.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A single transcription correction: replace `from` with `to`.
/// Post-processing fix for words the STT model mishears
/// (e.g. "Power to Keep" -> "Parakeet"). Model-agnostic.
///
/// With `snippet` set it's a **snippet** instead (voice text expansion):
/// `from` is the phrase you say, `to` the text Yap types in its place (it may
/// span lines). See [`apply_snippets`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryEntry {
    pub from: String,
    pub to: String,
    /// Whether the fuzzy near-miss pass (fuzzy.rs) may match this entry.
    /// Off for corrections whose near-misses are real words — `json → JSON`
    /// would otherwise eat the name "Jason". Exact replacement and the
    /// Whisper `initial_prompt` bias always apply regardless.
    #[serde(default = "default_true")]
    pub fuzzy: bool,
    /// A snippet, not a correction: say `from`, get `to` (verbatim). Applied
    /// to dictations only, after cleanup and the corrections; never part of
    /// the exact/fuzzy correction passes or the Whisper vocabulary prompt.
    #[serde(default)]
    pub snippet: bool,
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
/// style "Language Models" bubbles: Dictation Cleanup, Commands (key
/// `voiceAgent`, its name before 2026-10-18), Note Formatting, Chat & Voice
/// Assistant (key `chat`). Each scope carries its own provider/model/key/prompt
/// so a heavy cloud model can drive Commands while cleanup runs on the fast
/// local sidecar, etc. (OpenWhispr's `INFERENCE_SCOPES`, one config per bubble.)
///
/// **Back-compat:** the Dictation Cleanup scope is NOT stored here — it stays in
/// the top-level `pp_*` fields, which the whole cleanup pipeline already reads.
/// This map (`YapConfig::llm_scopes`) holds only the three newer scopes, keyed
/// `"voiceAgent"|"noteFormatting"|"chat"`. A missing key = the scope's feature is
/// off / unconfigured; an empty `provider` inherits the global cleanup endpoint;
/// Note Formatting falls back to the Cleanup scope (OpenWhispr `fallbackScope`).
/// Commands run in `pipeline::run_agent` (the wake word and edit mode), Note
/// Formatting in the notes' Actions engine, Chat in `commands::chat_turn`
/// (the Chat view and the Voice Assistant). The keys never change: they are
/// what config.json holds.
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

/// A nickname for an app Yap can open (Settings → Voice Assistant → Opening
/// apps): say `say` ("my editor") and Yap opens the app whose id is `app`
/// (`apps.rs`: an entry of the Start menu's list). `name` is the app's name
/// when the nickname was made, shown while the app isn't installed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppNickname {
    pub say: String,
    pub app: String,
    #[serde(default)]
    pub name: String,
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

/// A cloud voice service's choices (`tts_cloud`): its voice and model.
/// Empty = the service's default (`tts::elevenlabs::DEFAULT_VOICE`,
/// `DEFAULT_MODEL`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CloudVoiceChoice {
    /// The service's voice id ("JBFqnCBsd6RMkjVDRZzb").
    pub voice: String,
    /// Its name, to show while the list can't be read ("George").
    pub voice_name: String,
    /// The model id ("eleven_flash_v2_5").
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct YapConfig {
    /// Global hotkey for the input hook. Format: "kb:VKEY" or "mouse:ID".
    /// Default kb:120 = F9.
    #[serde(default = "default_hotkey")]
    pub hotkey: String,
    /// Optional second hotkey for **edit/rewrite mode** (one of the
    /// **Commands**, Settings → Commands): capture the selected text, then
    /// treat your spoken words as an instruction to rewrite it (or, if
    /// nothing is selected, to write new text). Empty = unbound (opt-in).
    /// Same "kb:VKEY"/"mouse:ID" format as `hotkey`.
    #[serde(default)]
    pub edit_hotkey: String,
    /// The **Voice Assistant** shortcut (Settings → Voice Assistant,
    /// `assistant.rs`): from any app, ask Yap's Chat something out loud and
    /// hear the answer; it never types into the app you're in. Empty =
    /// unbound (the default). Same spec format as `hotkey`; it never shares
    /// a key with the other three (`input_hook::clashes`).
    #[serde(default)]
    pub assistant_hotkey: String,
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
    /// Transcription corrections.
    #[serde(default)]
    pub dictionary: Vec<DictionaryEntry>,
    /// Fuzzy near-miss correction: snap words that ALMOST match a dictionary
    /// spelling (Levenshtein + phonetic, Handy's algorithm — see fuzzy.rs).
    /// Applies to ONNX models only; Whisper models get the dictionary as an
    /// `initial_prompt` bias instead.
    #[serde(default = "default_true")]
    pub dictionary_fuzzy: bool,
    /// "Suggest words from your corrections" (the Dictionary view): after a
    /// paste Yap watches the field for a little while (UI Automation, in
    /// memory only) and, when you fix a word it got wrong, offers to add it
    /// to the dictionary. Never adds one without asking. See `learn.rs`.
    #[serde(default = "default_true")]
    pub learn_words: bool,
    /// "Show tips" (Settings → General → Notifications): coaching hints and
    /// nudges ("Connect your calendar"). Off, Yap's notice policy drops every
    /// tip (`notice.rs`); alerts, questions and notices still show. On by
    /// default.
    #[serde(default = "default_true")]
    pub show_tips: bool,

    /// Recording mode: "toggle" (press to start, press again to stop) or
    /// "pushToTalk" (hold to record, release to stop).
    #[serde(default = "default_recording_mode")]
    pub recording_mode: String,
    /// "Silence other apps while you dictate" (`mute.rs`): once the start
    /// chime has played, other apps' audio sessions go quiet, and after the
    /// stop chime each comes back to exactly its level. Never Yap's own
    /// sound, a call app's, or a browser holding a meeting. Off by default.
    /// Configs from before 2026-10-17 call it `muteWhileRecording` (it muted
    /// the whole output device, the chime and any call included): the alias
    /// carries the choice over.
    #[serde(default, alias = "muteWhileRecording")]
    pub quiet_other_apps: bool,
    /// "Pause music and videos while you dictate" (`media_pause.rs`): media
    /// that's playing (Windows' media controls: Spotify, a YouTube tab…) is
    /// paused once the start chime has played and played again after the
    /// stop chime — only what Yap paused. Never a call app. Off by default,
    /// as Wispr Flow's.
    #[serde(default)]
    pub pause_media: bool,
    /// Append a single space after each injected transcription. With
    /// `smart_spacing` on it only applies where Yap couldn't read the field.
    #[serde(default)]
    pub append_trailing_space: bool,
    /// "Match the text around the cursor" (Settings → Advanced → Output): at
    /// record start Yap reads a little text either side of the caret over UI
    /// Automation (`selection::begin_dictation`, in memory only) and fits the
    /// dictation to it — spaces, a capital mid-sentence, a full stop before
    /// more of the sentence (`smart_insert.rs`). Where the field can't be
    /// read, a dictation pastes exactly as without it.
    #[serde(default = "default_true")]
    pub smart_spacing: bool,
    /// "Leave the full stop off chat messages" (Settings → Advanced →
    /// Output): in a chat app (Slack, Discord, WhatsApp, Teams…, desktop or
    /// in a browser; `app_kind.rs`), a message of one or two sentences in one
    /// paragraph pastes without its final full stop.
    #[serde(default = "default_true")]
    pub chat_no_full_stop: bool,
    /// After injecting text, simulate pressing Enter (auto-submit).
    #[serde(default)]
    pub auto_submit: bool,
    /// The "press enter" voice command: a dictation that ENDS with "press
    /// enter" (or "press return") has the phrase taken off and the submit key
    /// (`auto_submit_key`) pressed after the paste — once, even with
    /// `auto_submit` on. See `pipeline::take_voice_enter`.
    #[serde(default = "default_true")]
    pub voice_enter: bool,
    /// "Safer pasting into terminals" (Settings → Advanced → Output): in a
    /// terminal (`app_kind.rs`) a dictation pastes as one line with no space
    /// after it, and Enter is pressed only for "press enter" (auto-submit
    /// only in a coding CLI, where it sends a prompt); Claude Code and Codex
    /// in Windows Terminal keep the line breaks. The text around a
    /// terminal's cursor isn't read.
    #[serde(default = "default_true")]
    pub terminal_mode: bool,
    /// Restore the user's previous clipboard contents after pasting.
    #[serde(default = "default_true")]
    pub restore_clipboard: bool,
    /// Don't show any window on launch (dictation still works via the hotkey).
    #[serde(default)]
    pub start_hidden: bool,
    /// Deprecated (2026-07-09, pill retirement): the tray is Yap's only
    /// persistent surface now and is always built; this field is kept so old
    /// configs load but is no longer consulted.
    #[serde(default = "default_true")]
    pub show_tray_icon: bool,
    /// Launch Yap at OS login.
    #[serde(default)]
    pub autostart: bool,
    /// Chime volume (0.0–1.0).
    #[serde(default = "default_audio_feedback_volume")]
    pub audio_feedback_volume: f32,
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
    /// The Yap bar (`bar.rs`): a small pill above the taskbar on the monitor
    /// you're using, to dictate or take meeting notes, and where Yap's
    /// notices show while its window isn't focused. Off: the overlay shows
    /// only while dictating and notices come as Windows notifications.
    #[serde(default = "default_true")]
    pub bar_enabled: bool,
    /// Hide the idle bar while an app is fullscreen on its monitor (games,
    /// videos, slideshows). It always shows while recording.
    #[serde(default = "default_true")]
    pub bar_hide_fullscreen: bool,
    /// The bar's colours: "light" (Yap's white cards and ink buttons, the
    /// default) or "dark" (the in-app toasts' warm near-black). The
    /// `--bar-*` tokens in app.css; the page sets `data-bar-theme`.
    #[serde(default = "default_bar_theme")]
    pub bar_theme: String,
    /// The dictation capsule's ✕ (cancel) and ✓ (stop and paste) while it
    /// records: "off" (the default: Cancel stays in the tray), "hover" (only
    /// while the pointer is on the capsule) or "always". Never while the
    /// push-to-talk key is held. `bar.rs` carries it to the bar page.
    #[serde(default = "default_bar_dictation_buttons")]
    pub bar_dictation_buttons: String,
    /// Settings → General → Appearance: the font of the reading text —
    /// Chat's messages, notes and their Enhanced view, the meeting notepad's
    /// My thoughts / Transcript / Summary and its Ask answers; never the
    /// app's buttons and menus. "default" (Figtree, the UI's own), "sans"
    /// (Geist), "serif" (Literata), "system" (Segoe UI) or "dyslexic"
    /// (OpenDyslexic). Every window sets `data-reading-font` from it
    /// (`src/lib/appearance.js`, the `--yap-font-reading` token in app.css).
    #[serde(default = "default_reading_font")]
    pub reading_font: String,
    /// The reading text's size: "small", "default", "large" or "larger" (a
    /// step around each view's own size: `--yap-reading-scale`).
    #[serde(default = "default_reading_size")]
    pub reading_size: String,
    /// Motion in every Yap window: "system" (follows Windows' "Animation
    /// effects", i.e. `prefers-reduced-motion`) or "reduced" (no
    /// non-essential animations or transitions; spinners and countdowns
    /// stay).
    #[serde(default = "default_motion")]
    pub motion: String,
    /// Which key auto-submit presses after pasting: "enter", "ctrlEnter",
    /// or "shiftEnter".
    #[serde(default = "default_auto_submit_key")]
    pub auto_submit_key: String,
    /// Automatically check GitHub Releases for a newer Yap on launch.
    #[serde(default = "default_true")]
    pub update_checks_enabled: bool,
    /// Debug mode (Settings → Advanced → Debug Logging, OpenWhispr-style):
    /// raises Yap's file/console log level to `debug` for troubleshooting.
    /// A RUST_LOG env var overrides it. Applied live on save.
    #[serde(default)]
    pub debug_logging: bool,

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
    /// The bigger on-device model offered once in Settings (Qwen3.5-4B on a
    /// PC whose graphics card can run it, `local_llm::upgrade_offer`) was
    /// turned down ("No thanks"), or taken: it isn't offered again.
    #[serde(default)]
    pub local_llm_upgrade_dismissed: bool,
    /// The on-device model's **context**, in tokens: how much text the
    /// sidecar reads at once (`-c`), shared by Chat, the cleanup and the
    /// meeting notes. 0 (or missing) = the default, 8,192
    /// (`local_llm::DEFAULT_CONTEXT`); clamped to what the model was trained
    /// for and a floor (`local_llm::effective_context`). Changing it restarts
    /// the sidecar, never mid-dictation (`local_llm::restart_when_idle`).
    #[serde(default)]
    pub local_context: u32,
    /// **MTP drafting** for the on-device model (`--spec-type draft-mtp`:
    /// Qwen3.5's own multi-token-prediction layer drafts the next tokens,
    /// the model checks them): on wherever it can be — llamafile 0.10.6 or
    /// newer, a model with the layer, all of it on a graphics card with room
    /// for it (`local_llm::plan_for`). No switch in Settings: `false` here
    /// (config.json) turns it off, should a driver or model misbehave with it.
    #[serde(default = "default_true")]
    pub local_mtp: bool,
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
    /// The name **Commands** answer to (OpenWhispr "Agent Name"; was "the
    /// Voice Agent's wake word"): say it during a dictation and the AI does
    /// your spoken command instead of typing it. Empty = "Yap". Also added to
    /// the dictionary so the STT spells it right. (The name field is in
    /// Settings → Commands; the model and prompt are Language Models →
    /// Commands, the `voiceAgent` scope — its key kept for old configs.)
    #[serde(default)]
    pub agent_name: String,

    /// **Spoken replies** (Settings → Voice Assistant, the Chat view's 🔊):
    /// Chat's answers read aloud (`tts.rs`). Off by default.
    #[serde(default)]
    pub speak_replies: bool,
    /// "When to speak": "voice" (only an answer to a question you dictated,
    /// the default) or "always" (every Chat answer).
    #[serde(default = "default_speak_when")]
    pub speak_when: String,
    /// "Talk back hands-free": a question dictated into the chat box is sent
    /// when the dictation ends (no Enter). Part of spoken replies: only while
    /// `speak_replies` is on (`tts::sends_by_itself`). On by default.
    #[serde(default = "default_true")]
    pub talk_back_hands_free: bool,
    /// "Show the answer on the Yap bar" (Settings → Voice Assistant): while
    /// the assistant shortcut's answer is read aloud, the question and the
    /// answer show in a card above the bar. Off: just the voice (■ stays on
    /// the capsule). An answer that isn't read aloud (a meeting recording, a
    /// dictation started, the voice failed) shows anyway, or it would be
    /// lost (`assistant::card_on_bar`). On by default.
    #[serde(default = "default_true")]
    pub assistant_answer_card: bool,
    /// "Let Yap open apps when you ask" (Settings → Voice Assistant → Opening
    /// apps; `apps.rs`): "open Spotify" in Chat or to the Voice Assistant
    /// opens an app from the Start menu's list of apps installed on this PC
    /// — never a command, a file or a link. On by default: it's local and
    /// only ever opens an app from that list.
    #[serde(default = "default_true")]
    pub open_apps: bool,
    /// Apps Yap won't open ("Don't open this one"): their ids in that list.
    #[serde(default)]
    pub apps_blocked: Vec<String>,
    /// "Say … → open …": nicknames for apps ([`AppNickname`]).
    #[serde(default)]
    pub app_nicknames: Vec<AppNickname>,
    /// "Look things up on the web" (Settings → Voice Assistant → Web search;
    /// `web.rs`): Chat and the Voice Assistant may search the web and read
    /// pages when a question needs it, showing the sources. Off by default:
    /// the search words leave the PC.
    #[serde(default)]
    pub web_search: bool,
    /// "The AI's own search" (the default, recommended): a cloud model
    /// (Groq, Gemini, Claude, OpenAI, OpenRouter) searches with its
    /// provider's own search (`web::native`), on the key Chat already uses;
    /// local and self-hosted models — and any search the provider's can't
    /// do — use `web_search_service`. Off: every model uses the service.
    #[serde(default = "default_true")]
    pub web_search_native: bool,
    /// Yap's own search service with a key: "exa" (the default,
    /// recommended) or "tavily" — for local models, and for every model
    /// when `web_search_native` is off. Without its key, Yap uses Wikipedia
    /// and game wikis.
    #[serde(default = "default_web_search_service")]
    pub web_search_service: String,
    /// Each service's key (service id → key), like `pp_api_keys`: stored
    /// in config.json, never logged.
    #[serde(default)]
    pub web_search_keys: std::collections::HashMap<String, String>,
    /// The speech engine (`tts::ENGINES`): "kokoro" (the default: Kokoro's
    /// natural voices once downloaded, Windows voices until then) or
    /// "windows" (Windows' own voices, chosen).
    #[serde(default = "default_tts_engine")]
    pub tts_engine: String,
    /// Kokoro's voice (`tts::kokoro::VOICES`); af_bella by default.
    #[serde(default = "default_tts_voice")]
    pub tts_voice: String,
    /// The Windows voice's id; empty = Windows' default voice.
    #[serde(default)]
    pub tts_windows_voice: String,
    /// Speaking speed, 1.0 = normal (0.5–2.0).
    #[serde(default = "default_tts_rate")]
    pub tts_rate: f32,
    /// Spoken replies' volume, Yap's own (0.0–1.0, on a curve: `tts::gain`),
    /// under the Windows mixer's. Half by default.
    #[serde(default = "default_tts_volume")]
    pub tts_volume: f32,
    /// Who reads spoken replies (`tts::cloud::PROVIDERS`): "local" (the
    /// default — Kokoro or Windows voices, on this PC) or "elevenlabs"
    /// (ElevenLabs' voices on the person's own key: the words Yap speaks go
    /// there; until a key is set the local voice speaks).
    #[serde(default = "default_tts_provider")]
    pub tts_provider: String,
    /// Each cloud voice service's API key (service id → key), like
    /// `web_search_keys`: stored in config.json, never logged, sent only to
    /// that service, in a header.
    #[serde(default)]
    pub tts_cloud_keys: std::collections::HashMap<String, String>,
    /// Each cloud voice service's voice and model (service id → choice).
    #[serde(default)]
    pub tts_cloud: std::collections::HashMap<String, CloudVoiceChoice>,

    /// Show live partial transcripts in the overlay while you speak. On by
    /// default (2026-07-10, after live validation): the worker transcribes a
    /// bounded sliding window on a timer (see `partials.rs`), so the cost is
    /// independent of recording length. Preview-only — the final transcript on
    /// stop is always authoritative. Existing configs that saved an explicit
    /// `false` keep their opt-out (serde default applies to missing fields) —
    /// except once: `Default` said off until 2026-10-05, so the v1 one-time
    /// migration (`migrate_once`) turned pre-versioning `false`s back on.
    #[serde(default = "default_true")]
    pub streaming_partials: bool,

    /// Keep a local transcription history (powers the stats dashboard). Stored
    /// only on this machine; can be cleared from Settings. On by default.
    #[serde(default = "default_true")]
    pub history_enabled: bool,

    /// Run the local API bridge (Integrations): a token-authenticated loopback
    /// HTTP server exposing notes/folders/history to CLIs and coding agents
    /// (`bridge.rs`, OpenWhispr cliBridge port). On by default, like OpenWhispr;
    /// loopback-only + bearer token, so nothing is reachable off-machine.
    #[serde(default = "default_true")]
    pub bridge_enabled: bool,
    /// Let AI apps save notes through Yap's MCP server (its `create_note`
    /// tool; Settings → MCP). Off by default: they can only read.
    #[serde(default)]
    pub mcp_allow_writes: bool,

    /// Open the meeting notepad when a meeting recording starts (Settings →
    /// General → Meetings; `notepad.rs`): docked to the right edge of the
    /// screen beside the call, without taking the focus. On by default, as
    /// Wispr Flow's "Open Notepad when starting Notetaker".
    #[serde(default = "default_true")]
    pub meeting_open_notepad: bool,
    /// "Split the screen when joining": when a recording starts during a
    /// detected call, put the call's window on the left of the screen and the
    /// notepad on the right. Off by default (it moves another app's window).
    #[serde(default)]
    pub meeting_split_screen: bool,
    /// "Show live transcript": the notepad's Transcript tab shows the lines
    /// as they're transcribed. Off, it stays quiet until the meeting stops
    /// (transcription goes on either way). On by default, as Wispr Flow.
    #[serde(default = "default_true")]
    pub meeting_live_transcript: bool,
    /// The message the notepad's "Copy consent message" copies for the
    /// meeting chat, as the person edited it. Empty: Yap's default.
    #[serde(default)]
    pub meeting_consent_message: String,
    /// "Tell speakers apart in meetings" (beta; Settings → Speech-to-Text,
    /// and General → Meetings): label the voices on the call side while a
    /// meeting records, on this PC (`diarize.rs`). Off by default, and it
    /// only runs once its model is installed.
    #[serde(default)]
    pub meeting_diarization: bool,
    /// Which diarization model (`diarize/registry.rs` id) it uses.
    #[serde(default = "default_diarization_model")]
    pub diarization_model: String,

    /// Call detection (`meeting_detect.rs`): when Teams, Zoom, Meet, Slack,
    /// Discord, Webex… start using the mic, offer to take notes, and offer to
    /// stop and summarise when the call ends. On by default, like OpenWhispr's
    /// `notifyMeetingDetection`: it only reads Windows' own mic-usage record,
    /// locally, and nothing records without a click.
    #[serde(default = "default_true")]
    pub meeting_detection: bool,
    /// Start notes on a detected call by itself after a 10-second countdown
    /// on its prompt, unless cancelled (Wispr's "Meeting detected" card).
    /// Off by default: nothing records without a click.
    #[serde(default)]
    pub meeting_auto_start: bool,
    /// Which call apps it asks about (Settings → General → Meetings, or "Don't
    /// ask for Discord" on a prompt): app id → the person's choice. An app
    /// without an entry uses its default in `meeting_detect::APPS` (work apps
    /// yes, personal chat apps no), so apps added later start at their own.
    #[serde(default)]
    pub meeting_detect_apps: std::collections::BTreeMap<String, bool>,
    /// How it asks: "popup" (a card in the main window, or a Windows
    /// notification while Yap is in the background) or "quiet" (silently into
    /// the notification center, plus a tray menu item).
    #[serde(default = "default_meeting_detect_style")]
    pub meeting_detect_style: String,

    // ---- Meeting guard rails (Settings → General → Meetings) ----
    /// Hide Yap's meeting windows (the notepad and the recording overlay)
    /// from screenshots and screen sharing while a meeting records
    /// (`capture.rs`: Windows' exclude-from-capture flag). On by default, as
    /// Wispr Flow's "Don't show Notepad and Flow Bar in screen capture"; with
    /// it off, a meeting starting warns that the notes show in a share.
    #[serde(default = "default_true")]
    pub meeting_hide_from_capture: bool,
    /// Longest one meeting recording runs, in minutes (0 = no limit): Yap
    /// warns 5 minutes before, then stops and writes the action plan
    /// (`meeting_guard.rs`). 2 hours by default, as Wispr Flow.
    #[serde(default = "default_meeting_max_minutes")]
    pub meeting_max_minutes: u32,
    /// When a call Yap is recording ends: "ask" (the "Stop and summarise?"
    /// prompt, the default) or "stop" (stop and summarise automatically).
    #[serde(default = "default_meeting_call_end")]
    pub meeting_call_end: String,
    /// "End notes when the meeting goes quiet" (`meeting_guard.rs`): after
    /// 10 minutes with nobody speaking on either side, "Ending notes in
    /// 30 s…" counts down (Keep going / Stop now; anyone speaking cancels
    /// it), then the recording stops and the action plan is written. On by
    /// default, as Wispr Flow's.
    #[serde(default = "default_true")]
    pub meeting_end_when_quiet: bool,
    /// The global meeting shortcut (same spec format as `hotkey`): take notes
    /// on the live call (or in a new meeting note), or stop and write the
    /// action plan. Empty = unbound. Default Win+Alt+M, as Wispr Flow.
    #[serde(default = "default_meeting_hotkey")]
    pub meeting_hotkey: String,

    /// "Notify before scheduled meetings start" (Wispr's setting, read by
    /// `calendar.rs`): "15s" (right before, the default), "1min", "2min" or
    /// "never". Only matters once a calendar is connected (Settings →
    /// Connectors); the connections themselves live in calendar.json and
    /// Credential Manager, never here.
    #[serde(default = "default_meeting_reminder")]
    pub meeting_reminder: String,

    // ---- Memory (Settings → Memory; `memory.rs`) ----
    // The switches only: what Yap remembers lives in memory.json, never
    // here (Settings' wholesale saves would put an old copy back).
    /// "Search and reference past chats": Chat may look through earlier
    /// conversations (`search_chats`). On by default.
    #[serde(default = "default_true")]
    pub memory_search_chats: bool,
    /// "Learn from my chats": a quiet conversation is read for lasting
    /// facts about the person. On by default.
    #[serde(default = "default_true")]
    pub memory_learn_chats: bool,
    /// "Learn from meetings": a finished meeting's attendees and action plan
    /// (other people's words: off by default).
    #[serde(default)]
    pub memory_learn_meetings: bool,
    /// "Include sensitive topics in memory" (health, religion, politics…):
    /// off by default.
    #[serde(default)]
    pub memory_sensitive: bool,

    // ---- About you (Settings → General; `about_you.rs`) ----
    // Told to the AI model the person uses, with each question; kept here,
    // never sent to Yap's account service. (Until 22 Oct 2026, before it
    // shipped, the region had language, currency, units and time zone
    // fields of its own; a config.json that still has them loads, and they
    // go on its next save.)
    /// "What should Yap call you?" (optional).
    #[serde(default)]
    pub about_name: String,
    /// "Anything the AI should know about you" (optional, ≤ 300 characters).
    #[serde(default)]
    pub about_me: String,
    /// The region: a country, ISO 3166-1 alpha-2 ("GB"); empty = Automatic
    /// (Windows' home location). Its currency, units, language and spelling
    /// follow it; the time zone is the PC's.
    #[serde(default)]
    pub region_country: String,
    /// "Write in my region's spelling" (Language Models → Dictation
    /// cleanup): cleanup keeps the region's variety of English (British
    /// "colour", American "color"). On by default.
    #[serde(default = "default_true")]
    pub region_spelling: bool,

    /// Schema version, for the one-time migrations in `migrate_once` (the
    /// kind that must never run twice, unlike `load`'s idempotent fixes). A
    /// config saved before versioning has no field (0); `Default` and every
    /// [`save`] use the current `CONFIG_VERSION`.
    #[serde(default)]
    pub config_version: u32,
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
    "openai/gpt-oss-20b".into()
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
fn default_meeting_detect_style() -> String {
    "popup".into()
}
fn default_bar_theme() -> String {
    "light".into()
}
fn default_bar_dictation_buttons() -> String {
    "off".into()
}
fn default_reading_font() -> String {
    "default".into()
}
fn default_reading_size() -> String {
    "default".into()
}
fn default_motion() -> String {
    "system".into()
}
fn default_meeting_max_minutes() -> u32 {
    120
}
fn default_meeting_call_end() -> String {
    "ask".into()
}
fn default_meeting_hotkey() -> String {
    // Win+Alt+M (VK 'M' = 77), Wispr Flow's Notetaker shortcut.
    "kb:alt+win+77".into()
}
fn default_meeting_reminder() -> String {
    "15s".into()
}
fn default_web_search_service() -> String {
    "exa".into()
}
fn default_speak_when() -> String {
    "voice".into()
}
fn default_tts_engine() -> String {
    crate::tts::DEFAULT_ENGINE.into()
}
fn default_tts_voice() -> String {
    crate::tts::kokoro::DEFAULT_VOICE.into()
}
fn default_tts_rate() -> f32 {
    1.0
}
fn default_tts_volume() -> f32 {
    0.5
}
fn default_tts_provider() -> String {
    crate::tts::cloud::LOCAL.into()
}
fn default_diarization_model() -> String {
    crate::diarize::DEFAULT_MODEL.into()
}

impl Default for YapConfig {
    fn default() -> Self {
        Self {
            hotkey: default_hotkey(),
            edit_hotkey: String::new(),
            assistant_hotkey: String::new(),
            model_size: default_model_size(),
            use_gpu: true,
            input_device: None,
            sound_enabled: true,
            dictionary: Vec::new(),
            dictionary_fuzzy: true,
            learn_words: true,
            show_tips: true,
            recording_mode: default_recording_mode(),
            quiet_other_apps: false,
            pause_media: false,
            append_trailing_space: false,
            smart_spacing: true,
            chat_no_full_stop: true,
            auto_submit: false,
            voice_enter: true,
            terminal_mode: true,
            restore_clipboard: true,
            start_hidden: false,
            show_tray_icon: true,
            autostart: false,
            audio_feedback_volume: default_audio_feedback_volume(),
            selected_language: default_selected_language(),
            translate_to_english: false,
            model_unload_timeout: default_model_unload_timeout(),
            output_device: None,
            overlay_position: default_overlay_position(),
            bar_enabled: true,
            bar_hide_fullscreen: true,
            bar_theme: default_bar_theme(),
            bar_dictation_buttons: default_bar_dictation_buttons(),
            reading_font: default_reading_font(),
            reading_size: default_reading_size(),
            motion: default_motion(),
            auto_submit_key: default_auto_submit_key(),
            update_checks_enabled: true,
            debug_logging: false,
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
            local_llm_upgrade_dismissed: false,
            local_context: 0,
            local_mtp: true,
            routing_scope: default_routing_scope(),
            app_routes: Vec::new(),
            cleanup_profiles: Vec::new(),
            llm_scopes: std::collections::HashMap::new(),
            agent_name: String::new(),
            speak_replies: false,
            speak_when: default_speak_when(),
            talk_back_hands_free: true,
            assistant_answer_card: true,
            open_apps: true,
            apps_blocked: Vec::new(),
            app_nicknames: Vec::new(),
            web_search: false,
            web_search_native: true,
            web_search_service: default_web_search_service(),
            web_search_keys: std::collections::HashMap::new(),
            tts_engine: default_tts_engine(),
            tts_voice: default_tts_voice(),
            tts_windows_voice: String::new(),
            tts_rate: default_tts_rate(),
            tts_volume: default_tts_volume(),
            tts_provider: default_tts_provider(),
            tts_cloud_keys: std::collections::HashMap::new(),
            tts_cloud: std::collections::HashMap::new(),
            streaming_partials: true,
            history_enabled: true,
            bridge_enabled: true,
            mcp_allow_writes: false,
            meeting_open_notepad: true,
            meeting_split_screen: false,
            meeting_live_transcript: true,
            meeting_consent_message: String::new(),
            meeting_diarization: false,
            diarization_model: default_diarization_model(),
            meeting_detection: true,
            meeting_auto_start: false,
            meeting_detect_apps: std::collections::BTreeMap::new(),
            meeting_detect_style: default_meeting_detect_style(),
            meeting_hide_from_capture: true,
            meeting_max_minutes: default_meeting_max_minutes(),
            meeting_call_end: default_meeting_call_end(),
            meeting_end_when_quiet: true,
            meeting_hotkey: default_meeting_hotkey(),
            meeting_reminder: default_meeting_reminder(),
            memory_search_chats: true,
            memory_learn_chats: true,
            memory_learn_meetings: false,
            memory_sensitive: false,
            about_name: String::new(),
            about_me: String::new(),
            region_country: String::new(),
            region_spelling: true,
            config_version: CONFIG_VERSION,
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
    // A model evaluation (`eval.rs`) runs on its own settings, in memory (its
    // API key never goes to disk).
    if let Some(cfg) = crate::eval::config() {
        return cfg;
    }
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
    migrate_retired_groq_models(&mut cfg);
    // One-time migrations are saved straight away, so they can never run a
    // second time and undo a choice the user makes afterwards.
    if migrate_once(&mut cfg) {
        if let Err(e) = save(&cfg) {
            tracing::warn!("Failed to save migrated config: {}", e);
        }
    }
    cfg
}

/// Groq models retired in 2026 (non-Enterprise keys get HTTP 404 "does not
/// exist or you do not have access"), mapped to the replacements Groq names
/// on https://console.groq.com/docs/deprecations.
const RETIRED_GROQ_MODELS: [(&str, &str); 7] = [
    ("llama-3.1-8b-instant", "openai/gpt-oss-20b"),
    ("llama-3.3-70b-versatile", "openai/gpt-oss-120b"),
    ("qwen/qwen3-32b", "openai/gpt-oss-120b"),
    ("meta-llama/llama-4-scout-17b-16e-instruct", "openai/gpt-oss-120b"),
    ("moonshotai/kimi-k2-instruct-0905", "openai/gpt-oss-120b"),
    ("groq/compound", "openai/gpt-oss-120b"),
    ("groq/compound-mini", "openai/gpt-oss-20b"),
];

/// Is `model` one Groq retired (and Yap migrates away from)? The live model
/// list (`model_catalog.rs`) never offers one.
pub fn is_retired_groq_model(model: &str) -> bool {
    RETIRED_GROQ_MODELS.iter().any(|(old, _)| *old == model)
}

/// Swap a retired Groq model id for its replacement; other providers and ids
/// are left alone (a self-hosted server may legitimately use the same name).
fn migrate_groq_model(provider: &str, model: &mut String) {
    if provider != "groq" {
        return;
    }
    if let Some((_, new)) = RETIRED_GROQ_MODELS.iter().find(|(old, _)| *old == model.as_str()) {
        *model = (*new).to_string();
    }
}

/// Migration: move every saved Groq selection (cleanup, LLM scopes, cleanup
/// profiles) off retired models, so existing configs don't fail every call.
fn migrate_retired_groq_models(cfg: &mut YapConfig) {
    migrate_groq_model(&cfg.pp_provider, &mut cfg.pp_model);
    for s in cfg.llm_scopes.values_mut() {
        migrate_groq_model(&s.provider, &mut s.model);
    }
    for p in &mut cfg.cleanup_profiles {
        migrate_groq_model(&p.provider, &mut p.model);
    }
}

/// Current config schema version (see [`YapConfig::config_version`]). Bump it
/// together with a new step in [`migrate_once`].
const CONFIG_VERSION: u32 = 2;

/// One-time migrations, keyed by `config_version`. Returns whether anything
/// ran; [`load`] then saves the result so no step ever runs twice.
fn migrate_once(cfg: &mut YapConfig) -> bool {
    if cfg.config_version >= CONFIG_VERSION {
        return false;
    }
    if cfg.config_version < 1 {
        // v1 (2026-10-05): live partials became on-by-default on 2026-07-10,
        // but `Default` still said off, so every install created since then
        // saved `false` without the user choosing it. Turn it back on once;
        // anyone who switches it off again keeps it off.
        cfg.streaming_partials = true;
    }
    if cfg.config_version < 2 {
        // v2 (2026-10-18): Kokoro became the voice. Until then Windows voices
        // were the only engine (so "windows" was never a choice) and
        // `tts_voice` held a Windows voice's id: it moves to
        // `tts_windows_voice`, and Kokoro speaks in af_bella once installed.
        if cfg.tts_engine == crate::tts::WINDOWS {
            cfg.tts_engine = crate::tts::DEFAULT_ENGINE.into();
        }
        if !crate::tts::kokoro::is_voice(&cfg.tts_voice) {
            if cfg.tts_windows_voice.is_empty() {
                cfg.tts_windows_voice = std::mem::take(&mut cfg.tts_voice);
            }
            cfg.tts_voice = default_tts_voice();
        }
    }
    cfg.config_version = CONFIG_VERSION;
    true
}

/// Atomic file write: write to `<path>.tmp`, then rename over the target
/// (`std::fs::rename` replaces on Windows). A `tauri dev` rebuild or crash
/// mid-write can otherwise leave a torn JSON file — which the stores would
/// fail to parse and (before this fix) silently replace with an empty
/// default, destroying user data.
pub(crate) fn atomic_write(path: &std::path::Path, contents: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, contents)?;
    std::fs::rename(&tmp, path)
}

/// Move an unparseable store file aside (`<name>.corrupt-<unix-secs>`) so it
/// is NEVER overwritten by a fresh default — the data stays recoverable.
pub(crate) fn quarantine_corrupt(path: &std::path::Path) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let backup = path.with_extension(format!("corrupt-{ts}"));
    match std::fs::rename(path, &backup) {
        Ok(_) => tracing::error!(
            "Unparseable store {:?} quarantined to {:?} — starting fresh, data preserved",
            path,
            backup
        ),
        Err(e) => tracing::error!("Failed to quarantine corrupt store {:?}: {}", path, e),
    }
}

/// Persist config to disk (atomically).
pub fn save(cfg: &YapConfig) -> Result<(), String> {
    if crate::eval::active() {
        return Ok(()); // a model evaluation's settings stay in memory
    }
    std::fs::create_dir_all(data_dir()).map_err(|e| e.to_string())?;
    // Anything this build saves has been through `load`'s migrations already,
    // so stamp the current version even if the caller's copy lost the field —
    // a missing version would re-run the one-time migrations on next load.
    // Never lower it (a newer build's file, opened after a downgrade).
    let cfg = YapConfig {
        config_version: cfg.config_version.max(CONFIG_VERSION),
        ..cfg.clone()
    };
    let json = serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
    atomic_write(&config_path(), &json).map_err(|e| e.to_string())
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
    let mut out = text.to_string();
    for entry in dict.iter().filter(|e| !e.snippet) {
        if let Some(re) = term_regex(&entry.from) {
            let to = entry.to.clone();
            out = re
                .replace_all(&out, |_: &regex::Captures| to.clone())
                .into_owned();
        }
    }
    out
}

/// The case-insensitive, whole-word matcher for one dictionary term (see
/// [`apply_dictionary`]): `\b` only where the term starts/ends with a word
/// character. `None` for an empty term.
pub(crate) fn term_regex(term: &str) -> Option<regex::Regex> {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let term = term.trim();
    if term.is_empty() {
        return None;
    }
    let lead = if term.chars().next().is_some_and(is_word) { "\\b" } else { "" };
    let trail = if term.chars().last().is_some_and(is_word) { "\\b" } else { "" };
    regex::Regex::new(&format!("(?i){}{}{}", lead, regex::escape(term), trail)).ok()
}

/// The words of a snippet's spoken phrase, as matching sees them: runs of
/// letters and digits, lowercased ("My sign-off!" → my, sign, off).
fn phrase_words(phrase: &str) -> Vec<String> {
    phrase
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// Expand **snippets** (voice text expansion): wherever a snippet's spoken
/// phrase (`from`) appears as a whole phrase, put its text (`to`) in its
/// place, verbatim. Run on a dictation's final text, after AI cleanup and the
/// corrections, so the matching forgives what those did around the phrase:
/// case, and any punctuation or spacing between and around its words
/// ("My sign-off." matches "my sign off"). One pass, longest phrase first
/// ("my email address" beats "my email"), so an inserted text is never
/// expanded again. When the phrase is all that was said (nothing but
/// punctuation around it, as cleanup tends to add) the result is exactly the
/// snippet's text, without a stray capital or full stop.
pub fn apply_snippets(text: &str, dict: &[DictionaryEntry]) -> String {
    let mut snippets: Vec<(Vec<String>, &str)> = dict
        .iter()
        .filter(|e| e.snippet && !e.to.trim().is_empty())
        .map(|e| (phrase_words(&e.from), e.to.as_str()))
        .filter(|(words, _)| !words.is_empty())
        .collect();
    if snippets.is_empty() {
        return text.to_string();
    }
    // Most words first, then the longest; stable, so ties keep list order.
    snippets.sort_by_key(|(words, _)| {
        std::cmp::Reverse((words.len(), words.iter().map(|w| w.chars().count()).sum::<usize>()))
    });
    // Between the words: anything that isn't a letter or digit (spaces,
    // commas, hyphens…). At one position the regex takes the first
    // alternative that matches, i.e. the longest phrase.
    let alternatives: Vec<String> = snippets
        .iter()
        .map(|(words, _)| {
            let words: Vec<String> = words.iter().map(|w| regex::escape(w)).collect();
            format!("({})", words.join(r"[^\p{Alphabetic}\p{N}]+"))
        })
        .collect();
    let Ok(re) = regex::Regex::new(&format!(r"(?i)\b(?:{})\b", alternatives.join("|"))) else {
        return text.to_string();
    };
    let found: Vec<(std::ops::Range<usize>, &str)> = re
        .captures_iter(text)
        .filter_map(|caps| {
            let whole = caps.get(0)?;
            let which = (1..=snippets.len()).find(|&i| caps.get(i).is_some())?;
            Some((whole.range(), snippets[which - 1].1))
        })
        .collect();
    if let [(range, expansion)] = found.as_slice() {
        let mut around = text[..range.start].chars().chain(text[range.end..].chars());
        if !around.any(char::is_alphanumeric) {
            return expansion.to_string();
        }
    }
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (range, expansion) in &found {
        out.push_str(&text[last..range.start]);
        out.push_str(expansion);
        last = range.end + stray_full_stop(expansion, &text[range.end..]);
    }
    out.push_str(&text[last..]);
    out
}

/// How many bytes of `rest` (the text after an expanded snippet) to drop: a
/// full stop that cleanup put after the spoken phrase, when it would spoil
/// the snippet's own text — straight after text that ends in punctuation
/// ("Speak soon," + "." → ",."), or as the dictation's last character after a
/// snippet of several lines (a sign-off or an address ends where its text
/// ends, not on "Leeds."). A one-line snippet mid-sentence keeps the
/// sentence's punctuation ("Mail it to sam@example.com.").
fn stray_full_stop(expansion: &str, rest: &str) -> usize {
    let after = rest.trim_start();
    if !after.starts_with('.') || after.starts_with("..") {
        return 0;
    }
    let text = expansion.trim_end();
    let ends_in_punctuation = text.ends_with(['.', ',', ';', ':', '!', '?']);
    let last_thing = after[1..].trim().is_empty();
    if ends_in_punctuation || (last_thing && text.contains('\n')) {
        rest.len() - after.len() + 1
    } else {
        0
    }
}

/// Build the Whisper `initial_prompt` vocabulary from the dictionary: the
/// distinct canonical (`to`) spellings joined ", " (Handy's format — the
/// prompt biases whisper.cpp's decoder toward these spellings), in the order
/// given — callers pass `dict_usage::ranked(..)`, so the words you use most
/// lately come first and survive the cap. Capped at ~600 chars cut at a
/// comma boundary (Whisper only reads ~224 tokens of prompt; OpenWhispr
/// truncates the same way). Snippets are left out (their text isn't
/// vocabulary). `None` when there's nothing to bias towards.
pub fn dictionary_prompt(dict: &[DictionaryEntry]) -> Option<String> {
    const MAX_PROMPT_CHARS: usize = 600;
    let mut seen = std::collections::HashSet::new();
    let words: Vec<&str> = dict
        .iter()
        .filter(|e| !e.snippet)
        .map(|e| e.to.trim())
        .filter(|t| !t.is_empty() && seen.insert(t.to_lowercase()))
        .collect();
    if words.is_empty() {
        return None;
    }
    let mut prompt = words.join(", ");
    if prompt.len() > MAX_PROMPT_CHARS {
        let boundary = (0..=MAX_PROMPT_CHARS)
            .rev()
            .find(|i| prompt.is_char_boundary(*i))
            .unwrap_or(0);
        let cut = prompt[..boundary].rfind(',').unwrap_or(boundary);
        prompt.truncate(cut);
    }
    Some(prompt)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(from: &str, to: &str) -> DictionaryEntry {
        DictionaryEntry {
            from: from.to_string(),
            to: to.to_string(),
            fuzzy: true,
            snippet: false,
        }
    }

    fn snippet(from: &str, to: &str) -> DictionaryEntry {
        DictionaryEntry { snippet: true, ..entry(from, to) }
    }

    #[test]
    fn snippets_expand_a_whole_phrase_whatever_the_case_and_punctuation() {
        let dict = vec![snippet("my sign off", "Best wishes,\nSam")];
        // Cleanup capitalised it, hyphenated it and added commas around it.
        assert_eq!(
            apply_snippets("Thanks again, My sign-off, see you", &dict),
            "Thanks again, Best wishes,\nSam, see you"
        );
        assert_eq!(apply_snippets("ok MY SIGN OFF now", &dict), "ok Best wishes,\nSam now");
        // Only whole words: "mysign off" and "my sign offer" aren't it.
        assert_eq!(apply_snippets("mysign off", &dict), "mysign off");
        assert_eq!(apply_snippets("my sign offer stands", &dict), "my sign offer stands");
        // Every time it's said.
        assert_eq!(
            apply_snippets("my sign off and my sign off", &dict),
            "Best wishes,\nSam and Best wishes,\nSam"
        );
    }

    #[test]
    fn a_snippet_said_on_its_own_is_exactly_its_text() {
        let dict = vec![snippet("my address", "1 Long Lane\nLeeds")];
        // Cleanup made a sentence of it: no capital, no full stop survives.
        assert_eq!(apply_snippets("My address.", &dict), "1 Long Lane\nLeeds");
        assert_eq!(apply_snippets("  \"my address!\" ", &dict), "1 Long Lane\nLeeds");
        // Ending a dictation, a snippet of several lines ends where its text
        // does: no full stop on its last line (a sign-off, an address).
        assert_eq!(apply_snippets("Send it to my address.", &dict), "Send it to 1 Long Lane\nLeeds");
        let sign_off = vec![snippet("my sign-off", "Speak soon,\nNathan")];
        assert_eq!(
            apply_snippets("I'll see you soon. My sign off.", &sign_off),
            "I'll see you soon. Speak soon,\nNathan"
        );
        // Mid-dictation it stays: more text follows.
        assert_eq!(
            apply_snippets("It's my address. Thanks!", &dict),
            "It's 1 Long Lane\nLeeds. Thanks!"
        );
        // A one-line snippet keeps the sentence's punctuation…
        let email = vec![snippet("my email", "sam@example.com")];
        assert_eq!(apply_snippets("Mail it to my email.", &email), "Mail it to sam@example.com.");
        assert_eq!(apply_snippets("Is it my email?", &email), "Is it sam@example.com?");
        // …unless its own text ends in punctuation: no ",." anywhere.
        let thanks = vec![snippet("my thanks", "Many thanks,")];
        assert_eq!(apply_snippets("Great work. My thanks. Bye", &thanks), "Great work. Many thanks, Bye");
        // An ellipsis isn't a stray full stop.
        assert_eq!(apply_snippets("Send it to my address...", &dict), "Send it to 1 Long Lane\nLeeds...");
    }

    #[test]
    fn the_longest_snippet_wins_and_expansions_are_never_expanded_again() {
        let dict = vec![
            snippet("my email", "short"),
            snippet("my email address", "long, and my email too"),
        ];
        assert_eq!(apply_snippets("it's my email address ok", &dict), "it's long, and my email too ok");
        assert_eq!(apply_snippets("it's my email ok", &dict), "it's short ok");
    }

    #[test]
    fn snippets_insert_their_text_literally() {
        // `$1` is text, not a regex reference; regex characters in the
        // phrase are just punctuation.
        let dict = vec![snippet("price (list)", "$1 each")];
        assert_eq!(apply_snippets("the price list today", &dict), "the $1 each today");
    }

    #[test]
    fn snippets_need_a_phrase_and_a_text_and_leave_corrections_alone() {
        let dict = vec![
            snippet("", "orphan"),
            snippet("?!", "no words"),
            snippet("blank", "   "),
            entry("my sign off", "a correction"),
        ];
        assert_eq!(apply_snippets("blank my sign off", &dict), "blank my sign off");
        assert_eq!(apply_snippets("anything", &[]), "anything");
    }

    #[test]
    fn corrections_and_the_whisper_prompt_skip_snippets() {
        let dict = vec![snippet("brb", "Be right back"), entry("jaison", "JSON")];
        assert_eq!(apply_dictionary("brb, the jaison", &dict), "brb, the JSON");
        assert_eq!(dictionary_prompt(&dict).as_deref(), Some("JSON"));
        assert_eq!(dictionary_prompt(&[snippet("brb", "Be right back")]), None);
    }

    #[test]
    fn the_whisper_prompt_keeps_the_given_order_and_its_cap() {
        let dict = vec![entry("", "Zed"), entry("x", "alpha"), entry("y", "zed")];
        assert_eq!(dictionary_prompt(&dict).as_deref(), Some("Zed, alpha"));
        let many: Vec<DictionaryEntry> =
            (0..200).map(|i| entry("", &format!("Term{i:03}"))).collect();
        let p = dictionary_prompt(&many).unwrap();
        assert!(p.len() <= 600 && p.starts_with("Term000, Term001") && !p.ends_with(", "), "{p}");
    }

    #[test]
    fn old_dictionary_entries_load_as_corrections() {
        let e: DictionaryEntry = serde_json::from_str(r#"{"from":"a","to":"b"}"#).unwrap();
        assert!(e.fuzzy && !e.snippet);
        let cfg: YapConfig = serde_json::from_str("{}").unwrap();
        assert!(cfg.voice_enter, "the voice command is on unless switched off");
        assert!(cfg.learn_words, "word suggestions are on unless switched off");
        // So are terminal pasting and chat full stops, for configs saved before them.
        assert!(cfg.terminal_mode && cfg.chat_no_full_stop);
        let off: YapConfig = serde_json::from_str(r#"{"terminalMode":false,"chatNoFullStop":false}"#).unwrap();
        assert!(!off.terminal_mode && !off.chat_no_full_stop);
    }

    #[test]
    fn the_old_region_fields_still_load() {
        // A dev config.json from before the one Region picker (22 Oct 2026).
        let cfg: YapConfig = serde_json::from_str(
            r#"{"regionCountry":"US","regionLanguage":"en-US","regionCurrency":"EUR","regionUnits":"metric","regionTimeZone":"America/New_York","regionSpelling":false}"#,
        )
        .unwrap();
        assert_eq!(cfg.region_country, "US");
        assert!(!cfg.region_spelling);
        // …and leave on the next save.
        let saved = serde_json::to_value(&cfg).unwrap();
        assert!(saved.get("regionCurrency").is_none() && saved.get("regionTimeZone").is_none());
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
        let mut cfg = YapConfig {
            pp_provider: "groq".into(),
            pp_api_key: "gsk_active".into(),
            ..Default::default()
        };
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

    #[test]
    fn retired_groq_models_migrate_only_for_groq() {
        let scope = |provider: &str, model: &str| LlmScope {
            provider: provider.into(),
            model: model.into(),
            ..Default::default()
        };
        let mut cfg = YapConfig {
            pp_provider: "groq".into(),
            pp_model: "llama-3.1-8b-instant".into(),
            ..Default::default()
        };
        cfg.llm_scopes.insert("chat".into(), scope("groq", "qwen/qwen3-32b"));
        cfg.llm_scopes.insert("notes".into(), scope("ondevice", "llama-3.1-8b-instant"));
        cfg.llm_scopes.insert("agent".into(), scope("groq", "openai/gpt-oss-120b"));

        migrate_retired_groq_models(&mut cfg);

        assert_eq!(cfg.pp_model, "openai/gpt-oss-20b");
        assert_eq!(cfg.llm_scopes["chat"].model, "openai/gpt-oss-120b");
        // not Groq → untouched, even with a retired-looking id
        assert_eq!(cfg.llm_scopes["notes"].model, "llama-3.1-8b-instant");
        // already current → untouched
        assert_eq!(cfg.llm_scopes["agent"].model, "openai/gpt-oss-120b");
    }

    #[test]
    fn fresh_install_defaults_match_serde_defaults() {
        // A first run (no config.json) gets `YapConfig::default()`; an older
        // config.json that lacks a field gets that field's `#[serde(default)]`.
        // The two must agree, or new users silently start with different
        // settings — streaming_partials was on via serde but off in `Default`.
        // The one deliberate difference is `configVersion`: a missing field
        // marks a pre-versioning file, so its one-time migrations still run.
        let mut fresh = serde_json::to_value(YapConfig::default()).unwrap();
        let mut from_empty =
            serde_json::to_value(serde_json::from_str::<YapConfig>("{}").unwrap()).unwrap();
        assert_eq!(fresh["configVersion"], CONFIG_VERSION);
        assert_eq!(from_empty["configVersion"], 0);
        fresh.as_object_mut().unwrap().remove("configVersion");
        from_empty.as_object_mut().unwrap().remove("configVersion");
        assert_eq!(fresh, from_empty);
    }

    #[test]
    fn mute_while_recording_carries_over_to_silencing_other_apps() {
        // Saved before 2026-10-17: the old switch, on or off.
        let on: YapConfig = serde_json::from_str(r#"{"muteWhileRecording":true}"#).unwrap();
        assert!(on.quiet_other_apps && !on.pause_media);
        let off: YapConfig = serde_json::from_str(r#"{"muteWhileRecording":false}"#).unwrap();
        assert!(!off.quiet_other_apps);
        // Saved since: the new names, and the old one is gone.
        let saved = serde_json::to_value(&on).unwrap();
        assert_eq!(saved["quietOtherApps"], true);
        assert_eq!(saved["pauseMedia"], false);
        assert!(saved.get("muteWhileRecording").is_none());
        let fresh = YapConfig::default();
        assert!(!fresh.quiet_other_apps && !fresh.pause_media, "both off unless chosen");
    }

    #[test]
    fn one_time_migration_turns_partials_back_on_once() {
        // Saved by a pre-versioning build, whose `Default` had partials off.
        let mut cfg: YapConfig = serde_json::from_str(r#"{"streamingPartials":false}"#).unwrap();
        assert!(migrate_once(&mut cfg));
        assert!(cfg.streaming_partials);
        assert_eq!(cfg.config_version, CONFIG_VERSION);

        // Switched off again afterwards (and saved): the choice sticks.
        cfg.streaming_partials = false;
        let saved = serde_json::to_string(&cfg).unwrap();
        let mut reloaded: YapConfig = serde_json::from_str(&saved).unwrap();
        assert!(!migrate_once(&mut reloaded));
        assert!(!reloaded.streaming_partials);

        // Fresh installs start current: nothing to migrate.
        assert!(!migrate_once(&mut YapConfig::default()));
    }

    #[test]
    fn kokoro_becomes_the_voice_and_a_windows_voice_is_kept() {
        // Saved by the first Voice Agent build: Windows voices, one picked.
        let mut cfg: YapConfig =
            serde_json::from_str(r#"{"configVersion":1,"ttsEngine":"windows","ttsVoice":"HKEY\\Voices\\MSTTS_V110_enGB_GeorgeM"}"#)
                .unwrap();
        assert!(migrate_once(&mut cfg));
        assert_eq!(cfg.tts_engine, "kokoro");
        assert_eq!(cfg.tts_voice, "af_bella");
        assert_eq!(cfg.tts_windows_voice, "HKEY\\Voices\\MSTTS_V110_enGB_GeorgeM");
        // Windows' default ("") stays the default.
        let mut cfg: YapConfig = serde_json::from_str(r#"{"configVersion":1,"ttsEngine":"windows","ttsVoice":""}"#).unwrap();
        assert!(migrate_once(&mut cfg));
        assert_eq!((cfg.tts_engine.as_str(), cfg.tts_voice.as_str(), cfg.tts_windows_voice.as_str()), ("kokoro", "af_bella", ""));
        // Windows chosen after that sticks.
        cfg.tts_engine = "windows".into();
        let mut reloaded: YapConfig = serde_json::from_str(&serde_json::to_string(&cfg).unwrap()).unwrap();
        assert!(!migrate_once(&mut reloaded));
        assert_eq!(reloaded.tts_engine, "windows");
        // A fresh install: Kokoro, af_bella.
        let fresh = YapConfig::default();
        assert_eq!((fresh.tts_engine.as_str(), fresh.tts_voice.as_str()), ("kokoro", "af_bella"));
    }
}
