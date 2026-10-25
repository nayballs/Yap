// The Dictionary view's list, pure (e2e/dictionary-list.spec.js tests it
// without the app): what a search matches, the four sorts, the key "Most
// used" counts under, a delete's Undo, every way a correction is heard
// (`heardForms`; config.json's rows grouped by the word they write,
// `groupRows`, and written back as rows, `entryRows`:
// dictionary/variants.rs) and the warnings under a snippet's phrase
// (`snippetWarnings`).
//
// The dictionary is one list in the order the entries were added (new ones
// go on the end: + Add, a suggested word, an import), so its index is its
// age: "Recently added" is that order backwards, and every sort falls back
// to it.

/** The sorts, in the picker's order. `starred` is the default. */
export const SORTS = [
  { value: 'starred', label: 'Starred first' },
  { value: 'az', label: 'A–Z' },
  { value: 'used', label: 'Most used' },
  { value: 'recent', label: 'Recently added' },
];
export const DEFAULT_SORT = 'starred';
const SORT_IDS = new Set(SORTS.map((s) => s.value));

/** A saved sort if it's one of ours, else the default. */
export function sortOrDefault(value) {
  return SORT_IDS.has(value) ? value : DEFAULT_SORT;
}

/** Text as a search compares it: lower case, accents off, spaces collapsed. */
export function fold(text) {
  return String(text ?? '')
    .normalize('NFD')
    .replace(/\p{M}+/gu, '')
    .toLowerCase()
    .replace(/\s+/g, ' ')
    .trim();
}

/**
 * Every way a correction is heard: `from`, then `alsoHeard`
 * (dictionary/variants.rs) — one line each, none empty, none twice (any
 * case). A snippet's phrase only.
 */
export function heardForms(entry) {
  const out = [];
  const seen = new Set();
  const more = entry?.snippet ? [] : Array.isArray(entry?.alsoHeard) ? entry.alsoHeard : [];
  for (const h of [entry?.from, ...more]) {
    const one = oneLine(h);
    if (one && !seen.has(one.toLowerCase())) {
      seen.add(one.toLowerCase());
      out.push(one);
    }
  }
  return out;
}

/**
 * Does `entry` match the search? Every word of the query has to be in what
 * was heard (any of the ways it's heard; a snippet's phrase) or what's
 * written (a snippet's text), whatever the case or accents: "kube" finds
 * "cube and eighties → Kubernetes" and every other way it's heard; "sign
 * soon" a snippet whose phrase is "my sign-off" and text "Speak soon,".
 */
export function matches(entry, query) {
  const words = fold(query).split(' ').filter(Boolean);
  if (!words.length) return true;
  const heard = entry?.snippet ? [entry?.from] : [entry?.from, ...(Array.isArray(entry?.alsoHeard) ? entry.alsoHeard : [])];
  const hay = `${heard.map(fold).join('\n')}\n${fold(entry?.to)}`;
  return words.every((w) => hay.includes(w));
}

/**
 * config.json's rows as the view shows them (dictionary/variants.rs): the
 * corrections that write the same word exactly are one entry — its `from`
 * the first row's, the others' in `alsoHeard`, its star and ≈ the first
 * row's. Snippets and half-written rows stay as they are. In the rows'
 * order (an entry where its first row was). Copies; pure.
 */
export function groupRows(rows) {
  const out = [];
  const byWord = new Map();
  for (const row of rows ?? []) {
    if (!row) continue;
    const to = String(row.to ?? '').trim();
    const from = String(row.from ?? '').trim();
    const word = !row.snippet && from && to ? to : null;
    const into = word !== null ? byWord.get(word) : undefined;
    if (into) {
      if (!heardForms(into).some((h) => h.toLowerCase() === oneLine(from).toLowerCase())) {
        into.alsoHeard = [...(into.alsoHeard ?? []), from];
      }
      continue;
    }
    const entry = { ...row };
    if (Array.isArray(row.alsoHeard)) entry.alsoHeard = [...row.alsoHeard];
    else delete entry.alsoHeard;
    out.push(entry);
    if (word !== null) byWord.set(word, entry);
  }
  return out;
}

/**
 * An entry as config.json keeps it (the view's saves): a row per way it's
 * heard — the shape every Yap reads, so an older one sharing config.json
 * keeps them all — each trimmed, with the entry's ≈ and star (only when
 * set); none twice. A snippet, or a row with nothing heard or written:
 * as ever (or nothing). Pure.
 */
