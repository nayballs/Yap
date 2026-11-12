// **What was replaced inside a line** (12 Nov 2026, after Claude Code's own
// transcript, where a changed line shows its replaced part in a stronger red
// and what replaced it in a stronger green; the maintainer: "it highlights it
// in green then shows… what was replaced? can we have that as well?").
//
// A diff says a line went and a line came. When the two are the same line
// with a part changed, this finds the part: what the two lines share at
// their start and at their end is the same, and what lies between is what
// was replaced (one stretch a line, as Claude Code marks it; never a
// scatter of single characters).
//
// - **Whole words** (`wordChanges`): the shared start and end are counted in
//   words, numbers, runs of space and single marks, so a mark never stops
//   in the middle of a name.
// - **Only when it helps**: two lines with little in common ([`SHARED_MIN`]
//   of the longer one, spaces aside) are simply a line removed and a line
//   added, and get no mark.
// - **Which lines go together** (`pairChanges`): within a run of removed
//   lines followed at once by added ones — the same number, each with the
//   one at its place; a different number, each removed line with the most
//   alike added line after the last one taken.
// - **With the code's colours** (`linePieces`): a line's coloured tokens
//   (core/codeColours.js) cut at the marked stretch, so a piece has both its
//   colour and its mark.
//
// Pure. Used by the thread's rows (views/chat/StepRows.svelte).

/** A word or a number, a run of space, or one other character. */
const TOKEN = /[A-Za-z0-9_]+|\s+|[^A-Za-z0-9_\s]/g;
/** A closing bracket → its pair. */
const CLOSERS = { ')': '()', ']': '[]', '}': '{}' };
/** How much two lines must share, of the longer one's characters (spaces aside), for the rest to be marked. */
export const SHARED_MIN = 0.3;
/** The longest run of removed or added lines paired up (a longer one is left as it is). */
export const PAIR_MAX = 60;

const inked = (tokens) => tokens.reduce((n, t) => n + (/\S/.test(t) ? t.length : 0), 0);
const length = (tokens) => tokens.reduce((n, t) => n + t.length, 0);

/**
 * **The stretch of `before` that was replaced and the stretch of `after`
 * that replaced it**: `{ before: [from, to] | null, after: [from, to] |
 * null, shared }` — character places in each line (`null`: nothing there:
 * text only added, or only taken out), `shared` how alike they are (0–1).
 * `null` when the lines are the same, or share too little for a mark to
 * help.
 */
export function wordChanges(before, after) {
  const a = String(before ?? '');
  const b = String(after ?? '');
  if (a === b) return null;
  const ta = a.match(TOKEN) || [];
  const tb = b.match(TOKEN) || [];
  let start = 0;
  while (start < ta.length && start < tb.length && ta[start] === tb[start]) start++;
  let end = 0;
  while (end < ta.length - start && end < tb.length - start && ta[ta.length - 1 - end] === tb[tb.length - 1 - end]) end++;
  // A bracket opened inside both stretches closes inside them too: "trim()" and "contains(m)" each
  // keep their ")" rather than leave it to the shared end.
  const open = (tokens, pair) => tokens.reduce((n, t) => n + (t === pair[0] ? 1 : t === pair[1] ? -1 : 0), 0);
  while (end > 0) {
    const pair = CLOSERS[ta[ta.length - end]];
    if (!pair || open(ta.slice(start, ta.length - end), pair) <= 0 || open(tb.slice(start, tb.length - end), pair) <= 0) break;
    end--;
  }
  const most = Math.max(inked(ta), inked(tb));
  const shared = most ? (inked(ta.slice(0, start)) + inked(ta.slice(ta.length - end))) / most : 0;
  if (shared < SHARED_MIN) return null;
  const from = length(ta.slice(0, start));
  const stretch = (tokens, text) => {
    const to = text.length - length(tokens.slice(tokens.length - end));
    return to > from ? [from, to] : null;
  };
  return { before: stretch(ta, a), after: stretch(tb, b), shared };
}

/**
 * **A diff's rows with what was replaced marked** (`rows`: core/agents.js
 * `diffRows`): the same rows, a removed line and the added line that took
 * its place each with `changed: [[from, to]]` when they share enough.
 */
export function pairChanges(rows) {
  const out = (rows || []).map((r) => ({ ...r }));
  let i = 0;
  while (i < out.length) {
    if (out[i].mark !== 'del') {
      i++;
      continue;
    }
    let d = i;
    while (d < out.length && out[d].mark === 'del') d++;
    let a = d;
    while (a < out.length && out[a].mark === 'add') a++;
    const removed = d - i;
    const added = a - d;
    if (added && removed <= PAIR_MAX && added <= PAIR_MAX) {
      // Added lines not taken yet start here (a different number of each: in order, the most alike).
      let next = d;
      for (let r = i; r < d; r++) {
        let best = null;
        if (removed === added) {
          const c = wordChanges(out[r].text, out[d + (r - i)].text);
          if (c) best = { at: d + (r - i), c };
        } else {
          for (let k = next; k < a; k++) {
            const c = wordChanges(out[r].text, out[k].text);
            if (c && (!best || c.shared > best.c.shared)) best = { at: k, c };
          }
          if (best) next = best.at + 1;
        }
        if (!best) continue;
        if (best.c.before) out[r].changed = [best.c.before];
        if (best.c.after) out[best.at].changed = [best.c.after];
      }
    }
    i = a;
  }
  return out;
}

/**
 * **A row's text as pieces to draw**: `[{ text, kind, changed }]` — its
 * coloured tokens (`row.tokens`, core/codeColours.js) or its plain text, cut
 * wherever a marked stretch (`row.changed`) starts or ends. `[]` for an
 * empty line.
 */
export function linePieces(row) {
  const tokens = row?.tokens?.length ? row.tokens : row?.text ? [{ text: String(row.text), kind: '' }] : [];
  const marked = row?.changed || [];
  if (!marked.length) return tokens.map((t) => ({ text: t.text, kind: t.kind || '', changed: false }));
  const out = [];
  let at = 0;
  for (const t of tokens) {
    const end = at + t.text.length;
    // The places inside this token where a marked stretch starts or ends.
    const cuts = [...new Set([at, end, ...marked.flat().filter((p) => p > at && p < end)])].sort((x, y) => x - y);
    for (let i = 0; i + 1 < cuts.length; i++) {
      const [from, to] = [cuts[i], cuts[i + 1]];
      out.push({ text: t.text.slice(from - at, to - at), kind: t.kind || '', changed: marked.some(([a, b]) => from >= a && to <= b) });
    }
    at = end;
  }
  return out;
}
