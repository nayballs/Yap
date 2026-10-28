// The quick note's pure helpers (QuickNote.svelte; notes/quick_note.rs): what
// a dictation puts at the cursor, and the window's status line. No Svelte
// state, so the e2e suite can check them in Node.

/**
 * The text a dictation puts between `before` and `after` (the note's text
 * either side of the cursor or selection): its words with a space before
 * them when they'd touch a word, a space after when a word follows (not
 * before punctuation), and a new line after when "press enter" was said
 * (`newline`). Empty when there's nothing to put.
 */
export function dictationPiece(before, after, words, newline = false) {
  const text = String(words || '').trim();
  if (!text) return newline ? '\n' : '';
  const lead = before && !/\s$/.test(before) ? ' ' : '';
  if (newline) return `${lead}${text}\n`;
  const trail = after && !/^[\s.,;:!?)\]]/.test(after) ? ' ' : '';
  return `${lead}${text}${trail}`;
}

/** `value` with `piece` in place of `start`..`end`, and where the cursor goes after it. */
export function withPiece(value, start, end, piece) {
  const s = Math.max(0, Math.min(start ?? value.length, value.length));
  const e = Math.max(s, Math.min(end ?? s, value.length));
  return { value: value.slice(0, s) + piece + value.slice(e), caret: s + piece.length };
}

/**
 * The status line under the note: where it's kept, or that it's being
 * saved, or why it isn't. `state`: 'new' | 'saving' | 'saved' | 'unsaved'.
 */
export function statusLine(state, folder) {
  if (state === 'saving') return 'Saving…';
  if (state === 'unsaved') return "Not saved yet: Yap will try again with your next change";
  if (state === 'saved') return folder ? `Saved in ${folder}` : 'Saved';
  return 'Saved as you type, in Notes';
}
