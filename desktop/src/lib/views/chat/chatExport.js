// **Export…** (a conversation's ⋯, 15 Nov 2026; chat/export.rs): the chat as
// a Markdown file where the person picks, its pictures in a folder beside it.
// Never uploaded — Yap's version of the Claude app's Share.
import { invoke } from '@tauri-apps/api/core';
import { toast } from '../../ui/toast.svelte.js';

/** A file name for a chat's title: Windows' forbidden characters out. Pure. */
export function exportName(title) {
  const t = String(title || '')
    .replace(/[\\/:*?"<>|\u0000-\u001f]/g, ' ')
    .replace(/\s+/g, ' ')
    .trim()
    .replace(/^[.\s]+|[.\s]+$/g, '')
    .slice(0, 80)
    .trim();
  return t || 'Chat';
}

/** What the toast says once it's written. Pure. */
export function exportedLine(result) {
  const n = result?.pictures || 0;
  if (!n) return result?.path || '';
  return `${result.path} · ${n} picture${n === 1 ? '' : 's'} in “${result.folder}”`;
}

/** Ask where, then write it: `conv` is `{ id, title }`. */
export async function exportConversation(conv) {
  if (conv?.id == null) return;
  try {
    const { save } = await import('@tauri-apps/plugin-dialog');
    const path = await save({ defaultPath: `${exportName(conv.title)}.md`, filters: [{ name: 'Markdown', extensions: ['md'] }] });
    if (!path) return;
    const result = await invoke('chat_export', { id: conv.id, path });
    toast({ title: 'Chat exported', description: exportedLine(result), variant: 'success' });
  } catch (e) {
    toast({ title: "Couldn't export the chat", description: String(e), variant: 'destructive' });
  }
}
