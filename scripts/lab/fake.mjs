// The fake AI's scripted calls in a lab (`fake`, a script's `fake` step;
// docs/lab.md). They're JSON, so an argument that depends on what the turn
// has seen — a ref on a live page (`--fake --live-web`), a point of its
// screenshot — is a placeholder the lab fills in when the call is made, from
// the request's tool results (the e2e specs' `args: (r) => …`):
// - "$ref:<words>": the ref of the element or match named exactly <words> in
//   the turn's latest browser_read (mode "elements") or browser_find result
//   that lists it; "$ref:<kind>:<words>" only one of that kind (heading,
//   link, button, item, text, textbox, searchbox, frame…): a page's own
//   heading, not a link with the same words; words ending in `*`: any that
//   start so (a paragraph's first words: "$ref:text:A lighthouse is*");
// - "$x:<fraction>", "$y:<fraction>": that fraction of the latest
//   browser_screenshot's width or height (a point of it, in its pixels).
// Not found: 0 (the tool then says so, as a model's wrong ref would be told).

/** A request's tool results, oldest first (OpenAI's `tool` messages). */
export function toolResults(request) {
  return (request?.messages ?? [])
    .filter((m) => m.role === 'tool')
    .map((m) => {
      try {
        return JSON.parse(m.content);
      } catch {
        return null;
      }
    })
    .filter(Boolean);
}

const escape = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

/**
 * The ref of `words` (of `kind`, if given) in the latest list that has it;
 * `words` ending in `*`: any whose words start so (a paragraph's first words).
 */
export function refIn(request, words, kind = '') {
  const named = words.endsWith('*') ? `${escape(words.slice(0, -1))}[^"\\n]*` : escape(words);
  const re = new RegExp(`^\\[(\\d+)\\] ${kind ? escape(kind) : '\\S+'} "${named}"`, 'm');
  for (const r of toolResults(request).reverse()) {
    const lines = [r?.data?.elements, r?.data?.matches].filter((x) => typeof x === 'string').join('\n');
    const m = lines && re.exec(lines);
    if (m) return Number(m[1]);
  }
  return 0;
}

/** The latest screenshot's size in a request's results ("459×601" → [459, 601]). */
export function shotSize(request) {
  const shot = toolResults(request)
    .reverse()
    .find((r) => typeof r?.data?.size === 'string' && r.frame);
  return shot ? shot.data.size.split('×').map(Number) : [0, 0];
}

const KINDS = new Set(['heading', 'link', 'button', 'item', 'text', 'cell', 'label', 'textbox', 'searchbox', 'combobox', 'checkbox', 'radio', 'frame', 'element', 'tab', 'menuitem', 'option']);

/** One value filled in for `request` (a placeholder, or as it is). */
function fill(v, request) {
  if (typeof v === 'string') {
    const ref = /^\$ref:(.+)$/s.exec(v);
    if (ref) {
      const [first, ...rest] = ref[1].split(':');
      return rest.length && KINDS.has(first) ? refIn(request, rest.join(':'), first) : refIn(request, ref[1]);
    }
    const at = /^\$([xy]):([\d.]+)$/.exec(v);
    if (at) return Math.round(shotSize(request)[at[1] === 'x' ? 0 : 1] * Number(at[2]));
    return v;
  }
  if (Array.isArray(v)) return v.map((x) => fill(x, request));
  if (v && typeof v === 'object') return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, fill(x, request)]));
  return v;
}

/** Whether `v` holds a placeholder. */
function hasPlaceholder(v) {
  if (typeof v === 'string') return /^\$(ref:.+|[xy]:[\d.]+)$/s.test(v);
  if (Array.isArray(v)) return v.some(hasPlaceholder);
  if (v && typeof v === 'object') return Object.values(v).some(hasPlaceholder);
  return false;
}

/** `args` filled in for `request`. Pure. */
export function fillArgs(args, request) {
  return fill(args, request);
}

/**
 * A fake script's steps with each call whose arguments hold a placeholder
 * made a function of the request (the fake AI calls it when it answers that
 * request: support/fake-llm.js). Pure.
 */
export function templated(steps) {
  return (steps ?? []).map((s) =>
    Array.isArray(s?.tools) ? { ...s, tools: s.tools.map((t) => (hasPlaceholder(t.args) ? { ...t, args: (request) => fill(t.args, request) } : t)) } : s,
  );
}
