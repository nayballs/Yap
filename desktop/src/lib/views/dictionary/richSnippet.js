// Rich snippets (2026-10-31): a snippet's text may carry formatting —
// bold, italic, links, bulleted and numbered lists, line breaks — in Yap's
// own small markdown subset, stored as the snippet's text (an older Yap
// still reads it as text). The twin of src-tauri/src/pasting/rich.rs,
// which turns it into the paste's HTML and plain text; this one is for the
// Dictionary view: whether a snippet is rich, the preview of how it
// pastes (`parse` → a tree the view draws, never HTML strings), its plain
// text, and the formatting bar's edits (pure: text and selection in,
// text and selection out). e2e/dictionary-list.spec.js holds both to the
// same cases.
//
// The subset: **bold**, *italic* or _italic_ (never inside a word), a
// [link](https://…) — http(s) or mailto only —, lines starting "- ", "* ",
// "+ " or "• " (a bulleted list) or "1. " / "1) " (numbered, from the first
// number), a backslash before punctuation for the character itself, and
// every line break a line break. Nothing else: "<b>" is four characters.

const isAlnum = (c) => c !== undefined && /[\p{Alphabetic}\p{N}]/u.test(c);
const isSpace = (c) => c !== undefined && /\s/u.test(c);
const isPunct = (c) => c !== undefined && /^[!-/:-@[-`{-~]$/.test(c);
const isControl = (c) => /\p{Cc}/u.test(c);

/** Whether the character at `j` is escaped (an odd run of backslashes before it). */
function escaped(chars, j) {
  let n = 0;
  for (let k = j - 1; k >= 0 && chars[k] === '\\'; k--) n++;
  return n % 2 === 1;
}

function runLen(chars, i) {
  let n = 0;
  while (chars[i + n] === chars[i]) n++;
  return n;
}

/** A link a snippet may carry: http://, https:// (somewhere to go) or mailto:. */
export function safeLink(url) {
  const lower = String(url ?? '').toLowerCase();
  const prefix = ['http://', 'https://', 'mailto:'].find((p) => lower.startsWith(p));
  return !!prefix && lower.length > prefix.length && ![...url].some((c) => isSpace(c) || isControl(c));
}

/** A list item's line: { ordered, start, rest } or null. */
function listItem(line) {
  const t = line.replace(/^[ \t]+/, '');
  const first = [...t][0];
  if (first === undefined) return null;
  if ('-*+•'.includes(first)) {
    const rest = t.slice(first.length);
    return /^[ \t]/.test(rest) ? { ordered: false, start: 0, rest: rest.replace(/^[ \t]+/, '') } : null;
  }
  const m = /^(\d{1,9})[.)]([ \t]+)/.exec(t);
  if (!m) return null;
  return { ordered: true, start: Number(m[1]), rest: t.slice(m[0].length) };
}

function closer(chars, from, d, need) {
  let j = from;
  while (j < chars.length) {
    if (chars[j] !== d || escaped(chars, j)) {
      j++;
      continue;
    }
    const run = runLen(chars, j);
    const afterOk = !isAlnum(chars[j + run]);
    const beforeOk = !isSpace(chars[j - 1]);
    const fits = need === 1 ? run === 1 : run >= need;
    if (fits && afterOk && beforeOk) return [j, run];
    j += run;
  }
  return null;
}

function emphasisAt(chars, i, links) {
  const d = chars[i];
  const run = runLen(chars, i);
  const beforeOk = i === 0 || !isAlnum(chars[i - 1]);
  const opens = (len) => beforeOk && chars[i + len] !== undefined && !isSpace(chars[i + len]);
  if (run >= 2 && opens(2)) {
    const c = closer(chars, i + 2, d, 2);
    if (c) {
      const [j, len] = c;
      const k = j + len - 2;
      if (k > i + 2) return [{ type: 'bold', children: inlines(chars.slice(i + 2, k).join(''), links) }, j + len];
    }
  }
  if (run === 1 && opens(1)) {
    const c = closer(chars, i + 1, d, 1);
    if (c && c[0] > i + 1) return [{ type: 'italic', children: inlines(chars.slice(i + 1, c[0]).join(''), links) }, c[0] + 1];
  }
  return null;
}

function linkAt(chars, i) {
  let close = -1;
  for (let j = i + 1; j < chars.length; j++) {
    if (chars[j] === ']' && !escaped(chars, j)) {
      close = j;
      break;
    }
  }
  if (close < 0 || chars.slice(i + 1, close).includes('[') || chars[close + 1] !== '(') return null;
  let depth = 0;
  let end = -1;
  for (let j = close + 2; j < chars.length; j++) {
    const c = chars[j];
    if (isSpace(c) || isControl(c)) return null;
    if (c === '(') depth++;
    else if (c === ')') {
      if (depth === 0) {
        end = j;
        break;
      }
      depth--;
    }
  }
  if (end < 0) return null;
  const raw = chars.slice(close + 2, end).join('');
  const url = raw.startsWith('<') && raw.endsWith('>') ? raw.slice(1, -1) : raw;
  if (!safeLink(url)) return null;
  return [chars.slice(i + 1, close).join(''), url, end + 1];
}

function inlines(s, links) {
  const chars = [...s];
  const out = [];
  let buf = '';
  const flush = () => {
    if (buf) out.push({ type: 'text', text: buf });
    buf = '';
  };
  let i = 0;
  while (i < chars.length) {
    const c = chars[i];
    if (c === '\\' && isPunct(chars[i + 1])) {
      buf += chars[i + 1];
      i += 2;
      continue;
    }
    if (c === '[' && links) {
      const l = linkAt(chars, i);
      if (l) {
        flush();
        out.push({ type: 'link', children: inlines(l[0], false), url: l[1] });
        i = l[2];
        continue;
      }
    }
    if (c === '*' || c === '_') {
      const e = emphasisAt(chars, i, links);
      if (e) {
        flush();
        out.push(e[0]);
        i = e[1];
        continue;
      }
      const run = runLen(chars, i);
      buf += c.repeat(run);
      i += run;
      continue;
    }
    buf += c;
    i++;
  }
  flush();
  return out;
}

/**
 * The subset, line by line: `{ type: 'line', inlines }` and
 * `{ type: 'list', ordered, start, items: [inlines] }`; an inline is
 * `{ type: 'text', text }`, `{ type: 'bold' | 'italic', children }` or
 * `{ type: 'link', children, url }`.
 */
export function parse(md) {
  const blocks = [];
  for (const line of String(md ?? '').replace(/\r\n?/g, '\n').split('\n')) {
    const item = listItem(line);
    if (item) {
      const nodes = inlines(item.rest, true);
      const last = blocks.at(-1);
      if (last?.type === 'list' && last.ordered === item.ordered) last.items.push(nodes);
      else blocks.push({ type: 'list', ordered: item.ordered, start: item.start, items: [nodes] });
    } else {
      blocks.push({ type: 'line', inlines: inlines(line, true) });
    }
  }
  return blocks;
}

/** Whether a snippet's text uses the formatting (bold, italic, a link, a list). */
export function isRich(md) {
  return parse(md).some((b) => b.type === 'list' || b.inlines.some((n) => n.type !== 'text'));
}

/** A link's address as a person reads it ("mailto:" off). */
export const shownUrl = (url) => (/^mailto:/i.test(url) && url.length > 7 ? url.slice(7) : url);

function plainInlines(nodes) {
  let out = '';
  for (const n of nodes) {
    if (n.type === 'text') out += n.text;
    else if (n.type === 'link') {
      const label = plainInlines(n.children);
      const shown = shownUrl(n.url);
      out += !label.trim() || label === shown || label === n.url ? shown : `${label} (${shown})`;
    } else out += plainInlines(n.children);
  }
  return out;
}

/** The text as it pastes where formatting can't go: bullets as "• ", links as "text (url)". */
export function toPlain(md) {
  const lines = [];
  for (const b of parse(md)) {
    if (b.type === 'line') lines.push(plainInlines(b.inlines));
    else b.items.forEach((item, n) => lines.push(`${b.ordered ? `${b.start + n}. ` : '• '}${plainInlines(item)}`));
  }
  return lines.join('\n');
}

// ---- The formatting bar's edits ----------------------------------------------------
//
// Each takes `{ text, start, end }` (the selection) and returns the same
// for after the edit; the view applies the difference to the text box so
// its own Undo (Ctrl+Z) takes it back.

/** The `*` (or `_`) runs just before `at` and just after `end` in `text`. */
function runsAround(text, at, end, d) {
  let before = 0;
  while (text[at - 1 - before] === d) before++;
  let after = 0;
  while (text[end + after] === d) after++;
  return [before, after];
}

const isWrapped = (b, a, m) => (m === '**' ? b >= 2 && a >= 2 : b % 2 === 1 && a % 2 === 1);

/**
 * One line's part of the selection (`text.slice(from, to)`): where its
 * words start and end (spaces and a list marker at its line's start left
 * out), or null when there are none.
 */
function core(text, from, to) {
  let a = from;
  let b = to;
  if (a === 0 || text[a - 1] === '\n') {
    const item = listItem(text.slice(a, to));
    if (item) a = to - item.rest.length;
  }
  while (a < b && isSpace(text[a])) a++;
  while (b > a && isSpace(text[b - 1])) b--;
  return a < b ? [a, b] : null;
}

/** Bold (`**`) or italic (`*`) on the selection, or off when it's on already; per line. */
export function toggleWrap({ text, start, end }, m) {
  const d = m[0];
  if (start === end) {
    // Nothing selected: the markers, the caret between them.
    return { text: text.slice(0, start) + m + m + text.slice(end), start: start + m.length, end: start + m.length };
  }
  // The selection's lines, each its words.
  const parts = [];
  let from = start;
  for (;;) {
    const nl = text.indexOf('\n', from);
    const to = nl < 0 || nl >= end ? end : nl;
    const c = core(text, from, to);
    if (c) parts.push(c);
    if (to >= end) break;
    from = to + 1;
  }
  if (!parts.length) return { text, start, end };
  // On already (the markers around the words, or the words' own): off.
  const state = parts.map(([a, b]) => {
    const [outB, outA] = runsAround(text, a, b, d);
    if (isWrapped(outB, outA, m)) return 'outside';
    const inner = text.slice(a, b);
    let ib = 0;
    while (inner[ib] === d) ib++;
    let ia = 0;
    while (inner[inner.length - 1 - ia] === d) ia++;
    return inner.length > 2 * m.length && isWrapped(ib, ia, m) ? 'inside' : null;
  });
  const off = state.every(Boolean);
  const len = m.length;
  let out = '';
  let last = 0;
  let newStart = start;
  let newEnd = end;
  parts.forEach(([a, b], k) => {
    let words;
    if (off) {
      // The markers next to the words go (the innermost: "***x***" keeps the other).
      const outside = state[k] === 'outside';
      out += text.slice(last, outside ? a - len : a);
      words = outside ? text.slice(a, b) : text.slice(a + len, b - len);
      last = outside ? b + len : b;
    } else {
      out += text.slice(last, a) + m;
      words = text.slice(a, b);
      last = b;
    }
    // The words stay selected.
    if (k === 0) newStart = out.length;
    out += words;
    newEnd = out.length;
    if (!off) out += m;
  });
  out += text.slice(last);
  return { text: out, start: newStart, end: newEnd };
}

/** An email address or a web address, as the person selected it: the link it is. */
function asLink(s) {
  const t = s.trim();
  if (safeLink(t)) return t;
  if (/^www\.\S+\.\S+$/i.test(t)) return `https://${t}`;
  if (/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(t)) return `mailto:${t}`;
  return null;
}

/**
 * A link: the selection becomes its text, with "https://" selected for the
 * address; a selected address becomes the link, its text selected to type;
 * nothing selected, both to fill in.
 */
export function insertLink({ text, start, end }) {
  const sel = text.slice(start, end);
  const lead = sel.length - sel.trimStart().length;
  const words = sel.trim();
  const a = start + lead;
  const b = a + words.length;
  const url = words && asLink(words);
  if (url) {
    const label = words.includes('@') && url.startsWith('mailto:') ? words : 'link text';
    const md = `[${label}](${url})`;
    return { text: text.slice(0, a) + md + text.slice(b), start: a + 1, end: a + 1 + label.length };
  }
  const label = words || 'link text';
  const md = `[${label}](https://)`;
  const at = words ? a : start;
  const tail = words ? b : end;
  const urlAt = at + label.length + 3;
  return {
    text: text.slice(0, at) + md + text.slice(tail),
    start: words ? urlAt : at + 1,
    end: words ? urlAt + 'https://'.length : at + 1 + label.length,
  };
}

/** A bulleted (or numbered) list over the selection's lines, or no list when they're one already. */
export function toggleList({ text, start, end }, ordered) {
  const lineStart = text.lastIndexOf('\n', start - 1) + 1;
  // A selection ending at a line's start leaves that line out.
  const stop = end > start && text[end - 1] === '\n' ? end - 1 : end;
  const nl = text.indexOf('\n', stop);
  const lineEnd = nl < 0 ? text.length : nl;
  const lines = text.slice(lineStart, lineEnd).split('\n');
  const filled = lines.filter((l) => l.trim());
  const already = filled.length > 0 && filled.every((l) => listItem(l)?.ordered === ordered);
  let n = 0;
  const next = lines.map((l) => {
    if (!l.trim()) return l;
    const item = listItem(l);
    const words = item ? item.rest : l.replace(/^[ \t]+/, '');
    if (already) return words;
    n++;
    return `${ordered ? `${n}. ` : '- '}${words}`;
  });
  const block = next.join('\n');
  return {
    text: text.slice(0, lineStart) + block + text.slice(lineEnd),
    start: lineStart,
    end: lineStart + block.length,
  };
}
