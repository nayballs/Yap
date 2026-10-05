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
  focus stealing, no Windows notifications, no killing "orphaned" sidecars,
  and the installed app's saved window position is neither read nor written.
  A meeting recording opens no audio device: it plays `you.wav`/`them.wav`
  from `YAP_E2E_MEETING_AUDIO` (`YAP_E2E_MEETING_SPEED` times real time) or
  stays silent, and the `e2e_meeting_feed` command hands it transcript
  segments directly. It quits when the suite closes its stdin, including
  when the suite dies;
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
| `e2e/app.spec.js` | default | Home renders in portable mode · every sidebar view · every Settings section · a setting saved to `config.json` and read back after a reload · a new note survives a view switch · a dictionary entry is saved · Account signed out with the service unreachable · onboarding opens on its first step · a stub dictation lands in the Home feed (skipped without a microphone) |
| `e2e/no-mic.spec.js` | configured mic missing | starts anyway and says "No microphone found" (toast + overlay) when asked to record · Upload still transcribes a file |
| `e2e/bar.spec.js` | default | the Yap bar (`yap.overlay`), through the debug-only `bar_simulate` (a pretend cursor on a region, fake fullscreen kinds, Esc) and `bar_debug`: idle, bottom-centre on the work area, click-through and never activated — its ex-styles read inside the app and from outside it (`support/win32.js`: GetWindowLongPtr, and WindowFromPoint on the pill showing a click would go through) — and Wispr's measured pill (40 × 8, 14 px up, a 60 × 20 hit wrapper, its colours; `flowbar-spec.md`) · the cursor on the pill opens it and makes just that clickable, then click-through again; tooltips with the shortcut, in Wispr's tooltip style · a notice card (the update card, as a demo) to Wispr's card measurements, 26 px above the pill, with its hit strip · its menu: hide for an hour (Settings shows it, Show it now) and Turn off the bar (saved; Settings adopts it and turns it back on) · Settings → General → Yap bar · a call prompt as a "Meeting detected" card (its ^ menu, ✕, an app's mark), answered on the bar · fullscreen: no pill, a card over a borderless app, a card waiting out an exclusive one · ◉ starts meeting notes; the recording pill (69 × 30, half opacity, its ring, 5 bars, the stop circle; hovered, the timer) opens the notepad and ■ ends the meeting · the opt-in countdown (Esc cancels; left alone it records; the call-ended card) · dictating in the pill's place |
| `e2e/updates.spec.js` | pointed at a local `latest.json` | a manual check finds the next patch version, downloads and verifies it, the "ready" toast and Settings → About say so, and "Restart to update" stops short of the installer (debug builds never run it) |
| `e2e/meeting-detect.spec.js` | default | call detection through the debug-only `meeting_detect_simulate` hook: a call offers to take notes and "Not now" leaves it (and the app's next call) alone · a prompt leaves with its call · a prompt left alone fades (its `fadeMs` shortened from 30 s) and counts as "Not now" · "Record notes" creates a Meetings note and records it, the call ending offers "Stop and summarise", which ends it in the note and runs the summary step · "Keep recording" carries the notes into a rejoined huddle, whose end asks again · a Discord call isn't asked about by default · "Don't ask for Teams" confirms, Settings shows Teams off, and switching it back on asks again · "Quietly" keeps the prompt out of the window · Settings lists how Yap asks and every app's effective choice, in two columns · the Settings toggle turns it off and greys out the rest |
| `e2e/meeting-detect-no-mic.spec.js` | configured mic missing | "Record notes" says why it can't record and leaves no empty meeting note |
| `e2e/meetings.spec.js` | meeting audio from test WAVs | live You/Them segments, cut in pauses · ending the meeting without an AI model explains how to set one up · the call leaking from the speakers into the mic is flagged as echo, hidden, and kept |
| `e2e/mcp.spec.js` | Local API on, seeded meetings and a dictation, a stand-in user profile with AI apps' configs | Integrations' "Go to MCP" opens Settings → MCP · Wispr's four cards in order (Claude, ChatGPT, Gemini, Cursor) with what Yap found, then All other apps · Add to Claude merges into its config (other servers kept, `.bak` made) and Remove gives back the same bytes · Cursor, ChatGPT (TOML) and Claude Code (under its lock) get their own formats · a config that isn't plain JSON is left alone, with the reason · with the Local API off the page says so and turns it on, and Settings' copy agrees · a real `yap.exe mcp` session (`support/mcp-client.js`) lists, searches and pages meetings, reads notes and folders, and never returns the dictation · "Let AI apps save notes" adds `create_note` and the note shows in Notes · with no Yap, the tools ask to open it |
| `e2e/meeting-summary.spec.js` | Note Formatting pointed at a local fake AI | a two-hour meeting fed in ten-minute batches: 11 rolling digests, each under 4,500 tokens with a capped reply · one final call over digests + the raw tail · an action plan with a section per person, Decisions, Open questions and Unassigned · the deterministic checks (an invented owner, a dropped task, a made-up deadline) · Copy text |
| `e2e/meeting-guards.spec.js` | default | the meeting guard rails: the overlay and the notepad excluded from screen capture while a meeting records and back after (debug-only `capture_affinity`) · with hiding off, the screen-share tip, **Update settings** at the switch, and switching it back on mid-meeting · the length warning, **Keep going**, and the stop at the limit, in seconds (debug-only `e2e_meeting_limit`) · "When a call ends: Stop and summarise automatically" on a simulated call end · the meeting shortcut (Win+Alt+M through the in-page fallback) starting a "Meeting · …" note and stopping it, and taking notes on a live call · the Settings rows and recording a new shortcut |
| `e2e/notepad.spec.js` | Note Formatting pointed at a local fake AI | the meeting notepad (`yap.notepad`): it opens on record docked to the right edge, full height, without taking the focus, and out of screen captures · live You/Them lines, echo hidden, the dismissible tip, the empty state · closing it keeps recording and Notes reopens it · My thoughts and the title synced with Notes both ways · What did I miss? (nothing new without a model call, then only the lines since you looked, then a follow-up) · the AI title replacing call detection's made-up one, never a typed one · Started by mistake? (Keep, Generate summary, Discard) · Stop → "Step 2 of 3" → a failure with Retry → the plan · an automatic stop (the length limit) writing the plan in the notepad without bringing up the main window · the Settings rows, "Show live transcript" off · the dictation hotkey and the meeting shortcut caught in the notepad |

The update spec signs its dummy installer with a throwaway key made for the
run (`tauri signer`) and serves `latest.json` from a local server, using the
debug-only `YAP_UPDATE_TEST_*` hooks in `src-tauri/src/updates.rs`. The
meeting-summary spec's AI is `support/fake-llm.js`, an OpenAI-compatible
server with deterministic replies that records every request (see
[meetings.md](./meetings.md#how-its-tested)).

## Adding a test

```js
import { test, expect, openView, openSettings } from './support/fixtures.js';

test('chat: the empty state offers a new chat', async ({ yap, main, shot }) => {
  await openView(main, 'Chat');
  await expect(main.getByRole('button', { name: 'New chat' })).toBeVisible();
  await shot(main, '10-chat-empty');
});
```

- `main` is the main window, reset before each test (toasts dismissed,
  Settings closed, Home showing). `yap.onboarding`, `yap.notepad` and
  `yap.overlay` are the other windows (the notepad isn't reset: a spec that
  uses it tidies up after itself); `yap.invoke(cmd, args)` calls a Tauri command;
  `yap.readJson('notes.json')` reads the instance's data dir, and
  `expectStore(yap, file, check)` waits for a store to change.
- Find things the way a person does: roles, labels, placeholders and visible
  text (`getByRole`, `getByPlaceholder`, `getByText`). The sidebar is the
  `navigation` "Main", the Settings modal is the `dialog` "Settings" with
  its `navigation` "Settings sections", and each titled settings card is a
  `group` named after its title.
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
  `pressHotkey(main)` presses F24 (the global hook is off in test mode). The
  meeting shortcut the same way: `main.keyboard.press('Meta+Alt+KeyM')`.

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
