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

This file is the short version: the rules we work by, how to build, test and ship,
and a one-line index of every module. **The detail lives in
[`docs/architecture/`](./docs/architecture/README.md)**, one file per area: read the
one for the area you're changing before you change it. For *where it's going* and
the competitive strategy, see `ROADMAP.md` in the **private** docs repo (below).

> **Mission — the best-of-everything blend.** Take the best of superwhisper,
> OpenWhispr, Wispr Flow, Handy, Aqua and FluidVoice into one Windows-first,
> local-first app, porting proven patterns **from source**, not screenshots:
> OpenWhispr is cloned beside this repo at `../references/openwhispr` (Handy at
> `../references/Handy`) — read its implementation first. The Settings / Language
> Models / Prompt Studio surface tracks OpenWhispr's UX and wording while keeping
> Yap's backend contract (immutable guardrails + editable body). Full notes:
> [architecture README](./docs/architecture/README.md#background-notes-from-claudemd).
>
> **Private docs (2026-10-17).** The roadmap, competitor research, feature
> checklists and internal test plans are NOT in this public repo: they live in
> the private repo `nayballs/yap-private`, cloned beside this one as **`../yap-private`**
> (`ROADMAP.md`, `docs/wispr-flow-features.md`, `docs/openwhispr-*.md`,
> `docs/competitive-analysis.md`, …). Paths in this file named `ROADMAP.md` or
> one of those `docs/` files mean that repo. Write new competitor research,
> teardown findings and plans there (or in `../references/`), never in
> this repo: its `docs/` keeps only how Yap works.
>
> **Maintainer-specific working notes live in the private repo** (`docs/maintainer.md`:
> what you may do without asking, how to hand over a change to try, the maintainer's PC).

> **Privacy in the repo.** It's public: fixtures, tests, evals and comments use fictional
> people and details (example.com, 07700 900xxx), never real names, emails, places or
> details of the maintainer or of users. The pre-push guard enforces it
> (`scripts/privacy-guard.mjs`; `npm run hooks:install` once per clone).

---

## Where things are

- `desktop/` — **the desktop app** (Tauri): `src/` its windows (Svelte), `src-tauri/` its core (Rust), `e2e/` its UI tests, `e2e-real/` its robot tests, its own `package.json`. One app for every desktop system, Windows today, macOS and Linux later: code only Windows has stays behind `cfg(windows)` with a fallback that builds elsewhere ([app-shell.md, Platforms](./docs/architecture/app-shell.md#platforms-windows-today-macos-and-linux-later)).
- `phone/` — **the phone app** ([README](./phone/README.md)): `web/` today (`page/`, served by the PC; `tests/`); native apps go beside it. The PC's side is `desktop/src-tauri/src/link/`.
- `cloud/` the accounts service · `site/` the website · `evals/` model evaluations · `docs/` how Yap works · `scripts/` dev, build, release and guards.
- The repo is an npm workspace (since 4 Nov 2026): `npm ci` at the root installs it all into the root's `node_modules`, and the root's scripts run the desktop app's (`npm run tauri dev`, `npm run build`, `npm run test:app`). Paths in these docs are from the repo root.

---

## Stack

Versions and details: [app-shell.md](./docs/architecture/app-shell.md#stack).

- **Shell:** Tauri 2 — Rust (`desktop/src-tauri/src/`) + Svelte 5 / Vite 6 (`desktop/src/`). **Rich editor:** TipTap 3 on ProseMirror.
- **Windows:** `settings` (the main window), `onboarding`, `notepad`, `quick-note`, `overlay` (the Yap bar), `doc-<hash>` (a Markdown file's).
- **Audio:** `cpal` capture, `rodio` playback. **STT:** `transcribe-rs` (vendored, `desktop/src-tauri/vendor/transcribe-rs`) — whisper.cpp on Vulkan + ONNX on DirectML; `whatlang` for a transcript's language.
- **AI:** `reqwest` → any OpenAI-compatible endpoint, the ChatGPT plan, or the on-device sidecar on llama.cpp's own `llama-server` builds.
- **Spoken replies:** Kokoro-82M (ONNX on `ort`), Windows' voices, ElevenLabs, OpenAI, Gemini, Groq. **Speaker diarization (beta):** Nemotron-3-Diarization on NeMo-Speech.cpp.
- **Text injection:** Win32 clipboard + `SendInput`; UI Automation reads the field.
- **Updates/install:** `tauri-plugin-updater` + `updates.rs` + a custom NSIS installer. **Plugins:** autostart, window-state, opener, dialog, deep-link + single-instance (held on 2.4.x).
- **Accounts (optional):** Better Auth on a Cloudflare Worker + D1 (`cloud/`). **Calendar:** Google OAuth or iCal links; `rrule` + `chrono-tz`.
- **MCP:** `rmcp` 3.9 — Yap's server (`yap.exe mcp`) and the client of connectors (preview); `toml_edit` for Codex/ChatGPT's config.
- **Claude Code:** the person's own install, unmodified, as a child process (`claude -p`, stream-json) on their own sign-in (`ai/claude_code.rs`). Never "Claude Code" in a feature's name.
- **Context menus:** `webview2-com` cuts WebView2's menu down to the editing commands.

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
        the audio callback); emits           AI cleanup pass (ai/llm.rs, if enabled)
        "yap-amp" peak for the                    │
        scrolling waveform                   apply_dictionary()
                    │                              │
            "yap-state" drives the        text_injector (clipboard paste [+ Enter])
            overlay + tray                        │
                                            emits "yap-transcript"
```

After transcription: **AI cleanup (in the style of the kind of app, at its strength) → a transform on every dictation (optional) → dictionary (exact, then fuzzy near-misses) →
the "press enter" voice command → snippets → fit to the kind of app and the text
around the cursor → inject → submit key, or the correction watch**. Cleanup is
best-effort — any error/timeout falls back to the raw transcript, so dictation never
blocks. The full order and its rules: [dictation.md](./docs/architecture/dictation.md#the-pipeline-order).

---

## Module index (`desktop/src-tauri/src/`)

One folder per area (an area's `<area>.rs` lists its modules); a big module's parts are its children
(`dictation/pipeline/`…), each named in its area's doc. The tree and its rules: [app-shell.md](./docs/architecture/app-shell.md#modules).

**Dictation** — [dictation.md](./docs/architecture/dictation.md)
- `dictation/pipeline.rs` (+ `pipeline/`) — the heart: record, transcribe, deliver, Undo. The `processing` guard (`Busy`) is let go however a task ends; the warm engine is lent in turns (`EngineCell`), never loaded twice.
- `dictation/gesture.rs` — push-to-talk's pure state machine: hold, double-tap hands-free, tap, triple-tap.
- `dictation/coaching.rs` / `dictation/tips.rs` — push-to-talk coaching hints and feature tips, counted in `hints.json`.
- `dictation/input_hook.rs` (+ `input_hook/held.rs`) — low-level keyboard and mouse hooks: every binding, clashes, the keys held.
- `dictation/side_buttons.rs` — the mouse's side buttons set up in one step; Undo / Remove.
- `dictation/transforms.rs` — saved transforms: one-key rewrites of the selection or the last dictation.
- `dictation/ask_or_edit.rs` — the rewrite shortcut on a selection: edit it, or answer about it when in doubt.
- `dictation/command_screen.rs` — a Command that drafts from the window in front; a cloud model sees it only once allowed.
- `dictation/languages.rs` (+ `languages/`) — My languages, language shortcuts, the "You spoke French" nudge.
- `dictation/partials.rs` / `dictation/agent_detect.rs` — live partials; Commands by name.
- `dictation/mic.rs` — the dictation mic, opened on demand, on its own thread.
- `dictation/mics.rs` (+ `mics/`) — the microphones' order and Windows' list watched; `pipeline/unplugged.rs` ends a dictation whose mic left.
- `dictation/mic_watch.rs` / `dictation/mic_quiz.rs` / `dictation/mic_privacy.rs` / `dictation/mic_tips.rs` / `dictation/lid.rs` — a dead mic, the mic quiz, Windows' privacy switches, mic-quality tips, the laptop's lid.
- `dictation/stt.rs` — `SttEngine` over transcribe-rs: the model registry, download + verify.
- `dictation/stt_speed.rs` — the speech engine's device on this PC, from a one-time benchmark.
- `dictation/slow.rs` / `dictation/cap.rs` — "Taking longer than usual…"; the 15-minute dictation limit.
- `dictation/meter.rs` — the waveform's level (`yap-amp`).
- `dictation/recent_audio.rs` — a day of dictation audio (`recent-audio/`), to run one again.
- `dictation/media.rs` — Upload's audio-file decoding and chunking.
- `dictation/sound.rs` (+ `sound/packs.rs`) — Yap's sounds, made from numbers (no files).
- `dictation/mute.rs` / `dictation/media_pause.rs` / `dictation/other_apps.rs` — other apps silenced or paused while dictating, restored exactly; one ledger for every turn-down.
- `dictation/history.rs` (+ `history/keep.rs`) — local dictation history; Keep history.
- `dictation/stats.rs` (+ `stats/`) / `dictation/milestones.rs` — stats that outlast history (`dictation_stats.json`, numbers only); milestones.
- `dictation/check.rs` (+ `check/score.rs`) — `yap.exe dictation-check` (debug): your own dictations against a baseline.

**Pasting** — [pasting.md](./docs/architecture/pasting.md)
- `pasting/text_injector.rs` — private, verified clipboard paste, every `SendInput` checked; left on the clipboard when it can't land.
- `pasting/read_check.rs` — did the app itself read the paste; the person's clipboard put back after.
- `pasting/rich.rs` — rich snippets: Yap's markdown subset → plain text and CF_HTML.
- `pasting/as_said.rs` — "Use what I said", only where it can select exactly what it pasted.
- `pasting/last.rs` / `pasting/send_key.rs` — Paste / Copy last transcript; the Send shortcut (never Enter).
- `pasting/elevated.rs` — apps running as administrator: the text is left on the clipboard.
- `pasting/modifiers.rs` — stuck Alt/Win/Shift/Ctrl let go before Yap's keys.
- `pasting/slices.rs` — long pastes in slices, under coding CLIs' and terminals' limits.
- `pasting/stacked.rs` / `pasting/mentions.rs` / `pasting/file_tags.rs` / `pasting/compose.rs` — stacked messages, "at Sam" and files said aloud, a dictation in steps.
- `pasting/selection.rs` — UI Automation reads: the text around the cursor, the selection, did a paste land.
- `pasting/smart_insert.rs` — fits a dictation to the text around the cursor.
- `pasting/app_kind.rs` — terminal / coding CLI / chat app / other, and what each changes.

**Dictionary** — [dictionary.md](./docs/architecture/dictionary.md)
- `dictionary/fuzzy.rs` — near-miss correction (never everyday words), the prompt-echo guard.
- `dictionary/dict_usage.rs` / `dictionary/variants.rs` — how often each word is used; several ways one word is heard.
- `dictionary/files.rs` — the Dictionary's Import… / Export… files.
- `dictionary/learn.rs` / `dictionary/snippet_offer.rs` — words and snippets offered from what you do.
- `dictionary/spoken_names.rs` — how names spelled far from how they're said sound.
- `dictionary/screen_terms.rs` (+ `dictionary/screen_terms/`, `screen_terms/code.rs`) — names and terms read off the window in front, for that dictation only.

**AI models** — [ai-models.md](./docs/architecture/ai-models.md)
- `ai/about_you.rs` (+ `about_you/units.rs`) — About you: Region, name and note, the block every prompt ends with.
- `ai/llm.rs` (+ `llm/anthropic.rs`, `llm/responses.rs`) — the OpenAI-compatible client, immutable base prompts; Anthropic's and the ChatGPT plan's APIs.
- `ai/chatgpt.rs` (+ `chatgpt/`) — Sign in with ChatGPT; tokens in Credential Manager.
- `ai/claude_code.rs` (+ `claude_code/`) — the person's own Claude Code as a child process; `yap.exe fake-claude` the tests' stand-in.
- `ai/claude_plan.rs` (+ `claude_plan/`) — Your Claude plan in every scope, through the person's own Claude Code.
- `ai/cleanup_styles.rs` — cleanup's tone per kind of app; an answer that lost the dictation is thrown out.
- `ai/writing_samples.rs` — your own writing for rewrites, never cleanup.
- `ai/cleanup_strength.rs` / `cleanup_format.rs` / `cleanup_context.rs` — strength (None = off), lists laid out, the text before the cursor.
- `ai/local_llm.rs` (+ `local_llm/`) — the on-device sidecar on llama.cpp's own builds, self-updating; a key per launch, never on the command line.
- `ai/gguf.rs` / `ai/gpu.rs` / `ai/hardware.rs` — GGUF headers and memory estimates; the graphics cards; picks by hardware.
- `ai/ai_model.rs` — which model a scope's next call goes to.
- `ai/effort.rs` — effort per cloud model; Fast mode.
- `ai/model_catalog.rs` — providers' live model lists, cached a day.
- `ai/usage.rs` (+ `usage/`) — every AI call's tokens, counted in one place (`llm::post_chat_request`).
- `ai/eval.rs` (+ `ai/eval/`) — model evaluations (`yap.exe eval`) through the real Chat code.

**Chat and the Voice Assistant** — [chat-and-assistant.md](./docs/architecture/chat-and-assistant.md)
- `chat/chats.rs` — conversations (`chats.json`) and the checks on every answer.
- `chat/budget.rs` / `chat/compact.rs` — long chats sized to the model's real context.
- `chat/note_chats.rs` — a meeting's Ask thread, kept with its note.
- `chat/skills.rs` — Skills: saved instructions run with `/name`.
- `chat/tools.rs` (+ `tools/`) / `chat/note_text.rs` — the tool loop. After outside content (web, screen, a connector) apps, the clipboard and memory stay locked.
- `chat/activity.rs` / `chat/stream.rs` — what Chat is doing now; streamed answers, ■ Stop.
- `chat/commands.rs` / `chat/turn.rs` (+ `turn/`, `turn/fit.rs`) — the Chat view's commands; one chat turn's steps, in order.
- `chat/tools/agents.rs` — Yap Code's four tools; never after web content.
- `chat/tools/connectors.rs` — connectors' tools in Chat (preview), every call through `consent::decide`.
- `chat/assistant.rs` (+ `assistant/`: `connector_ask.rs`, `screen.rs`, `selected.rs`, `skill.rs`, `spoken_ok.rs`: every card that asks first, said and answered out loud) — the Voice Assistant; off during meetings.
- `chat/pages.rs` (+ `pages/`: `view.rs`, `see.rs`, `read.rs`, `act.rs`, `backend.rs`, `sim.rs`) — the browser beside the chat: its own WebView2, no Yap IPC, public http(s) only; `read.rs` a page read in parts (`offset`, `next`) and its tables as rows.
- `chat/see_screen.rs` — `look_at_screen`; never in a meeting.
- `chat/think.rs` — Think it through, on the on-device model.
- `chat/documents.rs` (+ `documents/rich.rs`, `blocks.rs`) — documents beside the chat; the person's edits win (`merge3`).
- `chat/pictures.rs` (+ `pictures/local.rs`: a picture file on this PC, by its path) — find and show pictures.

**Snip to Chat** — [snip.md](./docs/architecture/snip.md)
- `snip.rs` — drag a box over any screen into Chat; never sent by itself; the `yapsnip` scheme.
- `snip/waiting.rs` — the screenshots waiting in Chat's box, kept in Rust.
- `snip/capture.rs` / `snip/hdr.rs` — every monitor at once; HDR → sRGB.
- `snip/overlay.rs` / `snip/geom.rs` / `snip/pick.rs` / `snip/copy_save.rs` — the native overlay, its pure maths, the windows to pick, copy and save.
- `snip/attach.rs` / `snip/ocr.rs` / `snip/thumbs.rs` — pictures pasted into Chat, OCR, thumbnails at exact device pixels.
- `snip/images.rs` / `snip/turn.rs` / `snip/vision.rs` / `snip/dump.rs` — `chat-images/`, image parts per provider, which models read images.

**Memory** — [memory.md](./docs/architecture/memory.md)
- `memory.rs` (+ `memory/`, `memory/small.rs`, `memory/import.rs`) — what Yap remembers about you (`memory.json`).

**Meeting search** — [search.md](./docs/architecture/search.md)
- `search.rs` (+ `search/`) — search by meaning on this PC; never in a dictation's way.

**Web search and apps** — [web-and-apps.md](./docs/architecture/web-and-apps.md)
- `web.rs` (+ `web/`) — web search, or free sources with no key; never a search engine scraped.
- `apps.rs` (+ `apps/`) — opens Start-menu apps on request.

**Spoken replies** — [spoken-replies.md](./docs/architecture/spoken-replies.md)
- `voice/tts.rs` (+ `voice/tts/`, `voice/tts/marks.rs`) — the voices, the speaker queue, word timing.
- `voice/duck.rs` / `voice/read_aloud.rs` — other apps turned down while Yap speaks; Read aloud anywhere.

**Notes** — [notes.md](./docs/architecture/notes.md)
- `notes.rs` — the notes store (`notes.json`); nothing new only in fields an older Yap's save would drop.
- `notes/note_edits.rs` / `notes/transcript_edits.rs` — fixing lines and the summary; a summary run never writes over the person's words.
- `notes/tasks.rs` + `notes/task_store.rs` — the summary's checkboxes; tasks (`tasks.json`).
- `notes/retention.rs` / `notes/pins.rs` — deleting old transcripts; pinned notes (`note_pins.json`).
- `notes/versions.rs` / `notes/transform.rs` — each note's version history; transforms on a note.
- `notes/quick_note.rs` (+ `quick_note/`) — the quick note: a small always-on-top window, saved as ordinary notes.
- `notes/md_file.rs` (+ `md_file/`) — Markdown files in a `doc-<hash>` window, saved back only while unchanged on disk.

**Meetings** — [meetings.md](./docs/architecture/meetings.md) (guide: [`docs/meetings.md`](./docs/meetings.md))
- `meetings/meeting.rs` — the recorder: mic + loopback, chunking, echo, one clock; one recorder per start.
- `meetings/meeting_audio.rs` (+ `meetings/meeting_audio/`) / `meetings/disk_watch.rs` — the meeting's audio kept as FLAC; the drive's room.
- `meetings/meeting_summary.rs` (+ `meeting_summary/`) — rolling digests and the action plan.
- `meetings/meeting_end.rs` / `meetings/meeting_refine.rs` — a recording's end; the refined transcript.
- `meetings/meeting_assist.rs` / `meetings/coach_marks.rs` — the notepad's AI; first-meeting coach marks.
- `meetings/notepad.rs` / `meetings/sleep.rs` — the notepad window; the PC asleep mid-meeting.
- `meetings/import.rs` (+ `import/`) — Import a transcript… from other apps.

**Speakers** — [speakers.md](./docs/architecture/speakers.md)
- `speakers/diarize.rs` (+ `speakers/diarize/`) — the beta speaker engine in a `yap.exe diarize` child.
- `speakers.rs` (+ `speakers/labels.rs`) / `speakers/naming.rs` — the speaker map and names on lines; who each voice is.
- `speakers/voiceprint.rs` (+ `voiceprint/`) + `speakers/voices.rs` — voiceprints, never kept.
- `speakers/rerun.rs` / `speakers/flags.rs` / `speakers/clip.rs` / `speakers/call_ui.rs` — Find speakers, Merge / Split, a voice's ▶, the call app's view.

**Call detection and guard rails** — [call-detection-and-guards.md](./docs/architecture/call-detection-and-guards.md)
- `calls/meeting_detect.rs` (+ `meeting_detect/auto_offer.rs`) — notices calls and asks.
- `calls/call_audio.rs` / `calls/app_icons.rs` — the call's own mic and mute; the call apps' icons.
- `calls/meeting_guard.rs` — length limit, quiet meetings, check-ins.
- `calls/capture.rs` — meeting windows hidden from screen capture while recording.

**Calendar** — [calendar.md](./docs/architecture/calendar.md) (guide: [`docs/calendar.md`](./docs/calendar.md))
- `calendar.rs` (+ `calendar/`: `brief.rs`, `codes.rs`, `filing.rs`, `filing/companies.rs`) — Google/iCal sync, reminders, pre-meeting briefs, folder rules.

**Yap bar** — [yap-bar.md](./docs/architecture/yap-bar.md)
- `bar.rs` (+ `bar/overlay.rs`) — the bar: click-through, never focused, one card at a time.
- `bar/pointer.rs` / `bar/menu.rs` — first-use pointers; the ^ menu's extras.

**Notices** — [notices.md](./docs/architecture/notices.md)
- `notices/notice.rs` — the notice policy, in one place.
- `notices/alert.rs` / `notices/kinds.rs` / `notices/win_toast.rs` — notices to act on; kinds with a daily cap; Windows notifications.

**Integrations** — [integrations.md](./docs/architecture/integrations.md)
- `integrations/bridge.rs` — the token-authenticated local API (`~/.yap/cli-bridge.json`).
- `integrations/mcp.rs` — `yap.exe mcp`: Yap's MCP server (meetings + notes only).
- `integrations/mcp_clients.rs` — "Add to Claude / ChatGPT / …": edits AI apps' configs safely.
- `integrations/claude_live.rs` (+ `claude_live/`) — the loopback endpoint Claude Code runs talk back to, a key per run.
- `integrations/connectors.rs` (+ `connectors/`) — Connectors (preview; off: nothing read, started or sent): Yap as an MCP client.

**Yap Code** — [yap-code.md](./docs/architecture/yap-code.md)
- `agents.rs` (+ `agents/`, `agents/remote.rs`) — Yap Code (off by default): the person's own Claude Code sent to work in a workspace; its name one constant (`agents::FEATURE`).
- `agents/usage.rs` — what each agent turn used: tokens, the context's fill, its cost estimated (the Agent panel's numbers).

**Yap Link (the phone)** — [link.md](./docs/architecture/link.md)
- `link.rs` (+ `link/`) — Yap's AI from a paired phone on the home Wi-Fi (off by default); never on a Public network.
- `link/code.rs` (+ `code/gates.rs`, `code/view.rs`) / `link/lanes.rs` — Remote Control (off): Yap Code from a phone, every message through the gates.

**Accounts** — [accounts.md](./docs/architecture/accounts.md)
- `auth.rs` — optional accounts: sign-in, the session.

**Network receipts, Report a problem** — [privacy-receipts.md](./docs/architecture/privacy-receipts.md)
- `privacy/net.rs` — every request through one place (`.send_for(Purpose)`); never content.
- `privacy/diagnostics.rs` / `privacy/export.rs` — Report a problem and Download your data: a .zip, never uploaded.
- `privacy/reset.rs` — Delete some of your data and Reset app.

**Updates** — [updates-and-release.md](./docs/architecture/updates-and-release.md)
- `updates.rs` — background update checks and downloads, safe install.

**App shell, config, tests** — [app-shell.md](./docs/architecture/app-shell.md), [config-and-data.md](./docs/architecture/config-and-data.md), [build-and-test.md](./docs/architecture/build-and-test.md)
- `main.rs` — entry; `yap.exe mcp` / `diarize` / `eval` / `bench` / `call-dump` / `snip-dump` / `partials-dump` / `dictation-check` / `fake-claude` run before the app starts.
- `shell/bench.rs` (+ `shell/bench/`) — `yap.exe bench` ([`docs/benchmarks.md`](./docs/benchmarks.md)).
- `lib.rs` — setup and wiring: modules, plugins, every command (`generate_handler!`), shutdown.
- `shell/tray.rs` / `shell/context_menu.rs` — the tray; WebView2's menu cut to editing commands.
- `shell/awake.rs` — the PC kept awake while agents work with Remote Control on.
- `shell/procs.rs` — the kill-on-close job, so helpers never outlive Yap.
- `shell/com.rs` — COM's multithreaded apartment kept up while Yap runs.
- `shell/browsers.rs` / `shell/download.rs` / `shell/file_drop.rs` — which browser links open in; big downloads; files dragged in.
- `shell/webview_memory.rs` / `shell/theme.rs` / `shell/portable.rs` — hidden webviews' memory; the theme; portable mode.
- `shell/guide.rs` / `shell/help.rs` — getting started (`onboarding.json`); the shortcut sheet.
- Commands: each module's own, else its area's `commands.rs` (app-shell.md).
- `shell/window.rs` / `settings.rs` / `autostart.rs` / `logging.rs` / `commands.rs` — the main window (`show_quietly`), config applied, launch at sign-in, the log, links.
- `config.rs` — `YapConfig`, saved one at a time (`config::update`), migrations.
- `shell/saving.rs` — every JSON store's writes and reads: a failed write retried; an unreadable file never written over.
- `shell/e2e.rs` / `shell/realtest.rs` / `shell/lab.rs` — test mode, robot mode, Yap Lab's record and live web ([`docs/lab.md`](./docs/lab.md)).
- `privacy/log_privacy.rs` — a test: no user content in the log at `info` and above.

## Frontend index (`desktop/src/`)

[frontend.md](./docs/architecture/frontend.md) unless noted; its [folder layout](./docs/architecture/frontend.md#the-folder-layout-2026-10-24) has every folder and the big views' parts. Paths below are under `desktop/src/lib/`.
- `desktop/src/` — `main.js`, `App.svelte` (one chunk per window), `app.css`; `lib/` by kind: `views/`, `settings/`, a folder per other window, `components/`, `stores/`, `notices/`, `core/`, `ui/`.
- Theme — `--yap-*` / `--yap-paper-*` tokens in `desktop/src/app.css`, light and dark; never hard-code a colour; mark `desktop/src/assets/yap-logo.svg` ([dark theme](./docs/architecture/frontend.md#the-dark-theme-2026-10-22)).
- `core/appearance.js` — General → Appearance in every window: theme, reading font, Motion.
- `views/ControlPanel.svelte` — the main window: sidebar rail (`core/navRail.js`, `stores/navRail.svelte.js`), Settings, title bar (`stores/chatBrowser.svelte.js`).
- `ui/toast.svelte.js` + `ui/noticePolicy.js` — toasts under the notice policy; `notices/` Rust's events as toasts (`companyFolders.js`, `saveErrors.js`…).
- `views/home/HomeView` / `views/insights/InsightsView` / `views/UploadView` — the dictation feed, stats and audio-file upload; Import a transcript… (`components/TranscriptImportDialog.svelte`, [meetings.md](./docs/architecture/meetings.md)).
- `views/dictionary/` — the Dictionary view ([dictionary.md](./docs/architecture/dictionary.md)).
- `views/notes/` — the AI Notepad (`NotesView.svelte`, `NoteEditor`; [notes.md](./docs/architecture/notes.md)); `views/TasksView.svelte`; `components/MeetingTasks.svelte`.
- `components/richtext/` — the rich editor for Chat's documents and notes (`richEditor.js`, `richMarkdown.js`, `RichNote.svelte`); the note stays markdown exactly ([notes.md](./docs/architecture/notes.md#notes-as-pages-you-can-format-componentsrichtext-2026-10-31)).
- `core/markdown.js` — every rendered answer and note: markdown-it + DOMPurify ([notes.md](./docs/architecture/notes.md#the-markdown-renderer)).
- `views/chat/` — Chat ([chat-and-assistant.md](./docs/architecture/chat-and-assistant.md)): `ChatView.svelte` + `chatState.svelte.js`, `ConversationList`, `ChatThread`, `Composer`, the model picker over `core/chatPicker.js` (shared with the phone), the panel (`DocPanel`, `PageTab`); screenshots: [snip.md](./docs/architecture/snip.md#how-a-screenshot-looks-2026-10-31-after-the-claude-apps).
- `views/chat/` Code side — `ChatSideSwitch`, `codeChats.svelte.js`, `ModeChip`, `AgentTab` (its panel `AgentPanel` + `core/agentPanel.js`), `AgentTodoCard`; `stores/agents.svelte.js` ([yap-code.md](./docs/architecture/yap-code.md)).
- `views/help/` + `views/MeetingSetup.svelte` — the title bar's Help and the "Set up meeting notes" checklist.
- `views/MeetingsView.svelte` and `notepad/` — meetings and the notepad (`Notepad.svelte`, `stores/meetingAudio.svelte.js`; [meetings.md](./docs/architecture/meetings.md)); `components/BriefView.svelte` ([briefs](./docs/architecture/calendar.md#pre-meeting-briefs)); `components/SpeakerMenu` / `SpeakerCard`.
- `components/ConnectCalendarDialog.svelte` — "Connect your calendar" ([calendar.md](./docs/architecture/calendar.md)).
- `views/integrations/IntegrationsView.svelte` — local API and MCP cards ([integrations.md](./docs/architecture/integrations.md)).
- `bar/Overlay.svelte` + `bar/` — the Yap bar's page; `bar/ReadingCard.svelte` ([yap-bar.md](./docs/architecture/yap-bar.md)).
- `settings/ConnectionsView.svelte`, `settings/ReportProblem.svelte`, `settings/YourData.svelte` — network receipts, Report a problem, your data ([privacy-receipts.md](./docs/architecture/privacy-receipts.md)).
- `settings/` — Settings, always mounted in the modal: `Settings.svelte` the shell, a `*Page.svelte` per page, `configSync.svelte.js` the config copy, `hotkeyFallback.svelte.js` the in-page keys; `core/providers.js`. Every part: [frontend-settings.md](./docs/architecture/frontend-settings.md#the-module-map-split-2026-10-24).
- `settings/MemorySection.svelte` + `stores/memory.svelte.js` — Settings → Assistant → Memory ([memory.md](./docs/architecture/memory.md)).
- `settings/SkillsPage.svelte` + `stores/skills.svelte.js` + `core/skills.js` + `components/SkillMenu.svelte` — Settings → Skills and the "/" menu ([skills](./docs/architecture/chat-and-assistant.md#skills-and-one-tap-prompts-2026-10-29)).
- `settings/YapCodePage.svelte` + `settings/ClaudeCodeStatus.svelte` — Settings → Assistant → Yap Code ([yap-code.md](./docs/architecture/yap-code.md)).
- `settings/AccountSection.svelte` — Settings → Account ([accounts.md](./docs/architecture/accounts.md)).
- `settings/connectors/` — Settings → Connectors (preview, off by default): `ConnectorsPage`, `connectors.svelte.js` ([integrations.md](./docs/architecture/integrations.md#settings--connectors-wave-5-2026-11-08)).
- `settings/PhonePage.svelte` + `components/LinkPairPrompt.svelte` — Settings → Phone and the pairing prompt ([link.md](./docs/architecture/link.md)).
- `settings/ModelManager` / `ModelRow` (`stores/modelStore.svelte.js`, `core/speechModels.js`), `onboarding/` (`Onboarding.svelte`, `setupState.svelte.js`), `ui/` — speech models, first run, primitives.
- Window config (`tauri.conf.json`) — [app-shell.md](./docs/architecture/app-shell.md#window-config-desktopsrc-tauritauriconfjson).

## Gotchas that bite (details in the linked docs)

- When one of Yap's own WebView2 windows has focus the global hook never fires: every window with focusable UI needs the **in-page hotkey fallback** (`dictation/input_hook.rs`).
- WebView2 keeps `document.visibilityState === 'visible'` while a window is hidden: window visibility comes from Rust (`updates.rs`).
- Never swap the hidden windows' **DWM cloak** for "park off-screen, show+hide" (`shell/window.rs`).
- Never call a window's `.show()`: tao's activates it (the bar, the notepad took the focus) — `shell::window::show_quietly` (`app-shell.md`).
- Never `window.confirm/alert/prompt` in a Yap window — use `ui/ConfirmDialog.svelte` (`frontend.md`).
- The bar window is transparent: no `backdrop-filter`, no big shadows (`yap-bar.md`).
- A **signed-out Yap never contacts the account service** on its own (`auth.rs`).
- No user content in logs at `info`/`warn`/`error` — lengths, counts and ids only; the text at `debug` at most, never keys (`privacy/log_privacy.rs` checks; `app-shell.md`, Logging).
- Lock rules in `calls/meeting_detect.rs` and `calendar.rs`: never touch windows/WinRT/the network under their state locks.
- New settings that must change saved values once go in `config::migrate_once` (`config-and-data.md`).

---

## Build, run & feature flags (important)

| Feature | Effect |
|---------|--------|
| *(default)* | **STUB** — no `transcribe-rs`. `cargo check` stays fast. The app runs but returns placeholder text. |
| `engines` | Real STT: `transcribe-rs` with **whisper-vulkan** + ONNX + **ort-directml** (Whisper on any GPU via **Vulkan**, ONNX via DirectML; CPU with no GPU). **Release + nightly builds use it.** |
| `whisper` | Back-compat alias for `engines`. |
| `custom-protocol` | Required for release/standalone builds — embeds the frontend. `tauri build` sets it. |

GPU policy: **universal, no CUDA** (Whisper → Vulkan, ONNX → DirectML); building
`engines` needs the **Vulkan SDK**. Details: [build-and-test.md](./docs/architecture/build-and-test.md).

**Dev:** `scripts/dev.bat` (= `npm run tauri dev -- --features engines`, Vite on
**:51437**, hot-reloads `desktop/src/`) behind the **"Yap - Dev"** desktop shortcut; the plain
**"Yap"** shortcut is the *installed* app (nightly channel). It runs behind an animated
launcher screen (`scripts/dev-launcher.mjs`; `YAP_DEV_PLAIN=1` for the old output).
dev.bat first fast-forwards the checkout to `origin/main` (`scripts/dev-update.ps1`)
and stops AI apps' `yap.exe mcp` helpers running from its exe: merged work reaches the
maintainer's dev build by pushing to main; never fast-forward that checkout by hand
while Yap - Dev runs.

```bash
# real GPU pipeline (default in dev.bat) — requires the Vulkan SDK
npm run tauri dev -- --features engines
# stub (fast, no transcription)
npm run tauri dev
```

Accounts in debug builds: `cd cloud && npm run dev` (wrangler dev on :8787; see `cloud/README.md`).

> ⚠️ A *compiled release build* bakes the frontend into the binary — editing `desktop/src/`
> and restarting that `.exe` changes nothing. For live frontend changes use dev
> (Vite on :51437). If :51437 isn't listening, you're looking at a release build.

**CI on every push** (`.github/workflows/ci.yml`): the privacy guard's generic checks (keys, emails, phone numbers),
`npm run build` + `cargo clippy --locked --all-targets -- -D warnings` on the stub build,
then `cargo clippy --locked --release -- -D warnings` (debug-only code is dead in release:
gate it with `#[cfg(debug_assertions)]`), and the phone app's tests (`npm run test:link`, Chromium and Firefox).
⚠ Zero-warnings ratchet on the **latest stable** toolchain — keep local Rust current
(`rustup update`). `e2e.yml` runs the e2e suite on the same triggers. Dependabot opens
weekly update PRs; `pins.yml` keeps an issue open while a pinned download is behind
upstream (`npm run pins:check`); `issue-labels.yml` labels bug reports by area
(`npm run test:issue-labels`); both in build-and-test.md.

**Tests:**
- **Say in bold whether the maintainer can use the PC** whenever a big run starts or ends: **Hands off the keyboard (about N min)** for robot tests, which take the keyboard and focus; **You can use the PC** for the UI suite, builds and agent batches, adding that it may feel slower. Repeat it when it changes, and say plainly when it's all clear.
- `npm run test:app` — the end-to-end UI suite: Playwright drives a real stub debug build over CDP (~35 min for all 530+; one spec in seconds), named screenshots in `test-results/app/screenshots/`; its windows sit behind yours. Guide: [`docs/e2e-tests.md`](./docs/e2e-tests.md).
- `npm run test:real` — real-app robot tests: a robot-mode Yap pastes into real windows (`--engines` build: real STT). ⚠ **It takes the keyboard focus for ~2 min** (local only): it starts only after 10 idle minutes (`YAP_ROBOT_FORCE=1` when the person says so) and stops on any real input. Guide: [`docs/e2e-tests.md`](./docs/e2e-tests.md) "Real-app robot tests".
- `npm run test:link` — Yap Link's phone page (`phone/web/page/`): its sound maths in Node, then the page in headless Chromium (or Edge) and Firefox when installed, against a mocked PC and a faked mic (~1–2 min, nothing on screen). Guide: [its README](./phone/web/README.md).
- `npm run privacy:scan` / `npm run test:privacy` — the privacy guard over the whole tree, and its own tests ([build-and-test.md](./docs/architecture/build-and-test.md#the-privacy-guard-scriptsprivacy-guardmjs)).
- `npm run eval` — model evaluations: real models through Yap's own Chat code, every request logged; run it before changing a prompt, a tool or the default local model. Guide: [`docs/model-evals.md`](./docs/model-evals.md).
- `npm run -s lab -- <command>` — **Yap Lab**: a test Yap you question through its own Chat (typed or spoken) on the real web, reading back each turn's tools, cards and answer as JSON, and `report <prompt>` has it test a tool family and write up what fails. Real runs use the person's Claude plan: **ask first**; `npm run test:lab` is offline (`--fake`). Guide: [`docs/lab.md`](./docs/lab.md).
- `yap.exe dictation-check run <dir> --cleanup` (an engines debug build, e.g. `E:\Builds\t-real`) — the maintainer's own dictations against a baseline, before pushing any change to speech-to-text or cleanup. The set is private (`../yap-private/dictation-tests/`); never copy it here ([build-and-test.md](./docs/architecture/build-and-test.md#your-own-dictations-as-a-test-set-yapexe-dictation-check)).

### The dev → nightly → stable workflow (agreed 2026-10-05)
**Dev first, nightly for batches.** Every change goes through these steps in order:
1. **Claude builds it and tests it.** `npm run build` + `cargo clippy --all-targets
   --locked -- -D warnings` + `cargo test`, then **the e2e specs the change touches**
   (`npx playwright test -c desktop/e2e <spec>` after `npm run test:app:build`; add or
   extend a spec for anything new); the full suite only for a batch or before a nightly.
   For **pasting, inserting text, the clipboard, terminals/chat apps, the mic path or
   STT**, also the robot tests (`npm run test:real`, + `npm run test:real:build --
   --engines` for real STT), so Claude checks it itself instead of handing over a
   checklist (they take the keyboard focus ~1.5 min). For **speech-to-text or cleanup**, also the maintainer's own dictations
   (`yap.exe dictation-check collect`, then `run … --cleanup`: nothing flagged or worse
   than the baseline). Push to main: CI, the E2E workflow and the pre-push privacy
   guard re-check every push.
2. **The maintainer tries it in the dev build** ("Yap - Dev" = `scripts\dev.bat`, the
   real GPU pipeline from this checkout). Anything visual or about how it feels is
   checked HERE: Vite hot-reloads `desktop/src/` in seconds; Rust edits need a restart
   (a minute or two). Something off → Claude fixes it → it shows straight away. Never burn a 15-min nightly on something dev shows in seconds.
   Hand it over with exact steps (the private `docs/maintainer.md` says how).
3. **Nightly, once a batch feels right.** Claude dispatches it (or the 05:00 UTC cron
   picks up main) and the maintainer uses it day to day. Nightlies catch what only the
   installed release build shows: the updater, the installer, Windows notifications
   under Yap's own name (dev builds borrow PowerShell's), `com.contextmirror.yap://`
   deep links, start on login — those go straight to a nightly check; everything else goes
   through dev first.
4. **Stable** — tag `v*` deliberately for curated milestones, when the maintainer is happy.

The dev build skips the single-instance lock and shares `%APPDATA%\yap` with the
installed app: quit the installed Yap before starting Yap - Dev, or one hotkey press
starts two dictations.

### Releases
Two auto-update channels, both CI-built: **stable** (tag `v*` → `release.yml` → a
draft GitHub Release; polish its notes by hand) and **nightly** (`nightly.yml`, daily
05:00 UTC cron or dispatch → one rolling `nightly` pre-release,
`<base>-nightly.<run>`, `Yap-nightly-setup.exe`). Both sign updater artifacts with
the same minisign key (`TAURI_SIGNING_PRIVATE_KEY`); Authenticode is still pending.
Installer, channels and notes: [updates-and-release.md](./docs/architecture/updates-and-release.md).

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

---

## Config & data

Everything lives in `%APPDATA%/yap/` (portable: `<exe>/Data/`); what each holds:
[config-and-data.md](./docs/architecture/config-and-data.md). `config.json`,
`models/` (`diarization/`, `tts/`, `search/`), `llm/` (`vision/`, `engine/`, `engine.json`),
`history.json`, `notes.json`, `tasks.json`, `note-versions/`, `note_pins.json`, `skills.json`,
`meeting_folders.json`, `chats.json`, `calendar.json`, `chatgpt.json`, `updates.json`,
`model_catalog.json`, `groq_usage.json`, `ai_usage.json`, `dictation_stats.json`,
`dictionary_usage.json`, `hints.json`, `onboarding.json`, `mic_tips.json`,
`word_suggestions.json`, `snippet_suggestions.json`, `language_nudges.json`, `notices.json`,
`memory.json`, `writing_samples.json`, `chat-images/` (+ `thumbs/`), `connections.json`,
`quieted_apps.json`, `stt_speed.json`, `recent-audio/`, `meeting-audio/` (+ `cuts.json`),
`search-index/`, `link_devices.json`, `link/`, `claude_sessions.json`, `claude/`,
`agents.json`, `agents/`, `workspaces.json`, `remote_control.json`, `connectors.json`,
`connector_tools.json`, `connector_chats.json`, `connectors/`, `icons/`, `logs/`.

Every JSON store writes atomically and quarantines a corrupt file instead of crashing —
config.json too: set aside, never saved over, the person told; its saves one at a time
(`config::update`). A write that fails is kept in memory, retried with the next save and
said once, with Try again; a store file Yap couldn't read is never written over, read
again and merged (`shell/saving.rs`). Secrets never go in files: the account session,
calendar, ChatGPT and connector tokens live in Windows Credential Manager. The local
API's discovery file is `~/.yap/cli-bridge.json`.

Notable defaults: F9 in toggle mode, **Parakeet TDT 0.6B v3** on the device measured fastest here, AI cleanup off,
smart spacing / terminal mode / no full stop in chats on, the Yap bar on, Win+Alt+S snips to Chat (screenshots kept 30 days), call
detection on (it only asks), a 2-hour meeting limit, Win+Alt+M, meeting audio kept 14 days,
web search, spoken replies and search by meaning (its model downloads on demand) off, Yap Code off, other apps turned down while Yap speaks on. Every setting, file and default: [config-and-data.md](./docs/architecture/config-and-data.md).

---

## Current status

- **Dictation is complete and real**: local transcription (Whisper/Vulkan + ONNX/DirectML, or the processor where that's faster), push-to-talk, AI cleanup (cloud or on-device Qwen; a style per kind of app, offered in first setup) with "Use what I said", Commands (a reply drafted from the email or chat in front), saved transforms on one key (Polish, Translate…), My languages (several languages, a language or translation per shortcut, the "You spoke French" nudge), smart pasting into terminals and chat apps, dictionary learning, Undo, a day of audio to run a dictation again.
- **Meetings**: recorder (You/Them), its audio kept on disk (crash recovery, Transcribe again, ▶ a line), rolling digests + action plan, the notepad, call detection, guard rails, the calendar, beta speaker separation, transcripts imported from Teams, Zoom, Meet, Otter and Granola.
- **AI**: Chat with note tools, Ask across meetings (search by meaning on this PC, citing the moment), the Voice Assistant, spoken replies (Kokoro, or ElevenLabs on your key), Read aloud anywhere (karaoke card on the bar), web search, opening apps, memory (learned from chats, kept on this PC, names fed to dictation), Snip to Chat (any screen into Chat, HDR right, the overlay up ~50–60 ms after Win+Alt+S; read by the on-device 4B with its optional projector).
- **Chat and Code** (2026-11-07): with Yap Code on, Chat has two sides: **Chat**, plain conversations, and **Code**, Code chats each attached to a workspace (a folder on this PC), where a request sends an **agent**: the person's own Claude Code working headless there, followed in its Agent tab, reported back, Undo from backups. Both pay with the person's Claude plan or an API key (what differs, and when a chat moves sides: [yap-code.md](./docs/architecture/yap-code.md#chat-and-code)). Tested on Yap's stand-in for Claude Code and the maintainer's first real runs; much not tried yet. Markdown files also open in a Yap window.
- **Around it**: MCP server + local API, Yap Link (home preview: ask Yap's AI from a paired phone on the home Wi-Fi, the answer spoken there), optional accounts (live at `auth.contextmirror.com`), installer + auto-updater on stable and nightly, network receipts (every connection, by purpose) and Report a problem (a file you check, never uploaded).
- Much is unit-, e2e- and robot-tested but not yet tried in real use (a real call, real AI apps, answers heard on speakers…).
- **Not yet done**: search by meaning outside Ask across meetings, the call app's speaker names checked in a real call, one-click Outlook, a streaming STT model, Authenticode signing, non-Windows.

What was tested how, and every gap: [status.md](./docs/architecture/status.md).

---

## Positioning

Local-first dictation and meeting notes for Windows: speech, cleanup, meetings and the
assistant run on the user's own PC, free and open source, with the core never paywalled.
Competitor research and where Yap stands live in the **private** repo
(`../yap-private/docs/market-gap-analysis-2026-10.md`, then `ROADMAP.md`), never here.

---

## Keeping these docs current

When a feature lands, update the module's file in
[`docs/architecture/`](./docs/architecture/README.md) — it should always reflect what's
actually in the code — and its one-line entry in the index above (a new module gets a
new line: its path, what it is, and at most the one or two facts to know before
touching it). Keep CLAUDE.md short: detail goes in the architecture docs, not here.
