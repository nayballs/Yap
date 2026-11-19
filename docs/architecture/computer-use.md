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
- Not yet: clicks and typing (phase 2), batches, opening apps and the
  clipboard (phase 3), teach mode (phase 4).

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
- `computer/sim.rs` (debug builds and tests) — the stand-in desktop: a scene
  of displays and windows drawn in flat colours (`e2e_computer_scene`).
- `integrations/claude_live/computer.rs` — the `computer-use` MCP server for
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
- **Where it's asked:** in Chat, typed or out loud with Chat in view. Never a
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
- each masked window's rectangle, labelled "not allowed" or "Yap", never an
  app's name;
- how many of the allowed apps' windows are in it.

None on that display: the note says so and names the other displays.

**What's kept:**
- **The picture**, kept as Chat's screenshots are (`chat-images/`, source
  `"screen"`) and shown with the question, as `look_at_screen`'s are: the
  person sees exactly what the model saw.
- **The masked capture** of the display at its own resolution, in the session
  (`grants::Last`), for `zoom`.

**`zoom`** crops that capture: the region is given in the screenshot's
pixels, and the crop is sent at up to the frame's long side.

## Plan chats and Code chats

**The second server.** `RunHandle::mcp_config` lists a second server,
`computer-use`, beside `yap`, whenever the run's `Eyes::computer` is on:
- the same run, key and checks, at `/mcp/<run>/computer`
  (`server.rs` `Route::Computer`);
- Claude Code sees `mcp__computer-use__request_access` and the rest, the
  desktop contract's names;
- they're in `--allowedTools`, so Claude Code's own prompt never asks; Yap's
  card is the ask.

**Each call** runs `computer::execute` for the chat turn it belongs to:
- the card goes through the turn's stream;
- each screenshot is shown with the question;
- a look marks the turn's web content (the safety rule).

**What Claude is told:** the plan's system prompt gets a line
(`claude_live::computer::LINES`), and the server's `instructions` say the
same in short.

## Safety

- **What's on screen** is outside content: a screenshot sets `screen_seen`,
  so `STRONG_LOCKED` holds after it, as after `look_at_screen`.
- **The logs** carry counts, sizes and timings only: never an app's name, a
  window's title or contents, or the model's reason.
- **Phase 1 has no input**: no clicks, keys or typing. The system prompt says
  so, and Claude tells the person what to do instead.

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
- **The robot test** (real windows) comes with phase 2's input. Phase 1's
  capture is Snip's, which its robot tests already cover.
