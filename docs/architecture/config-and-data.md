# Config and data

`config.rs` (every setting) and the files Yap keeps in `%APPDATA%/yap/` and elsewhere, with the notable defaults.

[Architecture index](./README.md) · [CLAUDE.md](../../CLAUDE.md)

## `config.rs`

- **`config.rs`** — `YapConfig` (hotkey, model_size, use_gpu (since
  2026-10-22 it mirrors stt_device: `save` sets it to stt_device != "cpu",
  `load` turns an old `false` into "cpu"), stt_device (Settings →
  Speech-to-Text → Runs on: "auto" — the graphics card unless the processor
  measured clearly faster on this PC, `dictation/stt_speed.rs` — | "gpu" | "cpu"), keep_recent_audio (a day of dictation
  audio to run one again, on; `dictation/recent_audio.rs`), as_said_hotkey ("Use what I
  said", "" = unbound) + as_said_offer (the bar's offer after AI cleanup,
  on; both `pasting/as_said.rs`), paste_last_hotkey + copy_last_hotkey
  (`pasteLastHotkey` / `copyLastHotkey`, Paste / Copy last transcript, "" =
  unbound, the default; never a clash — `input_hook::configure_paste_last_from`
  / `configure_copy_last_from` leave one another shortcut has unbound;
  `pasting/last.rs`), snip_hotkey (Snip to Chat, default Win+Alt+S "kb:alt+win+83", "" = unbound; never a clash, `input_hook::configure_snip_from`) + snip_voice ("Say “snip” to take a screenshot", on) + chat_image_days ("Keep chat screenshots": 7 / 30 (default) / 90 / 0 = forever; another value counts as 30; changing it sweeps at once; all `snip.rs`, docs/architecture/snip.md) + snip_save_dir (the folder a snip's or a chat screenshot's Save opened in last, "" = Pictures; `snip/copy_save.rs`), rewrite_last_dictation (`rewriteLastDictation`,
  serde default on: the rewrite shortcut with nothing selected rewrites the
  last dictation — within a minute, in the same field, still ending at the
  cursor —; off, it writes new text as before 2026-10-23; Settings →
  Commands; `as_said::rewritable`; saved transforms follow it too),
  transforms (`transforms`, **saved transforms**, Settings → Commands →
  Transforms, `dictation/transforms.rs`: a list of `{id, builtin, name, instruction,
  hotkey, language}` in the person's order — `builtin` one of "polish" |
  "shorter" | "grammar" | "translate" | "prompt" (its name and instruction
  are Yap's, the stored ones ignored), else "" with the person's name and
  instruction; `hotkey` an input-hook spec or ""; `language` Translate's,
  "Spanish" until picked. Serde default = Yap's five, no shortcuts; at most
  12 run (`transforms::list`: usable ones, each id once). A key that would
  fire with one of the seven fixed shortcuts or an earlier transform's is
  left unbound, `transforms::assign`), mic_order (Settings → Dictation →
  Microphone, 2026-10-25, `dictation/mics.rs`: `[{id, name}]` — the Core
  Audio endpoint id, else the name — in the person's order; the first one
  connected is used, then Windows' default; empty, the default, is Windows'
  default alone) + mic_not_now (the microphones answered "Not now" to
  "… connected · Use it for dictation?": never asked again), input_device
  (the one mic picked before 25 Oct; **`migrate_once` v4** moves it to the
  front of `mic_order` by name and clears it — nothing else reads it; an
  unplugged pick now falls back to Windows' default instead of "No
  microphone found"), sound +
  volume, output_device, quiet_other_apps ("Silence other apps while you
  dictate", off; reads old configs' `muteWhileRecording`) + pause_media
  ("Pause music and videos while you dictate", off; both `dictation/mute.rs`),
  keep_mic_ready ("Keep the microphone ready", off: the mic opens when you
  dictate, stays warm a minute and closes, at once on Bluetooth; on, it's
  open the whole time Yap runs, as before 2026-10-22; `dictation/mic.rs`), recording_mode,
  overlay_position, dictionary (`DictionaryEntry`: from/to + `fuzzy` (default
  on) + `snippet` (default off) + `starred` (default off, written only when
  set; 2026-10-25)), append_trailing_space, smart_spacing ("Match
  the text around the cursor", default on), screen_names ("Spell names the
  way they're written on screen", default on; `dictionary/screen_terms.rs`),
  chat_no_full_stop ("Leave the full
  stop off chat messages", default on), auto_submit(+key),
  voice_enter (the "press enter" voice command, default on), terminal_mode
  ("Safer pasting into terminals", default on; both `pasting/app_kind.rs`),
  restore_clipboard, autostart, model_unload_timeout, selected_language,
  translate_to_english, the `pp*` AI-cleanup fields incl. `pp_preset` (Default/Email/
  Notes/Slack/Code/Custom), the editable `pp_prompt` body, `pp_api_keys` (per-provider
  key store — the UI swaps the active `pp_api_key` from it on provider switch),
  `pp_local_model` (the on-device GGUF; empty = the 1.5B) + `local_llm_upgrade_dismissed`
  (the 4B offered once, `local_llm::models::upgrade_offer`) + `local_context` (the
  on-device model's context in tokens; 0 = 8,192; `local_llm::context::effective_context`), `cleanup_profiles` (each
  with an optional per-profile LLM override: provider/base_url/model/api_key — empty
  provider = inherit global; since 2026-10-24 also **styles by app**,
  `ai/cleanup_styles.rs`: a profile with a `category` — "email", "workChat",
  "personalChat", "documents", "aiPrompts", "terminal", "other", ids
  `style-<category>`, one each, added at load — its `tone` ("formal",
  "casual", "veryCasual", "natural", "literal", "command", "off"; defaults
  formal / casual / casual / formal / literal / command / natural), `apps`
  (added: a process name or a site's name) and `removed_apps` (built-in apps
  taken out, by name); a style's `prompt` is *added* to `pp_prompt`) +
  `app_routes` smart routing (a rule may name a style) + `cleanup_styles`
  ("Match the style to the app": missing = decided once at load, on unless
  `pp_preset` isn't "default" — a body the person picked for everywhere
  stays as it was; no `migrate_once`) + `cleanup_offer_seen` (the one tip
  offering cleanup was shown; `cleanup_styles::offer`), streaming_partials,
  history_enabled, update_checks_enabled, dictionary_fuzzy, learn_words
  ("Suggest words from your corrections", default on, `dictionary/learn.rs`),
  show_tips ("Show tips", default on: coaching hints and nudges;
  `notices/notice.rs`), the meeting
  notepad: meeting_open_notepad (default on) + meeting_split_screen (default
  off) + meeting_live_transcript (default on) + meeting_consent_message (the
  notepad's "Copy consent message" text; empty = the default), meeting
  audio (`meetings/meeting_audio.rs`, Settings → Meetings): meeting_keep_audio ("Keep
  meeting audio", on: to transcribe a meeting again and play its lines; off,
  it's still written while a meeting records and deleted once it's
  transcribed) + meeting_audio_days (14; 1 / 7 / 14 / 30 / 90) +
  meeting_refine ("Improve the transcript after the meeting",
  `meetings/meeting_refine.rs`: "auto" — when chunks failed or a more accurate
  speech model is installed for the meeting's language —, "always",
  "never"; default "auto"), speaker
  diarization: meeting_diarization ("Tell speakers apart in meetings", beta,
  off) + diarization_model (a `speakers/diarize/registry.rs` id, default
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
  bar_hide_fullscreen (true) + bar_theme ("app" — Match app, the theme of
  Yap's windows — | "light" | "dark", app) +
  bar_dictation_buttons (the capsule's ✕ / ✓: "off" | "hover" | "always",
  off), General → Appearance (`src/lib/core/appearance.js`, every window):
  theme ("system" — Windows' app mode, followed live — | "light" | "dark",
  system; `shell/theme.rs` gives the opaque windows' webviews its background) +
  reading_font ("default" Figtree | "sans" Geist | "serif" Literata |
  "system" Segoe UI | "dyslexic" OpenDyslexic, default) + reading_size
  ("small" | "default" | "large" | "larger", default) + motion ("system" |
  "reduced", system), spoken replies (`voice/tts.rs`): speak_replies (off), speak_when ("voice" |
  "always", voice), talk_back_hands_free (true; only while speak_replies is
  on), tts_engine ("kokoro" — Windows voices until it's downloaded — or
  "windows"), tts_voice (Kokoro's, "af_bella"), tts_windows_voice ("" =
  Windows' default), tts_rate (1.0, 0.5–2.0), tts_volume (0.5: Yap's own
  gain on the voice, squared — `tts::gain`, half = −12 dB, 10 % = −40 dB —
  under the Windows mixer, live mid-sentence via the rodio sink's volume;
  `tts_volume_set {volume, save?}` → `yap-voice-volume`), tts_provider
  ("local" — the engines above — or "elevenlabs"; `voice/tts/cloud.rs`) +
  tts_cloud_keys (cloud voice service → API key, like web_search_keys:
  never logged, sent only to that service in its header) + tts_cloud
  (service → `CloudVoiceChoice {voice, voiceName, model}`, empty = George
  and `eleven_flash_v2_5`); config v2 moved a saved
  Windows voice from tts_voice to tts_windows_voice and "windows" (never a
  choice before) to "kokoro"), the Voice Assistant's assistant_hotkey (""
  = unbound, the default) + assistant_answer_card (true) +
  assistant_stop_on_pause (true: "Stop listening when I pause",
  `dictation/pause.rs` — a pause after the question ends it, never before you've
  spoken, never under a held push-to-talk key), Read aloud's read_hotkey ("" = unbound, the
  default; `voice/read_aloud.rs`: the selection in any app read aloud, its speed
  the card's − / +, saved as tts_rate), opening apps (`apps.rs`): open_apps (true) +
  apps_blocked (app ids, "Don't open this one") + app_nicknames
  (`AppNickname {say, app, name}`), the web (`web.rs`): web_search (false) +
  web_search_native (true: cloud models use their provider's own search) +
  web_search_service ("exa" | "tavily") + web_search_keys (service → key),
  About you (`ai/about_you.rs`, Settings → General): about_name + about_me (≤
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
  search_by_meaning ("Search my meetings by meaning", off; `search/`,
  Settings → Privacy & data: switched on by the model's download, off by its
  Delete; the index and model live in the data folder, below),
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
  stamps the current version, so a later user choice is never undone.
  **config.json is never lost** (2026-10-23; before, any file that didn't
  parse — and any read error — became the defaults, and the next save wrote
  them over the real file: API keys, dictionary, hotkeys gone). `load_at`:
  no file → the defaults; a file another program holds open → tried for
  ~200 ms, then the defaults *and* `save` refuses to write over it
  (`UNREAD`) until a read works again; a file this build can't read (torn,
  not JSON, a value of another type — a newer build's, after a downgrade) →
  `repair_at`, under the save lock: the file is **set aside as it was**
  (`config.corrupt-<unix secs>`, `-2`… if taken; copied when it can't be
  moved; never saved over), the settings that still read on their own are
  **kept** (`salvage`: only the unreadable ones reset) and written back,
  and the person is told **once** (`report_reset` → `init`, 4 s after start
  as the first load runs before logging and the windows: an `notices/alert.rs`
  "configReset" — "Your settings were reset" / "Some of your settings were
  reset", where the old file is, **Show file** → Explorer; the log says the
  copy's file name and counts, never the contents). A UTF-8 byte-order mark
  (Notepad) is read past. **Saves, one at a time**: `save` (and `load`'s
  migration and repair writes) hold one save lock (`SAVES`, re-entrant on
  its thread); `update(|c| …)` loads, changes and saves under it in one
  step (saving only if anything changed), so a change another thread saved
  meanwhile isn't lost — the tray's model switch, the bar's "off",
  `download_model_size`/`set_active_model`, `set_autostart`, learned words,
  the notepad's consent message and call detection's "Stop asking" /
  "Don't ask for …" (`calls/meeting_detect.rs`, outside its state lock) use it.
  `try_update(|c| … -> Result)` is the same for a change that can be
  refused (nothing saved, not half of it), and `settings::change_config`
  runs it and then applies the result as a Settings save does
  (`apply_config`: hotkeys, the pipeline, the bar…): the Chat picker's model
  and key (`ai/ai_model.rs`), the voice's volume and Speak replies (`voice/tts.rs`),
  Read aloud's speed (`voice/read_aloud.rs`) and the on-device context
  (`ai/local_llm.rs`), which until 2026-10-24 loaded, changed and saved on
  their own (a lint-style test keeps them so).
  Every save then emits **`yap-config-saved`** to every window (`init`
  registers the app; Settings' copy adopts what changed,
  [frontend-settings.md](./frontend-settings.md); the theme follows). A unit
  test never sets aside or writes the real config.json (`load` reads it at
  most, `save` is a no-op); the mechanics are tested on scratch files
  (`load_at` / `save_at` / `update_at`: torn, salvaged, held open, 8 threads
  saving at once, 8 threads updating at once). v1
  (2026-10-05) turned live partials back on: `Default` said off until then, so
  every install since July had saved `false`. v3 (2026-10-22, the dark
  theme) turned a saved bar_theme "light" — the old default, saved by every
  install — into "app" (Match app), once; a bar made dark stays dark. A test
  keeps `Default` and the serde defaults identical (bar `config_version`).

## Config & data

- Config: `%APPDATA%/yap/config.json` (auto-created; old files load — every field is
  `#[serde(default)]`). Portable mode → `<exe>/Data/`.
- Models: `%APPDATA%/yap/models/` — Whisper `.bin` files and extracted ONNX dirs,
  downloaded from `https://blob.handy.computer/` (SHA-256 verified).
- Speaker models: `%APPDATA%/yap/models/diarization/` — `nemotron-3-diarization/`
  (the GGUF from Hugging Face `nvidia/`, pinned commit) and
  `nemo-speech-0.2.0-cpu/` (NVIDIA's runtime DLLs + `licenses/`), each with a
  `NOTICE.txt` naming its source and licence (`speakers/diarize/registry.rs`; size +
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
  NOTICE.txt), each with a `NOTICE.txt` (`voice/tts/registry.rs`; size + SHA-256
  verified). Attribution: Settings → Voice Assistant → Voice. Deleted from the
  Kokoro row; ~352 MB.
- Meeting search (`search/`, [search.md](./search.md)): the model in
  `%APPDATA%/yap/models/search/` — `embeddinggemma-300M-Q8_0.gguf` (Hugging
  Face ggml-org/embeddinggemma-300M-GGUF at a pinned commit, Gemma Terms of
  Use; size + SHA-256 verified; ~318 MiB) with a `NOTICE.txt`, and the
  embedding server's `embedder.log`; it runs on the on-device model's
  engine (`llm/llamafile.exe`, downloaded with it when it isn't there). The
  index in `%APPDATA%/yap/search-index/vectors.bin` — each passage's vector
  (int8 + a scale, by a hash of its words; **no text**), appended as notes
  change, compacted when a note is deleted; `vectors.bin.old` when one for
  another model was set aside. Settings → Privacy & data → "Search my
  meetings by meaning": Rebuild deletes the index and reads everything
  again; Delete… removes the model and the index (the engine stays).
- Groq usage: `%APPDATA%/yap/groq_usage.json`.
- Speech engine timings: `%APPDATA%/yap/stt_speed.json` — per model id, the
  benchmark of each device on this PC (load, the first run after loading,
  the warm ms at three dictation lengths, its time for your dictations,
  whether its words matched), the pick, when, the processor's load before,
  how this person's dictations weigh the lengths (`shortShare`), and what it was measured on
  (the method, the model's files, the graphics adapters and drivers): a
  stale one is measured again (`dictation/stt_speed.rs`). Safe to delete (the graphics card until measured again).
- Recent dictation audio: `%APPDATA%/yap/recent-audio/` — `<id>.wav` (16 kHz
  mono) for each dictation of the last 24 h (≤ 500, ≤ 200 MB), and
  `<id>.pending` while one is being transcribed (`dictation/recent_audio.rs`). Never
  leaves the PC; deleted on schedule, with its history entry, by clearing
  history and by Settings → Privacy & data → Clear recent audio.
- Meeting audio: `%APPDATA%/yap/meeting-audio/<note id>/` — for each
  recording of the meeting, `<recording>-you.flac` and `-them.flac` (16 kHz
  mono FLAC, written as it records: ~50–70 MB an hour for the mic, far less
  for a call side that's often silent) and `<recording>.json` (when it
  started, how far each side is in the note, where dictations began,
  whether it ended — not after a crash, which the next start offers to
  transcribe) (`meetings/meeting_audio.rs`). Never leaves the PC; kept
  `meeting_audio_days` (14) from the recording's start, ≤ 10 GB in all (the
  oldest first), deleted with its note, by Settings → Meetings → Delete all
  meeting audio, or once transcribed with "Keep meeting audio" off — held
  until the refine after the meeting or a Try again is done when one is
  due, a day at most (`release`, `HELD_MS`). Its `.json` also names the
  speech model of the live pass (`model`), for the refine.
- Chat screenshots: `%APPDATA%/yap/chat-images/` — `img_<16 hex>.png` for each
  screenshot a question carries (Snip to Chat, `snip/images.rs`); `chats.json`
  keeps only its id, size, bytes and source. Deleted with its conversation, after
  "Keep chat screenshots" (30 days by default), and a day after a snip that was
  never sent. Never leaves the PC except inside the Chat request you send;
  never logged. Pictures pasted or dropped into Chat are stored here too (as
  PNGs, `snip/attach.rs`); a question whose screenshot went as its text
  (Windows' OCR, `snip/ocr.rs`) keeps it marked `asText`. A snip copied or
  saved from the overlay keeps nothing here; a save goes where the Save
  dialog says, in the folder used last (`snip_save_dir`).
- History: `%APPDATA%/yap/history.json` (local-only; cleared from Settings → Privacy & data → History).
- Notes: `%APPDATA%/yap/notes.json` — the AI Notepad store (folders, actions,
  participants, meeting transcripts).
- Chats: `%APPDATA%/yap/chats.json` — AI Chat conversations (`chat/chats.rs`).
- Model lists: `%APPDATA%/yap/model_catalog.json` — each keyed AI provider's
  chat models as it last listed them (id, name, created, context, tools,
  first seen), when, and why the last check failed (`ai/model_catalog.rs`).
  Never a key. Safe to delete (the curated lists show until the next check).
- Network receipts: `%APPDATA%/yap/connections.json` — every connection Yap
  made in the last 7 days, folded per purpose, feature and host within 10
  minutes (when, the host — never a path, query, header or body —, the
  purpose label, the feature, this PC / your network / internet, the
  count, failures, the last status, bytes sent and received), ≤ 4,000
  entries (`privacy/net.rs`, [privacy-receipts.md](./privacy-receipts.md)). Never
  content or a key. Cleared from Settings → Privacy & data → Network
  connections; safe to delete.
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
  (`dictionary/dict_usage.rs`) and the Dictionary view's "Most used" sort.
  Safe to delete (the order falls back to the list's).
- The Dictionary view's sort: the main window's `localStorage`
  (`yap.dictionarySort`), a convenience of that window, never config.json.
- Hints: `%APPDATA%/yap/hints.json` — how often each push-to-talk coaching hint
  showed, when the last did, and whether double-tap has been used (`dictation/coaching.rs`).
- Quieted apps: `%APPDATA%/yap/quieted_apps.json` — only while a dictation
  has other apps silenced (each session's id, exe and level before), so a
  Yap that crashed mid-dictation brings them back at its next start
  (`dictation/mute.rs`); gone again once they're back.
- Word suggestions: `%APPDATA%/yap/word_suggestions.json` — when Yap last
  offered a word for the dictionary, per "heard→term" pair your "Not now"
  count and "Don't suggest this", and the words you removed from the
  dictionary (`removed`, never suggested again) (`dictionary/learn.rs`). Never the
  field text it read.
- Memory: `%APPDATA%/yap/memory.json` — what Yap remembers about the person
  (Profile, Preferences, People, Projects, Topics: each a title, a summary,
  facts, where it came from, when), how far each chat has been read for it,
  the chats waiting to be read (across a quit) and the meetings read
  (`memory.rs`). Changed only as changes (Undo in the app); cleared from
  Settings → Memory, exported there as a .zip of Markdown. Sent with questions
  to the AI model the person uses, never to Yap.
- All of the above (plus config) write atomically and quarantine a corrupt file
  on load rather than crashing (`config::atomic_write`/`quarantine_corrupt`;
  config.json its own way, above). Each write has a temp file of its own
  (`<name>.<pid>-<n>.tmp`, renamed over the file, tried again for ~100 ms
  while another program holds it): until 2026-10-23 every write of a file
  shared `<name>.tmp`, so two at once could rename a half-written file into
  place. The app icons' cache writes the same way. A crash mid-write leaves
  its temp file behind, which nothing replaces any more, so **start-up
  sweeps them** (`config::sweep_temp_files`, on a thread of its own,
  2026-10-24): in the data folder, `icons/`, `llm/` and each meeting's
  `meeting-audio/` folder, only Yap's own names — `<file>.<pid>-<n>.tmp`,
  and a store's `<stem>.tmp` of before 2026-10-23 (`OLD_TEMP_STEMS`) — and
  only files over 10 minutes old (a dev build beside the installed one
  writes the same folder); anything else is never touched.
- **A write that fails is said, and nothing is lost** (`shell/saving.rs`,
  2026-10-24; until then it was a warning in the log while the page said
  it saved). Every store writes through its `Saver` (`notes::SAVER`,
  `history::SAVER`…): a failed write — the disk full, another program
  holding the file during the rename, a read-only file — leaves the store's
  data in memory, marked unsaved, and its next save writes all of it (each
  store writes its whole state every time); quitting tries once more
  (`saving::flush`). The person is told once, in plain words: "Couldn't save
  your note: the disk is full." (`SaveError`; `Reason` tells a full disk, a
  file another program holds — Windows reports that as access denied on a
  file that isn't read-only —, a read-only file and a folder Yap can't write
  apart). A command that changed the store (a note's edit, a folder, an
  action, a chat's rename or delete, memory's edits, Clear history, a
  history entry's delete) returns it as its error (each store's `saved()`),
  and the page shows it with **Try again** (`saves_retry` → every unsaved
  store written again; `src/lib/notices/saveErrors.js`, frontend.md); a save that
  failed by itself (history after a dictation, memory learning, calendar
  sync, usage and dictionary counts, call-prompt counts, word suggestions)
  is said by one alert through the notice policy (kind `saveFailed`,
  [notices.md](./notices.md)), 1.5 s later unless a command said it, naming
  the first store and whether others failed too, with Try again — and not
  again for that store until a save of it worked. The local API and the
  chat's note tools answer an AI app or model with the failure too, so
  neither says it saved. Caches (`model_catalog.json`, `stt_speed.json`,
  `updates.json`, `hints.json`, `quieted_apps.json`) are only logged. The
  log names the store and the error's kind, never what it holds.
- App icons: `%APPDATA%/yap/icons/` — `<app>.png` per installed call app and
  `sources.json` (what each was made from); a cache, rebuilt whenever it's
  missing, stale or unreadable (`calls/app_icons.rs`).
- Local API bridge discovery: `~/.yap/cli-bridge.json` (fixed path, NOT the
  data dir; written while the app runs, deleted on exit — see `integrations/bridge.rs` +
  `docs/local-api.md`).
- AI apps' own config files (Settings → AI apps (MCP) → Add to …): Yap only adds or
  removes its `yap` entry, keeps the original as `<file>.bak` the first time,
  and never edits a file that isn't plain JSON (`integrations/mcp_clients.rs`).
- Account session: a Windows Credential Manager generic credential
  (`yap-account.com.yap.dictation`, Local persistence), not a file — see `auth.rs`.
  Uninstalling with "Delete the application data" removes it too (updates never do).
- Notable defaults: hotkey `kb:120` (F9, rebindable), **default model
  `parakeet-tdt-0.6b-v3`** (fast/accurate, ONNX→DirectML), `use_gpu = true`,
  recording mode `toggle`, dictations **fitted to the text around the cursor**
  (`smart_spacing`) with **names spelled as the window shows them**
  (`screen_names`) and kept out of clipboard history (always), **one line and
  no Enter of Yap's own in a terminal** (`terminal_mode`; Claude Code/Codex in
  Windows Terminal keep line breaks), **no full stop on a short chat message**
  (`chat_no_full_stop`), the **Yap bar on** (`bar_enabled`), hidden over
  fullscreen apps (`bar_hide_fullscreen`), the dictation overlay always shown
  while recording/transcribing (no off switch — it's the hot-mic indicator),
  without ✕ / ✓ (`bar_dictation_buttons` "off": Cancel is in the tray),
  speech models on the graphics card unless the processor measured clearly
  faster on this PC (`stt_device` "auto"; the graphics card until measured), a day of dictation audio kept
  (`keep_recent_audio`), "Use what I said" offered on the bar after AI cleanup
  (`as_said_offer`) with no shortcut (`as_said_hotkey` ""), no Paste last /
  Copy last transcript shortcuts (`paste_last_hotkey`, `copy_last_hotkey`
  ""), Snip to Chat on
  Win+Alt+S (`snip_hotkey`), saying "snip" taking one (`snip_voice`) and chat
  screenshots kept 30 days (`chat_image_days`), a snip's Save opening in
  Pictures until a folder is chosen (`snip_save_dir` ""), the rewrite
  shortcut taking the last dictation when nothing is selected
  (`rewrite_last_dictation` on; the shortcut itself unbound), Yap's five
  saved transforms with no shortcuts (`transforms`; Translate to Spanish),
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
  **on** (`show_tips`; one notice at a time either way, `notices/notice.rs`), call
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
  (`meeting_brief`, on), meeting audio **kept 14 days** on this PC to
  transcribe a meeting again and play its lines (`meeting_keep_audio`,
  `meeting_audio_days`), and a meeting **improved after it ends when it
  helps** (`meeting_refine` "auto": chunks failed, or a more accurate model
  is installed); spoken replies **off** (`speak_replies`; once on, only
  for questions asked by voice, `speak_when`, and a dictated question is sent
  by itself, `talk_back_hands_free`) in **Kokoro's af_bella** at 1× once
  Kokoro is downloaded (`tts_engine` "kokoro"), Windows' default voice
  until then.
