# Notes

The AI Notepad: the notes store, folders, the Actions engine, notes you can fix (a line's words, the summary, tasks), the quick-note window, and the Notes and Tasks views. Meeting notes' recording and summaries are [meetings.md](./meetings.md).

[Architecture index](./README.md) · [CLAUDE.md](../../CLAUDE.md)

## `notes.rs`

- **`notes.rs`** — the AI Notepad's data layer (`notes.json`, camelCase). Stores:
  `content` (raw markdown, never overwritten by AI: the **My thoughts** tab),
  `enhanced_content` (the **Summary** tab; its tasks' ticks are its own
  `- [x]`, `notes/tasks.rs`), `enhanced_at_hash` (OpenWhispr's
  `len+first-50` staleness marker), `summary_edited_ms` (the person changed
  the summary's words: Yap never writes over it unasked) and `summary_ts`
  (when the summary's input was read: "New since this summary"; both 0 and
  unwritten on older notes, `notes/note_edits.rs`), `summary_ai` (Yap's
  summary as it last wrote it, for "Back to Yap's summary") and
  `summary_rating` (`{up, why, ms}`: the person's 👍 / 👎, on this PC only)
  — both 2026-10-25, unwritten when empty; an older Yap on the same data
  drops them when it saves, and the person's summary stays in
  `enhanced_content` either way —, `folder` (string, seeded with Personal +
  Meetings), `folder_chosen` (the person moved the note there: `apply_update`
  sets it on any folder change — the Notes view, the local API, Chat),
  `participants`
  (attendee names, shown as chips and fed to prompts for attribution),
  `transcript` (meeting-recorder segments, time-ordered; `source` = the side,
  "you" (mic) | "them" (call); `speaker` = the voice (`speakers.rs`; empty =
  the side's own, read through `speaker_id()` — every old note reads as
  You/Them), `speaker_locked` (the user said who this line is), `start_ms`/
  `end_ms` (place on that recording's clock, ms; none on old lines and
  markers), `words` (`[start, end]` ms from `start_ms` per word of `text`,
  when the model gave times), `edited` + `original` (the person fixed its
  words; what Yap heard: `notes/note_edits.rs`, below); `echo` marks
  speaker bleed, `dictated` a "You dictated here" marker — no text, `ts` where
  a mid-meeting dictation began; `failed` a stretch the speech model couldn't
  transcribe — no text, `start_ms`..`end_ms` the stretch, "Couldn't
  transcribe 0:42–1:12 · Try again" (`meetings/meeting.rs`, "Failed chunks");
  `is_talk()` = none of them, what every summary input reads), `speakers` (the speaker map, see `speakers.rs`), `digests` (rolling meeting digests, see `meetings/meeting_summary.rs`),
  `breaks` (the transcript's length at each stop — the notepad's paused
  dividers; `add_break`, never twice at one place or at 0),
  `note_type` ("personal" | "meeting"), `source` ("manual" | "upload" |
  "meeting"), and `title_auto` (the title is one Yap made up — "Teams call ·
  5 Oct, 14:30" — so the AI meeting title may replace it; cleared for good by
  a title edit, so a typed title is never overwritten: `set_ai_title` checks
  under the store lock). `note_update` takes an `origin` (the saving window's
  label) and emits `yap-note-changed` with the editable fields (+ `speakers`;
  after a speaker edit also `lineSpeakers`, `[voice, locked]` per line), which
  keeps the Notes view and the meeting notepad in sync both ways (each sends
  only the fields it edited). `modify(id, f)` changes one note under the store lock
  (read and write in one step — the chat's `edit_note`, "Transcribe again";
  only a change bumps `updated_ts` and saves). `insert_transcript` puts
  lines transcribed after the fact (a crash's, `meetings/meeting_audio.rs`) where they
  belong by time, digests and stop marks moving along (a stop mark after
  them when a later recording follows). `delete` takes the note's meeting
  audio with it (`meeting_audio::forget`) and its Ask threads
  (`chats::delete_for_note`, `chat/note_chats.rs`). **Saves** go through `SAVER`
  (`shell/saving.rs`, [config-and-data.md](./config-and-data.md)): a failed one
  keeps the notes in memory and is written with the next change; the
  commands that change notes return it as their error (`saved()`: "Couldn't
  save your note: the disk is full."), which the Notes view and the notepad
  show with Try again (`saveErrors.js`), and a meeting's lines that couldn't
  be saved are said once by the notice. **Written off the lock**
  (2026-10-25): `save_to_disk`, under the store lock, takes a snapshot (the
  JSON, numbered by `saving::Ordered`); `with_store` writes it once the lock
  is let go, one write at a time, never an older snapshot over a newer one —
  a long meeting's notes.json no longer holds every recorder chunk, line
  edit and tick behind the disk (unit-tested: 8 threads, the file ends as
  the store). Unit tests get an in-memory store per test
  thread (`cfg(test)`), never `notes.json` (saved only to a scratch file a
  test names, `TEST_PATH`). **An older Yap on the same data** (the installed
  nightly beside a dev build) drops fields of notes.json it doesn't know when
  it saves: nothing new is kept only there — tasks the person adds and
  archive marks are `tasks.json` (`notes/task_store.rs`), a deleted line is
  simply gone (its place in the meeting's `cuts.json`), a split line is two
  ordinary lines. It would drop `edited`/`original` (if from before
  2026-10-24) and `transcriptDeletedTs`; unit test `an_older_yap_saving_
  notes_json_keeps_added_tasks_and_split_lines`. `event` (optional) = the calendar meeting the note
  belongs to (`NoteEvent`: key, title, start/end, the invite's description,
  service), set by `link_event` (also the meeting's title as a real title,
  attendees merged, `note_type` meeting) and found by `find_by_event`.
  **A recording files its note** (`file_as_meeting`, from
  `meetings::commands::meeting_start`, every way a recording starts): it becomes a
  meeting note and, if it sits in Personal by default (not `folder_chosen`),
  moves to Meetings — where "Take notes", call detection, the bar and the
  calendar put theirs — and `yap-note-filed {noteId, from, to}` (+
  `yap-notes-changed`) tells the Notes view, which follows it there and
  says so: "Moved to Meetings · “Supplier call” is a meeting note now…"
  with **Keep in Personal** (a move, so it sticks). A note in a folder of
  the person's own, or moved to Personal on purpose, stays put.
  `updated_ts` untouched.
  **Folders** are user-creatable (backend `notes_folder_create` command) with
  counts shown in the sidebar; notes filter by active folder. `folder_rename`
  (trimmed, non-empty, never a name another folder has in any case — a
  change of case is fine; every note in it goes along, the folder keeps its
  place) and `folder_delete` (its notes move to **Personal**: OpenWhispr
  deletes them with the folder, Yap never loses a note to a clean-up) both
  refuse **Personal** and **Meetings** (`PERSONAL`/`MEETINGS`,
  `is_protected_folder`: the default folder, and where the recorder, call
  detection, the bar and the calendar file meeting notes), work under the
  store lock, leave the notes' `updated_ts` alone and return a
  `FolderChange` (the folders, the ids moved); `find_folder` = a folder by
  any case, as the list spells it. Commands `notes_folder_rename(from, to)` /
  `notes_folder_delete(name)` emit `yap-note-changed` (origin "folders") per
  moved note + `yap-notes-changed`, and take the meeting folder rules along
  (renamed: they follow; deleted: they go). **Folder rules for recurring
  meetings** (2026-10-29, [calendar.md](./calendar.md#day-to-day-2026-10-29),
  `calendar/filing.rs`, `meeting_folders.json`): the Meetings view's "File
  this meeting in…" (one note, through `notes::update`, so `folder_chosen`)
  and "Always file Weekly sync in…" (every note of the calendar series: the
  earlier ones when asked, each new one as the calendar makes it or a
  recording ties it to its meeting — never one the person moved). **Actions** (named
  prompt fragments) are built-in protected set (seeded "Generate Notes",
  "Meeting Notes", "Action Items", "Action Plan") + user-created; `note_enhance`
  (`notes/commands.rs`) invokes the **Actions engine**: picks an action by id, runs the
  Note Formatting scope's endpoint + the action's editable prompt (fallback →
  global cleanup endpoint), at temp 0.3 under an immutable base prompt:
  `llm::NOTE_BASE_PROMPT` (OpenWhispr's BASE_SYSTEM_PROMPT verbatim),
  `MEETING_NOTE_BASE_PROMPT` for meeting notes, or `ACTION_PLAN_BASE_PROMPT` for
  the Action Plan (found by `Action::kind == "actionPlan"`, so renaming is fine;
  its reply goes through `meeting_summary::postcheck_action_plan`). Built-ins
  seed via additive migration (by name, or by kind), so user edits to their
  prompts are never clobbered. For **meeting notes** the input comes from
  `meeting_summary` (the raw transcript when short, else digests + the latest
  stretch, after digesting any backlog); the Action Plan runs in Rust when a
  recording ends (`meetings/meeting_end.rs`: anything but Pause), through
  `notes::commands::run_enhance` (the body of `note_enhance`, with step reporting),
  which stores the result through `note_edits::write_summary` (never over the
  person's edited summary unless `replace`; ticks carried over).
  `set_enhanced` is the evaluations' and the tests' only (debug builds).
  For search by meaning ([search.md](./search.md)): `with_all(f)` reads
  every note in place (no copy of a thousand notes per keystroke), `stamp()`
  is a cheap fingerprint of them, and `delete(id)` tells the index
  (`search::removed`: its vectors go too). Code that removes part of a note
  for good — a meeting's transcript retention deleting lines — calls
  `crate::search::removed()` after the store has let go.
  `modify_or_remove(id, f, gone)` (2026-10-28) is `modify` and then the
  note taken away under the same lock when `gone` says so of it as changed
  (the quick note emptied, below); a note taken away either way loses its
  added tasks, audio, Ask threads and vectors (`forget_extras`).

## The quick note (`notes/quick_note.rs`)

A small window on its own shortcut, to jot or dictate a note from any app
and find it later in Notes (Wispr Flow's Scratchpad, adapted: local, no
sync; 2026-10-28; tabs, the hold, Send and the bar's button 2026-10-29). The
page is `src/lib/quicknote/QuickNote.svelte` (+ `quickNote.js`, its pure
helpers); its parts in Rust are `quick_note/key.rs` (the shortcut's tap and
hold), `tabs.rs` and `send.rs`.

- **The window** — `quick-note` in tauri.conf.json: 420×380 (min 300×220),
  undecorated (its header is the drag region), always on top while it's
  up, created hidden and unfocused, the DWM-cloaked first show like the
  notepad (`shell::window::init_hidden_webview`), never remembered by the
  window-state plugin, closing (Alt+F4) hides it (`init`). The notepad's
  paper and ink (`--yap-paper*`; `shell/theme.rs` gives its webview the same
  background, light #faf9f6 / dark #1c1a16), the reading font and size
  for the text (`--yap-font-reading`, `--yap-reading-scale`). Hidden, its
  webview gets WebView2's low memory target like every hidden window
  (`shell/webview_memory.rs`). **Where**: the first time since Yap started,
  and whenever it's on another monitor than the mouse cursor, at the bottom
  right of the cursor's monitor's work area, 24 px in (`corner`, pure; a
  test run uses the primary monitor and never reads the cursor) — clear
  of what you're working on in the middle and of the Yap bar at the bottom
  centre; moved by the person, it stays where they put it on that monitor
  (kept inside the screen, `window::fit_rect`).
- **Opening it** — the **Quick note** shortcut (Settings → Shortcuts,
  `quickNoteHotkey`, unbound by default; the tenth fixed binding in
  `dictation/input_hook.rs`, its press and release, `quick-note-key-pressed`
  / `-released` → `key::on_key`), the tray's **New quick note** (always a
  new note), **Quick note** in the Notes view's tools (`quick_note_open`),
  and the Yap bar's button when it's on (below). From one of Yap's own
  windows the hook never fires: the main window's, the notepad's and the
  quick note's own in-page fallbacks send the key's press and release
  (`quick_note_key {pressed}`) to the same place.
- **Tap or hold** (`quick_note/key.rs`, Wispr's `SCRATCHPAD_PTT_DELAY_MS`
  by behaviour; 2026-10-29). `Key` is pure: a press while the window isn't
  in front (`in_front`: Windows' foreground window is its handle; a test
  run never has the focus, so there it's "shown") brings it up at once
  (`open_soon`, the main thread) and waits `HOLD_MS`; let go sooner, it
  was a **tap** — the window stays up — or, pressed while it was in front,
  a tap that puts it away (on the key-up, so a hold there doesn't hide it).
  Still down at `HOLD_MS` it's a **hold**: `Pipeline::start_for_quick_note`
  starts a push-to-talk dictation armed for the note (`quick_note::arm` →
  `Goes::QuickNote`, `yap-dictation-held`), and the key-up stops it
  (`stop_session`, only that recording). `HOLD_MS` is 300 ms, push-to-talk's
  own tap line (`gesture::TAP_MAX_MS`: a press let go sooner is a tap
  everywhere in Yap), a little quicker than Wispr's 350 ms; the dictation
  never starts on the press, so a tap plays no chime. A dictation already
  under way (recording or transcribing): the hold only opens the window. The
  key is one of the input hook's **hold keys** (`HOLDS`): another key joining
  it under `gesture::CHORD_MS` was a shortcut sharing it — no dictation, or
  the one it started dropped silently (`Key::chord`); a key-up the hook lost
  leaves the dictation hands-free until the next press (`Key::key_lost`,
  `yap-dictation-locked`). The machine is locked while a step is acted on,
  so a key event and the hold's tick take turns. The quick note takes the
  focus on the press, so its page usually hears the release: a release
  heard by a page also lets go of the hook's own record of the key
  (`input_hook::quick_note_key_up`), or the next press from another app
  would look like the key repeating. No double-tap (Wispr's hands-free):
  a quick second tap closes the window, as a toggle should; hands-free into
  the note is its mic button, or the dictation key with it in front.
- **What it opens on** — `pick` (pure): the note it was on, or — the first
  open since Yap started — the newest quick note, **if it changed less than
  a day ago** (`RESUME_SECS`); else a new note. Coming back within the day
  you're still writing it; a note left alone a day is done (it's in Notes)
  and the window starts a clean page, so a scratchpad never grows into one
  endless note. "New note" (+, Ctrl+N) and the tray start one whenever.
  Rust remembers the window's note (`Current`: Unset / New / Note) and says
  what to show with `yap-quick-note-open {noteId, title, titleAuto,
  content, folder, fresh, tabs, close}`; the page saves what it has before
  it switches, keeps a new note being written, and starts a `fresh` one
  itself (so a save on its way can't take the new note's place).
- **Tabs** (`quick_note/tabs.rs`, 2026-10-29): several quick notes open at
  once. The page keeps them — **+** or Ctrl+T (Ctrl+N) a new one (the blank
  tab there, if any), **×** or Ctrl+W to close (the note stays in Notes;
  the last one closed leaves a new note), a click or Ctrl+Tab /
  Ctrl+Shift+Tab (Ctrl+PgDn / PgUp) to switch — and after every change
  tells Rust the open notes in order and the one in front
  (`quick_note_tabs {ids, active}`, after any save on its way; it sets
  `Current`), which remembers them in `quick_note_tabs.json` (its own file,
  a quiet saver; ≤ 20, ids only; [config-and-data.md](./config-and-data.md)).
  What it opens on is the rule above — the tab in front is "the note it was
  on"; the picked note gets a tab if it had none. As the window comes up
  (and the page loads, `quick_note_state`) `tabs::for_open` (pure) keeps
  the remembered tabs whose notes are quick notes changed within a day (or
  the picked one) and closes the rest (`close`): a tab left alone a day is
  done, as a note was before tabs. Only the tab in front can hold unsaved
  words (switching saves first); saves go to the tab they were typed in.
  Notes deleting a note closes its tab behind; the one in front lets go of
  it, as before. The tab strip is the header (its empty part still drags the
  window); the tab in front's name is the title (`.title`).
- **Saved as ordinary notes** — the page saves 400 ms after the last
  keystroke, one save at a time (`quick_note_save {id, content}`), and on
  the way out (Esc, hidden, another note). `save`: nothing is made until
  there's a word; the first one makes the note (`new_note`: `source`
  "quick", the **Quick notes** folder — made the first time, found in any
  case if renamed so; the person may move or delete it like any folder —
  and the first words as the title); later saves go through `apply_text`
  (pure: the text, and the title from the first line's first six words,
  "…" for more, ≤ 60 characters, no heading `#` or list mark, while the
  title is Yap's — empty or `title_auto` — never over one the person typed
  in Notes); **an emptied note goes** (`is_blank`, pure: no words, no title
  of the person's, no transcript or summary, not a meeting; never the note
  a meeting records into), under the store's lock (`modify_or_remove`).
  Every window is told (`yap-note-changed` origin `quick-note`,
  `yap-notes-changed`, `yap-note-deleted`), so Notes, search by meaning and
  Chat's note tools see quick notes as they're typed. A save that failed
  comes back as `unsaved` ("Couldn't save your note: …") rather than an
  error, so a new note's id still reaches the page: the footer says "Not
  saved yet" with **Try again** (`saves_retry`). The window follows Notes
  too: a title typed there shows in its header (and stays), the words
  change unless something's being typed, a note deleted there lets go.
- **An older Yap on the same data** (the installed nightly beside a dev
  build) keeps a quick note whole: it's made only of fields every Yap since
  2026-10-05 reads and saves back (`source`, `folder`, `titleAuto`, the
  folder in `folders`), so an older Yap lists it in Notes as an ordinary
  note in the Quick notes folder and its save changes nothing (unit test
  `an_older_yap_saving_notes_json_keeps_a_quick_note_a_quick_note`). It
  drops only the shortcut (`quickNoteHotkey` and its `moreHotkeys` entry):
  unbound again, as by default ([config-and-data.md](./config-and-data.md)).
- **Dictating into it** — while its window is in front, the dictation key
  (its in-page fallback's `dictation_key`, a language shortcut's
  `language_key`, the bar's Dictate) starts a dictation that's the note's:
  `capture_target` asks `takes_dictation(own_window)` (its mic button armed
  it — `quick_note_dictate`, `Pipeline::toggle_for_quick_note` — or the
  foreground window is its handle; atomics and one Win32 call, safe under
  the pipeline's locks) → `Goes::QuickNote` → `Shared::to_note`. It's
  cleaned up as any dictation (the AI cleanup, a transform on every
  dictation, the dictionary, snippets; no Commands by name, which paste),
  then `deliver_to_note` emits `yap-quick-note-dictation {text, newline}`
  — nothing pasted, no key pressed, never into the app behind it nor into
  Yap's own window by the clipboard — and the page puts it at the cursor
  (`dictationPiece`: a space before it when it would touch a word, after
  it before a word; a new line after it when "press enter" was said) with
  `execCommand('insertText')`, so **Ctrl+Z takes it back out** in one step
  (by hand, and saved, when the window hasn't the focus). Kept in history
  like any dictation ("yap.exe"). **Undo** of a cancelled one (the bar's
  "Dictation cancelled · Undo") lands in the note too: `to_note` lives
  through the cancel, as the Chat box's `to_chat` does.
- **Send to {app}** (`quick_note/send.rs`, Wispr's "Send in {app}";
  2026-10-29): `open` remembers the window in front before it shows
  (`send::remember_before`: `text_injector::app_window`, as Paste last
  finds it — the tray's open too; never one of Yap's own, so opened from
  Yap there's no button). The footer's **Send to Slack** shows the app's
  name (`core/appNames.js` from its process name) and its exe's own icon
  (`calls/app_icons.rs` `exe_icon_url`, drawn once per exe;
  `quick_note_send_target`), disabled while the note is empty. Clicking it
  (`quick_note_send {text}`: the selection if it has words, else the note,
  `sendText`) saves, hides the window and pastes by the ordinary path —
  `text_injector::inject_text(…, strict)`: back into that window or
  nowhere, its own paste keys, the paste checked; a paste that can't land
  is left on the clipboard and said once (`Left::alert`), a window that's
  gone too ("Your note is copied: press Ctrl+V"). Nothing in
  `text_injector`'s internals changed. Test mode pastes nothing: a spec
  stages the app (`e2e_quick_note_before`) and reads what went
  (`e2e_quick_note_sent`).
- **The Yap bar's button** (`bar_quick_note`, off; Wispr's "Add to Flow
  Bar"; 2026-10-29): on the empty page (no words in the note in front, the
  bar on, the button off) a slim offer, "Open it from the Yap bar too"
  with **Add to the Yap bar** (`quick_note_bar_button {on}` →
  `settings::change_config`, `yap-bar-changed {quickNote}` for Settings'
  copy) and ✕ Not now (that window's `localStorage`). Also Settings →
  General → Yap bar → Quick note button. The bar's side:
  [yap-bar.md](./yap-bar.md).
- **Keys** — Esc puts it away (what's typed is saved), Ctrl+T or Ctrl+N a
  new tab, Ctrl+W closes one, Ctrl+Tab switches; the buttons (the tabs and
  their ×, New note, Open in Notes, Close, the mic, Send) are in the tab
  order. **Open in Notes**: the main window's Notes view on the note
  (`yap-meeting-open-note`, as the notepad's back button), and the window
  steps aside. The in-page fallback: the Quick note shortcut (away), the
  meeting shortcut, the Voice Assistant's, Read aloud (its selection),
  Snip to Chat, Copy last, the language shortcuts and the dictation key.
- **Commands** — `quick_note_toggle`, `quick_note_open {fresh?}`,
  `quick_note_hide`, `quick_note_state` (the page loading), `quick_note_save
  {id, content}` → `{noteId, title, titleAuto, folder, removed, unsaved?}`,
  `quick_note_new`, `quick_note_dictate`, `quick_note_key {pressed}`,
  `quick_note_tabs {ids, active}`, `quick_note_send_target`,
  `quick_note_send {text}`, `quick_note_bar_button {on}`; debug-only
  `e2e_quick_note_dictation {text}` (its mic hearing a transcript),
  `e2e_quick_note_before {app}` / `e2e_quick_note_sent`, with
  `e2e_shortcut {name: 'quick-note', phase?}` (a tap; 'press' / 'release'
  alone: a hold) and the tray's `e2e_tray {click?}` (`shell/tray.rs`).
  Tests: unit tests in `quick_note.rs` (titles, blank, `pick`, `corner`, an
  older Yap's save, made and emptied), `key.rs` (tap, hold, chord, a lost
  key-up), `tabs.rs` (what closes, tidy, the file) and `pipeline.rs`
  (`Goes::QuickNote`), `input_hook.rs` (the binding);
  `e2e/quick-note.spec.js` (a made-up microphone, `YAP_E2E_MICS`, records
  the hold).

## Notes you can fix (`notes/note_edits.rs`, `notes/tasks.rs`)

Guide: [`docs/meetings.md`](../meetings.md#fixing-the-notes) (Wispr Flow's
Notetaker lets you fix lines, edit the summary and tick tasks; Yap's private
roadmap, "Meeting notes you can fix" and "Tasks").

- **`notes/note_edits.rs`** — **a transcript line's words**
  (`transcript_line_edit(noteId, line: LineRef {index, source, ts, startMs,
  text}, text, origin)` → `{index, line, changed, offer}`): the line found
  where the window showed it, else wherever the same line went (a split by
  the voice separation; `find_line`), refused when its words changed
  meanwhile; `apply_line_edit` (pure) — one line of words, never a marker,
  ≤ 4,000 chars, the first edit keeps what Yap heard in `original`, an edit
  back to it clears `edited`; side, voice, `ts`, `start_ms`/`end_ms` and
  `words` untouched (▶ plays the same stretch). `yap-note-changed` carries
  `lineEdit {index, line}` (`notepadText.js` `withLineEdit`). **The
  dictionary**: `learn::offer_for_line(old, new)` — the same `suggestions`
  rules and answers as a dictation's (the setting, words turned down or
  removed left out), returned for the window to ask in its own toast
  (`wordSuggestions.js` `askWordSuggestion`, answered by
  `word_suggestion_respond`); no three-minute gap before it, but it counts
  as the last prompt. **The summary's words** (`note_summary_update(id,
  text, origin)` → `{enhancedContent, summaryEditedMs, summaryTs}`;
  `apply_summary_edit`, pure): a change of words stamps `summary_edited_ms`, a
  change of ticks alone doesn't (`tasks::same_but_ticks`), emptied = no
  summary, written from nothing = made from the notes as they are; Chat's
  `edit_note` on the summary goes through it too (`chat/note_text.rs`). **The
  rule** (`write_summary`, `keeps_summary`, `may_replace`): a summary run
  goes over the person's summary only with `replace` (the window asked:
  `noteEdits.js` `askReplaceSummary`) and when their last edit came before
  the run read the note; else `SUMMARY_KEPT` — `run_enhance` checks before
  spending a model call, `meeting_end`'s job ends `kept`
  (`meeting_summarise(noteId, replace)`, `note_enhance(id, actionId,
  replace)`). A written summary clears the mark, sets `summary_ts` = the read
  time (seconds), keeps the old summary's ticks, keeps itself as
  `summary_ai` and clears the rating. **Back to Yap's summary**
  (2026-10-25; `note_summary_restore(id, origin)` → `{state, previous}`;
  `can_restore`, `restore_ai_summary`, pure): offered while the person's
  summary differs from Yap's (ticks aside); Yap's words come back with the
  person's ticks (`carry_ticks`), Yap's again; `previous` is the person's
  version, which the window's toast's **Undo** saves back through
  `note_summary_update`. Words edited back to Yap's own make it Yap's again.
  **A rewrite's Undo** (2026-10-29, "Make notes longer",
  `meetings/meeting_assist.rs`): `SummaryBefore` (the words, Yap's version,
  the person's mark and `summary_ts`) is taken before the rewrite goes
  through `write_summary`, and `note_summary_put_back(id, before, origin)`
  (`put_back`, pure) puts it back with the tasks ticked since still ticked.
  Note version history, when it lands, takes this place: the summary
  before a rewrite kept as a version, Undo restoring it.
  **Rating it** (`note_summary_rate(id, rating: up|down|null, why, origin)`,
  `apply_rating`, pure): 👍 / 👎 and, with a 👎, a "What was wrong?" note
  (one paragraph, ≤ 500 characters); the same again is no change, `null`
  takes it back; no summary, nothing to rate. Logged as up/down and the
  note's length, never its words. `SummaryState` (every summary command's
  answer, and `yap-note-changed`'s summary fields) carries `summaryAi` and
  `summaryRating` too.
- **`notes/tasks.rs`** — **the checkboxes in a summary**: `parse` (pure; line by
  line, fenced code skipped; `- * + •` bullets; owner from `### Name` — not a
  heading like "Action items" —, inline "Dave: …", or "Unassigned" under
  `## Unassigned`; "You"/"Everyone" normalised; `(due: …)` split off; `**`
  dropped), `set_done(md, line, text, done)` (the task on that line with
  those words, else the first with those words, else refused; only its mark
  changes, CRLF kept), `same_but_ticks`, `carry_ticks(old, new)` (a new
  task with the same words — or six in ten of the longer's content words —
  and a compatible owner comes ticked), `moment(note, task)` (the talk line
  sharing most of its content words: ≥ 2 and half), `list` (every note with
  tasks, newest first, each task with its `at {index, ts, secs}`). Commands
  `tasks_list`, `note_task_set(noteId, line, text, done, origin)` (→ the
  summary; `yap-note-changed` with `enhancedContent`, `summaryEditedMs`,
  `summaryTs`). Read only outside the app: `integrations/bridge.rs` `GET /v1/tasks/list`,
  `integrations/mcp.rs` `list_tasks` ([integrations.md](./integrations.md)). The
  pre-meeting brief's open items (`calendar/brief.rs` `plan_tasks`) read only
  `- [ ]`, so a ticked task drops out of the next brief.
- **`notes/transcript_edits.rs`** (2026-10-25) — **a fixed line, the rest**:
  `transcript_line_revert` (`revert_line`: an edited line's `original`
  back), `transcript_line_delete` (`remove_line`: the line leaves the
  transcript; stop marks after it move up; digests from the one that read it
  dropped, `drop_digests_from`; its place to `meeting_audio::set_cut` so
  Transcribe again and the refine never bring it back; kept in memory for
  Undo, `Removed`) and `transcript_line_restore` (`restore_line`: where it
  was when nothing changed since, its stops as they were, else by time),
  `transcript_line_split` (`split_line` at word `at`: each part its side,
  voice, lock and stretch — the words' times, else shared by length; on an
  edited line the times are the edited words' shares and `original` is split
  where the cut falls; both parts `edited`; `notes::replace_line` moves
  digests and stops). Delete and split refuse a note recording, being
  summarised or transcribed (`busy`). Windows follow `lineEdit` (revert) or
  `lineSplice` + `lineSpeakers` + `breaks` (delete: no lines; restore: the
  line; split: two). Unit-tested (each edit; Transcribe again keeping a
  deletion and splits).
- **`notes/task_store.rs`** (2026-10-25) — **`tasks.json`**: `OwnTask`s (the
  person's, with `noteId` or none) and `Archived` (a summary task by note and
  words); `Saver` "your tasks", a corrupt file set aside, `forget_note` on a
  note's delete. Commands in `tasks.rs`: `task_add`, `task_remove`,
  `tasks_set` (`mark_own` + `mark_summary` per note: Complete all, Undo),
  `tasks_archive` (`archive`), `note_tasks_get` (`note_tasks`: the summary's,
  then the added), `tasks_list` (`list_all`: "Your tasks" first, note id 0),
  event `yap-tasks-changed {notes}`. The local API and MCP list the added
  ones too.
- **`notes/retention.rs`** (2026-10-25) — **"Delete meeting transcripts
  after"** (`meeting_transcript_days`, Privacy & data; 0 = never, 30 / 90 /
  365): `due` (pure: notes with lines whose newest is older; never busy ones)
  → `strip` under `modify_quietly` (lines, digests, stops, speaker map out;
  `transcript_deleted_ts`; `updated_ts` untouched) → `meeting_audio::
  drop_audio` → `search::removed` (the deleted passages' vectors out of
  the index at its next look, seconds away, search by meaning on or off). At start (+90 s; 0.5 s in
  test mode), hourly, and on the setting set or shortened
  (`on_config_saved`). Events `yap-note-transcript-deleted`,
  `yap-notes-changed`. Unit-tested (`due`, `strip`).
- **Frontend** — `lib/core/noteEdits.js` (`editLine`, `taskToggled` + `setTask`,
  `saveSummary`, `askReplaceSummary`, `SUMMARY_KEPT`) and the pure helpers in
  `notepadText.js` (`lineRef`, `withLineEdit`, `taskText`, `withTaskMark`,
  `newSinceSummary`, `newSinceLine`; `transcriptItems` lines carry `edited`
  and `original`). A summary renders with `renderMarkdown(md, { tickable:
  true })` and its `change` maps the nth checkbox to `taskLines(md)[n]`
  (markdown.js); the tick shows at once (`withTaskMark`) and is put back if
  Rust refuses. `TasksView.svelte` — below.

## The Notes view

`src/lib/views/notes/` (split 2026-10-24): `NotesView.svelte` is the view —
it makes `notesState.svelte.js` (the notes and folders, the open note saved
as it's typed and kept in step with the notepad, the recorder, the actions,
the summary edited in place, the transcript's view and the note's chat),
follows Rust's events while Notes is open, and opens the note a call or the
Tasks view asks for. Its parts: `NoteList` (tools, folders, notes and their
menus), `NoteEditor` (title, tabs, the run line, the raw note) with
`NoteMeta` (the chip row), `NoteSummary` (rendered, ticked, edited),
`NoteTranscript` (the transcript box) and `NoteChat` (the thread, helpers,
the Ask bar) with `ActionPicker`; `ActionManager`; `noteFormat.js` (titles,
dates). [frontend.md](./frontend.md#the-folder-layout-2026-10-24).

- **`NotesView.svelte`** = the AI Notepad (OpenWhispr
  sidebar port: **New note / Quick note / Search notes / Actions** rows —
  **Quick note** brings up the quick-note window, above; **Search notes**
  (2026-10-25) opens "Search all notes…": every note, whatever its folder —
  its title, My thoughts, summary and transcript — holding every word typed
  (each at the start of a word, the last one half-typed too), and with
  "Search by meaning" on a few more that mean them ("by meaning"), ranked as
  Ask across meetings ranks; the results replace the folder's notes
  (`NoteSearch.svelte`, loaded the first time: the note, the line found with
  the words marked, where it is — "Transcript · 12:40 · Priya", "My
  thoughts", "Summary", the folder when it's another; ↓ / Enter from the
  box, Esc closes it) and a result opens its note **at the match**
  (`openRequested({…, find})`: the transcript line lit with its words
  marked, My thoughts with the words selected on their line, the Summary
  tab with them marked — `core/findWords.js`; `notes_search`,
  [search.md](./search.md#beyond-the-meetings-2026-10-25)); **FOLDERS with
  counts + NOTES list**; **right-click menus** (OpenWhispr's folder and note
  dropdowns, as context menus too: `ui/ContextMenu.svelte`, also from a row's
  hover **⋯** and the keyboard — the ContextMenu key, Shift+F10): a folder's
  **Rename** (in place: Enter or leaving it saves, Esc cancels) and
  **Delete…** (`ui/ConfirmDialog.svelte`: "Delete “Camping”?" · "Its 3 notes
  will move to Personal."; the view goes to Personal if it was on it),
  Personal and Meetings showing both off with the reason; a note's
  **Rename** (its title, focused), **Move to ▸** (the folders, this one
  ticked), **Read aloud** / Stop reading (`readNote`), **Copy as Markdown** (`noteMarkdown`), **Export…**,
  on a meeting note **Transcribe again…** (`meetingAudio.svelte.js`, asked
  first; off with the reason while it records or with no audio kept), **Delete…**
  (asked first; a meeting note goes through `meeting_delete`, which stops its
  recording first and makes the notepad let go, any other through
  `note_delete`); meta chip row shows date + attendees popover
  → add/remove participants, folder-move menu w/ New folder option, meeting
  **Record / ● 12:34 · Pause / Resume** chip (Pause = `meeting_pause`) and,
  on meeting notes, a **Notepad** chip (`notepad_open`), **🔊 Read aloud**
  (■ while it reads: the note as shown — its title, then the summary on
  the Summary tab, else the note — read with the reading card on the Yap
  bar, `voice/read_aloud.rs`; id `note:<id>`; it reads on if you leave Notes),
  export-to-markdown;
  ActionPicker split button + ActionManager dialog for custom actions +
  protected built-ins — while recording it becomes **End meeting &
  summarise** (`meeting_end` → Rust writes the Action Plan, shown here as it
  goes: the runline, the AI-setup card, the error; the Action Plan action and
  the **Action plan** button run the same Rust job via `meeting_summarise`);
  title + content saves send only the edited field with `origin: 'settings'`
  and follow `yap-note-changed` (the notepad's edits, the AI title; a made-up
  title shows muted); a reload of the open note (a recording's stop, the
  action plan written) keeps every field an event changed while its read was
  out (`changing` in NotesView.svelte, as in the notepad: a reply read
  before the notepad's edit would otherwise put the old text back, and the
  next keystroke here save it); **My thoughts ↔ Summary** tabs (the meeting notepad's
  names for the same two fields since 2026-10-22, also the list's "summary"
  tag and "## My thoughts" in an export; until then Raw / Enhanced) **+
  staleness dot**; on Summary, **Edit summary** / Done (its Markdown in a
  box, saved as typed; Escape ends it), "Edited" once it's the person's,
  "2 new lines since this summary · **Update**", tasks ticked in place, and
  every rewrite of an edited summary (Action plan, an action, Update) asked
  first (`askReplaceSummary`); since 2026-10-25 "Edited · **Back to Yap's
  summary**" with Undo, **Ctrl+F** to find in it, **👍 👎** under it and
  **Copy for email** beside Copy as Markdown / text (the notepad's, shared
  components: [meetings.md](./meetings.md#frontend)); the note's Ask thread
  is the meeting's kept one (`loadChat` on select, `keepChat` per turn,
  "New chat" / "Past chats" over it, the notepad's turns followed through
  `yap-note-chat`); a run the person outpaced says "Your summary
  was kept" (the safe
  renderer in `lib/core/markdown.js`, shared with chat answers, the notepad, the
  Voice Assistant's card and update notes — see "The markdown renderer"
  below) with **Copy as Markdown / Copy as text** (`markdownToText`:
  ☐ tasks, plain headings); embedded per-note **"Ask anything…" bar** = Chat
  scope grounded in the note, with mic button for in-box dictation, and on a
  meeting note the **one-tap helper chips** over it (`meetingHelpers` from
  `notepadText.js`: Questions to ask while it records, Follow-up email and
  Gaps and risks after → `meeting_assist`, answered in the thread with a
  Copy button; no AI → a toast with Open Language Models; since 2026-10-29
  Make me sound smart while it records and Make notes longer once there's
  a summary — asked first over an edited one, Undo in its toast —, "/" for
  the Skills that run on one meeting (`meeting_skill`; the placeholder
  stays "Ask anything…", its tooltip says "Type / for Skills"), and an
  answer's cited lines as pills that open the line here (`showLine`) or
  the Summary / My thoughts tab: [meetings.md](./meetings.md#frontend)); live
  **meeting transcript bubbles** under their speaker's name, once per run of
  one voice (`transcriptItems`; the name opens the speaker menu, below; follow
  the newest line; ▶ beside a bubble on hover, and Play this line in its
  menu, plays its stretch of the meeting's kept audio — `meetings/meeting_audio.rs`;
  a line's words fixed by double-click, Enter or F2 on it, or **Edit line**
  in its menu (Enter saves, Escape cancels, leaving saves; "edited" after
  it, what Yap heard in its tooltip; `notes/note_edits.rs`); opened from the
  Tasks view at a line, or from a passage an answer cites or a moment the
  Meetings view's search found ([search.md](./search.md)), that line scrolled
  to, focused and lit a moment (`noteRequest {id, line, ts, tab}`: `ts` finds
  the line again if the transcript was done again since; `tab` opens a
  summary passage on the Summary tab);
  over them "Transcribing again…" or, after a crash, "Yap closed while this
  meeting was recording…" with Transcribe it; echo lines
  hidden behind a "Show N lines…" toggle; a `dictated` marker is a quiet dashed
  "You dictated here · left out of the notes" divider with a mic glyph), an
  **AI notes so far** view of the
  rolling digests ("AI notes up to 40:12"), an **Action plan** button on
  finished meetings, a "Writing your action plan… / Catching up on the meeting
  — part 3 of 12" line, and, without an AI model, a "Your meeting is saved…"
  card linking to Settings → Language Models); the Home feed has **Ctrl+K
  search**.

## The Tasks view

- **`TasksView.svelte`** (sidebar **Tasks**, after Meetings: its own entry
  rather than a part of Meetings, as tasks outlive the meeting and are
  looked for on their own) = every task in notes' summaries (`tasks_list`):
  **By meeting** (newest first; the person's own first in each; the title
  opens the note) or **By person** (You, names A–Z, Everyone, Unassigned,
  No owner; each with its meeting) — the choice kept in this webview's
  `localStorage` (`yapTasksGroup`); "N open"; **Show done**. A row: its
  checkbox (labelled by the task), the task, the owner chip (yours
  highlighted), "Due Friday", and **Said at 12:40** (`tasks::moment`) →
  the note in Notes at that line. A tick is `note_task_set` (origin
  `tasks`), shown at once and put back on a refusal; a task ticked here
  stays listed, struck through, until the view closes (an easy undo); the
  list follows `yap-note-changed` / `yap-notes-changed` / `yap-tasks-changed`
  / a summary written.
  Empty: "No tasks yet" (what fills it), or "Nothing open". Since
  2026-10-25: **Add a task** at the top (no meeting: the "Your tasks" group,
  first; `task_add`), ✕ on an added task (`task_remove`), and **Archive done
  (N)** with Undo (`tasks_archive`: archived tasks aren't listed; a summary's
  stays ticked). A meeting note's **Tasks** tab (the Notes view's
  NoteEditor, the notepad): `components/MeetingTasks.svelte`.

## The markdown renderer

- **`lib/core/markdown.js`** — `renderMarkdown(md, {sources})` → safe HTML for
  notes' Summary tab, chat answers (Chat and a note's Ask bar), the notepad's
  Summary and Ask sheet, the Voice Assistant's bar card, and update notes
  (toast, Settings → About). Since 2026-10-21 the parser is **markdown-it**
  15.0.3 (MIT: CommonMark + GitHub's tables and `~~strikethrough~~`, bare
  links via linkify-it) and its HTML goes through **DOMPurify** 3.4.17
  (MPL-2.0 or Apache-2.0); both pinned exact in package.json. Until then it
  was a hand-written parser, and each way a model wrote markdown broke it in
  turn (blank lines between list items, tables, `<https://…>` links).
  **Safety, twice over**: markdown-it never passes HTML through (`html:
  false`: a model's `<img onerror=…>` or `<script>` shows as text), only
  `http(s)://` and `mailto:` become links (`validateLink`; `javascript:`,
  `data:`, `file:`, relative and `ftp:` stay text), images aren't rendered
  (a model's `![x](url)` would make Yap fetch it), and DOMPurify then keeps
  only the tags, attributes and classes the renderer makes (`PURIFY`,
  `CLASSES`; an `href` only on `a` and only web/mail, `start` only on `ol`,
  the one `input` a disabled checkbox; no `style`, no handlers, no `data-`
  or other `aria-` attributes). DOMPurify needs a DOM: in a test outside a
  page (`e2e/markdown.spec.js`) jsdom stands in, and without one it throws
  rather than return unsanitised HTML. **Yap's own rules** (markdown-it
  rules in the file): links are `a.md-link` with the address as their
  `title`, opened in the browser by main.js's click handler
  (`open_external`); **a web answer** (`options.sources`, chat/chats.rs): "[1]" /
  "[2, 3]" become `span.md-cites > a.md-link.md-cite` pills titled with the
  source (not a link's own text or inline code; an unknown number stays
  text), and only the sources' own links stay links (compared after
  markdown-it's normalising, a trailing slash or `#part` aside) — any other
  address shows as text; headings shift down a level (`#` → h2 … `####` and
  below → h5: the notes' and notepad's CSS styles them so); `- [ ]` / `- [x]`
  in a bullet list → `li.task` with a checkbox named by the task's words
  (`aria-label`, the one attribute an `input` may carry besides type,
  checked and disabled), disabled except with `options.tickable` (a note's
  Summary in the Notes view and the notepad: the checkbox is enabled, and
  `taskLines(md)` gives each checkbox's source line in render order, from
  the same parse — `notes/tasks.rs` ticks that line); tables in
  `div.md-table` (scrolls sideways) with the rule line's `:--:` / `--:` as
  classes `c` / `r`, ending at a line without a pipe (GitHub's spec would
  make the next sentence a row); fenced and indented code → `pre.md-code >
  code` (no highlighting), a fence an answer cut off still shows;
  `blockquote.md-quote` and `hr.md-rule`; single line breaks kept
  (`breaks: true`); "• " lines are bullets; a change of bullet mark ("- " then
  "* ") doesn't end a list; an ordered list keeps its start (`start="3"`).
  **Lists stay tight**: models write "1. …\n\n2. …", a CommonMark "loose"
  list whose items would each be a spaced-out paragraph (and until
  2026-10-21 showed "1." on every item): an item holding one paragraph
  renders without its `<p>`, one with several is `li.md-loose`, whose first
  and last `<p>` app.css trims. Styles for `.md-table`, `pre.md-code`,
  `a.md-link`, `.md-quote`, `.md-rule` live in app.css (quotes and rules in
  the text's own colour, for light views, dark toasts and the bar alike);
  each view sets its own paragraph, list and heading spacing, and Chat its
  pills. `markdownToText` (Copy as text) is line-based: ☐ / ☑ tasks, • bullets,
  plain headings, no bold or code marks. **`emailHtml(md, {title})`** /
  **`emailText`** (2026-10-25, "Copy for email"): the same safe render made
  mail-shaped — the title as an `h2`, tasks as ☐ / ☑ text (a mail client
  has no checkboxes), a table without its scroller, inline styles and no
  classes or colours (the reader's mail client sets them) — beside its
  plain-text twin, both on the clipboard in one `ClipboardItem`. Exports
  (Save as .md — `notes::commands::export_markdown`, pure —, Copy, Email)
  carry talk only: never an echo line, nor a mic-change marker. Tests: `e2e/markdown.spec.js`
  (Node + jsdom: lists, tables, links, raw HTML, hostile input against the
  allowlist, citations), plus the rendered views in chat-model, web-search
  (`16-chat-rich-web-answer`), notepad (`46-summary-rich-markdown`,
  `47-notes-enhanced-rich-markdown`) and meeting-summary specs. Bundle cost:
  ~134 kB minified / ~54 kB gzipped on the main chunk.
