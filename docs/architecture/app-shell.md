# App shell

The full stack list, the app's entry and plumbing (`lib.rs`, the tray, context menus, portable mode, logging, the Tauri commands) and the window config. Settings and data files are [config-and-data.md](./config-and-data.md).

[Architecture index](./README.md) · [CLAUDE.md](../../CLAUDE.md)

## Stack

- **Shell:** [Tauri 2](https://tauri.app) (Rust backend + webview frontend).
- **Frontend:** Svelte 5 + Vite 6 (`src/`). Windows: **settings** (the main
  ControlPanel), **onboarding**, **notepad** (the meeting notepad, docked to
  the screen edge while a meeting records), **quick-note** (the quick note,
  2026-10-28: a small floating window on its own shortcut,
  `notes/quick_note.rs`, [notes.md](./notes.md#the-quick-note-notesquick_noters)), **overlay** (since 2026-10-05 the
  **Yap bar**, `bar.rs`: an always-there pill, click-through except over what
  it draws; with the bar off, the transcribing overlay as before). The old
  pill window was retired 2026-07-09.
- **Markdown:** [`markdown-it`](https://github.com/markdown-it/markdown-it) 15.0.3
  (MIT) parses answers and notes, [`DOMPurify`](https://github.com/cure53/DOMPurify)
  3.4.17 (MPL-2.0 or Apache-2.0) sanitises the HTML; both pinned exact, used only by
  `lib/core/markdown.js` ([notes.md](./notes.md#the-markdown-renderer)).
- **Backend:** Rust (`src-tauri/src/`).
- **Audio:** `cpal` (capture) + `rodio` (start/stop chime, spoken replies).
- **Spoken replies:** **Kokoro-82M v1.0** (the default once downloaded:
  onnx-community's fp32 ONNX on Yap's own `ort`, CPU, `engines` builds;
  misaki's lexicons + the eSpeak NG CLI, a separate program, for text →
  phonemes) or WinRT `Windows.Media.SpeechSynthesis` (Windows' own voices,
  `windows` crate) → PCM → rodio, sentence by sentence (`voice/tts.rs`,
  `voice/tts/`).
- **STT:** [`transcribe-rs`](https://crates.io/crates/transcribe-rs) — one crate that
  wraps **whisper.cpp** (Vulkan) *and* a family of **ONNX** models (Parakeet, Moonshine,
  SenseVoice, GigaAM, Canary, Cohere) via `ort`/ONNX Runtime (DirectML). Behind the
  `engines` feature flag (see below).
- **AI cleanup:** `reqwest` → any **OpenAI-compatible** chat endpoint (Groq/OpenAI/
  OpenRouter, or local Ollama/LM Studio). `src/llm.rs`.
- **Text injection:** the Win32 clipboard (`windows` crate: private formats + a
  verified write) + Win32 `SendInput` (paste / Enter); UI Automation reads the text
  around the cursor (`pasting/selection.rs`). `arboard` remains for the chat's copy tool.
- **Model download:** `reqwest` streaming + `sha2` verify + `flate2`/`tar` extract.
- **Updates/install:** `tauri-plugin-updater` (GitHub Releases), driven Rust-side by
  `updates.rs` (background checks every ~4 h + background download, install on
  request) + custom NSIS installer.
- **Autostart:** `tauri-plugin-autostart`.
- **Window state:** `tauri-plugin-window-state` persists the main window's size, position, and
  maximized state across launches (overlay, onboarding denylisted; VISIBLE flag excluded
  so the window never un-hides on start-hidden launches).
- **External links:** `tauri-plugin-opener` (opens URLs in the default browser). Every
  link in Yap's pages goes through `shell::commands::open_external` (`lib/core/externalLinks.js`):
  web and mail links only, and a test run records the link (`e2e_opened_urls`) instead
  of opening a browser on the PC running it. Links inside rendered markdown (chat
  answers, notes: `a.md-link` from `lib/core/markdown.js`) open the same way, through one
  click handler in `main.js`, never navigating Yap's own window.
- **No browser context menu:** `webview2-com` (the copy wry already builds on)
  hooks each webview's `ContextMenuRequested`, cutting WebView2's menu down to
  the editing commands (`shell/context_menu.rs`; debug builds: Shift+right-click
  for Inspect). Items with actions get Yap's own menu, the same everywhere
  (`ui/contextMenu.svelte.js`).
- **Dialogs:** `tauri-plugin-dialog` (file open/save dialogs for note export and upload).
- **Accounts (optional):** `cloud/` is the sign-in server — [Better Auth](https://better-auth.com)
  on a Cloudflare Worker + D1 at `auth.contextmirror.com` (see `cloud/README.md`). Yap's side
  is `auth.rs`: system-browser OAuth (Google/GitHub/Discord, RFC 8252 + PKCE) handed back via
  `tauri-plugin-deep-link` (`com.contextmirror.yap://`), a loopback listener or a pasted code,
  plus in-app email codes and "Sign in with your phone" (a locally drawn QR code, RFC 8628
  device authorization); the session token lives in Windows Credential Manager
  (`keyring-core` + `windows-native-keyring-store`). Deep-link/single-instance are held on 2.4.x
  (2.5+ needs tauri 2.12).
- **MCP (AI apps):** [`rmcp`](https://crates.io/crates/rmcp), the official Rust MCP SDK
  (server + stdio, no macros): `yap.exe mcp` lets Claude, ChatGPT desktop, Gemini CLI,
  Cursor… read meetings and notes through the local API (`integrations/mcp.rs`); `toml_edit` adds Yap to
  Codex/ChatGPT's `config.toml` (`integrations/mcp_clients.rs`).
- **Speaker diarization (beta):** NVIDIA's Nemotron-3-Diarization (q8_0 GGUF,
  OpenMDW-1.1) on NeMo-Speech.cpp 0.2.0 (Apache-2.0, ggml), run by `yap.exe
  diarize`, a child process that loads the runtime's C API DLLs from the data
  folder with `libloading` (`LoadLibraryExW`, DLL_LOAD_DIR | DEFAULT_DIRS);
  `zip` unpacks NVIDIA's runtime release (`speakers/diarize.rs` + `speakers/diarize/`); its
  voices go on the "Them" lines as "Speaker 1, 2…" (`speakers/labels.rs`).
- **Calendar:** Google Calendar (installed-app OAuth in the system browser, PKCE, a
  loopback listener, two read-only scopes) or any private iCal link, read straight from
  the PC (`calendar.rs` + `calendar/`); [`rrule`](https://crates.io/crates/rrule) expands
  RFC 5545 recurrence and [`chrono-tz`](https://crates.io/crates/chrono-tz) is the IANA
  zone database (Windows zone names mapped in `calendar/tz.rs`); tokens and links in
  Windows Credential Manager, like the account session.
- **Data dir:** `%APPDATA%/yap/` (`config.json`, `models/` — speech models,
  and `models/diarization/` the speaker model + its runtime, `groq_usage.json`,
  `history.json`, `notes.json` — the AI Notepad store, `chats.json` — AI Chat
  conversations, `updates.json` — update announcements + the restart marker,
  `calendar.json` — connected calendars + the next week of meetings,
  `dictionary_usage.json` — how often/lately each dictionary word was used,
  `dictionary/dict_usage.rs`, `icons/` — a cache of the installed call apps' own icons,
  `calls/app_icons.rs`, `hints.json` — push-to-talk coaching counts, `dictation/coaching.rs`,
  `word_suggestions.json` — your answers to "Add this word to your
  dictionary?", `dictionary/learn.rs`, `notices.json` — call-prompt fatigue counts,
  `notices/notice.rs`).
  Every JSON store writes atomically and quarantines a corrupt file on load
  instead of crashing (`config::atomic_write`/`quarantine_corrupt`, used by all
  eleven stores; each write its own temp file). config.json too: set aside
  as it was, the settings that still read kept, the person told once, saves
  one at a time ([config-and-data.md](./config-and-data.md)). A store's
  write that fails keeps its data in memory, is retried with the next save
  and is said once, with Try again (`shell/saving.rs`; `saving::init` in setup,
  `saving::flush` in `shutdown_cleanup`); setup sweeps the temp files a
  crash mid-write left (`config::sweep_temp_files`, its own thread). Every
  store **reads** through `saving.rs` too (`Saver::read` / `load` /
  `loaded`, 2026-10-30): no file → empty; not the store's shape → set
  aside; **a file Yap couldn't read** (another program holding it, Windows
  refusing, a cloud file offline) → the store works in memory but never
  writes over it, said once ("Yap couldn't read your notes", Try again), read
  again in the background and on use, and once it reads, its contents and
  what was added meanwhile kept together (`saving::merge_json`) —
  [config-and-data.md](./config-and-data.md#a-file-yap-couldnt-read).
  `saving::init` says a store that couldn't be read before the app was up.

## Modules

**The source tree** (2026-10-24): `src-tauri/src/` is grouped by area, as
CLAUDE.md's module index is — `dictation/`, `pasting/`, `dictionary/`, `ai/`,
`chat/`, `voice/`, `meetings/`, `calls/`, `notices/`, `integrations/`,
`privacy/`, `shell/`, and the areas that are one module with parts
(`memory`, `calendar`, `snip`, `speakers`, `notes`, `bar`, `web`, `apps`);
`config.rs`, `auth.rs` and `updates.rs` stand alone; `main.rs` and `lib.rs`
are at the root. An area is `<area>.rs` (its `//!` summary and module list)
beside `<area>/`. A big module's parts are its children (`dictation/pipeline/`,
`ai/local_llm/`, `web/`, `apps/`, `voice/tts/`, `chat/tools/`,
`meetings/meeting_summary/`): private, with what other modules use
re-exported by the module, except `local_llm`'s, which callers name
(`local_llm::models::install`, `local_llm::context::context_view`).

- **`lib.rs`** — app entry / Tauri `setup`. Runs `portable::init()`, registers the
  updater/autostart/single-instance plugins, starts the input hook + pipeline
  (which no longer loads the speech model or opens the mic on this thread:
  the model loads in the background and the mic opens when a dictation needs
  it — `dictation/pipeline.rs`, `dictation/mic.rs`, [dictation.md](./dictation.md); 2026-10-22,
  cold start → tray 3.35 s → 0.99 s, the Yap bar 3.40 s → 1.03 s, the main
  window 3.34 s → 0.98 s, the model ready at ~3.3 s either way: medians of
  five launches of a release-optimised build with debug assertions on — so no
  single-instance lock — in test mode, portable, Parakeet V3 on DirectML),
  routes `dictation-key-pressed`/`-released` → `Pipeline.on_key()` (so recording works
  before the webview is ready), drives the **overlay** (the Yap bar,
  `bar::on_pipeline_state`) and **tray** off `yap-state`,
  builds the tray (always — it's the only persistent surface), reconciles
  autostart (release builds only: a dev build shares the installed app's `Yap`
  Run entry, so `set_autostart_enabled` leaves the OS setting alone), and
  starts the update scheduler last (`updates::init`). **What a launch shows**
  (2026-10-22; before it a release build started to the tray with nothing on
  screen but the Yap bar, so opening Yap from the Start menu seemed to do
  nothing): the sign-in entry passes **`--autostart`** (`AUTOSTART_ARG`, the
  autostart plugin's args), and a release launch brings the main window up
  unless it was at sign-in (`launched_at_sign_in`: the argument, or — for an
  entry written before it existed — autostart on and Windows up < 10 min),
  "Start hidden" is on, there's no speech model (onboarding instead) or it's
  the updater's relaunch (`updates::relaunch_after_update`: `updates::init`
  brings the window back if it was open) — `launch_shows_window`
  (`shell/autostart.rs`, with the sign-in helpers), pure,
  unit-tested. An autostart entry without the argument is written again on
  the next launch that lacks it. Dev builds show Settings on every launch.
  **`yap.exe bench`'s cold-start copy** (`bench_child`, 2026-10-24:
  `YAP_BENCH_CHILD=1`, honoured only in portable mode — `shell/bench/cold.rs`
  starts it on an empty data folder): no single-instance plugin (it must
  start, not wake a Yap already running), no window-state plugin, no
  keyboard hook, no orphan-sidecar sweep and no autostart reconcile (which
  would delete or repoint the real Yap's sign-in entry). At setup the ONNX
  models' DirectML adapter is set to the graphics card (`gpu::speech_adapter`,
  ai-models.md).
  `shutdown_cleanup()` (sidecar, bridge, other apps' sound back — `mute::restore_all`) runs on `RunEvent::Exit`
  AND from the updater's pre-install hook. Clears ort's 0-byte `DirectML.dll` stub (`stt::fix_directml_stub`).
  Gives the hidden settings/onboarding/notepad/quick-note webviews a one-shot **DWM-cloaked** show+hide
  at startup (`init_hidden_webview`, `shell/window.rs` — WebView2 created-hidden workaround). ⚠ Never
  swap the cloak for "park off-screen, show+hide, move back": DWM's close animation
  follows the window back on-screen — that flashed the "Welcome to Yap" window on
  every launch until 2026-09-25.
- **`shell/tray.rs`** — state-aware tray icon (runtime-generated coloured dot) + right-click
  menu (model submenu w/ checkmark, Cancel while recording — kept for Undo —, Settings/Quit, Check for
  updates → Settings → About + a manual check); left-click opens Settings. A ready
  update adds **"Restart to update to X"** under the version line (replacing
  "Check for updates…"; portable: "Get Yap X on GitHub…"), a green dot on the icon
  and "· Update X ready" in the tooltip (`tray::refresh`, driven by updates.rs).
  A live call Yap asks about, while nothing records, puts **"Record this Teams
  call"** above it (`meeting_detect::tray_item`; the click runs
  `on_tray_record` on a worker thread). The calendar's **next meeting**
  ("Next: Weekly sync in 12 min", "Now: Design review · Take notes";
  `calendar::tray_item`, [calendar.md](./calendar.md#day-to-day-2026-10-29))
  sits there too, and on the tooltip's second line; only whether there is
  one is in the menu's key, and its countdown sets the item's text in place
  (`apply`), so the menu isn't rebuilt each minute; the click runs
  `calendar::on_tray_next` on a worker thread. **"Copy last dictation"** sits above
  the model submenu (greyed out until there's a dictation; onto the
  clipboard, out of Win+V history, `pasting/last.rs`; "Paste last
  transcript" and "… (as said)" left the tray on 2026-10-25 — three ways to
  reuse one dictation read as jargon), then **"Read selected text aloud"** — "Stop reading aloud" while Yap reads
  (`read_aloud::read_selection(Origin::Tray)`: the app window on top, brought
  back, its selection read aloud with the reading card). All four are in the
  idle menu's cache key (`idle_menu_key`), so the menu rebuilds when they
  change (the reading card coming or going refreshes it); all tray work runs
  on the main thread. After Snip to Chat, **"New quick note"** (2026-10-28:
  the quick-note window on a new note, `notes/quick_note.rs`). Debug-only
  `e2e_tray {click?}` lists the idle menu's items and, last, `["tooltip",
  …]`, or chooses one (a test run's tray is never clicked).
- **`shell/context_menu.rs`** — no browser context menu in Yap's windows: each
  webview's `ContextMenuRequested` (WebView2, via `webview2-com`) keeps only
  the editing commands, or shows nothing (pure `keep`, unit-tested on the
  menus WebView2 really offered). Inspect: debug builds only, and only for a
  Shift+right-click (`offers_inspect(debug, shift)`; Shift read with
  `GetKeyState`/`GetAsyncKeyState`, compiled out of release). See "Window
  config" below.
- **`shell/webview_memory.rs`** — hidden webviews keep less in memory (2026-10-22):
  a watcher thread reads each window's visibility every 3 s (`target`, pure:
  hidden or minimised → low) and gives an off-screen window's webview
  WebView2's **low memory target** (`ICoreWebView2_19::
  put_MemoryUsageTargetLevel`), back to normal on screen. Scripts keep
  running (not `TrySuspend`: Settings' auto-save, the notepad and the toast
  policy need their hidden pages live). The Yap bar and the split preview
  are left alone. Debug-only `e2e_webview_memory` reports each window's
  target (`polish.spec.js`, `quick-note.spec.js`). The main window,
  onboarding, the notepad and the quick note are hidden most of the day,
  each a renderer process of its own.
- **`shell/theme.rs`** — the theme (Settings → General → Appearance → Theme,
  2026-10-22) on the Rust side: `resolve` (pure: "dark" / "light" as
  written, else Windows' app mode, `AppsUseLightTheme`), and `apply` gives
  the opaque windows' webviews their page's background
  (`set_background_color`: main and onboarding #f0ede7 / #161410, the
  notepad and the quick note #faf9f6 / #1c1a16 — a test keeps them equal to app.css) so
  nothing shows WebView2's white before a page paints, plus onboarding's
  Windows title bar dark or light (DWM immersive dark mode). Run at setup
  (before the DWM-cloaked first show), from `save_config`, and on the main
  window's `WindowEvent::ThemeChanged`. Debug-only `e2e_webview_background`
  reads a webview's DefaultBackgroundColor back (`theme.spec.js`). The pages
  themselves: [frontend.md](./frontend.md#the-dark-theme-2026-10-22).
- **`shell/guide.rs`** — **getting started** (2026-10-29): what the
  first-run setup and the main window's "Set up meeting notes" checklist
  remember, in `<data>/onboarding.json` (its own file: an older Yap never
  touches it) — the setup's calendar step answered (`guide_calendar_step`:
  "connected" | "skipped"; `calendar_step_due`, pure, is false once
  answered or with a calendar connected), the typing test's numbers
  (`guide_typing {wpm, speaking}`: words a minute, at most 250 typed and
  400 spoken; nothing typed or said), the checklist dismissed
  (`meeting_setup_dismiss`). `meeting_setup_status` (off the window's
  thread): a calendar connected (`calendar::connected`), a meeting note
  (`note_type` "meeting"), Yap in an AI app's config
  (`mcp_clients::added_anywhere`: added or since moved), dismissed, and
  `shown` (`checklist`, pure: not dismissed, not all done). Debug-only
  `e2e_guide_reset`. Unit tests; e2e `onboarding-steps.spec.js`,
  `meeting-setup.spec.js`.
- **`shell/help.rs`** — **the Help menu's shortcut sheet** (2026-10-29):
  `shortcut_sheet` → every global binding as the hook binds it, read from
  the input hook's own list (`input_hook::FIXED_KEYS` / `FIXED_NAMES`, each
  action's first shortcut and its more: `bound_first` / `bound_more`, so a
  key left unbound — clashing with an older shortcut's, Send on Enter — is
  "Not set"), so a binding added there shows up by itself; then the saved
  transforms' and the language shortcuts' keys (their own slots,
  `transforms::assign` / `languages::assign`), and the recording mode. `sheet` is pure and unit-tested; the page is
  `lib/views/help/ShortcutSheet.svelte` (frontend.md).
- **`shell/com.rs`** — COM's multithreaded apartment kept up for as long as
  Yap runs (2026-10-29): `keep_mta` (`CoIncrementMTAUsage`, never given back),
  first thing in `run`, and before any test in a test run (a `.CRT$XCU`
  initialiser). Notifications and other WinRT objects are used from threads
  without COM of their own (the implicit MTA); when the last thread that had
  joined the MTA left it (a UI Automation read), Windows took it down under
  them — an access violation in the notification tests now and then, or
  "Cannot create a WinRT object during host MTA shutdown".
- **`shell/portable.rs`** — portable-mode detection (a `portable` marker next to the exe
  redirects data to `<exe>/Data`).
- **`shell/download.rs`** — **one download helper for every big
  download** (2026-10-25): the on-device AI models and their engine
  (`ai/local_llm/models.rs`), the speech models (`dictation/stt.rs`), the
  meeting search model (`search/embedder.rs`), Kokoro and eSpeak NG
  (`voice/tts/registry.rs`) and the speaker engine
  (`speakers/diarize/registry.rs`) all fetch through `fetch(Fetch {url,
  partial, size, what, ticket}, report)`. **Resume**: the bytes already in
  the `.partial` file are asked for no more (HTTP `Range`; a server that
  sends the whole file instead starts it again, a 416 throws the partial
  away); a dropped connection or a server silent for `STALL` (60 s) keeps
  the partial for the next try. **Cancel** (`Ticket::begin(id)` for the
  whole install, every file of it; the command `download_cancel {id}` →
  bool; ids `llm:<curated id>`, `stt:<model>`, `search`, `kokoro`,
  `diarization:<id>`; `Ticket::unlisted` for the background runtime
  upgrade and the evaluations): it stops within 100 ms and **its partial
  file goes** (`CANCELLED`, which the pages never show as a failure); an
  install's half-made staging folder goes too. **Progress** (`Progress
  {downloaded, total, bytesPerSec, etaSecs}`, at most every 250 ms and the
  last always): the speed over the last 5 s (`Meter`) and the time left at
  it; `Overall` does the same over a package of files (Kokoro's 60). Every
  progress event (`local-llm-download-progress`, `stt-download-progress`,
  `yap-search-download`, `tts-download-progress`,
  `diarization-download-progress`) carries `bytesPerSec`, `etaSecs` and
  `cancelId` besides its old fields. **Room**: one that won't fit on the
  disk (with 64 MB to spare) is refused before it starts; one bigger than
  its pinned size is refused and its partial deleted. Files stay under
  their temporary names (`.partial`, `.installing`) until checked (each
  caller's SHA-256) and moved into place, so a half download is never
  taken for a model — by this Yap or an older one sharing the folder. The
  pages: `core/downloads.js` (`fromEvent`, `downloadLine` "1.2 GB of 2.8
  GB · 24 MB/s · about 1 min left", `cancelDownload`, `isCancelled`) and
  `ui/DownloadProgress.svelte` (the bar, the line, Cancel; `compact` in a
  model row) — Settings' Language Models, Speech-to-Text and speaker
  models, Voice Assistant's Kokoro row and offer, meeting search, the Chat
  model picker's row, onboarding. Unit-tested against a local server
  (resume, a server that won't, cancel mid-way, a bigger download, the
  maths); e2e `downloads.spec.js`.
- **Logging** (`shell/logging.rs`: `init_logging`, first thing in `run`) — tracing → stdout + a daily-rolling
  `<data>/logs/yap.log.*` file at `info`; panics are hooked into the log.
  ONNX Runtime's own log (`ort`) is held at `warn` in both modes
  (`log_directives`): at `info` each model load wrote ~1,000 lines of graph
  optimisation notes, 96% of a release build's startup log.
  **Debug mode** (Settings → Privacy & data → Debug logging, OpenWhispr Developer-
  section port): `config.debug_logging` raises `yap_lib` to `debug` live via a
  `reload::Layer` handle (`set_debug_logging`, applied on config save; RUST_LOG
  env always wins). `log_info`/`open_logs_folder` commands back the UI (current
  log file + copy path + open folder). **No user content at `info` and
  above** (since 2026-10-22): no transcripts or cleaned text, chat messages or
  answers, memory items, notes, meeting lines, About you, calendar titles,
  search queries or clipboard contents, nor a microphone's or a headset's
  name (a Bluetooth one's can carry its owner's, "Sam's AirPods"; since
  2026-10-30) — only lengths, counts, ids and outcomes (`Transcript
  chars=42`). The text may go to `debug!` (Debug
  logging, whose hint says so), never an API key or a token at any level.
  `privacy/log_privacy.rs` is a lint-style test that fails on an `info!`/`warn!`/
  `error!` formatting a content-named variable (`text = %…`, `{answer}`,
  `device = %…`); the
  webviews' `frontend_log` lines follow the same rule.
- **`privacy/net.rs`** / **`privacy/diagnostics.rs`** — the network receipts (every request
  goes out through `net::Recorded::send_for`; `net::init` in setup names
  `connections.json`, `net::flush` in `shutdown_cleanup`) and Report a
  problem's diagnostic file (its logs are this section's `info` and above,
  scrubbed again): [privacy-receipts.md](./privacy-receipts.md).
- **The Tauri commands** — each with its area (until 2026-10-24 most were in
  one `commands.rs`); `lib.rs`'s `generate_handler!` lists them all, and the
  pages call them by name. Most live in the module they serve
  (`voice/tts.rs`'s, `calendar.rs`'s…); the older ones are in an area's
  `commands.rs`:
  - `dictation/commands.rs` — recording (`toggle_recording`, `cancel_recording`,
    `dictation_key` — the in-page fallbacks' hotkey down/up), models
    (`installed_models`, `download_model_size`, `set_active_model`,
    `delete_model`, `model_language_info`; `set_active_model` is async and
    loads off the window's thread, and not at all when the model is already
    warm on its device — onboarding loaded it twice, the second time freezing
    the window), devices (`list_audio_devices`, `list_output_devices`,
    `set_mic_test` — idle level meter; the microphones' order is
    `dictation/mics.rs`'s `list_mics` and `mic_offer_respond`, since
    2026-10-25 instead of `set_input_device`),
    the shortcuts re-bound live (`configure_hotkey`, `configure_assistant_hotkey`
    / `assistant_key` — the assistant shortcut, live-applied / in-page,
    `configure_read_hotkey` — Read aloud's; its press is `read-key-pressed` →
    `read_aloud::read_selection` in lib.rs), `chat_dictate` / `chat_input_focus`
    (the Chat view's mic and box, dictation/pipeline.rs `to_chat`), debug-only
    `e2e_chat_dictation {text}` (a dictated chat question without a mic),
    Upload (`transcribe_file`/`cancel_file_transcription`/`audio_file_info`),
    history (`get_history`, `clear_history`, `delete_history_entry`,
    `get_stats`; `home_snapshot` — Home's history page, stats, the settings it
    shows and the installed models in one async call: frontend.md "Home's
    first paint").
  - `shell/settings.rs` — config (`get_config`/`save_config`; `change_config`
    changes one setting from elsewhere in one step and applies it as a save
    does, `apply_config`); `shell/window.rs` — windows (`open_settings`,
    `open_onboarding`, `close_onboarding`; `show_settings`/`show_onboarding`
    for the tray and setup); `shell/autostart.rs` — `set_autostart`;
    `shell/logging.rs` — `log_info`/`open_logs_folder`/`frontend_log`;
    `shell/commands.rs` — `open_external`, `is_portable`.
  - `ai/commands.rs` — `test_post_process`, `get_groq_usage`, the base
    prompts, the on-device model's status, install, folder, start and stop.
  - `notes/commands.rs` — the notes/actions/folders CRUD
    (`notes_folder_rename`/`_delete`; `note_delete` refuses the note a meeting
    is recording into — that's `meeting_delete` — and tells every window,
    `yap-note-deleted`) + `note_enhance` (`run_enhance`)/`note_ask`/`note_export`.
  - `chat/commands.rs` — chats CRUD (incl. `chat_rename`) + `chat_send` (RAG +
    tool loop; `scope: "meetings"` = asked across the meetings — the Meetings
    view's Ask bar, kept with the conversation — answered from the passages
    `search::ask` finds, cited ([search.md](./search.md)); `voice` = the
    question was dictated; returns
    the two messages' ids and `speak`, `tts::auto_speak`); its turn is
    `chat/turn.rs`'s `chat_turn` (shared with the Voice Assistant).
  - `meetings/commands.rs` — `meeting_start` (then `calendar::on_meeting_started`)/
    `meeting_state` (a recording ends through meetings/meeting_end.rs's
    `meeting_end`); `integrations/bridge.rs` — `bridge_status`.
  Elsewhere: `read_aloud_text` / `_selection` / `_action` / `_state`
  (voice/read_aloud.rs), `chat_view_state` + `assistant_stop`
  (chat/assistant.rs), `paste_last_transcript` (pasting/text_injector.rs).

## Window config (`src-tauri/tauri.conf.json`)
- **settings**: 1200×800 (min 860×600), titled "Yap" (it hosts the ControlPanel — see above),
  **undecorated** (custom in-page title bar w/ drag region + caption buttons — see the
  "Custom window chrome" bullet above), hidden, hide-on-close. Size/position/maximized
  persisted by `tauri-plugin-window-state`
  across launches; overlay, onboarding, notepad, split-preview, quick-note are excluded from persistence; the VISIBLE flag
  is excluded so the window never un-hides on start-hidden launches (see lib.rs window-state
  plugin setup; its flags: `shell/window.rs`). **Fitted to the screen** on every show (`window::fit_to_screen`,
  pure `fit_rect`, unit-tested): a window bigger than its monitor's work area,
  or partly off it, is shrunk and moved inside — 800 px is taller than a
  1080p laptop at 150 % (672 px above the taskbar) or a 1366 × 768 screen,
  where the centred window's title bar sat above the screen.
- **onboarding**: 620×720 (min 520×560), hidden, hide-on-close; fitted to the
  screen the same way (its buttons were under the taskbar on such laptops).
- **notepad**: the meeting notepad, 560×900 (min 360×480), undecorated (its own
  title bar), hidden, **unfocused** (opening it beside a call never takes the
  foreground; only `notepad::open`, a click in Yap, focuses it), hide-on-close
  (`notepad::init`), gets the DWM-cloaked init like settings/onboarding.
- **quick-note**: the quick note (2026-10-28, `notes/quick_note.rs`),
  420×380 (min 300×220), undecorated (its header is the drag region),
  **always on top**, hidden, unfocused (`focus: false`; its shortcut, the
  tray and Notes show it and `set_focus` it, never in test runs), close =
  hide (`quick_note::init`), the DWM-cloaked init. Placed by
  `quick_note.rs` (the bottom right of the cursor's monitor the first time
  and when it's on another one; else where the person put it), never by
  the window-state plugin.
- **split-preview** (not in tauri.conf.json): created on demand by
  `notepad::show_preview` the first time the notepad's split button is
  hovered — transparent, borderless, click-through, unfocusable, always on top,
  skip-taskbar; shown over the call's slot, hidden on pointer leave.
  Placed by `meetings/notepad.rs` (docked right, full height), never by the
  window-state plugin.
- settings + onboarding + notepad + quick-note are **created unfocused** (`focus: false`): otherwise wry
  `MoveFocus`es each new webview and the hidden windows grab the foreground at launch,
  so typing (and a dictation's paste, which never targets Yap's own windows) can land
  in an invisible window. A
  side effect is that their first `show()` doesn't activate, so every open path must
  `show()` + `set_focus()` (`window::show_settings`/`show_onboarding` do).
- **overlay** (the Yap bar, `bar.rs`): 520×560 logical, never resized,
  transparent, always-on-top, `skipTaskbar`, `focus: false` + **`focusable:
  false`** (WS_EX_NOACTIVATE); `overlay::win::install` subclasses it at setup
  (tool window, MA_NOACTIVATE, style enforcement) and it's click-through
  except over the pill/cards. Shown while the bar is on (hidden for an hour,
  over fullscreen apps, or off: only while dictating).
- **No browser context menu, in any window** (`shell/context_menu.rs`): WebView2's
  own right-click menu (Back, Refresh, Save as, Print, More tools, Inspect) is
  cut down to the editing commands. `on_page_load` (Started) hooks every
  webview once — settings, onboarding, notepad, quick-note, overlay, and split-preview
  when it's made — through `ICoreWebView2_11::add_ContextMenuRequested`. The
  pure `keep` keeps undo, redo, cut, copy, paste, paste as plain text
  (`pasteAndMatchStyle`), select all, emoji and the spelling items
  (`spellcheck` — WebView2 says that, the reference says `spellCheck`; both
  kept), with only the separators that still separate something; nothing
  left → handled, no menu; a failure reading it → no menu (fail closed).
  Plain UI: nothing; a text field: its edit menu; items with actions: Yap's
  own menu (`ui/contextMenu.svelte.js`, which cancels `contextmenu` first).
  **Debug builds: Shift+right-click for Inspect** (`inspectElement` kept
  only then — `offers_inspect(debug, shift)`, anywhere: in a dev build
  (`import.meta.env.DEV`) the pages let a Shift+right-click through even on
  items). A plain right-click never shows Inspect. Release builds (stable,
  nightly) compile the Shift read out and have DevTools off besides (no
  tauri `devtools` feature: wry's `devtools` is false without debug
  assertions), so no Inspect, F12 or Ctrl+Shift+I there; F12 / Ctrl+Shift+I
  still open DevTools in debug builds (`context-menu.real.js` checks F12).
  The names are the ones this PC's runtime reported (Oct 2026). Debug-only
  `e2e_context_menus` = each webview's hook + last menu (names, kinds,
  kept, handled, editable, target, inspect; labels in debug builds only);
  test mode marks every menu handled.
- `plugins.deep-link.desktop.schemes = ["com.contextmirror.yap"]` — the NSIS
  template registers it for normal installs (portable skips it) and
  single-instance forwards a second launch's link to the running app. Routes
  (`auth::handle_deep_link`): `auth/callback#token=…` (sign-in hand-back) and
  `account` (shows Settings → Account via `yap-open-settings` — the "Open Yap"
  button on `auth.contextmirror.com/security`, linked from the new-sign-in
  email).
