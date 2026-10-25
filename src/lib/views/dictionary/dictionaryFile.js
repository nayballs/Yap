// The Dictionary view's files, pure (e2e/dictionary-list.spec.js tests it
// without the app): reading an imported file, planning what it adds next
// to the dictionary there is, and writing an export. Picking, reading and
// saving the file is Rust's (dictionary/files.rs).
//
// Two kinds of file:
// - CSV (also semicolon- or tab-separated, as Excel writes them in some
//   countries): quoted cells with "" and line breaks inside, an optional
//   header naming the columns (Heard, Written, Kind, Near-misses, Starred,
//   and the obvious other names), else heard, written; one column is words.
// - Text: one entry a line — `heard → written` (or `->`, `=>`) or a word
//   alone; blank lines, `# notes` and list bullets left out.
// A word alone becomes a correction of itself ("Kubernetes → Kubernetes"):
// the recognizer gets it, any case is put right, and near-misses are caught.
//
// Loaded when Import… or Export… is first used (not with the main window).
import { oneLine, textOf, norm, entryKey } from './dictionaryList.js';

/** Longer than this isn't a word (or a phrase you say); a snippet's text can be long. */
export const LIMITS = { word: 100, text: 5000 };

const ARROW = /\s*(?:→|⟶|⇒|->|=>)\s*/;
const COMMENT = /^(?:#(?:\s|#|$)|\/\/)/;
const BULLET = /^(?:[-*•]|\d{1,3}[.)])\s+/;
const QUOTES = [
  ['"', '"'],
  ["'", "'"],
  ['“', '”'],
  ['‘', '’'],
  ['«', '»'],
];

/** A header cell's name → the column it is. */
const HEADERS = {
  from: ['heard', 'from', 'heardas', 'spoken', 'said', 'yousay', 'phrase', 'trigger', 'misheard', 'original', 'source', 'input'],
  to: ['written', 'to', 'replacewith', 'replacement', 'word', 'words', 'term', 'terms', 'correct', 'corrected', 'spelling', 'yaptypes', 'text', 'expansion', 'output'],
  kind: ['kind', 'type'],
  fuzzy: ['nearmisses', 'nearmiss', 'catchnearmisses', 'fuzzy'],
  starred: ['starred', 'star', 'favourite', 'favorite', 'pinned'],
};
const HEADER_OF = new Map(Object.entries(HEADERS).flatMap(([col, names]) => names.map((n) => [n, col])));

const headerName = (cell) => String(cell).toLowerCase().replace(/[^a-z]/g, '');

function unquote(s) {
  const t = s.trim();
  for (const [a, b] of QUOTES) {
    if (t.length >= 2 && t.startsWith(a) && t.endsWith(b)) return t.slice(a.length, -b.length).trim();
  }
  return t;
}

const YES = new Set(['yes', 'y', 'true', '1', 'on', 'x', '✓', '✔', '★', 'starred']);
const NO = new Set(['no', 'n', 'false', '0', 'off', '-', '☆']);
function yesNo(cell, otherwise) {
  const v = String(cell ?? '').trim().toLowerCase();
  if (YES.has(v)) return true;
  if (NO.has(v)) return false;
  return otherwise;
}

const isSnippetKind = (cell) => /^(?:snip|expan|shortcut)/.test(String(cell ?? '').trim().toLowerCase());

/** The file's text as Rust read it, line endings and a BOM tidied. */
function tidy(text) {
  return String(text ?? '')
    .replace(/^﻿/, '')
    .replace(/\r\n?/g, '\n');
}

/** The separator a CSV's first line uses most (outside quotes): , ; or tab. */
export function detectDelimiter(line) {
  const counts = { ',': 0, ';': 0, '\t': 0 };
  let quoted = false;
  for (const c of line) {
    if (c === '"') quoted = !quoted;
    else if (!quoted && c in counts) counts[c]++;
  }
  let best = ',';
  for (const d of [';', '\t']) if (counts[d] > counts[best]) best = d;
  return best;
}

