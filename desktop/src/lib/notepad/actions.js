// What every part of the meeting notepad does the same way: its window's
// label (the `origin` of its saves and stops), copying (one toast if the
// clipboard is busy) and Yap's main window (a Settings section, or this
// note in the Notes view).
import { invoke } from '@tauri-apps/api/core';
import { emit } from '@tauri-apps/api/event';
import { toast } from '../ui/toast.svelte.js';

/** This window's label: the `origin` of its saves and stops. */
export const LABEL = 'notepad';

/** Copy `text`: true once it's on the clipboard, else a toast says why. */
export async function copyToClipboard(text) {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    toast({ title: "Couldn't copy", description: 'The clipboard is busy. Try again.', variant: 'destructive' });
    return false;
  }
}

/** Show Yap's main window at a Settings section ('cleanup' = Language Models). */
export async function openSettings(section) {
  await invoke('open_settings').catch(() => {});
  emit('yap-open-settings', section).catch(() => {});
}

/** Back: the note in the main window's Notes view. */
export async function openNoteInYap(noteId) {
  await invoke('open_settings').catch(() => {});
  emit('yap-meeting-open-note', { noteId, stop: false }).catch(() => {});
}
