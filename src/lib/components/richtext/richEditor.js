// Yap's rich text editor (2026-10-31; after Claude's own documents, which
// edit on TipTap and ProseMirror too — none of their code): Chat's document
// panel's Preview, and notes' My thoughts, the meeting notepad's and the
// quick note's page (RichNote.svelte). Its own chunk, loaded the first time
// it's wanted. The note stays markdown (richMarkdown.js): what comes in is
// read block by block, and every change goes out as the markdown of the
// block it's in, the rest exactly as it was — saved as the place's own
// typing is (Chat: chatDoc `input`, merged with Chat's writing in Rust;
// notes: their save as you type, a typed or dictated version).
//
// - `load(text)`: the note's markdown, from outside (Chat's writing merged,
//   Undo, another window): only the blocks that differ are replaced, so
//   the caret and the person's own undo stay; never in the editor's undo
//   history. `mark`: the blocks that changed are washed a moment, as the
//   page marks a change that landed.
// - The toolbar's commands (`run`) and what's on at the caret (`onstate`):
//   text style, code, tables, checklists, the insert menu.
// - `selectionLines()`: the note lines a selection covers ("Ask about
//   this" / "Change this…").
// - Where the words are (`sourceRange`, `selectSource`): the selection as a
//   stretch of the note's text and back (a transform on it, its result
//   selected; a search's words); with `textbox`, the editor's own element
//   answers as a text box does (`value`, `selectionStart` / `End`,
//   `setSelectionRange`, `select`, `readOnly`), so what works on a note's
//   text box works on the page.
// - `insertPlain(piece)`: words put in at the caret as plain text (a
//   dictation into the quick note: Ctrl+Z takes it back out in one step).
//   A paste of plain text (Yap pasting a dictation) goes in the same way
//   (richMarkdown.js `plainSlice`: one line as its words, several as the
//   markdown the note's box would have held — a spoken list a list);
//   `onchange`'s `paste` says it was one.
// - Links: a click puts the caret in one; Ctrl+click opens it.
import { Editor, Extension } from '@tiptap/core';
import { Plugin, PluginKey, TextSelection } from '@tiptap/pm/state';
import { Decoration, DecorationSet } from '@tiptap/pm/view';
import { closeHistory } from '@tiptap/pm/history';
import { richExtensions, rawRenderer, SAFE_URL } from './richSchema.js';
import { readMarkdown, writeMarkdown, splitHead, sourceMap, sourceRange, sourcePositions, plainSlice } from './richMarkdown.js';
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

/**
 * The editor in `mount` (its root, which becomes the editable element), on
 * `text` (the note's markdown; its title line `title`, when it has one, is
 * shown above the page). `onchange(markdown, {paste})`: the person changed
 * it (`paste`: by pasting). `onstate(state)`: what's on at the caret, after
 * every change. `label`: the page's name to a screen reader;
 * `placeholder()`: what an empty one says; `rawTitle`: where a raw block is
 * edited; `textbox`: `mount` answers as a text box does (above).
 */
