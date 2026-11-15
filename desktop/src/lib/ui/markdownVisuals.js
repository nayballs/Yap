// **Maths, diagrams and pictures in rendered markdown** (15 Nov 2026, after
// the Claude app's answers): a Svelte action on the element markdown is
// rendered into, wherever it's read — Chat's answers (and a Code chat's),
// a meeting's Ask thread, a note's summary, the notepad, a document beside
// the chat:
//
//   <div class="cbody" use:markdownVisuals>{@html renderMarkdown(text)}</div>
//
// core/markdown.js only marks them, as the model wrote them; this draws them:
// - **Maths** (`.md-math`): KaTeX (ui/maths.js, a chunk loaded the first
//   time maths shows). Drawn in the same step as the answer's text arrives
//   (a MutationObserver's callback runs before the page is drawn), so a
//   formula never flickers as an answer is written. One KaTeX can't read
//   stays as written.
// - **Diagrams** (`.md-diagram`, a ```mermaid fence): mermaid (ui/diagrams.js,
//   its own chunks), in a shadow root beside its code, with `</>` to switch
//   to the code and back (remembered while the view lasts). A diagram whose
//   code is still changing (an answer being written) waits until it's
//   stood still a moment; one that can't be drawn (or isn't safe to) stays
//   as its code. Drawn again when the theme changes.
// - **Pictures** (`a.md-image`): a "Show image" box naming the site; only a
//   click fetches it (Rust `markdown_picture`, through privacy/net.rs), and
//   the picture is shown from memory, never kept.
import { invoke } from '@tauri-apps/api/core';
import { mathOf } from '../core/markdown.js';

const MATHS = '.md-math:not([data-drawn])';
const DIAGRAMS = '.md-diagram:not([data-drawn])';
const PICTURES = 'a.md-image:not([data-drawn])';

/** How long a diagram's code stands still before it's drawn (an answer still being written changes it). */
const SETTLE_MS = 400;

// The chunks, once loaded (every view shares them).
let maths = null;
let mathsLoading = null;
let diagrams = null;
let diagramsLoading = null;

function loadMaths() {
  mathsLoading ??= import('./maths.js').then((m) => (maths = m));
  return mathsLoading;
}
function loadDiagrams() {
  diagramsLoading ??= import('./diagrams.js').then((m) => (diagrams = m));
  return diagramsLoading;
}

// ---- maths ---------------------------------------------------------------------------

function drawMaths(el) {
  const m = mathOf(el.textContent);
  const html = m ? maths.mathsHtml(m.tex, m.display) : null;
  el.dataset.drawn = html ? 'true' : 'plain';
  // (Sanitised: core/visualSafety.js `cleanMaths`.)
  if (html) el.innerHTML = html;
}

// ---- diagrams ------------------------------------------------------------------------

/** The diagram's own sheet in its shadow root: it fits the column, centred. */
export const SHADOW_CSS = ':host{display:block;overflow-x:auto}svg{display:block;max-width:100%;height:auto;margin:0 auto}';

/**
 * A wide diagram shrinks to fit the column, but never below 65% of its own
 * size (its labels would be too small to read): past that it scrolls
 * sideways. Shared with the editor's page (richVisuals.js).
 */
export function keepReadable(root) {
  const svg = root.querySelector('svg');
  const width = svg?.viewBox?.baseVal?.width;
  if (width > 0) svg.style.minWidth = `${Math.round(width * 0.65)}px`;
}

/** `</>`: the switch between a diagram and its code. */
export const CODE_ICON =
  '<svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M5.5 4.5 2 8l3.5 3.5M10.5 4.5 14 8l-3.5 3.5"/></svg>';

const sourceOf = (box) => box.querySelector(':scope > pre.md-code > code')?.textContent ?? '';

/** Show the diagram (`code` false) or its code. */
function showCode(box, code) {
  box.classList.toggle('md-diagram-code', code);
  const sw = box.querySelector(':scope > .md-diagram-switch');
  if (!sw) return;
  sw.setAttribute('aria-pressed', String(code));
  sw.title = code ? 'Show the diagram' : 'Show the code';
  sw.setAttribute('aria-label', sw.title);
}

function paintDiagram(box, result, showingCode) {
  const source = sourceOf(box);
  if (!result.svg) {
    box.dataset.drawn = 'plain';
    box.title = `Not drawn: ${result.error}`;
    return;
  }
  box.dataset.drawn = 'true';
  const view = document.createElement('div');
  view.className = 'md-diagram-view';
  view.setAttribute('role', 'img');
  view.setAttribute('aria-label', 'Diagram');
  // (Sanitised: core/visualSafety.js `cleanDiagram`. Its sheet stays in the shadow root.)
  const root = view.attachShadow({ mode: 'open' });
  root.innerHTML = `<style>${SHADOW_CSS}</style>${result.svg}`;
  keepReadable(root);
  const sw = document.createElement('button');
  sw.type = 'button';
  sw.className = 'md-diagram-switch';
  sw.innerHTML = CODE_ICON;
  sw.addEventListener('click', (e) => {
    e.stopPropagation();
    const code = !box.classList.contains('md-diagram-code');
    if (code) showingCode.add(source);
    else showingCode.delete(source);
    showCode(box, code);
  });
  box.prepend(view);
  box.append(sw);
  showCode(box, showingCode.has(source));
}

