// **A Code chat's file paths as links** (15 Nov 2026, after the Claude app's
// Code tab: a path in an answer opens the file): a Svelte action on the
// element an answer's Markdown is rendered into, beside `colourCode`.
//
//   <div class="cbody" use:colourCode use:fileLinks={files}>{@html …}</div>
//
// - **What may be a path**: a link to a file (`[convert.js](src/convert.js)`:
//   core/markdown.js makes it `a.md-file` with its path), inline code that
//   reads as one (`src/convert.js`, `docs/lab.md:105`), and a path written
//   in the words (`src/convert.js:12`, `CLAUDE.md`). Code blocks never.
// - **Only files in the chat's workspace become links**: the candidates go
//   to Rust in one call (`code_files_check`, agents/files.rs), which resolves
//   each in the workspace it knows from the conversation — never a path
//   outside it, `..` or a link that leads out. What Rust says is kept while
//   Yap runs (a miss for a few seconds only: a file a turn is writing turns
//   up).
// - **Opened** with a click or Enter: `files.open({ rel, line })`, where
//   Settings → Yap Code's "Open file links in" says (views/chat/fileOpen.svelte.js:
//   the file view beside the chat, FileTab.svelte, by default; an editor; the
//   default app). A right-click, the ContextMenu key or Shift+F10 on one:
//   `files.menu(e, { rel, line, abs }, el)` (Open in Yap, Open in <editor>,
//   Show in folder, Copy path). A Chat (non-Code) chat has no `files`
//   (null): nothing is looked for and every path stays text.
// - **Its ⋯** (16 Nov 2026, as Yap's other items have one): a small button
//   that comes up at the end of the link the pointer is on or the keyboard is
//   at, over the words after it, and opens the same menu under itself. One
//   per answer, moved to the link in question, so it's always the next Tab
//   stop after the focused link (Shift+Tab back to it) and nothing moves in
//   the words: it's out of their flow (`position: absolute` where it stands:
//   app.css `.md-filemore`). It stays while its menu is open.
// - **Never markup**: the words are split into text nodes and an `a` made
//   here, never set as HTML.
//
// **The person's own @-mentions too** (`mentionLinks`, 15 Nov 2026, as the
// Claude app's sent message links a mentioned file): on a Code chat's sent
// message, each `@path` pill (`chatMentions.js` `mentionParts`, its path in
// `data-path`) that names a file in the chat's workspace becomes a link to
// its view, checked by Rust the same way and kept in the same cache. Only
// the pills: a path typed without an @ stays as the person wrote it.
import { invoke } from '@tauri-apps/api/core';

/** The context key Chat sets in a Code chat: `{ conversation, open(ref) }` (ChatView.svelte). */
export const CODE_FILES = 'yap-code-files';