/** CSV or text: the extension says, else what's in it. */
export function detectFormat(text, name = '') {
  const ext = (String(name).match(/\.([^.\\/]+)$/)?.[1] ?? '').toLowerCase();
  if (ext === 'csv' || ext === 'tsv') return 'csv';
  const lines = tidy(text)
    .split('\n')
    .map((l) => l.trim())
    .filter((l) => l && !COMMENT.test(l));
  if (!lines.length || lines.some((l) => ARROW.test(l))) return 'text';
  const delim = detectDelimiter(lines[0]);
  const withDelim = lines.filter((l) => l.includes(delim)).length;
  return withDelim / lines.length >= 0.6 ? 'csv' : 'text';
}

/**
 * RFC 4180-ish: `delim`-separated cells, a quoted cell may hold the
 * separator, "" for a quote and line breaks; unquoted cells are trimmed.
 * Each record keeps the line it starts on. Blank records are dropped.
 */
export function parseCsv(text, delim = ',') {
  const src = tidy(text);
  const records = [];
  let cells = [];
  let cell = '';
  let quoted = false;
  let inQuotes = false;
  let line = 1;
  let start = 1;
  const endCell = () => {
    cells.push(quoted ? cell : cell.trim());
    cell = '';
    quoted = false;
  };
  const endRecord = () => {
    endCell();
    if (cells.some((c) => c !== '')) records.push({ line: start, cells });
    cells = [];
  };
  for (let i = 0; i < src.length; i++) {
    const c = src[i];
    if (inQuotes) {
      if (c === '"') {
        if (src[i + 1] === '"') {
          cell += '"';
          i++;
        } else {
          inQuotes = false;
        }
      } else {
        if (c === '\n') line++;
        cell += c;
      }
    } else if (c === '"' && !quoted && cell.trim() === '') {
      inQuotes = true;
      quoted = true;
      cell = '';
    } else if (c === delim) {
      endCell();
    } else if (c === '\n') {
      endRecord();
      line++;
      start = line;
    } else {
      cell += c;
    }
  }
  if (cell !== '' || quoted || cells.length) endRecord();
  return records;
}

/**
 * One entry from what a line or record said, or why it's left out (`skip`).
 * `null`: nothing there at all.
 */
function entryFrom({ line, from, to, kind, fuzzy, starred, source }) {
  const snippet = isSnippetKind(kind);
  let heard = oneLine(from);
  let written = snippet ? textOf(to) : oneLine(to);
  if (!heard && !written) return null;
  if (snippet && (!heard || !written)) {
    return { skip: { line, source, reason: 'A snippet needs the phrase you say and its text.' } };
  }
  if (!snippet) {
    if (!heard) heard = written;
    if (!written) written = heard;
  }
  if (heard.length > LIMITS.word || (!snippet && written.length > LIMITS.word)) {
    return { skip: { line, source, reason: 'Too long for a word.' } };
  }
  if (written.length > LIMITS.text) return { skip: { line, source, reason: 'Too long for a snippet.' } };
  return { row: { line, from: heard, to: written, snippet, fuzzy: yesNo(fuzzy, true), starred: yesNo(starred, false) } };
}

function fromCsv(text, name) {
  const ext = (String(name).match(/\.([^.\\/]+)$/)?.[1] ?? '').toLowerCase();
  const first = tidy(text)
    .split('\n')
    .find((l) => l.trim());
  const delim = ext === 'tsv' ? '\t' : detectDelimiter(first ?? '');
  const records = parseCsv(text, delim);
  // A header: its cells name our columns (every one it has, or two or more).
  let cols = { from: 0, to: 1, kind: -1, fuzzy: -1, starred: -1 };
  let body = records;
  if (records.length) {
    const named = records[0].cells.map((c) => HEADER_OF.get(headerName(c)) ?? null);
    const filled = records[0].cells.filter((c) => c !== '').length;
    const known = named.filter(Boolean).length;
    if (known && (known === filled || known >= 2)) {
      cols = { from: -1, to: -1, kind: -1, fuzzy: -1, starred: -1 };
      named.forEach((col, i) => {
        if (col && cols[col] === -1) cols[col] = i;
      });
      body = records.slice(1);
    }
  }
  const cell = (cells, i) => (i >= 0 ? (cells[i] ?? '') : '');
  // The record as the file had it (empty cells at its end aside), to say why it's left out.
  const sourceOf = (cells) => {
    const used = [...cells];
    while (used.length && used[used.length - 1] === '') used.pop();
    return used.join(delim);
  };
  return body.map(({ line, cells }) =>
    entryFrom({
      line,
      source: sourceOf(cells),
      from: cell(cells, cols.from),
      to: cell(cells, cols.to),
      kind: cell(cells, cols.kind),
      fuzzy: cell(cells, cols.fuzzy),
      starred: cell(cells, cols.starred),
    })
  );
}

