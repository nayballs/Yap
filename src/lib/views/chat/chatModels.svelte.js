// What the Chat model pickers list — the short menu (ChatModelMenu.svelte,
// its main models picked by core/mainModels.js) and the provider rail
// (ChatModelPicker.svelte) — built from what Yap already knows:
// - the cloud providers' models: modelCatalog.js's merge of Yap's curated
//   list with the provider's live one, exactly the rows Settings shows
//   (items, then "Show N more"); a provider without a key shows its curated
//   models greyed with "Add key";
// - "On this PC": Yap's on-device models (local_llm_status: the curated set
//   with "Recommended for this PC" from the graphics card check, GGUFs of
//   the person's own in <data>/llm/), downloaded right here through
//   `local_llm_install` and its `local-llm-download-progress` events, and
//   uninstalled from an installed one's right-click menu
//   (core/localModels.js); and any self-hosted server on this PC, from its
//   `GET {base}/models`;
// - "Custom": a server elsewhere that Settings has the address of;
// - "Recent": the last three Chat models picked, kept in this webview
//   (localStorage, a per-window convenience; chatModelState.svelte.js).
// Pure helpers plus small stores; the Tauri calls are the pickers'. Only the
// pickers load this (their own chunks); the chip needs chatModelState.svelte.js.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { PP_CLOUD_MODELS } from '../../core/ppModels.js';
import { catalog, modelsFor } from '../../stores/modelCatalog.svelte.js';
import { ago } from '../../core/modelCatalog.js';
import { cancelDownload, downloadLine, fromEvent, isCancelled } from '../../core/downloads.js';
import { familyIcon, providerIcon, modelName } from '../../core/aiModel.js';
import { contextInfo, fmtTokens } from '../../core/localContext.js';
import { readsImages } from '../../stores/visionTags.svelte.js';
import { local, refreshLocal, sameUrl, readRecent, recentEntry, pushRecent } from './chatModelState.svelte.js';

export { local, refreshLocal, sameUrl, recentEntry, pushRecent };

/** The rail's cloud providers, in its order. `short` names the rail entry. */
export const CLOUD_PROVIDERS = [
  { id: 'groq', name: 'Groq', short: 'Groq' },
  { id: 'gemini', name: 'Google Gemini', short: 'Gemini' },
  { id: 'anthropic', name: 'Anthropic', short: 'Claude' },
  { id: 'openai', name: 'OpenAI', short: 'OpenAI' },
  { id: 'openrouter', name: 'OpenRouter', short: 'OpenRouter' },
];

export const cloudInfo = (p) => CLOUD_PROVIDERS.find((c) => c.id === p) ?? null;
export const isCloud = (p) => !!cloudInfo(p);
/** Where the provider hands out API keys. */
export const keyUrl = (p) => PP_CLOUD_MODELS[p]?.keyUrl || '';

/** "2.8 GB", "398 MB". */
export function fmtSize(mb) {
  const n = Number(mb) || 0;
  return n >= 1024 ? `${(n / 1024).toFixed(1)} GB` : `${Math.max(1, Math.round(n))} MB`;
}

const tag = (t, text) => ({ t, text });
/** At most two tags a row. */
const cap = (tags) => tags.filter(Boolean).slice(0, 2);
/** "Reads images" (Snip to Chat): the model takes a screenshot (stores/visionTags.svelte.js). */
const imagesTag = (images) => images === true && tag('img', 'Reads images');

// ------------------------------------------------------------ on this PC

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

/** "Up to 256K · Blurb": first, so a one-line row never cuts it off. */
const withMax = (desc, max) => [max ? `Up to ${fmtTokens(max)}` : '', desc].filter(Boolean).join(' · ');

