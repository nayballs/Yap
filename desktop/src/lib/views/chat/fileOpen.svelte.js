// **Where a Code chat's file links open** (16 Nov 2026; agents/editors.rs,
// agents/files.rs `code_file_open`): Settings → Yap Code → "Open file links
// in" (`agentsOpenFiles`) — Yap's file view beside the chat (the default), a
// code editor found on this PC at the line, or Windows' default app for the
// file. Rust decides: the page sends the workspace's file (`rel`, `line`) and
// gets back where it went; Yap's own view is the page's to show (a workbook
// as a sheet), with a toast when it stands in for what was asked (an editor
// no longer on this PC, a file Windows would run rather than open).
//
// A link's right-click menu (and the ContextMenu key, Shift+F10: ui/fileLinks.js):
// Open in Yap, Open in <editor>, Show in folder, Copy path.
import { invoke } from '@tauri-apps/api/core';
import { toast } from '../../ui/toast.svelte.js';
import { copyText } from '../../core/itemMenus.js';
import { openContextMenu } from '../../ui/contextMenu.svelte.js';
import { isSheetName } from './sheetView.js';

/** What Rust found (`code_editors`): the setting, the editors on this PC, "Open in …"'s editor. */
export const editors = $state({ loaded: false, choice: 'yap', list: [], preferred: null, missing: false });

let asking = null;

/** Ask Rust again (`refresh`: look at the PC again, as Settings does). */
export function loadEditors(refresh = false) {
  if (asking && !refresh) return asking;
  const now = invoke('code_editors', { refresh })
    .then((v) => {
      Object.assign(editors, { loaded: true, choice: v?.choice || 'yap', list: v?.editors || [], preferred: v?.preferred || null, missing: !!v?.missing });
      return editors;
    })
    .catch(() => editors)
    .finally(() => {
      if (asking === now) asking = null;
    });
  asking = now;
  return now;
}

/** "Open in VS Code": the editor "Open in …" uses, else just "editor". Pure. */
export const editorLabel = (ed = editors) => `Open in ${ed?.preferred?.name || 'editor'}`;

/** Yap's own view of the workspace's file `ref`: a workbook as a sheet (SheetTab), anything else in the file view. */
export function showInYap(ref, { doc, conversation }) {
  if (isSheetName(ref?.rel)) doc.showSheet({ src: { from: 'workspace', conversation, rel: ref.rel }, name: String(ref.rel).split('/').pop() });
  else doc.showFile({ rel: ref.rel, line: ref.line ?? null });
}

/**
 * **Open a file link**: where `with` says (null: the setting; 'yap', 'editor', 'default' or an editor's id
 * from the menu). Rust's answer, or null when it couldn't.
 */
export async function openFileLink(ref, { doc, conversation, with: how = null }) {
  if (!ref?.rel) return null;
  if (how === 'yap' || conversation == null) {
    showInYap(ref, { doc, conversation });
    return { how: 'yap' };
  }
  let r;
  try {
    r = await invoke('code_file_open', { conversation, path: ref.rel, line: ref.line ?? null, with: how });
  } catch (e) {
    toast({ title: "Couldn't open it", description: String(e?.message ?? e), variant: 'destructive' });
    return null;
  }
  if (!r || r.how === 'yap') {
    showInYap(ref, { doc, conversation });
    if (r?.note) toast({ title: 'Opened in Yap instead', description: r.note });
  }
  return r;
}

/** **A file link's menu**: Open in Yap, Open in <editor>, Show in folder, Copy path. `ref`: `{rel, line, abs}`. */
export function fileLinkMenuItems(ref, { doc, conversation, ed = editors }) {
  const at = (how) => () => openFileLink(ref, { doc, conversation, with: how });
  const items = [{ label: 'Open in Yap', icon: 'open', onselect: at('yap') }];
  // (Before Rust has said, "Open in editor": its own pick.)
  if (!ed.loaded || ed.preferred) items.push({ label: editorLabel(ed), icon: 'pencil', onselect: at(ed.preferred?.id || 'editor') });
  items.push({ separator: true });
  items.push({
    label: 'Show in folder',
    icon: 'folder',
    onselect: () =>
      invoke('code_file_reveal', { conversation, path: ref.rel }).catch((e) =>
        toast({ title: "Couldn't show it in its folder", description: String(e?.message ?? e), variant: 'destructive' }),
      ),
  });
  items.push({ label: 'Copy path', icon: 'copy', onselect: () => copyText(ref.abs || ref.rel, 'Path copied') });
  return items;
}

/** Open a file link's menu from `e` (a right-click, or the key on the focused link `el`). */
export function openFileLinkMenu(e, ref, el, ctx) {
  const opened = openContextMenu(e, fileLinkMenuItems(ref, ctx), { label: ref.rel, row: el });
  // (The menu's editor, fresh for the next time: the person may have changed Settings.)
  loadEditors();
  return opened;
}
