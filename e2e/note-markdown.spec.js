// Notes' own markdown through the rich text editor, on its own (no app
// needed; the harness doc-markdown.spec.js uses too, support/richtext.js):
// src/lib/components/richtext/ — the page notes' My thoughts, the meeting
// notepad and the quick note are edited on (2026-10-31). A note stays
// markdown, so an older Yap reads it as before:
// - what Yap and people write in notes — meeting summaries with their
//   tasks, My thoughts typed as plain lines, a quick note — read and
//   written back untouched comes out character for character;
// - a task ticked, a word typed, a line added changes only its own lines;
// - plain text pasted (a dictation) goes in as its words, a spoken list
//   laid out by the cleanup a list;
// - where the words are (`sourceMap`): a selection on the page is the
//   stretch of the note's text it shows, and back — a transform's
//   selection, its result selected, a search's words.
import { test, expect } from '@playwright/test';
import { EditorState, TextSelection } from '@tiptap/pm/state';
import { readMarkdown, writeMarkdown, sourceMap, sourceRange, sourcePositions, plainSlice } from '../src/lib/components/richtext/richMarkdown.js';
import { withDom, withoutDom, roundTrip, edit, editWhole, find, changedLines, itemAt } from './support/richtext.js';

let schema;
test.beforeAll(() => {
  schema = withDom();
});
test.afterAll(() => withoutDom());

// A meeting's summary as Yap writes it (meetings/meeting_summary, the action
// plan) and as a model may: owners' headings, tasks, a task with a list in
// it, numbered decisions with blank lines between, a quote, a table.
const SUMMARY = [
  'The team agreed the Q3 budget is fine overall; only the venue deposit moved.',
  '',
  '## Action plan',
  '',
  '### Alice',
  '- [ ] Send the revised budget to finance (due: Friday)',
  '- [ ] Flag the moved deposit in the totals',
  '',
  '### You',
  '- [x] Go through the budget line by line',
  '- [ ] Pick up catering once the venue confirms:',
  '  - the headcount for both days',
  '  - dietary needs',
  '',
  '## Decisions',
  '1. Keep the offsite budget as it is.',
  '',
  '2. Report the deposit in **this** quarter.',
  '',
  '> Travel is up 12%, mostly flights for the offsite.',
  '',
  '| Line | Change | Note |',
  '|:---|---:|:---|',
  '| Travel | +12% | flights |',
  '| Venue deposit | moved | `Q3`, not `Q4` |',
  '',
].join('\n');

const PLAN = [
  '## Recap',
  'The team set the offsite budget and the agenda.',
  '- Budget',
  '- Venue',
  '',
  '## Action plan',
  '',
  '### You',
  '- [ ] Draft the agenda (due: tonight)',
  '',
  '### Priya',
  '- [ ] Send the revised budget (due: Friday)',
  '',
  '## Decisions',
  '- Hold the offsite in Lisbon — the flights are cheaper',
  '',
  '## Unassigned',
  '- [ ] Book the venue',
].join('\n');

// My thoughts as people type them in a plain box: lines one under another,
// rough bullets, times, prices, arrows, addresses, file paths, tags.
const THOUGHTS = [
  'Ask Sam about the boiler\nCall the plumber 10:30\n- fix the gate latch\n- renew the permit',
  'TODO: book the vet -> before Friday\n50% off at the garden centre, 3 * 4 = 12\nidea: a_b_c naming, C:\\Users\\example\\notes.txt',
  '#garden #spring\nhttps://example.com/seeds and someone@example.com\n<3 the new layout',
  'Priya: the deposit moved\nme: fine, flag it\n\n\nsecond thought after a gap\n',
  '* one\n* two\n+ three\n\n1) first\n2) second',
  '[ ] not ticked yet\n[x] done already\n- [ ] a real task\n- [x] and a ticked one',
  'Tabs\tin\tlines   \nand trailing spaces   \n',
  'Quick note: milk, eggs, flour\r\nand the window cleaner\r\n',
  '   indented by three\n    and four (code)\n',
  '',
  ' ',
  '\n\n',
];

