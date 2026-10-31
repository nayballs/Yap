// Chat's document editor's markdown, in and out, on its own (no app
// needed): src/lib/views/chat/docMarkdown.js over docSchema.js (TipTap on
// ProseMirror). The note stays markdown: read into the editor and written
// back untouched it must come out character for character, and a change
// must change only the block it's in — the line of a list item, the row of
// a table, a paragraph — however the rest was written.
import { test, expect } from '@playwright/test';
import { JSDOM } from 'jsdom';
import { EditorState, TextSelection } from '@tiptap/pm/state';
import { docSchema } from '../src/lib/views/chat/docSchema.js';
import { readMarkdown, writeMarkdown, splitHead } from '../src/lib/views/chat/docMarkdown.js';

let dom;
let schema;
test.beforeAll(() => {
  dom = new JSDOM('');
  globalThis.window = dom.window;
  schema = docSchema();
});
test.afterAll(() => {
  delete globalThis.window;
  dom.window.close();
});

/** `md` read and written back, untouched. */
const roundTrip = (md) => {
  const r = readMarkdown(schema, md);
  return writeMarkdown(schema, r.doc, { kept: r.kept, tail: r.tail }).text;
};

/** `md` read, `change(state) → tr` applied, written back. */
function edit(md, change) {
  const r = readMarkdown(schema, md);
  const state = EditorState.create({ doc: r.doc, schema });
  const tr = change(state);
  return writeMarkdown(schema, tr.doc, { kept: r.kept, tail: r.tail }).text;
}

/** Where `text` starts in the document (the position before its first character). */
function find(doc, text) {
  let at = -1;
  doc.descendants((node, pos) => {
    if (at >= 0) return false;
    if (node.isText) {
      const i = node.text.indexOf(text);
      if (i >= 0) at = pos + i;
    }
    return true;
  });
  if (at < 0) throw new Error(`"${text}" isn't in the document`);
  return at;
}

/** The lines `a` and `b` differ in (by index; same length assumed unless noted). */
const changedLines = (a, b) => {
  const x = a.split('\n');
  const y = b.split('\n');
  const out = [];
  for (let i = 0; i < Math.max(x.length, y.length); i++) if (x[i] !== y[i]) out.push([x[i], y[i]]);
  return out;
};

// A document as Chat writes them (fictional), and the ways people and
// models write markdown: every one must come back exactly.
const BUILD = `A careful, quiet build for a fictional ranger ([guides.example.com](https://guides.example.com/ranger)).

## Character creation

| Choice | Pick | Why |
|---|:---:|--:|
| Race | Elf | Sees in the dark, **+2 Wits** |
| Origin | Custom | Keeps every choice open \\| no quest |
| Talent | Lone Wolf | Doubles your points [1] |

- **Definitive note:** doubling stops at 40 (*from my own knowledge*).
- Keep \`Quick step\` for the swamp

## Level by level

1. Mind wave
2. Eyestrike

3. Shadow step, then **hide**

## Checklist

- [ ] Two rings of the fox
- [x] A short bow
* Mixed marker item

<!-- yap:section Gear | what to wear and where to find it -->

<!-- yap:section Tips -->
`;

const ODD = [
  'Plain paragraph\nwith a soft break and a hard one  \nhere.',
  '__Bold__ and _italic_ and ~~gone~~ and ``code with ` tick``.',
  'Setext heading\n==============\n\nAnother\n-------',
  '> A quote\n> over two lines\n>\n> - with a list',
  '```js\nconst x = 1;\n```\n\n~~~\nplain\n~~~\n\n    indented code',
  '***\n\n___\n\n- - -',
  '1) One\n2) Two\n\n7. Seven\n8. Eight',
  '- a\n  - nested\n    - deeper\n- b',
  '• small model bullet\n• another',
  'Escapes: \\*not bold\\*, 1\\. not a list, a \\# hash, fish &amp; chips.',
  'A bare https://example.com/page link, <https://example.com/x>, and someone@example.com.',
  '-\n\n- \n- item',
  '- # Heading in a list\n- second',
  '[ref]: https://example.com\n\nUses [it][ref].',
  'Emoji 😀 and accents: café, naïve.\n\n\n\nThree blank lines above.',
  '| Only | Header |\n|---|---|',
  'Trailing spaces   \nand tabs\t\n',
  '<b>not html</b> &lt;tag&gt;',
  '',
  '\n\n',
];

