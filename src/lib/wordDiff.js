// What AI cleanup changed in a dictation (Home's "AI edited · N changes",
// HomeView.svelte): the words as said (history's `said`: the transcript
// with the corrections, the voice command and snippets, no cleanup) against
// the text Yap pasted, word by word — a longest common subsequence of
// words, no dependency.
//
// Words compare without capitals and punctuation (`wordKey`): cleanup adds
// those to nearly every dictation, so they aren't marked and don't count.
// The words it took out or put in are. A token that is only punctuation
// ("-", "—") is never a change: one taken out isn't shown, one put in reads
// as the cleaned text around it.

/** A word as compared: lower case, letters, digits and apostrophes. */
export function wordKey(token) {
  return String(token)
    .toLowerCase()
    .replace(/[‘’]/g, "'")
    .replace(/[^\p{L}\p{N}']/gu, '')
    .replace(/^'+|'+$/g, '');
}

/** `text` as tokens: { text, key, sep } (sep: the whitespace before it). */
function tokens(text) {
  const out = [];
  const re = /(\s*)(\S+)/g;
  let m;
  while ((m = re.exec(String(text ?? '')))) out.push({ text: m[2], key: wordKey(m[2]), sep: m[1] });
  return out;
}

/** A middle bigger than this (words × words) isn't aligned word by word. */
const MAX_CELLS = 250_000;

/** `a` against `b` (tokens) as ops: 'same' (a, b), 'del' (a), 'add' (b). */
function align(a, b) {
  const n = a.length;
  const m = b.length;
  const ops = [];
  // Too long to align (a dictation of many minutes rewritten whole): one
  // change, all of it out and all of it in.
  if (n * m > MAX_CELLS) {
    for (const t of a) ops.push({ type: 'del', a: t });
    for (const t of b) ops.push({ type: 'add', b: t });
    return ops;
  }
  // lcs[i][j]: the longest common run of a[i..] and b[j..] (flat).
  const w = m + 1;
  const lcs = new Int32Array((n + 1) * w);
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      lcs[i * w + j] = a[i].key === b[j].key ? lcs[(i + 1) * w + j + 1] + 1 : Math.max(lcs[(i + 1) * w + j], lcs[i * w + j + 1]);
    }
  }
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (a[i].key === b[j].key) {
      ops.push({ type: 'same', a: a[i++], b: b[j++] });
    } else if (lcs[(i + 1) * w + j] >= lcs[i * w + j + 1]) {
      ops.push({ type: 'del', a: a[i++] });
    } else {
      ops.push({ type: 'add', b: b[j++] });
    }
  }
  while (i < n) ops.push({ type: 'del', a: a[i++] });
  while (j < m) ops.push({ type: 'add', b: b[j++] });
  return ops;
}

/** The space to show before a token of the cleaned text: a line break where it had one. */
const spaceOf = (sep) => (sep.includes('\n') ? sep.replace(/[^\n]/g, '') : ' ');

/**
 * The words as said (`said`) against the cleaned text (`cleaned`).
 * Returns `{ parts, changes }`: `parts` — `{ type: 'same' | 'del' | 'add',
 * text, sep }` in reading order (`sep`: the space before the part, '' for
 * the first; the cleaned text's own line breaks kept), a change's words
 * taken out before the ones put in; `changes` — how many places the AI
 * changed words (a run of words out and/or in is one).
 */
export function wordDiff(said, cleaned) {
  const a = tokens(said);
  const b = tokens(cleaned);
  // The same start and end first: most of a dictation is.
  let start = 0;
  while (start < a.length && start < b.length && a[start].key === b[start].key) start++;
  let endA = a.length;
  let endB = b.length;
  while (endA > start && endB > start && a[endA - 1].key === b[endB - 1].key) {
    endA--;
    endB--;
  }
  const ops = [];
  for (let k = 0; k < start; k++) ops.push({ type: 'same', a: a[k], b: b[k] });
  ops.push(...align(a.slice(start, endA), b.slice(start, endB)));
  for (let k = 0; endA + k < a.length; k++) ops.push({ type: 'same', a: a[endA + k], b: b[endB + k] });

  // Each change (a run between same words): its words out, then its words
  // in; punctuation alone isn't one.
  const pieces = [];
  let changes = 0;
  let k = 0;
  while (k < ops.length) {
    if (ops[k].type === 'same') {
      pieces.push({ type: 'same', text: ops[k].b.text, sep: spaceOf(ops[k].b.sep) });
      k++;
      continue;
    }
    const run = [];
    while (k < ops.length && ops[k].type !== 'same') run.push(ops[k++]);
    const out = run.filter((o) => o.type === 'del' && o.a.key);
    const into = run.filter((o) => o.type === 'add');
    if (out.length || into.some((o) => o.b.key)) changes++;
    for (const o of out) pieces.push({ type: 'del', text: o.a.text, sep: ' ' });
    // Punctuation put in reads as the text around it.
    for (const o of into) pieces.push({ type: o.b.key ? 'add' : 'same', text: o.b.text, sep: spaceOf(o.b.sep) });
  }

  // Runs of one kind as one part.
  const parts = [];
  for (const p of pieces) {
    const prev = parts.at(-1);
    if (prev && prev.type === p.type) prev.text += p.sep + p.text;
    else parts.push({ ...p, sep: parts.length ? p.sep : '' });
  }
  return { parts, changes };
}
