// Settings' on-device model (ai/local_llm.rs, the llamafile sidecar): its
// status as Language Models and the scope link rows show it, the curated
// model downloading and its progress, and what Settings does with it
// (download, use, delete, the bigger model offered once). A store, not the
// page's: Settings is always mounted and its pages aren't — a download
// carries on, with its progress, when you leave the page.
//
// (The Chat model picker keeps its own, chatModels.svelte.js `local`.)
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { PROVIDER_ICONS } from '../core/providerIcons.js';
import { cancelDownload, isCancelled } from '../core/downloads.js';
import { config, persist } from './configSync.svelte.js';

export const localLlm = $state({
  // local_llm_status. Model/engine names come from the backend (one source
  // of truth); these literals are a fallback until it answers.
  status: {
    installed: false,
    running: false,
    // A start under way (on first use, at launch, Load now).
    loading: false,
    model: 'Qwen2.5 1.5B Instruct',
    engine: 'Mozilla llamafile (llama.cpp)',
  },
  installing: false,
  progress: { stage: '', percent: 0, downloaded_mb: 0, total_mb: 0 },
  error: '',
  // The last download was cancelled (nothing to say about it).
  cancelled: false,
  // Restarting the sidecar on another model.
  switching: false,
  // The curated model downloading right now (its card shows the progress).
  downloadingId: '',
  // The family tab shown in the model browser (kept while you're elsewhere).
  family: '',
});

/** Local model families: their tab's name and brand mark, in this order (OpenWhispr's). */
export const LLM_FAMILIES = [
  { value: 'qwen', label: 'Qwen', icon: PROVIDER_ICONS.qwen },
  { value: 'llama', label: 'Meta Llama', icon: PROVIDER_ICONS.llama },
  { value: 'gemma', label: 'Gemma', icon: PROVIDER_ICONS.gemini },
  { value: 'mistral', label: 'Mistral', icon: PROVIDER_ICONS.mistral },
  { value: 'phi', label: 'Phi', icon: '' },
];

/** The families the curated set has, in the order above. */
export function localFamilies() {
  return LLM_FAMILIES.filter((f) => (localLlm.status.curated || []).some((m) => m.family === f.value));
}

/** The active local model's filename ('' = the bundled default → its filename). */
export function activeLocalFile(cfg) {
  return cfg.ppLocalModel || localLlm.status.modelFile || '';
}

export async function refreshLocalLlm() {
  try {
    localLlm.status = await invoke('local_llm_status');
  } catch {
    /* not in Tauri */
  }
}

let progressListener = null;
/** A download's progress, as Rust reports it (once per window). */
export function listenLocalProgress() {
  if (progressListener) return;
  progressListener = listen('local-llm-download-progress', (e) => {
    if (e.payload) localLlm.progress = e.payload;
  });
}

/**
 * While Settings lives (call it while it initialises): while the on-device
 * model loads, read its status every 3 s so the page flips to "Loaded" on
 * its own (a load on first use, or an idle model freed, comes as
 * `yap-local-llm-changed`: adoptions.svelte.js); and the family tab
 * defaults to the active model's (or the first there is).
 */
export function followLocalLlm(getCfg) {
  $effect(() => {
    if (!localLlm.status.loading) return;
    const id = setInterval(refreshLocalLlm, 3000);
    return () => clearInterval(id);
  });
  $effect(() => {
    const fams = localFamilies();
    if (fams.length && !fams.some((f) => f.value === localLlm.family)) {
      const cfg = getCfg();
      const active = (localLlm.status.curated || []).find((m) => cfg && activeLocalFile(cfg) === m.filename);
      localLlm.family = active?.family || fams[0].value;
    }
  });
}

/**
 * Switching models (re)starts the sidecar with the newly selected GGUF:
 * choosing one loads it, as a download does. The sidecar reads the
 * selection from config.json, so save first.
 */
