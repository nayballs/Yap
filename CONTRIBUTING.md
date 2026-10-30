# Contributing to Yap

Thanks for wanting to help. Bug reports, ideas and pull requests are all welcome.

- **Found a bug?** [Open an issue](https://github.com/nayballs/Yap/issues/new/choose). Settings →
  About → **Report a problem** makes a diagnostic file you can look through and attach.
- **Have an idea?** Open a feature request and say what you were trying to do.
- **Planning a big change?** Please open an issue first, so we can agree on the approach
  before you spend the time.
- **Found a security problem?** Don't open an issue: see [SECURITY.md](./SECURITY.md).

## Building Yap

Yap is a [Tauri 2](https://tauri.app) app for Windows: Rust in `src-tauri/`, Svelte 5 in
`src/`. You need Windows 10 or 11, [Node.js](https://nodejs.org) 20+, and the latest stable
[Rust](https://rustup.rs) (CI fails on any warning from the newest stable, so run
`rustup update` now and then).

```bash
npm ci
npm run tauri dev                          # the stub: the whole app, placeholder transcription
npm run tauri dev -- --features engines    # real speech-to-text on your graphics card
```

- **The stub** (the default) builds fast and needs nothing else: the app runs, but a
  dictation returns placeholder text. Most UI and Chat work can be done here.
- **The `engines` feature** is what releases ship: Whisper on Vulkan and the ONNX models
  (Parakeet and others) on DirectML, so it works on any graphics card, or the processor. It
  needs the [Vulkan SDK](https://vulkan.lunarg.com) (`winget install LunarG.VulkanSDK`, then
  open a new terminal). The first build compiles whisper.cpp and takes a while.
- **`scripts\dev.bat`** runs the same thing with hot reload (Vite on port 51437): it picks
  `engines` when the Vulkan SDK is installed and the stub when it isn't. On `main` it first
  fast-forwards the checkout to `origin/main`; on any other branch it leaves your code alone.

If you have Yap installed, quit it before starting a dev build: both use `%APPDATA%\yap`.

## Running the tests

```bash
npm run build                                               # the frontend
cd src-tauri
cargo clippy --locked --all-targets -- -D warnings          # what CI runs, warnings are errors
cargo clippy --locked --release -- -D warnings
cargo test --locked
cd ..
npm run test:app                                            # the end-to-end suite
```

- **`npm run test:app`** builds a stub debug Yap and drives it with Playwright: about 20
  seconds plus the build, with screenshots in `test-results/app/screenshots/`. Add or extend
  a spec in `e2e/` for anything you change in the UI. Guide: [docs/e2e-tests.md](./docs/e2e-tests.md).
- **`npm run test:real`** pastes into real windows on your PC, so it takes the keyboard focus
  for a couple of minutes. It waits until the PC has been idle 10 minutes
  (`YAP_ROBOT_FORCE=1` starts it now) and stops the moment you touch the keyboard or mouse.
  It only runs locally, never in CI. Worth it for changes to pasting, the clipboard or the
  microphone.
- **`npm run eval`** puts real AI models through Yap's Chat code. Use it before changing a
  prompt or a tool. Guide: [docs/model-evals.md](./docs/model-evals.md).

CI runs the privacy guard, the build, both clippy passes and the end-to-end suite on every
pull request.

## Privacy: fictional data only

The repository is public, and Yap handles people's words. So:

- Run **`npm run hooks:install`** once per clone. It adds a pre-push check
  (`scripts/privacy-guard.mjs`) that blocks API keys, real email addresses and phone numbers.
  `npm run privacy:scan` checks the whole tree.
- Fixtures, tests, evals, screenshots and comments use **made-up people and places**:
  `example.com` addresses, `07700 900xxx` phone numbers, fictional towns. Never paste a real
  dictation, a real meeting or someone's name into the repo or an issue.
- Logs never hold what someone said or typed at `info` level or above: lengths, counts and
  ids only (`src-tauri/src/privacy/log_privacy.rs` checks this).

## Where things are

- **[CLAUDE.md](./CLAUDE.md)** is the short index: how Yap works, how to build and test, and
  one line per module.
- **[docs/architecture/](./docs/architecture/README.md)** has one file per area (dictation,
  pasting, meetings, chat…). Read the one for the area you're changing first, and update it
  in the same pull request so it keeps matching the code.

## Commits and pull requests

Commit messages follow [Conventional Commits](https://www.conventionalcommits.org), with the
area as the scope and a plain-English summary:

```
fix(pasting): a lost paste is typed instead
feat(meetings): rename a speaker from the transcript
docs(architecture): the dictionary's fuzzy pass
```

`feat` and `fix` commits become the release notes people see in the app, so write them for
someone who uses Yap. Keep a pull request to one change, say how you tested it, and include
a screenshot for anything visual.

By contributing you agree that your work is released under the [MIT License](./LICENSE).
