// Chat's document, editable as a page (2026-10-31; after Claude's own
// documents, which edit on TipTap and ProseMirror too — none of their code):
// the panel's Preview, loaded the first time it's shown (its own chunk, on
// top of DocPanel's). The note stays markdown (docMarkdown.js): what comes
// in is read block by block, and every change goes out as the markdown of
// the block it's in, the rest exactly as it was — saved as the Source box's
// typing is (chatDoc `input`: merged with Chat's writing in Rust, a typed
// version that Undo and Version history know).
//
// - `load(text)`: the document's markdown, from outside (Chat's writing
//   merged, Undo, another window): only the blocks that differ are
//   replaced, so the caret and the person's own undo stay; never in the
//   editor's undo history. `mark`: the blocks that changed are washed a
//   moment, as the page marks a change that landed.
// - The toolbar's commands (`run`) and what's on at the caret (`onstate`):
//   text style, code, tables, checklists, the insert menu.
// - `selectionLines()`: the note lines a selection covers ("Ask about
//   this" / "Change this…").
// - Links: a click puts the caret in one; Ctrl+click opens it.
import { Editor, Extension } from '@tiptap/core';
import { Plugin, PluginKey } from '@tiptap/pm/state';
import { Decoration, DecorationSet } from '@tiptap/pm/view';
import { docExtensions, rawRenderer, SAFE_URL } from './docSchema.js';
import { readMarkdown, writeMarkdown, splitHead } from './docMarkdown.js';
import { renderMarkdown } from '../../core/markdown.js';
import { openExternalLink } from '../../core/externalLinks.js';

// A raw block is drawn as the page draws it.
rawRenderer((md) => renderMarkdown(md));

/** A transaction that brought the document's text from outside: not news, not undoable. */
const OUTSIDE = 'yapOutside';

// The blocks a change from outside touched, washed a moment (`data-changed`).
const landedKey = new PluginKey('yapLanded');
const Landed = Extension.create({
  name: 'yapLanded',
  addProseMirrorPlugins() {
    return [
      new Plugin({
        key: landedKey,
        state: {
          init: () => DecorationSet.empty,
          apply(tr, set) {
            const m = tr.getMeta(landedKey);
            if (m === 'clear') return DecorationSet.empty;
            if (m) return DecorationSet.create(tr.doc, m.map(([from, to]) => Decoration.node(from, to, { 'data-changed': '' })));
            return set.map(tr.mapping, tr.doc);
          },
        },
        props: { decorations: (state) => landedKey.getState(state) },
      }),
    ];
  },
});

/** The heading levels the text style menu offers (`#`, `##`, `###`). */
export const STYLES = [
  { id: 'paragraph', label: 'Paragraph' },
  { id: 1, label: 'Heading 1' },
  { id: 2, label: 'Heading 2' },
  { id: 3, label: 'Heading 3' },
];

/**
 * The editor in `mount` (its root: the page's `.docmd`), on `text` (the
 * note's markdown, its title line `title` shown above the page).
 * `onchange(markdown)`: the person changed it. `onstate(state)`: what's on
 * at the caret, after every change.
 */