/**
 * Download a curated on-device model (and the llamafile engine if it's
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

/** Where the on-device model runs, in one line (the pane's header). */
export function whereLocal(st) {
  if (!st) return '';
  const gpu = st.gpu?.name;
  const onCard = gpu ? `Runs on your graphics card (${gpu})` : 'Runs on your graphics card';
  if (st.running && st.placement) {
    if (st.placement.kind === 'gpu') return onCard;
    if (st.slowOnThisPc) return '';
    return st.placement.kind === 'partial' ? 'Runs partly on your graphics card' : 'Runs on the processor';
  }
  if (st.gpuCapable) return onCard;
  return 'Runs on the processor: the smaller models are quickest';
}

/** The processor warning (Settings' words): `{ text, small }` or null. */
export function slowWarning(st) {
  if (!st?.slowOnThisPc) return null;
  const small = (st.curated || []).find((c) => c.isDefault) ?? null;
  const name = small?.display || 'Qwen2.5 1.5B';
  const p = st.placement;
  const text =
    p?.kind === 'partial'
      ? `Runs partly on the processor on this PC (${p.gpuLayers} of ${p.layers} layers on the graphics card): answers will be slow — ${name} is quicker.`
      : `Runs on the processor on this PC: answers will be slow — ${name} is quicker.`;
  return { text, small };
}

/** The on-device rows: `{ installed, available }` (recommended first). */
export function localRows(status) {
  const st = local.status;
  if (!st) return { installed: [], available: [] };
  const onDevice = !!status?.ready && status.model?.provider === 'ondevice';
  const active = st.activeFile || st.modelFile;
  const installed = [];
  const available = [];
  for (const m of st.curated || []) {
    const current = onDevice && m.installed && active === m.filename;
    const row = {
      key: `ondevice:${m.filename}`,
      kind: 'ondevice',
      provider: 'ondevice',
      model: '',
      file: m.filename,
      curatedId: m.id,
      name: m.display,
      desc: withMax(m.blurb, m.maxContext),
      maxContext: m.maxContext ?? null,
      where: 'On this PC',
      icon: familyIcon(m.display, m.family),
      glyph: 'chip',
      current,
      action: m.installed ? 'pick' : local.downloading === m.id ? 'downloading' : 'download',
      sizeMb: m.sizeMb,
      // Installed: its file's own size, and why it can't be uninstalled
      // now ("selected" | "busy" | null; ai/local_llm/uninstall.rs).
      fileMb: m.fileMb ?? null,
      inUse: m.inUse ?? null,
      inUseWhy: st.inUseNote || '',
      recommended: !!m.recommended,
      tools: m.tools !== false,
      // It reads screenshots here: its projector downloaded and on
      // (ai/local_llm/vision.rs).
      images: m.readsImages ? true : null,
      tags: cap([
        current && tag('use', 'In use'),
        m.readsImages && imagesTag(true),
        m.recommended && tag('rec', 'Recommended for this PC'),
        m.tools === false && tag('warn', 'no note tools'),
      ]),
    };
    (m.installed ? installed : available).push(row);
  }
  for (const o of st.own || []) {
    const current = onDevice && active === o.file;
    installed.push({
      key: `ondevice:${o.file}`,
      kind: 'ondevice',
      provider: 'ondevice',
      model: '',
      file: o.file,
      name: o.name,
      desc: withMax(`Your own model · ${fmtSize(o.sizeMb)}`, o.maxContext),
      maxContext: o.maxContext ?? null,
      where: 'On this PC',
      icon: familyIcon(o.name),
      glyph: 'chip',
      current,
      action: 'pick',
      own: true,
      fileMb: o.sizeMb ?? null,
      inUse: o.inUse ?? null,
      inUseWhy: st.inUseNote || '',
      tools: o.tools !== false,
      tags: cap([current && tag('use', 'In use'), o.tools === false && tag('warn', 'no note tools')]),
    });
  }
  available.sort((a, b) => Number(b.recommended) - Number(a.recommended));
  return { installed, available };
}

// ---------------------------------------------------- self-hosted servers

/** baseUrl → `{ loading, ok, models, error }` (ai/model_catalog.rs `llm_models`). */
export const serverLists = $state({});

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

