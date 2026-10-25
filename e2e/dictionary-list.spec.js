// The Dictionary view's pure parts on their own (no app needed):
// src/lib/views/dictionary/dictionaryList.js (search, the sorts, a delete's
// Undo) and dictionaryFile.js (reading an imported file — CSV with quotes,
// semicolons or tabs, text with arrows or words alone —, the import's plan
// of what's new, already there, spelled differently or said twice, what
// the person's choices change, the export and an import's Undo). The view
// itself, with these in it, is e2e/dictionary.spec.js. Every word here is
// made up or an everyday product name.
import { test, expect } from '@playwright/test';
import {
  arrange,
  matches,
  fold,
  sortOrDefault,
  usageKey,
  restoreRemoved,
  sameEntry,
  heardForms,
  cleanEntry,
  snippetWarning,
  snippetWarnings,
  SNIPPET_MAX,
} from '../src/lib/views/dictionary/dictionaryList.js';
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
  // new way a word's heard comes in with your spelling — another way the
  // entry writing it is heard (dictionary/variants.rs).
  const kept = importChanges(plan);
  expect(kept.add).toEqual([c('zen tricks', 'Zentrix'), s('my address', '1 Long Lane\nLeeds'), c('my sign-off', 'Cheers')]);
  expect(kept.extend).toEqual([{ entry: dict[2], heard: ['cuber netties'] }]);
  expect(kept.rows).toBe(4);
  expect(kept.respell).toEqual([]);

  // One unticked; the file's spelling for two conflicts.
  const [heard, word, newWay] = plan.conflicts;
  const chosen = importChanges(plan, { leaveOut: new Set([plan.add[2]]), useFile: new Set([heard, newWay]) });
  expect(chosen.add).toEqual([c('zen tricks', 'Zentrix'), s('my address', '1 Long Lane\nLeeds')]);
  expect(chosen.extend).toEqual([{ entry: dict[2], heard: ['cuber netties'] }]);
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

// ---- One entry, several ways it's heard (dictionary/variants.rs) ----

test("every way a word is heard: listed, searched, kept clean", () => {
  const kube = c('cooper netties', 'Kubernetes', { alsoHeard: ['cube an eighties', ' Cooper Netties ', '', 'kubernetes'] });
  expect(heardForms(kube)).toEqual(['cooper netties', 'cube an eighties', 'kubernetes']);
  // A snippet is said one way.
  expect(heardForms(s('my sign-off', 'x', { alsoHeard: ['sign me off'] }))).toEqual(['my sign-off']);
  // The search finds a word by any way it's heard.
  expect(matches(kube, 'eighties')).toBe(true);
  expect(matches(c('cooper netties', 'Kubernetes'), 'eighties')).toBe(false);
  // As config.json keeps it: none twice, none the same as `from`, none empty;
  // none at all, no field.
  expect(cleanEntry(kube)).toEqual({ from: 'cooper netties', to: 'Kubernetes', fuzzy: true, snippet: false, alsoHeard: ['cube an eighties', 'kubernetes'] });
  expect(cleanEntry(c(' jaison ', 'JSON', { alsoHeard: ['  '] }))).toEqual({ from: 'jaison', to: 'JSON', fuzzy: true, snippet: false });
  expect(cleanEntry(s('my sign-off', 'Speak soon,', { alsoHeard: ['x'] }))).toEqual({ from: 'my sign-off', to: 'Speak soon,', fuzzy: true, snippet: true });
  expect(cleanEntry(c('', 'Kubernetes'))).toBe(null);
  // A delete's Undo brings its ways back too, in a list of their own.
  const back = restoreRemoved([], [{ entry: kube, index: 0 }]);
  expect(back[0].alsoHeard).toEqual(kube.alsoHeard);
  expect(back[0].alsoHeard).not.toBe(kube.alsoHeard);
});

test('export and import round-trip a word heard several ways: a row a way, one entry back', () => {
  const entries = [
    c('cooper netties', 'Kubernetes', { alsoHeard: ['cube an eighties'], starred: true }),
    c('jaison', 'JSON', { fuzzy: false, alsoHeard: ['jay son'] }),
    c('Zentrix', 'Zentrix', { alsoHeard: ['zen tricks'] }),
    s('my sign-off', 'Speak soon,\nRobin'),
  ];
  const csv = toCsv(entries);
  expect(csv).toContain('cooper netties,Kubernetes,correction,yes,yes\r\ncube an eighties,Kubernetes,correction,yes,yes\r\n');
  expect(csv).toContain('jay son,JSON,correction,no,\r\n');
  const back = parseDictionaryFile(csv, exportName('csv'));
  expect(back.rows).toHaveLength(7);
  const into = importChanges(planImport(back.rows, []));
  expect(into.add).toEqual(entries);
  expect(into.rows).toBe(7);
  // …and into the same dictionary: all of it there already.
  const again = planImport(back.rows, entries);
  expect(again.add).toEqual([]);
  expect(again.same).toHaveLength(7);

  // Text: a line a way (snippets need CSV).
  const { text } = toText(entries);
  expect(text).toContain('cooper netties → Kubernetes\r\ncube an eighties → Kubernetes\r\n');
  expect(text).toContain('Zentrix\r\nzen tricks → Zentrix\r\n');
  const fromText = importChanges(planImport(parseDictionaryFile(text, 'x.txt').rows, [])).add;
  expect(fromText.map((e) => [e.to, heardForms(e)])).toEqual([
    ['Kubernetes', ['cooper netties', 'cube an eighties']],
    ['JSON', ['jaison', 'jay son']],
    ['Zentrix', ['Zentrix', 'zen tricks']],
  ]);
});

