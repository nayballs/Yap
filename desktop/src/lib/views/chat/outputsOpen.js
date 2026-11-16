// **Opening one of a chat's Outputs** (outputs.js's rows; OutputsList.svelte's `onopen`): where each kind
// lives — a kept page or a document beside the chat (KeptPageTab, the document panel), a shared picture in
// its tab, a file of a Code chat's workspace (one its work changed, or one it shared) where Settings → Yap
// Code's "Open file links in" says, as a file link opens (fileOpen.svelte.js `openFileLink`, 16 Nov 2026:
// Yap's file view, an editor, the default app — Rust decides, from the conversation's own workspace, and a
// file Windows would run opens in Yap), any other shared file as its card opens it. For the Code chat's
// summary (SummaryTab) and any chat's Outputs tab.
import { invoke } from '@tauri-apps/api/core';
import { toast } from '../../ui/toast.svelte.js';
import { copyText } from '../../core/itemMenus.js';
import { editorLabel, editors, openFileLink, showInYap } from './fileOpen.svelte.js';

/**
 * **The workspace's file at `path`**, as Rust finds it in Code chat `conversationId`'s own workspace
 * (`code_files_check`: `{rel, abs, line}`), or null: outside it, not a Code chat, or Rust couldn't say.
 */
async function workspaceFile(conversationId, path) {
  if (conversationId == null || !path) return null;
  try {
    const [ref] = await invoke('code_files_check', { conversation: conversationId, paths: [path] });
    return ref || null;
  } catch {
    return null;
  }
}

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
    // A file of the workspace: where its links open. One Rust doesn't find there: Yap's view, which says why.
    const ref = await workspaceFile(conversationId, it.rel);
    if (ref) await openFileLink({ rel: ref.rel, line: null }, { doc, conversation: conversationId });
    else showInYap({ rel: it.rel, line: null }, { doc, conversation: conversationId });
    return;
  }
  if (it?.kind === 'shared') {
    if (it.picture) {
      doc.showPicture({ id: it.item.id, w: it.item.w, h: it.item.h, caption: it.item.name });
      return;
    }
    // A file in the workspace: where its links open; any other as its card opens it.
    const ref = await workspaceFile(conversationId, it.item.path);
    if (ref) {
      await openFileLink({ rel: ref.rel, line: null }, { doc, conversation: conversationId });
      return;
    }
    try {
      await invoke('chat_file_open', { conversation: conversationId, path: it.item.path });
    } catch (e) {
      toast({ title: "Couldn't open it", description: String(e?.message ?? e), variant: 'destructive' });
    }
  }
}

/**
 * **An output's right-click menu** (Yap's menus: ui/contextMenu.svelte.js): Open (where its links open); for a
 * file in a Code chat's workspace whose links open elsewhere, Open in Yap too; for a file, Show in folder and
 * Copy path; for a file in the workspace, Open in <editor> (fileOpen.svelte.js: the setting's, else the first
 * found). `ref`: the workspace's file the row is (`code_files_check`'s `{rel, abs}`: a changed file's, or a
 * shared file inside the workspace), else null. `ed`: what Rust said of the editors (fileOpen.svelte.js).
 */
export function outputMenuItems(it, { doc, conversationId, ref = null, ed = editors }) {
  const items = [{ label: 'Open', icon: 'open', onselect: () => openOutput(it, { doc, conversationId }) }];
  // (Open goes to an editor or the default app: Yap's own view one click away.)
  if (ref && ed?.choice && ed.choice !== 'yap') items.push({ label: 'Open in Yap', icon: 'open', onselect: () => showInYap({ rel: ref.rel, line: null }, { doc, conversation: conversationId }) });
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
      label: editorLabel(),
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
