// **What each chat made** (chat/made.rs, 15 Nov 2026): its visuals, read with the chat
// (`chat_made_list`) and heard the moment one's made (`yap-chat-made`), and where each shows in the
// thread (`placeVisuals`): in the answer to the question it was made for, above the answer's words.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

/** conversation id → what it made, oldest first. */
let byChat = $state({});
let following = false;

export const made = {
  /** What conversation `id` made (none read yet: none). */
  of(id) {
    return id == null ? [] : (byChat[id] ?? []);
  },
  /** Read conversation `id`'s (opening it). */
  async load(id) {
    if (id == null) return;
    try {
      const got = await invoke('chat_made_list', { conversationId: id });
      const list = Array.isArray(got) ? got : [];
      // (One heard while this was read stays: by id, in the order made.)
      const heardHere = (byChat[id] ?? []).filter((m) => !list.some((x) => x.id === m.id));
      byChat = { ...byChat, [id]: [...list, ...heardHere].sort((a, b) => (a.at || 0) - (b.at || 0)) };
    } catch {
      /* an older Yap: none */
    }
  },
};

/** One made thing heard (`{conversation, made}`): added to its chat's, once. Pure but for the store. */
export function heard(p) {
  const id = p?.conversation;
  const m = p?.made;
  if (id == null || !m?.id) return;
  const list = byChat[id] ?? [];
  if (list.some((x) => x.id === m.id)) return;
  byChat = { ...byChat, [id]: [...list, m] };
}

/** Listen for what's made, from now on (once). */
export function followMade() {
  if (following) return;
  following = true;
  listen('yap-chat-made', (e) => heard(e.payload)).catch(() => {
    following = false;
  });
}

/**
 * **The question `after` names** in `messages`: the person's message at its place, while its time is
 * the same or not known yet (a question just sent has none here); else the person's message with that
 * time nearest its place; else none (-1: a Rewind took it). Pure.
 */
export function questionIndex(messages, after) {
  if (!after || !messages?.length) return -1;
  const ts = Number(after.ts) || 0;
  const at = Number(after.index);
  const isQ = (m) => m?.role === 'user';
  if (Number.isInteger(at) && isQ(messages[at]) && (!ts || !messages[at].ts || Number(messages[at].ts) === ts)) return at;
  if (!ts) return -1;
  let best = -1;
  messages.forEach((m, i) => {
    if (isQ(m) && Number(m.ts) === ts && (best < 0 || Math.abs(i - at) < Math.abs(best - at))) best = i;
  });
  return best;
}

/**
 * **Where each visual shows** in `messages`: in the first answer after its question (`inAnswer`, above
 * the answer's words), or — no answer yet (a Code chat's Claude still working) — under the question's
 * own work. `Map<index, { inAnswer, visuals }>`, each in the order made. `forks`: the chat's versions
 * (chat_get's `forks`, chat/chats/branches.rs): an answer retried shows only what its own turn made —
 * after the version before it (`since`), by the time it was saved — never another version's. Pure.
 */
/** Was visual `v` made by the turn of `answer`, the version shown of `fork` (none: it has no other)? */
function ownVersion(v, answer, fork) {
  if (!fork) return true;
  const made = Math.floor((Number(v?.at) || 0) / 1000);
  if (!made) return true;
  const saved = Number(answer?.ts) || 0;
  return made > (Number(fork.since) || 0) && (!saved || made <= saved);
}

export function placeVisuals(messages, items, forks = []) {
  const out = new Map();
  if (!items?.length || !messages?.length) return out;
  for (const v of items) {
    if (v?.kind !== 'visual') continue;
    const q = questionIndex(messages, v.after);
    if (q < 0) continue;
    let at = q;
    let inAnswer = false;
    for (let i = q + 1; i < messages.length; i++) {
      const m = messages[i];
      if (m.role === 'assistant') {
        at = i;
        inAnswer = true;
        break;
      }
      if (m.role === 'user' && !m.kind) break;
    }
    if (inAnswer && !ownVersion(v, messages[at], (forks || []).find((f) => f.at === at))) continue;
    const slot = out.get(at) ?? { inAnswer, visuals: [] };
    slot.visuals.push(v);
    out.set(at, slot);
  }
  return out;
}
