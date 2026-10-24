// The providers' live model lists in this window (model_catalog.rs): one
// shared store for every picker. `loadCatalog()` shows what Yap has at once
// and lets Rust check any list that's a day old behind it;
// `yap-model-catalog` brings each result (also after a key is saved, and
// the hourly background check). The merging is modelCatalog.js's.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { mergeModels, defaultFrom, offers, labelFor } from './core/modelCatalog.js';

/** `providers`: provider → `{ ok, models, error, fetchedAt, checkedAt, checking }`, keyed providers only. */
export const catalog = $state({ providers: {}, now: 0, loaded: false });

let listening = null;
let rev = 0;

function apply(snapshot) {
  if (!snapshot?.providers) return;
  // A command's reply can land after a newer event (the check it started
  // finishing first): never go back to an older snapshot.
  if ((snapshot.rev ?? 0) < rev) return;
  rev = snapshot.rev ?? 0;
  catalog.providers = snapshot.providers;
  catalog.now = snapshot.now ?? 0;
  catalog.loaded = true;
}

/** Read the lists (a picker opening); a stale one is checked behind it. */
export async function loadCatalog() {
  // Listening first: the check this starts can finish before a reply.
  if (!listening) listening = listen('yap-model-catalog', (e) => apply(e.payload)).catch(() => null);
  await listening;
  try {
    apply(await invoke('model_catalog'));
  } catch {
    /* the curated lists show */
  }
}

/** Check one provider's list now (Settings' "Check now"). */
export async function checkCatalog(provider) {
  const snapshot = await invoke('model_catalog_refresh', { provider });
  apply(snapshot);
  return snapshot;
}

/** The merged list for `provider` (see modelCatalog.js), `current` = the saved model. */
export function modelsFor(provider, current = '') {
  return mergeModels(provider, catalog.providers[provider], { current });
}

/** The model to select when switching to `provider`. */
export function defaultModel(provider) {
  return defaultFrom(modelsFor(provider), provider);
}

/** Does `provider` offer `id` right now (or, without a live list, does Yap's list have it)? */
export function providerOffers(provider, id) {
  return offers(modelsFor(provider, id), id);
}

/** A model's name: curated label, the provider's own name, else the id. */
export function modelLabel(provider, id) {
  return labelFor(provider, id, catalog.providers[provider]);
}
