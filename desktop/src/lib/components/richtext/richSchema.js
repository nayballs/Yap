// What Yap's rich text editor is made of (TipTap on ProseMirror, both MIT),
// shared by Chat's document panel and notes (Chat's documents first,
// 2026-10-31; factored out for notes the same day): the blocks and marks a
// note's markdown has, each able to say where it came from so it goes back
// out as it came in (richMarkdown.js). No Svelte state: the schema is built
// and tested alone.
//
// - TipTap's own nodes and marks for what markdown has — paragraphs,
//   headings, quotes, lists, code, rules, tables, hard breaks, bold,
//   italic, strike, code — with their keys, input rules ("## ", "- ",
//   "> ", "```") and commands, plus:
//   - `md` and `before` on every block that can come back verbatim (a
//     document's blocks, list items, table rows): its markdown as it was
//     read, and the lines before it; `md` never carried to a split-off half
//     (`keepOnSplit: false`);
//   - how each was written (a list's marker, a fence, a rule's `***`, a
//     mark's `__`), so a block that's changed is written back the same way;
//   - `checked` on a list item: a checklist item (`- [ ] `), its box ticked
//     in place; "[ ] " (or "[x] ") typed at the start of a bulleted item,
//     or of a paragraph of its own, makes one;
//   - `yapSection`: a section still to write (`<!-- yap:section … -->`),
//     a grey line nobody types into (chat/documents.rs fills it);
//   - `rawBlock`: anything else markdown can say that the editor can't edit
//     as such (a list item that starts with a heading…): drawn as the page
//     draws it, kept exactly, edited in Source (its tooltip: `rawTitle`);
//   - a plain link mark (web and mail links only, as markdown.js).
// - Headings are drawn a level down, as the page draws them (`#` → h2).
// - No images (markdown.js renders none: no remote fetches) and no
//   underline (markdown has none).
import { Node, Mark, Extension, InputRule, mergeAttributes, getSchema } from '@tiptap/core';
import { findWrapping } from '@tiptap/pm/transform';
import Document from '@tiptap/extension-document';
import Paragraph from '@tiptap/extension-paragraph';
import Text from '@tiptap/extension-text';
import Heading from '@tiptap/extension-heading';
import Bold from '@tiptap/extension-bold';
import Italic from '@tiptap/extension-italic';
import Strike from '@tiptap/extension-strike';
import Code from '@tiptap/extension-code';
import CodeBlock from '@tiptap/extension-code-block';
import Blockquote from '@tiptap/extension-blockquote';
import HorizontalRule from '@tiptap/extension-horizontal-rule';
import HardBreak from '@tiptap/extension-hard-break';
import { BulletList, OrderedList, ListItem, ListKeymap } from '@tiptap/extension-list';
import { Table, TableRow, TableHeader, TableCell } from '@tiptap/extension-table';
import { UndoRedo, Dropcursor, Gapcursor, Placeholder } from '@tiptap/extensions';

/** The blocks that can come back verbatim: their markdown as read, and the lines before them. */
const SOURCED = ['paragraph', 'heading', 'blockquote', 'bulletList', 'orderedList', 'listItem', 'codeBlock', 'horizontalRule', 'table', 'tableRow', 'yapSection', 'rawBlock'];

/** "[ ] " or "[x] " typed at the start of a line: a checklist item. */
const TASK_TYPED = /^\s*\[([ xX]?)\]\s$/;

/** A web or mail address: the only links a document keeps (as markdown.js). */
export const SAFE_URL = /^(https?:\/\/|mailto:)/i;

/** An attribute only the markdown needs: never drawn, never parsed from HTML. */
const hidden = (fallback = null, keepOnSplit = true) => ({ default: fallback, rendered: false, keepOnSplit, parseHTML: () => fallback });

const Source = Extension.create({
  name: 'yapSource',
  addGlobalAttributes() {
    return [{ types: SOURCED, attributes: { md: hidden(null, false), before: hidden(null, false) } }];
  },
});

// `#` is drawn as h2, `##` (a section) as h3…, as the page draws them;
// `data-level` takes it back to its level when it's pasted in again.
const DocHeading = Heading.extend({
  parseHTML() {
    return this.options.levels.map((level) => ({ tag: `h${level}`, getAttrs: (el) => ({ level: Number(el.getAttribute('data-level')) || level }) }));
  },
  renderHTML({ node, HTMLAttributes }) {
    return [`h${Math.min(node.attrs.level + 1, 5)}`, mergeAttributes(HTMLAttributes, { 'data-level': node.attrs.level }), 0];
  },
});

