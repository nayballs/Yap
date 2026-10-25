// The Dictionary view's pure parts on their own (no app needed):
// src/lib/views/dictionary/dictionaryList.js (search, the sorts, a delete's
// Undo) and dictionaryFile.js (reading an imported file — CSV with quotes,
// semicolons or tabs, text with arrows or words alone —, the import's plan
// of what's new, already there, spelled differently or said twice, what
// the person's choices change, the export and an import's Undo). The view
// itself, with these in it, is e2e/dictionary.spec.js. Every word here is
// made up or an everyday product name.
import { test, expect } from '@playwright/test';
import { arrange, matches, fold, sortOrDefault, usageKey, restoreRemoved, sameEntry } from '../src/lib/views/dictionary/dictionaryList.js';
import {
  parseCsv,
  detectDelimiter,
  detectFormat,
  parseDictionaryFile,
  planImport,
  importChanges,
  undoImport,
  toCsv,
  toText,
  exportName,
} from '../src/lib/views/dictionary/dictionaryFile.js';

const c = (from, to, more = {}) => ({ from, to, fuzzy: true, snippet: false, ...more });
const s = (from, to, more = {}) => ({ from, to, fuzzy: true, snippet: true, ...more });
const tos = (list) => list.map((e) => e.to);
/** The rows a file gives, as [from, to] (snippets marked). */
const pairs = (rows) => rows.map((r) => (r.snippet ? ['snippet', r.from, r.to] : [r.from, r.to]));

// ---- Search ----

test('a search matches what is heard or written, any case or accent, every word', () => {
  const kube = c('cube and eighties', 'Kubernetes');
  const kube2 = c('cuber netties', 'Kubernetes');
  const cafe = c('cafe zentrix', 'Café Zentrix');
  const sign = s('my sign-off', 'Speak soon,\nRobin');
  const list = [kube, kube2, cafe, sign];
  expect(arrange(list, { query: 'kube' })).toEqual([kube, kube2]); // the word: every way it's heard
  expect(arrange(list, { query: 'EIGHTIES' })).toEqual([kube]); // how it's heard
  expect(arrange(list, { query: 'café' })).toEqual([cafe]);
  expect(arrange(list, { query: 'cafe' })).toEqual([cafe]); // accents aside
  expect(arrange(list, { query: 'soon robin' })).toEqual([sign]); // a snippet's text, across its lines
  expect(arrange(list, { query: 'sign robin' })).toEqual([sign]); // words in either half
  expect(arrange(list, { query: 'kube robin' })).toEqual([]);
  expect(arrange(list, { query: '   ' })).toEqual(list);
  expect(matches(c('', ''), 'x')).toBe(false);
  expect(fold('  Zoë   Ní\tBhriain ')).toBe('zoe ni bhriain');
});

// ---- Sorts ----

test('the sorts: starred first, A–Z, most used, recently added — each falling back to the order added', () => {
  const a = c('zed', 'Zentrix');
  const b = c('alfa', 'Alpha', { starred: true });
  const d = c('kube', 'kubernetes');
  const e = s('brb', 'Be right back');
  const f = c('', ''); // a row being written
  const g = c('vex', 'Vexwell', { starred: true });
  const list = [a, b, d, e, f, g];
  expect(arrange(list, { sort: 'starred' })).toEqual([b, g, a, d, e, f]);
  // A–Z: a correction by its word, a snippet by its phrase; case aside; empty last.
  expect(arrange(list, { sort: 'az' })).toEqual([b, e, d, g, a, f]);
  expect(arrange(list, { sort: 'recent' })).toEqual([g, f, e, d, b, a]);
  const usage = {
    zentrix: { uses: 4, lastUsed: 100 },
    kubernetes: { uses: 9, lastUsed: 50 },
    vexwell: { uses: 4, lastUsed: 300 },
  };
  // Most uses first; a tie goes to the one used last; never used, the order added.
  expect(arrange(list, { sort: 'used', usage })).toEqual([d, g, a, b, e, f]);
  // Counted under the word, lowercased (dict_usage.rs `key`).
  expect(usageKey(c('x', '  Kubernetes '))).toBe('kubernetes');
  // A search and a sort together.
  expect(arrange(list, { query: 'e', sort: 'az' })).toEqual([e, d, g, a]);
  // An unknown saved sort is the default.
  expect(sortOrDefault('az')).toBe('az');
  expect(sortOrDefault('nonsense')).toBe('starred');
  expect(sortOrDefault(null)).toBe('starred');
});

