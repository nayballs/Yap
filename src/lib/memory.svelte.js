// Memory (src-tauri/src/memory.rs) in the main window: the shared store
// Settings → Memory draws (`memory.items`, whether an AI model is set up),
// kept current from `yap-memory-changed`, and the quiet toast when Yap
// remembered something by itself (`yap-memory-learned`: "Yap remembered:
// your sister is Rochelle", Undo, Manage). Never a Windows notification:
// Rust sends the toast's event only while the main window is on screen.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { toast } from './ui/toast.svelte.js';

export const memory = $state({ items: [], ai: { ready: true }, loaded: false });

let started = false;
let openSettings = () => {};

/** Re-read memory (after any change, or when the section opens). */
export async function loadMemory() {
  try {
    const r = await invoke('memory_list');
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
    toast({ title: "Couldn't undo that", description: String(err), variant: 'destructive' });
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
  listen('yap-memory-changed', () => loadMemory()).catch(() => {});
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
