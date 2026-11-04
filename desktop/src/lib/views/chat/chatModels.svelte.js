// What the Chat model picker (ChatModelPicker.svelte, a provider rail)
// lists, built from what Yap already knows. The rows themselves — the cloud
// providers' Latest and "More models (N)", OpenAI's two ways as one
// "ChatGPT" entry, On this PC, the servers, Recent, search — are
// core/chatPicker.js's (pure, shared with Yap Link's phone page); this file
// fills its `ctx` from the stores and the Tauri calls, and keeps what's
// this window's own:
// - "On this PC": local_llm_status, the on-device models downloaded right
//   here through `local_llm_install` and its `local-llm-download-progress`
//   events, and uninstalled from an installed one's right-click menu
//   (core/localModels.js);
// - a self-hosted server's own list, from its `GET {base}/models`;
// - More models open or closed as the person left it while Yap runs, else
//   open when the model in use is in it, which stays on show under it when
//   it's closed by hand;
// - "Recent": the last three Chat models picked, kept in this webview
//   (localStorage, a per-window convenience).
// The functions keep the picker's signatures (`status` first); reading the
// stores inside them keeps the picker reactive.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { PP_CLOUD_MODELS } from '../../core/ppModels.js';
import { catalog } from '../../stores/modelCatalog.svelte.js';
import { cancelDownload, downloadLine, fromEvent, isCancelled } from '../../core/downloads.js';
import { readsImages } from '../../stores/visionTags.svelte.js';
import * as picker from '../../core/chatPicker.js';

export {
  CLOUD_PROVIDERS,
  CLOUD_RAIL,
  cloudInfo,
  isCloud,
  tabOf,
  gptState,
  fmtSize,
  sameUrl,
  whereLocal,
  slowWarning,
  serverContextLine,
  recentEntry,
} from '../../core/chatPicker.js';

/** Where the provider hands out API keys. */
export const keyUrl = (p) => PP_CLOUD_MODELS[p]?.keyUrl || '';

// ------------------------------------------------------------ on this PC

/** local_llm_status, the download under way (a curated id) and its progress. */
export const local = $state({ status: null, downloading: '', progress: null, error: '', errorFor: '' });

/** baseUrl → `{ loading, ok, models, error }` (ai/model_catalog.rs `llm_models`). */
export const serverLists = $state({});

/** What chatPicker.js reads, from this window's stores (read as it builds: reactive). */
function ctxOf(status) {
  return {
    status,
    catalog: catalog.providers,
    local: local.status,
    downloading: local.downloading,
    readsImages,
    servers: serverLists,
  };
}

let progressListener = null;
function listenProgress() {
  if (!progressListener) {
    progressListener = listen('local-llm-download-progress', (e) => {
      const p = e.payload;
      if (p && local.downloading && (!p.model || p.model === local.downloading)) local.progress = p;
    }).catch(() => null);
  }
  return progressListener;
}

export async function refreshLocal() {
  try {
    local.status = await invoke('local_llm_status');
  } catch {
    /* not in Tauri */
  }
}

// A context change (ai/local_llm.rs `restart_when_idle`): read again when the
// restart waits, and when it's done.
let contextListener = null;
export function followLocalChanges() {
  if (!contextListener) {
    contextListener = listen('yap-local-llm-changed', () => {
      if (local.status) refreshLocal();
    }).catch(() => null);
  }
  return contextListener;
}

/**
 * Download a curated on-device model (and the llama.cpp engine if it's
 * missing) with Settings' own install path. Only one at a time; it carries
 * on when the picker closes. True when it's on this PC.
 */
export async function downloadLocal(id) {
  if (local.downloading) return false;
  // At once (the row turns into its progress bar), then the events.
  local.downloading = id;
  local.progress = { stage: '', model: id, percent: 0, downloaded_mb: 0, total_mb: 0 };
  local.error = '';
  local.errorFor = '';
  try {
    await listenProgress();
    await invoke('local_llm_install', { model: id });
    await refreshLocal();
    return true;
  } catch (e) {
    // Cancelled: back to its Download button, nothing to say.
    if (!isCancelled(e)) {
      local.error = String(e);
      local.errorFor = id;
    }
    return false;
  } finally {
    local.downloading = '';
    local.progress = null;
  }
}

