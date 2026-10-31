// The rich text editor's markdown, in and out, without the app
// (src/lib/components/richtext/: richMarkdown.js over richSchema.js, TipTap
// on ProseMirror): the harness doc-markdown.spec.js (Chat's documents) and
// note-markdown.spec.js (notes: summaries with tasks, My thoughts, the
// quick note) share — read, change, written back. ProseMirror's model
// needs a DOM: `withDom()` in a spec's beforeAll / afterAll.
import { JSDOM } from 'jsdom';
import { EditorState } from '@tiptap/pm/state';
import { richSchema } from '../../src/lib/components/richtext/richSchema.js';
import { readMarkdown, writeMarkdown } from '../../src/lib/components/richtext/richMarkdown.js';

let dom = null;
let schema = null;

/** A DOM for the editor's model (once per spec file), and its schema. */
export function withDom() {
  dom = new JSDOM('');
  globalThis.window = dom.window;
  schema = richSchema();
  return schema;
}

export function withoutDom() {
  delete globalThis.window;
  dom?.window.close();
  dom = null;
}

/** The editor's schema (after `withDom`). */
export const theSchema = () => schema;

/** `md` read and written back, untouched. */
export function roundTrip(md) {
  const r = readMarkdown(schema, md);
  return writeMarkdown(schema, r.doc, { kept: r.kept, tail: r.tail, blank: r.blank }).text;
}

/** `md` read, `change(state) → tr` applied, written back. */
export function edit(md, change) {
  const r = readMarkdown(schema, md);
  const state = EditorState.create({ doc: r.doc, schema });
  const tr = change(state);
  return writeMarkdown(schema, tr.doc, { kept: r.kept, tail: r.tail, blank: r.blank }).text;
}

/** `md` read, changed, written back: `{text, blocks, doc}`. */
export function editWhole(md, change) {
  const r = readMarkdown(schema, md);
  const state = EditorState.create({ doc: r.doc, schema });
  const doc = change ? change(state).doc : r.doc;
  return { ...writeMarkdown(schema, doc, { kept: r.kept, tail: r.tail, blank: r.blank }), doc };
}

/** Where `text` starts in the document (the position before its first character). */
export function find(doc, text) {
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
export function changedLines(a, b) {
  const x = a.split('\n');
  const y = b.split('\n');
  const out = [];
  for (let i = 0; i < Math.max(x.length, y.length); i++) if (x[i] !== y[i]) out.push([x[i], y[i]]);
  return out;
}

/** The list item whose words are `text`: its position. */
export function itemAt(doc, text) {
  let pos = null;
  doc.descendants((n, p) => {
    if (pos == null && n.type.name === 'listItem' && n.firstChild?.textContent === text) pos = p;
  });
  if (pos == null) throw new Error(`no item "${text}"`);
  return pos;
}
