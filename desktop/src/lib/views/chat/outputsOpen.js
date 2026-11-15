// **Opening one of a chat's Outputs** (outputs.js's rows; OutputsList.svelte's `onopen`): where each kind
// lives — a kept page or a document beside the chat (KeptPageTab, the document panel), a file the work
// changed in the file view, a shared picture in its tab, a shared file in the file view when it's in the
// workspace, else as its card opens it. For the Code chat's summary (SummaryTab) and any chat's Outputs tab.
import { invoke } from '@tauri-apps/api/core';
import { toast } from '../../ui/toast.svelte.js';
import { copyText } from '../../core/itemMenus.js';

/** Open `it` (an outputs.js row) from `doc` (the panel) in chat `conversationId`. */
export async function openOutput(it, { doc, conversationId }) {
  if (it?.kind === 'page') {
    doc.showKept({ id: it.page.id, conversation: conversationId, title: it.page.title, n: 0 });
    return;
  }
  if (it?.kind === 'doc') {
    doc.show(it.id, { title: it.name });
    return;
  }
  if (it?.kind === 'changed') {
    doc.showFile({ rel: it.rel, line: null });
    return;
  }
  if (it?.kind === 'shared') {
    if (it.picture) {
      doc.showPicture({ id: it.item.id, w: it.item.w, h: it.item.h, caption: it.item.name });
      return;
    }
    // A file in the workspace: the file view; any other as its card opens it.
    try {
      const [ref] = await invoke('code_files_check', { conversation: conversationId, paths: [it.item.path] });
      if (ref) {
        doc.showFile(ref);
        return;
      }
      await invoke('chat_file_open', { conversation: conversationId, path: it.item.path });
    } catch (e) {
      toast({ title: "Couldn't open it", description: String(e?.message ?? e), variant: 'destructive' });
    }
  }
}

/**
 * **An output's right-click menu** (Yap's menus: ui/contextMenu.svelte.js): Open; for a file, Show in folder
 * and Copy path; for a file in a Code chat's workspace, Open in editor. `ref`: the workspace's file the row
 * is (`code_files_check`'s `{rel, abs}`: a changed file's, or a shared file inside the workspace), else null.
 */
export function outputMenuItems(it, { doc, conversationId, ref = null }) {
  const items = [{ label: 'Open', icon: 'open', onselect: () => openOutput(it, { doc, conversationId }) }];
  const shared = it?.kind === 'shared' && it.item?.path ? it.item.path : null;
  const path = ref?.abs || shared;
  if (!path) return items;
  items.push({ separator: true });
  items.push({
    label: 'Show in folder',
    icon: 'folder',
    onselect: () =>
      invoke(ref ? 'code_file_reveal' : 'chat_file_reveal', { conversation: conversationId, path: ref ? ref.rel : shared }).catch((e) =>
        toast({ title: "Couldn't show it in its folder", description: String(e?.message ?? e), variant: 'destructive' }),
      ),
  });
  items.push({ label: 'Copy path', icon: 'copy', onselect: () => copyText(path, 'Path copied') });
  if (ref) {
    items.push({
      label: 'Open in editor',
      icon: 'pencil',
      onselect: () =>
        invoke('code_file_open_editor', { conversation: conversationId, path: ref.rel, line: null }).catch((e) =>
          toast({ title: "Couldn't open it in an editor", description: String(e?.message ?? e), variant: 'destructive' }),
        ),
    });
  }
  return items;
}

/** The workspace path to ask `code_files_check` about for an output row: a changed file's, a shared file's; null for a page or a document. Pure. */
export const outputPath = (it) => (it?.kind === 'changed' ? it.rel : it?.kind === 'shared' ? it.item?.path || null : null);

/** **Which of `outputs` are files in chat `conversationId`'s workspace**: `{path: {rel, abs} | null}` (Rust says, from the conversation's own workspace; none outside a Code chat). */
export async function outputRefs(conversationId, outputs) {
  const paths = (outputs || []).map(outputPath).filter(Boolean);
  if (conversationId == null || !paths.length) return {};
  try {
    const found = await invoke('code_files_check', { conversation: conversationId, paths });
    return Object.fromEntries(paths.map((p, i) => [p, found?.[i] || null]));
  } catch {
    return {};
  }
}