export function createRichEditor({
  mount,
  text,
  title,
  editable = true,
  label = 'Document text',
  placeholder = null,
  rawTitle = null,
  textbox = false,
  onchange = () => {},
  onstate = () => {},
}) {
  let head = '';
  let kept = new Map();
  let tail = [''];
  let blank = false;
  let last = null;
  let docTitle = title ?? '';
  let clearTimer = 0;

  const editor = new Editor({
    element: { mount },
    extensions: [...richExtensions({ placeholder, rawTitle }), Landed],
    injectCSS: false,
    editable,
    enableContentCheck: false,
    editorProps: {
      attributes: { role: 'textbox', 'aria-multiline': 'true', 'aria-label': label, spellcheck: 'true' },
      handleDOMEvents: { click: linkClick, auxclick: linkClick },
      clipboardTextParser: (pasted, $context) => plainSlice(editor.schema, pasted, $context.marks()),
    },
    onTransaction: ({ transaction }) => {
      // (Only what can change the pressed buttons: not the page losing the
      // focus, which happens as a window takes it away from a page it removes.)
      if (transaction.docChanged || transaction.selectionSet || transaction.storedMarksSet) onstate(state());
      if (!transaction.docChanged || transaction.getMeta(OUTSIDE)) return;
      const md = write().text;
      if (md === last) return;
      last = md;
      onchange(md, { paste: transaction.getMeta('paste') === true || transaction.getMeta('uiEvent') === 'paste' });
    },
  });

  function write() {
    return writeMarkdown(editor.schema, editor.state.doc, { kept, head, tail, blank });
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
    blank = read.blank;
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

  // ---- where the words are -------------------------------------------------------------

  /** The note's markdown now, and where each of the page's characters is in it. */
  function located() {
    const w = write();
    return { text: w.text, map: sourceMap(editor.state.doc, w.text, w.blocks) };
  }

  /** The selection as a stretch of the note's markdown, `{start, end}` (a caret: both the same). */
  function sourceSelection() {
    if (editor.isDestroyed) return { start: 0, end: 0 };
    const { text: md, map } = located();
    const sel = editor.state.selection;
    return sourceRange(map, sel.from, sel.to, { length: md.length, block: sel.$from.parent.isTextblock ? sel.$from.start() : null });
  }

  /** Select the stretch `start`..`end` of the note's markdown on the page (shown, not focused). */
  function selectSource(start, end) {
    if (editor.isDestroyed) return;
    const { map } = located();
    const at = sourcePositions(map, Math.max(0, start ?? 0), Math.max(0, end ?? start ?? 0));
    const doc = editor.state.doc;
    const size = doc.content.size;
    const sel = at
      ? TextSelection.between(doc.resolve(Math.min(at.from, size)), doc.resolve(Math.min(at.to, size)))
      : TextSelection.atStart(doc);
    editor.view.dispatch(editor.state.tr.setSelection(sel).scrollIntoView());
  }

  /**
   * Words in at the caret as plain text, as typing would put them: `piece(before,
   * after)` gives them from the line's text either side of the selection
   * (a new line after them when they end in "\n"); `atEnd`: at the end of
   * the note instead. One step for Ctrl+Z. Returns whether anything went in.
   */
  function insertPlain(piece, { atEnd = false } = {}) {
    if (editor.isDestroyed || !editor.isEditable) return false;
    if (atEnd) editor.view.dispatch(editor.state.tr.setSelection(TextSelection.atEnd(editor.state.doc)));
    const { $from, $to, from, to } = editor.state.selection;
    const before = $from.parent.isTextblock ? $from.parent.textBetween(0, $from.parentOffset, '\n', '\n') : '';
    const after = $to.parent.isTextblock ? $to.parent.textBetween($to.parentOffset, $to.parent.content.size, '\n', '\n') : '';
    const words = String(piece(before, after) ?? '');
    if (!words) return false;
    const newline = words.endsWith('\n');
    const body = newline ? words.slice(0, -1) : words;
    const tr = editor.state.tr;
    if (body.includes('\n')) tr.replaceRange(from, to, plainSlice(editor.schema, body, $from.marks()));
    else if (body) tr.insertText(body, from, to);
    else if (from !== to) tr.deleteRange(from, to);
    // Its own step in the undo history: never merged with the typing before.
    editor.view.dispatch(closeHistory(tr).scrollIntoView());
    if (newline) editor.commands.enter();
    // Nor with the typing after it.
    editor.view.dispatch(closeHistory(editor.state.tr));
    return true;
  }

  // ---- the editor's element as a text box ---------------------------------------------

  const BOX = ['value', 'selectionStart', 'selectionEnd', 'readOnly', 'setSelectionRange', 'select'];
  if (textbox) {
    Object.defineProperties(mount, {
      value: { configurable: true, get: () => last ?? '' },
      selectionStart: { configurable: true, get: () => sourceSelection().start },
      selectionEnd: { configurable: true, get: () => sourceSelection().end },
      readOnly: { configurable: true, get: () => editor.isDestroyed || !editor.isEditable },
      setSelectionRange: { configurable: true, value: (a, b) => selectSource(a, b) },
      select: { configurable: true, value: () => !editor.isDestroyed && editor.commands.selectAll() },
    });
  }

  load(text);
  onstate(state());

  return {
    editor,
    load,
    run,
    selectionLines,
    markBlocks,
    sourceSelection,
    selectSource,
    insertPlain,
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
    /** The page has the keyboard. */
    get focused() {
      return !editor.isDestroyed && editor.isFocused;
    },
    /** What an empty page says changed: drawn again. */
    refresh() {
      if (!editor.isDestroyed) editor.view.dispatch(editor.state.tr.setMeta(OUTSIDE, true).setMeta('addToHistory', false));
    },
    destroy() {
      clearTimeout(clearTimer);
      if (textbox) for (const k of BOX) delete mount[k];
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
