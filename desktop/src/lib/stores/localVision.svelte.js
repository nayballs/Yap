// "Read screenshots on this PC" (Snip to Chat, phase 3, 2026-10-28;
// src-tauri/src/ai/local_llm/vision.rs): the on-device model's projector,
// downloaded from Settings → Language Models or from Chat's "can't read
// images" line, its progress shown in both (a store, so the download
// carries on with its progress when you leave either), switched on or off,
// removed. Rust says what's there (`local_llm_status().vision`,
// `ai_model_status().imagesLocal`); this keeps only the download under way.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { cancelDownload, isCancelled } from '../core/downloads.js';

export const localVision = $state({
  // The curated model whose projector is downloading ('' = none).
  downloading: '',
  // `local-vision-download-progress`'s payload (core/downloads.js `fromEvent`).
  progress: null,
  error: '',
});

let progressListener = null;
function listenProgress() {
  if (!progressListener) {
    progressListener = listen('local-vision-download-progress', (e) => {
      if (e.payload && localVision.downloading) localVision.progress = e.payload;
    }).catch(() => null);
  }
  return progressListener;
}

/**
 * Download the active on-device model's projector (and turn it on). True
 * when it's on this PC; a cancel is quiet.
 */
export async function downloadVision(model) {
  if (localVision.downloading) return false;
  localVision.downloading = model || 'active';
  localVision.progress = { percent: 0, downloaded_mb: 0, total_mb: 0 };
  localVision.error = '';
  try {
    await listenProgress();
    await invoke('local_vision_install');
    return true;
  } catch (e) {
    if (!isCancelled(e)) localVision.error = String(e);
    return false;
  } finally {
    localVision.downloading = '';
    localVision.progress = null;
  }
}

/** Cancel it (its half-downloaded file goes). */
export function cancelVision(model) {
  return cancelDownload(`llm-vision:${model}`);
}

/** Remove it: the file goes (a model running with it is freed first). */
export async function removeVision(model) {
  localVision.error = '';
  try {
    await invoke('local_vision_remove', { model: model || null });
    return true;
  } catch (e) {
    localVision.error = String(e);
    return false;
  }
}

/** "Read screenshots on this PC" on or off (saved the way Settings saves). */
export async function setVisionOn(on) {
  localVision.error = '';
  try {
    await invoke('local_vision_set', { on });
    return true;
  } catch (e) {
    localVision.error = String(e);
    return false;
  }
}
