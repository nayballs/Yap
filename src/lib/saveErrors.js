// A save Yap couldn't make (src-tauri/src/saving.rs): a command's error
// "Couldn't save your note: the disk is full." The change itself happened —
// Yap keeps it in memory and saves it with the next change — so the caller
// carries on as if it worked, and the person sees one toast that says so,
// with Try again (`saves_retry`: every unsaved store written again).
//
//   try { await invoke('note_update', …) } catch (e) { if (!showSaveError(e)) … }
//
// A save that failed by itself (history after a dictation, memory learning)
// is said by Rust, once, through the notice policy (an alert, alerts.js).
import { invoke } from '@tauri-apps/api/core';
import { toast } from './ui/toast.svelte.js';

const START = "Couldn't save ";

/** Whether `e` is a failed save (the change is kept in memory). */
export function isSaveError(e) {
  return String(e ?? '').startsWith(START);
}

/**
 * Show a failed save with Try again; false when `e` is some other error
 * (the caller's to show). `onSaved` runs once Try again worked.
 */
export function showSaveError(e, { onSaved = null, again = false } = {}) {
  const message = String(e ?? '');
  if (!message.startsWith(START)) return false;
  const cut = message.indexOf(': ');
  const what = cut > 0 ? message.slice(0, cut) : message.replace(/\.$/, '');
  const why = cut > 0 ? message.slice(cut + 2) : '';
  toast({
    kind: 'error',
    key: 'save-failed',
    chip: 'Saving',
    icon: 'save',
    title: again ? what.replace(/^Couldn't/, "Still couldn't") : what,
    description: `${why.charAt(0).toUpperCase()}${why.slice(1)} Yap is keeping your changes and will try again with the next one.`.trim(),
    duration: 20_000,
    action: { label: 'Try again', onClick: () => retry(onSaved) },
  });
  return true;
}

async function retry(onSaved) {
  try {
    await invoke('saves_retry');
    toast({ kind: 'done', variant: 'success', title: 'Saved', key: 'save-retried' });
    onSaved?.();
  } catch (e) {
    if (!showSaveError(e, { onSaved, again: true })) {
      toast({ title: "Couldn't save", description: String(e), variant: 'destructive' });
    }
  }
}
