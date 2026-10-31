// A document's markdown in and out of its editor (DocPanel's own chunk),
// exactly: the note stays markdown, and what the person didn't change goes
// back out character for character (2026-10-31).
//
// In (`readMarkdown`): markdown-it reads it as the page does (markdown.js
// `markdownTokens`, Yap's rules and all) and each block becomes the
// editor's — paragraphs, headings, quotes, lists (checklists too), code,
// rules, tables, the sections still to write — keeping the lines it came
// from (`md`) and the lines before it (`before`): a document's blocks, each
// list item, each table row. What the editor can't hold as such is a
// `rawBlock`, kept whole.
//
// Out (`writeMarkdown`): a block that's as it was read (the same as the
// block read from its own lines, `kept`) is written as those lines; one the
// person changed is written afresh, the way it was written where that's
// known (its list marker, its fence, `__bold__`…), with the parts of it
// that didn't change (a list's other items, a table's other rows) still as
// they were; a new one in plain markdown. A block written afresh is read
// back to check it says the same, with its words escaped only where they'd
// say something else ("1. " starting a line, a `*` that would turn bold);
// so a change to one word changes that word's block, and only that.
// Where markdown can't say the marks as they are (bold opening on "+" right
// after a letter, two marks overlapping), the mark takes in that word's
// letters, or that block goes without that kind of mark: the words are
// always kept, never a stray "*" (tested over every place a character can
// be typed and every few characters a mark can cover:
// e2e/doc-markdown.spec.js). `head` (the title line, which the panel shows
// above the page) and the lines after the last block come back as they were.
import { markdownTokens } from '../../core/markdown.js';

const BLANK = /^[ \t]*\r?$/;

/** The index of the token that closes `tokens[i]` (itself when it's whole). */
function closing(tokens, i) {
  if (tokens[i].nesting !== 1) return i;
  let depth = 0;
  for (let j = i; j < tokens.length; j++) {
    depth += tokens[j].nesting;
    if (depth === 0) return j;
  }
  return tokens.length - 1;
}

/**
 * Where tokens `i`..`j` end in the text: the furthest any of them reaches
 * (two lists markdown.js joined into one — "- a" then "* b" — keep the
 * first list's lines on the opening token).
 */
function reach(tokens, i, j) {
  let e = 0;
  for (let k = i; k <= j; k++) if (tokens[k].map) e = Math.max(e, tokens[k].map[1]);
  return e;
}

/** `[s, e)`'s end without the blank lines it ends in (never before `s + 1`). */
function trimEnd(lines, s, e) {
  while (e > s + 1 && BLANK.test(lines[e - 1] ?? '')) e--;
  return e;
}

/** `lines` with the indent `ind` taken off each line that has it. */
const unindent = (lines, ind) => (ind ? lines.map((l) => (l.startsWith(ind) ? l.slice(ind.length) : l.trimStart())) : lines);

/**
 * The document's title line (`# Title`, shown above the page) and the
 * markdown under it. Pure.
 */
export function splitHead(text, title) {
  const s = String(text ?? '');
  const nl = s.indexOf('\n');
  const first = (nl < 0 ? s : s.slice(0, nl)).trim();
  if (/^#\s+/.test(first) && first.replace(/^#\s+/, '').trim() === String(title ?? '').trim() && title) {
    return { head: nl < 0 ? `${s}\n` : s.slice(0, nl + 1), body: nl < 0 ? '' : s.slice(nl + 1) };
  }
  return { head: '', body: s };
}

// ---- in --------------------------------------------------------------------------------

/**
 * `body` read for the editor: `{doc, kept, tail}` — the editor's document
 * (`schema`'s nodes), the blocks as read (by their `md`: `isKept`), and the
 * lines after the last block. Pure.
 */
