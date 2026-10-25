// Safe markdown → HTML for the Enhanced notes tab, chat answers, the
// notepad's Ask sheet and summary, the Voice Assistant's card and update
// notes.
//
// Parsing is markdown-it (CommonMark + GitHub's tables and strikethrough,
// bare links via linkify-it); the HTML it makes then goes through DOMPurify
// with a tight allowlist. Until 2026-10-21 this was a hand-written parser,
// and each new way a model wrote markdown broke it in turn (blank lines
// between list items, tables, <https://…> links).
//
// Safety, twice over: markdown-it never passes HTML through (`html: false`:
// a model's `<img onerror=…>` or `<script>` shows as text), only web and
// mail links become links (`validateLink`), images aren't rendered (no
// remote fetches), and DOMPurify then keeps only the tags, attributes and
// classes this file makes — no `style`, no event handlers, no other URLs.
//
// Yap's own rules on top (plugins below):
// - links are `a.md-link` with the address as their `title`, opened in the
//   browser by main.js's click handler (`open_external`);
// - a web answer (`options.sources`): its "[1]" / "[2, 3]" citations become
//   pills that open their source (not a link's own text; unknown numbers
//   stay text), and only the sources' own links stay links;
// - headings shift down a level (# → h2 … #### and below → h5): the notes'
//   and notepad's CSS styles them that way;
// - `- [ ]` / `- [x]` tasks are `li.task` with a disabled checkbox;
// - tables are wrapped in `div.md-table` (scrolls sideways when wide), with
//   the rule line's alignment as classes `c` / `r`, and end at a line
//   without a pipe;
// - fenced and indented code is `pre.md-code > code`; a fence an answer cut
//   off still shows;
// - a list item holding one paragraph renders tight even when the items
//   have blank lines between them (models write "1. …\n\n2. …"); one with
//   several is `li.md-loose`; a change of bullet mark doesn't end a list;
// - single line breaks are kept (`breaks: true`), and "• " lines are bullets.
import MarkdownIt from 'markdown-it';
import DOMPurify from 'dompurify';

// A link Yap may open (in the default browser: main.js routes clicks on
// `a.md-link` to the opener plugin). Only web and mail links; anything else
// (javascript:, file:, tauri:, relative) stays plain text.
const SAFE_URL = /^(https?:\/\/|mailto:)/i;

const md = new MarkdownIt('default', {
  html: false,
  linkify: true,
  breaks: true,
  typographer: false,
});
md.validateLink = (url) => SAFE_URL.test(url.trim());
// No images: a model's ![x](https://…) would make Yap fetch the address.
md.disable('image');
// Bare links: http(s) and mail addresses only (linkify-it's defaults also
// take ftp: and //; "example.com" alone isn't linked).
md.linkify.add('ftp:', null).add('//', null);

const escapeHtml = md.utils.escapeHtml;

// --- Parsing --------------------------------------------------------------

// A table ends at a line without a pipe (GitHub's spec would make "Pick
// one." under a table one more row). Runs only as the table's terminator.
md.block.ruler.before(
  'table',
  'yap_table_end',
  (state, line, _end, silent) => {
    if (!silent || state.parentType !== 'table') return false;
    return !state.src.slice(state.bMarks[line] + state.tShift[line], state.eMarks[line]).includes('|');
  },
  { alt: ['blockquote'] }
);

// Block markup: headings a level down, classes for quotes and rules.
md.core.ruler.push('yap_blocks', (state) => {
  for (const t of state.tokens) {
    if (t.type === 'heading_open' || t.type === 'heading_close') {
      t.tag = `h${Math.min(Number(t.tag.slice(1)) + 1, 5)}`;
    } else if (t.type === 'blockquote_open') {
      t.attrSet('class', 'md-quote');
    } else if (t.type === 'hr') {
      t.attrSet('class', 'md-rule');
    }
  }
});

