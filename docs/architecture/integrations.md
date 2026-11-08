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
  `approve`, `backup`, `reads`, `agents`, `eyes`, `turn`) — **the endpoint Claude Code runs talk back
  to** (2026-11-07): Yap starts the person's own Claude Code for Chat on
  their Claude plan and for [Yap Code](./yap-code.md) (ai-models.md, "The Claude Code
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
  (`Tools::Chat`: read only, plus Yap Code's with `RunSpec::agents`; `Tools::Agent`:
  `approve`), where its backups go,
  and who to ask (`Approval`: the agent's id, its conversation, typed or by
  voice). **A key per run**: 32 random bytes, compared by SHA-256, sent as
  `Authorization: Bearer`. The handle gives the run's `--mcp-config` (Yap's
  server only, `type: "http"`, its `timeout` 660,000 ms: Claude Code's limit
  per tool call, which also lifts its 5-minute idle floor for an HTTP server,
  so a prompt can wait its 10 minutes) and its `--settings` (the backup hook,
  for a run with a backup folder). **The key is never in a file**: both name
  it as `Bearer ${YAP_RUN_KEY}` (the hook lists `YAP_RUN_KEY` in
  `allowedEnvVars`, as an http hook must), and the key itself is only in
  memory and in the run's environment (`RunHandle::env` → `Spec::env`; the
  scrub keeps Yap's own names), which Claude Code fills them in from. Never on
  a command line or in a log. (The run's own commands inherit its
  environment; they already run with the person's own rights.) Dropping the
  handle unregisters the run: its URLs answer 404, and a prompt still
  waiting is denied.
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
- **Yap Code's tools from a plan chat** (`agents.rs`, 2026-11-07): for a Chat run
  registered with `RunSpec::agents` (`with_agents`: agents on, Claude Code here,
  not across the meetings), `start_agent`, `agent_status`, `message_agent` and
  `stop_agent`, listed with the tool loop's own schemas and descriptions, and
  each call run by the tool loop's own code (`chat/tools/agents.rs` `execute`),
  after the same safety rule and the evaluation's intercept. The list is read
  once per session, so a session made without them is started again to get
  them. [yap-code.md](./yap-code.md#from-a-plan-chat).
- **The screen and the pages from a plan chat** (`eyes.rs`, 2026-11-07; the
  one browser toolset since 8 Nov 2026): for a Chat run registered with
  `RunSpec::eyes` (`with_eyes`: the screen look's gate, the pages' and
  pictures', and whether it may act on them, as the tool loop's `Gates` has
  them; never an agent's run), `look_at_screen`, every `browser_*` tool
  (`browser_navigate`, `browser_read`, `browser_find`, `browser_scroll`,
  `browser_scroll_to`, `browser_screenshot`, `browser_zoom`, `browser_tabs`,
  `browser_wait`, `browser_batch`; and `browser_click`, `browser_type`,
  `browser_key`, `browser_hover`, `browser_form_input` while the run may
  act), `find_pictures`, `look_at_picture` and `show_picture` (`all_names`),
  listed with the tool loop's own schemas and descriptions (`readOnlyHint`
  only for the reads that change neither the page nor what's in view —
  `look_at_screen`, `browser_read`, `browser_find`, the pictures' look and
  find —, so Claude Code runs a scroll, a shot or a click in order), each
  call run by the tool loop's own code (`see_screen::execute`,
  `pages::execute`, `pictures::execute`) on the turn's own web side (a
  `web::Turn`: the person's links, the pages Claude Code's searches found,
  `Marks::found`). Refused first for a turn asked away from Chat
  (`Context::eyes`), acting for a turn not typed in Chat (`Eyes::act`), and
  — the screen and acting — during a meeting or a call (`eyes::refused`). The
  result's JSON is the first content block (its `displayText` the chip, a
  batch's `displayTexts` each step's); each picture looked at — the screen's,
  a page's screenshot or zoom, in order — follows as a line saying what it is
  and **MCP image content** (`{"type":"image","data","mimeType"}`), which
  Claude Code 2.1.296 turns into an image block the model reads. What a call
  shows goes to the turn's stream (`Event::Screen`, `Event::Page`,
  `Event::Shot`, `Event::SwitchTab`, `Event::CloseTab`, `Event::Acting`,
  `Event::Confirm`, `Event::Picture`); the screenshots are kept with the
  question, a page's shots with the answer (in its steps). A click, an Enter
  or a box ticked that buys, pays, books, sends, deletes or signs in shows
  the person's card and the call waits for it (two minutes at most), as an
  agent's ask-first card does; a capture or a page's shot takes a few
  seconds — both well inside the run's 11-minute tool timeout.
  [chat-and-assistant.md](./chat-and-assistant.md#chat-on-your-claude-plan-2026-11-07).
- **A Chat run's turn** (`turn.rs`): `RunHandle::begin_turn(Context, web)` gives
  the run its turn — the question, the answer before it, the conversation,
  where it was asked (`AgentsTurn`), web content from the start — and a `Turn`
  whose stream carries what a call shows (the agent's card, its tab) back to
  the plan turn; dropping it ends the turn, and a call waiting on the person
  stops. A call with no turn open does nothing. **The web flag** comes from
  the turn's start, the plan turn's events (`Marks::web`: a `WebSearch` or
  `WebFetch` beginning; `Marks::began`: one of Yap's calls that brings the
  screen's or a page's words in, `eyes::SEES`) and `agent_status`, a look or a
  page read (`Slot::set_web`). **One order for
  two paths**: a call that acts first waits, at most 5 s (`SEEN_WAIT`), for
  its own tool call to show in the turn's events (`Marks::began`,
  `Slot::seen`), since Claude Code writes a tool call's start before it runs
  it; not seen in time, it's refused as after the web.
- **`approve`** (`approve.rs`), an agent's permission prompt tool
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
  card: "Yap can't show questions while an agent runs…". Test mode can answer
  every card at once (`e2e_claude_approve_auto`).
- **The backup hook** (`backup.rs`): before Claude Code edits or writes a
  file, an http `PreToolUse` hook on `Edit|Write|MultiEdit|NotebookEdit`
  (`POST /hook/<run>/PreToolUse`, 60 s `timeout`) copies it into the run's
  backup folder once (the first copy wins), with a `manifest.json`; a file
  that didn't exist is noted as created; one over 50 MB as too big. It always
  answers `{}` at once, never blocking or denying a tool. `restore` (Undo the agent's changes)
  puts them back and deletes what the run created, touching only the paths
  the manifest names. A denied Write is still noted as created.
  [yap-code.md](./yap-code.md#undo-and-backups).
- **Limits**: hooks can be turned off by a managed policy
  (`disableAllHooks`, `allowManagedHooksOnly`, `allowedHttpHookUrls`;
  `httpHookAllowedEnvVars` can keep the key out of the header) and never run
  under `--bare`. Then no
  backups are made: the runner marks an agent whose edits went through with an
  empty backup list (`noBackups`), its tab says "No backups were made, so
  Undo can't put these files back.", and Settings → Yap Code says Undo needs
  Claude Code's hooks ([yap-code.md](./yap-code.md#undo-and-backups)). The
  endpoint's requests aren't in the network receipts (they never leave this
  PC).
- **The log** has ids, counts, tool names and outcomes, never a command, a
  file's path or contents, a prompt or a key.
- Tested in `claude_live/tests.rs` (the listener's refusals, the MCP subset,
  the reads, `approve`'s card and its ends, the hook, restore, the agent tools
  for a turn, the screen's and the pages' tools listed, allowed and refused,
  the key named in the files and only in the run's environment) and through the stand-in in the agents and plan e2e specs
  (`plan-agents.spec.js` for the agent tools over the endpoint, `plan-eyes.spec.js`
  for the screen and the pages: the stand-in's `_mcp` `dump` says what it got,
  image content as type and size).

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
