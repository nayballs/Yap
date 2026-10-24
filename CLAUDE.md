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

## Stack

Full list with versions and details: [app-shell.md](./docs/architecture/app-shell.md#stack).

- **Shell:** Tauri 2 — Rust backend (`src-tauri/src/`) + Svelte 5 / Vite 6 frontend (`src/`).
- **Windows:** `settings` (the main window), `onboarding`, `notepad` (meeting notes), `overlay` (the Yap bar).
- **Audio:** `cpal` capture, `rodio` playback (chimes, spoken replies).
- **STT:** `transcribe-rs` — whisper.cpp on Vulkan + ONNX models (Parakeet…) on DirectML; `engines` feature. Vendored (`src-tauri/vendor/transcribe-rs`, `[patch.crates-io]`) with two DirectML fixes for Parakeet — see its `YAP.md`.
- **AI:** `reqwest` → any OpenAI-compatible endpoint, or the on-device llamafile sidecar (`local_llm.rs`).
- **Spoken replies:** Kokoro-82M (ONNX on `ort`, CPU; eSpeak NG run separately) or Windows' voices; or ElevenLabs (cloud, the user's own key, streamed PCM).
- **Speaker diarization (beta):** Nemotron-3-Diarization on NeMo-Speech.cpp, in a `yap.exe diarize` child.
- **Text injection:** Win32 clipboard (private formats, verified) + `SendInput`; UI Automation reads the field.
- **Updates/install:** `tauri-plugin-updater` (GitHub Releases) driven by `updates.rs` + a custom NSIS installer.
- **Plugins:** autostart, window-state, opener, dialog, deep-link + single-instance (held on 2.4.x).
- **Accounts (optional):** Better Auth on a Cloudflare Worker + D1 (`cloud/`); session in Credential Manager.
- **MCP:** `rmcp` (`yap.exe mcp`); `toml_edit` for Codex/ChatGPT's config.
- **Calendar:** Google OAuth (PKCE, loopback) or private iCal links; `rrule` + `chrono-tz`.
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
        the audio callback); emits           AI cleanup pass (llm.rs, if enabled)
        "yap-amp" peak for the                    │
        scrolling waveform                   apply_dictionary()
                    │                              │
            "yap-state" drives the        text_injector (clipboard paste [+ Enter])
            overlay + tray                        │
                                            emits "yap-transcript"
