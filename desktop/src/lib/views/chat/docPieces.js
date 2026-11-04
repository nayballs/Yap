// The document panel's page, drawn a piece at a time (DocPanel's own chunk):
// cut into pieces at its sections (`splitSections`), each piece drawn once
// and kept while its HTML is the same (`drawParts`), and the writing caret
// at the end of the section being written (`placeCaret`). No Svelte state:
// tested on its own (e2e/doc-stream.spec.js).
import { isPlaceholder } from '../../core/markdown.js';

/**
 * `md` cut into the page's pieces: at each `#`/`##` heading and section
 * placeholder outside a code block — places where a block always starts, so
 * each piece draws exactly as it would in the whole. `[{md, line}]`, `line`
 * the piece's first line in `md`. Pure.
 */
export function splitSections(md) {
  const lines = String(md ?? '').split('\n');
  const out = [];
  let fence = null;
  let start = 0;
  lines.forEach((l, i) => {
    const f = /^\s{0,3}(`{3,}|~{3,})/.exec(l);
    if (fence) {
      if (f && f[1][0] === fence[0] && f[1].length >= fence.length) fence = null;
      return;
    }
    if (f) {
      fence = f[1];
      return;
    }
    if (i > start && (/^\s{0,3}#{1,2}\s+\S/.test(l) || isPlaceholder(l))) {
      out.push({ md: lines.slice(start, i).join('\n'), line: start });
      start = i;
    }
  });
  out.push({ md: lines.slice(start).join('\n'), line: start });
  return out;
}

/** A block's HTML as drawn, its lines and its landed mark aside. */
export const plainHtml = (el) => el.outerHTML.replace(/ data-(line|changed)="[^"]*"/g, '');

/**
 * Draw the page's pieces (`[{html, live, line}]`, in order) into `root`,
 * keeping the elements of every piece whose HTML is unchanged — the section
 * being written is the only piece drawn again as it grows, so a long
 * document costs nothing per frame and a selection elsewhere on it stays.
 * A piece's HTML has its blocks' lines counted from its own start
 * (`data-line`, renderMarkdown's `lines`): they're set to the page's here,
 * from its first line (`line`), so a piece that only moved is kept. With
 * `compare`, returns the top-level elements of pieces drawn new (outside a
 * section being written) that weren't on the page before: a change that
 * landed (none on a first drawing).
 */
export function drawParts(root, parts, { compare = false } = {}) {
  root.__yapCaret?.remove();
  const old = root.__yapParts ?? [];
  if (!old.length) compare = false;
  const before = compare ? new Set(old.flatMap((p) => p.nodes).filter((n) => n.nodeType === 1).map(plainHtml)) : null;
  const pool = new Map();
  for (const p of old) {
    if (!pool.has(p.html)) pool.set(p.html, []);
    pool.get(p.html).push(p);
  }
  const next = [];
  const made = [];
  for (const part of parts) {
    const kept = pool.get(part.html)?.shift();
    if (kept) {
      next.push({ ...kept, live: !!part.live });
      continue;
    }
    const t = root.ownerDocument.createElement('template');
    t.innerHTML = part.html;
    const nodes = [...t.content.childNodes];
    next.push({ html: part.html, nodes, live: !!part.live });
    if (!part.live) made.push(...nodes.filter((n) => n.nodeType === 1));
  }
  for (const list of pool.values()) for (const p of list) for (const n of p.nodes) n.remove();
  let at = root.firstChild;
  next.forEach((p, i) => {
    const base = parts[i].line ?? 0;
    for (const n of p.nodes) {
      if (n === at) at = at.nextSibling;
      else root.insertBefore(n, at);
      if (n.nodeType !== 1) continue;
      // Its lines in the piece, kept as drawn; on the page, from the piece's start.
      if (n.__yapLines === undefined) n.__yapLines = /^(\d+)-(\d+)$/.exec(n.getAttribute('data-line') ?? '')?.slice(1).map(Number) ?? null;
      if (n.__yapLines) n.setAttribute('data-line', `${n.__yapLines[0] + base}-${n.__yapLines[1] + base}`);
    }
  });
  while (at) {
    const n = at.nextSibling;
    at.remove();
    at = n;
  }
  root.__yapParts = next;
  return compare ? made.filter((el) => !before.has(plainHtml(el))) : [];
}

/**
 * The writing caret, at the end of the text of the section being written
 * (the last piece `drawParts` drew live), its label `who` drawn by CSS (so
 * find and a copy never meet it); one element, moved as the text grows.
 * Returns it, or `null` with nothing live.
 */
export function placeCaret(root, who) {
  const part = (root.__yapParts ?? []).findLast((p) => p.live);
  if (!part) return null;
  let last = null;
  for (const n of part.nodes) {
    if (n.nodeType === 3) {
      if (n.textContent.trim()) last = n;
      continue;
    }
    if (n.nodeType !== 1) continue;
    const walk = root.ownerDocument.createTreeWalker(n, 4 /* NodeFilter.SHOW_TEXT */);
    for (let t = walk.nextNode(); t; t = walk.nextNode()) if (t.textContent.trim()) last = t;
  }
  if (!last) return null;
  let caret = root.__yapCaret;
  if (!caret) {
    caret = root.__yapCaret = root.ownerDocument.createElement('span');
    caret.className = 'doccaret';
    caret.setAttribute('aria-hidden', 'true');
  }
  caret.setAttribute('data-who', who);
  last.parentNode.insertBefore(caret, last.nextSibling);
  return caret;
}
