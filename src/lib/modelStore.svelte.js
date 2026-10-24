// Shared reactive model state so the ModelManager (Speech-to-Text) and the
// tray and Settings' attention badge stay in sync. A download lives here
// too, not in the page: leaving Speech-to-Text mid-download (another
// Settings page, a search) unmounted ModelManager with its progress, so
// coming back showed Download again, a click said "already downloading",
// and a late error was lost (code review 2026-10-23).
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getConfig, installedModels } from './ipcShared.js';

export const modelStore = $state({
  installed: [], // model ids on disk
  active: null, // currently active / loaded model id
  loaded: false, // first refresh completed (gates "no model" attention badge)
  /** The download under way: { id, percent }. */
  downloading: null,
  /** The last download that failed: { id, message }, until the next. */
  downloadError: null,
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

let following = false;

/** `stt-download-progress` into `downloading` (once). */
function followProgress() {
  if (following) return;
  following = true;
  listen('stt-download-progress', (e) => {
    const d = modelStore.downloading;
    if (d && e.payload?.modelSize === d.id) d.percent = e.payload.percent;
  }).catch(() => {});
}

/**
 * Download model `id` and make it the one in use (a newly downloaded model
 * is ready to use). One at a time; a failure is kept in `downloadError`.
 */
export async function downloadModel(id) {
  if (modelStore.downloading) return;
  followProgress();
  modelStore.downloading = { id, percent: 0 };
  modelStore.downloadError = null;
  try {
    await invoke('download_model_size', { modelSize: id });
    await refreshModels();
    if (modelStore.active !== id) await setActiveModel(id).catch(() => {});
  } catch (e) {
    modelStore.downloadError = { id, message: String(e) };
  } finally {
    modelStore.downloading = null;
  }
}