export function entryRows(e) {
  const from = String(e?.from ?? '').trim();
  // A snippet's text keeps its inner line breaks; only the ends trim.
  const to = String(e?.to ?? '').trim();
  if (!to) return [];
  const flags = { fuzzy: e.fuzzy ?? true, snippet: !!e.snippet, ...(e.starred ? { starred: true } : {}) };
  const out = from ? [{ from, to, ...flags }] : [];
  if (!e.snippet) {
    const seen = new Set(out.map((r) => oneLine(r.from).toLowerCase()));
    for (const h of Array.isArray(e.alsoHeard) ? e.alsoHeard : []) {
      const more = String(h ?? '').trim();
      if (!more || seen.has(oneLine(more).toLowerCase())) continue;
      seen.add(oneLine(more).toLowerCase());
      out.push({ from: more, to, ...flags });
    }
  }
  return out;
}

/** The longest snippet text (as an import takes it, dictionaryFile.js `LIMITS.text`). */
export const SNIPPET_MAX = 5000;

/** Letters and digits of each word, lower case: "My sign-off!" → "my sign off". */
const words = (s) =>
  String(s ?? '')
    .toLowerCase()
    .split(/[^\p{L}\p{N}]+/u)
    .filter(Boolean);

function editDistance(a, b) {
  if (a === b) return 0;
  let prev = Array.from({ length: b.length + 1 }, (_, j) => j);
  for (let i = 1; i <= a.length; i++) {
    const cur = [i];
    for (let j = 1; j <= b.length; j++) cur[j] = Math.min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + (a[i - 1] === b[j - 1] ? 0 : 1));
    prev = cur;
  }
  return prev[b.length];
}

/** Near enough to be mixed up: a fifth of the longer one apart at most (≥ 4 letters). */
function near(a, b) {
  if (!a || !b) return false;
  if (a === b) return true;
  const len = Math.max(a.length, b.length);
  return len >= 4 && editDistance(a, b) <= Math.floor(len / 5);
}

/** Does `phrase` (words) have `part` (words) in it, as whole words? */
function hasWords(phrase, part) {
  if (!part.length || part.length > phrase.length) return false;
  for (let i = 0; i + part.length <= phrase.length; i++) {
    if (part.every((w, k) => phrase[i + k] === w)) return true;
  }
  return false;
}

/**
 * A snippet's phrase that could go wrong (pure; the Dictionary view's
 * warning under it): the same as or close to another snippet's phrase
 * ("my adress" beside "my address": which one did you say?), with words a
 * correction changes before snippets are looked for ("my jason" with
 * "jason → JSON": Yap writes "my JSON" first and the snippet never
 * fires), or a dictionary word itself (said in any sentence, it types the
 * snippet). `null` when it's fine. `dict`: the whole dictionary.
 */
export function snippetWarning(entry, dict) {
  return snippetWarnings(dict).get(entry) ?? null;
}

/**
 * [`snippetWarning`] for every snippet of `dict` at once (a `Map` entry →
 * warning, fine ones left out): the corrections are indexed once, so a
 * dictionary of thousands stays quick as you type.
 */
export function snippetWarnings(dict) {
  const out = new Map();
  const list = (dict ?? []).filter(Boolean);
  const snippets = list.filter((e) => e.snippet).map((e) => ({ e, words: words(e.from) })).filter((s) => s.words.length);
  if (!snippets.length) return out;
  // The corrections: their written words (a phrase), every way they're
  // heard (a phrase), and the one-word ones near-misses may change, by
  // first letter.
  const written = new Map();
  const heardByFirst = new Map();
  const nearWords = new Map();
  for (const c of list) {
    if (c.snippet) continue;
    const to = words(c.to);
    if (to.length && !written.has(to.join(' '))) written.set(to.join(' '), c);
    for (const h of heardForms(c)) {
      const hw = words(h);
      if (!hw.length || hw.join(' ') === to.join(' ')) continue; // a word heard as itself only fixes its case
      heardByFirst.set(hw[0], [...(heardByFirst.get(hw[0]) ?? []), { c, h, hw }]);
      if ((c.fuzzy ?? true) && hw.length === 1 && hw[0].length >= 4) {
        const k = hw[0][0];
        nearWords.set(k, [...(nearWords.get(k) ?? []), { c, h, w: hw[0] }]);
      }
    }
  }
  for (const s of snippets) {
    const key = s.words.join(' ');
    let warning = null;
    for (const t of snippets) {
      if (t === s) continue;
      const theirs = t.words.join(' ');
      if (theirs === key) {
        warning = { kind: 'same', other: t.e, text: `Another snippet is said “${oneLine(t.e.from)}” too.` };
        break;
      }
      if (!warning && near(theirs, key)) {
        warning = { kind: 'near', other: t.e, text: `Close to the snippet “${oneLine(t.e.from)}”: Yap may type the wrong one.` };
      }
    }
    if (!warning && written.has(key)) {
      const c = written.get(key);
      warning = { kind: 'word', other: c, text: `“${oneLine(c.to)}” is a dictionary word: said anywhere, it types this snippet.` };
    }
    if (!warning) {
      const hit =
        s.words.flatMap((w) => heardByFirst.get(w) ?? []).find((p) => hasWords(s.words, p.hw)) ??
        s.words
          .filter((w) => w.length >= 5)
          .map((w) => (nearWords.get(w[0]) ?? []).find((n) => near(w, n.w)))
          .find(Boolean);
      if (hit) {
        warning = {
          kind: 'correction',
          other: hit.c,
          text: `Yap changes “${hit.h}” to “${oneLine(hit.c.to)}” before it looks for snippets: say it another way.`,
        };
      }
    }
    if (warning) out.set(s.e, warning);
  }
  return out;
}

