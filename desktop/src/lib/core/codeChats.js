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
 * header stays, with how many it holds. Only workspaces with chats.
 */
export function groupByWorkspace(conversations, chats, collapsed = {}) {
  const groups = new Map();
  for (const c of conversations ?? []) {
    const ws = chats?.[c.id];
    if (!ws?.path) continue;
    const key = ws.path.toLowerCase();
    if (!groups.has(key)) groups.set(key, { key: `ws:${ws.path}`, label: ws.name, path: ws.path, workspace: ws, items: [], newest: 0 });
    const g = groups.get(key);
    g.items.push(c);
    g.newest = Math.max(g.newest, c.updatedTs || 0);
  }
  return [...groups.values()]
    .map((g) => {
      const items = [...g.items].sort((a, b) => (b.updatedTs || 0) - (a.updatedTs || 0));
      const folded = !!collapsed?.[g.path];
      return { key: g.key, label: g.label, path: g.path, workspace: g.workspace, collapsible: true, collapsed: folded, count: items.length, newest: g.newest, items: folded ? [] : items };
    })
    .sort((a, b) => b.newest - a.newest || a.label.localeCompare(b.label));
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