function fromText(text) {
  return tidy(text)
    .split('\n')
    .map((raw, i) => {
      const line = i + 1;
      const s = raw.trim().replace(BULLET, '');
      if (!s || COMMENT.test(s)) return null;
      const arrow = ARROW.exec(s);
      if (!arrow) return entryFrom({ line, source: s, from: unquote(s), to: '' });
      const heard = unquote(s.slice(0, arrow.index));
      const written = unquote(s.slice(arrow.index + arrow[0].length));
      if (!heard || !written) return { skip: { line, source: s, reason: 'Half a pair: say what Yap hears → what it writes.' } };
      return entryFrom({ line, source: s, from: heard, to: written });
    });
}

/**
 * Read an imported file: `{ format, rows, skipped }`. `rows` are entries
 * with the line they came from; `skipped` the lines left out and why.
 */
export function parseDictionaryFile(text, name = '') {
  const format = detectFormat(text, name);
  const results = format === 'csv' ? fromCsv(text, name) : fromText(text);
  const rows = [];
  const skipped = [];
  for (const r of results) {
    if (r?.row) rows.push(r.row);
    else if (r?.skip) skipped.push(r.skip);
  }
  return { format, rows, skipped };
}

const kindKey = (snippet, from) => `${snippet ? 's' : 'c'}\u0000${norm(from)}`;

/**
 * What the file's `rows` would do to `dict`:
 * - `add`: new entries;
 * - `same`: already in the dictionary (`{row, existing}`) — left out;
 * - `conflicts`: `{row, existing, why}` — `why: 'heard'` the same thing
 *   heard (a snippet's phrase) written another way; `why: 'spelling'` the
 *   same word spelled another way ("JSON" here, "Json" there; `all` = every
 *   correction spelling it yours);
 * - `repeats`: rows saying again what an earlier row of the file said.
 */
export function planImport(rows, dict) {
  const byTrigger = new Map();
  const bySpelling = new Map();
  for (const e of dict ?? []) {
    if (!e) continue;
    const k = kindKey(e.snippet, e.from);
    if (!byTrigger.has(k)) byTrigger.set(k, e);
    if (!e.snippet && (e.to ?? '').trim()) {
      const s = norm(e.to);
      bySpelling.set(s, [...(bySpelling.get(s) ?? []), e]);
    }
  }
  const plan = { add: [], same: [], conflicts: [], repeats: [] };
  const seen = new Set();
  const fileSpelling = new Map();
  for (const row of rows) {
    const k = kindKey(row.snippet, row.from);
    if (seen.has(k)) {
      plan.repeats.push(row);
      continue;
    }
    seen.add(k);
    const existing = byTrigger.get(k);
    if (existing) {
      const same = row.snippet ? textOf(existing.to) === row.to : oneLine(existing.to) === row.to;
      if (same) plan.same.push({ row, existing });
      else plan.conflicts.push({ row, existing, why: 'heard' });
      continue;
    }
    if (!row.snippet) {
      const word = norm(row.from) === norm(row.to);
      const spelt = bySpelling.get(norm(row.to)) ?? [];
      const exact = spelt.find((e) => oneLine(e.to) === row.to);
      if (exact && word) {
        plan.same.push({ row, existing: exact });
        continue;
      }
      if (spelt.length && !exact) {
        plan.conflicts.push({ row, existing: spelt[0], all: spelt, why: 'spelling' });
        continue;
      }
      if (word && fileSpelling.get(norm(row.to)) === row.to) {
        plan.repeats.push(row);
        continue;
      }
      if (!fileSpelling.has(norm(row.to))) fileSpelling.set(norm(row.to), row.to);
    }
    plan.add.push(row);
  }
  return plan;
}

/** A row as a dictionary entry (a star only when set, as config.json keeps it). */
export function entryOf(row) {
  return {
    from: row.from,
    to: row.to,
    fuzzy: row.fuzzy ?? true,
    snippet: !!row.snippet,
    ...(row.starred ? { starred: true } : {}),
  };
}

