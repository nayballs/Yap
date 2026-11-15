// **A chat's Outputs** (15 Nov 2026, after the Claude app's Outputs list): every page it kept
// (chat/made/pages.rs), document it wrote (chat/documents.rs) and file it shared (chat/shares.rs), each once,
// newest first in its kind — pages, then documents, then files. Pure.
import { kindLabel } from './shares.js';

/**
 * `messages`: the conversation's, as the thread has them; `made`: what it made (made.svelte.js). Returns
 * `[{key, kind: 'page'|'doc'|'file', title, label, …}]`.
 */
export function outputsOf(messages, made) {
  const pages = (made || [])
    .filter((m) => m?.kind === 'page' && m.id)
    .map((p) => {
      const n = p.versions?.length || 1;
      return { key: `kept:${p.id}`, kind: 'page', title: p.title || 'Page', label: n > 1 ? `Page · ${n} versions` : 'Page', page: p, at: p.at || 0 };
    })
    .sort((a, b) => b.at - a.at);
  const docs = [];
  const files = [];
  const seen = new Set();
  [...(messages || [])].reverse().forEach((m) => {
    if (m?.role !== 'assistant' && m?.kind !== 'agentReport') return;
    for (const d of m.documents || []) {
      if (d?.id == null || seen.has(`doc:${d.id}`)) continue;
      seen.add(`doc:${d.id}`);
      docs.push({ key: `doc:${d.id}`, kind: 'doc', title: d.title || 'Untitled document', label: 'Document', id: d.id });
    }
    for (const share of m.files || []) {
      for (const item of share?.items || []) {
        const key = `file:${item.path || item.id || item.name}`;
        if (!item?.name || seen.has(key)) continue;
        seen.add(key);
        files.push({ key, kind: 'file', title: item.name, label: kindLabel(item), item });
      }
    }
  });
  return [...pages, ...docs, ...files];
}
