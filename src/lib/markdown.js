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

// Inline transforms on an already-escaped line: code (kept apart, so nothing
// inside it is touched), links, bold, italic.
function inline(s) {
  const codes = [];
  let t = s.replace(/`([^`]+)`/g, (_, c) => `\u0000${codes.push(c) - 1}\u0000`);
  const links = [];
  const keep = (html) => `\u0001${links.push(html) - 1}\u0001`;
  // [text](url): the url is already HTML-escaped (a `"` is `&quot;`), so it
  // can't break out of the attribute; a ")" ends it, and the only entity a
  // url may hold is `&amp;` (an escaped quote or bracket ends it too).
  t = t.replace(/\[([^\]\n]+)\]\(((?:&amp;|[^\s&)])+)\)/g, (m, text, url) =>
    SAFE_URL.test(url) ? keep(`<a class="md-link" href="${url}" title="${url}">${text}</a>`) : m
  );
  // Bare web addresses; trailing punctuation ("…/app.") isn't part of them.
  t = t.replace(/\bhttps?:\/\/(?:&amp;|[^\s&])+/gi, (found) => {
    const tail = /[.,;:!?)]+$/.exec(found)?.[0] ?? '';
    const url = found.slice(0, found.length - tail.length);
    return keep(`<a class="md-link" href="${url}" title="${url}">${url}</a>`) + tail;
  });
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

export function renderMarkdown(md) {
  const lines = escapeHtml(md || '').split(/\r?\n/);
  const out = [];
  let list = null; // 'ul' | 'ol' | null

  const closeList = () => {
    if (list) {
      out.push(`</${list}>`);
      list = null;
    }
  };
  const openList = (kind) => {
    if (list !== kind) {
      closeList();
      out.push(`<${kind}>`);
      list = kind;
    }
  };

  let fence = null; // the lines of an open ``` block, or null

  for (const raw of lines) {
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
      closeList();
      fence = [];
      continue;
    }

    if (!trimmed) {
      closeList();
      continue;
    }

    const heading = /^(#{1,4})\s+(.*)$/.exec(trimmed);
    if (heading) {
      closeList();
      const level = Math.min(heading[1].length + 1, 5); // # → h2 … #### → h5
      out.push(`<h${level}>${inline(heading[2])}</h${level}>`);
      continue;
    }

    const task = /^[-*]\s+\[([ xX])\]\s+(.*)$/.exec(trimmed);
    if (task) {
      openList('ul');
      const checked = task[1].toLowerCase() === 'x' ? ' checked' : '';
      out.push(
        `<li class="task"><input type="checkbox" disabled${checked} /> ${inline(task[2])}</li>`
      );
      continue;
    }

    const bullet = /^[-*]\s+(.*)$/.exec(trimmed);
    if (bullet) {
      openList('ul');
      out.push(`<li>${inline(bullet[1])}</li>`);
      continue;
    }

    const numbered = /^\d+[.)]\s+(.*)$/.exec(trimmed);
    if (numbered) {
      openList('ol');
      out.push(`<li>${inline(numbered[1])}</li>`);
      continue;
    }

    closeList();
    out.push(`<p>${inline(trimmed)}</p>`);
  }
  // An answer cut off mid-block still shows its code.
  if (fence) out.push(`<pre class="md-code"><code>${fence.join('\n')}</code></pre>`);
  closeList();
  return out.join('\n');
}
