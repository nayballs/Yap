// Your mouse's side buttons (dictation/side_buttons.rs): what they do now —
// the Shortcuts page's row, from Rust (`side_buttons_state`, read again after
// every save) — and setting them up, Undo and Remove, each saved by Rust in
// one step (the hook's bindings with it) and taken into Settings' copy as
// any outside save is (`yap-config-saved`). Settings' own changes are
// saved first (`persist`), so the plan sees them.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { toast } from '../ui/toast.svelte.js';
import { persist } from './configSync.svelte.js';

export const BACK = 4;
export const FORWARD = 5;

/** A side button's name ("Back", "Forward"). */
export function sideButtonName(button) {
  return button === BACK ? 'Back' : button === FORWARD ? 'Forward' : 'That button';
}

/** `state`: `{ dictate, send, hold, summary, browser, undo }` (side_buttons.rs `State`), null until read. */
export const sideButtons = $state({ state: null });

export async function refreshSideButtons() {
  try {
    sideButtons.state = await invoke('side_buttons_state');
  } catch {
    // (A build without it: the row stays as it was.)
  }
}

let following = false;
/** Keep the row current for the window's life (idempotent). */
export function followSideButtons() {
  if (following) return;
  following = true;
  refreshSideButtons();
  listen('yap-config-saved', () => refreshSideButtons()).catch(() => {});
}

async function run(cmd, args) {
  await persist().catch(() => {});
  const state = await invoke(cmd, args);
  sideButtons.state = state;
  return state;
}

/** Set them up (`choice`: `{ dictate, send, hold, take }`), with Undo on the toast. */
export async function applySideButtons(choice) {
  const state = await run('side_buttons_apply', { choice });
  toast({
    key: 'side-buttons',
    title: 'Side buttons set up',
    description: state.summary || 'Nothing changed',
    variant: 'success',
    action: state.undo ? { label: 'Undo', onClick: () => undoSideButtons() } : null,
  });
  return state;
}

/** Undo the last setup or removal. */
export async function undoSideButtons() {
  try {
    await run('side_buttons_undo');
    toast({ key: 'side-buttons', title: 'Side buttons put back as they were', variant: 'success' });
  } catch (e) {
    toast({ title: "Couldn't undo that", description: String(e), variant: 'destructive' });
  }
}

/** Take the side buttons off Dictate and Send, with Undo on the toast. */
export async function removeSideButtons() {
  try {
    const state = await run('side_buttons_remove');
    toast({
      key: 'side-buttons',
      title: 'Side buttons removed',
      description: 'Back and Forward go back and forward again.',
      variant: 'success',
      action: state.undo ? { label: 'Undo', onClick: () => undoSideButtons() } : null,
    });
  } catch (e) {
    toast({ title: "Couldn't remove them", description: String(e), variant: 'destructive' });
  }
}
