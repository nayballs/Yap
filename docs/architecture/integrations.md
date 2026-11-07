# Integrations: local API and MCP

The token-authenticated local API, Yap's MCP server (`yap.exe mcp`) and adding it to AI apps' configs, plus the Integrations view. See also [`docs/local-api.md`](../local-api.md).

[Architecture index](./README.md) · [CLAUDE.md](../../CLAUDE.md)

## Modules

- **`integrations/bridge.rs`** — the **local API bridge** (OpenWhispr `cliBridge.js` port,
  backs the Integrations view): a token-authenticated loopback HTTP server
  (`tiny_http`, `127.0.0.1`, OS-assigned port) exposing `/v1` REST routes over
  notes/folders/history so terminals + coding agents can drive Yap data.
  Discovery = `~/.yap/cli-bridge.json` `{version, port, token}` (fixed path,
  written on start, deleted on exit); auth = `Bearer <token>` (SHA-256
  constant-time compare); note mutations emit `yap-notes-changed` so NotesView
  refreshes live. Toggled by `config.bridge_enabled` (default on; `sync()`
  runs at setup + every config save). A second Yap (dev next to installed)
  leaves a live bridge's file alone, and `stop()` only deletes the file while
  it's still ours. Also serves the MCP server's meeting views
  (`/v1/meetings/list`, `/v1/meetings/search`, `/v1/mcp/config`; built in
  `integrations/mcp.rs`) and the tasks in notes' summaries, read only (`/v1/tasks/list?
  status=open|done|all`: note, owner, task, due, done, when it was said;
  `notes/tasks.rs`). Debug builds take `YAP_BRIDGE_FILE` to move the discovery file
  (the e2e suite). See `docs/local-api.md`.
- **`integrations/mcp.rs`** — **Yap's MCP server** (Wispr Flow's Notetaker MCP, local): AI apps
  launch **`yap.exe mcp`** (main.rs sends that argument here before any of the app
  starts: no window, tray, hook or single-instance check; release builds are
  GUI-subsystem and inherit the client's stdio pipes fine) and speak MCP over
  stdin/stdout via `rmcp`, which serves both eras (the 2025 `initialize` handshake
  and 2026-07-28's per-request `_meta` + `server/discover`). The process holds no
  data: every tool calls the **running** Yap's local API (discovery file re-read per
  call), so notes.json keeps one writer; with Yap closed or the Local API off, tools
  answer "Open Yap to let your AI read your notes". Tools, meetings + notes only,
  **never dictation history**: `list_meetings`, `search_meetings` (matching
  transcript lines + page), `get_meeting` (AI summary/action plan, typed notes,
  digests when there's no summary, transcript in ~6k-token pages via
  `transcript_page`), `search_notes`, `get_note`, `list_tasks` (the action plans'
  tasks by meeting, open by default, whether ticked done, "said at 0:15"; read
  only), `list_folders`, and `create_note`
  only when `config.mcp_allow_writes` (Settings → AI apps (MCP); source `"mcp"`). Markdown
  for the model, local-time dates (chrono). Transcript lines carry the
  speaker map's names (`speakers::display_line`: "You", "Them", "Priya",
  "Priya?" for a guess, "Speaker 2"), and the instructions/page explain the
  labels whether or not voices are told apart (`who_is_who`); the local API's
  `/v1/notes/<id>` hands out `speakers` and each line's `speaker` as stored.
- **`integrations/mcp_clients.rs`** — "Add to Claude / ChatGPT / Gemini / Cursor…" (Settings →
  MCP): edits each AI app's own config to launch `yap.exe mcp`. Claude desktop
  (`%APPDATA%\Claude\claude_desktop_config.json` **and** the MSIX build's
  `%LOCALAPPDATA%\Packages\Claude_*\LocalCache\Roaming\Claude\…`), ChatGPT desktop +
  Codex (`~/.codex/config.toml`, `[mcp_servers.yap]` via toml_edit), Gemini CLI
  (`~/.gemini/settings.json`), Cursor (`~/.cursor/mcp.json`), Claude Code
  (`~/.claude.json`, under Claude Code's own proper-lockfile lock), VS Code
  (`%APPDATA%\Code\User\mcp.json`, `servers`) and Windsurf/Devin Desktop. Only the
  `yap` entry changes (other entries kept as their original text, so key order,
  indent, CRLF and BOM survive; remove = same bytes back), non-plain JSON is left
  alone with the reason, `.bak` on the first edit, temp-file + rename writes.
  Detects installed apps; status added / available / outdated (exe gone) /
  missing. Commands `mcp_clients_status`, `mcp_client_add`, `mcp_client_remove`
  (async, blocking IO off the main thread). Debug builds take
  `YAP_MCP_CLIENT_ROOT` as a stand-in user profile; test mode never touches the
  real one. ⚠ The exe written is this one, **except in a dev build** (debug, not a
  test run): there it's the installed Yap's (`installed_exe`, the uninstall
  entry's `DisplayIcon`) when one is installed — an AI app keeps `yap.exe mcp`
  running, and a child of `<drive>:\Builds\yap-dev\debug\yap.exe` locks it, so Yap - Dev's next
  build fails with "Access is denied" (2026-10-17, Claude Code's `~/.claude.json`).
  The launcher (`dev-update.ps1`) also stops any such helper before building.
- **`integrations/claude_live.rs`** (+ `claude_live/`: `server`, `rpc`,
  `approve`, `backup`, `reads`) — **the endpoint Claude Code runs talk back
  to** (2026-11-07): Yap starts the person's own Claude Code for Chat on
  their Claude plan and for [Jobs](./jobs.md) (ai-models.md, "The Claude Code
  engine"), and those runs reach this. Below.

## Claude Code runs' endpoint (`integrations/claude_live.rs`, 2026-11-07)

A loopback HTTP listener inside the running Yap. Unlike `yap.exe mcp` it needs
no Local API (the bridge can be off), and it reaches the live UI.

- **The listener** (`server.rs`): `127.0.0.1` on a port the system picks,
  started the first time a run is registered, then kept while Yap runs. A
  request is refused unless it comes from this PC, names this PC in its
  `Host` (a web page on a name that resolves here can't reach it), carries no
  other site's `Origin`, and names a registered run with that run's key (403,
  404, 401). No CORS headers, ever. Each request runs on a thread of its own,
  so a permission prompt waiting for the person never holds up a read.
- **A run** (`register_run(RunSpec)` → `RunHandle`): its label, its tools
  (`Tools::Chat`: read only; `Tools::Job`: `approve`), where its backups go,
  and who to ask (`Approval`: the job's id, its conversation, typed or by
  voice). **A key per run**: 32 random bytes, compared by SHA-256, sent as
  `Authorization: Bearer`. The handle gives the run's `--mcp-config` (Yap's
  server only, `type: "http"`, its `timeout` 660,000 ms: Claude Code's limit
  per tool call, which also lifts its 5-minute idle floor for an HTTP server,
  so a prompt can wait its 10 minutes) and its `--settings` (the backup hook,
  for a run with a backup folder). The key is in those two files in the run's
  folder under `%TEMP%\yap-claude\` (removed with the session) and in memory,
  never in a log. Dropping the handle unregisters the run: its URLs answer
  404, and a prompt still waiting is denied.
- **MCP over HTTP** at `POST /mcp/<run>` (`rpc.rs`): the Streamable HTTP
  transport's request/response subset, JSON answers only, no sessions and no
  SSE stream (`GET` is 405, which the transport allows). Claude Code 2.1.296
  first probes with `server/discover` (protocol 2026-07-28); Yap answers
  "method not found", and it falls back to `initialize` with 2025-11-25,
  which Yap echoes. Then `tools/list`, `tools/call`, `ping`.
- **Chat's tools** (`reads.rs`): `yap.exe mcp`'s seven reads — meetings,
  notes, tasks and folders, the same views through `mcp::run_tool` over any
  `Bridge`, here read in process (`bridge::read_route` / `read_path`, without
  the HTTP) —, Yap's memory (`search_memory`) and the calendar
  (`check_calendar`, while one is connected). Read only; never the dictation
  history. `RunHandle::allowed_tools` names them for `--allowedTools`, so a
  plan chat in `dontAsk` can use them.
- **`approve`** (`approve.rs`), a Job's permission prompt tool
  (`--permission-prompt-tool mcp__yap__approve`). When nothing else decides a
  tool call, Claude Code calls it with `{tool_name, input, tool_use_id}` and
  waits for `{"behavior":"allow","updatedInput":…}` or `{"behavior":"deny",
  "message":…}`. A run started with the flag keeps it out of the model's own
  tools (checked on 2.1.296), so only Claude Code calls it. **The person
  decides**: a card (`yap-claude-approve`) in plain words ("Run a command:
  npm test", "Edit file: src/app.ts"), Allow once / Deny
  (`claude_approve_answer`; `claude_approve_pending` lists the waiting ones).
  No answer in 10 minutes, the run ended, or Claude Code cancelled the call:
  a no, said to Claude in words it can put in its report. Every card's end
  goes out as `yap-claude-approve-done`. `AskUserQuestion` is denied with no
  card: "Yap can't show questions while a job runs…". Test mode can answer
  every card at once (`e2e_claude_approve_auto`).
- **The backup hook** (`backup.rs`): before Claude Code edits or writes a
  file, an http `PreToolUse` hook on `Edit|Write|MultiEdit|NotebookEdit`
  (`POST /hook/<run>/PreToolUse`, 60 s `timeout`) copies it into the run's
  backup folder once (the first copy wins), with a `manifest.json`; a file
  that didn't exist is noted as created; one over 50 MB as too big. It always
  answers `{}` at once, never blocking or denying a tool. `restore` (Undo job)
  puts them back and deletes what the run created, touching only the paths
  the manifest names. A denied Write is still noted as created.
  [jobs.md](./jobs.md#undo-and-backups).
- **Limits**: hooks can be turned off by a managed policy
  (`allowedHttpHookUrls`) and never run under `--bare`. The endpoint's
  requests aren't in the network receipts (they never leave this PC).
- **The log** has ids, counts, tool names and outcomes, never a command, a
  file's path or contents, a prompt or a key.
- Tested in `claude_live/tests.rs` (the listener's refusals, the MCP subset,
  the reads, `approve`'s card and its ends, the hook, restore) and through
  the stand-in in the Jobs and plan e2e specs.

## The Integrations view

- **`IntegrationsView.svelte`** = the
  Integrations surface (OpenWhispr `IntegrationsView.tsx`, local-first cut):
  a **Calendar** card (`CalendarLinkCard.svelte`: Connect → the "Connect
  your calendar" dialog, Manage → Settings → Connectors),
  an **AI apps (MCP)** card (`McpLinkCard.svelte`, Wispr's "Go to MCP" row:
  opens Settings → AI apps (MCP) via ControlPanel's `openSettings`), the Local API card
  (enable toggle + live status/port via `bridge_status`, discovery-file path +
  curl example with copy; the toggle tells Settings' copy via
  `yap-config-patched` so its auto-save can't switch it back), a
  Coding-agents card whose "Copy API guide" button copies a
  paste-into-your-agent endpoint cheat-sheet, and an endpoint reference
  table, folded into a "For developers" disclosure (2026-10-22: it was the
  longest thing on the page for people who never use it). (OpenWhispr's Google-Calendar OAuth / cloud API-keys / hosted-MCP
  cards need their paid cloud and are not ported; Yap's MCP is local.)
