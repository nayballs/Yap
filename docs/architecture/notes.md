# Notes

The AI Notepad: the notes store, folders, the Actions engine, and the Notes view. Meeting notes' recording and summaries are [meetings.md](./meetings.md).

[Architecture index](./README.md) · [CLAUDE.md](../../CLAUDE.md)

## `notes.rs`

- **`notes.rs`** — the AI Notepad's data layer (`notes.json`, camelCase). Stores:
  `content` (raw markdown, never overwritten by AI), `enhanced_content` (the
  Enhanced tab), `enhanced_at_hash` (OpenWhispr's `len+first-50` staleness
  marker), `folder` (string, seeded with Personal + Meetings), `participants`
  (attendee names, shown as chips and fed to prompts for attribution),
  `transcript` (meeting-recorder segments, time-ordered; `source` = the side,
  "you" (mic) | "them" (call); `speaker` = the voice (`speakers.rs`; empty =
  the side's own, read through `speaker_id()` — every old note reads as
  You/Them), `speaker_locked` (the user said who this line is), `start_ms`/
  `end_ms` (place on that recording's clock, ms; none on old lines and
  markers), `words` (`[start, end]` ms from `start_ms` per word of `text`,
  when the model gave times); `echo` marks
  speaker bleed, `dictated` a "You dictated here" marker — no text, `ts` where
  a mid-meeting dictation began; `is_talk()` = neither, what every summary
  input reads), `speakers` (the speaker map, see `speakers.rs`), `digests` (rolling meeting digests, see `meeting_summary.rs`),
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
  (read and write in one step — the chat's `edit_note`; only a change bumps
  `updated_ts` and saves). Unit tests get an in-memory store per test
  thread (`cfg(test)`), never `notes.json`. `event` (optional) = the calendar meeting the note
  belongs to (`NoteEvent`: key, title, start/end, the invite's description,
  service), set by `link_event` (also the meeting's title as a real title,
  attendees merged, `note_type` meeting) and found by `find_by_event`.
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
  (commands.rs) invokes the **Actions engine**: picks an action by id, runs the
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
  recording ends (`meeting_end.rs`: anything but Pause), through
  `commands::run_enhance` (the body of `note_enhance`, with step reporting).

## The Notes view

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
  ticked), **Copy as Markdown** (`noteMarkdown`), **Export…**, **Delete…**
  (asked first; a meeting note goes through `meeting_delete`, which stops its
  recording first and makes the notepad let go, any other through
  `note_delete`); meta chip row shows date + attendees popover
  → add/remove participants, folder-move menu w/ New folder option, meeting
  **Record / ● 12:34 · Pause / Resume** chip (Pause = `meeting_pause`) and,
  on meeting notes, a **Notepad** chip (`notepad_open`), export-to-markdown;
  ActionPicker split button + ActionManager dialog for custom actions +
  protected built-ins — while recording it becomes **End meeting &
  summarise** (`meeting_end` → Rust writes the Action Plan, shown here as it
  goes: the runline, the AI-setup card, the error; the Action Plan action and
  the **Action plan** button run the same Rust job via `meeting_summarise`);
  title + content saves send only the edited field with `origin: 'settings'`
  and follow `yap-note-changed` (the notepad's edits, the AI title; a made-up
  title shows muted); **Raw ↔ Enhanced dual-view + staleness dot** (the safe
  renderer in `lib/markdown.js`, shared with chat answers, the notepad, the
  Voice Assistant's card and update notes — see "The markdown renderer"
  below) with **Copy markdown / Copy text** (`markdownToText`:
  ☐ tasks, plain headings); embedded per-note **"Ask anything…" bar** = Chat
  scope grounded in the note, with mic button for in-box dictation, and on a
  meeting note the **one-tap helper chips** over it (`meetingHelpers` from
  `notepadText.js`: Questions to ask while it records, Follow-up email and
  Gaps and risks after → `meeting_assist`, answered in the thread with a
  Copy button; no AI → a toast with Open Language Models); live
  **meeting transcript bubbles** under their speaker's name, once per run of
  one voice (`transcriptItems`; the name opens the speaker menu, below; follow
  the newest line; echo lines
  hidden behind a "Show N lines…" toggle; a `dictated` marker is a quiet dashed
  "You dictated here · left out of the notes" divider with a mic glyph), an
  **AI notes so far** view of the
  rolling digests ("AI notes up to 40:12"), an **Action plan** button on
  finished meetings, a "Writing your action plan… / Catching up on the meeting
  — part 3 of 12" line, and, without an AI model, a "Your meeting is saved…"
  card linking to Settings → Language Models); the Home feed has **Ctrl+K
  search**.

## The markdown renderer

- **`lib/markdown.js`** — `renderMarkdown(md, {sources})` → safe HTML for the
  Enhanced notes, chat answers (Chat and a note's Ask bar), the notepad's
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
  (`open_external`); **a web answer** (`options.sources`, chats.rs): "[1]" /
  "[2, 3]" become `span.md-cites > a.md-link.md-cite` pills titled with the
  source (not a link's own text or inline code; an unknown number stays
  text), and only the sources' own links stay links (compared after
  markdown-it's normalising, a trailing slash or `#part` aside) — any other
  address shows as text; headings shift down a level (`#` → h2 … `####` and
  below → h5: the notes' and notepad's CSS styles them so); `- [ ]` / `- [x]`
  in a bullet list → `li.task` with a disabled checkbox; tables in
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
  pills. `markdownToText` (Copy text) is line-based: ☐ / ☑ tasks, • bullets,
  plain headings, no bold or code marks. Tests: `e2e/markdown.spec.js`
  (Node + jsdom: lists, tables, links, raw HTML, hostile input against the
  allowlist, citations), plus the rendered views in chat-model, web-search
  (`16-chat-rich-web-answer`), notepad (`46-summary-rich-markdown`,
  `47-notes-enhanced-rich-markdown`) and meeting-summary specs. Bundle cost:
  ~134 kB minified / ~54 kB gzipped on the main chunk.
