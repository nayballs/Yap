// A note's Ask threads, kept with it (src-tauri/src/chat/note_chats.rs): the
// notepad's Ask sheet and the Notes view's "Ask anything…" bar show the
// meeting's newest thread when the note opens, keep each turn as it's
// shown (`keepTurn`), follow each other through `yap-note-chat`, and list
// the earlier threads under Past chats (`pastChatItems`). Deleting the
// note deletes them.
import { invoke } from '@tauri-apps/api/core';

/** `{ current, threads }`: the note's newest thread (whole) and every thread, newest first. */
export const loadNoteChat = (noteId) => invoke('note_chat', { noteId });

/** One of the note's threads, whole. */
export const openNoteChat = (noteId, conversationId) => invoke('note_chat_open', { noteId, conversationId });

/**
 * Keep a turn the window just showed: `messages` = `[{ role, text, plain?,
 * stopped?, model? }]`, in thread `conversationId` (null: a new one).
 * Resolves to the thread's id.
 */
export const keepTurn = (noteId, conversationId, messages, origin) =>
  invoke('note_chat_add', { noteId, conversationId, messages, origin });

/** A kept thread's messages as a window shows them (`prefix` makes their ids). Pure. */
export function threadMessages(conversation, prefix = '') {
  return (conversation?.messages || []).map((m, i) => ({
    id: `${prefix}${conversation.id}-${i}`,
    role: m.role === 'assistant' ? 'assistant' : 'user',
    text: m.text,
    plain: !!m.plain,
    stopped: !!m.stopped,
    model: m.model ?? null,
    // The meeting's passages it cites (its pills open the moment).
    sources: m.sources || [],
  }));
}

/** A message as `keepTurn` keeps it (what the window showed). Pure. */
export function keptMessage(m) {
  const out = { role: m.role === 'assistant' ? 'assistant' : 'user', text: String(m.text ?? '') };
  if (m.plain) out.plain = true;
  if (m.stopped) out.stopped = true;
  if (m.model) out.model = m.model;
  if (m.role === 'assistant' && m.sources?.length) out.sources = m.sources;
  return out;
}

/** Rename one of a note's threads (Past chats → Rename this chat…); resolves to the title kept. */
export const renameNoteChat = (noteId, conversationId, title, origin) =>
  invoke('note_chat_rename', { noteId, conversationId, title, origin });

/** "Today, 14:05" / "Mon 20 Oct, 09:12": when a thread was last added to. Pure. */
export function threadWhen(updatedTs, now = Date.now()) {
  const d = new Date((updatedTs || 0) * 1000);
  const time = d.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' });
  const today = new Date(now).toDateString() === d.toDateString();
  return today ? `Today, ${time}` : `${d.toLocaleDateString(undefined, { weekday: 'short', day: 'numeric', month: 'short' })}, ${time}`;
}

/**
 * The Past chats menu's items (ui/contextMenu.svelte.js): every thread of
 * the note, newest first, the one shown ticked; `open(id)` shows one.
 */
export function pastChatItems(threads, shownId, open, rename = null) {
  const items = (threads || []).map((t) => ({
    label: `${t.title} · ${threadWhen(t.updatedTs)}`,
    toggle: true,
    checked: t.id === shownId,
    onselect: () => open(t.id),
  }));
  // The thread shown, renamed (`rename(id)`: the window asks for the name).
  if (rename && shownId != null && (threads || []).some((t) => t.id === shownId)) {
    items.push({ separator: true }, { label: 'Rename this chat…', icon: 'pencil', onselect: () => rename(shownId) });
  }
  return items;
}
