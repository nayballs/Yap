//! Yap configuration: stored as JSON in the app data dir.
//!
//! Deliberately tiny — a dictation tool only needs a hotkey, a model,
//! GPU toggle, the sound cue, and the correction dictionary.

use serde::{Deserialize, Serialize};
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

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
    /// Starred in the Dictionary view: listed first there (its "Starred
    /// first" sort) and given to the speech model first (`dict_usage::rank`,
    /// as Wispr Flow ranks starred words). Written only when set, so a
    /// dictionary with no stars saves as it always did.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub starred: bool,
    /// More ways an entry **in memory** is heard, beside `from` — never on
    /// disk: config.json keeps one row per way a word is heard (the shape
    /// every Yap reads; `dictionary/variants.rs`), and the Dictionary view
    /// groups the rows that write the same word. Memory's names and yours
    /// carry how they're said here (`spoken_names.rs`). Read if a page ever
    /// sends it, and [`save`] writes it out as rows of their own, so nothing
    /// sent this way is lost.
    #[serde(default, skip_serializing)]
    pub also_heard: Vec<String>,
}

impl DictionaryEntry {
    /// Every way it's heard: `from`, then [`Self::also_heard`] — trimmed,
    /// none empty, none twice (any case). A snippet's phrase only.
    pub fn heard(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        let more = if self.snippet { &[][..] } else { &self.also_heard[..] };
        for h in std::iter::once(self.from.as_str()).chain(more.iter().map(String::as_str)) {
            let h = h.trim();
            if !h.is_empty() && !out.iter().any(|o| o.to_lowercase() == h.to_lowercase()) {
                out.push(h);
            }
        }
        out
    }
}

