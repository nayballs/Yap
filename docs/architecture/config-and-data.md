# Config and data

`config.rs` (every setting) and the files Yap keeps in `%APPDATA%/yap/` and elsewhere, with the notable defaults.

[Architecture index](./README.md) · [CLAUDE.md](../../CLAUDE.md)

## `config.rs`

- **`config.rs`** — `YapConfig` (hotkey, model_size, use_gpu, input_device, sound +
  volume, output_device, quiet_other_apps ("Silence other apps while you
  dictate", off; reads old configs' `muteWhileRecording`) + pause_media
  ("Pause music and videos while you dictate", off; both `mute.rs`), recording_mode,
  overlay_position, dictionary (`DictionaryEntry`: from/to + `fuzzy` (default
  on) + `snippet` (default off)), append_trailing_space, smart_spacing ("Match
  the text around the cursor", default on), chat_no_full_stop ("Leave the full
  stop off chat messages", default on), auto_submit(+key),
  voice_enter (the "press enter" voice command, default on), terminal_mode
  ("Safer pasting into terminals", default on; both `app_kind.rs`),
  restore_clipboard, show_tray_icon, autostart, model_unload_timeout, selected_language,
  translate_to_english, the `pp*` AI-cleanup fields incl. `pp_preset` (Default/Email/
  Notes/Slack/Code/Custom), the editable `pp_prompt` body, `pp_api_keys` (per-provider
  key store — the UI swaps the active `pp_api_key` from it on provider switch),
  `pp_local_model` (the on-device GGUF; empty = the 1.5B) + `local_llm_upgrade_dismissed`
  (the 4B offered once, `local_llm::upgrade_offer`) + `local_context` (the
  on-device model's context in tokens; 0 = 8,192; `local_llm::effective_context`), `cleanup_profiles` (each
  with an optional per-profile LLM override: provider/base_url/model/api_key — empty
  provider = inherit global) + `app_routes` smart routing, streaming_partials,
  history_enabled, update_checks_enabled, dictionary_fuzzy, learn_words
  ("Suggest words from your corrections", default on, `learn.rs`),
  show_tips ("Show tips", default on: coaching hints and nudges;
  `notice.rs`), the meeting
  notepad: meeting_open_notepad (default on) + meeting_split_screen (default
  off) + meeting_live_transcript (default on) + meeting_consent_message (the
  notepad's "Copy consent message" text; empty = the default), speaker
  diarization: meeting_diarization ("Tell speakers apart in meetings", beta,
  off) + diarization_model (a `diarize/registry.rs` id, default
  `nemotron-3-diarization`), speaker names: meeting_speaker_names ("Suggest
  who's speaking", on: the name cards, Yap's strong guesses, the AI step at a
  meeting's end; `speakers/naming.rs`) + meeting_call_names (read who's
  talking from the call app's window, experimental, off, no setting;
  `speakers/call_ui.rs`, whose rules a `<data>/call_speakers.json` can
  override per app), call detection:
  meeting_detection + meeting_detect_style ("popup"|"quiet") +
  meeting_detect_apps (app id → bool overrides of `meeting_detect::APPS`'
  defaults) + meeting_auto_start (the opt-in 10-second countdown, off),
  meeting guard rails: meeting_hide_from_capture (true),
  meeting_max_minutes (120; 0 = no limit), meeting_call_end ("ask"|"stop"),
  meeting_end_when_quiet ("End notes when the meeting goes quiet", true),
  meeting_hotkey (`kb:alt+win+77`), meeting_reminder (the calendar's card:
  "15s" | "1min" | "2min" | "never"), meeting_brief ("Prepare a brief before
  meetings", true; `calendar/brief.rs`), bridge_enabled, mcp_allow_writes (AI apps
  may save notes over MCP; default off), the Yap bar: bar_enabled (true) +
  bar_hide_fullscreen (true) + bar_theme ("light" | "dark", light) +
  bar_dictation_buttons (the capsule's ✕ / ✓: "off" | "hover" | "always",
  off), General → Appearance (`src/lib/appearance.js`, every window):
  reading_font ("default" Figtree | "sans" Geist | "serif" Literata |
  "system" Segoe UI | "dyslexic" OpenDyslexic, default) + reading_size
  ("small" | "default" | "large" | "larger", default) + motion ("system" |
  "reduced", system), spoken replies (`tts.rs`): speak_replies (off), speak_when ("voice" |
  "always", voice), talk_back_hands_free (true; only while speak_replies is
  on), tts_engine ("kokoro" — Windows voices until it's downloaded — or
  "windows"), tts_voice (Kokoro's, "af_bella"), tts_windows_voice ("" =
  Windows' default), tts_rate (1.0, 0.5–2.0), tts_volume (0.5: Yap's own
  gain on the voice, squared — `tts::gain`, half = −12 dB, 10 % = −40 dB —
  under the Windows mixer, live mid-sentence via the rodio sink's volume;
  `tts_volume_set {volume, save?}` → `yap-voice-volume`), tts_provider
  ("local" — the engines above — or "elevenlabs"; `tts/cloud.rs`) +
  tts_cloud_keys (cloud voice service → API key, like web_search_keys:
  never logged, sent only to that service in its header) + tts_cloud
  (service → `CloudVoiceChoice {voice, voiceName, model}`, empty = George
  and `eleven_flash_v2_5`); config v2 moved a saved
  Windows voice from tts_voice to tts_windows_voice and "windows" (never a
  choice before) to "kokoro"), the Voice Assistant's assistant_hotkey (""
  = unbound, the default), opening apps (`apps.rs`): open_apps (true) +
  apps_blocked (app ids, "Don't open this one") + app_nicknames
  (`AppNickname {say, app, name}`), the web (`web.rs`): web_search (false) +
  web_search_native (true: cloud models use their provider's own search) +
  web_search_service ("exa" | "tavily") + web_search_keys (service → key),
  About you (`about_you.rs`, Settings → General): about_name + about_me (≤
  300 characters; told to the AI model with each question, never to Yap) +
  region_country (ISO; "" = Automatic, Windows' home location: the
  currency, units, language and spelling follow it, the time zone is the
  PC's — the language/currency/units/time-zone fields of before 22 Oct
  2026 still load and go on the next save) + region_spelling ("Write in
  my region's spelling", Dictation cleanup, on),
  Memory's switches (`memory.rs`, Settings → Memory; what Yap remembers is
  in memory.json, never here): memory_search_chats ("Search and reference
  past chats", on) + memory_learn_chats ("Learn from my chats", on) +
  memory_learn_meetings ("Learn from meetings", off) + memory_sensitive
  ("Include sensitive topics in memory", off),
  Commands' edit_hotkey (the rewrite shortcut)
  + agent_name + the `voiceAgent` scope (field and key names unchanged by
  the 2026-10-18 rename, so no setting resets). JSON
  load/save + `apply_dictionary` (corrections; skips snippets) +
  `apply_snippets` + `term_regex` (the whole-word matcher both use) +
  `dictionary_prompt` (the Whisper `initial_prompt` vocabulary, in the order
  given — callers pass `dict_usage::ranked(..)`; no snippets; ~600-char cap) +
  `resolve_cleanup` (per-app plan: body + endpoint).
  `data_dir()` is portable-aware. `load()` also migrates saved Groq picks (cleanup,
  LLM scopes, profiles) off models Groq retired in 2026 (`RETIRED_GROQ_MODELS` —
  default is now `openai/gpt-oss-20b`); extend that table when Groq retires more
  (`is_retired_groq_model` keeps them out of Groq's live model list too).
  Those fixes are idempotent; changes that must run only ONCE go in
  `migrate_once`, keyed by `config_version` (`CONFIG_VERSION`; a missing field =
  0, a pre-versioning file). `load()` saves straight after one runs, and `save()`
  stamps the current version, so a later user choice is never undone. v1
  (2026-10-05) turned live partials back on: `Default` said off until then, so
  every install since July had saved `false`. A test keeps `Default` and the
  serde defaults identical (bar `config_version`).

## Config & data

- Config: `%APPDATA%/yap/config.json` (auto-created; old files load — every field is
  `#[serde(default)]`). Portable mode → `<exe>/Data/`.
- Models: `%APPDATA%/yap/models/` — Whisper `.bin` files and extracted ONNX dirs,
  downloaded from `https://blob.handy.computer/` (SHA-256 verified).
- Speaker models: `%APPDATA%/yap/models/diarization/` — `nemotron-3-diarization/`
  (the GGUF from Hugging Face `nvidia/`, pinned commit) and
  `nemo-speech-0.2.0-cpu/` (NVIDIA's runtime DLLs + `licenses/`), each with a
  `NOTICE.txt` naming its source and licence (`diarize/registry.rs`; size +
  SHA-256 verified). Yap has no central third-party notices page: the
  attribution is in Settings → Speech-to-Text → Speakers in meetings and in
  those files.
- Kokoro (spoken replies' natural voice): `%APPDATA%/yap/models/tts/` —
  `kokoro-v1.0/` (model.onnx + `voices/` from Hugging Face
  onnx-community/Kokoro-82M-v1.0-ONNX @ 1939ad2a, Apache-2.0; `lexicon/`:
  misaki's US/GB dictionaries + LICENSE from hexgrad/misaki @ fba12365,
  Apache-2.0) and `espeak-ng-1.52.0/` (eSpeak NG's program, DLL and the data
  of the languages Kokoro reads, from its 1.52.0 MSI; **GPL-3.0**, run as a
  separate program, never linked; `COPYING` + the source link in
  NOTICE.txt), each with a `NOTICE.txt` (`tts/registry.rs`; size + SHA-256
  verified). Attribution: Settings → Voice Assistant → Voice. Deleted from the
  Kokoro row; ~352 MB.
- Groq usage: `%APPDATA%/yap/groq_usage.json`.
- History: `%APPDATA%/yap/history.json` (local-only; cleared from Settings → Privacy & data → History).
- Notes: `%APPDATA%/yap/notes.json` — the AI Notepad store (folders, actions,
  participants, meeting transcripts).
- Chats: `%APPDATA%/yap/chats.json` — AI Chat conversations (`chats.rs`).
- Model lists: `%APPDATA%/yap/model_catalog.json` — each keyed AI provider's
  chat models as it last listed them (id, name, created, context, tools,
  first seen), when, and why the last check failed (`model_catalog.rs`).
  Never a key. Safe to delete (the curated lists show until the next check).
- Updates: `%APPDATA%/yap/updates.json` — which update was announced (and when),
  the last check time, and the restart marker (`updates.rs`).
- Calendar: `%APPDATA%/yap/calendar.json` — connected calendars (name, kind,
  last sync, error), meetings from 12 h ago to 7 days ahead, answered reminders,
  the nudge, and meetings' pre-meeting briefs (`briefs`, by event key, until
  2 days after the meeting, ≤ 40: their lines, sources — note and memory ids,
  titles, dates — and a fingerprint) (`calendar.rs`). Never a token or a link: those are Credential
  Manager generic credentials `yap-calendar-<1-8>.com.yap.dictation` (Local
  persistence; portable adds a data-dir hash), deleted on Disconnect and, for
  an installed copy, by the uninstaller's "Delete the application data".
- Dictionary usage: `%APPDATA%/yap/dictionary_usage.json` — each dictionary
  word's use count and last use, which orders the Whisper vocabulary prompt
  (`dict_usage.rs`). Safe to delete (the order falls back to the list's).
- Hints: `%APPDATA%/yap/hints.json` — how often each push-to-talk coaching hint
  showed, when the last did, and whether double-tap has been used (`coaching.rs`).
- Quieted apps: `%APPDATA%/yap/quieted_apps.json` — only while a dictation
  has other apps silenced (each session's id, exe and level before), so a
  Yap that crashed mid-dictation brings them back at its next start
  (`mute.rs`); gone again once they're back.
- Word suggestions: `%APPDATA%/yap/word_suggestions.json` — when Yap last
  offered a word for the dictionary, per "heard→term" pair your "Not now"
  count and "Don't suggest this", and the words you removed from the
  dictionary (`removed`, never suggested again) (`learn.rs`). Never the
  field text it read.
- Memory: `%APPDATA%/yap/memory.json` — what Yap remembers about the person
  (Profile, Preferences, People, Projects, Topics: each a title, a summary,
  facts, where it came from, when), how far each chat has been read for it,
  the chats waiting to be read (across a quit) and the meetings read
  (`memory.rs`). Changed only as changes (Undo in the app); cleared from
  Settings → Memory, exported there as a .zip of Markdown. Sent with questions
  to the AI model the person uses, never to Yap.
- All of the above (plus config) write atomically and quarantine a corrupt file
  on load rather than crashing (`config::atomic_write`/`quarantine_corrupt`).
- App icons: `%APPDATA%/yap/icons/` — `<app>.png` per installed call app and
  `sources.json` (what each was made from); a cache, rebuilt whenever it's
  missing, stale or unreadable (`app_icons.rs`).
- Local API bridge discovery: `~/.yap/cli-bridge.json` (fixed path, NOT the
  data dir; written while the app runs, deleted on exit — see `bridge.rs` +
  `docs/local-api.md`).
- AI apps' own config files (Settings → AI apps (MCP) → Add to …): Yap only adds or
  removes its `yap` entry, keeps the original as `<file>.bak` the first time,
  and never edits a file that isn't plain JSON (`mcp_clients.rs`).
- Account session: a Windows Credential Manager generic credential
  (`yap-account.com.yap.dictation`, Local persistence), not a file — see `auth.rs`.
  Uninstalling with "Delete the application data" removes it too (updates never do).
- Notable defaults: hotkey `kb:120` (F9, rebindable), **default model
  `parakeet-tdt-0.6b-v3`** (fast/accurate, ONNX→DirectML), `use_gpu = true`,
  recording mode `toggle`, dictations **fitted to the text around the cursor**
  (`smart_spacing`) and kept out of clipboard history (always), **one line and
  no Enter of Yap's own in a terminal** (`terminal_mode`; Claude Code/Codex in
  Windows Terminal keep line breaks), **no full stop on a short chat message**
  (`chat_no_full_stop`), the **Yap bar on** (`bar_enabled`), hidden over
  fullscreen apps (`bar_hide_fullscreen`), the dictation overlay always shown
  while recording/transcribing (no off switch — it's the hot-mic indicator),
  without ✕ / ✓ (`bar_dictation_buttons` "off": Cancel is in the tray),
  live transcription preview **on**
  (`streaming_partials`), AI cleanup **off** (turned on locally, the model
  recommended for this PC: Qwen3.5 4B on a graphics card with ≥ 5.5 GiB of its
  own, else Qwen2.5 1.5B), the "press enter" voice command
  **on** (`voice_enter`), word suggestions from your corrections **on**
  (`learn_words` — it only asks; nothing is added without a click), the
  region the AI models are told **following Windows** (About you's Region
  "Automatic", `region_country` empty) with cleanup keeping its spelling (`region_spelling`), looking
  things up on the web **off** (`web_search`; once on, cloud models use
  their provider's own search — `web_search_native` — and local ones Exa),
  opening apps when asked **on** (`open_apps` — only apps in the Start
  menu, `apps.rs`), tips
  **on** (`show_tips`; one notice at a time either way, `notice.rs`), call
  detection **on**
  (`meeting_detection` — it only asks; nothing records without a click) as
  **pop-ups** (`meeting_detect_style`), for work apps only (Discord, WhatsApp,
  Signal and Telegram start switched off; `meeting_detect_apps` holds changes),
  no auto-start countdown (`meeting_auto_start` off);
  meeting guard rails: meeting windows **hidden from screen capture** while
  recording (`meeting_hide_from_capture`), a **2-hour** maximum recording length
  (`meeting_max_minutes`), **ask** when a recorded call ends
  (`meeting_call_end`), a recording that's gone **quiet for 10 minutes**
  ending after a 30 s countdown (`meeting_end_when_quiet`), meeting shortcut **Win+Alt+M** (`meeting_hotkey`); the
  meeting notepad opening when a meeting starts (`meeting_open_notepad`) with
  its live transcript (`meeting_live_transcript`), but **not** splitting the
  screen (`meeting_split_screen` off: it moves another app's window); with a
  calendar connected, the reminder card **right before** a meeting (15 s,
  `meeting_reminder`) and a **brief** of what to know going in
  (`meeting_brief`, on); spoken replies **off** (`speak_replies`; once on, only
  for questions asked by voice, `speak_when`, and a dictated question is sent
  by itself, `talk_back_hands_free`) in **Kokoro's af_bella** at 1× once
  Kokoro is downloaded (`tts_engine` "kokoro"), Windows' default voice
  until then.