test('read and written back untouched, a document comes out character for character', () => {
  expect(roundTrip(BUILD)).toBe(BUILD);
  for (const md of ODD) expect(roundTrip(md), JSON.stringify(md)).toBe(md);
  // Without a trailing line break, with CRLF lines, with blank lines first.
  expect(roundTrip(BUILD.trimEnd())).toBe(BUILD.trimEnd());
  const crlf = BUILD.replace(/\n/g, '\r\n');
  expect(roundTrip(crlf)).toBe(crlf);
  expect(roundTrip(`\n\n${BUILD}`)).toBe(`\n\n${BUILD}`);
});

test('the title line stays above the page', () => {
  expect(splitHead('# Ranger build\n\nText', 'Ranger build')).toEqual({ head: '# Ranger build\n', body: '\nText' });
  expect(splitHead('# Something else\n\nText', 'Ranger build')).toEqual({ head: '', body: '# Something else\n\nText' });
  const { head, body } = splitHead(`# Ranger build\n\n${BUILD}`, 'Ranger build');
  const r = readMarkdown(schema, body);
  const out = writeMarkdown(schema, r.doc, { kept: r.kept, tail: r.tail, head });
  expect(out.text).toBe(`# Ranger build\n\n${BUILD}`);
  // Each block's lines in the whole note (the title's line counted).
  const lines = out.text.split('\n');
  const first = out.blocks.find(Boolean);
  expect(lines.slice(first[0], first[1])).toEqual([BUILD.split('\n')[0]]);
});

test('the editor holds every block as itself: tables, checklists, sections still to write', () => {
  const { doc } = readMarkdown(schema, BUILD);
  const kinds = [];
  doc.forEach((n) => kinds.push(n.type.name));
  expect(kinds).toEqual(['paragraph', 'heading', 'table', 'bulletList', 'heading', 'orderedList', 'heading', 'bulletList', 'yapSection', 'yapSection']);
  const table = doc.child(2);
  expect(table.childCount).toBe(4);
  expect(table.firstChild.firstChild.type.name).toBe('tableHeader');
  expect(table.firstChild.child(1).attrs.align).toBe('center');
  expect(table.child(2).child(2).textContent).toBe('Keeps every choice open | no quest');
  const tasks = doc.child(7);
  expect([tasks.child(0).attrs.checked, tasks.child(1).attrs.checked, tasks.child(2).attrs.checked]).toEqual([false, true, null]);
  expect(doc.child(8).attrs).toMatchObject({ key: 'Gear', intent: 'what to wear and where to find it' });
  // A list item that starts with a heading: kept raw, exactly.
  expect(readMarkdown(schema, '- # Heading in a list\n- second').doc.firstChild.type.name).toBe('rawBlock');
});

test('a change changes only its own block: a word, a cell, an item, a tick', () => {
  // A word in a paragraph.
  const word = edit(BUILD, (s) => s.tr.insertText('very ', find(s.doc, 'quiet')));
  expect(changedLines(BUILD, word)).toEqual([[BUILD.split('\n')[0], BUILD.split('\n')[0].replace('quiet', 'very quiet')]]);
  // A table cell: that row, and only it (the rule and the other rows as written).
  const cell = edit(BUILD, (s) => s.tr.insertText('Wood ', find(s.doc, 'Elf')));
  expect(changedLines(BUILD, cell)).toEqual([['| Race | Elf | Sees in the dark, **+2 Wits** |', '| Race | Wood Elf | Sees in the dark, **+2 Wits** |']]);
  // A list item.
  const item = edit(BUILD, (s) => s.tr.insertText('heavy ', find(s.doc, 'short bow')));
  expect(changedLines(BUILD, item)).toEqual([['- [x] A short bow', '- [x] A heavy short bow']]);
  // A checklist box ticked.
  const tick = edit(BUILD, (s) => {
    let pos = null;
    s.doc.descendants((n, p) => {
      if (pos == null && n.type.name === 'listItem' && n.textContent === 'Two rings of the fox') pos = p;
    });
    return s.tr.setNodeMarkup(pos, undefined, { ...s.doc.nodeAt(pos).attrs, checked: true });
  });
  expect(changedLines(BUILD, tick)).toEqual([['- [ ] Two rings of the fox', '- [x] Two rings of the fox']]);
  // A word in a numbered list with a blank line before its last item.
  const num = edit(BUILD, (s) => s.tr.insertText('then ', find(s.doc, 'Eyestrike')));
  expect(changedLines(BUILD, num)).toEqual([['2. Eyestrike', '2. then Eyestrike']]);
  // A word in a nested list, and in a quote.
  const nested = edit(ODD[7], (s) => s.tr.insertText('very ', find(s.doc, 'deeper')));
  expect(nested).toBe('- a\n  - nested\n    - very deeper\n- b');
  const quote = edit(ODD[3], (s) => s.tr.insertText('short ', find(s.doc, 'list')));
  expect(quote).toBe('> A quote\n> over two lines\n>\n> - with a short list');
  // Bold and italic written as they were (`__`, `_`) when their block changes.
  const marks = edit(ODD[1], (s) => s.tr.insertText('really ', find(s.doc, 'gone')));
  expect(marks).toBe('__Bold__ and _italic_ and really ~~gone~~ and ``code with ` tick``.');
});

