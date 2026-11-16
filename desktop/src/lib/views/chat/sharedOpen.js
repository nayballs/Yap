// **Opening a file shared into the conversation** (16 Nov 2026: a file chip's click, its ⋯ and right-click menu; a
// shared file's Outputs row): beside the chat, in the view that suits it.
//
// - **A file in a Code chat's workspace** opens where its links open (fileOpen.svelte.js `openFileLink`: Settings →
//   Yap Code → "Open file links in": Yap's file view, a workbook as a sheet, an editor at the line, Windows' default
//   app — Rust decides, from the conversation's own workspace, `code_files_check`), and its menu is a file link's:
//   Open in Yap, Open in <editor>, Show in folder, Copy path.
// - **Any other shared file** (a plain chat's, or one outside the workspace): a spreadsheet in the sheet view (read
//   only: `Src::Shared`, chat/sheets/view.rs), a text, Markdown or code file in the file view (`chat_file_read`,
//   chat/shares.rs: only a file shared in that conversation), anything else — a PDF, a Word file — in its own app
//   (`chat_file_open`: a Markdown file in Yap's own window, never one that would run a program). Its menu: Open in
//   Yap (when Yap has a view of it), Open in its app, Show in folder, Copy path.
import { invoke } from '@tauri-apps/api/core';
import { toast } from '../../ui/toast.svelte.js';
import { copyText } from '../../core/itemMenus.js';
import { devInspect, openContextMenu } from '../../ui/contextMenu.svelte.js';
import { fileLinkMenuItems, loadEditors, openFileLink } from './fileOpen.svelte.js';
import { isSheetName } from './sheetView.js';

/** The context key Chat sets (ChatView.svelte): `{ conversation, code, open(item), menu(e, item, el), refOf(item) }`. */
export const SHARED_FILES = 'yap-shared-files';

const TEXT_KINDS = new Set(['markdown', 'text', 'code']);

/** **Yap's own view of a shared file** outside a workspace: "sheet", "file", or null (none: its own app). Pure. */
export function yapViewOf(item) {
  if (!item?.path || item.missing) return null;
  if (isSheetName(item.name || item.path)) return 'sheet';
  if (TEXT_KINDS.has(item.kind)) return 'file';
  return null;
}

/** What its own app is, as the menu says it ("Open in its own window": a Markdown file's, Yap's; '' for none). Pure. */
export function appLabel(item) {
  if (!item?.path || item.missing || !item.opens) return '';
  return item.opens === 'yap' ? 'Open in its own window' : 'Open in its app';
}

/** `${conversation}\n${path}` → the workspace's file it is (`{rel, abs}`) or null, as Rust said. */
const refs = new Map();

/** **The Code chat's workspace file a shared path is**, or null (not a Code chat, outside it): Rust says, kept. */
export async function workspaceRef(conversation, path, code = true) {
  if (!code || conversation == null || !path) return null;
  const key = `${conversation}\n${path}`;
  if (refs.has(key)) return refs.get(key);
  let ref = null;
  try {
    [ref = null] = await invoke('code_files_check', { conversation, paths: [path] });
  } catch {
    ref = null;
  }
  refs.set(key, ref);
  return ref;
}

/** What `workspaceRef` found already (undefined: not asked yet). */
export const knownRef = (conversation, path) => refs.get(`${conversation}\n${path}`);

/** Yap's own view of `item` beside the chat (`doc`: the panel); whether it has one. */
export function showSharedInYap(item, { doc, conversation }) {
  const how = yapViewOf(item);
  if (!doc || !how) return false;
  if (how === 'sheet') doc.showSheet({ src: { from: 'shared', conversation, path: item.path }, name: item.name });
  else doc.showFile({ rel: String(item.path).replace(/\\/g, '/'), line: null, shared: true, opens: item.opens || '' });
  return true;
}

/** Its own app (`chat_file_open`). */
export async function openInApp(item, conversation) {
  if (conversation == null || !item?.path) return;
  try {
    await invoke('chat_file_open', { conversation, path: item.path });
  } catch (e) {
    toast({ title: `Couldn't open ${item.name || 'it'}`, description: String(e?.message ?? e ?? ''), variant: 'destructive' });
  }
}

/** **A chip's click**: where its links open (a workspace's file), else Yap's view beside the chat, else its app. */
export async function openShared(item, { doc, conversation, code = true }) {
  if (!item?.path || item.missing) return;
  const ref = await workspaceRef(conversation, item.path, code);
  if (ref) {
    await openFileLink({ rel: ref.rel, line: null }, { doc, conversation });
    return;
  }
  if (showSharedInYap(item, { doc, conversation })) return;
  await openInApp(item, conversation);
}

function reveal(item, conversation) {
  return invoke('chat_file_reveal', { conversation, path: item.path }).catch((e) =>
    toast({ title: "Couldn't show it in its folder", description: String(e?.message ?? e ?? ''), variant: 'destructive' }),
  );
}

/**
 * **A chip's menu**: a workspace file's is a file link's (Open in Yap, Open in <editor>, Show in folder, Copy
 * path); any other's Open in Yap (when Yap has a view of it), Open in its app, Show in folder, Copy path — a file
 * no longer there only Copy path. `ref`: `workspaceRef`'s answer.
 */
export function sharedMenuItems(item, ref, { doc, conversation }) {
  if (!item?.path) return [];
  if (ref && !item.missing) return fileLinkMenuItems({ rel: ref.rel, line: null, abs: ref.abs }, { doc, conversation });
  const items = [];
  if (!item.missing) {
    if (doc && yapViewOf(item)) items.push({ label: 'Open in Yap', icon: 'open', onselect: () => showSharedInYap(item, { doc, conversation }) });
    if (appLabel(item)) items.push({ label: appLabel(item), icon: 'open', onselect: () => openInApp(item, conversation) });
    items.push({ separator: true });
    items.push({ label: 'Show in folder', icon: 'folder', onselect: () => reveal(item, conversation) });
  }
  items.push({ label: 'Copy path', icon: 'copy', onselect: () => copyText(item.path, 'Path copied') });
  return items;
}

/**
 * **Open a chip's menu** from `e` (its ⋯, a right-click, the ContextMenu key or Shift+F10 on it; `el` gets the
 * focus back). The event is kept as it was: Rust may be asked first whether the file is in the workspace.
 */
export async function openSharedMenu(e, item, el, { doc, conversation, code = true }) {
  if (devInspect(e)) return false;
  e.preventDefault();
  e.stopPropagation();
  const kept = {
    type: e.type,
    clientX: e.clientX,
    clientY: e.clientY,
    button: e.button,
    detail: e.detail,
    key: e.key,
    shiftKey: e.shiftKey,
    target: e.target,
    currentTarget: e.currentTarget,
    preventDefault() {},
    stopPropagation() {},
  };
  const ref = await workspaceRef(conversation, item?.path, code);
  // (The editor "Open in …" names, fresh for the next time: the person may have changed Settings.)
  if (ref) loadEditors();
  return openContextMenu(kept, sharedMenuItems(item, ref, { doc, conversation }), { label: item?.name || 'File', row: el });
}
