# SignPath Foundation application — draft answers

Apply at **https://signpath.org/apply**. Everything below is ready to paste;
the form is yours to submit (it's an application in your name). Pair with
[`SIGNING.md`](./SIGNING.md) for the CI wiring once approved.

## Before you submit

SignPath's conditions (signpath.org/terms) that need something from you:

- [ ] **2FA on GitHub** (and on SignPath once the account exists). Required for
      every team member.
- [ ] **A "Code signing policy" section** must be published (README or website).
      Text ready below — add it to the README when you submit. It credits
      SignPath and names who can change code and approve signing.
- [ ] **A release to point at.** They sign projects that are already released;
      Yap has rolling nightlies but no tagged stable release yet. Consider tagging
      `v0.1.0` first (see ROADMAP / the stable-release plan) so the application
      can link a real release page.

Things to know:

- **Publisher name.** Windows will show **"SignPath Foundation"** as the
  publisher, not "Nathan Lawrie" or "Yap". That's how the free program works.
- **Manual approval per release.** "Every release needs manual approval for
  signing." Sign **stable releases** (a click in SignPath per tag); leave the
  daily nightlies unsigned, or approve them by hand when it matters.
- **No proprietary components.** Yap's own code is MIT. The installer can pull
  in Microsoft's WebView2 bootstrapper (Microsoft-signed) — mention it if asked.
  Models and the llamafile runtime are downloaded at runtime, not bundled.

## Form answers

**Project name:** Yap

**Repository:** https://github.com/nayballs/Yap

**Homepage / download page:** https://contextmirror.com/yap

**License:** MIT (OSI-approved, no dual licensing)

**Short description:**
Yap is a free, open-source voice dictation app for Windows. Press a hotkey,
speak, and Yap transcribes locally on your GPU (Whisper via Vulkan, ONNX models
via DirectML), optionally cleans the text up with a local or user-configured AI
model, and types it into whatever app is focused. Audio and transcripts never
leave the PC unless the user turns on a cloud cleanup provider themselves.

**What will be signed:**
The Windows NSIS installer (`Yap_<version>_x64-setup.exe`) and the application
executable inside it (`yap.exe`), built by GitHub Actions
(`.github/workflows/release.yml`, Tauri 2 + Rust + Svelte).

**Build system:** GitHub Actions (windows-latest), public workflow files in
the repository; releases are built from tagged commits on `main`.

**Release cadence:** Stable releases tagged manually (`v*`); a nightly
pre-release channel is built daily from `main` (we'd sign stable releases).

**Users / downloads:** (fill in — GitHub release download counts are on the
Insights → Traffic page and the releases page)

**Team roles:**
- Authors (commit without extra review): Nathan Lawrie (@nayballs)
- Reviewers (review outside contributions): Nathan Lawrie (@nayballs)
- Approvers (approve each signing request): Nathan Lawrie (@nayballs)

**Anything else:**
The app has an auto-updater (Tauri updater, minisign-verified). Our release
pipeline will Authenticode-sign the installer *before* computing the updater
signature, so existing installs keep updating. An unsigned installer currently
triggers SmartScreen's "unknown publisher" warning, which is the main reason
we're applying.

## README section to add when you submit

```markdown
## Code signing policy

Free code signing provided by [SignPath.io](https://about.signpath.io),
certificate by [SignPath Foundation](https://signpath.org).

Team roles:
- Committers and reviewers: [Nathan Lawrie](https://github.com/nayballs)
- Approvers: [Nathan Lawrie](https://github.com/nayballs)

Privacy policy: Yap works offline and doesn't send your audio or transcripts
anywhere. It only contacts the network when you ask it to: downloading a
model, checking for updates, using a cloud AI cleanup provider you configured,
or signing in to the optional Yap account (see the
[privacy policy](https://auth.contextmirror.com/privacy)).
```