test('a delete is undone where the entries were, once, with other edits kept', () => {
  const list = [c('a', 'A'), c('b', 'B'), c('c', 'C'), c('d', 'D')];
  const removed = [
    { entry: list[1], index: 1 },
    { entry: list[3], index: 3 },
  ];
  const after = [list[0], list[2], c('e', 'E')]; // and one added meanwhile
  expect(tos(restoreRemoved(after, removed))).toEqual(['A', 'B', 'C', 'D', 'E']);
  // Already back (an Undo pressed twice): not added again.
  expect(tos(restoreRemoved(restoreRemoved(after, removed), removed))).toEqual(['A', 'B', 'C', 'D', 'E']);
  expect(sameEntry(c('Power To Keep', 'Parakeet'), c('power to keep', 'Parakeet'))).toBe(true);
  expect(sameEntry(c('power to keep', 'Parakeet'), s('power to keep', 'Parakeet'))).toBe(false);
});

// ---- Reading a file ----

test('CSV: quotes, doubled quotes, separators and line breaks inside quotes, CRLF, blank lines', () => {
  const csv = '﻿heard,written\r\n"power, to keep",Parakeet\r\n\r\njaison,"the ""JSON"" format"\r\n"my address","1 Long Lane\r\nLeeds"\r\n  spaced  ,  out  \r\n';
  const records = parseCsv(csv, ',');
  expect(records.map((r) => r.cells)).toEqual([
    ['heard', 'written'],
    ['power, to keep', 'Parakeet'],
    ['jaison', 'the "JSON" format'],
    ['my address', '1 Long Lane\nLeeds'],
    ['spaced', 'out'],
  ]);
  // Each record keeps the line it starts on (a quoted line break counts).
  expect(records.map((r) => r.line)).toEqual([1, 2, 4, 5, 7]);
  expect(detectDelimiter('heard;written;kind')).toBe(';');
  expect(detectDelimiter('heard\twritten')).toBe('\t');
  expect(detectDelimiter('"a;b",c')).toBe(','); // inside quotes doesn't count
  expect(detectDelimiter('Kubernetes')).toBe(',');
});

test('CSV: our own header, other names for the columns, no header, one column of words', () => {
  const ours = parseDictionaryFile(
    'Heard,Written,Kind,Near-misses,Starred\npower to keep,Parakeet,correction,yes,yes\njaison,JSON,correction,no,\nmy sign-off,"Speak soon,\nRobin",snippet,,\n',
    'mine.csv'
  );
  expect(ours.format).toBe('csv');
  expect(ours.rows).toEqual([
    { line: 2, from: 'power to keep', to: 'Parakeet', snippet: false, fuzzy: true, starred: true },
    { line: 3, from: 'jaison', to: 'JSON', snippet: false, fuzzy: false, starred: false },
    { line: 4, from: 'my sign-off', to: 'Speak soon,\nRobin', snippet: true, fuzzy: true, starred: false },
  ]);

  // Columns named another way, in another order, with one we don't know.
  const other = parseDictionaryFile('Notes;Replacement;Spoken\nnone;Zentrix;zen tricks\n', 'x.csv');
  expect(pairs(other.rows)).toEqual([['zen tricks', 'Zentrix']]);

  // No header: heard, written.
  expect(pairs(parseDictionaryFile('cuber netties,Kubernetes\nvex well,Vexwell\n', 'x.csv').rows)).toEqual([
    ['cuber netties', 'Kubernetes'],
    ['vex well', 'Vexwell'],
  ]);
  // One column: words, each a correction of itself (a "Word" header or not).
  expect(pairs(parseDictionaryFile('Word\nZentrix\nQwen3\n', 'w.csv').rows)).toEqual([
    ['Zentrix', 'Zentrix'],
    ['Qwen3', 'Qwen3'],
  ]);
  expect(pairs(parseDictionaryFile('Zentrix\nQwen3\n', 'w.csv').rows)).toEqual([
    ['Zentrix', 'Zentrix'],
    ['Qwen3', 'Qwen3'],
  ]);
  // Tab-separated.
  expect(pairs(parseDictionaryFile('heard\twritten\nzen tricks\tZentrix\n', 'x.tsv').rows)).toEqual([['zen tricks', 'Zentrix']]);
});

