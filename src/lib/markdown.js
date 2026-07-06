// Minimal, safe markdown → HTML renderer for the Enhanced notes tab.
//
// Deliberately tiny: the note-enhancement base prompt (llm::NOTE_BASE_PROMPT)
// forbids tables / horizontal rules / block quotes, so the output space is
// headings, paragraphs, bold/italic/code, bullet + numbered lists, and task
// checkboxes — exactly what this covers. All input is HTML-escaped FIRST, so
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

// Inline transforms on an already-escaped line: code, bold, italic.
function inline(s) {
  return s
    .replace(/`([^`]+)`/g, '<code>$1</code>')
    .replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>')
    .replace(/(^|[\s(])\*([^*\s][^*]*)\*(?=$|[\s).,;:!?])/g, '$1<em>$2</em>');
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

  for (const raw of lines) {
    const line = raw.trimEnd();
    const trimmed = line.trim();

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
  closeList();
  return out.join('\n');
}
