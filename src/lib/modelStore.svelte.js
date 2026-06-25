// Shared reactive model state so the ModelManager (Models section) and the
// bottom StatusBar selector stay in sync — switching in one updates the other.
import { invoke } from '@tauri-apps/api/core';

export const modelStore = $state({
  installed: [], // model ids on disk
  active: null, // currently active / loaded model id
});

export async function refreshModels() {
  try {
    modelStore.installed = await invoke('installed_models');
    const cfg = await invoke('get_config');
    if (cfg) modelStore.active = cfg.modelSize ?? null;
  } catch {
    /* best-effort */
  }
}

export async function setActiveModel(id) {
  await invoke('set_active_model', { modelSize: id });
  modelStore.active = id;
  await refreshModels();
}
