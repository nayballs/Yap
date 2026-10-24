// Fixing meeting notes by hand, for the meeting notepad and the Notes view
// alike (src-tauri/src/note_edits.rs and tasks.rs):
// - a transcript line's words (`editLine`: double-click or Edit line; the
//   line keeps its time, voice and audio, is marked edited, and a misheard
//   word or name it fixed is offered for the dictionary, here, by
//   "Suggest words from your corrections": `askWordSuggestion`);
// - the summary's words (`saveSummary`); an edited summary is replaced only
//   after `askReplaceSummary` says so;
// - the summary's tasks, ticked in place (`taskToggled` + `setTask`, with
//   notepadText.js `withTaskMark` for the instant change on screen).
// The pure parts (`lineRef`, `withLineEdit`, `taskText`, `withTaskMark`,
// `newSinceSummary`) live in notepadText.js, unit-tested in
// e2e/notes-edit.spec.js.
import { invoke } from '@tauri-apps/api/core';
import { askConfirm } from './ui/contextMenu.svelte.js';
import { askWordSuggestion } from './wordSuggestions.js';
import { taskLines } from './markdown.js';
import { lineRef, taskText } from './notepadText.js';

/** What a summary run says when it left the person's edited summary alone (note_edits.rs). */
export const SUMMARY_KEPT = "Your edited summary was kept: Yap didn't write over it.";

/**
 * Save line `i`'s new words. Resolves to `{ index, line, changed }` (the
 * line as stored); a dictionary offer, when the edit fixed a misheard word,
 * shows as this window's toast. Throws with Rust's reason.
 */
export async function editLine(noteId, transcript, i, text, origin) {
  const r = await invoke('transcript_line_edit', { noteId, line: lineRef(transcript, i), text, origin });
  if (r?.offer) askWordSuggestion(r.offer);
  return r;
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