```

After transcription: **AI cleanup → dictionary (exact, then fuzzy near-misses) →
the "press enter" voice command → snippets → fit to the kind of app and the text
around the cursor → inject → submit key, or the correction watch**. Cleanup is
best-effort — any error/timeout falls back to the raw transcript, so dictation never
blocks. The full order and its rules: [dictation.md](./docs/architecture/dictation.md#the-pipeline-order).

---

## Module index (`src-tauri/src/`)

**Dictation** — [dictation.md](./docs/architecture/dictation.md)
- `pipeline.rs` — the heart: buffer and pre-roll, modes, the start chime on the mic's first audio, the model loaded off the startup thread, `run_stt` → `deliver`, Undo, chat/assistant routing; the `processing` guard (`Busy`) let go however a task ends, a crash said.
- `gesture.rs` — push-to-talk's pure state machine: hold, double-tap hands-free, lone tap.
- `coaching.rs` — push-to-talk coaching hints (≤ 3 each; counts in `hints.json`).
- `input_hook.rs` — low-level keyboard/mouse hooks: seven bindings (Read aloud's the fifth, Use what I said's the sixth, Snip to Chat's the seventh) + up to 12 saved transforms' slots, combo rules, `clashes`, menu-mask key; Esc while a snip is up.
- `transforms.rs` — saved transforms: one-key rewrites (Polish, Shorter, Fix grammar only, Translate, Make it a prompt, your own) of the selection or the last dictation, no speaking; their shortcuts, the tray's Transforms menu, the bar's Undo.
- `partials.rs` / `agent_detect.rs` — live partials' sliding window; Commands by name (see `pipeline.rs`).
- `mic.rs` — the dictation mic, opened on demand: held by a dictation / the mic check / "Keep the microphone ready", warm a minute after, closed at once on Bluetooth (HFP); its own thread.
- `mic_watch.rs` — dead-mic watchdog: reopen the stream, then "No sound from <mic>".
- `mic_privacy.rs` — reads Windows' microphone privacy switches; names the one blocking Yap.
- `stt.rs` — `SttEngine` over transcribe-rs, the 14-model registry, download + verify, word times; each engine loads on a `Device` (graphics card or processor).
- `stt_speed.rs` — the speech engine's device on this PC: a one-time background benchmark per model and PC at three dictation lengths (new driver, new download → again); everyday lengths (3 and 11 s) decide: the graphics card unless the processor takes ≥ 20 % less time at both; Settings' "Runs on" says what it chose and why.
- `recent_audio.rs` — a day of dictation audio (`recent-audio/`): failed dictations listed, Home's Run again and Play.
- `media.rs` — Upload's audio-file decoding (Symphonia) and chunking.
- `sound.rs` — start/stop chimes; the output device shared with spoken replies.
- `mute.rs` / `media_pause.rs` — silence other apps / pause media while dictating; exact restore.
- `history.rs` — local dictation history (with its audio's id and the words as said when cleanup changed them; failed / not pasted) and the stats dashboard.

**Pasting** — [pasting.md](./docs/architecture/pasting.md)
- `text_injector.rs` — private, verified clipboard paste with each app's paste keys, a lost paste typed, submit key, re-focus, Paste last transcript (and as said).
- `as_said.rs` — the last paste: "Use what I said" swaps a cleaned paste for the words as said (shortcut, bar, tray, Home's Paste what I said), and the rewrite shortcut with nothing selected rewrites the last dictation (≤ 1 min, still ending at the cursor; Settings → Commands can turn it off) — only where it can select exactly what it pasted; else copied or refused.
- `elevated.rs` — apps running as administrator: the text is left on the clipboard.
- `selection.rs` — UI Automation reads: text around the cursor, edit-mode selection, read aloud's selection (read-only text too), post-paste watch; did a paste land.
- `smart_insert.rs` — fits a dictation to the text around the cursor (case, spaces, full stop).
- `app_kind.rs` — terminal (which one, its paste keys; an editor's terminal) / coding CLI / chat app / other, and what each changes.

**Dictionary** — [dictionary.md](./docs/architecture/dictionary.md)
- `fuzzy.rs` — near-miss correction (Levenshtein + Soundex), prompt-echo guard, sound distances.
- `dict_usage.rs` — how often each dictionary word is used; orders the Whisper prompt.
- `learn.rs` — "Suggest words from your corrections": the 60 s field watch and the offer.
- `screen_terms.rs` (+ `screen_terms/`) — "Spell names the way they're written on screen": the target window's visible text read at record start (UIA, bounded) → its names and terms, for that dictation only: a strict near-miss pass (every model), cleanup's "Spell these names as written" line.

**AI models** — [ai-models.md](./docs/architecture/ai-models.md)
- `about_you.rs` — About you: one Region (Windows' country, or one picked; money, units, language and spelling follow it, the clock is the PC's), name and note → the block every model's prompt ends with, searches and prices aimed at the country, the web's temperatures in their scale first, cleanup spelling.
- `llm.rs` — OpenAI-compatible client, immutable base prompts, chat/tool calls, rate-limit waits.
- `local_llm.rs` — on-device llamafile sidecar: models, recommendation, server args (MTP), context, restarts, warm-up, the runtime's background upgrade (retry without MTP, the way back, launch repair, cross-Yap lock).
- `gguf.rs` — read-only GGUF headers and memory estimates per context length.
- `gpu.rs` — the graphics card (DXGI), for the recommended on-device model.
- `ai_model.rs` — which model a scope's next call goes to; the Chat picker's switch.
- `model_catalog.rs` — providers' live model lists (and self-hosted servers'), cached a day; which read images, where they say.
- `usage.rs` — daily Groq token/request usage.
- `eval.rs` (+ `eval/`) — model evaluations (`yap.exe eval`) through the real Chat code.

**Chat and the Voice Assistant** — [chat-and-assistant.md](./docs/architecture/chat-and-assistant.md)
- `chats.rs` — conversations (`chats.json`), tool chips, sources; a question's screenshots (ids; deleted with it); the history a model gets (earlier searches replayed) and the checks on every answer (made-up sources, claimed searches, unsourced citations).
- `tools.rs` / `note_text.rs` — the tool loop: note tools (safe partial edits), apps, web (a local model must search first), safety rule.
- `stream.rs` — streaming answers: SSE from every provider (tool calls reassembled and checked, a whole answer when streaming fails), a step's text held until it's an answer (a forced search's never shown), ■ Stop, a newer question in the conversation replacing the turn still answering, a local stream stepping aside for a dictation; the checked answer replaces what streamed.
- `assistant.rs` — the Voice Assistant: ask Chat out loud from any app, spoken from the answer's first sentence; off during meetings.
- `pause.rs` — "Stop listening when I pause": a 1.5 s pause ends a Voice Assistant question (the mic's level against a learned noise floor, checked by the live preview's words; never before speech, never under a held key); pure.

**Snip to Chat** — [snip.md](./docs/architecture/snip.md)
- `snip.rs` — drag a box over any screen, into Chat: Win+Alt+S / ✂ on the bar / tray / Chat's + / saying "snip"; the conversation Chat shows, else a new one; never sent by itself; the `yapsnip` scheme.
- `snip/capture.rs` / `snip/hdr.rs` — every monitor at once on Windows.Graphics.Capture (FP16 ÷ the SDR white level on an HDR monitor, GDI as the fallback); the HDR → sRGB tables.
- `snip/overlay.rs` / `snip/geom.rs` — the native per-monitor overlay (painted cloaked, physical pixels); the box, crop and label maths.
- `snip/images.rs` / `snip/turn.rs` / `snip/vision.rs` — `chat-images/` and "Keep chat screenshots"; a question's `image_url` parts per provider; which models read images and the one-click switch. `snip/dump.rs`: `yap.exe snip-dump` (debug).

**Memory** — [memory.md](./docs/architecture/memory.md)
- `memory.rs` (+ `memory/`) — what Yap remembers about you (`memory.json`): learned from quiet chats and meetings (a model without tools by the careful small path, `memory/small.rs`), sent with questions, remember/forget/search_chats tools, names for dictation, export, Undo.

**Web search and apps** — [web-and-apps.md](./docs/architecture/web-and-apps.md)
- `web.rs` (+ `web/native.rs`) — web search (Exa/Tavily/wikis, or the provider's own), guarded page reads.
- `apps.rs` — opens Start-menu apps on request: index, matching, fast path, consent.

**Spoken replies** — [spoken-replies.md](./docs/architecture/spoken-replies.md)
- `tts.rs` (+ `tts/`) — Kokoro / Windows voices / ElevenLabs (cloud, own key, local voice as fallback), text normalisation, the speaker queue (a read-aloud paced a sentence ahead, its position, pause), Kokoro's download; `tts/marks.rs` word timing (Windows' word cues, ElevenLabs' timestamps, else from Kokoro's audio).
- `read_aloud.rs` — Read aloud, anywhere: the selection in any app (shortcut, 🔊 on the bar, tray), notes and Chat answers read with a karaoke reading card on the Yap bar; ⏮ ⏯ ⏭, speed, Stop, Esc.

**Notes** — [notes.md](./docs/architecture/notes.md)
- `notes.rs` — the notes store (`notes.json`), folders, the Actions engine, transcript fields.
- `note_edits.rs` — notes you can fix: a transcript line's words (marked edited, kept through Transcribe again, offered to the dictionary), the summary's words; a summary run never writes over the person's unasked ("kept").
- `tasks.rs` — the summary's checkboxes: ticked in place (`- [x]`), carried into a new summary, listed across notes for the Tasks view ("Said at" the line), read only over the local API / MCP.

**Meetings** — [meetings.md](./docs/architecture/meetings.md) (guide: [`docs/meetings.md`](./docs/meetings.md))
- `meeting.rs` — the recorder: mic + loopback, chunking, echo, dictations kept out, one clock; `replay` from disk.
- `meeting_audio.rs` (+ `meeting_audio/`) — the meeting's audio kept on disk as it records (own FLAC encoder, 14 days): crash recovery, Transcribe again (`redo.rs`: notes, summary, names on matching lines kept), ▶ a line.
- `meeting_summary.rs` — rolling digests, bounded inputs, action-plan checks, helper inputs.
- `meeting_end.rs` — a recording's end: "Started by mistake?", AI title, the action-plan job.
- `meeting_assist.rs` — the notepad's AI: What did I miss?, one-tap helpers, the AI title.
- `notepad.rs` — the meeting notepad window: docking, split screen and its preview.

**Speakers** — [speakers.md](./docs/architecture/speakers.md)
- `diarize.rs` (+ `diarize/`) — the beta speaker engine: the `yap.exe diarize` child, its wire, models.
- `speakers.rs` (+ `speakers/labels.rs`) — the speaker map, names on lines, lines split by voice.
- `speakers/naming.rs` — who each voice is: names suggested from what's said and the invite (+ the AI's checked answers), "Speaker 2 is Priya?" cards, "It was me".
- `speakers/call_ui.rs` — who the call app shows talking (experimental, off): UI Automation reads on the meeting's clock; `yap.exe call-dump`.

**Call detection and guard rails** — [call-detection-and-guards.md](./docs/architecture/call-detection-and-guards.md)
- `meeting_detect.rs` — notices calls (mic consent store), prompts, per-app choice, fatigue, tray item.
- `app_icons.rs` — the call apps' own installed icons, cached.
- `meeting_guard.rs` — length limit, quiet meetings, check-ins, the meeting shortcut, notices.
- `capture.rs` — hides meeting windows from screen capture while recording; every Yap window out of a snip's capture.

**Calendar** — [calendar.md](./docs/architecture/calendar.md) (guide: [`docs/calendar.md`](./docs/calendar.md))
- `calendar.rs` (+ `calendar/`) — Google/iCal connections, sync, parsing, reminder cards, notes tie-in, pre-meeting briefs (`calendar/brief.rs`: last time, open items, memory, the agenda, every line cited; the AI's lines checked).

**Yap bar** — [yap-bar.md](./docs/architecture/yap-bar.md)
- `bar.rs` — the bar: regions, click-through, never focused, placement, one card at a time.
- `overlay.rs` — the bar window's Win32 side and its pure placement / hit-test maths.

**Notices** — [notices.md](./docs/architecture/notices.md)
- `notice.rs` — the notice policy: one place, priorities, holds, cooldowns, call-prompt fatigue.
- `alert.rs` — one-off notices to act on (paste blocked, mic blocked…), one button.
- `win_toast.rs` — Windows notification plumbing (WinRT), banners and quiet posts.

**Integrations** — [integrations.md](./docs/architecture/integrations.md)
- `bridge.rs` — the token-authenticated local API (`~/.yap/cli-bridge.json`).
- `mcp.rs` — `yap.exe mcp`: Yap's MCP server over the local API (meetings + notes only).
- `mcp_clients.rs` — "Add to Claude / ChatGPT / …": edits AI apps' configs safely.

**Accounts** — [accounts.md](./docs/architecture/accounts.md)
- `auth.rs` — optional accounts: browser/email/phone sign-in, the session, where you're signed in.

**Network receipts, Report a problem** — [privacy-receipts.md](./docs/architecture/privacy-receipts.md)
- `net.rs` — every request through one place (`.send_for(Purpose)`, the feature from `net::within`): host, purpose, status, bytes, never content; on this PC / your network / internet; `connections.json` (7 days, folded per 10 min); the updater, the Local API and the account picture by hand.
- `diagnostics.rs` — Report a problem: a .zip shown before it's saved (system, an allow-listed settings summary, connections, errors, 3 days of `info`+ logs scrubbed); Save dialog, never uploaded; GitHub's new-issue page.

**Updates** — [updates-and-release.md](./docs/architecture/updates-and-release.md)
- `updates.rs` — background update checks and downloads, announcements, safe install.

**App shell, config, tests** — [app-shell.md](./docs/architecture/app-shell.md), [config-and-data.md](./docs/architecture/config-and-data.md), [build-and-test.md](./docs/architecture/build-and-test.md)
- `main.rs` — entry; `yap.exe mcp` / `diarize` / `eval` / `call-dump` / `snip-dump` run before the app starts.
- `lib.rs` — Tauri setup: plugins, hook + pipeline, tray, shutdown cleanup, DWM-cloak init.
- `tray.rs` — tray icon and menu (models, Paste last transcript, Read selection aloud, Snip to Chat, Record this call, updates).
- `context_menu.rs` — WebView2's context menu cut to editing commands (Inspect: debug + Shift).
- `procs.rs` — process lineage; the kill-on-close job so helpers never outlive Yap.
- `webview_memory.rs` — hidden windows' webviews get WebView2's low memory target; back to normal on screen.
- `theme.rs` — the theme on the Rust side: the opaque windows' webview backgrounds (no white flash), onboarding's title bar, Windows' app mode (frontend.md).
- `portable.rs` — portable mode (a `portable` marker → `<exe>/Data`).
- `commands.rs` — the Tauri commands; logging is `lib.rs init_logging` (app-shell.md).
- `config.rs` — `YapConfig`, load/save (one at a time; `update` in one step; an unreadable file set aside, the person told), migrations, dictionary helpers (config-and-data.md).
- `e2e.rs` / `realtest.rs` — test mode, and robot mode's real pastes into the test's own windows.
- `log_privacy.rs` — a test: no user content in the log at `info` and above (app-shell.md, Logging).

## Frontend index (`src/`)

[frontend.md](./docs/architecture/frontend.md) unless noted.
- Theme — `--yap-*` / `--yap-paper-*` tokens in `src/app.css`, warm light and a warm dark (`[data-yap-theme='dark']`, from the bar's dark scheme; never hard-code a colour), Figtree + EB Garamond; mark `src/assets/yap-logo.svg` ([frontend.md](./docs/architecture/frontend.md#the-dark-theme-2026-10-22)).
- `lib/appearance.js` — General → Appearance in every window: Theme (System / Light / Dark → `data-yap-theme`, System follows Windows live; the bar's Match app → `data-bar-theme`; `index.html`'s head script puts it on before the first paint), reading font/size tokens (`--yap-font-reading`, `--yap-reading-scale`) for chats/notes/notepad only, and reduced Motion.
- `lib/ControlPanel.svelte` — the main window (label `settings`): sidebar, Settings modal, title bar.
- `ui/toast.svelte.js` + `ui/noticePolicy.js` — app-wide toasts running the page's notice policy.
- `HomeView` / `InsightsView` / `UploadView` — dictation feed (a dictation's "AI edited · N changes" word diff, `lib/wordDiff.js`, and Paste what I said), stats, audio-file transcription; `lib/appNames.js` (apps by their names), `lib/aiErrors.js` (AI failures in plain words).
- `App.svelte` — one chunk per window: each loads only its own view (the Yap bar ~150 KB, not the whole app).
- `DictionaryView.svelte` — corrections and snippets ([dictionary.md](./docs/architecture/dictionary.md)).
- `NotesView.svelte` — the AI Notepad ([notes.md](./docs/architecture/notes.md)); `lib/noteEdits.js` — fixing a line, the summary, ticking tasks (shared with the notepad); `TasksView.svelte` — sidebar Tasks across meetings.
- `lib/markdown.js` — every rendered answer/note: markdown-it + DOMPurify, citation pills ([notes.md](./docs/architecture/notes.md#the-markdown-renderer)).
- `ChatView.svelte` + `ChatModelPicker.svelte` — Chat and its model picker, a snip in the box (`chatImages.js`, `chatShots.svelte.js`; [snip.md](./docs/architecture/snip.md)) ([chat-and-assistant.md](./docs/architecture/chat-and-assistant.md)); `lib/streaming.js` — streamed answers in Chat and the notepad's Ask sheet (the channel, ■, half-written markdown closed off, the checked answer's smooth swap).
- `MeetingsView` / `Notepad` / `SplitPreview` — meetings and the notepad ([meetings.md](./docs/architecture/meetings.md)); `meetingAudio.svelte.js` — a meeting's kept audio in both: Transcribe again, ▶ a line, the crash banner; `BriefView.svelte` + `brief.js` — a meeting's pre-meeting brief everywhere it shows ([calendar.md](./docs/architecture/calendar.md#pre-meeting-briefs)); `SpeakerMenu` / `SpeakerCard` — a speaker's menu and the name cards ([speakers.md](./docs/architecture/speakers.md)).
- `ConnectCalendarDialog.svelte` — "Connect your calendar" ([calendar.md](./docs/architecture/calendar.md)).
- `IntegrationsView.svelte` — local API and MCP cards ([integrations.md](./docs/architecture/integrations.md)).
- `Overlay.svelte` + `bar/` — the Yap bar's page (🎤 ◉ 🔊 ✂ ^), the reading card `bar/ReadingCard.svelte` ([yap-bar.md](./docs/architecture/yap-bar.md)).
- `settings/ConnectionsView.svelte` (Privacy & data → Network connections) + `settings/ReportProblem.svelte` (About) — [privacy-receipts.md](./docs/architecture/privacy-receipts.md); `lib/connections.js` notes the account picture.
- `Settings.svelte` + `settings/` — Settings, always mounted in the modal: the nav by scope with search (`settingsIndex.js`; old links land via `nav.js` aliases), General / Dictation / Shortcuts / Meetings / Privacy & data / About as page components, one row style in `lib/ui/` ([frontend-settings.md](./docs/architecture/frontend-settings.md)); `AboutYouSettings.svelte` General → Language and region + About you, `RegionSpelling.svelte` Dictation Cleanup's spelling switch, `TransformsSettings.svelte` Commands → Transforms.
- `MemorySection.svelte` + `memory.svelte.js` — Settings → Assistant → Memory and the "Yap remembered…" toast ([memory.md](./docs/architecture/memory.md)).
- `AccountSection.svelte` — Settings → Account ([accounts.md](./docs/architecture/accounts.md)).
- `ModelManager` / `ModelRow`, `Onboarding`, `lib/ui/` — speech models, first run, primitives, right-click menus.
- Window config (`tauri.conf.json`) — [app-shell.md](./docs/architecture/app-shell.md#window-config-src-tauritauriconfjson).

## Gotchas that bite (details in the linked docs)

- When one of Yap's own WebView2 windows has focus the global hook never fires: every window with focusable UI needs the **in-page hotkey fallback** (`input_hook.rs`).
- WebView2 keeps `document.visibilityState === 'visible'` while a window is hidden: window visibility comes from Rust (`updates.rs`).
- Never swap the hidden windows' **DWM cloak** for "park off-screen, show+hide" (`lib.rs`).
- Never `window.confirm/alert/prompt` in a Yap window — use `ui/ConfirmDialog.svelte` (`frontend.md`).
- The bar window is transparent: no `backdrop-filter`, no big shadows (`yap-bar.md`).
- A **signed-out Yap never contacts the account service** on its own (`auth.rs`).
- No user content in logs at `info`/`warn`/`error` — lengths, counts and ids only; the text at `debug` at most, never keys (`log_privacy.rs` checks; `app-shell.md`, Logging).
- Lock rules in `meeting_detect.rs` and `calendar.rs`: never touch windows/WinRT/the network under their state locks.
- New settings that must change saved values once go in `config::migrate_once` (`config-and-data.md`).

---

## Build, run & feature flags (important)

| Feature | Effect |
|---------|--------|
| *(default)* | **STUB** — no `transcribe-rs`. `cargo check` stays fast. The app runs but returns placeholder text. |
| `engines` | Real multi-engine STT: `transcribe-rs` with **whisper-vulkan** + ONNX + **ort-directml**. Whisper runs on the GPU via **Vulkan** (any GPU), ONNX via DirectML; CPU fallback with no GPU. **This is what release + nightly builds use.** |
| `whisper` | Back-compat alias for `engines`. |
| `custom-protocol` | Required for release/standalone builds — embeds the frontend. `tauri build` sets it automatically. |

GPU policy: **universal, no CUDA** (Whisper → Vulkan, ONNX → DirectML); building
`engines` needs the **Vulkan SDK**. Details: [build-and-test.md](./docs/architecture/build-and-test.md).

**Dev:** `scripts/dev.bat` (= `npm run tauri dev -- --features engines`, Vite on
**:51437**, hot-reloads `src/`) behind the **"Yap - Dev"** desktop shortcut; the plain
**"Yap"** shortcut is the *installed* app (nightly channel).
dev.bat first fast-forwards the checkout to `origin/main` (`scripts/dev-update.ps1`)
and stops AI apps' `yap.exe mcp` helpers running from its exe.
So merged work reaches the maintainer's dev build by pushing to main; never
fast-forward that checkout by hand while Yap - Dev runs.

```bash
# real GPU pipeline (default in dev.bat) — requires the Vulkan SDK
npm run tauri dev -- --features engines
# stub (fast, no transcription)
npm run tauri dev
```

Accounts in debug builds: `cd cloud && npm run dev` (wrangler dev on :8787; see `cloud/README.md`).

> ⚠️ A *compiled release build* bakes the frontend into the binary — editing `src/`
> and restarting that `.exe` changes nothing. For live frontend changes use dev
> (Vite on :51437). If :51437 isn't listening, you're looking at a release build.

**CI on every push** (`.github/workflows/ci.yml`): the privacy guard's generic checks
(keys, emails, phone numbers), `npm run build` + `cargo clippy
--locked --all-targets -- -D warnings` on the stub build, then `cargo clippy --locked
--release -- -D warnings` (debug-only code is dead in release: gate it with
`#[cfg(debug_assertions)]`). ⚠ Zero-warnings ratchet on
the **latest stable** toolchain — keep local Rust current (`rustup update`).
`.github/workflows/e2e.yml` runs the e2e suite on the same triggers.

