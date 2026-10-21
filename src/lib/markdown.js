// Minimal, safe markdown → HTML renderer for the Enhanced notes tab, chat
// answers, the notepad's Ask sheet and update notes.
//
// Deliberately tiny: the note-enhancement base prompt (llm::NOTE_BASE_PROMPT)
// forbids tables / horizontal rules / block quotes, so the output space is
// headings, paragraphs, bold/italic/code, bullet + numbered lists, and task
// checkboxes; chat answers add fenced code blocks and links (web and mail
// only, opened in the browser by main.js). All input is HTML-escaped FIRST, so
// model output can never inject markup (no raw-HTML passthrough by design).
// If notes ever need a full editor, this is the piece Milkdown/CodeMirror
// replaces (see ROADMAP "AI Notepad").

function escapeHtml(s) {
  return s
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;');
}

// A link Yap may open (in the default browser: main.js routes clicks on
// `a.md-link` to the opener plugin). Only web and mail links; anything else
// (javascript:, file:, tauri:) stays plain text.
const SAFE_URL = /^(https?:\/\/|mailto:)/i;

// An answer written from the web (chats.rs `sources`, web.rs): its citation
// markers ("[1]", "[2, 3]") become small pills that open their source, and
// only links its sources gave stay clickable — any other address the model
// wrote shows as plain text (no phishing link, nothing smuggled out in one).
function webContext(sources) {
  if (!Array.isArray(sources) || sources.length === 0) return null;
  const strip = (u) => String(u || '').replace(/#.*$/, '').replace(/\/+$/, '');
  const urls = new Set(sources.map((s) => escapeHtml(strip(s.url))));
  return {
    byN: new Map(sources.map((s) => [String(s.n), s])),
    allows: (escapedUrl) => urls.has(strip(escapedUrl)),
  };
}

function citePill(web, n) {
  const s = web.byN.get(String(n).trim());
  if (!s || !SAFE_URL.test(s.url)) return null;
  const label = `${s.title || s.site}${s.site ? ` · ${s.site}` : ''}`;
  return `<a class="md-link md-cite" href="${escapeHtml(s.url)}" title="${escapeHtml(label)}" aria-label="Source ${s.n}: ${escapeHtml(label)}">${s.n}</a>`;
}

// Inline transforms on an already-escaped line: code (kept apart, so nothing
// inside it is touched), links, citations (`web`), bold, italic.
function inlineWith(s, web = null) {
  const codes = [];
  let t = s.replace(/`([^`]+)`/g, (_, c) => `\u0000${codes.push(c) - 1}\u0000`);
  const links = [];
  const keep = (html) => `\u0001${links.push(html) - 1}\u0001`;
  // [text](url): the url is already HTML-escaped (a `"` is `&quot;`), so it
  // can't break out of the attribute; a ")" ends it, and the only entity a
  // url may hold is `&amp;` (an escaped quote or bracket ends it too).
  t = t.replace(/\[([^\]\n]+)\]\(((?:&amp;|[^\s&)])+)\)/g, (m, text, url) => {
    if (!SAFE_URL.test(url)) return m;
    if (web && !web.allows(url)) return text;
    return keep(`<a class="md-link" href="${url}" title="${url}">${text}</a>`);
  });
  // <https://…> (markdown's autolink, escaped by now): the link without its
  // angle brackets.
  t = t.replace(/&lt;(https?:\/\/(?:&amp;|[^\s&])+)&gt;/gi, (m, url) =>
    web && !web.allows(url) ? keep(url) : keep(`<a class="md-link" href="${url}" title="${url}">${url}</a>`)
  );
  // Bare web addresses; trailing punctuation ("…/app.") isn't part of them.
  t = t.replace(/\bhttps?:\/\/(?:&amp;|[^\s&])+/gi, (found) => {
    const tail = /[.,;:!?)]+$/.exec(found)?.[0] ?? '';
    const url = found.slice(0, found.length - tail.length);
    if (web && !web.allows(url)) return keep(url) + tail;
    return keep(`<a class="md-link" href="${url}" title="${url}">${url}</a>`) + tail;
  });
  // Citations: "[1]", "[2, 3]" (not a link's text: that's gone by now).
  if (web) {
    t = t.replace(/\[(\d{1,2}(?:\s*,\s*\d{1,2})*)\]/g, (m, nums) => {
      const pills = nums.split(',').map((n) => citePill(web, n));
      return pills.every(Boolean) ? keep(`<span class="md-cites">${pills.join('')}</span>`) : m;
    });
  }
  t = t
    .replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>')
    .replace(/(^|[\s(])\*([^*\s][^*]*)\*(?=$|[\s).,;:!?])/g, '$1<em>$2</em>');
  return t
    .replace(/\u0001(\d+)\u0001/g, (_, i) => links[+i])
    .replace(/\u0000(\d+)\u0000/g, (_, i) => `<code>${codes[+i]}</code>`);
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

// A line's indent, a tab counting as four spaces.
// GitHub's tables: a header row, a rule line (|---|:---:|---:|), then rows.
// Models (Groq's gpt-oss especially) write them; until 2026-10-21 Chat
// showed the raw pipes.
const TABLE_RULE = /^\|?\s*:?-{2,}:?\s*(\|\s*:?-{2,}:?\s*)*\|?$/;
/** A table line's cells: outer pipes dropped, split on unescaped pipes. */
function tableCells(line) {
  const inner = line.trim().replace(/^\|/, '').replace(/(?<!\\)\|$/, '');
  return inner.split(/(?<!\\)\|/).map((c) => c.trim().replaceAll('\\|', '|'));
}
function tableAlign(rule) {
  if (/^:-+:$/.test(rule)) return ' class="c"';
  if (/^-+:$/.test(rule)) return ' class="r"';
  return '';
}

