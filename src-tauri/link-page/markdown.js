// Yap Link: an answer's markdown, shown as Chat shows it (src/lib/core/
// markdown.js, its spoken-answer subset): paragraphs with their line breaks
// kept, headings, bullet and numbered lists (one level inside another, "• "
// lines too, blank lines between items still one list), **bold**,
// *italic*, ~~struck~~, `code`, fenced code, quotes and rules.
//
// Built as DOM nodes with text in them, never as HTML: whatever a model
// writes (`<img onerror=…>`, `<script>`) shows as text. Links show as their
// words, as before (a link here would take the page away from Yap), and
// "[1]"-style citations are left out (the phone has no sources to show).
// A mark left open while the answer streams in ("**3.9 met") shows without
// its stars, so nothing flickers when it closes.

const CITE = /[ \t]*\[\d+(?:\s*,\s*\d+)*\]/g;
const LIST = /^(\s*)([-*+•]|\d{1,9}[.)])\s+(.*)$/;
const FENCE = /^\s{0,3}(`{3,}|~{3,})/;
const RULE = /^\s{0,3}([-*_])(?:\s*\1){2,}\s*$/;
const HEADING = /^\s{0,3}(#{1,6})\s+(.*?)(?:\s+#+)?\s*$/;
const QUOTE = /^\s{0,3}>/;

const blank = (s) => /^\s*$/.test(s);
const indentOf = (s) => s.match(/^\s*/)[0].replace(/\t/g, '    ').length;
const startsBlock = (s) => FENCE.test(s) || RULE.test(s) || HEADING.test(s) || QUOTE.test(s) || LIST.test(s);

/** The answer's blocks: {type: p | h | list | code | quote | hr, …}. Pure. */
export function parseBlocks(src) {
  const lines = String(src).replace(/\r\n?/g, '\n').split('\n');
  const out = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (blank(line)) {
      i++;
      continue;
    }
    let m = line.match(FENCE);
    if (m) {
      // A fence an answer cut off (still streaming) still shows as code.
      const body = [];
      i++;
      while (i < lines.length && !lines[i].trim().startsWith(m[1])) body.push(lines[i++]);
      i++;
      out.push({ type: 'code', text: body.join('\n') });
      continue;
    }
    if (RULE.test(line)) {
      out.push({ type: 'hr' });
      i++;
      continue;
    }
    m = line.match(HEADING);
    if (m) {
      out.push({ type: 'h', level: m[1].length, text: m[2] });
      i++;
      continue;
    }
    if (QUOTE.test(line)) {
      const q = [];
      while (i < lines.length && QUOTE.test(lines[i])) q.push(lines[i++].replace(/^\s{0,3}>\s?/, ''));
      out.push({ type: 'quote', children: parseBlocks(q.join('\n')) });
      continue;
    }
    if (LIST.test(line)) {
      const [list, next] = parseList(lines, i);
      out.push(list);
      i = next;
      continue;
    }
    const p = [];
    while (i < lines.length && !blank(lines[i]) && (p.length === 0 || !startsBlock(lines[i]))) p.push(lines[i++].trim());
    out.push({ type: 'p', lines: p });
  }
  return out;
}

const ordered = (marker) => /\d/.test(marker);

/** A list from line `i`: [{type: 'list', ordered, start, items: [{lines, list}]}, the line after it]. */
function parseList(lines, i) {
  const first = lines[i].match(LIST);
  const base = indentOf(first[1]);
  const list = { type: 'list', ordered: ordered(first[2]), start: ordered(first[2]) ? parseInt(first[2], 10) : 1, items: [] };
  let afterBlank = false;
  while (i < lines.length) {
    const line = lines[i];
    if (blank(line)) {
      afterBlank = true;
      i++;
      continue;
    }
    const m = line.match(LIST);
    const ind = indentOf(line);
    if (m && ind <= base + 1) {
      // Another item here (a change of bullet mark doesn't end a list; a
      // change between bullets and numbers does).
      if (ordered(m[2]) !== list.ordered) break;
      list.items.push({ lines: [m[3].trim()], list: null });
      afterBlank = false;
      i++;
      continue;
    }
    const item = list.items[list.items.length - 1];
    if (m && ind > base + 1 && item) {
      // One level in: a list inside the item.
      const [inner, next] = parseList(lines, i);
      if (item.list) item.list.items.push(...inner.items);
      else item.list = inner;
      afterBlank = false;
      i = next;
      continue;
    }
    // More of the item's words: indented, or straight after it.
    if (item && (ind > base || !afterBlank) && !startsBlock(line)) {
      item.lines.push(line.trim());
      i++;
      continue;
    }
    break;
  }
  return [list, i];
}

// ------------------------------------------------------------- inline

const MARKS = [
  { open: '**', tag: 'strong' },
  { open: '__', tag: 'strong', word: true },
  { open: '~~', tag: 's' },
  { open: '*', tag: 'em' },
  { open: '_', tag: 'em', word: true },
];
const wordChar = (c) => !!c && /[\p{L}\p{N}]/u.test(c);

/** The earliest well-formed mark in `s` from `from`: {at, end, mark, inner} or null. */
function nextMark(s, from) {
  for (let at = from; at < s.length; at++) {
    for (const mark of MARKS) {
      if (!s.startsWith(mark.open, at)) continue;
      const after = s[at + mark.open.length];
      if (!after || /\s/.test(after)) continue;
      if (mark.word && wordChar(s[at - 1])) continue;
      // A single * or _ that's really half of ** or __ is left to that one.
      if (mark.open.length === 1 && s[at + 1] === mark.open) continue;
      let close = s.indexOf(mark.open, at + mark.open.length + 1);
      while (close !== -1) {
        const before = s[close - 1];
        const doubled = mark.open.length === 1 && s[close + 1] === mark.open;
        const wordy = mark.word && wordChar(s[close + mark.open.length]);
        if (!/\s/.test(before) && !doubled && !wordy) break;
        close = s.indexOf(mark.open, close + 1);
      }
      if (close === -1) continue;
      return { at, end: close + mark.open.length, mark, inner: s.slice(at + mark.open.length, close) };
    }
  }
  return null;
}

/** Plain words: links as their words, citations and a stray ** gone (bold still being written; `__` and `~~` stay, as in `__init__`). */
function words(s) {
  return s
    .replace(/!?\[([^\]]*)\]\((?:[^)\s]+)(?:\s+"[^"]*")?\)/g, '$1')
    .replace(/<((?:https?:\/\/|mailto:)[^>\s]+)>/g, '$1')
    .replace(CITE, '')
    .replace(/\*\*/g, '');
}

function appendText(doc, parent, s) {
  // Single line breaks kept (as Chat: `breaks: true`).
  const parts = s.split('\n');
  parts.forEach((part, n) => {
    if (n > 0) parent.appendChild(doc.createElement('br'));
    if (part) parent.appendChild(doc.createTextNode(part));
  });
}

function appendMarked(doc, parent, s) {
  let pos = 0;
  while (pos < s.length) {
    const m = nextMark(s, pos);
    if (!m) break;
    appendText(doc, parent, words(s.slice(pos, m.at)));
    const el = doc.createElement(m.mark.tag);
    appendMarked(doc, el, m.inner);
    parent.appendChild(el);
    pos = m.end;
  }
  appendText(doc, parent, words(s.slice(pos)));
}

/** `s`'s inline markdown into `parent`: code spans first, then the marks. */
export function appendInline(doc, parent, s) {
  // Links' words first, so a mark inside a link's address isn't read as one.
  const text = s.replace(/!?\[([^\]]*)\]\((?:[^)\s]+)(?:\s+"[^"]*")?\)/g, '$1');
  const re = /(`+)([\s\S]*?[^`])\1(?!`)/g;
  let pos = 0;
  for (let m = re.exec(text); m; m = re.exec(text)) {
    appendMarked(doc, parent, text.slice(pos, m.index));
    const code = doc.createElement('code');
    code.textContent = m[2].replace(/^ (.*) $/, '$1');
    parent.appendChild(code);
    pos = m.index + m[0].length;
  }
  // A code span still open while it streams: its backtick goes.
  appendMarked(doc, parent, text.slice(pos).replace(/`+/g, ''));
}

// --------------------------------------------------------------- build

function buildList(doc, list) {
  const el = doc.createElement(list.ordered ? 'ol' : 'ul');
  if (list.ordered && list.start !== 1) el.start = list.start;
  for (const item of list.items) {
    const li = doc.createElement('li');
    appendInline(doc, li, item.lines.join('\n'));
    if (item.list) li.appendChild(buildList(doc, item.list));
    el.appendChild(li);
  }
  return el;
}

function build(doc, blocks, into) {
  for (const b of blocks) {
    let el;
    if (b.type === 'p') {
      el = doc.createElement('p');
      appendInline(doc, el, b.lines.join('\n'));
      // A paragraph that was only a citation, or marks, says nothing.
      if (!el.textContent.trim()) continue;
    } else if (b.type === 'h') {
      // Shifted down, as Chat's: # and ## → h3, the rest → h4.
      el = doc.createElement(b.level <= 2 ? 'h3' : 'h4');
      appendInline(doc, el, b.text);
    } else if (b.type === 'list') {
      el = buildList(doc, b);
    } else if (b.type === 'code') {
      el = doc.createElement('pre');
      const code = doc.createElement('code');
      code.textContent = b.text;
      el.appendChild(code);
    } else if (b.type === 'quote') {
      el = doc.createElement('blockquote');
      build(doc, b.children, el);
    } else {
      el = doc.createElement('hr');
    }
    into.appendChild(el);
  }
  return into;
}

/** The answer `text` as nodes for its card: a DocumentFragment. */
export function renderAnswer(text, doc = document) {
  return build(doc, parseBlocks(text), doc.createDocumentFragment());
}
