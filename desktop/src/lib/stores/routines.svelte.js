// Routines for Settings → Skills (desktop/src-tauri/src/chat/routines.rs):
// the list, the recent runs and today's count, read again on
// `yap-routines-changed` (a save, a switch, a run starting or ending), and
// the calls that change them. Pure wording is core/routines.js.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

/** `routines`, `runs` (newest first), `today` / `cap`, `max`; `loaded`: read at least once. */
export const routines = $state({ routines: [], runs: [], today: 0, cap: 20, max: 20, loaded: false });

let started = false;

/** Read them (again). */
export async function refreshRoutines() {
  try {
    const l = await invoke('routines_list');
    routines.routines = l?.routines || [];
    routines.runs = l?.runs || [];
    routines.today = l?.today ?? 0;
    routines.cap = l?.cap ?? 20;
    routines.max = l?.max ?? 20;
    routines.loaded = true;
  } catch {
    /* keep what's there */
  }
}

/** Load them once per window and follow Rust's changes. */
export function initRoutines() {
  if (started) return;
  started = true;
  refreshRoutines();
  listen('yap-routines-changed', () => refreshRoutines()).catch(() => {});
}

/** Save one (new, or a change); the list follows. Throws Rust's words. */
export async function saveRoutine(routine) {
  const saved = await invoke('routine_save', { routine });
  await refreshRoutines();
  return saved;
}

export async function deleteRoutine(id) {
  await invoke('routine_delete', { id });
  await refreshRoutines();
}

/** On or off: at once here, then saved (an error reads the list again and is thrown). */
export async function setRoutineOn(id, on) {
  const r = routines.routines.find((x) => x.id === id);
  if (r) r.on = on;
  try {
    await invoke('routine_set_on', { id, on });
  } catch (e) {
    await refreshRoutines();
    throw e;
  } finally {
    refreshRoutines();
  }
}

/** Run now: queued behind any run going (the runs list shows it at once). */
export async function runNow(id) {
  await invoke('routine_run_now', { id });
  await refreshRoutines();
}

/** A run's result: the main window on its chat, or on the meeting's note. */
export async function openResult({ chat = null, note = null }) {
  await invoke('routine_open', { chat, note });
}