export function readMarkdown(schema, body) {
  const text = String(body ?? '');
  const lines = text.split('\n');
  const ctx = { schema, lines, kept: new Map() };
  const tokens = markdownTokens(text);
  const blocks = [];
  let end = 0;
  for (let i = 0; i < tokens.length; ) {
    const j = closing(tokens, i);
    const t = tokens[i];
    if (!t.map) {
      i = j + 1;
      continue;
    }
    const s = Math.max(t.map[0], end);
    const e = trimEnd(lines, s, Math.max(reach(tokens, i, j), s + 1));
    const md = lines.slice(s, e).join('\n');
    let node = null;
    try {
      node = block(ctx, tokens, i, j);
      node?.check();
    } catch {
      node = null;
    }
    node ??= schema.nodes.rawBlock.create({ md });
    node = keep(ctx, node.type.create({ ...node.attrs, md, before: lines.slice(end, s) }, node.content, node.marks));
    blocks.push(node);
    end = e;
    i = j + 1;
  }
  const doc = schema.nodes.doc.create(null, blocks.length ? blocks : [schema.nodes.paragraph.create()]);
  return { doc, kept: ctx.kept, tail: lines.slice(end) };
}

/** Remember `node` as read: written back verbatim while it's the same. */
function keep(ctx, node) {
  const md = node.attrs.md;
  if (!ctx.kept.has(md)) ctx.kept.set(md, []);
  ctx.kept.get(md).push(node);
  return node;
}

/** The blocks of `tokens` (from, to) — inside a quote or a list item. */
function blocksIn(ctx, tokens, from, to) {
  const out = [];
  for (let k = from; k < to; ) {
    const kc = closing(tokens, k);
    const node = block(ctx, tokens, k, kc);
    if (!node) throw new Error(`no block for ${tokens[k].type}`);
    out.push(node);
    k = kc + 1;
  }
  return out;
}

/** The editor's block for tokens `i`..`j` (`null`: none, kept raw). */
function block(ctx, tokens, i, j) {
  const t = tokens[i];
  const N = ctx.schema.nodes;
  switch (t.type) {
    case 'paragraph_open':
      return N.paragraph.create(null, inline(ctx, tokens[i + 1]));
    case 'heading_open': {
      const level = t.markup[0] === '#' ? t.markup.length : t.markup[0] === '=' ? 1 : 2;
      return N.heading.create({ level }, inline(ctx, tokens[i + 1]));
    }
    case 'blockquote_open': {
      // (Inside a quote a line carries its "> ": its items and rows aren't
      // kept as lines of their own, the quote is.)
      ctx.quoted = (ctx.quoted ?? 0) + 1;
      try {
        return N.blockquote.create(null, blocksIn(ctx, tokens, i + 1, j));
      } finally {
        ctx.quoted -= 1;
      }
    }
    case 'bullet_list_open':
    case 'ordered_list_open':
      return list(ctx, tokens, i, j);
    case 'fence': {
      const info = t.info || '';
      const code = t.content.replace(/\n$/, '');
      return N.codeBlock.create({ language: info.trim().split(/\s+/)[0] || null, fence: t.markup, info }, code ? ctx.schema.text(code) : null);
    }
    case 'code_block': {
      const code = t.content.replace(/\n$/, '');
      return N.codeBlock.create({ fence: null }, code ? ctx.schema.text(code) : null);
    }
    case 'hr':
      return N.horizontalRule.create({ markup: t.markup });
    case 'table_open':
      return table(ctx, tokens, i, j);
    case 'yap_pending':
      return N.yapSection.create({ key: t.meta.key, intent: t.meta.intent });
    default:
      return null;
  }
}