// A change of bullet mark ("- a" then "* b", or "1." then "2)") goes on
// with the same list: CommonMark starts a new one, which models never mean.
md.core.ruler.push('yap_merge_lists', (state) => {
  const tokens = state.tokens;
  for (let i = tokens.length - 2; i >= 0; i--) {
    const [a, b] = [tokens[i].type, tokens[i + 1].type];
    if ((a === 'bullet_list_close' && b === 'bullet_list_open') || (a === 'ordered_list_close' && b === 'ordered_list_open')) {
      tokens.splice(i, 2);
    }
  }
});

// A list item with one paragraph renders tight (no <p>), as in a tight
// list: models put blank lines between items, which makes a "loose" list
// whose every item would otherwise be a spaced-out paragraph. An item with
// several paragraphs keeps them.
md.core.ruler.push('yap_tight_items', (state) => {
  const tokens = state.tokens;
  for (let i = 0; i < tokens.length; i++) {
    if (tokens[i].type !== 'list_item_open') continue;
    const level = tokens[i].level;
    const paragraphs = [];
    for (let j = i + 1; j < tokens.length; j++) {
      const t = tokens[j];
      if (t.type === 'list_item_close' && t.level === level) break;
      if (t.type === 'paragraph_open' && t.level === level + 1) paragraphs.push(j);
    }
    if (paragraphs.length === 1) {
      tokens[paragraphs[0]].hidden = true;
      tokens[paragraphs[0] + 2].hidden = true;
    } else if (paragraphs.length > 1) {
      tokens[i].attrJoin('class', 'md-loose'); // app.css trims its first and last <p>
    }
  }
});

// Tasks: a bullet item starting "[ ] " or "[x] " gets a checkbox (disabled
// unless the view ticks tasks: `options.tickable`), named by its words.
// `env.tasks` collects each task's source line, in the order they render.
md.core.ruler.push('yap_tasks', (state) => {
  const tokens = state.tokens;
  const lists = [];
  for (let i = 0; i < tokens.length; i++) {
    const t = tokens[i];
    if (t.type === 'bullet_list_open' || t.type === 'ordered_list_open') lists.push(t.type);
    else if (t.type === 'bullet_list_close' || t.type === 'ordered_list_close') lists.pop();
    else if (t.type === 'list_item_open' && lists.at(-1) === 'bullet_list_open') {
      const inline = tokens[i + 1]?.type === 'paragraph_open' ? tokens[i + 2] : null;
      const first = inline?.children?.[0];
      const m = first?.type === 'text' ? /^\[([ xX])\][ \t]+/.exec(first.content) : null;
      if (!m) continue;
      first.content = first.content.slice(m[0].length);
      const box = new state.Token('yap_task', '', 0);
      box.meta = { checked: m[1] !== ' ', label: inline.content.replace(/^\[[ xX]\][ \t]+/, '').trim() };
      inline.children.unshift(box);
      t.attrJoin('class', 'task');
      state.env?.tasks?.push(t.map?.[0] ?? -1);
    }
  }
});

