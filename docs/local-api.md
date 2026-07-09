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
| GET | `/v1/notes/search?q=&limit=` | keyword scorer (same as the Chat RAG / `search_notes` tool); `limit` default 20 |
| GET | `/v1/notes/{id}` | the full note: raw `content`, `enhancedContent`, `transcript`, `participants`, … |
| POST | `/v1/notes/create` | body `{title?, content?, folder?}`; source is recorded as `"api"` |
| PATCH | `/v1/notes/{id}` | body `{title?, content?, folder?, participants?}`; returns the updated note |
| DELETE | `/v1/notes/{id}` | 204 |
| GET | `/v1/folders/list` | folder names (strings) |
| POST | `/v1/folders/create` | body `{name}`; idempotent per name (case-insensitive) |
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
