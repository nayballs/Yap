# Notes

The AI Notepad: the notes store, folders, the Actions engine, notes you can fix (a line's words, the summary, tasks), and the Notes and Tasks views. Meeting notes' recording and summaries are [meetings.md](./meetings.md).

[Architecture index](./README.md) · [CLAUDE.md](../../CLAUDE.md)

## `notes.rs`

- **`notes.rs`** — the AI Notepad's data layer (`notes.json`, camelCase). Stores:
  `content` (raw markdown, never overwritten by AI: the **My thoughts** tab),
  `enhanced_content` (the **Summary** tab; its tasks' ticks are its own
  `- [x]`, `notes/tasks.rs`), `enhanced_at_hash` (OpenWhispr's
  `len+first-50` staleness marker), `summary_edited_ms` (the person changed
  the summary's words: Yap never writes over it unasked) and `summary_ts`
  (when the summary's input was read: "New since this summary"; both 0 and
  unwritten on older notes, `notes/note_edits.rs`), `folder` (string, seeded with Personal +
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
  audio with it (`meeting_audio::forget`). **Saves** go through `SAVER`
  (`shell/saving.rs`, [config-and-data.md](./config-and-data.md)): a failed one
  keeps the notes in memory and is written with the next change; the
  commands that change notes return it as their error (`saved()`: "Couldn't
  save your note: the disk is full."), which the Notes view and the notepad
  show with Try again (`saveErrors.js`), and a meeting's lines that couldn't
  be saved are said once by the notice. Unit tests get an in-memory store per test
  thread (`cfg(test)`), never `notes.json` (saved only to a scratch file a
  test names, `TEST_PATH`). `event` (optional) = the calendar meeting the note
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
  moved note + `yap-notes-changed`. **Actions** (named
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
  time (seconds) and keeps the old summary's ticks.
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
  sidebar port: **New note / Search notes / Actions** rows; **FOLDERS with
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
  first (`askReplaceSummary`); a run the person outpaced says "Your summary
  was kept" (the safe
  renderer in `lib/core/markdown.js`, shared with chat answers, the notepad, the
  Voice Assistant's card and update notes — see "The markdown renderer"
  below) with **Copy as Markdown / Copy as text** (`markdownToText`:
  ☐ tasks, plain headings); embedded per-note **"Ask anything…" bar** = Chat
  scope grounded in the note, with mic button for in-box dictation, and on a
  meeting note the **one-tap helper chips** over it (`meetingHelpers` from
  `notepadText.js`: Questions to ask while it records, Follow-up email and
  Gaps and risks after → `meeting_assist`, answered in the thread with a
  Copy button; no AI → a toast with Open Language Models); live
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
  list follows `yap-note-changed` / `yap-notes-changed` / a summary written.
  Empty: "No tasks yet" (what fills it), or "Nothing open".

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
  plain headings, no bold or code marks. Tests: `e2e/markdown.spec.js`
  (Node + jsdom: lists, tables, links, raw HTML, hostile input against the
  allowlist, citations), plus the rendered views in chat-model, web-search
  (`16-chat-rich-web-answer`), notepad (`46-summary-rich-markdown`,
  `47-notes-enhanced-rich-markdown`) and meeting-summary specs. Bundle cost:
  ~134 kB minified / ~54 kB gzipped on the main chunk.