/** A path's own shape: folders with `/` or `\`, a name with an ending, then a line (`:12`, `:12:4`, `#L12`). */
const PATH_RE = /(?:[A-Za-z]:[\\/])?(?:[\w@+~-][\w@.+~-]*[\\/])*[\w@+~-][\w@.+~-]*\.[A-Za-z][A-Za-z0-9]{0,11}(?::\d{1,7}(?::\d{1,7})?|#L\d{1,7}(?:-L\d{1,7})?)?/g;
/** The same, the whole of an inline code's text. */
const WHOLE_RE = new RegExp(`^${PATH_RE.source}$`);
/** The most candidates an answer sends (agents/files.rs `CHECK_MAX`). */
const MAX = 200;
/** How long a miss is believed (ms). */
const MISS_MS = 8_000;

/** `${conversation}\n${path}` → `{ ref, at }`: Rust's answer, `ref` null for none. */
const known = new Map();

/** Whether `text` (inline code's, trimmed) reads as a file's path. Pure. */
export function looksLikePath(text) {
  const t = String(text || '').trim();
  return t.length > 2 && t.length <= 512 && WHOLE_RE.test(t);
}

/** The paths written in `text`: `[{ start, end, path }]`, each where it is. Pure. */
export function pathsIn(text) {
  const out = [];
  for (const m of String(text || '').matchAll(PATH_RE)) {
    const start = m.index;
    const end = start + m[0].length;
    // Not part of a longer word, an address or a number ("v1.2", "example.com/x" in a link).
    const before = text[start - 1] ?? '';
    const after = text[end] ?? '';
    if (/[\w/\\.:@-]/.test(before) || /[\w/\\]/.test(after)) continue;
    // Trailing full stop of a sentence: never part of the path (the pattern needs a letter after a dot anyway).
    out.push({ start, end, path: m[0] });
  }
  return out;
}

const cached = (conv, path) => {
  const k = known.get(`${conv}\n${path}`);
  if (!k) return undefined;
  if (k.ref == null && Date.now() - k.at > MISS_MS) return undefined;
  return k.ref;
};

/** Text nodes worth looking at: not in code, a link, a button, maths or a diagram, nor already a file link. */
function textNodes(node) {
  const out = [];
  const walk = node.ownerDocument.createTreeWalker(node, NodeFilter.SHOW_TEXT, {
    acceptNode: (t) => (t.parentElement?.closest('a, code, pre, button, svg, .md-filelink, .md-math, .md-math-display, .md-diagram') ? NodeFilter.FILTER_REJECT : /[.]/.test(t.data) ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_REJECT),
  });
  while (walk.nextNode()) out.push(walk.currentNode);
  return out;
}

/** Every candidate in `node` now: `{ kind, el|text, path, start, end }`. */
function candidates(node) {
  const out = [];
  for (const a of node.querySelectorAll('a.md-file[data-path]:not([data-rel])')) out.push({ kind: 'a', el: a, path: a.dataset.path });
  for (const c of node.querySelectorAll('code:not([data-rel]):not([data-nofile])')) {
    if (c.closest('pre, a')) continue;
    if (looksLikePath(c.textContent)) out.push({ kind: 'code', el: c, path: c.textContent.trim() });
    else c.dataset.nofile = '';
  }
  for (const t of textNodes(node)) for (const p of pathsIn(t.data)) out.push({ kind: 'text', text: t, ...p });
  return out;
}

/** Make `el` open `ref` (a link's look, the keyboard's Enter). */
function mark(el, ref) {
  el.dataset.rel = ref.rel;
  if (ref.abs) el.dataset.abs = ref.abs;
  if (ref.line) el.dataset.fileLine = String(ref.line);
  el.classList.add('md-filelink');
  el.setAttribute('role', 'link');
  el.tabIndex = 0;
  el.title = `Open ${ref.rel}${ref.line ? ` at line ${ref.line}` : ''}`;
}

/** The text nodes' paths that are files, made links (the last first, so earlier places stay right). */
function linkText(conv, list) {
  const byNode = new Map();
  for (const c of list) {
    if (c.kind !== 'text') continue;
    const ref = cached(conv, c.path);
    if (!ref) continue;
    if (!byNode.has(c.text)) byNode.set(c.text, []);
    byNode.get(c.text).push({ ...c, ref });
  }
  for (const [t, hits] of byNode) {
    if (!t.parentNode) continue;
    hits.sort((a, b) => b.start - a.start);
    for (const h of hits) {
      const tail = t.splitText(h.start);
      tail.splitText(h.end - h.start);
      const a = t.ownerDocument.createElement('a');
      a.className = 'md-file';
      a.textContent = tail.data;
      mark(a, h.ref);
      tail.replaceWith(a);
    }
  }
}

/** A sent message's @-mention pills not looked at yet: `{ kind, el, path }`. */
function mentionCandidates(node) {
  return [...node.querySelectorAll('.mention[data-path]:not([data-rel]):not([data-nofile])')].map((el) => ({ kind: 'mention', el, path: el.dataset.path }));
}

/** A Code chat's answer: its paths that are the workspace's files, links. */
export function fileLinks(node, files) {
  return linker(node, files, candidates);
}

/** A Code chat's sent message: its @-mentions of the workspace's files, links. */
export function mentionLinks(node, files) {
  return linker(node, files, mentionCandidates);
}

/** The action itself: `find(node)` lists what may be a path there. */
function linker(node, files, find) {
  let ctx = files;
  let timer = 0;
  let here = true;

  async function look() {
    timer = 0;
    const conv = ctx?.conversation;
    if (!here || conv == null) return;
    const list = find(node);
    if (!list.length) return;
    const need = [...new Set(list.map((c) => c.path))].filter((p) => cached(conv, p) === undefined).slice(0, MAX);
    if (need.length) {
      let got = [];
      try {
        got = await invoke('code_files_check', { conversation: conv, paths: need });
      } catch {
        got = [];
      }
      const at = Date.now();
      need.forEach((p, i) => known.set(`${conv}\n${p}`, { ref: got?.[i] ?? null, at }));
      if (!here || ctx?.conversation !== conv) return;
    }
    // (Looked at again: the answer may have been rendered anew meanwhile.)
    const now = find(node);
    for (const c of now) {
      const ref = c.kind === 'text' ? null : cached(conv, c.path);
      if (c.kind !== 'text' && ref) mark(c.el, ref);
    }
    linkText(conv, now);
  }

  const soon = () => {
    if (!timer) timer = setTimeout(look, 60);
  };
  soon();
  const seen = new MutationObserver(soon);
  seen.observe(node, { childList: true, subtree: true, characterData: true });

  /** The file link `e` is on, in this node. */
  const linkOf = (e) => {
    const el = e.target instanceof Element ? e.target.closest('.md-filelink[data-rel]') : null;
    return el && node.contains(el) ? el : null;
  };
  const refOf = (el) => ({ rel: el.dataset.rel, line: Number(el.dataset.fileLine) || null, abs: el.dataset.abs || '' });
  function opened(e) {
    const el = linkOf(e);
    if (!el) return false;
    e.preventDefault();
    e.stopPropagation();
    const { rel, line } = refOf(el);
    ctx?.open?.({ rel, line });
    return true;
  }
  /** Its menu (Yap's, at the pointer or under the link): nothing without a `menu`, so the message's own shows. */
  function menu(e) {
    if (onMore(e)) return moreMenu(e);
    const el = linkOf(e);
    if (!el || !ctx?.menu) return false;
    return ctx.menu(e, refOf(el), el) !== false;
  }

  // ---- The ⋯: one button, after the link the pointer is on or the keyboard is at ----
  const more = new MoreButton(node, {
    menu: (e, link, button) => (ctx?.menu ? ctx.menu(e, refOf(link), button) !== false : false),
  });
  const onMore = (e) => more.has(e.target);
  const moreMenu = (e) => more.menu(e);
  const over = (e) => {
    const el = linkOf(e);
    if (el && ctx?.menu) more.show(el);
  };
  const out = (e) => {
    if (linkOf(e) || onMore(e)) more.hideSoon();
  };
  const focusIn = (e) => {
    const el = linkOf(e);
    if (el && ctx?.menu) more.show(el);
    else if (!onMore(e)) more.hideSoon();
  };
  const focusOut = () => more.hideSoon();

  const click = (e) => {
    // (The ⋯ handles its own click: its menu, never the file.)
    if (!onMore(e)) opened(e);
  };
  const key = (e) => {
    if (onMore(e)) {
      if (e.key === 'ContextMenu' || (e.key === 'F10' && e.shiftKey)) moreMenu(e);
      return;
    }
    if (e.key === 'Enter') opened(e);
    else if (e.key === 'ContextMenu' || (e.key === 'F10' && e.shiftKey)) menu(e);
  };
  node.addEventListener('click', click);
  node.addEventListener('keydown', key);
  node.addEventListener('contextmenu', menu);
  node.addEventListener('pointerover', over);
  node.addEventListener('pointerout', out);
  node.addEventListener('focusin', focusIn);
  node.addEventListener('focusout', focusOut);
  return {
    update(next) {
      ctx = next;
      if (!ctx?.menu) more.hide();
      soon();
    },
    destroy() {
      here = false;
      clearTimeout(timer);
      seen.disconnect();
      more.destroy();
      node.removeEventListener('click', click);
      node.removeEventListener('keydown', key);
      node.removeEventListener('contextmenu', menu);
      node.removeEventListener('pointerover', over);
      node.removeEventListener('pointerout', out);
      node.removeEventListener('focusin', focusIn);
      node.removeEventListener('focusout', focusOut);
    },
  };
}

/** How long the ⋯ waits before it goes (ms): long enough to move the pointer onto it. */
const MORE_GRACE_MS = 220;
const SVG = 'http://www.w3.org/2000/svg';

/**
 * **A file link's ⋯** (app.css `.md-filemore`): made the first time it's needed, put right after the link in
 * question (so the focus goes link → ⋯ → the next link), hidden when neither is hovered or focused and its menu
 * is shut. `menu(event, link, button)` opens the link's menu under it.
 */
class MoreButton {
  constructor(node, { menu }) {
    this.node = node;
    this.open = menu;
    this.el = null;
    this.link = null;
    this.timer = 0;
    this.menuUp = false;
  }

  /** Whether `t` is the ⋯ or in it. */
  has(t) {
    return !!this.el && t instanceof Node && this.el.contains(t);
  }

  button() {
    if (this.el) return this.el;
    const doc = this.node.ownerDocument;
    const b = doc.createElement('button');
    b.type = 'button';
    b.className = 'md-filemore';
    b.hidden = true;
    // (`data-more`: Yap's menus open under a ⋯, its right edge at the button's: ui/contextMenu.svelte.js.)
    b.dataset.more = '';
    b.dataset.testid = 'md-filelink-more';
    b.setAttribute('aria-haspopup', 'menu');
    b.setAttribute('aria-expanded', 'false');
    const svg = doc.createElementNS(SVG, 'svg');
    svg.setAttribute('viewBox', '0 0 24 24');
    svg.setAttribute('fill', 'currentColor');
    svg.setAttribute('aria-hidden', 'true');
    for (const cx of [5.5, 12, 18.5]) {
      const c = doc.createElementNS(SVG, 'circle');
      c.setAttribute('cx', String(cx));
      c.setAttribute('cy', '12');
      c.setAttribute('r', '1.9');
      svg.append(c);
    }
    b.append(svg);
    // (Its own click: the menu under it. Stopped there, so the answer never takes it for a click on its words.)
    b.addEventListener('click', (e) => this.menu(e));
    this.el = b;
    return b;
  }

  /** At the end of `link`, shown (unless it's busy with another: focused there, or its menu up). */
  show(link) {
    const b = this.button();
    if (this.link !== link || !b.isConnected || b.previousSibling !== link) {
      // (Busy with its own link: left there, and whatever watches its menu left running.)
      const busy = this.menuUp || (b.isConnected && b.ownerDocument.activeElement === b);
      if (busy && this.link?.isConnected) return;
      link.after(b);
      this.link = link;
    }
    // (Its menu up: the timer is what watches it shut, so it runs on.)
    if (!this.menuUp) {
      clearTimeout(this.timer);
      this.timer = 0;
    }
    const rel = link.dataset.rel || '';
    b.setAttribute('aria-label', `More for ${rel}`);
    b.title = `More for ${rel}`;
    b.hidden = false;
    this.fit();
  }

  /**
   * Over the words after the link (over the link's own end when the line has no room), its middle on the
   * middle of the link's last line: where an out-of-flow box lands in a line depends on the font and the line's
   * height, so it's measured.
   */
  fit() {
    const b = this.el;
    const rects = this.link?.getClientRects?.();
    const last = rects?.length ? rects[rects.length - 1] : null;
    b.classList.remove('md-filemore-in');
    b.style.translate = '';
    if (!last) return;
    const room = (this.node.closest('.cbubble') || this.node).getBoundingClientRect();
    if (b.getBoundingClientRect().right > room.right - 1) b.classList.add('md-filemore-in');
    const box = b.getBoundingClientRect();
    const dy = Math.round((last.top + last.bottom) / 2 - (box.top + box.bottom) / 2);
    if (dy) b.style.translate = `0 ${dy}px`;
  }

  /** Gone in a moment, unless the pointer or the focus is on it or its link, or its menu is up. */
  hideSoon() {
    if (!this.el || this.el.hidden) return;
    clearTimeout(this.timer);
    this.timer = setTimeout(() => {
      this.timer = 0;
      const b = this.el;
      if (!b) return;
      const doc = b.ownerDocument;
      if (this.menuUp && doc.querySelector('[role="menu"]')) return this.hideSoon();
      if (this.menuUp) {
        this.menuUp = false;
        b.setAttribute('aria-expanded', 'false');
      }
      const here = (el) => !!el?.isConnected && (el.matches(':hover') || el === doc.activeElement);
      if (here(b) || here(this.link)) return;
      this.hide();
    }, MORE_GRACE_MS);
  }

  hide() {
    clearTimeout(this.timer);
    this.timer = 0;
    if (this.el) this.el.hidden = true;
  }

  /** The link's menu under the ⋯ (a click, Enter or Space; a right-click on it at the pointer). */
  menu(e) {
    const b = this.el;
    const link = this.link;
    if (!b || !link?.isConnected) return false;
    const opened = this.open(e, link, b);
    if (opened) {
      this.menuUp = true;
      b.setAttribute('aria-expanded', 'true');
      // (Watched until it's shut: the focus comes back here, and the ⋯ goes as usual after.)
      this.hideSoon();
    }
    return opened;
  }

  destroy() {
    clearTimeout(this.timer);
    this.el?.remove();
    this.el = null;
    this.link = null;
  }
}
