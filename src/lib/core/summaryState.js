// A meeting summary's fields beside its words, as the windows keep them
// (src-tauri/src/notes/note_edits.rs `SummaryState`): whether "Back to Yap's
// summary" is offered, and Rust's answer laid onto the note shown. Pure
// (unit-tested in e2e/notes-summary.spec.js); summaryTools.js acts on them.

/** A summary's words with every tick taken off (a tick isn't an edit). */
const unticked = (md) => (md || '').replace(/^(\s*[-*+•][ \t]+\[)[xX](\])/gm, '$1 $2').trim();

/** Whether the note offers "Back to Yap's summary": the person edited Yap's, and Yap's says something else. */
export function canRestore(note) {
  return !!note?.summaryEditedMs && !!note.summaryAi?.trim() && unticked(note.summaryAi) !== unticked(note.enhancedContent);
}

/** A summary's fields as Rust says them (a `SummaryState`, or `yap-note-changed`'s), onto `note`. */
export function applySummaryState(note, s) {
  if (!note || !s) return;
  if (typeof s.enhancedContent === 'string') note.enhancedContent = s.enhancedContent;
  if (typeof s.summaryEditedMs === 'number') note.summaryEditedMs = s.summaryEditedMs;
  if (typeof s.summaryTs === 'number') note.summaryTs = s.summaryTs;
  if ('summaryAi' in s) note.summaryAi = s.summaryAi || '';
  if ('summaryRating' in s) note.summaryRating = s.summaryRating ?? null;
}
