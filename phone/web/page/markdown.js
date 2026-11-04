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

// Every pattern here stays linear on any text (an answer can carry words a
// web page put there): anchored, or with no two quantifiers that can trade
// the same characters.
const CITE = / ?\[\d{1,3}(?:, ?\d{1,3})*\]/g;
const LINK = /!?\[([^[\]]*)\]\([^()\s]+(?: "[^"]*")?\)/g;
const AUTOLINK = /<((?:https?:\/\/|mailto:)[^<>\s]+)>/g;
const LIST = /^(\s*)([-*+•]|\d{1,9}[.)])\s+(.*)$/;
const FENCE = /^\s{0,3}(`{3,}|~{3,})/;
const RULE = /^\s{0,3}([-*_])(?:\s*\1){2,}\s*$/;
const HEADING = /^\s{0,3}(#{1,6})(?:\s+(.*))?$/;
const QUOTE = /^\s{0,3}>/;

const blank = (s) => /^\s*$/.test(s);
const indentOf = (s) => s.match(/^\s*/)[0].replace(/\t/g, '    ').length;
const startsBlock = (s) => FENCE.test(s) || RULE.test(s) || HEADING.test(s) || QUOTE.test(s) || LIST.test(s);

// Quotes in quotes and lists in lists go this deep; deeper is their words.
const MAX_DEPTH = 4;

/** The answer's blocks: {type: p | h | list | code | quote | hr, …}. Pure. */
export function parseBlocks(src, depth = 0) {
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
      out.push({ type: 'h', level: m[1].length, text: headingText(m[2] || '') });
      i++;
      continue;
    }
    if (QUOTE.test(line) && depth < MAX_DEPTH) {
      const q = [];
      while (i < lines.length && QUOTE.test(lines[i])) q.push(lines[i++].replace(/^\s{0,3}>\s?/, ''));
      out.push({ type: 'quote', children: parseBlocks(q.join('\n'), depth + 1) });
      continue;
    }
    if (LIST.test(line)) {
      const [list, next] = parseList(lines, i, depth);
      out.push(list);
      i = next;
      continue;
    }
    const p = [];
    while (i < lines.length && !blank(lines[i]) && (p.length === 0 || !startsBlock(lines[i]))) p.push(lines[i++].trim());
    // (A quote too deep: its words, without their marks.)
    out.push({ type: 'p', lines: depth >= MAX_DEPTH ? p.map((l) => l.replace(/^(?:>\s?)+/, '')) : p });
  }
  return out;
}

/** A heading's words, its closing #s ("## Tides ##") gone. */
function headingText(s) {
  let t = s.trimEnd();
  let j = t.length;
  while (j > 0 && t[j - 1] === '#') j--;
  if (j < t.length && (j === 0 || /\s/.test(t[j - 1]))) t = t.slice(0, j).trimEnd();
  return t;
}

const ordered = (marker) => /\d/.test(marker);

/** A list from line `i`: [{type: 'list', ordered, start, items: [{lines, list}]}, the line after it]. */
function parseList(lines, i, depth) {
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
    // An item further out than this list: the list it's in carries on.
    if (m && ind < base - 1) break;
    const item = list.items[list.items.length - 1];
    if (m && (ind <= base + 1 || !item || depth + 1 >= MAX_DEPTH)) {
      // Another item here (a change of bullet mark doesn't end a list; a
      // change between bullets and numbers does).
      if (ordered(m[2]) !== list.ordered) break;
      list.items.push({ lines: [m[3].trim()], list: null });
      afterBlank = false;
      i++;
      continue;
    }
    if (m && item) {
      // One level in: a list inside the item.
      const [inner, next] = parseList(lines, i, depth + 1);
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

const wordChar = (c) => !!c && /[\p{L}\p{N}]/u.test(c);
const space = (c) => !c || /\s/.test(c);
// How far back a closing mark looks for its opening one: plenty for an
// answer, and a bound on the work for any text.
const LOOK_BACK = 32;

/** Words as shown: a citation gone, a bare <link> as its address. */
const words = (s) => s.replace(AUTOLINK, '$1').replace(CITE, '');

function appendText(doc, parent, s) {
  // Single line breaks kept (as Chat: `breaks: true`).
  s.split('\n').forEach((part, n) => {
    if (n > 0) parent.appendChild(doc.createElement('br'));
    if (part) parent.appendChild(doc.createTextNode(part));
  });
}

/**
 * The marks in `s` (no code in it): runs of *, _ and ~~ paired as
 * CommonMark's delimiter runs are, simplified — a run can open when a
 * non-space follows it, close when one comes before it (an _ not inside a
 * word); a closer takes the nearest opener of its kind, two of each for
 * <strong> (~~ for <s>), one for <em>. One pass, each closer looking back
 * at most LOOK_BACK runs.
 */
function appendMarked(doc, parent, s) {
  const items = [];
  let last = 0;
  for (const m of s.matchAll(/\*+|_+|~+/g)) {
    if (m.index > last) items.push({ text: s.slice(last, m.index) });
    last = m.index + m[0].length;
    const ch = m[0][0];
    if (ch === '~' && m[0].length !== 2) {
      items.push({ text: m[0] });
      continue;
    }
    const before = s[m.index - 1];
    const after = s[last];
    let open = !space(after);
    let close = !space(before);
    if (ch === '_') {
      open = open && !wordChar(before);
      close = close && !wordChar(after);
    }
    items.push({ ch, n: m[0].length, open, close, opens: [], closes: [] });
  }
  if (last < s.length) items.push({ text: s.slice(last) });

  const stack = []; // indexes of runs that may still open
  items.forEach((it, i) => {
    if (it.text != null) return;
    if (it.close) {
      for (let k = stack.length - 1; k >= 0 && k >= stack.length - LOOK_BACK && it.n > 0; k--) {
        const o = items[stack[k]];
        if (o.ch !== it.ch) continue;
        const use = it.ch === '~' ? 2 : o.n >= 2 && it.n >= 2 ? 2 : 1;
        if (o.n < use || it.n < use) continue;
        const tag = it.ch === '~' ? 's' : use === 2 ? 'strong' : 'em';
        o.n -= use;
        it.n -= use;
        o.opens.push(tag);
        it.closes.push(tag);
        // Runs between the two can't pair any more; a spent opener goes too.
        stack.length = o.n > 0 ? k + 1 : k;
        k = stack.length;
      }
    }
    if (it.open && it.n > 0) stack.push(i);
  });

  const into = [parent];
  for (const it of items) {
    if (it.text != null) {
      appendText(doc, into[into.length - 1], words(it.text));
      continue;
    }
    for (let c = 0; c < it.closes.length && into.length > 1; c++) into.pop();
    // What's left of a run is text — but a ** with nothing to pair (bold
    // still being written) goes, so it never flickers in.
    const left = it.ch === '*' ? '*'.repeat(it.n % 2) : it.ch.repeat(it.n);
    if (left) appendText(doc, into[into.length - 1], left);
    for (let o = it.opens.length - 1; o >= 0; o--) {
      const el = doc.createElement(it.opens[o]);
      into[into.length - 1].appendChild(el);
      into.push(el);
    }
  }
}

/** Code spans in `s`: [{from, to, code}], a run of backticks closed by the next run as long. */
function codeSpans(s) {
  const runs = [];
  for (let i = 0; i < s.length; ) {
    if (s[i] !== '`') {
      i++;
      continue;
    }
    let j = i;
    while (j < s.length && s[j] === '`') j++;
    runs.push({ at: i, len: j - i });
    i = j;
  }
  const spans = [];
  for (let k = 0; k < runs.length; ) {
    let close = k + 1;
    while (close < runs.length && runs[close].len !== runs[k].len) close++;
    if (close >= runs.length) {
      k++;
      continue;
    }
    const code = s.slice(runs[k].at + runs[k].len, runs[close].at);
    spans.push({ from: runs[k].at, to: runs[close].at + runs[close].len, code });
    k = close + 1;
  }
  return spans;
}

/** `s`'s inline markdown into `parent`: code spans first, then the marks. */
export function appendInline(doc, parent, s) {
  // Links' words first, so a mark inside a link's address isn't read as one.
  const text = s.replace(LINK, '$1');
  let pos = 0;
  for (const span of codeSpans(text)) {
    appendMarked(doc, parent, text.slice(pos, span.from));
    const code = doc.createElement('code');
    const c = span.code;
    code.textContent = c.length > 2 && c[0] === ' ' && c[c.length - 1] === ' ' ? c.slice(1, -1) : c;
    parent.appendChild(code);
    pos = span.to;
  }
  // A code span still open while it streams: its backticks go.
  appendMarked(doc, parent, text.slice(pos).replace(/`/g, ''));
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
      if (!el.textContent.trim()) continue;
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