function list(ctx, tokens, i, j) {
  const N = ctx.schema.nodes;
  const { lines } = ctx;
  const open = tokens[i];
  const ordered = open.type === 'ordered_list_open';
  const start = ordered ? Number(open.attrGet('start') ?? 1) : null;
  const items = [];
  let prevEnd = null;
  let seq = true;
  let loose = false;
  for (let k = i + 1; k < j; ) {
    const it = tokens[k];
    const kc = closing(tokens, k);
    if (it.type !== 'list_item_open' || !it.map) throw new Error('not a list item');
    const s = it.map[0];
    const e = trimEnd(lines, s, it.map[1]);
    const before = prevEnd == null ? [] : lines.slice(prevEnd, s);
    if (before.length) loose = true;
    // A checklist item: markdown.js's rule took "[ ] " off and left a box.
    const first = tokens[k + 1]?.type === 'paragraph_open' ? tokens[k + 2] : null;
    const box = first?.children?.find((c) => c.type === 'yap_task');
    const kids = blocksIn(ctx, tokens, k + 1, kc);
    // An item's first line is its words: none ("-" over a nested list) is an empty one.
    if (!kids.length || kids[0].type === N.bulletList || kids[0].type === N.orderedList) kids.unshift(N.paragraph.create());
    if (kids[0].type !== N.paragraph) throw new Error('a list item starting with something else');
    if (ordered && Number(it.info) !== start + items.length) seq = false;
    const ind = /^[ \t]*/.exec(lines[s])[0];
    const md = ctx.quoted ? null : unindent(lines.slice(s, e), ind).join('\n');
    const marker = ordered ? `${it.info}${it.markup}` : it.markup;
    const item = N.listItem.create({ checked: box ? box.meta.checked : null, marker, ind, md, before }, kids);
    items.push(md == null ? item : keep(ctx, item));
    prevEnd = e;
    k = kc + 1;
  }
  if (!items.length) throw new Error('an empty list');
  return ordered
    ? N.orderedList.create({ start, delim: open.markup || '.', loose, seq }, items)
    : N.bulletList.create({ marker: open.markup || '-', loose }, items);
}

function table(ctx, tokens, i, j) {
  const N = ctx.schema.nodes;
  const { lines } = ctx;
  const rows = [];
  for (let k = i + 1; k < j; k++) {
    const tr = tokens[k];
    if (tr.type !== 'tr_open') continue;
    const kc = closing(tokens, k);
    const cells = [];
    for (let c = k + 1; c < kc; c++) {
      const cell = tokens[c];
      if (cell.type !== 'th_open' && cell.type !== 'td_open') continue;
      const align = /text-align:(\w+)/.exec(cell.attrGet('style') || '')?.[1] ?? null;
      const type = cell.type === 'th_open' ? N.tableHeader : N.tableCell;
      cells.push(type.create({ align }, N.paragraph.create(null, inline(ctx, tokens[c + 1]))));
    }
    const md = tr.map && !ctx.quoted ? lines[tr.map[0]].trim() : null;
    rows.push(md == null ? N.tableRow.create(null, cells) : keep(ctx, N.tableRow.create({ md }, cells)));
    k = kc;
  }
  const open = tokens[i];
  return N.table.create({ rule: open.map && !ctx.quoted ? (lines[open.map[0] + 1] ?? '').trim() : null }, rows);
}

/** An inline token's children as the editor's text, marks and breaks. */
function inline(ctx, tok) {
  const S = ctx.schema;
  const out = [];
  const active = [];
  const set = () => active.reduce((s, m) => m.addToSet(s), []);
  const text = (t, extra = null) => {
    if (!t) return;
    let marks = set();
    if (extra) marks = extra.addToSet(marks);
    out.push(S.text(t, marks));
  };
  const pop = (name) => {
    for (let k = active.length - 1; k >= 0; k--) {
      if (active[k].type.name === name) {
        active.splice(k, 1);
        return;
      }
    }
  };
  for (const c of tok?.children ?? []) {
    switch (c.type) {
      case 'text':
        text(c.content);
        break;
      case 'softbreak':
        out.push(S.nodes.hardBreak.create({ soft: true }, null, set()));
        break;
      case 'hardbreak':
        out.push(S.nodes.hardBreak.create({ soft: false }, null, set()));
        break;
      case 'code_inline':
        text(c.content, S.marks.code.create({ markup: c.markup }));
        break;
      case 'strong_open':
        active.push(S.marks.bold.create({ markup: c.markup }));
        break;
      case 'em_open':
        active.push(S.marks.italic.create({ markup: c.markup }));
        break;
      case 's_open':
        active.push(S.marks.strike.create());
        break;
      case 'link_open':
        active.push(S.marks.link.create({ href: c.attrGet('href'), title: c.attrGet('title') || null, auto: c.markup === 'linkify' || c.markup === 'autolink' ? c.markup : null }));
        break;
      case 'strong_close':
        pop('bold');
        break;
      case 'em_close':
        pop('italic');
        break;
      case 's_close':
        pop('strike');
        break;
      case 'link_close':
        pop('link');
        break;
      case 'yap_task':
        break;
      default:
        text(c.content);
    }
  }
  return out;
}

