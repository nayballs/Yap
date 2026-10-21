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
  title shows muted); **Raw ↔ Enhanced dual-view + staleness dot** (safe renderer
  in `lib/markdown.js`, shared with chat answers and the notepad: everything
  HTML-escaped first; headings, lists, tasks, bold/italic, inline code, fenced
  code blocks `pre.md-code`, `[text](url)` and bare links `a.md-link` — web and
  mail only, styles in app.css) with **Copy markdown / Copy text** (`markdownToText`:
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