export function createDocEditor({ mount, text, title, editable = true, onchange = () => {}, onstate = () => {} }) {
  let head = '';
  let kept = new Map();
  let tail = [''];
  let last = null;
  let docTitle = title ?? '';
  let clearTimer = 0;

  const editor = new Editor({
    element: { mount },
    extensions: [...docExtensions(), Landed],
    injectCSS: false,
    editable,
    enableContentCheck: false,
    editorProps: {
      attributes: { role: 'textbox', 'aria-multiline': 'true', 'aria-label': 'Document text', spellcheck: 'true' },
      handleDOMEvents: { click: linkClick, auxclick: linkClick },
    },
    onTransaction: ({ transaction }) => {
      onstate(state());
      if (!transaction.docChanged || transaction.getMeta(OUTSIDE)) return;
      const md = write().text;
      if (md === last) return;
      last = md;
      onchange(md);
    },
  });

  function write() {
    return writeMarkdown(editor.schema, editor.state.doc, { kept, head, tail });
  }

  /** The document's markdown from outside; `mark`: wash the blocks that changed. */
  function load(next, { mark = false, title: t = docTitle } = {}) {
    if (editor.isDestroyed) return;
    docTitle = t ?? '';
    const value = String(next ?? '');
    if (value === last) return;
    const { head: h, body } = splitHead(value, docTitle);
    const read = readMarkdown(editor.schema, body);
    head = h;
    kept = read.kept;
    tail = read.tail;
    last = value;
    const now = editor.state.doc;
    const start = now.content.findDiffStart(read.doc.content);
    if (start == null) return;
    let { a: endA, b: endB } = now.content.findDiffEnd(read.doc.content);
    const overlap = start - Math.min(endA, endB);
    if (overlap > 0) {
      endA += overlap;
      endB += overlap;
    }
    const tr = editor.state.tr.replace(start, endA, read.doc.slice(start, endB)).setMeta(OUTSIDE, true).setMeta('addToHistory', false);
    editor.view.dispatch(tr);
    if (mark) {
      const touched = [];
      editor.state.doc.forEach((node, offset, i) => {
        if (offset < endB && offset + node.nodeSize > start) touched.push(i);
      });
      markBlocks(touched);
    }
  }

  /** Wash the document's blocks `indexes` a moment (`ms`), as a change that landed. */
  function markBlocks(indexes, ms = 1700) {
    if (editor.isDestroyed || !indexes.length) return;
    const ranges = [];
    editor.state.doc.forEach((node, offset, i) => {
      if (indexes.includes(i) && node.type.name !== 'yapSection') ranges.push([offset, offset + node.nodeSize]);
    });
    editor.view.dispatch(editor.state.tr.setMeta(landedKey, ranges).setMeta(OUTSIDE, true).setMeta('addToHistory', false));
    clearTimeout(clearTimer);
    clearTimer = setTimeout(() => {
      if (!editor.isDestroyed) editor.view.dispatch(editor.state.tr.setMeta(landedKey, 'clear').setMeta(OUTSIDE, true).setMeta('addToHistory', false));
    }, ms);
  }

  /** What's on at the caret: the toolbar's pressed buttons. */
  function state() {
    if (editor.isDestroyed) return {};
    const { $from } = editor.state.selection;
    let style = null;
    if (editor.isActive('paragraph')) style = 'paragraph';
    for (const level of [1, 2, 3, 4, 5, 6]) if (editor.isActive('heading', { level })) style = level;
    let item = null;
    for (let d = $from.depth; d > 0; d--) {
      if ($from.node(d).type.name === 'listItem') {
        item = $from.node(d);
        break;
      }
    }
    return {
      style,
      code: editor.isActive('code'),
      codeBlock: editor.isActive('codeBlock'),
      checklist: item?.attrs.checked != null,
      table: editor.isActive('table'),
      quote: editor.isActive('blockquote'),
      bullets: editor.isActive('bulletList'),
      numbers: editor.isActive('orderedList'),
      // A block the toolbar can't style here (a section still to write, a raw block).
      atom: editor.state.selection.node?.isAtom ?? false,
    };
  }

  /** Checklist on or off for the list items the selection is in (a list first if it's in none). */
  function toggleChecklist() {
    const items = () => {
      const { from, to } = editor.state.selection;
      const out = [];
      editor.state.doc.nodesBetween(from, to, (node, pos) => {
        if (node.type.name !== 'listItem') return true;
        // Its own first line is in the selection (not just a nested item of it).
        const firstEnd = pos + 1 + node.firstChild.nodeSize;
        if (pos + 1 <= to && firstEnd >= from) out.push([node, pos]);
        return true;
      });
      return out;
    };
    const chain = editor.chain().focus();
    // Checklists are bulleted (markdown's "- [ ] "): a numbered list or none becomes one.
    if (!editor.isActive('listItem') || editor.isActive('orderedList')) chain.toggleBulletList();
    chain.run();
    const list = items();
    if (!list.length) return;
    const on = list.every(([n]) => n.attrs.checked != null);
    const tr = editor.state.tr;
    for (const [n, pos] of list) tr.setNodeMarkup(pos, undefined, { ...n.attrs, checked: on ? null : false });
    editor.view.dispatch(tr);
  }

  /** A toolbar command by name. */
  function run(name, arg) {
    if (editor.isDestroyed || !editor.isEditable) return;
    const c = editor.chain().focus();
    switch (name) {
      case 'style':
        (arg === 'paragraph' ? c.setParagraph() : c.setHeading({ level: arg })).run();
        break;
      case 'code':
        c.toggleCode().run();
        break;
      case 'table':
        c.insertTable({ rows: 3, cols: 3, withHeaderRow: true }).run();
        break;
      case 'rowAfter':
        c.addRowAfter().run();
        break;
      case 'columnAfter':
        c.addColumnAfter().run();
        break;
      case 'deleteRow':
        c.deleteRow().run();
        break;
      case 'deleteColumn':
        c.deleteColumn().run();
        break;
      case 'deleteTable':
        c.deleteTable().run();
        break;
      case 'checklist':
        toggleChecklist();
        break;
      case 'rule': {
        c.setHorizontalRule().run();
        // The caret on a line of its own under it, not at the start of the next block.
        const { $from } = editor.state.selection;
        const at = $from.depth ? $from.index(0) : -1;
        if (at > 0 && editor.state.doc.child(at - 1).type.name === 'horizontalRule' && $from.parent.content.size > 0 && $from.parentOffset === 0) {
          const pos = $from.before(1);
          editor.chain().insertContentAt(pos, { type: 'paragraph' }).setTextSelection(pos + 1).run();
        }
        break;
      }
      case 'quote':
        c.toggleBlockquote().run();
        break;
      case 'bullets':
        c.toggleBulletList().run();
        break;
      case 'numbers':
        c.toggleOrderedList().run();
        break;
      case 'codeBlock':
        c.toggleCodeBlock().run();
        break;
      default:
        break;
    }
  }

  /**
   * The note's lines the selection covers, `{fromLine, toLine}` (0-based,
   * the title's line counted), or `null` with nothing selected.
   */
  function selectionLines() {
    const sel = editor.state.selection;
    if (sel.empty) return null;
    const { blocks } = write();
    const doc = editor.state.doc;
    const $a = doc.resolve(sel.from);
    const $b = doc.resolve(sel.to);
    let a = Math.min($a.index(0), doc.childCount - 1);
    let b = Math.min($b.depth === 0 ? $b.index(0) - 1 : $b.index(0), doc.childCount - 1);
    // A selection that ends at a block's very start ends in the one before.
    if (b > a && $b.depth > 0 && $b.parentOffset === 0 && $b.index($b.depth - 1) === 0) b -= 1;
    while (a <= b && !blocks[a]) a++;
    while (b >= a && !blocks[b]) b--;
    if (a > b) return null;
    return { fromLine: blocks[a][0], toLine: blocks[b][1] - 1 };
  }

  load(text);
  onstate(state());

  return {
    editor,
    load,
    run,
    selectionLines,
    markBlocks,
    /** The markdown the editor holds now. */
    get text() {
      return last;
    },
    setEditable(on) {
      if (!editor.isDestroyed && editor.isEditable !== on) editor.setEditable(on, false);
    },
    /** The caret into the page (`at`: 'end', 'start', or where it was). */
    focus(at = null) {
      if (!editor.isDestroyed) editor.commands.focus(at);
    },
    destroy() {
      clearTimeout(clearTimer);
      editor.destroy();
    },
  };
}

/** A click on a link puts the caret there; Ctrl+click (or the middle button) opens it. */
function linkClick(view, e) {
  const a = e.target instanceof Element ? e.target.closest('a.md-link') : null;
  if (!a || !view.dom.contains(a)) return false;
  e.preventDefault();
  e.stopPropagation();
  const href = a.getAttribute('href') || '';
  if ((e.ctrlKey || e.metaKey || e.button === 1) && SAFE_URL.test(href)) openExternalLink(href);
  return false;
}
