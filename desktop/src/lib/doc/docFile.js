// A Markdown file's window (DocWindow.svelte, notes/md_file.rs): what the
// page does when it reads the file again, and the words it shows. Pure.

/** How often the page looks at the file on disk (its modified time and size). */
export const POLL_MS = 1500;
/** Saved this long after the last change. */
export const SAVE_MS = 600;

/**
 * The file read again (its stamp changed, or a save found it changed):
 * what to do with `read.text`, given what the page holds — `text` (on the
 * page now), `base` (the file as the page last read or wrote it) and
 * `dirty` (typed since, not saved).
 *
 * - 'same': the file holds what the page does (the edits are in it): in step.
 * - 'reload': nothing unsaved here: the file's text in place.
 * - 'touched': the file's words are what the page last had (only its time
 *   changed): the edits here stay, saved as before.
 * - 'conflict': changed there and here: the person picks (Reload · Keep mine).
 */
export function afterRead({ text, base, dirty }, read) {
  if (read === text) return 'same';
  if (!dirty) return 'reload';
  if (read === base) return 'touched';
  return 'conflict';
}

/** The footer's line for a save state. */
export function statusLine(state) {
  switch (state) {
    case 'saving':
      return 'Saving…';
    case 'saved':
      return 'Saved';
    case 'unsaved':
      return 'Not saved';
    case 'readonly':
      return 'Read-only';
    default:
      return '';
  }
}

/** "UTF-8 · CRLF": how the file is written (kept as it is). */
export function formLine(loaded) {
  return [loaded?.encoding, loaded?.eol].filter(Boolean).join(' · ');
}
