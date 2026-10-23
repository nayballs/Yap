# The Yap bar

The always-there pill above the taskbar: the `overlay` window's Rust side (regions, click-through, never focused, placement, fullscreen, cards), its Win32 maths, and its page. Which card shows when is [notices.md](./notices.md).

[Architecture index](./README.md) · [CLAUDE.md](../../CLAUDE.md)

## Modules

- **`bar.rs`** — the **Yap bar** (2026-10-05, after Wispr Flow's Flow Bar —
  `references/wispr-flow/README.md` (beside this repo), screenshots 20–22, 25): the
  `overlay` window as one fixed-size (520×560 logical), transparent,
  always-on-top tool window, drawn to Wispr's measured Flow Bar
  (`references/wispr-flow/flowbar-spec.md`). **Idle** a 40×8
  pill outline 14 px above the work area; **hover** opens it into 🎤 Dictate (`toggle_recording`, tooltip
  "Dictate · F9"), ◉ Meeting notes (`meeting_guard::start_or_stop`, as the
  meeting shortcut: the live call, else a new meeting note; while recording,
  `meeting_end::end`), 🔊 Read aloud (`read-aloud`: the selection in the app
  you're in, read aloud — the bar never takes the focus, so it's still
  selected there; ■ `read-stop` while reading; its tooltip names the Read
  aloud shortcut, `Status.readHotkey`), ✂ Snip to Chat (`snip`: drag a box
  over any screen, the picture into Chat — `snip.rs`, [snip.md](./snip.md); the
  bar never took the focus, so a cancel gives it back to the app you were in;
  its tooltip names the snip shortcut, `Status.snipHotkey`) and a ^ menu (Open Yap, New meeting note, Settings,
  Hide the bar for 1 hour, Turn off the bar — `bar_action`); **dictating** =
  the dictation overlay (waveform, live partials, Transcribing…, errors), with
  ✕ (`dictation-cancel`) and ✓ (`dictation-stop`: stop and paste, as the
  hotkey's second press — into the app the dictation started in) while it
  records, as `bar_dictation_buttons` says (`Status.dictationButtons`):
  **"off"** (the default, the maintainer's call: Cancel stays in the tray), "hover"
  (only while Rust's pointer is on the capsule, which is then a region) or
  "always"; never while the push-to-talk key is held (`yap-dictation-held`,
  from the press until it locks hands-free), as Wispr's bar draws them only
  hands-free; and after a cancel "Dictation
  cancelled · Undo" (`yap-state` "cancelled", `dictation-undo`, whatever the
  setting) while the audio is kept; and after a dictation whose words AI
  cleanup changed, "Cleaned up · Use what I said" (`yap-state` "cleaned",
  `Dictation::Cleaned`, for `as_said::OFFER_MS`; `bar_action "as-said"`:
  the paste swapped for the words as said, `as_said.rs`) — a region of its
  own like Undo, shown with the bar off too;
  **meeting** = a 69×30 recording pill at half opacity (5 bars, the stop
  circle; hovered, full opacity and the timer; body → `notepad::open`, ■ →
  `meeting_end::end`). **Click-through except over what
  it draws**: the page reports its interactive rects (`data-region` →
  `bar_regions`, CSS px) and a poller (`GetCursorPos` every 250 ms, 30 ms
  while the cursor is near) hit-tests them (`overlay::region_at`, per-monitor
  scale) and clears `WS_EX_TRANSPARENT` only while the cursor is on one,
  telling the page (`yap-bar-pointer`; a click-through window gets no
  mouseleave, so Rust is the authority on hover) — Electron's
  `setIgnoreMouseEvents(true, {forward:true})` done natively, as Wispr's
  traced bar does. **Never focused**: `focusable: false` (WS_EX_NOACTIVATE),
  a window subclass that answers `WM_MOUSEACTIVATE` with MA_NOACTIVATE and
  keeps TOOLWINDOW/NOACTIVATE/LAYERED through tao's style rewrites
  (`WM_STYLECHANGING`), and if Windows activates it anyway (`WM_ACTIVATE`)
  the focus goes back to the app you were in — so 🎤 dictates there.
  **Follows the cursor's monitor** (not the foreground window) after 200 ms
  on it, bottom-centre on the work area (`overlay::place`: taskbars on any
  edge, an auto-hidden one kept clear, per-monitor DPI with a re-place after
  a DPI-changing move, unplugged monitors), frozen while a dictation records
  or transcribes. **Fullscreen** (`bar_hide_fullscreen`, on;
  `overlay::fullscreen_on` = the foreground window covering the whole monitor
  and not maximised/shell/Yap's own, or `SHQueryUserNotificationState`
  D3D-exclusive / presentation mode): the idle pill hides; a card still shows
  over a borderless app, but waits (queued, unseen) out an exclusive one; it
  always shows while recording. **Cards** (`show_card`/`update_card`/
  `dismiss_card`, `Card` + an `OnAction` callback; answers come back through
  `bar_card_action`): Yap's notices while the main window isn't focused,
  instead of a Windows notification — `false` when the bar is off or hidden
  for an hour, and the caller posts the notification as before. **One card
  at a time** (`notice.rs`): every card asked for goes into `cards` with
  when it came (`queued`); `repick` (on every change, dictation state,
  meeting state and poller tick) puts the one `notice::pick` chooses on
  screen (`front`: a question before an alert before a notice before a tip,
  then the one up, then the oldest; a card the pointer is on stays), holds
  tips and notices while a dictation runs, drops ones that waited too long
  unseen (their `expire_action` reported) and the same notice again within
  its cooldown (`show_card` still returns `true`: it was said). The page
  gets only the front card (`bar_status.cards`, the rest in `waiting`).
  Users:
  `meeting_detect` (call prompts as Wispr's "Meeting detected" card, "Taking
  notes…", failures), `meeting_guard` (length warning with Keep going,
  the quiet countdown and the check-ins, notices), `updates` ("Yap X is ready"), `calendar` (the reminder before a
  meeting, as a call card), `coaching` (push-to-talk tips), `learn` (a word
  to add to the dictionary), `alert` (a paste into an admin app, Windows'
  microphone privacy), `assistant` (the Voice Assistant's answer, style
  "answer"; Esc stops its voice). The bar also follows `yap-assistant`
  (`State.assistant`, in `Status.assistant`): the window shows while a
  question is asked, answered or read aloud, bar on or off, and ■ on the
  capsule is `bar_action "assistant-stop"`. **The reading card**
  (`read_aloud.rs`, `set_reading`, `State.reading` / `Status.reading`):
  while Yap reads text aloud (or the card has a note: "Nothing selected")
  the window shows, bar on or off, hidden for an hour or over a fullscreen
  app (`shows`), takes clicks on the card (`clickable`), the poller watches
  (`State::watching`), and Esc stops the reading (`escape`, before any
  card's own Esc). An Esc watch (a low-level keyboard
  hook that only looks) runs only while an on-screen card offers Esc (the
  auto-start countdown, the assistant's answer) or the reading card is up. Debug-only `bar_simulate` (pretend cursor on a
  region, fake fullscreen kinds, Esc, demo cards) and `bar_debug` (the real
  ex-styles, WindowFromPoint on the pill, the thread's active window) for the
  e2e suite; a test run's bar only sees `bar_simulate`'s cursor unless it's
  started with `YAP_E2E_REAL_CURSOR=1` (debug-only, `e2e::real_cursor`: the
  real one, for checking a real click reaches it — never while the maintainer's
  using the mouse). Every card answer (button, ✕, Esc, its timer) is logged
  as "bar: card answered" with the card and action; the hover check logs at
  debug when it changes. Settings → General → Yap bar (`bar_enabled`, `bar_hide_fullscreen`, `bar_theme` Match app / Light / Dark;
  `overlay_position` puts it at the top) and Settings → Dictation → While you dictate (`bar_dictation_buttons`,
  the live preview). Bar off = the old behaviour: the
  overlay only while dictating, notices as Windows notifications — but a
  showing dictation's ✕ / ✓ and Undo still take clicks (`clickable`).
- **`overlay.rs`** — the bar window's Win32 side + its pure, unit-tested maths:
  `place` (work-area placement), `fullscreen_on` (None / Borderless /
  Exclusive), `region_at` (hit-testing with slack), `enforced_ex_style`, and
  `win::` (subclass install, click-through toggle, monitors via
  `MonitorFromPoint`/`GetMonitorInfoW`/`GetDpiForMonitor`, auto-hidden taskbars
  via `ABM_GETAUTOHIDEBAREX`, the foreground window, `force_topmost`).

## The bar's page

- **`lib/Overlay.svelte`** — the **Yap bar** page (window `overlay`, `bar.rs`),
  laid out to Wispr's measured Flow Bar (`flowbar-spec.md`, checked by
  `bar.spec.js`) but in **Yap's own colours** (2026-10-06, the maintainer): every
  colour is a `--bar-*` token in app.css, in two schemes picked in Settings →
  General → Yap bar → **Colours** (`bar_theme`: **Match app** — the default
  since 2026-10-22, the same as Yap's theme, System / Light / Dark — or Light
  or Dark as an override; carried to the page in `bar::Status.theme` ("app" |
  "light" | "dark"), which `lib/appearance.js` `setBarChoice` resolves into
  `data-bar-theme`, following the app's theme live — Windows switching app
  mode included; the main window follows it too, for the Connect calendar
  dialog's card picture). The bar window stays transparent with no
  `color-scheme` whatever the theme (the grey box). **Light**:
  Yap's window — white cards with the warm `#dcd7cb` edge, ink text, ink
  primary buttons, amber chips, burnt-orange waveforms. **Dark**: the in-app
  toasts' palette — warm near-black `#1c1a16`, sand chips, cream buttons, Yap
  amber (the app's dark theme starts from it). Both keep Yap's recording red:
  one stage anchored to the bottom (or top) edge — the pill's place below,
  states sharing one grid cell so they cross-fade, each in a hit wrapper
  (6 × 10 px of nearly invisible padding, reported as region "pill") whose
  bottom is 8 px up, so every pill sits **14 px above the work area**. The
  **idle pill** is 40×8 (light: white at 85 % with a 1 px ink border at
  30 %; dark: warm ink at 55 %, cream border), radius 6, no shadow, in a
  60×20 wrapper; it morphs into the open
  pill (not measured yet: Yap's own 196×38 — 🎤, ◉, 🔊, ✂ and ^); tooltips (ink in both schemes,
  radius 8, 12 / 600: "Dictate F9", "New note Win + Alt + M") and the ^ menu
  sit above it. The **meeting pill**: 69×30, the bar surface, a 2 px ring in Yap's
  recording red (`#e5645e`; Wispr's is emerald), radius 22.5, half opacity
  until hovered; 5 2×18 bars in the scheme's amber (Wispr's staggered timing), then the 19 px
  stop circle with its 8×8 square; hovered, it widens to show the timer.
  `bar/DictationCapsule.svelte` (the dictation overlay — waveform in Yap
  amber, live partials word-paced by `bar/dictation.svelte.js`, which listens
  for the page's whole life; a question to the **Voice Assistant**
  (`dictation.assistant`, `data-ask`) is its own variant: an amber edge and
  a ✦ for the dot, "Ask Yap…" + waveform (or the words) while it listens,
  "Thinking…", then three bars + "Speaking…", with a ■ (`assistant-stop`,
  region "assistant") while it thinks or speaks, and while the pause after
  the question runs out ("Stop listening when I pause", `pause.rs`:
  `yap-assistant-pause {leftMs}` → `dictation.pauseMs`) an amber hairline
  running down at its foot (`.pauseline`, kept with Motion reduced); a small amber lock after the dot while push-to-
  talk's double-tap holds it hands-free; the dead-mic watchdog's "No sound
  from <device> — is it muted?" — or "Windows is blocking the microphone" — in
  the waveform's place, with a red edge; while it records a quiet ✕ on the
  left and the primary ✓ on the right, 26 px circles — `buttons` "off" (the
  default) never, "hover" only while `hovered` (Overlay's `pointer ===
  'dictation'`), "always" always, and none while `dictation.held` (the
  push-to-talk key down, not locked); where they may show, their places in
  the capsule's ends are kept (39 px padding) and they only fade (and
  settle from 0.82×) in and out there, `visibility: hidden` at rest, so
  pointing never changes the width or moves the words; cancelled,
  "Dictation cancelled" + Undo with a hairline running down the kept time;
  cleaned, "Cleaned up" + **Use what I said** the same way (`offerMs`).
  It's a `data-region` ("dictation") whenever it has a button to offer, so
  the click-through bar takes clicks on it, never focused). **Cards** sit 26 px above the pill whatever it
  shows (positioned from the dock's height, gliding 0.16 s when it grows),
  one at a time (`notice.rs` picks it; the rest wait in `bar.waiting`); it
  fades in and out in 0.12 s and has Wispr's 4 px hit strip on the pill's
  side. **The reading card** (`bar/ReadingCard.svelte` + `bar/reading.svelte.js`,
  `yap-read` / `yap-read-word` / `read_aloud_state`) sits nearest the pill
  whenever Yap reads aloud, a notice card above it: 440 wide, the bar
  surface, radius 16; a chip ("Reading aloud" / "Paused", its tooltip how
  the word timing was found), "3 of 42", ✕ (Stop); the sentence in the
  **reading font and size** from Settings → General → Appearance
  (`--yap-font-reading`, OpenDyslexic included; 17 px × `--yap-reading-scale`,
  line height 1.65 × `--yap-reading-leading`, a little word spacing), up to
  five lines before it scrolls, the word being said kept in view; **the
  word being said** an amber mark (`--bar-chip`, drawn with a box-shadow so
  the words never move); ⏮ / ⏸▶ (the ink circle) / ⏭, the speed (− 1.25× +,
  0.5–2×), Stop (a red square); a hairline of how far through the text, in
  amber. The whole card is region "reading" (+ its hit strip). A note
  (`phase` "note": "Nothing selected", "Not now" during a meeting or call,
  "Couldn't read it aloud") is a 400-wide card with the chip, title, body
  (`{key}` = the shortcut, `fillHotkey`) and ✕ (`dismiss`), gone after
  4.5 s. Light and Dark from the `--bar-*` tokens. `e2e/read-aloud.spec.js`.
  `bar/BarCard.svelte` (notices: update, guard rails, "Taking notes", the
  call ending, errors) is Wispr's card in Yap's colours: 400 wide,
  the bar surface with a 1 px edge, radius 16, padding 20; a chip naming its
  kind (Update, Meeting, Call, Tip, Error; icon `tip` = a bulb, the coaching
  hints, whose `{key}` the card names from its `hotkey`), the title 15 / 600, the body 15
  muted, a ghost button and a primary one (ink on light, cream on dark) at
  radius 8, the ✕ 24×24 at 30 % top-right; a `countdown` line with a ring
  ("Esc to cancel" only when the card has an `escapeAction`, its `done`
  text at zero), or as the card's **headline** (`Countdown.headline`: the
  quiet-meeting countdown, "Ending notes in 28 s…" at 20 / 650 beside the
  seconds in a 40 px ring, above the title — readable at a glance).
  `bar/CallCard.svelte` (the call
  prompt as Wispr's "Meeting detected" card, on the same tokens but one row
  with slimmer padding: the app's icon on a light tile, drawn by the shared
  `lib/CallAppIcon.svelte` — its own as installed here (`app_icons.rs`,
  loaded per window by `callAppIcons.svelte.js` when first drawn, again after
  10 min), else its mark from `bar/callApps.js` (Simple Icons CC0 glyphs),
  else a letter badge in its colour (Teams/Slack/Webex/Whereby), else a
  phone; `data-icon` names which —
  "Teams call" over "● Now", a cream split button [Yap] Record notes with the
  other answers in its ^ menu, a corner ✕, a 30 s fade hairline, the
  countdown ring; also the calendar's reminder, "Design review" over "● In
  1 min · with Tanay +1": a calendar glyph when there's no call app, an amber
  "soon" dot, a long line ending in "…", and a two-line answer "Join / &
  take notes", as on Wispr's pre-meeting card, and — with the meeting's
  pre-meeting brief made (`Card.brief`, [calendar.md](./calendar.md#pre-meeting-briefs)) —
  one slim line under the row, "Brief · Last met Thu 16 Oct · 3 open
  items ⌄", that unfolds `BriefView` inside the card (≤ 260 px, scrolling;
  its pills open notes in the main window, where the card then follows);
  the card's row is now a `.row` inside a column, unchanged without a
  brief; `preview` draws it as a
  static, inert, aria-hidden picture with no regions, for the connect
  dialog's band); shared fade and countdown timers in
  `bar/cardTimers.svelte.js`. Every `data-region` element is
  reported to Rust on layout changes. ⚠ The window is transparent: **no
  `backdrop-filter`**, no shadow on the pills and only a 1–2 px one on cards
  (one reaching a transparent WebView2 window's edge draws a grey box).
