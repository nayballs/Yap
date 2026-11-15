// **A document's outline, edited before the writing** (OutlineCard.svelte,
// chat/cards/outline.rs): the draft the card's Edit works on, and the answer
// it sends. Pure, so the e2e suite checks it with no app.

/** The most sections a document has (documents.rs `MAX_SECTIONS`). */
export const MAX_SECTIONS = 12;

let nextKey = 1;

/** An editable copy of `sections` (`[{key, intent}]`), each row with an id of its own for the list. */
export function draftOf(sections = []) {
  return (sections || []).map((s) => ({ id: nextKey++, key: s.key || '', intent: s.intent || '' }));
}

/** A new, empty row. */
export const blankRow = () => ({ id: nextKey++, key: '', intent: '' });

/** `rows` with row `i` moved by `by` (−1 up, +1 down), kept inside the list. */
export function moved(rows, i, by) {
  const j = i + by;
  if (i < 0 || i >= rows.length || j < 0 || j >= rows.length) return rows;
  const out = rows.slice();
  [out[i], out[j]] = [out[j], out[i]];
  return out;
}

/** The rows that will be written: a heading each (trimmed, a leading "#" gone), once, at most MAX_SECTIONS. */
export function sectionsOf(rows = []) {
  const out = [];
  for (const r of rows) {
    const key = (r.key || '').trim().replace(/^#+\s*/, '').trim();
    if (!key || out.some((s) => s.key.toLowerCase() === key.toLowerCase())) continue;
    out.push({ key, intent: (r.intent || '').trim() });
    if (out.length === MAX_SECTIONS) break;
  }
  return out;
}

/** Whether `rows` say something different from `sections` (the model's). */
export function changedFrom(rows, sections = []) {
  const now = sectionsOf(rows);
  return now.length !== sections.length || now.some((s, i) => s.key !== sections[i].key || s.intent !== (sections[i].intent || ''));
}

/**
 * **What Edit's Go sends** (`chat_outline_answer`'s `outline`): `{sections, note}`, or `null` when nothing changed
 * and there's no note (a plain Go). Nothing left to write: `undefined` (the card won't send it).
 */
export function answerOf(rows, sections, note = '') {
  const now = sectionsOf(rows);
  if (!now.length) return undefined;
  const words = (note || '').trim();
  if (!words && !changedFrom(rows, sections)) return null;
  return { sections: now, note: words };
}

/** What the card says once answered, under the headings. */
export function endingOf(state, outline = {}) {
  const changed = (outline.changed || []).length > 0;
  return (
    {
      allowed: 'Written as outlined',
      changed: 'Written with your changes',
      redo: 'You asked for changes: a new outline follows',
      refused: 'Not written',
      timedOut: 'Not answered in time: written as outlined',
    }[state] || (changed ? 'Written with your changes' : '')
  );
}
