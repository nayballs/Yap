# Frontend: the main window

The Svelte side's look (theme, window chrome) and the main window: the ControlPanel with its toast system, Home, Insights, Upload, the speech model browser, onboarding and the UI primitives. Other views live with their feature: Notes ([notes.md](./notes.md)), Chat ([chat-and-assistant.md](./chat-and-assistant.md)), Meetings and the notepad ([meetings.md](./meetings.md)), Dictionary ([dictionary.md](./dictionary.md)), Integrations ([integrations.md](./integrations.md)), the Yap bar ([yap-bar.md](./yap-bar.md)); Settings is [frontend-settings.md](./frontend-settings.md).

[Architecture index](./README.md) · [CLAUDE.md](../../CLAUDE.md)

## Look and the main window

- **Theme (2026-07-09)** — the main window is **warm-light, Wispr-Flow-inspired**:
  every token lives in `src/app.css` (`--yap-*`: paper surfaces w/ white cards,
  warm ink text, ONE amber accent reserved for keycap chips / active states /
  toggles, **ink** filled buttons via `--yap-ink*`, serif display numerals via
  `--yap-font-display`, light shadow tokens). Typefaces = **the exact pair Wispr Flow
  ships** (identified from its install at
  `%LOCALAPPDATA%\WisprFlow\app-*\resources\assets\fonts`; both OFL/Google
  Fonts, bundled via `@fontsource-variable/*` imports in main.js): **Figtree**
  = UI sans (Segoe UI fallback), **EB Garamond** (+italic) =
  `--yap-font-display` (hero headlines, stat numerals, Settings page titles).
  The meeting notepad's layout tokens, **`--yap-paper-*`** (paper, ink,
  text-2, muted, faint, date, rule, s1, s2, s2-strong, ring, danger, snag,
  band — the connect dialog's sand) + `--yap-live`/`--yap-live-strong`,
  `--yap-ease-spring` and the popover/dialog/sheet shadows: Wispr's measured
  roles (2026-10-05), set to **Yap's own palette** since 2026-10-06 (warm
  paper and ink, an amber focus ring, burnt-orange "live", recording-red
  Stop/Resume); the Meetings view's grey card, Take notes pill and the
  "Connect your calendar" dialog use them too. **Settings attention badge** (`lib/attention.svelte.js`):
  Settings computes real needs-action items (update available / no STT model /
  cleanup on a cloud provider with no key) into a shared runes store; the
  ControlPanel cog + the matching Settings nav rows show a red count chip
  (Wispr's Settings "1" pattern). **Onboarding is warm-light too** (2026-07-09
  redesign: same tokens as the main window, serif display headlines, ink CTA,
  brand icons on the model cards via `providerIcons.js`); the
  `:root[data-yap-theme='dark']` token block in app.css is currently unset
  everywhere — kept only as the seed of a future dark mode. App.svelte sets
  per-window `color-scheme`. **Brand mark** (2026-07-09):
  `src/assets/yap-logo.svg` — warm-ink rounded badge, amber yapping mouth,
  cream soundwaves (replaced the old blue `yap-icon.png` in the title bar +
  onboarding; `src-tauri/icons/*` regenerated from it via `npx tauri icon`).