export async function onLocalModelChange() {
  localLlm.error = '';
  localLlm.switching = true;
  try {
    await persist();
    if (localLlm.status.running) await invoke('local_llm_stop');
    await invoke('local_llm_start');
  } catch (e) {
    localLlm.error = `${e}`;
  } finally {
    localLlm.switching = false;
    refreshLocalLlm();
  }
}

/** "Load now": the model loads (Settings' own Start). */
export async function loadLocalNow() {
  localLlm.error = '';
  localLlm.status = { ...localLlm.status, loading: true };
  try {
    await invoke('local_llm_start');
  } catch (e) {
    localLlm.error = `${e}`;
  } finally {
    await refreshLocalLlm();
  }
}

/** "Free memory": the model stops now; it loads again when it's needed. */
export async function freeLocalNow() {
  localLlm.error = '';
  try {
    await invoke('local_llm_stop');
  } catch (e) {
    localLlm.error = `${e}`;
  } finally {
    await refreshLocalLlm();
  }
}

export function openLlmFolder() {
  invoke('open_llm_folder').catch(() => {});
}

/**
 * Download a curated model (pulls the llamafile runtime too if missing),
 * then make it the active model and (re)start the sidecar.
 */
export async function downloadCurated(m) {
  if (localLlm.downloadingId || localLlm.installing) return;
  localLlm.error = '';
  localLlm.downloadingId = m.id;
  localLlm.cancelled = false;
  localLlm.installing = true;
  localLlm.progress = { stage: '', percent: 0 };
  try {
    const filename = await invoke('local_llm_install', { model: m.id });
    config.cfg.ppLocalModel = m.isDefault ? '' : filename; // '' keeps default semantics
    await persist();
    if (localLlm.status.running) await invoke('local_llm_stop');
    await invoke('local_llm_start');
  } catch (e) {
    // Cancelled: nothing to say (its half-downloaded file is gone).
    if (!isCancelled(e)) localLlm.error = `${e}`;
    localLlm.cancelled = isCancelled(e);
  } finally {
    localLlm.installing = false;
    localLlm.downloadingId = '';
    // What's on disk and running now, whatever happened (nothing polls
    // while the model isn't loading).
    await refreshLocalLlm();
  }
}

/** Cancel the curated model's download (shell/download.rs: its partial file goes). */
export function cancelCurated(m) {
  return cancelDownload(localCancelId(m));
}

/** What cancels a curated model's install (Rust's `models::cancel_id`). */
export function localCancelId(m) {
  return `llm:${m?.id || 'qwen2.5-1.5b'}`;
}

/** Switch the active local model to an already-downloaded curated one. */
export async function activateCurated(m) {
  if (localLlm.switching) return;
  config.cfg.ppLocalModel = m.isDefault ? '' : m.filename;
  await onLocalModelChange();
}

/**
 * The bigger model offered once (local_llm::upgrade_offer: a graphics card
 * that can run it, the 1.5B in use): taken or turned down, it isn't
 * offered again. Never downloaded without this click.
 */
export async function takeUpgrade(m) {
  if (m.installed) {
    config.cfg.localLlmUpgradeDismissed = true;
    await activateCurated(m);
    return;
  }
  await downloadCurated(m);
  if (!localLlm.error && !localLlm.cancelled) {
    config.cfg.localLlmUpgradeDismissed = true;
    await persist();
    await refreshLocalLlm();
  }
}

export async function dismissUpgrade() {
  config.cfg.localLlmUpgradeDismissed = true;
  await persist();
  await refreshLocalLlm();
}

/** Delete a downloaded curated model; if it was active, fall back to the default. */
export async function deleteCurated(m) {
  localLlm.error = '';
  try {
    await invoke('local_llm_delete', { filename: m.filename });
    if (activeLocalFile(config.cfg) === m.filename) {
      config.cfg.ppLocalModel = '';
      await persist();
      if (localLlm.status.running) {
        await invoke('local_llm_stop');
        await invoke('local_llm_start');
      }
    }
  } catch (e) {
    localLlm.error = `${e}`;
  } finally {
    await refreshLocalLlm();
  }
}