// ---- out -------------------------------------------------------------------------------

/** Is `node` as it was read (`kept`)? Its own lines then say it exactly. */
function isKept(kept, node) {
  const md = node.attrs.md;
  return md != null && (kept.get(md) ?? []).some((k) => k === node || k.eq(node));
}

/**
 * The editor's document as markdown: `{text, blocks}` — `text` is `head`,
 * every block (as it was read where it's the same, else written afresh),
 * then `tail`; `blocks[i]` is the `[start, end)` lines of the document's
 * i-th block in the whole text (`null` for one that writes nothing: an
 * empty paragraph). Pure.
 */
export function writeMarkdown(schema, doc, { kept = new Map(), head = '', tail = [''] } = {}) {
  const ctx = { schema, kept };
  const out = [];
  const blocks = [];
  const headLines = head ? head.split('\n').length - 1 : 0;
  let prev = null;
  doc.forEach((node, _offset, i) => {
    if (node.type.name === 'paragraph' && node.content.size === 0) {
      blocks[i] = null;
      return;
    }
    // (A block split off another carries its lines' marks: a new block.)
    const copy = prev && node.attrs.md != null && prev.attrs.md === node.attrs.md;
    prev = node;
    const before = (!copy && node.attrs.before) || (out.length ? [''] : []);
    out.push(...before);
    const lines = isKept(kept, node) ? node.attrs.md.split('\n') : checked(ctx, node);
    blocks[i] = [headLines + out.length, headLines + out.length + lines.length];
    out.push(...lines);
  });
  out.push(...tail);
  return { text: head + out.join('\n'), blocks };
}

/**
 * A block written afresh, read back to check it says the same: its words
 * escaped only where they must be (else everywhere markdown could read
 * them as markup).
 */
function checked(ctx, node) {
  let lines = blockLines(ctx, node, false);
  if (sameBlock(readBack(ctx, lines), node)) return lines;
  lines = blockLines(ctx, node, true);
  if (sameBlock(readBack(ctx, lines), node)) return lines;
  // Marks markdown can't say as they overlap ("*a **b* c**"): a kind at a
  // time left out of this block until it reads back as written — the words
  // always kept, never a stray "*" in them.
  let plain = node;
  for (const name of ['strike', 'italic', 'bold', 'code', 'link']) {
    const type = ctx.schema.marks[name];
    if (!type) continue;
    plain = withoutMark(plain, type);
    const tried = blockLines(ctx, plain, true);
    if (sameBlock(readBack(ctx, tried), plain)) return tried;
  }
  return lines;
}

/** `node` with mark `type` taken off everything in it. */
function withoutMark(node, type) {
  if (node.isLeaf) return node.marks.some((m) => m.type === type) ? node.mark(type.removeFromSet(node.marks)) : node;
  const kids = [];
  node.forEach((child) => kids.push(withoutMark(child, type)));
  return node.copy(node.content.constructor.fromArray(kids));
}

/** The first block of `lines` read as the editor reads it. */
function readBack(ctx, lines) {
  try {
    return readMarkdown(ctx.schema, lines.join('\n')).doc.firstChild;
  } catch {
    return null;
  }
}

