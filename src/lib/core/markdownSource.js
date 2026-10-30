// Markdown as source, simply coloured (the Chat document panel's Source
// view, after Claude Code's file view): headings, list and quote marks, a
// table's pipes, code, **bold**, links, and a section still to write (its
// placeholder comment) — each wrapped in a span whose class the view
// colours (`md-s-*`). The text itself is never changed: the spans add no
// characters, so the coloured copy lines up exactly under the editable text
// box laid over it, and a find counts the same characters. Pure.

const ESC = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' };
const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ESC[c]);
const span = (cls, html) => (html ? `<span class="${cls}">${html}</span>` : '');

// Inline: `code`, **bold** / __bold__, [text](link), *emphasis*.
const INLINE = /(`[^`\n]+`)|(\*\*[^*\n]+\*\*|__[^_\n]+__)|(\[[^\]\n]+\]\([^)\s]+\))|(\*[^*\s][^*\n]*\*)/g;

function inline(text) {
  let out = '';
  let at = 0;
  for (const m of text.matchAll(INLINE)) {
    out += esc(text.slice(at, m.index));
    const t = m[0];
    if (m[1]) out += span('md-s-icode', esc(t));
    else if (m[2]) out += span('md-s-mark', esc(t.slice(0, 2))) + span('md-s-strong', esc(t.slice(2, -2))) + span('md-s-mark', esc(t.slice(-2)));
    else if (m[3]) {
      const k = t.indexOf('](');
      out += span('md-s-link', esc(t.slice(0, k + 1))) + span('md-s-url', esc(t.slice(k + 1)));
    } else out += span('md-s-mark', '*') + span('md-s-em', esc(t.slice(1, -1))) + span('md-s-mark', '*');
    at = m.index + t.length;
  }
  return out + esc(text.slice(at));
}

/** A table row: its pipes quiet, its cells as text. */
function row(line) {
  return line
    .split('|')
    .map((cell) => inline(cell))
    .join(span('md-s-mark', '|'));
}

/**
 * `src` as coloured HTML, line for line (`\r` left out, as a text box shows
 * it). Pure.
 */
export function highlightMarkdown(src) {
  const lines = String(src ?? '').split('\n');
  let fence = null;
  return lines
    .map((raw) => {
      const line = raw.replace(/\r$/, '');
      const f = /^\s{0,3}(`{3,}|~{3,})/.exec(line);
      if (fence) {
        if (f && f[1][0] === fence[0] && f[1].length >= fence.length) {
          fence = null;
          return span('md-s-fence', esc(line));
        }
        return span('md-s-code', esc(line));
      }
      if (f) {
        fence = f[1];
        return span('md-s-fence', esc(line));
      }
      if (/^\s*<!--\s*yap:section\b/.test(line)) return span('md-s-comment', esc(line));
      const h = /^(\s{0,3}#{1,6})(\s.*)?$/.exec(line);
      if (h) return span('md-s-heading', span('md-s-mark', esc(h[1])) + inline(h[2] || ''));
      if (/^\s{0,3}([-*_])(\s*\1){2,}\s*$/.test(line)) return span('md-s-mark', esc(line));
      if (line.includes('|') && /^[\s|:-]+$/.test(line) && line.includes('-')) return span('md-s-mark', esc(line));
      if (/^\s*\|/.test(line)) return row(line);
      const q = /^(\s{0,3}>+\s?)(.*)$/.exec(line);
      if (q) return span('md-s-quote', span('md-s-mark', esc(q[1])) + inline(q[2]));
      const li = /^(\s*)([-*+]|\d{1,9}[.)])(\s+)(\[[ xX]\]\s+)?(.*)$/.exec(line);
      if (li) return esc(li[1]) + span('md-s-mark', esc(li[2])) + esc(li[3]) + (li[4] ? span('md-s-mark', esc(li[4])) : '') + inline(li[5]);
      return inline(line);
    })
    .join('\n');
}
