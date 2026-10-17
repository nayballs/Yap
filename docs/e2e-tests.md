# End-to-end UI tests

`npm run test:app` launches a **real Yap build**, clicks through it the way a
person would, checks the results (on screen and in Yap's data files) and saves
a **named screenshot** at each key step. Any uncaught exception or unhandled
promise rejection in any of Yap's webviews fails the test that was running.

It's [Playwright](https://playwright.dev) driving the app over the Chrome
DevTools Protocol that WebView2 exposes. Every Yap window (the main window,
onboarding, the meeting notepad, the overlay) is a Playwright page. The build is the default
**stub** build (no `engines` feature: transcription returns
`[STT stub: received 1.5s of audio, engine=parakeet]`), so it needs no GPU,
Vulkan SDK or model. The suite takes about 20 seconds, plus the build.

## Running it

```bash
npm run test:app                       # build the test app, then run the suite
npm run test:app:build                 # only the build
npx playwright test -c e2e             # only the suite, against the last build
npx playwright test -c e2e -g notes    # tests whose title matches "notes"
npx playwright show-report test-results/app/report
```

The first build compiles the stub app from scratch (a few minutes); after that
it's an incremental build of seconds. It lands in its own Cargo target dir,
`src-tauri/target/e2e` (override with `YAP_E2E_TARGET_DIR`; `YAP_E2E_EXE`
points the suite at an exe directly), so it never competes with a running
`tauri dev` for its target dir.

**Safe next to your own Yap**, installed or dev. Each test instance:

- runs in **portable mode** from its own folder (`test-results/app/runs/…`):
  config, notes, history and logs live in that folder's `Data/`, never in
  `%APPDATA%\yap`;
- has its own WebView2 profile and DevTools port (both picked fresh), and a
  `config.json` seeded with hotkey F24, no autostart, no update checks, no
  local API bridge and no sound;
- runs in **test mode** (`YAP_E2E=1`, honoured by debug builds only, see
  `src-tauri/src/e2e.rs`): no global keyboard/mouse hook, nothing pasted,
  typed or copied into other apps (a dictation still reaches history), no
  other app's sound silenced or paused, no
  reading the text around another app's cursor or its window's title, no
  focus stealing, no Windows notifications, no killing "orphaned" sidecars,
  and the installed app's saved window position is neither read nor written.
  A meeting recording opens no audio device: it plays `you.wav`/`them.wav`
  from `YAP_E2E_MEETING_AUDIO` (`YAP_E2E_MEETING_SPEED` times real time) or
  stays silent, and the `e2e_meeting_feed` command hands it transcript
  segments directly. `e2e_meeting_dictation`, `e2e_meeting_output_change`
  and `e2e_meeting_quiet` stage a hotkey dictation and a switch of output
  device at a point of that audio, and shorten the quiet-side warning's
  timings, without any microphone or speakers; `e2e_meeting_checkins`
  shortens the quiet-meeting countdown's and the check-ins' timings and
  keeps the player's "mic" delivering digital silence meanwhile (so the
  meeting's audio clock runs, as with a real mic). For dictation,
  `e2e_dictation_mic { silent }` zeroes the real microphone's audio (a
  "muted" mic for the dead-mic watchdog), `e2e_coaching_reset` makes every
  push-to-talk hint due again, `e2e_mic_privacy { blocked }` stands in for
  Windows' microphone privacy switches (a test run reads no registry) and
  `e2e_paste_elevated { on }` makes every paste target count as an app
  running as administrator (a test run reads no other process). The correction watch after a paste
  (`learn.rs`) never reads another app's field in a test run:
  `e2e_learn_correction { pasted, edited, fresh }` hands Yap the edit it
  would have seen, and the rules, the prompt and the dictionary are real. It reads no installed apps'
  icons (Settings' call apps, the bar's call card): a spec that wants one
  to look installed points `YAP_E2E_APP_ICONS` at a folder of
  `<app id>.png` stand-ins (`support/icons.js` draws them). It quits when
  the suite closes its stdin, including when the suite dies;
- talks to an account service that isn't there (`YAP_AUTH_URL` points at a
  closed local port), so it's signed out and never touches your saved session;
- keeps the local API's discovery file in its run folder (`YAP_BRIDGE_FILE`,
  never `~/.yap/cli-bridge.json`) and treats `runs/<name>/ai-apps/` as the
  user profile when Settings → MCP edits AI apps' config files
  (`YAP_MCP_CLIENT_ROOT`): a run never adds Yap to your real Claude, Cursor…

A Yap window (1200×800) shows on screen while the suite runs, plus the
onboarding window for a moment, the Yap bar (the `overlay` window, a small
pill above the taskbar of the primary monitor), and in the meeting specs the
meeting notepad, docked to the right edge of the screen. They never take the
focus, and Playwright's clicks and keys go to the webviews, not to your mouse
or keyboard. (Don't click on them while the suite runs: a real click on the
notepad's Stop, say, ends a test's meeting.) A test run's bar never reads
your cursor or the window in front of you: `bar_simulate` stands in.

## Where the results go

Everything lands in `test-results/app/` (git-ignored, cleared at the start of
each run):