/**
 * What a self-hosted server says of a model's context, read-only: "8K
 * context, set in Ollama", "Up to 128K"; '' when it doesn't say.
 */
export function serverContextLine(c) {
  if (!c) return '';
  if (c.set) return `${fmtTokens(c.set)} context, set in ${c.app}`;
  return c.max ? `Up to ${fmtTokens(c.max)}` : '';
}

/** "3 models on this server", "Asking the server…", or why it couldn't. */
export function serverLine(s) {
  const l = serverLists[s.baseUrl];
  if (!l || (l.loading && !l.ok)) return { text: 'Asking the server for its models…' };
  if (l.ok) return { text: l.models.length === 1 ? '1 model on this server' : `${l.models.length} models on this server` };
  return { text: `Couldn't list the server's models: ${l.error}`, warn: true };
}

/** A server's rows: its models, the one set up for it, the Chat model on it. */
export function serverRows(s, status) {
  const l = serverLists[s.baseUrl];
  const cur =
    status?.ready && status.model?.provider === s.provider && status.host === s.host ? status.model.id : '';
  const models = l?.ok ? l.models.map((m) => ({ id: m.id, name: m.name || m.id, tools: m.tools })) : [];
  for (const id of [s.model, cur]) {
    if (id && !models.some((m) => m.id === id)) {
      models.unshift({ id, name: id, tools: id === cur ? !!status.tools : null });
    }
  }
  const ctxLine = (id) => serverContextLine((l?.contexts || []).find((c) => c.id === id));
  return models.map((m) => {
    const images = readsImages(s.provider, m.id);
    return {
      key: `${s.provider}:${s.baseUrl}:${m.id}`,
      kind: 'server',
      provider: s.provider,
      model: m.id,
      baseUrl: s.baseUrl,
      name: m.name,
      // The context first: a one-line row cuts its end off.
      desc: [ctxLine(m.id), m.id === s.model ? `Set up in Settings · ${s.host}` : s.host].filter(Boolean).join(' · '),
      where: s.onThisPc ? 'On this PC' : s.host,
      icon: familyIcon(m.name, m.id),
      glyph: 'server',
      current: !!cur && m.id === cur,
      action: 'pick',
      tools: m.tools !== false,
      images,
      tags: cap([imagesTag(images), m.tools === false && tag('warn', 'no note tools')]),
    };
  });
}

// ------------------------------------------------------------ the cloud

/** A provider's rows as Settings lists them: `{ connected, rows, more }`. */
export function cloudRows(p, status) {
  const connected = !!status?.keyed?.includes(p);
  const cur = connected && status?.ready && status.model?.provider === p ? status.model.id : '';
  const merged = modelsFor(p, cur);
  const short = cloudInfo(p)?.short || p;
  // The context the provider's live list gives ("1M context"), as info:
  // only from the list Yap already has, never a call of its own.
  const live = catalog.providers[p]?.models ?? [];
  const liveOf = (id) => live.find((m) => m.id === id || m.aliases?.includes(id) || m.base === id) ?? null;
  const ctxOf = (id) => liveOf(id)?.context ?? null;
  const descOf = (i) => {
    const info = contextInfo(ctxOf(i.value));
    return !info || /context/i.test(i.desc || '') ? i.desc : [i.desc, info].filter(Boolean).join(' · ');
  };
  const row = (i) => {
    const images = readsImages(p, i.value);
    return {
      key: `${p}:${i.value}`,
      kind: 'cloud',
      provider: p,
      model: i.value,
      name: i.label,
      desc: descOf(i),
      where: short,
      icon: familyIcon(i.label, i.value) ?? providerIcon(p),
      current: !!cur && i.value === cur,
      action: connected ? 'pick' : 'key',
      tools: i.tools !== false,
      images,
      // For the menu's main models (core/mainModels.js): when the provider
      // says it came out, and whether it's on its way out or gone.
      created: liveOf(i.value)?.created ?? null,
      deprecated: !!liveOf(i.value)?.deprecated,
      missing: !!i.missing,
      isNew: i.tag === 'new',
      tags: connected
        ? cap([
            i.missing && tag('gone', 'No longer offered'),
            i.tag === 'new' && tag('new', 'New'),
            imagesTag(images),
            i.tools === false && tag('warn', 'no note tools'),
          ])
        : [tag('key', 'Add key'), imagesTag(images)].filter(Boolean),
    };
  };
  const rows = merged.items.map(row);
  const more = merged.more.map(row);
  // The Chat model when no list has it (an id typed in Settings).
  if (cur && !rows.some((r) => r.current) && !more.some((r) => r.current)) {
    rows.unshift(row({ value: cur, label: modelName(status.model), desc: 'The model id saved in Settings', tools: status.tools ? null : false }));
  }
  return { connected, rows, more, live: merged.live, missing: merged.missing };
}