test('CSV: what is left out, and why', () => {
  const long = 'x'.repeat(101);
  const { rows, skipped } = parseDictionaryFile(
    `heard,written,kind\n,,\nmy address,,snippet\n${long},Word\nok,OK\n`,
    'x.csv'
  );
  expect(pairs(rows)).toEqual([['ok', 'OK']]);
  expect(skipped.map((k) => [k.line, k.reason])).toEqual([
    [3, 'A snippet needs the phrase you say and its text.'],
    [4, 'Too long for a word.'],
  ]);
  // A correction with one half: a word.
  expect(pairs(parseDictionaryFile('heard,written\nZentrix,\n,Vexwell\n', 'x.csv').rows)).toEqual([
    ['Zentrix', 'Zentrix'],
    ['Vexwell', 'Vexwell'],
  ]);
});

test('text: heard → written with →, -> or =>, words alone, notes, bullets and quotes', () => {
  const text = [
    '# My words',
    '',
    'power to keep → Parakeet',
    'jaison -> JSON',
    '  cuber netties => Kubernetes  ',
    '- Zentrix',
    '2. “vex well” → “Vexwell”',
    '#hashtag',
    'half →',
    '// a note',
    'Café Zentrix',
  ].join('\r\n');
  const { format, rows, skipped } = parseDictionaryFile(text, 'words.txt');
  expect(format).toBe('text');
  expect(pairs(rows)).toEqual([
    ['power to keep', 'Parakeet'],
    ['jaison', 'JSON'],
    ['cuber netties', 'Kubernetes'],
    ['Zentrix', 'Zentrix'],
    ['vex well', 'Vexwell'],
    ['#hashtag', '#hashtag'], // a word with a #, not a note
    ['Café Zentrix', 'Café Zentrix'],
  ]);
  expect(rows.map((r) => r.line)).toEqual([3, 4, 5, 6, 7, 8, 11]);
  expect(skipped).toEqual([{ line: 9, source: 'half →', reason: 'Half a pair: say what Yap hears → what it writes.' }]);
});

test('CSV or text: the extension, else what is in the file', () => {
  expect(detectFormat('a → b', 'list.csv')).toBe('csv');
  expect(detectFormat('power to keep → Parakeet\nZentrix', 'list.txt')).toBe('text');
  expect(detectFormat('power to keep,Parakeet\njaison,JSON\nZentrix', 'list.txt')).toBe('csv');
  expect(detectFormat('Zentrix\nVexwell\nQwen3, the model', 'list.txt')).toBe('text');
  expect(detectFormat('', 'empty.txt')).toBe('text');
});

// ---- The import's plan ----

test('an import: new, already there, the same heard written another way, a word spelled another way, said twice', () => {
  const dict = [
    c('power to keep', 'Parakeet'),
    c('jaison', 'JSON'),
    c('cube and eighties', 'Kubernetes'),
    c('vex well', 'Vexwell'),
    s('my sign-off', 'Speak soon,\nRobin'),
  ];
  const { rows } = parseDictionaryFile(
    [
      'Heard,Written,Kind',
      'Power To Keep,Parakeet,', // already there (heard: any case)
      'jaison,Json,', // heard the same, written another way
      'Kubernetes,Kubernetes,', // the word alone: already there under another way it's heard
      'VEXWELL,VEXWELL,', // a word you have, in another case: spelled differently
      'cuber netties,KUBERNETES,', // a new way it's heard, spelled differently
      'zen tricks,Zentrix,', // new
      'ZEN TRICKS,Zentricks,', // the file says it again: the first wins
      'Zentrix,Zentrix,', // a word the file just added: said twice
      'my sign-off,"Speak soon,\nRobin",snippet', // the same snippet
      'my address,"1 Long Lane\nLeeds",snippet', // a new snippet
      'my sign-off,Cheers,correction', // a correction, not the snippet: new
    ].join('\n'),
    'import.csv'
  );
  const plan = planImport(rows, dict);
  expect(pairs(plan.add)).toEqual([
    ['zen tricks', 'Zentrix'],
    ['snippet', 'my address', '1 Long Lane\nLeeds'],
    ['my sign-off', 'Cheers'],
  ]);
  expect(plan.same.map((x) => [x.row.from, x.existing.to])).toEqual([
    ['Power To Keep', 'Parakeet'],
    ['Kubernetes', 'Kubernetes'],
    ['my sign-off', 'Speak soon,\nRobin'],
  ]);
  expect(plan.conflicts.map((x) => [x.why, x.row.from, x.row.to, x.existing.to])).toEqual([
    ['heard', 'jaison', 'Json', 'JSON'],
    ['spelling', 'VEXWELL', 'VEXWELL', 'Vexwell'],
    ['spelling', 'cuber netties', 'KUBERNETES', 'Kubernetes'],
  ]);
  expect(pairs(plan.repeats)).toEqual([
    ['ZEN TRICKS', 'Zentricks'],
    ['Zentrix', 'Zentrix'],
  ]);

  // Nothing chosen: the new ones come in; every conflict keeps yours, and a
  // new way a word's heard comes in with your spelling.
  const kept = importChanges(plan);
  expect(kept.add).toEqual([
    c('zen tricks', 'Zentrix'),
    s('my address', '1 Long Lane\nLeeds'),
    c('my sign-off', 'Cheers'),
    c('cuber netties', 'Kubernetes'),
  ]);
  expect(kept.respell).toEqual([]);

  // One unticked; the file's spelling for two conflicts.
  const [heard, word, newWay] = plan.conflicts;
  const chosen = importChanges(plan, { leaveOut: new Set([plan.add[2]]), useFile: new Set([heard, newWay]) });
  expect(chosen.add).toEqual([c('zen tricks', 'Zentrix'), s('my address', '1 Long Lane\nLeeds'), c('cuber netties', 'KUBERNETES')]);
  expect(chosen.respell.map((r) => [r.entry.from, r.to])).toEqual([
    ['jaison', 'Json'],
    ['cube and eighties', 'KUBERNETES'], // every correction spelling it yours
  ]);
  // The word alone, the file's way: yours respelled, nothing new added.
  const wordWins = importChanges(plan, { useFile: new Set([word]) });
  expect(wordWins.respell.map((r) => [r.entry.from, r.to])).toEqual([['vex well', 'VEXWELL']]);
  expect(wordWins.add.map((e) => e.from)).not.toContain('VEXWELL');
});

