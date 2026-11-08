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
npm run -s lab -- sheet                      # every turn's screenshots on one PNG
npm run -s lab -- report browser-tools       # the whole family tested, a Markdown report
npm run -s lab -- run browser-tools          # a feature script: its steps, its checks, a sheet
npm run -s lab -- stop
```

Read the JSON, find what's wrong (a refusal that shouldn't be, a parameter ignored, a
step that never shows), look at the turn's screenshots (every question takes them, in
the light and the dark theme: [Screenshots](#screenshots)), fix it, `build`, `start`
again, ask again. `-s` keeps npm's
own banner off stdout, so stdout is exactly one line of JSON (or run
`node scripts/lab/lab.mjs <command>`). Every command exits 0 when `ok` is true, 1
otherwise. A real question can take minutes and a report much longer: keep
`--timeout` under your shell's own limit (a Claude Code Bash call stops at 10
minutes) and `wait` for the rest — the turn goes on in the test Yap whether or not a
command is waiting on it.

**Cost.** A real run's Chat is the person's own Claude Code on their own Claude plan:
each question uses their plan's usage (a `report` uses a lot). **Ask the maintainer
before any real `ask`, `report` or `run`.** `start`, `check`, `state`, `shot`, `probe`,
`set`, `sheet`, `logs` and `stop` ask the model nothing. `--fake` runs cost nothing at all. An API model
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
| `--size <W>x<H>` | The main window's size in CSS pixels (default `1280x860`, at least `860x600`), set at the end of the test Yap's setup (`shell/lab.rs`, `YAP_LAB_WINDOW`), so shots compare between runs. `start` says what it got: `window: {width, height, dpr}`. |
| `--no-shots` | No screenshots after each question (they're on by default). |
| `--theme light\|dark` | Shots in one theme only (both by default). `ask`, `report` and `run` take both flags too, for themselves. |
| `--fake` | Offline: the e2e suite's fake AI (`fake-gpt-5`, scripted with `fake`) and stand-in web; with `--model claude-plan`, Yap's stand-in for Claude Code (`yap.exe fake-claude`) on `desktop/e2e/support/fake-claude/plan-chat.jsonl` (`--fake-claude-script <file>` for another). `--fake --live-web`: the fake AI on the real web (the lab's own checks). |

For the plan (and for `--yap-code on`), `start` checks Claude Code the way Yap does
(`claude_plan_status`: found, its version, `claude auth status`): not installed, too
old or **not signed in, it says so and stops** — the lab never signs in.

```json
{"ok":true,"pid":30064,"model":{"kind":"plan","provider":"claude-plan","model":"claude-sonnet-5-5"},"dataDir":"…\\test-results\\lab\\runs\\lab\\app\\Data","windows":["onboarding","settings","notepad","quick-note","overlay"],"window":{"width":1280,"height":860,"dpr":1},"yapPid":49136,"claudeCode":{"installed":true,"version":"2.1.296","tooOld":false,"signedIn":true,"plan":"max","authMethod":"claude.ai","error":null,"standIn":false},"lab":"…\\test-results\\lab"}
```

### `check`

`start` on the plan, then `stop`: is Claude Code on this PC and signed in, as Yap sees
it? Asks the model nothing.

### `ask "<text>" [--spoken] [--chat new|same|<id>|code:<workspace>] [--timeout 600] [--cards allow|deny] [--no-shots] [--theme light|dark]`

Puts a question to Chat and waits until the turn ends (answered, stopped or failed),
**or a card waits for the person**, or `--timeout` seconds pass.

- Typed: in Chat's own box and Send, as a person would (Playwright over CDP: the
  same `chat_send` and stream the Chat view uses). `--spoken`: through the Voice
  Assistant's path (`e2e_assistant_ask`: its shortcut's steps with the words in place
  of the microphone).
- `--chat new` (default) a new chat; `same` the last turn's conversation (left as it
  is when Chat still shows it: a follow-up in a Code chat too); `<id>` that
  conversation (Chat opens on it); `code:<workspace>` a new Code chat there
  (`--yap-code on`).
- `--cards allow|deny`: answer every card this turn asks at once (otherwise `ask`
  returns `status: "waiting"` and you `answer` it).
- `shots`: the turn's screenshots ([Screenshots](#screenshots)) — after it ends, and
  of each card while it waited — each `{file, window, theme, set, width, height,
  dpr}` (the thread's with `pieces`, `cssHeight`, `scrollHeight`); `shotErrors` says
  what couldn't be taken.

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

### `wait [--timeout 600] [--agents [--cards allow|deny]]`

Waits on the current turn again (after `running`), and returns `ask`'s JSON.
`--agents`: waits until no Yap Code agent is queued or running (one asking counts as
stopped), and returns the agents, the cards they wait on and the reports they put in
their chats. With `--cards`, it answers each card the agents ask on the way (a
permission prompt, a plan's Go ahead) — shots of it first, into the last turn's folder
— and goes on waiting; `autoAnswered` lists them. `agents` returns the same view at
once, without waiting.

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

### `shot [--window thread|main|bar|panel|page] [--theme light|dark|both] [--name <n>] [--selector <css>]`

A PNG over CDP. `thread`: the current turn of Chat's thread, whole (scrolled and
stitched); `main`: the main window; `bar`: the Yap bar; `panel`: the panel beside the
chat; `page`: the live page beside the chat itself (its own WebView2 — in `main` and
`panel` its area is blank). `--selector`: one element of that window instead (the first
match, scrolled into view), e.g. `--window main --selector
'[data-testid="chat-agent-todo-card"]'`. Without `--theme` it's as it is now, saved as
`shots/<time>-<window>.png`; with it, in that theme or both. `--name`: into the current
turn's folder as `<name>-01-panel-light.png`…, listed with the turn's shots (the sheet
shows it under "Shot “<name>”").

```json
{"ok":true,"shot":"…\\shots\\003\\agent-done-01-panel-light.png","window":"panel","shots":[{"file":"…\\shots\\003\\agent-done-01-panel-light.png","window":"panel","theme":"light","set":"named:agent-done","width":460,"height":808,"dpr":1},{"file":"…\\shots\\003\\agent-done-02-panel-dark.png","window":"panel","theme":"dark","set":"named:agent-done","width":460,"height":808,"dpr":1}]}
```

### `probe --window main|thread|panel|page|bar [--css <selector>] [--text <words>]`

What's on screen, over CDP: how many elements match `--css` in that window and how
many of them show; whether `--text` is in its visible words (`thread`: Chat's thread;
`panel`: the panel beside the chat); and the window's theme. A script's `expect`
uses it.

```json
{"ok":true,"window":"thread","found":true,"theme":"light","words":"THE END","has":true}
```

### `set '<json>'` or `set [--web on|off] [--yap-code on|off] [--connectors on|off]`

Changes the running test Yap as another surface would (`save_config`, Settings told,
checked after): `web`, `yapCode` (with it: Claude Code checked, the scratch workspace
made and added), `connectors`, `bypass` (new chats' Mode Bypass permissions, the test Yap's own one-time OK given; off: Auto), `config: {…}` (other settings; never a key, a sign-in
or Chat's model — `start` sets those), and `files: [{path, text} | {path, png: {width,
height, shape, color, background}}]` seeded into **the lab's scratch workspace only**
(`shape`: circle, square, triangle, stripes; drawn by the lab, nothing from outside).

```json
{"ok":true,"config":{"webSearch":true},"files":["pictures/sun.png","notes.md"]}
```

### `state`

```json
{"ok":true,"lab":{"model":{…},"effort":null,"web":true,"yapCode":false,"connectors":false,"fake":true,"shots":true,"theme":"both","size":"1280x860","dataDir":"…","pid":51836,"upMs":80769,"workspace":null},
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

### `report <prompt> [--timeout 540] [--cards deny|allow] [--no-wait] [--no-shots] [--theme light|dark]`

Asks one of the built-in testing prompts (`scripts/lab/prompts/`) in a new chat and
saves the answer as Markdown in `test-results/lab/reports/<prompt>-<time>.md`, with a
table and a list of every tool call the lab recorded (to check the model's claims
against), and a **Screenshots** section: the turn's contact sheet beside the report
(`<prompt>-<time>.sheet.png` and `.sheet.html`) and a relative link to each shot. Its
JSON has `sheet: {png, html, width, height}`. Cards are answered `deny` unless `--cards allow` (the yap-code prompt needs
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

### `sheet [--turns 1-3] [--out <png>]`

A **contact sheet** of the run's shots: one PNG, 1600 pixels wide, and the same page as
HTML beside it (`.html`, the shots by relative path, so it opens from disk) —
`test-results/lab/sheets/sheet-<time>.png` unless `--out`. A section a turn (its
number, how it ended, its question's first words), then its sets of shots (after the
turn, while a card waited, each named shot), light beside dark, each at no more than
its own CSS size; a thread shot taller than 1,400 pixels is cut into even pieces
(`1/3`, `2/3`…) with light and dark side by side, so it stays readable. `--turns`: `3`,
`1-3`, `2-` or `1,4`. Rendered in the lab's own headless browser (Playwright's Chromium
when it's installed, else Edge; `YAP_LAB_CHANNEL` to choose). Works after `stop` too.

```json
{"ok":true,"png":"…\\sheets\\sheet-2026-11-08T13-38-52.png","html":"…\\sheets\\sheet-2026-11-08T13-38-52.html","width":1600,"height":4688,"turns":[1],"shots":6}
```

### `run <script.json|.mjs> [--no-shots] [--theme light|dark]`

Runs a **feature script** ([below](#feature-scripts)) on the running lab: its steps in
order, its checks, then a contact sheet of its turns
(`sheets/<script>-<time>.png`). A path, or a name in `scripts/lab/scripts/`. Prints one
JSON summary and **exits 1 when any check failed** (a soft one only warns). A step the
rest depend on (`ask`, `answer`, `wait`, `set`, `fake`) stops the script when it fails
(`stopped`). A real run's questions use the plan like `ask`: **ask the maintainer
first**, and run a long script in the background (a Bash call stops at 10 minutes).

`agent-panel` on a `--fake` lab (abridged):

```json
{"ok":true,"script":"…\\scripts\\lab\\scripts\\agent-panel.json","name":"agent-panel","passed":13,"failed":0,"failures":[],
 "warnings":[{"step":13,"check":"thread: [data-testid=\"chat-agent-todo-card\"] shows","detail":"{\"count\":0,\"visible\":0}","note":"The to-do card shows only when the agent kept a to-do list.","soft":true}],
 "turns":[1],"sheet":{"png":"…\\sheets\\agent-panel-2026-11-08T13-41-32.png","html":"…","width":1600,"height":5329,"turns":[1],"shots":16},
 "steps":[{"n":1,"step":"fake","ok":true},{"n":2,"step":"ask","ok":true,"turn":1,"status":"answered","conversation":1,"tools":["start_agent"],"shots":4},
          {"n":3,"step":"expect","ok":true,"checks":[{"check":"used start_agent","pass":true},{"check":"nothing refused","pass":true}]},
          {"n":5,"step":"shot","ok":true,"skipped":"main (light): nothing in the main window matches [data-testid=\"chat-agent-todo-card\"]; …"},
          {"n":7,"step":"wait","ok":true,"agents":["1a1bb647a2bde18b288: done"],"answered":["approve:1 allow"]},…],"ms":7717}
```

A failed check says what it wanted and what there was:
`{"step":3,"check":"used browser_click","detail":"no call of browser_click (called: browser_navigate, browser_read)"}`.

### `fake '<json steps>'` (`--fake` only)

Scripts the fake AI's next calls, as the e2e specs' `fakeLlm.scriptChat` does:
`[{"tools":[{"name":"browser_navigate","args":{"url":"…"}}]},{"content":"The answer."}]`.

An argument that depends on what the turn has seen — a ref on a live page
(`--fake --live-web`), a point of its screenshot — is a **placeholder** the lab
fills in as the call is made, from the request's tool results (`fake.mjs`, as an
e2e spec's `args: (r) => …`): `"$ref:<words>"` the ref of the element or match
named exactly so in the turn's latest `browser_read` (elements) or `browser_find`
that lists it, `"$ref:<kind>:<words>"` only of that kind (`heading`, `link`,
`button`, `item`, `text`, `textbox`, `frame`…), words ending in `*` any that
start so (`"$ref:text:A lighthouse is a tower*"`: a paragraph); `"$x:0.5"`,
`"$y:0.3"` that fraction of the latest `browser_screenshot`'s width or height.
Not found: 0, which the tool refuses as it would a model's wrong ref.
`{"tools":[{"name":"browser_drag","args":{"from":{"ref":"$ref:item:Item 3"},"to":{"ref":"$ref:item:Item 1"}}}]}`.
The shots and zooms need a fake that reads images: `start --fake --model fake-gpt-5-vision`.

### `stop`

Closes the test Yap and the control process; the lab folder stays for inspection.

## Screenshots

Every `ask` and `report` takes them (unless `--no-shots`), over CDP, without touching
the screen:

- **after the turn ends**, and **whenever a card is waiting** (before it's answered,
  once a card; `wait --agents --cards` does the same for an agent's cards);
- of **the thread's turn, whole**: from the turn's question to the end of the thread,
  however long — the thread scrolled a screen at a time, each screen captured at the
  device's pixels, the pieces stitched into one tall PNG (from just above the question,
  never into the turn before it; the "↓" button and the list's button hidden — the
  list's button sat over every capture's top-left corner, half cut off, 8 Nov 2026; the
  thread's scroll put back after). Its entry says
  how many `pieces`, its `cssHeight` and the thread's `scrollHeight`;
- of **the panel beside the chat**, when it's open: its document, picture, page or
  Agent tab, as it shows (a page tab's own area is blank: the page is a webview of its
  own);
- of **the live page tab** (that webview), when one is on screen (a tab set to a
  desktop width comes out at its CSS size, the page drawn small in its top-left
  corner: Playwright's capture under the page's zoom — Yap's own shots of it, and
  the person's view, are the panel's size);
- each **in both themes**: Yap's windows marked `data-yap-theme="light"`, then
  `"dark"`, as Settings' Theme marks them (core/appearance.js), motion reduced while
  they're taken (no transition caught half way), then put back; the live page — a
  site, not Yap — with `prefers-color-scheme` light, then dark (a site with no dark
  scheme looks the same). `--theme light|dark` takes one.

The main window is the same size every run (`--size`, 1280 × 860 by default), so shots
of one run compare with another's. They go in `test-results/lab/shots/<turn>/`, window
by window and theme by theme: `01-thread-light.png`, `02-thread-dark.png`,
`03-panel-light.png`…; a card's set `card-confirm-1-01-thread-light.png`…; a named
shot's `<name>-01-panel-light.png`…. The turn's JSON lists them, and so does
`shots/<turn>/index.json` (what `sheet` reads). A shot that couldn't be taken is in
`shotErrors`; it never fails the turn.

## Feature scripts

A feature's check written down once and run again after each fix: `run <script>`. A
script is JSON (or a `.mjs` whose default export is one, or a function `({ drawPng }) =>
one`), in `scripts/lab/scripts/`:

```json
{
  "name": "example",
  "about": "What it checks, in a line.",
  "needs": { "web": true },
  "steps": [
    { "set": { "web": true } },
    { "ask": "Open https://example.com/ beside the chat and read me its heading.", "timeout": 300 },
    { "expect": { "status": "answered", "used": "browser_navigate", "refused": false,
                  "tool": { "name": "browser_read", "ok": true, "result": "Example Domain" },
                  "answerContains": "Example Domain", "visible": "Example Domain",
                  "selector": { "window": "main", "css": "[data-testid=\"chat-panel-slot\"]" } } },
    { "shot": "the-page", "window": "page" },
    { "ask": "Now scroll it to the bottom.", "chat": "same" },
    { "expect": { "tool": { "name": "browser_scroll", "path": "data.scroll.at_end", "equals": true } },
      "soft": true, "note": "Why this one only warns." }
  ]
}
```

`needs`: what the lab must have been started with (`web`, `yapCode`, `connectors`):
`run` says how to start it otherwise. `todo`: a line `run` repeats as a warning. Every
step can carry a `note` (shown with its failures) and `soft: true` (its failures only
warn).

| Step | |
|---|---|
| `ask` | `"ask": "<text>"`, with `spoken`, `chat` (`new`, `same`, an id, `code:<workspace>`), `cards` (`allow`/`deny`), `timeout`, `shots: false`, `theme`. |
| `answer` | `"answer": "next"` (the first card waiting), a card's id, or `{ "kind": "page", "label": "…" }`; `how`: `allow` (default), `deny`, `always`. |
| `wait` | `"wait": "turn"` or `"agents"` (with `cards`: answer the agents' cards on the way). |
| `shot` | `"shot": "<name>"`, `window` (`thread` default, `main`, `panel`, `page`, `bar`), `theme` (both by default), `selector`, `optional: true` (nothing to shoot is a skip, not a failure). |
| `set` | As the `set` command: `{ "web": true, "yapCode": true, "config": {…}, "files": [ … ] }`. |
| `expect` | Checks on the last turn (below). |
| `fake` | The fake AI's next calls (as `fake`): played only in a `--fake` lab, skipped otherwise, so one script can be checked offline too. |
| `note` | Just words, in the summary. |

| `expect` | Passes when |
|---|---|
| `status` | the turn ended so (`answered`, `waiting`, `failed`, …; a list: any of them). |
| `used` / `notUsed` | each tool named was called / wasn't (a name, or `"/regex/"`; Yap's and Claude Code's calls, MCP names as Yap's). |
| `refused` | `true`: some call was refused; `false`: none; a tool's name: that one was. |
| `failed` | `true`: some call failed (`ok: false`); `false`: none did. |
| `card` | `true` / `false`: a card was asked or not; `{ "kind": "page", "does": "buy", … }`: one like it was. |
| `answerContains` / `answerNotContains` / `answerMatches` | the answer has the words (any case) / hasn't / matches the regex. |
| `pictures`, `files`, `documents`, `pagesShown`, `sources`, `tools` | how many: a number, or `{ "min": 1, "max": 3 }`. |
| `tool` | some call matches all of it (a list: each one does): `name`, `via`, `ok`, `refused`, `args` (`{ "tab": 1, "keys": "/enter/i", "ref": "*" }`: equal, a regex, or there at all), `argsHas` / `argsLack`, `result` / `resultNot` (words or a regex on the whole result), and `path` into the result's JSON (`"data.scroll.y"`) with `equals`, `gt`, `gte`, `lt`, `matches` or `notMatches`. The whole arguments and results: the turn's transcript. |
| `agent` | an agent (from the last `wait agents`, else now) has `status`, `files` (a count), `folder`, `error` (`null`: none), `mode` (`bypassPermissions`…), `plan`, `readOnly` (its latest turn read only); `count` how many must (one by default). |
| `selector` | `{ "window": "main", "css": "…" }` shows (`present: false`: doesn't; `count`: how many show), over CDP (`probe`). |
| `visible` / `notVisible` | the thread's visible words have / haven't these. |

**The scripts there:**

| Script | Start with | What it covers |
|---|---|---|
| `browser-tools` | the default `start` (web on) | The browser tools' fixes of 8 Nov 2026, in three questions on Wikipedia, MDN and example.com: `browser_scroll` reporting where it ended (`data.scroll.y`, not at the top), `browser_scroll_to` by ref scrolling, `tab` on `browser_find` and `browser_scroll`, "changed" after a hover, PageDown and a click that opens a box, Escape closing a hover preview (soft), Enter after typing submitting through the check (`went to`, no card), a dropdown's choice not called typing, `find_pictures` and the lead picture (the thread's shot shows which), the bad-ref error not blaming the page. Then, in two more, the gaps closed the same day: a double click selecting a word and a right-click on Wikipedia (no menu of its own: said so); MDN at a desktop width (`browser_tabs` width 1280: its shot saying so, its live examples listed as frames from another site; soft, MDN decides what it draws); jQuery UI's sortable demo, a list in a same-origin frame, read and an item dragged (`browser_drag`, what moved said: soft). Turns 4 and 5 carry the fake AI's calls (placeholders for the live pages' refs), so `start --fake --live-web --model fake-gpt-5-vision` runs them with no model's usage; turns 1–3 carry none. |
| `share-files` | `start --yap-code on` | Chat sharing files from a Code chat's workspace: one picture, then three (inline), then `notes.md` (a card); the workspace seeded with three drawn pictures and a made-up `notes.md`. **Its `todo`**: written before the share-files tool reached main — put its real name in `used` and check where an answer keeps file cards (`turns.mjs` `files`) once it has. |
| `agent-panel` | `start --yap-code on` | An agent at work in `lab-sandbox`, its cards allowed: the Agent panel and the thread's to-do card while it runs and once it's done (named shots, both themes), the agent done with a file changed. Then a plan nobody acts on (8 Nov 2026): a fresh agent asked for a plan first, its prompt answered no (`answer next deny`), its plan in Needs you with its buttons in the row (`plan-waiting`); a newer fresh agent at work in the same Code chat (`newer-turn-running`), and once it's done the plan is "Plan not used" in Finished, not Needs you (`plan-not-used`). Then a Bypass permissions Code chat asked to catch up on the workspace (`set` `bypass`): no card, the agent read only at Bypass, never a plan, done, nothing in Needs you. Its `fake` steps make it pass on `--fake` too (the stand-in agent). |

## Checking that it looks right

The JSON says what a turn did; the shots say how it looked. The maintaining session can
look at PNGs itself (its Read tool shows them), so:

1. **Open the sheet first** (`sheet`, or the one `run` and `report` made): one picture
   of every turn, light beside dark. Then open a single shot when something needs a
   closer look — a thread shot at its full size, the panel, the page.
2. **What to look for**, in both themes:
   - text cut off: a line ending mid-word at an edge, an ellipsis where the whole
     thing should show, a card or a pill clipped by its box;
   - overlap: a button over text, a card over the composer, two things in one place;
   - contrast in dark: grey on grey, a light box left in the dark theme (a hard-coded
     colour), an icon that vanishes, a link you can't tell from text;
   - alignment: the thread's column, cards' edges and the panel's header lining up;
     spacing that differs between two cards of one kind;
   - things that didn't render: an empty panel, a picture's empty frame, a chip with
     no words, raw Markdown (`**`, `|---|`) in an answer, a spinner left spinning
     after the turn ended;
   - the content itself: the right picture (the lead image, not a logo), the
     document's sections all written, the to-do card's items ticked when it's done.
3. Compare with an earlier run's sheet when a change was visual: the window's size is
   fixed, so the same question lays out the same way.
4. **When the feature is visual, send the sheet to the maintainer** with the exact
   steps to see it in the dev build (the private `docs/maintainer.md` says how): the
   sheet is a quick way to show light and dark at once, never a replacement for them
   trying it. Lab sheets and reports stay in `test-results/` (git-ignored): never
   commit them (the repo is public).

## Where its files are

`test-results/lab/` (git-ignored; `YAP_LAB_DIR` for another):

| Path | What |
|---|---|
| `control.json` | the running lab: its state, port, token, pids (token: only for the CLI on this PC) |
| `start.json`, `daemon.log` | the options it started with; the control process's log |
| `record.jsonl` | the test Yap's record (below) |
| `turns/NNN.json` | each turn's full transcript (its shots listed in `result.shots`) |
| `shots/NNN/` | each turn's screenshots and their `index.json`; `shots/` itself: `shot`'s own unnamed ones |
| `sheets/` | contact sheets (`sheet`, `run`), each PNG with its HTML |
| `reports/` | `report`'s Markdown, each with its sheet (`.sheet.png`, `.sheet.html`) |
| `workspace/lab-sandbox/` | the scratch workspace (`--yap-code on`) |
| `runs/lab/app/Data/` | the test Yap's own data folder (config, chats, logs): portable mode, never `%APPDATA%\yap` |
| `runs/lab/stdout.log` | its log (`logs`) |
| `build.log` | the last `build` |

`npm run test:lab` uses `test-results/lab-test/`, so a lab in use keeps running.
Each `start` begins a new run: the last run's `turns/` and `shots/NNN/` go (its turn
numbers start at 1 again); reports, sheets and `shot`'s own files stay.

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
- **Screenshots** (`scripts/lab/shots.mjs`) are Playwright's CDP captures of the test
  Yap's own webviews (clips at the device's pixels), the thread's pieces stitched with
  Playwright's bundled PNG codec (`png.mjs`); the theme is marked on `<html>` and held
  by a MutationObserver while they're taken. The main window's size is set by
  `shell/lab.rs` from `YAP_LAB_WINDOW`. The contact sheet (`sheet.mjs`) is an HTML page
  rendered in a headless browser of the lab's own. Scripts: `script.mjs` (the runner,
  and the checks, pure); `fake.mjs` (the fake AI's placeholders, filled in from each
  request's tool results: `fake`, pure).

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

Offline, ~1 minute plus the incremental build (`YAP_LAB_NO_BUILD=1` skips it; the
build goes where `lab build` puts it, so set `YAP_LAB_TARGET_DIR` when that's someone
else's): the pure parts (options, the model, the config, the build's freshness, a
turn's record summed up, shot names and themes, PNG stitching, the sheet's layout, a
script's checks), then a `--fake` run end to end — build, start (its own data folder,
the scratch workspace, the window at 1280 × 860, one lab at a time), a typed question
with its tools' arguments and results and the page beside the chat, a long answer's
shots (the thread stitched from several screens, its height the thread's whole
`scrollHeight`, the question at its top and the answer's end at its bottom, the dark
shot darker than the light one, the panel and the page, the theme put back), a card
that waits (its shots taken first) and is answered Don't (the click refused), shots of
the main window, the panel and the page, state, a spoken question and a follow-up in
the same conversation, an agent's permission prompt answered, a report with its sheet
and Screenshots section, `set` (a setting, files seeded, a path outside refused), a
script `run` that passes and one whose wrong `expect` fails it with exit 1, a `sheet`
of two turns (1600 wide, its HTML beside it), logs, stop — and the plan's path on
Yap's stand-in for Claude Code (Claude Code's own `WebSearch` recorded). You can use
the PC while it runs.