// A dictation's words, as Yap's cleanup writes them.
const DICTATED =
  'So the plan is to ship the beta on Friday, then gather feedback over the weekend. ' +
  "Priya will handle the release notes and I'll check the installer on a clean machine.";

test('notes read and written back untouched come out character for character', () => {
  for (const md of [SUMMARY, PLAN, ...THOUGHTS, DICTATED, `${DICTATED}\n\nSecond paragraph.\n`]) expect(roundTrip(md), JSON.stringify(md)).toBe(md);
  // CRLF and a title line's absence alike.
  const crlf = SUMMARY.replace(/\n/g, '\r\n');
  expect(roundTrip(crlf)).toBe(crlf);
});

test("a summary's task ticked changes its own line only, as Rust's tick does (notes/tasks.rs)", () => {
  for (const [md, item] of [
    [SUMMARY, 'Flag the moved deposit in the totals'],
    [SUMMARY, 'Pick up catering once the venue confirms:'],
    [PLAN, 'Book the venue'],
  ]) {
    const out = edit(md, (s) => {
      const pos = itemAt(s.doc, item);
      return s.tr.setNodeMarkup(pos, undefined, { ...s.doc.nodeAt(pos).attrs, checked: true });
    });
    const line = md.split('\n').find((l) => l.includes(item));
    expect(changedLines(md, out)).toEqual([[line, line.replace('[ ]', '[x]')]]);
  }
  // Unticked again: back to the note as it was.
  const back = edit(SUMMARY, (s) => {
    const pos = itemAt(s.doc, 'Go through the budget line by line');
    return s.tr.setNodeMarkup(pos, undefined, { ...s.doc.nodeAt(pos).attrs, checked: false });
  });
  expect(changedLines(SUMMARY, back)).toEqual([['- [x] Go through the budget line by line', '- [ ] Go through the budget line by line']]);
});

test('typed in My thoughts: a word changes its own line, Enter makes a paragraph, the rest as typed in the box', () => {
  const md = THOUGHTS[0];
  // A word in the second of three lines (one paragraph, its line breaks kept).
  const word = edit(md, (s) => s.tr.insertText('back ', find(s.doc, 'plumber') + 'plumber '.length));
  expect(changedLines(md, word)).toEqual([['Call the plumber 10:30', 'Call the plumber back 10:30']]);
  // A bullet's words.
  const item = edit(md, (s) => s.tr.insertText(' online', find(s.doc, 'renew the permit') + 'renew the permit'.length));
  expect(changedLines(md, item)).toEqual([['- renew the permit', '- renew the permit online']]);
  // Enter at the end of a line: a paragraph of its own, a blank line before it.
  const split = edit('Buy milk', (s) => {
    const end = s.doc.content.size - 1;
    return s.tr.split(end).insertText('Call Sam', end + 2);
  });
  expect(split).toBe('Buy milk\n\nCall Sam');
  // The rough notes' markup characters, typed as words: still words.
  const rough = edit('Plain.', (s) => s.tr.insertText('3 * 4 = 12 -> done, #tag, C:\\temp', 1, 7));
  expect(readMarkdown(schema, rough).doc.textContent).toBe('3 * 4 = 12 -> done, #tag, C:\\temp');
  // A tick in My thoughts' own task.
  const ticked = edit(THOUGHTS[5], (s) => {
    const pos = itemAt(s.doc, 'a real task');
    return s.tr.setNodeMarkup(pos, undefined, { ...s.doc.nodeAt(pos).attrs, checked: true });
  });
  expect(changedLines(THOUGHTS[5], ticked)).toEqual([['- [ ] a real task', '- [x] a real task']]);
});

