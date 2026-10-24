// Settings' copy of the config, in step with config.json (2026-10-23; out
// of Settings.svelte 2026-10-24). The pure merge is configMerge.js; the
// defaults and the saved payload are configDefaults.js.
//
// This copy is read once, at Settings' mount (`readConfig`); other surfaces
// save config.json too — the Speech-to-Text page's model switch
// (modelStore), the tray's model menu, onboarding, a notice's button — and
// Rust sends `yap-config-saved` after every save. This copy's auto-save
// used to write the whole copy back, so the next switch flipped in Settings
// put the old speech model (and onboarding's choices) back. `base` is
// config.json as this copy last agreed with it (read, or saved). A save
// from anywhere: what changed on disk since, and not here, is adopted
// (`adoptSaved`). The auto-save (`persist`): this copy's changes put onto
// config.json as it is now (`merge3`), so a setting saved elsewhere since
// stays — and nothing is written when this copy has nothing to add. An
// edit being made is never undone: a setting changed both here and there
// keeps this copy's. One at a time (`inTurn`).
//
// Settings is mounted once, in the main window, for the window's life: one
// copy per window, so the state is this module's. Pages get `cfg` as a
// prop and change it in place; the auto-save does the saving.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { onMount } from 'svelte';
import { merge3, same, copy, fill } from './configMerge.js';
import { FIELD_DEFAULTS, cleanConfig } from './configDefaults.js';

/**
 * `cfg`: the copy (null until read). `loaded`: set up (the auto-save and the
 * effects that follow the copy wait for it). `saved`: the nav's "Saved ✓"
 * pulse after a write.
 */
export const config = $state({ cfg: null, loaded: false, saved: false });

let base = null;
// This copy as first set up (defaults and fill-ins included): a setting
// config.json doesn't have yet, unchanged here since, is no change of this
// copy's (`fill`).
let pristine = null;
let syncing = Promise.resolve();
// Settings' reaction to values taken in from outside (`syncConfig`).
let adopted = () => {};

/** Run `fn` after every merge before it (a read-merge-write never interleaves). */
export function inTurn(fn) {
  const run = syncing.then(fn, fn);
  syncing = run.catch(() => {});
  return run;
}

/** Read config.json (Settings' mount): the copy, over the defaults. */
export async function readConfig() {
  const stored = await invoke('get_config');
  base = copy(stored);
  config.cfg = { ...FIELD_DEFAULTS, ...stored };
  return config.cfg;
}

/** The copy as set up, before any change worth saving (`fill`'s source). */
export function setPristine() {
  pristine = cleanConfig(config.cfg);
}

// `next` (the merge) into this copy where it differs from `mine` (this copy
// as it was merged) — unless the person changed that setting since.
function adopt(mine, next) {
  const cfg = config.cfg;
  const now = cleanConfig(cfg);
  const taken = [];
  for (const k of Object.keys(next)) {
    if (same(next[k], mine[k]) || !same(now[k], mine[k])) continue;
    cfg[k] = copy(next[k]);
    taken.push(k);
  }
  adopted(taken);
}

async function adoptSaved() {
  if (!config.loaded || !config.cfg || !base) return;
  const disk = await invoke('get_config');
  const mine = cleanConfig(config.cfg);
  adopt(mine, merge3(fill(base, pristine), mine, disk));
  base = disk;
}

/** Save this copy's changes onto config.json as it is now (the auto-save; a page that must save first). */
export function persist() {
  return inTurn(async () => {
    const mine = cleanConfig(config.cfg);
    const disk = await invoke('get_config');
    const next = merge3(fill(base, pristine), mine, disk);
    adopt(mine, next);
    if (same(next, disk)) {
      base = disk;
      return;
    }
    await invoke('save_config', { cfg: next });
    base = copy(next);
    config.saved = true;
    setTimeout(() => (config.saved = false), 1200);
  });
}

/**
 * Keep the copy in step for the life of the component that calls it (while
 * it initialises: Settings): adopt what other surfaces save, and auto-save
 * 400 ms after any change (not the first run: opening Settings saves
 * nothing). `onAdopted(keys)` runs after outside values were taken in.
 */
export function syncConfig({ onAdopted = () => {} } = {}) {
  config.cfg = null;
  config.loaded = false;
  config.saved = false;
  base = null;
  pristine = null;
  adopted = onAdopted;

  onMount(() => {
    const un = listen('yap-config-saved', () => {
      inTurn(adoptSaved).catch(() => {});
    });
    return () => un.then((f) => f());
  });

  // ---- Auto-save: debounce any change, then persist ----
  let saveTimer = null;
  let primed = false; // skip the first run (initial load) so we don't save on open
  $effect(() => {
    if (!config.loaded || !config.cfg) return;
    // Deep-read every field so the effect re-runs on any change (incl. dictionary).
    JSON.stringify(config.cfg);
    if (!primed) {
      primed = true;
      return;
    }
    clearTimeout(saveTimer);
    saveTimer = setTimeout(persist, 400);
  });
}
