# Computer use

Chat's model sees, and later drives, the apps the person allows on their PC
(2026-11-19). It works like the Claude desktop app's computer use, built on
Yap's own parts:
- **The tools** are the desktop app's (`request_access`, `screenshot`,
  `zoom`…), the names and shapes Claude knows.
- **The rules** match: an access card first, masked screenshots, tiers by the
  kind of app, never Yap itself.

The plan and its phases are in the private repo
(`docs/computer-use-plan.md`). This file says what's built.

**Phase 1, seeing and permission (2026-11-19)**:
- Built: `request_access` with its card, `list_granted_applications`,
  `screenshot` (one display, every window not allowed masked), `zoom`,
  `switch_display`, `cursor_position`.

**Phase 2, input (2026-11-20)**:
- Built: the five clicks with modifiers, `mouse_move`, `left_click_drag`,
  `left_mouse_down` / `left_mouse_up`, `scroll`, `type`, `key`, `hold_key`,
  `wait`.
- Each action goes through the checks before it's sent ([below](#the-checks-before-every-action)).
- The bar says "Yap is using your PC · Esc to stop", with a border round the
  display.
- Esc, or the person's own mouse or keys, stops it.

**Phase 3, batches, apps, the clipboard, an API key, any app (2026-11-20)**:
- Built: `computer_batch`, `open_application`, `read_clipboard` and
  `write_clipboard` ([below](#phase-3-batches-apps-and-the-clipboard)).
- Claude on an Anthropic key gets the API's own computer toolset
  ([below](#an-api-key-the-apis-own-toolset)).
- The Voice Assistant uses apps from any app, its cards on the bar
  ([below](#from-any-app-the-voice-assistant)).

**Phase 4, teach mode (2026-11-20)**:
- Built: `request_teach_access`, `teach_step`, `teach_batch`: Claude guides the
  person step by step with a tooltip on screen
  ([below](#teach-mode-a-guided-tour)).

**Phase 5, elements instead of pixels (2026-11-20, beyond the desktop app)**:
- Built: `read_window`, `click_element`, `set_value`, `toggle_element`,
  `select_element` ([below](#elements-instead-of-pixels)).

## Modules (`desktop/src-tauri/src/`)

- `computer.rs` — the area:
  - the tools' definitions and the system prompt's line;
  - who's offered them (`offered`, `may`) and what refuses a call now
    (`refused_now`);
  - the one executor every way in calls (`execute`).
- `computer/access.rs` — `request_access`:
  - names resolved to the Start menu's apps (`apps::index`,
    `apps::matching::find`);
  - the access card (a `confirm` card, act `"access"`) and the person's
    answer (`chat_computer_access`).
- `computer/grants.rs` — the apps allowed, per conversation, in memory:
  - each app's tier: browsers read only, terminals and IDEs clicks only,
    the rest full;
  - how a window is told to be an app's (`Granted::owns`);
  - which apps are never allowed (`refused`).
- `computer/windows.rs` — the windows on screen:
  - topmost first, each with its frame, class, exe and AppUserModelID;
  - a Store app's real process found behind `ApplicationFrameHost`;
  - a packaged app's family read from its folder (`package_family`).
- `computer/frame.rs` (pure) — a screenshot's pixels ↔ the display's
  physical ones; zoom regions.
- `computer/shot.rs` — a screenshot:
  - the capture of one display;
  - the masking (`mask`, `pieces`: pure);
  - the display chosen (`choose`);
  - the picture at the frame's size (`picture`);
  - where the pixels come from (`Desk`: this PC, or the stand-in).
- `computer/sim.rs` (debug builds and tests) — the stand-in desktop:
  - a scene of displays and windows (`e2e_computer_scene`);
  - buttons, fields, password fields, links and text inside them, drawn with
    a small built-in font so a model can read them;
  - input changes it: a click focuses, presses or opens, typing fills the
    focused field, keys are logged (`e2e_computer_state` reads it back).
- `computer/act.rs` — the input tools: each call's point read in the last
  screenshot's frame, the PC taken, the checks, the card, the steps sent,
  the person's stop.
- `computer/checks.rs` (pure) — the checks before every action, in order
  ([below](#the-checks-before-every-action)).
- `computer/input.rs` — each action as low-level steps (pure), and the steps
  sent:
  - `SendInput` with absolute virtual-desktop points, every event marked
    `OWN_MARK`;
  - keys by virtual key and scan code;
  - typing as Unicode keystrokes;
  - a stop let go of what it pressed.
- `computer/batch.rs` — `computer_batch`: the actions read first, run in
  order through the same executor, stopped at the first error, coordinates
  from the screenshot before it.
- `computer/open.rs` — `open_application`: an allowed app's window raised
  (restored if minimised), else the app started from its Start-menu entry
  and its first window brought forward.
- `computer/clipboard.rs` — `read_clipboard` / `write_clipboard`, each with
  its grant.
- `computer/teach.rs` — teach mode: the tour's card, each step's tooltip
  waiting for Next or Exit, Next running the step's actions, the tour ended
  with the turn.
- `computer/teach/tooltip.rs` — the tooltip: a native, never-focused window
  beside the anchor with a ring on the spot, kept out of every capture.
- `computer/elements.rs` — elements instead of pixels: `read_window`'s lines
  and refs (kept in the chat's session until the next read), and an element
  acted on through the same checks.
- `computer/uia/tree.rs` — a window's UI Automation tree read depth first
  (time-boxed), and an element found again by its place and acted on
  through its own pattern (Invoke, Value, Toggle, SelectionItem).
- `computer/uia.rs` — UI Automation, time-boxed: the element under a point
  (a link, a control's name), the focused one (a password field).
- `computer/watch.rs` — one chat in control at a time (`HOLD`, 10 s), and the
  person's own input stopping it (fed by `dictation/input_hook.rs`): during
  an action or between two, never their answer on a card of Yap's.
- `computer/indicator.rs` — the bar's line (`bar::set_computer`) and the
  border round the display (four click-through strips, kept out of every
  capture).
- `integrations/claude_live/computer.rs` — the `yap-computer` MCP server for
  a plan chat's or a Code chat's Claude Code.

Frontend:
- `views/chat/ComputerAccess.svelte`: the card, in `PageConfirm`'s chunk,
  above the answer as a question's card is.
- Settings → Voice Assistant: "Let Yap's AI use apps on your PC".

## Who gets it

`computer::offered` and, for a plan chat, `plan.rs` `session_eyes` /
`turn_eyes` (the run's `Eyes::computer`) decide.

**Who:**
- **The switch:** `computer_use`, "Let Yap's AI use apps on your PC", off by
  default. Its toggle is disabled until "Let Yap's AI see your screen" and
  "…with cloud models too" are both on.
- **Which models:** Claude only, on the plan (`claude-plan`) or an Anthropic
  key (`computer::may`). Other cloud models come once evaluations say so,
  and never a model on this PC.
- **Where it's asked:** typed in Chat, or out loud with the Voice
  Assistant's shortcut from any app (`computer::in_chat`; phase 3). Never a
  phone's question, a routine's (`Gates::unattended`) or a question across
  the meetings.
- **Code chats:** a Code chat's own session gets it with the switch on
  (`agents/runner.rs` `direct_eyes`). A Remote Control turn from a phone
  never does, and nor does an agent Chat sends out.

**Refused at call time** (`refused_now`), each with words the model passes
on:
- the switch off;
- the screen's switches off;
- a meeting or a call (`assistant::is_off`);
- a dictation in progress (`dictation_state`).

## The access card

**What `request_access` takes:**
- the apps, by their Start-menu names, up to 20;
- a reason;
- `clipboardRead`, `clipboardWrite` and `systemKeyCombos`.

**What the card shows:**
- each app, with its icon from `apps_icons` and its tier as a tag ("Read
  only", "Clicks only");
- the reason;
- the extras asked for, each a box ticked to start with.

Allow lets the whole set in, and only the extras left ticked. Don't lets
nothing in. No answer in 5 minutes (`access::WAIT`) is a no.

**Never allowed** (listed on the card and in the result, with why):
- Yap itself;
- a password manager (`dictionary/screen_terms.rs` `password_manager`, the
  same list Yap never reads);
- a name the Start menu doesn't have (the closest is named);
- an app whose windows Yap can't tell apart, such as a `steam://` entry.

**Bypass permissions** (a Code chat): the apps are allowed at once, and the
card shows already answered ("Allowed in this chat (Bypass permissions)").
Nothing asks in Bypass (the maintainer's rule), and the refusals above
still hold.

**Nobody to ask** (an evaluation, a turn with no page): refused, and the
model is told to ask in words.

The card is kept with the answer (`ChatMessage.confirms`), answered. An older
Yap's save drops `access`, and the card stays, its label the apps' names.

**What's allowed** lasts for the conversation, in memory only. It's gone when
Yap quits, or after an hour with no computer use in that chat
(`grants::IDLE`).

## Telling an app's windows apart (`Granted::owns`)

A window is an allowed app's when any of these holds:
- **Its exe matches.** The app's exe comes from its shortcut's target, or
  from the `--processStart` app a Squirrel launcher starts (never
  `Update.exe`).
- **Its AppUserModelID matches.** It can be the window's own (its property
  store) or a packaged app's family, read from its `WindowsApps` folder:
  `Microsoft.WindowsNotepad_8wekyb3d8bbwe!App` owns the window whose
  package is `microsoft.windowsnotepad_8wekyb3d8bbwe`.
- **It's the shell, for File Explorer.** File Explorer also owns the desktop
  (`Progman`, `WorkerW`) and the taskbar.

Yap's own windows are never anyone's.

## A screenshot

1. **The display:**
   - the one `switch_display` named;
   - else the one holding most of the allowed apps' windows;
   - else the pointer's;
   - else the primary.

   Displays are named "Display N" from their device (`\\.\DISPLAY2`).
2. **The capture:** as Snip's (`snip::capture`, HDR right). Yap's windows
   are kept out of the capture for that moment (`calls/capture.rs`
   `for_snip`), so the bar, which is click-through, never shows.
3. **The windows:** every one that draws something, topmost first. Hidden,
   minimised, cloaked, click-through and tiny windows are left out: a
   click-through overlay often covers a whole display and draws little, and
   masking it would hide everything.
4. **The mask** (`shot::mask`, pure). Each window's visible part is its frame
   less every window above it:
   - an allowed app's part is kept as it is;
   - any other's is painted grey with an edge, so where it is shows and never
     what's in it;
   - Yap's own windows are painted in Yap's colour, so the model never sees
     through them to something a click there would miss;
   - everywhere else is the background.
5. **The frame:** at most 1,920 px on the long side (`computer::EDGE`, about
   1080p) and never over the provider's own limit. One frame per display, so
   the model's coordinates always map back the same way. `scale` sends a
   smaller picture, and coordinates stay in the frame.

**What comes back to the model:**
- the picture, in the next step for the tool loop, or as MCP image content
  for Claude Code;
- the display and the other displays;
- each allowed app's window, by its app and where it shows (`windows`);
- each masked area where it shows, labelled "not allowed", "Yap", "the
  desktop" or "the taskbar" (File Explorer's), never another app's name
  (`masked`).

None on that display: the note says so and names the other displays.

**What's kept:**
- **The picture**, kept as Chat's screenshots are (`chat-images/`, source
  `"screen"`) and shown with the question, as `look_at_screen`'s are: the
  person sees exactly what the model saw.
- **The masked capture** of the display at its own resolution, in the session
  (`grants::Last`), for `zoom`.

**`zoom`** looks at the screen as it is now: the display is captured and
masked again, the region is given in the last screenshot's pixels, and the
crop is sent at up to the frame's long side. If the display changed since
(another size, another display), it crops the last capture instead and says
so.

## Phase 3: batches, apps and the clipboard

**`computer_batch`** (`computer/batch.rs`) runs several actions in one call,
so a sequence Claude can predict costs one round trip.
- The actions are the desktop contract's: `key`, `type`, the clicks,
  `mouse_move`, `left_click_drag`, `scroll`, `hold_key`, `screenshot`,
  `zoom`, `cursor_position`, `left_mouse_down` / `up` and `wait`. At most 50.
- They're all read first. An unknown action or a missing field refuses the
  batch, and nothing runs.
- Each runs through the same executor and checks as on its own: the window
  under the point, the tier, the ask-first card, the person taking over.
- It **stops at the first error**: "Batch stopped at actions[2]
  (left_click): … 2 completed, 3 remaining."
- **Coordinates**, clicks and zoom regions alike, refer to the screenshot
  taken before the batch, never one taken inside it. After the batch, the
  last screenshot it took is the reference.
- A screenshot or zoom inside it comes back in place, in order; each action's
  summary is its own chip.

**`open_application`** (`computer/open.rs`) brings an allowed app to the
front, at any tier, since bringing an app forward only shows it.
- A window of it on screen, or minimised, is raised.
- Otherwise the app starts from its own Start-menu entry (`apps::open`, never
  a path the model wrote), and its first window comes forward once it's up
  (8 s at most).
- It never raises the desktop or the taskbar for File Explorer.
- Yap has no background app mode: an app opened is in front, as under the
  desktop app's full-screen control.
- A test run starts nothing: `apps::open` records it, and the stand-in
  desktop gets a window for it.

**`read_clipboard` / `write_clipboard`** (`computer/clipboard.rs`) each need
their grant from the access card (`clipboardRead`, `clipboardWrite`).
- Up to 100,000 characters each way.
- What's read is outside content for the turn's safety rule.
- The logs carry lengths only.
- `type` still types new lines as Enter; the clipboard is Claude's to use
  for a long text (write it, then Ctrl+V).

## An API key: the API's own toolset

Claude 5 models on an Anthropic key (`computer::takes_toolset`: Opus, Sonnet
and Haiku 5.5, Fable) get Anthropic's own computer toolset, from its public
docs.
- **The request** (`ai/llm/anthropic.rs` `body`): `{"type":
  "computer_toolset_20260801"}` goes in place of its 17 members
  (`computer::MEMBERS`) as function tools. The desktop contract's other
  tools stay ordinary tools beside it: `request_access`,
  `list_granted_applications`, `switch_display`, `open_application`, the
  clipboard, `computer_batch`.
- **Its calls:** Claude calls a member by its name with `toolset_name:
  "computer"`, and the same executor runs it.
- **Their results** echo `toolset_name` (`toolset_result`). A screenshot's or
  a zoom's picture goes inside its own result
  (`chat/tools/agent/fold.rs`), and a failure is `is_error`.
- **Several in one step** are a batch. After one fails, the rest answer "Not
  executed: an earlier computer action in this turn failed."
  (`chat/tools/agent/calls.rs`).
- **Refused:** if the API refuses the toolset (the account or the model),
  the step is asked again with the members as function tools, and those are
  used for the rest of the run (`computer::refuse_toolset`).
- **Screenshots** are never removed from a turn's history: the 5.5 models'
  thinking is tied to them.

## Teach mode: a guided tour

When the person wants to learn how ("teach me", "show me how"), Claude guides
them instead of doing it all itself (the desktop contract's teach tools).
- **`request_teach_access`** is the access card worded as a tour: "Let Yap's
  AI guide you through this?", the same apps and tiers, no clipboard or
  system-key extras (`access::request_for`, `AccessCard::teach`). Out loud:
  "Shall I guide you through …?" and "Let Yap guide you?" on the bar.
  - Allowed, the tour begins: Yap's main window steps aside (hidden if it
    showed), and comes back when the turn ends.
- **`teach_step`** shows one tooltip and waits for the person:
  - **the tooltip** (`teach/tooltip.rs`) is Yap's dark card, "Step 2 of 5",
    the explanation, what Next will do, then Exit and Next. It sits beside
    the step's anchor (flipped to stay on the display, centred without one),
    with a ring on the spot;
  - it never takes the focus, so the app being taught stays in front, and
    it's kept out of every capture (`WDA_EXCLUDEFROMCAPTURE`);
  - **Next** runs the step's actions as a batch (`batch::run`: the same
    checks), then a screenshot comes back with the result;
  - **Exit**, **Esc** (the input hook) or no answer in 15 minutes answers
    `exited: true`, and every step after it in the turn answers the same.
- **`teach_batch`** queues several steps. Every one is checked before the
  first tooltip shows. Anchors and clicks all refer to the screenshot before
  the call. It answers `stepsCompleted`, each step's results and a final
  screenshot; `exited` with `stepsCompleted` on Exit; `stepFailed` when an
  action fails.
- **The watch during a tour:** it's off while a tooltip waits, so the
  person's click on Next is theirs. While a step's actions run, a key, a
  button or the wheel stops them as ever, but a move of the mouse doesn't
  (`watch::lenient`): the person's hand is on it.
- **The tour ends with the turn** (`chat/turn.rs` calls
  `teach::turn_ended`): the tooltip goes, Yap's window comes back.
- A test run never puts the tooltip on the real screen: `e2e_computer_teach`
  reads it and presses Next or Exit (Yap Lab: `teach next|exit`).

## Elements instead of pixels

Beyond the desktop app's contract (the plan's phase 5): in most Windows apps
(Win32, WinForms, WPF, UWP) Claude can act on a window's controls by their
accessibility instead of their pixels. It's faster, and it doesn't miss when
a window moves. Custom-drawn apps (games, some Electron apps) expose little,
so screenshots and clicks stay the way there.
- **`read_window`** reads an allowed app's window (the one named, else the
  allowed app in front) through UI Automation's control view, depth first,
  at most 400 elements in 4 s. It answers one line an element:
  - the ref, role and name;
  - its value, never a password box's;
  - its state (ticked, selected, focused, disabled);
  - what it takes (`[click]`, `[set_value]`, `[toggle]`, `[select]`);
  - where it is on the last screenshot (`at [x, y]`).

  Nameless panes and groups are walked through but not listed. A link says
  to open it in Yap's browser. The refs (`e1`, `e2`…) last until the next
  read, kept in the chat's session (`grants::Session::elements`). What's in
  the window is outside content for the turn's safety rule.
- **`click_element`, `set_value`, `toggle_element`, `select_element`** act
  by ref through the element's own pattern: Invoke (else Toggle, else
  Select), Value, Toggle and SelectionItem. The mouse never moves and no key
  is sent, so the person can keep working beside it.
  - **The same checks as a click or typing** (`checks::decide`, as a left
    click or as typing into the element): the app allowed, never Yap's or
    an administrator's window, the tier (a terminal's or IDE's take a click,
    never `set_value`; a browser's nothing), never a link, never a password
    box, card or bank details refused, and a Send, Pay or Delete asked first
    on the card (nothing asks in Bypass).
  - **Found again** by its place in the tree, and it must still be what was
    read (its role and name). Otherwise the call says the window changed,
    so read it again.
  - The bar's line says what it does, as for any action; one chat at a
    time.

## From any app: the Voice Assistant

A question asked with the Voice Assistant's shortcut from any app may use
apps too (phase 3).
- **The access card** is said in one line ("Let me use Example Mail, to
  write your reply? Say yes, or press Allow.") and put on the bar: "Let Yap
  use your apps?", each app with what it lets Yap do, the reason, the
  extras, then **Allow** and **Don't** (`access::spoken_line`, `bar_card`).
- **A spoken yes** in that conversation allows it, as for every card that
  asks out loud (`chat/assistant/spoken_ok.rs`).
- **The cards that ask first** (Send, Pay, Delete…) are the page actions'
  ones, said and on the bar. Delete and pay take a click.
- In Bypass permissions nothing asks, so nothing is said.

## Plan chats and Code chats

**The second server.** `RunHandle::mcp_config` lists a second server,
`yap-computer`, beside `yap`, whenever the run's `Eyes::computer` is on:
- the same run, key and checks, at `/mcp/<run>/computer`
  (`server.rs` `Route::Computer`);
- Claude Code sees `mcp__yap-computer__request_access` and the rest: the
  desktop contract's tools under Yap's own server name. It can't be
  `computer-use`: Claude Code 2.1.296 keeps that name for itself and drops
  a server given it without a word. Its init lists every other server but
  not that one (checked on this PC, 19 Nov 2026);
- they're in `--allowedTools`, so Claude Code's own prompt never asks; Yap's
  card is the ask.

**Each call** runs `computer::execute` for the chat turn it belongs to:
- the card goes through the turn's stream;
- each screenshot is shown with the question;
- a look marks the turn's web content (the safety rule).

**What Claude is told:** the plan's system prompt gets a line
(`claude_live::computer::LINES`), and the server's `instructions` say the
same in short.

## The parity pass (Yap Lab against the desktop app's own tools)

Every phase ends with the same tasks run two ways:
- Claude inside Yap, in Yap Lab on the stand-in desktop (`set --computer on`);
- Claude in the desktop app, through its own computer-use tools, on a test
  app only.

Each tool is compared. The table of what matches, what differs on purpose
and the gaps fixed is in the private plan. Phase 1's pass (19 Nov 2026) fixed
three gaps:
- the server's name (`computer-use` is Claude Code's own);
- each allowed window named by its app;
- the desktop and the taskbar called what they are.

## The checks before every action

`computer/checks.rs` `decide`, the first that applies deciding. Before it:
- a meeting, a call or a dictation (`refused_now`);
- another chat holding the PC, or the person having taken over this turn
  (`watch::begin`).

1. **Where it lands.** A pointer action lands on the window under its point
   (and a drag's end); keys and typing land on the window in front.
   - **Yap's own:** refused. Yap's window in front counts as nothing in front,
     so the model is told to click the app first.
   - **Not an allowed app's**, or nothing there: refused.
   - **Running as administrator** (`pasting/elevated.rs`): refused, since
     Windows drops the input (UIPI). The person does it.
   - Stricter than the desktop app: it checks the window under the point,
     not only the one in front.
2. **The tier.**
   - A browser: nothing but a look.
   - A terminal or an IDE: a plain left click and a scroll, never typing,
     keys, right or middle clicks, modifier clicks or drags.
3. **A system combo** without the `systemKeyCombos` grant: refused. These
   are the Windows key, Alt+F4, Alt+Tab, Alt+Esc, Ctrl+Esc and
   Ctrl+Shift+Esc (`input::system_combo`).
4. **A link under a click** (UI Automation): refused. Its address goes to
   Yap's browser instead.
5. **Typing** into a password field, or text with card or bank details
   (`act/rules.rs` `typed_secret`): refused.
6. **A click on a control named Send, Pay, Buy, Delete, Sign in…**
   (`act/rules.rs` `needs_ok`), or Enter in a chat app: **asks first**, on
   the browser's card ("press ‘Send’ on Example Mail").
   - In Bypass permissions nothing asks: the action goes ahead, and Claude is
     told to ask in the chat instead.
7. **Robot mode:** only the windows the test opened (`shell/realtest.rs`
   `gate`).

## Driving the PC

- **One chat at a time.** A chat holds the PC for 10 s after its last input
  action; another chat's actions are refused meanwhile.
- **The indicator.**
  - The bar shows "Yap is using your PC", the action's summary and "Esc to
    stop". It shows bar on or off, never follows the pointer and never takes
    a click.
  - A thin red border goes round the display.
  - Both go 4 s after the last action.
- **The person takes over.** `dictation/input_hook.rs` reports their real
  input: anything not injected and not Yap's own. Any of these stops it:
  - a key (Esc above all);
  - a button or the wheel;
  - a move of more than 10 px within one burst, robot mode's thresholds,
    but only while an action runs.
- **When it counts** (`watch.rs`, 20 Nov 2026, found by the first real-API
  run): from an action's start until the bar's line goes, so **between two
  actions too**. Esc pressed while Claude thinks about its next step takes
  the line away at once, and that next step is refused. Before, only input
  during an action counted, and most of a real turn is the model thinking.
- **Never Yap's own cards.** While a card waits (the access card, an
  ask-first card, a question: `confirm.rs`'s `Pending` holds a
  `watch::CardUp`), the person's clicks and keys answer it, and nothing from
  before it went stops what follows. Moves don't count for 1.5 s after a
  card goes either (the hand leaving it). Before, the mouse moving to a Send
  card's Allow stopped the very click it allowed.
- **What a stop does.**
  - The action stops between two steps, and what it held is let go.
  - The indicator goes at once.
  - The model is told the person took over.
  - The rest of that turn's input actions are refused, until the person's
    next message.
  - ■ Stop drops the turn: the steps stop the same way (`Driving`'s flag on
    drop).
- **The bar's own Esc watch** ignores injected keys, so a model's Esc never
  stops a reading or closes a card.

## Safety

- **What's on screen** is outside content: a screenshot sets `screen_seen`,
  so `STRONG_LOCKED` holds after it, as after `look_at_screen`.
- **The logs** carry counts, sizes and timings only: never an app's name, a
  window's title or contents, or the model's reason.
- **Every input event is Yap's own** (`OWN_MARK`). Yap's own hook lets it
  by, so a model typing Win+Alt+S never fires a Yap shortcut, and the watch
  never takes it for the person's.

## Tests

- **Unit tests** (`cargo test computer::`, `claude_live::computer`), on the
  stand-in desktop:
  - the frame maths (negative origins, a 4K display at 1,920 px);
  - masking (allowed over masked, masked over allowed, Yap's own);
  - telling windows apart (packaged, Store, Squirrel, File Explorer);
  - the tiers, and the refusals;
  - the card's extras, and Bypass;
  - the whole of phase 1 end to end.
- **e2e:** `computer-access.spec.js`:
  - a plan chat's card with what can't be allowed;
  - Allow, then a screenshot with the other app and Yap masked, its picture
    back as MCP image content, kept with the question;
  - Don't, after which the screenshot is refused;
  - the Settings switch.
- **e2e for phase 2:** `computer-input.spec.js`:
  - a click and typing land in the stand-in app, and the bar shows the
    indicator, then lets it go;
  - Send asks first, and Don't presses nothing; Allow once by mouse sends
    (the person's move, click and keys on the card never take over);
  - Esc stops a held key, and the rest of the turn is refused;
  - Esc between two actions (the stand-in Claude Code waiting 3 s) takes the
    bar's line away at once, and the next action never runs.
- **Unit tests for phase 2:** the checks' table; the steps (combos, a
  click's modifiers, a drag, what a stop lets go); the watch; the whole of
  phase 2 on the stand-in desktop.
- **e2e for phase 3:** `computer-batch.spec.js`:
  - a plan chat's batch (two clicks, typing and a screenshot that comes back
    in place), `write_clipboard`, and `open_application` of an app not on
    screen, which opens from its Start-menu entry;
  - a batch stopped at its first error, nothing after it run, and the
    clipboard refused without its grant;
  - Claude on an Anthropic key (the fake server): the toolset in place of
    its members, `toolset_name` on calls and results, a screenshot's picture
    in its own result;
  - the Voice Assistant from another app: the access card said and on the
    bar, a spoken yes, then the action done.
- **Unit tests for phase 3:** a batch read whole first; a batch, the
  clipboard and `open_application` on the stand-in desktop; the request
  with the toolset; which models take it; the access card said out loud.
- **e2e for phase 4:** `computer-teach.spec.js`:
  - the tour's card, a step whose Next runs it with a screenshot after;
  - a batch left with Exit on its second step;
  - Yap's window stepping aside, and the tour ending with the turn;
  - Esc on a tooltip as its Exit.
- **Unit tests for phase 4:** a step checked first; a tour on the stand-in
  desktop (Next, a batch left with Exit, nothing after it); where the
  tooltip goes.
- **e2e for phase 5:** `computer-elements.spec.js`: a window read by ref,
  its text set, a box ticked and a button clicked with the mouse never
  moving; Send by ref asking first (Don't: nothing pressed).
- **Unit tests for phase 5:** the tree's lines; what's listed; elements by
  ref on the stand-in desktop (Send asked, a link and a password box
  refused, a stale ref).
- **Robot tests** (real windows, idle PC only):
  - `computer-see.real.js`: the test pad allowed, an Edge page over it
    masked;
  - `computer-input.real.js`: clicks into the pad's box, types, Ctrl+A,
    types over it, and never types into its password box;
  - `computer-teach.real.js`: the real tooltip beside the pad's box, its
    Next clicked as the person would, the focus kept, the step done;
  - `computer-elements.real.js`: the pad (WinForms) read by ref through
    real UI Automation, its box filled through its Value pattern, its
    password box refused, its button pressed through Invoke.