test('plain text pasted or dictated: one line as its words, several as the box held them (a spoken list a list)', () => {
  const paste = (into, text, at = null) =>
    edit(into, (s) => {
      const pos = at == null ? s.doc.content.size - 1 : at(s.doc);
      const tr = s.tr.setSelection(TextSelection.create(s.doc, pos));
      return tr.replaceSelection(plainSlice(schema, text, []));
    });
  // One line: its words, at the caret, whatever they look like.
  expect(paste('Buy flour', 'eggs and ', (d) => find(d, 'flour'))).toBe('Buy eggs and flour');
  for (const words of ['- not a list', '1. not numbered', '# not a heading', '**not bold**', 'a | b']) {
    const out = paste('', words);
    expect(readMarkdown(schema, out).doc.firstChild.type.name, words).toBe('paragraph');
    expect(readMarkdown(schema, out).doc.textContent, words).toBe(words);
  }
  // Several lines: a list the cleanup laid out, after the words before it.
  const list = paste('', 'Shopping:\n1. Eggs\n2. Milk');
  expect(list).toBe('Shopping:\n\n1. Eggs\n2. Milk');
  const kinds = [];
  readMarkdown(schema, list).doc.forEach((n) => kinds.push(n.type.name));
  expect(kinds).toEqual(['paragraph', 'orderedList']);
  // Lines of words keep their line breaks; a blank line is a new paragraph.
  expect(paste('', 'Line one\nLine two\n\nAfter a gap')).toBe('Line one\nLine two\n\nAfter a gap');
  // Into the middle of a line: the first and last lines join the words either side.
  expect(paste('Before after', 'one\ntwo', (d) => find(d, 'after'))).toBe('Before one\ntwoafter');
});

/** `md` read, written, and where its words are. */
function located(md, change = null) {
  const w = editWhole(md, change);
  return { ...w, map: sourceMap(w.doc, w.text, w.blocks) };
}

test("where the words are: every character of the page found where it's written in the note", () => {
  for (const md of [SUMMARY, PLAN, ...THOUGHTS.filter((t) => t.trim()), DICTATED]) {
    const { text, map, doc } = located(md);
    // Every character of the page, once each, in order.
    let chars = 0;
    doc.descendants((n) => {
      if (n.isText) chars += n.text.length;
      if (n.type.name === 'hardBreak') chars += 1;
    });
    expect(map.length, JSON.stringify(md.slice(0, 30))).toBe(chars);
    for (let i = 1; i < map.length; i++) {
      expect(map[i][0]).toBeGreaterThan(map[i - 1][0]);
      expect(map[i][1]).toBeGreaterThan(map[i - 1][1]);
    }
    // Each at its own character (none of these notes escapes a word).
    for (const [pos, offset] of map) {
      const ch = doc.textBetween(pos, pos + 1, '\n', '\n') || '\n';
      expect(text[offset], `${JSON.stringify(md.slice(0, 30))} @${offset}`).toBe(ch);
    }
  }
});

