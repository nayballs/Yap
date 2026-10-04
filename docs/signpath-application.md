# SignPath Foundation application — draft answers

Apply at **https://signpath.org/apply**. Everything below is ready to paste;
the form is yours to submit (it's an application in your name, and it ends in a
reCAPTCHA). Pair with [`SIGNING.md`](./SIGNING.md) for the CI wiring once
approved. Conditions quoted from https://signpath.org/terms (checked 2026-10-04).

## The gate: reputation

SignPath puts *its own name* on the certificate, so it won't vouch for software
"based on source code that nobody knows": downloadable programs need "a certain
verifiable reputation". The form has a required **Reputation** field (media
coverage, blog posts, download statistics, GitHub insights, community
discussions). They decide case by case and ask applicants not to argue a
rejection, so apply once there is something to show.

On 2026-10-04 Yap had 1 star, 0 forks and ~14 unique visitors a fortnight —
too early. Things that would count: a launch post (Show HN, r/LocalLLaMA,
r/Windows, r/speechtech) and its discussion, stars, stable-release download
counts (`gh api repos/nayballs/Yap/releases --jq '.[] | {tag_name, d: [.assets[].download_count]}'`),
any write-ups or videos.

## Everything else (already met unless ticked off below)

- OSI licence, no commercial dual-licensing: MIT.
- No proprietary components: Yap's code is MIT. The installer bootstraps
  Microsoft's WebView2 (Microsoft-signed, a system component); models and the
  llamafile runtime are downloaded at runtime, not bundled.
- Released in the form to be signed: the NSIS installer on GitHub Releases
  (stable from v0.1.0).
- Maintained; functionality documented on the download page.
- Uninstaller: the NSIS uninstaller.
- MFA: GitHub 2FA is on (and required for the account). Turn it on for
  SignPath too once the account exists.
- Privacy: nothing leaves the PC unless the user asks (model downloads, update
  checks, a cloud cleanup provider they configured, the optional account).
- Builds are verifiable: public GitHub Actions workflows, tagged commits.

To do when you apply:

- [ ] **"Code signing policy"** on the homepage *and* the download/release
      pages, with SignPath's wording (text below): the README, the
      contextmirror.com/yap page, and the release notes.
- [ ] Every release then needs your manual approval in SignPath. Sign stable
      releases; leave nightlies unsigned.
- [ ] Windows will show **"SignPath Foundation"** as the publisher, not
      "Nathan Lawrie" or "Yap". That's how the free program works.

## Form answers

**Project Name\*** (a Google search should identify it): Yap — voice dictation for Windows

**Repository URL\***: https://github.com/nayballs/Yap

**Homepage URL\***: https://contextmirror.com/yap

**Download URL** (must mention SignPath Foundation code signing): https://contextmirror.com/yap

**Privacy Policy URL**: https://auth.contextmirror.com/privacy

**Wikipedia URL**: (none)

**Tagline\*** (one sentence, may appear on signpath.org):
Free, open-source voice dictation for Windows that transcribes on your own GPU
and types into any app.

**Description\*** (a short paragraph, nothing version-specific):
Yap is a voice dictation app for Windows. Press a hotkey, speak, and Yap
transcribes locally on the user's GPU (Whisper and ONNX speech models via
Vulkan and DirectML), optionally tidies the text with a local or
user-configured AI model, and types it into whatever app is focused. Audio and
transcripts stay on the PC unless the user chooses a cloud provider. It also
includes a correction dictionary, voice editing of selected text, notes, a
meeting recorder and transcription of audio files.

**Reputation\***: (fill in at the time — see above)

**Maintainer Type**: Individual

**Build System\***: GitHub Actions

**First Name\* / Last Name\***: Nathan / Lawrie

**Email\***: your email (it becomes the SignPath login)

**Company Name**: (leave empty)

**Primary Discovery Channel\***: whatever's true (e.g. an AI assistant)

Then the reCAPTCHA and the Code of Conduct checkbox.

## Code signing policy text

SignPath requires the heading "Code signing policy", the sentence "Free code
signing provided by SignPath.io, certificate by SignPath Foundation", the team
roles, and a privacy statement. For the README (and, adapted, the website and
release notes):

```markdown
## Code signing policy

Free code signing provided by [SignPath.io](https://about.signpath.io),
certificate by [SignPath Foundation](https://signpath.org).

Team roles:
- Committers and reviewers: [Nathan Lawrie](https://github.com/nayballs)
- Approvers: [Nathan Lawrie](https://github.com/nayballs)

Privacy policy: This program will not transfer any information to other
networked systems unless specifically requested by the user or the person
installing or operating it — downloading a model, checking for updates, using
a cloud AI cleanup provider they configured, or signing in to the optional
Yap account (see the [privacy policy](https://auth.contextmirror.com/privacy)).
```

Add it only once SignPath has accepted Yap (or at the moment you apply):
before that, the sentence would claim signing that doesn't exist.
