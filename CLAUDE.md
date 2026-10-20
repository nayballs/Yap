# CLAUDE.md — how Yap works

Yap is a tiny **local voice-dictation tool**: press a global hotkey, speak, press
again — Yap transcribes **locally on the GPU**, optionally runs the text through an
**AI cleanup pass** (filler/punctuation/grammar), and types it into whatever window
is focused. A chime marks start/stop, a correction dictionary fixes mis-heard jargon,
and the **Yap bar** — a tiny pill above the taskbar on whichever screen you're using —
shows a live waveform while you talk, opens into Dictate / Meeting notes on hover, and
carries Yap's notices (call prompts) while its window isn't focused. Two ways to
talk to Yap's AI, kept apart: **Commands** (say its name in a dictation, or the
rewrite shortcut: the AI writes and types for you) and the **Voice Assistant**
(its own shortcut: ask out loud from any app and hear the answer).

This file documents how Yap actually runs today. For *where it's going* and the
competitive strategy, see `ROADMAP.md` in the **private** docs repo (below).

> Origin note: the core dictation plumbing (input hook, text injection) was ported
> from "Voice Mirror"; the multi-engine STT, AI cleanup, settings, tray, overlay,
> installer and the rest is Yap's own.

> **Mission — the best-of-everything blend.** Yap's strategy is to take the best of
> every top-tier dictation app — **superwhisper**, **OpenWhispr**, **Wispr Flow**,
> **Handy**, **Aqua**, **FluidVoice** — and combine them into one Windows-first,
> local-first app. We port proven patterns **from source**, not from screenshots:
> **OpenWhispr is cloned at `E:\Projects\references\openwhispr`** (Handy at
> `references/Handy`) — when working on a feature one of these apps does well, read
> its actual implementation first. See ROADMAP.md "North star" +
> `docs/openwhispr-teardown.md` + `docs/openwhispr-parity.md`. Every Wispr Flow
> feature with Yap's status, as a checklist with a "Next up" list:
> `docs/wispr-flow-features.md`.
>
> **Private docs (2026-10-17).** The roadmap, competitor research, feature
> checklists and internal test plans are NOT in this public repo: they live in
> the private repo `nayballs/yap-private`, cloned at **`E:\Projects\yap-private`**
> (`ROADMAP.md`, `docs/wispr-flow-features.md`, `docs/openwhispr-*.md`,
> `docs/competitive-analysis.md`, …). Paths in this file named `ROADMAP.md` or
> one of those `docs/` files mean that repo. Write new competitor research,
> teardown findings and plans there (or in `E:\Projects\references\`), never in
> this repo: its `docs/` keeps only how Yap works.

> Active work note: the Settings / Language-Models / Prompt-Studio surface is being
> reshaped to track **OpenWhispr** (`E:\Projects\references\openwhispr`) as the design
> reference — porting its UX patterns and wording while keeping Yap's own backend
> contract (split immutable-guardrails + editable body). Treat that repo as the
> source of truth for this redesign pass.

> **Working agreement — standing permission (Nathan, 2026-10-04).** Nathan has given
> Claude full permission to do whatever the work needs **without stopping to
> double-check first**: commits/pushes to main, deploys of `cloud/`, releases and
> nightlies, test emails or sign-in codes to his own inbox, email/DNS hygiene such as
> DMARC, and configuring the project's third-party dashboards (Cloudflare, Google,
> GitHub, Discord, Resend). Do it, verify it, then report what was done. Limits that
> still stand regardless: never type passwords, OTPs or secrets for him (he pastes
> secrets himself, e.g. into `npx wrangler secret put`), never create accounts or
> move money, and if the harness blocks an action (DNS edits have been blocked
> before) hand him the exact clicks instead of retrying.

---

## Stack

- **Shell:** [Tauri 2](https://tauri.app) (Rust backend + webview frontend).
- **Frontend:** Svelte 5 + Vite 6 (`src/`). Windows: **settings** (the main
  ControlPanel), **onboarding**, **notepad** (the meeting notepad, docked to
  the screen edge while a meeting records), **overlay** (since 2026-10-05 the
  **Yap bar**, `bar.rs`: an always-there pill, click-through except over what
  it draws; with the bar off, the transcribing overlay as before). The old
  pill window was retired 2026-07-09.
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
  eleven stores).

---

## Architecture / runtime flow

```
global hotkey ─▶ input_hook ─▶ "dictation-key-pressed" / "-released"
                                        │  (lib.rs routes both → pipeline.on_key)
                                        ▼
                              Pipeline (toggle OR push-to-talk)
                    ┌───────────────────┴───────────────────┐
              start_recording                        stop_and_transcribe
                    │                                        │
        cpal mic stream → 16kHz mono f32      take audio ─▶ STT engine (blocking task)
        buffer (downmix + resample in              │  (transcribe-rs, warm engine)
        the audio callback); emits           AI cleanup pass (llm.rs, if enabled)
        "yap-amp" peak for the                    │
        scrolling waveform                   apply_dictionary()
                    │                              │
            "yap-state" drives the        text_injector (clipboard paste [+ Enter])
            overlay + tray                        │
                                            emits "yap-transcript"
```

Pipeline order after transcription: **AI cleanup → dictionary (exact, then a
fuzzy near-miss pass) → the "press enter" voice command → snippets → fit to the
kind of app (`app_kind.rs`: a terminal gets one line and no trailing space; a
chat app's short message loses its full stop) and to the text around the
cursor (smart spacing; where the field couldn't be read, append-space as
before) → inject (an app running as administrator takes no keys from Yap:
the text is left on the clipboard and the person told, `elevated.rs`) →
submit key (auto-submit or the voice command, once; in a
terminal Enter, only for the voice command or a coding CLI's auto-submit) —
or, with no submit key, the correction watch (`learn.rs`: fix a word Yap
typed and it offers to add it to the dictionary)**
(`pipeline::finish_dictation` does the dictionary, the command and snippets;
`text_injector::prepare_dictation` the fitting; `app_kind::submit_key` the
key). Cleanup is
best-effort — any error/timeout falls back to the raw transcript, so dictation
never blocks. The dictionary runs Handy's split (ported from its source):
**Whisper models** get the correct spellings as the decoder's `initial_prompt`
(ASR biasing, threaded through `SttEngine::transcribe` at every call site —
dictation/partials/upload/meeting — the words used most lately first,
`dict_usage::ranked`; plus an OpenWhispr-ported prompt-echo guard
in `stt.rs`), **ONNX models** get **`fuzzy.rs`** — Levenshtein + Soundex 1–3-word
n-gram correction ("jaison"→"JSON", "Chat G P T"→"ChatGPT"; threshold 0.18,
≥3-char terms), gated by `config.dictionary_fuzzy` (default on, "Catch
near-misses" toggle in the Dictionary view) with a **per-entry ≈ opt-out**
(`entry.fuzzy` — exempts corrections whose near-misses are real words, e.g.
`json → JSON` eating the name "Jason"). See `docs/fuzzy-dictionary.md`. The
exact + fuzzy correction is ONE function, `pipeline::apply_corrections(text,
&cfg)`, run the same way on a dictation, on every meeting segment (both sides,
before `meeting::ingest`) and on an Upload's whole text; the dictionary words
in its result count as used (`dict_usage::record`). **Snippets** (an entry
with `snippet: true`: a spoken phrase → text, may span lines;
`config::apply_snippets`) are dictation-only: whole phrase, any case and
punctuation, longest first in one pass, a phrase said alone becomes exactly
its text, and cleanup's full stop after a phrase is dropped where it would
spoil the text (`stray_full_stop`: after a text ending in punctuation, or as
the dictation's last character after a text of several lines — a sign-off or
address ends as saved; a one-line snippet mid-sentence keeps the sentence's
"."); never in the corrections, the fuzzy pass or the Whisper prompt (the
cleanup model is told to keep the phrases word for word). The **"press
enter" voice command** (`voice_enter`, default on;
`pipeline::take_voice_enter`): a dictation whose LAST words are "press
enter"/"press return" (final text, else the raw transcript in case cleanup
dropped it) loses them and presses `auto_submit_key` after the paste.

### Key modules (`src-tauri/src/`)
- **`lib.rs`** — app entry / Tauri `setup`. Runs `portable::init()`, registers the
  updater/autostart/single-instance plugins, starts the input hook + pipeline,
  routes `dictation-key-pressed`/`-released` → `Pipeline.on_key()` (so recording works
  before the webview is ready), drives the **overlay** (the Yap bar,
  `bar::on_pipeline_state`) and **tray** off `yap-state`,
  builds the tray (always — it's the only persistent surface), reconciles
  autostart (release builds only: a dev build shares the installed app's `Yap`
  Run entry, so `set_autostart_enabled` leaves the OS setting alone), and
  starts the update scheduler last (`updates::init`).
  `shutdown_cleanup()` (sidecar, bridge, other apps' sound back — `mute::restore_all`) runs on `RunEvent::Exit`
  AND from the updater's pre-install hook. Clears ort's 0-byte `DirectML.dll` stub (`stt::fix_directml_stub`).
  Gives the hidden settings/onboarding webviews a one-shot **DWM-cloaked** show+hide
  at startup (`init_hidden_webview` — WebView2 created-hidden workaround). ⚠ Never
  swap the cloak for "park off-screen, show+hide, move back": DWM's close animation
  follows the window back on-screen — that flashed the "Welcome to Yap" window on
  every launch until 2026-09-25.
- **`pipeline.rs`** — the heart. Owns the mic stream + shared state (`recording`,
  audio `buffer`, idle `preroll` ring, warm STT `engine`, live `config`,
  `last_activity`, `target_hwnd`). Audio callback buffers while recording,
  downmixes→mono, resamples→16 kHz, and emits a throttled **peak amplitude**
  (`yap-amp`) for the scrolling waveform; while idle it keeps a rolling ~300 ms
  **pre-roll** ring that `start_recording` prepends (anti first-word-clipping).
  `recording_mode` selects toggle vs push-to-talk; in push-to-talk `on_key` reads
  **gestures** (`gesture.rs`, Wispr Flow's by behaviour): hold to talk, a quick tap
  (< 300 ms) + a second press within 500 ms locks it **hands-free** until the next
  press (`yap-dictation-locked` → a lock on the overlay), a lone tap is thrown away
  (never a junk dictation) and earns a coaching hint (`coaching.rs`), as does a
  hold of a minute or more. Key events and the tap timer act under one lock
  (`gesture`), timed by the input hook's own timestamps; a recording started or
  stopped elsewhere (the bar's Dictate, the tray's Cancel) is followed, and one
  started by Dictate stops on the next press. `run_stt` (async) transcribes, then
  `deliver` (also robot mode's way in, `realtest.rs`; the target is captured by
  `capture_target`, mic audio arrives through `take_audio`) does cleanup →
  `finish_dictation` (dictionary via `apply_corrections`, shared with meetings
  and Upload; then the "press enter" command, then snippets) →
  `text_injector::prepare_dictation` (fitted to the kind of app,
  `target_kind` — `app_kind::of_window`, read in `start_recording` beside
  `target_hwnd` — and to the text around the cursor, which `start_recording`
  started reading in the background via `selection::begin_dictation` — not
  for edit mode, nor a terminal with terminal mode on) → inject (into the
  captured `target_hwnd`) → the submit key when auto-submit or the voice command
  asked (once, `app_kind::submit_key`; a dictation of only "press enter" presses it only while the
  target window still has the focus). **The dictation signal** for a meeting
  recording: `start_recording` (plain dictation, the edit hotkey, Commands by
  name, the assistant shortcut) calls `dictation_began()` and the stop/cancel `dictation_ended()`, one
  atomic (`DICTATION`: dictations started << 1 | recording) that
  `meeting.rs` reads lock-free in its mic callback (`dictation_state()`) to
  keep dictations out of a meeting's "You" side. `PREROLL_SAMPLES` is shared
  with it (the meeting blanks a dictation's pre-roll too). **Commands** (two
  different things share no name any more, 2026-10-18: *Commands* type for
  you, the *Voice Assistant* answers out loud): by name (`agent_detect.rs`,
  OpenWhispr `detectAgentName` port) a dictation addressing the
  agent (`agent_name`, default "Yap"; fuzzy-matched) routes the whole
  transcript through the Commands scope (`llm_scopes["voiceAgent"]`, its key kept) in write mode (`run_agent`, shared with
  the edit hotkey = the rewrite shortcut) instead of cleanup — the agent prompt strips the name+command.
  **The Voice Assistant's shortcut** (`Pipeline::on_assistant_key`, the
  hook's `assistant-key-*` or the in-page `assistant_key`): the same
  `on_key` (toggle / push-to-talk gestures; no coaching hints for it) with
  `ASSISTANT_ARMED` set for the press, so the recording it starts is the
  assistant's (`destination(edit, assistant, chat_armed, chat_focused,
  own_window)` → `Goes::Paste | ChatBox | Assistant`, pure, in
  `capture_target`, which calls `assistant::listening` before "recording");
  `run_stt` hands its transcript, after `apply_corrections` only, to
  `assistant::heard` (`ask_assistant`): no cleanup, Command, paste or
  history; nothing heard → "Yap didn't catch a question"; a cancel isn't
  kept for Undo; every early end calls `assistant::nothing`.
  **Barge-in**: `start_recording` first calls `tts::stop_for_dictation()` —
  a spoken reply stops before anything records, and if Yap was audible in
  the last ~450 ms the pre-roll is left out (it would hold Yap's voice).
  **Questions for the Chat view** (`to_chat`, decided in `capture_target` by
  the pure `chat_target(edit, armed, focused, own_window)`): the chat's mic
  (`chat_dictate` → `toggle_for_chat`, which arms `CHAT_ARMED` for the
  recording it starts) or a dictation while the chat box has the focus
  (`chat_input_focus` → `CHAT_FOCUSED`) and one of Yap's windows is in front;
  never edit mode. Such a dictation skips the wake word and the paste:
  `deliver_to_chat` emits `yap-chat-dictation {text, send}` (`send` =
  `tts::sends_by_itself`: "Talk back hands-free" or "press enter" said) and
  keeps it in history. A `processing` guard blocks
  starting a new recording while one is still transcribing (no overlapping `run_stt`
  / duplicate model load), and the buffer is capped at 15 min so a stuck key can't
  OOM. With `streaming_partials` on, `stream_partials` (a per-session worker)
  transcribes a bounded **sliding window** of the buffer every ~500 ms
  (`partials.rs`: text older than the window freezes as committed text, the
  window advances at quiet points via `media::quietest_index`, 20 s hard cap —
  per-tick cost independent of recording length; de-flickered by `smart_diff`,
  adaptive backoff on slow machines, warm-loads the engine if it was
  idle-unloaded) and emits `yap-partial` text. An **idle watcher** unloads the model after
  `model_unload_timeout`; the next dictation lazily reloads it. **No microphone** (none
  plugged in, a headset that connects after login, a CI runner) doesn't take the
  pipeline down: it runs without a stream (`has_mic`), a hotkey press retries
  (`ensure_mic`), picking a mic (`set_input_device`) starts capture, and a recording
  attempt without one says "No microphone found" (on the overlay) — or, when
  Windows' privacy switches are why, "Windows is blocking microphone access for
  desktop apps" (`mic_privacy.rs`) — Upload, models and Settings work regardless. **Dead-mic watchdog** (`mic_watch.rs`): the audio
  callback tallies each recording's live audio (`mic_tally`: how much, any sample
  not exactly 0.0); a per-recording watcher (`spawn_mic_watch`) reopens the stream
  once (`reopen_mic`) after 2 s without audio or 3 s of nothing but zeros, and at
  5 s says "No sound from <mic> — check it isn't muted or unplugged"
  (`yap-dictation-error`, plus `yap-mic-silent` for the overlay's line) while
  the recording carries on;
  sound coming back takes it back, and a recording that ends still silent isn't
  transcribed (the overlay ends on the reason). **Windows' microphone privacy
  switches** (`mic_privacy::blocked`) are checked when a recording starts — if
  they block desktop apps it's said at once (`say_mic_blocked`: the overlay's
  "Windows is blocking the microphone" + an alert with **Open settings**,
  `alert.rs`; `mic_dead` set, so a stop with no sound isn't transcribed; the
  recording goes on, and sound arriving takes it back) — and again before the
  watchdog's report, which then blames the switch instead of the mic.
  Capture start, `ensure_mic`,
  `set_input_device` and the reopen all go through `open_stream` (the stream slot
  is shared weakly with `Shared`; the reopen stops the dead stream first).
  **Cancel and Undo**: `cancel(undo)` — the bar's ✕, the tray's Cancel and
  `cancel_recording` keep a recording of ≥ 0.5 s that heard a sound in `kept`
  (`Kept`: its audio, `target_hwnd`, `target_kind`, edit mode and selection)
  for `UNDO_MS` (10 s): `yap-dictation-cancelled { undoMs }` + `yap-state`
  "cancelled" (the bar's "Dictation cancelled · Undo"), then "idle" and it's
  dropped; a new dictation drops it too (`capture_target`), and
  push-to-talk's lone tap is simply thrown away. `Pipeline::undo` (`bar_action`
  "dictation-undo") puts the target back and runs `run_stt` on the kept audio
  as a stop would — the paste goes back to the window the dictation started
  in, whatever has the focus now, or nowhere (`undoing` → `inject_text`'s
  `strict`: a window that's closed or won't come to the front leaves the
  text on the clipboard, `Delivery::WindowGone`, with "Your text is copied:
  press Ctrl+V") — after reading that window's cursor context afresh (a
  closed window's never: `read_context` refuses a target whose process is
  gone). `Pipeline::stop` is the bar's ✓ (the hotkey's second
  press). `holds_undo` keeps the updater from restarting under a pending Undo.
- **`gesture.rs`** — push-to-talk's pure gesture state machine: key down/up and
  the clock in (`now_ms`, monotonic, also what the input hook stamps events
  with), Start / Stop / Discard / Lock + an optional hint + "tick me in N ms"
  out. States Idle → Held → (TapPending → LockHeld → Locked). `TAP_MAX_MS` 300,
  `DOUBLE_TAP_GAP_MS` 500, `LONG_HOLD_MS` 60 s. Every call is told whether a
  dictation records right now and follows that first. Unit-tested.
- **`coaching.rs`** — push-to-talk coaching hints: a lone tap → "Hold {key} while
  you talk" (or double-tap for hands-free), a minute-long hold → "Double-tap
  {key} for hands-free". Each ≤ 3 times, never within 10 min of another, none
  once double-tap has been used; counts in `<data>/hints.json` (not config.json:
  Settings' auto-saved copy would write old counts back). Surface (one
  place, `notice::place`): a Tip toast in the main window while it's focused
  (`yap-hint`), else a Yap bar card (icon `tip`; it waits out a dictation or
  a meeting and is dropped if it can't show within 10 s), else the window's
  toast if it's on screen; neither → nothing shown, nothing counted (never a
  Windows notification). "Show tips" off (`show_tips`) → none. Texts say
  `{key}`; the page names the hotkey (`fillHotkey` in `lib/hotkeys.js`; the
  card's `hotkey` field). Not for the edit hotkey.
- **`mic_watch.rs`** — the dictation's dead-mic watchdog, pure: `MicTally`
  (lock-free, fed by the audio callback), `DeadMicWatch::poll(elapsed, samples,
  heard)` → Wait / Reopen / Report / Recovered / Fine, the messages (the full one
  and the overlay's short "No sound from <device> — is it muted?"). Only exact
  zeros count (a quiet room is never all zeros). Unit-tested over simulated
  callback chunks. Before it blames the mic, the pipeline asks Windows' privacy
  switches (`mic_privacy.rs`). Meetings have their own quiet-side watch (`meeting.rs`).
- **`mic_privacy.rs`** — Windows' microphone privacy switches, read only (never
  written): HKLM `…\CapabilityAccessManager\ConsentStore\microphone` `Value`
  ("Microphone access", the device), HKCU's ("Let apps access your
  microphone"), HKCU's — else HKLM's — `…\microphone\NonPackaged` ("Let desktop
  apps access your microphone"), and HKLM `…\Policies\Microsoft\Windows\AppPrivacy`
  `LetAppsAccessMicrophone` = 2 (an organisation's policy); a
  missing value is no answer. Pure `decide` names the widest switch that's off
  (policy, device, apps, desktop apps), unit-tested. Asked at a recording's
  start, in the no-mic path and before the dead-mic watchdog's report (see
  `pipeline.rs`): "Windows is blocking microphone access for desktop apps" +
  which switch to turn on + **Open settings** (`ms-settings:privacy-microphone`)
  as an alert (`alert.rs`), "Windows is blocking the microphone" in the
  overlay's line; logged when capture starts. Test mode reads no registry:
  debug-only `e2e_mic_privacy { blocked }` stands in; an `#[ignore]`d
  `mic_privacy_this_machine` prints this PC's switches.
- **`stt.rs`** — `SttEngine` trait + a real `transcribe-rs` engine (`#[cfg(feature =
  "engines")]`) and a stub (default build). Holds the **14-model registry**
  (`ModelDescriptor`: id/filename/url/sha256/is_directory/engine_type), resolves
  legacy/custom ids, and does download → SHA-256 verify → (tar.gz) extract.
  `apply_accelerator_settings` sets whisper→Vulkan(Auto) / ONNX→DirectML.
  `transcribe_timed` (default: `transcribe`, no times) keeps the times the
  model gives (`Timed { text, words: Vec<TimedText> }`, seconds from the
  audio's start): Parakeet's words (`TimestampGranularity::Word`), Whisper's
  segments (a phrase each), SenseVoice's tokens joined at "▁"; none for
  Moonshine/GigaAM/Canary/Cohere or the stub. Only meetings call it.
- **`llm.rs`** — the AI cleanup client (OpenAI-compatible). Frames the transcript as
  data (delimiters + one-shot) so small models *clean* it instead of *answering* it.
  The system prompt is split FluidVoice-style: an immutable `BASE_PROMPT` (guardrails:
  output-only, never answer the transcript) that's always prepended via
  `build_system_prompt()` to the user's editable **body** (tone/format = a preset or
  custom text). Records token/request usage (best-effort). Also carries
  `EDIT_BASE_PROMPT` (edit/rewrite mode's guardrails), `NOTE_BASE_PROMPT` +
  `MEETING_NOTE_BASE_PROMPT` + `NOTE_DEFAULT_FRAGMENT` (note enhancement's
  guardrails/action fragment), `ACTION_PLAN_BASE_PROMPT` +
  `ACTION_PLAN_DEFAULT_FRAGMENT` (the Action Plan: never invent owners/dates)
  and `MEETING_DIGEST_PROMPT` (one rolling meeting digest), the notepad
  helpers' immutable prompts (`CATCH_UP_PROMPT`, `MEETING_ASK_PROMPT`,
  `MEETING_TITLE_PROMPT`, and the one-tap chips' `MEETING_QUESTIONS_PROMPT`,
  `MEETING_FOLLOW_UP_PROMPT` — a plain-text email, subject line first — and
  `MEETING_BLIND_SPOTS_PROMPT`; all: only what's in the meeting, never
  invent owners/dates/numbers, say when there's too little; every meeting
  prompt explains the labels through one `other_speakers!()` phrase: "You:",
  a speaker's name (a known person, like an attendee; the "Speakers besides
  You" line says which are guesses), "Speaker 2", or "Them"), `enhance_note`
  (the Actions-engine call, base prompt chosen by the caller), `chat` +
  `ChatOptions` (temperature, `max_tokens` — sent for local servers only —
  and timeout), `note_chat` (embedded per-note chat + Chat-surface turns), and
  `post_chat_message` (the tool-loop variant that returns the whole assistant
  message, incl. `tool_calls`, instead of just the text). **Chat's timeout**
  (`chat_timeout`: `note_chat`, `post_chat_message`): 20 s for a cloud model,
  **2 minutes for a server on this PC** (`LOCAL_CHAT_TIMEOUT`, a loopback
  address: Yap's sidecar, Ollama, LM Studio) — Qwen3.5-4B on a processor
  takes ~25 s a turn, its slowest calls ~50 s; the Chat view says "Still
  working (12 s): the model runs on this PC…" after 8 s. Dictation cleanup and
  edit mode keep 20 s and never wait (the raw text is the fallback). A
  rejected tool call's error starts `TOOL_CALL_REJECTED`
  (`tool_call_rejection`: Groq's `tool_use_failed`, llama.cpp's "Failed to
  parse input"), which the tool loop recovers from. **Rate limits**
  (HTTP 429 — Groq's free plan allows 8,000 tokens a minute, and one
  tool-using chat answer can ask for more): calls someone is happy to wait for
  (`chat` — meeting summaries/digests/assist —, `note_chat`,
  `post_chat_message` — the chat tool loop) wait as long as the provider says
  (`rate_limit_wait`: `retry-after-ms`, `retry-after`, else "try again in
  10.98s" in the body; else 2 s, 4 s…) and retry, ≤ 30 s in all
  (`RATE_LIMIT_PATIENCE`, ≤ 4 retries); dictation cleanup and edit mode never
  wait (the raw text is the fallback). A limit that outlasts that is said
  plainly (`rate_limited_error`: "Groq is limiting how much Yap can ask for
  right now (its free plan allows 8,000 tokens a minute). Try again in 11
  s."), not as a raw `HTTP 429` body. A 400 about the temperature is asked
  again without one; a 400 about reasoning on a call with tools (OpenAI's
  GPT-6 take tools on Chat Completions only with reasoning off) again with
  `reasoning_effort: "none"`, once each.
- **`local_llm.rs`** — the on-device AI cleanup sidecar: runs **Mozilla llamafile**
  (llama.cpp, single-file OpenAI-compatible server) as a hidden child process on a
  free localhost port, serving **Qwen2.5-1.5B-Instruct** (Q4_K_M GGUF) when no
  other is picked (`pp_local_model` empty = `MODEL_FILENAME`) — or a curated
  one (`CURATED_MODELS`, incl. **Qwen3.5-4B**, bartowski's Q4_K_M, SHA-pinned)
  or **any user GGUF** dropped into `<data>/llm/` and picked via `pp_local_model`
  (Settings shows a model picker + "Open models folder"; switching restarts the
  sidecar). **Recommended for this PC** (`recommended_id`, from `gpu.rs`):
  Qwen3.5-4B where the graphics card can run it (2026-10-18, the model
  evaluations: 94–98% of the scenarios at ~1.2 s a turn on an RTX 5070 Ti, with
  Chat's tools; the 1.5B 79–81% without them), else the 1.5B; existing installs
  keep their model and are offered the 4B **once** (`upgrade_offer`: capable
  card, on-device in use, on the 1.5B, not turned down —
  `config.local_llm_upgrade_dismissed`), never downloaded by itself. The
  curated entry's `default` = the bundled file, `needs_gpu` = only recommended
  with a card. **One server set-up** for the sidecar and the evaluations
  (`server_args`): `-ngl 999 -c <context>`, **`--reasoning off`** (no thinking for
  any model: Qwen3.5 loops and times out with it on), **`--cache-ram 0`**
  (llama.cpp's host prompt cache off: with it on, a dictation's cleanup right
  after a Chat turn took 1.3–1.4 s instead of 0.13 s on the 1.5B — the slow
  copy back of the saved prompt — and a 4B server peaked at 6.7 GB on the
  processor; Qwen3.5-4B's peak is now 3.85 GB there, 3.7 GB on the card),
  `--ctx-checkpoints 4` (Qwen3.5's recurrent-state checkpoints, ~50 MB each),
  `-lv 4` (the log names the layers that went to the GPU). **The context**
  (2026-10-20, Nathan's ask: "show what context length it's set at, and let
  me set my own"): `config.local_context` (tokens; 0 = `DEFAULT_CONTEXT`
  8,192, today's behaviour) — one setting for the one sidecar, so Chat,
  dictation cleanup and the meeting notes share it. `effective_context`
  clamps it to `MIN_CONTEXT` 2,048..`MAX_CONTEXT` 1M and to what the model
  was trained for (`trained_context`: its GGUF header's `context_length`,
  `gguf.rs`, else `CuratedLlm::max_context`; a stale file falls back);
  `context_for(cfg)` is what `start()` passes as `-c`. `context_choices`:
  4K/8K/16K/32K/64K/128K/256K up to the model's maximum (and that maximum
  when it falls between; an unknown one stops at 32K). `context_view(cfg)`
  (`local_llm_status().context`): `{tokens, configured, default, max,
  serving, pending, onCard, roomMib, usedMib, options: [{tokens,
  estimateMib, over}]}` — each choice's memory from `gguf::estimate`, `over`
  = more than `card_room` (the card's own memory − 2 GiB for Windows, the
  apps on screen and Yap's speech model; never llama.cpp's "MiB free",
  which on WDDM ignores every other process), `usedMib` what the running
  server's log says (`log_facts`: the non-host `… buffer size` lines,
  `n_ctx`). **`local_llm_set_context {tokens}`** (the pickers): saved
  through `save_config` (0 for the default), `yap-local-context-changed
  {localContext}` (Settings' copy adopts it, or its wholesale auto-save
  would put the old one back), and a running sidecar on another context
  is restarted by **`restart_when_idle`**: never while a dictation records
  or is being transcribed/cleaned up (`pipeline_busy`: `dictation_state` +
  `Pipeline::is_busy`; pure `restart_step` Wait/Settle/Go, a 1.5 s settle
  after one), a newer change supersedes an older one (generation), one
  restart at a time, then `yap-local-llm-changed {pending, error}` (the
  pickers re-read) + `yap-chat-model-changed {scope: null}` (the chips).
  `start()` itself is one at a time now (`STARTING`: a second caller finds
  it up instead of spawning another llamafile). `budget_factor(context)` /
  `budget_factor_for(provider, base_url)`: whole 8K multiples, 1–4, only
  when the call really goes to the running sidecar — what the meeting
  notes' final call and the Notes view's Ask grow by (`meeting_summary.rs`
  `FinalBudget`, `commands::note_ask`). Debug-only
  `e2e_local_llm_serving {…, ctx, usedMib}` (the stand-in "restarts" on a
  change, `e2e_local_llm_restarts`) and `e2e_local_llm_busy {busy}` (a
  dictation as the guard sees one, no mic). **Model id** = the
  GGUF's name (`model_id`, `active_model_id`: `Qwen_Qwen3.5-4B-Q4_K_M`),
  what `effective_endpoint` sends and `tools::supports_tools` reads (it used to
  be `qwen2.5-1.5b-instruct` whatever ran, so a bigger model never got its
  tools). **Where it runs** (`placement`, from the log's `load_tensors:
  offloaded N/M layers to GPU`, read once ready; no line once loaded = the
  processor): a big model left on the processor (`slow_on_this_pc`) makes
  Settings say "Runs on the processor on this PC: answers will be slow — Qwen2.5
  1.5B is quicker" (+ Use / Download it). Debug-only `e2e_local_llm_serving`
  stages a running model's placement for the e2e suite (display only). Owns
  install (runtime + model download, SHA-256 verified, per-stage
  progress events naming the curated `model`, which the Chat model picker
  draws in that model's row; a test run downloads only from 127.0.0.1 — the
  debug-only `YAP_LLM_SOURCES` stand-ins, `{"runtime"|"<curated id>": {url,
  sha256}}` — and refuses anything else), process lifecycle (spawn/health-wait/kill + orphan cleanup at
  startup — only sidecars whose parent Yap is gone, via `procs.rs`, so a dev build
  never kills the installed app's; and **the model never outlives Yap**:
  `procs::end_with_yap` puts the sidecar — like the speaker engine
  (`yap.exe diarize`) and eSpeak — in a kill-on-close job object only Yap
  holds, so Windows ends it however Yap ends: Quit, a crash, End task, the
  updater's exit; never used for apps Yap opens, a link's browser or an
  installer), and `effective_endpoint()` which routes `llm.rs` to the sidecar when
  provider = "ondevice" (falls back to the configured endpoint if it's down).
- **`gguf.rs`** — a GGUF model's header, **read-only** (2026-10-20): magic,
  version 2/3, the metadata (a few scalars kept, everything else skipped by
  seeking — the tokenizer's ~250 k strings too: ~20 ms for the 4B's 10 MB
  header in a debug build) and the tensor list (where the data starts, the
  token embedding's size, whether there's an `output.weight` of its own),
  never the weights; absurd counts/lengths are errors, never a hang or a
  huge allocation. `Shape`: `context_length` (n_ctx_train), block count,
  heads, KV heads (one or per layer), key/value length,
  `full_attention_interval` (Qwen3.5: attention every 4th layer),
  `nextn_predict_layers`, the `ssm.*` sizes, the vocabulary. `model_file`
  caches per file (size + mtime). **`estimate(shape, file, ctx, on_card)`**,
  calibrated against llamafile's log on the RTX 5070 Ti — model on the card
  = the tensors' data less the token embedding (kept on the processor)
  unless the output shares it (no `output.weight`: copied to the card);
  KV = attention layers × KV heads × (k + v) × 2 bytes per token (one
  unified cache for 4 slots); recurrent state per slot (f32 conv + state,
  × 4 slots); compute = vocab × 512 × 4 + 2 KiB per token of context.
  Within 1% of the log (64K: 2% high): Qwen3.5-4B 3,821 / 4,637 MiB estimated vs 3,815 /
  4,641 at 8K / 32K, Qwen2.5-1.5B 1,472 / 2,192 vs 1,480 / 2,186
  (docs/model-evals.md "Context length"). Unit-tested on synthetic headers
  (`tests::Writer`, the two models' real shapes); `#[ignore]`d
  `this_pcs_models` reads `%APPDATA%\yap\llm` read-only.
- **`gpu.rs`** — the graphics card, for the on-device model recommended for
  this PC: DXGI (`CreateDXGIFactory1` → `EnumAdapters1` → `GetDesc1`, software
  adapters and Microsoft's Basic Render Driver left out), the card with the most
  memory of its own, cached; **`can_run_big_model`** = ≥ `BIG_MODEL_VRAM_MIB`
  (5.5 GiB, so "6 GB" cards count) of dedicated video memory — the 4B takes
  ~3.7 GiB on the card (model 2,863 MiB + 8k context's KV 256 + recurrent state
  201 + compute 495, read from the sidecar's log), beside Yap's speech model
  (~1–1.6 GB) and Windows (~0.5–1 GB); integrated graphics report little of
  their own (this PC's Ryzen Radeon: 2 GiB). Debug-only `YAP_E2E_GPU`
  ("none" | "<name>,<MiB>") stands in for the card in test mode. `#[ignore]`d
  `this_machine` prints this PC's cards.
- **`notes.rs`** — the AI Notepad's data layer (`notes.json`, camelCase). Stores:
  `content` (raw markdown, never overwritten by AI), `enhanced_content` (the
  Enhanced tab), `enhanced_at_hash` (OpenWhispr's `len+first-50` staleness
  marker), `folder` (string, seeded with Personal + Meetings), `participants`
  (attendee names, shown as chips and fed to prompts for attribution),
  `transcript` (meeting-recorder segments, time-ordered; `source` = the side,
  "you" (mic) | "them" (call); `speaker` = the voice (`speakers.rs`; empty =
  the side's own, read through `speaker_id()` — every old note reads as
  You/Them), `speaker_locked` (the user said who this line is), `start_ms`/
  `end_ms` (place on that recording's clock, ms; none on old lines and
  markers), `words` (`[start, end]` ms from `start_ms` per word of `text`,
  when the model gave times); `echo` marks
  speaker bleed, `dictated` a "You dictated here" marker — no text, `ts` where
  a mid-meeting dictation began; `is_talk()` = neither, what every summary
  input reads), `speakers` (the speaker map, see `speakers.rs`), `digests` (rolling meeting digests, see `meeting_summary.rs`),
  `breaks` (the transcript's length at each stop — the notepad's paused
  dividers; `add_break`, never twice at one place or at 0),
  `note_type` ("personal" | "meeting"), `source` ("manual" | "upload" |
  "meeting"), and `title_auto` (the title is one Yap made up — "Teams call ·
  5 Oct, 14:30" — so the AI meeting title may replace it; cleared for good by
  a title edit, so a typed title is never overwritten: `set_ai_title` checks
  under the store lock). `note_update` takes an `origin` (the saving window's
  label) and emits `yap-note-changed` with the editable fields (+ `speakers`;
  after a speaker edit also `lineSpeakers`, `[voice, locked]` per line), which
  keeps the Notes view and the meeting notepad in sync both ways (each sends
  only the fields it edited). `modify(id, f)` changes one note under the store lock
  (read and write in one step — the chat's `edit_note`; only a change bumps
  `updated_ts` and saves). Unit tests get an in-memory store per test
  thread (`cfg(test)`), never `notes.json`. `event` (optional) = the calendar meeting the note
  belongs to (`NoteEvent`: key, title, start/end, the invite's description,
  service), set by `link_event` (also the meeting's title as a real title,
  attendees merged, `note_type` meeting) and found by `find_by_event`.
  **Folders** are user-creatable (backend `notes_folder_create` command) with
  counts shown in the sidebar; notes filter by active folder. `folder_rename`
  (trimmed, non-empty, never a name another folder has in any case — a
  change of case is fine; every note in it goes along, the folder keeps its
  place) and `folder_delete` (its notes move to **Personal**: OpenWhispr
  deletes them with the folder, Yap never loses a note to a clean-up) both
  refuse **Personal** and **Meetings** (`PERSONAL`/`MEETINGS`,
  `is_protected_folder`: the default folder, and where the recorder, call
  detection, the bar and the calendar file meeting notes), work under the
  store lock, leave the notes' `updated_ts` alone and return a
  `FolderChange` (the folders, the ids moved); `find_folder` = a folder by
  any case, as the list spells it. Commands `notes_folder_rename(from, to)` /
  `notes_folder_delete(name)` emit `yap-note-changed` (origin "folders") per
  moved note + `yap-notes-changed`. **Actions** (named
  prompt fragments) are built-in protected set (seeded "Generate Notes",
  "Meeting Notes", "Action Items", "Action Plan") + user-created; `note_enhance`
  (commands.rs) invokes the **Actions engine**: picks an action by id, runs the
  Note Formatting scope's endpoint + the action's editable prompt (fallback →
  global cleanup endpoint), at temp 0.3 under an immutable base prompt:
  `llm::NOTE_BASE_PROMPT` (OpenWhispr's BASE_SYSTEM_PROMPT verbatim),
  `MEETING_NOTE_BASE_PROMPT` for meeting notes, or `ACTION_PLAN_BASE_PROMPT` for
  the Action Plan (found by `Action::kind == "actionPlan"`, so renaming is fine;
  its reply goes through `meeting_summary::postcheck_action_plan`). Built-ins
  seed via additive migration (by name, or by kind), so user edits to their
  prompts are never clobbered. For **meeting notes** the input comes from
  `meeting_summary` (the raw transcript when short, else digests + the latest
  stretch, after digesting any backlog); the Action Plan runs in Rust when a
  recording ends (`meeting_end.rs`: anything but Pause), through
  `commands::run_enhance` (the body of `note_enhance`, with step reporting).
- **`meeting.rs`** — the meeting recorder (OpenWhispr `meetingRecordingStore`
  port, fully offline): mic ("You") + **WASAPI loopback** ("Them" — cpal input
  stream on the default output device) on a dedicated capture thread; a worker
  cuts each source into ~15 s chunks **at the quietest 20 ms frame of the last
  4 s** (`media::quietest_frame`, not mid-word; every mid-meeting cut leaves
  the newest 400 ms buffered), transcribes them on the shared
  warm engine (`pipeline::EngineSlot`, taken per chunk so dictation still
  works; silence-gated), runs the **correction dictionary** over each chunk's
  text (`pipeline::apply_corrections`, both sides, so the echo check compares
  corrected with corrected), works off a backlog in ≤30 s chunks back to back,
  caps each source's buffer at 20 min (then drops audio + a "Transcription
  is falling behind" notice),
  dates each segment by its speech onset, places it on the recording's clock
  (`start_ms`/`end_ms`) with its words' times (`stt::transcribe_timed` →
  `speakers::word_times`, lined up with the corrected text; `Heard::line`),
  flags **speaker echo** (a "you"
  chunk whose words repeat "them" in order AND whose 20 ms loudness envelope
  follows the call 0–600 ms later — the timing test keeps a headphone user's
  own reply; flagged segments are kept, hidden, left out of summaries), and
  `ingest`s segments: persist to `notes.transcript`, emit `yap-meeting-segment`,
  kick the rolling digests. Each source is a `Captured` (`AudioBuf`) on a
  **session clock** (positions in samples); `push_audio` is the one push path
  (device callbacks and the e2e player). **Dictation kept out of "You"**: the
  mic's `DictationGate` reads `pipeline::dictation_state()` per callback
  (lock-free) and, while a hotkey dictation records, appends silence instead of
  the audio (same length, so times and the echo check stay aligned); a new
  dictation also blanks the newest ~340 ms already buffered (its pre-roll +
  40 ms), the callback that sees it end is blanked too, and each blanked span
  leaves a `dictated` marker segment (dated where it began; one per span,
  however the chunks are cut). **One clock**: `keep_in_step` (every 200 ms)
  fills a side whose device went silent while the other moved on ≥ 0.5 s
  (loopback with nothing playing, a device switch) with silence up to the
  other. **Follows the default output**: the capture thread reads Windows'
  default render endpoint id every 2 s (`IMMDeviceEnumerator`, eConsole like
  cpal; `OutputFollower` decides) and reopens the loopback stream on a new one
  (old stream first, a failure keeps the old one and retries; logged).
  **Quiet sides**: `QuietWatch` (every 200 ms) — one side heard nothing above
  the silence gate for 3 min of meeting while the other talked ≥ 10 s →
  "Yap can't hear the call" / "…your mic" once per quiet stretch ("Yap
  hasn't heard the call/your Teams call for 3 minutes. If it plays through a
  headset, make that Windows' default output device." / "…your mic…Settings
  → General"), in one place (`warn` → `meeting_guard::notice`: the main
  window while it's focused, else the Yap bar, else a Windows notification;
  "Transcription is falling behind" the same way); a dictation counts as the
  mic heard, not as talk. `activity()` (the recording's note, its audio
  clock in ms — captured audio, standing still while none arrives — and a
  talk counter: both sides' talk samples plus talk segments ingested) is
  what `meeting_guard`'s quiet-meeting watch reads each second; the
  `Session` keeps both sources for it. Commands: `meeting_start`/`meeting_stop`/
  `meeting_state` (signatures unchanged). Test mode opens no audio device (WAVs
  from `YAP_E2E_MEETING_AUDIO`, or silence; `e2e::e2e_meeting_feed`; the
  player also stages a dictation and an output switch, see `e2e.rs`).
  **Speaker diarization's tee**: with "Tell speakers apart" on and its model
  installed, `start` opens a `diarize::Session` before the capture and gives
  the "Them" `Captured` its `Tee` — every sample appended (device audio,
  `fill_if_stalled`'s silence) also goes to the engine, so its clock is the
  session clock; the worker calls `Session::finish` after its last chunk and
  before the final `yap-meeting-state` (so the summary starts after the final
  labels). Off, there is no tee. **The labels on lines**
  (`speakers::labels`): `start` registers the recording once its capture
  runs (`labels::begin(note, Session::id)` — its lines are those from the
  transcript's length then), `ingest` lets lines the latest labels cover get
  theirs at once (`after_ingest`), and the worker, after `finish` (or, in a
  test run with no engine, `diarize::finish_scripted`), calls `labels::end`
  (an engine that never sent final labels: labelled as far as it went).
- **`diarize.rs`** (+ `diarize/`) — **telling the voices on the call side
  apart** (beta, off by default; the spike: yap-private
  `docs/diarization-spike.md`). Engine: **Nemotron-3-Diarization** (q8_0 GGUF,
  107 MB, OpenMDW-1.1) on **NeMo-Speech.cpp 0.2.0** (CPU build, 5.5 MB zip,
  Apache-2.0), streaming with the **30 s preset** (`v3-offline`) on 4 OpenMP
  threads: ~2.4% of real time, labels ~10 s (≤ 21 s) behind the audio, final
  at the stop, no audio kept. **`yap.exe diarize --runtime <dir> --model
  <gguf> [--preset] [--every-ms] [--no-dither] [--seed]`** (main.rs →
  `child.rs`): its own process (a 0.2.0 runtime's asserts abort it; its ggml
  isn't whisper.cpp's), below-normal priority, no window; loads
  `nemo_speech_asr_c.dll` with `LoadLibraryExW(…, DLL_LOAD_DIR |
  DEFAULT_DIRS)` (`libloading`; its ggml + MSVC runtime from that folder),
  runs in the model's folder (the C ABI's narrow path), adds **−60 dBFS
  white-noise dither** (`Dither`, seeded; exact digital silence breaks
  Nemotron), pushes ≥ 0.25 s at a time. **Wire** (`protocol.rs`, `PROTOCOL`
  1): stdin = frames `tag:u8 len:u32le payload` — `'A'` f32le samples, `'S'`
  a u32 count of silence; EOF = finish. stdout = JSON lines: `ready
  {protocol, speakers, frame_ms, runtime, load_ms}`, `segments {final,
  audio_ms, labelled_ms, compute_ms, lag_ms_median?, lag_ms_max?, segments:
  [{start_ms, end_ms, speaker}]}` (the whole list each time; speakers 1…8 by
  first appearance, stable — `timeline::SpeakerOrder`) every 5 s of audio
  once labels moved on, then `final: true`; or `error {message}`. stderr →
  the log at debug. **Yap side**: `Session::for_meeting(note_id)` (config +
  installed) → `tee()` → `finish()` (≤ ~20 s, normally < 1 s; dropped
  unfinished = cancelled, nobody told). A feeding thread batches the tee's
  frames into the child's stdin; a child that dies is started again
  (`MAX_RESTARTS` 2) at its place on the clock as a new `stream` (a voice is
  `(stream, speaker)`; the gap is in `gaps`); past 5 min of queued audio the
  tee sends silence (memory bounded, clock right). **The API for the speaker
  map**: `diarize::on_segments(|tl: &Timeline| …)` — every few seconds while
  recording and once with `done: true`; `diarize::timeline(note_id)` — the
  latest. `Timeline {note_id, session, segments: [Segment {start_ms, end_ms,
  speaker, stream}], audio_ms, labelled_ms, done, restarts, gaps, error}`, ms
  on that recording's session clock ("Them" sample position / 16; a resumed
  meeting is a new session, new numbers; `Session::id` = its `session`).
  `speakers::labels` (registered at setup) puts them on the "Them" lines;
  the final handler runs before `finish` returns. **Models** (`registry.rs`): Nemotron-3 (Recommended, the
  default; HF `nvidia/Nemotron-3-Diarization` pinned to commit `f667ed73…`)
  + its runtime package (NVIDIA's v0.2.0 release zip: `bin/*.dll` minus the
  CLI/llama/nmt/tts, `share/licenses/` → `licenses/`), and the sherpa-onnx
  fallback (pyannote seg-3.0 + ERes2Net + sherpa 1.13.8, pinned) listed as
  **"For phone-only calls — coming later"** (`available: false`, no engine).
  Install = download (resume), size + SHA-256, unpack into
  `<folder>.installing/`, rename into `<data>/models/diarization/<folder>/`,
  plus a `NOTICE.txt`; installed = every required file there at its pinned
  size; delete takes the runtime too when nothing else uses it (refused
  while a recording uses it). Commands `diarization_status` / `_download`
  (`diarization-download-progress {id, stage, percent, downloadedBytes,
  totalBytes}`, then `diarization-changed`) / `_delete`; debug-only
  `diarize_wav {path, speakers?}` (the real child on a file, a second at a
  time; `speakers` caps voices afterwards — the engine takes no count),
  `diarization_timeline {noteId}` and `e2e_diarize_timeline {segments,
  labelledMs, done, error?}` (test mode: a scripted timeline for the
  recording now running, session `E2E_SESSION`; a `done` one is held until
  the recording stops — `finish_scripted` — where the engine's final labels
  come). Debug-only env: `YAP_DIARIZATION_MODELS`
  (another models folder), `YAP_DIARIZATION_SOURCES` (stand-in URLs + SHA-256s,
  JSON per package); a test run only fetches from loopback. Logs: counts and
  times, never audio. Measured (`tests/diarize_child.rs`, the spike's TTS
  meetings, 0.25 s collar): 0.4–2.7% on 2–3 voices and dial-ins, 2.3% on 4
  with overlap + noise, 0.3% / 0.7% at 30 / 60 min, 15.9% on 4 voices made
  from two (as the spike), ~50% on phone-only audio (the fallback's job).
- **`speakers.rs`** — **who said what** in a meeting: the note's speaker map
  (`Note::speakers`: voice id → `Speaker {name, email, source: user |
  platform | voice | calendar | ai, locked, mergedInto, not}`) and the names
  it puts on lines everywhere. Voices: "you", "them" (the call, not told
  apart), later "them-1"… (the voice separation), "p-1"… (a person the user
  named on some lines). **Precedence** (OpenWhispr's `speakerAssignmentPolicy`
  by behaviour): the user's name is locked against every automatic relabel,
  and so is a line they assigned (`speaker_locked`); otherwise call app >
  voice match > calendar = AI guess, never downwards; "Not Priya" (`not`)
  keeps that name off the voice for good. A calendar/AI name is a guess:
  displayed "Priya?" (`display`), told to the prompts as a guess
  (`speakers_line`). Edits (`rename` everywhere — to another voice's name =
  merge; `rename_lines` — a voice by that name, else a new person, lines
  locked; naming a line after a guess confirms it; `merge` — the more trusted
  name wins, later lines of the merged id follow; `not`; "You" never renamed
  or merged) are the commands `speaker_rename` / `speaker_rename_lines` /
  `speaker_merge` / `speaker_not` (return the note, emit `yap-note-changed`
  with `lineSpeakers` + `yap-notes-changed`); `suggest` is the automatic
  path. **1:1 calls** (`calendar_guess`, from `notes::link_event`): exactly
  one other invitee (no you, rooms or declines) → "Them" named after them,
  source calendar. `known_people` (attendees + map names) is what owner
  checks accept. **Line timing**: `word_times(text,
  timed)` (LCS line-up of the model's words with the corrected text; a word
  the dictionary changed takes what it replaced), `line_timing`, and
  `split_line(seg, &[Turn {start_ms, end_ms, speaker}])` — a line split at
  its words where the speaker changes (most overlap, else nearest ≤ 1.5 s,
  else neighbours; one-word blips < 400 ms smoothed), in proportion without
  word times; a user-locked line, marker or unplaced line comes back
  unchanged. Lines' `start_ms`/`end_ms` are on the same session clock as
  `diarize::Timeline` (sample position / 16, 0 at each recording's start).
  **`speakers/labels.rs` — the engine's voices on lines**: a recording
  (`begin(note, session)` from `meeting::start`: its lines = from the
  transcript's length then) takes its session's timelines (`on_segments`;
  in a test run with no engine, `E2E_SESSION`'s): each labels that
  recording's "Them" lines that are talk, placed, settled (`end_ms ≤
  labelled_ms`; all once `done` without an error), unlabelled and not
  `speaker_locked` (never You/echo/markers/old lines) — `split_line` with
  the turns near it, the parts replacing the line under `notes::modify`
  (`notes::replace_line`: digests and breaks after it move along; a line
  with no voice near stays "Them") — then emits `yap-note-changed` (origin
  "speakers": the windows take only the lines and the map, never the
  title/notes being typed) with `lineSpeakers` + `lineSplice {at, remove,
  lines}` + `breaks` (windows splice, else refetch; `hasLine` drops a
  segment event for a line they already have). `after_ingest` labels lines the latest
  timeline already covers; `end` handles an engine that never sent its
  final labels (as far as they went). **Voice ids**: engine voice `(stream,
  speaker)` → "them-N", N numbered on from the note's highest "them-N" in
  the order voices first get a line (`VoiceIds`; a restarted engine's
  voices and a resumed meeting's are new numbers, never merged by
  themselves). **The 1:1 rule** (`settle_them`, each pass, per recording):
  "Them"'s name (a calendar guess, or the user's) goes to the recording's
  voice when it found exactly one (a guess stays a guess); with several, a
  guess goes to the voice with the most words (the first to qualify keeps
  it while recording; moved at the end), never one with its own name or a
  "Not" for it, and a user's name stays on "Them"; "Them" keeps its name for
  unlabelled lines (`voices` leaves out a line-less "Them" once voices are
  told apart; `speakers_line` says a name once). An error at the end →
  `yap-meeting-speakers {noteId, message}` (the notepad: "Couldn't tell
  speakers apart this time.") + an info log. Unit-tested; e2e with scripted
  timelines; measured with the real engine + Parakeet on the spike's TTS
  meetings (`diarization-real.spec.js`, 2026-10-18): 93.6–99.5% of words on
  the right person, every call line labelled, a name ~15 s (≤ ~23 s) after
  the words.
- **`meeting_summary.rs`** — meeting summaries that keep up with long meetings
  (map-reduce, the map done while recording). A bigger on-device context
  lets the final call read more (`FinalBudget::for_endpoint`: the single
  pass and the final input × `local_llm::budget_factor_for`, 1–4 — 14,000 /
  18,000 at 32K — only on the running sidecar; `prepare_final` and
  `compose_meeting_input_within` take it; the live digests and the helpers
  keep the 8K budgets). Every ~10 min of new talk
  (2,000 est. tokens, or 10 min with ≥ 250) a background **digest** (key
  points, decisions, action items `Owner: task (due: …)`, open questions;
  `llm::MEETING_DIGEST_PROMPT` + a one-shot; ≤ 2,600 tokens of transcript + the
  meeting so far ≤ 400) is written with the Note Formatting endpoint and
  stored on the note (`yap-meeting-digest`); owners must be You/Everyone/an
  attendee/a name in the speaker map/a name said in that part (else
  Unassigned; `speakers::known_people`) and deadlines must have
  been said (`check_digest`). **Speakers**: every input labels lines with the
  speaker map's names (`transcript_lines(&note.speakers, …)` →
  `speakers::prompt_label`: "You", "Them", "Priya", "Speaker 2") and, when
  anyone is named or voices are told apart, adds "Speakers besides You: …"
  under "Attendees" (`people_lines`; a calendar guess is said to be one);
  plain You/Them meetings read exactly as before. On a **local** endpoint digests wait for
  dictations to finish and are dropped mid-call when one starts
  (`unless_busy` — closing the connection cancels it in llama.cpp), and
  replies are capped. `prepare_final` (one digest turn at a time,
  `DIGEST_TURN`) digests any backlog with `yap-meeting-summary-progress`;
  `compose_meeting_input` keeps the final input ≤ 4,500 tokens whatever the
  length (raw transcript when ≤ 3,500); `postcheck_action_plan` moves invented
  owners' tasks to Unassigned, strips made-up deadlines and restores dropped
  digest tasks; `ask_context` bounds a long meeting for the Ask bar and the
  chat `get_note` tool. Every input reads talk only (`TranscriptSegment::
  is_talk`: no echo, no "You dictated here" markers; markers weigh 0 tokens in
  the digest windows). The notepad's helpers live here too: `catch_up_input`
  ("What did I miss?": what was said since segment `since`, raw when ≤ 2,400
  tokens, else the digests covering it + the latest ≤ 1,400; context ≤ ~700),
  `meeting_ask_messages` (a follow-up question, ≤ ~3.3k), `title_messages` +
  `clean_title` (the AI meeting title: the start of the meeting, one-shot,
  ≤ 60 chars), the one-tap helpers' inputs — `questions_input` (the latest
  ≤ 1,200 tokens of talk + digests of what came before ≤ 500), `recap_input`
  (the action plan ≤ 1,200 + `ask_context` of the meeting: ≤ 1,800 with no
  plan, else 500 for the email / 800 for the gaps), both with attendees, the
  invite ≤ 200 and typed notes ≤ 300, `None` (too little: < 20 words and
  nothing else) — `helper_messages`, `clean_email` (the email as plain text:
  preamble, fences and markdown out, "Subject: " first, `email_subject` when
  missing) — every helper call ≤ ~3.4k with its reply; and `chat_beside_dictation` (on a local endpoint: wait for
  dictation to finish, drop the call when one starts and ask again — never
  slows a dictation). See [`docs/meetings.md`](./docs/meetings.md).
- **`meeting_end.rs`** — the end of a meeting recording, wherever it was
  stopped from (notepad Stop, Notes view "End meeting & summarise", the Yap
  bar, call detection, an automatic stop). It follows the recorder through
  `yap-meeting-state` (Rust listener; `meeting.rs` is untouched; the final
  one comes after the speaker engine's last labels are on the lines, so the
  title and the action plan read the voices): a start
  opens the notepad (`notepad::on_meeting_started`); every stop (pause or end)
  marks its place (`notes::add_break` → `yap-meeting-breaks {noteId,
  breaks}`, the notepad's ⏸ divider); a stop is the **end** of
  the meeting unless it came through `meeting_pause` (the Notes view's Pause).
  The end: fewer than 20 words of speech (`MISTAKE_WORDS`) and nothing typed →
  `yap-meeting-ended {noteId, mistake, words, surface}` and no summary (the
  window Rust picks — where the stop came from (`meeting_end`'s `origin`) if
  on screen, else the notepad, else the main window — shows **"Started by
  mistake?"** Keep / Discard; with no Yap window on screen, a **Yap bar card**
  asks instead (`meeting-mistake`; Keep, ✕ or a minute = keep; withdrawn on
  resume or delete; the bar off: nobody asks); `meeting_discard` deletes the
  note, emits `yap-note-deleted`, hides the notepad; `meeting_delete` — the
  notepad's ⋯ → Delete — pauses a recording note first, then the same);
  otherwise the AI title (if due) and
  the **action plan job** (`start_summary`: one per note, `yap-meeting-summary
  {noteId, run, state: running|done|error|needsAi|nothing, step, steps, error}`,
  "Step 2 of 3"; `meeting_summary_status`; `meeting_summarise` runs it again —
  Generate summary, Retry, the Notes view's Action plan). Commands
  `meeting_end(origin)`, `meeting_pause`, `meeting_summarise`,
  `meeting_summary_status`, `meeting_discard`, `meeting_delete`; Rust callers
  use `end()` / `pause()`.
- **`meeting_assist.rs`** — the notepad's AI helpers, on the meeting's model
  (Note Formatting scope, else cleanup), bounded for an 8k local model, via
  `chat_beside_dictation`: **"What did I miss?"** (`meeting_catch_up(noteId,
  since, question?)` → `{answer, nothingNew, upto}`; with nothing new since
  `since` it answers "Nothing new since you last looked." without a model;
  `question` = a follow-up from the same mini chat), the **one-tap helpers**
  (`meeting_assist(noteId, kind)` → `{answer, plain, tooLittle}`; `kind` =
  `Helper`: `questions` — "Questions to ask", 3–5 for now, while recording;
  `followUpEmail` — "Follow-up email", a recap to send the attendees,
  `plain` text, `clean_email`ed; `blindSpots` — "Gaps and risks": not
  decided / missing owners or dates / risks nobody followed up; inputs from
  `meeting_summary::questions_input` / `recap_input`, talk only; too little
  to go on → `Helper::too_little()` without a model; reply caps 300 / 600 /
  500 on a local model) and the **AI meeting
  title** (`maybe_title`, on every `yap-meeting-segment` and at the end: once
  the meeting has 150 words of speech or a digest — or, at the end, 20 words
  — and its title is open to the AI, `llm::MEETING_TITLE_PROMPT` → `clean_title`
  → `notes::set_ai_title` → `yap-note-changed {origin: "ai"}`; ≤ 3 tries, 2 min
  apart).
- **`notepad.rs`** — the **meeting notepad** window (label `notepad`, static in
  tauri.conf.json, created hidden + unfocused so `capture.rs` always finds it
  and opening it never takes the foreground — tao keeps the don't-focus marker,
  so every `show()` is `SW_SHOWNOACTIVATE`; close = hide, the recording goes
  on; denylisted from the window-state plugin; gets the DWM-cloak init).
  `on_meeting_started` (from `meeting_end`) switches it to the recording note
  (`yap-notepad-note`) and, with `meeting_open_notepad` (default on), docks it
  to the right edge of the work area — full height, `notepad_width` = 30% of
  the width clamped to 400–800 px at 100% (scaled) and ≤ half (768 px on a
  2560 px screen, Wispr's size) — on the monitor
  with the call's window, else the cursor's; already on screen, it stays put.
  **Split the screen** (`meeting_split_screen`, default off): during a
  detected call, `pick_call_window` (unit-tested) finds the call app's main
  window — visible, top-level, not Yap's process, not minimised/tool/owned/
  cloaked, ≥ 200×150, titled; a browser window whose title shows the meeting
  first, else the largest — from `meeting_detect::call_window_exes()` (its
  exes, or the browser a meeting tab is in); a maximised window is restored first, then
  moved left of the notepad (`SetWindowPos`, async, invisible borders added
  back via `DWMWA_EXTENDED_FRAME_BOUNDS`). Never in test mode. The notepad's
  **split button** does it now (`notepad_split` → `split_now`; test mode: an
  error, "Test runs never move other apps' windows."), and **hovering** it
  shows a glass outline over the call's slot (`notepad_split_preview(show)`:
  an on-demand `split-preview` window — transparent, borderless,
  click-through, unfocusable, always on top, the slot + 24 px for its shadow,
  `preview_rect`; a generation counter keeps a late show from sticking; never
  in test mode; in `capture::MEETING_WINDOWS` and the window-state denylist).
  `open(app, noteId)` (the Notes view's "Notepad", the Yap bar) docks if
  hidden and focuses. Commands `notepad_open`, `notepad_state`,
  `notepad_split`, `notepad_split_preview`, `notepad_consent_message` (saves
  `meeting_consent_message`, emits `yap-consent-message-changed` for Settings'
  config copy); `yap-notepad-visible` tells the page when Rust shows/hides it.
- **`meeting_detect.rs`** — **call detection** (OpenWhispr
  `meetingDetectionEngine.js` port): notices a call starting (Teams, Zoom,
  Google Meet, Slack huddles, Discord, Webex, GoTo, WhatsApp/Signal/Telegram…),
  offers to take notes, and offers to stop and summarise when it ends. Signal =
  Windows' per-app **microphone consent store** (`HKCU\…\CapabilityAccessManager\
  ConsentStore\microphone`: packaged apps + `NonPackaged\<exe path, \ → #>`;
  `LastUsedTimeStop == 0` = on the mic now, ignored if it predates this boot),
  read for a known-call-app allowlist only (`APPS`); a browser counts when one
  of its windows shows a meeting (Meet/Teams/Zoom… in the title; the call
  sticks while you switch tabs). Yap's own mic use never counts (`yap.exe`
  isn't a call app — it always holds the mic for the pre-roll). Event-driven:
  a thread blocks in `RegNotifyChangeKeyValue`; the detector rescans every
  2 s only while a call is starting/live/ending (60 s safety net; 5 s polling
  if the watch fails). Debounce: starts after 5 s on the mic (20 s for chat
  apps — voice notes), ends after 15 s off it (a device switch or rejoin is a
  gap). **Per-app choice**: each `APPS` entry has `asks_by_default` (work apps
  — Teams, Zoom, Meet, Webex, Slack, GoTo, Whereby, Jitsi — yes; Discord,
  WhatsApp, Signal, Telegram no); the person's overrides live in
  `config.meeting_detect_apps` (app id → bool; effective = override, else the
  default, so apps added later start at theirs). A switched-off app's calls
  are still tracked but get no prompt (start or end) and no tray item. Asks
  **once per call**, never while already recording, in one of two styles
  (`config.meeting_detect_style`): **"popup"** (default) — in **one place**
  (`notice::place_for`): a sticky in-app toast while the main window is
  focused, else a card on the **Yap bar** (`native::bar_card`: Wispr's
  "Meeting detected" card — the app's icon (its own as installed,
  `app_icons.rs`, else its mark), "Teams call" over "● Now", split button
  Record notes | ^ Not now / Don't ask for Teams, corner ✕; the call ending
  asks in a notice card), else — the bar off or hidden — a silent Windows
  notification via `win_toast.rs`, else the in-app toast if the window is on
  screen (also when Windows refuses the notification); focusing the window
  moves a pending prompt in-app (`on_main_window_focused`, off the bar), and
  a start prompt left alone **fades after 30 s as "Not now"** (OpenWhispr's
  auto-dismiss; `fadeMs` in the prompt view, the toast's `onExpire`; it
  answers `ignore`, a "Not now" that fatigue counts apart) — or **"quiet"** —
  no in-app toast, the notification goes
  silently into the notification centre (`win_toast::post_quietly` →
  `SetSuppressPopup`) and stays out of the window unless its body is clicked.
  Both styles: start prompts carry a third answer, **"Don't ask for Teams"**
  (`never`: saves the override, withdraws the prompt everywhere, emits
  `yap-meeting-detect-choice {app, asks, confirm}` so Settings adopts it and,
  window on screen, a "Won't ask about Teams calls" toast links to Settings →
  General → Meetings); the end prompt follows the style. Prompts wait for a
  dictation to finish (+2.5 s); "Not now" snoozes that app for 5 min
  (OpenWhispr's cooldown). Opt-in **"Start notes automatically after 10
  seconds"** (`meeting_auto_start`, Wispr's countdown; off by default because
  nothing records without a click): a start prompt that's sure to be seen (a
  focused window's toast, or the bar's card) counts down (`auto_start_at` in
  the prompt view; Start now / Not now / Esc cancel), then runs "Record notes"
  (`auto_start_later`, after any dictation); the same app's call again within
  10 min (a rejoin, a reload) is asked about without one. In a test run the
  suite says whether the main window counts as focused (`notice::window_view`:
  by default while it's on screen; `e2e_focus`). **Call-prompt fatigue**
  (`notice::Fatigue`, Wispr's by behaviour; `<data>/notices.json`): three
  "Not now"s in a row, or five start prompts left to fade, and "Stop asking
  about calls?" is offered once (`offer_to_stop`, an alert with **Stop
  asking**); 35 start prompts with no meeting ever recorded and call
  detection switches itself off with "Yap stopped asking about calls" +
  **Turn back on** (`stopped_asking`); Record notes / Don't ask reset the
  streaks; someone who has recorded a meeting (≥ 20 words) is never asked.
  `set_detection` saves the switch and tells Settings
  (`yap-meeting-detect-choice {enabled}`). **Tray**: while a call of an asked-about app is live
  and nothing records, `tray_item()` puts "Record this Teams call" at the top
  of the idle menu (`meeting_record:<call id>` → `on_tray_record`, same as
  Record notes); `sync_tray` calls `tray::refresh` when it changes. **Record
  notes** → `notes::create` ("Teams call · 5 Oct, 14:30", folder Meetings,
  source "meeting", `set_title_auto` so the AI title may replace it) +
  `notes::mark_meeting` + the `meeting_start` command (which opens the
  notepad); opens the note if the window is visible — that's the
  confirmation, no toast on top — (else a "Taking notes…" bar card or Windows
  notification); a failed start deletes the note and says why.
  **Call ended** while recording it → "Stop and summarise?" — by default never
  an auto-stop (OpenWhispr doesn't, and the mic can't tell an ended call from
  a rejoin, breakout room or phone hand-off): `stop_and_summarise` ends the
  meeting (`meeting_end::end` — Rust writes the Action Plan) and shows it
  where the person looks: the notepad if it's on screen, the note in Notes
  (`yap-meeting-open-note {noteId, stop: false}`) if the main window is, else
  the main window comes up on it. Opt-in **"When a call
  ends: Stop and summarise automatically"** (`config.meeting_call_end` "ask" | "stop",
  Wispr's "Stop Notetaker when a call ends"; `at_call_end`) stops and
  summarises without asking, via `meeting_guard::stop_after_call` (same rule:
  only apps Yap asks about; no window comes up). For the notepad's split
  screen: `call_window_exes()` (the live call's exes) and `title_shows`.
  **Calendar**: before asking, `calendar::claims_call(app)` (under this
  module's lock; it takes only the calendar's) — a call during a meeting
  whose reminder card is up or was answered isn't asked about, a snoozed
  card comes back instead; each call start also goes to
  `calendar::on_call_started` (outside the lock: the switch card for
  back-to-back meetings). Snapshot
  `meeting_detect_status` + `yap-meeting-detect` (`{enabled, style, apps,
  calls, prompt}`); answers `meeting_detect_respond(promptId,
  record|dismiss|never|stop|keep)`; debug-only `meeting_detect_simulate(appId,
  active, fadeMs?)` (no debounce; `fadeMs` shortens that call's fade) for the
  e2e suite. Gated by `config.meeting_detection` (default **on**, as
  OpenWhispr's `notifyMeetingDetection`; `sync()` on every config save re-reads
  the style and per-app choices too). `mic_records()` hands `app_icons.rs` the
  call apps' consent-store keys (exe paths, package families) and their
  `LastUsedTimeStart`, nothing else. Lock rule: never touch windows/WinRT
  while holding its state lock (window getters wait on the main thread). See
  [`docs/meetings.md`](./docs/meetings.md).
- **`app_icons.rs`** — the call apps' **own icons as installed on this PC**
  (`meeting_app_icons` → `{appId: data URL}`, off the main thread) for
  Settings' "Ask about calls in" list and the bar's call card
  (`CallAppIcon.svelte`). Read-only and local (nothing launched, no network;
  only icon and manifest files, plus the consent-store keys call detection
  reads): **desktop apps** from the exe paths in the microphone record
  (`meeting_detect::mic_records`, most recently used first), then the usual
  install folders (`KNOWN_PATHS`; Squirrel's `app-*` → the newest with the
  exe) — the exe's icon at 48 px (`SHDefExtractIconW` → `GetIconInfo` →
  32-bit `GetDIBits` → straight RGBA, alpha or mask → PNG via the `png`
  crate, already built for arboard); **packaged apps** (new Teams, WhatsApp,
  Telegram/Slack from the Store) from the consent store's family names +
  `FAMILIES` → `GetPackagesByPackageFamily` + `GetPackagePathByFullName` →
  `AppxManifest.xml`'s `Square44x44Logo` (a small tag scanner; the listed
  application's, else the store logo) → the variant made for a light
  background at ~48 px (`targetsize-48_altform-lightunplated`; high-contrast
  and dark-theme ones last, folder qualifiers too). Cache `<data>/icons/
  <app>.png` + `sources.json` (path, size, mtime, when made): remade when the
  source changes or after a week, dropped when the app's gone. With call
  detection off the microphone record isn't read. Browser-only apps (Meet,
  Whereby, Jitsi) have none: their bundled mark. Test mode reads no
  installed apps: debug-only `YAP_E2E_APP_ICONS` names a folder of stand-in
  `<app>.png`s. An `#[ignore]`d unit test (`this_machine`, `YAP_ICONS_OUT`)
  runs the real thing read-only and writes the cache somewhere to look at.
- **`meeting_guard.rs`** — guard rails around a meeting recording (Phase 8,
  Wispr Flow's Notetaker settings) + their notices. **Maximum recording
  length** (`config.meeting_max_minutes`: 60/120/180/240, 0 = no limit,
  default **120**, as Wispr): a watcher thread per recording (follows
  `yap-meeting-state`; Settings re-read every second) warns 5 min before —
  "Notes stop in 5 minutes" + **Keep going** (another hour;
  `meeting_keep_going`), in one place (`notice::place`): a sticky in-app
  toast while the main window is focused (`yap-meeting-limit` with `inApp`,
  `meeting_limit_status` after a reload), else a Yap bar card, else a
  Windows notification — then stops and writes the action plan. A
  warning always gives the full 5 min (limit lowered mid-meeting, PC slept
  through it); raising/removing the limit takes it back (`step`/`kept_going`,
  pure + unit-tested). **Quiet meetings** (`config.meeting_end_when_quiet`,
  "End notes when the meeting goes quiet", default **on**, as Wispr), on
  the same watcher: after 10 min with nobody speaking (from the start or the
  last speech) "Ending notes in 30 s…" counts down — **Keep going** (another
  10 min) / **Stop now**; anyone speaking or the setting going off cancels
  it — then `stop_and_summarise` + a notice that says why ("Stopped: the
  meeting went quiet"). Speech = `meeting::activity()`'s talk counter (audio
  above the recorder's silence gate on either side, a dictation's blanked
  span not; plus transcribed talk segments, the e2e feed's too); quiet time
  runs on the **meeting's audio clock** (`clock_ms`: captured audio, so a PC
  asleep or a dead device never counts). **Check-ins** "Still in your
  meeting?" (Stop and summarise / Keep recording): 10 min after the end of
  the note's calendar meeting (`notes::event_of`; < 24 h long, recording
  started before then) and 1 h into any recording; each once per
  recording, held back while anyone spoke in the last 3 min, never while
  the length warning is up or due within 10 min (`Watch::length_busy`),
  gone unanswered after 2 min; the countdown replaces one, the length
  warning's Keep going counts as one. Rules pure + unit-tested
  (`QuietWatch::step`, `pick_surface`). **Where they show**
  (`surface_now`): the notepad if on screen, else the main window if
  focused (both as a sticky toast: `yap-meeting-checkin {prompt}` with
  `surface` = the window label, `meeting_checkin_status` after a reload,
  `meetingCheckIn.js`), else a Yap bar card (id `meeting-checkin`; the
  countdown as its headline, `Countdown.headline`), else a Windows
  notification (tag `meeting-checkin`); answers `meeting_checkin_respond {id,
  answer: keep|stop}`; the stop notice goes to the same surface
  (`Notice.surface`). The countdown's ✕ only hides it; a check-in's ✕ =
  Keep recording. **The meeting shortcut** (`config.meeting_hotkey`,
  default `kb:alt+win+77` = **Win+Alt+M**, as Wispr; "" = unbound):
  `start_or_stop` — recording → stop and summarise; a live call (any app) →
  `meeting_detect::on_tray_record`; else a new "Meeting · 5 Oct, 14:30" note
  (Meetings) + `meeting_start`, opened if the window is visible, and a
  "Taking notes" notice. 1 s debounce. Global hook event
  `meeting-key-pressed`; the in-page fallbacks (Settings, the notepad) call
  `meeting_shortcut { origin }` with their window label, so a question about
  the meeting ("Started by mistake?") shows where the key was pressed.
  `notice` (`yap-meeting-notice` `{kind, title, body, icon, variant,
  settings, noteId}`) carries the screen-share tip, "Stopped at 2 hours",
  "Teams call ended · writing your action plan", "Taking notes" and the
  recorder's warnings (kind `warning`) in **one place** (`notice::place`:
  the main window while it's focused, else the Yap bar via `notice_native`,
  else a Windows notification, else the window on screen); one addressed to
  a window (`surface`) shows there; the same one again within 10 min isn't
  shown (`notice.rs`). **Every stop goes through
  `stop_and_summarise`** = `meeting_end::end` (Rust writes the action plan;
  no window comes up — the notepad shows it when open). Debug-only `e2e_meeting_limit
  {limitMs, warnMs, keepGoingMs}` and `e2e_meeting_checkins {quietMs,
  countdownMs, recentMs, afterEndMs, hourMs, fadeMs, marginMs}` (which also
  turns on the test player's live mic, `e2e::set_live_mic`) shorten the
  timings for the e2e suite.
- **`capture.rs`** — hides Yap's meeting windows from screen capture and
  sharing: while a meeting records and `config.meeting_hide_from_capture` is
  on (default **on**, Wispr's "Don't show Notepad and Flow Bar in screen
  capture"), the windows labelled **`overlay`**, **`notepad`** and
  **`split-preview`** (`MEETING_WINDOWS`, looked up by label, a missing one
  skipped; the preview syncs itself when created) get
  `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)` (fallback
  `WDA_MONITOR` before Windows 10 2004): on your monitor, left out of
  screenshots, recordings and every screen share. Lifted when the recording
  stops. Applied on the main thread (`run_on_main_thread`, in order, no lock)
  on every `yap-meeting-state`, on `save_config` (toggled mid-meeting), and on
  `on_page_load` (lib.rs: a window created mid-recording; `sync_window`).
  Off + a meeting starting → the screen-share tip once per note ("Your
  meeting notes show up in screen shares and screenshots" + **Update
  settings** → `general#screen-sharing`). Debug-only `capture_affinity` reads
  each window's affinity back (`GetWindowDisplayAffinity`) for the e2e suite.
- **`calendar.rs`** (+ `calendar/`) — **the calendar** (Wispr Flow's,
  local-first: no Yap server; see [`docs/calendar.md`](./docs/calendar.md)).
  **Connections** (≤ 8; Settings → Connectors, or the "Connect your
  calendar" dialog every other Connect calendar opens): Google Calendar in one click
  (`calendar/google.rs`: OAuth for installed apps in the system browser, PKCE
  S256 + `state` via `auth.rs`'s helpers, a one-shot `tiny_http` listener on
  `127.0.0.1`, `access_type=offline`; scopes `calendar.calendarlist.readonly`
  + `calendar.events.owned.readonly` only; the client id/secret baked in from
  `YAP_GOOGLE_CALENDAR_CLIENT_ID`/`_SECRET` by `option_env!` — none = no
  Google: "Google sign-in is in the installed Yap. In this build, use a
  private iCal link…"; the token is revoked on Disconnect) or any private iCal link
  ("Outlook Calendar" = a guided publish-your-calendar ICS flow; "Other
  calendar"; HTTPS only, `webcal://` → https). Secrets (refresh token, link)
  in Credential Manager (`calendar/vault.rs`: `yap-calendar-<n>.com.yap.dictation`,
  Local persistence; test mode: `calendar-secrets.e2e.json`); `calendar.json`
  keeps connections' names, meetings from 12 h back to 7 days ahead, reminder
  answers (2 days) and the nudge. **Sync** (a tokio scheduler waking ≤ 30 s;
  a wall-clock jump = the PC slept → sync now): every 12 min, on wake, on
  Sync, after a connect; a failure keeps the old meetings and shows why;
  `invalid_grant` → "Connect again". **Parsing** (`calendar/ics.rs`, Yap's own,
  lenient: byte-wise unfolding, RFC 6868, bad lines skipped) + recurrence via
  `rrule` on wall-clock time then the event's zone (EXDATE/RDATE/
  RECURRENCE-ID overrides); **zones** (`calendar/tz.rs`): IANA (`chrono-tz`) →
  Windows names (CLDR table) → the feed's VTIMEZONE → "(UTC±hh:mm)" → floating;
  RFC 5545 gap/overlap rules. **Filters** (`calendar/model.rs`, Wispr's): a
  meeting has another invitee (not a room, not declined) or a join link; no
  all-day, nothing > 6 h, nothing cancelled or declined; Maybe = tentative,
  overlaps = Conflict. **Join links** (`calendar/links.rs`): HTTPS links of
  known services (Teams/Meet/Zoom/Webex/Slack/GoTo/Whereby/Jitsi/…) from
  conference data, location or description. **The reminder card**
  (`meeting_reminder`: "15s" default | "1min" | "2min" | "never"): "Design
  review · In 1 min", Join & take notes / Start notes / Snooze 2 min / ✕,
  until 5 min after the start, one at a time; while another meeting records,
  a **switch** card (ends it via `meeting_end::end`, then starts the next).
  It goes on screen through **`present_card`/`withdraw_card` only**, in
  **one place** (`notice.rs`): the in-app toast while the main window is
  focused (the snapshot's `card` with `inApp`), else the **Yap bar**
  (`bar::show_card`, id "calendar": a call-style card
  — the call app's mark or a calendar, the meeting over "● In 1 min · with
  Tanay +1" in amber, green once it's on, kept current with `update_card`
  every 5 s; Join & take notes | ^ Start notes, Snooze 2 min; ✕ = dismiss; no
  Esc watch), else — the bar off or hidden — a Windows notification under
  the "calendar" tag; a card follows you onto the bar when the window loses
  focus, back into the window when it gets it (`MoveIn`,
  `on_main_window_focused`), and off a bar that goes away. **Into notes**:
  `on_meeting_started` (from `commands::meeting_start`, every route) ties the
  note to the meeting on now (service of the live call first, nearest start;
  from 10 min before) — title over a placeholder (`is_placeholder_title` /
  `title_open_to_ai`; then a real title), attendees, the cleaned invite
  (`invite_context` → action plan + Ask), and for a 1:1 meeting "Them" named
  after the one other invitee as a guess (`notes::link_event` →
  `speakers::calendar_guess`) — and `note_changed`s every window;
  `claims_call` / `on_call_started` for call detection (above);
  `meeting::with_attendees` puts a meeting note's participants (and the
  speaker map's names) into Whisper's prompt. Commands `calendar_status`/`_sync`/`_connect_google`/
  `_cancel_google`/`_add_link`/`_disconnect`/`_event(key, open|join|start|
  joinStart|switch|joinSwitch)`/`_card(id, join|start|snooze|dismiss)`/
  `_nudge`/`_meeting_notes(query)` (`hasPlan`, `hasTalk`: "Generate
  summary"); events `yap-calendar` (snapshot), `yap-calendar-connected`
  (`{kind, label, meetings}`: the new calendar's meetings on now or in the
  next 7 days, which `_add_link` also returns), `yap-calendar-error`;
  `meetings_context` backs the
  Meetings view's Ask bar (`chat_send` scope "meetings"). Debug-only:
  `YAP_GOOGLE_{AUTH,TOKEN,REVOKE,API}_URL` (+ the client at run time) and
  `calendar_e2e_opened` (test mode records links instead of opening them).
  Lock rule: never hold its state lock while touching windows, the network,
  notes or call detection.
- **`media.rs`** — audio-file decode front-end for Upload: pure-Rust **Symphonia**
  (mp3/wav/m4a/aac/flac/ogg-vorbis; no opus yet) → downmix mono → 16 kHz
  (`pipeline::resample_linear`), plus `chunk_ranges` (~60 s windows cut at the
  quietest sample of each window's last 5 s). Consumed by
  `pipeline::run_file_transcription` (progress events, cancel flag, `processing`
  guard, the correction dictionary over the whole joined text via
  `pipeline::apply_corrections` — so a mis-hearing split across two chunks is
  still caught — and a history record with the raw and corrected text) via the
  `transcribe_file` command.
- **`chats.rs`** — the AI Chat surface's conversation store (`chats.json`): a
  `Conversation` (title, messages, timestamps) with `list`/`get`/`create`/
  `append`/`delete`/`rename` (`chat_rename`: one trimmed line ≤ 80 chars,
  `updated_ts` untouched so the list order stays; unit tests use an
  in-memory store per thread, never `chats.json`); `chat_send` (commands.rs)
  creates a conversation on the first message using a first-50-chars title
  rule. An answer keeps its tool
  chips (`ChatMessage.tools`, only written when there are some), so a
  conversation opened later still shows what the AI looked up and changed,
  and the model that wrote it (`ChatMessage.model` = `ChatModel {provider,
  id, name?, onThisPc?}`, only when known; older answers have none). A
  question that was dictated carries `voice: true` ("Only when I asked by
  voice" reads its answer out); `append` returns the message's index, which
  the pages use as its id (`chat:<conversation>:<index>`). A web answer keeps
  its sources (`ChatMessage.sources` = `Source {n, title, url, site}`,
  numbered as it cites them; `append_sourced`; only written when there are
  some). **A citation with no source behind it** is taken out before the
  answer is saved (`strip_unsourced_citations`, in `chat_turn`: a small model
  answering from memory wrote "Python 3.12.0 [1]" with no search): kept are
  the numbers of this turn's sources, else of the latest earlier answer that
  had some (`earlier_source_numbers`, what ChatView's `citedSources` draws a
  follow-up's "[1]" against); code, a link's text and "[0]" are left alone.
- **`ai_model.rs`** — which model a scope's next call goes to, resolved the
  calls' way (`Scope::Chat` → `commands::resolve_chat_endpoint`;
  `Scope::Notes` → `meeting_summary::resolve_endpoint`): `ai_model_status
  {scope}` → `{ready, error, model, host, tools (supports_tools), fromScope,
  provider (the one the scope is set to, ready or not), keyed, maskedKeys
  (each keyed provider's key as "gsk…AbCd", `mask_key` — never the key),
  servers, onDevice, context (the on-device model's, when the call goes to
  it: the chip's tooltip "Context: 8K tokens")}` for the Chat view's model chip + picker and the Ask
  bars' model lines; `chat_set_model {provider, model, baseUrl?, file?}` =
  the picker's switch (Chat scope only, switched on; a keyed provider, the
  installed on-device model, or a self-hosted / custom server Settings has
  the address of — `servers(cfg)`: the Chat scope's, cleanup's, any other
  scope's or cleanup profile's local/custom endpoint, one per address, its
  key taken from where it's set up (`model_catalog::endpoint_key`), never
  an address the page makes up; same provider keeps its URL + key, a new
  one gets its standard URL + the shared key; `file` picks one of the
  on-device GGUFs → `pp_local_model` (`set_local_model`, "" = the bundled
  1.5B) — the one sidecar serves it for every feature, so one serving
  another GGUF stops and starts again on it), saved through `save_config`,
  then `yap-chat-model-changed {scope, ppLocalModel?}` (Settings adopts
  both). `chat_provider_key {provider, key}` = the picker's "Connect
  Claude": `set_provider_key` saves it where Settings does (`pp_api_keys`,
  cleanup's `pp_api_key` when cleanup is on it, the Chat scope's key when
  Chat is), through `save_config`, whose `model_catalog::on_config_saved`
  checks that provider's list at once; the page tells Settings' copy with
  the window event `yap-provider-key-saved` (not broadcast to every
  window). `wake_on_device` (in `chat_send` and
  `note_ask`): a chat on the on-device model starts the sidecar
  (`local_llm::ensure_started`) or says it isn't installed, never a silent
  fallback to the endpoint underneath. `save_config` emits
  `yap-config-saved`, which the chip re-reads on. Unit-tested.
- **`model_catalog.rs`** — **the AI providers' live model lists**
  (2026-10-18), so a model Google or Groq releases tomorrow shows up in
  Yap's pickers without a Yap update and a retired one stops being offered.
  Asks **the provider itself, with the key already saved** (`key_for`: the
  scope's own, `provider_api_key`'s shared one, else any scope's or cleanup
  profile's; only providers with a key, `keyed`; never a catalog of Yap's
  or a third party's): Groq `GET /openai/v1/models`, OpenAI `GET /v1/models`
  (Bearer), Anthropic `GET /v1/models` (`x-api-key` + `anthropic-version`,
  paged by `after_id`), Gemini's native `GET /v1beta/models` (the key in
  `x-goog-api-key`, never the URL; paged by `pageToken`), OpenRouter
  `GET /api/v1/models`; 15 s, off the main thread. **Keeps chat models
  only**, per provider and unit-tested on fixtures trimmed from the real
  lists (`parse_groq` … `parse_openrouter`, `chat_capable`): no embedding,
  speech/TTS/transcription, image/video/music, moderation/guard, realtime/
  live/native-audio, search-preview, computer-use, deep-research or Codex
  models; Gemini needs `generateContent` (no `-latest` aliases, `-exp`,
  custom-tools variant), Groq and OpenRouter text out (Groq: active, not a
  model `config::is_retired_groq_model`), OpenAI GPT / o-series without the
  Responses-only `-pro` and `-latest` aliases, Anthropic not `retired`
  (`deprecated` marked); dated snapshots fold into the alias the list has
  (`fold_snapshots`: `gpt-4.1-2025-04-14` → `gpt-4.1`'s `aliases`), one
  without it keeps a `base` (`claude-haiku-4-5-20251001` →
  `claude-haiku-4-5`) so a curated alias still matches. **Names**: the
  provider's (`clean_name`: "OpenAI: " / "Qwen/" cut), else `derive_name`
  ("GPT-5.5", "GPT-OSS 20B", "Llama 4 Scout 17B 16E Instruct"). Per model:
  id, name, desc (first sentence), created, context, `tools` / `thinks` when
  the provider says (Groq's `supported_features`, OpenRouter's
  `supported_parameters`, Gemini's `thinking`), `firstSeen`. **Cache**
  `<data>/model_catalog.json` (per provider: the last good list +
  `fetchedAt`, the last attempt's `checkedAt` + `error` in words — "the API
  key wasn't accepted" —, never a key; atomic, quarantined when corrupt).
  **When**: a list is fresh for a day (`FRESH_FOR`), a failed check tried
  again after an hour; `init` checks what's due 40 s after launch, then
  hourly (not in a test run); `on_config_saved` (from `save_config`) checks
  a provider at once when its key was set or changed; `model_catalog` (the
  pickers) returns the cached lists and checks a due one behind them; one
  check per provider at a time; every change is `yap-model-catalog`
  (snapshot `{providers, now, rev}`, keyed providers only; `rev` taken under
  the store lock so a late command reply never wins over a newer event).
  A failed last check = `ok: false` (the page shows the curated list).
  `tools_for(provider, model)` feeds `tools::supports_tools`: a cloud model
  its provider says can't call tools gets no note tools ("· no note tools").
  **Self-hosted / custom servers** (`llm_models {provider: local|custom,
  scope, baseUrl}`): `GET {base}/models` with the scope's key (OpenWhispr's
  OpenAICompatiblePanel reading: `data` or `models`, `id` or `name`), kept a
  minute in memory, plus what the server says of each model's **context**
  (`server_contexts`, read-only, best-effort, 3 s each: LM Studio's `GET
  /api/v0/models` `max_context_length` / `loaded_context_length`, else
  Ollama's `GET /api/ps` (a loaded model's `context_length`) and `POST
  /api/show` per model, ≤ 16 (`model_info.<arch>.context_length`, a
  Modelfile's `num_ctx`); `ListView.contexts [{id, max, set, app}]`; a
  server that's neither says nothing). The picker shows it first in the row
  ("8K context, set in Ollama · localhost:11434", "Up to 128K · …"). Commands `model_catalog`, `model_catalog_refresh
  {provider}` (Settings' Check now), `llm_models {provider, scope?, baseUrl?,
  force?}`. Logs: provider + counts. Test mode never contacts a provider:
  debug-only `YAP_MODELS_STANDIN` (`<standin>/<host><path>?<query>`,
  `e2e/support/fake-llm.js`); none set = nothing fetched; a self-hosted
  server only on 127.0.0.1. `#[ignore]`d `real_saved_lists`
  (`YAP_MODEL_LISTS=<dir>`) runs the parsers over saved real lists, never
  fetching.
- **`tools.rs`** — the AI Chat tool-calling agent loop: six tools ported
  near-verbatim from OpenWhispr's `services/tools/*` (`search_notes`,
  `get_note`, `create_note`, `update_note`, `list_folders`,
  `copy_to_clipboard` — executed locally over `notes.rs`/`arboard`) and
  three of Yap's own for **changing part of a note safely** (after the note
  tools of Wispr Flow's live Ask — behaviour only; the pure part is
  **`note_text.rs`**): `search_in_note(note_id, query, field?)` (the lines a
  phrase is on, numbered `12| …`, the note's own text, the lines around the
  first 5 hits; forgiving — case, whitespace runs, curly quotes, dashes; in
  content, the `enhanced` summary and the transcript (talk only, `[m:ss]
  You: …`) unless one is named; ≤ 20 hits / ~4k chars), `read_note(note_id,
  field, offset?, limit?)` (a page of ≤ 400 lines and ≤ 3,000 tokens — the
  bound `get_note` puts on a meeting via `ask_context` — with `more` /
  `next_offset`; a line longer than a page is cut and says so) and
  `edit_note(note_id, field: content|enhanced|title, old_text, new_text)`:
  `old_text` must appear **exactly once** (overlapping counts), byte for
  byte except that `\r\n` = `\n` (the new text takes the note's own line
  endings); 0 matches → nothing changes and the error lists the places that
  differ only in spacing/case/quote marks, quoted from the note (`near`),
  and spots copied line numbers; 2+ → the lines they're on; `old_text` ""
  only fills an empty field; the transcript is never edited. Bookkeeping: a
  new title clears `title_auto`, a content edit leaves the summary stale
  (`enhanced_at_hash` untouched), a summary written into an empty field is
  stamped fresh, one emptied is no summary. Matched and replaced under the
  store lock (`notes::modify`). `update_note` stays for folder moves and
  whole rewrites the user asks for (its description and `TOOL_INSTRUCTIONS`
  say so; the instructions steer search → `edit_note` for everything else);
  `get_note` returns the notes (`content`) and the summary (`enhanced`)
  apart, each ≤ 2,000 tokens (`cut` names what was shortened). Failures
  carry `error` (for the model) besides `displayText` (the chip).
  `run_tool_loop` drives the OpenAI tool-call protocol via
  `llm::post_chat_message` for up to `MAX_TOOL_STEPS` (20) turns, takes
  arguments as a JSON string or an object (ids as numbers or strings), and
  hands every note a tool created or changed (`changed_note`; an edit that
  changed nothing doesn't count) to `on_change` as it happens — `chat_send`
  emits `yap-note-changed` (origin `"chat"`) + `yap-notes-changed`, so the
  notepad and the lists follow live. Chips say what happened: `Found
  "beta" on 2 lines in "Launch plan"`, `Edited "Launch plan": "Friday." →
  "Monday."`, `Couldn't edit "Launch plan": the text is in 2 places`. Gated
  by `supports_tools` — cloud providers always qualify (unless the
  provider's own model list says that model can't call tools,
  `model_catalog::tools_for`), local models by
  `local_model_uses_tools`: a family the model evaluations saw fail never
  (`NO_TOOL_FAMILIES`: Llama 3.2, which loops), one they saw succeed from its
  size (`TOOL_FAMILIES`: Qwen3.5 ≥ 4B, thinking off), else ≥4B params
  (`LOCAL_TOOL_MIN_PARAMS_B`; the size read from `-4B`, `_4B`, `:8b`) — so
  smaller local models fall back to plain keyword-RAG chat instead. **A tool
  call the provider rejects** (Groq's HTTP 400 `tool_use_failed`: gpt-oss
  left out `note_id`, or called a tool not in the request; llama.cpp's 500
  "Failed to parse input" for a call written as text — `llm::rejected_tool_call`)
  never ends the turn in an error: the model is told what was wrong (a
  user-role note from Yap, with what it sent) and asked again once
  (`REJECTED_CALL_RETRIES`), then answers without tools (`ANSWER_NOW_NOTE`, as
  when out of steps). A web tool past its per-turn cap stays offered (`web::
  Turn` answers "you've searched 3 times"): taking it away made Groq 400 the
  next call to it. Unit-tested end to end against
  `notes.rs`' **in-memory test store** (under `cfg(test)` every test thread
  gets its own empty store and nothing reads or writes `notes.json`), and
  in `e2e/chat-tools.spec.js` against the scripted fake AI. **Folders are
  Yap's own rule** (a deliberate change from OpenWhispr, whose
  `create_note`/`update_note` made any folder the model named — a chat once
  left an empty "Camping" folder behind): `note_folder` files a note into a
  folder that exists, by any case (as the list spells it); no folder = Personal
  (a new note) / no move; a new folder only with `new_folder: true`, which the
  tool descriptions and `TOOL_INSTRUCTIONS` keep for a user asking for one
  ("Never make up a folder"). An unknown name without it changes nothing
  (an `update_note`'s title included) and tells the model the folders there
  are and to leave `folder` empty; chip `Couldn't create "Tent list": no
  folder called "Camping"`. (The MCP server's `create_note` still makes the
  folder it's given.) Plus **`open_app(name)` / `list_apps(query)`**
  (`app_definitions` + `APP_TOOL_INSTRUCTIONS`, offered while
  `open_apps` is on; run by `apps.rs` off the async runtime, with the
  answer just before the question for its consent check). Every step
  re-sends the instructions + schemas, so they're kept short
  (`a_tool_step_stays_light`: ≤ 2,250 est. tokens; 2,879 before the
  2026-10-18 trim, ~2,180 after). Plus **`web_search(query)` /
  `read_page(url, offset?)`** (`web_definitions` + `WEB_TOOL_INSTRUCTIONS`,
  ~470 tokens more, `the_web_tools_stay_light`; offered while "Look things
  up on the web" is on; run by `web.rs`'s per-turn `web::Turn`, passed as
  `run_tool_loop`'s last argument). **The safety rule**
  (`refused_after_web`, `ACTION_TOOLS` = open_app, create_note,
  update_note, edit_note, copy_to_clipboard): once web content is in a turn
  (`Turn::seen`), those are no longer offered (`without_actions`) and a call
  to one is refused with a reason for the model (chip `Didn't open an app:
  not after reading the web`); the person's next message is a clean turn.
  Unit-tested through the real loop against a local fake model + Exa
  (`a_web_page_can_never_make_the_chat_act`).
- **`web.rs`** — **looking things up on the web** (2026-10-18; Settings →
  Voice Assistant → Web search, `web_search` off by default; design in
  yap-private `docs/web-search-research.md` §7). **Backends** (`search`):
  a cloud model's **own search** (`web/native.rs`, below) by default, else
  Yap's own (`own_search`): **Exa** (`POST https://api.exa.ai/search`, `x-api-key`, its guide's
  recommended `{query, type: "auto", contents: {highlights: true}}` +
  `numResults: 5`) or **Tavily** (`POST https://api.tavily.com/search`,
  Bearer, `{query, max_results: 5}`), keys in `web_search_keys` (service →
  key, like `pp_api_keys`, never logged); with no key — or when the service
  fails (its error kept as `note`, said in the chip: 401/403 wrong key, Exa
  402 / Tavily 432 out of free searches, 433, 429, offline), or when it
  found nothing ("Exa found nothing." as the note) — the **keyless
  lookup**: the game's own MediaWiki (`GAME_WIKIS`: bg3.wiki, warcraft.wiki.gg,
  minecraft.wiki, Fandom wikis…; any `*.wiki.gg` / `*.fandom.com` named in
  the question) else Wikipedia in the person's language (`wiki_language`:
  the speech language, else Windows', else English; English again when it
  finds nothing), through `api.php` (`generator=search` + `inprop=url`, then
  TextExtracts on Wikipedia, `action=parse` HTML elsewhere → `fragment_text`
  with dom_query, infoboxes/navboxes/footnotes out → `best_passages` by
  keyword); `wiki_terms` drops the game's names and question words ("Where
  do I find Gale in Baldur's Gate 3?" → "Gale"), `wiki_retry` tries fewer.
  A keyed game question gets the wiki's best article first too. Descriptive
  User-Agent (`USER_AGENT`, contact URL), one request at a time per host
  250 ms apart, ≤ 5 results, ~700 chars each, 10-minute cache. Never a
  key-less scrape of Google/Bing/DuckDuckGo. **`read_page`**: `check_url`
  (http(s), ports 80/443, no user:pass, not `localhost`/`.local`/… or a
  local IP — `is_public`: loopback, private, link-local, CGNAT, ULA,
  mapped/NAT64/6to4 carrying one, documentation, reserved), the host
  resolved and every address checked (`resolve_public`), the connection
  pinned to them (`resolve_to_addrs`, no proxy, no cookies), redirects
  followed by hand (≤ 5, each checked), 10 s, 3 MB, HTML/plain text only;
  `page_text` = dom_smoothie (readability.js port, `TextMode::Formatted`),
  `page_part` ≈ 3,000 tokens a read with `next_offset`. **`Turn`**: only
  links the person gave (their messages, `links_in`) or a search this turn
  returned (`allowed`); ≤ 3 searches and ≤ 3 reads a turn (a call past the
  cap is answered "you've searched 3 times", the tool still offered);
  sources numbered as they come (`number`), `sources_for(reply)` = the ones
  cited (`citations`: [1], [2, 3], [1][4]) or all; results carry
  `DATA_NOT_INSTRUCTIONS`. Chips: `Searched the web: "q"` (+ " (bg3.wiki)"
  when keyless, + the service's error), `Read bg3.wiki/wiki/Gale`, `Didn't
  read evil.example/…: not a link from you or the search`. **Models that
  can't call tools** (`commands::small_model_answer`): the fixed pipeline —
  `needs_web` (conservative: "look up", "search the web", "google",
  "latest", "news", "price of", "where do I find", a known game; never
  "my notes"/"my meeting"), `search_query` (the ask stripped),
  `grounded_sources` (≤ 3 sources, ≤ 1,500 tokens) + `grounded_prompt`
  (answer only from them, cite, < 80 words) with the history fitted
  (`fit_history`, ≤ 2,000) for an 8k model; no search → `no_search_line`,
  and an answer that `claims_browsing` anyway gets "*Yap didn't search the
  web for this answer.*". Chips `Searched the web…` + `Answered from 1 web
  page`. **The AI's own search** (`web/native.rs`, 2026-10-18, Nathan's
  call: "let the cloud models use their own search tools, and only give
  local models Yap's"; `web_search_native`, on by default): a cloud chat
  model (`Settings::for_chat(cfg, provider, model, base_url, key)` →
  `NativeSearch::for_chat`) keeps the ONE tool loop and calls Yap's own
  `web_search`; running it is a short **sub-call** to the same provider,
  on the chat's endpoint and key, with nothing but its search tool
  (`NativeSearch::body`, pure): **Groq** chat completions +
  `tools: [{type: "browser_search"}]`, `tool_choice: "required"`,
  `reasoning_effort: "low"`, `max_completion_tokens` 1,024, on gpt-oss
  (another Groq model searches with `openai/gpt-oss-20b`); **Gemini**
  `…/v1beta/models/<model>:generateContent` beside the chat's `/openai`
  endpoint, `tools: [{google_search: {}}]`, `x-goog-api-key` (no forcing
  exists; Gemini 2+ text models); **Anthropic** `POST /v1/messages`
  (`anthropic-version: 2023-06-01`, `x-api-key`) with the basic
  `web_search_20250305` (`max_uses: 2`; no `tool_choice` — forced is a 400
  on Opus/Sonnet 5.5 —, no temperature/thinking); **OpenAI** the
  Responses API `POST /v1/responses`, `tools: [{type: "web_search"}]`,
  `tool_choice: "required"`, `include: ["web_search_call.action.sources"]`,
  `store: false`, `reasoning.effort: "low"` on gpt-5+/o3/o4 (gpt-4o,
  gpt-4.1 not nano); **OpenRouter** chat completions + `plugins: [{id:
  "web", max_results: 5}]` (any model). Docs as read 2026-10-18 (sources in
  the module doc). Parsers (pure, fixtures from each provider's documented
  shape; Groq's from the eval spike): Groq's `executed_tools` (`browser.search`
  results, `browser.open` pages as `L<n>:` lines; `【cursor†L4-L9】` →
  those lines are the passage), Gemini's `groundingChunks` + `groundingSupports`
  (segment text = passage; byte offsets), Anthropic's `web_search_result`s +
  text `citations[].cited_text` (`page_age` → date; an `error_code` in a
  200 = failure), OpenAI's/OpenRouter's `url_citation` annotations (the
  sentence before it, or OpenRouter's `content`; the markdown link at the
  span → the mark; `utm_*` ignored when matching pages) → ≤ 5 `Hit`s (read
  or cited first) + the provider's short answer as `summary` with its
  citations renumbered to Yap's `[n]` (`render_summary`; gpt-oss's 【…】
  out). The tool result is the usual numbered `results` + `summary`, under
  `DATA_NOT_INSTRUCTIONS`, so the safety rule, the 3-search cap, chips,
  pills, the Sources row and chats.json are unchanged. Chip `Searched the
  web with Groq: "q"` (Google / Anthropic / OpenAI / OpenRouter). **Fallback**
  (unsupported model, 4xx, 429 — never waited out —, unreachable, nothing
  found): Yap's own search (`own_search`), its reason first in the `note`
  (`http_error`: "Groq's search is limiting how much Yap can search right
  now.") → `Searched the web: "q" (Exa) · Groq's search is limiting…`; not
  cached. A failure that will repeat (400/401/402/403/404/422, or Gemini's
  429 "check your plan and billing" with no retry delay = grounding not on
  a free key's plan: Gemini 3+ needs billing) is remembered 30 min per
  provider + model + endpoint (`UNAVAILABLE`, cleared by `clear_caches`):
  Yap's own search answers straight away, same reason in the chip. **Gemini's terms** (Gemini API Additional Terms, 2026-04-28): a
  grounded result may feed a follow-up answer when that answer shows its
  Links (≤ 5) — `Turn::sources_for` lists every Google link of a grounded
  turn (cited first, ≤ 5), unmodified (Google's
  `vertexaisearch.cloud.google.com/grounding-api-redirect/…`, the site from
  its title: `source_site`); the sub-call's text is never stored. Usage goes
  to `usage::record`; a model evaluation logs and paces the sub-call
  (`eval::Call`, ~4k tokens estimated). Custom/self-hosted/on-device models
  have none (Yap's own). Not for the Meetings view's Ask bar. Test mode never goes online: debug-only `YAP_EXA_URL`,
  `YAP_TAVILY_URL`, `YAP_WEB_STANDIN` (every other site as
  `<standin>/<host><path>`), else an error. `#[ignore]`d
  `real_keyless_lookup` (read-only, no key: Wikipedia + bg3.wiki).
- **`apps.rs`** — **opening apps when you ask** (2026-10-18; Settings →
  Voice Assistant → Opening apps): "open Teams" in Chat or to the Voice
  Assistant opens an app installed on this PC — only an entry of the
  index, never a command line, arguments, a file, a folder or a link.
  **Index** (`apps(refresh)`, in memory, rebuilt after `FRESH` = 5 min or
  on Refresh; read-only, no network): the Start menu's "All apps" =
  `shell:AppsFolder`'s children (`SHGetKnownFolderItem(FOLDERID_AppsFolder)`
  → `BHID_EnumItems`, on an STA thread of its own): display name, parsing
  name as the id (an AppUserModelID like `MSTeams_8wekyb3d8bbwe!MSTeams`,
  or a known-folder path `{6D809377-…}\7-Zip\7zFM.exe`), the item's ID
  list and, for a desktop app, **the Start menu shortcut behind it**
  (`System.AppUserModel.BestShortcut`, an ID list → a `.lnk`/`.url` path
  in one of the Start menu's Programs folders) plus its target and
  arguments (`System.Link.*`, read for the launcher label, never passed);
  ~240 entries → ~190 apps in ~300 ms on Nathan's PC (204 of 205 desktop
  entries have a shortcut). Fallback: the Start menu `.lnk`s (ProgramData
  + AppData; a `.url` only with a game store's link) + App Paths. `tidy`:
  junk out (`junk` → `is_junk`: "Uninstall …"/`unins*.exe`, read-mes,
  help/`.chm`, docs, licences, `http(s)://` and `.url` links, "… on the
  Web", `.msi`, folders; a Store app keeps "Help" — Get Help; "Revo
  Uninstaller" stays; a **game store link** — `steam://`,
  `com.epicgames.launcher://`, `battlenet://`, `uplay://`, EA, GOG,
  Amazon (`LAUNCHER_LINKS`) — is an app whatever file holds it; a shortcut
  to a web page is junk), one entry per name. **Opening** (`open`): a
  packaged app (`id` has `!`) by
  `IApplicationActivationManager::ActivateApplication` (brings it forward,
  gives its pid); a desktop app by **its shortcut file** with
  `ShellExecuteExW`'s default verb, as a Start menu click (its arguments,
  working folder, run-as apply); only without one by the entry's ID list
  (`SEE_MASK_IDLIST`). ⚠ Opened by its ID list, World of Warcraft said
  "Opened" and showed nothing (2026-10-18): its launcher needs the
  shortcut's `--productcode=wow`, which nothing documents the ID list to
  carry (not confirmed by a launch; the shortcut file is the documented
  way). Then
  `bring_forward(pid, since)` (≤ 8 s: the started process's main window,
  or that of a process it started since the launch — `procs::family`, a
  launcher handing over; a Battle.net already running isn't one, and may
  stay behind the window in front). Logs the app's name and how it opened ("its
  Start menu shortcut" / "app activation" / "its app-list entry"), never a
  path. A launch Windows refuses → "Windows couldn't start X: …", never
  "Opening…". `launcher(target, args)`: "Steam" for `steam://`, "Battle.net"
  for `… Launcher.exe --productcode=…` → "Opening World of Warcraft (in
  Battle.net)." **Matching** (`find`, pure): `keys(name)` — the
  full name, without qualifiers ("(64-bit)", " - Insiders", versions),
  without the maker (`VENDORS`: "Teams"), initials ("VS Code", "WoW"), the
  last word (not a `GENERIC` one), each also with **numbers as digits**
  (`numerals`: one–twenty, II–X: "three" = "3" = "III") — vs the request
  (`FILLER` "the/app/please" out, apostrophes dropped, "my", a maker,
  `ALIASES`: calc, cmd, files, vscode, store…, numbers as digits), exact,
  a few of its words, or a near-miss (`fuzzy::spelling_distance` ≤ 0.2
  and `plain_spelling_distance` ≤ 0.3: "spottify" yes, "spotifire" no);
  nicknames first (`app_nicknames`); `CONFIDENT` 0.80, others within
  `MARGIN` 0.03 → `Several` (four Pythons, two Teams: ask, never guess);
  `apps_blocked` → `Blocked`; else **by sound** (`sound_of`:
  `fuzzy::sound_distance` ≤ 0.1 over ≥ 5 sounds, spelled ≤ 0.45 apart —
  "bolders gait" → Baldur's Gate 3) when exactly one app sounds so and no
  other scores ≥ 0.6 or sounds within 0.25; else `Nothing` + close names
  (sound-alikes first). **Fast path** (`plan_with` / `plan_for(text, cfg,
  conversation)`, before any model, in `commands::chat_turn` →
  `app_turn` and `assistant::answer`): `open_request(text)` (pure: the
  whole message is "[greeting/please/can you/try to/yes…] open|launch|
  start|play|run|fire up|boot up|load up… <app> [please/for me/now/
  again]", one sentence; `plain` = no joining words "and/about/for/to…";
  `play` — a phrase naming no app is the model's, "play the next song")
  → one app → opened, "Opening Microsoft Teams." + chip `Opened Microsoft
  Teams` (no model call: works with no AI and the 1.5B local model);
  several → "I found 2 apps for “teams”: … Which one?"; switched off →
  "You've asked Yap not to open …"; a plain name not installed → "I
  couldn't find an app called …" (+ "Did you mean …?") unless a
  tool-capable model is there; "Let Yap open apps" off → it says so; "open
  my notes", "start the meeting" (`NOT_APPS`), "open the notes about Teams"
  → the model. **Yap's own questions, answered** (a per-conversation
  memory, `recall` / `remember` / `moved_on`, in memory only): its
  question's apps (`OFFER_FOR` 5 min, gone when a message goes to the
  model) and the app it last opened or suggested (`LAST_FOR` 30 min) —
  `follow_up` (pure): after "Did you mean X?" a "yes" (`is_affirmative`:
  yes/yeah/sure/please/do it/go ahead/open it/that one/"Yes, I do.") opens
  X; after "Which one?" "the second one" / "2" / "the last one" / "3.13" /
  "the classic one" / a name opens that one (a "yes" → "Which one: A or
  B?"); "no"/"never mind"/"neither" → "Okay, I won't open anything." (no
  chip); "open it (again)" → the last app (none: the model's). **Model
  tools** (`execute_tool(name, args, question, previous)`): `open_app`
  acts only when the person's own message asks to open something
  (`asked_to_open`: open/launch/start/run/play…) or is a plain yes to the
  answer just before, which asked a question naming that app
  (`consented`: "Would you like me to open Baldur's Gate 3?" → "Yes."; not
  "Are you struggling?"), so a note saying "open X" can't (chip `Didn't
  open "X": you didn't ask to open an app`). `list_apps` takes a query
  and gives ≤ 15 names (`LIST_MAX`; no query: the count, 15, "pass a
  query" — 840 → 135 est. tokens on Nathan's PC). **A model without
  tools** gets `NO_TOOLS_NOTE` (+ `NO_TOOLS_OPEN`: say "open" and the
  name), and an answer that claims an open the person asked for
  (`claims_unasked`: "Opening …", "I'm trying to open …", "I've
  launched …") becomes `correct_claim`'s "I can't open apps myself. Say
  “open Baldur's Gate 3” and Yap will." (a long answer keeps its text).
  Spoken "Opening …" follows the usual meeting rule (the Voice Assistant
  is off during meetings; Chat's auto-speak stays quiet; TTS reads
  "Battle.net" as "Battle dot net"). Commands `apps_list {refresh}`,
  `apps_icons {ids}` (`IShellItemImageFactory`, 32 px →
  `app_icons::encode_png`, cached per run). Test mode: nothing launched
  (recorded, debug-only `e2e_opened_apps {clear}`), index =
  `YAP_E2E_APPS` (a JSON `[{id, name, target?, args?}]`, read each call),
  no icons; unit tests never read `config.json` or this PC's apps
  (`run_tool`, a thread-local stand-in index; `games()` = Nathan's WoW /
  Classic / Battle.net / Baldur's Gate 3 / Steam entries). `#[ignore]`d
  `this_pcs_apps` (read-only: names, junk, the games' shortcuts and
  launchers, sample matches, `list_apps`' size, icons),
  `opens_notepad_for_real` (opens Notepad the app's way, closes only that
  pid's window; skipped while a Notepad runs) and
  `opens_shortcuts_for_real` (in its own temp folder, never the Start
  menu: a `.url` to `ms-settings:about` — or `calculator:` while Settings
  is open — closes only the new window; a `.lnk` to cmd.exe whose
  arguments write a file into its working folder; a `.lnk` to Notepad with
  a file, skipped while a Notepad runs).
- **`bridge.rs`** — the **local API bridge** (OpenWhispr `cliBridge.js` port,
  backs the Integrations view): a token-authenticated loopback HTTP server
  (`tiny_http`, `127.0.0.1`, OS-assigned port) exposing `/v1` REST routes over
  notes/folders/history so terminals + coding agents can drive Yap data.
  Discovery = `~/.yap/cli-bridge.json` `{version, port, token}` (fixed path,
  written on start, deleted on exit); auth = `Bearer <token>` (SHA-256
  constant-time compare); note mutations emit `yap-notes-changed` so NotesView
  refreshes live. Toggled by `config.bridge_enabled` (default on; `sync()`
  runs at setup + every config save). A second Yap (dev next to installed)
  leaves a live bridge's file alone, and `stop()` only deletes the file while
  it's still ours. Also serves the MCP server's meeting views
  (`/v1/meetings/list`, `/v1/meetings/search`, `/v1/mcp/config`; built in
  `mcp.rs`). Debug builds take `YAP_BRIDGE_FILE` to move the discovery file
  (the e2e suite). See `docs/local-api.md`.
- **`mcp.rs`** — **Yap's MCP server** (Wispr Flow's Notetaker MCP, local): AI apps
  launch **`yap.exe mcp`** (main.rs sends that argument here before any of the app
  starts: no window, tray, hook or single-instance check; release builds are
  GUI-subsystem and inherit the client's stdio pipes fine) and speak MCP over
  stdin/stdout via `rmcp`, which serves both eras (the 2025 `initialize` handshake
  and 2026-07-28's per-request `_meta` + `server/discover`). The process holds no
  data: every tool calls the **running** Yap's local API (discovery file re-read per
  call), so notes.json keeps one writer; with Yap closed or the Local API off, tools
  answer "Open Yap to let your AI read your notes". Tools, meetings + notes only,
  **never dictation history**: `list_meetings`, `search_meetings` (matching
  transcript lines + page), `get_meeting` (AI summary/action plan, typed notes,
  digests when there's no summary, transcript in ~6k-token pages via
  `transcript_page`), `search_notes`, `get_note`, `list_folders`, and `create_note`
  only when `config.mcp_allow_writes` (Settings → MCP; source `"mcp"`). Markdown
  for the model, local-time dates (chrono). Transcript lines carry the
  speaker map's names (`speakers::display_line`: "You", "Them", "Priya",
  "Priya?" for a guess, "Speaker 2"), and the instructions/page explain the
  labels whether or not voices are told apart (`who_is_who`); the local API's
  `/v1/notes/<id>` hands out `speakers` and each line's `speaker` as stored.
- **`mcp_clients.rs`** — "Add to Claude / ChatGPT / Gemini / Cursor…" (Settings →
  MCP): edits each AI app's own config to launch `yap.exe mcp`. Claude desktop
  (`%APPDATA%\Claude\claude_desktop_config.json` **and** the MSIX build's
  `%LOCALAPPDATA%\Packages\Claude_*\LocalCache\Roaming\Claude\…`), ChatGPT desktop +
  Codex (`~/.codex/config.toml`, `[mcp_servers.yap]` via toml_edit), Gemini CLI
  (`~/.gemini/settings.json`), Cursor (`~/.cursor/mcp.json`), Claude Code
  (`~/.claude.json`, under Claude Code's own proper-lockfile lock), VS Code
  (`%APPDATA%\Code\User\mcp.json`, `servers`) and Windsurf/Devin Desktop. Only the
  `yap` entry changes (other entries kept as their original text, so key order,
  indent, CRLF and BOM survive; remove = same bytes back), non-plain JSON is left
  alone with the reason, `.bak` on the first edit, temp-file + rename writes.
  Detects installed apps; status added / available / outdated (exe gone) /
  missing. Commands `mcp_clients_status`, `mcp_client_add`, `mcp_client_remove`
  (async, blocking IO off the main thread). Debug builds take
  `YAP_MCP_CLIENT_ROOT` as a stand-in user profile; test mode never touches the
  real one. ⚠ The exe written is this one, **except in a dev build** (debug, not a
  test run): there it's the installed Yap's (`installed_exe`, the uninstall
  entry's `DisplayIcon`) when one is installed — an AI app keeps `yap.exe mcp`
  running, and a child of `<drive>:\t\debug\yap.exe` locks it, so Yap - Dev's next
  build fails with "Access is denied" (2026-10-17, Claude Code's `~/.claude.json`).
  The launcher (`dev-update.ps1`) also stops any such helper before building.
- **`auth.rs`** — Yap accounts (optional; nothing in dictation depends on it).
  Email codes: `auth_email_send`/`auth_email_verify` call the account service
  directly. Google/GitHub/Discord: `auth_start` opens the system browser on
  the server's `/api/auth/electron/init-oauth-proxy` with a PKCE challenge +
  `state`; the account page hands back a one-time code by **deep link**
  (installed builds — `is_registered` must point at *this* exe), a one-shot
  **loopback** listener (dev/portable — the port rides at the end of `state`
  as `-<port>`), or **paste** (`auth_submit_code`); `redeem` swaps it for a
  session at `/electron/token`. **Phone** (RFC 8628 device authorization):
  `auth_device_start` gets a code from `/device/code` (client `yap-desktop`),
  checks both links stay on the service's origin, draws the QR **locally**
  (`qrcode` crate, encoder only → one SVG path; `Status.device` carries it +
  the grouped code `WDJB-MJHT` + expiry + `expired`) and polls `/device/token`
  at the server's interval (`slow_down` adds 5 s; one attempt at a time — a
  newer one or `auth_device_cancel` retires the poller; the device code never
  leaves Rust). A **lapsed** code (`expired_token`, or Yap's deadline passing
  while the service still answers) is no dead end: polling stops and the view
  stays with `expired: true`, **no** `yap-auth-error`; the page asks for the
  next code with `auth_device_start({renew: true})`, which does nothing once
  the attempt was cancelled or signed in. Approval answers with an ordinary
  session token → `get-session` → `finish_sign_in`, the attempt marked
  approved meanwhile so no renewal can replace (and discard) it. Denied /
  code gone / unreachable → `yap-auth-error`; approved after a cancel, or a
  session `get-session` can't put a name to → that session is revoked.
  `finish_sign_in` (every route) drops any other sign-in left waiting
  (browser or phone), emits signed-in at once and fills in the linked
  providers after. Every route's token (+ a profile copy) is stored in
  Windows Credential Manager (`yap-account.com.yap.dictation`, **Local**
  persistence — never config.json, logs or the webview; any non-production
  service URL gets its own entry, `yap-account@<host>`, so a dev build can't
  touch the real session). Signed in, startup + every 24 h
  `refresh` re-validates via `get-session` (server says no → signed out;
  unreachable → keep, flagged `offline`). `auth_sign_out` revokes best-effort
  then forgets locally; `auth_delete_account` returns `"reauth"` when the
  server wants a fresh sign-in (sessions > 1 day old). A phone sign-in under
  a day old can't delete the account or sign devices out (403
  `NEW_PHONE_SESSION` → a friendly "sign in another way" error). A fresh sign-in that
  replaces this PC's session (that re-auth) revokes the old one server-side,
  so it can't linger as a phantom device. **Where you're signed in**:
  `auth_list_sessions` (the service's own `GET /api/account/sessions` →
  `{id, current, label, createdAt, lastActiveAt, expiresAt}`, unix secs, this
  PC first; `label` parsed from the session's user agent — "Yap 0.1.1 on
  Windows", "Chrome on macOS"), `auth_revoke_session(id)` (`POST
  /api/account/sessions/revoke`, refuses this PC) and
  `auth_revoke_other_sessions` (Better Auth's). The service's endpoints work
  for a session of any age and send no tokens or IPs (Better Auth's own
  `/list-sessions` wanted a sign-in < 1 day old and handed out every
  session's token; it's disabled). A 401 from these re-checks via
  `get-session` and signs out only if the session is really gone. Emits
  `yap-auth-changed` (status snapshot) and `yap-auth-error`. The Account page
  only offers what the service reports at `/api/providers` (configured
  providers; email once mail can be sent) — `auth_check_methods` when the page
  opens, plus daily while signed in. ⚠ A **signed-out Yap never contacts the
  account service on its own** (the privacy promise in the README, the
  release notes and SignPath's policy) — keep any new auth call behind a
  sign-in or a user action. Debug builds talk
  to `http://localhost:8787` (`cloud/`'s `wrangler dev`); `YAP_AUTH_URL`
  overrides any build.
- **`history.rs`** — local-only transcription history (`history.json`): each
  dictation's timestamp, raw + final text, model, and focused app. Best-effort,
  gated by `history_enabled`. Derives the stats dashboard (words, time-saved vs
  typing, day streak, 30-day activity) without a date crate (UTC day-numbers like
  `usage.rs`). Powers `get_history`/`clear_history`/`get_stats`. Every change
  (a dictation or upload saved, an entry deleted from Home or the local API,
  all cleared) emits **`yap-history-changed`** after the write, which Home and
  Insights re-pull on. Not `yap-transcript`: a dictation emits that before it
  pastes and saves, so a re-pull on it could miss the new entry.
- **`usage.rs`** — daily Groq usage tracker (tokens summed locally + requests from
  `x-ratelimit-*` headers), persisted to `groq_usage.json`, auto-resets at midnight
  UTC; powers the `get_groq_usage` command + `groq-usage` event.
- **`updates.rs`** — a running Yap notices updates by itself (OpenWhispr
  `updater.js` cadence, owned by Rust because the settings webview is usually
  hidden + timer-throttled). Scheduler: first check 30–60 s after launch, then
  every 4 h ± 20 min, waking ≤ every 5 min and comparing the **wall clock** (a
  slept-through check runs minutes after resume); showing the main window
  re-checks when the last check is > 1 h old; failures retry in 30 min; gated by
  `update_checks_enabled` (manual checks always work). Installed builds then
  **download in the background** (skipped on a metered connection —
  `Windows.Networking.Connectivity`), keeping the verified installer in memory,
  so "Restart to update" is instant. One snapshot (`update_status` /
  `yap-update` event: idle | checking | available | downloading | ready |
  installing + version/notes/progress/error/deferred/…) feeds every surface;
  its `rev` goes up with every broadcast, and the page never applies an
  older one, since a command reply (`update_check`'s snapshot) can land
  after a newer event (a quick download already "ready").
  **Announcements** (`updates.json`): once per "pending update" episode. A
  check the user starts (About, status bar, tray → About) IS the announcement
  (`run_check` marks it): the result shows where they asked, no toast or
  Windows notification repeats it. Otherwise the
  in-app toast if the main window is visible (page acks via `update_ack`),
  else one **silent Windows notification** (WinRT `ToastNotification` under
  the app identifier = the NSIS shortcut's AppUserModelID; dev builds borrow
  PowerShell's; Yap's logo in the `appLogoOverride` slot — an unpackaged app's
  toast only loads local files, so the binary writes `icons/128x128@2x.png` to
  `<data>/notification-logo.png`; Restart / Later buttons + body → Settings →
  About; if Windows refuses it, the toast waits for the window; a download the
  user asked for while hidden — metered "Download and restart", the tray's
  "Download and install" — gets a toast with a live **progress bar**, data-bound
  and updated in place (`notify::progress`, ≤ 2/s; `sync_progress` on every
  state change fills it on "Restarting…" or removes it on failure; background
  downloads stay silent); a newer version replacing a
  pending one stays quiet; one reminder after 3 days. **Install**
  (`request_install`, from toast/About/tray/notification): never mid-dictation
  (deferred until the pipeline goes idle + 2.5 s, via the `yap-state` hook),
  refused with a message during a meeting recording or model download
  (`yap-update-blocked`); the updater's `on_before_exit` runs
  `shutdown_cleanup` + saves window state, because on Windows `Update::install`
  launches the installer (passive, `/R` relaunch) and `std::process::exit`s
  without the Exit handler. A restart marker lets the relaunched Yap reopen the
  window if it was open and toast "Updated to X". Deliberately **no
  install-on-quit**: the passive installer's `/R` would relaunch a Yap the
  user just quit, so a downloaded update stays one click away (and the next
  launch re-finds it). Nightly and stable announce alike (once per episode;
  a daily nightly user who keeps updating hears about each build once). The
  "What's new" notes are latest.json's `notes`, written by
  `scripts/release-notes.mjs` in both release workflows. Portable builds get
  "Get it on GitHub". Dev builds never auto-check — debug-only
  `YAP_UPDATE_TEST_ENDPOINT` (+ `_PUBKEY`, `_PORTABLE`, `_METERED`) point them
  at a local `latest.json`; the installer itself never runs from a dev build.
  Talks to GitHub Releases only (never the account service). ⚠ WebView2 keeps
  `document.visibilityState === 'visible'` while a Tauri window is **hidden**
  (`hide()` doesn't touch the controller's visibility) — so window visibility
  comes from Rust (`is_visible()`), and "the window was shown" from
  `WindowEvent::Focused(true)` on the settings window (lib.rs →
  `updates::on_main_window_focused`), never from the page.
- **`win_toast.rs`** — the shared Windows-notification plumbing (WinRT
  `ToastNotification`, Windows only): `app_id` (the NSIS shortcut's
  AppUserModelID; dev builds borrow PowerShell's), `logo_xml`, `esc`, and
  `post(tag, xml, data, on_activated)` / `update` / `remove` in the "yap"
  group, plus `post_quietly` (no banner — `SetSuppressPopup`, straight into
  the notification centre; the call prompts' "Quietly" style) and `build`
  (the toast, unshown — what the unit tests check). updates.rs posts under the
  "update" tag (always with a banner), meeting_detect.rs under "call",
  meeting_guard.rs under "meeting-limit" (Keep going) and "meeting-notice"
  (Update settings / Open note), calendar.rs under "calendar" (the reminder
  card's buttons), alert.rs under the alert's kind; each keeps its live
  toast so the buttons work from the notification center. `post` goes
  through the notice policy: the same notification again within its kind's
  cooldown isn't posted, and it skips the banner (`notice::native_quietly`)
  while a dictation records or `SHQueryUserNotificationState` says busy,
  unless it's an alert or a question. `allowed()` = not portable, not a test
  run.
- **`notice.rs`** — **Yap's notice policy** (Wispr Flow's rules, by
  behaviour), the one place that decides how loud a notice may be; the page
  runs the same rules (`ui/noticePolicy.js`). **One place**
  (`place_for`/`place`): the main window's toast while it's focused, else
  the Yap bar, else a Windows notification, else the window if it's on
  screen — used by alerts, coaching, learn, call prompts, calendar cards,
  the length warning and meeting notices (`window_view`: test runs say for
  themselves whether the window counts as focused, `e2e_focus`). **Rules per
  kind** (`rule`: a bar card's id / a notification's tag): priority (Prompt —
  call, calendar, meeting-limit, meeting-checkin — > Alert — micBlocked,
  pasteBlocked, calls-fatigue, assistantNoModel, and the Voice Assistant's
  answer `assistant` (never held, never stale) — > Info > Tip — hint, word-suggestion), held
  while dictating (+ `SETTLE_MS` 1.5 s, and while onboarding is on screen)
  or a meeting records, a cooldown for the same text (micBlocked and
  meeting-notice 10 min), how long it may wait unseen (tips 10 s, a word
  suggestion and notices 60 s; questions as long as it takes), whether a
  sticky offer yields to its equals (meeting-mistake, update), whether "Show
  tips" mutes it. **`pick`** chooses the one card on screen (rank, then the
  one up, then the oldest; a card the pointer is on stays) and the stale
  ones to drop; **`Ledger`** the cooldowns (`admit`); **`Fatigue`** the call
  prompts' 3 / 5 / 35 rule (`notices.json`, `with_call_fatigue`). The moment
  comes from `on_pipeline_state` (lib.rs's `yap-state`), `on_meeting` (the
  bar's `yap-meeting-state`), `on_onboarding` and `sync` (config: `show_tips`).
  All pure parts unit-tested. Debug-only `e2e_notice_reset` (the `main`
  fixture runs it before every test), `e2e_focus {focused}` and `e2e_notice
  {surface: bar|window, kind, title, body, level, timeoutMs, dictating}`
  (a notice through the real policy, or a pretend dictation's `yap-state`).
- **`alert.rs`** — one-off notices the person must act on (`elevated.rs`'s
  "Your text is copied: press Ctrl+V", `mic_privacy.rs`'s "Windows is blocking
  microphone access…", call-prompt fatigue's "Stop asking about calls?" /
  "Yap stopped asking about calls", the Voice Assistant's "Pick an AI model
  for the Voice Assistant" with Open settings → Language Models → Chat &
  Voice Assistant): in one place (`notice::place`) — a
  toast in the main window while it's focused (`yap-alert` →
  `lib/alerts.js`, kind `alert`), else a Yap bar card, else a Windows
  notification, else the window on screen; the same alert again within its
  kind's cooldown isn't shown (the mic one: once per 10 min; the overlay's
  line says it every press). One optional button (`Action`: **Open
  settings**, **Stop asking** / **Turn back on** →
  `meeting_detect::set_detection`) comes back to `act` from all three
  (`alert_action` from the toast). `show_soon` runs it on a thread of its
  own (window queries; a hotkey's handler may hold the pipeline's locks).
  Test mode only logs what Open settings would open.
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
  `nemotron-3-diarization`), call detection:
  meeting_detection + meeting_detect_style ("popup"|"quiet") +
  meeting_detect_apps (app id → bool overrides of `meeting_detect::APPS`'
  defaults) + meeting_auto_start (the opt-in 10-second countdown, off),
  meeting guard rails: meeting_hide_from_capture (true),
  meeting_max_minutes (120; 0 = no limit), meeting_call_end ("ask"|"stop"),
  meeting_end_when_quiet ("End notes when the meeting goes quiet", true),
  meeting_hotkey (`kb:alt+win+77`), meeting_reminder (the calendar's card:
  "15s" | "1min" | "2min" | "never"), bridge_enabled, mcp_allow_writes (AI apps
  may save notes over MCP; default off), the Yap bar: bar_enabled (true) +
  bar_hide_fullscreen (true) + bar_theme ("light" | "dark", light) +
  bar_dictation_buttons (the capsule's ✕ / ✓: "off" | "hover" | "always",
  off), spoken replies (`tts.rs`): speak_replies (off), speak_when ("voice" |
  "always", voice), talk_back_hands_free (true; only while speak_replies is
  on), tts_engine ("kokoro" — Windows voices until it's downloaded — or
  "windows"), tts_voice (Kokoro's, "af_bella"), tts_windows_voice ("" =
  Windows' default), tts_rate (1.0, 0.5–2.0), tts_volume (0.5: Yap's own
  gain on the voice, squared — `tts::gain`, half = −12 dB, 10 % = −40 dB —
  under the Windows mixer, live mid-sentence via the rodio sink's volume;
  `tts_volume_set {volume, save?}` → `yap-voice-volume`); config v2 moved a saved
  Windows voice from tts_voice to tts_windows_voice and "windows" (never a
  choice before) to "kokoro"), the Voice Assistant's assistant_hotkey (""
  = unbound, the default), opening apps (`apps.rs`): open_apps (true) +
  apps_blocked (app ids, "Don't open this one") + app_nicknames
  (`AppNickname {say, app, name}`), the web (`web.rs`): web_search (false) +
  web_search_native (true: cloud models use their provider's own search) +
  web_search_service ("exa" | "tavily") + web_search_keys (service → key),
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
- **`fuzzy.rs`** — fuzzy dictionary correction (Handy `audio_toolkit/text.rs` port):
  1–3-word n-grams vs the dictionary's `from`/`to` spellings, normalized Levenshtein
  + Soundex phonetic boost (the `natural` crate's nonstandard variant, replicated
  exactly), threshold 0.18, ≥3-char terms, case/punctuation preserved; shortest-first
  n-gram choice (deliberate fix over Handy's word-swallowing longest-first greedy),
  and never a run whose first word adds nothing (`first_word_adds_nothing`: the
  later words make the same term as closely alone), so the word or list marker
  before a term survives ("to Kubernetes", "1. kubernetes"; found by the real-app
  robot, 2026-10-17). Max 3 words per n-gram, so "Chat G P T" (4) leaves a "T".
  Runs after `apply_dictionary` for ONNX models (`dictionary_fuzzy`, default on);
  snippets are never candidates.
  Also `is_prompt_echo` (OpenWhispr `dictionaryEchoFilter` port) used by `stt.rs`
  against Whisper hallucinating the dictionary prompt on silence, and, for
  `learn.rs`, `spelling_distance` (Handy's score on the letters alone, no
  length gate) and `sound_distance` over `sound_key` (a Soundex/Metaphone-
  style consonant key: alike sounds merged, silent letters and vowels
  dropped — "cube and eighties" ≈ "Kubernetes", "kpnts"/"kprnts").
- **`dict_usage.rs`** — how much each dictionary word is used, for the Whisper
  prompt's order (Wispr Flow ranks its vocabulary the same way):
  `<data>/dictionary_usage.json`, word (lowercased `to`) → `{uses, lastUsed}`
  (unix secs) — its own store, never config.json (Settings' wholesale saves
  would put old counts back); cached in memory, atomic write, corrupt file
  quarantined. `record(dict, text)` (from `apply_corrections`: dictations,
  meeting lines, Upload) counts every dictionary word in the corrected text —
  a correction that fired, or a word the recognizer (with the prompt) spelled
  right by itself; a no-op in unit tests. `rank` (pure) / `ranked`: used in
  the last day, then 14 days, then the rest; within each most uses first; then
  the order added. The meeting prompt ranks the dictionary but keeps the
  attendees ahead of it. Best-effort everywhere.
- **`learn.rs`** — **"Suggest words from your corrections"**
  (`config.learn_words`, default on; Wispr Flow's dictionary auto-learn as
  measured from its shipped code — `references/wispr-flow/rea-teardown/
  05-dictionary-learning.md` — and OpenWhispr's `correctionLearner.js`
  alignment, but local and asked, never assumed). **The watch**
  (`after_paste`, from `run_stt` after a paste with no submit key, not into
  a terminal (`app_kind`: its text is the program's output too); never in a
  test run; the rules are `Watch`, pure): a thread holds the focused field
  (`selection::WatchedField`: the target window's process, not a password
  box, not read-only) and reads it **whole** (≤ 15,000 chars, the caret in
  it), finds the pasted text right after the paste (the occurrence at the
  caret) and remembers ~24 chars either side of it (`Anchors`, with an
  8-char fallback when those were edited), then every 700 ms (reads ≥ 400 ms
  apart) for **60 s** finds the span between them again — **focused or
  not** (no end on a focus/app change: a fix often follows a glance
  elsewhere). Each read's span within the **drift gate** (`within_drift`:
  char Levenshtein, lowercased, ≤ `0.4/(len/30+1)+0.2` of the paste's
  length) becomes the **last good edit**. It ends (`End`, named in the
  debug log: `time_up`, `next_dictation` (`pipeline::dictation_state`),
  `field_emptied` (a chat message sent), `region_emptied` (the dictation
  deleted), `region_gone` (its neighbours deleted), `anchor_mismatch`,
  `enter_pressed` (a line break added right after it), `unreadable` (3
  failed reads in a row), `too_long`) and, **whatever the end**, the last
  good edit is judged — "fix a word, press Enter" still counts (a fix and
  the Enter in one read too). In memory only; logs carry lengths, counts
  and the end's name. **The candidates** (`suggestions`, pure,
  unit-tested): pasted vs edited
  words aligned by LCS (case- and edge-punctuation-blind); the gaps between
  matches are the changes. Kept: 1–4 words → 1–4 words (spelled-out letters
  count as one) whose new text looks like a term — a capital inside a word,
  an acronym, digits, a word that isn't an everyday one (the built-in
  `FREQUENT` list with simple stemming, then **Windows' spell checker**,
  `ISpellChecker` for the user's locale + English, read-only), a capital
  mid-sentence, or several words heard as one non-everyday word ("power to
  keep" → "Parakeet") — and that spells or sounds like what Yap typed
  (`fuzzy::spelling_distance`/`sound_distance` ≤ 0.5, ≤ 0.3 for keys of ≤ 3);
  plus a case change that makes a term of a non-everyday word ("github" →
  "GitHub"). Skipped: insertions/deletions, punctuation, a new word ending
  in a contraction/possessive (`'s 't 'll 've 're 'd 'm` or a bare `'`;
  "O'Brien" is fine), a single everyday
  word heard ("motion" → "Notion" would rewrite every "motion"), only
  function words, terms (or triggers) already in the dictionary, and edits
  rewriting most of the dictation. One per spelling (case-blind), terms
  with a capital first, ≤ 3. **The prompt** (`offer`): "Add
  “Kubernetes” to your dictionary? Yap heard “cube and eighties”." — Add /
  Not now / "Don't suggest this"; in one place (`notice::place`): a toast
  (`yap-word-suggestion`, `wordSuggestions.js`, kind `tip`) while the main
  window is focused, else a Yap bar card (`word-suggestion`, icon
  `dictionary`; ✕ = Not now, 30 s fade = no answer; it waits out a
  dictation, up to a minute), else the window's toast if it's on screen;
  neither → nothing (never a Windows notification). Add
  (`word_suggestion_respond`) appends `{from: heard, to: term, fuzzy: true}`,
  saves config.json, updates the pipeline's live config and emits
  `yap-dictionary-learned {entries, added}`, which Settings adopts (and
  passes to DictionaryView as `yap-dictionary-external`). Not now twice (✕
  counts) or "Don't suggest this" retires the pair; one prompt per 3 min.
  **A word you remove** from the dictionary is never suggested again,
  however it's heard (Wispr's implicit blocklist): `commands::save_config`
  (every surface's save) hands the old and new dictionaries to
  `on_dictionary_saved`, and a correction whose trigger and spelling are
  both gone (an edit of one isn't a removal; snippets don't count) goes
  into the store's `removed` terms. Answers in
  `<data>/word_suggestions.json` (not config.json: Settings' wholesale
  saves). Debug-only `e2e_learn_correction {pasted, edited?, before?,
  reads?, fresh?}` stages an edit for the e2e suite (skips the 3-min gap):
  the edited span, or `reads` — the field's whole text read by read (`null`
  = a failed read) — which the real `Watch` runs over (`staged_watch`).
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
  `text_injector::paste_last_from_tray`, worker thread). All three are in the
  idle menu's cache key (`idle_menu_key`), so the menu rebuilds when they
  change; all tray work runs on the main thread.
- **`bar.rs`** — the **Yap bar** (2026-10-05, after Wispr Flow's Flow Bar —
  `E:\Projects\references\wispr-flow\README.md`, screenshots 20–22, 25): the
  `overlay` window as one fixed-size (520×560 logical), transparent,
  always-on-top tool window, drawn to Wispr's measured Flow Bar
  (`E:\Projects\references\wispr-flow\flowbar-spec.md`). **Idle** a 40×8
  pill outline 14 px above the work area; **hover** opens it into 🎤 Dictate (`toggle_recording`, tooltip
  "Dictate · F9"), ◉ Meeting notes (`meeting_guard::start_or_stop`, as the
  meeting shortcut: the live call, else a new meeting note; while recording,
  `meeting_end::end`) and a ^ menu (Open Yap, New meeting note, Settings,
  Hide the bar for 1 hour, Turn off the bar — `bar_action`); **dictating** =
  the dictation overlay (waveform, live partials, Transcribing…, errors), with
  ✕ (`dictation-cancel`) and ✓ (`dictation-stop`: stop and paste, as the
  hotkey's second press — into the app the dictation started in) while it
  records, as `bar_dictation_buttons` says (`Status.dictationButtons`):
  **"off"** (the default, Nathan's call: Cancel stays in the tray), "hover"
  (only while Rust's pointer is on the capsule, which is then a region) or
  "always"; never while the push-to-talk key is held (`yap-dictation-held`,
  from the press until it locks hands-free), as Wispr's bar draws them only
  hands-free; and after a cancel "Dictation
  cancelled · Undo" (`yap-state` "cancelled", `dictation-undo`, whatever the
  setting) while the audio is kept;
  **meeting** = a 69×30 recording pill at half opacity (5 bars, the stop
  circle; hovered, full opacity and the timer; body → `notepad::open`, ■ →
  `meeting_end::end`). **Click-through except over what
  it draws**: the page reports its interactive rects (`data-region` →
  `bar_regions`, CSS px) and a poller (`GetCursorPos` every 250 ms, 30 ms
  while the cursor is near) hit-tests them (`overlay::region_at`, per-monitor
  scale) and clears `WS_EX_TRANSPARENT` only while the cursor is on one,
  telling the page (`yap-bar-pointer`; a click-through window gets no
  mouseleave, so Rust is the authority on hover) — Electron's
  `setIgnoreMouseEvents(true, {forward:true})` done natively, as Wispr's
  traced bar does. **Never focused**: `focusable: false` (WS_EX_NOACTIVATE),
  a window subclass that answers `WM_MOUSEACTIVATE` with MA_NOACTIVATE and
  keeps TOOLWINDOW/NOACTIVATE/LAYERED through tao's style rewrites
  (`WM_STYLECHANGING`), and if Windows activates it anyway (`WM_ACTIVATE`)
  the focus goes back to the app you were in — so 🎤 dictates there.
  **Follows the cursor's monitor** (not the foreground window) after 200 ms
  on it, bottom-centre on the work area (`overlay::place`: taskbars on any
  edge, an auto-hidden one kept clear, per-monitor DPI with a re-place after
  a DPI-changing move, unplugged monitors), frozen while a dictation records
  or transcribes. **Fullscreen** (`bar_hide_fullscreen`, on;
  `overlay::fullscreen_on` = the foreground window covering the whole monitor
  and not maximised/shell/Yap's own, or `SHQueryUserNotificationState`
  D3D-exclusive / presentation mode): the idle pill hides; a card still shows
  over a borderless app, but waits (queued, unseen) out an exclusive one; it
  always shows while recording. **Cards** (`show_card`/`update_card`/
  `dismiss_card`, `Card` + an `OnAction` callback; answers come back through
  `bar_card_action`): Yap's notices while the main window isn't focused,
  instead of a Windows notification — `false` when the bar is off or hidden
  for an hour, and the caller posts the notification as before. **One card
  at a time** (`notice.rs`): every card asked for goes into `cards` with
  when it came (`queued`); `repick` (on every change, dictation state,
  meeting state and poller tick) puts the one `notice::pick` chooses on
  screen (`front`: a question before an alert before a notice before a tip,
  then the one up, then the oldest; a card the pointer is on stays), holds
  tips and notices while a dictation runs, drops ones that waited too long
  unseen (their `expire_action` reported) and the same notice again within
  its cooldown (`show_card` still returns `true`: it was said). The page
  gets only the front card (`bar_status.cards`, the rest in `waiting`).
  Users:
  `meeting_detect` (call prompts as Wispr's "Meeting detected" card, "Taking
  notes…", failures), `meeting_guard` (length warning with Keep going,
  the quiet countdown and the check-ins, notices), `updates` ("Yap X is ready"), `calendar` (the reminder before a
  meeting, as a call card), `coaching` (push-to-talk tips), `learn` (a word
  to add to the dictionary), `alert` (a paste into an admin app, Windows'
  microphone privacy), `assistant` (the Voice Assistant's answer, style
  "answer"; Esc stops its voice). The bar also follows `yap-assistant`
  (`State.assistant`, in `Status.assistant`): the window shows while a
  question is asked, answered or read aloud, bar on or off, and ■ on the
  capsule is `bar_action "assistant-stop"`. An Esc watch (a low-level keyboard
  hook that only looks) runs only while an on-screen card offers Esc (the
  auto-start countdown, the assistant's answer). Debug-only `bar_simulate` (pretend cursor on a
  region, fake fullscreen kinds, Esc, demo cards) and `bar_debug` (the real
  ex-styles, WindowFromPoint on the pill, the thread's active window) for the
  e2e suite; a test run's bar only sees `bar_simulate`'s cursor unless it's
  started with `YAP_E2E_REAL_CURSOR=1` (debug-only, `e2e::real_cursor`: the
  real one, for checking a real click reaches it — never while Nathan's
  using the mouse). Every card answer (button, ✕, Esc, its timer) is logged
  as "bar: card answered" with the card and action; the hover check logs at
  debug when it changes. Settings → General → Yap bar (`bar_enabled`, `bar_hide_fullscreen`, `bar_theme` Light / Dark,
  `bar_dictation_buttons`; `overlay_position` puts it at the top). Bar off = the old behaviour: the
  overlay only while dictating, notices as Windows notifications — but a
  showing dictation's ✕ / ✓ and Undo still take clicks (`clickable`).
- **`overlay.rs`** — the bar window's Win32 side + its pure, unit-tested maths:
  `place` (work-area placement), `fullscreen_on` (None / Borderless /
  Exclusive), `region_at` (hit-testing with slack), `enforced_ex_style`, and
  `win::` (subclass install, click-through toggle, monitors via
  `MonitorFromPoint`/`GetMonitorInfoW`/`GetDpiForMonitor`, auto-hidden taskbars
  via `ABM_GETAUTOHIDEBAREX`, the foreground window, `force_topmost`).
- **`context_menu.rs`** — no browser context menu in Yap's windows: each
  webview's `ContextMenuRequested` (WebView2, via `webview2-com`) keeps only
  the editing commands, or shows nothing (pure `keep`, unit-tested on the
  menus WebView2 really offered). Inspect: debug builds only, and only for a
  Shift+right-click (`offers_inspect(debug, shift)`; Shift read with
  `GetKeyState`/`GetAsyncKeyState`, compiled out of release). See "Window
  config" below.
- **`input_hook.rs`** — low-level Windows keyboard + mouse hooks; specs `kb:VKEY`,
  `kb:ctrl+shift+VKEY` (modifier combo; `win` is a modifier too, e.g. the
  meeting shortcut `kb:alt+win+77` = Win+Alt+M), `kb:165` (single right-side modifier, e.g.
  RightAlt — never suppressed, it's AltGr), `mods:ctrl+alt` (modifier-only chord) /
  `mouse:ID` — combo semantics ported from OpenWhispr's `windows-key-listener.c`
  (press = key down w/ required modifiers held, release = key up OR required
  modifier up; chords fire on completion; suppressed keys are excluded from the
  GetAsyncKeyState self-heal — the hook eats them before the key-state table
  updates). Four bindings: dictation, edit (`edit-key-*`, Commands' rewrite
  shortcut), the meeting
  shortcut (`meeting-key-*`, `configure_meeting`; only its press is used —
  `meeting_guard`) and the Voice Assistant's (`assistant-key-*`,
  `configure_assistant`; unbound by default; `configure_assistant_from`
  leaves one that clashes unbound). **`clashes(a, b)`** (pure, unit-tested;
  `hotkeysClash` in `lib/hotkeys.js` is the same rule, Settings refuses with
  it): the same key however written, combos on one key nesting (Ctrl+Space
  inside Ctrl+Shift+Space), a chord completing inside a combo or bigger
  chord; a bare key and its combos don't (a bare key fires only with no
  modifier held), unless it's a modifier itself. A combo holding **Alt or Win** whose main key is swallowed
  taps an unassigned **menu-mask key** (vkE8, AutoHotkey's `#MenuMaskKey`)
  while the modifiers are still down, sent by the forwarder thread
  (`needs_menu_mask`): otherwise releasing Alt opens the focused app's menu
  bar (Office KeyTips) and Win the Start menu. Emits press AND release (via an emit-forwarder thread — the hook
  callback never blocks — plus a 30 s re-hook self-heal), each with the moment the
  key moved as its payload (`gesture::now_ms`): events are handled one after
  another, and push-to-talk's tap timing mustn't stretch while the last one starts. The capture UI is
  `ui/HotkeyInput.svelte` + shared `lib/hotkeys.js` (parse/format/match — also
  drives the in-window fallbacks; combos read Win first, "Win + Alt + M"). ⚠ **Known Windows
  gotcha:** when one of Yap's OWN WebView2 windows has focus, the LL hook never
  receives the hotkey (WebView2/Chromium front-runs the hook chain on focus) —
  so the Settings + onboarding pages catch the hotkey **in-page** (keydown and
  keyup → `dictation_key { pressed }`, the hook's own path into `on_key`, so
  push-to-talk's gestures work there too; Settings and the notepad also catch the meeting shortcut →
  `meeting_shortcut`, and the assistant shortcut → `assistant_key { pressed }`). Any new Yap window with focusable UI needs the
  same fallback.
- **`text_injector.rs`** — clipboard paste (+ optional clipboard restore) and
  `press_submit` (Enter / Ctrl+Enter / Shift+Enter) via `SendInput`. **An app
  running as administrator** (`elevated.rs`, checked first — the paste's target,
  "Paste last transcript"'s, the submit key's window) gets no keys: the text is
  left on the clipboard (`leave_on_clipboard`, as privately as a paste) and
  `inject_text` returns `Delivery::LeftOnClipboard` for the caller to say so.
  Captures the
  dictation **target window** at record-start (`current_foreground`, skipping Yap's
  own windows) and **re-focuses** it before pasting (`focus_window`, via the
  `AttachThreadInput` workaround) so focus changes mid-transcription don't misfire.
  **Paste privacy** (`clip`, Win32 via the `windows` crate — arboard can't add
  formats): the dictation goes on the clipboard with
  `ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory` = 0,
  `CanUploadToCloudClipboard` = 0 and `Clipboard Viewer Ignore`, so it never lands in
  Win+V history, cloud clipboard sync or a well-behaved clipboard manager; the restore
  of the user's own clipboard carries them too (it's in their history already). A
  remote-desktop/VM target (`syncs_clipboard`: mstsc, msrdc, vmconnect, Citrix,
  VMware, VirtualBox, Parsec, AnyDesk…) gets a plain write — its clipboard sync is a
  monitor too. **Every write is verified** (`GetClipboardSequenceNumber` must move;
  4 tries, 0/15/40/80 ms), else the text is typed (`type_unicode`); outcomes at
  `debug`. The snapshot keeps **text, an image** (CF_DIBV5 + PNG, raw bytes) **or
  copied files** (CF_HDROP + drop effect), ≤ 64 MB, and an empty clipboard is left
  empty; `selection_via_copy` (edit-mode fallback) empties the clipboard, polls for
  the Ctrl+C result instead of a fixed sleep, and restores privately.
  `prepare_dictation(text, append_space, dictionary, kind)` (run_stt's one call)
  remembers the dictation and fits it to the kind of app (`kind`, `app_kind.rs`,
  already filtered by the settings: a terminal → `terminal_text`, nothing else; a
  chat app → `drop_chat_full_stop` first) and to the text around the cursor read
  at record start (`selection::take_context` → `smart_insert::fit_to_context`;
  dictionary spellings never lower-cased), or, with no context, appends the space
  as before. **"Paste
  last transcript"** (tray item + `paste_last_transcript` command → the text it
  pasted; errors "Nothing to paste yet"): this session's last dictation (unfitted),
  else history's newest dictation (`history::latest_dictation` — app an `.exe` or
  empty, so never an Upload); forgotten when history entries are deleted/cleared.
  It pastes into the foreground app, or — the tray menu holds the foreground — the
  top real app window (`pick_paste_window`, unit-tested: not Yap's, minimised, tool,
  no-activate, cloaked, untitled or a shell window), classifying it
  (`app_kind::of_window`) and re-reading the cursor's context there
  (`selection::read_context`, not in a terminal). Test mode returns the text without pasting
  ("e2e: not pasting the last transcript"). No hotkey for it yet. (Verify-after-paste
  over UIA is still a follow-up.) `#[ignore]`d `private_clipboard_round_trip` runs
  the real clipboard code on a private window station (never the person's clipboard).
- **`elevated.rs`** — apps running as administrator: Windows (UIPI) drops keys
  sent to a higher integrity level, and `SendInput` doesn't say so. The target's
  level is compared with Yap's (`OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)`
  → its token → `TokenIntegrityLevel`; a token Windows won't show plus a refused
  `PROCESS_QUERY_INFORMATION` = above Yap; anything unknown pastes as before);
  pure `blocks_input`, unit-tested (and against Yap's own token). Above Yap →
  the text stays on the clipboard (`text_injector`) and the dictation, the
  rewrite or "Paste last transcript" shows "Your text is copied: press Ctrl+V"
  (`alert()`, through `alert.rs`) — no submit key, no correction watch;
  history keeps it. Test mode reads no other process: debug-only
  `e2e_paste_elevated { on }` makes every target count as one (plain test
  mode just says so; robot mode really leaves the text on the clipboard). ⚠ A
  real elevated window can't be robot-tested: opening one takes a UAC prompt.
- **`selection.rs`** — UI Automation reads of the field Yap types into. **After
  the paste** (`WatchedField`, for `learn.rs`): the focused element held on
  the watch's thread, read whether it still has the focus or not;
  `read(limit)` → a `FieldWindow` of the **whole field** (TextPattern's
  document range, else the ValuePattern value; line breaks as `\n`, the
  caret's offset in it, an empty field empty (the stand-in rules below);
  longer than `limit` → its start with `to_end` false); same
  refusals (other processes, passwords, read-only, test runs), 300 ms
  timeouts. **Edit
  mode** (`capture_selection(target, copy_fallback)`): the selected text
  (TextPattern, else the Ctrl+C trick — never in a terminal with terminal mode
  on, where Ctrl+C would interrupt the program), 2 s UIA timeouts. **The text around the cursor** (`begin_dictation(target,
  read)` from `start_recording`, `take_context()` at paste): when `smart_spacing` is
  on and it isn't edit mode, a background thread reads ≤ 500 characters before and
  after the caret and the selection (a long selection as its start + end) —
  TextPattern (`GetSelection`, else TextPattern2's caret), else ValuePattern for
  Edit/Document/ComboBox fields — cut at word boundaries,
  with 200 ms UIA connection/transaction timeouts (`CUIAutomation8`). A
  ValuePattern has no caret: when the element's own window is a Win32 edit
  control (`edit_class`: `Edit`, `RichEdit20W`/`RICHEDIT50W`…, WinForms'
  `WindowsForms10.EDIT…`/`…RichEdit20W…` — a WinForms TextBox/RichTextBox has no
  TextPattern) the control is asked (`EM_GETSEL` via `SendMessageTimeoutW`, no
  pointers, `SMTO_ABORTIFHUNG`, the same time box; positions past 65,535 → unknown)
  and its UTF-16 units placed in the value (`units_to_bytes`: an EDIT's `\r\n` is
  two, a RichEdit's break one; a RichEdit's final paragraph mark dropped); any
  other value-only field (older Chromium/Electron, Firefox through the MSAA proxy)
  → caret taken to be at the end, where it is after typing. The watch's read finds
  its caret the same way. **Only the field's own text**: every range is kept inside the focused element's
  `DocumentRange` (`keep_within`; Chromium's ranges are page positions and ran on
  past a text box into the page), a selection outside it isn't read. **An empty
  field reads as empty** (`stands_in_for_empty`, pure): Chromium (Edge/Chrome/
  Electron/WebView2) gives an empty textarea/input/contenteditable an empty
  ValuePattern value but its accessible name ("Type a message") or U+FFFC as its
  TextPattern text — so a field's whole text (≤ 200 chars) that's only markers
  (U+FFFC, zero-widths)/whitespace, its placeholder (HelpText), or its label —
  its Name or the name of the element labelling it (LabeledBy) — unless
  the ValuePattern says those words are really there (Slate's in-editor
  placeholder: same words + U+FEFF) is empty; so is a web editor whose class says
  it shows a CSS placeholder, itself or its one line (`class_says_empty`: Quill's
  `ql-blank` = Slack, `is-empty`/`is-editor-empty` = ProseMirror/Tiptap,
  `ck-placeholder`, `placeholder`); a value-only field by its placeholder,
  markers or — an edit or a document, not a combo box — exactly its label. The
  post-paste watch applies the same rules. **A focused element with no text of
  its own** (an editor's inner part) is read through the text field it's part
  of: `field_for` walks up to 3 parents (`ControlViewWalker`), in the same
  process, never past a read-only element (`pick_field`, pure); edit mode's
  selection and the post-paste watch walk the same way. **Read-only** = the
  ValuePattern's flag **or** the legacy IAccessible state's
  `STATE_SYSTEM_READONLY` (`legacy_read_only`). Never: another
  process's element than the target window's, a password box, read-only text, or in
  a test run. The paste
  never waits: still reading → no context. Kept in memory for that dictation only;
  logged as lengths, never text. `#[ignore]`d `reads_real_text_fields` reads hidden
  EDIT and RichEdit controls end to end, through TextPattern and through the
  ValuePattern + `EM_GETSEL` path (mid-text, a selection, over a line break),
  and from a child window inside the field (read through it; not through a
  read-only one).
- **`smart_insert.rs`** — `fit_to_context(text, &FieldContext, names)`, pure and
  unit-tested (conservative: a wrong fix is worse than the old capital). Mid-sentence
  (before ends in a letter/digit/`,`/`;`/spaced dash — not `.?!`, `:`, a new line or
  a bullet) → lower-case the first letter only for a plainly sentence-case word
  (never "I"/"I'm", all-caps, "iPhone", a dictionary `to` term, or a word written
  capitalised mid-sentence nearby) that's a known ordinary word (`COMMON_WORDS`:
  English + common fr/de/es/it/pt/nl function words, no names like Will/May, no
  "El"/"Las"/"Los") or that the surrounding text writes in lower case — so names keep
  their capital. Text after the caret starting lower-case or with `,.;:!?)…` → drop
  one final `.`. Leading space only after a letter/digit/closing punctuation (a
  straight quote after a word, a spaced dash), never after whitespace, an opening
  bracket/quote or at the field start; trailing space only before a letter/digit;
  none next to Chinese/Japanese (`is_cjk`; Hangul uses spaces). Replacing a
  selection: its first word's case (same safety rules, or the same word again), its
  end punctuation, and any space it began/ended with. Edit mode and Commands
  by name paste the AI's text untouched, as before.
- **`app_kind.rs`** — **what kind of app a dictation goes into**, and what that
  changes about the paste (Wispr Flow's app categories + terminal/coding-CLI
  handling + chat full stops, by behaviour). `of_window(hwnd)` (at record start
  beside `target_hwnd`, → `Shared.target_kind`; also "Paste last transcript"):
  the window's process name (`text_injector::app_name_for`), class and title
  (`GetWindowTextW`, which never messages another process's window) — no UIA;
  logs the process + `describe()` at `debug`, never the title; a test run reads
  nothing (`Other`). Pure `classify(exe, class, title)` → **`Terminal { coding_cli,
  windows_terminal }`** (Windows Terminal — `CASCADIA_HOSTING_WINDOW_CLASS` —,
  conhost's `ConsoleWindowClass` whatever process it reports, cmd, PowerShell/
  pwsh, WezTerm, Alacritty, mintty/Git Bash, Tabby, Hyper, ConEmu, PuTTY/KiTTY,
  Warp…) | **`Messaging`** (Slack, Discord, WhatsApp, Telegram, Signal, Teams,
  Element, Mattermost…; or a browser whose title shows WhatsApp Web, Slack,
  Discord, Messenger, Microsoft Teams, Telegram, Google Chat/Messages — unread
  counts and Edge's "and 2 more pages" stripped) | **`Other`** (incl. Claude
  desktop, VS Code — its integrated terminal can't be told apart yet).
  **Coding CLIs** from the terminal's title, as they set it on Windows (2026):
  Claude Code "✳ <session>" idle / "◐"/"◑" working / "Claude Code" / "claude";
  Gemini CLI "◇  Ready (…)", "✦  …", "✋  …", "⏲  …", "Gemini CLI (…)"; Qwen Code;
  Codex a braille spinner + project while working (idle it's only the project:
  undetected → treated as a plain shell, the safe side); OpenCode "OpenCode" /
  "OC | …"; else the program as the title or cmd's "<title> - <command>" (aider,
  codex, gemini, copilot, cursor-agent, crush…; a folder path never counts).
  `with_settings(terminal_mode, chat_no_full_stop)` turns a kind off.
  **`terminal_text(text, keep_lines)`**: `\r\n`/`\r`/U+2028… → `\n`, tabs → a
  space, control characters out, trimmed; unless `keep_lines` one line
  (`join_lines`: bullets "- • * –" dropped and items joined "; " — "Fix these:
  the login bug; the search" —, a header without punctuation gets ": ", numbers
  kept, other lines joined with a space). `Terminal::keeps_lines` = Windows
  Terminal + Claude Code or Codex (bracketed paste / Codex's paste-burst check);
  every other CLI and host is joined until known safe. **`submit_key(kind,
  auto_submit, voice_enter, key)`**: in a terminal "enter", and only for the
  voice command or auto-submit in a detected coding CLI (never auto-submit at a
  plain shell's prompt); elsewhere as configured. **`drop_chat_full_stop(text,
  ctx, dictionary)`**: a final "." off a message of ≤ 2 sentences in one
  paragraph (`sentence_count`: abbreviations — "e.g.", "a.m.", "Dr.", "etc." —
  and initials don't end one), only at the end of the draft (nothing after the
  cursor, no selection being replaced, the draft before it counted, no line
  break in it); never "?", "!", "…", "...", an abbreviation's own full stop or a
  snippet saved ending with one. In a terminal with terminal mode on, the text
  around the cursor isn't read, edit mode never sends Ctrl+C
  (`selection::capture_selection(target, false)`) and the rewrite / Command
  reply goes through `terminal_text` too. An `#[ignore]`d `this_machines_windows`
  classifies the PC's real windows (`YAP_APP_KIND_TITLES=1` prints titles).
- **`sound.rs`** — start/stop chimes (volume + output-device aware); each
  takes an optional `Played` callback, run on the chime's thread once it has
  played (or at once when it can't): how `mute.rs` goes after the start
  chime and comes back after the stop chime. `open_output` (the configured
  output device, else the default) is shared with `tts.rs`.
- **`tts.rs`** — **spoken replies** (Settings → Voice Assistant, Chat's Voice
  replies toggle, ▶ on answers). **Engines** (`Engine` → `Synth`, text →
  `Pcm`; `prepare` (Kokoro joins sentences under 3 words to the next),
  `cancel`): **Kokoro** (`tts/kokoro.rs`, below; the default) and
  **Windows voices** (`WindowsVoices`: WinRT `SpeechSynthesizer`,
  `SynthesizeTextToStreamAsync` → WAV → `parse_wav`; rate via
  `SpeakingRate`, minimal appended silence). `chosen(cfg)`: Kokoro with
  `tts_voice` when `tts_engine` isn't "windows" and Kokoro can speak
  (installed, in this build), else Windows voices with `tts_windows_voice`
  — so Windows voices speak until Kokoro is downloaded, and Kokoro takes
  over by itself once it is unless Windows was chosen. **Which Windows
  voices apps see**: only the OneCore voices
  (`HKLM\…\Speech_OneCore\Voices`, e.g. this PC's en-GB George, Hazel,
  Susan — the default is George); not the old SAPI-only desktop voices
  (Zira here) and not Windows 11's natural Narrator voices (Aria, Jenny,
  Guy…), which are Narrator-only. **Text prep** (pure, unit-tested):
  `speakable_blocks` (markdown → one block per heading / list item / table
  row / paragraph, each ending like a sentence; a fenced code block →
  "(code block)", links → their text, bare URLs → their domain, `[x]`/☑ →
  "Done: …", emphasis, HTML, emoji and arrows out, "→" read "to", a web
  answer's `[1]` / `[2, 3]` / gpt-oss `【…】` markers never read), then
  **`tts/normalize.rs`** (`for_speech`, every engine, UK style,
  conservative — a clear pattern or nothing: £ $ € amounts incl. pence/cents
  and k/m/bn ("twelve thousand four hundred pounds", "three dollars fifty"),
  numbers with separators, decimals ("three point one four"), percentages,
  ordinals, dates ("18 Oct" → "the eighteenth of October", "October 14th" →
  "October the fourteenth", ISO and dd/mm/yyyy — month-first only when the
  day can't be a month —, "Sat" before a date, years after "in"/"since"/a
  month, "the 1990s"), times ("2:30 pm" → "two thirty p.m.", "14:00" →
  "fourteen hundred"), ranges ("10-15" climbing only), units after a number
  (km, kg, GB, °C, ms…), versions ("v2.1", "1.2.3"), "Q3" → "Q three", "4K",
  e.g./i.e./etc./vs./approx./Dr/Mr/No./#/&/w/, emails ("… at example dot
  com") and domains ("github dot com"), codes and long numbers digit by
  digit) and `sentences` (split after `.?!…` + space, never after an
  abbreviation or an initial — `app_kind::is_abbreviation` — over 220
  chars cut at a comma); `utterances` = all three. **The speaker**
  (`Speaker`, unit-tested on a simulated clock): one utterance at a time on
  a worker thread — "speaking" at once, the output opened on the worker
  (rodio on `config.output_device`), each sentence made while the one before
  plays; `stop()` empties the sink and drops the queue at once; a new
  utterance replaces the old; it checks before every sentence and while one
  plays that no dictation records (`pipeline::dictation_state`), refuses to
  start over one, and refuses while a meeting records (the loopback would
  put Yap in "Them"; `meeting::start` stops it). `yap-tts-state
  {speaking, messageId, error}` on every change. **When to speak** (pure):
  `auto_speak(speak_replies, speak_when, asked_by_voice, dictating,
  meeting)` (chat_send's `speak`), `sends_by_itself(speak_replies,
  hands_free, said_enter)` — the Voice Assistant's answers ignore both
  (`assistant::reply`), and every state change is passed to
  `assistant::on_tts_state` (its answer ended). Commands `tts_voices` (`{engine, active,
  engines, kokoro: {voices, defaultVoice}, kokoroStatus, windows: {voices,
  defaultVoice, error}}`), `tts_speak {text, messageId}`, `tts_preview
  {voice, rate, engine?}` (Kokoro not installed → "Download Kokoro to hear
  its voices"), `tts_stop`, `tts_state`, `tts_kokoro_status` /
  `tts_kokoro_download` (`tts-download-progress {percent, downloadedBytes,
  totalBytes}`, then `tts-kokoro-changed`) / `tts_kokoro_delete` (stops
  speaking, unloads), `voice_replies_set {on}` (Chat's toggle: saves like
  Settings, then `yap-voice-replies {on}` for Settings' copy). A
  `tts-idle` thread unloads Kokoro after the speech model's unload time (10
  min when that's "never"). ⚠ **No echo cancellation**: Yap's voice from
  speakers can reach the mic; the barge-in stops it before a dictation and
  drops the pre-roll, but a mic hearing the speakers (no headset) during an
  answer is a later job. Test mode: stand-in Windows voices, Kokoro as
  silence once installed from the suite's local server, a simulated clock
  (`SimPlayback`), no device; what would have been said (with its `engine`)
  goes to the debug-only `tts_e2e_spoken {clear?}`. An `#[ignore]`d
  `windows_voices_on_this_pc` lists the voices and synthesises a sentence
  to a WAV in a temp folder (never played).
  - **`tts/kokoro.rs`** — **Kokoro-82M v1.0** (hexgrad, Apache-2.0), the
    natural voice: onnx-community's **fp32** export (the fp16 one turns
    longer British sentences into NaN — `checks::nan_probe`; int8 is 3.5–4×
    slower on x86; DirectML can't run it) on Yap's own `ort` (pinned
    `=2.0.0-rc.12`, the `engines` feature, so one ONNX Runtime), **CPU**,
    `min(4, logical cores / 2)` intra-op threads that never spin, at
    **24,000 Hz** (Voice Mirror's 22,050 played it ~8% slow and low).
    Loaded lazily the first time it speaks (~0.6 s), kept warm, unloaded
    when idle (`idle_unload`, never under an open synthesiser); every run
    has its own `RunOptions`, so `cancel` (a stop, a new answer, a dictation
    starting: `Speaker::stop` → `Engine::cancel`) ends a sentence being made
    within ~10 ms. 54 voices (`VOICES`, `voices/<id>.bin`: 510×256 styles, the
    row for the call's token count; grouped by accent and gender, af_bella
    the default); the phonemes go through Kokoro's own symbol table
    (`VOCAB`, `tokenize`), ≤ 510 tokens a call (`chunk`, cut at a space),
    Kokoro's padding trimmed and a 180 ms pause added (`trim_and_pad`).
    Test mode: silence once installed; a stub build outside test mode has
    none (`in_build`, `NOT_IN_BUILD`) and Windows voices speak. `#[ignore]`d
    `checks::kokoro_on_this_pc` (engines build; `YAP_TTS_MODELS`,
    `YAP_TTS_SAMPLES`): speed at 1/2/4 threads, a cancel, and WAVs of the
    research's answer and a names text with both G2P ways, raw and tidied
    (measured 2026-10-18: §"Current status").
  - **`tts/g2p.rs`** — text → Kokoro's phonemes, **dictionary first**: the
    misaki lexicons Kokoro was trained on (US/GB, gold then silver), looked
    up with misaki's `en.Lexicon` rules ported (no spaCy tagger: capitals,
    `'s`/`-s`/`-ed`/`-ing` from the stem, short unknown all-caps words
    spelled out, "the"/"to"/"a"/"an"/"in"/"am" by the next sound, hyphened
    parts, leftover digits as words, US flaps as "T"); only words neither
    lexicon has (names: "Priya", "Siobhan") go to **eSpeak NG as a separate
    program** (`Espeak::ipa`: `espeak-ng.exe -q --ipa --tie=^ -b 1`, one line
    in → one line out, `ESPEAK_DATA_PATH`, hidden window, 5 s limit; never
    linked: GPL-3.0) in one call per sentence, its IPA mapped to misaki's
    symbols (`espeak_english`, misaki's E2M, its dead GB "e^ə" rule fixed);
    no eSpeak → spelled out. Kokoro's es/fr/hi/it/pt-BR voices read their
    text all through eSpeak (`Phonemizer::Espeak`, misaki's `EspeakG2P`
    map; punctuation kept by phonemizing between it); ja/zh voices read
    English. `EspeakEnglish` (all of it through eSpeak, Voice Mirror's way)
    only if a lexicon won't read, and for the comparison. Why dictionary
    first: it's what Kokoro was trained on, and eSpeak runs only for the
    odd name (both scored alike on Parakeet).
  - **`tts/registry.rs`** — Kokoro's download into `<data>/models/tts/`
    (debug: `YAP_TTS_MODELS`): `kokoro-v1.0/` (model.onnx 325.5 MB + 54
    voices from **Hugging Face onnx-community/Kokoro-82M-v1.0-ONNX @
    1939ad2a**, the misaki lexicons + LICENSE from **hexgrad/misaki @
    fba12365**, NOTICE.txt) and `espeak-ng-1.52.0/` (eSpeak NG's own 1.52.0
    **MSI**, unpacked with `msiexec /a` — an administrative install, files
    only, nothing registered, no admin rights; only the exe, the DLL and the
    data of the languages Kokoro reads kept, 2.2 MB — + `COPYING` (GPL-3.0)
    from its repo at the tag + NOTICE.txt with the source link): ~379 MB
    down, ~352 MB on disk, every file pinned by size + SHA-256 (the
    diarization registry's `download`/`verify`, resumable), staged in
    `<folder>.installing/` then renamed; a file already installed at its
    hash is kept (hard link); a failure leaves no half folder. Test runs
    only fetch from 127.0.0.1 (`YAP_TTS_SOURCES` stand-ins: `{package:
    {dest: {url, sha256, size}}}`; a debug build takes a zip for the MSI).
    `#[ignore]`d `real_download_from_the_pinned_sources`
    (`YAP_TTS_DOWNLOAD_DIR`).
- **`assistant.rs`** — the **Voice Assistant** (2026-10-18; Settings →
  Voice Assistant): ask Yap's Chat out loud from any app with its own
  shortcut (`assistant_hotkey`, unbound by default) and hear the answer;
  it never takes the focus or types. Not **Commands** (the agent's name in a
  dictation, the rewrite shortcut: the AI types for you — Wispr's Command
  Mode and old "Hey Flow"; Wispr has no spoken-answer assistant). **Off
  during meetings** (Nathan: Bella talking over a meeting "would be too
  much"): while a meeting records or a call is on (`tts::meeting_quiet` =
  `meeting::is_recording` || `meeting_detect::call_live`), a press doesn't
  listen — `off_for_meeting` shows "The Voice Assistant is off during
  meetings" (alert `assistantMeetingOff`, over the meeting, every press) —
  and nothing in Yap talks: `tts::say` refuses, Chat's auto-speak stays
  quiet. The question is transcribed with the correction dictionary (exact
  + near-miss, as a dictation), never the AI cleanup pass (the Chat model
  reads it as asked; cleanup would add delay), snippets or the voice
  command. Phases
  `Idle → Listening → Thinking → Speaking` in `yap-assistant {phase,
  conversationId, messageId}` (the bar). `listening` (from
  `capture_target`; bumps a generation, drops the last answer's card),
  `heard(question)` (after the STT + dictionary) → `answer`: the
  conversation chosen first, then "Open Teams" — or "Yes." to Yap's "Did
  you mean …?" — needs no model (`apps::plan_for`, below: opened,
  "Opening Microsoft Teams." said); the turn goes through `chat_turn` with
  scope "assistant" (8 history messages, not 19); otherwise no Chat model
  (`commands::resolve_chat_endpoint` errs) → the `assistantNoModel` alert
  with **Open settings** (`alert::OPEN_AI_SETTINGS` → Language Models →
  Chat & Voice Assistant; a model that fails → `assistantFailed`, the
  reason + "Your question is in Chat."); else **`choose_conversation(viewing,
  open_exists, latest, now)`** (pure): the conversation open in the Chat
  view (`chat_view_state {open, conversationId, newChat}`: ChatView
  reports it), a new one if "New chat" was clicked there, else the last
  updated if under `RECENT_SECS` (30 min) — so pressing again asks a
  follow-up — else a new one; then `commands::chat_turn` (the Chat view's
  own turn: RAG, tools, `voice: true`), `yap-assistant-turn {phase:
  asked|answered|failed|open, conversationId, show}` for the main window
  (`show` = the window was in front: ControlPanel brings Chat up on it).
  **`reply(meeting, interrupted)`** (pure): always **spoken** (whatever
  "Speak replies" / "When to speak" say), except when a meeting or call
  started while the answer was coming, or a dictation did → shown only; a
  newer question or ■ (the generation) → not said at all (it's in Chat).
  While the main window isn't in front (`notice::place` = Bar) a card
  (`style: "answer"`, id `assistant`: the question, the answer's markdown
  ≤ 1,200 chars, a note "Not read aloud during a meeting", Open in Chat;
  ✕ / Esc = `stop`) — **"Show the answer on the Yap bar"**
  (`assistant_answer_card`, on) decides it for a spoken answer; one not
  read aloud always shows (`card_on_bar`, pure), also when the voice
  fails —, shown before the voice starts; it goes when the
  voice ends (`on_tts_state`), or after `read_ms` when shown only. **A web
  answer** (`turn.sources`, `web.rs`) is said short (`spoken_answer`:
  `tts::brief` — its first two sentences, no `[n]` — then "The details and
  sources are in Chat."), its card shows that and "2 sources in Chat"; the
  model is told a spoken question wants the answer first. **Any other
  spoken answer** is asked for short (`SPOKEN_ANSWER_NOTE`, last in Chat's
  system prompt for the Voice Assistant, or a question dictated into Chat
  with spoken replies on: "one or two short sentences (under 40 words) of
  plain speech…" — the model evaluations' egg answer was 55–98 words with
  lists before, 12–17 after), and one longer than `SPOKEN_MAX_WORDS` (60)
  anyway is said short too, "The rest is in Chat." `stop`
  (■ on the capsule = `bar_action "assistant-stop"`, `assistant_stop`).
  The bar's capsule shows while `phase != Idle`, the bar on or off.
  Debug-only `e2e_assistant_ask {text, listenMs}` (`Robot::ask_assistant`:
  the press's own steps — barge-in, the target, "recording" for
  `listenMs` — with `text` as the transcript).
- **`mute.rs`** — **other apps' sound while you dictate** (2026-10-17; it
  replaced the old endpoint mute, which silenced Yap's own start chime and
  any call): **"Silence other apps while you dictate"**
  (`config.quiet_other_apps`, off; old configs' `muteWhileRecording` is its
  serde alias) and **"Pause music and videos while you dictate"**
  (`config.pause_media`, off, as Wispr's; `media_pause.rs`). The pipeline
  calls `begin(plan)` in `start_recording` and `after_start_chime(token)`
  from the start chime's `Played` (no chime: a thread), `end()` at stop /
  cancel and `after_stop_chime(token)` after the stop chime (`Pipeline::
  stop_chime_then_restore`; a cancel: at once). Quieting = per app, never
  the device: every active output's sessions (`IAudioSessionManager2` →
  `IAudioSessionControl2` + `ISimpleAudioVolume`) set to 0, never Yap's
  own pid, system sounds, a call app's exe (`meeting_detect::call_audio`:
  every `APPS` exe/package, call or not), the browser a live meeting tab is
  in, or any browser while a meeting records (`quiet_verdict`, pure).
  Restore = each session by its instance id back to exactly its level, only
  if it's still at Yap's level (`restore_verdict`: one that moved is left
  alone); a dictation that ended before its chime finished quiets nothing,
  one that starts before the last restore keeps everything down and brings
  it all back at its end (a generation token; COM/WinRT work serialized on
  `WORK`, never under the state lock the pipeline takes). Bluetooth: if the
  default output dropped into a call profile (< 44.1 kHz) during the
  dictation, the restore polls it every 50 ms ≤ 5 s, + 500 ms
  (`codec_wait`; no wait when it was already in one at the start — Yap's
  always-open mic on a headset, a call). Crash safety: Windows keeps an
  app's session level across restarts, so what's down is written to
  `<data>/quieted_apps.json` while it is; `recover()` at setup and every
  restore bring back sessions of those apps still at Yap's level (a week).
  `restore_all()` in `shutdown_cleanup`. **Test runs** touch nothing
  (`realtest::other_apps` = Nobody); **robot mode** only the sessions of
  processes the test started (`realtest::audio_permits`: the process or an
  ancestor is in `YAP_REALTEST_PIDS`, or owns only windows titled with the
  nonce) and media whose title has the nonce (`media_permits`), never the
  developer's own music. Debug-only `robot_view` (the robot's
  `realtest_other_apps`).
- **`media_pause.rs`** — "Pause music and videos while you dictate":
  WinRT `GlobalSystemMediaTransportControlsSessionManager` (windows crate
  feature `Media_Control`), sessions **Playing** → explicit `TryPauseAsync`
  (never the play/pause toggle), kept; afterwards explicit `TryPlayAsync`
  only for those still **Paused**. Never a call app's session (its app id
  against the call apps' exes and package names), no browser's while a
  meeting records or is live in one (`pause_verdict`, pure). Debug-only
  `list` for the robot.
- **`portable.rs`** — portable-mode detection (a `portable` marker next to the exe
  redirects data to `<exe>/Data`).
- **`e2e.rs`** — **test mode** for the end-to-end UI suite (debug builds started
  with `YAP_E2E=1`; compiled out of release builds): no global input hook, no
  paste/Enter/Ctrl+C into other apps (nor "Paste last transcript"), no other
  app's sound silenced or paused (`realtest::other_apps`), no reading the
  text around another app's cursor or its window's title (`app_kind`), no `set_focus` (tao's fallback presses Alt
  in the focused app), no Windows notifications, no native context menu on
  screen (`context_menu.rs` records what it would hold; robot mode shows it), no orphan-sidecar sweep, no
  window-state plugin, no call-detection registry reads (the debug-only
  `meeting_detect_simulate` stands in), no installed apps' icons (the
  debug-only `YAP_E2E_APP_ICONS` folder of stand-ins, `app_icons_dir`), no
  audio device for meeting recordings
  (`spawn_meeting_audio`: `you.wav`/`them.wav` from `YAP_E2E_MEETING_AUDIO` at
  `YAP_E2E_MEETING_SPEED`× real time, or silence; a configured mic that isn't
  plugged in still fails, found by listing devices; it pushes through
  `meeting::push_audio` + `keep_in_step` like the real capture and logs "the
  test audio has played to the end"; with `set_live_mic` — the guard rails'
  `e2e_meeting_checkins` turns it on — its "mic" carries on with digital
  silence after the files, as a real one does, so the audio clock the
  quiet-meeting guard counts on runs), the debug-only
  `e2e_meeting_feed { segments }` command (hours of transcript in seconds,
  corrected with the dictionary like real lines), `e2e_meeting_dictation
  { from, to }` (the pipeline's dictation signal over that stretch of the test
  audio — no mic involved), `e2e_meeting_output_change { at? }` (a default-
  output switch: "Them" gets nothing for 1 s, then follows the "new device")
  and `e2e_meeting_quiet { quietSecs?, talkSecs? }` (short quiet-side
  timings), `e2e_main_on_screen` (the main window back from a minimize or
  hide the desktop did behind the suite's back — no focus, no "window shown"
  hooks; the `main` fixture runs it before every test, since call prompts
  only go in-app while the window is on screen), `e2e_dictation_mic { silent }`
  (the dictation mic's real audio zeroed, as a muted mic's — the dead-mic
  watchdog's case, its stream really reopened), `e2e_learn_correction` (an
  edit of a pasted dictation as the correction watch would see it, or the
  field's text read by read for the watch's rules to run over — the watch
  thread itself never runs in a test run), no spoken reply played (`tts.rs`:
  stand-in voices, Kokoro as silence once its stand-in files are installed
  from the suite's local server — `YAP_TTS_SOURCES` —, a simulated clock, no
  audio device; what would have been said in `tts_e2e_spoken`), no on-device
  model downloaded but the suite's small fakes from 127.0.0.1
  (`YAP_LLM_SOURCES`, `e2e/support/local-llm.js`), `e2e_chat_dictation {text}` (a
  question dictated into the Chat view, no mic needed), `e2e_assistant_ask
  {text, listenMs}` (a question to the Voice Assistant, as its shortcut
  asks it, no mic needed), no app ever launched (`apps.rs`: "open Teams"
  is recorded for `e2e_opened_apps`, the index is the stand-in list
  `YAP_E2E_APPS` names, no icons), never the internet for the web
  (`web.rs`: the debug-only `YAP_EXA_URL` / `YAP_TAVILY_URL` /
  `YAP_WEB_STANDIN` point at `e2e/support/fake-web.js`; unset, a search or a
  page read fails; a provider's own search goes to the chat's configured
  endpoint, the fake AI in the suite, which answers Groq's
  `browser_search` and Gemini's `generateContent` shapes), no AI
  provider's model list (`model_catalog.rs`: the debug-only
  `YAP_MODELS_STANDIN` points at `e2e/support/fake-llm.js`; unset, the
  curated lists), `e2e_coaching_reset` (every
  push-to-talk hint due again), `e2e_notice_reset` / `e2e_focus` /
  `e2e_notice` (notice.rs: cooldowns and fatigue forgotten before every
  test, whether the main window counts as focused — by default while it's on
  screen —, a notice staged through the real policy or a pretend
  dictation's `yap-state`), `e2e_paste_elevated { on }` (every paste
  target an app running as administrator), `e2e_mic_privacy { blocked }`
  (Windows' microphone privacy switches, never read in a test run), calendar secrets in
  `<data>/calendar-secrets.e2e.json` instead of
  Credential Manager and links recorded (`calendar_e2e_opened`) instead of
  opened in a browser, and it quits when its stdin closes. See
  [`docs/e2e-tests.md`](./docs/e2e-tests.md).
- **`realtest.rs`** — **robot mode** for the real-app robot tests (`e2e-real/`,
  `npm run test:real`): test mode + `YAP_REALTEST=1`, debug builds only. The paste,
  the submit key, "Paste last transcript", the UI Automation reads (the text
  around the cursor, the correction-learning watch) and `app_kind`'s title read
  are real again (`gate` / `permits` / `recheck_foreground` / `check_after`,
  called from `text_injector`, `selection`, `app_kind`, `learn`; in a normal run
  they're always "go", in plain test mode always "no") — **only into windows the
  test opened**: a process in `YAP_REALTEST_PIDS` or a descendant
  (`procs::lineage`, matched by start time) or a title containing
  `YAP_REALTEST_NONCE` (≥ 12 chars; pure `decide`, unit-tested). A refusal is
  logged, emitted as `yap-error` and kept for `realtest_refusals`; the foreground
  is re-checked right before every `SendInput` (the paste, typing, the submit
  key) and again as it returns + 10 ms later (`check_after`: "the keys may have
  gone to another window (process N)", which the suite fails on). Edit mode's
  Ctrl+C stays off. Debug-only
  commands: `realtest_dictate_text { text, hwnd }` (`pipeline::Robot::dictate_text`:
  `capture_target` → `selection::wait_for_context` → `deliver`, the same code a
  spoken dictation runs after its STT; returns `Delivered { text, raw, kind,
  submit, error, leftOnClipboard }`), `realtest_dictate_wav { path, hwnd,
  cancel? }` (`dictate_audio`: the
  WAV fed through `take_audio` in real time between a real `start_recording` and
  `stop_and_transcribe` — or, with `cancel`, the bar's ✕, kept for Undo —, the
  real mic's audio dropped meanwhile — `TEST_FEED`; a
  recording may start without a mic), `realtest_delivered` (what the latest
  dictation — an Undo's — sent to the paste), `realtest_window`, `realtest_refusals`,
  `realtest_clipboard { save | restore | setText | setImage | check }` (never
  hands the clipboard's contents out), `realtest_other_apps` (the test's own
  audio sessions and media, and what `mute.rs` holds down). **Other apps'
  sound** (`mute.rs`, `media_pause.rs`) has its own guard: `other_apps()` =
  All in a normal run, Nobody in plain test mode (nothing silenced or
  paused), TestsOwn in robot mode — an audio session only if its process or
  an ancestor is in `YAP_REALTEST_PIDS` (by start time) or a process whose
  visible titled windows **all** carry the nonce (`audio_permits`, pure
  `decide_audio`: so a Windows Terminal hosting the developer's own windows
  never counts), media only if its title has the nonce (`media_permits`) —
  so a robot run never lowers or pauses the developer's own music. See
  [`docs/e2e-tests.md`](./docs/e2e-tests.md) "Real-app robot tests".
- **`eval.rs`** (+ `eval/`) — **model evaluations**, `yap.exe eval …` (main.rs;
  debug builds only): real models (Groq/OpenAI/Anthropic/OpenRouter/Gemini/
  custom, or a GGUF on Yap's own llamafile started for the run — the installed
  runtime and model read-only, else downloaded via `local_llm::download_verified`)
  through **`commands::chat_turn`** itself (fast path, tool loop, plain chat +
  RAG) over `evals/scenarios/*.json` (seed notes/folders, stand-in apps from
  `evals/apps/` or this PC's Start menu read-only, multi-turn user messages,
  checks on tool calls + arguments, apps "opened", notes afterwards, answer
  text/length, false claims), scored pass/fail with reasons + soft metrics.
  Side effects stay in the run: its own data dir (`portable::use_data_dir`),
  settings in memory (`eval::config`, read by `config::load`; `save` a no-op —
  keys never reach a file), notes in memory (`notes::eval_reset`), test mode
  for apps (opens recorded), the clipboard tool recorded
  (`eval::intercept_tool`); web scenarios (`needs_web` / `web`) search live
  (`eval::web_live` lets `web::Endpoints::current` use the real services in
  test mode: the run's Exa/Tavily key, else the keyless wikis) or against a
  stand-in server (`eval/standin.rs`, through `YAP_EXA_URL` /
  `YAP_TAVILY_URL` / `YAP_WEB_STANDIN`). Logs per run in `<out>/<stamp>-<label>/`:
  `transcript.jsonl` (every request body incl. tool schemas, every response,
  429s/waits, tool calls + results, keys redacted), `summary.md`/`.json`.
  Paces Groq to its 8k tokens/min (and its rate-limit headers). `--raw` sends
  hand-written requests (the Groq `browser_search` spike). A local model's
  llamafile runs with the sidecar's own `local_llm::server_args` (thinking off,
  caches capped; `--llamafile-arg` after them overrides) and sends its GGUF's
  name as the model id, as the sidecar does; a voice turn is the Voice
  Assistant's (scope "assistant"). **`--cleanup`** (`eval/cleanup.rs`)
  times `llm::cleanup` itself over `evals/cleanup/dictations.json` — back to
  back, after a Chat turn, after a long one — with llama.cpp's prompt tokens
  processed, and keeps every output (`cleanup.md`). The hooks in
  `llm.rs` (`eval::Call`), `tools.rs`, `config.rs`, `notes.rs` are no-ops
  outside a run. See [`docs/model-evals.md`](./docs/model-evals.md).
- **Logging** (`lib.rs init_logging`) — tracing → stdout + a daily-rolling
  `<data>/logs/yap.log.*` file at `info`; panics are hooked into the log.
  **Debug mode** (Settings → Advanced → Debug Logging, OpenWhispr Developer-
  section port): `config.debug_logging` raises `yap_lib` to `debug` live via a
  `reload::Layer` handle (`set_debug_logging`, applied on config save; RUST_LOG
  env always wins). `log_info`/`open_logs_folder` commands back the UI (current
  log file + copy path + open folder). ⚠ transcripts DO appear in logs at
  `info` — the UI warns users to skim before sharing.
- **`commands.rs`** — Tauri commands: recording (`toggle_recording`, `cancel_recording`,
  `dictation_key` — the in-page fallbacks' hotkey down/up),
  config (`get_config`/`save_config`), models (`installed_models`, `download_model`,
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
  live-applied / in-page), `chat_view_state` + `assistant_stop`
  (assistant.rs),
  `meeting_start` (then `calendar::on_meeting_started`)/`meeting_stop`/
  `meeting_state`, `transcribe_file`/`cancel_file_transcription`/
  `audio_file_info`, `log_info`/`open_logs_folder`, `delete_history_entry`.
  (`paste_last_transcript` lives in `text_injector.rs`.)

### Frontend (`src/`)
- **Theme (2026-07-09)** — the main window is **warm-light, Wispr-Flow-inspired**:
  every token lives in `src/app.css` (`--yap-*`: paper surfaces w/ white cards,
  warm ink text, ONE amber accent reserved for keycap chips / active states /
  toggles, **ink** filled buttons via `--yap-ink*`, serif display numerals via
  `--yap-font-display`, light shadow tokens). Typefaces = **the exact pair Wispr Flow
  ships** (identified from its install at
  `%LOCALAPPDATA%\WisprFlow\app-*\resources\assets\fonts`; both OFL/Google
  Fonts, bundled via `@fontsource-variable/*` imports in main.js): **Figtree**
  = UI sans (Segoe UI fallback), **EB Garamond** (+italic) =
  `--yap-font-display` (hero headlines, stat numerals, Settings page titles).
  The meeting notepad's layout tokens, **`--yap-paper-*`** (paper, ink,
  text-2, muted, faint, date, rule, s1, s2, s2-strong, ring, danger, snag,
  band — the connect dialog's sand) + `--yap-live`/`--yap-live-strong`,
  `--yap-ease-spring` and the popover/dialog/sheet shadows: Wispr's measured
  roles (2026-10-05), set to **Yap's own palette** since 2026-10-06 (warm
  paper and ink, an amber focus ring, burnt-orange "live", recording-red
  Stop/Resume); the Meetings view's grey card, Take notes pill and the
  "Connect your calendar" dialog use them too. **Settings attention badge** (`lib/attention.svelte.js`):
  Settings computes real needs-action items (update available / no STT model /
  cleanup on a cloud provider with no key) into a shared runes store; the
  ControlPanel cog + the matching Settings nav rows show a red count chip
  (Wispr's Settings "1" pattern). **Onboarding is warm-light too** (2026-07-09
  redesign: same tokens as the main window, serif display headlines, ink CTA,
  brand icons on the model cards via `providerIcons.js`); the
  `:root[data-yap-theme='dark']` token block in app.css is currently unset
  everywhere — kept only as the seed of a future dark mode. App.svelte sets
  per-window `color-scheme`. **Brand mark** (2026-07-09):
  `src/assets/yap-logo.svg` — warm-ink rounded badge, amber yapping mouth,
  cream soundwaves (replaced the old blue `yap-icon.png` in the title bar +
  onboarding; `src-tauri/icons/*` regenerated from it via `npx tauri icon`).
- **Custom window chrome (2026-07-09)** — the settings window is **undecorated**
  (`decorations: false` in tauri.conf.json): ControlPanel draws a 40px
  `data-tauri-drag-region` title bar (brand left; min / max-restore / close
  right, Win11-style hover incl. red close) over the warm frame, Wispr-style.
  Buttons use `getCurrentWindow()` (capability grants: minimize,
  toggle-maximize, is-maximized, close in `capabilities/default.json`);
  close still routes through hide-on-close; double-click on the drag region
  toggles maximize (Tauri built-in). Trade-off: the native snap-layout flyout
  on hover-over-maximize is gone (Win+arrows still snap).
- **`lib/ControlPanel.svelte`** — the **main window** (window label is still
  `settings`, historic): an OpenWhispr-style control panel — slim sidebar
  (**Home / Insights / Chat / Notes / Meetings / Upload / Dictionary / Integrations**) + **Settings as a modal
  overlay** (cogwheel). `Settings.svelte` renders `embedded` inside the modal
  and stays **always mounted** so its in-window hotkey fallback + auto-save run
  for the window's lifetime. App-wide **toast notification system**
  (`ui/toast.svelte.js` + `ui/ToastHost.svelte`, OpenWhispr timer logic in
  **Wispr-Flow card styling** since 2026-07-09): dark rounded card with a
  per-variant category chip (Tip/Done/Error, override via `chip`; `icon:
  'update'`), always-visible circular ✕, optional light **action button**
  bottom-right (`action: { label, onClick, keepOpen? }` — Wispr's "Open
  Settings") + a quiet `secondary` ("Later") + a small `tertiary` text link on
  its own line under them ("Don't ask for Teams"), hover-pause, `onClose` (✕) /
  `onExpire` (timer ran out, or waited too long) callbacks, copyable mono
  error boxes, progress hairlines (`duration <= 0` sticky), plus a
  determinate `progress` bar, a `busy` chip spinner and an `expand` "What's
  new" toggle that unfolds markdown in the card; `updateToast(id, patch)`
  changes a toast in place (on screen or waiting); `isToastLive` = not
  closed. It runs **Yap's notice policy** (`ui/noticePolicy.js`, the page's
  half of `notice.rs`): every toast has a `kind` (prompt | offer | alert |
  update | info | warning | tip — what Yap says by itself — or error | done —
  the answer to a click; by default from the variant) and two lanes, **one
  notice + one answer on screen**, the rest queued (higher priority first,
  then oldest; a question pushes a tip back unless hovered; an `offer` —
  "Started by mistake?" — or the update gives way to its equals and comes
  back); tips/notices/updates held while a dictation records (`yap-state`,
  + 1.5 s), the update also while a meeting records; a tip waits ≤ 30 s and
  a notice ≤ 60 s unseen, then is dropped (`onExpire`); the same toast again
  (kind + text, or `key`) refreshes the one up instead of stacking; a
  `warning` doesn't repeat within 10 min. Default times: done 3 s, info 6 s,
  error 6 s, alert / tip 10 s, warning 12 s, prompts until answered. Success
  toasts only where nothing else shows it (copy from a menu, a file saved,
  a note moved, "Added to Claude" with its restart step, "Won't ask about
  Teams", "Updated to X", sign-in); a state the screen already shows gets
  none (recording / paused / ended, an action run, a switch flipped, a row
  gone, the transcript's copy ✓). Mounted in ControlPanel (and the notepad)
  and wired to action runs, uploads, clipboard copies, backend `yap-error`
  events (kind `alert`; a dictation's own errors are the overlay's only,
  `yap-dictation-error`), push-to-talk coaching (`yap-hint` → a 10 s Tip
  toast, kind `tip`, `{key}` named by `fillHotkey`), **word suggestions**
  (`wordSuggestions.js` — learn.rs's `yap-word-suggestion`: "Add
  “Kubernetes” to your dictionary? Yap heard “cube and eighties”." with Add /
  Not now / a "Don't suggest this" link, chip Dictionary, `icon:
  'dictionary'`, 30 s; ✕ = Not now; after Add "“Kubernetes” is in your
  dictionary" + Open Dictionary), **alerts** (`alerts.js` — alert.rs's
  `yap-alert`, kind `alert` (a test's `level` may name another): "Your text
  is copied: press Ctrl+V" with chip Paste, `icon: 'clipboard'`; "Windows is
  blocking microphone access for desktop apps" + Open settings, chip
  Microphone, `icon: 'mic'`; "Stop asking about calls?" + Stop asking, chip
  Calls), and the **update toast**
  (`updates.svelte.js` — the shared update store started by ControlPanel:
  sticky "Yap X is ready" + Restart to update / Later / What's new, then
  Downloading… / Restarting Yap… in place; "Updated to X" after the
  restart), and the **call prompts** (`meetingDetect.svelte.js` — mirrors
  meeting_detect.rs's snapshot: "Teams call detected — Record notes / Not now"
  with a small **"Don't ask for Teams"** link on its own line under the
  buttons (the toast's `tertiary`), fading after the prompt's `fadeMs` (30 s)
  as "Not now" (the toast's `onExpire`), and a sticky "…call ended — Stop and
  summarise / Keep recording", both with `icon: 'call'` and withdrawn when
  Rust withdraws the prompt; the card's ✕ runs its `onClose` = the quiet
  answer; `yap-meeting-detect-choice` with `confirm` shows "Won't ask about
  Teams calls" + Open Settings (→ `general#meetings`);
  `yap-meeting-open-note` switches to Notes and hands NotesView a
  `noteRequest` to open, and stop if asked), the **meeting guard rails**
  (`meetingGuard.js` — meeting_guard.rs / capture.rs: the sticky "Notes stop
  in 5 minutes" + **Keep going** toast (`icon: 'timer'`, counting down,
  only when `inApp`, withdrawn when Rust sends `{warning: null}`), and one-off
  `yap-meeting-notice` toasts: the screen-share tip (`icon: 'screen'`,
  **Update settings** → `general#screen-sharing`), "Stopped at 2 hours", "Teams
  call ended · Yap stopped recording…", "Taking notes" (a notice addressed
  to the notepad, `surface`, isn't shown here), and meeting.rs's warnings
  (kind `warning`, 12 s: "Yap can't hear the call", "Transcription is
  falling behind") in whichever view is open; and, through `meetingCheckIn.js`
  (shared with the notepad, `initMeetingCheckIns(label)`), the quiet
  countdown "Ending notes in 28 s…" — Keep going / Stop now, a draining
  bar, its ✕ only hiding it — and "Still in your meeting?" — Stop and
  summarise / Keep recording, ✕ = keep — when `yap-meeting-checkin`'s
  `surface` names this window), and **"Started by
  mistake?"** (`meetingSummary.svelte.js` — the shared store of the Rust
  action-plan jobs, `summaries.byNote` / `.progress`, also used by NotesView
  and the notepad; `askStartedByMistake` = a sticky "Only a few words were
  captured. Keep this meeting or discard it." with Discard / Keep, `icon:
  'alert'`, kind `offer`, shown by the window `yap-meeting-ended`'s `surface`
  names), and the
  **calendar** (`calendar.svelte.js` — the shared store of calendar.rs's
  snapshot, started by ControlPanel: the reminder card as a sticky toast
  while Rust says `inApp` (else it's on the bar),
  chip Meeting / Next meeting, `icon: 'calendar'`, "In 1 min" counting,
  Join & take notes / Start notes / Snooze 2 min, ✕ or Esc = dismiss,
  withdrawn when Rust withdraws it; "Calendar connected", sign-in errors
  (in the connect dialog instead while it's open); the one-time "Connect
  your calendar" nudge after a real meeting ends (`yap-meeting-ended`, not a
  mistake) with Connect calendar (the dialog, over the Meetings view) / Not
  now; `connectDialog` + `openConnectDialog`/`closeConnectDialog`).
  **`ConnectCalendarDialog.svelte`** = Wispr Flow's "Connect your calendar"
  modal (its screenshot 27) in Yap's flows, mounted once by ControlPanel and
  opened by every Connect calendar outside Settings (the Meetings view's
  card, the nudge, Integrations' Calendar card): a warm beige band
  (`--yap-paper-band`) showing the bar's real pre-meeting card
  (`bar/CallCard.svelte` with `preview`, sample data "Design review · ● In 2
  min · with Sam", Join / & take notes ^), then "Connect your calendar", one
  line, and three full-width outlined buttons with `CalendarMark.svelte`'s
  marks (shared with Connectors): **Continue with Google** (the browser
  flow, waiting in place with Cancel; closing doesn't cancel it; off in a
  build without the client, "Google sign-in is in the installed Yap. In this
  build, use a private iCal link below."), **Continue with Outlook** (the
  publish steps + the ICS field) and **iCloud or another calendar** (where
  to find the link + the field; Yap's checks say why a link fails), then
  "Calendar connected · 6 meetings in the next 7 days" for 2.4 s and it
  closes. Wispr's dialog tokens (scrim `rgba(26,26,26,.3)`, paper card,
  radius 20, the dialog shadow, scale-in); Esc / round ✕ / scrim close it;
  labelled by its title, focus trapped (Tab wraps) and returned (to the
  opener, or the Meetings title). **`MeetingsView.svelte`** = the Meetings
  view (Wispr's Notetaker page): serif title, Sync calendar (an icon) /
  settings gear / **◉ Take notes** (beige, Wispr's "Start Notetaker": the
  meeting shortcut as a button via `meeting_shortcut` — notes on the live
  call, else a new meeting note; **■ Stop and summarise** while one
  records); **Today** and **Upcoming · Next 7 days** (by day, 3 at a time +
  Show more; time, name, Conflict / Maybe tags, service, attendees, "In 1
  min", Note ready; from 10 min before: Join meeting / Start / Join +
  Start, or Switch notes while another records, or ● Recording · Open note;
  opening a meeting makes or opens its note via `calendar_event`); with no
  calendar, one grey card (calendar icon, "No meetings found", ink Connect
  calendar → the dialog); **Past meeting notes** by day ("TODAY, 6 OCT",
  time · attendees · "Generate summary" on a meeting with talk and no plan
  → `meeting_summarise` + opens the note) with search
  (`calendar_meeting_notes`); a bottom **Ask bar** cycling example
  questions ("What questions were left unanswered in my last meeting?")
  that opens Chat with the question (scope `meetings`, through
  `chatRequest.svelte.js`), plus a **Past chats ↗** chip (opens Chat). **`HomeView.svelte`** = the Wispr-style
  Home: time-of-day greeting with the hotkey as **amber keycaps**, a dark
  **rotating hero card** (5 tips — Commands (voice edit) / AI cleanup / meeting notes /
  per-app profiles / the Voice Assistant — picked by day, dot nav, CTAs open the right Settings
  section or view via `onnavigate`), the dictation feed (day-grouped flat rows
  w/ hover-revealed meta + copy/delete via `delete_history_entry`, live refresh
  on `yap-history-changed`, icon-expand search on Ctrl+K), and a right-rail stats
  card with serif display numerals. **`InsightsView.svelte`** = the stats
  dashboard promoted out of Settings→History (serif hero number, stat grid,
  30-day amber heatmap, top-apps-by-words bars from `get_history`); Settings →
  History keeps only the enable toggle + recent list + clear. **`DictionaryView.svelte`** =
  the dictionary (promoted out of Settings → Advanced; syncs with
  Settings' cfg copy via `yap-dictionary-changed`/`-external` events) in two
  sections over the one list: **Corrections** (heard → replace with, the ≈
  near-miss button, "Catch near-misses", and "Suggest words from your
  corrections" — `learnWords`, `learn.rs`; it rides along in the
  `yap-dictionary-changed` detail with `entries` and `fuzzy` so Settings'
  copy keeps it) and **Snippets** ("You say" pill →
  "Yap types" growing text box, line breaks kept; nothing seeded). Settings'
  `buildClean` keeps each entry's `snippet` flag — drop it and an auto-save
  turns snippets into corrections. Settings → Advanced → Output has "Say
  “press enter” to send" (`voiceEnter`) under Auto-submit; the key picker
  shows when either is on.
  **`UploadView.svelte`** = local audio-**file** transcription (drop/browse →
  Symphonia decode → chunked transcription on the warm engine with progress +
  cancel — see `media.rs`); **`NotesView.svelte`** = the AI Notepad (OpenWhispr
  sidebar port: **New note / Search notes / Actions** rows; **FOLDERS with
  counts + NOTES list**; **right-click menus** (OpenWhispr's folder and note
  dropdowns, as context menus too: `ui/ContextMenu.svelte`, also from a row's
  hover **⋯** and the keyboard — the ContextMenu key, Shift+F10): a folder's
  **Rename** (in place: Enter or leaving it saves, Esc cancels) and
  **Delete…** (`ui/ConfirmDialog.svelte`: "Delete “Camping”?" · "Its 3 notes
  will move to Personal."; the view goes to Personal if it was on it),
  Personal and Meetings showing both off with the reason; a note's
  **Rename** (its title, focused), **Move to ▸** (the folders, this one
  ticked), **Copy as Markdown** (`noteMarkdown`), **Export…**, **Delete…**
  (asked first; a meeting note goes through `meeting_delete`, which stops its
  recording first and makes the notepad let go, any other through
  `note_delete`); meta chip row shows date + attendees popover
  → add/remove participants, folder-move menu w/ New folder option, meeting
  **Record / ● 12:34 · Pause / Resume** chip (Pause = `meeting_pause`) and,
  on meeting notes, a **Notepad** chip (`notepad_open`), export-to-markdown;
  ActionPicker split button + ActionManager dialog for custom actions +
  protected built-ins — while recording it becomes **End meeting &
  summarise** (`meeting_end` → Rust writes the Action Plan, shown here as it
  goes: the runline, the AI-setup card, the error; the Action Plan action and
  the **Action plan** button run the same Rust job via `meeting_summarise`);
  title + content saves send only the edited field with `origin: 'settings'`
  and follow `yap-note-changed` (the notepad's edits, the AI title; a made-up
  title shows muted); **Raw ↔ Enhanced dual-view + staleness dot** (safe renderer
  in `lib/markdown.js`, shared with chat answers and the notepad: everything
  HTML-escaped first; headings, lists, tasks, bold/italic, inline code, fenced
  code blocks `pre.md-code`, `[text](url)` and bare links `a.md-link` — web and
  mail only, styles in app.css) with **Copy markdown / Copy text** (`markdownToText`:
  ☐ tasks, plain headings); embedded per-note **"Ask anything…" bar** = Chat
  scope grounded in the note, with mic button for in-box dictation, and on a
  meeting note the **one-tap helper chips** over it (`meetingHelpers` from
  `notepadText.js`: Questions to ask while it records, Follow-up email and
  Gaps and risks after → `meeting_assist`, answered in the thread with a
  Copy button; no AI → a toast with Open Language Models); live
  **meeting transcript bubbles** under their speaker's name, once per run of
  one voice (`transcriptItems`; the name opens the speaker menu, below; follow
  the newest line; echo lines
  hidden behind a "Show N lines…" toggle; a `dictated` marker is a quiet dashed
  "You dictated here · left out of the notes" divider with a mic glyph), an
  **AI notes so far** view of the
  rolling digests ("AI notes up to 40:12"), an **Action plan** button on
  finished meetings, a "Writing your action plan… / Catching up on the meeting
  — part 3 of 12" line, and, without an AI model, a "Your meeting is saved…"
  card linking to Settings → Language Models); the Home feed has **Ctrl+K
  search**. **`ChatView.svelte`** = the
  AI Chat surface (OpenWhispr `chat/ChatView.tsx` port): conversation sidebar
  (Today/Yesterday/Previous 7 Days/Older grouping, Ctrl+N, hover-delete;
  **several at once**: Shift+click a range, Ctrl+click one, Ctrl+A in the
  list → a "3 selected · Delete · Cancel" bar, Delete key / Esc; the list's
  ⋯ menu has Select all and "Delete all chats…", every delete asked first;
  `chat_delete_many` = one write, `chats::delete_many`) +
  thread; `chat_send` answers via the Chat scope with **eager keyword-RAG**
  over the notes library (top-5 `<note>` snippets; `chats.rs` persists
  conversations to `chats.json`) **plus the tool-calling agent loop**
  (`tools.rs`: search_notes/get_note/create_note/update_note/list_folders/
  copy_to_clipboard, plus search_in_note/read_note/edit_note for changing
  part of a note, executed locally, ≤20-step loop over the OpenAI tool
  protocol, gated to cloud or ≥4B local models — smaller models fall back to
  plain RAG chat; tool-activity chips render in the thread above the answer
  and are kept with it in chats.json; a question from
  the Meetings view's Ask bar arrives through `chatRequest` and is sent with
  `scope: 'meetings'`). **A web answer** (`web.rs`): its `[1]` citations are
  small pills that open their source (`renderMarkdown(md, {sources})` in
  `markdown.js`, where only the sources' links stay links — any other
  address the model wrote is plain text), a **Sources** row under it
  (number, title, site; `openExternalLink`), kept in chats.json; web chips
  carry a globe, a refused action (the safety rule) a shield. The composer (2026-10-20, after Claude
  Code's: one row — **+** (a "Chat options" popover: the Voice replies switch,
  the voice volume slider, Voice settings…), the message box (aria-label
  "Message"), the mic, and Send only once there's text; an amber dot on the +
  while voice replies are on; under the box on the left, quiet, the **model
  chip** (its `quiet` look; never inside +: always in view) — `ModelChip.svelte`, names in `aiModel.js` from
  `ppModels.js` + `providerIcons.js`) shows the model the next message goes
  to — the provider's mark (`ProviderMark.svelte`), "GPT-OSS 20B · Groq" /
  "Qwen3.5 4B · On this PC", "· no note tools" for a local model under 4B,
  where it runs in its tooltip (and, on the on-device model, "Context: 8K
  tokens": `ai_model_status.context`), "No AI model" when nothing is set up. The
  chip opens **the Chat model picker** (`ChatModelPicker.svelte`, data in
  `chatModels.svelte.js`; Nathan's "Concept A, provider rail",
  2026-10-19): a popover above the composer (`data-model-picker-above`;
  560 px, its rail a strip across the top below 520) — **search** across
  the top ("Search models and providers", Ctrl+K, typing anywhere searches,
  Enter picks the first result) over every provider's models, connected or
  not, and this PC's; a **rail** (`tablist`): Recent (the last 3 Chat models
  used, this webview's localStorage `yapChatRecentModels`), Groq / Gemini /
  Claude (Anthropic) / OpenAI / OpenRouter with their marks and a dot
  (green = a key, hollow = not set up), Custom when a server elsewhere is
  set up, and **On this PC**; it opens on the current model's provider (or
  the one the scope is set to). The **pane**: the provider's mark on a
  tile, its name, a status line from the live lists ("Connected · checked
  2 h ago", "Couldn't check: …", "Not set up"), the key masked + Change
  key; the rows are exactly Settings' list (`modelCatalog.js`'s merge:
  curated, then "New", the rest behind "Show N more"), each a button
  (`aria-pressed`) with a ✓ for the current one, the model family's mark
  (GPT-OSS → openai-oss.svg, Qwen, Gemma… `familyIcon`), a one-line
  description and ≤ 2 tags (New, Recommended for this PC, In use, no note
  tools, No longer offered — with "Groq no longer offers GPT-OSS 120B. Pick
  another model." above). A provider **without a key** shows "Connect
  Claude" in place (paste a key · Save key · Get a key ↗ via
  `openExternalLink`; `chat_provider_key`), its curated models greyed with
  "Add key" until then (a click on one goes to the key field); once saved
  its live list is fetched at once and its models are pickable. **On this
  PC** ("Private: nothing leaves this PC" + where it runs: "Runs on your
  graphics card (NVIDIA …)" from `gpu.rs` / the sidecar's placement, or
  the processor), then the on-device model's **context**
  (`LocalContext.svelte` `compact`, shared with Settings; words in
  `localContext.js`): "Context [8K tokens · ≈ 3.7 GB ▾]" — every choice up
  to the model's maximum with what it takes (`local_llm_status().context`,
  `gguf::estimate`), "· may not fit" and a warning line ("More than fits on
  your graphics card (about 13.6 GB to spare): part of it would run on the
  processor, slowly.") past the card's room, "Uses 3.7 GB on the graphics
  card" once the running model's log measured it, the one-line note
  ("Longer context: longer chats and meeting notes fit, more graphics
  memory, a little slower. This model goes up to 256K."), and "Restarting
  the model with the new context…" while `local_llm_set_context`'s restart
  waits for a dictation (`yap-local-llm-changed` re-reads,
  `followLocalChanges`); Settings' processor warning with "Use Qwen2.5 1.5B
  Instruct"; a note when cleanup uses the on-device model too
  (`local_llm_status.sharedOnDevice`: picking another switches it there as
  well); **Installed** (Yap's curated GGUFs on disk and the person's own,
  `local_llm_status.own`; In use / pick to use; each row starts with its
  maximum, "Up to 256K · …", `maxContext`), self-hosted servers on this
  PC (`llm_models`: their `/models`, "3 models on this server", and the
  context Ollama / LM Studio say: "8K context, set in Ollama · …"), and
  **Download** (the rest, "Recommended for this PC" first): a Download
  button with the size runs `local_llm_install` right in the row with a
  progress bar from `local-llm-download-progress` (which now names the
  curated `model`; one at a time, it carries on when the picker closes, no
  cancel — the install path has none), then the row is installed and
  pickable; "Open models folder". The footer: "Changes the model for Chat
  only. Dictation cleanup stays as it is." (or that Chat uses cleanup's
  model until one is picked) + **Language model settings…**
  (`cleanup#chat`). Keyboard: ↑/↓ through the rail (switching as it goes)
  or the rows (roving tab stops), ←/→ or Tab between them, Enter picks, Esc
  closes and gives the focus back to the chip; amber focus rings; the
  window's in-page hotkey fallbacks run first (capture phase). Monochrome
  marks (OpenAI, Anthropic, xAI, OpenRouter — Simple Icons' CC0 glyph) are
  CSS masks in `currentColor`, Claude's in its brand colour. Settings →
  Language Models keeps its own pickers. Each answer
  shows its model quietly under it, as do the Notes view's Ask bar and the
  notepad's Ask sheet (`nextModel(scope)` fetched beside the call; none for
  Yap's own "Nothing new" / "too little"). **Spoken replies** (`tts.rs`):
  **Voice replies** (🔊 / 🔇 pill beside the model chip, `aria-pressed`) is
  Settings' "Speak replies" (`voice_replies_set`; Settings adopts
  `yap-voice-replies`, the toggle re-reads on `yap-config-saved`); every
  answer has **▶ Read aloud** under it (■ Stop reading + "Speaking…" with
  three bars while it's read; also on the Notes view's Ask bar answers and
  the notepad's Ask sheet, not while that meeting records), via the shared
  `speech.svelte.js` store (`speakingId` from `yap-tts-state`; each window
  toasts failures for its own ids: `chat:` / `notes:` / `preview` /
  `notepad:`). With replies on, chat_send's `speak` reads an answer out by
  itself. The **mic** is `chat_dictate`, and the box reports its focus
  (`chat_input_focus`), so a question dictated into it arrives as
  `yap-chat-dictation` (inserted at the caret, never pasted) and is sent at
  once when `send` says so; such questions are sent with `voice: true` (a
  small mic on the bubble). Esc, leaving Chat, another conversation, the
  next question and any dictation starting stop the voice. **The Voice
  Assistant** (`assistant.rs`): the view reports what it shows
  (`chat_view_state`: the conversation, or "New chat" clicked) and follows
  `yap-assistant-turn` live — it switches to the question's conversation,
  the question with its mic, "Thinking…" (`assistantPending`), the answer
  (Rust reads it aloud); ControlPanel brings Chat up for one asked while the
  window is in front (`show`) or "Open in Chat" (`chatRequest {assistant}`).
  An empty chat says "Or press F8 in any app to ask out loud…", or offers
  **Set an assistant shortcut** (→ Settings `voice#assistant-shortcut`).
  No streaming or
  semantic vectors yet (ROADMAP step 3). **`IntegrationsView.svelte`** = the
  Integrations surface (OpenWhispr `IntegrationsView.tsx`, local-first cut):
  a **Calendar** card (`CalendarLinkCard.svelte`: Connect → the "Connect
  your calendar" dialog, Manage → Settings → Connectors),
  an **AI apps (MCP)** card (`McpLinkCard.svelte`, Wispr's "Go to MCP" row:
  opens Settings → MCP via ControlPanel's `openSettings`), the Local API card
  (enable toggle + live status/port via `bridge_status`, discovery-file path +
  curl example with copy; the toggle tells Settings' copy via
  `yap-config-patched` so its auto-save can't switch it back), a
  Coding-agents card whose "Copy API guide" button copies a
  paste-into-your-agent endpoint cheat-sheet, and an endpoint reference
  table. (OpenWhispr's Google-Calendar OAuth / cloud API-keys / hosted-MCP
  cards need their paid cloud and are not ported; Yap's MCP is local.)
- **`lib/Overlay.svelte`** — the **Yap bar** page (window `overlay`, `bar.rs`),
  laid out to Wispr's measured Flow Bar (`flowbar-spec.md`, checked by
  `bar.spec.js`) but in **Yap's own colours** (2026-10-06, Nathan): every
  colour is a `--bar-*` token in app.css, in two schemes picked in Settings →
  General → Yap bar → **Colours** (`bar_theme`, carried to the page in
  `bar::Status.theme`, which sets `data-bar-theme`; the main window follows it
  too, for the Connect calendar dialog's card picture). **Light** (default):
  Yap's window — white cards with the warm `#dcd7cb` edge, ink text, ink
  primary buttons, amber chips, burnt-orange waveforms. **Dark**: the in-app
  toasts' palette — warm near-black `#1c1a16`, sand chips, cream buttons, Yap
  amber. Both keep Yap's recording red:
  one stage anchored to the bottom (or top) edge — the pill's place below,
  states sharing one grid cell so they cross-fade, each in a hit wrapper
  (6 × 10 px of nearly invisible padding, reported as region "pill") whose
  bottom is 8 px up, so every pill sits **14 px above the work area**. The
  **idle pill** is 40×8 (light: white at 85 % with a 1 px ink border at
  30 %; dark: warm ink at 55 %, cream border), radius 6, no shadow, in a
  60×20 wrapper; it morphs into the open
  pill (not measured yet: Yap's own 128×38); tooltips (ink in both schemes,
  radius 8, 12 / 600: "Dictate F9", "New note Win + Alt + M") and the ^ menu
  sit above it. The **meeting pill**: 69×30, the bar surface, a 2 px ring in Yap's
  recording red (`#e5645e`; Wispr's is emerald), radius 22.5, half opacity
  until hovered; 5 2×18 bars in the scheme's amber (Wispr's staggered timing), then the 19 px
  stop circle with its 8×8 square; hovered, it widens to show the timer.
  `bar/DictationCapsule.svelte` (the dictation overlay — waveform in Yap
  amber, live partials word-paced by `bar/dictation.svelte.js`, which listens
  for the page's whole life; a question to the **Voice Assistant**
  (`dictation.assistant`, `data-ask`) is its own variant: an amber edge and
  a ✦ for the dot, "Ask Yap…" + waveform (or the words) while it listens,
  "Thinking…", then three bars + "Speaking…", with a ■ (`assistant-stop`,
  region "assistant") while it thinks or speaks; a small amber lock after the dot while push-to-
  talk's double-tap holds it hands-free; the dead-mic watchdog's "No sound
  from <device> — is it muted?" — or "Windows is blocking the microphone" — in
  the waveform's place, with a red edge; while it records a quiet ✕ on the
  left and the primary ✓ on the right, 26 px circles — `buttons` "off" (the
  default) never, "hover" only while `hovered` (Overlay's `pointer ===
  'dictation'`), "always" always, and none while `dictation.held` (the
  push-to-talk key down, not locked); where they may show, their places in
  the capsule's ends are kept (39 px padding) and they only fade (and
  settle from 0.82×) in and out there, `visibility: hidden` at rest, so
  pointing never changes the width or moves the words; cancelled,
  "Dictation cancelled" + Undo with a hairline running down the kept time.
  It's a `data-region` ("dictation") whenever it has a button to offer, so
  the click-through bar takes clicks on it, never focused). **Cards** sit 26 px above the pill whatever it
  shows (positioned from the dock's height, gliding 0.16 s when it grows),
  one at a time (`notice.rs` picks it; the rest wait in `bar.waiting`); it
  fades in and out in 0.12 s and has Wispr's 4 px hit strip on the pill's
  side.
  `bar/BarCard.svelte` (notices: update, guard rails, "Taking notes", the
  call ending, errors) is Wispr's card in Yap's colours: 400 wide,
  the bar surface with a 1 px edge, radius 16, padding 20; a chip naming its
  kind (Update, Meeting, Call, Tip, Error; icon `tip` = a bulb, the coaching
  hints, whose `{key}` the card names from its `hotkey`), the title 15 / 600, the body 15
  muted, a ghost button and a primary one (ink on light, cream on dark) at
  radius 8, the ✕ 24×24 at 30 % top-right; a `countdown` line with a ring
  ("Esc to cancel" only when the card has an `escapeAction`, its `done`
  text at zero), or as the card's **headline** (`Countdown.headline`: the
  quiet-meeting countdown, "Ending notes in 28 s…" at 20 / 650 beside the
  seconds in a 40 px ring, above the title — readable at a glance).
  `bar/CallCard.svelte` (the call
  prompt as Wispr's "Meeting detected" card, on the same tokens but one row
  with slimmer padding: the app's icon on a light tile, drawn by the shared
  `lib/CallAppIcon.svelte` — its own as installed here (`app_icons.rs`,
  loaded per window by `callAppIcons.svelte.js` when first drawn, again after
  10 min), else its mark from `bar/callApps.js` (Simple Icons CC0 glyphs),
  else a letter badge in its colour (Teams/Slack/Webex/Whereby), else a
  phone; `data-icon` names which —
  "Teams call" over "● Now", a cream split button [Yap] Record notes with the
  other answers in its ^ menu, a corner ✕, a 30 s fade hairline, the
  countdown ring; also the calendar's reminder, "Design review" over "● In
  1 min · with Tanay +1": a calendar glyph when there's no call app, an amber
  "soon" dot, a long line ending in "…", and a two-line answer "Join / &
  take notes", as on Wispr's pre-meeting card; `preview` draws it as a
  static, inert, aria-hidden picture with no regions, for the connect
  dialog's band); shared fade and countdown timers in
  `bar/cardTimers.svelte.js`. Every `data-region` element is
  reported to Rust on layout changes. ⚠ The window is transparent: **no
  `backdrop-filter`**, no shadow on the pills and only a 1–2 px one on cards
  (one reaching a transparent WebView2 window's edge draws a grey box).
- **`lib/Notepad.svelte`** — the **meeting notepad** (window `notepad`,
  `notepad.rs`), **Wispr Flow's notepad, measured** (2026-10-05, from its DOM
  over CDP: `E:\Projects\references\wispr-flow\notepad-spec.md`; sizes, type,
  motion matched in Yap's own code, wording and icons; the colours are Yap's
  own since 2026-10-06). Palette = the `--yap-paper-*` tokens in app.css
  (paper `#faf9f6`, ink `#26231c`, surfaces s1/s2, the burnt-orange
  `--yap-live` and recording-red `--yap-live-strong`, the spring
  `--yap-ease-spring`, popover/dialog/sheet shadows). Top to bottom:
  **header** (52 px, a drag region): **back** (beige square → the note in the
  main window's Notes, `yap-meeting-open-note`), **⋯** (Copy as Markdown /
  Copy as text / Copy consent message / Audio settings while recording →
  Settings → General / Save as .md via the dialog plugin + `note_export` /
  **Delete** in red → a Wispr-style confirm dialog → `meeting_delete`),
  **split screen** (tooltip "Split screen with meeting"; hover →
  `notepad_split_preview`, click → `notepad_split`), a **Share** split button
  (a local "Share notes" popover: Copy as Markdown / Copy as text / Save as .md
  / Email = a `mailto:` with the summary as text; the link half copies the
  notes in one click, its icon swapping to ✓), and min / max / close (close
  hides; the recording goes on). Ink tooltips via `data-tip`. **Title**: a
  textarea in EB Garamond 36/45, −0.04em, placeholder "New note", growing with
  its lines; a made-up title stays muted until the AI or the person names it;
  the date in the locale's format ("5 Oct, 22:32"). **Tabs** (15/600, ink
  underline, a hairline under the row): My thoughts (the note's `content`,
  autosaved, synced with NotesView both ways via `yap-note-changed`/`origin`),
  Transcript (a 3-bar burnt-orange waveform while recording), Summary ("+" until a
  summary exists). **Transcript**: the timer box (clock, elapsed, search —
  an inline input that filters and highlights lines, dividers hidden — and
  copy all) with the dismissible tip strip attached; speaker groups (a run of
  one voice: "You" / "Them" in teal, or the speaker map's names — "Priya",
  "Speaker 2", a guess in italics as "Priya?") of chat bubbles with Wispr's
  grouped corners and, on hover, a person button ("Who said this?") and a
  copy button each; the name (or the person button, for one line) opens
  **`SpeakerMenu.svelte`** (shared with the Notes view): "It's Priya" / "Not
  Priya" on a guess, Rename X everywhere…, Rename just this line / these N
  lines… (a name field with the attendees and known names as picks), Merge
  with… (other voices; never You) → the `speaker_*` commands; other windows
  follow `yap-note-changed`'s `lineSpeakers` (`withLineSpeakers`); Escape
  closes just the menu; a dashed **⏸ divider** wherever a recording stopped
  (`note.breaks`, `yap-meeting-breaks`) and a "**You dictated here · left
  out of the notes**" divider (mic glyph) for a `dictated` marker; echo
  behind "Show N lines…"; "Yap is listening" / "No transcript yet" / "Live
  transcript is off" states (layout: `notepadText.js` `transcriptItems`,
  `bubbleShape`, `highlightParts`). **Summary** (the Rust job: "• Turning 12
  minutes of talk into an action plan… · Step 2 of 3", "Catching up on the
  meeting: part 3 of 12…", the rendered plan, the AI-setup card, the digests
  so far while recording). **Bottom** (over the content, a 150 px fade):
  recording → the consent line "Let people know you're taking notes. **Learn
  more**" — a popover with why ("Yap transcribes on this PC; nothing is
  uploaded") and an **editable message for the meeting chat** (default in
  `notepadText.js`; saved on blur via `notepad_consent_message` →
  `meetingConsentMessage`, empty = the default; Copy → "Copied: paste it into
  the meeting chat"); after the meeting → an ink **Generate summary** pill
  (no summary yet) or the summary's error line ("The summary didn't come
  through" + detail + a Retry icon, `role="alert"`); then the **bar** (a
  32 px-radius tray): a white pill **Stop** / **Resume** / **Start** (recording-red
  ■ or ring-dot) and the **Ask anything** pill input, with the lead helper
  chip inside it while the sheet is down — "What did I miss?" while
  recording, "Follow-up email" after (a send arrow once typed). Focusing it
  or the chip opens the **Ask sheet** (a bottom sheet, 45% tall, sheet
  shadow; grab handle, New chat, Minimise; Stop shrinks to a 48 px circle):
  right-aligned question bubbles, markdown answers with 👍 👎 ⎘ (ratings kept
  in this webview's localStorage only, `yapNotepadFeedback`, never sent; the
  email shown `plain`, pre-wrapped, exactly as ⎘ copies it), and under the
  thread a **Suggestions** row of one-tap helper chips switching with the
  recording state (`meetingHelpers`/`MEETING_HELPERS` in `notepadText.js`:
  What did I miss? + Questions to ask while recording, Follow-up email +
  Gaps and risks after; hidden while a question is typed; the pill chip
  moves the focus to the field as it goes);
  "What did I miss?" = `meeting_catch_up` with `since` = the segments seen
  (all of them whenever the Transcript tab is on screen — window shown per
  `yap-notepad-visible`/`isVisible` — and scrolled to its newest line
  unsearched, or covered by the last answer); typed questions = the same
  command with `question`; no AI → a link to Language Models. Escape or a
  click above it puts it away. Toasts sit above the bar
  (`--yap-toast-bottom`); the quiet countdown, "Still in your meeting?" and the quiet stop's
  notice whenever Rust picks the notepad (it's on screen;
  `meetingCheckIn.js`). It has
  the **in-page hotkey fallback** (dictation, and the meeting shortcut when
  one is set), config re-read on focus.
- **`lib/SplitPreview.svelte`** — the split preview window's page (label
  `split-preview`, App.svelte routes it, transparent body): one glass
  rectangle inset 24 px (`rgba(255,255,255,.10)`, a 2 px
  `rgba(255,255,255,.45)` border, radius 12, a dark hairline + soft shadow),
  readable against any desktop.
- **`lib/Settings.svelte`** — the settings surface, now rendered **inside the
  ControlPanel's modal** (`embedded` prop; ✕ closes). Grouped sidebar (App / AI models / Data / Connections / System):
  two pages for two different things (2026-10-18; both were "Voice
  Agent"), each opening with one sentence and an example in a note
  (`role="note"`) that links to the other: **Commands** (App, after
  General; id `commands`): "Tell Yap's AI what to write, and it types the
  result where you are" — *Say "Hey Jarvis, reply that I'll be late" in
  any app; Yap writes it and types it there* (the agent's name, live); **Say
  its name** (`#settings-wake-word`: the Commands scope's enable switch
  `llmScopes.voiceAgent.enabled` "Answer to its name in any dictation", the
  agent name — `AgentNameField.svelte`), **Rewrite selected text** (the
  **Rewrite shortcut**, `editHotkey`, moved here from General →
  Activation), **Model and prompt** → Language Models → Commands. **Voice
  Assistant** (id `voice`): "Ask Yap something by voice and hear the
  answer, like a voice assistant" — the example names the shortcut once
  set; **Assistant shortcut** (`#settings-assistant-shortcut`,
  `ui/HotkeyInput` `assistantHotkey`, clearable, "Set a shortcut", with a
  gentle "Set an assistant shortcut…" line while none is set; its line
  follows the recording mode), **Opening apps** (`OpenAppsSettings.svelte`,
  `#settings-opening-apps`: "Let Yap open apps when you ask" `openApps`;
  **Nicknames** — "Say … → App to open" with a type-ahead of the apps,
  `appNicknames`, duplicates refused, ✕ to remove; **Apps Yap found** —
  `apps_list`, a count, Refresh, a search box, two columns of the app's
  own icon (`apps_icons`, else its letter) + a switch each, off = "Yap
  won't open this one" `appsBlocked`; the first 40, then Show all),
  **Web search** (`WebSearchSettings.svelte`, `#settings-web-search`:
  "Look things up on the web" `webSearch`, off; **"Search with"**, a radio
  list: **The AI's own search** (Recommended, `webSearchNative`, the
  default: "Cloud models (Groq, Google Gemini, Anthropic, OpenAI,
  OpenRouter) search with their provider's built-in search, on the API key
  Chat already uses. Local and self-hosted models use Yap's search below.")
  / **Exa** / **Tavily** (every model, `webSearchService`); with the AI's
  own search, **"What each provider charges for its search"** (Groq: no
  charge on the free plan but ~4,000 of its 8,000 tokens a minute per
  searched answer, paid $5/1k + $1/1k pages; Gemini 3+ billing only,
  5,000 a month free then $14/1k — on a free key Yap's own search answers —,
  2.5 Flash/Flash-Lite where still offered 500 a day free, free tier used
  to improve Google's products; Anthropic and OpenAI $10/1k + tokens; OpenRouter the
  provider's price or $0.007 a search; as published 18 Oct 2026) and
  **"Yap's search"** (Exa (Recommended) / Tavily, for local and
  self-hosted models and when a provider's search is unavailable); each
  key masked in `webSearchKeys` with "Get a free key →" (its dashboard via
  `open_external`) and its free allowance; "Wikipedia and reference wikis"
  (no key needed), always included; where the search words go (a cloud
  model's provider, else the service / Wikipedia); the explain line adds a
  web example while it's on), **Spoken replies in Chat** ("Speak replies" `speakReplies`;
  "When to speak" Segmented When I asked by voice / Always `speakWhen`;
  "Talk back hands-free" `talkBackHandsFree`, greyed while replies are off),
  switching "Speak replies" on while Kokoro isn't downloaded offers it
  under the switch (`KokoroOffer.svelte`, "Download Kokoro (379 MB)" / Not
  now; replies stay on, Windows voices meanwhile), **Voice**
  (`VoiceSettings.svelte`: **Engine** Segmented Kokoro / Windows voices
  `ttsEngine`, "Natural voices, made on this PC. Recommended"; Kokoro's
  **download row** (`ModelRow`: Kokoro-82M · size · 54 voices ·
  Recommended, Download → progress → Active, delete via `ui/ConfirmDialog`;
  in a stub build "In the full Yap build"); the **voice picker** following
  the engine — Kokoro's 54 in `<optgroup>`s British English / American
  English / Other languages × Female / Male (`kokoroOptions` in
  `kokoro.svelte.js`, "Bella (default)", `ttsVoice`), or Windows' with its
  default first (`ttsWindowsVoice`) — **▶ Preview** / ■ Stop →
  `tts_preview` with the shown engine, voice and speed (off until Kokoro's
  downloaded); Speed slider 0.5–2.0× `ttsRate`; Volume slider `ttsVolume`
  (also in Chat's + menu, beside the Voice replies switch); the credit line: Kokoro
  Apache-2.0 from onnx-community, misaki Apache-2.0, eSpeak NG GPL-3.0 run
  separately, licences and the source link saved with them;
  `kokoro.svelte.js` is the download store) and **Model** → Language
  Models → Chat & Voice Assistant. `yap-voice-replies` (Chat's toggle) is
  adopted into the config copy. **Hotkeys**: the four (`HOTKEY_CMD` /
  `HOTKEY_NAMES`: dictation hotkey, rewrite shortcut, meeting shortcut,
  assistant shortcut) are refused when they'd fire on one press
  (`hotkeysClash`: "Already used by the …", or "Clashes with the … (Win +
  Alt + M): one press would start both"); the in-page fallback catches the
  meeting, assistant and dictation keys, in that order. **Language Models**
  tabs read Dictation Cleanup / **Commands** (`VoiceAgentConfig.svelte`,
  scope `voiceAgent`, "Name and shortcut in Commands") / Note Formatting /
  **Chat & Voice Assistant** (`ChatConfig.svelte`, scope `chat`, a line
  linking to the Voice Assistant page).
  **General** (hotkey, recording mode — in push-to-talk its line says how
  double-tap goes hands-free, "More shortcuts" naming the rewrite and
  assistant shortcuts with buttons to Commands / Voice Assistant, mic, sound+volume, then "Silence other apps
  while you dictate" (`quietOtherApps`) and "Pause music and videos while
  you dictate" (`pauseMedia`) where "Mute while recording" was; **Yap bar** group
  (`#settings-bar`, the bar menu's Settings → "general#bar"): Show the Yap bar
  (`barEnabled`; "Turn off the bar" in its menu arrives as `yap-bar-changed`
  and is adopted), a "Hidden until 15:40 · Show it now" row while hidden for an
  hour, Hide in fullscreen apps (`barHideFullscreen`), the live-preview toggle,
  "Cancel and finish buttons while dictating" (`ui/Segmented` When you point
  at it / Always / Off → `barDictationButtons`, Off by default, a line per
  choice)
  and Position (`overlayPosition`) — the dictation overlay itself is always on,
  it's the hot-mic indicator; **Notifications** group
  (`#settings-notifications`): "Show tips" (`showTips`, on: coaching hints
  and the calendar nudge; questions, alerts and errors always show);
  **Meetings** group (`#settings-meetings`, the
  target of `yap-settings-goto` "general#meetings"): "Detect calls and offer to
  take notes" (`meetingDetection`), then — disabled while it's off — "How Yap
  asks" (`ui/Segmented` Pop-up / Quietly → `meetingDetectStyle`, a one-line
  explanation of the choice) and "Ask about calls in" (a two-column list,
  one row per call app from the `meetingDetect.apps` snapshot: [its icon,
  `CallAppIcon` at 20 px — the real one as installed, else its mark or
  letter; greyed with the switch while detection is off] [name] [switch],
  showing the effective choice; a flip writes `meetingDetectApps[app]`; a
  `yap-meeting-detect-choice` from Rust is adopted into Settings' config copy
  so auto-save can't undo it), then, in Wispr's order: the guard rails
  "When a call ends" (Select: Ask me / Stop and summarise automatically →
  `meetingCallEnd`; disabled while detection is off), "Maximum recording
  length" (Select 1–4 hours / No limit → `meetingMaxMinutes`), "End notes
  when the meeting goes quiet" (`meetingEndWhenQuiet`, where Wispr has its
  auto-end switch: after the length), "Hide Yap's
  meeting windows from screen sharing" (`meetingHideFromCapture`, wrapped in
  `#settings-screen-sharing` — the screen-share tip's "Update settings"
  target); the meeting notepad's "Open the notepad when a meeting starts"
  (`meetingOpenNotepad`) and "Split the screen when joining"
  (`meetingSplitScreen`, greyed out while the first is off); "Meeting
  shortcut" (`ui/HotkeyInput`, clearable → `meetingHotkey`; paused while
  capturing via `configure_meeting_hotkey`; the three hotkeys can't clash);
  "Notify before scheduled meetings start" (Select: Right before / 1 minute /
  2 minutes / Never → `meetingReminder`) with a "Calendar" row under it
  (Connected: <account> + Manage, or Connect calendar → Connectors);
  "Show live transcript" (`meetingLiveTranscript`, the notepad's live lines)
  + "Tell speakers apart in meetings" (`SpeakersToggle`, the same switch as
  below, + a link to the models) + an always-visible consent line, "Recording a
  call? Let people know you're taking notes."), **Speech-to-Text** (`ModelManager` + GPU +
  language/translate, and **Speakers in meetings** (`#settings-speakers`,
  `SpeakerModels.svelte` + `diarization.svelte.js`, `diarize.rs`): the speaker
  models in `ModelRow`s (Download / Active / delete — the active one too;
  the fallback greyed "For phone-only calls — coming later", status
  `unavailable`), the **"Tell speakers apart in meetings"** switch (Beta chip,
  "It all runs on this PC"; `SpeakersToggle.svelte`: off until a model is
  installed — switching it on without one offers "Download and turn on" /
  Not now with a progress bar instead; deleting the active model turns it
  off), and where the files come from with their licences (OpenMDW-1.1,
  Apache-2.0)), **Language Models** (OpenWhispr-style: enable toggle → mode
  selector Cloud Providers/Local/Self-Hosted → provider pill tabs (Groq/Anthropic/
  OpenAI/OpenRouter/Custom, brand icons) → API Key (masked + "Get your API key"
  link) → Select Model rows (`ModelPicker.svelte`: `ppModels.js`'s curated
  list merged with the provider's live one by `modelCatalog.js` — curated it
  still lists, then "New" (released after `CURATED_AT`, by the provider's
  date or, for Gemini's dateless list, not in the registry's `seen`; 120
  days), the rest behind "Show N more" with a filter past 12, the saved
  model it no longer lists kept with "No longer offered" and a line saying
  so, "Groq's current models, checked 2 h ago · Check now" or "Couldn't
  check…: … Showing Yap's list. · Try again"; no key, offline or a failed
  check = exactly the curated list; switching provider picks its first model
  it still offers; `ui/SelectList.svelte` draws the tags); a self-hosted or
  custom endpoint's Model field suggests its server's `/models`
  (`EndpointModels.svelte`, a `<datalist>` + "3 models on this server"), as
  do the cleanup profiles' model fields;
  UI-only `ppMode`/`cloudProvider` state resolves to the unchanged `ppProvider`
  contract. Local = the on-device model browser (family tabs, Download / Use /
  delete; each card's size line ends with its maximum, "2.8 GB · up to
  256K"; above the cards the same **Context** control as the Chat picker's,
  `LocalContext.svelte`, adding "Shared by Chat, dictation cleanup and
  meeting notes."; Settings adopts `yap-local-context-changed` and re-reads
  on `yap-local-llm-changed`): **"Recommended for this PC"** on `local_llm_status`'s pick (Qwen3.5
  4B with a capable card, else the 1.5B; `isDefault` = the model an empty
  `ppLocalModel` means), and under its heading one quiet line at most — the
  4B offered once (`upgrade`: "Your NVIDIA GeForce RTX 5070 Ti can run Qwen3.5
  4B…" Download / No thanks → `localLlmUpgradeDismissed`), or, a big model
  left on the processor (`slowOnThisPc`, `placement`), "Runs on the processor
  on this PC: answers will be slow — Qwen2.5 1.5B Instruct is quicker." + Use
  it (read afresh whenever the section opens). Below it, **Prompt Studio** (`PromptStudio.svelte`, OpenWhispr port):
  View (full effective prompt via `get_base_prompt`) / Customize (preset +
  body + Save/Reset) / Test tabs. Plus usage meter + profiles w/ per-profile
  model override + per-app rules), **History** (stats
  dashboard + recent list + enable/clear), **Advanced** (output toggles —
  "Match the text around the cursor" (`smartSpacing`) first, "Leave the full
  stop off chat messages" (`chatNoFullStop`), then "Append
  trailing space", whose line reads "Only where Yap can't read the text around
  the cursor" while the first is on, auto-submit (its line adds "In a terminal,
  only for coding tools like Claude Code" while terminal pasting is on), "Say
  “press enter” to send" + the key, "Safer pasting into terminals"
  (`terminalMode`), and "Restore clipboard after
  paste", which says dictations never show up in Win+V history — system,
  dictionary), **About** (version + an **Updates** card on the shared update
  store: "Last checked …" / Check for updates, Downloading… %, "Yap X is ready to
  install" + Restart to update, release notes + release-page link, the
  "Check for updates automatically" toggle), **Connectors** (first in the
  **Connections** group; `ConnectorsSection.svelte`, laid out like Wispr's
  Settings → Connectors: serif title + one line on what a calendar gives you,
  a rounded light card per connector — **Google Calendar** ("Reminders for
  Google meetings before they begin", Connect → the browser, "Finish in your
  browser…" + Cancel; a build without the client says "Google sign-in is in
  the installed Yap. In this build, use a private iCal link below, under
  Other calendar."), **Outlook
  Calendar** (Connect → the guided publish-your-calendar ICS form), **Other
  calendar** (an iCal link, help for Google/iCloud/Fastmail) — each
  connection a row with its account or link host, "Synced …" or the error,
  "Connect again" when Google needs it, and a ⋯ menu Sync now / Disconnect;
  then a plain row with the AI apps' logos, "Give your AI access to your
  meeting transcripts and notes" + **Go to MCP**), **MCP** (in the same
  **Connections** group, between Data and System; `McpSection.svelte`, laid out like Wispr's Settings →
  MCP: serif title, a light card each for Claude / ChatGPT / Gemini / Cursor with
  "Allow X to access your meeting notes and transcripts" + **Add to X** (→ Added ✓
  · Remove), the ChatGPT card's "desktop app and Codex, not the web" note, then
  **All other apps:** with one-click rows for Claude Code / VS Code / Windsurf +
  the command and JSON, the "Let AI apps save notes to Yap" switch
  (`cfg.mcpAllowWrites`), a Local-API-off warning with "Turn on the Local API",
  and the privacy line; it re-reads when it comes into view, as Settings stays
  mounted), and **Account** (bottom of the
  sidebar — `AccountSection.svelte`). The status bar's update link follows the
  same store (Check for updates → Restart to update / Downloading… / Up to date ✓).
- **`lib/AccountSection.svelte` / `account.svelte.js`** — Settings → Account:
  Continue with Google/GitHub/Discord, "Email me a code" → 6-digit entry
  (auto-submits), a waiting state with paste-the-code fallback, then the
  profile (avatar or initials, linked providers), sign-out, a **"Where you're
  signed in"** group (one row per session: label + amber "This device" chip,
  "Signed in <date> · last active <day>" — day granularity, the server only
  renews a session about daily — per-row Sign out for the others, and "Sign
  out of other devices" with a confirm step; loads whenever a session shows,
  for a session of any age), and delete-account (with an in-place "Confirm
  it's you" re-sign-in when the server wants a session < 1 day old; it only
  deletes if the same account signs back in). **Sign in with your phone**
  (under the provider buttons; not in "Confirm it's you", which a phone
  sign-in can't satisfy) swaps the options for the QR code from
  `status.device` (its SVG path at 5 px per module, `forced-color-adjust:
  none`; the code in large monospace, spelled out for screen readers; the
  "Can't scan?" typed-address fallback; an `aria-live` "Waiting for your
  phone…" with a quiet "New code in m:ss"; Cancel). A lapsed code (clock at
  0 or `device.expired`) is blurred under **"Show a new code"** (WhatsApp's
  reload-QR pattern); while the panel is really in view (window visible —
  asked of the window on every focus change, `isVisible` && !`isMinimized`,
  since WebView2 reports the page visible while the window is hidden — +
  IntersectionObserver, as the Settings modal hides with `display: none` but
  stays mounted) it renews itself at most 3 times per start by the person
  (every poll costs the service D1 writes), then waits for the button. Focus
  goes to the panel heading on start and back to the phone button (signed
  in: the profile heading) when the panel closes. It stays until the phone
  approves — then it flips to signed in like any sign-in — or the attempt
  fails (toast says why). `account.svelte.js` is the
  shared runes store (`auth_status` + `yap-auth-changed`) that also drives the
  "signed in as" buttons at the bottom of both sidebars (ControlPanel +
  Settings) and toasts background sign-in results.
- **`lib/ModelManager.svelte` / `ModelRow.svelte` / `models.js`** — the 14-model
  browser, OpenWhispr-style: vendor pill tabs (All/NVIDIA/OpenAI/Community via
  `ui/PillTabs.svelte`) + compact one-line rows (status dot, brand icon from
  `providerIcons.js` + `assets/providers/*.svg` (MIT, from OpenWhispr), name,
  size, Download/Active/delete; delete asks first in `ui/ConfirmDialog.svelte` —
  ⚠ never `window.confirm/alert/prompt` in a Yap window: Tauri routes them to the
  dialog plugin, which these windows aren't allowed, so they throw).
  `ModelCard.svelte` (big cards) remains only in
  Onboarding. `ModelRow` also draws the speaker models (`SpeakerModels.svelte`:
  `unavailable` = a listed model's note instead of Download, `deleteActive`).
- **`lib/Onboarding.svelte`** — first-run **guided setup** (5 steps): model picker →
  mic check (live level meter via `set_mic_test` idle-amp mode + live device switch)
  → one-click **local AI cleanup** install (the model recommended for this PC
  first and picked: Qwen3.5 4B with a graphics card that can run it, "Your
  <card> runs Qwen3.5 4B quickly…", else Qwen2.5 1.5B) → tray pointer → "try
  it here" live dictation test with a change-shortcut recorder.
- **`lib/ui/`** — primitives: Toggle, Select, Slider, Group, Row, Button, Input, Textarea,
  **ContextMenu** (at the pointer or under a ⋯ (`alignEnd`), clamped to the
  window; ↑/↓/Home/End/Enter, → into a submenu, ← / Esc back; Esc, Tab, a
  click outside, a resize or a scroll close it and the focus goes back to the
  row; `{ label, icon, onselect, disabled, danger, checked, toggle, items }`,
  separators and a quiet `note`; it cancels `contextmenu` on itself; a
  scroll in its first 250 ms — the right-clicked row scrolling into view —
  doesn't close it) and **ConfirmDialog** (the notepad's Wispr-style confirm
  for the main window: Cancel focused, Esc/scrim cancel, Tab kept inside,
  the focus returned). **Right-click is app-wide and the same everywhere**
  (`ui/contextMenu.svelte.js`): one menu + one confirm per window
  (`ui/ContextMenuHost.svelte`, mounted by App.svelte in the main window,
  onboarding and the notepad); `openContextMenu(e, items, {label, row})`
  from a right-click (at the pointer), a row's hover ⋯ (`data-more`: under
  it) or the keyboard (`menuKey`: the ContextMenu key, Shift+F10); nothing
  to offer → the event is left alone (WebView2's filter shows nothing); a
  right-click in a text field or on selected text is the native edit menu's;
  `askConfirm({title, body, run, returnFocus})` for every delete; Esc
  handlers elsewhere wait for them (`menuOrDialogOpen`). One order: the main
  action, Copy-type ones, the destructive one last in red behind the
  confirm; a right-click also selects the row where a click does (unless
  that loses unsaved work). Where: Notes' folders and notes (+ its Ask
  thread and transcript lines), Chat's conversations (Rename in place,
  Copy as Markdown, Delete…; the hover ⋯ replaced the trash) and messages
  (Copy / Copy as Markdown / Read aloud; a question: Copy — `itemMenus.js`),
  Home's dictations (Copy, Copy raw transcript, Delete…; the trash asks
  too), Meetings' past notes (Open, Generate summary, Copy as Markdown,
  Delete… via `meeting_delete`; a hover ⋯) and calendar meetings (Open note
  + their row buttons), Dictionary rows (Edit, ≈ Catch near-misses,
  Delete…; × asks too unless the row is empty), model rows (`ModelRow`:
  speech, speaker, Kokoro — Download / Use / Delete…) and the local LLM
  cards (Download / Use / Delete…, now asked first), Connectors' calendars
  (their ⋯ = this menu: Sync now, Disconnect… asked first), Account's other
  devices (Sign out), the Actions manager's rows (Edit, Delete…), the
  notepad's transcript lines (Copy line, Who said this?) and Ask answers
  (Copy, Copy as Markdown, Read aloud, Good / Bad answer) and questions
  (Copy). `e2e/right-click.spec.js`.

### Window config (`src-tauri/tauri.conf.json`)
- **settings**: 1200×800 (min 860×600), titled "Yap" (it hosts the ControlPanel — see above),
  **undecorated** (custom in-page title bar w/ drag region + caption buttons — see the
  "Custom window chrome" bullet above), hidden, hide-on-close. Size/position/maximized
  persisted by `tauri-plugin-window-state`
  across launches; overlay, onboarding, notepad, split-preview are excluded from persistence; the VISIBLE flag
  is excluded so the window never un-hides on start-hidden launches (see lib.rs window-state
  plugin setup).
- **onboarding**: 620×720 (min 520×560), hidden, hide-on-close.
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

---

## Build, run & feature flags (important)

Cargo features in `src-tauri/Cargo.toml`:

| Feature | Effect |
|---------|--------|
| *(default)* | **STUB** — no `transcribe-rs`. `cargo check` stays fast. The app runs but returns placeholder text. |
| `engines` | Real multi-engine STT: `transcribe-rs` with **whisper-vulkan** + ONNX + **ort-directml**. Whisper runs on the GPU via **Vulkan** (any GPU), ONNX via DirectML; CPU fallback with no GPU. **This is what release + nightly builds use.** |
| `whisper` | Back-compat alias for `engines`. |
| `custom-protocol` | Required for release/standalone builds — embeds the frontend. `tauri build` sets it automatically. |

GPU policy: **UNIVERSAL, no CUDA.** Whisper → **Vulkan** (NVIDIA/AMD/Intel; `vulkan-1.dll`
ships with the GPU driver), ONNX → **DirectML** (any DX12 GPU). Same approach as **Handy**
(`references/Handy`, whose Windows target is `["whisper-vulkan","ort-directml"]`). Building
`whisper-vulkan` needs the **Vulkan SDK** at build time (glslc + headers + loader) — install
from https://vulkan.lunarg.com locally; CI uses `humbletim/install-vulkan-sdk`. No nvcc /
CUDA arch list. One small installer, GPU on every GPU.

### Run in dev (what we use)
Use **`scripts/dev.bat`** ("yap.dev") — it runs `npm run tauri dev -- --features engines`
(the **real** GPU pipeline; needs the Vulkan SDK installed). A commented line switches to the
fast no-GPU stub for pure UI work. Dev hot-reloads the frontend on every edit (Vite on **:51437**).
A **"Yap - Dev"** desktop shortcut launches it; the plain **"Yap"** desktop shortcut is the
*installed* app (`D:\Hobby Project\Yap`, follows the nightly channel).
**It always runs the latest main** (2026-10-17): before starting, dev.bat runs
`scripts/dev-update.ps1`, which fast-forwards the checkout to `origin/main` (putting back
files whose only change is line endings — the Tauri CLI rewrites `src-tauri/Cargo.toml` —
listing what's new, and running `npm install` when `package-lock.json` changed), then
dev.bat starts its updated self afresh (cmd reads a running batch file line by line, so
reading on in a rewritten one would go wrong). It skips the update and says why on another
branch, with real local edits, offline, or when it isn't a fast-forward — Yap still starts.
It then stops any AI app's `yap.exe mcp` helper running from the dev build's own exe
(`<drive>:\t\debug\yap.exe`), which would otherwise make the build fail with "Access is
denied" (see `mcp_clients.rs`).
So merged work reaches Nathan by pushing to main; never fast-forward his checkout by hand
while Yap - Dev runs.

```bash
# real GPU pipeline (default in dev.bat) — requires the Vulkan SDK
npm run tauri dev -- --features engines
# stub (fast, no transcription)
npm run tauri dev
```

Accounts: debug builds sign in against a local copy of the account server —
`cd cloud && npm run dev` (wrangler dev on :8787; email codes print to its
console; `npm run mock:provider` stands in for a social provider). See
`cloud/README.md`.

> ⚠️ A *compiled release build* bakes the frontend into the binary — editing `src/`
> and restarting that `.exe` changes nothing. For live frontend changes use dev
> (Vite on :51437). If :51437 isn't listening, you're looking at a release build.

### CI on every push
`.github/workflows/ci.yml` runs on every push/PR to `main`: `npm run build` (frontend,
also produces the `dist/` that `generate_context!` needs) + `cargo clippy --locked
--all-targets -- -D warnings` (test code included) on the fast **stub** build (no
`engines`, no Vulkan SDK) — a few minutes on a Windows runner, so a broken commit
can never reach a nightly. The real GPU pipeline is only exercised by
nightly/release builds. ⚠ Zero-warnings ratchet: any warning or clippy lint fails
the push, and the runner uses the **latest stable** toolchain, whose clippy can flag
lints an older local one doesn't — keep local Rust current (`rustup update`) or read
the CI log. (It sat red Jul 9 → Sep 25 2026 on accumulated lints, masking real
failures.)

`.github/workflows/e2e.yml` runs the **end-to-end UI suite** on the same triggers
(plus `workflow_dispatch`), separately so `ci.yml` stays quick — see below.

### End-to-end UI tests (`npm run test:app`)
Playwright drives a real **stub** debug build (frontend embedded) over WebView2's
CDP: every window is a page, tests click through Home / every view / every
Settings section / Notes / Dictionary / Account / onboarding / a stub dictation /
the no-mic path / push-to-talk's tap, hold and double-tap + the dead-mic watchdog /
the update toast / meetings and the meeting notepad (its own
page, `yap.notepad`), and save **named screenshots** to
`test-results/app/screenshots/<spec>/` (git-ignored; HTML report in
`test-results/app/report/`, each instance's logs + data in `test-results/app/runs/`).
Any uncaught JS error in any webview fails the test. ~20 s plus the build
(`npm run test:app:build`, own target dir `src-tauri/target/e2e` or
`YAP_E2E_TARGET_DIR`). Safe beside the installed app and a dev build: portable
data dir, own WebView2 profile + CDP port, F24 hotkey, test mode (`e2e.rs`),
unreachable account service. (Runners are elevated, so WebView2 ignores the suite's
`WEBVIEW2_*` variables there; e2e.yml sets the DevTools flag as an HKLM WebView2
policy for `yap.exe` instead.) CI uploads `test-results/app` as the **`e2e-results`**
artifact on every run — review it with
`gh run download <run-id> --repo nayballs/Yap --name e2e-results --dir e2e-<run-id>`
and read the PNGs under `screenshots/`. Full guide (incl. adding a test):
[`docs/e2e-tests.md`](./docs/e2e-tests.md).

**The speaker engine** has three layers of tests: unit tests (`diarize/*`:
the wire, dither, speaker order, timelines, the registry, the zip), the e2e
`diarization.spec.js` (Settings with a fake model + runtime from a local
server via `YAP_DIARIZATION_SOURCES`; test mode never downloads from the
internet), and, locally with the files on disk (never on CI, never
downloading): `cargo test --test diarize_child -- --ignored --nocapture`
(`YAP_DIARIZATION_MODELS`, `YAP_DIARIZATION_AUDIO` = the spike's
`audio/conv`: the real child on its WAVs, scored against their RTTMs) and
`diarization-real.spec.js` (`YAP_E2E_DIARIZATION_MODELS` /
`_AUDIO`: `diarize_wav`, and a meeting recorded from a 3-voice file with
the engine labelling it, its "Them" lines ending with one voice per person;
`YAP_E2E_DIARIZATION_STT` = a Parakeet folder with an `engines` build as
`YAP_E2E_EXE` scores the words put on the right person). The labels on
lines without a model: `speaker-labels.spec.js` (scripted timelines,
`e2e_diarize_timeline`). The real download once:
`cargo test real_download -- --ignored --nocapture`
(`YAP_DIARIZATION_DOWNLOAD_DIR`, ~112 MB).

### Real-app robot tests (`npm run test:real`)
The second layer: a **robot-mode** Yap (`realtest.rs`) pastes into **real windows
the harness opens** — a WinForms test pad (`e2e-real/support/yaprobot.cs`,
compiled with the .NET Framework's `csc`), Windows Terminal (a PowerShell prompt,
and a stand-in for Claude Code: its "✳ …" title + bracketed paste, runs nothing),
Edge app windows with throwaway profiles (a web page's text box; a "WhatsApp"
page = a chat app), Notepad (skipped while Notepad is open) — and the specs
(`e2e-real/*.real.js`) assert what actually landed: smart spacing, snippets,
"press enter", Paste last transcript, the clipboard (yours back, private while
pasted), AI cleanup via the fake model, terminal one-line mode / coding-CLI line
breaks / auto-submit, chat full stops, the mic path from a TTS WAV
(`realtest_dictate_wav`) incl. the dead-mic watchdog, the **correction-learning
flow** (the real watch over UI Automation, a word fixed in the field, Enter, the
suggestion toast, Add → `config.json`), **other apps' sound** (the test pad
looping a near-silent tone goes to 0 after the start chime and back to exactly
its level; an Edge page whose media title has the nonce pauses and plays
again: `other-apps.real.js`), an admin app (a stand-in: the
text left on the clipboard, nothing pasted), ✕ then Undo pasting into the
window the dictation started in (even with another window in front), and **real STT** (Parakeet
hears Windows TTS saying "the quick brown fox…") on an `engines` build
(`npm run test:real:build -- --engines` → `<drive>:\t-real`; the model copied
read-only from `%APPDATA%\yap\models`). **The guard**: Yap may paste / press keys /
read only in windows of `YAP_REALTEST_PIDS` (+ descendants, by start time) or
titled with the run's `YAP_REALTEST_NONCE`; everything else is refused (logged,
toast, `realtest_refusals`), the foreground re-checked right before every
`SendInput` and again just after (a focus change in between fails the test,
naming the process). Silencing and pausing other apps follow the same rule
(sessions of the test's processes, or of a process whose windows all carry
the nonce; media titled with the nonce): your own music is never touched.
⚠ **It takes the keyboard focus for ~2 min** (runs locally
only, not on CI): say so before running it while Nathan's at the PC; another app
grabbing the focus mid-run makes Yap refuse (the harness retries a refused paste
twice). Known gaps it found are "known gap: …" tests pinning today's result (see
docs). Guide: [`docs/e2e-tests.md`](./docs/e2e-tests.md) "Real-app robot tests".

### Model evaluations (`npm run eval`)
How real models do on Yap's own Chat code, with every request and response
logged: `npm run eval -- --model groq:openai/gpt-oss-20b --keys-from-yap`,
`--model local:installed` (Yap's llamafile on the CPU; `--ngl 999` on the GPU),
`--only`, `--tools on`, `--raw evals/raw/groq-browser-search.json`, and
`--cleanup` (dictation cleanup latency per model). Nothing launches, no window, the
installed app's data and `%APPDATA%\yap` are never written (keys are read from
its config.json read-only). Builds a debug stub into `$YAP_EVAL_TARGET_DIR`;
runs land in `$YAP_EVAL_OUT` / `--out` (Nathan's PC: `E:/yap-builds/evals`,
`E:/yap-builds/eval-runs`). Use it before changing a prompt, a tool or the
default local model. Guide: [`docs/model-evals.md`](./docs/model-evals.md).

### The dev → nightly → stable workflow (agreed with Nathan, 2026-10-05)
**Dev first, nightly for batches.** Every change goes through these steps in order:
1. **Claude builds it and tests it.** `npm run build` + `cargo clippy --all-targets
   --locked -- -D warnings` + `cargo test`, then the e2e robot suite (`npm run
   test:app`: drives the real app in ~20 s and leaves screenshots to look at; add or
   extend a spec for anything new). For anything about **pasting, inserting text,
   the clipboard, terminals/chat apps, the mic path or STT**, also the real-app
   robot tests (`npm run test:real`, + `npm run test:real:build -- --engines` for
   real STT): they paste into real windows, so Claude checks it itself instead of
   handing Nathan a checklist (they take the keyboard focus ~1.5 min). Push to
   main: CI and the E2E workflow re-check every push.
2. **Nathan tries it in the dev build** ("Yap - Dev" desktop shortcut =
   `scripts\dev.bat`, the real GPU pipeline from this checkout). Anything visual or
   about how it feels gets checked HERE: Vite hot-reloads `src/` edits in seconds;
   Rust edits need the dev build restarted (a minute or two to recompile). Something
   off → Claude fixes it → he sees it straight away. Never burn a 15-min nightly to
   check something dev shows in seconds.
3. **Nightly, once a batch feels right.** Claude dispatches it (or the 05:00 UTC
   cron picks up main) and Nathan uses it day to day on the installed app.
   Nightlies catch what only the installed release build can show: the updater
   flow, the installer, Windows notifications under Yap's own name (dev builds
   borrow PowerShell's), `com.contextmirror.yap://` deep links, start on login.
   Those go straight to a nightly check; everything else goes through dev first.
4. **Stable** — tag `v*` deliberately for curated milestones, when Nathan's happy.

**Guiding Nathan through step 2.** When work is ready for him, don't just say "try
it in dev": give him the exact steps, every time:
- **Quit the installed Yap first** (tray icon → Quit). The dev build skips the
  single-instance lock and shares `%APPDATA%\yap` (config, hotkey, notes) with
  the installed app, so with both running one hotkey press starts two dictations.
- **Double-click "Yap - Dev"** on the desktop (it updates itself to the latest main
  first). It opens a console window: leave it
  open, closing it closes the dev Yap. The first start after Rust changes compiles
  for a few minutes; it's ready when the Yap window appears (and Vite listens on
  :51437).
- Then a short numbered checklist of what to try, what should happen (✅ lines),
  and what to send back (screenshots of anything off).
- Say which fixes appear on their own (frontend, hot reload) and which need the dev
  build closed and reopened (Rust).
- **When he's done:** close the dev console window, then start the installed
  **Yap** again (desktop shortcut or Start menu).
- He works day shifts and follows along on his phone while at work. Queue
  hands-on dev testing for when he's home, and keep at-work messages short.

### Release / installer
Tagging `v*` (or running the **release** GitHub Action) builds via `tauri-action`
with `--features engines`, producing a custom **NSIS installer** (normal/portable,
WebView2 bootstrap) + a signed `latest.json` on a draft GitHub Release. The in-app
updater (`tauri-plugin-updater`, driven by `updates.rs`) checks that endpoint ~30 s
after launch and every ~4 h while Yap runs, pre-downloads, and installs when the user
clicks "Restart to update". Its "What's new" comes from latest.json's `notes`:
both workflows run `scripts/release-notes.mjs`, the app's feat/fix commit subjects
since the previous release (stable: the previous `v*` tag, via tauri-action's
`releaseBody`, so polish the draft's text by hand; nightly: since the commit the
rolling release's body records as `<!-- built-from: <sha> -->`). **Currently unsigned**
(Authenticode) — Windows shows a SmartScreen warning until a cert is added; the
`signCommand` slot is ready. Updater artifacts are minisign-signed
(`TAURI_SIGNING_PRIVATE_KEY` GitHub secret). The uninstaller's **Delete the
application data** checkbox (`; --- YAP DATA ---` in `src-tauri/nsis/installer.nsi`)
removes Yap's real data (`%APPDATA%\yap`, `~\.yap`) **and the saved sign-in**
(`CredDeleteW` on the `yap-account.com.yap.dictation` credential) **and the
calendars' tokens/links** (`yap-calendar-1…8.com.yap.dictation`); never on `/UPDATE`
runs (the updater always passes it), and unticked keeps both — a reinstall stays signed in.
Google Calendar needs the build to carry an OAuth client: the optional
`YAP_GOOGLE_CALENDAR_CLIENT_ID` / `_SECRET` repository secrets, passed to the
build by both workflows (unset = the Connectors page says this build can't
connect to Google; iCal links still work). Setup: [`docs/calendar.md`](./docs/calendar.md).

### Release channels (stable + nightly)
Yap ships **two auto-update channels** (Chrome Stable/Canary style), both CI-built
on GitHub Actions (Yap builds cleanly there — ONNX + DirectML, no CUDA):

- **Stable** — tag `v*` → `.github/workflows/release.yml` → a normal (non-prerelease)
  GitHub Release. Installed stable copies check
  `…/releases/latest/download/latest.json` (the `endpoints` in `tauri.conf.json`).
  Cut deliberately for curated versions (`0.1.0`, `0.2.0`, …).
- **Nightly** — `.github/workflows/nightly.yml` (daily `schedule` cron at 05:00 UTC,
  plus manual `workflow_dispatch`) → a **single rolling `nightly` pre-release** whose
  assets are overwritten in place (`gh release upload --clobber`). Version is
  `<baseVersion>-nightly.<run_number>` (e.g. `0.1.0-nightly.42`) — a semver prerelease,
  monotonic via the run number so the updater always sees "newer". The installer + sig
  are renamed to the **constant** names `Yap-nightly-setup.exe(.sig)` so the download
  URL never changes across nightlies.

**Channel separation:** a nightly install follows the nightly endpoint because it is
built with `-c src-tauri/tauri.nightly.conf.json`, which overrides only the updater
`endpoints` to `…/releases/download/nightly/latest.json` (same identifier/productName
as stable — it's the same app on a different endpoint). Because a GitHub *pre-release*
never resolves as `/releases/latest/`, stable users never see nightly builds, and the
two channels don't cross. Both channels sign with the **same** minisign key
(`TAURI_SIGNING_PRIVATE_KEY`) — the pubkey in `tauri.conf.json` must match it or
installed copies reject updates. See `docs/SIGNING.md` for Authenticode plans.

#### How to run / cut a nightly (it's all CI — no local build needed)
- **Trigger a nightly now:** `gh workflow run nightly.yml --repo nayballs/Yap`
  (otherwise it fires on the 05:00-UTC cron). Then find the run:
  `gh run list --workflow=nightly.yml --repo nayballs/Yap --limit 1`
- **Watch it to completion:** `gh run watch <run-id> --repo nayballs/Yap --exit-status`
  (build ≈ 15 min — installs the Vulkan SDK, compiles whisper.cpp + Vulkan).
- **Verify it published:**
  `curl -sL https://github.com/nayballs/Yap/releases/download/nightly/latest.json`
  → the `version` field should be the new `0.1.0-nightly.<N>`.
- **Get it on this machine:** a running installed nightly notices it by itself within
  ~4 h (tray "Restart to update to …", the toast or a Windows notification) — or
  **Settings → About → Check for updates** / the tray's "Check for updates…" right away
  (an installed nightly auto-follows the nightly channel — no reinstall). First-time
  install: grab `Yap-nightly-setup.exe` from https://github.com/nayballs/Yap/releases/tag/nightly.
- **If a nightly build fails:** `gh run view <run-id> --repo nayballs/Yap --log-failed`.
- **Run from SOURCE instead (live dev, no release):** from the project folder run
  **`scripts\dev.bat`** (= `npm run tauri dev -- --features engines`). Hot-reloads the
  frontend on every edit. Needs the **Vulkan SDK** installed locally
  (https://vulkan.lunarg.com) so the `whisper-vulkan` backend compiles; the commented
  line in `dev.bat` switches to the fast no-GPU stub if you don't have it.
- **Requires** the `TAURI_SIGNING_PRIVATE_KEY` GitHub secret (already set). If that key
  ever has a password, also add `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.

---

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
- History: `%APPDATA%/yap/history.json` (local-only; cleared from Settings → History).
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
  the nudge (`calendar.rs`). Never a token or a link: those are Credential
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
- All of the above (plus config) write atomically and quarantine a corrupt file
  on load rather than crashing (`config::atomic_write`/`quarantine_corrupt`).
- App icons: `%APPDATA%/yap/icons/` — `<app>.png` per installed call app and
  `sources.json` (what each was made from); a cache, rebuilt whenever it's
  missing, stale or unreadable (`app_icons.rs`).
- Local API bridge discovery: `~/.yap/cli-bridge.json` (fixed path, NOT the
  data dir; written while the app runs, deleted on exit — see `bridge.rs` +
  `docs/local-api.md`).
- AI apps' own config files (Settings → MCP → Add to …): Yap only adds or
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
  (`learn_words` — it only asks; nothing is added without a click), looking
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
  `meeting_reminder`); spoken replies **off** (`speak_replies`; once on, only
  for questions asked by voice, `speak_when`, and a dictated question is sent
  by itself, `talk_back_hands_free`) in **Kokoro's af_bella** at 1× once
  Kokoro is downloaded (`tts_engine` "kokoro"), Windows' default voice
  until then.

---

## Current status

**Transcription is REAL and GPU-accelerated** (no longer a stub in `engines`
builds), and the dictation pipeline around it is complete: multi-engine STT
(Whisper/Vulkan + ONNX/DirectML), the 14-model registry + manager, recording modes
(push-to-talk with double-tap hands-free and coaching tips, 2026-10-17), a dead-mic
watchdog (reopen, then "No sound from <mic>"),
language/translate, **AI cleanup** (BYO key or local sidecar — Qwen3.5 4B on a capable
graphics card, else Qwen2.5 1.5B, 2026-10-18, chosen and measured with the model
evaluations; cleanup ~0.34 s a dictation on the 4B, ~0.13 s on the 1.5B, on an RTX
5070 Ti) with per-app routing +
named profiles + per-profile model choice, **Commands** (the rewrite shortcut +
the agent's name in a dictation; "Voice Agent" until 2026-10-18), the **Voice
Assistant** (`assistant.rs`, 2026-10-18: its own shortcut, ask Chat out loud
from any app and hear the answer, a bar capsule and card, follow-ups,
barge-in, off (and Yap silent) during a meeting recording or a call; the rules unit-tested and the whole
flow e2e-tested from a staged transcript with the main window hidden and
in front — not yet asked with a real mic and heard on speakers), **opening
apps when asked** (`apps.rs`, 2026-10-18: "open Teams" in Chat or out loud,
from the Start menu's list, with no model needed; nicknames, apps switched
off, the `open_app`/`list_apps` tools; matching and the request patterns
unit-tested, this PC's real list indexed read-only, the flows e2e-tested
against a stand-in list — never launching anything; same day, games: a
desktop app opens through its Start menu shortcut (World of Warcraft's
launcher had lost its `--productcode`), numbers and sound-alikes ("boulders
gate three"), "play X", "open it again", Yap answering "yes" / "the second
one" to its own questions, a yes to the model's offer as consent, lighter
tool steps for Groq's rate limit, no "Opening…" from a model that can't —
checked with real shortcut launches in a temp folder, not yet with a real
game), **spoken replies** (`tts.rs`, 2026-10-18: Chat's answers read
aloud in Windows' voices — the Voice replies toggle, ▶ on every answer,
"only when I asked by voice", hands-free talk-back, barge-in; text prep, the
queue and the decisions unit-tested, synthesis checked silently on this PC's
voices, the whole loop e2e-tested in test mode; not yet heard on speakers
by a person) with **Kokoro** as the natural local voice and the default
(2026-10-18: Kokoro-82M v1.0 fp32 on Yap's `ort`, CPU; measured on this PC
— Ryzen 7 9800X3D, release build, 4 threads: ~0.6 s to load + 0.17 s for a
lexicon, a 5.4 s sentence in 0.71 s, the research answer at RTF 0.12 (0.18
at 2 threads, 0.31 at 1), its first 6 s piece ready in 0.75 s, a stop ends
a sentence being made within ~10 ms; Parakeet round trip of the research
answer after the text pass: 0 misread words for af_bella / bf_emma /
bm_george (before it, "£12,400" lost its "pounds"); the download, the
Settings rows and the fallback e2e-tested against a local server, the real
download and msiexec unpack checked once; not yet heard on speakers by a
person), combo hotkeys, the audio pre-roll (anti first-word clipping), **live
streaming partials** (sliding-window, on by default, word-paced overlay reveal —
validated live 2026-07-10), transcription history + stats, cleanup presets,
**other apps silenced or paused while you dictate** (`mute.rs` + `media_pause.rs`,
2026-10-17: per app after the start chime, never calls, back to exactly
their level after the stop chime; the rules unit-tested, the volume drop and
restore and the pause and resume checked by the robot on an Edge page it
opened),
the dictionary's **snippets**, the **"press enter" voice command** and a Whisper
vocabulary ranked by use (2026-10-17: unit-tested, the Dictionary/Settings UI
e2e-tested, not yet tried with real speech), **terminal & coding-CLI pasting**
and **no full stop on short chat messages** (`app_kind.rs`, 2026-10-17:
classification and text rules unit-tested, checked read-only against this PC's
real windows, the Settings switches e2e-tested; not yet dictated into a real
terminal or chat app), **word suggestions from your corrections** (`learn.rs`,
2026-10-17: fix a word Yap typed and it offers to add it to the dictionary;
the same day aligned with Wispr Flow's measured mechanics — a 60 s watch of
the whole field that survives switching apps, the drift gate, the last good
edit judged however the watch ends (sent, Enter, time), 4-word corrections,
no contractions, removed words never offered again; the rules and the
watch's ends unit-tested, the prompt/Add/toggle/bar card, a fix-then-send
and a removed word e2e-tested through staged edits and field reads, the UIA
reads and Windows' spell checker by `--ignored` tests on hidden windows —
not yet tried in real apps),
the Groq usage meter, and the installer + auto-updater (background checks + download
while Yap runs, "Restart to update" from toast/tray/About/Windows notification —
`updates.rs`, 2026-10-05) + portable mode + release CI.
The **Yap bar** (`bar.rs`, 2026-10-05, after Wispr Flow's Flow Bar) is the
always-there surface: a pill above the taskbar on the cursor's monitor,
clickable only where it draws, never focused, hidden over fullscreen apps,
with Dictate / Meeting notes on hover and Yap's notices as cards (call prompts
as Wispr's "Meeting detected" card) — e2e-tested with real-window checks of
its ex-styles and hit-testing; real mouse clicks, multi-monitor moves, DPI
mixes and games still want a hands-on pass.
On top of that, the main window is now a full **ControlPanel** (Home dictation feed
w/ Ctrl+K search, Chat, Notes, Upload, Dictionary, Settings as an always-mounted
modal, app-wide toasts): local audio-**file** transcription (Upload — `media.rs`
Symphonia decode + chunking), an **AI Notepad** (`notes.rs` — folders/actions/
participants/transcripts, an Actions engine, ActionPicker/ActionManager, attendee +
folder management, markdown export, an embedded per-note chat), a **meeting
recorder** (`meeting.rs` — mic + WASAPI loopback → You/Them transcript, cut in
pauses, speaker echo flagged, corrected with the dictionary like a dictation;
hotkey dictations kept out of "You" with a "You dictated here" marker; "Them"
follows Windows' default output device, both sides kept on one clock; a side
quiet for 3 minutes while the other talks gets a warning — all e2e-tested in
`meeting-audio.spec.js`, the real device switch only through a simulation
hook so far; `meeting_summary.rs` — rolling ~10-minute digests
while it records, then **End meeting & summarise** → an **action plan** with a
section per person + Decisions / Open questions / Unassigned, every AI call
within an 8k local context whatever the length; e2e-tested with a two-hour
meeting against a fake AI, and once with real speech through Parakeet
2026-10-05) with **call detection** (`meeting_detect.rs`
— Teams/Zoom/Meet/Slack/Discord/Webex… taking the mic → "Record notes?", the
call ending → "Stop and summarise?"; per-app choice (work apps on, Discord &
co. off, "Don't ask for X" on the prompt), a pop-up or quiet style, a 30 s
fade, and a tray item; e2e-tested via a simulation hook, the
registry signal checked read-only on Nathan's PC, no real call yet),
**guard rails** (`meeting_guard.rs` + `capture.rs`, Wispr Flow's Notetaker
settings: meeting windows hidden from screen capture while recording, a
2-hour maximum length with a "Keep going" warning, an optional auto-stop
when a recorded call ends, and the Win+Alt+M meeting shortcut;
e2e-tested, the capture flag also checked once with a desktop capture of the
overlay, a real screen share not yet; 2026-10-17: a meeting quiet for 10
minutes ends after an "Ending notes in 30 s…" countdown, and "Still in
your meeting?" check-ins after a calendar meeting's end and an hour in —
rules unit-tested, e2e-tested in seconds through the notepad, the bar and
a local iCal feed, not yet in a real meeting) and the **meeting notepad**
(`notepad.rs` + `Notepad.svelte`, Phase 8, 2026-10-05: a window docked to the
right of the screen when a meeting starts, in Wispr Flow's measured layout —
My thoughts synced with Notes, the live transcript in grouped bubbles with
paused/dictated dividers, search and copy, the summary written by Rust in
steps with a Retry, the Ask sheet ("What did I miss?" since you last looked,
questions, 👍 👎 kept locally; one-tap helpers, 2026-10-17: Questions to ask
while recording, a plain-text Follow-up email and Gaps and risks after, also
on a meeting note in Notes — e2e-tested against the fake AI, not yet tried
with a real model), the AI meeting title, "Started by mistake?"
Keep/Discard (on the Yap bar when no window is on screen), the ⋯ menu,
local Share (copy, .md, email), delete, an editable consent message, split
screen with the call now or when joining (a glass preview on hover); every
stop, Yap's own included, writes the action plan in Rust without bringing up
the main window; e2e-tested in `notepad.spec.js`, split screen and its
preview unit-tested only),
and an **AI Chat** surface (`chats.rs` + eager
keyword-RAG over notes, plus a **tool-calling agent loop** in `tools.rs` — nine tools,
including safe partial note edits (search → exact-once replace, `note_text.rs`),
≤20-step loop, gated to cloud or ≥4B local models). An **MCP server** (`mcp.rs`,
`yap.exe mcp`) lets Claude, ChatGPT desktop/Codex, Gemini CLI, Cursor, Claude Code,
VS Code and Windsurf read meetings and notes (never dictations) through the local
API, added in one click from Settings → MCP (`mcp_clients.rs`); tested in-process,
against the real binary and end to end (2026-10-05), not yet with a real AI app.
The **calendar** (`calendar.rs` + `calendar/`, Wispr Flow's, local-first,
2026-10-05): Google Calendar in one click (read-only, PKCE, loopback) or any
private iCal link (Outlook's published calendar, Google's secret address,
iCloud…) under Settings → **Connectors**; a **Meetings** view (today, the next
7 days, Conflict/Maybe, Join + Start, past meeting notes with search, an Ask
bar into Chat); a reminder card before each meeting, on the Yap bar while the
main window isn't focused (**Join & take notes**, switch notes for
back-to-back meetings); every recording during a meeting
takes its name, attendees and invite, and call detection doesn't ask twice.
Recurrence and time zones (incl. Windows zone names) unit-tested; e2e-tested
against a local iCal server and a local fake of Google. Google needs the
OAuth client set up in Google Cloud + the two repository secrets
([`docs/calendar.md`](./docs/calendar.md)); not yet tried against a real
calendar.
Every JSON store now writes
atomically with corrupt-file quarantine. The default (no-feature) build still ships
the stub for fast `cargo check`. **Optional accounts** (`auth.rs` + `cloud/`): email
codes and Google/GitHub/Discord sign-in, sign-out, delete-account — tested end
to end locally (2026-10-01, `wrangler dev` + a mock provider). **Live since
2026-10-02** at `https://auth.contextmirror.com` (contextmirror.com's DNS moved to
Cloudflare; D1 `yap-auth`, WEUR): Google (Cloud project `yap-accounts`, published;
branding verified 2026-10-04, so its chooser says "continue to Yap" with the logo)
and GitHub (OAuth app "Yap") verified with real sign-ins; email codes send via
Resend from `mail.contextmirror.com`. Discord (application "Yap") is live and
verified with a real sign-in (2026-10-04); Microsoft was dropped the same day. New
accounts need a verified email (`databaseHooks` in `cloud/src/auth.ts`). **Sign in
with your phone** (QR code, device flow) verified end to end in the installed
nightly `0.1.1-nightly.107` (2026-10-04), along with the "Where you're signed in"
device list. Known quirk: Better Auth links accounts by exact email, so
`…@gmail.com` and `…@googlemail.com` (same Gmail inbox) are separate accounts.
**Real-app robot tests** (`npm run test:real`, `realtest.rs` + `e2e-real/`,
2026-10-17): paste, smart spacing, snippets, "press enter", Paste last transcript,
the clipboard, terminals (PowerShell, a Claude Code stand-in), chat apps, the mic
path, the dead-mic watchdog, learning words from corrections and real Parakeet
STT, all checked against real windows by Claude. The three gaps its first run
found are fixed (2026-10-17): a WinForms text box gives UI Automation no caret,
so mid-text dictations were fitted as if at the end — Yap now asks the edit
control (`EM_GETSEL`); the fuzzy pass swallowed a short word before a term
("Deploy it to Kubernetes" → "Deploy it Kubernetes"); and Edge/Chrome report an
empty text box's text as its label or U+FFFC (a web chat message kept its full
stop and got a trailing space) and their ranges ran past the box into the page
— reads now stay in the field and an empty one reads as empty.
**Dictation extras** (2026-10-17): a paste into an app running as
administrator is spotted (its integrity level) and left on the clipboard
with "Your text is copied: press Ctrl+V" (`elevated.rs`, `alert.rs`); text
boxes read through their labels (LabeledBy), the text field a focused part
is in, and the legacy read-only bit (`selection.rs`); ✕ / ✓ on the
dictation capsule (off by default, the same day: Settings → Yap bar turns
them on when pointed at, or always; never while push-to-talk is held) and
"Dictation cancelled · Undo" (10 s); Windows'
microphone privacy switches named with Open settings (`mic_privacy.rs`).
Unit-tested, e2e-tested (`bar.spec.js`, `dictation-alerts.spec.js`,
`no-mic.spec.js`) and robot-tested (`dictation-extras.real.js`, the label
case in `browser.real.js`); a real elevated window and a privacy switch
really flipped in Windows are still untried (a UAC prompt; Nathan's
settings).
**Calmer notices** (2026-10-17, after Wispr Flow's rules by behaviour):
one notice policy in Rust and on the page (`notice.rs`,
`ui/noticePolicy.js`): one place per notice (the window while it's focused,
else the bar), one card on the bar and one notice + one answer in a window
at a time, by priority; tips and notices held while dictating; stale ones
dropped; repeats within a cooldown dropped; call-prompt fatigue ("Stop
asking about calls?"); "Show tips"; a dictation's errors on the overlay
only; 19 success toasts that repeated what the screen showed removed.
Unit-tested and e2e-tested (`notices.spec.js` + the updated specs); real
use (a call while the window sits on a second monitor, the bar off with
Windows notifications, Focus Assist) not yet tried.

**Looking things up on the web** (`web.rs`, 2026-10-18, off by default):
Chat and the Voice Assistant search (Exa or Tavily with the person's free
key, else Wikipedia and game wikis with none), read pages on the PC behind
an SSRF guard, cite `[1]` with a Sources row, and never act (apps, note
writes, clipboard) once web content is in a turn; small local models get
a fixed search-then-answer pipeline. Unit-tested (recorded Exa/Tavily/wiki
JSON, the URL guard, the safety rule through the real loop), e2e-tested
against a local fake web (`web-search.spec.js`), and the keyless lookup
checked once for real (Wikipedia, bg3.wiki); not yet tried with a real Exa
or Tavily key. **The AI's own search** (`web/native.rs`, 2026-10-18): cloud
models search with their provider's own search as a sub-call, local ones
with Yap's. Parsers unit-tested on each provider's documented shape (Groq's
recorded), the Groq and Gemini paths and the fallback e2e-tested against
the fake AI; for real, through the eval harness on Nathan's keys: Groq
gpt-oss-20b passed all 4 web scenarios (its search sub-call 3.9–4.8k
tokens, ~2.4 s; a searched answer ~8k tokens over 3 calls, so the free
plan's 8,000 a minute makes the next answer wait), Gemini 3.5 Flash on a
free key has no Google Search grounding ("check your plan and billing")
and fell back to bg3.wiki correctly; Gemini 2.5 Flash/Flash-Lite are
closed to new keys. Anthropic, OpenAI and OpenRouter searches untried
with real keys; Gemini grounding untried on a billed key.

Not yet done: the AI Chat surface has no streaming responses, no semantic-vector
search (keyword-RAG only), no calendar tools and no conversation
search/archive; spoken replies have no cloud voices, read Kokoro's
Japanese and Mandarin voices' text as English, don't check a slow PC's
speed before choosing Kokoro (the research's first-use RTF check), wait for the whole answer
before the first sentence, don't listen again by themselves after an answer,
and have no echo cancellation (a mic that hears the speakers during an
answer is a later job); the AI Notepad has no rich markdown editor (plain textarea)
or folder "add existing note" picker; meetings tell the call's voices apart
only with the beta speaker engine on, and don't name them by themselves
("Speaker 1"… until the user, a 1:1 invite or, later, the call app names
them; voices of two recordings of one meeting aren't matched up; tasks are
attributed from attendees, the speaker map and names said)
and remove speaker echo only from chunks that are entirely the call (see
[`docs/meetings.md`](./docs/meetings.md) "Limits"), and a real call (live
WASAPI loopback + a real LLM's action plan, a headset plugged in mid-call, a
hotkey dictation mid-call) still wants one hands-on pass;
the calendar has no one-click Outlook (Microsoft Graph) and no pre-meeting
brief;
a true streaming model for the partial pass (spike-gated
Stage 2 — see [`ROADMAP.md`](./ROADMAP.md) Phase 1);
multi-phrase dictionary entries; verify-after-paste (UIA `ValuePattern`);
terminals that don't paste on Ctrl+V (mintty/Git Bash, Alacritty, WezTerm,
PuTTY want Shift+Insert or Ctrl+Shift+V) and an IDE's integrated terminal (VS
Code's counts as the editor);
Authenticode signing (blocked on SignPath approval); audio-history export; and
non-Windows (Linux/macOS) polish. See [`ROADMAP.md`](./ROADMAP.md).

---

## Competitive context (why the roadmap looks the way it does)

Yap is in the **local-STT, hotkey, type-anywhere** category. Handy (~25k★, same
Rust+Tauri stack) is the OSS leader but **outputs raw, unpolished text** — it has no
AI cleanup. Paid tools (Wispr Flow, superwhisper, Aqua) win on exactly that cleanup
layer; Wispr Flow's own stack is **Whisper + a fine-tuned Llama** — the same two
stages Yap now runs, except Yap keeps transcription **local/free** and uses a cheap/
fast cleanup model (Groq `openai/gpt-oss-20b`) or a fully-local one.

**Yap's wedge (now real):** local + private + free transcription **plus** instant AI
cleanup, Windows-first. Monetisation stays fair — core free/local forever; any future
paid tier is *convenience* (a hosted cleanup option) or a one-time Pro, never the basic
dictation.

> Keep this file updated as features land — it should always reflect what's actually
> in the code.
