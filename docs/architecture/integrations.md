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
  `call`, `catalog` + `catalog.json`, `import`, `privacy`, `fake`, `tests`) — **connectors
  (preview, 2026-11-08)**: the other direction, Yap as an MCP client of
  servers the person adds. Below.

## Connectors (preview)

Yap as an **MCP client**: the person adds a server at a link or a program on
this PC, and Yap keeps its tools for Chat, the Voice Assistant and plan chats
to use. Waves 1 (the core and programs), 2 (links with tokens), 3 (signing
in through the browser), 4 (the tools in Chat's loop,
[below](#connectors-in-chat-wave-4)), 5 (Settings → Connectors and its
catalogue, [below](#settings--connectors-wave-5-2026-11-08)), 6 (everywhere
else in Chat: the composer's per-chat switches, the Voice Assistant's spoken
ask, the phone's reads, the meeting rule, and the preview's own switch in
Settings: [below](#everywhere-else-in-chat-wave-6)), 7 (plan chats, through
Yap's run endpoint: [below](#connectors-in-plan-chats-wave-7)) and 8 (privacy
and evaluations: [below](#connectors-and-your-data-wave-8)) are built.

- **The switch** (`connectors::enabled`): `connectors_preview` in config.json
  (off by default; shown as **Connectors (preview)** on Settings →
  Connectors since wave 6), or `YAP_CONNECTORS=1` in a debug build (the e2e
  suite sets it per spec). Off, no command reads or writes a connectors file,
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
  of the definition it was set for, where it came from (`origin`: `custom`,
  `catalog` with the entry's id and the catalogue's version, or `import`
  with `from`: `claude-desktop` | `paste`) and when a tool of it was last
  called (`lastUsed`, saved at most once a minute). Fields this build
  doesn't know are kept. [config-and-data.md](./config-and-data.md).
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
  is "Needs sign-in" (the browser sign-in starts there, below). A `server/discover` a 2025
  server refuses with a 4xx comes back as a JSON-RPC error so Auto falls
  back. **The older HTTP+SSE** (`remote/legacy.rs`, which rmcp no longer
  has): for a link marked so, or when the first POST answers 404 or 405 —
  `GET` the stream, its `endpoint` event (same origin only), each message
  `POST`ed there, answers read off the stream; the same receipts and rules.
- **Signing in through the browser** (`oauth.rs` + `oauth/`, `refresh.rs`;
  wave 3, 2026-11-08): the MCP authorization spec (revision 2026-07-28) in
  Yap's own code on its own reqwest — not rmcp's `auth` feature, which
  brings another reqwest and its own HTTP —, every request through `net.rs`
  as "Connector sign-in · A connector"
  ([privacy-receipts.md](./privacy-receipts.md)) on the connector's own
  place rules (a server on the internet signs in over `https://` on the
  internet only, a name resolving to this PC or the network refused as the
  connection is made; one on this PC or the network may use `http://`
  there), no redirects followed, answers read to 256 KB, 20 s each.
  - **Discovery** (`oauth/discover.rs`): the server asked without a token
    (an `initialize` POST); its `401`'s `WWW-Authenticate: Bearer
    resource_metadata=…, scope=…` (a Bearer challenge among others, quoted
    or bare); its **protected resource metadata** (RFC 9728) from the
    challenge's address, else `/.well-known/oauth-protected-resource/<path>`,
    then the root one — its `resource` must be this server (the same origin,
    the server's path or a parent of it), or Yap stops ("…'s sign-in is for
    another server (…), so Yap stopped there"); the **authorization
    server** must be one it names, and its metadata (RFC 8414 with the
    issuer's path inserted, OpenID with it inserted, then appended) must
    name that same issuer (a trailing slash aside), offer PKCE with **S256**
    (without `code_challenge_methods_supported`, refused: "doesn't meet the
    MCP security rules"), and point only at endpoints a sign-in may use (an
    endpoint on another host is fine: some issuers' token endpoints are).
  - **The client** (`oauth/client.rs`), in the spec's order: a
    **pre-registered** one — the person's own, entered by hand
    (`connector_set_client`: their explicit choice, so first; its secret in
    the vault), else Yap's own app with that vendor from the catalogue (the
    hook, `client::catalog_client`, with an empty list until wave 5b) —;
    **Yap's client ID metadata document** (CIMD) when the server says
    `client_id_metadata_document_supported` — its URL is the client id,
    nothing registered; `client::METADATA_URL` is unset until the maintainer
    approves where it's hosted (skipped while unset; a test run's
    `YAP_E2E_CIMD_URL`), and `client::metadata_document` is the file —;
    **dynamic client registration** (RFC 7591) at the server's
    `registration_endpoint`: `client_name` "Yap", `application_type:
    "native"`, `token_endpoint_auth_method: "none"`, the code and refresh
    grants, the loopback's redirects (`http://127.0.0.1:51439/connectors/callback`,
    the port this sign-in listens on if it's another, and the port-less
    `http://127.0.0.1/connectors/callback` for servers that allow any port);
    kept in connectors.json **for that issuer only**, before the sign-in
    goes on (never registered twice), reused, dropped when the issuer
    changes or the server no longer knows it (`invalid_client`); else
    **ask**: "… needs an app registration Yap doesn't have. Ask its admin for
    a client ID (with http://127.0.0.1:51439/connectors/callback as its
    redirect)…". A pasted token (`Spec::token`) never comes here.
  - **The browser** (`oauth/loopback.rs`): the authorize address with
    `response_type=code`, the client id, the redirect, a 32-character
    `state`, PKCE (a 32-byte verifier, its S256 challenge), the `resource`
    (RFC 8707: the protected resource metadata's) and the **scopes** — the
    challenge's, else the resource's `scopes_supported`; on a step-up those
    asked for before and the ones the server asked for since; plus
    `offline_access` when the authorization server lists it and there's
    anything to ask for —, opened with `shell::commands::open_link` (the
    person's chosen browser, `shell/browsers.rs`; a test run keeps it for
    `e2e_opened_urls`), never in a Yap webview. **The loopback**:
    `127.0.0.1` on the fixed port **51439** (beside Vite's 51437 and Link's
    51443), else any free port; only a `GET` from this PC with this
    listener's `Host` at `/connectors/callback` carrying this attempt's
    `state` once (one code, one `iss`) — anything else a 404 —; one answer,
    then it closes; 10 minutes at most, or Cancel; each answer's connection
    closed (the ChatGPT sign-in's lesson). **`iss`** (RFC 9207): another
    issuer's, or none when the server says it always sends one, and the
    code is never used and nothing from the answer is shown ("The browser's
    answer didn't come from the sign-in server Yap asked…"); a denial says
    Yap's words for its error code (`access_denied`: "You didn't allow Yap
    to use …"), never the server's text. The page: "Signed in · You're
    signed in to <name>. You can close this tab and go back to Yap.", or
    "Not signed in" and why.
  - **Tokens** (`oauth/tokens.rs`): the code swapped with the verifier,
    the redirect and the resource; a public client sends its id, one with a
    secret authenticates as it was registered or as the server lists first
    (`client_secret_basic`, `client_secret_post`). Only a Bearer token is
    taken. The **grant** goes to the vault (`vault::Grant`: the access and
    refresh tokens, when they run out and were issued, the scopes, and the
    issuer, resource and client they're bound to), the sign-in to
    connectors.json (`auth`: issuer, resource, client, scopes, `signedIn`,
    the endpoints); `auth.kind` becomes `oauth`. Then the connection starts
    afresh and the tools are listed.
  - **Using them** (`oauth::access_token`, `oauth::Source`): every request
    of an `oauth` connector's connection carries its access token
    (`remote::Http::with_sign_in`) — only if the grant was issued for this
    server's resource, so a changed address never gets it —, refreshed **30
    s before it runs out** (half its life for one that lasts under a
    minute; one that says no expiry, when it's refused). **One refresh at a
    time across Yaps** (`refresh.rs`: a mutex per connector, then
    `<data>/connectors/c<N>.refresh.lock`, waited for up to 15 s), and the
    vault read again once it's held: another Yap's newer tokens are used as
    they are, so a rotating refresh token is never spent twice; the next
    one is kept at once. A **401** to the token: one refresh, the request
    sent again (the server did nothing with it), a second 401 ends the
    sign-in. A refresh turned down (`invalid_grant`, `invalid_client`) ends
    it too: the tokens leave the vault, the state is **expired** ("Your
    sign-in to … has ended. Sign in again."), the call says so. A **`403
    insufficient_scope`** keeps the scopes the server wants (`auth.wants`):
    "… needs more access for this. Sign in again.", and the next sign-in
    asks for them with the ones before; once per scope (asked for already:
    given up, the call just fails).
  - **Signing out** (`oauth::sign_out`): under the refresh guard, the
    refresh token revoked (RFC 7009, the access token when there's none)
    where the server has a `revocation_endpoint` — `confirmed: false` when
    it hasn't or refused, so the person is told to disconnect Yap on the
    service's page too —, the tokens gone from the vault; the registration
    and the person's own client stay, so signing in again is one click.
    Removing a connector revokes first (10 s at most).
  - **The states** (`oauth::SignInState`, `signIn` on `yap-connector-status`
    and in the view): `needs-sign-in` (a link whose server wants a sign-in,
    an `oauth` one without tokens, or wanting more access), `signing-in`
    ("Signing in… Finish in your browser."), `signed-in`, `expired`. **One
    sign-in at a time**: a new one cancels the one before. A program, a
    token or a header connector doesn't sign in in the browser.
  - **Nothing secret logged at any level**: the connector's id, outcomes,
    counts and an error code only; `oauth::tests::the_sign_in_never_logs_a_secret`
    reads the sign-in's files and fails on a log call formatting a token, a
    code, a verifier, a secret, the state or an address.
- **The vault** (`vault.rs`): one connector's secrets as one JSON value in
  Windows Credential Manager, `yap-connector-c<N>-<part>.com.yap.dictation`
  (1,100-character parts, at most 8 — room for a sign-in's two JWTs and a
  client secret), never in a file, the log or the page (`Secrets`' and
  `Grant`'s `Debug` name which there are); the uninstaller removes all 24 ×
  8 names ("Delete the application data", below). A test run keeps them in its data folder's
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
  `connector_details {id}` (server, address, stderr), the sign-in's —
  `connector_sign_in {id}` (answers once it's over: `{outcome: "signed-in" |
  "cancelled", connector}`, a failure its error in plain words),
  `connector_sign_in_cancel {id}`, `connector_sign_out {id}` (`{confirmed,
  connector}`), `connector_set_client {id, clientId, secret?}` (the person's
  own client; an empty id forgets it) —, the catalogue's and the imports'
  (below) — `connector_catalog`, `connectors_import_scan {source}`,
  `connectors_import {source, picks}`, and `connector_add`'s `catalog` —,
  and in debug builds
  only `connector_call_tool {id, tool, args, timeoutMs}` (a tool called
  directly, Off and unknown ones refused: the tests' and a debug check's).
  A view has `signIn` for a link that signs in in the browser (`{state,
  host, client, ownClientId, hasOwnSecret, scopes, redirectUri}`: the
  sign-in server's host the browser opens, the client the last sign-in used,
  the redirect a hand-entered client registers), and `origin`, `catalog`
  (the entry it came from) and `lastUsed`.
  **Events**: `yap-connectors-changed` (any change) and
  `yap-connector-status` (`{id, state, line, signIn?, …}`: `signIn`
  "needs-sign-in" | "signing-in" | "signed-in" | "expired", and the line
  saying it).
- **Never another way in**: only those commands add a connector; Chat has
  no tool for it, a page beside the chat has no Yap IPC, the deep link takes
  none.
- **The log**: the connector's id, outcomes, timings, counts and the tool's
  name at `info`; never its name, a path, a command line, arguments, a
  result, stderr or a secret (`log_privacy.rs` holds every connectors module
  to it: wave 8, below).
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
  `--redirect`, `--hang`, `--slow-ms`) through the commands. **The sign-in**:
  unit tests of the challenge, the metadata addresses' order, the resource
  and issuer checks, S256, the endpoints' place rules, the order of client
  choice, Yap's metadata document and its registration, PKCE (RFC 7636's
  vector), the authorize address, `state` and `iss`, the loopback's one
  answer, token responses and client authentication, the refresh margin,
  the scopes, one sign-in at a time and the refresh guard (a unit test's
  loopback listens on any free port, never the fixed 51439 a running Yap's
  sign-in or the e2e suite's needs — `loopback::first_port` —, except the
  one test of the app's own order, which passes with it busy too); and against
  `fake.rs`'s OAuth fake (an MCP server with its own authorization server)
  the whole sign-in — registered once and reused, PKCE checked, the tokens
  in the vault and in no file, calls carrying them, the receipts —, two
  refreshes at once becoming one, another Yap's newer tokens used, a 401
  refreshed and rotated, a revoked grant ending the sign-in, step-up, an
  answer with another `iss` never used, Cancel, a grant never sent to
  another address, sign-out revoking, and the person's own client. And
  `connectors-signin.spec.js` with `desktop/e2e/support/fake-mcp-oauth.js`
  (in every mode: dynamic registration, Yap's metadata document, a
  hand-entered client with its secret, a short-lived token refreshed with
  its refresh token rotating, a revoked token and a revoked grant, step-up,
  another `iss`, metadata naming another issuer, a resource for another
  server, no S256, a decline, Cancel, metadata only where the challenge
  says, OpenID for an issuer with a path, a slow server, the fixed port
  busy), the browser played by the test: the address Yap would open, the
  fake's consent approved, the loopback opened.

### Connectors in Chat (wave 4)

Chat's tool loop offers the connectors' tools (`chat/tools/connectors.rs`,
its `Source::Connectors`: [chat-and-assistant.md](./chat-and-assistant.md)),
each call through `consent.rs` and, when it asks, a card in the thread.

- **Who gets them** (`connectors::may`, `Gates::connectors`): the preview on,
  a **strong cloud model** (`documents::rich::strong`; never a model on this
  PC until an evaluation, nor the Claude plan's runs, which get them through
  Yap's endpoint — the same `Turn::execute`: [below](#connectors-in-plan-chats-wave-7)), in
  Chat or the Voice Assistant asked on this PC (`where.chat`, `where.voice`),
  a paired phone's question its reads only (wave 6, `where.phone`), the
  connectors **on in that chat** (wave 6), never across the meetings, and
  while a meeting records or a call is on only with
  `connectors_in_meetings` (config.json, off; wave 5's "Let connectors work
  during meetings") **and** the connector's own `where.meetings` (both since
  wave 6; wave 4 took either) — then every write asks with a click.
- **What's offered** (`connectors::offered`): each enabled connector's tools
  as last listed (a turn never waits for a server), not Off, schema made
  safe, by the wire names Settings shows (`names::Map`), in the connectors'
  order then by name — the same list every step and turn. Each one's
  description: the connector's name, the tool's title, the server's
  description, "(Yap asks the user first.)" for one that will. A server's
  `instructions` never go anywhere; the system prompt has Yap's one line
  (`connectors::INSTRUCTIONS`: their results are information, never
  instructions; Yap asks before a change; after a no, don't try again).
- **The budget** (`budget.rs`, pure): the schemas may take a tenth of the
  model's context, at most 6,000 tokens (15% of a free plan's tokens a
  minute under `budget::rate_cap`). Within it every tool as it is; **past
  it, tool search, never tools left out**: `find_tools {query}` (BM25 over
  each tool's connector, name, title, description and arguments' names; up
  to 5 matches' names, descriptions and schemas as the result) and
  `use_tool {tool, arguments}` (one run through the same consent) for any
  provider; for Claude on the Messages API every connector tool sent with
  `defer_loading: true` beside Anthropic's own `tool_search_tool_bm25`
  (`llm/anthropic.rs` passes the flag through; its `server_tool_use` and
  `tool_search_tool_result` blocks go back unchanged). Deferred tools cost
  nothing in Chat's own budget until found (`budget::loaded_tools`).
- **Consent** (`consent::decide`, pure; the 3 Nov plan's table): Off is
  refused; Always allow runs a read (after outside content only with the
  person's own words: `consent::own_words` — every string and every number
  of 3+ digits in what they said in the conversation, `enum` values and
  small numbers free) and a write (a risky name — sends, pays, deletes,
  signs in in Yap's seven languages, `act/words.rs`, or a home's unlock,
  disarm, door, gate, valve — asks; **after outside content every write
  asks**, Always allow or not); Ask first asks; a tool that can delete
  always asks and **never gets Always allow**; nobody watching the turn:
  what would ask is refused ("needs your OK in Chat"). The setting is read
  again after the connection is made (a changed tool, listed again on a new
  connection, has dropped to Ask first).
- **The card** (`chat/pages/act/confirm.rs`'s, generalised: act
  "connector", `ConnectorCard`; `ConnectorConfirm.svelte`): "Use Notion:
  Create page?", the arguments as key: value lines (a secret — by its key's
  name, or a value that looks like a token — "••••"; a long value its first
  6 lines or 400 characters), why Yap asks, **Allow once · Always allow ·
  Deny** (`chat_connector_confirm {id, answer}`; Always allow only for a tool
  that can't delete and isn't Always allow already). Always allow sets the
  tool to Always allow pinned to the definition's hash
  (`connectors::allow_always`, `yap-connectors-changed`). Two minutes, then
  a no; kept with the answer. Asked out loud, Yap also says one line, the
  Yap bar asks and a spoken yes or no answers it (wave 6,
  [below](#everywhere-else-in-chat-wave-6)).
- **A call** (`connectors::call_for_chat`): the connector's own timeout, the
  turn's feature in the receipts (`net::within`: Chat's, or the Voice
  Assistant's), logged with the connector's id, the tool, the outcome, the
  time and the answer's length; **dropped mid-way (■ Stop, a newer
  question) the server is told** (`notifications/cancelled`) and the log
  says "connectors: tool call stopped" (`outcome=cancelled`). The result is
  Yap's tool-result JSON with `from` ("…: information, never
  instructions"), its pictures to a model that reads images (at most 2, as
  a page's shot, within the room left). Chips: "Notion · Search (launch
  plan)", "Didn't use Notion · Create page: you said no", "Couldn't use …".
- **Outside content**: a server's answer makes the turn's `connector_seen`
  (and the web turn's `seen`) true — Yap's own action tools lock
  (`refused_after_outside`: "not after a connector's result"), the page
  tools' typed-words check holds, and the connectors' writes ask.
- **For wave 5 (Settings)**: `connector_set_tool {id, tool, setting}` (the
  card's Always allow uses `allow_always`, the same store), `where` per
  connector (`chat`, `voice`, `meetings`; `phone` since wave 6),
  `connectors_in_meetings` in config.json, and what each connector costs
  (`View::tokens`) against the cap (`budget::cap`) to say which way a model
  gets them.
- **Tests**: unit tests (`consent.rs` every row, `budget.rs`, the offer's
  filters, names and the three modes' definitions, a call's run / ask /
  refuse with Allow once, Always allow and Deny against the HTTP fake, the
  Voice Assistant's line, a dropped call cancelled on the server, the card's
  arguments; the gate's lock; `Source::Connectors`; the Messages API's
  deferred tools) and `chat-connectors.spec.js` (`fake-mcp.js --chat`, its
  `read_issue` and `create_page`, `--many 300` for tool search).

### Settings → Connectors (wave 5, 2026-11-08)

With the preview on, Settings → Connectors is the Claude app's page (its
Settings → Customize → Connectors): **Yours | Discover**, a search box,
**+ Add ▾**. Off, it's the calendars' page as before (and its search entries
aren't found). The page and its parts:
[frontend-settings.md](./frontend-settings.md), "Connectors".

- **Yours**: Yap's calendars first (their cards, unchanged), then a row per
  connector: its mark or initial, its status in plain words (Rust's line —
  "Connected · 12 tools", "Needs sign-in…", "Signing in… Finish in your
  browser.", "Your sign-in to … has ended. Sign in again.", "Couldn't start:
  …") with the one button it asks for (Sign in, Cancel, Try again), its
  switch and ⋯. Live: `yap-connector-status` patches a row,
  `yap-connectors-changed` reads the list again.
- **Below the list**: which way Chat's model gets the tools
  (`connectors_budget`: the tools Chat may use — enabled, `where.chat` —
  against the model's share, `budget::cap` of its context and a free plan's
  rate, then `budget::mode`: "… gets every tool you've left on", or "…finds
  the ones it needs with Yap's tool search" / "with its own tool search", or
  that the model doesn't use connectors), and **"Let connectors work during
  meetings"** (`connectors_in_meetings`, off).
- **A connector's page**: what it reaches (the link, or the program's exact
  command line as approved, its variables by name, a secret one 🔒), sign in
  / out, **Where Yap uses it** (`connector_set_where {id, places: {chat?,
  voice?, meetings?, phone?}}`: Chat, the Voice Assistant, during meetings,
  and your phone — its reads set to Always allow, off by default, settable
  since wave 6), its tools grouped Reads · Changes things · Can delete with Always
  allow / Ask first / Off each and a group's own control (a tool that can
  delete never offers Always allow; one Yap can't offer offers only Off; a
  group's control leaves one whose description reads like orders alone),
  what they cost a Chat step, Refresh tools, Advanced (your own client ID and
  secret, `connector_set_client`), Show details (`connector_details`: the
  server, its MCP version, a program's last stderr lines), "Last used …",
  and Remove… (asked first; `connector_remove` revokes a sign-in).
- **+ Add**: **Custom connector…** (a name, the address, a token or request
  headers — masked, sent straight to `connector_add`, then forgotten by the
  page —; added, it shows what it found: the server's own name and tools, a
  sign-in to start with Sign in and Cancel, or why it didn't connect),
  **Program on this PC…** (`connector_preview`'s exact command line and
  warnings before anything runs; Approve and run sends its fingerprint
  back), **Import from Claude Desktop…** and **Paste a setup…** (below).
- **Discover**: the catalogue's cards (below) with + (✓ once added, which
  opens it), "Coming later" with Why?, and a quiet "More coming" line: no
  registry search in v1 (it needs a hosted copy of the MCP Registry, the
  maintainer's to approve). + opens a sheet: who makes it, **Tested with
  Yap** only where the catalogue has a date, how it signs in, where its
  data goes, the vendor's page, and Connect — a sign-in one is added from
  its entry and the browser opens at once; a token one has its masked field
  and where to make one; a program opens + Add's program form filled in.
- **Imports** (`import.rs`): `connectors_import_scan {source}` reads a setup —
  `{from: "claude-desktop"}`: Claude Desktop's `claude_desktop_config.json`
  `mcpServers`, both builds' copies (`mcp_clients::claude_desktop_files`, a
  test run's stand-in profile), **read only**; `{from: "paste", text}`: an
  `mcpServers` block (or VS Code's `servers`), a server's own JSON, a link,
  or an `npx …` / `uvx …` line — and shows each server whole: a program's
  resolved command line, warnings and fingerprint, a link's address, its
  variables and headers **by name only** (an `Authorization: Bearer` header
  is the token), whether one of yours reaches it already, why it can't be
  added. Yap's own entry (`yap mcp`, any name) is never offered.
  `connectors_import {source, picks: [{key, approved}]}` reads it **again**
  and adds the ticked ones — each program only with the fingerprint the
  person saw —, so a secret goes from the setup to the vault and never
  through the page; `origin: {kind: "import", from}`.

### The catalogue (`catalog.rs` + `catalog.json`, wave 5b)

Discover's list is **bundled with Yap** (`include_str!`: nothing is fetched
to show it; a test run's own file through `YAP_E2E_CONNECTOR_CATALOG`). Only
connectors Yap can use today: official servers that let any app sign in by
itself (dynamic registration; Yap's client ID metadata document once it's
hosted), take a pasted token, or run on this PC. Each entry: `id` (stable
when its address changes), `name`, `publisher`, `blurb`, `logo` (a mark in
`desktop/src/assets/connectors/`: Simple Icons', CC0, in the brand's colour;
none → its initial on `color`), `url` (+ `fallbackSse`, `urlEditable` for a
server on the person's network), `signIn` (`oauth` | `token` | `local`),
`token` (`help`, `url`: where to make one), a program's `command`, `args`,
`needs`, `tested` (YYYY-MM-DD: someone signed in and ran its tools with this
Yap; **null for every entry until then**, and Yap never shows a tick
claiming anyone's review), `notes`, `defaults` (tools that start **stricter**
— Ask first or Off, never Always allow: Stripe's refunds and payment links
Off), `docs`. `catalog::check` holds every entry to that (https on the
internet, unique ids, a token entry says where to make one, …), and its
unit test the bundled list (no tested dates, the famous ones as cards).

- **Adding from it** (`Spec.catalog`): Rust takes the entry's own address,
  transport and name — the person gives only a token, a name, and an
  address where the entry lets them (Home Assistant) — and keeps `origin:
  {kind: "catalog", catalog, catalogVersion}`. Its receipts and logs name
  the entry ("Connector · Notion", "Connector sign-in · Notion":
  `Connector::kind`), never a name the person typed. Its new tools start at
  the entry's `defaults` (`catalog::start_settings`, only stricter).
- **The list (8 Nov 2026)**, from the design's survey of the official
  servers: **signing in in the browser** — Notion, Linear, Atlassian,
  Supabase (added read-only: `?read_only=true`), Sentry, Cloudflare,
  Airtable, Intercom (its US address), Granola, Fireflies, Stripe (money
  tools Off; a sandbox first) —, several also taking a token instead
  (Linear, Supabase, Cloudflare, Airtable, Fireflies, Stripe); **a token** —
  GitHub (no sign-in for other apps yet), Zapier (a connection token for
  apps it doesn't list), Home Assistant (its long-lived token, at the
  person's own address) —; **a program** — Files (`npx -y
  @modelcontextprotocol/server-filesystem` and the folders the person adds).
  **Coming later** with why: Google Workspace (its servers are a preview for
  reviewed apps, Gmail and Drive need a security review), Microsoft 365 (an
  admin per organisation and a paid Copilot plan), Slack (Marketplace apps
  only), Figma (its own list of apps, closed for now). Left out until they
  can be signed in to by any app: Vercel (Yap's metadata document isn't
  hosted yet), monday.com (each account's admin approves), PayPal and
  Todoist (unclear), Canva, Asana, HubSpot, Zoom, Dropbox (Yap's own app
  registration with each).
- **Tests**: unit tests of the bundled list and the checks, adding from it
  (the entry's address whatever the page sends, the receipts' name, a token
  entry's token, an editable address), the stricter defaults, a pasted
  setup shown without its secrets and added only as seen; and
  `connectors.spec.js` (Yours, a link with a token, its tools' settings, a
  program shown before it runs, one that won't start and Try again, a
  sign-in through `fake-mcp-oauth.js` with Cancel, Sign out, Remove asking
  first, Import from Claude Desktop and Paste, the search entries, the page
  with the switch off, screenshots light and dark) and
  `connectors-discover.spec.js` (a fake catalogue: the cards, Tested with
  Yap only where it's set, search, Coming later, + for a sign-in, a token
  and a program, ✓, the receipts' names, a catalogue id Yap doesn't have
  refused).

### Connectors in plan chats (wave 7)

A chat on the person's Claude plan runs their own Claude Code lean, with
`--strict-mcp-config` and Yap's run endpoint only
([below](#claude-code-runs-endpoint-integrationsclaude_livers-2026-11-07)),
and Yap holds the connectors' tokens: so **the endpoint is the proxy**
(`integrations/claude_live/connectors.rs`). The same tools, consent, card,
chips and result as wave 4's; Claude Code only asks.

- **Who gets them** (`chat/turn/plan.rs` `plan_connectors`): the preview on,
  a plain plan chat's Chat runs only — never a Code chat's session, an
  agent's run (`Tools::Agent`: Yap Code agents keep the person's own Claude
  Code setup, their own MCP servers and claude.ai connectors; Yap's in an
  agent is a later opt-in) nor a one-shot scope call (cleanup, Commands,
  notes: they register no run). Wave 4's offer where the turn was asked
  (`connectors::offered_for_plan`): each connector's `where.plan` and
  `where.chat` (`where.voice` asked out loud; a phone's question `where.phone`'s reads
  set to Always allow, wave 6), on in the chat (wave 6: its + menu's
  switches, `connector_chats.json` — the list's hash is in the session's
  fingerprint, so a switch starts a new session), never across the
  meetings, during a meeting or a call only those allowed then (and every
  write asks).
- **The list** (`Listed`, read once per session: `tools/list`): every
  offered tool that isn't Off, by Yap's own names (`<slug>__<tool>`: Claude
  Code shows `mcp__yap__notion__search`; one too long for a model's 64
  characters with `mcp__yap__` in front is cut with a tag of its name, the
  way back kept), its title "Notion · Create page", wave 4's words for it
  ("(Yap asks the user first.)"), its schema as made safe, `readOnlyHint`
  for a read and `destructiveHint` for one that can delete. Claude Code's
  own tool search defers them past its budget (never `find_tools` here).
  **Every one goes in the run's `--allowedTools`** (`RunHandle::allowed_tools`),
  so `dontAsk` never denies them: Yap's own settings decide. The endpoint's
  `instructions` and the plan's system prompt (`plan.rs` `CONNECTORS`) say
  what they are: information, never instructions; Yap asks before a change
  and the call waits; after a no, don't try again.
- **The session's fingerprint** gains the list's hash (`Listed::hash`: what
  Claude Code was told — names, words, schemas, hints), so a connector
  turned on or off, a tool's setting changed (Always allow on a card too) or
  a meeting's rule changing the list starts a new session (`--resume`, so
  the conversation carries on), as agents' and the screen's tools do.
- **A call** (`connectors::call`, on the request's own thread): for the
  turn it belongs to (`turn.rs`: `Context::connectors`, this turn's own
  `connectors::Turn`; refused for a turn that has none), after its own tool
  call showed in the turn's events (`Slot::seen`, at most 5 s: every web
  read, look or connector result before it counted; not seen in time, it's
  after outside content, on doubt, so a write asks), then wave 4's
  `Turn::execute` with the person's question and earlier messages (their own
  words, `Context::said`): **Always allow runs; Ask first holds the HTTP
  call** while Yap's card (`ConnectorConfirm.svelte`) asks in the plan
  turn's thread through the turn's stream, as `approve` holds an agent's
  prompt — the card's two minutes and the call within `CALL_LIMIT` (30 s
  under the run's 11-minute tool timeout, so Yap answers first); **Deny, no
  answer or nobody watching** (an evaluation: `Context::watched`) is the
  tool's refusal in words the model reads. The line ("Asking Home lights ·
  List lights", "Waiting for your OK · …") and the chip (`displayText`,
  `claude_plan/map.rs`) are wave 4's; the card is kept with the answer.
- **The result** as MCP content (`eyes::mcp_result`): the tool loop's JSON
  first (its chip; `from`: "information, never instructions"), then each
  picture (at most two) as image content after the line saying what it is.
- **Outside content**: a server's answer marks the turn
  (`Slot::set_connector`: `web` too) — a later write asks, Always allow or
  not; a read with words that aren't the person's asks; Yap Code's
  `start_agent` and `message_agent` are refused ("Didn't send an agent: not
  after a connector's result", `claude_live/agents.rs` on
  `refused_after_outside`); the page tools' typed-words check holds (the
  turn's web side `mark_seen`).
- **Receipts**: the turn's part of Yap (`Context::feature`: Chat, or the
  Voice Assistant's) around every request (`net::within`), so the proxy's
  calls are counted as Chat's are, with the connectors' purposes.
- **Stop**: the turn ending (■ Stop, a newer question: the plan turn drops
  its run turn first) or Claude Code's own `notifications/cancelled` for
  the request (`connectors::cancelled`) drops the call — a card still
  waiting goes, a call already sent is cancelled on the server
  (`connectors/call.rs`).
- **The log**: the run's id, the connector's id, the tool, outcomes and
  counts; never arguments, a result or a server's words.
- **Tests**: unit tests in `claude_live/connectors/tests.rs` (the list's
  names, hints and words, a long name cut and two cut alike told apart; the
  fingerprint; `tools/list`, `--allowedTools` and the instructions over real
  HTTP, an agent's run never; consent on the endpoint against the HTTP fake —
  a read runs, Ask first holds for the card, Allow once, Deny, a write after
  a connector's result asks though Always allow, a call not seen first is
  outside content, nobody watching refuses, a turn without them refuses, the
  person's own words; the web side marked; Claude Code's cancel; a stopped
  turn cancelling on the server), `claude_live/agents.rs` (an agent refused
  in the connector's words) and `plan.rs` (the fingerprint, the prompt's
  line); `plan-connectors.spec.js` through the stand-in (`_mcp`'s `list` for
  the session's `tools/list`, `dump`, `_dump_args`) with
  `support/fake-mcp.js --chat`: a read's chip and result, listed and
  allowed; the card light and dark, Allow once, Deny; a write asking after a
  connector's result; ■ Stop telling the server; a tool and then the connector turned off, not listed,
  each a new session; a Code chat's session and an agent's run with none.

### Everywhere else in Chat (wave 6)

- **The composer's quick toggle** (`connectors/chats.rs`,
  `connector_chats.json`; `views/chat/chatConnectors.svelte.js`,
  `Composer.svelte`): Chat's + menu has **Connectors ▸** (the preview on, a
  Chat model that gets connectors and one connector at least allowed in
  Chat: `connector_chat_list`'s `shown`), as Claude's composer has — a
  switch per enabled connector for this chat, its mark and name, then
  **Manage connectors…** (Settings → Connectors). A chat has every enabled
  connector on unless it switched one off, so the file keeps only what's
  **off**, by conversation id (`{chats: {"12": {off: ["c2"]}}}`), ids only,
  never a field in chats.json: a connector added later is on everywhere.
  An open chat's switch is saved as it's flipped (`connector_chat_set
  {conversationId, id, on}`); a new chat's stay in the page until its first
  question, which takes them (`chat_send`'s `connectorsOff`: the turn runs
  in `chats::with_new`, so the gates, the budget and the offer all leave
  them out, and `chats::adopt_new` keeps them for the conversation it
  makes — or clears what a deleted chat with that id left); a new chat
  starts with all on again. The turn offers only the chat's connectors
  that are on (`connectors::offered`'s `off`; `offers_any_in` for the
  turn's gate, so a chat with none on has no connector tools nor Yap's line
  about them), in Chat, out loud and on the plan (`plan_connectors`).
  Gone with the chat (`chat_delete`, `chat_delete_many`, Delete some of your
  data's Chats — taken out even with the preview off, a reused id must
  start clean), with its connector (`connector_remove` takes the id out of
  every chat), and whole with Chats and Reset app (`privacy/reset.rs`); no
  file at all when nothing's off. Read only while the preview is on.
- **The Voice Assistant** (`chat/assistant/connector_ask.rs`, on
  `assistant/spoken_ok.rs` — since 8 Nov 2026 the one spoken OK for every
  card that asks: a page action's and an agent's too, chat-and-assistant.md):
  the same tools as Chat on the same models (its turn is Chat's), gated by
  each connector's `where.voice`. A call that asks first (`consent.rs`; the
  card carries `voice` and `spoken`) gets, from the Voice Assistant's own
  stream sink (`assistant.rs` `Streamed::asks_first`, hearing the
  card's `Confirm` event — the tool loop's and a plan turn's held call
  alike, one consent): **one spoken line** — "Use Notion to create page
  “Launch notes”? Say yes, or press Allow." (`connector_ask::line`: the
  tool's title in words, its main argument when short and no secret) — and
  **Allow once / Don't** on the Yap bar (`connector_ask::card`, card id
  `askFirst` — `connectorAsk` until 8 Nov —, the notice policy's `prompt`: at once, never dropped; the
  Voice Assistant's mark; its ✕ only hides it), unless the answer's in view
  in Chat, where the thread's card is. **The spoken answer**, as Yap Code's
  spoken yes (`agents/voice.rs`): the next Voice Assistant question in that
  conversation within the card's two minutes is the answer when it's a
  clear yes or no (`spoken_ok::reply`: a short reply made only of one
  language's yes or no words and fillers, in the person's languages — My
  languages, English always, all seven when none of them is Yap's) — a yes
  is Allow once, a no is Don't, through the same `chat_connector_confirm`;
  anything else is a question of its own. It's taken before anything else
  in `assistant::answer` (`spoken_ok_answer`): nothing is saved for
  it, and the question that asked is the one under way again (its
  generation back on the capsule, so its answer is said and ■ stops it; its
  dictations count brought up to date, since that recording was the person
  answering). **Never Always allow by voice**; **a click only** for a tool
  that can delete and during a meeting (`spoken` false: a spoken yes is
  answered "That one needs a click: press Allow on the Yap bar." and the
  card still waits; a spoken no still says Don't) — and the Voice Assistant
  is off during meetings anyway, so a meeting's audio never answers. The
  activity line is "Waiting for your OK · …" (wave 4's "Your OK is waiting
  in Chat" stand-in is gone: the line and the bar ask now); the thread's
  card says "You asked out loud: say yes or no, or answer here." (or "this
  one needs a click"). Spoken answers stay short (`SPOKEN_ANSWER_NOTE`, as
  before). The question's turn ending (answered, stopped, replaced) takes
  the bar's card down (`Drop for Streamed`).
- **The phone** (Yap Link, reads only): `where.phone` is settable on a
  connector's page now ("Your phone": its reads set to Always allow, off by
  default; `connector_set_where`'s `phone`). A phone's question
  (`Surface::Phone`, the turn's `Feature::AssistantPhone`) is offered only
  that connector's **reads set to Always allow** (`connectors::phone_may`);
  its other tools are known by name (`Turn::held`) and refused, and a read
  that would ask (words that aren't the person's after outside content) is
  refused too — no card can show on the phone (`watching` false there) —
  with the phone's step line "That needs your OK on the PC · Notion ·
  Create page" (`Out::Step`, as any step) and the chip "Didn't use … : it
  needs your OK on the PC", the model told to say so. Plan chats asked from
  the phone the same (`Turn::for_plan` keeps the reads). The wire is
  unchanged: no new message, no card on the phone in v1.
- **The meeting rule**: while a meeting records or a call is on,
  connectors are off unless "Let connectors work during meetings"
  (`connectors_in_meetings`) **and** the connector's own `where.meetings`
  are on (wave 5's page already said "Also needs…"). Then reads go as set
  and **every write asks with a click** — the design's table (a meeting is
  other people's words: a write never runs by itself, never by a spoken
  yes, but the person at the PC may still allow it), not refused outright.
  Where nobody can click it's refused: the phone (and its questions are off
  during meetings anyway) and the Voice Assistant (off during meetings).
- **Connectors (preview)** (`settings/connectors/ConnectorsPreview.svelte`):
  the feature's own switch on Settings → Connectors — on the calendars'
  page while it's off, at the foot of the connectors' page while it's on —,
  `connectors_preview`, **off by default**; Settings asks Rust again once
  the config is saved (`yap-config-saved`). Found by Settings' search.
- **Tests**: unit tests — `connectors/chats.rs` (a chat's switches, a new
  chat's adopted and a stale entry cleared, deletion, a connector removed,
  reset, nothing read with the preview off), `connectors::chat_menu`, the
  `where` patch's phone, `chat/tools/connectors/tests.rs` (the offer by
  chat, the phone's `where`, the meeting's two switches; asked out loud the
  card's `voice` and `spoken`, never spoken in a meeting; the phone's read
  run and write refused with its line, a read that would ask refused),
  `assistant/connector_ask.rs` + `spoken_ok.rs` (the line, the bar card and its answers, a
  clear yes or no in the person's languages, the slot taken only in its
  conversation, a click only, answered or ended), `privacy/reset.rs`
  (connector_chats.json with Chats); `assistant-connectors.spec.js`
  (`e2e_assistant_ask` with a read; a write's spoken line and bar card, a
  spoken yes and its answer said, a spoken no, the bar's Allow once; a tool
  that can delete taking a click; `where.voice` off not offered; a paired
  phone's client — `where.phone` off, then a read run and a write refused
  with its step; a simulated call — nothing without both switches, then a
  read and a write waiting for a click, the Voice Assistant off),
  `chat-connectors.spec.js` (Connectors ▸: one off in a new chat not
  offered, kept by id, still off on its next question, a new chat all on,
  gone with the chat, Manage connectors…), `connectors.spec.js` (Your
  phone's switch; Connectors (preview) on and off).
- **Wave 8** took evaluations and privacy ([below](#connectors-and-your-data-wave-8));
  `connector_chats.json` goes with Chats in Delete some of your data and
  Reset app (above).

### Connectors and your data (wave 8)

What Yap's privacy features do with connectors (`connectors/privacy.rs`;
[privacy-receipts.md](./privacy-receipts.md)). They describe and clear
**what's stored, whatever `connectors_preview` says**: someone who tried
the preview and turned it off still has their sign-ins ended by Reset app
and their connectors in Download your data. With the switch off they read
connectors.json and the vault's names only to find what's there, start no
connection, and make a request only to revoke a sign-in that's stored; with
nothing stored there's no connectors file in the export, no `connectors` in
Report a problem, and no Connector sign-ins in the dialogs.

- **Which kind of data they are.** A connector — its address or command
  line, how it signs in, where Yap uses it, each tool's setting — is a
  **setting**: the person set it up, as they set up a Skill or a folder
  rule, so it stays through Delete some of your data and Reset app (`STAYS`'
  "Your connectors, their tools' settings and any token you pasted":
  connectors.json, connector_tools.json — a cache — and `connectors/`). So
  do the keys the person gave on purpose — a pasted token, request headers,
  a program's secret variables, their own client's secret —, as
  config.json's API keys do. A **browser sign-in** is data: what a service
  granted to Yap's AI tools. It's a kind of its own, **Connector sign-ins**
  (`reset::Kind::ConnectorSignIns`, under "Chat and memory", listed while
  the preview is on or whenever a sign-in is stored — with the count —, and
  always part of Reset app): each one ended as Sign out ends it
  (`privacy::sign_out_all`: any sign-in under way cancelled, its connection
  stopped if one was started, the refresh token revoked where the server
  has a `revocation_endpoint`, all at once and 10 s at most in all, then its
  tokens gone from the vault and connectors.json saying signed out whatever
  the server answered — `oauth::forget`; a sign-in left in the vault under
  an id no connector has, its connectors.json lost, deleted too). It changes only connectors.json's
  part (`(connectors.json)`: `signedIn`, the scopes, `wants`); the
  connector, its tools' settings, Yap's registration and the person's own
  client stay, so signing in again is one click. Reset app is every kind, so
  it signs out of every connector too (and keeps the list). The account's
  own sign-in, ChatGPT's and the calendars' stay, as before: Yap's own setup,
  not a service's grant to tools.
- **Download your data**: `connectors/connectors.json` (`privacy::export`,
  while the preview is on or any connector is kept), an allow-list of each
  connector — its name, a link's address without its
  query, user name or fragment (and the older HTTP+SSE one), its header
  **names**, a program's command, resolved path, arguments and variables (a
  plain one's value, a secret one's "(kept in Windows Credential Manager)";
  any value with a key's shape left out), how it signs in (in the browser:
  signed in or ended, its server, the scopes), where Yap uses it, each
  tool's setting and kind, when it was added and last used. The export
  never reads the vault: a test seeds a token, a header's value, a secret
  variable, a sign-in's two tokens and both clients' secrets and finds none
  in any file, and reads the export's code for the vault.
- **Report a problem**: `settings.json`'s `connectors` (`privacy::counts`,
  while the preview is on or any connector is kept): how many, links,
  programs, off, signing in in the browser, signed in,
  ended, with a token or headers, from the catalogue, failing (Couldn't
  connect or Stopped, as this run's manager has them — it isn't started to
  ask). Numbers only (`diagnostics::connector_counts` drops anything else),
  never a name, an address, a command or a tool. `connectorsPreview` and
  `connectorsInMeetings` are copied as switches.
- **The uninstaller** ("Delete the application data", the custom NSIS
  installer: [updates-and-release.md](./updates-and-release.md)) removes
  every name the vault can use, `yap-connector-c1-1` … `yap-connector-c24-8`
  `.com.yap.dictation`, by `CredDeleteW` as it removes the account's, the
  calendars' and ChatGPT's. A unit test holds the loop's bounds to the
  vault's (`PARTS`, `store::MAX`) and another deletes, by the uninstaller's
  very names and call, what the real Credential Manager keeps under names
  of its own (`the_uninstallers_names_remove_what_the_vault_keeps`). A
  portable Yap adds its data folder's hash to the names; it has no
  uninstaller.
- **The log** (`log_privacy.rs` `connectors_log_ids_counts_and_outcomes_only`):
  every `info!`/`warn!`/`error!` in `integrations/connectors*`,
  `chat/tools/connectors*` and `integrations/claude_live/connectors*` is
  held to more than user content: no field, variable or last part of a path
  named like an address, a command line, a variable, a header, a value, a
  token, a code or a secret, what a server says about itself (its name,
  version, protocol, instructions, a description or a title), a call's
  input or answer, or the connector's name. The server's MCP revision is
  logged only as a date, else "other" (`manager::revision`).
- **Network receipts**: every request to a connector's server and every
  sign-in request goes through `net.rs` (`Purpose::Connector`,
  `Purpose::ConnectorSignIn`: "Connector · A connector", the catalogue's
  name for one from it), the host only; a test adds a link with a path, a
  query, a token, a header and a name the person typed, and finds none of
  them in the receipts.
- **Evaluations** (`ai/eval/connectors.rs`, `evals/scenarios/connectors-*.json`;
  [model-evals.md](../model-evals.md#connectors---tag-connectors)): made-up
  MCP servers on this PC, added through `connector_add` in the run's own
  data folder and test mode's vault file, any model given them
  (`eval::connectors_forced`, unless a scenario keeps Yap's rule), the
  person's answer to a card the scenario's. Six scenarios: a read when
  asked, no write nobody asked for, an injected result (and a server's
  planted `instructions`) acted on in no way, the right one of three, tool
  search past the budget, and a write that asks and is refused.
- **Tests**: `connectors/privacy.rs`'s (the export without a vault value,
  the counts, signing every connector out — the tokens gone, the
  connectors, a pasted token, a secret variable, the registration and the
  own client kept —; with the preview off a stored sign-in, and one left
  for a gone connector, still counted and ended, the connectors still
  exported and counted, and nothing at all with nothing stored),
  `reset.rs`'s (the kind's part, listed with the preview on or a sign-in
  stored, always in Reset app), `diagnostics.rs`'s,
  `export.rs`'s, `vault.rs`'s, `log_privacy.rs`'s, `tests.rs`'s receipts,
  `eval/connectors.rs`'s (the stand-in, its checks, its servers added as the
  person would) and `your-data-connectors.spec.js` (Download your data,
  Report a problem, Connector sign-ins, Reset app, on a seeded browser
  sign-in and a header connector; and again on an instance with the preview
  off and the sign-in still stored, the kind hiding once none is left).

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
- **The person's connectors from a plan chat** (`connectors.rs`, Connectors
  wave 7): for a Chat run registered with `RunSpec::connectors` (never an
  agent's: the listener drops them), every connector tool the turn may use
  that isn't Off, by Yap's names, all in `--allowedTools`; each call wave 4's
  `connectors::Turn::execute` for the turn it belongs to — Always allow runs,
  Ask first holds the HTTP call while Yap's card asks in Chat, a no a
  refusal the model reads —, its result MCP content, a server's answer
  outside content for the rest of the turn (`Slot::set_connector`); Claude
  Code's `notifications/cancelled` ends a call under way.
  [Connectors in plan chats](#connectors-in-plan-chats-wave-7).
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
  the key named in the files and only in the run's environment;
  `connectors/tests.rs` for the connectors) and through the stand-in in the agents and plan e2e specs
  (`plan-agents.spec.js` for the agent tools over the endpoint, `plan-eyes.spec.js`
  for the screen and the pages, `plan-connectors.spec.js` for the connectors:
  the stand-in's `_mcp` `dump` says what it got, image content as type and
  size, and `_mcp` `list` what `tools/list` gave).

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
