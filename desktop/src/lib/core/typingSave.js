// When a note's typing is saved (the Notes view's My thoughts and title, the
// meeting notepad's, the quick note): once the typing pauses, and never later
// than `MOST_MS` after the first change not saved yet. Only a pause, and a
// crash (Yap killed, the PC's power) while someone typed on without one lost
// everything since the last pause: a paragraph, a meeting's notes typed
// fast. Now at most the last two seconds. Each save is notes.json written
// whole and atomically (shell/saving.rs), read back as it is at the next
// start; the note's versions fold saves this close together into one
// (notes/versions.rs).

/** The pause after the last key that saves (ms). */
export const PAUSE_MS = 600;
/** The longest typing goes unsaved while it goes on (ms). */
export const MOST_MS = 2000;

/**
 * How long a save waits from `now`: `pause` after this key, but no later than `MOST_MS` after `since`, the first
 * change not saved yet (pure).
 */
export function saveDelay(since, now = Date.now(), pause = PAUSE_MS) {
  return Math.max(0, Math.min(pause, since + MOST_MS - now));
}
