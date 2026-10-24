# Build and test

Feature flags, running in dev, CI, the end-to-end UI suite and the real-app robot tests, plus test mode (`e2e.rs`) and robot mode (`realtest.rs`). The guides are [`docs/e2e-tests.md`](../e2e-tests.md) and [`docs/model-evals.md`](../model-evals.md); the speaker engine's tests are in [speakers.md](./speakers.md).

[Architecture index](./README.md) · [CLAUDE.md](../../CLAUDE.md)

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
*installed* app (it follows the nightly channel).
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
So merged work reaches the maintainer's dev build by pushing to main; never fast-forward
that checkout by hand while Yap - Dev runs.

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

## CI on every push
`.github/workflows/ci.yml` runs on every push/PR to `main`: `npm run build` (frontend,
also produces the `dist/` that `generate_context!` needs) + `cargo clippy --locked
--all-targets -- -D warnings` (test code included) on the fast **stub** build (no
`engines`, no Vulkan SDK) — a few minutes on a Windows runner, so a broken commit
can never reach a nightly. Then `cargo clippy --locked --release -- -D warnings`
(lib + exe, the release profile, since 2026-10-22): code only debug builds call —
the e2e hooks, the model evaluations (`eval.rs`, `#[cfg(debug_assertions)]`) — is
dead in release, and gets `#[cfg(debug_assertions)]` (or `any(test,
debug_assertions)`) itself; until then six such warnings showed only in release
builds. The real GPU pipeline is only exercised by
nightly/release builds. ⚠ Zero-warnings ratchet: any warning or clippy lint fails
the push, and the runner uses the **latest stable** toolchain, whose clippy can flag
lints an older local one doesn't — keep local Rust current (`rustup update`) or read
the CI log. (It sat red Jul 9 → Sep 25 2026 on accumulated lints, masking real
failures.)

`.github/workflows/e2e.yml` runs the **end-to-end UI suite** on the same triggers
(plus `workflow_dispatch`), separately so `ci.yml` stays quick — see below.