const indentOf = (line) => {
  let n = 0;
  for (const c of line) {
    if (c === ' ') n += 1;
    else if (c === '\t') n += 4;
    else break;
  }
  return n;
};

// `options.sources`: an answer's web sources ([{ n, title, url, site }]):
// citations become pills, and only the sources' links stay links.
//
// Lists: a blank line between two items of one list doesn't end it (models
// often write "1. …\n\n2. …", which showed "1." on every item); a line
// indented under an item belongs to it; an item indented under another
// starts a list inside it; and an ordered list starting at 3 says so
// (`start="3"`), so its numbers are the ones written.
export function renderMarkdown(md, options = {}) {
  const web = webContext(options.sources);
  const inline = (s) => inlineWith(s, web);
  const lines = escapeHtml(md || '').split(/\r?\n/);
  const out = [];
  // The open lists, outermost first: { kind: 'ul' | 'ol', indent }. The
  // innermost one's last <li> is still open (its </li> comes later), so a
  // continuation line or a nested list can go inside it.
  const lists = [];
  let blank = false; // a blank line since the last item
  // Onto the last line out (an open item's end, a line under it).
  const append = (s) => {
    out[out.length - 1] += s;
  };

  const closeTop = () => {
    const top = lists.pop();
    append(`</li></${top.kind}>`);
  };
  const closeLists = () => {
    while (lists.length) closeTop();
  };
  const item = (kind, indent, html, cls = '', start = 1) => {
    // Leave the lists this item isn't in.
    while (lists.length && indent < lists.at(-1).indent - 1) closeTop();
    const top = lists.at(-1);
    if (top && indent <= top.indent + 1) {
      if (top.kind !== kind) {
        closeTop(); // another kind of list at the same level
        return item(kind, indent, html, cls, start);
      }
      append('</li>'); // the next item of the same list
    } else {
      // A new list: at the top, or inside the open item (indented under it).
      out.push(kind === 'ol' && start !== 1 ? `<ol start="${start}">` : `<${kind}>`);
      lists.push({ kind, indent });
    }
    out.push(`<li${cls}>${html}`);
    blank = false;
  };

  let fence = null; // the lines of an open ``` block, or null

  for (let at = 0; at < lines.length; at++) {
    const raw = lines[at];
    const line = raw.trimEnd();
    const trimmed = line.trim();

    // A fenced code block: kept verbatim (already escaped), monospace.
    if (fence) {
      if (/^```/.test(trimmed)) {
        out.push(`<pre class="md-code"><code>${fence.join('\n')}</code></pre>`);
        fence = null;
      } else {
        fence.push(raw);
      }
      continue;
    }
    if (/^```/.test(trimmed)) {
      closeLists();
      fence = [];
      continue;
    }

    // A blank line: a list may go on after it (the next item decides).
    if (!trimmed) {
      blank = true;
      continue;
    }

    const indent = indentOf(line);

    // A table: this line's cells over a rule line with as many.
    if (trimmed.includes('|') && at + 1 < lines.length && TABLE_RULE.test(lines[at + 1].trim())) {
      const head = tableCells(trimmed);
      const rule = tableCells(lines[at + 1].trim());
      if (head.length >= 2 && rule.length === head.length) {
        closeLists();
        blank = false;
        const aligns = rule.map(tableAlign);
        const rows = [];
        at += 2;
        while (at < lines.length && lines[at].trim().includes('|')) {
          rows.push(tableCells(lines[at]));
          at++;
        }
        at--; // the loop's own step
        const cell = (tag, i, text) => `<${tag}${aligns[i]}>${inline(text ?? '')}</${tag}>`;
        out.push(
          '<div class="md-table"><table><thead><tr>' +
            head.map((h, i) => cell('th', i, h)).join('') +
            '</tr></thead><tbody>' +
            rows.map((r) => `<tr>${head.map((_, i) => cell('td', i, r[i])).join('')}</tr>`).join('') +
            '</tbody></table></div>'
        );
        continue;
      }
    }

    const heading = /^(#{1,4})\s+(.*)$/.exec(trimmed);
    if (heading) {
      closeLists();
      const level = Math.min(heading[1].length + 1, 5); // # → h2 … #### → h5
      out.push(`<h${level}>${inline(heading[2])}</h${level}>`);
      continue;
    }

    const task = /^[-*]\s+\[([ xX])\]\s+(.*)$/.exec(trimmed);
    if (task) {
      const checked = task[1].toLowerCase() === 'x' ? ' checked' : '';
      item('ul', indent, `<input type="checkbox" disabled${checked} /> ${inline(task[2])}`, ' class="task"');
      continue;
    }

    const bullet = /^[-*•]\s+(.*)$/.exec(trimmed);
    if (bullet) {
      item('ul', indent, inline(bullet[1]));
      continue;
    }

    const numbered = /^(\d{1,9})[.)]\s+(.*)$/.exec(trimmed);
    if (numbered) {
      item('ol', indent, inline(numbered[2]), '', Number(numbered[1]));
      continue;
    }

    // A line indented under an item is part of it ("1. **Game**\n   A
    // turn-based RPG…"), after a blank line too.
    if (lists.length && indent >= 2) {
      append(`${blank ? '<br><br>' : '<br>'}${inline(trimmed)}`);
      blank = false;
      continue;
    }

    closeLists();
    blank = false;
    out.push(`<p>${inline(trimmed)}</p>`);
  }
  // An answer cut off mid-block still shows its code.
  if (fence) out.push(`<pre class="md-code"><code>${fence.join('\n')}</code></pre>`);
  closeLists();
  return out.join('\n');
}
