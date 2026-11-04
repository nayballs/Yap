// What a meeting's summary offers beside its words, in the notepad and the
// Notes view alike (src-tauri/src/notes/note_edits.rs):
// - "Back to Yap's summary" (`canRestore`, `restoreSummary`): Yap keeps its
//   own version beside one the person edited; going back to it keeps their
//   ticks, and the toast's Undo saves their version back;
// - 👍 / 👎 and "What was wrong?" (`rateSummary`): kept in the note on this
//   PC, never sent;
// - "Copy for email" (`copySummaryForEmail`): the summary as rich HTML and
//   plain text on the clipboard at once (markdown.js `emailHtml`).
// The pure parts are summaryState.js and markdown.js's `emailHtml`,
// unit-tested in e2e/notes-summary.spec.js and e2e/markdown.spec.js.
import { invoke } from '@tauri-apps/api/core';
import { emailHtml, emailText } from './markdown.js';
import { saveSummary } from './noteEdits.js';
import { applySummaryState } from './summaryState.js';
import { toast } from '../ui/toast.svelte.js';

export { canRestore, applySummaryState } from './summaryState.js';

/**
 * "Back to Yap's summary": Yap's version over the person's, and a toast
 * with Undo (their version saved back). `note()` is the note shown now
 * (the answer lands only if it's still the same one); `changing()` marks
 * the summary changed for the window's reloads.
 */
export async function restoreSummary(note, origin, { current = () => note, changing = () => {} } = {}) {
  const id = note.id;
  let r;
  try {
    r = await invoke('note_summary_restore', { id, origin });
  } catch (e) {
    toast({ title: "Couldn't go back to Yap's summary", description: String(e), variant: 'destructive' });
    return;
  }
  const now = current();
  if (now?.id === id) {
    changing();
    applySummaryState(now, r.state);
  }
  toast({
    kind: 'done',
    chip: 'Summary',
    title: "Back to Yap's summary",
    description: 'Tasks you ticked stay ticked.',
    duration: 8000,
    action: {
      label: 'Undo',
      onClick: async () => {
        try {
          const s = await saveSummary(id, r.previous, origin);
          const n = current();
          if (n?.id === id) {
            changing();
            applySummaryState(n, s);
          }
        } catch (e) {
          toast({ title: "Couldn't put your summary back", description: String(e), variant: 'destructive' });
        }
      },
    },
  });
}

/** 👍 ('up') / 👎 ('down') / null (taken back), with what was wrong; resolves to the summary's state. */
export function rateSummary(id, rating, why, origin) {
  return invoke('note_summary_rate', { id, rating, why: why || null, origin });
}

/** The summary as the email's two formats: `{ html, text }`. */
export function summaryForEmail(note) {
  const title = note?.title?.trim() || '';
  const md = note?.enhancedContent || '';
  return { html: emailHtml(md, { title }), text: emailText(md, { title }) };
}

/**
 * "Copy for email": the summary as rich HTML (headings, lists, tasks as
 * ☐ / ☑) and plain text on the clipboard together — a mail client pastes
 * the HTML, a chat box the text. Falls back to the text alone. Resolves
 * true when it copied.
 */
export async function copySummaryForEmail(note) {
  if (!note?.enhancedContent?.trim()) return false;
  const { html, text } = summaryForEmail(note);
  try {
    await navigator.clipboard.write([
      new ClipboardItem({
        'text/html': new Blob([html], { type: 'text/html' }),
        'text/plain': new Blob([text], { type: 'text/plain' }),
      }),
    ]);
    return true;
  } catch {
    try {
      await navigator.clipboard.writeText(text);
      return true;
    } catch {
      return false;
    }
  }
}