test('an import keeps a star and the near-miss switch the file gives', () => {
  const plan = planImport(parseDictionaryFile('Heard,Written,Near-misses,Starred\njaison,JSON,no,yes\n', 'x.csv').rows, []);
  expect(importChanges(plan).add).toEqual([{ from: 'jaison', to: 'JSON', fuzzy: false, snippet: false, starred: true }]);
});

test("an import's Undo takes out what it added and puts back what it respelled", () => {
  const list = [c('jaison', 'Json'), c('zen tricks', 'Zentrix'), c('vex well', 'Vexwell'), c('cube and eighties', 'Kubernetes')];
  const out = undoImport(list, {
    added: [c('zen tricks', 'Zentrix'), c('not there', 'Gone')],
    respelled: [{ from: 'jaison', snippet: false, was: 'JSON', now: 'Json' }],
  });
  expect(out).toEqual([c('jaison', 'JSON'), c('vex well', 'Vexwell'), c('cube and eighties', 'Kubernetes')]);
});

// ---- Export ----

test('export: CSV that Import reads back the same, text with words alone and pairs', () => {
  const entries = [
    c('power, to keep', 'Parakeet', { starred: true }),
    c('jaison', 'the "JSON" format', { fuzzy: false }),
    s('my sign-off', 'Speak soon,\nRobin'),
    c('Zentrix', 'Zentrix'),
  ];
  const csv = toCsv(entries);
  expect(csv.startsWith('﻿Heard,Written,Kind,Near-misses,Starred\r\n')).toBe(true);
  expect(csv).toContain('"power, to keep",Parakeet,correction,yes,yes\r\n');
  expect(csv).toContain('jaison,"the ""JSON"" format",correction,no,\r\n');
  expect(csv).toContain('my sign-off,"Speak soon,\nRobin",snippet,,\r\n');
  const back = parseDictionaryFile(csv, exportName('csv'));
  expect(back.skipped).toEqual([]);
  expect(importChanges(planImport(back.rows, [])).add).toEqual(entries);

  const { text, left } = toText(entries);
  expect(left).toBe(1);
  expect(text).toBe(
    '# Yap dictionary: what Yap hears → what it writes, one a line.\r\npower, to keep → Parakeet\r\njaison → the "JSON" format\r\nZentrix\r\n'
  );
  expect(pairs(parseDictionaryFile(text, 'x.txt').rows)).toEqual([
    ['power, to keep', 'Parakeet'],
    ['jaison', 'the "JSON" format'],
    ['Zentrix', 'Zentrix'],
  ]);
  expect(exportName('txt', new Date(2026, 9, 5))).toBe('Yap dictionary 2026-10-05.txt');
});
