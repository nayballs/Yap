# Updates and releases

How a running Yap finds and installs updates, the installer, the stable and nightly channels, and how to cut a nightly. The dev → nightly → stable workflow itself is in [CLAUDE.md](../../CLAUDE.md).

[Architecture index](./README.md) · [CLAUDE.md](../../CLAUDE.md)

## `updates.rs`

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

## Release / installer
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
The bundle's **publisher** is "Yap" (2026-10-22: Yap is its own publisher, no person's name). Tauri's
installer keeps the install folder under `Software\<publisher>\Yap`, so an install
made under the old publisher has none there: `ReadPreviousInstallDir`
(`; --- YAP: PREVIOUS INSTALL DIR ---`) falls back to the uninstall entry's
`InstallLocation`, and an update still lands in the folder it replaces.
Google Calendar needs the build to carry an OAuth client: the optional
`YAP_GOOGLE_CALENDAR_CLIENT_ID` / `_SECRET` repository secrets, passed to the
build by both workflows (unset = the Connectors page says this build can't
connect to Google; iCal links still work). Setup: [`docs/calendar.md`](../calendar.md).

## Release channels (stable + nightly)
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

### Checksums and build provenance (2026-10-25)
For people who download an installer by hand (the updater keeps checking minisign
signatures, untouched by this):

- **`SHA256SUMS.txt`** on every release, written by `scripts/sha256sums.mjs`
  (`<hex>  <name>` per file, sorted, LF: `sha256sum -c SHA256SUMS.txt` checks a
  download in the same folder). **Stable** (`release.yml`, "Attach SHA256SUMS.txt"):
  after tauri-action and the constant-name copy, every file on the draft is read
  back from the release itself (the asset API, `curl -o`), so the names and bytes
  are the published ones — `Yap_<v>_x64-setup.exe`, its `.sig`, `latest.json`,
  `Yap-setup.exe`; an old `SHA256SUMS.txt` is deleted first, as `Yap-setup.exe`
  is, so a re-run for the same version replaces it. **Nightly** (`nightly.yml`,
  "Checksums"): over the staged constant-name files (`Yap-nightly-setup.exe`, its
  `.sig`, `latest.json`), uploaded with them (`--clobber`).
- **Build provenance attestations** (`actions/attest-build-provenance@v4`, a
  wrapper over `actions/attest`): a SLSA provenance statement for the installers
  (stable: `release-assets/*-setup.exe`, the versioned one and `Yap-setup.exe`;
  nightly: `Yap-nightly-setup.exe`, attested before it's published, so a nightly
  that can't be attested isn't), signed with Sigstore through the job's OIDC token
  and stored by GitHub. Both jobs' permissions add `id-token: write` and
  `attestations: write` (no `artifact-metadata`: that's for registry storage
  records). Anyone can check a download with
  `gh attestation verify Yap-setup.exe --repo nayballs/Yap` — it proves which
  workflow, commit and repo built the file, not that the file is safe. When
  Authenticode lands, attest (and checksum) **after** signing: signing changes the
  installer's bytes.
- Validated without a run (2026-10-25): `actionlint` 1.7.12 clean on all four
  workflows; the stable step dry-run against the published v0.1.0 (listing,
  downloads, `sha256sums.mjs`, the attestation glob; no delete or upload), its
  `SHA256SUMS.txt` passing `sha256sum -c`. The first real run is the next nightly.

### How to run / cut a nightly (it's all CI — no local build needed)
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
