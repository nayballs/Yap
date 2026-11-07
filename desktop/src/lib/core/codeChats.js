// **Code chats** (desktop/src-tauri/src/jobs/code_chat.rs): Chat has two
// sides, flipped by the switch at the top of its list — **Chat** (plain
// conversations, as ever) and **Code** (conversations attached to one of the
// person's workspaces, where Chat hands tasks to their own Claude Code).
// The words they share and the list's grouping, pure.

/** The feature's name, in one place (it may become "Yap Code"). Never "Claude Code" anything. */
export const FEATURE_NAME = 'Jobs';
/** What the UI calls a conversation attached to a workspace, and its side of Chat. */
export const CODE_NAME = 'Code';
/** What the UI calls a job folder. */
export const WORKSPACE = 'workspace';
export const WORKSPACES = 'Workspaces';

/** The label on a Code chat's message box: "Code · garden-planner". */
export function codeLabel(name) {
  return name ? `${CODE_NAME} · ${name}` : CODE_NAME;
}

/** Are two paths the same folder (`\` or `/`, a trailing one, case)? */
export function samePath(a, b) {
  const norm = (p) => String(p ?? '').trim().replace(/\//g, '\\').replace(/\\+$/, '').toLowerCase();
  return !!a && !!b && norm(a) === norm(b);
}

/** Which side a conversation lives on: `chats` maps a Code chat's id to its workspace. */
export function sideOf(id, chats) {
  return id != null && chats?.[id] ? 'code' : 'chat';
}

/**
 * **The Code side's list**: Code chats grouped by workspace, each group the
 * workspace's name (its path on hover) and its chats newest first; the
 * groups by their newest chat. `collapsed` (path → true) folds a group: its
 * header stays, with how many it holds (`count`) and which (`ids`: its
 * status dot, the strongest of theirs). Only workspaces with chats.
 */
export function groupByWorkspace(conversations, chats, collapsed = {}) {
  // Newest first: by when it was last changed, then (the same second) the
  // later conversation (ids only grow).
  const newer = (a, b) => (b.updatedTs || 0) - (a.updatedTs || 0) || (b.id || 0) - (a.id || 0);
  const groups = new Map();
  for (const c of conversations ?? []) {
    const ws = chats?.[c.id];
    if (!ws?.path) continue;
    const key = ws.path.toLowerCase();
    if (!groups.has(key)) groups.set(key, { key: `ws:${ws.path}`, label: ws.name, path: ws.path, workspace: ws, items: [] });
    groups.get(key).items.push(c);
  }
  return [...groups.values()]
    .map((g) => {
      const items = [...g.items].sort(newer);
      const folded = !!collapsed?.[g.path];
      return { key: g.key, label: g.label, path: g.path, workspace: g.workspace, collapsible: true, collapsed: folded, count: items.length, ids: items.map((c) => c.id), top: items[0], items: folded ? [] : items };
    })
    .sort((a, b) => newer(a.top, b.top) || a.label.localeCompare(b.label));
}

/** The plain conversations (the Chat side): no workspace. */
export function plainOnly(conversations, chats) {
  return (conversations ?? []).filter((c) => !chats?.[c.id]);
}

/**
 * The workspaces as the picker lists them: most recently used first (Rust's
 * order, `usedTs`), the one in use marked.
 */
export function pickerRows(workspaces, current) {
  return (workspaces ?? []).map((w) => ({ ...w, on: !!current && samePath(w.path, current.path) }));
}

// ---- an empty Code chat's greeting (CodeEmpty.svelte) ----

/** What a Code chat is for, in one line. */
export const CODE_INTRO = `Ask for a change, a fix or an explanation. ${FEATURE_NAME} run your own Claude Code here, and you can follow each step beside the chat.`;

/** The one-tap prompts: each fills the box, never sent by itself (a request here can start a job). */
export const CODE_PROMPTS = ['Explain how this project is laid out', "Find and fix what's failing", 'What changed here recently?'];

/**
 * **What stands between this PC and a job**, from Claude Code's status
 * (stores/claudePlan.svelte.js) and how jobs pay (`jobsPay`): nothing (null,
 * also while it's still being looked for), or a line and its one button —
 * `install` (not on this PC), `update` (too old), `sign-in` (signed out, on
 * the plan: a key pays without a sign-in), `check` (signing in, in the
 * terminal Yap opened).
 */
export function claudeCodeHint(cc, pay = 'plan') {
  if (!cc?.loaded) return null;
  if (!cc.installed) return { state: 'missing', text: `${FEATURE_NAME} run your own Claude Code, and it isn't on this PC yet.`, action: 'install', label: 'Install Claude Code' };
  if (cc.tooOld) {
    const version = cc.version ? `Claude Code ${cc.version}` : 'This Claude Code';
    return { state: 'old', text: `${version} is too old for jobs: Yap needs ${cc.minVersion} or newer.`, action: 'update', label: 'How to update' };
  }
  if (cc.loggedIn || pay === 'key') return null;
  if (cc.signingIn) return { state: 'signing-in', text: 'Finish signing in to Claude Code in the terminal Yap opened.', action: 'check', label: 'Check again' };
  return { state: 'signed-out', text: "Claude Code isn't signed in on this PC yet.", action: 'sign-in', label: 'Sign in' };
}
