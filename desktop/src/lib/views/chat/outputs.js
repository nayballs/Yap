// **A chat's Outputs** (15 Nov 2026, after the Claude app's right column,
// whose Outputs lists everything a session made): one list, drawn by
// OutputsList.svelte, for the Code chat's summary (SummaryTab.svelte) and
// anywhere else that lists what a chat made. Each kind adds its own rows
// here, so there's one list, never two:
//
// - `page`: a page it keeps (chat/made/pages.rs `save_page`: `made`, what
//   it made), newest first; opens beside the chat (KeptPageTab). First, as
//   the Claude app lists its artifact first.
// - `doc`: a document it wrote (chat/documents.rs: an answer's
//   `documents`), newest first; opens beside the chat.
// - `changed`: a file the chat's work changed (its agents' turns' `files`,
//   relative to the workspace: agents/store.rs), newest first, each once;
//   opens in the file view beside the chat.
// - `shared`: a file shared into the conversation (`share_files`,
//   chat/shares.rs: an answer's `files`), newest first; a picture opens in
//   the picture tab, a file in the workspace in the file view, any other as
//   its card does.
//
// `[{ key, kind, name, detail, label, … }]`: `name` the row's words,
// `detail` its quieter part (a folder), `label` its right-hand word. No
// Svelte state: pure.

/** A path's last part and the folder before it (`/` or `\`). Pure. */
export function splitPath(p) {
  const parts = String(p || '')
    .split(/[\\/]/)
    .filter(Boolean);
  return { name: parts.pop() || String(p || ''), folder: parts.join('/') };
}

/** The files the chat's `agents` changed, newest first, each once (its newest turn's). Pure. */
function changedFiles(agents) {
  const all = [];
  for (const a of agents || []) {
    const turns = a?.turnList || [];
    if (turns.length) {
      for (const t of turns) for (const f of t.files || []) all.push({ f, ts: Number(t.endedTs) || Number(t.startedTs) || Number(a.updatedTs) || 0, n: Number(t.n) || 0 });
    } else {
      for (const f of a?.files || []) all.push({ f, ts: Number(a.updatedTs) || 0, n: 0 });
    }
  }
  all.sort((x, y) => y.ts - x.ts || y.n - x.n);
  const seen = new Set();
  const out = [];
  for (const { f } of all) {
    const k = String(f).replace(/\\/g, '/').toLowerCase();
    if (!f || seen.has(k)) continue;
    seen.add(k);
    out.push(f);
  }
  return out;
}

/**
 * **A chat's outputs**: `agents` (its agents, as the agents store has them),
 * `messages` (its thread) and `made` (what it made: made.svelte.js). Pages it
 * keeps first, then documents, then changed files, then files shared.
 */
export function outputItems({ agents = [], messages = [], made = [] } = {}) {
  const out = [];
  const pages = (made || []).filter((m) => m?.kind === 'page' && m.id).sort((a, b) => (Number(b.at) || 0) - (Number(a.at) || 0));
  for (const p of pages) {
    const n = p.versions?.length || 1;
    out.push({ key: `page:${p.id}`, kind: 'page', name: p.title || 'Page', detail: '', label: n > 1 ? `Page · ${n} versions` : 'Page', page: p });
  }
  const docs = new Set();
  for (const m of [...(messages || [])].reverse()) {
    if (m?.role !== 'assistant') continue;
    for (const d of m.documents?.length ? m.documents : m.liveDocs || []) {
      if (d?.id == null || docs.has(d.id)) continue;
      docs.add(d.id);
      out.push({ key: `doc:${d.id}`, kind: 'doc', name: d.title || 'Untitled document', detail: '', label: 'Document', id: d.id });
    }
  }
  for (const f of changedFiles(agents)) {
    const { name, folder } = splitPath(f);
    out.push({ key: `changed:${f}`, kind: 'changed', name, detail: folder, label: 'Changed', rel: String(f).replace(/\\/g, '/') });
  }
  const seen = new Set();
  for (const m of [...(messages || [])].reverse()) {
    if (m?.role !== 'assistant') continue;
    for (const share of m.files?.length ? m.files : m.liveFiles || []) {
      for (const item of share?.items || []) {
        const id = item?.path || item?.id;
        if (!id || seen.has(id)) continue;
        seen.add(id);
        const { folder } = splitPath(item.path || '');
        out.push({ key: `shared:${id}`, kind: 'shared', name: item.name || splitPath(item.path).name, detail: folder, label: item.kind === 'image' ? 'Picture' : 'Shared', item, picture: item.kind === 'image' && !!item.id });
      }
    }
  }
  return out;
}