const DocBold = Bold.extend({ addAttributes: () => ({ markup: hidden('**') }) });
const DocItalic = Italic.extend({ addAttributes: () => ({ markup: hidden('*') }) });
// Code may be bold or in a link too (`**`x`**`), as markdown allows.
const DocCode = Code.extend({ excludes: '', addAttributes: () => ({ markup: hidden('`') }) });
const DocCodeBlock = CodeBlock.extend({
  addAttributes() {
    return { ...this.parent?.(), fence: hidden('```'), info: hidden(null) };
  },
});
const DocRule = HorizontalRule.extend({ addAttributes: () => ({ markup: hidden('---') }) });
// A soft line break (markdown.js keeps single line breaks) or a hard one ("  ").
const DocBreak = HardBreak.extend({ addAttributes: () => ({ soft: hidden(true) }) });
const DocBullets = BulletList.extend({
  addAttributes() {
    return { ...this.parent?.(), marker: hidden('-'), loose: hidden(false) };
  },
});
const DocNumbers = OrderedList.extend({
  addAttributes() {
    return { ...this.parent?.(), delim: hidden('.'), loose: hidden(false), seq: hidden(true) };
  },
});
const DocTable = Table.extend({
  addAttributes() {
    return { ...this.parent?.(), rule: hidden(null) };
  },
});
// A cell holds one line of text in markdown: one paragraph.
const DocCell = TableCell.extend({ content: 'paragraph' });
const DocHeader = TableHeader.extend({ content: 'paragraph' });

/**
 * A list item, or a checklist item (`checked` true / false): its box
 * before it, ticked in place; a new item split from a checklist item is
 * one too, not ticked.
 */
const DocItem = ListItem.extend({
  addAttributes() {
    return {
      checked: { default: null, keepOnSplit: false, parseHTML: (el) => (el.matches('[data-checked]') ? el.getAttribute('data-checked') === 'true' : null), renderHTML: (a) => (a.checked == null ? {} : { 'data-checked': String(a.checked) }) },
      marker: hidden(null, false),
      ind: hidden(null, false),
    };
  },
  addKeyboardShortcuts() {
    return {
      ...this.parent?.(),
      Enter: () => {
        const item = this.editor.state.selection.$from.node(-1);
        const task = item?.type.name === this.name && item.attrs.checked != null;
        return this.editor.commands.splitListItem(this.name, task ? { checked: false } : {});
      },
      // Ctrl+Enter ticks (or unticks) the checklist item the caret is in.
      'Mod-Enter': () => {
        const { $from } = this.editor.state.selection;
        for (let d = $from.depth; d > 0; d--) {
          const node = $from.node(d);
          if (node.type.name !== this.name) continue;
          if (node.attrs.checked == null) return false;
          this.editor.view.dispatch(this.editor.state.tr.setNodeMarkup($from.before(d), undefined, { ...node.attrs, checked: !node.attrs.checked }));
          return true;
        }
        return false;
      },
    };
  },
  // "[ ] " typed: a checklist item (markdown.js ticks bulleted ones only).
  addInputRules() {
    return [
      new InputRule({
        find: TASK_TYPED,
        handler: ({ state, range, match }) => {
          const checked = /x/i.test(match[1] || '');
          const tr = state.tr;
          const $from = tr.doc.resolve(range.from);
          const item = $from.depth > 1 ? $from.node(-1) : null;
          if (item?.type.name === this.name) {
            // A bulleted item's own first line, not one that's a checklist item already.
            if ($from.index(-1) !== 0 || item.attrs.checked != null || $from.node(-2)?.type.name !== 'bulletList') return null;
            tr.delete(range.from, range.to).setNodeMarkup($from.before(-1), undefined, { ...item.attrs, checked });
            return undefined;
          }
          // A paragraph of its own: a checklist of one item.
          if ($from.depth !== 1 || $from.parent.type.name !== 'paragraph') return null;
          tr.delete(range.from, range.to);
          const block = tr.doc.resolve(range.from).blockRange();
          const wrapping = block && findWrapping(block, this.editor.schema.nodes.bulletList);
          if (!wrapping) return null;
          tr.wrap(block, wrapping);
          const $in = tr.selection.$from;
          for (let d = $in.depth; d > 0; d--) {
            if ($in.node(d).type.name !== this.name) continue;
            tr.setNodeMarkup($in.before(d), undefined, { ...$in.node(d).attrs, checked });
            break;
          }
          return undefined;
        },
      }),
    ];
  },
  addNodeView() {
    return ({ node, getPos, editor }) => {
      let current = node;
      const li = document.createElement('li');
      const box = document.createElement('input');
      box.type = 'checkbox';
      box.className = 'taskbox';
      box.contentEditable = 'false';
      box.tabIndex = -1;
      const body = document.createElement('div');
      body.className = 'itembody';
      li.append(box, body);
      const draw = (n) => {
        const task = n.attrs.checked != null;
        li.classList.toggle('task', task);
        box.hidden = !task;
        box.checked = !!n.attrs.checked;
        box.setAttribute('aria-label', n.textContent.trim().slice(0, 80) || 'Task');
        if (task) li.setAttribute('data-checked', String(!!n.attrs.checked));
        else li.removeAttribute('data-checked');
      };
      draw(node);
      // A click ticks it, keeping the caret where it was. (Its click isn't
      // cancelled: a cancelled one puts the box back after the redraw.)
      box.addEventListener('mousedown', (e) => e.preventDefault());
      box.addEventListener('click', () => {
        const pos = typeof getPos === 'function' ? getPos() : null;
        if (pos == null || !editor.isEditable) return;
        editor.view.dispatch(editor.state.tr.setNodeMarkup(pos, undefined, { ...current.attrs, checked: !current.attrs.checked }));
      });
      return {
        dom: li,
        contentDOM: body,
        update(n) {
          if (n.type !== current.type) return false;
          current = n;
          draw(n);
          return true;
        },
        stopEvent: (e) => e.target === box,
        ignoreMutation: (m) => m.target === box || m.target === li,
      };
    };
  },
});

