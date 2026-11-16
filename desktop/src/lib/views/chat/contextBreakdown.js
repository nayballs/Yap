// **A Code chat's context, broken down** (15 Nov 2026, after the Claude app's
// right column): the summary's Context from Claude Code's own `/context`
// (agents/context.rs `code_chat_context`) — "16% used · 156k / 1M", a bar
// coloured by category, a row per category (its tokens and share of the
// window), the deferred ones with no share, then the lists behind them (MCP
// tools, memory files, custom agents, skills), each with its count. Pure: no
// Svelte state, so the e2e spec checks it directly.

/** The categories in the order the Claude app lists them, each its colour token (app.css `--yap-ctx-*`). */
export const CATEGORY_ORDER = [
  ['Messages', 'messages'],
  ['Memory files', 'memory'],
  ['System tools', 'tools'],
  ['MCP tools', 'mcp'],
  ['System prompt', 'prompt'],
  ['Skills', 'skills'],
  ['MCP server instructions', 'instructions'],
  ['Custom agents', 'agents'],
  ['Autocompact buffer', 'buffer'],
  ['Free space', 'free'],
];

/**
 * What the categories with more behind them are, on hover (their names underlined with dots, as the Claude app
 * marks them).
 */
export const CATEGORY_TIPS = {
  'Memory files': 'The CLAUDE.md files Claude Code reads into every turn: yours, the project’s and any it imports.',
  'MCP tools': 'Tools from the MCP servers Claude Code has, described to the model on every turn.',
  Skills: 'Each skill’s name and description, so Claude knows when to use one. A skill’s full text loads only when it’s used.',
  'MCP server instructions': 'What each MCP server tells Claude about using its tools.',
  'Custom agents': 'Your subagents’ descriptions, so Claude can hand work to one.',
};

/**
 * A token count as the Claude app writes it: three figures at most —
 * `628`, `4.3k`, `32.2k`, `156k`, `1M`. Pure.
 */
export function fmtTokens(n) {
  const v = Math.max(0, Number(n) || 0);
  const short = (x) => (x >= 100 ? String(Math.round(x)) : String(Number(x.toFixed(1))));
  if (v < 1000) return String(Math.round(v));
  if (v < 999_500) return `${short(v / 1000)}k`;
  return `${short(v / 1_000_000)}M`;
}

/** `part` of `whole` as a share: `9.8%`, `0.0%`. Pure. */
export function fmtShare(part, whole) {
  if (!(whole > 0)) return '0%';
  return `${((Number(part) || 0) * 100 / whole).toFixed(1)}%`;
}

/** A long path's last two parts (`…\garden-planner\CLAUDE.md`), as it was written. Pure. */
export function shortPath(p) {
  const text = String(p || '');
  const sep = text.includes('\\') ? '\\' : '/';
  const parts = text.split(/[\\/]/).filter(Boolean);
  return parts.length > 3 ? `…${sep}${parts.slice(-2).join(sep)}` : text;
}

const tokenOf = (name) => CATEGORY_ORDER.find(([n]) => n.toLowerCase() === String(name).toLowerCase())?.[1] || 'other';

/**
 * **What the summary draws** from `b` (Rust's `Breakdown`), or null when
 * there's nothing to draw:
 * `{ pct, used, window, label, rows, deferred, segments, lists }` —
 * `rows` the categories in the window ({key, name, tokens, share, colour}),
 * Claude's order first and any it adds after (before the buffer and the
 * free space); `deferred` ({key, name, tokens}); `segments` the bar's parts
 * ({key, colour, width}: a % of the window, free space left as the bar's
 * own track); `lists` the folded lists that have rows ({key, title, count,
 * rows: [{key, cells: [main, quieter, tokens]}]}).
 */
