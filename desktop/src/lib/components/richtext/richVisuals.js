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
// - **Pictures** (16 Nov 2026): `![alt](https://…)` (the editor reads it as a
//   "!" and a link, exactly as written) shows as the picture, its text
//   hidden: a "Show image" box naming its site, fetched only when clicked
//   (ui/markdownVisuals.js `pictureBox`, Rust `markdown_picture`), then the
//   picture from memory — shown again whenever the page draws it, while the
//   window lasts. With the caret on or beside it, its text shows to edit,
//   beside the picture; a click on a shown picture puts the caret there.
import { Extension } from '@tiptap/core';
import { Plugin, PluginKey, TextSelection } from '@tiptap/pm/state';
import { Decoration, DecorationSet } from '@tiptap/pm/view';
import { mathAt } from '../../core/markdown.js';
import { CODE_ICON, SHADOW_CSS, keepReadable, pictureBox, pictureOf } from '../../ui/markdownVisuals.js';

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

/** A picture's address the page may fetch when asked: the web's only (core/markdown.js `PICTURE_URL`). */
const PICTURE_URL = /^https?:\/\//i;

/**
 * The pictures in a line of text: a "!" just before a link written
 * `[alt](https://…)` (never a bare address, nor in inline code), as
 * core/markdown.js reads "![alt](https://…)" in a render:
 * `[{ from, to, url, alt }]` (positions in the document, the "!" to the
 * link's end).
 */
export function picturesIn(block, start) {
  const out = [];
  /** The text node before this one, when it ends in a "!" outside a link and code. */
  let bang = null;
  let link = null;
  const close = () => {
    if (link) out.push(link);
    link = null;
  };
  const code = (n) => n.marks.some((m) => m.type.name === 'code');
  // The same marks round the "!" and the link's words (the "!" right before
  // the "[": `*see ![alt](…)*`, not `**wow!**[alt](…)`).
  const same = (a, b) => a.length === b.length && a.every((m, i) => m.eq(b[i]));
  block.forEach((child, offset) => {
    const at = start + offset;
    const mark = child.isText ? child.marks.find((m) => m.type.name === 'link') : null;
    if (mark && link?.mark.eq(mark)) {
      link.to = at + child.nodeSize;
      link.alt += child.text;
      return;
    }
    close();
    const href = String(mark?.attrs.href || '');
    if (mark && !mark.attrs.auto && PICTURE_URL.test(href) && bang && !code(child) && same(bang.marks, child.marks.filter((m) => m !== mark))) {
      link = { from: at - 1, to: at + child.nodeSize, url: href, alt: child.text, mark };
    }
    bang = child.isText && !mark && !code(child) && child.text.endsWith('!') ? child : null;
  });
  close();
  return out.map(({ from, to, url, alt }) => ({ from, to, url, alt: alt.trim() || 'Picture' }));
}

/** Pictures shown on any page in this window, by address: their in-memory address (never kept on disk). */
const shownPictures = new Map();

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
    // A picture: the box (or the picture, once shown) before its text, which
    // shows only with the caret at it. (Its key says which, so showing it
    // makes it anew.)
    for (const p of picturesIn(node, pos + 1)) {
      const src = shownPictures.get(p.url);
      decos.push(
        Decoration.widget(p.from, (view) => pictureWidget(view, p, src), {
          side: -1,
          key: `p${src ? 's' : 'b'}:${hash(p.url)}:${hash(p.alt)}`,
          ignoreSelection: true,
          // Its clicks are its own: never the page's caret or a link's.
          stopEvent: () => true,
        })
      );
      if (!at(p.from, p.to)) decos.push(Decoration.inline(p.from, p.to, { class: 'rich-hidden-source' }));
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

/**
 * A picture on the page: its "Show image" box, or (`src`) the picture. The
 * box's click fetches it (then every page in the window draws it); a click
 * on the picture puts the caret just after its text, which shows to edit.
 */
function pictureWidget(view, p, src) {
  const wrap = document.createElement('span');
  wrap.className = 'rich-picture';
  wrap.contentEditable = 'false';
  wrap.dataset.url = p.url;
  // (Never the page's: no caret moved, no focus lost, no link opened.)
  wrap.addEventListener('mousedown', (e) => e.preventDefault());
  if (src) {
    const img = pictureOf(src, p.alt, p.url);
    img.addEventListener('click', (e) => {
      e.preventDefault();
      if (!view.editable) return;
      // Where its text ends now (the page may have changed since it was drawn).
      const found = picturesAt(view.state.doc).find((q) => q.url === p.url && q.alt === p.alt);
      if (!found) return;
      view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, found.to)).scrollIntoView());
      view.focus();
    });
    wrap.append(img);
  } else {
    wrap.append(
      pictureBox(p.url, p.alt, (shown) => {
        // (At most 100 kept: the oldest let go.)
        if (shownPictures.size >= 100) {
          const [oldest, oldSrc] = shownPictures.entries().next().value;
          shownPictures.delete(oldest);
          URL.revokeObjectURL(oldSrc);
        }
        shownPictures.set(p.url, shown);
        redraw();
      })
    );
  }
  return wrap;
}

/** Every picture in a document (`picturesIn` for each line of text). */
function picturesAt(doc) {
  const out = [];
  doc.descendants((node, pos) => {
    if (node.type.name === 'codeBlock') return false;
    if (!node.isTextblock) return true;
    out.push(...picturesIn(node, pos + 1));
    return false;
  });
  return out;
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
