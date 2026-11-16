// **What each chat made** (chat/made.rs, 15 Nov 2026): its visuals and the pages it keeps, read with the
// chat (`chat_made_list`) and heard the moment one's made or saved again (`yap-chat-made`), and where each
// shows in the thread (`placeMade`): in the answer to the question it was made for, above the answer's
// words — a visual's frame, a page's card for each version an answer saved.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { toast } from '../../ui/toast.svelte.js';

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

/** Who hears each thing made (`onMade`). */
const listeners = new Set();

/**
 * One made thing heard (`{conversation, made}`): added to its chat's, or — a page saved again — in its
 * place. Pure but for the store.
 */
export function heard(p) {
  const id = p?.conversation;
  const m = p?.made;
  if (id == null || !m?.id) return;
  const list = byChat[id] ?? [];
  const at = list.findIndex((x) => x.id === m.id);
  if (at >= 0 && m.kind !== 'page') return;
  byChat = { ...byChat, [id]: at >= 0 ? list.map((x, i) => (i === at ? m : x)) : [...list, m] };
  for (const fn of listeners) fn(id, m);
}

/** Listen for what's made, from now on (once). */
export function followMade() {
  if (following) return;
  following = true;
  listen('yap-chat-made', (e) => heard(e.payload)).catch(() => {
    following = false;
  });
}

/** `fn(conversation, made)` for each thing made from now on; returns the function that stops it. */
export function onMade(fn) {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

/**
 * **A message a visual's or a page's button sent** (`sendPrompt`, visualBridge.js: the person's click,
 * text only): into `chat` — its own — as if typed; or, made after outside content (a page, the screen, a
 * connector) or while an answer's being written, into the box for the person to read and send.
 */
export function promptFromVisual(chat, text, v, conversation) {
  if (conversation == null || chat.activeId !== conversation) return;
  if (v?.outside || chat.sending) {
    chat.input = text;
    chat.inputEl?.focus();
    if (v?.outside) toast({ title: 'From the visual', description: 'In the box for you to read: send it if you want to.' });
    return;
  }
  chat.send({ text });
}

/**
 * **The question `after` names** in `messages`: the person's message at its place, while its time is
 * the same or not known yet (a question just sent has none here — but one sent after `after`'s was
 * saved, an Edit's new words in its place, is another: `shownAt`); else the person's message with that
 * time nearest its place; else none (-1: a Rewind took it, or an Edit set it aside). Pure.
 */
export function questionIndex(messages, after) {
  if (!after || !messages?.length) return -1;
  const ts = Number(after.ts) || 0;
  const at = Number(after.index);
  const isQ = (m) => m?.role === 'user';
  const m = messages[at];
  // (Saved by its time; or not read back yet: the same one unless it was sent a second or more after `after`'s.)
  const same = !ts || (m?.ts ? Number(m.ts) === ts : !(Number(m?.shownAt) > ts));
  if (Number.isInteger(at) && isQ(m) && same) return at;
  if (!ts) return -1;
  let best = -1;
  messages.forEach((m, i) => {
    if (isQ(m) && Number(m.ts) === ts && (best < 0 || Math.abs(i - at) < Math.abs(best - at))) best = i;
  });
  return best;
}

/**
 * Was `thing` (a visual, or a page's version: its `at`) made by the turn of `answer`, the version shown of
 * `fork` (none: it has no other)? Retry and Edit keep every answer (chat/chats/branches.rs): a retried one
 * shows only what its own turn made — after the version before it (`since`), by the time it was saved.
 */
function ownVersion(thing, answer, fork) {
  if (!fork) return true;
  const made = Math.floor((Number(thing?.at) || 0) / 1000);
  if (!made) return true;
  const saved = Number(answer?.ts) || 0;
  return made > (Number(fork.since) || 0) && (!saved || made <= saved);
}

/** The answer to the question at `q`: its index, or `q` itself while there's none yet. Pure. */
function answerOf(messages, q) {
  for (let i = q + 1; i < messages.length; i++) {
    const m = messages[i];
    if (m.role === 'assistant') return { at: i, inAnswer: true };
    if (m.role === 'user' && !m.kind) break;
  }
  return { at: q, inAnswer: false };
}

/**
 * **Where each thing made shows** in `messages`: in the first answer after its question (`inAnswer`, above
 * the answer's words), or — no answer yet (a Code chat's Claude still working) — under the question's
 * own work. A visual by the question it was made for; a page's card once for each version, by the
 * question whose answer saved it. `forks`: the chat's versions (chat_get's `forks`,
 * chat/chats/branches.rs): an answer shows only what its own version's turn made ([`ownVersion`]).
 * `Map<index, { inAnswer, visuals, pages: [{page, version}] }>`, each in the order made. Pure.
 */
export function placeMade(messages, items, forks = []) {
  const out = new Map();
  if (!items?.length || !messages?.length) return out;
  const place = (q, thing, add) => {
    const { at, inAnswer } = answerOf(messages, q);
    if (inAnswer && !ownVersion(thing, messages[at], (forks || []).find((f) => f.at === at))) return;
    if (!out.has(at)) out.set(at, { inAnswer, visuals: [], pages: [] });
    add(out.get(at));
  };
  for (const v of items) {
    if (v?.kind === 'visual') {
      const q = questionIndex(messages, v.after);
      if (q >= 0) place(q, v, (s) => s.visuals.push(v));
    } else if (v?.kind === 'page') {
      for (const version of v.versions || []) {
        const q = questionIndex(messages, version.after);
        if (q >= 0) place(q, version, (s) => s.pages.push({ page: v, version }));
      }
    }
  }
  return out;
}

/** [`placeMade`]'s visuals alone (`Map<index, {inAnswer, visuals}>`). Pure. */
export function placeVisuals(messages, items, forks = []) {
  return new Map([...placeMade(messages, items, forks)].filter(([, s]) => s.visuals.length));
}