| Path | What |
|------|------|
| `screenshots/<spec>/<NN-name>.png` | the named screenshots, in test order |
| `report/` | Playwright's HTML report, screenshots attached to each test |
| `playwright/` | failure details: error context and `trace.zip` (open with `npx playwright show-trace`) |
| `runs/<spec>-w<N>/` | each test instance: `stdout.log` plus `app/Data/` (its config, notes, history and `logs/yap.log.*`) |
| `fixtures/app-icons-<spec>/` | the stand-in "installed" app icons a spec hands Yap (rewritten each run) |

## CI

`.github/workflows/e2e.yml` runs the suite on every push and PR to `main`, and
on demand (`gh workflow run e2e.yml --repo nayballs/Yap`). It builds the test
app, runs the suite and uploads `test-results/app` as the **`e2e-results`**
artifact on every run, pass or fail (about 3 minutes with a warm Rust cache).
The runner has no audio devices, so the run also proves Yap starts and works
without a microphone (the stub dictation test is skipped there), and its
screen is 1024×768, so CI screenshots are a little smaller than local ones.

Hosted runners run everything elevated, and WebView2 ignores an elevated
app's `WEBVIEW2_*` variables (Microsoft's
[security notes](https://learn.microsoft.com/microsoft-edge/webview2/concepts/security#for-an-elevated-host-app-use-appropriate-override-flags)),
so the workflow sets the DevTools flag as a machine-wide WebView2 policy for
`yap.exe` instead. The suite reads the port WebView2 picked from
`DevToolsActivePort` in the instance's profile either way.

To review a run's screenshots and logs:

```bash
gh run list --workflow=e2e.yml --repo nayballs/Yap --limit 5
gh run download <run-id> --repo nayballs/Yap --name e2e-results --dir e2e-results-<run-id>
```

then open `screenshots/` (Claude can read the PNGs directly) or
`report/index.html`.

## What's covered

| Spec | Instance | Tests |
|------|----------|-------|
| `e2e/app.spec.js` | default | Home renders in portable mode · every sidebar view · every Settings section · a setting saved to `config.json` and read back after a reload · a new note survives a view switch · a dictionary entry is saved · a snippet added (multi-line text), kept across a view switch, edited, still a snippet after a Settings auto-save, removed · "Say “press enter” to send" on by default, saved off and on, the key picker with it · Account signed out with the service unreachable · onboarding opens on its first step · a stub dictation lands in the Home feed (skipped without a microphone). (What snippets and the voice command do to a dictation is unit-tested: the stub's text can't say them.) |
| `e2e/paste.spec.js` | seeded history (two dictations, then an Upload) | "Paste last transcript" (`paste_last_transcript`) returns the newest dictation — never the newer Upload — and pastes nothing in a test run (the log says so); after a deleted entry, the one before; after Clear history, "Nothing to paste yet" · Settings → Advanced → Output: "Match the text around the cursor" on by default, "Append trailing space" saying it's only for fields Yap can't read, the Win+V line, and switching it off and on saved to `config.json` · "Leave the full stop off chat messages" and "Safer pasting into terminals" on by default, auto-submit's line saying where it doesn't press Enter, each switched off and on saved to `config.json`. (The paste itself, the clipboard's privacy formats and the reading of real text fields are Rust tests: `smart_insert`, and the `--ignored` `private_clipboard_round_trip` / `reads_real_text_fields`; so are telling terminals, coding CLIs and chat apps apart and what that does to a dictation: `app_kind`, with an `--ignored` `this_machines_windows` over the PC's real windows.) |
| `e2e/push-to-talk.spec.js` | push-to-talk mode | Settings → General's line on double-tap · (with a microphone:) a hold transcribing when the key comes up · a lone tap thrown away (nothing transcribed) with the "Hold F24 while you talk" Tip toast, once (`hints.json`; not again within ten minutes) · a double-tap recording hands-free past the gap, the overlay's lock, the next press transcribing, no hints after · the hint as a card on the Yap bar with the main window away · a "muted" mic (`e2e_dictation_mic`): its stream reopened once at ~3 s, "No sound from … — is it muted?" in the overlay and the full message as a toast at ~5 s, the recording going on, nothing transcribed on stop and the overlay ending on the reason · the sound coming back taking the warning back and the dictation going through |
| `e2e/no-mic.spec.js` | configured mic missing | starts anyway and says "No microphone found" (toast + overlay) when asked to record · with Windows' privacy switch off (`e2e_mic_privacy`), "Windows is blocking microphone access for desktop apps" with Open settings instead · Upload still transcribes a file |
| `e2e/dictation-alerts.spec.js` | seeded history (a dictation) | a dictation's alerts (`alert.rs`): an app running as administrator (`e2e_paste_elevated`) — "Paste last transcript" and a dictation (with a microphone) say "Your text is copied: press Ctrl+V" as a toast, or as a Yap bar card with the main window away, nothing pasted, history kept · Windows' microphone privacy (`e2e_mic_privacy`, with a microphone): said when a recording starts (the overlay's line and a toast naming the switch), no "No sound from…" after it, Open settings (logged, never opened, in test mode), nothing transcribed on stop · the switch turned off mid-recording: the dead-mic watchdog blames it, on a bar card with the main window away |
| `e2e/bar.spec.js` | default | the Yap bar (`yap.overlay`), through the debug-only `bar_simulate` (a pretend cursor on a region, fake fullscreen kinds, Esc) and `bar_debug`: idle, bottom-centre on the work area, click-through and never activated — its ex-styles read inside the app and from outside it (`support/win32.js`: GetWindowLongPtr, and WindowFromPoint on the pill showing a click would go through) — and Wispr's measured pill (40 × 8, 14 px up, a 60 × 20 hit wrapper, its colours; `flowbar-spec.md`) · the cursor on the pill opens it and makes just that clickable, then click-through again; tooltips with the shortcut, in Wispr's tooltip style · a notice card (the update card, as a demo) to Wispr's card measurements, 26 px above the pill, with its hit strip · its menu: hide for an hour (Settings shows it, Show it now) and Turn off the bar (saved; Settings adopts it and turns it back on) · Settings → General → Yap bar · a call prompt as a "Meeting detected" card (its ^ menu, ✕, an installed app's own icon — a stand-in for Teams — and Google Meet's mark), answered on the bar · fullscreen: no pill, a card over a borderless app, a card waiting out an exclusive one · ◉ starts meeting notes; the recording pill (69 × 30, half opacity, its ring, 5 bars, the stop circle; hovered, the timer) opens the notepad and ■ ends the meeting · the opt-in countdown (Esc cancels; left alone it records; the call-ended card) · dictating in the pill's place · (with a microphone:) the capsule's ✕ and ✓, a clickable region (`bar_simulate` on "dictation") that never activates the bar; ✓ transcribing as the hotkey would · ✕ → "Dictation cancelled" + Undo, nothing transcribed, Undo transcribing it, once · the tray's Cancel (`cancel_recording`) keeping it too, a new dictation dropping it, Undo gone after 10 s · with the bar off, ✓ still clickable |
| `e2e/updates.spec.js` | pointed at a local `latest.json` | a manual check finds the next patch version, downloads and verifies it, the "ready" toast and Settings → About say so, and "Restart to update" stops short of the installer (debug builds never run it) |
| `e2e/meeting-detect.spec.js` | default | call detection through the debug-only `meeting_detect_simulate` hook: a call offers to take notes and "Not now" leaves it (and the app's next call) alone · a prompt leaves with its call · a prompt left alone fades (its `fadeMs` shortened from 30 s) and counts as "Not now" · "Record notes" creates a Meetings note and records it, the call ending offers "Stop and summarise", which ends it in the note and runs the summary step · "Keep recording" carries the notes into a rejoined huddle, whose end asks again · a Discord call isn't asked about by default · "Don't ask for Teams" confirms, Settings shows Teams off, and switching it back on asks again · "Quietly" keeps the prompt out of the window · Settings lists how Yap asks and every app's effective choice, in two columns, each row with the app's icon (stand-ins for Teams and Discord, `YAP_E2E_APP_ICONS`; the bundled mark or a letter for the rest), lined up and greyed with detection off · the Settings toggle turns it off and greys out the rest |
| `e2e/calendar.spec.js` | a local iCal server (`support/calendar-feed.js`) and a local fake of Google (`support/fake-google.js`, via the debug-only `YAP_GOOGLE_*_URL`) | Settings → Connectors: the three cards, the Outlook guide, a link that was reset, one that works · the Meetings view: today and the next 7 days, Conflict and Maybe, all-day and non-meetings hidden, Show more, a draft note · the reminder card, in the window and on the Yap bar (a call card: Teams' letter badge, "● In 1 min · with Tanay +1"): **Join & take notes** opens the link (recorded, never opened, in test mode) and records into a note with the meeting's name and attendees, the bar's copy goes too, and its simulated call isn't asked about again · Esc dismisses, the bar's ^ menu snoozes (a calendar mark for a meeting without a call app), a snoozed card comes back with its call, the bar's ✕ dismisses everywhere · back to back: a call during the next meeting offers to switch notes (on the bar too), and switching ends the first · Google in one click: consent with PKCE, read-only scopes, meetings listed, the ⋯ menu, Disconnect revokes the token and clears the meetings · the "Connect your calendar" dialog from every Connect calendar (the Meetings view's grey card, Integrations, the nudge after a meeting over the Meetings view): the bar's own card in its band (a picture: nothing to press or read out; its sample Google Meet call with Meet's mark), three ways in, the focus kept inside and given back, Esc / ✕ / the scrim closing it · an iCal link through it: the Outlook steps, Back, a reset link's reason in the dialog, then "Calendar connected · 7 meetings in the next 7 days" and the dialog closing on the listed meetings, no toast as well · Google through it: "Finish connecting in your browser…", Cancel, a refusal shown in the dialog, then connected with its one meeting |
| `e2e/calendar-nudge.spec.js` | Chat pointed at a local fake AI | no calendar yet: one grey "No meetings found" card whose Connect calendar opens the dialog, Continue with Google off in a build without Google's client ("Google sign-in is in the installed Yap…"), and Settings → Connectors saying the same · the "Connect your calendar" nudge once after a meeting ends, and Not now means never · past meeting notes by day, with search, Generate summary (the action plan, in the note) and the Ask bar asking Chat about your meetings · ◉ Take notes in the header starts meeting notes as the meeting shortcut does, and Stop and summarise ends them |
| `e2e/chat-tools.spec.js` | Chat pointed at a local fake AI, scripted call by call | the AI Chat changing part of a note through its tools: `search_in_note` (both lines found, numbered), an `edit_note` whose text is in two places refused (nothing changed; the model told "appears 2 times … (lines 1, 3)"), the edit of the one place, the answer · the note changed there only in notes.json, the meeting notepad showing it following at once · the thread's chips (Found … / Couldn't edit … / Edited "Launch plan": "Friday." → "Monday.") and the same chips when the conversation is opened again · the Notes view showing the edit · the chat never inventing a folder: a `create_note` into a folder that doesn't exist refused (nothing saved, the model given the folders and told to leave `folder` empty), the retry filed in Personal, no new folder |
| `e2e/notes-menus.spec.js` | default | right-click menus in Notes (`ui/ContextMenu.svelte`): a folder renamed in place (Esc keeps the name, a name another folder has refused, Enter renames it in its place with its notes, the open note's chip following) and deleted after a question ("Its 2 notes will move to Personal.", Cancel, Delete: the notes in Personal, the view on Personal; an empty folder) · Personal and Meetings: the items off with the reason, Rust refusing both commands · Shift+F10, arrow keys, Enter and Esc (the focus back on the row), the ⋯ button, a click outside · a note row's menu: Move to ▸ (a submenu, by mouse and by ←/→), Copy as Markdown (the page's clipboard stubbed), Delete… (asked first); a recording meeting's note stops first (`meeting_delete`, the notepad letting go) and `note_delete` refuses it · Yap's rows cancel the `contextmenu` event, plain UI doesn't · WebView2's own menu (`e2e_context_menus`): every window hooked, plain UI's Back/Refresh/Save as/Print dropped (Inspect kept in a debug build), a text field's edit commands kept, and no native menu on screen in test mode |
| `e2e/meeting-detect-no-mic.spec.js` | configured mic missing | "Record notes" says why it can't record and leaves no empty meeting note |
| `e2e/meetings.spec.js` | meeting audio from test WAVs | live You/Them segments, cut in pauses · ending the meeting without an AI model explains how to set one up · the call leaking from the speakers into the mic is flagged as echo, hidden, and kept |
| `e2e/meeting-audio.spec.js` | meeting audio from test WAVs, a correction dictionary | a hotkey dictation mid-meeting (`e2e_meeting_dictation`) leaves no "You" line, a "You dictated here" marker and `dictated: true` in notes.json, the call going on after it · the dictionary on transcribed meeting lines, on lines handed over (a fuzzy near-miss and an exact one) and on an uploaded file, History keeping raw and corrected · a default-output switch (`e2e_meeting_output_change`) followed, the recording going on, "Them" as long as "You" · "Yap hasn't heard the call / your mic" after 6 s (`e2e_meeting_quiet`), the recording going on |
| `e2e/mcp.spec.js` | Local API on, seeded meetings and a dictation, a stand-in user profile with AI apps' configs | Integrations' "Go to MCP" opens Settings → MCP · Wispr's four cards in order (Claude, ChatGPT, Gemini, Cursor) with what Yap found, then All other apps · Add to Claude merges into its config (other servers kept, `.bak` made) and Remove gives back the same bytes · Cursor, ChatGPT (TOML) and Claude Code (under its lock) get their own formats · a config that isn't plain JSON is left alone, with the reason · with the Local API off the page says so and turns it on, and Settings' copy agrees · a real `yap.exe mcp` session (`support/mcp-client.js`) lists, searches and pages meetings, reads notes and folders, and never returns the dictation · "Let AI apps save notes" adds `create_note` and the note shows in Notes · with no Yap, the tools ask to open it |
| `e2e/meeting-summary.spec.js` | Note Formatting pointed at a local fake AI | a two-hour meeting fed in ten-minute batches: 11 rolling digests, each under 4,500 tokens with a capped reply · one final call over digests + the raw tail · an action plan with a section per person, Decisions, Open questions and Unassigned · the deterministic checks (an invented owner, a dropped task, a made-up deadline) · Copy text |
| `e2e/other-apps.spec.js` | an old config with `muteWhileRecording: true` | Settings → General: "Silence other apps while you dictate" carried over (on) where "Mute while recording" was, "Pause music and videos while you dictate" off, both saved under their new names (the old key gone) · a stub dictation with both on touches nothing in a test run ("other apps: left alone in a test run") |
| `e2e/meeting-quiet.spec.js` | a local iCal feed, reminders off | quiet meetings and check-ins, in seconds (debug-only `e2e_meeting_checkins`): "Ending notes in N s…" in the notepad, then "Stopped: the meeting went quiet" · a transcribed line cancelling the countdown, **Keep going**, **Stop now** · with the notepad closed, the countdown as a Yap bar card · "Still in your meeting?" an hour in (seconds), held back while lines keep coming, Keep recording · the calendar check-in after a meeting ran over, Stop and summarise · the Settings switch (off: a quiet recording carries on) |
| `e2e/meeting-guards.spec.js` | default | the meeting guard rails: the overlay and the notepad excluded from screen capture while a meeting records and back after (debug-only `capture_affinity`) · with hiding off, the screen-share tip, **Update settings** at the switch, and switching it back on mid-meeting · the length warning, **Keep going**, and the stop at the limit, in seconds (debug-only `e2e_meeting_limit`) · "When a call ends: Stop and summarise automatically" on a simulated call end · the meeting shortcut (Win+Alt+M through the in-page fallback) starting a "Meeting · …" note and stopping it, and taking notes on a live call · the Settings rows and recording a new shortcut |
| `e2e/notepad.spec.js` | Note Formatting pointed at a local fake AI | the meeting notepad (`yap.notepad`) in Wispr Flow's layout, screenshots named after the states in Wispr's captures: the copy/save/email text and transcript layout (`notepadText.js`, pure) · it opens on record docked to the right edge, full height, without taking the focus, and out of screen captures, with the serif title, the locale's date, the waveform and "+ Summary" · grouped bubbles, echo hidden, the dismissible tip, the empty states · closing it keeps recording and Notes reopens it · a new note's Start · My thoughts and the title synced with Notes both ways · paused and "You dictated here" dividers, hover copy, search, copy all · the Ask sheet: What did I miss? (nothing new without a model call, then only the lines since you looked), 👍 kept locally, copy, a typed question, New chat, Escape · the one-tap helpers (`meeting_assist`): Questions to ask while recording (from the keyboard; talk only, a lean input), then Follow-up email (the fake's markdown and preamble cleaned up, copied as plain text with its subject line first) and Gaps and risks reading the action plan, the chips switching with the recording state and stepping aside while a question is typed, the same chips in the Notes view, and without an AI model the setup card's link to Language Models · the header: the ⋯ menu recording and after, copy as Markdown/text, Audio settings, Share and its one-click copy, the split tooltip (test mode refuses to split), back to Notes, Delete (its dialog; a recording one stops first) · the consent message (copy, an edit saved on blur and kept by Settings, ⋯ → Copy consent message, back to the default) · the AI title replacing call detection's made-up one, never a typed one or a calendar meeting's name (a local iCal feed), and the stopped state · Started by mistake? (Keep, Generate summary, Discard; on the Yap bar with no window on screen) · Stop → "Step 2 of 3" → a failure above the bar with Retry → the plan · an automatic stop (the length limit) writing the plan in the notepad without bringing up the main window · the Settings rows, "Show live transcript" off · the dictation hotkey and the meeting shortcut caught in the notepad |

The update spec signs its dummy installer with a throwaway key made for the
run (`tauri signer`) and serves `latest.json` from a local server, using the
debug-only `YAP_UPDATE_TEST_*` hooks in `src-tauri/src/updates.rs`. The
meeting-summary spec's AI is `support/fake-llm.js`, an OpenAI-compatible
server with deterministic replies that records every request (see
[meetings.md](./meetings.md#how-its-tested)). For the AI Chat's tool loop a
spec scripts it call by call (`scriptChat([{ tools: [{ name, args }] },
…, { content }])`: each chat call that offers tools takes the next step), and
each recorded request keeps its `messages`, so a spec can read the tool
results Yap sent back (`chat-tools.spec.js`).

## Adding a test

```js
import { test, expect, openView, openSettings } from './support/fixtures.js';

test('chat: the empty state offers a new chat', async ({ yap, main, shot }) => {
  await openView(main, 'Chat');
  await expect(main.getByRole('button', { name: 'New chat' })).toBeVisible();
  await shot(main, '10-chat-empty');
});
```

- `main` is the main window, reset before each test (back on screen if the
  desktop minimized it, the "Connect your calendar" dialog closed, toasts
  dismissed, Settings closed, Home showing).
  Yap sends some prompts to the window only while it's on screen (a call
  prompt goes to a Windows notification otherwise, which test mode never
  shows), so a test that starts one mid-test calls `mainOnScreen(yap)` first,
  as `meeting-detect.spec.js` does. `yap.onboarding`, `yap.notepad` and
  `yap.overlay` are the other windows (the notepad isn't reset: a spec that
  uses it tidies up after itself); `yap.invoke(cmd, args)` calls a Tauri command;
  `yap.readJson('notes.json')` reads the instance's data dir, and
  `expectStore(yap, file, check)` waits for a store to change.
- Find things the way a person does: roles, labels, placeholders and visible
  text (`getByRole`, `getByPlaceholder`, `getByText`). The sidebar is the
  `navigation` "Main", the Settings modal is the `dialog` "Settings" with
  its `navigation` "Settings sections", each titled settings card is a
  `group` named after its title, and `connectDialog(main)` is the "Connect
  your calendar" dialog (named after its step's title).
- `shot(page, 'NN-name')` saves `screenshots/<spec>/NN-name.png`; number the
  names so the folder reads in test order.
- Tests in a spec share one app instance and can run in any order, so don't
  depend on another test's leftovers. A spec that needs the app started
  differently overrides `yapOptions`: `test.use({ yapOptions: { name,
  config, env, data } })` (`data` seeds files in `Data/`, e.g.
  `{ 'notes.json': {...} }`), or `test.extend` when the values are computed
  (see `updates.spec.js`, `mcp.spec.js`).
- Don't click anything that leaves the app: sign-in buttons and external
  links open the browser, "Download …" fetches a model, "Open logs folder"
  opens Explorer, Browse opens a native file dialog. Native surfaces (tray,
  file dialogs, Windows notifications, dragging from Explorer) can't be
  driven over CDP; `no-mic.spec.js` shows how to stand in for a file drop.
- The hotkey reaches the app through the main window's in-page fallback:
  `pressHotkey(main)` presses F24 (the global hook is off in test mode);
  `hotkeyDown(main)` / `hotkeyUp(main)` hold it for push-to-talk. The
  fallback sends both to `dictation_key`, the hook's own path into the
  pipeline, so gestures behave as with the real hook. The
  meeting shortcut the same way: `main.keyboard.press('Meta+Alt+KeyM')`.

## Real-app robot tests (`npm run test:real`)

The suite above never lets Yap touch another app: nothing is pasted, no key
is pressed, no other window is read. The **robot suite** (`e2e-real/`) does
exactly that, safely: a **robot-mode** Yap pastes into real windows the
harness opens itself — a WinForms test pad, Windows Terminal, Edge, Notepad —
and every spec asserts **what actually landed there** (the field's text, the
terminal's buffer, what a stand-in coding CLI received). It's how Claude
tests paste, smart spacing, snippets, "press enter", the clipboard, terminal
and chat-app handling, the microphone path and real speech-to-text without
handing Nathan a checklist.

```bash
npm run test:real                         # build the test app (stub), then run
npx playwright test -c e2e-real           # only run, against the last build
npx playwright test -c e2e-real terminal  # one spec
npm run test:real:build -- --engines      # the real-STT build for stt.real.js
npx playwright show-report test-results/real/report
```

> ⚠ **It takes the keyboard focus** for about two minutes (the suite says
> so when it starts): windows open, come to the front and close. Don't type
> or click meanwhile — a key pressed into a robot window can spoil a test,
> and a click elsewhere makes Yap refuse the paste (the harness tries a
> refused paste again, twice). The window that was in front gets the focus
> back at the end. It needs a desktop session, so it doesn't run on CI.

The stub build is the UI suite's own (robot mode is part of every debug
build). `--engines` builds the real STT (whisper on Vulkan, ONNX on DirectML;
needs the Vulkan SDK) into a short target dir of its own,
`YAP_REAL_ENGINES_TARGET_DIR` (default `<drive>:\t-real`: whisper.cpp's CMake
build nests paths past `MAX_PATH` under `src-tauri\target`, the same reason
`scripts/dev.bat` uses `<drive>:\t`). `stt.real.js` uses it when it's there
(`YAP_REAL_ENGINES_EXE` points elsewhere), and Yap's default model, Parakeet
V3, **copied read-only** from `%APPDATA%\yap\models\parakeet-tdt-0.6b-v3-int8`
(or `YAP_REAL_MODEL_DIR`) into `test-results/real/cache` once, then
hard-linked into the robot's portable data dir. Nothing else in
`%APPDATA%\yap` is read. Without the build or the model the spec is skipped,
and says why.

### The guard: only the robot's own windows

Robot mode is test mode (`YAP_E2E=1`: portable data dir, own WebView2
profile and DevTools port, no global hooks, no account service, no
notifications, no update checks, F24 hotkey — `launchYap` from the UI suite)
plus `YAP_REALTEST=1`, **debug builds only** (`src-tauri/src/realtest.rs`;
compiled out of release builds). It lets Yap paste, press a submit key, read
the field around the cursor (UI Automation) and look at a window's title
(`app_kind`) **only in a window the test opened**:

- a window of a process in `YAP_REALTEST_PIDS` (the test pad) or one of its
  descendants — matched by start time too, so an id Windows has since given
  to another process never counts;
- or a window whose title contains `YAP_REALTEST_NONCE`, a random value of
  at least 12 characters made for each run and put in the title of every
  other window the harness opens (Windows Terminal, Edge, Notepad).

Anything else — your windows, Yap's own — is **refused**: nothing is sent,
"Robot mode refused to paste: …" goes to the log and to Yap's window as an
error toast, and `realtest_refusals` lists it. The window in front is checked
again **right before every `SendInput`** (the paste's Ctrl+V, typing, the
submit key), so clicking elsewhere mid-run can't redirect a paste: Yap
refuses instead. That leaves the moment between the check and Windows
delivering the keys; Yap checks again as `SendInput` returns and 10 ms later,
and if the window in front changed it says "the keys may have gone to
another window (process N)" — the suite fails that test, naming the
process. Reads follow the same rule, quietly (the text around the cursor,
`app_kind`'s look at the title, and the correction-learning watch).
`testpad.real.js`'s first test proves it with a second test pad Yap wasn't
given.

**Other apps' sound** ("Silence other apps while you dictate", "Pause music
and videos while you dictate": `mute.rs`, `media_pause.rs`) has the same
kind of guard, so a robot run never lowers or pauses your own music, call
or browser:

- in plain test mode (the UI suite) nothing is silenced or paused at all;
- in robot mode Yap may turn down an audio session only if the process
  playing it, or one it descends from, is in `YAP_REALTEST_PIDS` (by start
  time) or is a process whose visible, titled windows **all** carry the
  nonce (the test's own Edge, a throwaway profile; a Windows Terminal that
  also hosts your windows never counts) — `realtest::audio_permits`;
- and it may pause only media whose title in Windows' media controls has
  the nonce (the test page sets it) — `realtest::media_permits`;
- `realtest_other_apps` reports only those sessions and media.

The harness keeps to the same rule. Its helper (`support/yaprobot.cs`,
compiled with the C# compiler every Windows has into
`test-results/real/bin/yaprobot.exe`) presses keys or closes a window only
when it's titled with the nonce, and never prints another window's title or
text. Its one click is a right-click on the robot's **own** Yap
(`rightclick`, for `context-menu.real.js`): only on a window of that
process, brought to the front, the point checked to be on it right before
the click; the pointer goes back where it was, and a menu that came up is
closed with Esc. Windows Terminal windows are new windows (`wt -w new`) that may live in
your own terminal's process, which is why they're known by title and closed
one by one (confirming its "close all tabs?" for that window only). Each Edge
window gets a throwaway profile (none of your tabs, sign-ins or extensions).
Notepad is skipped while Notepad is open: Windows 11 would open the test
file as a tab in your window. The clipboard is snapshotted when a robot
starts (in Yap's memory, never printed) and put back when it stops; the
clipboard spec's test values are put there privately (out of Win+V history).

### How a robot dictates

No microphone or hotkey: debug-only commands drive the real pipeline.

- `realtest_dictate_text { text, hwnd }` — `text` as if the STT had just
  heard it, into the window in front (`hwnd` must be it): the target and the
  text around its cursor read as at a recording's start, then cleanup, the
  dictionary, "press enter", snippets, fitting to the kind of app and the
  cursor, the paste, the submit key, history. Returns what went to the paste:
  `{ text, raw, kind, submit, error, leftOnClipboard }` (`kind` as
  `AppKind::describe` says it: "terminal (Claude Code in Windows Terminal)",
  "chat app"…; `leftOnClipboard`: the target counted as an app running as
  administrator, so nothing was pasted).
- `realtest_dictate_wav { path, hwnd }` — a WAV played into the dictation
  **in real time, in the mic's place** (the real mic's audio is dropped
  meanwhile), between a real `start_recording` and the stop the hotkey would
  make: the live preview, the dead-mic watchdog, the STT and the paste all
  run. `null` when nothing was transcribed. The speech comes from Windows'
  own text-to-speech written straight to a 16 kHz WAV (`support/tts.js`,
  never played aloud, cached in `test-results/real/cache/tts`). With
  `cancel: true` the recording is cancelled at the end instead (the bar's
  ✕), kept for Undo; `bar_action { action: "dictation-undo" }` then
  transcribes and pastes it, and `realtest_delivered` returns what that
  dictation sent to the paste.
- `realtest_window { hwnd }` (the guard's verdict, no side effects),
  `realtest_refusals`, and `realtest_clipboard { action: save | restore |
  setText | setImage | check }` (`check` compares, it never hands the
  clipboard's contents out).

### What's covered

| Spec | Target | Tests |
|------|--------|-------|
| `testpad.real.js` | the test pad (WinForms: text box, rich text box, password box) | the guard (the pad allowed by its process, Yap's own window refused, a second pad Yap wasn't given refused for a dictation and for Paste last transcript, its field neither read nor changed, the refusal toast) · smart spacing fitted to the text before the cursor (mid-sentence lower case and space, a new sentence, after a space, a name keeping its capital, the rich text box) · mid-text in a text box with no TextPattern, the caret asked of the control ("I want | please" → "I want the blue one please", a selection replaced, the rich text box over a line break) · a password box pasted into but never read · snippets (a multi-line sign-off with its full stop dropped, said alone, a one-line link mid-sentence keeping the sentence's) · "press enter" (the text then Enter; only the command: just Enter) · Paste last transcript, fitted where it lands · the clipboard (your text back after the paste, the dictation not left there, the four keep-it-out-of-history formats present at the pad's Ctrl+V and on what came back; an image back byte for byte) · AI cleanup through the fake model, and "press enter" it dropped still sending |
| `browser.real.js` | Edge app windows | a web page's text box: mid-text (lower case, spaces, no full stop before what follows), after a sentence, replacing a selected word · only the box is read, never the page around it (page text on the box's line: empty, at the box's start, at its end) · a chat app (a page titled "WhatsApp - …"): a short message without its full stop, a question with its mark, three sentences with theirs, the setting off · an empty message box reads as empty, no full stop and no space after it (a textarea labelled "Type a message"; one labelled by a `<label>`; a contenteditable, as WhatsApp Web's and Gmail's are, and a draft there followed) |
| `dictation-extras.real.js` | the test pad, an Edge page | an app running as administrator, a stand-in (`e2e_paste_elevated`: a real elevated window needs a UAC prompt): no Ctrl+V reaches the pad, its text unchanged, the dictation on the clipboard (privately), "Your text is copied: press Ctrl+V" in Yap's window; "Paste last transcript" the same; the next dictation pastes as usual · a WAV dictation cancelled (the bar's ✕), nothing pasted, then Undo: transcribed and pasted into the pad · Undo with the Edge page in front: it lands in the pad, where the dictation started, and the page stays empty · Undo after the dictation's window (an Edge page) closed: nothing reaches the pad in front, the text is on the clipboard and Yap says it couldn't get back to that window |
| `terminal.real.js` | Windows Terminal | a PowerShell prompt: a list dictated as one line ("List the files: the logs; the notes.") and nothing run · "press enter" running a harmless `Echo` (its output read back) · a stand-in for Claude Code (Claude Code's idle title "✳ …", bracketed paste, runs nothing, logs what arrives): the line breaks kept in one bracketed paste, nothing sent; "press enter" sending it · auto-submit: Enter in the coding CLI, never at the shell prompt · "Safer pasting into terminals" off: just another app |
| `audio.real.js` | the test pad | a spoken WAV through a real recording to the paste (the stub's transcript) · a dead mic (7 s of digital silence): reopened at ~3 s, "No sound from … — check it isn't muted or unplugged" in the log and as a toast at ~5 s, nothing transcribed or pasted |
| `other-apps.real.js` | the test pad playing a near-silent tone on a loop, an Edge page playing one with the nonce in its media title | "Silence other apps while you dictate": the pad's audio session goes to 0 after the start chime (the recording started first) and comes back to exactly its level after the stop chime, `yaprobot.exe` the only session quieted · "Pause music and videos while you dictate": the page's player pauses during a WAV dictation and plays again after, Windows' media controls saying playing → paused → playing, its volume untouched · your own sessions and media never reported or changed (the guard above) |
| `stt.real.js` | the test pad, an `engines` build | Parakeet V3 hears "The quick brown fox jumps over the lazy dog" (Windows TTS) and it lands in the field |
| `learn.real.js` | the test pad, an Edge page | "Suggest words from your corrections" for real: the watch reads the field after the paste ("learn: watching the field…" in the log), the word is fixed in the field, Enter ends the watch (`enter_pressed`), the suggestion toast shows in Yap's window — Add writes "cube and eighties → Kubernetes" to `config.json` and the next dictation uses it; on the web page "graph ana → Grafana", and Not now leaves the dictionary alone |
| `context-menu.real.js` | the robot's own Yap (with `YAP_E2E_NO_INSPECT=1`: a release build's menus) | a real right-click (SendInput) and the windows Yap's process tree — WebView2's browser included — puts up: on plain UI none at all (WebView2 asked, every item dropped, the event handled); in a text field its menu (a `Chrome_WidgetWin_1` popup) with the edit commands only (emoji, undo, redo, cut, copy, paste, paste as plain text, select all), closed with Esc |
| `notepad.real.js` | Windows 11 Notepad | a dictation lands, the next one follows it after a space (skipped while Notepad is open) |

**Known gaps.** A gap the robot finds becomes a test named "known gap: …"
that pins **today's** result, with a comment saying what the right one is:
when the gap is fixed the test fails, and gets the right expectation and a
new name. (Not `test.fail()`: that passes on any failure, a stolen focus
included.) None are open now. The first run (2026-10-17) found three, all
fixed since, their tests now asserting the right result:

- Chromium (Edge, Chrome, Electron, WebView2) reads an **empty** text box's
  text as its label ("Type a message") or U+FFFC, so a short message in a
  web chat app kept its full stop and got a space after it ("Sounds good. "),
  and its ranges ran on past the box into the page's text (`yaprobot caret`
  shows the raw ranges). `selection.rs` now reads only the field's own
  document range, and an empty field as empty whatever stands in for its
  text; `browser.real.js` expects "Sounds good".
- The fuzzy pass took the short word before a term into its 2-word n-gram
  and swallowed it ("Deploy it to cube and eighties." became "Deploy it
  Kubernetes."); `fuzzy.rs` no longer takes a run whose first word adds
  nothing to the match, and `learn.real.js` now expects "Deploy it to
  Kubernetes.".
- A WinForms text box gives UI Automation a value but no caret, so Yap read
  it as if the cursor were at the end ("I want | please" + "The blue one."
  went in as " the blue one."). Yap now asks the edit control for its
  selection (`EM_GETSEL`, `selection.rs`), and `testpad.real.js` expects
  "I want the blue one please".

Also seen, not pinned: now and then (a few in a hundred on Nathan's PC, not
on demand) a Ctrl+V reaches the test pad's WinForms box and nothing is
pasted — the pad says so on stderr ("Ctrl+V reached the box but nothing was
pasted"); a bare Ctrl+V from the harness did the same, so it looks like the
box's own clipboard read losing a race, not Yap.

### Results

`test-results/real/` (git-ignored): `report/` (Playwright's), `playwright/`
(failure details), `runs/<spec>-w<N>/` (each robot Yap's `stdout.log` — with
debug logging on: what kind of app each dictation went into, how much text
around the cursor was read, as lengths — and its `app/Data/`, and the test
windows' temp folders under `targets/`), `screenshots/`, `cache/` (TTS
speech, the model copy) and `bin/yaprobot.exe`.

### Adding a robot test

```js
import { test, expect } from './support/robot.js';

test('a snippet in the test pad', async ({ robot }) => {
  const { pad } = robot;
  await robot.configure({ dictionary: [{ from: 'my email', to: 'me@example.com', fuzzy: false, snippet: true }] });
  await pad.set('box', 'Write to ');
  const r = await robot.dictate(pad, 'My email.');
  await expect.poll(() => pad.text('box')).toBe('Write to me@example.com');
  await robot.configure({ dictionary: [] });
});
```

`robot` is one robot-mode Yap plus the test pad per spec file (`robotOptions`
changes how Yap starts: `config`, `exe`, `prepare`); `robot.shell()`,
`robot.claudeCode()`, `robot.editor()`, `robot.chat()` and `robot.notepad()`
open the other targets, which close when the test ends. `robot.dictate(target,
text)` / `robot.dictateWav(target, file)` bring the target to the front and
dictate; `robot.logMark()` / `robot.logSince(mark)` read Yap's log. Assert
on what landed in the target, and on the returned `{ text, kind, submit,
error }`. Only harmless text in a shell: nothing but `echo` is ever run.

## Why Playwright over CDP

Tauri's official route is WebDriver: `tauri-driver` plus a Microsoft Edge
Driver whose version must match the installed WebView2 runtime exactly ("If
the two versions do not match, you may experience your WebDriver testing
suite hanging while trying to connect"), or WebdriverIO's newer embedded
driver, a Rust plugin compiled into the app that must never ship. WebView2
already speaks CDP, which Playwright supports directly
([WebView2 guide](https://playwright.dev/docs/webview2)): no driver to keep in
step with the runtime, nothing extra in the app, one npm devDependency, every
window reachable at once, and auto-waiting locators, screenshots and traces.
The trade-off: Playwright calls a CDP connection lower fidelity than its own
protocol, which costs nothing this suite uses.