test('an import adds a new way a word is heard to the entry writing it, and its Undo takes it off again', () => {
  const kube = c('cooper netties', 'Kubernetes');
  const json = c('jaison', 'JSON', { fuzzy: false });
  const dict = [kube, json];
  const { rows } = parseDictionaryFile(
    'Heard,Written,Near-misses\ncube an eighties,Kubernetes,yes\ncuber,Kubernetes,yes\njay son,JSON,yes\n',
    'x.csv'
  );
  const plan = planImport(rows, dict);
  expect(plan.add).toHaveLength(3);
  expect(plan.add[0].into).toBe(kube);
  expect(rows[0].into).toBeUndefined(); // the file's rows as they were
  // The exact-only JSON isn't joined by a row catching near-misses: its own entry.
  expect(plan.add[2].into).toBeUndefined();
  const change = importChanges(plan);
  expect(change.extend).toEqual([{ entry: kube, heard: ['cube an eighties', 'cuber'] }]);
  expect(change.add).toEqual([c('jay son', 'JSON')]);
  expect(change.rows).toBe(3);

  // Undo, as the view does it: the ways it gave entries there go again.
  const after = [c('cooper netties', 'Kubernetes', { alsoHeard: ['cube an eighties', 'cuber'] }), json, c('jay son', 'JSON')];
  const undone = undoImport(after, {
    added: change.add,
    extended: [{ from: 'cooper netties', to: 'Kubernetes', snippet: false, heard: ['cube an eighties', 'cuber'] }],
  });
  expect(undone).toEqual([c('cooper netties', 'Kubernetes'), json]);
  // A way that was there before stays.
  const before = [c('cooper netties', 'Kubernetes', { alsoHeard: ['kuber', 'cuber'] })];
  expect(undoImport(before, { extended: [{ from: 'cooper netties', to: 'Kubernetes', snippet: false, heard: ['cuber'] }] })[0].alsoHeard).toEqual(['kuber']);
});

// ---- Richer snippets ----

test("a snippet's phrase warns when it's another snippet's, near one, has a corrected word, or is a dictionary word", () => {
  const address = s('my address', '1 Long Lane');
  const adress = s('my adress', '2 Long Lane');
  const twice = s('My Address', '3 Long Lane');
  const jason = s('my jason file', 'x');
  const word = s('grafana', 'The Grafana board');
  const kloud = s('my klouds link', 'https://example.com');
  const fine = s('my sign-off', 'Speak soon,\nRobin');
  const notion = s('my notion page', 'https://example.com/notion');
  const dict = [
    address,
    adress,
    twice,
    jason,
    word,
    kloud,
    fine,
    notion,
    c('jaison', 'JSON', { alsoHeard: ['jason'] }),
    c('graph ana', 'Grafana'),
    c('kloud', 'Claude'),
    c('Notion', 'Notion'), // heard as itself: only its case is fixed
  ];
  const w = snippetWarnings(dict);
  expect(w.get(address).kind).toBe('same');
  expect(w.get(twice).text).toBe('Another snippet is said “my address” too.');
  expect(w.get(adress)).toMatchObject({ kind: 'near', text: 'Close to the snippet “my address”: Yap may type the wrong one.' });
  expect(w.get(jason)).toMatchObject({ kind: 'correction', text: 'Yap changes “jason” to “JSON” before it looks for snippets: say it another way.' });
  expect(w.get(word)).toMatchObject({ kind: 'word', text: '“Grafana” is a dictionary word: said anywhere, it types this snippet.' });
  expect(w.get(kloud).kind).toBe('correction'); // near "kloud", which near-misses change
  expect(w.has(fine)).toBe(false);
  expect(w.has(notion)).toBe(false);
  expect(snippetWarning(fine, dict)).toBe(null);
  // Near-misses off for that word: only its exact words.
  expect(snippetWarnings([kloud, c('kloud', 'Claude', { fuzzy: false })]).has(kloud)).toBe(false);
  expect(SNIPPET_MAX).toBe(5000);
});

test('the warnings stay quick for a dictionary of thousands', () => {
  const big = [];
  for (let i = 0; i < 2000; i++) big.push(c(`heard ${i} way`, `Word${i}`, { alsoHeard: [`other ${i}`] }));
  for (let i = 0; i < 200; i++) big.push(s(`my phrase number ${i}`, `Text ${i}`));
  const t0 = performance.now();
  const w = snippetWarnings(big);
  const ms = performance.now() - t0;
  expect(w.size).toBeGreaterThan(0); // "my phrase number 1" and "…11" are close
  expect(ms).toBeLessThan(250);
});