// An answer written from the web (chat/chats.rs `sources`, web.rs): only links
// its sources gave stay links — any other address the model wrote shows as
// text (no phishing link, nothing smuggled out in one) — and its citation
// markers ("[1]", "[2, 3]") become pills that open their source. An answer
// asked across meetings (search/ask.rs) cites passages of the person's
// meetings and notes instead: a source with a `note` and no address, whose
// pill (`a.md-passage`, href "#passage-<n>") the view opens at its line.
function webContext(sources) {
  if (!Array.isArray(sources) || sources.length === 0) return null;
  const strip = (u) => String(u || '').replace(/#.*$/, '').replace(/\/+$/, '');
  const urls = new Set(sources.filter((s) => s.url).map((s) => strip(md.normalizeLink(String(s.url || '')))));
  return {
    byN: new Map(sources.map((s) => [String(s.n), s])),
    allows: (href) => urls.has(strip(href)),
  };
}

/** A passage of the person's meetings and notes (not a web page). */
const isPassage = (s) => s && s.note != null && !s.url;

const CITATION = /\[(\d{1,2}(?:\s*,\s*\d{1,2})*)\]/g;

md.core.ruler.push('yap_web', (state) => {
  const web = state.env?.web;
  if (!web) return;
  for (const block of state.tokens) {
    if (block.type !== 'inline' || !block.children) continue;
    const out = [];
    const open = []; // per open link: kept?
    for (const t of block.children) {
      if (t.type === 'link_open') {
        const keep = web.allows(t.attrGet('href'));
        open.push(keep);
        if (keep) out.push(t);
        continue;
      }
      if (t.type === 'link_close') {
        if (open.pop()) out.push(t);
        continue;
      }
      // Citations, outside links only.
      if (t.type !== 'text' || open.some(Boolean)) {
        out.push(t);
        continue;
      }
      let at = 0;
      for (const m of t.content.matchAll(CITATION)) {
        const cited = m[1].split(',').map((n) => web.byN.get(n.trim()));
        if (!cited.every((s) => s && (SAFE_URL.test(String(s.url)) || isPassage(s)))) continue;
        if (m.index > at) out.push(textToken(state, t.content.slice(at, m.index)));
        const pills = new state.Token('yap_cite', '', 0);
        pills.meta = { sources: cited };
        out.push(pills);
        at = m.index + m[0].length;
      }
      if (at === 0) out.push(t);
      else if (at < t.content.length) out.push(textToken(state, t.content.slice(at)));
    }
    block.children = out;
  }
});

function textToken(state, content) {
  const t = new state.Token('text', '', 0);
  t.content = content;
  return t;
}

// --- Rendering ------------------------------------------------------------

const rules = md.renderer.rules;

rules.link_open = (tokens, idx) => {
  const href = escapeHtml(tokens[idx].attrGet('href') || '');
  return `<a class="md-link" href="${href}" title="${href}">`;
};

rules.yap_cite = (tokens, idx) => {
  const pills = tokens[idx].meta.sources.map((s) => {
    const label = escapeHtml(`${s.title || s.site}${s.site ? ` · ${s.site}` : ''}`);
    const n = escapeHtml(String(s.n));
    if (isPassage(s)) {
      // Opened in Yap by the view (its click handler), never as a link.
      return `<a class="md-cite md-passage" href="#passage-${n}" title="${label}" aria-label="Passage ${n}: ${label}">${n}</a>`;
    }
    return `<a class="md-link md-cite" href="${escapeHtml(s.url)}" title="${label}" aria-label="Source ${n}: ${label}">${n}</a>`;
  });
  return `<span class="md-cites">${pills.join('')}</span>`;
};

rules.yap_task = (tokens, idx) => {
  const { checked, label } = tokens[idx].meta;
  return `<input type="checkbox" disabled${checked ? ' checked' : ''} aria-label="${escapeHtml(label || 'Task')}"> `;
};

// Code: monospace, kept verbatim (escaped), no highlighting.
rules.fence = rules.code_block = (tokens, idx) =>
  `<pre class="md-code"><code>${escapeHtml(tokens[idx].content.replace(/\n$/, ''))}</code></pre>\n`;

// Tables: in a sideways-scrolling wrapper, alignment as classes, compact.
const ALIGN = { center: ' class="c"', right: ' class="r"' };
const cell = (tag) => (tokens, idx) => {
  const align = /text-align:(\w+)/.exec(tokens[idx].attrGet('style') || '')?.[1];
  return `<${tag}${ALIGN[align] ?? ''}>`;
};
rules.table_open = () => '<div class="md-table"><table>';
rules.table_close = () => '</table></div>\n';
rules.th_open = cell('th');
rules.td_open = cell('td');
for (const tag of ['thead', 'tbody', 'tr', 'th', 'td']) {
  rules[`${tag}_close`] = () => `</${tag}>`;
  if (!rules[`${tag}_open`]) rules[`${tag}_open`] = () => `<${tag}>`;
}

// --- Sanitising -----------------------------------------------------------

// Exactly what the rules above make, nothing else.
const PURIFY = {
  ALLOWED_TAGS: [
    'p', 'br', 'strong', 'em', 's', 'code', 'pre', 'a', 'span', 'input',
    'ul', 'ol', 'li', 'h2', 'h3', 'h4', 'h5', 'blockquote', 'hr',
    'div', 'table', 'thead', 'tbody', 'tr', 'th', 'td',
  ],
  ALLOWED_ATTR: ['class', 'href', 'title', 'aria-label', 'start', 'type', 'checked', 'disabled'],
  // (DOMPurify's own URI check stays: it tests every attribute's value, so a
  // web-and-mail-only pattern here would drop start="3"; `href` is held to
  // SAFE_URL in the hook below.)
  ALLOW_DATA_ATTR: false,
  ALLOW_ARIA_ATTR: false,
};
const CLASSES = new Set(['md-link', 'md-cite', 'md-cites', 'md-passage', 'md-code', 'md-table', 'md-quote', 'md-rule', 'md-loose', 'task', 'c', 'r']);
// The one in-page address: a passage pill's (rendered by yap_cite only).
const PASSAGE_HREF = /^#passage-\d{1,2}$/;
// Which elements may carry which of the narrower attributes.
const ONLY_ON = { href: ['A'], 'aria-label': ['A', 'INPUT'], start: ['OL'], type: ['INPUT'], checked: ['INPUT'], disabled: ['INPUT'] };

let purifier = null;
// While a tickable render is sanitised (synchronously): its checkboxes stay enabled.
let tickable = false;
function purify() {
  if (purifier) return purifier;
  const p = DOMPurify(globalThis.window);
  // Every Yap window has a DOM; without one (a test outside a page) nothing
  // is rendered unsanitised.
  if (!p.isSupported) throw new Error('renderMarkdown needs a DOM to sanitise its HTML');
  p.addHook('afterSanitizeAttributes', (node) => {
    for (const name of node.getAttributeNames()) {
      if (ONLY_ON[name] && !ONLY_ON[name].includes(node.nodeName)) node.removeAttribute(name);
    }
    if (node.hasAttribute('href')) {
      const href = node.getAttribute('href');
      const passage = PASSAGE_HREF.test(href) && /(^|\s)md-passage(\s|$)/.test(node.getAttribute('class') || '');
      if (!passage && !SAFE_URL.test(href)) node.removeAttribute('href');
    }
    if (node.hasAttribute('class')) {
      const kept = node.getAttribute('class').split(/\s+/).filter((c) => CLASSES.has(c));
      if (kept.length) node.setAttribute('class', kept.join(' '));
      else node.removeAttribute('class');
    }
    // The only input is a task's checkbox, ticked only where the view
    // ticks tasks (a note's summary), never in an answer.
    if (node.nodeName === 'INPUT') {
      node.setAttribute('type', 'checkbox');
      if (tickable) node.removeAttribute('disabled');
      else node.setAttribute('disabled', '');
    }
  });
  purifier = p;
  return p;
}

/**
 * Markdown → safe HTML.
 * `options.sources`: an answer's web sources ([{ n, title, url, site }]):
 * citations become pills, and only the sources' links stay links.
 * `options.tickable`: task checkboxes you can tick (a note's summary; the
 * view maps each one to its line with `taskLines`).
 */
export function renderMarkdown(text, options = {}) {
  const env = { web: webContext(options.sources) };
  const html = md.render(bulletsToDashes(text || ''), env);
  tickable = !!options.tickable;
  try {
    return purify().sanitize(html, PURIFY).trim();
  } finally {
    tickable = false;
  }
}

/**
 * The source line (0-based) of each task checkbox `renderMarkdown(text)`
 * makes, in the order they render: the nth checkbox in the page is the
 * task on `taskLines(text)[n]` (src-tauri/src/notes/tasks.rs ticks it there).
 */
export function taskLines(text) {
  const env = { tasks: [] };
  md.parse(bulletsToDashes(text || ''), env);
  return env.tasks;
}

// "• item" lines (small models write them) as markdown bullets, outside code
// fences.
function bulletsToDashes(src) {
  if (!src.includes('•')) return src;
  let fence = null;
  return src
    .split('\n')
    .map((line) => {
      const f = /^\s*(`{3,}|~{3,})/.exec(line);
      if (f) {
        if (!fence) fence = f[1][0];
        else if (f[1][0] === fence) fence = null;
        return line;
      }
      return fence ? line : line.replace(/^(\s*)•(?=[ \t])/, '$1-');
    })
    .join('\n');
}

// --- Copy for email ---------------------------------------------------------

// Inline styles only (a mail client drops a page's stylesheet), and no
// colours: the email takes its reader's own. Sizes as a mail client's body.
const EMAIL_STYLE = {
  DIV: 'font-family:Segoe UI,Calibri,Arial,sans-serif;font-size:14px;line-height:1.5',
  H2: 'font-size:20px;margin:0 0 12px',
  H3: 'font-size:17px;margin:18px 0 6px',
  H4: 'font-size:15px;margin:14px 0 4px',
  H5: 'font-size:14px;margin:12px 0 4px',
  P: 'margin:0 0 8px',
  UL: 'margin:0 0 8px;padding-left:22px',
  OL: 'margin:0 0 8px;padding-left:22px',
  LI: 'margin:2px 0',
  TABLE: 'border-collapse:collapse;margin:0 0 8px',
  TH: 'border:1px solid;padding:4px 8px;text-align:left',
  TD: 'border:1px solid;padding:4px 8px',
  BLOCKQUOTE: 'margin:0 0 8px;padding-left:10px;border-left:3px solid',
  PRE: 'margin:0 0 8px;font-family:Consolas,monospace;white-space:pre-wrap',
};

/**
 * A summary as an email: rich HTML for the clipboard's `text/html` (Outlook,
 * Gmail and the rest paste headings, lists and tasks as they are), from the
 * same safe render as the views (`renderMarkdown`), then made mail-shaped —
 * the title as its heading, tasks as ☐ / ☑ (a mail client has no
 * checkboxes), inline styles, no classes. `emailText` is its plain-text
 * twin, for the same clipboard's `text/plain`.
 */
export function emailHtml(md, { title = '' } = {}) {
  const doc = globalThis.window.document;
  const box = doc.createElement('div');
  box.innerHTML = renderMarkdown(md);
  // (The renderer puts a space after a task's checkbox already.)
  for (const input of [...box.querySelectorAll('li.task > input')]) {
    input.replaceWith(doc.createTextNode(input.hasAttribute('checked') ? '☑' : '☐'));
  }
  for (const li of box.querySelectorAll('li.task')) li.setAttribute('style', `${EMAIL_STYLE.LI};list-style:none;margin-left:-18px`);
  // A table's scrolling wrapper and a cite's span aren't needed in a mail.
  for (const wrap of [...box.querySelectorAll('div.md-table')]) wrap.replaceWith(...wrap.childNodes);
  for (const el of box.querySelectorAll('*')) {
    el.removeAttribute('class');
    el.removeAttribute('title');
    if (EMAIL_STYLE[el.nodeName] && !el.hasAttribute('style')) el.setAttribute('style', EMAIL_STYLE[el.nodeName]);
  }
  const heading = title.trim() ? `<h2 style="${EMAIL_STYLE.H2}">${escapeHtml(title.trim())}</h2>` : '';
  return `<div style="${EMAIL_STYLE.DIV}">${heading}${box.innerHTML}</div>`;
}

/** `emailHtml`'s plain-text twin: the title, then the summary as `markdownToText` writes it. */
export function emailText(md, { title = '' } = {}) {
  const body = markdownToText(md);
  return title.trim() ? `${title.trim()}\n\n${body}` : body;
}

// The same markdown as plain text, for pasting where markdown shows raw
// (Teams/Slack messages, email): heading marks and emphasis dropped, task
// checkboxes as ☐ / ☑, bullets as •. Indentation and blank lines are kept.
export function markdownToText(md) {
  return (md || '')
    .split(/\r?\n/)
    .map((line) =>
      line
        .replace(/^(\s*)#{1,6}\s+/, '$1')
        .replace(/^(\s*)[-*]\s+\[ \]\s+/, '$1☐ ')
        .replace(/^(\s*)[-*]\s+\[[xX]\]\s+/, '$1☑ ')
        .replace(/^(\s*)[-*]\s+/, '$1• ')
        .replace(/\*\*([^*]+)\*\*/g, '$1')
        .replace(/`([^`]+)`/g, '$1')
    )
    .join('\n')
    .trim();
}
