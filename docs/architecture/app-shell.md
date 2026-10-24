# App shell

The full stack list, the app's entry and plumbing (`lib.rs`, the tray, context menus, portable mode, logging, the Tauri commands) and the window config. Settings and data files are [config-and-data.md](./config-and-data.md).

[Architecture index](./README.md) · [CLAUDE.md](../../CLAUDE.md)

## Stack

- **Shell:** [Tauri 2](https://tauri.app) (Rust backend + webview frontend).
- **Frontend:** Svelte 5 + Vite 6 (`src/`). Windows: **settings** (the main
  ControlPanel), **onboarding**, **notepad** (the meeting notepad, docked to
  the screen edge while a meeting records), **overlay** (since 2026-10-05 the
  **Yap bar**, `bar.rs`: an always-there pill, click-through except over what
  it draws; with the bar off, the transcribing overlay as before). The old
  pill window was retired 2026-07-09.
- **Markdown:** [`markdown-it`](https://github.com/markdown-it/markdown-it) 15.0.3
  (MIT) parses answers and notes, [`DOMPurify`](https://github.com/cure53/DOMPurify)
  3.4.17 (MPL-2.0 or Apache-2.0) sanitises the HTML; both pinned exact, used only by
  `lib/markdown.js` ([notes.md](./notes.md#the-markdown-renderer)).
- **Backend:** Rust (`src-tauri/src/`).
- **Audio:** `cpal` (capture) + `rodio` (start/stop chime, spoken replies).
- **Spoken replies:** **Kokoro-82M v1.0** (the default once downloaded:
  onnx-community's fp32 ONNX on Yap's own `ort`, CPU, `engines` builds;
  misaki's lexicons + the eSpeak NG CLI, a separate program, for text →
  phonemes) or WinRT `Windows.Media.SpeechSynthesis` (Windows' own voices,
  `windows` crate) → PCM → rodio, sentence by sentence (`tts.rs`,
  `tts/`).
- **STT:** [`transcribe-rs`](https://crates.io/crates/transcribe-rs) — one crate that
  wraps **whisper.cpp** (Vulkan) *and* a family of **ONNX** models (Parakeet, Moonshine,
  SenseVoice, GigaAM, Canary, Cohere) via `ort`/ONNX Runtime (DirectML). Behind the
  `engines` feature flag (see below).
- **AI cleanup:** `reqwest` → any **OpenAI-compatible** chat endpoint (Groq/OpenAI/
  OpenRouter, or local Ollama/LM Studio). `src/llm.rs`.
- **Text injection:** the Win32 clipboard (`windows` crate: private formats + a
  verified write) + Win32 `SendInput` (paste / Enter); UI Automation reads the text
  around the cursor (`selection.rs`). `arboard` remains for the chat's copy tool.
- **Model download:** `reqwest` streaming + `sha2` verify + `flate2`/`tar` extract.
- **Updates/install:** `tauri-plugin-updater` (GitHub Releases), driven Rust-side by
  `updates.rs` (background checks every ~4 h + background download, install on
  request) + custom NSIS installer.
- **Autostart:** `tauri-plugin-autostart`.
- **Window state:** `tauri-plugin-window-state` persists the main window's size, position, and
  maximized state across launches (overlay, onboarding denylisted; VISIBLE flag excluded
  so the window never un-hides on start-hidden launches).
- **External links:** `tauri-plugin-opener` (opens URLs in the default browser). Every
  link in Yap's pages goes through `commands::open_external` (`lib/externalLinks.js`):
  web and mail links only, and a test run records the link (`e2e_opened_urls`) instead
  of opening a browser on the PC running it. Links inside rendered markdown (chat
  answers, notes: `a.md-link` from `lib/markdown.js`) open the same way, through one
  click handler in `main.js`, never navigating Yap's own window.
- **No browser context menu:** `webview2-com` (the copy wry already builds on)
  hooks each webview's `ContextMenuRequested`, cutting WebView2's menu down to
  the editing commands (`context_menu.rs`; debug builds: Shift+right-click
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
  Cursor… read meetings and notes through the local API (`mcp.rs`); `toml_edit` adds Yap to
  Codex/ChatGPT's `config.toml` (`mcp_clients.rs`).
- **Speaker diarization (beta):** NVIDIA's Nemotron-3-Diarization (q8_0 GGUF,
  OpenMDW-1.1) on NeMo-Speech.cpp 0.2.0 (Apache-2.0, ggml), run by `yap.exe
  diarize`, a child process that loads the runtime's C API DLLs from the data
  folder with `libloading` (`LoadLibraryExW`, DLL_LOAD_DIR | DEFAULT_DIRS);
  `zip` unpacks NVIDIA's runtime release (`diarize.rs` + `diarize/`); its
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
  `dict_usage.rs`, `icons/` — a cache of the installed call apps' own icons,
  `app_icons.rs`, `hints.json` — push-to-talk coaching counts, `coaching.rs`,
  `word_suggestions.json` — your answers to "Add this word to your
  dictionary?", `learn.rs`, `notices.json` — call-prompt fatigue counts,
  `notice.rs`).
  Every JSON store writes atomically and quarantines a corrupt file on load
  instead of crashing (`config::atomic_write`/`quarantine_corrupt`, used by all
  eleven stores; each write its own temp file). config.json too: set aside
  as it was, the settings that still read kept, the person told once, saves
  one at a time ([config-and-data.md](./config-and-data.md)). A store's
  write that fails keeps its data in memory, is retried with the next save
  and is said once, with Try again (`saving.rs`; `saving::init` in setup,
  `saving::flush` in `shutdown_cleanup`).

## Modules

- **`lib.rs`** — app entry / Tauri `setup`. Runs `portable::init()`, registers the
  updater/autostart/single-instance plugins, starts the input hook + pipeline
  (which no longer loads the speech model or opens the mic on this thread:
  the model loads in the background and the mic opens when a dictation needs
  it — `pipeline.rs`, `mic.rs`, [dictation.md](./dictation.md); 2026-10-22,
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
  brings the window back if it was open) — `launch_shows_window`, pure,
  unit-tested. An autostart entry without the argument is written again on
  the next launch that lacks it. Dev builds show Settings on every launch.
  `shutdown_cleanup()` (sidecar, bridge, other apps' sound back — `mute::restore_all`) runs on `RunEvent::Exit`
  AND from the updater's pre-install hook. Clears ort's 0-byte `DirectML.dll` stub (`stt::fix_directml_stub`).
  Gives the hidden settings/onboarding webviews a one-shot **DWM-cloaked** show+hide
  at startup (`init_hidden_webview` — WebView2 created-hidden workaround). ⚠ Never
  swap the cloak for "park off-screen, show+hide, move back": DWM's close animation
  follows the window back on-screen — that flashed the "Welcome to Yap" window on
  every launch until 2026-09-25.
- **`tray.rs`** — state-aware tray icon (runtime-generated coloured dot) + right-click
  menu (model submenu w/ checkmark, Cancel while recording — kept for Undo —, Settings/Quit, Check for
  updates → Settings → About + a manual check); left-click opens Settings. A ready
  update adds **"Restart to update to X"** under the version line (replacing
  "Check for updates…"; portable: "Get Yap X on GitHub…"), a green dot on the icon
  and "· Update X ready" in the tooltip (`tray::refresh`, driven by updates.rs).
  A live call Yap asks about, while nothing records, puts **"Record this Teams
  call"** above it (`meeting_detect::tray_item`; the click runs
  `on_tray_record` on a worker thread). **"Paste last transcript"** sits above
  the model submenu (greyed out until there's a dictation;
  `text_injector::paste_last_from_tray`, worker thread), and under it **"Read
  selection aloud"** — "Stop reading aloud" while Yap reads
  (`read_aloud::read_selection(Origin::Tray)`: the app window on top, brought
  back, its selection read aloud with the reading card). All four are in the
  idle menu's cache key (`idle_menu_key`), so the menu rebuilds when they
  change (the reading card coming or going refreshes it); all tray work runs
  on the main thread.
- **`context_menu.rs`** — no browser context menu in Yap's windows: each
  webview's `ContextMenuRequested` (WebView2, via `webview2-com`) keeps only
  the editing commands, or shows nothing (pure `keep`, unit-tested on the
  menus WebView2 really offered). Inspect: debug builds only, and only for a
  Shift+right-click (`offers_inspect(debug, shift)`; Shift read with
  `GetKeyState`/`GetAsyncKeyState`, compiled out of release). See "Window
  config" below.
- **`webview_memory.rs`** — hidden webviews keep less in memory (2026-10-22):
  a watcher thread reads each window's visibility every 3 s (`target`, pure:
  hidden or minimised → low) and gives an off-screen window's webview
  WebView2's **low memory target** (`ICoreWebView2_19::
  put_MemoryUsageTargetLevel`), back to normal on screen. Scripts keep
  running (not `TrySuspend`: Settings' auto-save, the notepad and the toast
  policy need their hidden pages live). The Yap bar and the split preview
  are left alone. Debug-only `e2e_webview_memory` reports each window's
  target (`polish.spec.js`). The main window, onboarding and the notepad
  are hidden most of the day, each a renderer process of its own.
- **`theme.rs`** — the theme (Settings → General → Appearance → Theme,
  2026-10-22) on the Rust side: `resolve` (pure: "dark" / "light" as
  written, else Windows' app mode, `AppsUseLightTheme`), and `apply` gives
  the opaque windows' webviews their page's background
  (`set_background_color`: main and onboarding #f0ede7 / #161410, the
  notepad #faf9f6 / #1c1a16 — a test keeps them equal to app.css) so
  nothing shows WebView2's white before a page paints, plus onboarding's
  Windows title bar dark or light (DWM immersive dark mode). Run at setup
  (before the DWM-cloaked first show), from `save_config`, and on the main
  window's `WindowEvent::ThemeChanged`. Debug-only `e2e_webview_background`
  reads a webview's DefaultBackgroundColor back (`theme.spec.js`). The pages
  themselves: [frontend.md](./frontend.md#the-dark-theme-2026-10-22).
- **`portable.rs`** — portable-mode detection (a `portable` marker next to the exe
  redirects data to `<exe>/Data`).
- **Logging** (`lib.rs init_logging`) — tracing → stdout + a daily-rolling
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
  search queries or clipboard contents — only lengths, counts, ids and
  outcomes (`Transcript chars=42`). The text may go to `debug!` (Debug
  logging, whose hint says so), never an API key or a token at any level.
  `log_privacy.rs` is a lint-style test that fails on an `info!`/`warn!`/
  `error!` formatting a content-named variable (`text = %…`, `{answer}`); the
  webviews' `frontend_log` lines follow the same rule.
- **`commands.rs`** — Tauri commands: recording (`toggle_recording`, `cancel_recording`,
  `dictation_key` — the in-page fallbacks' hotkey down/up),
  config (`get_config`/`save_config`), models (`installed_models`,
  `download_model_size`, `set_active_model`, `delete_model`, `model_language_info`),
  devices (`list_audio_devices`, `list_output_devices`, `set_input_device` — live
  stream swap, `set_mic_test` — idle level meter), windows (`open_settings`,
  `open_onboarding`, `close_onboarding`),
  `configure_hotkey`, `set_autostart`, `is_portable`, `test_post_process`,
  `get_groq_usage`, history (`get_history`, `clear_history`, `get_stats`),
  plus the notes/actions/folders CRUD (`notes_folder_rename`/`_delete`;
  `note_delete` refuses the note a meeting is recording into — that's
  `meeting_delete` — and tells every window, `yap-note-deleted`) +
  `note_enhance`/`note_ask`/`note_export`,
  chats CRUD (incl. `chat_rename`) + `chat_send` (RAG + tool loop; `scope: "meetings"` = the
  Meetings view's Ask bar, grounded in `calendar::meetings_context`; `voice`
  = the question was dictated; returns the two messages' ids and `speak`,
  `tts::auto_speak`), `chat_dictate` / `chat_input_focus` (the Chat view's
  mic and box, pipeline.rs `to_chat`), debug-only `e2e_chat_dictation
  {text}` (a dictated chat question without a mic), `chat_send`'s turn
  itself is `chat_turn` (shared with the Voice Assistant),
  `configure_assistant_hotkey` / `assistant_key` (the assistant shortcut,
  live-applied / in-page), `configure_read_hotkey` (Read aloud's,
  live-applied; its press is `read-key-pressed` → `read_aloud::read_selection`
  in lib.rs), `read_aloud_text` / `_selection` / `_action` / `_state`
  (read_aloud.rs), `chat_view_state` + `assistant_stop`
  (assistant.rs),
  `meeting_start` (then `calendar::on_meeting_started`)/
  `meeting_state` (a recording ends through meeting_end.rs's `meeting_end`), `transcribe_file`/`cancel_file_transcription`/
  `audio_file_info`, `log_info`/`open_logs_folder`, `delete_history_entry`.
  (`paste_last_transcript` lives in `text_injector.rs`.)

## Window config (`src-tauri/tauri.conf.json`)
- **settings**: 1200×800 (min 860×600), titled "Yap" (it hosts the ControlPanel — see above),
  **undecorated** (custom in-page title bar w/ drag region + caption buttons — see the
  "Custom window chrome" bullet above), hidden, hide-on-close. Size/position/maximized
  persisted by `tauri-plugin-window-state`
  across launches; overlay, onboarding, notepad, split-preview are excluded from persistence; the VISIBLE flag
  is excluded so the window never un-hides on start-hidden launches (see lib.rs window-state
  plugin setup). **Fitted to the screen** on every show (`commands::fit_to_screen`,
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
- **split-preview** (not in tauri.conf.json): created on demand by
  `notepad::show_preview` the first time the notepad's split button is
  hovered — transparent, borderless, click-through, unfocusable, always on top,
  skip-taskbar; shown over the call's slot, hidden on pointer leave.
  Placed by `notepad.rs` (docked right, full height), never by the
  window-state plugin.
- settings + onboarding + notepad are **created unfocused** (`focus: false`): otherwise wry
  `MoveFocus`es each new webview and the hidden windows grab the foreground at launch,
  so typing (and a dictation's paste, which never targets Yap's own windows) can land
  in an invisible window. A
  side effect is that their first `show()` doesn't activate, so every open path must
  `show()` + `set_focus()` (`commands::show_settings`/`show_onboarding` do).
- **overlay** (the Yap bar, `bar.rs`): 520×560 logical, never resized,
  transparent, always-on-top, `skipTaskbar`, `focus: false` + **`focusable:
  false`** (WS_EX_NOACTIVATE); `overlay::win::install` subclasses it at setup
  (tool window, MA_NOACTIVATE, style enforcement) and it's click-through
  except over the pill/cards. Shown while the bar is on (hidden for an hour,
  over fullscreen apps, or off: only while dictating).
- **No browser context menu, in any window** (`context_menu.rs`): WebView2's
  own right-click menu (Back, Refresh, Save as, Print, More tools, Inspect) is
  cut down to the editing commands. `on_page_load` (Started) hooks every
  webview once — settings, onboarding, notepad, overlay, and split-preview
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
