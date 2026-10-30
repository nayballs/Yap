// Fixing meeting notes by hand, for the meeting notepad and the Notes view
// alike (src-tauri/src/notes/note_edits.rs and notes/tasks.rs):
// - a transcript line's words (`editLine`: double-click or Edit line; the
//   line keeps its time, voice and audio, is marked edited, and a misheard
//   word or name it fixed is offered for the dictionary, here, by
//   "Suggest words from your corrections": `askWordSuggestion`);
// - the summary's words (`saveSummary`); an edited summary is replaced only
//   after `askReplaceSummary` says so; a name or term swapped in it is
//   offered for the dictionary at Done (`offerSummaryWords`);
// - the summary's tasks, ticked in place (`taskToggled` + `setTask`, with
//   notepadText.js `withTaskMark` for the instant change on screen).
// The pure parts (`lineRef`, `withLineEdit`, `taskText`, `withTaskMark`,
// `newSinceSummary`) live in notepadText.js, unit-tested in
// e2e/notes-edit.spec.js.
import { invoke } from '@tauri-apps/api/core';
import { askConfirm } from '../ui/contextMenu.svelte.js';
import { askWordSuggestion } from '../notices/wordSuggestions.js';
import { toast } from '../ui/toast.svelte.js';
import { showSaveError } from '../notices/saveErrors.js';
import { taskLines } from './markdown.js';
import { lineRef, taskText, withLineEdit, withLineSplit, breaksAfterSplit, withLineRemoved, withLineRestored } from './notepadText.js';

/** What a summary run says when it left the person's edited summary alone (notes/note_edits.rs). */
export const SUMMARY_KEPT = "Your edited summary was kept: Yap didn't write over it.";

/**
 * Save line `i`'s new words. Resolves to `{ index, line, changed }` (the
 * line as stored); a dictionary offer, when the edit fixed a misheard word,
 * shows as this window's toast. Throws with Rust's reason. `ref`: the line
 * as it was when its editing began (`lineRef`) — lines split or deleted
 * before it meanwhile move it, and its place now is another line's.
 */
export async function editLine(noteId, transcript, i, text, origin, ref = null) {
  const r = await invoke('transcript_line_edit', { noteId, line: ref ?? lineRef(transcript, i), text, origin });
  if (r?.offer) askWordSuggestion(r.offer);
  return r;
}

// ---- a line, the rest (notes/transcript_edits.rs): revert, delete with Undo, split ----
//
// Each takes the view's `ctx`: `{ note: () => the open note (its $state),
// origin, changing(...fields), reload() }`, changes the line on screen
// as Rust stored it, and says what went wrong in a toast.

/** Put this window's copy of line `index` as Rust says it is now (else reload). */
function applyLine(ctx, index, line) {
  const note = ctx.note();
  if (!note) return;
  const t = withLineEdit(note.transcript, { index, line });
  ctx.changing('transcript');
  if (t) note.transcript = t;
  else ctx.reload();
}

/** A failed save is said with Try again (the change itself happened). */
function failed(title, e, ctx) {
  if (showSaveError(e)) {
    ctx.reload();
    return;
  }
  toast({ title, description: String(e), variant: 'destructive' });
}

/** "Revert to what Yap heard" on line `i`. */
export async function revertLine(ctx, i) {
  const note = ctx.note();
  if (!note) return;
  try {
    const r = await invoke('transcript_line_revert', { noteId: note.id, line: lineRef(note.transcript, i), origin: ctx.origin });
    if (r?.changed && ctx.note()?.id === note.id) applyLine(ctx, r.index, r.line);
  } catch (e) {
    failed("Couldn't put the line back", e, ctx);
  }
}

/** This window's copy of the note with lines taken out or put back (`t`), and its stop marks; else reload. */
function applyLines(ctx, t, breaks) {
  const note = ctx.note();
  if (!note) return;
  ctx.changing('transcript', 'breaks');
  if (t) {
    note.transcript = t;
    if (Array.isArray(breaks)) note.breaks = breaks;
  } else ctx.reload();
}