test('a letter or a markup character typed anywhere: only its block changes, and the note reads back as typed', () => {
  for (const md of [BUILD, ODD[1], ODD[3], ODD[7], ODD[9], ODD[10]]) {
    const first = readMarkdown(schema, md);
    const places = [];
    first.doc.descendants((n, pos) => {
      if (n.isText) for (let k = 0; k <= n.text.length; k += 2) places.push(pos + k);
    });
    for (const [i, at] of places.entries()) {
      // A letter or markup typed; every ninth, a character deleted instead.
      const typed = ['Z', '*', '_', '`', '[', '|', '#', '~', ''][i % 9];
      const r = readMarkdown(schema, md);
      const state = EditorState.create({ doc: r.doc, schema });
      const one = typed || (state.doc.textBetween(at, Math.min(at + 1, state.doc.content.size)).length === 1 ? null : 'Z');
      const doc = (one == null ? state.tr.delete(at, at + 1) : state.tr.insertText(one, at)).doc;
      const out = writeMarkdown(schema, doc, { kept: r.kept, tail: r.tail }).text;
      const back = readMarkdown(schema, out).doc;
      const where = `${JSON.stringify(md.slice(0, 30))}: ${typed} at ${at}`;
      // It says what the page says: the same blocks, the same words.
      expect(back.childCount, where).toBe(doc.childCount);
      back.forEach((n, _o, k) => expect(n.type.name, where).toBe(doc.child(k).type.name));
      expect(back.textContent, where).toBe(doc.textContent);
      // Every other block as it was written.
      const typedIn = doc.resolve(at).index(0);
      first.doc.forEach((n, _o, k) => {
        if (k !== typedIn) expect(out, where).toContain(n.attrs.md);
      });
    }
  }
});

// Bold, italic, strike, code or a link put over any few characters — across
// words, at punctuation, over another mark's edge: markdown can't always
// say it as it is ("dark, **+2**" made italic up to the "2"), and then the
// mark takes in a word's letters or is left out of that block — but the
// words always read back as they are, never a stray "*" among them.
test('marks put over any few characters: the words always read back as they are', () => {
  const text = (d) => d.textBetween(0, d.content.size, '\n', ' ').replace(/\s+/g, ' ').trim();
  for (const md of [BUILD, ODD[1], ODD[3], ODD[10], 'Mixed ***strong em*** and *em **strong** em* and [a **bold** link](https://example.com/a_b) here.']) {
    const first = readMarkdown(schema, md);
    const spans = [];
    first.doc.descendants((n, pos) => {
      if (!n.isTextblock) return true;
      for (let a = pos + 1; a < pos + 1 + n.content.size; a += 3) for (const len of [1, 2, 4, 7]) if (a + len <= pos + 1 + n.content.size) spans.push([a, a + len]);
      return false;
    });
    for (const [a, b] of spans) {
      for (const name of ['bold', 'italic', 'code', 'strike', 'link', 'unbold']) {
        const r = readMarkdown(schema, md);
        let tr = EditorState.create({ doc: r.doc, schema }).tr;
        if (name === 'unbold') tr = tr.removeMark(a, b, schema.marks.bold);
        else tr = tr.addMark(a, b, name === 'link' ? schema.marks.link.create({ href: 'https://example.com/z' }) : schema.marks[name].create());
        const out = writeMarkdown(schema, tr.doc, { kept: r.kept, tail: r.tail }).text;
        const back = readMarkdown(schema, out).doc;
        const where = `${JSON.stringify(md.slice(0, 30))}: ${name} ${a}-${b}`;
        expect(back.childCount, where).toBe(tr.doc.childCount);
        expect(text(back), where).toBe(text(tr.doc));
      }
    }
  }
});