/**
 * What applying `plan` does, given the person's choices: `leaveOut` (rows
 * of `plan.add` unticked) and `useFile` (conflicts where the file's
 * spelling wins; the rest keep yours). `{ add: entries, respell: [{entry,
 * to}] }` — `entry` one already in the dictionary.
 */
export function importChanges(plan, { leaveOut = new Set(), useFile = new Set() } = {}) {
  const add = plan.add.filter((row) => !leaveOut.has(row)).map(entryOf);
  const respell = [];
  for (const c of plan.conflicts) {
    const fileWins = useFile.has(c);
    if (c.why === 'heard') {
      if (fileWins) respell.push({ entry: c.existing, to: c.row.to });
      continue;
    }
    if (fileWins) for (const entry of c.all ?? [c.existing]) respell.push({ entry, to: c.row.to });
    // A new way it's heard still comes in, in whichever spelling won.
    if (norm(c.row.from) !== norm(c.row.to)) {
      add.push(entryOf({ ...c.row, to: fileWins ? c.row.to : oneLine(c.existing.to) }));
    }
  }
  return { add, respell };
}

// ---- Undo (an import's; a delete's is dictionaryList.js `restoreRemoved`) ----

/**
 * `list` without what an import added (`added`, entries) and with what it
 * respelled put back (`respelled`: `{from, snippet, was, now}`).
 */
export function undoImport(list, { added = [], respelled = [] }) {
  // How many of each the import added: that many go, the last ones first
  // (the import put them at the end), so one of the same you had stays.
  const toGo = new Map();
  for (const a of added) toGo.set(entryKey(a), (toGo.get(entryKey(a)) ?? 0) + 1);
  const keep = new Array(list.length).fill(true);
  for (let i = list.length - 1; i >= 0 && toGo.size; i--) {
    const key = entryKey(list[i]);
    const n = toGo.get(key);
    if (!n) continue;
    keep[i] = false;
    if (n === 1) toGo.delete(key);
    else toGo.set(key, n - 1);
  }
  const out = list.filter((_, i) => keep[i]);
  const back = new Map(respelled.map((r) => [entryKey({ from: r.from, to: r.now, snippet: r.snippet }), r.was]));
  return out.map((e) => {
    const was = back.get(entryKey(e));
    if (was === undefined) return e;
    back.delete(entryKey(e));
    return { ...e, to: was };
  });
}

// ---- Export ----

const csvCell = (v) => {
  const s = String(v ?? '');
  return /[",;\t\r\n]|^\s|\s$/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
};

/**
 * `entries` as CSV: a header Import… reads back (Heard, Written, Kind,
 * Near-misses, Starred), Windows line ends, and a BOM so Excel reads it as
 * UTF-8.
 */
export function toCsv(entries) {
  const rows = [['Heard', 'Written', 'Kind', 'Near-misses', 'Starred']];
  for (const e of entries) {
    rows.push([
      e.from ?? '',
      e.to ?? '',
      e.snippet ? 'snippet' : 'correction',
      e.snippet ? '' : (e.fuzzy ?? true) ? 'yes' : 'no',
      e.starred ? 'yes' : '',
    ]);
  }
  return `﻿${rows.map((r) => r.map(csvCell).join(',')).join('\r\n')}\r\n`;
}

/**
 * `entries` as text: `heard → written` a line, a word alone when it's
 * heard as itself. Snippets can't be told apart in plain text (their text
 * has lines of its own), so they stay out: `left` says how many.
 */
export function toText(entries) {
  const lines = ['# Yap dictionary: what Yap hears → what it writes, one a line.'];
  let left = 0;
  for (const e of entries) {
    if (e.snippet) {
      left++;
      continue;
    }
    const from = oneLine(e.from);
    const to = oneLine(e.to);
    if (!from && !to) continue;
    lines.push(!from || from === to ? to : `${from} → ${to || from}`);
  }
  return { text: `${lines.join('\r\n')}\r\n`, left };
}

/** "Yap dictionary 2026-10-25.csv". */
export function exportName(kind, now = new Date()) {
  const d = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}-${String(now.getDate()).padStart(2, '0')}`;
  return `Yap dictionary ${d}.${kind}`;
}
