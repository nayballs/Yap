// A note's page in the app (desktop/src/lib/components/richtext/RichNote.svelte: the
// rich text editor notes' My thoughts, the meeting notepad and the quick
// note are edited on, 2026-10-31). The page answers as the note's text box
// did — `value` (its markdown), `selectionStart` / `End`,
// `setSelectionRange` — so a spec reads and places the caret the same way
// in the page, its Source box or the plain box that stands in while the
// page loads.
import { expect } from '@playwright/test';

/** What the note's page (or box) holds: its markdown. */
export function noteText(box) {
  return box.evaluate((el) => el.value);
}

/** The page (or box) holds `want` — the text, or a pattern it matches — once it's saved there. */
export async function expectText(box, want, options = {}) {
  const poll = expect.poll(() => noteText(box), options);
  await (want instanceof RegExp ? poll.toMatch(want) : poll.toBe(want));
}

/** The note's page is in (the editor, not the plain box standing in for it while it loads). */
export async function pageIn(box) {
  await expect(box).toHaveAttribute('contenteditable', 'true', { timeout: 15_000 });
}

/**
 * What the page (or box) holds replaced with `text`, typed as a person
 * would: each line break a Shift+Enter, a line break of its own (as the
 * plain box held them; Enter would start a paragraph).
 */
export async function fillLines(box, text) {
  await box.focus();
  await box.press('Control+a');
  const lines = String(text).split('\n');
  if (!text) await box.press('Delete');
  for (const [i, line] of lines.entries()) {
    if (i) await box.press('Shift+Enter');
    if (line) await box.page().keyboard.insertText(line);
  }
}

/** The caret (or a selection) at `start`..`end` of the note's markdown, the page focused. */
export async function selectText(box, start, end = start) {
  await box.evaluate(
    (el, [a, b]) => {
      el.focus();
      el.setSelectionRange(a, b);
    },
    [start, end]
  );
}

/**
 * The caret to the end of its line (End), and a moment for the editor to
 * take it in: a key the editor acts on itself (Enter) goes where its own
 * selection is, which follows the page's a beat later.
 */
export async function toLineEnd(page) {
  await page.keyboard.press('End');
  await page.waitForTimeout(80);
}