- **Reading text and Motion (2026-10-22, Settings → General → Appearance;
  after Claude desktop's own Appearance settings, in free fonts)** —
  `lib/appearance.js` marks `<html>` in **every** window (App.svelte calls
  `initAppearance()`: `get_config` now, again on every `yap-config-saved`,
  and when `prefers-reduced-motion` changes; Settings also applies its own
  copy at once, before the 400 ms auto-save) with `data-reading-font`,
  `data-reading-size` and `data-motion`, and app.css turns them into
  tokens: **`--yap-font-reading`** (Default = Figtree, Sans = **Geist**,
  Serif = **Literata**, System = Segoe UI Variable / Segoe UI,
  **OpenDyslexic**), **`--yap-reading-step`** (Text size: Small 0.92 /
  Default 1 / Large 1.12 / Larger 1.25), **`--yap-reading-fit`** (each
  font's own size evened out by its measured x-height: Geist 0.95,
  OpenDyslexic 0.9, the rest 1), `--yap-reading-scale` = step × fit and
  **`--yap-reading-leading`** (OpenDyslexic 1.08). Only the **reading
  text** uses them, each view multiplying its own base size and line height
  (`font-size: calc(12.5px * var(--yap-reading-scale))`): Chat's messages
  and answers (`.cbubble`, tables included), Notes' content (`textarea.raw`),
  its Enhanced view (`.rendered`), its transcript lines (`.bubble p`, not
  the speaker labels) and per-note Ask thread, the meeting notepad's My
  thoughts, Transcript bubbles, Summary (its display-serif section headings
  keep EB Garamond, scaled by the step) and "AI notes so far", and the Ask
  sheet's questions and answers (`.mq`, `.mabody`). Never the sidebar,
  buttons, menus, chips or titles. Code stays monospace, scaled by the step
  alone (`pre.md-code code` in app.css, the views' inline `code`). **Fonts**
  are bundled, never fetched (`main.js`: `@fontsource-variable/geist`,
  `@fontsource-variable/literata` `wght` + `wght-italic`; OpenDyslexic's
  regular, bold and italic woff2 from `@fontsource/opendyslexic` as
  `@font-face` rules in app.css, skipping the package's duplicate .woff
  files); `font-display: swap`, and WebView2 loads a font's files only once
  some text uses it (and only the scripts' subsets it shows). All are
  **SIL OFL 1.1**: Geist © The Geist Project Authors (Vercel), Literata ©
  The Literata Project Authors (TypeTogether for Google), OpenDyslexic ©
  Abbie Gonzalez (Reserved Font Name "OpenDyslexic"; used unmodified);
  each package's LICENSE ships in `node_modules`, and the woff2 files carry
  their copyright and licence in their name tables (no separate
  third-party notices file in the repo; Figtree and EB Garamond are
  credited the same way). Literata was picked over **Source Serif 4** and
  **Newsreader** (all OFL) from a side-by-side at 12.5–18 px: the most
  readable at body size (x-height 0.51 em, sturdy serifs, a distinct I / l
  / 1), where Newsreader (0.44 em) thins out. The picker (`ui/Select`'s
  per-option `style`) draws each name in its own font, the closed box too,
  and a preview line under it shows the reading text as it'll look.
  **Motion**: Reduced (or System while Windows' "Animation effects" is
  off) stops every transition and animation with one `!important` rule in
  app.css (`:root[data-motion='reduced'] *`), except what says something is
  under way or how long is left: `.spin`, `.spinner`, `.progress`,
  `.drain`, `.run-down`, `.undoline > span` and anything marked
  `data-keep-motion`. The Yap bar's Svelte transitions (`fade`, `scale`,
  `flip`, which run as Web Animations that CSS can't stop) take
  `motionMs(ms)` = 0 while reduced. `e2e/appearance.spec.js`.
- **Custom window chrome (2026-07-09)** — the settings window is **undecorated**
  (`decorations: false` in tauri.conf.json): ControlPanel draws a 40px
  `data-tauri-drag-region` title bar (brand left; min / max-restore / close
  right, Win11-style hover incl. red close) over the warm frame, Wispr-style.
  Buttons use `getCurrentWindow()` (capability grants: minimize,
  toggle-maximize, is-maximized, close in `capabilities/default.json`);
  close still routes through hide-on-close; double-click on the drag region
  toggles maximize (Tauri built-in). Trade-off: the native snap-layout flyout
  on hover-over-maximize is gone (Win+arrows still snap).
- **`lib/ControlPanel.svelte`** — the **main window** (window label is still
  `settings`, historic): an OpenWhispr-style control panel — slim sidebar
  (**Home / Insights / Chat / Notes / Meetings / Upload / Dictionary / Integrations**) + **Settings as a modal
  overlay** (cogwheel). `Settings.svelte` renders `embedded` inside the modal
  and stays **always mounted** so its in-window hotkey fallback + auto-save run
  for the window's lifetime. App-wide **toast notification system**
  (`ui/toast.svelte.js` + `ui/ToastHost.svelte`, OpenWhispr timer logic in
  **Wispr-Flow card styling** since 2026-07-09): dark rounded card with a
  per-variant category chip (Tip/Done/Error, override via `chip`; `icon:
  'update'`), always-visible circular ✕, optional light **action button**
  bottom-right (`action: { label, onClick, keepOpen? }` — Wispr's "Open
  Settings") + a quiet `secondary` ("Later") + a small `tertiary` text link on
  its own line under them ("Don't ask for Teams"), hover-pause, `onClose` (✕) /
  `onExpire` (timer ran out, or waited too long) callbacks, copyable mono
  error boxes, progress hairlines (`duration <= 0` sticky), plus a
  determinate `progress` bar, a `busy` chip spinner and an `expand` "What's
  new" toggle that unfolds markdown in the card; `updateToast(id, patch)`
  changes a toast in place (on screen or waiting); `isToastLive` = not
  closed. It runs **Yap's notice policy** (`ui/noticePolicy.js`, the page's
  half of `notice.rs`): every toast has a `kind` (prompt | offer | alert |
  update | info | warning | tip — what Yap says by itself — or error | done —
  the answer to a click; by default from the variant) and two lanes, **one
  notice + one answer on screen**, the rest queued (higher priority first,
  then oldest; a question pushes a tip back unless hovered; an `offer` —
  "Started by mistake?" — or the update gives way to its equals and comes
  back); tips/notices/updates held while a dictation records (`yap-state`,
  + 1.5 s), the update also while a meeting records; a tip waits ≤ 30 s and
  a notice ≤ 60 s unseen, then is dropped (`onExpire`); the same toast again
  (kind + text, or `key`) refreshes the one up instead of stacking; a
  `warning` doesn't repeat within 10 min. Default times: done 3 s, info 6 s,
  error 6 s, alert / tip 10 s, warning 12 s, prompts until answered. Success
  toasts only where nothing else shows it (copy from a menu, a file saved,
  a note moved, "Added to Claude" with its restart step, "Won't ask about
  Teams", "Updated to X", sign-in); a state the screen already shows gets
  none (recording / paused / ended, an action run, a switch flipped, a row
  gone, the transcript's copy ✓). Mounted in ControlPanel (and the notepad)
  and wired to action runs, uploads, clipboard copies, backend `yap-error`
  events (kind `alert`; a dictation's own errors are the overlay's only,
  `yap-dictation-error`), push-to-talk coaching (`yap-hint` → a 10 s Tip
  toast, kind `tip`, `{key}` named by `fillHotkey`), **word suggestions**
  (`wordSuggestions.js` — learn.rs's `yap-word-suggestion`: "Add
  “Kubernetes” to your dictionary? Yap heard “cube and eighties”." with Add /
  Not now / a "Don't suggest this" link, chip Dictionary, `icon:
  'dictionary'`, 30 s; ✕ = Not now; after Add "“Kubernetes” is in your
  dictionary" + Open Dictionary), **alerts** (`alerts.js` — alert.rs's
  `yap-alert`, kind `alert` (a test's `level` may name another): "Your text
  is copied: press Ctrl+V" with chip Paste, `icon: 'clipboard'`; "Windows is
  blocking microphone access for desktop apps" + Open settings, chip
  Microphone, `icon: 'mic'`; "Stop asking about calls?" + Stop asking, chip
  Calls), and the **update toast**
  (`updates.svelte.js` — the shared update store started by ControlPanel:
  sticky "Yap X is ready" + Restart to update / Later / What's new, then
  Downloading… / Restarting Yap… in place; "Updated to X" after the
  restart), and the **call prompts** (`meetingDetect.svelte.js` — mirrors
  meeting_detect.rs's snapshot: "Teams call detected — Record notes / Not now"
  with a small **"Don't ask for Teams"** link on its own line under the
  buttons (the toast's `tertiary`), fading after the prompt's `fadeMs` (30 s)
  as "Not now" (the toast's `onExpire`), and a sticky "…call ended — Stop and
  summarise / Keep recording", both with `icon: 'call'` and withdrawn when
  Rust withdraws the prompt; the card's ✕ runs its `onClose` = the quiet
  answer; `yap-meeting-detect-choice` with `confirm` shows "Won't ask about
  Teams calls" + Open Settings (→ `general#meetings`);
  `yap-meeting-open-note` switches to Notes and hands NotesView a
  `noteRequest` to open, and stop if asked), the **meeting guard rails**
  (`meetingGuard.js` — meeting_guard.rs / capture.rs: the sticky "Notes stop
  in 5 minutes" + **Keep going** toast (`icon: 'timer'`, counting down,
  only when `inApp`, withdrawn when Rust sends `{warning: null}`), and one-off
  `yap-meeting-notice` toasts: the screen-share tip (`icon: 'screen'`,
  **Update settings** → `general#screen-sharing`), "Stopped at 2 hours", "Teams
  call ended · Yap stopped recording…", "Taking notes" (a notice addressed
  to the notepad, `surface`, isn't shown here), and meeting.rs's warnings
  (kind `warning`, 12 s: "Yap can't hear the call", "Transcription is
  falling behind") in whichever view is open; and, through `meetingCheckIn.js`
  (shared with the notepad, `initMeetingCheckIns(label)`), the quiet
  countdown "Ending notes in 28 s…" — Keep going / Stop now, a draining
  bar, its ✕ only hiding it — and "Still in your meeting?" — Stop and
  summarise / Keep recording, ✕ = keep — when `yap-meeting-checkin`'s
  `surface` names this window), and **"Started by
  mistake?"** (`meetingSummary.svelte.js` — the shared store of the Rust
  action-plan jobs, `summaries.byNote` / `.progress`, also used by NotesView
  and the notepad; `askStartedByMistake` = a sticky "Only a few words were
  captured. Keep this meeting or discard it." with Discard / Keep, `icon:
  'alert'`, kind `offer`, shown by the window `yap-meeting-ended`'s `surface`
  names), and the
  **calendar** (`calendar.svelte.js` — the shared store of calendar.rs's
  snapshot, started by ControlPanel: the reminder card as a sticky toast
  while Rust says `inApp` (else it's on the bar),
  chip Meeting / Next meeting, `icon: 'calendar'`, "In 1 min" counting,
  Join & take notes / Start notes / Snooze 2 min, ✕ or Esc = dismiss,
  withdrawn when Rust withdraws it; "Calendar connected", sign-in errors
  (in the connect dialog instead while it's open); the one-time "Connect
  your calendar" nudge after a real meeting ends (`yap-meeting-ended`, not a
  mistake) with Connect calendar (the dialog, over the Meetings view) / Not
  now; `connectDialog` + `openConnectDialog`/`closeConnectDialog`).

## Views

- **`HomeView.svelte`** = the Wispr-style
  Home: time-of-day greeting with the hotkey as **amber keycaps**, a dark
  **rotating hero card** (5 tips — Commands (voice edit) / AI cleanup / meeting notes /
  per-app profiles / the Voice Assistant — picked by day, dot nav, CTAs open the right Settings
  section or view via `onnavigate`), the dictation feed (day-grouped flat rows
  w/ hover-revealed meta + copy/delete via `delete_history_entry`, live refresh
  on `yap-history-changed`, icon-expand search on Ctrl+K), and a right-rail stats
  card with serif display numerals.
- **`InsightsView.svelte`** = the stats
  dashboard promoted out of Settings→History (serif hero number, stat grid,
  30-day amber heatmap, top-apps-by-words bars from `get_history`); Settings →
  History keeps only the enable toggle + recent list + clear.
- **`UploadView.svelte`** = local audio-**file** transcription (drop/browse →
  Symphonia decode → chunked transcription on the warm engine with progress +
  cancel — see `media.rs`);

## Shared components

- **`lib/ModelManager.svelte` / `ModelRow.svelte` / `models.js`** — the 14-model
  browser, OpenWhispr-style: vendor pill tabs (All/NVIDIA/OpenAI/Community via
  `ui/PillTabs.svelte`) + compact one-line rows (status dot, brand icon from
  `providerIcons.js` + `assets/providers/*.svg` (MIT, from OpenWhispr), name,
  size, Download/Active/delete; delete asks first in `ui/ConfirmDialog.svelte` —
  ⚠ never `window.confirm/alert/prompt` in a Yap window: Tauri routes them to the
  dialog plugin, which these windows aren't allowed, so they throw).
  `ModelCard.svelte` (big cards) remains only in
  Onboarding. `ModelRow` also draws the speaker models (`SpeakerModels.svelte`:
  `unavailable` = a listed model's note instead of Download, `deleteActive`).
- **`lib/Onboarding.svelte`** — first-run **guided setup** (5 steps): model picker →
  mic check (live level meter via `set_mic_test` idle-amp mode + live device switch)
  → one-click **local AI cleanup** install (the model recommended for this PC
  first and picked: Qwen3.5 4B with a graphics card that can run it, "Your
  <card> runs Qwen3.5 4B quickly…", else Qwen2.5 1.5B) → tray pointer → "try
  it here" live dictation test with a change-shortcut recorder.
- **`lib/ui/`** — primitives: Toggle, Select, Slider, Group, Row, Button, Input, Textarea,
  **ContextMenu** (at the pointer or under a ⋯ (`alignEnd`), clamped to the
  window; ↑/↓/Home/End/Enter, → into a submenu, ← / Esc back; Esc, Tab, a
  click outside, a resize or a scroll close it and the focus goes back to the
  row; `{ label, icon, onselect, disabled, danger, checked, toggle, items }`,
  separators and a quiet `note`; it cancels `contextmenu` on itself; a
  scroll in its first 250 ms — the right-clicked row scrolling into view —
  doesn't close it) and **ConfirmDialog** (the notepad's Wispr-style confirm
  for the main window: Cancel focused, Esc/scrim cancel, Tab kept inside,
  the focus returned). **Right-click is app-wide and the same everywhere**
  (`ui/contextMenu.svelte.js`): one menu + one confirm per window
  (`ui/ContextMenuHost.svelte`, mounted by App.svelte in the main window,
  onboarding and the notepad); `openContextMenu(e, items, {label, row})`
  from a right-click (at the pointer), a row's hover ⋯ (`data-more`: under
  it) or the keyboard (`menuKey`: the ContextMenu key, Shift+F10); nothing
  to offer → the event is left alone (WebView2's filter shows nothing); a
  right-click in a text field or on selected text is the native edit menu's;
  `askConfirm({title, body, run, returnFocus})` for every delete; Esc
  handlers elsewhere wait for them (`menuOrDialogOpen`). One order: the main
  action, Copy-type ones, the destructive one last in red behind the
  confirm; a right-click also selects the row where a click does (unless
  that loses unsaved work). Where: Notes' folders and notes (+ its Ask
  thread and transcript lines), Chat's conversations (Rename in place,
  Copy as Markdown, Delete…; the hover ⋯ replaced the trash) and messages
  (Copy / Copy as Markdown / Read aloud; a question: Copy — `itemMenus.js`),
  Home's dictations (Copy, Copy raw transcript, Delete…; the trash asks
  too), Meetings' past notes (Open, Generate summary, Copy as Markdown,
  Delete… via `meeting_delete`; a hover ⋯) and calendar meetings (Open note
  + their row buttons), Dictionary rows (Edit, ≈ Catch near-misses,
  Delete…; × asks too unless the row is empty), model rows (`ModelRow`:
  speech, speaker, Kokoro — Download / Use / Delete…) and the local LLM
  cards (Download / Use / Delete…, now asked first), Connectors' calendars
  (their ⋯ = this menu: Sync now, Disconnect… asked first), Account's other
  devices (Sign out), the Actions manager's rows (Edit, Delete…), the
  notepad's transcript lines (Copy line, Who said this?) and Ask answers
  (Copy, Copy as Markdown, Read aloud, Good / Bad answer) and questions
  (Copy). `e2e/right-click.spec.js`.
