// Shared reactive model state so the ModelManager (Speech-to-Text) and the
// tray and Settings' attention badge stay in sync.
import { invoke } from '@tauri-apps/api/core';

export const modelStore = $state({
  installed: [], // model ids on disk
  active: null, // currently active / loaded model id
  loaded: false, // first refresh completed (gates "no model" attention badge)
});

export async function refreshModels() {
  try {
    modelStore.installed = await invoke('installed_models');
    const cfg = await invoke('get_config');
    if (cfg) modelStore.active = cfg.modelSize ?? null;
    modelStore.loaded = true;
  } catch {
    /* best-effort */
  }
}

export async function setActiveModel(id) {
  await invoke('set_active_model', { modelSize: id });
  modelStore.active = id;
  await refreshModels();
}
