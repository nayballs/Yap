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
// - **Opened** with a click or Enter: `files.open({ rel, line })`, the file
//   view beside the chat (FileTab.svelte). A Chat (non-Code) chat has no
//   `files` (null): nothing is looked for and every path stays text.
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

  function opened(e) {
    const el = e.target instanceof Element ? e.target.closest('.md-filelink[data-rel]') : null;
    if (!el || !node.contains(el)) return false;
    e.preventDefault();
    e.stopPropagation();
    ctx?.open?.({ rel: el.dataset.rel, line: Number(el.dataset.fileLine) || null });
    return true;
  }
  const click = (e) => opened(e);
  const key = (e) => {
    if (e.key === 'Enter') opened(e);
  };
  node.addEventListener('click', click);
  node.addEventListener('keydown', key);
  return {
    update(next) {
      ctx = next;
      soon();
    },
    destroy() {
      here = false;
      clearTimeout(timer);
      seen.disconnect();
      node.removeEventListener('click', click);
      node.removeEventListener('keydown', key);
    },
  };
}