/** The pane's status line for a cloud provider. */
export function providerLine(p, status) {
  const short = cloudInfo(p)?.short || p;
  if (!status?.keyed?.includes(p)) return { text: 'Not set up' };
  const entry = catalog.providers[p];
  if (entry?.checking) return { text: `Checking ${short}'s models…` };
  if (modelsFor(p).live) return { text: `Connected · checked ${ago(entry.fetchedAt)}` };
  if (entry?.error) return { text: `Couldn't check: ${entry.error}. Showing Yap's list.`, warn: true };
  return { text: 'Connected' };
}

// ------------------------------------------------------------- recent

/** The Recent rows, as each provider lists them now (gone ones left out). */
export function recentRows(status) {
  const out = [];
  for (const e of readRecent()) {
    let row = null;
    if (isCloud(e.provider)) {
      const { rows, more } = cloudRows(e.provider, status);
      row = [...rows, ...more].find((r) => r.model === e.model) ?? null;
    } else if (e.provider === 'ondevice') {
      row = localRows(status).installed.find((r) => r.file === e.file) ?? null;
    } else {
      const s = (status?.servers || []).find((x) => x.provider === e.provider && sameUrl(x.baseUrl, e.baseUrl));
      if (s) row = serverRows(s, status).find((r) => r.model === e.model) ?? null;
    }
    if (row && !out.some((r) => r.key === row.key)) out.push(row);
  }
  return out;
}

// ------------------------------------------------------------- search

/** Every model the picker knows matching `q`, grouped by where it runs. */
export function searchGroups(q, status) {
  const terms = q.toLowerCase().split(/\s+/).filter(Boolean);
  if (terms.length === 0) return [];
  const match = (r, extra) => {
    // "images" (or "vision") finds the models that read them.
    const hay = `${r.name} ${r.model} ${r.file || ''} ${extra} ${r.images ? 'reads images vision screenshots' : ''}`.toLowerCase();
    return terms.every((t) => hay.includes(t));
  };
  const groups = [];
  for (const p of CLOUD_PROVIDERS) {
    const { rows, more } = cloudRows(p.id, status);
    const hits = [...rows, ...more].filter((r) => match(r, `${p.name} ${p.short}`));
    if (hits.length) groups.push({ id: p.id, label: p.short, icon: providerIcon(p.id), rows: hits });
  }
  const { installed, available } = localRows(status);
  const servers = status?.servers || [];
  const pc = [...installed, ...available].filter((r) => match(r, 'on this pc on-device local private'));
  for (const s of servers.filter((x) => x.onThisPc)) {
    pc.push(...serverRows(s, status).filter((r) => match(r, `on this pc self-hosted local server ${s.host}`)));
  }
  if (pc.length) groups.push({ id: 'pc', label: 'On this PC', glyph: 'pc', rows: pc });
  const custom = servers
    .filter((x) => !x.onThisPc)
    .flatMap((s) => serverRows(s, status).filter((r) => match(r, `custom server ${s.host}`)));
  if (custom.length) groups.push({ id: 'custom', label: 'Custom', glyph: 'server', rows: custom });
  return groups;
}
