// Kokoro, the natural voice of spoken replies (tts.rs, tts/kokoro.rs,
// tts/registry.rs): whether this build has it, whether it's installed, a
// download in progress, and the actions. Shared by Settings → Voice Assistant's
// engine row and its "Download Kokoro" offer.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export const kokoro = $state({
  loaded: false,
  inBuild: false,
  installed: false,
  espeak: false,
  downloadBytes: 0,
  sizeBytes: 0,
  downloading: null, // { percent, downloadedBytes, totalBytes }
  error: '',
});

let started = false;

export async function refreshKokoro() {
  try {
    const s = await invoke('tts_kokoro_status');
    Object.assign(kokoro, s.status || {});
    kokoro.downloading = s.downloading || null;
    kokoro.loaded = true;
  } catch {
    /* an older backend: no Kokoro row */
  }
}

/** Load once and follow downloads (Settings is always mounted). */
export function startKokoro() {
  if (started) return;
  started = true;
  refreshKokoro();
  listen('tts-download-progress', (e) => {
    if (e.payload) kokoro.downloading = e.payload;
  });
  listen('tts-kokoro-changed', () => refreshKokoro());
}

/** Download Kokoro (and eSpeak NG). True when it's installed. */
export async function downloadKokoro() {
  if (kokoro.downloading) return false;
  kokoro.error = '';
  kokoro.downloading = { percent: 0, downloadedBytes: 0, totalBytes: kokoro.downloadBytes };
  try {
    await invoke('tts_kokoro_download');
    return true;
  } catch (e) {
    kokoro.error = `Couldn't download Kokoro: ${e}`;
    return false;
  } finally {
    kokoro.downloading = null;
    await refreshKokoro();
  }
}

export async function deleteKokoro() {
  kokoro.error = '';
  try {
    await invoke('tts_kokoro_delete');
    return true;
  } catch (e) {
    kokoro.error = `Couldn't delete it: ${e}`;
    return false;
  } finally {
    await refreshKokoro();
  }
}

export function formatMb(bytes) {
  const mb = bytes / 1_000_000;
  return mb >= 1000 ? `${(mb / 1000).toFixed(1)} GB` : `${Math.max(1, Math.round(mb))} MB`;
}

/**
 * Kokoro's voices as Select options, grouped by accent and gender: British
 * English, American English, then the other languages (the language in the
 * label). The default is marked.
 */
export function kokoroOptions(voices, defaultVoice) {
  const gender = (v) => (v.gender === 'male' ? 'Male' : 'Female');
  const order = (v) => (v.accent === 'British English' ? 0 : v.accent === 'American English' ? 1 : 2);
  const sorted = [...voices].sort(
    (a, b) => order(a) - order(b) || gender(a).localeCompare(gender(b)) || a.accent.localeCompare(b.accent) || a.name.localeCompare(b.name)
  );
  return sorted.map((v) => {
    const english = order(v) < 2;
    const group = english ? `${v.accent} · ${gender(v)}` : `Other languages · ${gender(v)}`;
    let label = english ? v.name : `${v.name} · ${v.accent}`;
    if (v.id === defaultVoice) label += ' (default)';
    return { value: v.id, label, group };
  });
}
