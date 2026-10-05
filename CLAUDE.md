# CLAUDE.md — how Yap works

Yap is a tiny **local voice-dictation tool**: press a global hotkey, speak, press
again — Yap transcribes **locally on the GPU**, optionally runs the text through an
**AI cleanup pass** (filler/punctuation/grammar), and types it into whatever window
is focused. A chime marks start/stop, a correction dictionary fixes mis-heard jargon,
and a floating overlay shows a live waveform while you talk.

This file documents how Yap actually runs today. For *where it's going* and the
competitive strategy, see [`ROADMAP.md`](./ROADMAP.md).

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
> `docs/openwhispr-teardown.md` + `docs/openwhispr-parity.md`.

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
  the screen edge while a meeting records), **overlay**. (The always-on pill
  window was retired 2026-07-09 — the transcribing overlay + tray are the
  only floating surfaces.)
- **Backend:** Rust (`src-tauri/src/`).
- **Audio:** `cpal` (capture) + `rodio` (start/stop chime).
- **STT:** [`transcribe-rs`](https://crates.io/crates/transcribe-rs) — one crate that
  wraps **whisper.cpp** (Vulkan) *and* a family of **ONNX** models (Parakeet, Moonshine,
  SenseVoice, GigaAM, Canary, Cohere) via `ort`/ONNX Runtime (DirectML). Behind the
  `engines` feature flag (see below).
- **AI cleanup:** `reqwest` → any **OpenAI-compatible** chat endpoint (Groq/OpenAI/
  OpenRouter, or local Ollama/LM Studio). `src/llm.rs`.
- **Text injection:** `arboard` (clipboard) + Win32 `SendInput` (paste / Enter).
- **Model download:** `reqwest` streaming + `sha2` verify + `flate2`/`tar` extract.
- **Updates/install:** `tauri-plugin-updater` (GitHub Releases), driven Rust-side by
  `updates.rs` (background checks every ~4 h + background download, install on
  request) + custom NSIS installer.
- **Autostart:** `tauri-plugin-autostart`.
- **Window state:** `tauri-plugin-window-state` persists the main window's size, position, and
  maximized state across launches (overlay, onboarding denylisted; VISIBLE flag excluded
  so the window never un-hides on start-hidden launches).
- **External links:** `tauri-plugin-opener` (opens URLs in the default browser).
- **Dialogs:** `tauri-plugin-dialog` (file open/save dialogs for note export and upload).
- **Accounts (optional):** `cloud/` is the sign-in server — [Better Auth](https://better-auth.com)
  on a Cloudflare Worker + D1 at `auth.contextmirror.com` (see `cloud/README.md`). Yap's side
  is `auth.rs`: system-browser OAuth (Google/GitHub/Discord, RFC 8252 + PKCE) handed back via
  `tauri-plugin-deep-link` (`com.contextmirror.yap://`), a loopback listener or a pasted code,
  plus in-app email codes and "Sign in with your phone" (a locally drawn QR code, RFC 8628
  device authorization); the session token lives in Windows Credential Manager
  (`keyring-core` + `windows-native-keyring-store`). Deep-link/single-instance are held on 2.4.x
  (2.5+ needs tauri 2.12).
- **Data dir:** `%APPDATA%/yap/` (`config.json`, `models/`, `groq_usage.json`,
  `history.json`, `notes.json` — the AI Notepad store, `chats.json` — AI Chat
  conversations, `updates.json` — update announcements + the restart marker).
  Every JSON store writes atomically and quarantines a corrupt file on load
  instead of crashing (`config::atomic_write`/`quarantine_corrupt`, used by all
  six stores).

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
fuzzy near-miss pass) → append-space → auto-submit (Enter) → inject**. Cleanup is
best-effort — any error/timeout falls back to the raw transcript, so dictation
never blocks. The dictionary runs Handy's split (ported from its source):
**Whisper models** get the correct spellings as the decoder's `initial_prompt`
(ASR biasing, threaded through `SttEngine::transcribe` at every call site —
dictation/partials/upload/meeting — plus an OpenWhispr-ported prompt-echo guard
in `stt.rs`), **ONNX models** get **`fuzzy.rs`** — Levenshtein + Soundex 1–3-word
n-gram correction ("jaison"→"JSON", "Chat G P T"→"ChatGPT"; threshold 0.18,
≥3-char terms), gated by `config.dictionary_fuzzy` (default on, "Catch
near-misses" toggle in the Dictionary view) with a **per-entry ≈ opt-out**
(`entry.fuzzy` — exempts corrections whose near-misses are real words, e.g.
`json → JSON` eating the name "Jason"). See `docs/fuzzy-dictionary.md`.

### Key modules (`src-tauri/src/`)
- **`lib.rs`** — app entry / Tauri `setup`. Runs `portable::init()`, registers the
  updater/autostart/single-instance plugins, starts the input hook + pipeline,
  routes `dictation-key-pressed`/`-released` → `Pipeline.on_key()` (so recording works
  before the webview is ready), drives the **overlay** and **tray** off `yap-state`,
  builds the tray (always — it's the only persistent surface), reconciles
  autostart (release builds only: a dev build shares the installed app's `Yap`
  Run entry, so `set_autostart_enabled` leaves the OS setting alone), and
  starts the update scheduler last (`updates::init`).
  `shutdown_cleanup()` (sidecar, bridge, WASAPI mute) runs on `RunEvent::Exit`
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
  `recording_mode` selects toggle vs push-to-talk. `run_stt` (async) does cleanup →
  dictionary → inject (into the captured `target_hwnd`). **Voice Agent wake word**
  (`agent_detect.rs`, OpenWhispr `detectAgentName` port): a dictation addressing the
  agent by name (`agent_name`, default "Yap"; fuzzy-matched) routes the whole
  transcript through the Voice-Agent scope in write mode (`run_agent`, shared with
  the edit hotkey) instead of cleanup — the agent prompt strips the name+command. A `processing` guard blocks
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
  attempt without one says "No microphone found" (toast + overlay) — Upload, models
  and Settings work regardless.
- **`stt.rs`** — `SttEngine` trait + a real `transcribe-rs` engine (`#[cfg(feature =
  "engines")]`) and a stub (default build). Holds the **14-model registry**
  (`ModelDescriptor`: id/filename/url/sha256/is_directory/engine_type), resolves
  legacy/custom ids, and does download → SHA-256 verify → (tar.gz) extract.
  `apply_accelerator_settings` sets whisper→Vulkan(Auto) / ONNX→DirectML.
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
  and `MEETING_DIGEST_PROMPT` (one rolling meeting digest), `enhance_note`
  (the Actions-engine call, base prompt chosen by the caller), `chat` +
  `ChatOptions` (temperature, `max_tokens` — sent for local servers only —
  and timeout), `note_chat` (embedded per-note chat + Chat-surface turns), and
  `post_chat_message` (the tool-loop variant that returns the whole assistant
  message, incl. `tool_calls`, instead of just the text).
- **`local_llm.rs`** — the on-device AI cleanup sidecar: runs **Mozilla llamafile**
  (llama.cpp, single-file OpenAI-compatible server) as a hidden child process on a
  free localhost port, serving **Qwen2.5-1.5B-Instruct** (Q4_K_M GGUF) by default
  — or **any user GGUF** dropped into `<data>/llm/` and picked via `pp_local_model`
  (Settings shows a model picker + "Open models folder"; switching restarts the
  sidecar). Owns install (runtime + model download, SHA-256 verified, per-stage
  progress events), process lifecycle (spawn/health-wait/kill + orphan cleanup at
  startup — only sidecars whose parent Yap is gone, via `procs.rs`, so a dev build
  never kills the installed app's), and `effective_endpoint()` which routes `llm.rs` to the sidecar when
  provider = "ondevice" (falls back to the configured endpoint if it's down).
- **`notes.rs`** — the AI Notepad's data layer (`notes.json`, camelCase). Stores:
  `content` (raw markdown, never overwritten by AI), `enhanced_content` (the
  Enhanced tab), `enhanced_at_hash` (OpenWhispr's `len+first-50` staleness
  marker), `folder` (string, seeded with Personal + Meetings), `participants`
  (attendee names, shown as chips and fed to prompts for attribution),
  `transcript` (meeting-recorder You/Them segments, time-ordered; `echo` marks
  speaker bleed), `digests` (rolling meeting digests, see `meeting_summary.rs`),
  `note_type` ("personal" | "meeting"), `source` ("manual" | "upload" |
  "meeting"), and `title_auto` (the title is one Yap made up — "Teams call ·
  5 Oct, 14:30" — so the AI meeting title may replace it; cleared for good by
  a title edit, so a typed title is never overwritten: `set_ai_title` checks
  under the store lock). `note_update` takes an `origin` (the saving window's
  label) and emits `yap-note-changed` with the editable fields, which keeps the
  Notes view and the meeting notepad in sync both ways (each sends only the
  fields it edited).
  **Folders** are user-creatable (backend `notes_folder_create` command) with
  counts shown in the sidebar; notes filter by active folder. **Actions** (named
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
  4 s** (`media::quietest_frame`, not mid-word), transcribes them on the shared
  warm engine (`pipeline::EngineSlot`, taken per chunk so dictation still
  works; silence-gated), works off a backlog in ≤30 s chunks back to back,
  caps each source's buffer at 20 min (then drops audio + `yap-meeting-warning`),
  dates each segment by its speech onset, flags **speaker echo** (a "you"
  chunk whose words repeat "them" in order AND whose 20 ms loudness envelope
  follows the call 0–600 ms later — the timing test keeps a headphone user's
  own reply; flagged segments are kept, hidden, left out of summaries), and
  `ingest`s segments: persist to `notes.transcript`, emit `yap-meeting-segment`,
  kick the rolling digests. Commands: `meeting_start`/`meeting_stop`/
  `meeting_state` (signatures unchanged). Test mode opens no audio device (WAVs
  from `YAP_E2E_MEETING_AUDIO`, or silence; `e2e::e2e_meeting_feed`).
- **`meeting_summary.rs`** — meeting summaries that keep up with long meetings
  (map-reduce, the map done while recording). Every ~10 min of new talk
  (2,000 est. tokens, or 10 min with ≥ 250) a background **digest** (key
  points, decisions, action items `Owner: task (due: …)`, open questions;
  `llm::MEETING_DIGEST_PROMPT` + a one-shot; ≤ 2,600 tokens of transcript + the
  meeting so far ≤ 400) is written with the Note Formatting endpoint and
  stored on the note (`yap-meeting-digest`); owners must be You/Everyone/an
  attendee/a name said in that part (else Unassigned) and deadlines must have
  been said (`check_digest`). On a **local** endpoint digests wait for
  dictations to finish and are dropped mid-call when one starts
  (`unless_busy` — closing the connection cancels it in llama.cpp), and
  replies are capped. `prepare_final` (one digest turn at a time,
  `DIGEST_TURN`) digests any backlog with `yap-meeting-summary-progress`;
  `compose_meeting_input` keeps the final input ≤ 4,500 tokens whatever the
  length (raw transcript when ≤ 3,500); `postcheck_action_plan` moves invented
  owners' tasks to Unassigned, strips made-up deadlines and restores dropped
  digest tasks; `ask_context` bounds a long meeting for the Ask bar and the
  chat `get_note` tool. The notepad's helpers live here too: `catch_up_input`
  ("What did I miss?": what was said since segment `since`, raw when ≤ 2,400
  tokens, else the digests covering it + the latest ≤ 1,400; context ≤ ~700),
  `meeting_ask_messages` (a follow-up question, ≤ ~3.3k), `title_messages` +
  `clean_title` (the AI meeting title: the start of the meeting, one-shot,
  ≤ 60 chars), and `chat_beside_dictation` (on a local endpoint: wait for
  dictation to finish, drop the call when one starts and ask again — never
  slows a dictation). See [`docs/meetings.md`](./docs/meetings.md).
- **`meeting_end.rs`** — the end of a meeting recording, wherever it was
  stopped from (notepad Stop, Notes view "End meeting & summarise", the Yap
  bar, call detection, an automatic stop). It follows the recorder through
  `yap-meeting-state` (Rust listener; `meeting.rs` is untouched): a start
  opens the notepad (`notepad::on_meeting_started`); a stop is the **end** of
  the meeting unless it came through `meeting_pause` (the Notes view's Pause).
  The end: fewer than 20 words of speech (`MISTAKE_WORDS`) and nothing typed →
  `yap-meeting-ended {noteId, mistake, words, surface}` and no summary (the
  window Rust picks — where the stop came from (`meeting_end`'s `origin`) if
  on screen, else the notepad, else the main window — shows **"Started by
  mistake?"** Keep / Discard; `meeting_discard` deletes the note, emits
  `yap-note-deleted`, hides the notepad); otherwise the AI title (if due) and
  the **action plan job** (`start_summary`: one per note, `yap-meeting-summary
  {noteId, run, state: running|done|error|needsAi|nothing, step, steps, error}`,
  "Step 2 of 3"; `meeting_summary_status`; `meeting_summarise` runs it again —
  Generate summary, Retry, the Notes view's Action plan). Commands
  `meeting_end(origin)`, `meeting_pause`, `meeting_summarise`,
  `meeting_summary_status`, `meeting_discard`; Rust callers use `end()` /
  `pause()`.
- **`meeting_assist.rs`** — the notepad's AI helpers, on the meeting's model
  (Note Formatting scope, else cleanup), bounded for an 8k local model, via
  `chat_beside_dictation`: **"What did I miss?"** (`meeting_catch_up(noteId,
  since, question?)` → `{answer, nothingNew, upto}`; with nothing new since
  `since` it answers "Nothing new since you last looked." without a model;
  `question` = a follow-up from the same mini chat) and the **AI meeting
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
  the width clamped to 400–600 px at 100% (scaled) and ≤ half — on the monitor
  with the call's window, else the cursor's; already on screen, it stays put.
  **Split the screen** (`meeting_split_screen`, default off): during a
  detected call, `pick_call_window` (unit-tested) finds the call app's main
  window — visible, top-level, not Yap's process, not minimised/tool/owned/
  cloaked, ≥ 200×150, titled; a browser window whose title shows the meeting
  first, else the largest — from `meeting_detect::latest_call()` (its exes, or
  the browser a meeting tab is in); a maximised window is restored first, then
  moved left of the notepad (`SetWindowPos`, async, invisible borders added
  back via `DWMWA_EXTENDED_FRAME_BOUNDS`). Never in test mode. `open(app,
  noteId)` (the Notes view's "Notepad", the Yap bar) docks if hidden and
  focuses. Commands `notepad_open`, `notepad_state`; `yap-notepad-visible`
  tells the page when Rust shows/hides it.
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
  (`config.meeting_detect_style`): **"popup"** (default) — a sticky in-app
  toast when the main window is visible, a silent Windows notification
  (`win_toast.rs`) when it isn't focused; answering either withdraws both,
  focusing the window moves a pending prompt in-app (`on_main_window_focused`),
  and an in-app start prompt left alone **fades after 30 s as "Not now"**
  (OpenWhispr's auto-dismiss; `fadeMs` in the prompt view, the toast's
  `onExpire`) — or **"quiet"** — no in-app toast, the notification goes
  silently into the notification centre (`win_toast::post_quietly` →
  `SetSuppressPopup`) and stays out of the window unless its body is clicked.
  Both styles: start prompts carry a third answer, **"Don't ask for Teams"**
  (`never`: saves the override, withdraws the prompt everywhere, emits
  `yap-meeting-detect-choice {app, asks, confirm}` so Settings adopts it and,
  window on screen, a "Won't ask about Teams calls" toast links to Settings →
  General → Meetings); the end prompt follows the style. Prompts wait for a
  dictation to finish (+2.5 s); "Not now" snoozes that app for 5 min
  (OpenWhispr's cooldown). **Tray**: while a call of an asked-about app is live
  and nothing records, `tray_item()` puts "Record this Teams call" at the top
  of the idle menu (`meeting_record:<call id>` → `on_tray_record`, same as
  Record notes); `sync_tray` calls `tray::refresh` when it changes. **Record
  notes** → `notes::create` ("Teams call · 5 Oct, 14:30", folder Meetings,
  source "meeting", `set_title_auto` so the AI title may replace it) +
  `notes::mark_meeting` + the `meeting_start` command (which opens the
  notepad); opens the note if the window is visible (else a "Taking notes…"
  Windows notification); a failed start deletes the note and says why.
  **Call ended** while recording it → "Stop and summarise?" — never an
  auto-stop (OpenWhispr doesn't, and the mic can't tell an ended call from a
  rejoin, breakout room or phone hand-off): opens the note and NotesView ends
  it there (`meeting_end`), so the Action Plan runs (in Rust, `meeting_end.rs`;
  `yap-meeting-open-note {noteId, stop}`; Rust stops it itself after 8 s if
  the page didn't — any stop but a pause writes the plan). For the notepad's
  split screen: `latest_call()` (the live call's exes) and `title_shows`.
  Snapshot
  `meeting_detect_status` + `yap-meeting-detect` (`{enabled, style, apps,
  calls, prompt}`); answers `meeting_detect_respond(promptId,
  record|dismiss|never|stop|keep)`; debug-only `meeting_detect_simulate(appId,
  active, fadeMs?)` (no debounce; `fadeMs` shortens that call's fade) for the
  e2e suite. Gated by `config.meeting_detection` (default **on**, as
  OpenWhispr's `notifyMeetingDetection`; `sync()` on every config save re-reads
  the style and per-app choices too). Lock rule: never touch windows/WinRT
  while holding its state lock (window getters wait on the main thread). See
  [`docs/meetings.md`](./docs/meetings.md).
- **`media.rs`** — audio-file decode front-end for Upload: pure-Rust **Symphonia**
  (mp3/wav/m4a/aac/flac/ogg-vorbis; no opus yet) → downmix mono → 16 kHz
  (`pipeline::resample_linear`), plus `chunk_ranges` (~60 s windows cut at the
  quietest sample of each window's last 5 s). Consumed by
  `pipeline::run_file_transcription` (progress events, cancel flag, `processing`
  guard, history record) via the `transcribe_file` command.
- **`chats.rs`** — the AI Chat surface's conversation store (`chats.json`): a
  `Conversation` (title, messages, timestamps) with `list`/`get`/`create`/
  `append`/`delete`; `chat_send` (commands.rs) creates a conversation on the
  first message using a first-50-chars title rule.
- **`tools.rs`** — the AI Chat tool-calling agent loop: six tools ported
  near-verbatim from OpenWhispr's `services/tools/*` (`search_notes`,
  `get_note`, `create_note`, `update_note`, `list_folders`,
  `copy_to_clipboard` — executed locally over `notes.rs`/`arboard`), plus their
  `TOOL_INSTRUCTIONS` system-prompt lines. `run_tool_loop` drives the OpenAI
  tool-call protocol via `llm::post_chat_message` for up to `MAX_TOOL_STEPS`
  (20) turns. Gated by `supports_tools` — cloud providers always qualify,
  local models need ≥4B params (`LOCAL_TOOL_MIN_PARAMS_B`) — so smaller local
  models fall back to plain keyword-RAG chat instead.
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
  it's still ours. See `docs/local-api.md`.
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
  `usage.rs`). Powers `get_history`/`clear_history`/`get_stats`.
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
  installing + version/notes/progress/error/deferred/…) feeds every surface.
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
  "update" tag (always with a banner), meeting_detect.rs under "call"; each
  keeps its live toast so the buttons work from the notification center.
  `allowed()` = not portable, not a test run.
- **`config.rs`** — `YapConfig` (hotkey, model_size, use_gpu, input_device, sound +
  volume, output_device, mute_while_recording, recording_mode,
  overlay_position, dictionary, append_trailing_space, auto_submit(+key),
  restore_clipboard, show_tray_icon, autostart, model_unload_timeout, selected_language,
  translate_to_english, the `pp*` AI-cleanup fields incl. `pp_preset` (Default/Email/
  Notes/Slack/Code/Custom), the editable `pp_prompt` body, `pp_api_keys` (per-provider
  key store — the UI swaps the active `pp_api_key` from it on provider switch), `cleanup_profiles` (each
  with an optional per-profile LLM override: provider/base_url/model/api_key — empty
  provider = inherit global) + `app_routes` smart routing, streaming_partials,
  history_enabled, update_checks_enabled, dictionary_fuzzy, the meeting
  notepad: meeting_open_notepad (default on) + meeting_split_screen (default
  off), call detection:
  meeting_detection + meeting_detect_style ("popup"|"quiet") +
  meeting_detect_apps (app id → bool overrides of `meeting_detect::APPS`'
  defaults)). JSON
  load/save + `apply_dictionary` + `dictionary_prompt` (the Whisper
  `initial_prompt` vocabulary) + `resolve_cleanup` (per-app plan: body + endpoint).
  `data_dir()` is portable-aware. `load()` also migrates saved Groq picks (cleanup,
  LLM scopes, profiles) off models Groq retired in 2026 (`RETIRED_GROQ_MODELS` —
  default is now `openai/gpt-oss-20b`); extend that table when Groq retires more.
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
  n-gram choice (deliberate fix over Handy's word-swallowing longest-first greedy).
  Runs after `apply_dictionary` for ONNX models (`dictionary_fuzzy`, default on).
  Also `is_prompt_echo` (OpenWhispr `dictionaryEchoFilter` port) used by `stt.rs`
  against Whisper hallucinating the dictionary prompt on silence.
- **`tray.rs`** — state-aware tray icon (runtime-generated coloured dot) + right-click
  menu (model submenu w/ checkmark, Cancel while recording, Settings/Quit, Check for
  updates → Settings → About + a manual check); left-click opens Settings. A ready
  update adds **"Restart to update to X"** under the version line (replacing
  "Check for updates…"; portable: "Get Yap X on GitHub…"), a green dot on the icon
  and "· Update X ready" in the tooltip (`tray::refresh`, driven by updates.rs).
  A live call Yap asks about, while nothing records, puts **"Record this Teams
  call"** above it (`meeting_detect::tray_item`; the click runs
  `on_tray_record` on a worker thread). Both items are in the idle menu's cache
  key (`idle_menu_key`), so the menu rebuilds when they change; all tray work
  runs on the main thread.
- **`overlay.rs`** — shows/positions the bottom (or top) center "transcribing" overlay
  window on `yap-state`. **Screen-aware**: positions on the monitor holding the
  mouse cursor (Win32 `GetCursorPos` vs Tauri monitor rects, both physical px;
  Handy's `get_monitor_with_cursor` pattern), primary-monitor fallback.
- **`input_hook.rs`** — low-level Windows keyboard + mouse hooks; specs `kb:VKEY`,
  `kb:ctrl+shift+VKEY` (modifier combo), `kb:165` (single right-side modifier, e.g.
  RightAlt — never suppressed, it's AltGr), `mods:ctrl+alt` (modifier-only chord) /
  `mouse:ID` — combo semantics ported from OpenWhispr's `windows-key-listener.c`
  (press = key down w/ required modifiers held, release = key up OR required
  modifier up; chords fire on completion; suppressed keys are excluded from the
  GetAsyncKeyState self-heal — the hook eats them before the key-state table
  updates). Emits press AND release (via an emit-forwarder thread — the hook
  callback never blocks — plus a 30 s re-hook self-heal). The capture UI is
  `ui/HotkeyInput.svelte` + shared `lib/hotkeys.js` (parse/format/match — also
  drives the in-window fallbacks). ⚠ **Known Windows
  gotcha:** when one of Yap's OWN WebView2 windows has focus, the LL hook never
  receives the hotkey (WebView2/Chromium front-runs the hook chain on focus) —
  so the Settings + onboarding pages catch the hotkey **in-page** (keydown
  fallback → `toggle_recording`). Any new Yap window with focusable UI needs the
  same fallback.
- **`text_injector.rs`** — clipboard paste (+ optional clipboard restore) and
  `press_submit` (Enter / Ctrl+Enter / Shift+Enter) via `SendInput`. Captures the
  dictation **target window** at record-start (`current_foreground`, skipping Yap's
  own windows) and **re-focuses** it before pasting (`focus_window`, via the
  `AttachThreadInput` workaround) so focus changes mid-transcription don't misfire.
  The clipboard snapshot/restore preserves **text or image** so a paste doesn't wipe
  a copied image; `selection_via_copy` (edit-mode fallback) polls for the Ctrl+C
  result instead of a fixed sleep. Falls back to direct Unicode typing
  (`type_unicode`) if the clipboard is unavailable. (UI-Automation content
  verification is a deferred follow-up.)
- **`sound.rs`** — start/stop chimes (volume + output-device aware).
- **`mute.rs`** — mute-while-recording: mutes the default render endpoint via
  WASAPI/COM (`IMMDeviceEnumerator` → `IAudioEndpointVolume`) while recording and
  restores it after — only unmuting what Yap itself muted.
- **`portable.rs`** — portable-mode detection (a `portable` marker next to the exe
  redirects data to `<exe>/Data`).
- **`e2e.rs`** — **test mode** for the end-to-end UI suite (debug builds started
  with `YAP_E2E=1`; compiled out of release builds): no global input hook, no
  paste/Enter/Ctrl+C into other apps, no `set_focus` (tao's fallback presses Alt
  in the focused app), no Windows notifications, no orphan-sidecar sweep, no
  window-state plugin, no call-detection registry reads (the debug-only
  `meeting_detect_simulate` stands in), no audio device for meeting recordings
  (`spawn_meeting_audio`: `you.wav`/`them.wav` from `YAP_E2E_MEETING_AUDIO` at
  `YAP_E2E_MEETING_SPEED`× real time, or silence; a configured mic that isn't
  plugged in still fails, found by listing devices), the debug-only
  `e2e_meeting_feed { segments }` command (hours of transcript in seconds),
  and it quits when its stdin closes. See
  [`docs/e2e-tests.md`](./docs/e2e-tests.md).
- **Logging** (`lib.rs init_logging`) — tracing → stdout + a daily-rolling
  `<data>/logs/yap.log.*` file at `info`; panics are hooked into the log.
  **Debug mode** (Settings → Advanced → Debug Logging, OpenWhispr Developer-
  section port): `config.debug_logging` raises `yap_lib` to `debug` live via a
  `reload::Layer` handle (`set_debug_logging`, applied on config save; RUST_LOG
  env always wins). `log_info`/`open_logs_folder` commands back the UI (current
  log file + copy path + open folder). ⚠ transcripts DO appear in logs at
  `info` — the UI warns users to skim before sharing.
- **`commands.rs`** — Tauri commands: recording (`toggle_recording`, `cancel_recording`),
  config (`get_config`/`save_config`), models (`installed_models`, `download_model`,
  `download_model_size`, `set_active_model`, `delete_model`, `model_language_info`),
  devices (`list_audio_devices`, `list_output_devices`, `set_input_device` — live
  stream swap, `set_mic_test` — idle level meter), windows (`open_settings`,
  `open_onboarding`, `close_onboarding`),
  `configure_hotkey`, `set_autostart`, `is_portable`, `test_post_process`,
  `get_groq_usage`, history (`get_history`, `clear_history`, `get_stats`),
  plus the notes/actions/folders CRUD + `note_enhance`/`note_ask`/`note_export`,
  chats CRUD + `chat_send` (RAG + tool loop), `meeting_start`/`meeting_stop`/
  `meeting_state`, `transcribe_file`/`cancel_file_transcription`/
  `audio_file_info`, `log_info`/`open_logs_folder`, `delete_history_entry`.

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
  `--yap-font-display` (hero headlines, stat numerals, Settings page titles). **Settings attention badge** (`lib/attention.svelte.js`):
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
  (**Home / Chat / Notes / Upload / Dictionary / Integrations**) + **Settings as a modal
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
  `onExpire` (timer ran out) callbacks, copyable mono error
  boxes, progress hairlines (3.5 s / 6 s durations; `duration <= 0` sticky),
  plus a determinate `progress` bar, a `busy` chip spinner and an `expand`
  "What's new" toggle that unfolds markdown in the card; `updateToast(id,
  patch)` changes a live toast in place; mounted in ControlPanel and wired to
  action runs, meeting start/stop, uploads, clipboard copies, debug-mode
  toggles, backend `yap-error` events, and the **update toast**
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
  `noteRequest` to open, and stop if asked), and **"Started by mistake?"**
  (`meetingSummary.svelte.js` — the shared store of the Rust action-plan jobs,
  `summaries.byNote` / `.progress`, also used by NotesView and the notepad;
  `askStartedByMistake` = a sticky "Only a few words were captured. Keep this
  meeting or discard it." with Discard / Keep, shown by the window
  `yap-meeting-ended`'s `surface` names). **`HomeView.svelte`** = the Wispr-style
  Home: time-of-day greeting with the hotkey as **amber keycaps**, a dark
  **rotating hero card** (4 tips — voice edit / AI cleanup / meeting notes /
  per-app profiles — picked by day, dot nav, CTAs open the right Settings
  section or view via `onnavigate`), the dictation feed (day-grouped flat rows
  w/ hover-revealed meta + copy/delete via `delete_history_entry`, live refresh
  on `yap-transcript`, icon-expand search on Ctrl+K), and a right-rail stats
  card with serif display numerals. **`InsightsView.svelte`** = the stats
  dashboard promoted out of Settings→History (serif hero number, stat grid,
  30-day amber heatmap, top-apps-by-words bars from `get_history`); Settings →
  History keeps only the enable toggle + recent list + clear. **`DictionaryView.svelte`** =
  the correction dictionary (promoted out of Settings → Advanced; syncs with
  Settings' cfg copy via `yap-dictionary-changed`/`-external` events).
  **`UploadView.svelte`** = local audio-**file** transcription (drop/browse →
  Symphonia decode → chunked transcription on the warm engine with progress +
  cancel — see `media.rs`); **`NotesView.svelte`** = the AI Notepad (OpenWhispr
  sidebar port: **New note / Search notes / Actions** rows; **FOLDERS with
  counts + NOTES list**; meta chip row shows date + attendees popover
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
  in `lib/markdown.js`) with **Copy markdown / Copy text** (`markdownToText`:
  ☐ tasks, plain headings); embedded per-note **"Ask anything…" bar** = Chat
  scope grounded in the note, with mic button for in-box dictation; live
  **You/Them meeting transcript bubbles** (follow the newest line; echo lines
  hidden behind a "Show N lines…" toggle), an **AI notes so far** view of the
  rolling digests ("AI notes up to 40:12"), an **Action plan** button on
  finished meetings, a "Writing your action plan… / Catching up on the meeting
  — part 3 of 12" line, and, without an AI model, a "Your meeting is saved…"
  card linking to Settings → Language Models); the Home feed has **Ctrl+K
  search**. **`ChatView.svelte`** = the
  AI Chat surface (OpenWhispr `chat/ChatView.tsx` port): conversation sidebar
  (Today/Yesterday/Previous 7 Days/Older grouping, Ctrl+N, hover-delete) +
  thread; `chat_send` answers via the Chat scope with **eager keyword-RAG**
  over the notes library (top-5 `<note>` snippets; `chats.rs` persists
  conversations to `chats.json`) **plus the tool-calling agent loop**
  (`tools.rs`: search_notes/get_note/create_note/update_note/list_folders/
  copy_to_clipboard executed locally, ≤20-step loop over the OpenAI tool
  protocol, gated to cloud or ≥4B local models — smaller models fall back to
  plain RAG chat; tool-activity chips render in the thread). No streaming or
  semantic vectors yet (ROADMAP step 3). **`IntegrationsView.svelte`** = the
  Integrations surface (OpenWhispr `IntegrationsView.tsx`, local-first cut):
  Local API card (enable toggle + live status/port via `bridge_status`,
  discovery-file path + curl example with copy), a Coding-agents card whose
  "Copy API guide" button copies a paste-into-your-agent endpoint cheat-sheet,
  and an endpoint reference table. (OpenWhispr's Google-Calendar OAuth /
  cloud API-keys / hosted-MCP cards need their paid cloud and are not ported.)
- **`lib/Overlay.svelte`** — the click-through bottom/top overlay, Yap's only
  floating dictation surface (the pill was retired 2026-07-09): a **light**
  capsule matching the app's identity (white `--yap-s2` surface + warm border +
  ink text; a little Yap card floating on screen), with a **burnt-orange**
  (`--yap-primary`) scrolling amplitude waveform while recording, "Transcribing…"
  while processing, and an error state. Red pulsing dot + moving waveform carry
  visibility on any background, so **no drop shadow** (dodges the boxy-shadow
  artifact on the tightly-fitted transparent WebView2 window).
- **`lib/Notepad.svelte`** — the **meeting notepad** (window `notepad`,
  `notepad.rs`; Wispr Flow Notetaker's notepad, warm-light): a custom title
  bar (brand, drag region, Open in Yap / minimise / close — close hides it,
  the recording goes on); the meeting title in EB Garamond (editable; a
  made-up one muted until the AI or the person names it) + date and
  "● Recording 12:34"; underlined tabs **My thoughts** (the note's `content`,
  autosaved, synced with NotesView both ways via `yap-note-changed`/`origin`),
  **Transcript** (elapsed row, a dismissible tip that lines arrive every ~15 s
  and the transcript is tidied when you stop — after the meeting each speaker
  turn becomes one paragraph —, a "Yap is listening" empty state, You (amber)
  / Them (slate) turns following the newest line, echo hidden behind "Show N
  lines…") and **Summary** (the Rust job: "• Turning 12 minutes of talk into
  an action plan… · Step 2 of 3", "Catching up on the meeting: part 3 of
  12…", the rendered plan + Copy markdown/text, "The summary didn't come
  through" + Retry, the AI-setup card, the digests so far while recording).
  Footer: recording → "Always get consent when transcribing others." + **■
  Stop** (`meeting_end` origin notepad) + **What did I miss?**; stopped →
  **Resume** + **Generate summary** (no summary yet, or it failed). "What did
  I miss?" opens an inline catch-up chat (`meeting_catch_up` with `since` =
  the segments seen: all of them whenever the Transcript tab is on screen —
  window shown per `yap-notepad-visible`/`isVisible` — and scrolled to its
  newest line, or covered by the last answer; follow-up questions in the same
  box; no AI → a link to Language Models). Toasts sit above the footer
  (`--yap-toast-bottom`). It has the **in-page hotkey fallback** (dictation,
  and the meeting shortcut when one is set), config re-read on focus.
- **`lib/Settings.svelte`** — the settings surface, now rendered **inside the
  ControlPanel's modal** (`embedded` prop; ✕ closes). Grouped sidebar (App / AI models / Data / System):
  **General** (hotkey, recording mode, mic, sound+volume, mute, recording-overlay
  group: live-preview toggle + overlay position — the overlay itself is always on,
  it's the hot-mic indicator; **Meetings** group (`#settings-meetings`, the
  target of `yap-settings-goto` "general#meetings"): "Detect calls and offer to
  take notes" (`meetingDetection`), then — disabled while it's off — "How Yap
  asks" (`ui/Segmented` Pop-up / Quietly → `meetingDetectStyle`, a one-line
  explanation of the choice) and "Ask about calls in" (a two-column list of
  switches, one per call app from the `meetingDetect.apps` snapshot, showing
  the effective choice; a flip writes `meetingDetectApps[app]`; a
  `yap-meeting-detect-choice` from Rust is adopted into Settings' config copy
  so auto-save can't undo it), the meeting notepad's two rows, "Open the
  notepad when a meeting starts" (`meetingOpenNotepad`) and "Split the screen
  when joining" (`meetingSplitScreen`, greyed out while the first is off), +
  an always-visible consent line, "Recording a
  call? Let people know you're taking notes."), **Speech-to-Text** (`ModelManager` + GPU +
  language/translate), **Language Models** (OpenWhispr-style: enable toggle → mode
  selector Cloud Providers/Local/Self-Hosted → provider pill tabs (Groq/Anthropic/
  OpenAI/OpenRouter/Custom, brand icons) → API Key (masked + "Get your API key"
  link) → Select Model registry rows (`ppModels.js` + `ui/SelectList.svelte`);
  UI-only `ppMode`/`cloudProvider` state resolves to the unchanged `ppProvider`
  contract. Below it, **Prompt Studio** (`PromptStudio.svelte`, OpenWhispr port):
  View (full effective prompt via `get_base_prompt`) / Customize (preset +
  body + Save/Reset) / Test tabs. Plus usage meter + profiles w/ per-profile
  model override + per-app rules), **History** (stats
  dashboard + recent list + enable/clear), **Advanced** (output toggles, system,
  dictionary), **About** (version + an **Updates** card on the shared update
  store: "Last checked …" / Check for updates, Downloading… %, "Yap X is ready to
  install" + Restart to update, release notes + release-page link, the
  "Check for updates automatically" toggle), and **Account** (bottom of the
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
  size, Download/Active/delete). `ModelCard.svelte` (big cards) remains only in
  Onboarding.
- **`lib/Onboarding.svelte`** — first-run **guided setup** (5 steps): model picker →
  mic check (live level meter via `set_mic_test` idle-amp mode + live device switch)
  → one-click **local AI cleanup** install → tray pointer → "try it here" live
  dictation test with a change-shortcut recorder.
- **`lib/ui/`** — primitives: Toggle, Select, Slider, Group, Row, Button, Input, Textarea.

### Window config (`src-tauri/tauri.conf.json`)
- **settings**: 1200×800 (min 860×600), titled "Yap" (it hosts the ControlPanel — see above),
  **undecorated** (custom in-page title bar w/ drag region + caption buttons — see the
  "Custom window chrome" bullet above), hidden, hide-on-close. Size/position/maximized
  persisted by `tauri-plugin-window-state`
  across launches; overlay, onboarding, notepad are excluded from persistence; the VISIBLE flag
  is excluded so the window never un-hides on start-hidden launches (see lib.rs window-state
  plugin setup).
- **onboarding**: 620×720 (min 520×560), hidden, hide-on-close.
- **notepad**: the meeting notepad, 560×900 (min 360×480), undecorated (its own
  title bar), hidden, **unfocused** (opening it beside a call never takes the
  foreground; only `notepad::open`, a click in Yap, focuses it), hide-on-close
  (`notepad::init`), gets the DWM-cloaked init like settings/onboarding.
  Placed by `notepad.rs` (docked right, full height), never by the
  window-state plugin.
- settings + onboarding + notepad are **created unfocused** (`focus: false`): otherwise wry
  `MoveFocus`es each new webview and the hidden windows grab the foreground at launch,
  so typing (and a dictation's paste, which never targets Yap's own windows) can land
  in an invisible window. A
  side effect is that their first `show()` doesn't activate, so every open path must
  `show()` + `set_focus()` (`commands::show_settings`/`show_onboarding` do).
- **overlay**: 330×48, transparent, click-through, always-on-top, not focused, hidden
  until recording/processing.
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
the no-mic path / the update toast / meetings and the meeting notepad (its own
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

### The dev → nightly → stable workflow (agreed with Nathan, 2026-10-05)
**Dev first, nightly for batches.** Every change goes through these steps in order:
1. **Claude builds it and tests it.** `npm run build` + `cargo clippy --all-targets
   --locked -- -D warnings` + `cargo test`, then the e2e robot suite (`npm run
   test:app`: drives the real app in ~20 s and leaves screenshots to look at; add or
   extend a spec for anything new). Push to main: CI and the E2E workflow re-check
   every push.
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
- **Double-click "Yap - Dev"** on the desktop. It opens a console window: leave it
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
(`CredDeleteW` on the `yap-account.com.yap.dictation` credential); never on `/UPDATE`
runs (the updater always passes it), and unticked keeps both — a reinstall stays signed in.

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
- Groq usage: `%APPDATA%/yap/groq_usage.json`.
- History: `%APPDATA%/yap/history.json` (local-only; cleared from Settings → History).
- Notes: `%APPDATA%/yap/notes.json` — the AI Notepad store (folders, actions,
  participants, meeting transcripts).
- Chats: `%APPDATA%/yap/chats.json` — AI Chat conversations (`chats.rs`).
- Updates: `%APPDATA%/yap/updates.json` — which update was announced (and when),
  the last check time, and the restart marker (`updates.rs`).
- All of the above (plus config) write atomically and quarantine a corrupt file
  on load rather than crashing (`config::atomic_write`/`quarantine_corrupt`).
- Local API bridge discovery: `~/.yap/cli-bridge.json` (fixed path, NOT the
  data dir; written while the app runs, deleted on exit — see `bridge.rs` +
  `docs/local-api.md`).
- Account session: a Windows Credential Manager generic credential
  (`yap-account.com.yap.dictation`, Local persistence), not a file — see `auth.rs`.
  Uninstalling with "Delete the application data" removes it too (updates never do).
- Notable defaults: hotkey `kb:120` (F9, rebindable), **default model
  `parakeet-tdt-0.6b-v3`** (fast/accurate, ONNX→DirectML), `use_gpu = true`,
  recording mode `toggle`, overlay always shown while recording/transcribing (no
  off switch — it's the hot-mic indicator), live transcription preview **on**
  (`streaming_partials`), AI cleanup **off**, call detection **on**
  (`meeting_detection` — it only asks; nothing records without a click) as
  **pop-ups** (`meeting_detect_style`), for work apps only (Discord, WhatsApp,
  Signal and Telegram start switched off; `meeting_detect_apps` holds changes),
  the meeting notepad opening when a meeting starts (`meeting_open_notepad`)
  but **not** splitting the screen (`meeting_split_screen` off: it moves
  another app's window).

---

## Current status

**Transcription is REAL and GPU-accelerated** (no longer a stub in `engines`
builds), and the dictation pipeline around it is complete: multi-engine STT
(Whisper/Vulkan + ONNX/DirectML), the 14-model registry + manager, recording modes,
language/translate, **AI cleanup** (BYO key or local sidecar) with per-app routing +
named profiles + per-profile model choice, **edit/rewrite mode** + the **Voice Agent
wake word**, combo hotkeys, the audio pre-roll (anti first-word clipping), **live
streaming partials** (sliding-window, on by default, word-paced overlay reveal —
validated live 2026-07-10), transcription history + stats, cleanup presets, real WASAPI mute,
the Groq usage meter, and the installer + auto-updater (background checks + download
while Yap runs, "Restart to update" from toast/tray/About/Windows notification —
`updates.rs`, 2026-10-05) + portable mode + release CI.
On top of that, the main window is now a full **ControlPanel** (Home dictation feed
w/ Ctrl+K search, Chat, Notes, Upload, Dictionary, Settings as an always-mounted
modal, app-wide toasts): local audio-**file** transcription (Upload — `media.rs`
Symphonia decode + chunking), an **AI Notepad** (`notes.rs` — folders/actions/
participants/transcripts, an Actions engine, ActionPicker/ActionManager, attendee +
folder management, markdown export, an embedded per-note chat), a **meeting
recorder** (`meeting.rs` — mic + WASAPI loopback → You/Them transcript, cut in
pauses, speaker echo flagged; `meeting_summary.rs` — rolling ~10-minute digests
while it records, then **End meeting & summarise** → an **action plan** with a
section per person + Decisions / Open questions / Unassigned, every AI call
within an 8k local context whatever the length; e2e-tested with a two-hour
meeting against a fake AI, and once with real speech through Parakeet
2026-10-05) with **call detection** (`meeting_detect.rs`
— Teams/Zoom/Meet/Slack/Discord/Webex… taking the mic → "Record notes?", the
call ending → "Stop and summarise?"; per-app choice (work apps on, Discord &
co. off, "Don't ask for X" on the prompt), a pop-up or quiet style, a 30 s
fade, and a tray item; e2e-tested via a simulation hook, the
registry signal checked read-only on Nathan's PC, no real call yet) and the
**meeting notepad** (`notepad.rs` + `Notepad.svelte`, Phase 8, 2026-10-05: a
window docked to the right of the screen when a meeting starts — My thoughts
synced with Notes, the live transcript, the summary written by Rust in steps
with a Retry, "What did I miss?" since you last looked, the AI meeting title,
"Started by mistake?" Keep/Discard, optional split screen with the call;
e2e-tested in `notepad.spec.js`, split screen unit-tested only), and an **AI Chat** surface (`chats.rs` + eager
keyword-RAG over notes, plus a **tool-calling agent loop** in `tools.rs` — six tools,
≤20-step loop, gated to cloud or ≥4B local models). Every JSON store now writes
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

Not yet done: the AI Chat surface has no streaming responses, no semantic-vector
search (keyword-RAG only), and no `web_search`/calendar tools or conversation
search/archive/rename; the AI Notepad has no rich markdown editor (plain textarea)
or folder "add existing note" picker; meetings have no speaker diarization
("Them" is everyone else; tasks are attributed from attendees and names said)
and remove speaker echo only from chunks that are entirely the call (see
[`docs/meetings.md`](./docs/meetings.md) "Limits"), and a real call (live
WASAPI loopback + a real LLM's action plan) still wants one hands-on pass;
a true streaming model for the partial pass (spike-gated
Stage 2 — see [`ROADMAP.md`](./ROADMAP.md) Phase 1);
fuzzy/near-miss dictionary matching; verify-after-paste (UIA `ValuePattern`);
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
