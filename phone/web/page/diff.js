// **One file's diff** in the agent's live view (Remote Control, wave 2):
// what an agent changed in a file, as its tab on the PC shows it
// (AgentFiles.svelte) — monospace, removed lines red and added green, a
// gap between edits, scrolled sideways inside the code only (never the
// page), with Wrap to fold long lines instead. The lines come from the PC
// when asked (`agent_diff`: 400 at most, then "see the rest on your PC").
// Built as nodes, never HTML.

import { diffLine, diffTitle, diffTotals } from './core/agents.js';
import { h } from './dom.js';

/** A file's line in the list: "+12 −3", or "New file · 12 lines". Pure. */
export function fileFacts(diffs) {
  const t = diffTotals(diffs || []);
  const title = diffs?.length === 1 ? diffTitle(diffs[0]) : t.made === 'new' ? `New file · ${t.added === 1 ? '1 line' : `${t.added} lines`}` : '';
  return { added: t.added, removed: t.removed, title, made: t.made };
}

/** The +N −M counts as nodes. */
export function counts({ added = 0, removed = 0 }) {
  return h(
    'span',
    { class: 'dcounts', 'aria-label': `${added} added, ${removed} removed` },
    h('span', { class: 'dadd', text: `+${added}` }),
    h('span', { class: 'ddel', text: `−${removed}` }),
  );
}

/**
 * **A diff's lines** (`{lines, more, wrap}`; `onWrap()` flips Wrap): the
 * code, each line marked, Wrap above it and, past the PC's 400 lines, the
 * rest left to the PC.
 */
export function renderDiff({ lines = [], more = false, wrap = false, loading = false, onWrap }) {
  const box = h('div', { class: 'diff', testid: 'agent-diff', data: { wrap: String(!!wrap) } });
  box.append(
    h(
      'div',
      { class: 'dbar' },
      h('span', { class: 'dnote', text: loading ? 'Asking your PC for the changes…' : lines.length ? '' : 'No lines to show.' }),
      h('button', {
        type: 'button',
        class: 'dwrap',
        testid: 'agent-diff-wrap',
        'aria-pressed': String(!!wrap),
        text: 'Wrap',
        on: { click: () => onWrap?.() },
      }),
    ),
  );
  if (lines.length) {
    const code = h('pre', { class: 'dcode', tabindex: '0', 'aria-label': 'The changes' });
    for (const raw of lines) {
      const l = diffLine(raw);
      // (A part's place in the file, `@@ -a,b +c,d @@`, reads as a gap: none before the first part.)
      if (String(raw).startsWith('@@ ') && !code.childNodes.length) continue;
      code.append(
        h(
          'span',
          { class: `dl ${l.mark}`, testid: 'diff-line', data: { mark: l.mark } },
          h('span', { class: 'dm', 'aria-hidden': 'true', text: l.mark === 'del' ? '−' : l.mark === 'add' ? '+' : l.mark === 'gap' ? '⋯' : ' ' }),
          l.mark === 'gap' ? '' : l.text || ' ',
        ),
      );
    }
    box.append(code);
  }
  if (more) box.append(h('p', { class: 'dmore', testid: 'agent-diff-more', text: '…more lines · see the rest on your PC' }));
  return box;
}