test('new blocks are plain markdown, and the words never turn into markup', () => {
  // A paragraph split in two: a blank line between.
  const split = edit('First half second half.', (s) => s.tr.split(find(s.doc, 'second')));
  expect(split).toBe('First half\n\nsecond half.');
  // Bold added to a word: **word**, a space at its edge kept outside.
  const bold = edit('Make this loud now.', (s) => s.tr.addMark(find(s.doc, 'loud'), find(s.doc, 'loud') + 5, schema.marks.bold.create()));
  expect(bold).toBe('Make this **loud** now.');
  // Typed words that would read as markup are escaped, only there.
  const typed = edit('Plain.', (s) => s.tr.insertText('1. not a list and *not* bold, C# fine', 1, 7));
  expect(typed).toBe('1\\. not a list and \\*not\\* bold, C\\# fine');
  const safe = edit('Plain.', (s) => s.tr.insertText('Price: 5 * 3 = 15, done', 1, 7));
  expect(safe).toBe('Price: 5 * 3 = 15, done');
  // A line of dashes or a section's placeholder typed as words: still words.
  expect(edit('Plain.', (s) => s.tr.insertText('---', 1, 7))).toBe('\\---');
  expect(edit('Plain.', (s) => s.tr.insertText('<!-- yap:section Fake | not one -->', 1, 7))).toBe('\\<!-- yap:section Fake \\| not one --\\>');
  expect(edit('Plain.', (s) => s.tr.insertText('# not a heading', 1, 7))).toBe('\\# not a heading');
  // A new table: a header row, the rule, cells.
  const table = edit('Intro.', (s) => {
    const cell = (type, t) => schema.nodes[type].create(null, schema.nodes.paragraph.create(null, t ? schema.text(t) : null));
    const t = schema.nodes.table.create(null, [
      schema.nodes.tableRow.create(null, [cell('tableHeader', 'Item'), cell('tableHeader', 'Why')]),
      schema.nodes.tableRow.create(null, [cell('tableCell', 'Rope'), cell('tableCell', 'a | b')]),
    ]);
    return s.tr.insert(s.doc.content.size, t);
  });
  expect(table).toBe('Intro.\n\n| Item | Why |\n|---|---|\n| Rope | a \\| b |');
  // A new checklist, rule, quote, code block.
  const more = edit('Intro.', (s) => {
    const p = (t) => schema.nodes.paragraph.create(null, schema.text(t));
    const list = schema.nodes.bulletList.create(null, [schema.nodes.listItem.create({ checked: false }, p('Pack')), schema.nodes.listItem.create({ checked: true }, p('Go'))]);
    return s.tr.insert(s.doc.content.size, [list, schema.nodes.horizontalRule.create(), schema.nodes.blockquote.create(null, p('Said once')), schema.nodes.codeBlock.create(null, schema.text('let a = 1;'))]);
  });
  expect(more).toBe('Intro.\n\n- [ ] Pack\n- [x] Go\n\n---\n\n> Said once\n\n```\nlet a = 1;\n```');
  // A heading made of a paragraph.
  const heading = edit('Intro.\n\nTips', (s) => s.tr.setBlockType(find(s.doc, 'Tips'), find(s.doc, 'Tips'), schema.nodes.heading, { level: 2 }));
  expect(heading).toBe('Intro.\n\n## Tips');
  // An item added to a numbered list: numbered in order.
  const added = edit('1. One\n2. Two', (s) => {
    const at = find(s.doc, 'One') + 3;
    return s.tr.setSelection(TextSelection.create(s.doc, at)).split(at, 2);
  });
  expect(added).toBe('1. One\n2. \n3. Two'.replace('2. \n', '2.\n'));
});
