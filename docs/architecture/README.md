# Yap architecture

How Yap works, area by area. This is the detail that used to be in
[CLAUDE.md](../../CLAUDE.md), moved here verbatim on 2026-10-21 so that file
stays short; CLAUDE.md keeps the rules, the build/test/release workflow and a
one-line index of every module pointing here. Read the file for the area you're
touching before changing it, and when a feature lands, update its file here (and
its one line in CLAUDE.md's module index).

## The files

- [dictation.md](./dictation.md) — the pipeline order, `pipeline.rs`, push-to-talk gestures and coaching, the input hook, the mic (dead-mic watchdog, Windows' privacy switches), STT and which device each model runs on (`stt_speed.rs`), Upload's decoding, chimes, other apps' sound while dictating, history and a day of dictation audio to run one again (`recent_audio.rs`).
- [pasting.md](./pasting.md) — the paste and the submit key, apps running as administrator, UI Automation reads of the field, smart spacing, the kind of app (terminals, coding CLIs, chat apps), the last paste: "Use what I said" and rewriting the last dictation (`as_said.rs`).
- [dictionary.md](./dictionary.md) — near-miss (fuzzy) correction, the vocabulary's order, learning words from corrections, the Dictionary view.
- [ai-models.md](./ai-models.md) — what every model is told about the person (About you), the LLM client and its prompts, the on-device model (llamafile, GGUF headers, the graphics card, context length), which model a call goes to, the providers' live model lists, Groq usage, the model evaluations.
- [chat-and-assistant.md](./chat-and-assistant.md) — chats, the tool loop and its note tools, the Voice Assistant, the Chat view and its model picker.
- [snip.md](./snip.md) — Snip to Chat: capturing every monitor (HDR right), the native overlay, chat screenshots on disk and how long they're kept, which models read images and the switch, a question's images on their way.
- [memory.md](./memory.md) — what Yap remembers about you: the store, learning from chats and meetings, what a question gets, the memory tools, names for dictation, Settings → Memory.
- [web-and-apps.md](./web-and-apps.md) — web search (Yap's own and the cloud models' own), reading pages, opening apps.
- [spoken-replies.md](./spoken-replies.md) — reading answers aloud: Kokoro, Windows' voices, text prepared for speech, the speaker; Read aloud, anywhere (`read_aloud.rs`) and its word timing.
- [notes.md](./notes.md) — the notes store, folders, the Actions engine, the Notes view; notes you can fix (lines, the summary, tasks) and the Tasks view.
- [meetings.md](./meetings.md) — the recorder, its audio kept on disk (crash recovery, Transcribe again, ▶ a line), digests and the action plan, the end of a meeting, the notepad's AI helpers, the meeting notepad window, the Meetings view.
- [speakers.md](./speakers.md) — the speaker engine (diarization, beta) and the speaker map: who said what.
- [call-detection-and-guards.md](./call-detection-and-guards.md) — call detection, the call apps' icons, a recording's guard rails, hiding meeting windows from screen capture.
- [calendar.md](./calendar.md) — calendar connections, sync, the reminder card, the "Connect your calendar" dialog.
- [yap-bar.md](./yap-bar.md) — the Yap bar: its window, its Win32 side and its page (the dictation capsule, cards).
- [notices.md](./notices.md) — the notice policy, alerts, Windows notifications.
- [integrations.md](./integrations.md) — the local API, the MCP server, adding Yap to AI apps, the Integrations view.
- [accounts.md](./accounts.md) — optional accounts: sign-in, sessions, Settings → Account.
- [privacy-receipts.md](./privacy-receipts.md) — network receipts (every connection Yap makes, by host and purpose: `net.rs`, Settings → Privacy & data → Network connections) and Report a problem (a diagnostic file checked before it's saved: `diagnostics.rs`, Settings → About).
- [updates-and-release.md](./updates-and-release.md) — the updater, the installer, the stable and nightly channels, cutting a nightly.
- [frontend.md](./frontend.md) — theme, window chrome, the ControlPanel and its toasts, Home / Insights / Upload, the speech model browser, onboarding, UI primitives and right-click menus.
- [frontend-settings.md](./frontend-settings.md) — Settings, page by page.
- [app-shell.md](./app-shell.md) — the full stack list, `lib.rs`, the tray, context menus, portable mode, logging, the Tauri commands, the window config.
- [config-and-data.md](./config-and-data.md) — every setting (`config.rs`), the data files, the notable defaults.
- [build-and-test.md](./build-and-test.md) — feature flags, running in dev, CI, the end-to-end UI suite, the real-app robot tests, test mode and robot mode.
- [status.md](./status.md) — what works today, how it was tested, and what's not done yet.

The behaviour guides beside this folder go deeper on a few areas:
[`docs/meetings.md`](../meetings.md), [`docs/calendar.md`](../calendar.md),
[`docs/e2e-tests.md`](../e2e-tests.md), [`docs/model-evals.md`](../model-evals.md),
[`docs/fuzzy-dictionary.md`](../fuzzy-dictionary.md),
[`docs/local-api.md`](../local-api.md), [`docs/qr-sign-in.md`](../qr-sign-in.md),
[`docs/SIGNING.md`](../SIGNING.md).

Paths in these files named `ROADMAP.md`, `docs/wispr-flow-features.md`,
`docs/openwhispr-*.md` or another research/plan file mean the **private** repo
`nayballs/yap-private` (see CLAUDE.md), never this one.

## Background notes (from CLAUDE.md)

> Origin note: the core dictation plumbing (input hook, text injection) was ported
> from "Voice Mirror"; the multi-engine STT, AI cleanup, settings, tray, overlay,
> installer and the rest is Yap's own.

> **Mission — the best-of-everything blend.** Yap's strategy is to take the best of
> every top-tier dictation app — **superwhisper**, **OpenWhispr**, **Wispr Flow**,
> **Handy**, **Aqua**, **FluidVoice** — and combine them into one Windows-first,
> local-first app. We port proven patterns **from source**, not from screenshots:
> **OpenWhispr is cloned beside this repo, at `../references/openwhispr`** (Handy at
> `../references/Handy`) — when working on a feature one of these apps does well, read
> its actual implementation first. See ROADMAP.md "North star" +
> `docs/openwhispr-teardown.md` + `docs/openwhispr-parity.md`. Every Wispr Flow
> feature with Yap's status, as a checklist with a "Next up" list:
> `docs/wispr-flow-features.md`.

> Active work note: the Settings / Language-Models / Prompt-Studio surface is being
> reshaped to track **OpenWhispr** (`../references/openwhispr`) as the design
> reference — porting its UX patterns and wording while keeping Yap's own backend
> contract (split immutable-guardrails + editable body). Treat that repo as the
> source of truth for this redesign pass.