/** The key dictionary_usage.json counts a word under (dict_usage.rs `key`). */
export function usageKey(entry) {
  return String(entry?.to ?? '')
    .trim()
    .toLowerCase();
}

const collator = new Intl.Collator(undefined, { sensitivity: 'base', numeric: true });

/** What A–Z goes by: a correction's word, a snippet's phrase; an empty one last. */
function azKey(entry) {
  return String((entry.snippet ? entry.from : entry.to) || entry.from || '').trim();
}

/**
 * `entries` (the dictionary, in the order added) as the list shows them:
 * the ones matching `query`, in `sort` order. `usage` maps `usageKey` →
 * `{uses, lastUsed}`. Returns the same objects (never copies).
 */
export function arrange(entries, { query = '', sort = DEFAULT_SORT, usage = {} } = {}) {
  const list = [];
  entries.forEach((entry, index) => {
    if (entry && matches(entry, query)) list.push({ entry, index });
  });
  const byAge = (a, b) => a.index - b.index;
  let cmp = byAge;
  if (sort === 'starred') {
    cmp = (a, b) => Number(!!b.entry.starred) - Number(!!a.entry.starred) || byAge(a, b);
  } else if (sort === 'recent') {
    cmp = (a, b) => b.index - a.index;
  } else if (sort === 'az') {
    cmp = (a, b) => {
      const ka = azKey(a.entry);
      const kb = azKey(b.entry);
      if (!ka !== !kb) return ka ? -1 : 1;
      return collator.compare(ka, kb) || collator.compare(a.entry.from || '', b.entry.from || '') || byAge(a, b);
    };
  } else if (sort === 'used') {
    const use = (e) => usage?.[usageKey(e)] ?? null;
    cmp = (a, b) => {
      const ua = use(a.entry);
      const ub = use(b.entry);
      return (ub?.uses ?? 0) - (ua?.uses ?? 0) || (ub?.lastUsed ?? 0) - (ua?.lastUsed ?? 0) || byAge(a, b);
    };
  }
  return list.sort(cmp).map((x) => x.entry);
}

/** One line of a word or phrase: spaces (and any line break) collapsed. */
export const oneLine = (s) => String(s ?? '').replace(/\s+/g, ' ').trim();
/** A snippet's text: its line breaks kept (Windows' too), the ends trimmed. */
export const textOf = (s) => String(s ?? '').replace(/\r\n?/g, '\n').trim();
/** What a match compares: one line, any case. */
export const norm = (s) => oneLine(s).toLowerCase();

/** What makes two entries the same: kind, what's heard (any case) and what's written. */
export const entryKey = (e) => `${e?.snippet ? 's' : 'c'}\u0000${norm(e?.from)}\u0000${textOf(e?.to)}`;

/** The same entry (`entryKey`). */
export function sameEntry(a, b) {
  return entryKey(a) === entryKey(b);
}

/**
 * `list` with `removed` (`{entry, index}`) back where they were — a
 * delete's Undo. It works on the list as it is by then (other edits kept),
 * matching entries by what they say, so it works the same from the view
 * or, after leaving it, on config.json; one that's back already isn't
 * added twice.
 */
export function restoreRemoved(list, removed) {
  const out = list.slice();
  const there = new Set(out.map(entryKey));
  for (const { entry, index } of [...removed].sort((a, b) => a.index - b.index)) {
    const key = entryKey(entry);
    if (there.has(key)) continue;
    there.add(key);
    const copy = { ...entry };
    if (Array.isArray(entry.alsoHeard)) copy.alsoHeard = [...entry.alsoHeard];
    out.splice(Math.min(index, out.length), 0, copy);
  }
  return out;
}

/** "1 entry", "2,000 entries". */
export function entriesLabel(n) {
  return `${n.toLocaleString()} ${n === 1 ? 'entry' : 'entries'}`;
}