test('a selection is the stretch of the note it shows, and back: a word, lines, across marks and escapes', () => {
  const pick = (md, from, to) => {
    const { text, map, doc } = located(md);
    const a = find(doc, from);
    const b = find(doc, to) + to.length;
    const r = sourceRange(map, a, b, { length: text.length });
    return { text, map, r, a, b, slice: text.slice(r.start, r.end) };
  };
  // The transform spec's own note: the middle line, exactly (notes-history.spec.js).
  const LINE = 'so basically we should like ship it on friday';
  const note = `Agenda\n${LINE}\nThanks all`;
  const mid = pick(note, 'so basically', 'friday');
  expect(mid.slice).toBe(LINE);
  expect(mid.r).toEqual({ start: 'Agenda\n'.length, end: 'Agenda\n'.length + LINE.length });
  // And back: those offsets select those words on the page.
  expect(sourcePositions(mid.map, mid.r.start, mid.r.end)).toEqual({ from: mid.a, to: mid.b });
  // A bulleted item's words, not its marker; a task's, not its box.
  expect(pick(THOUGHTS[0], 'fix', 'latch').slice).toBe('fix the gate latch');
  expect(pick(SUMMARY, 'Send', 'Friday)').slice).toBe('Send the revised budget to finance (due: Friday)');
  // Across a bold word: its markup goes with it.
  expect(pick('Make this **loud** now.', 'this', 'loud').slice).toBe('this **loud');
  expect(pick('Make this **loud** now.', 'this', 'now').slice).toBe('this **loud** now');
  // Words written escaped: the stretch that says them.
  const escaped = edit('Plain.', (s) => s.tr.insertText('1. not a list and *not* bold', 1, 7));
  expect(pick(escaped, 'not a', 'list').slice).toBe('not a list');
  expect(pick(escaped, 'not', 'bold').slice).toBe('not a list and \\*not\\* bold');
  // Across lines and blocks.
  expect(pick(SUMMARY, 'Alice', 'deposit in').slice).toBe('Alice\n- [ ] Send the revised budget to finance (due: Friday)\n- [ ] Flag the moved deposit in');
  // A cell of a table.
  expect(pick(SUMMARY, 'Venue deposit', 'Venue deposit').slice).toBe('Venue deposit');
});

test("a caret is where it is in the note: a line's end, its start, an empty line", () => {
  const md = 'Buy milk\nCall Sam\n\nBook the vet';
  const { text, map, doc } = located(md);
  const caret = (pos) => {
    const $p = doc.resolve(pos);
    return sourceRange(map, pos, pos, { length: text.length, block: $p.parent.isTextblock ? $p.start() : null }).start;
  };
  // The end of "Buy milk", the start of "Call Sam" (after the line break), the end of the paragraph.
  expect(caret(find(doc, 'Buy milk') + 'Buy milk'.length)).toBe('Buy milk'.length);
  expect(caret(find(doc, 'Call Sam'))).toBe('Buy milk\n'.length);
  expect(caret(find(doc, 'Call Sam') + 'Call Sam'.length)).toBe('Buy milk\nCall Sam'.length);
  // The start of the next paragraph.
  expect(caret(find(doc, 'Book'))).toBe('Buy milk\nCall Sam\n\n'.length);
  // Offsets back to a caret: after the character before it.
  expect(sourcePositions(map, 'Buy milk'.length, 'Buy milk'.length)).toEqual({ from: find(doc, 'Buy milk') + 8, to: find(doc, 'Buy milk') + 8 });
  expect(sourcePositions(map, md.length, md.length)).toEqual({ from: doc.content.size - 1, to: doc.content.size - 1 });
  // A transform's result put in place of a line (notes/transform.rs) and selected.
  const POLISHED = 'We should ship it on Friday.';
  const after = `Agenda\n${POLISHED}\nThanks all`;
  const l = located(after);
  const start = 'Agenda\n'.length;
  const at = sourcePositions(l.map, start, start + POLISHED.length);
  expect(l.doc.textBetween(at.from, at.to)).toBe(POLISHED);
  // A whole-note selection (Select all) is the note's text from its first word to its last.
  const all = sourceRange(l.map, 0, l.doc.content.size, { length: after.length });
  expect(after.slice(all.start, all.end)).toBe(after);
});

test('an empty note, and one of only spaces: nothing to find, a caret at the start', () => {
  for (const md of ['', ' ', '\n\n']) {
    const { map } = located(md);
    expect(map).toEqual([]);
    expect(sourceRange(map, 1, 1, { length: md.length })).toEqual({ start: 0, end: 0 });
    expect(sourcePositions(map, 0, 0)).toBeNull();
  }
  // A state the editor can make from it: an empty paragraph, typed into.
  const typed = edit('', (s) => s.tr.insertText('First words', 1));
  expect(typed).toBe('First words');
  expect(EditorState.create({ doc: readMarkdown(schema, '').doc, schema }).doc.childCount).toBe(1);
  expect(writeMarkdown(schema, readMarkdown(schema, '').doc).text).toBe('');
});
