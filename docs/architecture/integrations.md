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
- **`integrations/connectors.rs`** (+ `connectors/`: `store`, `cache`, `vault`,
  `manager`, `local`, `remote` + `remote/legacy`, `names`, `schema`, `results`,
  `call`, `fake`, `tests`) — **connectors (preview, 2026-11-08)**: the other
  direction, Yap as an MCP client of servers the person adds. Below.

## Connectors (preview)

Yap as an **MCP client**: the person adds a server at a link or a program on
this PC, and Yap keeps its tools for Chat, the Voice Assistant and plan chats
to use. Waves 1 (the core and programs) and 2 (links with tokens) are built;
signing in through the browser (3), the tools in Chat's loop (4), Settings →
Connectors (5) and the rest come after. **Nothing offers a connector's tools
to a model yet.**

- **The switch** (`connectors::enabled`): `connectors_preview` in config.json
  (off by default), or `YAP_CONNECTORS=1` in a debug build (the e2e suite
  sets it per spec). Off, no command reads or writes a connectors file,
  starts a program or makes a request: `connectors_list` answers `{enabled:
  false, connectors: []}` and the rest refuse ("Connectors aren't switched on
  in this Yap yet."). Unit tests check that nothing is read.
- **The SDK**: `rmcp` 3.9 as a client too (`client`,
  `transport-child-process`, `transport-streamable-http-client`; never its
  `…-reqwest` or `auth` features, so Yap's own reqwest 0.12 makes every
  request). Both MCP eras through `ClientLifecycleMode::Auto`
  (`server/discover`, else `initialize` at 2025-11-25). Yap declares no
  sampling, roots or elicitation (`manager::Handler`); a server's
  `instructions` are never kept.
- **The store** (`store.rs`, `connectors.json`): connectors `c1`…`c24` (the
  lowest free id, so the vault's names are bounded), a slug each
  (`names::slug`: `[a-z0-9]`, a letter first, at most 12, unique), the
  transport (a `remote` link, or a `local` program with its resolved path
  and the fingerprint the person allowed), the auth kind, where Yap may use
  it (chat, voice, plan on; phone, meetings off), and **each tool's setting**
  — `allow` (Always allow), `ask` (Ask first) or `off` — pinned to the hash
  of the definition it was set for. Fields this build doesn't know are kept.
  [config-and-data.md](./config-and-data.md).
- **The tools** (`cache.rs`, `connector_tools.json`): each connector's tools as
  last listed (all pages; at most 500), each with its **kind** from the
  annotations — `readOnlyHint` → Reads, `destructiveHint: false` → Changes,
  anything else (none at all too) → Deletes —, its **hash** (name, title,
  description, schema and annotations as sent, keys in order), its cost in
  tokens, a **flag** when its description reads like orders to the model
  (ignore the rules, keep it from the user, `<important>`, `.ssh`, the
  system prompt) and a **skip** reason when its schema can't be made safe.
  **Defaults** (`cache::apply`): Reads → Always allow, Changes and Deletes →
  Ask first, flagged or skipped → Off. A **changed** tool (another hash: a
  rug pull) that was Always allow drops to Ask first; a removed one goes; a
  tool that can delete is never Always allow. Listed when it's added, on Try
  again, and before the first call of a new connection (`connectors::ensure`).
- **Names** (`names.rs`): a model would see `<slug>__<tool>`, the tool's
  name with anything but `[A-Za-z0-9_-]` made `_`, cut to 64 with a
  4-character hash when longer or when two come out alike: what OpenAI,
  Anthropic, Gemini and Groq all take. Yap's own tools never have `__`, so a
  server can't shadow one; the way back is `names::Map`, never the text.
- **Schemas** (`schema.rs`): local `$ref`s resolved (a remote one refused, a
  recursive one cut), `type: object` ensured, type arrays → one type +
  `nullable`, `const` → a one-string `enum`, `oneOf` → `anyOf`, `allOf`
  merged, only the keywords every provider reads kept (Gemini's subset: no
  `additionalProperties`, `examples`, `$schema`, `if`…), `required` only of
  properties there are, an array given `items`, depth 10 and 16 KB at most.
- **Results** (`results.rs`): Yap's tool-result shape (`{success, data:
  {text, structured?, pictures?}, displayText}` or `{success: false,
  error}`): text joined, `structuredContent` when there's no text, an
  embedded text resource read, a binary one and a `resource_link` named, a
  picture kept aside (for a model that reads images, wave 4) and named
  ("[a picture, 640×480]"), audio dropped, `isError` → failure, cut at 1 MB.
- **A call** (`call.rs`): 60 s by default (a connector's own up to 10 min), a
  progress notification starting the clock again (never past 30 min), a
  `Stop` (■ Stop, a newer question) or a dropped future ending it, and the
  server told either way (`notifications/cancelled`: rmcp for its own
  timeout, `call::Abandon` for the rest). Never retried: a broken connection
  is "Didn't hear back from … It may or may not have done it." Two turns
  share one connection, each call its own request.
- **The manager** (`manager.rs`): one connection per connector, made when
  needed (at most 30 s for a program, 15 s for a link — a test run's
  `YAP_E2E_CONNECTOR_START_MS` shortens a link's), shared by callers, let go
  after 10 idle minutes (a sweep every 30 s: a program's stdin closed, 3 s,
  then its job ended), **three failures in 10 minutes** (a failed start, a
  connection that ended by itself) → Stopped until Try again. Every
  connection stops at Quit (`shutdown_cleanup`, within 3 s). Statuses in
  plain words — "Connected · 9 tools", "Starts when Chat needs it",
  "Starting…", "Needs sign-in…", "Couldn't connect: …", "Couldn't start: …",
  "Stopped by itself 3 times in 10 minutes…" —, each change emitted. Its
  clock and its way to connect are passed in, so its lifecycle is tested on
  a fake clock against `fake.rs`.
- **Programs** (`local.rs`): found on `PATH` with `PATHEXT` (`npx` →
  `npx.cmd`) or as given; **its exact command line shown before it first
  runs** — `connector_preview` gives it resolved and whole, with warnings
  (your permissions; `npx -y`/`uvx` fetching the latest version each start;
  a shell's `-c`; downloads; deletes; folders outside yours) and a
  fingerprint, and `connector_add` takes a program only with that
  fingerprint; one whose command line no longer matches never starts. Yap's
  own exe with `mcp` is refused. **Started** by rmcp's child-process
  transport on process-wrap: a **job object of its own** (the child started
  suspended, assigned, resumed, so a `node.exe` that `npx.cmd` starts can't
  slip out), `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` (Quit, a crash or End task
  end the whole tree), no window (`CREATE_NO_WINDOW`). A `.cmd` goes
  through Rust's batch escaping. Its environment is the person's minus
  `YAP_*`, plus its own variables (a secret one's from the vault; names
  with TOKEN, KEY, SECRET, PASSWORD, PAT… are secret by default); its
  folder `<data>/connectors/c<N>/`; its last 64 KB of stderr in memory only
  (Show details), never logged. No sandbox: the preview says so.
- **Links** (`remote.rs`): rmcp's Streamable HTTP transport on **Yap's own
  reqwest, every request through `net.rs`** ("Connector · A connector"; the
  catalogue's name later, never one the person typed;
  [privacy-receipts.md](./privacy-receipts.md)). The rules: `https://`, or
  `http://` only to this PC or the person's network; no credentials in the
  address; never a cloud metadata address; **private addresses only when
  the person typed one** — a link to a name on the internet is resolved by
  a guard that refuses it when any address it gets is on this PC or a
  network (checked as the connection is made, so a DNS answer that changes
  can't slip one in; `web::is_public`, MCP's SSRF advice), while a link to
  `localhost`, a private address or a `.local` name reaches it, straight
  (no proxy); **redirects** only `307`/`308` to the same origin, at most 5,
  anything off the server's host refused ("… sent Yap on to …"); a JSON
  answer or one event over 1 MB refused. **Auth**: a bearer token
  (`Authorization: Bearer`) or request headers, their values in the vault,
  only ever sent to that server; a 401 to a token or a header is "turned
  the token down", never a sign-in; a 401 with a challenge and nothing sent
  is "Needs sign-in" (wave 3 starts there). A `server/discover` a 2025
  server refuses with a 4xx comes back as a JSON-RPC error so Auto falls
  back. **The older HTTP+SSE** (`remote/legacy.rs`, which rmcp no longer
  has): for a link marked so, or when the first POST answers 404 or 405 —
  `GET` the stream, its `endpoint` event (same origin only), each message
  `POST`ed there, answers read off the stream; the same receipts and rules.
- **The vault** (`vault.rs`): one connector's secrets as one JSON value in
  Windows Credential Manager, `yap-connector-c<N>-<part>.com.yap.dictation`
  (1,100-character parts, at most 4), never in a file, the log or the page
  (`Secrets`' `Debug` names which there are); the uninstaller removes all
  24 × 4 names. A test run keeps them in its data folder's
  `connector-secrets.e2e.json`; unit tests in memory, and one test uses the
  real store under names of its own and removes them.
- **Commands** (wave 5's Settings and wave 4's loop build on them):
  `connectors_list`, `connector_preview {spec}`, `connector_add {spec}` (a link —
  `url`, `token`, `headers`, `sse`, `fallbackSse` — or a program —
  `command`, `args`, `env`, `approved` —; then connected and listed, kept
  with its status if that fails), `connector_remove {id}` (stopped, its
  tools and secrets gone), `connector_set_enabled {id, enabled}`,
  `connector_refresh_tools {id}` (Try again: a fresh connection, listed;
  what changed), `connector_set_tool {id, tool, setting}`,
  `connector_details {id}` (server, address, stderr), and in debug builds
  only `connector_call_tool {id, tool, args, timeoutMs}` (a tool called
  directly, Off and unknown ones refused: the tests' and a debug check's).
  **Events**: `yap-connectors-changed` (any change) and
  `yap-connector-status` (`{id, state, line, …}`).
- **Never another way in**: only those commands add a connector; Chat has
  no tool for it, a page beside the chat has no Yap IPC, the deep link takes
  none.
- **The log**: the connector's id, outcomes, timings, counts and the tool's
  name at `info`; never its name, a path, a command line, arguments, a
  result, stderr or a secret (`log_privacy.rs` holds `args`, `arguments`,
  `result_text` and `stderr` to it).
- **Tests**: unit tests per part (names, schemas, results, the store, the
  cache's defaults and rug pulls, the vault — and the real Credential
  Manager under test names —, a command line's quoting, fingerprint and
  warnings, **a grandchild ended with the program's job**, the lifecycle on
  a fake clock, calls' timeouts, progress, stops and dropped futures against
  `fake.rs`'s server, links against its HTTP fake — headers, a token turned
  down, a sign-in wanted, a redirect off the host, no answer, slow, the
  older HTTP+SSE, a name that resolves to this PC —, the receipts' purpose,
  and the switch off reading nothing); and `connectors-core.spec.js` with
  `desktop/e2e/support/fake-mcp.js` (stdio, and `--http` with `--token`,
  `--redirect`, `--hang`, `--slow-ms`) through the commands.

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