/** Cancel a curated model's download (its half-downloaded file goes). */
export function cancelLocal(id) {
  return cancelDownload(`llm:${id}`);
}

/** "Downloading… 45% · 1.2 GB of 2.8 GB · 24 MB/s · about 1 min left", "Getting the AI engine… 30%". */
export function progressLine(p) {
  if (!p) return 'Starting the download…';
  const pct = `${p.percent || 0}%`;
  const line = downloadLine(fromEvent(p));
  if (p.stage === 'runtime') return `Getting the AI engine first… ${pct}`;
  return `Downloading… ${pct}${line ? ` · ${line}` : ''}`;
}

/** The on-device rows: `{ installed, available }` (recommended first). */
export const localRows = (status) => picker.localRows(ctxOf(status));

// ---------------------------------------------------- self-hosted servers

/** Ask a server for its models (kept a minute on Rust's side). */
export async function loadServer(s, force = false) {
  const prev = serverLists[s.baseUrl];
  serverLists[s.baseUrl] = { ok: false, models: [], error: '', ...prev, loading: true };
  try {
    const v = await invoke('llm_models', { provider: s.provider, scope: s.scope, baseUrl: s.baseUrl, force });
    serverLists[s.baseUrl] = {
      loading: false,
      ok: !!v?.ok,
      models: v?.models ?? [],
      contexts: v?.contexts ?? [],
      error: v?.ok ? '' : v?.error || '',
    };
  } catch (e) {
    serverLists[s.baseUrl] = { loading: false, ok: false, models: [], contexts: [], error: String(e) };
  }
}

/** "3 models on this server", "Asking the server…", or why it couldn't. */
export const serverLine = (s) => picker.serverLine(s, { servers: serverLists });

/** A server's rows: its models, the one set up for it, the Chat model on it. */
export const serverRows = (s, status) => picker.serverRows(s, ctxOf(status));

// ------------------------------------------------------------- the cloud

/**
 * A provider's rows, split as the picker shows them (core/latestModels.js):
 * `{ connected, split, latest, older, live, missing }`.
 */
export const cloudRows = (p, status) => picker.cloudRows(p, ctxOf(status));

/** Every model a provider offers, as the picker lists them: Latest, then More models. */
export const allCloudRows = (p, status) => picker.allCloudRows(p, ctxOf(status));

// "More models (N)" per cloud provider: open or closed as the person left
// it, while Yap runs (this window's module, kept when the picker closes).
const moreChoice = $state({});

/**
 * Is `p`'s More models open: as the person left it, else when the model in
 * use is one of them (so it shows, ✓ where it is).
 */
export function moreIsOpen(p, older) {
  return moreChoice[p] ?? older.some((r) => r.current);
}

/** The person opened or closed `p`'s More models (or picked one of them: open). */
export function setMoreOpen(p, open) {
  moreChoice[p] = !!open;
}

/** The pane's status line for a cloud provider. */
export const providerLine = (p, status) => picker.providerLine(p, ctxOf(status));

// ------------------------------------------------------------- recent

const RECENT_KEY = 'yapChatRecentModels';

function readRecent() {
  try {
    const v = JSON.parse(localStorage.getItem(RECENT_KEY) || '[]');
    return Array.isArray(v) ? v.filter((e) => e && typeof e.provider === 'string').slice(0, 3) : [];
  } catch {
    return [];
  }
}

/** A Chat model used (picked, or the one in use when the picker opened). */
export function pushRecent(entry) {
  if (!entry?.provider) return;
  const list = [entry, ...readRecent().filter((e) => !picker.sameEntry(e, entry))].slice(0, 3);
  try {
    localStorage.setItem(RECENT_KEY, JSON.stringify(list));
  } catch {
    /* a convenience only */
  }
}

/** The Recent rows, as each provider lists them now (gone ones left out). */
export const recentRows = (status) => picker.recentRows(readRecent(), ctxOf(status));

// ------------------------------------------------------------- search

/** Every model the picker knows matching `q`, grouped by where it runs. */
export const searchGroups = (q, status) => picker.searchGroups(q, ctxOf(status));