/** A drawn diagram's drawing replaced (the theme changed). */
function redrawDiagram(box, svg) {
  const root = box.querySelector(':scope > .md-diagram-view')?.shadowRoot;
  if (!root) return;
  root.innerHTML = `<style>${SHADOW_CSS}</style>${svg}`;
  keepReadable(root);
}

// ---- pictures ------------------------------------------------------------------------

const hostOf = (url) => {
  try {
    return new URL(url).hostname.replace(/^www\./, '');
  } catch {
    return '';
  }
};

function drawPicture(a, shown) {
  const url = a.getAttribute('href') || '';
  const alt = a.textContent || 'Picture';
  const host = hostOf(url);
  if (shown.has(url)) {
    a.replaceWith(pictureOf(shown.get(url), alt, url));
    return;
  }
  // The box is the button: "Show image", and the site it would come from.
  const box = document.createElement('button');
  box.type = 'button';
  box.className = 'md-imgbox';
  box.dataset.drawn = 'true';
  box.title = `${alt}
From ${url}
Yap fetches it only when you click.`;
  box.setAttribute('aria-label', `Show image: ${alt}${host ? `, from ${host}` : ''}`);
  const label = document.createElement('span');
  label.className = 'md-imgshow';
  label.textContent = 'Show image';
  const from = document.createElement('span');
  from.className = 'md-imghost';
  from.textContent = host;
  box.append(label, from);
  box.addEventListener('click', async (e) => {
    e.stopPropagation();
    e.preventDefault();
    if (box.classList.contains('loading')) return;
    box.classList.add('loading');
    box.classList.remove('failed');
    label.textContent = 'Loading…';
    try {
      const bytes = await invoke('markdown_picture', { url });
      const src = URL.createObjectURL(new Blob([bytes]));
      shown.set(url, src);
      if (box.isConnected) box.replaceWith(pictureOf(src, alt, url));
    } catch (err) {
      box.classList.add('failed');
      label.textContent = "Couldn't load it";
      box.title = `${String(err?.message ?? err ?? 'It could not be fetched.')}
Click to try again.`;
    } finally {
      box.classList.remove('loading');
    }
  });
  a.replaceWith(box);
}

function pictureOf(src, alt, url) {
  const img = document.createElement('img');
  img.className = 'md-img';
  img.dataset.drawn = 'true';
  img.src = src;
  img.alt = alt;
  img.title = `${alt}\n${url}`;
  img.decoding = 'async';
  return img;
}

// ---- the action ------------------------------------------------------------------------

export function markdownVisuals(node) {
  let here = true;
  /** Diagrams shown as their code (`</>`), by source: kept while this view lasts. */
  const showingCode = new Set();
  /** Pictures shown here, by address: their in-memory address (drawn again as an answer re-renders). */
  const shown = new Map();
  /** Diagrams waiting for their code to stand still, by source. */
  const waiting = new Map();

  const look = () => {
    if (!here) return;
    // Maths: at once when KaTeX is here, else once it is.
    const formulas = node.querySelectorAll(MATHS);
    if (formulas.length) {
      if (maths) formulas.forEach(drawMaths);
      else loadMaths().then(look, () => {});
    }
    for (const a of node.querySelectorAll(PICTURES)) drawPicture(a, shown);
    const boxes = node.querySelectorAll(DIAGRAMS);
    if (!boxes.length) return;
    if (!diagrams) {
      loadDiagrams().then(look, () => {});
      return;
    }
    for (const box of boxes) {
      const source = sourceOf(box);
      const now = diagrams.drawnNow(source);
      if (now) {
        paintDiagram(box, now, showingCode);
        continue;
      }
      // Not drawn yet: once its code has stood still (each new piece of an answer resets it).
      clearTimeout(waiting.get(source));
      waiting.set(
        source,
        setTimeout(() => {
          waiting.delete(source);
          if (!here || ![...node.querySelectorAll(DIAGRAMS)].some((b) => sourceOf(b) === source)) return;
          diagrams.drawDiagram(source).then(look, () => {});
        }, SETTLE_MS)
      );
    }
  };
  look();
  // Rendered again (an answer still being written, an edit): its new parts.
  // (This action's own changes are seen too, and find nothing left to do.)
  const seen = new MutationObserver(look);
  seen.observe(node, { childList: true, subtree: true });
  // The theme changed: each diagram in its colours — at once when it was
  // drawn ahead for it (ui/diagrams.js draws both), else as soon as it is.
  const theme = new MutationObserver(() => {
    if (!diagrams) return;
    for (const box of node.querySelectorAll('.md-diagram[data-drawn="true"]')) {
      const source = sourceOf(box);
      const now = diagrams.drawnNow(source);
      if (now?.svg) redrawDiagram(box, now.svg);
      else diagrams.drawDiagram(source).then((r) => r.svg && box.isConnected && redrawDiagram(box, r.svg), () => {});
    }
  });
  theme.observe(document.documentElement, { attributes: true, attributeFilter: ['data-yap-theme'] });
  return {
    destroy() {
      here = false;
      seen.disconnect();
      theme.disconnect();
      for (const t of waiting.values()) clearTimeout(t);
      for (const src of shown.values()) URL.revokeObjectURL(src);
    },
  };
}
