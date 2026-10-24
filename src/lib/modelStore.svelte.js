// Shared reactive model state so the ModelManager (Speech-to-Text) and the
// tray and Settings' attention badge stay in sync.
import { invoke } from '@tauri-apps/api/core';
import { getConfig, installedModels } from './ipcShared.js';

export const modelStore = $state({
  installed: [], // model ids on disk
  active: null, // currently active / loaded model id
  loaded: false, // first refresh completed (gates "no model" attention badge)
});

export async function refreshModels() {
  try {
    // Both asked at once (shared with the rest of the window's start-up).
    const [installed, cfg] = await Promise.all([installedModels(), getConfig()]);
    modelStore.installed = installed;
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