**Tests:**
- `npm run test:app` — the end-to-end UI suite: Playwright drives a real stub debug build over CDP (~20 s + the build), leaving named screenshots in `test-results/app/screenshots/`. Guide: [`docs/e2e-tests.md`](./docs/e2e-tests.md).
- `npm run test:real` — real-app robot tests: a robot-mode Yap pastes into real windows the harness opens (`--engines` build: real STT). ⚠ **It takes the keyboard focus for ~2 min** (local only, not CI): say so before running it while the maintainer is at the PC. Guide: [`docs/e2e-tests.md`](./docs/e2e-tests.md) "Real-app robot tests".
- `npm run privacy:scan` / `npm run test:privacy` — the privacy guard over the whole tree, and its own tests ([build-and-test.md](./docs/architecture/build-and-test.md#the-privacy-guard-scriptsprivacy-guardmjs)).
- `npm run eval` — model evaluations: real models through Yap's own Chat code, every request logged; use it before changing a prompt, a tool or the default local model. Guide: [`docs/model-evals.md`](./docs/model-evals.md).

### The dev → nightly → stable workflow (agreed 2026-10-05)
**Dev first, nightly for batches.** Every change goes through these steps in order:
1. **Claude builds it and tests it.** `npm run build` + `cargo clippy --all-targets
   --locked -- -D warnings` + `cargo test`, then the e2e robot suite (`npm run
   test:app`: drives the real app in ~20 s and leaves screenshots to look at; add or
   extend a spec for anything new). For anything about **pasting, inserting text,
   the clipboard, terminals/chat apps, the mic path or STT**, also the real-app
   robot tests (`npm run test:real`, + `npm run test:real:build -- --engines` for
   real STT): they paste into real windows, so Claude checks it itself instead of
   handing the maintainer a checklist (they take the keyboard focus ~1.5 min). Push
   to main: CI and the E2E workflow re-check every push (and the pre-push privacy
   guard every push from here).
2. **The maintainer tries it in the dev build** ("Yap - Dev" desktop shortcut =
   `scripts\dev.bat`, the real GPU pipeline from this checkout). Anything visual or
   about how it feels gets checked HERE: Vite hot-reloads `src/` edits in seconds;
   Rust edits need the dev build restarted (a minute or two to recompile). Something
   off → Claude fixes it → it shows straight away. Never burn a 15-min nightly to
   check something dev shows in seconds. Hand it over with exact steps (the private
   `docs/maintainer.md` says how).
3. **Nightly, once a batch feels right.** Claude dispatches it (or the 05:00 UTC
   cron picks up main) and the maintainer uses it day to day on the installed app.
   Nightlies catch what only the installed release build can show: the updater
   flow, the installer, Windows notifications under Yap's own name (dev builds
   borrow PowerShell's), `com.contextmirror.yap://` deep links, start on login.
   Those go straight to a nightly check; everything else goes through dev first.
4. **Stable** — tag `v*` deliberately for curated milestones, when the maintainer
   is happy.

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

Everything lives in `%APPDATA%/yap/` (portable: `<exe>/Data/`): `config.json`,
`models/` (speech; `diarization/`, `tts/`), `llm/` (on-device GGUFs), `history.json`,
`notes.json`, `chats.json`, `calendar.json`, `updates.json`, `model_catalog.json`,
`groq_usage.json`, `dictionary_usage.json`, `hints.json`, `word_suggestions.json`,
`notices.json`, `memory.json` (what Yap remembers about you), `chat-images/` (Chat's screenshots, Snip to Chat), `connections.json` (a week of network receipts: hosts and purposes, never content), `quieted_apps.json`
(only mid-dictation), `stt_speed.json` (each model's device timings),
`recent-audio/` (a day of dictation audio), `meeting-audio/` (each meeting's audio as
FLAC, 14 days), `icons/`, `logs/`. Every JSON
store writes atomically and quarantines a corrupt file instead of crashing —
config.json too: set aside, never saved over, the person told; its saves one at a time
(`config::update`; [config-and-data.md](./docs/architecture/config-and-data.md)). Secrets
never go in files: the account session and calendar tokens live in Windows Credential
Manager. The local API's discovery file is `~/.yap/cli-bridge.json`.

Notable defaults: F9 in toggle mode, **Parakeet TDT 0.6B v3** on the device measured fastest here, AI cleanup off,
smart spacing / terminal mode / no full stop in chats on, the Yap bar on, Win+Alt+S snips to Chat (screenshots kept 30 days), call
detection on (it only asks), a 2-hour meeting limit, Win+Alt+M, meeting audio kept 14 days,
web search and spoken replies off. Every setting, file and default: [config-and-data.md](./docs/architecture/config-and-data.md).

---

## Current status

- **Dictation is complete and real**: local transcription (Whisper/Vulkan + ONNX/DirectML, or the processor where that's faster on the PC), push-to-talk, AI cleanup (cloud or on-device Qwen) with "Use what I said", Commands, saved transforms on one key (Polish, Translate…), smart pasting into terminals and chat apps, dictionary learning, Undo, a day of audio to run a dictation again.
- **Meetings**: recorder (You/Them), its audio kept on disk (crash recovery, Transcribe again, ▶ a line), rolling digests + action plan, the notepad, call detection, guard rails, the calendar, beta speaker separation.
- **AI**: Chat with note tools, the Voice Assistant, spoken replies (Kokoro, or ElevenLabs on your key), Read aloud anywhere (karaoke card on the bar), web search, opening apps, memory (learned from chats, kept on this PC, names fed to dictation), Snip to Chat (a screenshot of any screen into Chat, HDR right; the overlay up ~50–60 ms after Win+Alt+S).
- **Around it**: MCP server + local API, optional accounts (live at `auth.contextmirror.com`), installer + auto-updater on stable and nightly, network receipts (every connection, by purpose) and Report a problem (a diagnostic file you check, never uploaded).
- Much is unit-, e2e- and robot-tested but not yet tried in real use (a real call, real AI apps, answers heard on speakers…).
- **Not yet done**: Chat's semantic search, a rich notes editor, the call app's speaker names checked in a real call, one-click Outlook, a streaming STT model, Authenticode signing, non-Windows.

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
new line). Keep CLAUDE.md short: detail goes in the architecture docs, not here.
