// **Maths and diagrams on the editor's page** (15 Nov 2026): the page a
// document beside the chat is read and edited on at rest, and notes' My
// thoughts and the quick note, draw what ui/markdownVisuals.js draws in a
// rendered answer — so a document Chat wrote doesn't turn its diagrams back
// into code once Chat stops writing.
//
// Decorations only: the document (and so the markdown written back,
// richMarkdown.js) never changes.
// - **Maths**: `$…$`, `\(…\)`, `$$…$$`, `\[…\]` in a line of text (read as
//   core/markdown.js `mathAt` reads it; never in inline code) shows drawn by
//   KaTeX, its text hidden; with the caret on or beside it, its text shows
//   to edit.
// - **Diagrams**: a ```mermaid code block shows its diagram above it, the
//   code hidden; with the caret in it (a click on the diagram, or `</>`) the
//   code shows to edit, under the diagram as it changes.
// Each only once it's drawn: until KaTeX and mermaid have loaded (their own
// chunks, the first time one shows), or when one can't be drawn, the text
// shows as it is.
import { Extension } from '@tiptap/core';
import { Plugin, PluginKey, TextSelection } from '@tiptap/pm/state';
import { Decoration, DecorationSet } from '@tiptap/pm/view';
import { mathAt } from '../../core/markdown.js';
import { CODE_ICON, SHADOW_CSS, keepReadable } from '../../ui/markdownVisuals.js';

const key = new PluginKey('yapVisuals');

// The chunks, once loaded (shared with ui/markdownVisuals.js's: the same modules).
let maths = null;
let diagrams = null;
let mathsLoading = null;
let diagramsLoading = null;

/** A character that never starts or ends maths: stands in for inline code and other inline nodes. */
const NOT_TEXT = '\u0001';

/** The maths in a line of text: `[{ from, to, tex, display }]` (positions in the document). */
function mathsIn(block, start) {
  let text = '';
  block.forEach((child) => {
    if (child.isText && !child.marks.some((m) => m.type.name === 'code')) text += child.text;
    else text += NOT_TEXT.repeat(child.isText ? child.text.length : child.nodeSize);
  });
  const out = [];
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (c !== '$' && c !== '\\') continue;
    const m = mathAt(text, i);
    if (!m) {
      if (c === '\\') i++;
      continue;
    }
    if (!text.slice(i, m.end).includes(NOT_TEXT)) out.push({ from: start + i, to: start + m.end, tex: m.tex, display: m.display });
    i = m.end - 1;
  }
  return out;
}

const isDiagram = (node) => node.type.name === 'codeBlock' && String(node.attrs.language || '').toLowerCase() === 'mermaid';

/** The last drawing of each diagram on the page, by where it starts: shown while its code is edited and drawn again. */
const lastDrawn = new Map();
/** Diagrams to draw once their code has stood still (as it's typed), by source. */
const waiting = new Map();
function drawSoon(source) {
  clearTimeout(waiting.get(source));
  waiting.set(
    source,
    setTimeout(() => {
      waiting.delete(source);
      diagrams.drawDiagram(source).then(redraw, () => {});
    }, 400)
  );
}

/** The page's decorations now: drawn maths and diagrams, their text hidden unless the caret is at them. */
function decorate(state) {
  const { from: selFrom, to: selTo } = state.selection;
  const at = (from, to) => selTo >= from && selFrom <= to;
  const decos = [];
  let wantsMaths = false;
  let wantsDiagrams = false;
  state.doc.descendants((node, pos) => {
    if (isDiagram(node)) {
      wantsDiagrams = true;
      if (!diagrams) return false;
      const source = node.textContent;
      const inside = at(pos + 1, pos + node.nodeSize - 1);
      const drawn = diagrams.drawnNow(source);
      if (drawn === undefined) drawSoon(source);
      if (drawn?.svg) {
        if (lastDrawn.size > 50) lastDrawn.clear();
        lastDrawn.set(pos, drawn.svg);
      }
      // Being edited: its last drawing stays up while the new code is drawn.
      const svg = drawn?.svg ?? (inside ? lastDrawn.get(pos) : null);
      if (!svg) return false;
      decos.push(Decoration.widget(pos, (view, getPos) => diagramWidget(view, svg, getPos), { side: -1, key: `d:${hash(svg)}`, ignoreSelection: true }));
      if (!inside) decos.push(Decoration.node(pos, pos + node.nodeSize, { class: 'rich-hidden-source' }));
      return false;
    }
    if (node.type.name === 'codeBlock') return false;
    if (!node.isTextblock) return true;
    for (const m of mathsIn(node, pos + 1)) {
      wantsMaths = true;
      if (!maths || at(m.from, m.to)) continue;
      const html = maths.mathsHtml(m.tex, m.display);
      if (!html) continue;
      decos.push(Decoration.widget(m.from, () => mathsWidget(html, m.display), { side: -1, key: `m${m.display ? 'd' : 'i'}:${hash(m.tex)}`, ignoreSelection: true }));
      decos.push(Decoration.inline(m.from, m.to, { class: 'rich-hidden-source' }));
    }
    return false;
  });
  if (wantsMaths && !maths) mathsLoading ??= import('../../ui/maths.js').then((mod) => ((maths = mod), redraw()));
  if (wantsDiagrams && !diagrams) diagramsLoading ??= import('../../ui/diagrams.js').then((mod) => ((diagrams = mod), redraw()));
  return DecorationSet.create(state.doc, decos);
}

