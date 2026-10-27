// Uninstalling an on-device model (ai/local_llm/uninstall.rs), the same in
// the Chat model picker and Settings → Language Models: the menu's
// Uninstall… (off, with why, while the model is in use), Yap's confirm
// dialog with the file's real size, then `local_llm_uninstall`; a refusal
// in a toast, the dialog left open. Every list re-reads on
// `yap-local-llm-changed`.
import { invoke } from '@tauri-apps/api/core';
import { askConfirm } from '../ui/contextMenu.svelte.js';
import { toast } from '../ui/toast.svelte.js';

/** "5.4 GB", "398 MB"; '' for an empty or unknown size. */
export function fmtModelSize(mb) {
  const n = Number(mb) || 0;
  if (n <= 0) return '';
  return n >= 1024 ? `${(n / 1024).toFixed(1)} GB` : `${Math.round(n)} MB`;
}

/** Why an installed model can't go now (local_llm_status's `inUse`): the menu's note. */
export function inUseNote(inUse) {
  if (inUse === 'selected') return 'Switch to another model first';
  if (inUse === 'busy') return "It's working right now: try again in a moment";
  return '';
}

/**
 * The confirm dialog, then the uninstall. `m`: `{ name, file, sizeMb, own,
 * inUse }` — `own`, a GGUF not on Yap's download list (one the person
 * added, or one taken off the list since): named by its file, and Yap
 * can't download it again. `done` runs once it's gone (the list read
 * again); `after` says where the focus goes then.
 */
export function askUninstall(m, { returnFocus = null, after = null, done = null } = {}) {
  const what = m.own ? m.file : m.name;
  const size = fmtModelSize(m.sizeMb);
  const file = size ? `its ${size} file` : 'its file';
  askConfirm({
    title: `Uninstall ${what}?`,
    body: m.own
      ? `This deletes ${file} from this PC. It isn't on Yap's download list, so Yap can't download it again.`
      : `This deletes ${file} from this PC. You can download it again.`,
    confirmLabel: 'Uninstall',
    busyLabel: 'Uninstalling…',
    returnFocus,
    after,
    run: async () => {
      try {
        await invoke('local_llm_uninstall', { file: m.file });
      } catch (e) {
        toast({ title: `Couldn't uninstall ${what}`, description: String(e), variant: 'destructive' });
        return false;
      }
      await done?.();
      return true;
    },
  });
}

/** An installed model's Uninstall… for its right-click menu: off, with why under it, while it's in use. */
export function uninstallItems(m, opts = {}) {
  const note = inUseNote(m.inUse);
  return [
    { label: 'Uninstall…', icon: 'trash', danger: true, disabled: !!note, onselect: () => askUninstall(m, opts) },
    note ? { note } : null,
  ];
}
