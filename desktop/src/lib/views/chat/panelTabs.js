// The panel's row of tabs (DocPanel's own chunk): a conversation's documents,
// the pictures Chat found online and the live pages it brought up, from its
// answers (chatDoc.svelte.js keeps which one shows, the ones closed and the
// ones a turn opened before its answer was kept). No Svelte state: pure.
import { docKey, picKey, pageKey } from './chatDoc.svelte.js';

/**
 * The panel's items in a conversation (`messages`, as the thread has them):
 * its answers' documents, pictures and pages — saved with the answer, or
 * (while it's written) as they come (`liveDocs`, `livePictures`,
 * `livePages`) — each once, in the order they first came, newest last. A
 * document keeps its latest title. `[{key, kind: 'doc', id, title} | {key,
 * kind: 'picture', id, picture} | {key, kind: 'page', page}]`.
 */
export function panelItems(messages) {
  const out = [];
  const at = new Map();
  for (const m of messages || []) {
    if (m?.role !== 'assistant') continue;
    for (const d of m.documents?.length ? m.documents : m.liveDocs || []) {
      if (d?.id == null) continue;
      const key = docKey(d.id);
      if (at.has(key)) {
        if (d.title) out[at.get(key)] = { ...out[at.get(key)], title: d.title };
        continue;
      }
      at.set(key, out.length);
      out.push({ key, kind: 'doc', id: d.id, title: d.title || '' });
    }
    for (const p of (m.pictures?.length ? m.pictures : m.livePictures || []).filter((x) => !x?.chip)) {
      if (!p?.id) continue;
      const key = picKey(p.id);
      if (at.has(key)) continue;
      at.set(key, out.length);
      out.push({ key, kind: 'picture', id: p.id, picture: p });
    }
    for (const p of m.pages?.length ? m.pages : m.livePages || []) {
      if (!p?.url) continue;
      const key = pageKey(p.url);
      if (at.has(key)) continue;
      at.set(key, out.length);
      out.push({ key, kind: 'page', page: p });
    }
  }
  return out;
}

/**
 * The row: the conversation's items, then any shown that no answer has
 * yet (`extra`), less the ones closed (`hidden`). The document the panel
 * has loaded (`id`) carries its title as it is now (`title`).
 */
export function panelTabs(messages, { extra = [], hidden = [], id = null, title = '' } = {}) {
  const all = panelItems(messages);
  const keys = new Set(all.map((t) => t.key));
  for (const t of extra) {
    if (keys.has(t.key)) continue;
    keys.add(t.key);
    all.push(t);
  }
  return all.filter((t) => !hidden.includes(t.key)).map((t) => (t.kind === 'doc' && t.id === id && title ? { ...t, title } : t));
}

/** The tab that takes `key`'s place when it's closed: the next, else the one before. */
export function neighbour(tabs, key) {
  const i = tabs.findIndex((t) => t.key === key);
  return i < 0 ? null : (tabs[i + 1] ?? tabs[i - 1] ?? null);
}