export function contextView(b) {
  const window = Number(b?.window) || 0;
  const cats = Array.isArray(b?.categories) ? b.categories : [];
  if (!(window > 0) || !cats.length) return null;
  const inWindow = cats.filter((c) => c.kind !== 'deferred');
  const bufferAt = CATEGORY_ORDER.findIndex(([, t]) => t === 'buffer');
  const rank = (c) => {
    const at = CATEGORY_ORDER.findIndex(([n]) => n.toLowerCase() === String(c.name).toLowerCase());
    if (at >= 0) return at;
    // A category Claude Code adds later: with the others in use, before the buffer and the free space.
    return c.kind === 'free' ? CATEGORY_ORDER.length : c.kind === 'buffer' ? bufferAt : bufferAt - 0.5;
  };
  const colourOf = (c) => {
    const t = tokenOf(c.name);
    return t !== 'other' ? t : c.kind === 'free' ? 'free' : c.kind === 'buffer' ? 'buffer' : 'other';
  };
  const rows = [...inWindow]
    .sort((x, y) => rank(x) - rank(y))
    .map((c) => ({ key: c.name, name: c.name, tokens: Number(c.tokens) || 0, share: fmtShare(c.tokens, window), colour: colourOf(c), kind: c.kind }));
  const deferred = cats.filter((c) => c.kind === 'deferred').map((c) => ({ key: c.name, name: c.name, tokens: Number(c.tokens) || 0 }));
  const segments = rows
    .filter((r) => r.kind !== 'free' && r.tokens > 0)
    .map((r) => ({ key: r.key, colour: r.colour, width: Math.min(100, (r.tokens * 100) / window) }));
  const used = Number(b.used) || rows.filter((r) => r.kind === 'used').reduce((s, r) => s + r.tokens, 0);
  const pct = Math.round(Number.isFinite(Number(b.percent)) ? Number(b.percent) : (used * 100) / window);
  const list = (key, title, items, cells, full = null) => ({ key, title, count: items.length, rows: items.map((it, i) => ({ key: `${key}:${i}`, cells: cells(it), title: full?.(it) || '' })) });
  const lists = [
    list('mcp', 'MCP tools', b.mcpTools || [], (t) => [t.name, t.server, fmtTokens(t.tokens)]),
    list('memory', 'Memory files', b.memoryFiles || [], (f) => [shortPath(f.path), f.kind, fmtTokens(f.tokens)], (f) => f.path),
    list('agents', 'Custom agents', b.agents || [], (a) => [a.name, a.source, fmtTokens(a.tokens)]),
    list('skills', 'Skills', b.skills || [], (s) => [s.name, s.source, fmtTokens(s.tokens)]),
  ].filter((l) => l.count > 0);
  return { pct, used, window, label: `${pct}% used · ${fmtTokens(used)} / ${fmtTokens(window)}`, rows, deferred, segments, lists };
}

/** A pull request's state and checks in words (agents/prs.rs `Pr`). Pure. */
export const PR_STATE = { open: 'Open', draft: 'Draft', merged: 'Merged', closed: 'Closed' };
export const PR_CHECKS = {
  failing: 'Checks failing',
  running: 'Checks running',
  cancelled: 'Checks cancelled',
  passing: 'Checks passing',
  skipped: 'Checks skipped',
  neutral: 'Checks neutral',
  none: '',
};
/** GitHub's states in the order its checks list says them (agents/prs.rs `Counts`). */
const CHECK_STATES = ['failing', 'cancelled', 'pending', 'skipped', 'neutral', 'successful'];

/**
 * **A pull request's checks in GitHub's words** (16 Nov 2026, agents/prs.rs `checks` and `counts`): the rolled-up
 * word ("Checks cancelled"), "8 of 10 passed" when not all did (GitHub counts skipped and neutral as passed:
 * "8 of 10 checks passed"), and every state on hover ("1 failing, 2 cancelled, 1 neutral, 6 successful checks").
 * A cancelled check is cancelled, never failing. `{ word, detail, title }`; no word with no checks. Pure.
 */
export function prChecks(p) {
  const word = PR_CHECKS[p?.checks] ?? '';
  const n = p?.counts || {};
  const of = (k) => (Number.isFinite(n[k]) && n[k] > 0 ? n[k] : 0);
  const total = CHECK_STATES.reduce((sum, k) => sum + of(k), 0);
  if (!word || !total) return { word, detail: '', title: word };
  const passed = of('successful') + of('skipped') + of('neutral');
  const parts = CHECK_STATES.filter((k) => of(k)).map((k) => `${of(k)} ${k}`);
  const title = `${parts.join(', ')} ${total === 1 ? 'check' : 'checks'}`;
  return { word, detail: passed < total ? `${passed} of ${total} passed` : '', title };
}

/** The quiet line for a Changes section with no list (agents/prs.rs `Status`), or null: no section. Pure. */
export function prsNote(prs) {
  switch (prs?.status) {
    case 'ok':
      return prs.list?.length ? '' : 'No pull requests for this branch yet.';
    case 'no-branch':
      return 'Not on a branch, so no pull requests.';
    case 'no-remote':
      return 'This repository has no GitHub remote.';
    case 'no-gh':
      return "Install GitHub's gh to see this branch's pull requests.";
    case 'signed-out':
      return "Sign in to GitHub's gh (gh auth login) to see this branch's pull requests.";
    case 'failed':
      return "Couldn't read this branch's pull requests.";
    default:
      return null;
  }
}
