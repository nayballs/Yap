# Frontend: the main window

The Svelte side's look (theme, window chrome) and the main window: the ControlPanel with its toast system, Home, Insights, Upload, the speech model browser, onboarding and the UI primitives. Other views live with their feature: Notes and Tasks ([notes.md](./notes.md)), Chat ([chat-and-assistant.md](./chat-and-assistant.md)), Meetings and the notepad ([meetings.md](./meetings.md)), Dictionary ([dictionary.md](./dictionary.md)), Integrations ([integrations.md](./integrations.md)), the Yap bar ([yap-bar.md](./yap-bar.md)); Settings is [frontend-settings.md](./frontend-settings.md).

[Architecture index](./README.md) · [CLAUDE.md](../../CLAUDE.md)

## The folder layout (2026-10-24)

`src/` keeps `main.js` (fonts, the markdown links, mounting), `App.svelte`
(one chunk per window, below) and `app.css` (every token). `src/lib/` is by
kind, a folder each:

| Folder | What's in it |
|---|---|
| `views/` | The main window: `ControlPanel.svelte` (sidebar, title bar, the Settings modal, toasts) and its views; `help/` the title bar's Help (`HelpMenu`, `ShortcutSheet`, `WhatsNew`) and `MeetingSetup.svelte` the sidebar's "Set up meeting notes". A view with parts of its own has a folder: `home/` (`HomeView`, `homeData.js`), `chat/`, `notes/`, `integrations/` (`IntegrationsView`, `CalendarLinkCard`, `McpLinkCard`), `dictionary/` (`DictionaryView`, `ImportPreview`, `dictionaryList.js`, `dictionaryFile.js`), `insights/` (`InsightsView`, `AiUsageCard`; 2026-10-25); the rest are one file (`UploadView`, `MeetingsView`, `TasksView`) |
| `settings/` | The Settings modal: `Settings.svelte`, its pages and every part only Settings uses (the module map: [frontend-settings.md](./frontend-settings.md#the-module-map-split-2026-10-24)) |
| `notepad/` | The meeting notepad window, and its split-screen preview (`SplitPreview.svelte`) |
| `quicknote/` | The quick-note window (2026-10-28, `notes/quick_note.rs`): `QuickNote.svelte` (the note, saved as typed one save at a time; New note, Open in Notes, Esc; a dictation at the cursor; the in-page hotkeys) and `quickNote.js` (pure: where a dictation goes, the status line). Its icons are the notepad's `NotepadIcon` |
| `onboarding/` | The first-run window |
| `bar/` | The Yap bar window: `Overlay.svelte` (its page) and its cards |
| `components/` | What more than one window or view draws: `BriefView`, `SpeakerMenu`, `SpeakerCard`, `MeetingJob`, `FailedChunk`, `CallAppIcon`, `CalendarMark`, `LocalContext`, `ConnectCalendarDialog` (the main window's and the setup's calendar step), `MicQuiz` (the setup's mic step, Settings → Dictation), `TypingTest` (the setup's last step, Insights) |
| `stores/` | App-wide reactive stores (`*.svelte.js`): `account`, `attention`, `calendar`, `callAppIcons`, `chatRequest`, `meetingAudio`, `meetingDetect`, `meetingSummary`, `memory`, `modelCatalog`, `modelStore`, `speech`, `updates`, `upload` |
| `notices/` | Rust's events turned into toasts: `alerts`, `cleanupOffer`, `featureTips`, `meetingCheckIn`, `meetingGuard`, `saveErrors`, `wordSuggestions` |
| `core/` | Helpers and catalogues with no Svelte state of their own: `markdown`, `streaming`, `hotkeys`, `providers`, `providerIcons`, `ppModels`, `speechModels` (the speech-to-text catalogue; `models.js` until 2026-10-24), `modelCatalog`, `localContext`, `aiModel`, `aiErrors`, `aiUsage` (the AI usage tracker in words: token counts, costs, the context ring's colour and label, the features' names), `notepadText`, `noteEdits`, `itemMenus`, `wordDiff`, `appNames`, `appearance`, `brief`, `connections`, `externalLinks`, `ipcShared`, `findWords` (a search's words found again where its result opens: marked with the CSS Custom Highlight API, selected in a text box) |
| `ui/` | The primitives: rows, switches, selects, buttons, the right-click menu and confirm dialog, toasts |

A store, helper or component one area alone uses lives in that area's
folder (`views/chat/chatModels.svelte.js`, `settings/kokoro.svelte.js`,
`onboarding/ModelCard.svelte`); one more areas share goes to
`components/`, `stores/`, `notices/` or `core/`.

**The big views are a folder of parts.** The top component is the window
or the view: it makes the state, follows Rust's events while it's up and
lays out its parts. What the parts share lives in a `*.svelte.js` beside
them, made by the top component each time it's made (views come and go as
the sidebar switches) and handed to the parts: a function returning an
object of getters, setters and actions (`createNotesState`,
`createChatState`, `createSetupState`), or small classes where only a
slice is shared (the notepad's). Each part carries its own styles; what
several draw is in each of them, or — the notepad's tooltips and ink
button — in one shared sheet under the page's class (`notepad/notepad.css`
under `.pad`, as `settings/settings.css` under `.settings-shell`), every
selector weighing what its scoped original did, so the same rule wins.

| Folder | Parts |
|---|---|
| `notepad/` | `Notepad.svelte` — the window: the note shown and the meeting, kept in step with Rust's events and the other windows; the in-page hotkeys; the layout. `NotepadHeader` (back, ⋯, split screen, Share, window buttons), `NotepadTitle`, `NotepadTabs`, `ThoughtsTab`, `TranscriptTab` + `TranscriptLog` (the timer box, search and copy; the lines), `SummaryTab`, `GenerateSummary` (or the summary's error with Retry), `ConsentLine`, `AskTray` (Stop / Resume, the Ask sheet), `DeleteMeetingDialog`, `NotepadIcon`. What outlasts a tab switch (each tab's body is made again on every switch): `transcriptView.svelte.js`, `summaryEditor.svelte.js`, `consentMessage.svelte.js`; `actions.js` the window's label, copying, Yap's main window; `notepad.css` |
| `views/notes/` | `NotesView.svelte` (the view, its events, the note a call or Tasks asks to open) with `notesState.svelte.js` (the notes and folders, the open note saved as it's typed and kept in step, the recorder, actions, the summary edited in place, the transcript's view, the note's chat); `NoteList` (folders, notes, their menus) with `NoteSearch` (every note by words and meaning, loaded the first time a search is typed), `NoteEditor` (title, tabs, what's being written, the raw note) with `NoteMeta` (the chip row), `NoteSummary`, `NoteTranscript`, `NoteChat` and `ActionPicker`; `ActionManager`; `noteFormat.js` |
| `views/chat/` | `ChatView.svelte` (the view, its events and keys, the screenshot viewer) with `chatState.svelte.js` (conversations, sending and the streamed answer, the Voice Assistant's questions, Snip to Chat, spoken replies, picking several); `ConversationList` (+ `chatLibrary.svelte.js` and `ChatLibraryList`, loaded when shown: the sidebar's search and the archive), `ChatThread` (+ `ActivityLine`: what the turn is doing and its time) + `ChatMessage` (the bubble, its tool chips folded under it in `ChatSteps` with `chatSteps.js`, sources, ▶ and the model), `Composer` (screenshots, +, the box, ■ Stop, the model chip); `ModelChip`, `ChatModelPicker`, `ProviderMark`, `chatModels.svelte.js`, `chatImages.js`, `chatShots.svelte.js`; the document beside the chat (2026-10-30; two views 2026-10-31): `DocPanel.svelte` (its own chunk, loaded the first time a document opens: the title, Undo, Preview / Source, expand, ⋯ — Find, Copy as Markdown, Export as Markdown…, Open in Notes —, ✕; the page, or the coloured Source under its text box; find; the selection's "Ask about this" / "Change this…"; a landed change marked) with `chatDoc.svelte.js` (made by `chatState`: the document shown, the sections streaming in, the view, the Source box's merged saves, each document's scroll per view, the split's width), `docTools.js` (the panel's chunk: copy, export, a block's lines in the note), `core/markdownSource.js` (Source's colours: spans that add no character), the split and the room in `ChatView`, the pill and a question's quote in `ChatMessage`, the quote over the box in `Composer` (`chatState`'s `quote`, sent as `chat_send`'s `selection`) |
| `onboarding/` | `Onboarding.svelte` (the dots, the step shown, the footer, the in-page hotkey, the "Connect your calendar" dialog) with `setupState.svelte.js` (every step's state and this showing's steps, `steps` / `stepId`, the config this window changed, its raw event subscriptions, `listen` for its components); `StepHeader`, `ModelStep` (+ `ModelCard`), `LanguageStep`, `MicStep`, `CleanupStep`, `CalendarStep`, `TrayStep`, `TryStep`, `TypingStep`; `cleanupProviders.js` |

An answer's tool chips (2026-10-31, `ChatSteps.svelte`) fold into one
line under it ("Searched the web 3 times, read 2 pages ▸",
`chatSteps.js` `stepsSummary`) that opens to them, one a line with its
mark: a globe for the web, a calendar, a document mark for a document's
("Started "Build" (3 sections)", "Wrote 3 sections in …", the rich tools'
"Changed a table in …"; `chatSteps.js` `DOC_CHIP`), a shield for an
action refused after the web. While an answer comes, `ActivityLine.svelte`
at the thread's foot says what's under way and for how long
(chat-and-assistant.md, `chat/activity.rs`). Until then a tool's own chip
was a full-width row 30 px tall (`.toolchip.tool`) over the answer.

**Find** is one bar everywhere (`components/SummaryFind.svelte`: a
summary's Ctrl+F, and Chat's document panel with `label`): every match lit
with the CSS Custom Highlight API, found across the text's pieces (a match
spanning **bold**, or Source's colour spans), so the rendered HTML is never
changed. **`renderMarkdown(…, {lines: true})`** puts `data-line="a-b"` (the
block's source lines) on each top-level block (DOMPurify keeps only that
shape): the document panel maps a selection back to the markdown with it.

**The main window's chunk** is held near 720 kB (`vite.config.js`'
`chunkSizeWarningLimit`, 700 until Chat's documents on 2026-10-30 took it
to 708; 715 since the panel's two views, a selection's quote and find
across a text's pieces took it to 712 on 2026-10-31; 725 since Chat's
activity line, its folded steps and Sources row, and each model's effort
on the chip and in Settings took it to 722 the same day, the picker's
part in the picker's own chunk): the document panel
itself loads on demand (`{#await import('./DocPanel.svelte')}`, with its
own `docTools.js` and `core/markdownSource.js`), its state, the quote and
the pill don't.

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
  "Connect your calendar" dialog use them too. **What needs doing** (`lib/stores/attention.svelte.js`):
  Settings keeps real needs-action items live (`followAttention`) (update available / no STT model /
  cleanup on a cloud provider with no key) into a shared runes store; they're
  counted in **one place**, the title bar's **bell** (its count, its name
  "Notifications (1)", and its list naming each item, a click opening the
  Settings page that fixes it); inside Settings the matching nav rows show
  which page. (Until 2026-10-22 the sidebar's Settings row repeated the
  bell's count: "1" and "1" for one item.) **Onboarding is warm-light too** (2026-07-09
  redesign: same tokens as the main window, serif display headlines, ink CTA,
  brand icons on the model cards via `providerIcons.js`). Since 2026-10-22
  there's a **warm dark theme** too (below): every token has a dark value in
  `:root[data-yap-theme='dark']`, and no component hard-codes a surface or
  text colour. **Brand mark** (2026-07-09):
  `src/assets/yap-logo.svg` — warm-ink rounded badge, amber yapping mouth,
  cream soundwaves (replaced the old blue `yap-icon.png` in the title bar +
  onboarding; `src-tauri/icons/*` regenerated from it via `npx tauri icon`).
- **The theme** — Settings → General → Appearance → **Theme**: System (the
  default: Windows' app mode, followed live) / Light / Dark; details in
  [The dark theme](#the-dark-theme-2026-10-22) below.
- **Reading text and Motion (2026-10-22, Settings → General → Appearance;
  after Claude desktop's own Appearance settings, in free fonts)** —
  `lib/core/appearance.js` marks `<html>` in **every** window (App.svelte calls
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
  its Summary tab (`.rendered`), its transcript lines (`.bubble p`, not
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
- **Help (2026-10-29, `lib/views/help/`)** — a "?" in the title bar
  beside the bell (`HelpMenu.svelte`, the bell's look and tooltip; a
  `menu` with ↑/↓/Home/End, Esc or a click outside closes it and the focus
  goes back to "?"): **Keyboard shortcuts** (also **Ctrl+/** anywhere in
  the main window, again to close: `ShortcutSheet.svelte`, loaded on first
  use — every global shortcut from `shortcut_sheet`, shell/help.rs, in the
  input hook's order, named here with what it does — Dictate's line
  following the recording mode — or by the hook's own name for one this
  page doesn't know yet, its keys as amber keycaps, more keys beside it,
  "Not set"; then the transforms' and the language shortcuts' keys and the
  keys inside Yap's window; "Change them in Settings" → Shortcuts; a
  dialog: Esc, ✕, the scrim), **Microphone help** (Settings → Dictation at
  the mic quiz, `dictation#mic-quiz`), **Languages** (`dictation#languages`),
  **Report a problem** (`about#report`), **What's new** (`WhatsNew.svelte`:
  the release notes Yap has here — an update's, `updates.notes` — rendered;
  without any, the release page on GitHub) and **GitHub**. `help.spec.js`.
- **"Set up meeting notes" (2026-10-29, `lib/views/MeetingSetup.svelte`)**
  — a small card in the sidebar above Settings (Wispr Flow's "Set up
  Notetaker", by behaviour): Connect a calendar (the dialog, over
  Meetings), Record your first meeting ("or try one now": Meetings, whose
  Take notes starts one), Add Yap to an AI app (Settings → AI apps (MCP)),
  each ticked by itself (`meeting_setup_status`, asked again as the view
  changes, a calendar connects, a meeting ends or the window comes back to
  the front) with a three-step bar; ✕ dismisses it for good; all done, it
  goes; under 700 px of window it's left out. `meeting-setup.spec.js`.
- **`lib/views/ControlPanel.svelte`** — the **main window** (window label is still
  `settings`, historic): an OpenWhispr-style control panel — slim sidebar
  (**Home / Insights / Chat / Notes / Meetings / Tasks / Upload / Dictionary / Integrations**) + **Settings as a modal
  overlay** (cogwheel). `Settings.svelte` renders `embedded` inside the modal
  and stays **always mounted** so its in-window hotkey fallback + auto-save run
  for the window's lifetime (the shell and its modules:
  [frontend-settings.md](./frontend-settings.md#the-module-map-split-2026-10-24)). App-wide **toast notification system**
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
  half of `notices/notice.rs`): every toast has a `kind` (prompt | offer | alert |
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
  (`wordSuggestions.js` — dictionary/learn.rs's `yap-word-suggestion`: "Add
  “Kubernetes” to your dictionary? Yap heard “cube and eighties”." with Add /
  Not now / a "Don't suggest this" link, chip Dictionary, `icon:
  'dictionary'`, 30 s; ✕ = Not now; after Add "“Kubernetes” is in your
  dictionary" + Open Dictionary), **snippet suggestions**
  (`snippetSuggestions.js` — dictionary/snippet_offer.rs's
  `yap-snippet-suggestion`: "Type that often? Save it as a snippet" with Save
  snippet / Not now / "Don't suggest this again", chip Dictionary, 30 s; ✕ =
  Not now; after Save "Snippet saved" + Open Dictionary), **alerts** (`alerts.js` — notices/alert.rs's
  `yap-alert`, kind `alert` (a test's `level` may name another): "Your text
  is copied: press Ctrl+V" with chip Paste, `icon: 'clipboard'`; "Windows is
  blocking microphone access for desktop apps" + Open settings, chip
  Microphone, `icon: 'mic'`; "Stop asking about calls?" + Stop asking, chip
  Calls; "Couldn't save your dictation history" + Try again, chip Saving,
  `icon: 'save'`, shell/saving.rs), **failed saves** (`saveErrors.js`, 2026-10-24:
  a command whose store couldn't be saved rejects with "Couldn't save your
  note: the disk is full." — the change itself happened, kept in memory —
  so the caller carries on as if it worked and `showSaveError(e)` shows one
  toast, kind `error`, key `save-failed`: "Couldn't save your note" / "The
  disk is full. Yap is keeping your changes and will try again with the
  next one." + **Try again** (`saves_retry`: "Saved", or "Still couldn't
  save…"); false for any other error, the caller's own. Used by the Notes
  view and the notepad (note edits, folders, delete), Actions, Chat (delete,
  rename), Home (a dictation's delete), Clear history and Memory; config.json's
  savers pass their own save as `retry` — the Dictionary view and Settings'
  auto-save, whose next save that works takes its toast down with
  `clearSaveError(e, { retry })`), and the **update toast**
  (`updates.svelte.js` — the shared update store started by ControlPanel:
  sticky "Yap X is ready" + Restart to update / Later / What's new, then
  Downloading… / Restarting Yap… in place; "Updated to X" after the
  restart), and the **call prompts** (`meetingDetect.svelte.js` — mirrors
  calls/meeting_detect.rs's snapshot: "Teams call detected — Record notes / Not now"
  with a small **"Don't ask for Teams"** link on its own line under the
  buttons (the toast's `tertiary`), fading after the prompt's `fadeMs` (30 s)
  as "Not now" (the toast's `onExpire`), and a sticky "…call ended — Stop and
  summarise / Keep recording", both with `icon: 'call'` and withdrawn when
  Rust withdraws the prompt; the card's ✕ runs its `onClose` = the quiet
  answer; `yap-meeting-detect-choice` with `confirm` shows "Won't ask about
  Teams calls" + Open Settings (→ `meetings#call-apps-list`);
  `yap-meeting-open-note` switches to Notes and hands NotesView a
  `noteRequest` to open, and stop if asked), the **meeting guard rails**
  (`meetingGuard.js` — calls/meeting_guard.rs / calls/capture.rs: the sticky "Notes stop
  in 5 minutes" + **Keep going** toast (`icon: 'timer'`, counting down,
  only when `inApp`, withdrawn when Rust sends `{warning: null}`), and one-off
  `yap-meeting-notice` toasts: the screen-share tip (`icon: 'screen'`,
  **Update settings** → `meetings#screen-sharing`), "Stopped at 2 hours", "Teams
  call ended · Yap stopped recording…", "Taking notes" (a notice addressed
  to the notepad, `surface`, isn't shown here), and meetings/meeting.rs's warnings
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

## The dark theme (2026-10-22)

Settings → General → Appearance → **Theme** (`config.theme`: "system" |
"light" | "dark", default system; after Claude desktop's Theme, and Wispr
Flow's dark mode of 19 Oct 2026), for every Yap window.

- **The palette** — Yap's own warm dark, started from the Yap bar's dark
  scheme, not a cold grey: the frame `--yap-bg` #161410, the content sheet
  `--yap-s1` #1c1a16 (the toasts' card), cards/panels/inputs `--yap-s2`
  #24211c, menus and hovers `--yap-s3` #2b2722, toggle tracks and chips
  `--yap-raised` #36312a (elevation goes up in lightness, the frame darkest);
  cream text `--yap-fg` #f2efe8 stepped by opacity, `--yap-muted` #a8a294;
  Yap amber brightened (`--yap-primary` #e2982a, links and the focus ring
  #f0b04a), keycaps a touch deeper (#e8a33f); filled buttons **cream**
  (`--yap-ink` #ece8df with `--yap-ink-fg` #1c1a16 on it, the bar's dark
  primary); warm hairlines (#3a352b / #2e2a23); danger #e5645e as text and
  `--yap-danger-fill` #c0443c under white text (5.1:1); softer shadows (less
  blur and opacity, a faint light ring on menus: dark surfaces show depth by
  lightness). The notepad's `--yap-paper-*` roles all have dark values
  (paper #1c1a16, white pills → `--yap-paper-card` #2a2621, "Them" in a
  lighter teal). New tokens that hold in both themes: `--yap-scrim`
  (behind Settings and the Actions manager), `--yap-paper-scrim` (behind the
  paper dialogs), `--yap-feature*` (Home's tip card: ink on light, a raised
  warm card with a hairline and a cream button on dark), `--yap-mark-tile`
  (a light tile under black or coloured brand marks), `--yap-danger-fill*`.
- **How it's applied** — `lib/core/appearance.js` (every window's App.svelte
  calls `initAppearance()`) resolves the choice (System via
  `matchMedia('(prefers-color-scheme: dark)')`, followed as Windows switches)
  and marks `<html data-yap-theme="light|dark">`; Settings applies its copy
  at once, the other windows on `yap-config-saved`. **No flash of light**: the choice is kept in the
  origin's localStorage (`yap.theme`, `yap.barTheme`, shared by every Yap
  window), which an inline head script in `index.html` reads before the
  first paint, naming the window too (`data-window`, from
  `__TAURI_INTERNALS__`); app.css gives the opaque windows — main,
  onboarding, notepad — their background and `color-scheme` from
  `[data-window]` (never the bar or the split preview: a transparent window
  with a `color-scheme` paints the grey box). Rust (`shell/theme.rs`) gives the
  same three windows' **webviews** the theme's background
  (`set_background_color`: what WebView2 shows before a page paints — a
  window shown after a while hidden under the low memory target, a resize),
  at startup before their cloaked first show, after every config save, and
  on `WindowEvent::ThemeChanged` (Windows' app mode, read from
  `AppsUseLightTheme`); a unit test keeps its colours equal to app.css's.
- **What follows it** — the main window (title bar and its min / max /
  close, every view, every Settings page, dialogs, toasts — lifted a step
  lighter with a hairline on the dark page —, menus, tooltips, the model
  picker), the meeting notepad, onboarding, and the Yap bar through **Bar
  colours → Match app** (its own Light / Dark stay as overrides; `--bar-*`).
  Brand marks stay legible: monochrome provider marks invert on dark
  (`.micon.mono`, `.picon.mono`, `.sicon.mono`, Settings → MCP's), the AI
  apps' logos sit on `--yap-mark-tile`, a near-black call-app letter badge
  gets an edge, Google/GitHub sign-in buttons turn to their dark versions.
  **Exceptions, on purpose**: the QR code stays dark on white; the toasts
  and the onboarding's mock taskbar are dark in both; the split preview's
  glass and the bar's recording dots are the same in both. Onboarding, the
  one window with Windows' own title bar, gets it dark or light to match
  (`shell/theme.rs`, DWM's immersive dark mode).
- **Contrast** (AA, `e2e/theme.spec.js` measures the lowest on each page):
  body text 15:1, Settings descriptions 6.8:1, nav labels 11:1 (main window
  14:1), nav group labels 7.2:1, links 9.1:1 (Chat's 6.7:1), keycaps 7.6:1,
  the tip card 7.4:1, the Home feed 4.8:1; switches: an off edge 4.9:1, on
  14:1; the focus ring 9.1:1; Insights' busiest heatmap cells 7.3:1, its
  bars 5.4:1 on their tracks, their labels 4.8:1.

## Views

- **`HomeView.svelte`** = the Wispr-style
  Home: time-of-day greeting with the hotkey as **amber keycaps**, a dark
  **rotating hero card** (5 tips — Commands (voice edit) / AI cleanup / meeting notes /
  per-app profiles / the Voice Assistant — picked by day, except that someone with no
  dictations yet starts on AI cleanup until they browse; dot nav, CTAs open the right Settings
  section or view via `onnavigate`), the dictation feed (day-grouped flat rows
  w/ hover-revealed meta + copy/delete via `delete_history_entry`, live refresh
  on `yap-history-changed`, icon-expand search on Ctrl+K), and a right-rail stats
  card with serif display numerals ("1 dictation" / "N dictations"). In a
  narrow window (a container query on `.wrap` ≤ 760 px: the 860 px window
  minimum, a laptop at 150 %) the stats go above the feed as one row of four
  and the tip card keeps its width; ≤ 560 px its button goes under its text.
  With no dictations yet and **no speech model** (Settings' attention item
  `models`), the empty feed says "One step left: a speech model" with
  **Download a speech model** (→ Settings → Speech-to-Text) instead of
  "Ready when you are" — the hotkey does nothing until there's one.
  App names are the ones people know (`lib/core/appNames.js` `appLabel`:
  "ms-teams.exe" → "Teams", "WINWORD.EXE" → "Word"; history keeps the
  process name) — Home's rows, Insights and Settings' per-app rules.
  A dictation whose audio is still kept
  (`dictation/recent_audio.rs`, a day) has **Play** (▶/■: `recent_audio_wav` as a blob)
  and **Run again** (↻: `history_rerun` on the model in use, or — with others
  installed — a menu of them; also in the row's right-click menu, with "Run
  again with ›"), answered by a "Transcribed again" toast with Copy. A failed
  one (`status`: `failed` — "Not transcribed · Yap closed before it was
  transcribed" — or `notPasted` — a "Didn't paste" flag and why) says so in the
  row, with a Run again button in view; a **cancelled** one (2026-10-25,
  `cancelled`: "Cancelled · not transcribed") has **Transcribe** (the same
  `history_rerun`, "Transcribed") and **Keep** / "Kept" (`aria-pressed`,
  `recent_audio_keep`). Any dictation with its audio has **Save audio…**
  (`recent_audio_save`, Windows' Save dialog) and **Keep its audio** /
  "Don't keep its audio" in its right-click menu. **What the AI changed** (2026-10-23):
  a dictation whose words AI cleanup changed (history's `said`, the words as
  said, beside its `text`) shows a quiet pill under its text, always in view
  — "✦ AI edited · N changes" (a `button`, `aria-expanded` /
  `aria-controls`) — that opens a panel (`role="region"`, "What AI cleanup
  changed") with the text as a word diff: words the AI took out struck
  through in `--yap-danger` on a faint danger wash (`<del>`), words it put
  in underlined in `--yap-primary` on `--yap-primary-wash` (`<ins>`), each
  with a visually hidden "taken out: " / "put in: " for screen readers
  (never colour alone; checked in light and dark), a legend, and **Paste
  what I said** (`as_said_paste_entry`: the latest dictation swapped in its
  app, its window brought back — "Swapped for what you said" —, any other
  copied: "Copied what you said" + why, unless Yap's own alert said it) and
  **Copy what I said**. `lib/core/wordDiff.js` (no dependency): a word LCS over
  normalised words (`wordKey`: lower case, letters, digits, apostrophes —
  capitals and punctuation aren't changes, cleanup adds them to almost
  every dictation, so a punctuation-only cleanup shows nothing), the same
  start and end trimmed first, a middle over 250,000 cells one change; a run
  of words out and/or in is one change; the cleaned text's line breaks
  kept. The row's menu gets Paste / Copy what I said instead of Copy raw
  transcript there.
  **The rail and the cards around the feed** (2026-10-29): the stats card's
  numbers come from `dictation/stats.rs` (they outlast clearing history:
  "N dictations, counted on this PC · See Insights"); under it
  **`home/MeetingsCard.svelte`** — "Meetings" with See all (→ Meetings):
  with a calendar connected, Upcoming / Past tabs (arrow keys between
  them; Upcoming first when something's coming), three each — a meeting to
  come opens its note made ahead (`eventAction(ev, 'open')`), a past one its
  note in Notes (`onopennote`); its notes from `calendar_meeting_notes`
  after Home's own data (`home/homeMeetings.svelte.js`, again on
  `yap-notes-changed`). Hidden for someone with no calendar and no
  meeting notes. Over the feed, **`home/WelcomeBack.svelte`** — after 14
  days without a dictation (`home_snapshot`'s `welcome`): "Welcome back",
  the gap in words ("3 weeks", "a month"), the shortcut as keycaps and one
  new thing (`core/whatsNew.js` `WHATS_NEW`, newest first, dated: the
  newest after the last dictation is "New", else the newest is "Worth a
  try"; add a line when a feature lands), its button opening Settings or a
  view; ✕ or the button answers it (`welcome_back_dismiss`, once per gap).
  Keep history 24 hours (`historyKeep` 'day'): a quiet notice over the
  feed, and Keep its audio isn't offered (it goes with its entry). Deleting
  a dictation says the stats still count it. Home re-pulls on
  `yap-history-changed` and `yap-stats-changed`, once for the two.
- **`insights/InsightsView.svelte`** = the stats
  dashboard (serif hero number, stat grid, 30-day amber heatmap, top-apps
  bars grouped by `appLabel`), all from `get_stats` (`dictation/stats.rs`,
  2026-10-29: counted on this PC, numbers only, kept when history is
  cleared): the hero says since when and the time saved, with how it's
  worked out under it ("You talk at 120 words a minute, timed over 14
  dictations; most people type about 40", or the assumed rates until a few
  are timed); six cards — words today, **words a minute** ("–" until
  measured), day streak, **week streak · longest N**, dictations, words per
  dictation —; **Fixes** — the words AI cleanup changed, the words the
  dictionary fixed, fixes this month "up 12% on this time last month";
  then the 30 days by the PC's own dates. **Share** (top right): a menu —
  Copy as a picture / Save as a picture… — of a card drawn on a canvas in
  the theme's colours (`insights/shareCard.js`, 1200 × 630: the numbers
  only, never a word said or an app), copied or saved through Rust
  (`dictation/stats/share.rs`, the PNG as the call's body).
  Settings → Privacy & data's History group has **Keep dictation history**
  (Always / 24 hours / Never, `PrivacyPage.svelte`: "24 hours" asks first
  when some would go, with the count, `history_counts`; the select is set
  back meanwhile) and Clear history (the list itself is Home's feed);
  "You've dictated 1,000 words" is `notices/milestones.js` (a tip toast
  with See Insights; `yap-open-insights` from the bar card's). Then, since
  2026-10-25, **`insights/AiUsageCard.svelte` — AI usage** (the AI usage
  tracker, [ai-models.md](./ai-models.md) `ai/usage.rs`; shown once any
  call is counted, in Insights' own chunk): "Counted on this PC from each AI
  answer's own token count. Nothing leaves this PC."; three tiles — tokens
  today, the last 7 days, the last 30 days, each "4.4K on this PC · 1.2K
  cloud · ≈ $0.01" (or "free, on your PC") —; a Today / Last 7 days / Last
  30 days switch (`aria-pressed`) for **By model** (name · where, a bar,
  tokens, "free, on your PC" / "≈ $0.02" / "no price known", the speed when
  said) and **By feature** (AI cleanup, Chat, Voice Assistant, Commands,
  Transforms, Translation, Meeting notes, Notes, Memory, Speaker names,
  Meeting briefs, Search by meaning: a bar, tokens, calls); calls that
  didn't say their tokens counted apart; **On-device speed**, one bar a
  day for the last 30 (tokens written a second, `role="img"` with the
  latest in words); the fine line on the estimates (list prices, the day
  they were checked, free plans, cached prompts, tokens with no price) and
  the 90 days kept. Bars in `--yap-primary` on `--yap-raised`, as "Where you
  dictate". Re-read on `yap-ai-usage`. `focus="ai-usage"` (Chat's context
  ring's See all, through ControlPanel's `insightsFocus`) scrolls it into
  view once drawn. Words and formats: `core/aiUsage.js` (shared with Chat's
  context ring, [chat-and-assistant.md](./chat-and-assistant.md#the-context-ring-2026-10-25)).
- **`insights/TypingCard.svelte` — Typing and speaking** (2026-10-29): the
  typing test's number (`guide_status`) beside the speaking speed over your
  dictations (`get_stats`' `wpm`), else the test's line read aloud, and how
  much faster speaking is, above AI usage, or "Take the typing test"; the
  test itself (`components/TypingTest.svelte`) opens in the card.
- **`UploadView.svelte`** = local audio-**file** transcription (drop/browse →
  Symphonia decode → chunked transcription on the warm engine with progress +
  cancel — see `dictation/media.rs`); its line names the model ("transcribes with
  Parakeet V3", `core/speechModels.js`), not its id. **The job lives in
  `upload.svelte.js`** (2026-10-24), which ControlPanel starts
  (`initUpload`, like the update store): the file, its progress, Cancel,
  the result and the `yap-upload-*` listeners outlive the view, so leaving
  Upload mid-job (another view, Chat opening by itself) and coming back
  finds it as it is, and "Transcription complete" comes wherever you are
  (until then the view held it all and unmounted with it, code review
  2026-10-23 #8). The view keeps only the drop target (its listener's
  promise kept, so leaving early still lets go) and the model's name.
  `e2e/upload.spec.js` (debug-only `e2e_upload_pace` holds each chunk).
- **Speech-to-Text's downloads** live in `modelStore.svelte.js`
  (`downloadModel`: `downloading {id, percent}` from
  `stt-download-progress`, a failure in `downloadError` until the next),
  not in `ModelManager.svelte`, which unmounts with its Settings page:
  coming back mid-download shows its bar, never Download again, and a
  failure that came meanwhile is still said.
- **Page titles** — Insights, Upload, Dictionary, Integrations, Meetings and Tasks
  use the serif display title Settings' pages have (`--yap-font-display`,
  28–30 px; `polish.spec.js` checks the face).
- **One chunk per window** (`App.svelte`, 2026-10-22): each window imports
  only its own view (`VIEWS[label]()`, a dynamic `import()` each — never a
  ternary of imports, which Vite gave one preload list, the last one's, so
  the main window lost its CSS). The Yap bar loads ~150 KB of script
  instead of the whole ~1 MB app; the notepad ~310 KB. **The main window
  keeps its everyday views in its chunk** — Home, Chat, Notes —
  and since 2026-10-25 Insights, Dictionary, Tasks, Upload and Integrations
  load on first use (`{#await import(…)}` in ControlPanel): the views
  added that week took the chunk to 735 KB, past `chunkSizeWarningLimit`
  700 in vite.config.js; then 683 KB. The afternoon's batch took it to
  705 KB, so Meetings loads on first use too, fetched as soon as the window
  is idle (`requestIdleCallback`) so opening it doesn't wait: 683 KB again
  (698 kB by 2026-10-30, just under the limit). Not for start-up speed — measured
  2026-10-22, when it stayed one chunk (~590 KB): loading the seven views other than
  Home on demand (with an idle-time preload, the old view kept until the
  new one's code was in) cut its initial script 904 → 711 KB and its heap
  4.9 → 4.7 MB, but Home came up no sooner — 62.7 → 57.3 ms from a reload
  to Home on screen (median of 22, inside the ±20 ms between runs), 256 →
  253 ms with the CPU throttled 4× — so it wasn't kept. V8 compiles a
  function only when it first runs; a CPU profile of the load puts ~4 ms in
  ControlPanel's code and most of the rest in IPC: 77 calls per load, 64
  before Home shows (50 `plugin:event|listen`, `get_config` ×4,
  `installed_models` ×2). Fewer round trips there is the start-up win.
  One part of it loads when first used: the Dictionary view's Import… /
  Export… (`dictionaryFile.js` + `ImportPreview.svelte`, ~16 KB, 2026-10-25),
  which kept the main chunk under the 700 KB warning (678 → 697 KB with
  the view's search, stars, sorts and selection in it). Since 2026-10-25
  Chat's model picker (`ChatModelPicker.svelte`, ~24 KB) and the context
  ring's panel (`ContextPanel.svelte`, ~7 KB) load the first time they
  open: the context ring took the chunk to 704 KB, and with them out it's
  681 KB. By 2026-10-30 it was 746 KB again, so Settings' two biggest
  pages, each on a page of its own and rarely opened, load when their page
  first shows (`{#await import(…)}` in Settings.svelte): Memory
  (`MemorySection.svelte`, ~23 KB) and Account (`AccountSection.svelte`,
  ~27 KB): 698 KB. (Neither shares a class with `settings.css` whose tie a
  chunk's CSS order would decide: frontend-settings.md, "CSS order".)
- **Home's first paint** (2026-10-24): Home's data comes in **one call
  asked first** — `home_snapshot` (`dictation/commands.rs`, async: off the window's
  thread; the history page, the stats, the settings Home shows, the
  installed models), started by `App.svelte` for the main window while
  ControlPanel's code is still loading (`lib/views/home/homeData.js`: `prefetchHome`,
  then `homeSnapshot` hands Home that answer once and asks afresh after);
  `.home[data-ready]` once it's in. Before, Home asked in four calls (two
  sync, holding the window's thread) issued after ~60 others and answered
  behind 66 of them. And **reads asked at once go once**
  (`lib/core/ipcShared.js` `shared`: a call made while the same one is on its
  way gets its answer, a copy; Appearance and the model list use it):
  `get_config` 4 → 3, `installed_models` 2 → 1. Measured from a reload of
  the main window (stub debug build, the PC busy with other builds,
  medians of 10): Home ready **132 → 73 ms**, behind **66 → 3** answered
  calls; 79 → 76 calls in all. The 52 listeners (47 events; Tauri has one
  call per listener, `listen_js` is crate-private, so they can't be
  batched) run beside it and were never what Home waited for: each takes
  ~20–30 ms, all in flight at once. e2e `home-first-paint.spec.js` guards
  the order and the count (not the milliseconds).
- **AI errors in plain words** (`lib/core/aiErrors.js` `aiError`): Chat's
  "Couldn't send" says "Couldn't reach the AI model on this PC
  (127.0.0.1:11434). Check that it's running (Ollama, LM Studio…)…", "…check
  your internet connection…", a refused API key, a model the provider
  dropped or a provider-side failure, with **Open Language Models** when
  that's where it's fixed; anything else stays raw in the copyable error box.

## Shared components

- **`lib/settings/ModelManager.svelte` / `ModelRow.svelte` / `core/speechModels.js`** — the 14-model
  browser, OpenWhispr-style: vendor pill tabs (All/NVIDIA/OpenAI/Community via
  `ui/PillTabs.svelte`) + compact one-line rows (status dot, brand icon from
  `providerIcons.js` + `assets/providers/*.svg` (MIT, from OpenWhispr), name,
  size, Download/Active/delete; delete asks first in `ui/ConfirmDialog.svelte` —
  ⚠ never `window.confirm/alert/prompt` in a Yap window: Tauri routes them to the
  dialog plugin, which these windows aren't allowed, so they throw).
  `ModelCard.svelte` (big cards) remains only in
  Onboarding (`onboarding/ModelCard.svelte`). `ModelRow` also draws the speaker models (`SpeakerModels.svelte`:
  `unavailable` = a listed model's note instead of Download, `deleteActive`).
- **`lib/onboarding/Onboarding.svelte`** — first-run **guided setup** (its
  parts, 2026-10-24: `setupState.svelte.js` holds every step's state across
  the steps, the config fields this window changed and its raw event
  subscriptions; `StepHeader`, `ModelStep`, `LanguageStep`, `MicStep`,
  `CleanupStep`, `CalendarStep`, `TrayStep`, `TryStep`, `TypingStep`;
  `cleanupProviders.js` the brands and cloud providers) (eight steps since
  2026-10-29, the calendar's only until it's answered: model → languages →
  mic → AI cleanup → calendar → tray → try it → typing speed): model picker
  (**two of the 14 shown**: Parakeet V3, recommended, and the best other pick
  for the PC's language — Parakeet V2 where `navigator.language` is English,
  else Whisper Large v3 Turbo; the rest behind "More models (12)", as the AI
  cleanup step does, so the Download button shows without scrolling; a model
  already installed or being set up further down keeps the list open) →
  **languages** (2026-10-29, `LanguageStep`: "I only speak English" saves
  `myLanguages` ["en"] and moves on in one click; else the PC's language
  and the common ones as chips — `aria-pressed`, saved as they're picked —
  and a search for the rest; with a speech model in use, what it makes of
  them, `my_languages_status`, and when it misses one the model that covers
  them all with Download / Use it, `setup.choose`) →
  mic check (live level meter via `set_mic_test` idle-amp mode — the bar's
  perceptual levels, `dictation/meter.rs`, "Hearing you" above 0.3 — + live device switch;
  under it the **mic quiz**, `components/MicQuiz.svelte`: read the line,
  every connected mic listens at once with its own level, each one's
  verdict, the best marked, a Bluetooth headset's note, "Use this one"
  puts it first in the order — `dictation/mic_quiz.rs`)
  → one-click **local AI cleanup** install (the model recommended for this PC
  first and picked: Qwen3.5 4B with a graphics card that can run it, "Your
  <card> runs Qwen3.5 4B quickly…", else Qwen2.5 1.5B; then Qwen2.5 1.5B as
  the light one; **two shown, the rest behind "More models (N)"** so the
  Download button shows without scrolling — a pick further down keeps the
  list open). **Which way is recommended** (2026-10-24): **Private** (on
  this PC) when the graphics card runs the recommended model or a model is
  already here, else **Cloud** ("Recommended for this PC" on that card,
  picked; Private says "slower without a graphics card"), with the privacy
  line "Your voice never leaves this PC: … only the text of each dictation
  goes to the provider you pick" (`data-privacy="cloud"`); nothing is on
  until the person enables one — "Maybe later" leaves cleanup off. Once on,
  the step says what it does where (Styles by app: Email · formal, chats ·
  casual, AI prompts · word for word…; `ai/cleanup_styles.rs`) → the
  **calendar** for meeting notes (2026-10-29, `CalendarStep`: what a calendar
  gives, Connect calendar opening the main window's own
  `ConnectCalendarDialog` over the setup, the snapshot asked every 1.5 s
  while the step is up; connected — "Calendar connected · N meetings coming
  up" — or **Skip for now**, it never shows again: `shell/guide.rs`) → the tray (the Yap bar, click the icon to open Yap,
  right-click to paste the last dictation; the footer pinned at the bottom
  of every step) → "try it here" live dictation test with **Settings' own
  shortcut picker** (`ui/HotkeyInput.svelte` + `validateHotkey`: combos,
  chords, mouse buttons, the Copilot key; the old recorder kept only the
  bare key; in push-to-talk it says "Hold …, then let go") → the optional
  **typing test** (2026-10-29, `TypingStep` + `components/TypingTest.svelte`:
  type a short passage — the clock from the first key, Enter or Done to
  stop, the characters typed right ÷ 5 a minute —, then say it with the
  dictation shortcut, timed from the dictation's own `yap-state` and
  counted from its transcript — or, once Yap has timed a few dictations,
  the speaking speed over them (`get_stats`' `wpm`, `dictation/stats.rs`),
  and no line to say; both speeds and how much faster speaking is; only
  the numbers kept, `guide_typing`; Finish works either way). With no microphone the mic step says so. **Finish** opens
  Yap's main window on Home (`open_settings`), so setup doesn't end on an
  empty screen. **By hardware** (2026-10-24, `hardware_advice`,
  ai-models.md `ai/hardware.rs`): the model step says where speech runs ("on
  its processor: no graphics card needed" without a real card); the AI
  cleanup step says what the private model and a cloud provider each add
  to a dictation on this PC (`[data-testid=cleanup-times]`), and on a small
  PC the Cloud card is picked and badged "Quicker on this PC" (the
  Private card loses "Recommended"). The download button says "Setting up
  Parakeet V3…" once the download is done (its check, unpacking and load,
  ~5–8 s), not "Downloading… 100%". Timed walk: docs/benchmarks.md "The
  first run, step by step".
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
  Home's dictations (Copy, Copy raw transcript — or, AI-edited, Paste what
  I said and Copy what I said —, Run again, Play, Delete…; the trash asks
  too), Meetings' past notes (Open, Generate summary, Copy as Markdown,
  Delete… via `meeting_delete`; a hover ⋯) and calendar meetings (Open note
  + their row buttons), Dictionary rows (Edit, Star, ≈ Catch near-misses,
  Delete…; × asks too unless the row is empty; a delete has Undo), model rows (`ModelRow`:
  speech, speaker, Kokoro — Download / Use / Delete…), the on-device model
  cards in Settings and the Chat model picker's installed on-device models
  (Download / Use / Uninstall…, 2026-10-27, `core/localModels.js`: off
  with a `note` naming what uses it — "Dictation cleanup uses it. Switch
  that to another model first", `local_llm_status.inUseNote` — while the
  model is in use; the picker stays open meanwhile), Connectors' calendars
  (their ⋯ = this menu: Sync now, Disconnect… asked first), Account's other
  devices (Sign out), the Actions manager's rows (Edit, Delete…), the
  notepad's transcript lines (Copy line, Who said this?) and Ask answers
  (Copy, Copy as Markdown, Read aloud, Good / Bad answer) and questions
  (Copy). `e2e/right-click.spec.js`.
