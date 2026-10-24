// Settings "needs attention" badge (Wispr-style red count on the Settings
// cog + per-section rows). Settings keeps it live (`followAttention`) — it
// is always mounted in the main window, so the state is live even while the
// modal is closed — and ControlPanel, Home and the Settings nav render it.
//
// Each item: { section: <Settings section id>, label: <short human reason> }.
import { listen } from '@tauri-apps/api/event';
import { onMount } from 'svelte';
import { updates } from './updates.svelte.js';
import { modelStore, refreshModels } from './modelStore.svelte.js';
import { cleanupNeedsKey } from '../core/providers.js';

export const attention = $state({ items: [] });

export function attentionCount(section = null) {
  if (!section) return attention.items.length;
  return attention.items.filter((i) => i.section === section).length;
}

/**
 * What needs attention now. Real conditions only: an update waiting, no
 * speech-to-text model installed, or AI cleanup pointed at a cloud provider
 * with no key. `cfg` = Settings' config copy.
 */
export function attentionItems(cfg) {
  const items = [];
  if (updates.status === 'ready') {
    items.push({ section: 'about', label: `Update ${updates.version} ready to install` });
  } else if (updates.status === 'available' && (updates.portable || updates.metered)) {
    items.push({ section: 'about', label: `Yap ${updates.version} is available` });
  }
  if (modelStore.loaded && modelStore.installed.length === 0) {
    items.push({ section: 'models', label: 'No speech-to-text model installed' });
  }
  if (cleanupNeedsKey(cfg)) {
    items.push({ section: 'cleanup', label: 'AI cleanup needs an API key' });
  }
  return items;
}

/**
 * Keep `attention.items` live for the life of the component that calls it
 * (while it initialises: Settings). `getCfg()` = Settings' config copy once
 * it's set up, else null.
 *
 * The speech models on disk, for the "no model" item (and Home's first-step
 * card), are read at once — not only when the Speech-to-Text page first
 * opens: a new install never showed the badge until then — and again when
 * the pipeline says it needs one or has one (onboarding's download happens
 * in its own window).
 */
export function followAttention(getCfg) {
  onMount(() => {
    refreshModels();
    const un = listen('yap-state', (e) => {
      if (e.payload === 'needs-model' || (e.payload === 'idle' && modelStore.installed.length === 0)) {
        refreshModels();
      }
    });
    return () => un.then((f) => f());
  });
  $effect(() => {
    const cfg = getCfg();
    if (!cfg) return;
    attention.items = attentionItems(cfg);
  });
}