/// A reusable, named cleanup profile (FluidVoice's `DictationPromptProfile`): a
/// library entry that per-app routing rules bind to, so one body can serve many
/// apps and is edited in one place. The immutable `llm::BASE_PROMPT` guardrails
/// are always prepended, exactly as for the global body.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
    /// **Styles by app** (`cleanup_styles.rs`): the built-in kind of app
    /// this profile is the style for ("email", "workChat", "personalChat",
    /// "documents", "aiPrompts", "terminal", "other"); empty for a profile
    /// of the person's own. A style's `prompt` is *added* to the main
    /// cleanup body (empty: the main body alone), where a person's own
    /// profile's replaces it.
    #[serde(default)]
    pub category: String,
    /// The tone ("natural", "formal", "casual", "veryCasual", "literal",
    /// "command", "off"): Yap's own line just before the transcript
    /// (`cleanup_styles::line`). Empty: the body alone decides.
    #[serde(default)]
    pub tone: String,
    /// A style's apps and sites the person added ("Basecamp", "myapp.exe").
    #[serde(default)]
    pub apps: Vec<String>,
    /// A style's built-in apps the person took out (their names: "Discord").
    #[serde(default)]
    pub removed_apps: Vec<String>,
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
/// Formatting in the notes' Actions engine, Chat in `turn::chat_turn`
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
    /// The rewrite shortcut with nothing selected rewrites the **last
    /// dictation** — within a minute, in the same field, while it still
    /// ends at the cursor (`as_said.rs`) — instead of writing new text
    /// (Settings → Commands). On by default; off, nothing selected always
    /// means write mode, as before 2026-10-23.
    #[serde(default = "default_true")]
    pub rewrite_last_dictation: bool,
    /// **Saved transforms** (Settings → Commands → Transforms,
    /// `transforms.rs`): one-key rewrites of the selection or the last
    /// dictation, each with an optional shortcut, in the person's order. A
    /// config without the key starts with Yap's five built-ins.
    #[serde(default = "crate::dictation::transforms::defaults")]
    pub transforms: Vec<crate::dictation::transforms::Transform>,
    /// **A transform on every dictation** (Settings → Commands →
    /// Transforms, `transforms::for_dictation`): a saved transform's id run
    /// after cleanup on every dictation. "" (the default): none.
    #[serde(default)]
    pub auto_transform: String,
    /// …only for dictations into this kind of app (a
    /// `cleanup_styles::Category` id: "email"); "" = every app.
    #[serde(default)]
    pub auto_transform_kind: String,
    /// **My languages** (Settings → Dictation → My languages,
    /// `dictation/languages.rs`): the languages you speak, as codes ("en",
    /// "fr"), in your order. Empty: none picked (a config from before keeps
    /// the one `selected_language` names). The main shortcut's language is
    /// still `selected_language` ("auto": the likeliest of these).
    #[serde(default)]
    pub my_languages: Vec<String>,
    /// **Language shortcuts**: a key of its own that dictates in a fixed
    /// language (or any of yours), optionally pasted translated into
    /// another (at most `languages::MAX_SHORTCUTS`).
    #[serde(default)]
    pub language_shortcuts: Vec<crate::dictation::languages::LanguageShortcut>,
    /// "Tell me when I speak a language I didn't pick" (`languages/nudge.rs`;
    /// only Whisper models say which language they heard). On by default.
    #[serde(default = "default_true")]
    pub language_nudge: bool,
    /// "Write in Latin letters (Hinglish)" (My languages, with Hindi among
    /// them; `languages/hinglish.rs`): Hindi dictations come out in Latin
    /// letters. Off by default; an older Yap drops it on a save, and Hindi
    /// is then written in Devanagari again.
    #[serde(default)]
    pub hinglish: bool,
    /// The **Voice Assistant** shortcut (Settings → Voice Assistant,
    /// `assistant.rs`): from any app, ask Yap's Chat something out loud and
    /// hear the answer; it never types into the app you're in. Empty =
    /// unbound (the default). Same spec format as `hotkey`; it never shares
    /// a key with the other three (`input_hook::clashes`).
    #[serde(default)]
    pub assistant_hotkey: String,
    /// The **Read aloud** shortcut (Settings → Shortcuts / Voice Assistant,
    /// `read_aloud.rs`): select text in any app, press it, and Yap reads the
    /// selection aloud in the chosen voice, with a reading card on the Yap
    /// bar. Empty = unbound (the default: the bar's 🔊 and the tray work
    /// without it). Never shares a key with the others.
    #[serde(default)]
    pub read_hotkey: String,
    /// "Use what I said" (Settings → Shortcuts, `as_said.rs`): right after a
    /// dictation AI cleanup changed, swap what Yap pasted for the words as
    /// said (corrections and snippets still applied). Empty = unbound (the
    /// default). Never shares a key with the other shortcuts.
    #[serde(default)]
    pub as_said_hotkey: String,
    /// **Paste last transcript** (Settings → Shortcuts, `pasting/last.rs`):
    /// the latest dictation pasted again where you're typing, as the tray's
    /// item does. Empty = unbound (the default). Never shares a key with the
    /// other shortcuts (`input_hook::clashes`).
    #[serde(default)]
    pub paste_last_hotkey: String,
    /// **Copy last transcript** (Settings → Shortcuts, `pasting/last.rs`): the
    /// latest dictation onto the clipboard, kept out of Win+V history as a
    /// paste is; nothing pasted. Empty = unbound (the default).
    #[serde(default)]
    pub copy_last_hotkey: String,
    /// **Quick note** (Settings → Shortcuts, `notes/quick_note.rs`): from any
    /// app, a small window to jot or dictate a note, saved in Notes; again
    /// while it's in front puts it away. Empty = unbound (the default).
    /// Never shares a key with the other shortcuts. An older Yap sharing
    /// the folder drops it on its next save: unbound again, nothing else.
    #[serde(default)]
    pub quick_note_hotkey: String,
    /// **Send** (Settings → Shortcuts, `pasting/send_key.rs`, 2026-10-29): an
    /// extra key or mouse button that presses the send key in the window in
    /// front — Enter, or `auto_submit_key` as auto-submit would (Enter in a
    /// terminal). Empty = unbound (the default); never Enter itself, never a
    /// key another shortcut has. An older Yap sharing the folder drops it on
    /// its next save: unbound again, nothing else.
    #[serde(default)]
    pub send_hotkey: String,
    /// **More shortcuts for one action** (Settings → Shortcuts, "+ Add
    /// another"; `input_hook.rs`, 2026-10-25): up to `input_hook::MORE` more
    /// keys or mouse buttons per action, keyed by its first shortcut's
    /// field ("hotkey", "editHotkey", "meetingHotkey"…). The first stays in
    /// its own field, so a config from before (or an older Yap reading this
    /// one) is read as ever; [`load`] tidies the lists.
    #[serde(default)]
    pub more_hotkeys: std::collections::BTreeMap<String, Vec<String>>,
    /// "Triple-tap for meeting notes" (Settings → Dictation, push-to-talk;
    /// `gesture.rs`): three quick taps of the dictation key throw the
    /// dictation away and start meeting notes. Off by default.
    #[serde(default)]
    pub triple_tap_meeting: bool,
    /// The Yap bar offers "Use what I said" for a few seconds after a
    /// dictation whose words AI cleanup changed (`as_said.rs`). Off by
    /// default since 2026-10-25 (`migrate_once` v5): an offer to undo the
    /// cleanup after every dictation made it look unsure of itself. Its
    /// shortcut works either way.
    #[serde(default)]
    pub as_said_offer: bool,
    /// **Snip to Chat** (Settings → Shortcuts, `snip.rs`): drag a box over
    /// any screen and the image goes into Chat. Default Win+Alt+S (free in
    /// Windows' and Game Bar's own Win+Alt set); empty = unbound. Never
    /// shares a key with the other shortcuts (`input_hook::clashes`).
    #[serde(default = "default_snip_hotkey")]
    pub snip_hotkey: String,
    /// "Say 'snip' to take a screenshot": a dictation or Voice Assistant
    /// question that's only "snip" ("snip it", "take a screenshot"…) takes
    /// one instead of being pasted or asked (`snip::is_snip_phrase`). On by
    /// default.
    #[serde(default = "default_true")]
    pub snip_voice: bool,
    /// "Then listen for my question" (under "Say 'snip'…", 2026-10-25):
    /// after a spoken "snip" lands in Chat, a Voice Assistant question
    /// starts by itself and takes the screenshot (`snip::listens_after`).
    /// On by default; never during a meeting or a call.
    #[serde(default = "default_true")]
    pub snip_listen: bool,
    /// "Copy every snip to the clipboard" (Settings → Shortcuts, 2026-10-25):
    /// a snip that goes to Chat is on the clipboard too (PNG + DIB, out of
    /// Win+V history), as Windows' Snipping Tool does. On by default. A
    /// picture pasted or dropped into Chat isn't copied again.
    #[serde(default = "default_true")]
    pub snip_copy: bool,
    /// "Keep chat screenshots" (Settings → Privacy & data): days a chat's
    /// screenshots are kept (7, 30, 90; 0 = forever). Deleting a
    /// conversation deletes its screenshots whatever this says. Default 30.
    #[serde(default = "default_chat_image_days")]
    pub chat_image_days: u32,
    /// Snip to Chat's **Save** (Shift as you let go, Ctrl+S, a chat
    /// screenshot's "Save image as…"): the folder the Save dialog opened in
    /// last (`snip::copy_save`). Empty = Pictures.
    #[serde(default)]
    pub snip_save_dir: String,
    /// Active model id from the STT registry (e.g. "parakeet-tdt-0.6b-v3",
    /// "small"). Field name kept as `model_size` for config back-compat.
    #[serde(default = "default_model_size")]
    pub model_size: String,
    /// Use GPU acceleration (whisper → Vulkan, ONNX → DirectML; any GPU).
    /// Since 2026-10-22 it mirrors `stt_device`: false ⇔ "cpu" (an old config
    /// with it off keeps the processor).
    #[serde(default = "default_true")]
    pub use_gpu: bool,
    /// Settings → Speech-to-Text → Runs on: "auto" (the default — the
    /// graphics card unless the processor measured clearly faster on this
    /// PC, per model, by `stt_speed.rs`), "gpu" (the graphics card) or
    /// "cpu" (the processor).
    #[serde(default = "default_stt_device")]
    pub stt_device: String,
    /// Keep the audio of recent dictations on this PC for 24 hours, so one
    /// that failed, or came out wrong, can be run again (`recent_audio.rs`;
    /// Settings → Privacy & data). On by default; only with history on.
    #[serde(default = "default_true")]
    pub keep_recent_audio: bool,
    /// The one microphone picked before 2026-10-25 (its name; None =
    /// Windows' default), and what an older Yap still reads: [`save`] writes
    /// the first of [`YapConfig::mic_order`] here, and a different name on
    /// load is a pick made in an older build, which goes first
    /// ([`follow_an_older_builds_mic`]).
    #[serde(default)]
    pub input_device: Option<String>,
    /// Settings → Dictation → Microphone (`dictation/mics.rs`, 2026-10-25):
    /// the microphones in the person's order — dictation and meetings use
    /// the first one that's connected, then Windows' default. Empty (the
    /// default): Windows' default only.
    #[serde(default)]
    pub mic_order: Vec<crate::dictation::mics::MicChoice>,
    /// Microphones the person answered "Not now" to ("… connected · Use it
    /// for dictation?", `mics/offer.rs`): never asked about again.
    #[serde(default)]
    pub mic_not_now: Vec<crate::dictation::mics::MicChoice>,
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
    /// Settings → General → Notifications, each kind of nice-to-know notice
    /// on its own (`notices/kinds.rs`), on by default: suggestions (words,
    /// snippets, languages), milestones, feature tips and news (what's new
    /// after an update). An older Yap drops them when it saves config.json:
    /// one switched off comes back on.
    #[serde(default = "default_true")]
    pub notify_suggestions: bool,
    #[serde(default = "default_true")]
    pub notify_milestones: bool,
    #[serde(default = "default_true")]
    pub notify_feature_tips: bool,
    #[serde(default = "default_true")]
    pub notify_news: bool,

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
    /// "Keep the microphone ready" (Settings → Dictation, `mic.rs`): the
    /// microphone stays open the whole time Yap runs, as it always did until
    /// 2026-10-22 — a dictation starts instantly and catches a first word
    /// said with the key, but Windows shows the mic in use and a Bluetooth
    /// headset plays everything in call quality. Off (the default), the mic
    /// opens when a dictation starts, stays warm a minute after (never on
    /// Bluetooth) and closes.
    #[serde(default)]
    pub keep_mic_ready: bool,
    /// Append a single space after each injected transcription. With
    /// `smart_spacing` on it only applies where Yap couldn't read the field.
    #[serde(default)]
    pub append_trailing_space: bool,
    /// "Match the text around the cursor" (Settings → Dictation → Typing into apps): at
    /// record start Yap reads a little text either side of the caret over UI
    /// Automation (`selection::begin_dictation`, in memory only) and fits the
    /// dictation to it — spaces, a capital mid-sentence, a full stop before
    /// more of the sentence (`smart_insert.rs`). Where the field can't be
    /// read, a dictation pastes exactly as without it.
    #[serde(default = "default_true")]
    pub smart_spacing: bool,
    /// "Spell names the way they're written on screen" (Settings →
    /// Dictation → Typing into apps): at record start Yap reads the visible
    /// text of the window being dictated into over UI Automation, in the
    /// background (`screen_terms.rs`), and keeps the names and terms in it
    /// (capitalised names, CamelCase, acronyms, words with digits, handles),
    /// in memory, for that dictation only: the Whisper prompt gets them, the
    /// near-miss pass fixes a name heard a little wrong (ONNX models), and AI
    /// cleanup is told the ones the transcript seems to contain.
    #[serde(default = "default_true")]
    pub screen_names: bool,
    /// "Leave the full stop off chat messages" (Settings → Dictation →
    /// Typing into apps): in a chat app (Slack, Discord, WhatsApp, Teams…, desktop or
    /// in a browser; `app_kind.rs`), a message of one or two sentences in one
    /// paragraph pastes without its final full stop.
    #[serde(default = "default_true")]
    pub chat_no_full_stop: bool,
    /// "Send several messages: say “new message”" (Settings → Dictation →
    /// Typing into apps, `pasting/stacked.rs`, 2026-10-29): in a chat app, a
    /// dictation that's sent anyway (auto-submit, or "press enter" said) and
    /// has "new message" said as a sentence of its own goes as several
    /// messages, each pasted and sent in turn; one that isn't sent gets its
    /// parts on lines of their own. Off by default: a message can't be taken
    /// back. An older Yap drops it on its next save: off again.
    #[serde(default)]
    pub stacked_messages: bool,
    /// "Turn “at Sam” into a mention" (`pasting/mentions.rs`, 2026-10-29): in
    /// Slack and Discord "at <a name on screen>" is typed as "@Sam" and
    /// picked from the app's list with Tab (only when the list shows that
    /// person); in Teams, Google Chat and other team chats it's written
    /// "@Sam". Needs `screen_names`. On by default; an older Yap drops it:
    /// on again.
    #[serde(default = "default_true")]
    pub chat_mentions: bool,
    /// "Tag files in AI chat boxes" (`pasting/file_tags.rs`, 2026-10-29): in
    /// Cursor's, Windsurf's and VS Code's AI chat box a file's name said
    /// aloud is typed "@file" ("#file" in VS Code) and picked from the
    /// list; in Claude Code and Codex it's written "@path". Needs
    /// `screen_names`. On by default; an older Yap drops it: on again.
    #[serde(default = "default_true")]
    pub file_tags: bool,
    /// After injecting text, simulate pressing Enter (auto-submit).
    #[serde(default)]
    pub auto_submit: bool,
    /// The "press enter" voice command: a dictation that ENDS with "press
    /// enter" (or "press return") has the phrase taken off and the submit key
    /// (`auto_submit_key`) pressed after the paste — once, even with
    /// `auto_submit` on. See `pipeline::take_voice_enter`.
    #[serde(default = "default_true")]
    pub voice_enter: bool,
    /// "Safer pasting into terminals" (Settings → Dictation → Typing into apps): in a
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
    // (`showTrayIcon`, retired with the pill on 2026-07-09: the tray is
    // always built. An old config.json's value is ignored on load.)
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
    /// The bar's colours: "app" (the default: the same as Yap's windows,
    /// `theme`), "light" (Yap's white cards and ink buttons) or "dark" (the
    /// in-app toasts' warm near-black). The `--bar-*` tokens in app.css; the
    /// page sets `data-bar-theme` (src/lib/core/appearance.js resolves "app").
    #[serde(default = "default_bar_theme")]
    pub bar_theme: String,
    /// The dictation capsule's ✕ (cancel) and ✓ (stop and paste) while it
    /// records: "off" (the default: Cancel stays in the tray), "hover" (only
    /// while the pointer is on the capsule) or "always". Never while the
    /// push-to-talk key is held. `bar.rs` carries it to the bar page.
    #[serde(default = "default_bar_dictation_buttons")]
    pub bar_dictation_buttons: String,
    /// A quick-note button on the open bar (Wispr Flow's "Add to Flow Bar";
    /// off by default): switched on from the quick note's empty page or
    /// Settings → Yap bar. An older Yap saving config.json drops it: the
    /// button goes until it's switched on again, nothing else.
    #[serde(default)]
    pub bar_quick_note: bool,
    /// Settings → General → Appearance → Theme, for every Yap window:
    /// "system" (the default: Windows' app mode, Settings → Personalisation →
    /// Colours, followed live), "light" or "dark". Every page sets
    /// `data-yap-theme` from it (`src/lib/core/appearance.js`, the `--yap-*`
    /// tokens in app.css), and `theme.rs` gives the opaque windows' webviews
    /// the same background before their page paints.
    #[serde(default = "default_theme")]
    pub theme: String,
    /// Settings → General → Appearance: the font of the reading text —
    /// Chat's messages, notes and their Enhanced view, the meeting notepad's
    /// My thoughts / Transcript / Summary and its Ask answers; never the
    /// app's buttons and menus. "default" (Figtree, the UI's own), "sans"
    /// (Geist), "serif" (Literata), "system" (Segoe UI) or "dyslexic"
    /// (OpenDyslexic). Every window sets `data-reading-font` from it
    /// (`src/lib/core/appearance.js`, the `--yap-font-reading` token in app.css).
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
    /// Debug mode (Settings → Privacy & data → Debug logging, OpenWhispr-style):
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
    /// PC whose graphics card can run it, `local_llm::models::upgrade_offer`) was
    /// turned down ("No thanks"), or taken: it isn't offered again.
    #[serde(default)]
    pub local_llm_upgrade_dismissed: bool,
    /// The on-device model's **context**, in tokens: how much text the
    /// sidecar reads at once (`-c`), shared by Chat, the cleanup and the
    /// meeting notes. 0 (or missing) = the default, 8,192
    /// (`local_llm::context::DEFAULT_CONTEXT`); clamped to what the model was trained
    /// for and a floor (`local_llm::context::effective_context`). Changing it restarts
    /// the sidecar, never mid-dictation (`local_llm::context::restart_when_idle`).
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
    /// "Load the on-device model when Yap starts" (Settings → Language
    /// Models → the on-device models): the sidecar starts at launch and
    /// after a Settings save, as it always did. Off (the default): it loads
    /// the first time something uses it (`local_llm::on_demand`). New in
    /// 2026-10-25; an older Yap drops it on a save and it's off again.
    #[serde(default)]
    pub local_model_at_launch: bool,
    /// "Free its memory after it's unused for": minutes with no request
    /// before the sidecar is stopped (5 / 15 / 60; 0 = never). Missing =
    /// 15 (`local_llm::on_demand::DEFAULT_IDLE_MINS`).
    #[serde(default = "default_local_model_idle_mins")]
    pub local_model_idle_mins: u32,
    /// "Read screenshots on this PC" (Settings → Language Models, the
    /// on-device models; Chat's "can't read images" line): the on-device
    /// model loads its projector, once downloaded, when a screenshot goes to
    /// it (`local_llm::vision`). On by default — downloading it is the
    /// choice; off keeps the file and sends screenshots as their text. New
    /// in 2026-10-28; an older Yap drops it on a save and it's on again,
    /// which only matters once the projector is downloaded.
    #[serde(default = "default_true")]
    pub local_vision: bool,
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
    /// Reusable named cleanup profiles that `app_routes` bind to — and the
    /// built-in **styles by app** (a profile with a `category`,
    /// `cleanup_styles.rs`).
    #[serde(default)]
    pub cleanup_profiles: Vec<CleanupProfile>,
    /// "Match the style to the app" (Language Models → Dictation cleanup →
    /// Styles by app, `cleanup_styles.rs`): emails formal, chats casual, AI
    /// prompts and terminal commands as said. Missing (`None`) = on unless
    /// the person had their own cleanup body (`pp_preset` not "default"),
    /// decided once at load (`cleanup_styles::on_load`): a preset or body
    /// they chose for everywhere stays exactly as it was.
    #[serde(default)]
    pub cleanup_styles: Option<bool>,
    /// The one offer of AI cleanup to someone who has it off on a PC whose
    /// graphics card runs the on-device model (`cleanup_styles::offer`) was
    /// shown: it never comes back.
    #[serde(default)]
    pub cleanup_offer_seen: bool,
    /// **Cleanup strength** (Language Models → Dictation cleanup,
    /// `ai/cleanup_strength.rs`): "light" | "medium" | "high", how much AI
    /// cleanup changes a dictation. None is cleanup off
    /// (`post_process_enabled`), so it isn't stored here. Missing =
    /// "medium", what cleanup always did: a config from before keeps it.
    #[serde(default = "default_cleanup_strength")]
    pub cleanup_strength: String,
    /// "Send the text before the cursor to cleanup" (Privacy & data,
    /// `ai/cleanup_context.rs`): a sentence or two before the cursor goes
    /// with a dictation to a cleanup model that isn't on this PC (a cloud
    /// provider, a server on the network). Off by default; a model on this
    /// PC gets it whenever it's read (`smart_spacing`).
    #[serde(default)]
    pub cursor_context_cloud: bool,

    /// Per-mode LLM config for the AI **scopes** beyond cleanup (OpenWhispr's
    /// Language-Model bubbles), keyed `"voiceAgent"|"noteFormatting"|"chat"`.
    /// (Step-1 storage for the multi-mode Language Models feature.) The
    /// Dictation Cleanup scope stays in the `pp_*` fields above for back-compat.
    /// See [`LlmScope`] and `ROADMAP.md` Phase 4 "Multi-mode Language Models".
    #[serde(default)]
    pub llm_scopes: std::collections::HashMap<String, LlmScope>,
    /// **Effort**, per model (2026-10-31, `ai/effort.rs`): how hard each
    /// cloud model reasons when it answers in Chat, the Voice Assistant and
    /// a meeting's Ask, chosen in Chat's model picker (or Settings → Language
    /// Models → Chat), keyed `"<provider>:<model>"` → "low" | "medium" |
    /// "high" | "xhigh" | "max". A model missing here runs at its provider's
    /// default. An older Yap's save drops it (that model's default again).
    #[serde(default)]
    pub chat_effort: std::collections::BTreeMap<String, String>,
    /// **Fast mode** (2026-10-31, `ai/effort.rs`): Claude Opus 5.5 answers
    /// faster at twice the price (Anthropic's `speed: "fast"`). Off unless
    /// switched on in Chat's model picker.
    #[serde(default)]
    pub fast_mode: bool,
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
    /// "Let the Voice Assistant see the window in front when you ask about
    /// it" (Settings → Voice Assistant; `chat/assistant/screen.rs`): a
    /// spoken question about the screen ("what's this error?") takes a
    /// screenshot of the window that was in front when the shortcut was
    /// pressed, every Yap window left out. On by default, for a model on
    /// this PC: the picture never leaves it. An older Yap drops the field on
    /// its save and it's on again (an older Yap never takes the window).
    #[serde(default = "default_true")]
    pub assistant_screen: bool,
    /// "…with cloud models too": the screenshot (or its text) goes to the
    /// cloud provider Chat's model is on. Off by default; asked once, the
    /// first time a question about the screen meets a cloud model
    /// (`assistant_screen_asked`). An older Yap drops it: off again.
    #[serde(default)]
    pub assistant_screen_cloud: bool,
    /// The one ask about the cloud has been shown.
    #[serde(default)]
    pub assistant_screen_asked: bool,
    /// "Read aloud" (Settings → Voice Assistant → Spoken replies): how much
    /// of an answer Yap reads out by itself — the assistant shortcut's and
    /// Chat's voice replies — "short" (a sentence or two), "30s", "1m" (the
    /// default), "2m" or "whole" (`assistant::SpokenLength`). The rest is in
    /// Chat. ▶ Read aloud on a message always reads all of it.
    #[serde(default = "default_spoken_length")]
    pub spoken_length: String,
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
    /// "Keep history" (Settings → Privacy & data): "always", "day" (24
    /// hours) or "never" (which also turns `history_enabled` off: what an
    /// older Yap reads). "" = never chosen: always. An older Yap's save
    /// drops it; `dictation/history/keep.rs` brings "day" back.
    #[serde(default)]
    pub history_keep: String,

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
    /// Meetings → Notepad; `notepad.rs`): docked to the right edge of the
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
    /// "Keep meeting audio" (Settings → Meetings): each recording's audio
    /// stays on this PC for `meeting_audio_days`, to transcribe a meeting
    /// again and play its lines (`meeting_audio.rs`). Off, it's still
    /// written while a meeting records (a crash loses nothing) and deleted
    /// once the meeting is transcribed. On by default.
    #[serde(default = "default_true")]
    pub meeting_keep_audio: bool,
    /// For how many days (14, like Wispr Flow's playback).
    #[serde(default = "default_meeting_audio_days")]
    pub meeting_audio_days: u32,
    /// "Delete meeting transcripts after" (Settings → Privacy & data): a
    /// meeting's transcript (and its audio) goes this many days after the
    /// meeting, its notes, summary and tasks kept (`notes/retention.rs`; 30
    /// / 90 / 365). 0, the default: kept.
    #[serde(default)]
    pub meeting_transcript_days: u32,
    /// "Improve the transcript after the meeting" (Settings → Meetings):
    /// "auto" (when it helps: chunks failed, or a more accurate speech
    /// model is installed), "always" or "never" (`meeting_refine.rs`).
    #[serde(default = "default_meeting_refine")]
    pub meeting_refine: String,
    /// "Tell speakers apart in meetings" (beta; Settings → Speech-to-Text,
    /// and Meetings): label the voices on the call side while a
    /// meeting records, on this PC (`diarize.rs`). Off by default, and it
    /// only runs once its model is installed.
    #[serde(default)]
    pub meeting_diarization: bool,
    /// Which diarization model (`diarize/registry.rs` id) it uses.
    #[serde(default = "default_diarization_model")]
    pub diarization_model: String,
    /// "Suggest who's speaking" (Settings → Meetings): names for a
    /// meeting's voices from what's said and who was invited, on cards to
    /// confirm, a strong one put on as Yap's guess; and, when a meeting
    /// ends, one short question per voice still unnamed to the meeting's
    /// model (one that can call tools), each answer checked against the
    /// transcript (`speakers/naming.rs`). On.
    #[serde(default = "default_true")]
    pub meeting_speaker_names: bool,
    /// Read who's talking from the call app's window (Teams, Zoom, Meet in
    /// a browser) while a meeting records, through UI Automation, read
    /// only (`speakers/call_ui.rs`). Experimental, off: no setting shows it.
    #[serde(default)]
    pub meeting_call_names: bool,

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
    /// Call apps whose calls start notes by themselves after the countdown,
    /// whatever `meeting_auto_start` says (app ids): the person said yes to
    /// "Start notes automatically for Teams calls?", asked once after a few
    /// meetings recorded from it (`meeting_detect.rs`). New in 2026-10-29:
    /// an older Yap drops it on its next save, and those calls ask again.
    #[serde(default)]
    pub meeting_auto_start_apps: Vec<String>,
    /// Which call apps it asks about (Settings → Meetings, or "Don't
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

    // ---- Meeting guard rails (Settings → Meetings) ----
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
    /// "Prepare a brief before meetings" (`calendar/brief.rs`): before a
    /// calendar meeting, last time with these people, their open items, what
    /// memory knows about them and the invite's agenda, every line citing its
    /// note — on the reminder card, the Meetings view and the notepad. Its
    /// "In short" comes from the meeting's AI model when there is one. On by
    /// default.
    #[serde(default = "default_true")]
    pub meeting_brief: bool,

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

    // ---- Meeting search (Settings → Privacy & data; `search.rs`) ----
    /// "Search my meetings by meaning": the embedding model reads every
    /// meeting and note on this PC, and Ask across meetings and the
    /// Meetings view's search use it besides keywords. Off until its model
    /// is downloaded (the download turns it on).
    #[serde(default)]
    pub search_by_meaning: bool,

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
fn default_stt_device() -> String {
    "auto".into()
}
fn default_true() -> bool {
    true
}
fn default_local_model_idle_mins() -> u32 {
    crate::ai::local_llm::on_demand::DEFAULT_IDLE_MINS
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
fn default_cleanup_strength() -> String {
    "medium".into()
}
fn default_routing_scope() -> String {
    "all_apps".into()
}
fn default_meeting_detect_style() -> String {
    "popup".into()
}
fn default_bar_theme() -> String {
    "app".into()
}
fn default_theme() -> String {
    "system".into()
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
fn default_meeting_audio_days() -> u32 {
    crate::meetings::meeting_audio::DEFAULT_DAYS
}
fn default_meeting_refine() -> String {
    crate::meetings::meeting_refine::AUTO.into()
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
fn default_snip_hotkey() -> String {
    // Win+Alt+S (VK 'S' = 83): "S for snip", beside Win+Alt+M.
    "kb:alt+win+83".into()
}
fn default_chat_image_days() -> u32 {
    crate::snip::images::KEEP_DEFAULT
}
fn default_web_search_service() -> String {
    "exa".into()
}
fn default_speak_when() -> String {
    "voice".into()
}
fn default_spoken_length() -> String {
    "1m".into()
}
fn default_tts_engine() -> String {
    crate::voice::tts::DEFAULT_ENGINE.into()
}
fn default_tts_voice() -> String {
    crate::voice::tts::kokoro::DEFAULT_VOICE.into()
}
fn default_tts_rate() -> f32 {
    1.0
}
fn default_tts_volume() -> f32 {
    0.5
}
fn default_tts_provider() -> String {
    crate::voice::tts::cloud::LOCAL.into()
}
fn default_diarization_model() -> String {
    crate::speakers::diarize::DEFAULT_MODEL.into()
}

impl Default for YapConfig {
    fn default() -> Self {
        Self {
            hotkey: default_hotkey(),
            edit_hotkey: String::new(),
            rewrite_last_dictation: true,
            transforms: crate::dictation::transforms::defaults(),
            auto_transform: String::new(),
            auto_transform_kind: String::new(),
            my_languages: Vec::new(),
            language_shortcuts: Vec::new(),
            language_nudge: true,
            hinglish: false,
            assistant_hotkey: String::new(),
            read_hotkey: String::new(),
            as_said_hotkey: String::new(),
            paste_last_hotkey: String::new(),
            copy_last_hotkey: String::new(),
            quick_note_hotkey: String::new(),
            send_hotkey: String::new(),
            more_hotkeys: Default::default(),
            triple_tap_meeting: false,
            as_said_offer: false,
            snip_hotkey: default_snip_hotkey(),
            snip_voice: true,
            snip_listen: true,
            snip_copy: true,
            chat_image_days: default_chat_image_days(),
            snip_save_dir: String::new(),
            model_size: default_model_size(),
            use_gpu: true,
            stt_device: default_stt_device(),
            keep_recent_audio: true,
            input_device: None,
            mic_order: Vec::new(),
            mic_not_now: Vec::new(),
            sound_enabled: true,
            dictionary: Vec::new(),
            dictionary_fuzzy: true,
            learn_words: true,
            show_tips: true,
            notify_suggestions: true,
            notify_milestones: true,
            notify_feature_tips: true,
            notify_news: true,
            recording_mode: default_recording_mode(),
            quiet_other_apps: false,
            pause_media: false,
            keep_mic_ready: false,
            append_trailing_space: false,
            smart_spacing: true,
            screen_names: true,
            chat_no_full_stop: true,
            stacked_messages: false,
            chat_mentions: true,
            file_tags: true,
            auto_submit: false,
            voice_enter: true,
            terminal_mode: true,
            restore_clipboard: true,
            start_hidden: false,
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
            bar_quick_note: false,
            theme: default_theme(),
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
            local_model_at_launch: false,
            local_model_idle_mins: default_local_model_idle_mins(),
            local_vision: true,
            routing_scope: default_routing_scope(),
            app_routes: Vec::new(),
            cleanup_profiles: Vec::new(),
            cleanup_styles: None,
            cleanup_offer_seen: false,
            cleanup_strength: default_cleanup_strength(),
            cursor_context_cloud: false,
            llm_scopes: std::collections::HashMap::new(),
            chat_effort: std::collections::BTreeMap::new(),
            fast_mode: false,
            agent_name: String::new(),
            speak_replies: false,
            speak_when: default_speak_when(),
            talk_back_hands_free: true,
            assistant_answer_card: true,
            assistant_screen: true,
            assistant_screen_cloud: false,
            assistant_screen_asked: false,
            spoken_length: default_spoken_length(),
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
            history_keep: String::new(),
            bridge_enabled: true,
            mcp_allow_writes: false,
            meeting_open_notepad: true,
            meeting_split_screen: false,
            meeting_live_transcript: true,
            meeting_consent_message: String::new(),
            meeting_keep_audio: true,
            meeting_audio_days: default_meeting_audio_days(),
            meeting_transcript_days: 0,
            meeting_refine: default_meeting_refine(),
            meeting_diarization: false,
            diarization_model: default_diarization_model(),
            meeting_speaker_names: true,
            meeting_call_names: false,
            meeting_detection: true,
            meeting_auto_start: false,
            meeting_auto_start_apps: Vec::new(),
            meeting_detect_apps: std::collections::BTreeMap::new(),
            meeting_detect_style: default_meeting_detect_style(),
            meeting_hide_from_capture: true,
            meeting_max_minutes: default_meeting_max_minutes(),
            meeting_call_end: default_meeting_call_end(),
            meeting_end_when_quiet: true,
            meeting_hotkey: default_meeting_hotkey(),
            meeting_reminder: default_meeting_reminder(),
            meeting_brief: true,
            memory_search_chats: true,
            memory_learn_chats: true,
            memory_learn_meetings: false,
            memory_sensitive: false,
            search_by_meaning: false,
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
/// detected by [`crate::shell::portable`]). Otherwise it's `%APPDATA%/yap/` (or the
/// platform equivalent) — left unchanged so existing installs are unaffected.
pub fn data_dir() -> PathBuf {
    if let Some(dir) = crate::shell::portable::data_dir() {
        return dir.clone();
    }
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("yap")
}

fn config_path() -> PathBuf {
    data_dir().join("config.json")
}

/// Load config from disk, falling back to defaults. A config.json Yap can't
/// read is never saved over: one that isn't a config this build can read is
/// set aside and what can be read of it kept, the person told once
/// ([`load_at`], [`init`]); one another program holds runs Yap on the
/// defaults without saving them over it.
pub fn load() -> YapConfig {
    // A model evaluation (`eval.rs`) runs on its own settings, in memory (its
    // API key never goes to disk).
    if let Some(cfg) = crate::ai::eval::config() {
        return cfg;
    }
    // A unit test reads the real config.json at most: never sets it aside
    // or writes it.
    let mut cfg = if cfg!(test) {
        match read_at(&config_path()) {
            Read::Parsed(cfg) => *cfg,
            _ => YapConfig::default(),
        }
    } else {
        let loaded = load_at(&config_path());
        if let Some(reset) = loaded.reset {
            report_reset(reset);
        }
        loaded.cfg
    };
    // Migration: users still on the "default" cleanup preset get the current
    // default body (so prompt improvements land without a "Modified" badge). A
    // hand-customised body carries `pp_preset != "default"`, so it's preserved.
    if cfg.pp_preset == "default" {
        cfg.pp_prompt = default_pp_prompt();
    }
    // Styles by app: decided once for a config without the switch, and the
    // built-in styles' profiles there to edit.
    crate::ai::cleanup_styles::on_load(&mut cfg);
    migrate_retired_groq_models(&mut cfg);
    // More shortcuts per action: tidied (`input_hook::tidy_more`).
    crate::dictation::input_hook::tidy_more(&mut cfg);
    // The GPU switch from before "Runs on" (2026-10-22): off meant the
    // processor. (`save` keeps the two in step, so this never undoes a
    // choice made since.)
    if !cfg.use_gpu {
        cfg.stt_device = "cpu".into();
    }
    // One-time migrations are saved straight away, so they can never run a
    // second time and undo a choice the user makes afterwards.
    follow_an_older_builds_mic(&mut cfg);
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
const CONFIG_VERSION: u32 = 5;

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
        if cfg.tts_engine == crate::voice::tts::WINDOWS {
            cfg.tts_engine = crate::voice::tts::DEFAULT_ENGINE.into();
        }
        if !crate::voice::tts::kokoro::is_voice(&cfg.tts_voice) {
            if cfg.tts_windows_voice.is_empty() {
                cfg.tts_windows_voice = std::mem::take(&mut cfg.tts_voice);
            }
            cfg.tts_voice = default_tts_voice();
        }
    }
    if cfg.config_version < 3 {
        // v3 (2026-10-22): Yap's windows got a theme, and the bar's colours
        // "Match app" by default. Until then "light" was the bar's default
        // and every install saved it without choosing it: it becomes "app"
        // once (a light app keeps a light bar). A bar someone made dark
        // stays dark.
        if cfg.bar_theme.eq_ignore_ascii_case("light") {
            cfg.bar_theme = default_bar_theme();
        }
    }
    if cfg.config_version < 4 {
        // v4 (2026-10-25): one microphone became an order of them
        // (`mics.rs`). The one picked stays first — by its name, the only
        // thing kept until now (its endpoint id is added when Settings next
        // saves the order) — then Windows' default, which is what an
        // unplugged pick fell back to anyway… except it said "No microphone
        // found" before.
        // `input_device` itself stays: an older Yap reads only that.
        if let Some(name) = cfg.input_device.clone().map(|n| n.trim().to_string()).filter(|n| !n.is_empty()) {
            if !cfg.mic_order.iter().any(|m| m.name == name) {
                cfg.mic_order.insert(0, crate::dictation::mics::MicChoice { id: String::new(), name });
            }
        }
    }
    if cfg.config_version < 5 {
        // v5 (2026-10-25): the bar's "Use what I said" after a cleaned
        // dictation is off unless chosen (the maintainer: offering to undo
        // the cleanup every time looked unprofessional). Turned off once,
        // for everyone; switched on again in Settings it stays on.
        cfg.as_said_offer = false;
    }
    cfg.config_version = CONFIG_VERSION;
    true
}

/// The installed nightly and Yap - Dev share one config, and an older Yap
/// knows a single microphone (`input_device`). [`save`] writes the first of
/// the order there, so a different name means the person picked one in an
/// older build: it goes first here too. (None with an order: left alone.)
fn follow_an_older_builds_mic(cfg: &mut YapConfig) {
    let Some(name) = cfg.input_device.as_deref().map(str::trim).filter(|n| !n.is_empty()).map(str::to_string) else {
        return;
    };
    if cfg.mic_order.first().is_some_and(|m| m.name == name) {
        return;
    }
    let choice = match cfg.mic_order.iter().position(|m| m.name == name) {
        Some(i) => cfg.mic_order.remove(i),
        None => crate::dictation::mics::MicChoice { id: String::new(), name },
    };
    cfg.mic_order.insert(0, choice);
}

// ---- Files on disk: atomic writes, corrupt files set aside ------------------

/// Atomic file write: write to a temp file beside `path`, then rename it over
/// the target (`std::fs::rename` replaces on Windows). A `tauri dev` rebuild
/// or crash mid-write can otherwise leave a torn JSON file — which the stores
/// would fail to parse and (before this fix) silently replace with an empty
/// default, destroying user data. Each write has a temp file of its own
/// (`<name>.<pid>-<n>.tmp`): two writes of one file at once shared
/// `<name>.tmp` until 2026-10-23, so one could rename the other's half-written
/// file into place. The rename is tried again for a moment while another
/// program (an antivirus, a backup tool) holds the target. The temp file's
/// bytes reach the disk before the rename (2026-10-29): NTFS can write the
/// rename to its journal before the data, so a power cut or a blue screen
/// moments after a save left the store renamed into place but full of
/// zeros — set aside as corrupt, everything in it gone with the old file.
pub(crate) fn atomic_write(path: &Path, contents: &str) -> std::io::Result<()> {
    atomic_write_bytes(path, contents.as_bytes())
}

/// [`atomic_write`] for bytes.
pub(crate) fn atomic_write_bytes(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "yap".into());
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = path.with_file_name(format!("{name}.{}-{n}.tmp", std::process::id()));
    let written = std::fs::File::create(&tmp)
        .and_then(|mut file| file.write_all(bytes).and_then(|()| file.sync_all()))
        .and_then(|()| rename_patiently(&tmp, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

/// `std::fs::rename`, tried again for up to ~100 ms while the target is held
/// by another program (sharing or lock violation, access denied).
fn rename_patiently(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut tries = 0;
    loop {
        match std::fs::rename(from, to) {
            Err(e) if tries < 5 && held(&e) => {
                tries += 1;
                std::thread::sleep(Duration::from_millis(20));
            }
            done => return done,
        }
    }
}

/// Another program has the file open (Windows: access denied, a sharing or
/// lock violation).
pub(crate) fn held(e: &std::io::Error) -> bool {
    e.kind() == std::io::ErrorKind::PermissionDenied || matches!(e.raw_os_error(), Some(32 | 33))
}

/// Move a file Yap can't read aside as `<stem>.corrupt-<unix-secs>` (`-2`,
/// `-3`… when that's taken; copied when it can't be moved) so it is NEVER
/// overwritten by a fresh default — the data stays recoverable. Returns
/// where it went.
fn set_aside(path: &Path) -> std::io::Result<PathBuf> {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let backup = (1..)
        .map(|n| match n {
            1 => path.with_extension(format!("corrupt-{secs}")),
            n => path.with_extension(format!("corrupt-{secs}-{n}")),
        })
        .find(|p| !p.exists())
        .unwrap_or_else(|| path.with_extension("corrupt"));
    match rename_patiently(path, &backup) {
        Ok(()) => Ok(backup),
        Err(_) => std::fs::copy(path, &backup).map(|_| backup),
    }
}

/// How old a temp file must be before the start-up sweep takes it: a write
/// takes milliseconds, so one this old was left by a crash (or a Yap killed
/// mid-write) — never one another Yap is writing now (a dev build shares the
/// folder with the installed one).
const STALE_TEMP: Duration = Duration::from_secs(10 * 60);

/// The stores whose writes went through `<stem>.tmp` before 2026-10-23
/// (`with_extension("tmp")`), whose leftovers the sweep takes too.
const OLD_TEMP_STEMS: &[&str] = &[
    "config",
    "notes",
    "chats",
    "memory",
    "calendar",
    "history",
    "groq_usage",
    "dictionary_usage",
    "hints",
    "word_suggestions",
    "notices",
    "quieted_apps",
    "stt_speed",
    "model_catalog",
    "updates",
];

/// A temp file of Yap's own writes: `<file>.<pid>-<n>.tmp` ([`atomic_write`]),
/// or a store's `<stem>.tmp` of before 2026-10-23. Nothing else.
fn is_yap_temp(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".tmp") else {
        return false;
    };
    if OLD_TEMP_STEMS.contains(&stem) {
        return true;
    }
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    match stem.rsplit_once('.').and_then(|(file, tag)| Some((file, tag.split_once('-')?))) {
        Some((file, (pid, n))) => !file.is_empty() && digits(pid) && digits(n),
        None => false,
    }
}

/// Remove the temp files of Yap's own writes ([`is_yap_temp`]) older than
/// `older_than` from `dir` (files only, not its subfolders). Returns how many.
fn sweep_temps_in(dir: &Path, older_than: Duration) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let now = std::time::SystemTime::now();
    entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter(|e| is_yap_temp(&e.file_name().to_string_lossy()))
        .filter(|e| {
            e.metadata()
                .and_then(|m| m.modified())
                .is_ok_and(|at| now.duration_since(at).is_ok_and(|age| age >= older_than))
        })
        .filter(|e| std::fs::remove_file(e.path()).is_ok())
        .count()
}

/// At start-up (on a thread of its own): the temp files a crash mid-write
/// left behind — each write's is unique since 2026-10-23, so nothing
/// replaced them — in the data folder and the folders Yap writes the same
/// way (`icons/`, `llm/`, each meeting's in `meeting-audio/`). Only Yap's
/// own names, only stale ones.
pub fn sweep_temp_files() {
    let dir = data_dir();
    let mut dirs = vec![dir.clone(), dir.join("icons"), dir.join("llm")];
    if let Ok(meetings) = std::fs::read_dir(dir.join("meeting-audio")) {
        dirs.extend(meetings.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| e.path()));
    }
    let removed: usize = dirs.iter().map(|d| sweep_temps_in(d, STALE_TEMP)).sum();
    if removed > 0 {
        tracing::info!(removed, "Removed temp files an interrupted save left behind");
    }
}

/// Move an unparseable store file aside (`<name>.corrupt-<unix-secs>`) so it
/// is NEVER overwritten by a fresh default — the data stays recoverable.
/// Whether it was (a store that couldn't set it aside must not write over
/// it: `saving::Saver::read` treats it as a file it couldn't read).
pub(crate) fn quarantine_corrupt(path: &Path) -> bool {
    match set_aside(path) {
        Ok(backup) => {
            tracing::error!(
                file = %file_name(path),
                aside = %file_name(&backup),
                "A store's file couldn't be read as one: set aside, starting fresh, the data kept"
            );
            true
        }
        Err(e) => {
            tracing::error!(file = %file_name(path), kind = ?e.kind(), "A store's file couldn't be read as one, nor set aside ({e})");
            false
        }
    }
}

/// A path's file name, for the log (never the folder: the user's name).
fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

// ---- config.json: read, repaired, saved one at a time ------------------------

/// Saves of config.json, one at a time. Some ten threads load, change and
/// save it (the tray, the bar, call detection, Settings' auto-save…).
static SAVES: Mutex<()> = Mutex::new(());

thread_local! {
    /// This thread holds [`SAVES`] already (a save inside [`update`], a
    /// repair inside a load inside it): it isn't taken twice.
    static HOLDING_SAVES: Cell<bool> = const { Cell::new(false) };
}

/// [`SAVES`], held until dropped (nothing more where this thread holds it).
struct SaveLock(Option<MutexGuard<'static, ()>>);

impl Drop for SaveLock {
    fn drop(&mut self) {
        if self.0.is_some() {
            HOLDING_SAVES.with(|h| h.set(false));
        }
    }
}

fn lock_saves() -> SaveLock {
    if HOLDING_SAVES.with(Cell::get) {
        return SaveLock(None);
    }
    let guard = SAVES.lock().unwrap_or_else(|p| p.into_inner());
    HOLDING_SAVES.with(|h| h.set(true));
    SaveLock(Some(guard))
}

/// config.json files Yap couldn't open when it last read them (another
/// program held them, or one it couldn't read couldn't be set aside): Yap
/// runs on the defaults meanwhile and never saves them over the real file,
/// until a read of it works again.
static UNREAD: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Mark `path` unread or read; returns whether it was unread before.
fn mark_unread(path: &Path, unread: bool) -> bool {
    let mut list = UNREAD.lock().unwrap_or_else(|p| p.into_inner());
    let was = list.iter().any(|p| p == path);
    if unread && !was {
        list.push(path.to_path_buf());
    } else if !unread && was {
        list.retain(|p| p != path);
    }
    was
}

/// What reading config.json found.
enum Read {
    Parsed(Box<YapConfig>),
    /// No file: a first run, or one moved aside.
    Missing,
    /// There, but not a config this build can read — torn, not JSON, a
    /// value of another type: its bytes.
    Corrupt(Vec<u8>),
    /// There, but it couldn't be opened (another program holds it).
    Unopened(std::io::Error),
}

fn read_at(path: &Path) -> Read {
    match std::fs::read(path) {
        Ok(bytes) => match serde_json::from_slice(without_bom(&bytes)) {
            Ok(cfg) => Read::Parsed(Box::new(cfg)),
            Err(_) => Read::Corrupt(bytes),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Read::Missing,
        Err(e) => Read::Unopened(e),
    }
}

/// `<dir>/config.json` read and nothing else: no migration, no saving, no
/// setting aside (`yap.exe bench` reads another Yap's settings this way).
pub fn peek(dir: &Path) -> Option<YapConfig> {
    match read_at(&dir.join("config.json")) {
        Read::Parsed(cfg) => Some(*cfg),
        _ => None,
    }
}

/// `bytes` without the byte-order mark Notepad puts first.
pub(crate) fn without_bom(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes)
}

/// A config.json Yap couldn't read: where it went, what became of it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Reset {
    /// The file as it was, set aside. `None`: it couldn't be, so Yap runs on
    /// the defaults without saving over it.
    pub moved_to: Option<PathBuf>,
    /// Some settings were read from it (only those it couldn't read went
    /// back to their defaults); else all of them did.
    pub kept: bool,
    /// The settings (JSON names) that went back to their defaults when some
    /// were kept.
    pub dropped: Vec<String>,
}

/// config.json as [`load_at`] read it.
struct Loaded {
    cfg: YapConfig,
    /// It couldn't be read and was set aside: the person is to be told.
    reset: Option<Reset>,
}

/// config.json at `path`: parsed; none, the defaults; held by another
/// program (after a few tries), the defaults, never saved over it
/// ([`UNREAD`]); not a config this build can read, repaired ([`repair_at`]).
/// Until 2026-10-23 every one of those was "the defaults", and the next save
/// wrote them over the real file: API keys, the dictionary and the hotkeys
/// gone.
fn load_at(path: &Path) -> Loaded {
    let mut tries = 0;
    let read = loop {
        match read_at(path) {
            // Another program holding it is usually gone in a moment.
            Read::Unopened(_) if tries < 4 => {
                tries += 1;
                std::thread::sleep(Duration::from_millis(50));
            }
            read => break read,
        }
    };
    match read {
        Read::Parsed(cfg) => {
            mark_unread(path, false);
            Loaded { cfg: *cfg, reset: None }
        }
        Read::Missing => {
            mark_unread(path, false);
            Loaded { cfg: YapConfig::default(), reset: None }
        }
        Read::Unopened(e) => {
            if !mark_unread(path, true) {
                tracing::warn!(error = %e, "config.json couldn't be opened: Yap runs on the defaults and won't save over it until it can read it");
            }
            Loaded { cfg: YapConfig::default(), reset: None }
        }
        Read::Corrupt(_) => repair_at(path),
    }
}

/// A config.json this build can't read, under the save lock (read again
/// there: another thread may have repaired it, or saved a good one,
/// meanwhile). The file as it was is set aside (`config.corrupt-<secs>`),
/// never saved over. The settings in it that can still be read are kept —
/// a value of another type (a newer build's, after a downgrade) resets only
/// that setting — and written back as config.json; a torn file or one that
/// isn't JSON leaves the defaults. Where it couldn't be set aside, Yap runs
/// on what it could read without saving over it.
fn repair_at(path: &Path) -> Loaded {
    let _saves = lock_saves();
    let bytes = match read_at(path) {
        Read::Corrupt(bytes) => bytes,
        Read::Parsed(cfg) => return Loaded { cfg: *cfg, reset: None },
        Read::Missing | Read::Unopened(_) => return load_at(path),
    };
    let salvaged = salvage(&bytes);
    let moved_to = match set_aside(path) {
        Ok(to) => Some(to),
        Err(e) => {
            mark_unread(path, true);
            tracing::warn!(error = %e, "config.json couldn't be read nor set aside: Yap won't save over it");
            None
        }
    };
    let (cfg, kept, dropped) = match salvaged {
        Some((cfg, dropped)) => {
            if moved_to.is_some() {
                if let Err(e) = write_at(path, &cfg) {
                    tracing::warn!("config.json: the settings kept couldn't be written back ({e})");
                }
            }
            (cfg, true, dropped)
        }
        None => (YapConfig::default(), false, Vec::new()),
    };
    let aside = moved_to.as_deref().and_then(Path::file_name).map(|n| n.to_string_lossy().into_owned());
    tracing::warn!(
        aside = aside.as_deref().unwrap_or("(not set aside)"),
        bytes = bytes.len(),
        kept,
        reset_settings = dropped.len(),
        "config.json couldn't be read: set aside, never saved over"
    );
    Loaded { cfg, reset: Some(Reset { moved_to, kept, dropped }) }
}

/// The settings in `bytes` — JSON, but not a config this build can read —
/// that can still be read, each on its own, and the names of those that
/// can't. `None`: not a JSON object at all (torn, empty).
fn salvage(bytes: &[u8]) -> Option<(YapConfig, Vec<String>)> {
    let serde_json::Value::Object(fields) = serde_json::from_slice(without_bom(bytes)).ok()? else {
        return None;
    };
    let mut kept = serde_json::Map::new();
    let mut dropped = Vec::new();
    for (name, value) in fields {
        // Every field has a serde default: a config of this one alone reads
        // when its value does.
        let alone = serde_json::Value::Object(std::iter::once((name.clone(), value.clone())).collect());
        if serde_json::from_value::<YapConfig>(alone).is_ok() {
            kept.insert(name, value);
        } else {
            dropped.push(name);
        }
    }
    let cfg = serde_json::from_value(serde_json::Value::Object(kept)).ok()?;
    Some((cfg, dropped))
}

/// Persist config to disk (atomically, one save at a time), then
/// `yap-config-saved` to every window: Settings' copy adopts what changed
/// (Settings.svelte), the theme follows (appearance.js). Refused while
/// config.json is one Yap couldn't open when it last read it ([`UNREAD`]).
pub fn save(cfg: &YapConfig) -> Result<(), String> {
    // A model evaluation's settings stay in memory; a unit test never writes
    // the real config.json.
    if crate::ai::eval::active() || cfg!(test) {
        return Ok(());
    }
    save_at(&config_path(), cfg)?;
    if let Some(app) = APP.get() {
        let _ = app.emit(EVENT_SAVED, ());
    }
    Ok(())
}

/// Change the config in one step: loaded, changed by `change`, saved when
/// anything changed — never interleaved with another save, so a change
/// another thread saved meanwhile isn't lost (load → change → save on two
/// threads at once kept only one). Returns the config as it is now.
pub fn update(change: impl FnOnce(&mut YapConfig)) -> Result<YapConfig, String> {
    let _saves = lock_saves();
    let mut cfg = load();
    if changed(&mut cfg, change) {
        save(&cfg)?;
    }
    Ok(cfg)
}

/// [`update`] with a change that can be refused: on `Err` nothing is saved
/// (not half a change) and the error comes back.
pub fn try_update(change: impl FnOnce(&mut YapConfig) -> Result<(), String>) -> Result<YapConfig, String> {
    let _saves = lock_saves();
    let mut cfg = load();
    if try_changed(&mut cfg, change)? {
        save(&cfg)?;
    }
    Ok(cfg)
}

/// A page's save (`mine`: the config it read, `base`, with its changes)
/// put onto config.json as it is now (`now`), for [`update`]: what the page
/// changed comes from it, the rest from `now` — so a change Rust saved
/// between the page's `get_config` and its `save_config` (the bar's mic
/// pick, an offer's answer, a migration) stays. A JSON three-way merge, as
/// the page's own (`settings/configMerge.js` `merge3`): objects merge key by
/// key (one provider's key here, another's there); a setting changed on
/// both sides keeps the page's, the edit being made. Pure.
pub fn merge_saved(base: &YapConfig, mine: &YapConfig, now: &YapConfig) -> YapConfig {
    // Compared as config.json keeps them: one dictionary row per way a word
    // is heard (`also_heard` isn't serialised).
    let value = |c: &YapConfig| {
        let rows = YapConfig { dictionary: crate::dictionary::variants::rows(&c.dictionary), ..c.clone() };
        serde_json::to_value(rows).ok()
    };
    let (Some(b), Some(m), Some(n)) = (value(base), value(mine), value(now)) else {
        return mine.clone();
    };
    merge3(Some(&b), Some(&m), Some(&n))
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_else(|| mine.clone())
}

/// [`merge_saved`]'s three-way merge of JSON values (`None`: not there).
fn merge3(base: Option<&serde_json::Value>, mine: Option<&serde_json::Value>, theirs: Option<&serde_json::Value>) -> Option<serde_json::Value> {
    use serde_json::Value;
    if mine == base {
        return theirs.cloned();
    }
    if theirs == base || mine == theirs {
        return mine.cloned();
    }
    if let (Some(Value::Object(m)), Some(Value::Object(t))) = (mine, theirs) {
        let b = base.and_then(Value::as_object);
        let keys: std::collections::BTreeSet<&String> = m.keys().chain(t.keys()).collect();
        let out = keys
            .into_iter()
            .filter_map(|k| merge3(b.and_then(|b| b.get(k)), m.get(k), t.get(k)).map(|v| (k.clone(), v)))
            .collect();
        return Some(Value::Object(out));
    }
    mine.cloned()
}

/// Apply `change` to a copy of `cfg`, kept only if it worked: whether
/// anything changed.
fn try_changed(cfg: &mut YapConfig, change: impl FnOnce(&mut YapConfig) -> Result<(), String>) -> Result<bool, String> {
    let mut next = cfg.clone();
    change(&mut next)?;
    Ok(changed(cfg, |c| *c = next))
}

#[cfg(test)]
fn try_update_at(path: &Path, change: impl FnOnce(&mut YapConfig) -> Result<(), String>) -> Result<YapConfig, String> {
    let _saves = lock_saves();
    let mut cfg = load_at(path).cfg;
    if try_changed(&mut cfg, change)? {
        save_at(path, &cfg)?;
    }
    Ok(cfg)
}

/// Apply `change` to `cfg`: whether anything changed.
fn changed(cfg: &mut YapConfig, change: impl FnOnce(&mut YapConfig)) -> bool {
    let before = serde_json::to_value(&*cfg).ok();
    change(cfg);
    before.is_none() || serde_json::to_value(&*cfg).ok() != before
}

#[cfg(test)]
fn update_at(path: &Path, change: impl FnOnce(&mut YapConfig)) -> Result<YapConfig, String> {
    let _saves = lock_saves();
    let mut cfg = load_at(path).cfg;
    if changed(&mut cfg, change) {
        save_at(path, &cfg)?;
    }
    Ok(cfg)
}

/// `cfg` saved to `path` (see [`save`]). A save that fails says so as every
/// store's does (`shell/saving.rs`): "Couldn't save your settings: the disk
/// is full." — the page shows it with Try again (`notices/saveErrors.js`).
/// Nothing is kept here: the page that saved holds the change, and saves it
/// again.
fn save_at(path: &Path, cfg: &YapConfig) -> Result<(), String> {
    use crate::shell::saving::{Reason, SaveError};
    let failed = |reason| String::from(SaveError { what: "your settings", reason, whole: "your settings" });
    let _saves = lock_saves();
    if UNREAD.lock().unwrap_or_else(|p| p.into_inner()).iter().any(|p| p == path) {
        // Yap couldn't open config.json when it last read it: never saved over.
        return Err(failed(Reason::Unread));
    }
    // A test run's full disk (`e2e_fail_saves` with "config").
    #[cfg(debug_assertions)]
    if crate::shell::saving::staged_failure("config") {
        return Err(failed(Reason::DiskFull));
    }
    // Anything this build saves has been through `load`'s migrations already,
    // so stamp the current version even if the caller's copy lost the field —
    // a missing version would re-run the one-time migrations on next load.
    // Never lower it (a newer build's file, opened after a downgrade).
    let cfg = YapConfig {
        config_version: cfg.config_version.max(CONFIG_VERSION),
        // One row per way a word is heard, as every Yap reads it
        // (dictionary/variants.rs): an entry sent with `alsoHeard` is
        // written out as rows of its own, never dropped.
        dictionary: crate::dictionary::variants::rows(&cfg.dictionary),
        // "Runs on" is what's chosen; the old GPU switch follows it.
        use_gpu: cfg.stt_device != "cpu",
        // An older Yap knows one microphone: the first of the order.
        input_device: cfg.mic_order.first().map(|m| m.name.clone()),
        ..cfg.clone()
    };
    write_at(path, &cfg).map_err(|e| failed(Reason::at(&e, path)))
}

/// `cfg` written to `path` as it is (atomically).
fn write_at(path: &Path, cfg: &YapConfig) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_string_pretty(cfg).map_err(std::io::Error::other)?;
    atomic_write(path, &json)
}

// ---- Telling the person -------------------------------------------------------

/// Every window's event after a save of config.json, from wherever it came.
pub const EVENT_SAVED: &str = "yap-config-saved";

static APP: OnceLock<AppHandle> = OnceLock::new();

/// A config.json Yap couldn't read, waiting to be told (once).
static RESET: Mutex<Option<Reset>> = Mutex::new(None);

/// …and where the last one went, for its alert's "Show file".
static OLD_FILE: Mutex<Option<PathBuf>> = Mutex::new(None);

/// A config.json Yap couldn't read was set aside (or couldn't be): told now
/// when the app is up, else by [`init`].
fn report_reset(reset: Reset) {
    if let Some(to) = &reset.moved_to {
        *OLD_FILE.lock().unwrap_or_else(|p| p.into_inner()) = Some(to.clone());
    }
    *RESET.lock().unwrap_or_else(|p| p.into_inner()) = Some(reset);
    if let Some(app) = APP.get() {
        tell_reset(app);
    }
}

/// Start with the app: `yap-config-saved` after every save from now on,
/// and a config.json Yap couldn't read at start (the first load runs before
/// logging and the windows) said once the windows are up.
pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
    if RESET.lock().unwrap_or_else(|p| p.into_inner()).is_some() {
        let app = app.clone();
        let _ = std::thread::Builder::new().name("config-reset".into()).spawn(move || {
            std::thread::sleep(Duration::from_secs(4));
            tell_reset(&app);
        });
    }
}

fn tell_reset(app: &AppHandle) {
    let Some(reset) = RESET.lock().unwrap_or_else(|p| p.into_inner()).take() else {
        return;
    };
    let aside = reset.moved_to.as_deref().and_then(Path::file_name).map(|n| n.to_string_lossy().into_owned());
    tracing::warn!(
        aside = aside.as_deref().unwrap_or("(not set aside)"),
        kept = reset.kept,
        reset_settings = reset.dropped.len(),
        "Settings: config.json couldn't be read; telling the person"
    );
    crate::notices::alert::show_soon(app, reset_alert(&reset));
}

/// What the person is told about a config.json Yap couldn't read.
pub(crate) fn reset_alert(reset: &Reset) -> crate::notices::alert::Alert {
    let (title, body) = match (&reset.moved_to, reset.kept) {
        (Some(to), true) => {
            let n = reset.dropped.len();
            let which = if n == 1 { "1 setting".to_string() } else { format!("{n} settings") };
            (
                "Some of your settings were reset",
                format!(
                    "Yap couldn't read all of its settings file, so {which} went back to the default. The rest are as you left them. The file as it was is saved as {}.",
                    to.display()
                ),
            )
        }
        (Some(to), false) => (
            "Your settings were reset",
            format!(
                "Yap couldn't read its settings file, so it started with the default settings. The old file is saved as {}.",
                to.display()
            ),
        ),
        (None, _) => (
            "Yap couldn't read your settings",
            "It's using the default settings for now and won't save over your settings file. Close any program that has config.json open, then restart Yap.".to_string(),
        ),
    };
    crate::notices::alert::Alert {
        kind: "configReset",
        icon: "settings",
        title: title.into(),
        body,
        action: reset.moved_to.is_some().then_some(crate::notices::alert::Action {
            id: crate::notices::alert::SHOW_OLD_SETTINGS,
            label: "Show file",
        }),
        timeout_ms: 60_000,
    }
}

/// The alert's "Show file": the settings file Yap couldn't read, in
/// Explorer. A test run only says so in the log.
pub(crate) fn show_old_file(app: &AppHandle) {
    let Some(path) = OLD_FILE.lock().unwrap_or_else(|p| p.into_inner()).clone() else {
        return;
    };
    if crate::shell::e2e::active() {
        tracing::info!("e2e: not showing the old settings file");
        return;
    }
    use tauri_plugin_opener::OpenerExt;
    if let Err(e) = app.opener().reveal_item_in_dir(&path) {
        tracing::warn!("Settings: couldn't show the old settings file ({e})");
    }
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
        // Every way it's heard, the longest first: "cube" never eats the
        // start of "cube and eighties" (`dictionary/variants.rs`).
        let mut heard = entry.heard();
        heard.sort_by_key(|h| std::cmp::Reverse(h.chars().count()));
        for h in heard {
            if let Some(re) = term_regex(h) {
                let to = entry.to.clone();
                out = re
                    .replace_all(&out, |_: &regex::Captures| to.clone())
                    .into_owned();
            }
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
/// ends, not on "Norwich."). A one-line snippet mid-sentence keeps the
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
            starred: false,
            also_heard: Vec::new(),
        }
    }

    fn snippet(from: &str, to: &str) -> DictionaryEntry {
        DictionaryEntry { snippet: true, ..entry(from, to) }
    }

    // ---- config.json on disk: a scratch folder of its own per test ----

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("yap-config-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The files beside config.json (copies moved aside, temp files left).
    fn others(dir: &std::path::Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n != "config.json")
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_config_that_cant_be_read_is_moved_aside_never_saved_over() {
        let dir = scratch("torn");
        let path = dir.join("config.json");
        // Torn mid-write: the hotkey, a key and the dictionary in it.
        let torn: &[u8] = br#"{ "hotkey": "kb:121", "ppApiKeys": { "groq": "example-key" }, "dictionary": [ { "from": "power to keep", "#;
        std::fs::write(&path, torn).unwrap();
        let loaded = load_at(&path);
        // The defaults for now…
        assert_eq!(loaded.cfg.hotkey, YapConfig::default().hotkey);
        // …and the file moved aside as it was, never to be saved over.
        let aside = others(&dir);
        assert_eq!(aside.len(), 1, "{aside:?}");
        assert!(aside[0].starts_with("config.corrupt-"), "{aside:?}");
        assert_eq!(std::fs::read(dir.join(&aside[0])).unwrap(), torn);
        assert!(!path.exists());
        // Told once, with where it is.
        let reset = loaded.reset.expect("the person is told");
        assert_eq!(reset.moved_to, Some(dir.join(&aside[0])));
        assert!(!reset.kept);
        // The next save writes a fresh config.json beside it.
        let cfg = YapConfig { hotkey: "kb:122".into(), ..loaded.cfg };
        save_at(&path, &cfg).unwrap();
        let again = load_at(&path);
        assert_eq!(again.cfg.hotkey, "kb:122");
        assert!(again.reset.is_none(), "nothing more to tell");
        assert_eq!(std::fs::read(dir.join(&aside[0])).unwrap(), torn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_setting_this_build_cant_read_resets_alone_the_rest_kept() {
        let dir = scratch("salvage");
        let path = dir.join("config.json");
        // A value of another type (a newer build's, opened after a
        // downgrade): only the dictionary can't be read.
        let text = r#"{ "hotkey": "kb:121", "ppApiKeys": { "groq": "example-key" }, "dictionary": "not a list", "modelSize": "moonshine-base", "configVersion": 3 }"#;
        std::fs::write(&path, text).unwrap();
        let loaded = load_at(&path);
        assert_eq!(loaded.cfg.hotkey, "kb:121");
        assert_eq!(loaded.cfg.pp_api_keys.get("groq").map(String::as_str), Some("example-key"));
        assert_eq!(loaded.cfg.model_size, "moonshine-base");
        assert!(loaded.cfg.dictionary.is_empty());
        let reset = loaded.reset.expect("the person is told");
        assert!(reset.kept);
        assert_eq!(reset.dropped, vec!["dictionary".to_string()]);
        // The file as it was, aside; what could be kept is config.json again.
        let aside = others(&dir);
        assert_eq!(aside.len(), 1, "{aside:?}");
        assert_eq!(std::fs::read_to_string(dir.join(&aside[0])).unwrap(), text);
        let again = load_at(&path);
        assert!(again.reset.is_none());
        assert_eq!((again.cfg.hotkey.as_str(), again.cfg.model_size.as_str()), ("kb:121", "moonshine-base"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn saves_from_many_threads_never_tear_the_file() {
        let dir = scratch("threads");
        let path = dir.join("config.json");
        save_at(&path, &YapConfig::default()).unwrap();
        let problems = std::sync::Mutex::new(Vec::<String>::new());
        // Windows refusing a rename or a read for a while (an antivirus
        // scanning the new file: in the temp folder on C:, under load, for
        // 10 s and more, no process holding either file) is a save refused
        // and the file left whole — not a tear (`held`).
        let (written, refused) = (AtomicU64::new(0), AtomicU64::new(0));
        std::thread::scope(|s| {
            for t in 0..8usize {
                let (path, problems, written, refused) = (&path, &problems, &written, &refused);
                s.spawn(move || {
                    // Each thread's config is a different size: a torn
                    // write shows as a file that doesn't parse.
                    let cfg = YapConfig {
                        hotkey: format!("kb:{}", 100 + t),
                        dictionary: (0..t * 40).map(|i| entry(&format!("word {i}"), &format!("Word {i}"))).collect(),
                        ..YapConfig::default()
                    };
                    for _ in 0..40 {
                        match save_at(path, &cfg) {
                            Ok(()) => {
                                written.fetch_add(1, Ordering::Relaxed);
                            }
                            Err(e) if ["(os error 5)", "(os error 32)", "(os error 33)"].iter().any(|code| e.ends_with(code)) => {
                                refused.fetch_add(1, Ordering::Relaxed);
                            }
                            Err(e) => problems.lock().unwrap().push(format!("save: {e}")),
                        }
                        match std::fs::read(path) {
                            Ok(bytes) => {
                                if let Err(e) = serde_json::from_slice::<YapConfig>(&bytes) {
                                    problems.lock().unwrap().push(format!("read: {e}"));
                                }
                            }
                            Err(e) if held(&e) => {
                                refused.fetch_add(1, Ordering::Relaxed);
                            }
                            Err(e) => problems.lock().unwrap().push(format!("read: {e}")),
                        }
                    }
                });
            }
        });
        let problems = problems.into_inner().unwrap();
        assert!(problems.is_empty(), "{} problems, first: {:?}", problems.len(), problems.first());
        assert!(written.into_inner() > 0, "saves went through");
        // Nothing left beside it (no temp file) — unless Windows refused a
        // save, when it may have refused its temp file's removal too (the
        // start-up sweep takes those).
        if refused.into_inner() == 0 {
            assert_eq!(others(&dir), Vec::<String>::new());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn changes_from_many_threads_are_all_kept() {
        let dir = scratch("update");
        let path = dir.join("config.json");
        save_at(&path, &YapConfig::default()).unwrap();
        std::thread::scope(|s| {
            for t in 0..8usize {
                let path = &path;
                s.spawn(move || {
                    for i in 0..10 {
                        update_at(path, |c| c.dictionary.push(entry(&format!("t{t} w{i}"), "x"))).unwrap();
                    }
                });
            }
        });
        // Every thread's every change: none saved over by another's.
        assert_eq!(load_at(&path).cfg.dictionary.len(), 80);
        // Nothing changed: nothing written.
        let before = std::fs::metadata(&path).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(30));
        update_at(&path, |c| c.hotkey = c.hotkey.clone()).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), before);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A change that fails saves nothing; changes from many threads at once
    /// are all kept.
    #[test]
    fn a_change_that_fails_saves_nothing_and_none_is_lost() {
        let dir = scratch("try-update");
        let path = dir.join("config.json");
        save_at(&path, &YapConfig::default()).unwrap();
        let before = std::fs::read(&path).unwrap();
        let refused = try_update_at(&path, |c| {
            c.hotkey = "kb:121".into();
            Err("no such model".into())
        });
        assert_eq!(refused.unwrap_err(), "no such model");
        assert_eq!(std::fs::read(&path).unwrap(), before, "half a change never saved");
        std::thread::scope(|s| {
            for t in 0..8usize {
                let path = &path;
                s.spawn(move || {
                    for i in 0..10 {
                        try_update_at(path, |c| {
                            c.dictionary.push(entry(&format!("t{t} w{i}"), "x"));
                            Ok(())
                        })
                        .unwrap();
                    }
                });
            }
        });
        assert_eq!(load_at(&path).cfg.dictionary.len(), 80);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A page's save (`save_config` with the config it read): put onto
    /// config.json as it is now, under the save lock. Until 2026-10-30 it
    /// saved the page's whole copy, so a change Rust saved between the
    /// page's `get_config` and its `save_config` (the bar's mic pick, a
    /// provider's key) was put back.
    #[test]
    fn a_pages_save_keeps_what_rust_saved_since_it_read_the_config() {
        let dir = scratch("page-save");
        let path = dir.join("config.json");
        let mut first = YapConfig::default();
        first.pp_api_keys.insert("groq".into(), "gsk-example".into());
        first.dictionary = vec![entry("jason", "JSON")];
        save_at(&path, &first).unwrap();
        // The page reads it, and changes the hotkey, the dictionary and one key.
        let base = load_at(&path).cfg;
        let mut mine = base.clone();
        mine.hotkey = "kb:121".into();
        mine.dictionary.push(entry("tory", "Tauri"));
        mine.pp_api_keys.insert("groq".into(), "gsk-new".into());
        // Meanwhile Rust saves a mic pick, another provider's key, and
        // "learn words" off.
        update_at(&path, |c| {
            c.mic_order = vec![crate::dictation::mics::MicChoice { id: "{0.0.1}".into(), name: "Desk mic".into() }];
            c.pp_api_keys.insert("openai".into(), "sk-example".into());
            c.learn_words = false;
        })
        .unwrap();
        // The page's save.
        update_at(&path, |c| *c = merge_saved(&base, &mine, c)).unwrap();
        let saved = load_at(&path).cfg;
        // The page's changes…
        assert_eq!(saved.hotkey, "kb:121");
        assert_eq!(saved.dictionary.iter().map(|e| e.from.as_str()).collect::<Vec<_>>(), ["jason", "tory"]);
        assert_eq!(saved.pp_api_keys.get("groq").map(String::as_str), Some("gsk-new"));
        // …and Rust's, to settings the page didn't touch.
        assert_eq!(saved.mic_order.first().map(|m| m.name.as_str()), Some("Desk mic"));
        assert_eq!(saved.input_device.as_deref(), Some("Desk mic"));
        assert_eq!(saved.pp_api_keys.get("openai").map(String::as_str), Some("sk-example"));
        assert!(!saved.learn_words);
        // A setting changed on both sides keeps the page's (the edit being made).
        let base = saved.clone();
        let mine = YapConfig { learn_words: true, ..base.clone() };
        update_at(&path, |c| c.learn_words = false).unwrap();
        update_at(&path, |c| *c = merge_saved(&base, &mine, c)).unwrap();
        assert!(load_at(&path).cfg.learn_words);
        // Nothing changed on the page: config.json as it is.
        let now = YapConfig { hotkey: "kb:122".into(), ..base.clone() };
        assert_eq!(merge_saved(&base, &base, &now).hotkey, "kb:122");
        // A word heard several ways (`also_heard`, in memory only) is never lost.
        let heard = DictionaryEntry { also_heard: vec!["tori".into()], ..entry("tory", "Tauri") };
        let mine = YapConfig { dictionary: vec![heard], ..base.clone() };
        let rows: Vec<String> = merge_saved(&base, &mine, &base).dictionary.into_iter().map(|e| e.from).collect();
        assert_eq!(rows, ["tory", "tori"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A save that fails says so as every store's does (`shell/saving.rs`):
    /// the page knows it by "Couldn't save " and shows Try again.
    #[test]
    fn a_failed_save_says_what_and_why_and_writes_nothing() {
        let dir = scratch("failed-save");
        let path = dir.join("config.json");
        save_at(&path, &YapConfig::default()).unwrap();
        let before = std::fs::read(&path).unwrap();
        crate::shell::saving::stage_failures("config", 1);
        let cfg = YapConfig { hotkey: "kb:121".into(), ..YapConfig::default() };
        assert_eq!(save_at(&path, &cfg).unwrap_err(), "Couldn't save your settings: the disk is full.");
        assert_eq!(std::fs::read(&path).unwrap(), before);
        // The next save writes it.
        save_at(&path, &cfg).unwrap();
        assert_eq!(load_at(&path).cfg.hotkey, "kb:121");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The places that change a setting by themselves — the Chat picker's
    /// model and key, the voice's volume and "Speak replies", Read aloud's
    /// speed, the on-device model's context, call detection's "Stop asking"
    /// and "Don't ask for…" — change it in one step (`update`): a load →
    /// change → save of their own could put back what another thread saved
    /// in between (code review 2026-10-23 #2).
    #[test]
    fn settings_changed_outside_settings_change_in_one_step() {
        let files = [
            ("ai/ai_model.rs", include_str!("ai/ai_model.rs")),
            ("voice/tts.rs", include_str!("voice/tts.rs")),
            ("voice/tts/playback.rs", include_str!("voice/tts/playback.rs")),
            ("voice/tts/speaker.rs", include_str!("voice/tts/speaker.rs")),
            ("voice/tts/text.rs", include_str!("voice/tts/text.rs")),
            ("voice/read_aloud.rs", include_str!("voice/read_aloud.rs")),
            ("ai/local_llm.rs", include_str!("ai/local_llm.rs")),
            ("ai/local_llm/context.rs", include_str!("ai/local_llm/context.rs")),
            ("ai/local_llm/models.rs", include_str!("ai/local_llm/models.rs")),
            ("ai/local_llm/runtime.rs", include_str!("ai/local_llm/runtime.rs")),
            ("calls/meeting_detect.rs", include_str!("calls/meeting_detect.rs")),
        ];
        for (file, src) in files {
            let code: Vec<&str> = src.lines().filter(|l| !l.trim_start().starts_with("//")).collect();
            for call in ["config::save(", "settings::save_config("] {
                let found: Vec<&&str> = code.iter().filter(|l| l.contains(call)).collect();
                assert!(found.is_empty(), "{file} loads, changes and saves the config ({found:?}): use config::update or settings::change_config");
            }
        }
    }

    /// A crash mid-write leaves its temp file behind: the start-up sweep
    /// takes Yap's own (`<file>.<pid>-<n>.tmp`, and the `<store>.tmp` of
    /// before 2026-10-23) once they're stale, and nothing else.
    #[test]
    fn the_start_up_sweep_takes_only_yaps_own_stale_temp_files() {
        let dir = scratch("sweep");
        let an_hour_ago = std::time::SystemTime::now() - Duration::from_secs(3600);
        let make = |name: &str, stale: bool| {
            let p = dir.join(name);
            std::fs::write(&p, "{").unwrap();
            if stale {
                std::fs::File::options().write(true).open(&p).unwrap().set_modified(an_hour_ago).unwrap();
            }
        };
        // A crash's leftovers.
        for name in ["notes.json.4321-7.tmp", "config.json.12-0.tmp", "notes.tmp", "config.tmp"] {
            make(name, true);
        }
        // Yap's, but being written now (another Yap shares the folder).
        make("history.json.999-3.tmp", false);
        // Not Yap's names, or not files.
        let theirs = [
            "notes.json",
            "config.corrupt-1700000000",
            "report.tmp",
            "notes.json.tmp",
            "photo.1-2.png",
            "notes.json.abc-1.tmp",
            "x.12-.tmp",
            "a.b-c.tmp",
            ".tmp",
        ];
        for name in theirs {
            make(name, true);
        }
        std::fs::create_dir(dir.join("cache.1-2.tmp")).unwrap();
        assert_eq!(sweep_temps_in(&dir, Duration::from_secs(600)), 4);
        let mut left: Vec<String> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        left.sort();
        let mut want: Vec<String> = theirs.iter().map(|s| s.to_string()).chain(["history.json.999-3.tmp".into(), "cache.1-2.tmp".into()]).collect();
        want.sort();
        assert_eq!(left, want);
        // A folder that isn't there: nothing to do.
        assert_eq!(sweep_temps_in(&dir.join("missing"), Duration::from_secs(600)), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Another program has config.json open (an antivirus, a backup tool)
    /// when Yap reads it: Yap runs on the defaults, and never saves them
    /// over the real file once it's free again.
    #[cfg(windows)]
    #[test]
    fn a_config_yap_couldnt_open_is_never_saved_over() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = scratch("held");
        let path = dir.join("config.json");
        let mine = YapConfig { hotkey: "kb:121".into(), ..YapConfig::default() };
        save_at(&path, &mine).unwrap();
        let held = std::fs::OpenOptions::new().read(true).share_mode(0).open(&path).unwrap();
        let loaded = load_at(&path);
        assert_eq!(loaded.cfg.hotkey, YapConfig::default().hotkey, "the defaults for now");
        assert!(loaded.reset.is_none(), "nothing moved aside");
        drop(held);
        // Free again, but this copy is the defaults: refused.
        assert!(save_at(&path, &loaded.cfg).is_err());
        assert_eq!(load_at(&path).cfg.hotkey, "kb:121", "the real one is still there");
        // Read since: saves go through.
        save_at(&path, &YapConfig { hotkey: "kb:122".into(), ..mine }).unwrap();
        assert_eq!(load_at(&path).cfg.hotkey, "kb:122");
        let _ = std::fs::remove_dir_all(&dir);
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
        let sign_off = vec![snippet("my sign-off", "Speak soon,\nImogen")];
        assert_eq!(
            apply_snippets("I'll see you soon. My sign off.", &sign_off),
            "I'll see you soon. Speak soon,\nImogen"
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
    fn a_retired_setting_in_an_old_file_is_ignored() {
        // `showTrayIcon` (retired 2026-07-09): the rest of the file still loads.
        let cfg: YapConfig = serde_json::from_str(r#"{"showTrayIcon":false,"startHidden":true}"#).unwrap();
        assert!(cfg.start_hidden);
        assert!(serde_json::to_value(&cfg).unwrap().get("showTrayIcon").is_none());
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
    fn the_bar_matches_the_app_once_and_a_dark_bar_stays_dark() {
        // Saved before the theme: "light" was the default everyone saved.
        let mut cfg: YapConfig = serde_json::from_str(r#"{"configVersion":2,"barTheme":"light"}"#).unwrap();
        assert!(migrate_once(&mut cfg));
        assert_eq!(cfg.bar_theme, "app");
        assert_eq!(cfg.theme, "system");
        // A bar made dark on purpose.
        let mut cfg: YapConfig = serde_json::from_str(r#"{"configVersion":2,"barTheme":"dark"}"#).unwrap();
        assert!(migrate_once(&mut cfg));
        assert_eq!(cfg.bar_theme, "dark");
        // Light chosen again afterwards sticks.
        cfg.bar_theme = "light".into();
        let mut reloaded: YapConfig = serde_json::from_str(&serde_json::to_string(&cfg).unwrap()).unwrap();
        assert!(!migrate_once(&mut reloaded));
        assert_eq!(reloaded.bar_theme, "light");
        // A fresh install and an old file without the fields.
        let fresh = YapConfig::default();
        assert_eq!((fresh.theme.as_str(), fresh.bar_theme.as_str()), ("system", "app"));
        let old: YapConfig = serde_json::from_str("{}").unwrap();
        assert_eq!((old.theme.as_str(), old.bar_theme.as_str()), ("system", "app"));
    }

    #[test]
    fn the_one_microphone_picked_becomes_the_first_in_the_order_once() {
        use crate::dictation::mics::MicChoice;
        // Picked before 25 Oct: its name only.
        let mut cfg: YapConfig = serde_json::from_str(r#"{"configVersion":3,"inputDevice":"Microphone (Fabrikam USB Mic)"}"#).unwrap();
        assert!(migrate_once(&mut cfg));
        assert_eq!(cfg.mic_order, vec![MicChoice { id: String::new(), name: "Microphone (Fabrikam USB Mic)".into() }]);
        // Kept for an older Yap, which reads only that.
        assert_eq!(cfg.input_device.as_deref(), Some("Microphone (Fabrikam USB Mic)"));
        // Windows' default: nothing to keep, the order stays empty.
        let mut cfg: YapConfig = serde_json::from_str(r#"{"configVersion":3,"inputDevice":null}"#).unwrap();
        assert!(migrate_once(&mut cfg));
        assert!(cfg.mic_order.is_empty());
        // The order changed afterwards (and saved): it stays as it is.
        let path = scratch("mic-cleared").join("config.json");
        let mut cfg: YapConfig = serde_json::from_str(r#"{"configVersion":3,"inputDevice":"Old mic"}"#).unwrap();
        assert!(migrate_once(&mut cfg));
        cfg.mic_order.clear();
        save_at(&path, &cfg).unwrap();
        let Read::Parsed(reloaded) = read_at(&path) else { panic!("saved config unreadable") };
        let mut reloaded = *reloaded;
        follow_an_older_builds_mic(&mut reloaded);
        assert!(!migrate_once(&mut reloaded));
        assert!(reloaded.mic_order.is_empty());
    }

    #[test]
    fn the_bars_use_what_i_said_is_off_once_and_then_as_chosen() {
        let mut cfg: YapConfig = serde_json::from_str(r#"{"configVersion":4,"asSaidOffer":true}"#).unwrap();
        assert!(migrate_once(&mut cfg));
        assert!(!cfg.as_said_offer);
        // Switched on again afterwards: it stays on.
        cfg.as_said_offer = true;
        assert!(!migrate_once(&mut cfg));
        assert!(cfg.as_said_offer);
        // A fresh install, and a file without the field: off.
        assert!(!YapConfig::default().as_said_offer);
        assert!(!serde_json::from_str::<YapConfig>("{}").unwrap().as_said_offer);
    }

    #[test]
    fn an_older_yap_keeps_the_microphone_and_its_pick_comes_back() {
        use crate::dictation::mics::MicChoice;
        let mic = |name: &str| MicChoice { id: String::new(), name: name.into() };
        let path = scratch("mic-older-yap").join("config.json");
        let cfg = YapConfig { mic_order: vec![mic("Headset"), mic("Desk mic")], ..YapConfig::default() };
        save_at(&path, &cfg).unwrap();
        // An older Yap reads only `inputDevice`, and saves back without the order.
        let mut old: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(old["inputDevice"], "Headset");
        old.as_object_mut().unwrap().remove("micOrder");
        let mut back: YapConfig = serde_json::from_value(old.clone()).unwrap();
        follow_an_older_builds_mic(&mut back);
        assert_eq!(back.mic_order, vec![mic("Headset")]);
        // A mic picked in the older build goes first; the rest keep their order.
        let mut cfg = YapConfig { mic_order: vec![mic("Headset"), mic("Desk mic")], input_device: Some("Desk mic".into()), ..YapConfig::default() };
        follow_an_older_builds_mic(&mut cfg);
        assert_eq!(cfg.mic_order, vec![mic("Desk mic"), mic("Headset")]);
        let mut cfg = YapConfig { mic_order: vec![mic("Headset")], input_device: Some("Webcam mic".into()), ..YapConfig::default() };
        follow_an_older_builds_mic(&mut cfg);
        assert_eq!(cfg.mic_order, vec![mic("Webcam mic"), mic("Headset")]);
        // Already first: nothing moves.
        let mut cfg = YapConfig { mic_order: vec![mic("Headset"), mic("Desk mic")], input_device: Some("Headset".into()), ..YapConfig::default() };
        follow_an_older_builds_mic(&mut cfg);
        assert_eq!(cfg.mic_order, vec![mic("Headset"), mic("Desk mic")]);
        // A fresh install: Windows' default.
        assert!(YapConfig::default().mic_order.is_empty() && YapConfig::default().mic_not_now.is_empty());
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
