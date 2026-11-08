# Yap Lab (`npm run lab`)

A developer tool for the Claude Code session that maintains Yap: it starts an
**isolated test Yap**, puts questions to **that Yap's own Chat** (its AI, with all of
Yap's tools), and reads back **exactly what each turn did** — the tools it called with
their arguments and results, the cards it asked, the pages it brought up, the answer.
So Yap's own model can test Yap's tools from the inside on the real web, and the
maintaining session can fix what it finds.

Why: when Chat on a Claude plan tested every browser tool and wrote a report, it found
problems the e2e suite's fakes never would (spoken turns refusing page actions, an
address the model knows refused, paging missing). The lab makes that loop routine.

It's dev tooling only: the shipped app's behaviour doesn't change. A release build has
none of it (`shell/lab.rs` is debug-only, like test mode).

## The loop

```bash
npm run -s lab -- build                      # the test Yap, from this checkout (~1 min incremental)
npm run -s lab -- start                      # Chat on the person's Claude plan, the real web on
npm run -s lab -- ask "Open https://en.wikipedia.org/wiki/Lighthouse beside the chat and read me its first section"
npm run -s lab -- ask "Now scroll to the history section" --chat 3
npm run -s lab -- ask "What's on the page beside the chat?" --spoken --chat 3
npm run -s lab -- shot --window page         # what the page looks like
npm run -s lab -- report browser-tools       # the whole family tested, a Markdown report
npm run -s lab -- stop
```

Read the JSON, find what's wrong (a refusal that shouldn't be, a parameter ignored, a
step that never shows), fix it, `build`, `start` again, ask again. `-s` keeps npm's
own banner off stdout, so stdout is exactly one line of JSON (or run
`node scripts/lab/lab.mjs <command>`). Every command exits 0 when `ok` is true, 1
otherwise. A real question can take minutes and a report much longer: keep
`--timeout` under your shell's own limit (a Claude Code Bash call stops at 10
minutes) and `wait` for the rest — the turn goes on in the test Yap whether or not a
command is waiting on it.

**Cost.** A real run's Chat is the person's own Claude Code on their own Claude plan:
each question uses their plan's usage (a `report` uses a lot). **Ask the maintainer
before any real `ask` or `report`.** `start`, `check`, `state`, `shot`, `logs` and
`stop` ask the model nothing. `--fake` runs cost nothing at all. An API model
(`--model anthropic:…`) costs money and only works with a key the person set in
`YAP_LAB_ANTHROPIC_KEY` themselves.

**The PC.** You can use the PC while the lab runs: the test Yap's windows sit behind
yours and never take the focus (test mode), nothing is typed into or clicked in your
apps, and Playwright's clicks go to the test Yap's own webviews over CDP, never the
mouse. It may feel a little slower.

## Commands

All of them print one line of JSON: `{"ok": true, …}` or `{"ok": false, "error": "…"}`.

### `build [--target <dir>]`

