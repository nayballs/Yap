// Speaker diarization's models (speakers/diarize.rs): what's installed, a download in
// progress, and the actions, shared by Settings → Speech-to-Text's "Speakers
// in meetings" section and the same switch in Settings → Meetings.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { cancelDownload, isCancelled } from '../core/downloads.js';

/** Cancel speaker model `id`'s download (Rust's `registry::cancel_id`). */
export function cancelDiarization(id) {
  return cancelDownload(`diarization:${id}`);
}

export const diarization = $state({
  models: [], // registry::ModelInfo rows
  loaded: false,
  downloading: null, // { id, stage, percent, downloadedBytes, totalBytes }
  inUse: false, // a meeting recording is using a model
  // Voice matching (speakers/voiceprint.rs): ready, and whether this build runs it.
  voiceMatching: false,
  voiceMatchingInBuild: false,
  error: '',
});

let started = false;

export async function refreshDiarization() {
  try {
    const s = await invoke('diarization_status');
    diarization.models = s.models || [];
    diarization.downloading = s.downloading || null;
    diarization.inUse = !!s.inUse;
    diarization.voiceMatching = !!s.voiceMatching;
    diarization.voiceMatchingInBuild = !!s.voiceMatchingInBuild;
    diarization.loaded = true;
  } catch {
    /* an older backend: the section stays empty */
  }
}

/** Load, and follow downloads (once); every page showing it asks again (files can change under it). */
export function startDiarization() {
  refreshDiarization();
  if (started) return;
  started = true;
  listen('diarization-download-progress', (e) => {
    if (e.payload) diarization.downloading = e.payload;
  });
  listen('diarization-changed', () => refreshDiarization());
}

export function modelById(id) {
  return diarization.models.find((m) => m.id === id) || null;
}

export function isInstalled(id) {
  return !!modelById(id)?.installed;
}

/** The model to offer: the chosen one if it can be installed, else the recommended. */
export function offeredModel(id) {
  const chosen = modelById(id);
  if (chosen?.available) return chosen;
  return diarization.models.find((m) => m.recommended && m.available) || null;
}

/** Download model `id` (and its runtime). True when it's installed. */
export async function downloadDiarization(id) {
  if (diarization.downloading) return false;
  const name = modelById(id)?.name || 'the speaker model';
  diarization.error = '';
  diarization.downloading = { id, stage: 'runtime', percent: 0, downloadedBytes: 0, totalBytes: 0 };
  try {
    await invoke('diarization_download', { id });
    return true;
  } catch (e) {
    // Cancelled: nothing to say (no half-downloaded file stays).
    if (!isCancelled(e)) diarization.error = `Couldn't download ${name}: ${e}`;
    return false;
  } finally {
    diarization.downloading = null;
    await refreshDiarization();
  }
}

export async function deleteDiarization(id) {
  diarization.error = '';
  try {
    await invoke('diarization_delete', { id });
    return true;
  } catch (e) {
    diarization.error = `Couldn't delete it: ${e}`;
    return false;
  } finally {
    await refreshDiarization();
  }
}

export function formatBytes(bytes) {
  const mb = bytes / 1_000_000;
  return mb >= 1000 ? `${(mb / 1000).toFixed(1)} GB` : `${Math.max(1, Math.round(mb))} MB`;
}
