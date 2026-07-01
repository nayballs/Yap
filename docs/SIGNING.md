# Code signing (Windows Authenticode) via SignPath Foundation

Goal: Authenticode-sign the Yap installer so Windows SmartScreen stops warning
"unknown publisher" on first run. We use **SignPath Foundation**, which provides
**free** code-signing certificates to qualifying open-source projects.

> Status: **not yet active.** The steps below are the setup. Steps 1–3 are yours
> (account + portal config + approval, which takes a few days). Steps 4–5 are the
> CI wiring, mostly drafted here — we finish + test them once the account is live.

---

## Why this needs care (the auto-updater ordering)

Yap's in-app updater (`tauri-plugin-updater`) verifies a **minisign signature**
over the installer's *bytes* (`latest.json`). Authenticode signing **changes the
installer's bytes**. So the order MUST be:

1. Build the installer (unsigned).
2. **Authenticode-sign** it (SignPath).
3. **Then** compute the minisign updater signature over the *signed* installer +
   write `latest.json`.
4. Upload the signed installer + `latest.json` to the release.

If we sign after generating `latest.json` (the naive "sign at the end" step), the
minisign hash won't match and **auto-update breaks for existing users.** SignPath
signs post-build, so the release workflow has to be reordered as above — that's
the main integration work, and why it needs a test release to validate.

---

## 1. Apply to SignPath Foundation (you)

1. Go to **https://signpath.org/apply** (SignPath Foundation OSS program).
2. Submit Yap: repo `https://github.com/nayballs/Yap`, MIT licence, Windows
   desktop app, Tauri. Mention it's a released app with an installer + auto-updater.
3. Wait for approval (manual review, typically a few days). They set up a SignPath
   **organization** for you with a certificate under the Foundation's program.

## 2. Configure the SignPath portal (you, after approval)

In `https://app.signpath.io` under your org:
1. **Project** — create one, e.g. slug `yap`.
2. **Artifact configuration** — a single-file config for the NSIS installer
   (`Yap_x.y.z_x64-setup.exe`). SignPath signs the `.exe` inside.
3. **Signing policy** — create `release-signing` (and optionally `test-signing`).
4. **Trusted build system** — connect **GitHub Actions**. SignPath verifies the
   build's provenance (repo + workflow), so signing only works from our real CI.
5. Note these values — they become workflow inputs / secrets:
   - Organization ID (GUID)
   - Project slug (`yap`)
   - Signing policy slug (`release-signing`)
   - **API token** → GitHub secret `SIGNPATH_API_TOKEN`

## 3. Add the GitHub secret (you)

Repo → Settings → Secrets and variables → Actions → New repository secret:
- `SIGNPATH_API_TOKEN` = the token from SignPath.

(The org ID / project / policy slugs can be plain workflow inputs — they're not
secret.)

## 4. Wire the release workflow (we do this together, then test)

The plan for `.github/workflows/release.yml`, reordered per the updater section:

1. Build the installer with `tauri-action` but **without** publishing/updater
   signing yet — i.e. drop `TAURI_SIGNING_PRIVATE_KEY` from the build step so it
   produces an **unsigned, no-`latest.json`** installer, and don't create the
   release in that step.
2. **Sign** the installer:
   ```yaml
   - name: Authenticode sign (SignPath)
     uses: signpath/github-action-submit-signing-request@v1
     with:
       api-token: ${{ secrets.SIGNPATH_API_TOKEN }}
       organization-id: <ORG_GUID>
       project-slug: yap
       signing-policy-slug: release-signing
       artifact-configuration-slug: installer
       github-artifact-id: ${{ steps.upload.outputs.artifact-id }}
       wait-for-completion: true
       output-artifact-directory: signed/
   ```
3. **Now** generate the updater signature over the *signed* installer and build
   `latest.json` (tauri signer CLI):
   ```bash
   npx @tauri-apps/cli signer sign \
     --private-key "$TAURI_SIGNING_PRIVATE_KEY" \
     --password "$TAURI_SIGNING_PRIVATE_KEY_PASSWORD" \
     signed/Yap_x.y.z_x64-setup.exe
   # then assemble latest.json with the resulting .sig + the release URL
   ```
4. Create the GitHub release and upload the **signed** installer + `latest.json`.

The existing inline-updater path (tauri-action with `TAURI_SIGNING_PRIVATE_KEY`)
stays as the fallback until this is proven on a test tag.

## 5. Verify (we do this on a test release)

- Download the installer from the draft release → right-click → Properties →
  **Digital Signatures** tab shows a valid signature.
- Fresh Windows VM: run the installer → SmartScreen should no longer say
  "unknown publisher" (SignPath's cert is trusted; reputation is immediate under
  the Foundation program).
- From an older installed Yap, trigger **Check for updates** → it downloads the
  signed installer and the minisign check **passes** (proves the ordering is right).

---

## Alternatives (for reference)

- **Certum Open Source Code Signing** — ~£10–30/yr, cloud (SimplySign) token.
  Works as an inline `signCommand` (simpler updater ordering) but costs money.
- **Azure Trusted Signing** — ~$10/mo, instant SmartScreen trust, native GitHub
  Actions support. Best UX if the free route stalls.

We chose SignPath Foundation because it's **free** for OSS. If approval stalls or
the Foundation criteria aren't met, Certum is the cheap paid fallback.