The e2e suite's build (stub debug build: Chat needs no speech-to-text; the frontend
embedded; test mode compiled in) into the lab's own Cargo target dir:
`YAP_LAB_TARGET_DIR`, else `E:\Builds\yap-lab` on a PC with an `E:\Builds` folder (the
maintainer's keeps every build there), else `desktop/src-tauri/target/lab`. A kept
cache: leave it there. Writes `lab-build.json` beside `debug/` (the commit it was built
from). The build's output goes to `test-results/lab/build.log`.

```json
{"ok":true,"exe":"E:\\Builds\\yap-lab\\debug\\yap.exe","commit":"29e5e3917919a1a430e3ac9e69395ac8a69ae038","dirty":true,"log":"…\\test-results\\lab\\build.log","ms":58891}
```

### `start [options]`

Starts the test Yap and a small **control process** that keeps it running between
commands (holding the CDP connection; it answers the CLI on `127.0.0.1` with a random
token in `control.json`). Returns once the test Yap is ready.

| Option | |
|---|---|
| `--model <m>` | `claude-plan` (the default: the plan's Claude Sonnet 5.5), `claude-plan:<model id>`, or `haiku` / `sonnet` / `opus` / `fable` — all on the person's Claude plan. `anthropic:<model id>`: the Anthropic API on `YAP_LAB_ANTHROPIC_KEY` (money; refused without it). |
| `--effort <e>` | `low`, `medium`, `high`, `xhigh`, `max`: Chat's effort for that model. |
| `--web on\|off` | Web search and the pages beside the chat (default on). |
| `--yap-code on\|off` | Yap Code: agents, Code chats (default off). With it, the scratch workspace `lab-sandbox` is made (a tiny made-up project) and added, unless `--workspace <dir>` names another; `--keep-workspace` keeps the scratch one's changes from last time. |
| `--connectors on\|off` | The connectors feature (default off). The lab never connects or signs in to one. |
| `--build <exe>` | Another build than the lab's own. |
| `--stale-ok` | Start even though the build is older than the checkout's last commit (refused otherwise). |
| `--idle <min>` | Quit after this many minutes without a command (default 120). |
| `--fake` | Offline: the e2e suite's fake AI (`fake-gpt-5`, scripted with `fake`) and stand-in web; with `--model claude-plan`, Yap's stand-in for Claude Code (`yap.exe fake-claude`) on `desktop/e2e/support/fake-claude/plan-chat.jsonl` (`--fake-claude-script <file>` for another). `--fake --live-web`: the fake AI on the real web (the lab's own checks). |

For the plan (and for `--yap-code on`), `start` checks Claude Code the way Yap does
(`claude_plan_status`: found, its version, `claude auth status`): not installed, too
old or **not signed in, it says so and stops** — the lab never signs in.

```json
{"ok":true,"pid":30064,"model":{"kind":"plan","provider":"claude-plan","model":"claude-sonnet-5-5"},"dataDir":"…\\test-results\\lab\\runs\\lab\\app\\Data","windows":["onboarding","settings","notepad","quick-note","overlay"],"yapPid":49136,"claudeCode":{"installed":true,"version":"2.1.296","tooOld":false,"signedIn":true,"plan":"max","authMethod":"claude.ai","error":null,"standIn":false},"lab":"…\\test-results\\lab"}
```

### `check`

`start` on the plan, then `stop`: is Claude Code on this PC and signed in, as Yap sees
it? Asks the model nothing.

### `ask "<text>" [--spoken] [--chat new|<id>|code:<workspace>] [--timeout 600] [--cards allow|deny]`

Puts a question to Chat and waits until the turn ends (answered, stopped or failed),
**or a card waits for the person**, or `--timeout` seconds pass.

- Typed: in Chat's own box and Send, as a person would (Playwright over CDP: the
  same `chat_send` and stream the Chat view uses). `--spoken`: through the Voice
  Assistant's path (`e2e_assistant_ask`: its shortcut's steps with the words in place
  of the microphone).
- `--chat new` (default) a new chat; `<id>` that conversation (Chat opens on it);
  `code:<workspace>` a new Code chat there (`--yap-code on`).
- `--cards allow|deny`: answer every card this turn asks at once (otherwise `ask`
  returns `status: "waiting"` and you `answer` it).

```json
{"ok":true,"turn":2,"status":"answered","conversation":2,"question":"Buy me a ticket for the 09:42 here: https://rail.example.com/results?…","spoken":false,
 "answer":"I did not buy it: you said no.","model":{"provider":"custom","id":"fake-gpt-5","onThisPc":true},
 "steps":["Brought up rail.example.com/results?… beside the chat","Read rail.example.com/results?… beside the chat","Didn't click ‘Buy tickets’: you said no"],
 "tools":[{"via":"yap","name":"browser_navigate","args":"{\"url\":\"https://rail.example.com/results?…\"}","result":"Brought up rail.example.com/results?… beside the chat","ok":true},
          {"via":"yap","name":"browser_click","args":"{\"ref\":1}","result":"Didn't click ‘Buy tickets’: you said no · error: The user said no to pressing ‘Buy tickets’, so Yap didn't. … · data: {\"allowed\":false,\"asked\":true}","ok":false,"refused":true}],
 "cards":[{"id":"confirm:1","kind":"page","act":"press","label":"Buy tickets","site":"rail.example.com","does":"buy","state":"refused","answers":["allow","deny"]}],
 "pending":[],"pages":[{"url":"https://rail.example.com/results?…","onScreen":true,"loaded":true}],"pagesShown":["https://rail.example.com/results?…"],
 "pictures":[],"documents":[],"sources":["https://rail.example.com/results?…"],"error":null,"ms":62834,"transcript":"…\\test-results\\lab\\turns\\002.json"}
```

- `status`: `answered`, `stopped`, `failed` (`error` says why), `waiting` (a card:
  `pending` and `hint` say which and how to answer) or `running` (timed out: still
  going; `wait`).
- `steps`: the answer's chips, as Chat shows them. `tools`: every tool call, in order —
  `via: "yap"` from Yap's tool loop (an API or fake model), `via: "claude"` from Claude
  Code's own stream (a plan chat: its own tools such as `WebSearch`, and Yap's MCP
  tools as `server: "yap"` under Yap's names). `ok: false` is what the tool said;
  `refused: true` is the lab's guess that it was one of Yap's rules rather than a
  failure. Pictures are counted, never inlined.
- Long fields are cut with their full length said (`… [+1234 chars, 1634 in all]`);
  the **transcript** file has everything: the result, every tool call whole, the
  conversation as Chat saved it, the pages, and the turn's part of the record.

### `wait [--timeout 600] [--agents]`

Waits on the current turn again (after `running`), and returns `ask`'s JSON.
`--agents`: waits until no Yap Code agent is queued or running (one asking counts as
stopped), and returns the agents, the cards they wait on and the reports they put in
their chats.

### `answer <card-id> allow|deny|always [--timeout 600] [--no-wait]`

Answers a card as the person would, then waits on as `wait` does. Card ids:
`confirm:<n>` (Chat's "Yap wants to…" cards: a page action, an agent starting, a
connector's tool; `always` is a connector's Always allow), `approve:<n>` (an agent's
permission prompt), `plan:<agent id>` (an agent's plan: `allow` is Go ahead, `deny`
stops it). A card unanswered for 2 minutes is a no (Yap's own rule), so answer
promptly.

```json
{"ok":true,"answered":{"id":"approve:1","how":"allow","wasWaiting":true},"agents":[{"id":"1a1bad23b2f261d619b","folder":"lab-sandbox","status":"done","conversation":1,"mode":"auto","plan":false,"text":"Done. I changed Draft to Final in notes.md and the tests pass.","files":["notes.md"],"commands":["npm test"],"denials":[],"error":null,"cost":0.041}],"pending":[],"reports":[{"conversation":1,"report":"The agent in lab-sandbox: it's done.\n…","answer":"OK"}]}
```

### `cancel`

Chat's ■ for the current turn (the Voice Assistant's stop for a spoken one).

### `shot [--window main|bar|panel|page]`

A PNG over CDP, saved in `test-results/lab/shots/`. `main`: the main window; `bar`: the
Yap bar; `panel`: the panel beside the chat; `page`: the live page beside the chat
itself (its own WebView2 — in `main` and `panel` its area is blank).

```json
{"ok":true,"shot":"…\\test-results\\lab\\shots\\2026-11-08T10-58-53-page.png","window":"page"}
```

### `state`

```json
{"ok":true,"lab":{"model":{…},"effort":null,"web":true,"yapCode":false,"connectors":false,"fake":true,"dataDir":"…","pid":51836,"upMs":80769,"workspace":null},
 "turn":{"n":2,"status":"waiting","conversation":2,"question":"…"},
 "conversations":[{"id":2,"title":"Buy me a ticket…","updated":1794135484}],
 "pages":[{"key":"page:https://rail.example.com/…","loaded":true,"onScreen":true,"tab":1,"url":"https://rail.example.com/…"}],
 "agents":[],"pending":[{"id":"confirm:1","kind":"page","label":"Buy tickets","site":"rail.example.com","does":"buy","state":"asking","answers":["allow","deny"]}],"appsOpened":[]}
```

### `logs [--since N] [--grep <regex>] [--tail N]`

The test Yap's log (debug level: its own data folder): the last 200 lines, or from line
`N` on (`next` is where to go on from).

```json
{"ok":true,"file":"…\\runs\\lab\\stdout.log","next":75,"lines":["72: … INFO yap_lib::chat::tools::agent::fold: Chat tool call tool=browser_click success=false", "…"]}
```

### `report <prompt> [--timeout 540] [--cards deny|allow] [--no-wait]`

Asks one of the built-in testing prompts (`scripts/lab/prompts/`) in a new chat and
saves the answer as Markdown in `test-results/lab/reports/<prompt>-<time>.md`, with a
table and a list of every tool call the lab recorded (to check the model's claims
against). Cards are answered `deny` unless `--cards allow` (the yap-code prompt needs
`allow` for its agents' edits: they're in the scratch workspace). A report takes a
while: if `--timeout` passes first, the lab still saves it when the turn ends — `wait`
returns its path.

| Prompt | Start with |
|---|---|
| `browser-tools` | the default `start` (web on): search, the pages beside the chat, pictures, on example.com, Wikipedia and MDN. |
| `yap-code` | `start --yap-code on`, then `report yap-code --cards allow`: every agent tool in `lab-sandbox`. |
| `connectors` | `start --connectors on`: connector tools and tool search; it never connects or signs in, so it tests only what's there. |

Each prompt tells the model to list its tools of that family with their parameters,
test each one and each parameter on safe public pages or the scratch workspace, note
refusals word for word, and report what works, what fails (with the exact error),
what's inconsistent and suggested fixes, most important first. No sign-ins, purchases
or forms with personal details. A new prompt is a Markdown file in that folder.

### `fake '<json steps>'` (`--fake` only)

Scripts the fake AI's next calls, as the e2e specs' `fakeLlm.scriptChat` does:
`[{"tools":[{"name":"browser_navigate","args":{"url":"…"}}]},{"content":"The answer."}]`.

### `stop`

Closes the test Yap and the control process; the lab folder stays for inspection.

## Where its files are

`test-results/lab/` (git-ignored; `YAP_LAB_DIR` for another):

| Path | What |
|---|---|
| `control.json` | the running lab: its state, port, token, pids (token: only for the CLI on this PC) |
| `start.json`, `daemon.log` | the options it started with; the control process's log |
| `record.jsonl` | the test Yap's record (below) |
| `turns/NNN.json` | each turn's full transcript |
| `shots/`, `reports/` | `shot`'s PNGs, `report`'s Markdown |
| `workspace/lab-sandbox/` | the scratch workspace (`--yap-code on`) |
| `runs/lab/app/Data/` | the test Yap's own data folder (config, chats, logs): portable mode, never `%APPDATA%\yap` |
| `runs/lab/stdout.log` | its log (`logs`) |
| `build.log` | the last `build` |

`npm run test:lab` uses `test-results/lab-test/`, so a lab in use keeps running.

## How it works

- **The test Yap** is the e2e harness's (`desktop/e2e/support/yap.js` `launchYap`):
  a copy of the build in portable mode with its own data folder, WebView2 profile and
  CDP port, in **test mode** (`YAP_E2E=1`, `shell/e2e.rs`), seeded with the e2e suite's
  base config plus the lab's: Chat's model, the switches asked for, debug logging, and
  nothing that spends the model's time behind the lab's back (no cleanup, no learning
  from chats, no meeting reminders, no spoken replies). Nothing of the person's is
  copied: no config, keys, memory or chats.
- **A lab run** is test mode with `YAP_LAB=1` (`shell/lab.rs`). Unless `--fake`
  (`YAP_LAB_LIVE=1`), the two things test mode swaps for stand-ins are real again:
  **the web** (`web::Endpoints::current`: search, page reads, Yap's free sources and
  the pages beside the chat, with all of Yap's usual safety rules) and **Claude Code**
  (`claude_code::cli::find`: the person's own, on their own sign-in).
- **The record** (`YAP_LAB_RECORD`, `record.jsonl`): `stream` — every event the Chat
  view's channel got (steps, chips, cards, page tabs, pictures; the answer's text only
  as its length); `sent` — what `chat_send` answered; `tool` — each call of Yap's tool
  loop with its arguments and result (the hook the model evaluations already use,
  `eval::tool_called`); `claude` — Claude Code's own stream-json messages for a run Yap
  started (its tool calls and results, never the deltas); `event` — the app-wide
  events a spoken question and an agent go by (`yap-assistant-turn`,
  `yap-assistant-stream`, `yap-chat-answering`, `yap-claude-approve`, `yap-agent`,
  `yap-alert`…). Long strings are cut and pictures' base64 left out. The lab sums a
  turn up from it (`scripts/lab/turns.mjs`) with the conversation as Chat saved it
  (`chat_get`) and the live pages (`e2e_page_views`) — never by reading the screen.
- **Cards** are answered with the app's own commands (`chat_page_confirm`,
  `chat_connector_confirm`, `claude_approve_answer`, `agent_go_ahead` /
  `agent_stop`), exactly as the card's buttons do.

## What still works as in a test run, not as for a person

Test mode stays on, so these are as in the e2e suite even in a real lab run — a report
that hits one should say so, and it isn't a bug in the tool:

- **The screen**: `look_at_screen` (and the Voice Assistant's look at the window in
  front) sees test mode's made-up monitors (`e2e_snip`), never the person's real
  screen, so nothing on it ever goes to the model.
- **Apps**: `open_app` records the app (`state`'s `appsOpened`) instead of launching
  it; the person's browser is never opened ("Search in your browser", "Open in …").
- **Other apps' text**: no reading the window in front, the selection or the text
  around the cursor in another app; no pastes, no keys into other apps.
- **Claude Code's own copies of plan chats**: a test run never looks in the person's
  `~/.claude`, so deleting a lab chat doesn't delete Claude Code's transcript of it
  (it stays in Claude Code's projects folder, named after the lab's data folder).
- **Model lists**: providers' live model lists aren't fetched (the plan's models are
  Yap's own list).
- **About you's region**: Windows' country isn't read: no region in the prompts.
- **Connectors**: their tokens would live in the lab's data folder, never Credential
  Manager — but the lab never connects one (no sign-in), and has no command to add
  one yet: the `connectors` report tests what a fresh test Yap offers (tool search,
  the feature's own refusals), not a connected service.
- **Notifications, the tray, the keyboard hook**: off, as in every test run.

## Safety rules

- Never `%APPDATA%\yap`: the test Yap is portable, in the lab's folder. Nothing of the
  person's is copied into it.
- Never a sign-in: the lab doesn't run `claude auth login` (test mode never opens it),
  never reads Claude Code's credentials (`claude auth status` says whether it's
  signed in), never connects a connector.
- Never a key, unless the person set `YAP_LAB_ANTHROPIC_KEY` themselves: it goes only
  into the test Yap's config in the lab's folder, for an `anthropic:` model.
- Never the person's screen, apps, mouse or keyboard (test mode; Playwright works on
  the test Yap's own webviews).
- The prompts and anything you ask: public, neutral pages (example.com, Wikipedia,
  MDN) or the scratch workspace; no logins, purchases, bookings, messages, or forms
  with personal details. The repo is public: no personal details in prompts or
  reports you commit (reports stay in `test-results/`, git-ignored).
- **Ask the maintainer before a real `ask` or `report`**: it uses their Claude plan.

## Its own tests (`npm run test:lab`)

Offline, ~1 minute plus the incremental build (`YAP_LAB_NO_BUILD=1` skips it): the
pure parts (options, the model, the config, the build's freshness, a turn's record
summed up), then a `--fake` run end to end — build, start (its own data folder, the
scratch workspace, one lab at a time), a typed question with its tools' arguments and
results and the page beside the chat, a card that waits and is answered Don't (the
click refused), shots of the main window, the panel and the page, state, a spoken
question and a follow-up in the same conversation, an agent's permission prompt
answered, a report, logs, stop — and the plan's path on Yap's stand-in for Claude Code
(Claude Code's own `WebSearch` recorded). You can use the PC while it runs.