/** "Delete line" on line `i`: gone at once, with Undo in the toast. */
export async function deleteLine(ctx, i) {
  const note = ctx.note();
  if (!note) return;
  const id = note.id;
  try {
    const r = await invoke('transcript_line_delete', { noteId: id, line: lineRef(note.transcript, i), origin: ctx.origin });
    const now = ctx.note();
    if (now?.id === id) applyLines(ctx, withLineRemoved(now.transcript, r.index, r.line), r.breaks);
    toast({
      title: 'Line deleted',
      description: 'Its words are gone from the transcript and from what Yap writes from it.',
      key: `line-deleted-${id}`,
      duration: 8_000,
      action: { label: 'Undo', onClick: () => restoreLine(ctx, id, r.index, r.line) },
    });
  } catch (e) {
    failed("Couldn't delete the line", e, ctx);
  }
}

/** Undo a deleted line (`line`, from `index`). */
async function restoreLine(ctx, noteId, index, line) {
  try {
    const r = await invoke('transcript_line_restore', {
      noteId,
      index,
      source: line.source,
      ts: line.ts,
      startMs: line.startMs ?? null,
      origin: ctx.origin,
    });
    const now = ctx.note();
    if (now?.id === noteId) applyLines(ctx, withLineRestored(now.transcript, r.index, r.line), r.breaks);
  } catch (e) {
    failed("Couldn't put the line back", e, ctx);
  }
}

/** "Split line" on line `i`, before word `at` (notepadText.js `splitWordAt`). */
export async function splitLine(ctx, i, at) {
  const note = ctx.note();
  if (!note) return false;
  const id = note.id;
  try {
    const r = await invoke('transcript_line_split', { noteId: id, line: lineRef(note.transcript, i), at, origin: ctx.origin });
    const now = ctx.note();
    if (now?.id === id) {
      const t = withLineSplit(now.transcript, r.index, r.lines);
      ctx.changing('transcript', 'breaks');
      if (t) {
        now.transcript = t;
        now.breaks = breaksAfterSplit(now.breaks, r.index);
      } else ctx.reload();
    }
    return true;
  } catch (e) {
    failed("Couldn't split the line", e, ctx);
    return false;
  }
}

/**
 * A `change` on a summary rendered with `renderMarkdown(md, { tickable:
 * true })`: `{ line, text, done }` for the checkbox ticked (the nth
 * checkbox is the task on `taskLines(md)[n]`), else null.
 */
export function taskToggled(e, md) {
  const box = e?.target;
  if (box?.tagName !== 'INPUT' || box.type !== 'checkbox') return null;
  const boxes = [...e.currentTarget.querySelectorAll('input[type="checkbox"]')];
  const line = taskLines(md)[boxes.indexOf(box)];
  if (line == null || line < 0) return null;
  const text = taskText(md, line);
  return text ? { line, text, done: box.checked } : null;
}

/** Tick a task in note `noteId`'s summary; resolves to the summary as stored. */
export function setTask(noteId, task, origin) {
  return invoke('note_task_set', { noteId, line: task.line, text: task.text, done: task.done, origin });
}

/** Save the summary's markdown; resolves to `{ enhancedContent, summaryEditedMs, summaryTs }`. */
export function saveSummary(id, text, origin) {
  return invoke('note_summary_update', { id, text, origin });
}

/**
 * The summary edited from `before` (at Edit) to `after` (at Done): a name
 * or term it swapped for another ("Shivon" → "Siobhan"; one or two clear
 * swaps, never a rewrite) is offered for the dictionary in this window, as
 * a fixed line's is (dictionary/learn.rs `offer_for_summary`). Best-effort.
 */
export function offerSummaryWords(before, after) {
  if (!before?.trim() || !after?.trim() || before === after) return;
  invoke('summary_word_offer', { before, after })
    .then((offer) => offer && askWordSuggestion(offer))
    .catch(() => {});
}

/**
 * Before Yap writes over a summary the person edited: "Replace your edited
 * summary?" (the window's ConfirmDialog). `run` starts the new summary
 * (with `replace: true`).
 */
export function askReplaceSummary({ run, returnFocus = null }) {
  askConfirm({
    title: 'Replace your edited summary?',
    body: 'Yap writes the summary again from the meeting and your notes, and the changes you made to it go. Tasks you ticked stay ticked.',
    confirmLabel: 'Replace summary',
    busyLabel: 'Starting…',
    danger: true,
    returnFocus,
    run,
  });
}