/** A short hash for a widget's key (its content changed: a new widget). */
function hash(s) {
  let h = 0;
  for (let i = 0; i < s.length; i++) h = (Math.imul(h, 31) + s.charCodeAt(i)) | 0;
  return (h >>> 0).toString(36);
}

function mathsWidget(html, display) {
  const el = document.createElement('span');
  el.className = `md-math rich-math${display ? ' md-math-display' : ''}`;
  el.dataset.drawn = 'true';
  el.contentEditable = 'false';
  // (Sanitised: core/visualSafety.js `cleanMaths`.)
  el.innerHTML = html;
  return el;
}

function diagramWidget(view, svg, getPos) {
  const box = document.createElement('div');
  box.className = 'md-diagram rich-diagram';
  box.dataset.drawn = 'true';
  box.contentEditable = 'false';
  const pic = document.createElement('div');
  pic.className = 'md-diagram-view';
  pic.setAttribute('role', 'img');
  pic.setAttribute('aria-label', 'Diagram');
  // (Sanitised: core/visualSafety.js `cleanDiagram`. Its sheet stays in the shadow root.)
  const root = pic.attachShadow({ mode: 'open' });
  root.innerHTML = `<style>${SHADOW_CSS}</style>${svg}`;
  keepReadable(root);
  const sw = document.createElement('button');
  sw.type = 'button';
  sw.className = 'md-diagram-switch';
  sw.title = 'Edit the code';
  sw.setAttribute('aria-label', sw.title);
  sw.innerHTML = CODE_ICON;
  // A click on it (or its `</>`): the caret into its code, which shows.
  const edit = (e) => {
    e.preventDefault();
    const pos = getPos();
    if (!view.editable || pos == null) return;
    const node = view.state.doc.nodeAt(pos);
    if (!node || !isDiagram(node)) return;
    view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, pos + 1)).scrollIntoView());
    view.focus();
  };
  sw.addEventListener('mousedown', edit);
  pic.addEventListener('mousedown', edit);
  box.append(pic, sw);
  return box;
}

// Every editor on the page, to draw again when a chunk loads, a diagram is drawn or the theme changes.
const views = new Set();
let redrawing = false;
function redraw() {
  if (redrawing) return;
  redrawing = true;
  queueMicrotask(() => {
    redrawing = false;
    for (const v of views) if (!v.isDestroyed) v.dispatch(v.state.tr.setMeta(key, 'redraw').setMeta('addToHistory', false));
  });
}
let watchingTheme = false;
function watchTheme() {
  if (watchingTheme || typeof MutationObserver === 'undefined') return;
  watchingTheme = true;
  new MutationObserver(redraw).observe(document.documentElement, { attributes: true, attributeFilter: ['data-yap-theme'] });
}

export const RichVisuals = Extension.create({
  name: 'yapVisuals',
  addProseMirrorPlugins() {
    return [
      new Plugin({
        key,
        state: {
          init: (_, state) => decorate(state),
          apply: (tr, set, _old, state) => (tr.docChanged || tr.selectionSet || tr.getMeta(key) ? decorate(state) : set),
        },
        props: { decorations: (state) => key.getState(state) },
        view(v) {
          views.add(v);
          watchTheme();
          return { destroy: () => views.delete(v) };
        },
      }),
    ];
  },
});