/** Two blocks that say the same: the same kinds, words, marks and links (how they were written aside). */
function sameBlock(a, b) {
  if (!a || !b || a.type !== b.type) return false;
  const n = a.type.name;
  if (a.isTextblock && n !== 'codeBlock') return (n !== 'heading' || a.attrs.level === b.attrs.level) && words(a) === words(b);
  if (a.childCount !== b.childCount) return false;
  if (n === 'listItem' && a.attrs.checked !== b.attrs.checked) return false;
  if (n === 'yapSection' && (a.attrs.key !== b.attrs.key || a.attrs.intent !== b.attrs.intent)) return false;
  if (n === 'codeBlock') return a.textContent === b.textContent;
  for (let k = 0; k < a.childCount; k++) if (!sameBlock(a.child(k), b.child(k))) return false;
  return true;
}

/**
 * What a textblock says: its words with their marks (links with their
 * addresses), line breaks, and none of the spaces markdown drops (at its
 * ends, around a line break).
 */
function words(node) {
  const key = (marks) => marks.map((m) => (m.type.name === 'link' ? `link:${m.attrs.href}:${m.attrs.title ?? ''}` : m.type.name)).sort().join(',');
  // Each character that isn't a space with its marks (written when they
  // change); spaces as one, in no mark, none at a line's ends.
  let out = '';
  let at = null;
  let space = false;
  node.forEach((child) => {
    if (child.type.name === 'hardBreak') {
      out += '\u0001';
      space = false;
      return;
    }
    const k = key(child.marks);
    for (const ch of child.text ?? '') {
      if (/\s/.test(ch)) {
        space = true;
        continue;
      }
      if (space && out && !out.endsWith('\u0001')) out += ' ';
      space = false;
      if (k !== at) out += `\u0002${k}\u0003`;
      at = k;
      out += ch;
    }
  });
  return out;
}

/** A block's lines, written afresh (its kept parts as they were); `esc`: escape every markup character. */
function blockLines(ctx, node, esc) {
  switch (node.type.name) {
    case 'paragraph':
      return inlineMd(node, esc).split('\n');
    case 'heading': {
      const words = inlineMd(node, esc).replace(/\n/g, ' ');
      return [`${'#'.repeat(node.attrs.level)}${words ? ` ${words}` : ''}`];
    }
    case 'blockquote':
      return childLines(ctx, node, esc, () => true).map((l) => (l ? `> ${l}` : '>'));
    case 'bulletList':
    case 'orderedList':
      return listLines(ctx, node, esc);
    case 'codeBlock':
      return codeLines(node);
    case 'horizontalRule':
      return [node.attrs.markup || '---'];
    case 'table':
      return tableLines(ctx, node, esc);
    case 'yapSection': {
      const { key, intent } = node.attrs;
      return [intent ? `<!-- yap:section ${key} | ${intent} -->` : `<!-- yap:section ${key} -->`];
    }
    case 'rawBlock':
      return String(node.attrs.md ?? '').split('\n');
    default:
      return [node.textContent];
  }
}

/** A container's children, a blank line between them (or none, where `gap` says so). */
function childLines(ctx, node, esc, gap) {
  const out = [];
  let prev = null;
  node.forEach((child) => {
    if (prev && gap(prev, child)) out.push('');
    out.push(...blockLines(ctx, child, esc));
    prev = child;
  });
  return out;
}

function listLines(ctx, node, esc) {
  const ordered = node.type.name === 'orderedList';
  const start = node.attrs.start ?? 1;
  const out = [];
  node.forEach((item, _o, i) => {
    if (i > 0) out.push(...(item.attrs.before ?? (node.attrs.loose ? [''] : [])));
    if (isKept(ctx.kept, item)) {
      const lines = item.attrs.md.split('\n');
      // A numbered list numbered in order stays so.
      if (ordered && node.attrs.seq) lines[0] = lines[0].replace(/^\d+/, String(start + i));
      out.push(...lines);
      return;
    }
    const marker = ordered ? `${start + i}${node.attrs.delim || '.'}` : item.attrs.marker || node.attrs.marker || '-';
    const box = item.attrs.checked == null ? '' : item.attrs.checked ? '[x] ' : '[ ] ';
    // Its parts: a nested list right under its line, anything else after a blank line.
    const body = childLines(ctx, item, esc, (_a, b) => b.type.name !== 'bulletList' && b.type.name !== 'orderedList');
    const pad = ' '.repeat(marker.length + 1);
    out.push(`${marker} ${box}${body[0] ?? ''}`.trimEnd(), ...body.slice(1).map((l) => (l ? pad + l : '')));
  });
  return out;
}

