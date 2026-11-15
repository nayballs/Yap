// **Comments on an agent's plan** (AgentApprove.svelte's plan card, PlanHistory.svelte;
// integrations/claude_live/approve.rs `claude_approve_revise`, 15 Nov 2026, the Claude app's plan
// review): words of the plan selected and commented on, the commented words marked in the plan, and
// the plans sent back with comments kept folded. The pure parts are checked by the e2e suite with no app.

/** The most comments a plan sends back, and how long a quote or a comment may be (approve.rs). */
export const COMMENTS_MAX = 20;
export const QUOTE_MAX = 400;
export const COMMENT_MAX = 2000;

/** A selection's words as a comment quotes them: on one line, at most QUOTE_MAX characters. */
export function quoteOf(text) {
  const flat = String(text || '').replace(/\s+/g, ' ').trim();
  return flat.length <= QUOTE_MAX ? flat : `${flat.slice(0, QUOTE_MAX - 1).trimEnd()}…`;
}

/** `list` with a comment added (`quote`: the words it's on, empty for the whole plan); nothing to say: as it was. */
export function withComment(list = [], quote, text) {
  const words = String(text || '').trim().slice(0, COMMENT_MAX);
  if (!words || list.length >= COMMENTS_MAX) return list;
  return [...list, { quote: quoteOf(quote), text: words }];
}

/** "Plan before your comments · 2 comments", "…· 1 comment". */
export function beforeLabel(plan, i = 0, n = 1) {
  const k = plan?.comments?.length || 0;
  const count = `${k} comment${k === 1 ? '' : 's'}`;
  return n > 1 ? `Version ${i + 1}, before your comments · ${count}` : `Plan before your comments · ${count}`;
}

/** The plans turn `t` sent back with comments (its `plansBefore`), oldest first. */
export const plansBeforeOf = (t) => (Array.isArray(t?.plansBefore) ? t.plansBefore : []);

/**
 * **The selected words, if they're inside `el`** (the plan as shown): `{quote, rect}` — the words on one line, and
 * where the selection ends, for the Comment button — or `null`.
 */
export function selectionIn(el) {
  const sel = typeof window !== 'undefined' ? window.getSelection() : null;
  if (!el || !sel || sel.isCollapsed || !sel.rangeCount) return null;
  const range = sel.getRangeAt(0);
  if (!el.contains(range.commonAncestorContainer)) return null;
  const quote = quoteOf(sel.toString());
  if (!quote) return null;
  const rects = range.getClientRects();
  const rect = rects.length ? rects[rects.length - 1] : range.getBoundingClientRect();
  return { quote, rect };
}

/**
 * **Mark each comment's words in the plan** (a Svelte action on the plan's rendered markdown): the first place
 * each quote's words appear, across the element's text nodes, wrapped in `<mark class="pmark">`. Updated as the
 * comments change; the plan's HTML is drawn again first ({@html}), so marks never pile up.
 */
export function markQuotes(el, quotes) {
  const apply = (list) => {
    for (const m of el.querySelectorAll('mark.pmark')) m.replaceWith(...m.childNodes);
    el.normalize();
    for (const q of (list || []).map((x) => (typeof x === 'string' ? x : x?.quote)).filter(Boolean)) mark(el, q);
  };
  apply(quotes);
  return { update: apply };
}

/** Wrap the first run of `quote`'s words in `el`'s text (spaces as they fall: one or more of any kind). */
function mark(el, quote) {
  const words = quote.replace(/…$/, '').split(/\s+/).filter(Boolean);
  if (!words.length) return;
  const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
  const nodes = [];
  let text = '';
  for (let n = walker.nextNode(); n; n = walker.nextNode()) {
    nodes.push({ node: n, start: text.length });
    text += n.data;
  }
  const esc = (w) => w.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const found = new RegExp(words.map(esc).join('\\s+')).exec(text);
  if (!found) return;
  const from = found.index;
  const to = from + found[0].length;
  for (const { node, start } of nodes) {
    const end = start + node.data.length;
    if (end <= from || start >= to) continue;
    const a = Math.max(from, start) - start;
    const b = Math.min(to, end) - start;
    if (b <= a) continue;
    const range = document.createRange();
    range.setStart(node, a);
    range.setEnd(node, b);
    const m = document.createElement('mark');
    m.className = 'pmark';
    range.surroundContents(m);
  }
}
