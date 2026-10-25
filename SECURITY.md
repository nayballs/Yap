# Security policy

Yap runs on your PC with access to your microphone, your clipboard and your keyboard, so
security reports are taken seriously. Thank you for helping keep its users safe.

## Reporting a vulnerability

**Please report it privately, not in a public issue.**

Use GitHub's private vulnerability reporting:
[**Report a vulnerability**](https://github.com/nayballs/Yap/security/advisories/new) (or the
repository's **Security** tab → **Report a vulnerability**). Only the maintainer sees it.

Please include:

- what an attacker can do, and what they need first (another program on the PC, a web page,
  a file the user opens…);
- the steps to reproduce, or a proof of concept;
- the Yap version (Settings → About) and whether it's the stable or nightly build.

Leave out anything personal: use made-up text in place of real dictations or meetings.

If the link above doesn't work for you, open an ordinary issue that only asks for a private
contact, without any details, and the maintainer will get back to you.

**What happens next**: you'll get a reply within a week. Once the problem is confirmed, the
fix goes out in a nightly first, then a stable release, and the advisory is published with
credit to you, unless you'd rather not be named.

## Supported versions

| Version | Security fixes |
|---|---|
| The latest stable release (`v*`, [Releases](https://github.com/nayballs/Yap/releases/latest)) | Yes |
| The current nightly (the rolling [`nightly`](https://github.com/nayballs/Yap/releases/tag/nightly) pre-release) | Yes |
| Older releases | No: Yap updates itself, so please update first |

## In scope

- **The local API** (`src-tauri/src/integrations/bridge.rs`): an HTTP server on `127.0.0.1`
  that only answers requests with the token from `~/.yap/cli-bridge.json`. Ways to reach it
  without the token, from another machine, or from a web page in the browser are in scope.
- **The MCP server** (`yap.exe mcp`, `src-tauri/src/integrations/mcp.rs`): what an AI app can read or change
  through it — meetings and notes only, never dictation history, and notes written only when
  the user allows it.
- **The updater** (`src-tauri/src/updates.rs`, `tauri-plugin-updater`): an update is installed
  only when its minisign signature matches the key built into Yap. Getting Yap to install
  anything else is in scope.
- **The keyboard and mouse hooks and the clipboard** (`src-tauri/src/dictation/input_hook.rs`,
  `src-tauri/src/pasting/`): Yap watches for its shortcuts and pastes through the clipboard, then puts back
  what was there. Leaking keystrokes or clipboard contents, or pasting into a window the user
  didn't dictate into, is in scope.
- **Web content in Chat** (`src-tauri/src/web.rs`, `src-tauri/src/chat/tools.rs`): pages the
  AI reads must never make Yap act (open apps, change notes, copy to the clipboard).
- **Secrets and personal data**: the account session and calendar tokens live in Windows
  Credential Manager, AI providers' keys in Yap's settings on the PC, and the logs hold no
  dictated text at `info` level and above. A way for a web page, an AI app or another
  machine to get at them is in scope.

## Out of scope

- Anything that needs an attacker already running code as the same Windows user: they can
  read the user's files and keystrokes without Yap.
- The AI providers, search services and model hosts a user chooses to connect (Groq, OpenAI,
  Exa…): please report those to them.
- The installer not yet being Authenticode-signed (Windows SmartScreen warns about it). It's
  known and planned. Every release lists SHA-256 checksums (`SHA256SUMS.txt`) and a build
  provenance attestation you can check with
  `gh attestation verify <file> --repo nayballs/Yap`.