function codeLines(node) {
  const code = node.textContent;
  const longest = Math.max(0, ...[...code.matchAll(/^\s*(`{3,}|~{3,})/gm)].map((m) => m[1].length));
  let fence = node.attrs.fence || '```';
  if (longest >= fence.length) fence = fence[0].repeat(longest + 1);
  const info = node.attrs.fence ? (node.attrs.info ?? node.attrs.language ?? '') : (node.attrs.language ?? '');
  return [`${fence}${info}`, ...(code ? code.split('\n') : []), fence];
}

function tableLines(ctx, node, esc) {
  const out = [];
  const cols = node.firstChild?.childCount ?? 0;
  const aligns = [];
  node.firstChild?.forEach((cell) => aligns.push(cell.attrs.align ?? null));
  const row = (r) => (isKept(ctx.kept, r) ? r.attrs.md : `| ${cellsOf(r, esc).join(' | ')} |`);
  node.forEach((r, _o, i) => {
    out.push(row(r));
    if (i === 0) {
      const old = node.attrs.rule;
      const fits = old && old.split('|').filter((c) => c.trim()).length === cols && ruleAligns(old).join() === aligns.join();
      out.push(fits ? old : `|${aligns.map((a) => (a === 'center' ? ':---:' : a === 'right' ? '---:' : a === 'left' ? ':---' : '---')).join('|')}|`);
    }
  });
  return out;
}

/** A rule line's alignments ("|:--|--:|" → ['left', 'right']). */
function ruleAligns(rule) {
  return rule
    .split('|')
    .map((c) => c.trim())
    .filter(Boolean)
    .map((c) => (c.startsWith(':') && c.endsWith(':') ? 'center' : c.endsWith(':') ? 'right' : c.startsWith(':') ? 'left' : null));
}

/** A row's cells as markdown: one line each, pipes escaped. */
function cellsOf(row, esc) {
  const out = [];
  row.forEach((cell) => out.push(inlineMd(cell.firstChild ?? cell, esc).replace(/\n/g, ' ').replace(/(?<!\\)\|/g, '\\|')));
  return out;
}

// ---- inline ------------------------------------------------------------------------------

/** The order marks open in (outermost first). */
const ORDER = { link: 0, bold: 1, italic: 2, strike: 3, code: 4 };
const byOrder = (a, b) => (ORDER[a.type.name] ?? 9) - (ORDER[b.type.name] ?? 9);

/**
 * Markdown's markup characters, escaped (`esc`: inline markup anywhere,
 * and what starts a block — "-", "+", "=", "1." — at a piece's start, which
 * may be a line's; a backslash before punctuation is harmless mid-line),
 * else only a backslash that would escape what follows.
 */
function escapeText(t, esc) {
  if (!esc) return t.replace(/\\(?=[!-/:-@[-`{-~])/g, '\\\\');
  return t.replace(/[\\`*_[\]~<>&|#]/g, '\\$&').replace(/^(\s*)([-+=])/, '$1\\$2').replace(/^(\s*\d+)([.)])/, '$1\\$2');
}

/**
 * A textblock's words as markdown: marks opened and closed around them
 * (outermost first; spaces kept outside a mark, where markdown wants
 * them), a soft break as a line break, links as written.
 */
function inlineMd(node, esc) {
  // Its pieces, a mark's edge never on a space: "**word** " not "**word **".
  const pieces = [];
  node.forEach((child) => pieces.push({ node: child, text: child.isText ? child.text : null, marks: [...child.marks].sort(byOrder) }));
  const split = [];
  pieces.forEach((p, k) => {
    if (p.text == null || p.marks.some((m) => m.type.name === 'code')) {
      split.push(p);
      return;
    }
    const prev = pieces[k - 1]?.marks ?? [];
    const next = pieces[k + 1]?.marks ?? [];
    const shared = (other) => p.marks.filter((m) => other.some((o) => o.eq(m)));
    const lead = /^\s*/.exec(p.text)[0];
    const trail = p.text.length > lead.length ? /\s*$/.exec(p.text)[0] : '';
    if (lead) split.push({ text: lead, marks: shared(prev) });
    const core = p.text.slice(lead.length, p.text.length - trail.length);
    if (core) split.push({ text: core, marks: p.marks });
    if (trail) split.push({ text: trail, marks: shared(next) });
  });
  flank(split);
  // `_` / `__` can't open or close inside a word: there, `*` / `**`.
  const delim = (m, k, fallback) => {
    const want = m.attrs.markup || fallback;
    if (want[0] !== '_') return want;
    let after = '';
    for (let i = k; i < split.length; i++) {
      if (!split[i].marks.some((o) => o.eq(m))) {
        after = split[i].text?.[0] ?? '';
        break;
      }
    }
    return WORD.test(out.at(-1) ?? '') || WORD.test(after) ? '*'.repeat(want.length) : want;
  };
  // Each mark opened: what opened it, and so what closes it.
  const opening = (m, k) => {
    switch (m.type.name) {
      case 'bold': {
        const d = delim(m, k, '**');
        return { mark: m, open: d, close: d };
      }
      case 'italic': {
        const d = delim(m, k, '*');
        return { mark: m, open: d, close: d };
      }
      case 'strike':
        return { mark: m, open: '~~', close: '~~' };
      case 'code': {
        const { ticks, pad } = codeTicks(split, k);
        return { mark: m, open: pad ? `${ticks} ` : ticks, close: pad ? ` ${ticks}` : ticks };
      }
      case 'link': {
        const form = linkForm(split, k, m);
        if (form === 'bare') return { mark: m, open: '', close: '' };
        if (form === 'angle') return { mark: m, open: '<', close: '>' };
        const title = m.attrs.title ? ` "${m.attrs.title.replace(/"/g, '\\"')}"` : '';
        return { mark: m, open: '[', close: `](${linkTarget(m.attrs.href)}${title})` };
      }
      default:
        return { mark: m, open: '', close: '' };
    }
  };
  let out = '';
  let open = [];
  split.forEach((p, k) => {
    // Marks already open stay outermost (in the order they opened), so a
    // mark added inside another nests in it: "*em **strong** em*".
    // (Code is always innermost: nothing is markup inside it.)
    const isCode = (m) => m.type.name === 'code';
    const kept = open.map((e) => e.mark).filter((m) => !isCode(m) && p.marks.some((o) => o.eq(m)));
    const marks = [...kept, ...p.marks.filter((m) => !isCode(m) && !kept.some((o) => o.eq(m))), ...p.marks.filter(isCode)];
    let same = 0;
    while (same < open.length && same < marks.length && open[same].mark.eq(marks[same])) same++;
    for (let o = open.length - 1; o >= same; o--) out += open[o].close;
    open = open.slice(0, same);
    for (let o = same; o < marks.length; o++) {
      const entry = opening(marks[o], k);
      out += entry.open;
      open.push(entry);
    }
    // An address written bare (or in <…>) as it is; a link's words never
    // linked again inside it ("https\://…", "\@").
    const link = open.find((e) => e.mark.type.name === 'link');
    if (p.text != null) {
      if (marks.some((m) => m.type.name === 'code') || (link && link.open !== '[')) out += p.text;
      else if (link && esc) out += escapeText(p.text, esc).replace(/:\/\//g, '\\://').replace(/@/g, '\\@');
      else out += escapeText(p.text, esc);
    } else if (p.node?.type.name === 'hardBreak') out = `${out.replace(/[ \t]+$/, '')}${p.node.attrs.soft === false ? '  \n' : '\n'}`;
  });
  for (let o = open.length - 1; o >= 0; o--) out += open[o].close;
  // No spaces at a line's start (four would make code) or at the end.
  return out.replace(/^[ \t]+/gm, '').replace(/[ \t]+$/, '');
}

const WORD = /[\p{L}\p{N}]/u;
const PUNCT = /[\p{P}\p{S}]/u;
const DELIMITED = new Set(['bold', 'italic', 'strike']);

/**
 * Where markdown can't open or close bold, italic or strike — a mark that
 * starts on punctuation right after a letter ("Z**+2**"), or ends on it
 * right before one — the mark takes in that word's letters too: the words
 * stay as they are, never a stray "**". (Markdown has no way to say it.)
 */
function flank(split) {
  const text = (p) => (p?.text != null && !p.marks.some((m) => m.type.name === 'code') ? p.text : null);
  const opens = (a, b) => b.marks.filter((m) => DELIMITED.has(m.type.name) && !a.marks.some((o) => o.eq(m)));
  for (let k = 1; k < split.length; k++) {
    const a = split[k - 1];
    const b = split[k];
    const ta = text(a);
    const tb = text(b);
    if (ta == null || tb == null) continue;
    // Opening at b's start, on punctuation, after a letter: b's marks take a's last word.
    if (opens(a, b).length && PUNCT.test(tb[0] ?? '') && WORD.test(ta.at(-1) ?? '')) {
      const word = /[\p{L}\p{N}]+$/u.exec(ta)[0];
      a.text = ta.slice(0, -word.length);
      b.text = word + tb;
    }
    // Closing at a's end, on punctuation, before a letter: a's marks take b's first word.
    if (opens(b, a).length && PUNCT.test(ta.at(-1) ?? '') && WORD.test(tb[0] ?? '')) {
      const word = /^[\p{L}\p{N}]+/u.exec(tb)[0];
      a.text = ta + word;
      b.text = tb.slice(word.length);
    }
  }
  for (let k = split.length - 1; k >= 0; k--) if (split[k].text === '') split.splice(k, 1);
}

/**
 * The backticks around a code span — as written, or more than any run
 * inside it — and whether it needs a space inside them (it starts or ends
 * with a backtick, or with spaces at both ends that markdown would take).
 */
function codeTicks(split, k) {
  let text = '';
  for (let i = k; i < split.length && split[i].marks.some((m) => m.type.name === 'code'); i++) text += split[i].text ?? '';
  const mark = split[k].marks.find((m) => m.type.name === 'code');
  const longest = Math.max(0, ...(text.match(/`+/g) ?? []).map((r) => r.length));
  const want = mark?.attrs.markup || '`';
  const ticks = want.length > longest ? want : '`'.repeat(longest + 1);
  return { ticks, pad: /^`|`$/.test(text) || (/^ [^]* $/.test(text) && !!text.trim()) };
}

/**
 * How a link is written: `bare` — its address alone, as it was, its words
 * still its address and nothing after it that would run on into it —,
 * `angle` (`<https://…>`, when something would), or `inline` (`[words](…)`).
 */
function linkForm(split, k, mark) {
  if (!mark.attrs.auto) return 'inline';
  let text = '';
  let i = k;
  for (; i < split.length && split[i].marks.some((m) => m.eq(mark)); i++) text += split[i].text ?? '';
  const after = split.slice(i).map((p) => p.text ?? ' ').join('').slice(0, 8);
  const href = mark.attrs.href || '';
  const whole = text === href || `mailto:${text}` === href;
  if (!whole && `http://${text}` !== href) return 'inline';
  // (A trailing full stop or comma ends a bare address; a letter or "~" carries it on.)
  if (after === '' || /^\s/.test(after) || /^[.,;:!?]+(\s|$)/.test(after)) return 'bare';
  return whole ? 'angle' : 'inline';
}

/** A link's address in markdown: spaces and brackets that would end it, encoded. */
function linkTarget(href) {
  return String(href ?? '').replace(/[ ()<>]/g, (c) => encodeURIComponent(c));
}