/** A section still to write (`<!-- yap:section Key | intent -->`): a grey line, never typed into. */
const YapSection = Node.create({
  name: 'yapSection',
  group: 'block',
  atom: true,
  selectable: true,
  draggable: false,
  addAttributes: () => ({ key: { default: '', rendered: false }, intent: { default: '', rendered: false } }),
  parseHTML: () => [{ tag: 'p[data-yap-section]', getAttrs: (el) => ({ key: el.getAttribute('data-key') || '', intent: el.getAttribute('data-intent') || '' }) }],
  renderHTML({ node }) {
    const { key, intent } = node.attrs;
    const attrs = { class: 'md-pending', 'data-yap-section': '', 'data-key': key, 'data-intent': intent, title: 'Not written yet', contenteditable: 'false' };
    return intent ? ['p', attrs, ['span', { class: 'md-pending-key' }, key], ': ', ['span', { class: 'md-pending-intent' }, intent]] : ['p', attrs, ['span', { class: 'md-pending-key' }, key]];
  },
});

/**
 * Markdown the editor can't edit as such: drawn as the page draws it
 * (`render`), kept exactly (its `md`). Set the drawing with `rawRenderer`.
 */
let render = () => '';
export function rawRenderer(fn) {
  render = fn;
}
const RawBlock = Node.create({
  name: 'rawBlock',
  group: 'block',
  atom: true,
  selectable: true,
  addOptions: () => ({ title: 'Edit this part in Source' }),
  parseHTML: () => [{ tag: 'div[data-yap-raw]', getAttrs: (el) => ({ md: el.getAttribute('data-md') || '' }) }],
  renderHTML: ({ node }) => ['div', { 'data-yap-raw': '', 'data-md': node.attrs.md ?? '', class: 'docraw' }],
  addNodeView() {
    return ({ node }) => {
      const dom = document.createElement('div');
      dom.className = 'docraw';
      dom.contentEditable = 'false';
      dom.title = this.options.title;
      dom.innerHTML = render(node.attrs.md ?? '');
      return { dom };
    };
  },
});

/** A link: web and mail addresses only; `auto` when the address was written bare. */
const Link = Mark.create({
  name: 'link',
  inclusive: false,
  priority: 1000,
  addAttributes: () => ({
    href: { default: null },
    title: { default: null },
    auto: hidden(null),
  }),
  parseHTML: () => [{ tag: 'a[href]', getAttrs: (el) => (SAFE_URL.test(el.getAttribute('href') || '') ? { href: el.getAttribute('href'), title: el.getAttribute('title') } : false) }],
  renderHTML: ({ HTMLAttributes }) => ['a', mergeAttributes(HTMLAttributes, { class: 'md-link' }), 0],
});

/**
 * The editor's extensions (`editing`: with its undo, its drop and gap
 * cursors; `placeholder()`: what an empty one says; `rawTitle`: a raw
 * block's tooltip, where it's edited).
 */
export function richExtensions({ editing = true, placeholder = null, rawTitle = null } = {}) {
  const base = [
    Document,
    Paragraph,
    Text,
    DocHeading.configure({ levels: [1, 2, 3, 4, 5, 6] }),
    DocBold,
    DocItalic,
    Strike,
    DocCode,
    DocCodeBlock,
    Blockquote,
    DocRule,
    DocBreak,
    DocBullets,
    DocNumbers,
    DocItem,
    ListKeymap,
    DocTable.configure({ resizable: false }),
    TableRow,
    DocHeader,
    DocCell,
    YapSection,
    rawTitle ? RawBlock.configure({ title: rawTitle }) : RawBlock,
    Link,
    Source,
  ];
  if (!editing) return base;
  const out = [...base, UndoRedo, Dropcursor.configure({ class: 'docdrop', color: false, width: 2 }), Gapcursor];
  if (placeholder) out.push(Placeholder.configure({ placeholder, showOnlyWhenEditable: false }));
  return out;
}

/** The schema alone (no editor): richMarkdown.js and its tests. */
export function richSchema() {
  return getSchema(richExtensions({ editing: false }));
}
