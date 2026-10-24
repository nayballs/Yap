// Memory (src-tauri/src/memory.rs) in the main window: the shared store
// Settings → Memory draws (`memory.items`, whether an AI model is set up),
// kept current from `yap-memory-changed`, and the quiet toast when Yap
// remembered something by itself (`yap-memory-learned`: "Yap remembered:
// your sister is Rochelle", Undo, Manage). Never a Windows notification:
// Rust sends the toast's event only while the main window is on screen.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { toast } from '../ui/toast.svelte.js';
import { showSaveError } from '../saveErrors.js';

export const memory = $state({ items: [], ai: { ready: true }, loaded: false });

let started = false;
let openSettings = () => {};
// Loads overlap (the section opening, a save's own re-read, the change
// event) and `memory_list` takes a variable time (each name's dictation
// hint asks Windows' spell checker), so an older load can answer after a
// newer one: only an answer at least as new as the one shown is applied,
// never a stale snapshot over a fresh one.
let requested = 0;
let shown = 0;

/** Re-read memory (after any change, or when the section opens). */
export async function loadMemory() {
  const mine = ++requested;
  try {
    const r = await invoke('memory_list');
    if (mine < shown) return;
    shown = mine;
    memory.items = r?.items ?? [];
    memory.ai = r?.ai ?? { ready: true };
    memory.loaded = true;
  } catch {
    /* an older backend: the section shows empty */
  }
}

/** Undo a batch of changes ("Undo" on a toast). */
export async function undoMemory(batch) {
  try {
    await invoke('memory_undo', { batch });
    toast({ title: 'Undone', variant: 'success', chip: 'Memory', icon: 'memory', duration: 3_000 });
  } catch (err) {
    // Undone, but not saved yet (saveErrors.js says so, with Try again).
    if (!showSaveError(err)) toast({ title: "Couldn't undo that", description: String(err), variant: 'destructive' });
  }
}

/** A toast for a change the person made (an apply, a delete): Undo with it. */
export function changedToast(title, batch, description = '') {
  toast({
    kind: 'info',
    title,
    description,
    chip: 'Memory',
    icon: 'memory',
    duration: 8_000,
    action: batch ? { label: 'Undo', onClick: () => undoMemory(batch) } : undefined,
  });
}

/**
 * Follow memory in the main window (idempotent). `opts.openSettings(section)`
 * opens Settings at a section ("Manage" → Settings → Memory).
 */
export function initMemory(opts = {}) {
  if (opts.openSettings) openSettings = opts.openSettings;
  if (started) return;
  started = true;
  // `listen` registers asynchronously: a change made before it's in place
  // sends an event nobody hears, so memory is read once more as soon as it
  // is (a load that started earlier may predate that change).
  listen('yap-memory-changed', () => loadMemory())
    .then(() => loadMemory())
    .catch(() => {});
  listen('yap-memory-learned', (e) => {
    const p = e.payload;
    if (!p?.title) return;
    toast({
      kind: 'info',
      title: p.title,
      description: p.description || '',
      chip: 'Memory',
      icon: 'memory',
      duration: 8_000,
      action: { label: 'Undo', onClick: () => undoMemory(p.batch) },
      secondary: { label: 'Manage', onClick: () => openSettings('memory') },
    });
  }).catch(() => {});
}
