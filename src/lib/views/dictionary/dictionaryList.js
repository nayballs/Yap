// The Dictionary view's list, pure (e2e/dictionary-list.spec.js tests it
// without the app): what a search matches, the four sorts, the key "Most
// used" counts under, and a delete's Undo.
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
 * Does `entry` match the search? Every word of the query has to be in what
 * was heard (a snippet's phrase) or what's written (a snippet's text),
 * whatever the case or accents: "kube" finds "cube and eighties →
 * Kubernetes" and every other way it's heard; "sign soon" a snippet whose
 * phrase is "my sign-off" and text "Speak soon,".
 */
export function matches(entry, query) {
  const words = fold(query).split(' ').filter(Boolean);
  if (!words.length) return true;
  const hay = `${fold(entry?.from)}\n${fold(entry?.to)}`;
  return words.every((w) => hay.includes(w));
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
    out.splice(Math.min(index, out.length), 0, { ...entry });
  }
  return out;
}

/** "1 entry", "2,000 entries". */
export function entriesLabel(n) {
  return `${n.toLocaleString()} ${n === 1 ? 'entry' : 'entries'}`;
}