## The privacy guard (`scripts/privacy-guard.mjs`)
The repo is public, so nothing personal or secret may land in it (2026-10-22).
**Before a push** (`.githooks/pre-push`, which `npm run hooks:install`, once per
clone, copies into the repository's shared hooks folder so every worktree runs it,
and removes any `core.hooksPath`; the guard itself is also copied to
`%USERPROFILE%\.yap-private\` for worktrees on branches older than it, and the
hook refuses the push when it finds neither) it reads the
commits about to go out — the remote's tip..the new one, or for a new branch
whatever no remote branch has — and checks the lines their diffs add (a merge:
the lines its resolution adds), their messages and the paths they touch, never
the author/committer headers, for:
- **the private terms**: names and details listed one per line in a file outside
  every repo, `%USERPROFILE%\.yap-private\terms.txt` (or `YAP_PRIVATE_TERMS`),
  matched case-insensitively as whole words (a phrase's spaces match any
  whitespace). No file: skipped with one warning. A hit prints the line with the
  term masked as `[private term]`, never the term (terminal output ends up in logs);
- **API-key shapes** (Groq, OpenAI, Anthropic, xAI, Tavily, ElevenLabs, AWS,
  GitHub, Google, Stripe, Slack, Resend, private-key PEM headers) unless plainly a
  fake (a test word, a run like `abcdef` or `aaaaaa`, too few distinct
  characters) — printed cut short;
- **email addresses** except documentation domains (`example.com/.org/.net`,
  `*.example.*`, `.test`…), GitHub's noreply, Google Calendar ids and the
  product's own (`no-reply@mail.contextmirror.com`, `contextmirror@proton.me`);
- **UK phone numbers** except Ofcom's drama ranges (07700 900xxx, 020 7946 0xxx…).

A finding blocks the push with the commit, file, line and rule. Only for a false
positive, after checking it really is one: `git push --no-verify`. `npm run
privacy:scan` (`--scan-tree`) checks every tracked file as it is now;
`--range A..B` a range of commits. CI (`ci.yml`, first step) runs the tree scan
and the pushed range without private terms (it has no file), then the guard's
own tests (`npm run test:privacy`: a throwaway repo, terms file and commits,
the real hook through `git push`). Fixtures use fictional people and details
(Imogen, Declan, Rochelle; Bristol, Norwich; example.com; 07700 900xxx).

## End-to-end UI tests (`npm run test:app`)
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
[`docs/e2e-tests.md`](../e2e-tests.md).

## Real-app robot tests (`npm run test:real`)
The second layer: a **robot-mode** Yap (`realtest.rs`) pastes into **real windows
the harness opens** — a WinForms test pad (`e2e-real/support/yaprobot.cs`,
compiled with the .NET Framework's `csc`), Windows Terminal (a PowerShell prompt,
and a stand-in for Claude Code: its "✳ …" title + bracketed paste, runs nothing),
Edge app windows with throwaway profiles (a web page's text box; a "WhatsApp"
page = a chat app), Notepad (skipped while Notepad is open) — and the specs
(`e2e-real/*.real.js`) assert what actually landed: smart spacing, snippets,
"press enter", Paste last transcript, the clipboard (yours back, private while
pasted), AI cleanup via the fake model, terminal one-line mode / coding-CLI line
breaks / auto-submit, Git Bash's mintty pasted with Shift+Insert, VS Code's
terminal (an Edge stand-in for xterm.js's input; `YAP_REALTEST_EDITORS`), a
lost paste typed instead (the pad's "nopaste" box), chat full stops, the mic path from a TTS WAV
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
only, not on CI): say so before running it while the maintainer is at the PC; another app
grabbing the focus mid-run makes Yap refuse (the harness retries a refused paste
twice). A gap it finds becomes a "known gap: …" test pinning today's result
until it's fixed (none open now). Guide: [`docs/e2e-tests.md`](../e2e-tests.md) "Real-app robot tests".

## Test mode and robot mode

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
  watchdog's case, its stream really reopened), `e2e_mic_status` (the
  dictation mic's state, `mic.rs`: open, live, Bluetooth, opens/closes, the
  last open's timing), `e2e_dictate_audio { seconds }` (a dictation of a
  tone played in the mic's place — a real start and stop, the mic held as
  for any dictation; no microphone needed), `e2e_crash_next_dictation` (the
  next dictation panics in its paste step: `pipeline::Busy`), `e2e_fail_saves
  { store, count }` (a store's next saves fail as a full disk would:
  `saving.rs`), `e2e_upload_pace { ms }` (each Upload chunk held that long,
  so the job is still going when a spec comes back), `YAP_MIC_WARM_MS` (the mic's
  warm window, short) and `YAP_E2E_MODEL_LOAD_MS` (the startup model load,
  slowed down), `e2e_learn_correction` (an
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
  `browser_search` and Gemini's `generateContent` shapes), never the PC's
  own region settings (`about_you.rs`: the debug-only `YAP_E2E_LOCALE` — a
  tag or JSON — stands in for Windows; unset, nothing is detected), no AI
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
  [`docs/e2e-tests.md`](../e2e-tests.md).
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
  Ctrl+C stays off. `YAP_REALTEST_EDITORS` (robot mode only:
  `stand_in_editors`) names processes `app_kind` treats as editors with an
  integrated terminal, so an Edge page can stand in for VS Code's. Debug-only
  commands: `realtest_dictate_text { text, hwnd }` (`pipeline::Robot::dictate_text`:
  `capture_target` → `selection::wait_for_context` → `deliver`, the same code a
  spoken dictation runs after its STT; returns `Delivered { text, raw, kind,
  submit, error, leftOnClipboard, paste, typed }`), `realtest_dictate_wav { path, hwnd,
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
  [`docs/e2e-tests.md`](../e2e-tests.md) "Real-app robot tests".
