# Yap local API (the Integrations bridge)

Yap runs a tiny **loopback HTTP server** while the app is open so terminals,
scripts, and coding agents on the same machine can read and write Yap data.
It's the local-first port of OpenWhispr's CLI bridge (`cliBridge.js` →
`src-tauri/src/bridge.rs`): loopback-only, bearer-token authenticated, no
cloud, no account.

Toggled in **Integrations → Local API** (`config.bridge_enabled`, on by
default).

## Discovery

On start Yap writes a discovery file to the **fixed** path
`~/.yap/cli-bridge.json` (not the data dir — external tools must find it
without knowing how Yap was installed; portable mode does not move it):

```json
{ "version": 1, "port": 54321, "token": "<64 hex chars>" }
```

- The port is OS-assigned per session (OpenWhispr scans 8200–8219 instead; the
  file is the source of truth either way, so Yap skips the fixed range and its
  collision risk).
- The file is removed on clean exit. Missing file / refused connection ⇒ Yap
  isn't running (or the bridge is switched off).
- The token is a fresh CSPRNG value per session. Send it on every request:
  `Authorization: Bearer <token>`.

```bash
TOKEN=$(jq -r .token ~/.yap/cli-bridge.json)
PORT=$(jq -r .port ~/.yap/cli-bridge.json)
curl -s -H "Authorization: Bearer $TOKEN" "http://127.0.0.1:$PORT/v1/notes/list"
```

## Conventions

Same envelope contract as OpenWhispr's v1 API:

- Lists: `{ "data": [...], "has_more": false, "next_cursor": null }`
- Single resources: `{ "data": {...} }`
- Errors: `{ "error": { "code": "not_found" | "validation_error" | "unauthorized" | "forbidden" | "internal_error", "message": "..." } }`
- Mutating verbs take JSON bodies (≤ 1 MB). Create returns **201** + the
  resource; deletes return **204**.
- Note ids are integers. **Transcription ids are the history entry's
  unix-seconds `ts`** (Yap's history has no row ids).

## Routes

| Method | Path | Notes |
|--------|------|-------|
| GET | `/v1/health` | `{data:{ok,version,app:"yap"}}` |
| GET | `/v1/notes/list` | query: `folder` (name, case-insensitive), `note_type` (`personal`\|`meeting`), `limit` (default 100). Summaries, newest-updated first. |
| GET | `/v1/notes/search?q=&limit=` | keyword scorer (same as the Chat RAG / `search_notes` tool); `limit` default 20. Items: `{id, title, folder, noteType, score, preview, createdTs, updatedTs}` |
| GET | `/v1/notes/{id}` | the full note: raw `content`, `enhancedContent`, `transcript`, `participants`, … |
| POST | `/v1/notes/create` | body `{title?, content?, folder?, source?}`; source is recorded as `"api"` (or `"mcp"` when the MCP server saves it) |
| PATCH | `/v1/notes/{id}` | body `{title?, content?, folder?, participants?}`; returns the updated note |
| DELETE | `/v1/notes/{id}` | 204 |
| GET | `/v1/folders/list` | folder names (strings) |
| POST | `/v1/folders/create` | body `{name}`; idempotent per name (case-insensitive) |
| GET | `/v1/meetings/list?folder=&limit=` | meetings (recorded notes), newest first: `{id, title, folder, participants, createdTs, updatedTs, startTs, endTs, durationSecs, lines, pages, hasSummary, hasNotes, digests}`; `limit` default 50 |
| GET | `/v1/meetings/search?q=&limit=` | meetings whose title, attendees, typed notes, AI summary or transcript mention the words (3+ letters), best first, each with up to 3 `matches: [{at, speaker, text, page}]` and a `summaryMatch` line; `limit` default 10 |
| GET | `/v1/mcp/config` | `{data:{allowWrites}}`: whether AI apps may save notes (see below) |
| GET | `/v1/transcriptions/list?limit=` | dictation history, newest first (`limit` default 50) |
| GET | `/v1/transcriptions/{ts}` | one entry: `{ts, raw, text, model, app}` |
| DELETE | `/v1/transcriptions/{ts}` | 204 |

No audio routes: Yap doesn't retain dictation audio.

Note mutations made through the bridge emit `yap-notes-changed`, so an open
NotesView refreshes live.

## Security model

- Bound to `127.0.0.1` only, plus a per-request loopback re-check.
- Bearer token compared via SHA-256 digests (no timing leak), regenerated
  every session.
- The discovery file is written `0600` on Unix; on Windows the user-profile
  ACLs already restrict it to the owning user.
- No CORS headers — browsers can't read responses cross-origin, so a web page
  can't drive the bridge even by port-guessing (and it never has the token).

## For coding agents

The **Integrations → Coding agents → "Copy API guide"** button copies a
self-contained markdown cheat-sheet of everything above, meant to be pasted
into an agent conversation or saved as a skill/rules file (the local-first
equivalent of OpenWhispr's hosted MCP + `agent-skills/openwhispr-cli`).

## MCP: AI apps read your meetings (`yap.exe mcp`)

Wispr Flow's Notetaker has an MCP page ("Sync Notetaker with your favorite AI
apps… The Wispr MCP does not have access to your dictations") backed by
Wispr's cloud. Yap's is local: the AI app runs **`yap.exe mcp`** on this PC,
and that process reads the meetings from the running Yap through this local
API. No server in between, no account. **Settings → MCP** (the Connections
group, laid out like Wispr's page; Integrations has a "Go to MCP" card) adds
it to an app in one click (`src/lib/McpSection.svelte` →
`src-tauri/src/mcp_clients.rs`); the server is `src-tauri/src/mcp.rs`.

What an AI app can do, and what it can't:

- Read **meetings and notes**: never the dictation history (the
  `/v1/transcriptions` routes aren't reachable from any tool).
- Read only, unless the person switches on **"Let AI apps save notes to Yap"**
  (`config.mcpAllowWrites`, off by default), which adds `create_note`.
  Restart the AI app after changing it (they list tools when they start).
- Only while Yap is open with the Local API on. Otherwise every tool answers
  "Yap isn't open… Open Yap to let your AI read your notes", so the model can
  tell the person what to do. The server re-reads the discovery file on every
  call, so a restarted Yap is found again without restarting the AI app.

What the AI app reads, it sends to its own AI service (Anthropic, OpenAI,
Google…) like anything else you ask it; the page says so. With the Local API
off, the page says that too, with a "Turn on the Local API" button.

### How it runs

- `yap.exe mcp` is the same exe as the app. `main.rs` sees the argument and
  serves MCP **before any of the app starts** (no window, tray, hook, single
  instance check or second bridge), then exits when the AI app closes stdin.
- Release builds are GUI-subsystem exes. They inherit the stdio pipes an AI
  app hands them like any process (checked with Node's `child_process`, as
  Claude Desktop, Cursor, VS Code and Gemini CLI use, with `windowsHide` on
  and off), and never flash a console window. Only protocol messages go to
  stdout; logs go to stderr (`YAP_MCP_LOG` sets the filter), which the apps
  keep in their MCP logs.
- One exe instead of a separate `yap-mcp.exe`: nothing to add to the NSIS
  installer, CI or the updater, and the server is always the same version as
  the app. The NSIS updater closes every running `yap.exe`, `yap.exe mcp`
  included; the AI apps restart their server (the spec says they should).
- Protocol: the official Rust SDK, [`rmcp`](https://crates.io/crates/rmcp)
  (server + stdio only, no macros: four new crates). It serves both protocol
  eras: the `initialize` handshake of 2024-11-05 … 2025-11-25, which every
  client in use speaks, and 2026-07-28's per-request `_meta` with
  `server/discover` (a dual-era client probes that first and falls back).

### Tools

| Tool | Arguments | What the model gets (markdown) |
|------|-----------|--------------------------------|
| `list_meetings` | `limit` (1–100, default 20) | meetings newest first: id, title, local date and time, length, attendees, whether there's an AI summary, how many transcript pages |
| `search_meetings` | `query`, `limit` (1–20, default 5) | matching meetings with their best transcript lines (`[12:04] Them: …`, and the page each is on) and the matching summary line |
| `get_meeting` | `id`, `transcript_page` (default 1) | header (when, length, attendees, folder), the AI summary or action plan, notes typed during the meeting, the rolling AI notes when there's no summary yet, and one page of transcript |
| `search_notes` | `query`, `limit` (1–30, default 10) | all notes (personal and meetings): id, title, type, folder, date, preview |
| `get_note` | `id` | the note's AI-enhanced version and text; for a meeting, a pointer to `get_meeting` |
| `list_folders` | — | folders and how many notes each holds |
| `create_note` | `title`, `content`, `folder` | only with writes allowed: saves a note (source `"mcp"`; a new folder is created) |

A transcript page is at most about 6,000 tokens (`mcp::PAGE_TOKENS`, roughly
20 minutes of talk), so a two-hour meeting arrives in a handful of pages that
each fit any client's context. Pages after the first carry only transcript.
Echo segments (the call leaking into the mic) are left out, as in summaries.
"You" is the person using Yap and "Them" everyone else on the call (not told
apart), and the tool descriptions and server instructions say so.

### Adding Yap to an AI app

Settings → MCP follows Wispr's page: a light card each for **Claude,
ChatGPT, Gemini and Cursor** ("Allow Claude to access your meeting notes and
transcripts", **Add to Claude**, then **Added ✓ · Remove**), then **All
other apps:** with one-click rows for Claude Code, VS Code and Windsurf, the
command and the JSON. Each **Add** writes that app's own config file, as its
docs describe adding a local server (checked 2026-10-05):

| App | File (Windows) | Entry |
|-----|----------------|-------|
| Claude (desktop) | `%APPDATA%\Claude\claude_desktop_config.json`; the MSIX build (today's installer) reads `%LOCALAPPDATA%\Packages\Claude_pzs8sxrjxfjjc\LocalCache\Roaming\Claude\claude_desktop_config.json` instead ([claude-code#26073](https://github.com/anthropics/claude-code/issues/26073)), so every copy that exists is written | `mcpServers.yap = {command, args}` |
| ChatGPT | `~\.codex\config.toml`, shared by the ChatGPT desktop app (Settings → MCP servers), the Codex CLI and IDE extension ([OpenAI docs](https://learn.chatgpt.com/docs/extend/mcp)) | `[mcp_servers.yap]` |
| Gemini | `~\.gemini\settings.json` (Gemini CLI) | `mcpServers.yap` |
| Cursor | `~\.cursor\mcp.json` | `mcpServers.yap` with `type: "stdio"` |
| Claude Code | `~\.claude.json` (user scope, what `claude mcp add --scope user` writes) | `mcpServers.yap` with `type`, `env` |
| VS Code | `%APPDATA%\Code\User\mcp.json` (and `Code - Insiders`) | `servers.yap` with `type: "stdio"` |
| Windsurf | `%APPDATA%\devin\mcp_config.json` (renamed Devin Desktop in June 2026) and the older `~\.codeium\windsurf\mcp_config.json` | `mcpServers.yap` |

**ChatGPT on the web** can't use Yap: it only connects to MCP servers on the
internet. (OpenAI's Secure MCP Tunnel can relay a local server to a business
workspace; Yap doesn't set that up.) The ChatGPT card says so: "In the
ChatGPT desktop app and Codex. ChatGPT on the web can't reach apps on your
PC."

**All other apps** get the command (`"C:\…\yap.exe" mcp`) and the usual JSON:

```json
{
  "mcpServers": {
    "yap": {
      "command": "C:\\…\\yap.exe",
      "args": ["mcp"]
    }
  }
}
```

The edits follow the rules a careful person would by hand:

- **Only the `yap` entry changes.** Every other byte stays: other servers,
  settings, key order and the file's own indentation, line endings and BOM
  (JSON values are carried over as their original text), and comments in
  TOML (`toml_edit`). Removing Yap after adding it gives back the same file.
- A JSON file that **isn't plain JSON** (comments, trailing commas, a typo)
  is left alone, and the card says why ("settings.json isn't plain JSON (line
  2, column 3), so Yap left it alone"); the JSON snippet still works by hand.
- The first edit keeps the original as **`<file>.bak`** (an existing `.bak`
  is never replaced). Writes go to a temp file renamed over the old one.
- `~\.claude.json` is edited under **Claude Code's own lock**
  (`~\.claude.json.lock`, a proper-lockfile directory, abandoned after 10 s):
  Claude Code re-reads the file under that lock before every save, so running
  sessions keep Yap's entry.
- An app says **Added ✓** when every copy of it has a `yap` entry whose exe
  exists (another copy of Yap counts, say the installed app seen from a dev
  build), and **Update** when it points at a Yap that's gone.
- Apps not found on the PC say "Not installed" and offer no button.
- Settings stays mounted while hidden, so the page looks again whenever it
  comes into view (an app installed meanwhile, the Local API switched in
  Integrations).

Known issues in the apps themselves: Codex Desktop on Windows can rewrite
`config.toml` at startup and drop custom servers
([codex#24718](https://github.com/openai/codex/issues/24718), open); the
card then shows "Add to ChatGPT" again. A dev build adds its own
`target\debug\yap.exe`, which can't be rebuilt while an AI app runs it; the
page warns about that in dev builds.

### Testing it

- `cargo test` covers the protocol in-process (`mcp::tests`: a 2025
  handshake, tools/list, every tool against a fake bridge, a 2026-07-28
  client with `server/discover`, Yap closed), the config editing
  (`mcp_clients::tests`: other servers kept, idempotent, clean removal,
  malformed files, BOM/CRLF/tabs, TOML, the `.bak`, Claude Code's lock, a
  stand-in profile) and the real binary (`tests/mcp_stdio.rs`: spawns
  `yap.exe mcp` against a fake bridge).
- `e2e/mcp.spec.js` adds and removes Yap through Settings → MCP in a
  stand-in profile, then runs a real `yap.exe mcp` session against the test
  instance's bridge (see [e2e-tests.md](./e2e-tests.md)).
- Debug builds only: `YAP_BRIDGE_FILE` moves the discovery file (the app and
  `yap.exe mcp` both read it), `YAP_MCP_CLIENT_ROOT` makes a folder the
  stand-in user profile for AI apps' config files, and a test-mode instance
  never touches the real profile even without it.
