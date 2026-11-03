// **What Chat's model picker lists**, as rows (views/chat/ChatModelPicker.svelte,
// a provider rail), from what Yap already knows. Pure: everything it reads
// comes in a `ctx`, so the same rows, groups and words are built in two
// places — the desktop picker (chatModels.svelte.js fills `ctx` from its
// stores and the Tauri calls, and reads it reactively) and Yap Link's
// phone page (src-tauri/link-page/picker.js fills it from the PC's
// `ready.picker`; `src-tauri/build.rs` serves this file and the pure modules
// it imports at core/, so it must never import anything else).
//
// `ctx` = {
//   status,          // ai_model_status('chat'): ready, model, provider, host, keyed, servers, effort…
//   catalog,         // provider → model_catalog's list ({ ok, models, fetchedAt, error, checking })
//   local,           // local_llm_status (null while it's read)
//   downloading,     // the curated on-device model downloading now ('' if none)
//   readsImages,     // (provider, model) → true | false | null ("Reads images")
//   servers,         // baseUrl → a server's own list ({ loading, ok, models, contexts, error }), if asked
// }
//
// - the cloud providers' models: modelCatalog.js's merge of Yap's curated
//   list with the provider's live one, split into Latest, the newest of each
//   family, and "More models (N)", the rest newest first
//   (core/latestModels.js); a provider without a key shows its curated
//   models greyed with "Add key"; OpenAI's two ways, the ChatGPT plan and an
//   API key, are one rail entry, "ChatGPT" (GPT_TAB), each way's rows saying
//   which pays;
// - "On this PC": Yap's on-device models (the curated set with "Recommended
//   for this PC", GGUFs of the person's own) and any self-hosted server on
//   this PC; "Custom": a server elsewhere that Settings has the address of;
// - "Recent": the last three Chat models picked (`recentRows`, from entries
//   the caller keeps).
import { mergeModels, labelFor, ago } from './modelCatalog.js';
import { contextInfo, fmtTokens } from './localContext.js';
import { latestModels } from './latestModels.js';
import { familyIcon, providerIcon } from './modelIcons.js';
import { CHATGPT, GPT_TAB } from './providerIds.js';

/**
 * The cloud providers, in the rail's order. `short` names the rail entry;
 * `tab` is the entry when it isn't the provider's own (OpenAI's two ways
 * share "ChatGPT"), `way` what a row from it says pays (Recent, search).
 */
export const CLOUD_PROVIDERS = [
  { id: 'groq', name: 'Groq', short: 'Groq' },
  { id: 'gemini', name: 'Google Gemini', short: 'Gemini' },
  { id: 'anthropic', name: 'Anthropic', short: 'Claude' },
  // OpenAI's models, two ways, one entry (2026-11-02): Sign in with ChatGPT,
  // the person's plan, first (ai/chatgpt.rs); then an OpenAI API key.
  { id: 'chatgpt', name: 'ChatGPT', short: 'ChatGPT', tab: GPT_TAB, way: 'ChatGPT plan', words: 'chatgpt openai gpt plan' },
  { id: 'openai', name: 'OpenAI', short: 'OpenAI', tab: GPT_TAB, way: 'OpenAI API key', words: 'chatgpt openai gpt api key' },
  { id: 'openrouter', name: 'OpenRouter', short: 'OpenRouter' },
];

export const cloudInfo = (p) => CLOUD_PROVIDERS.find((c) => c.id === p) ?? null;
export const isCloud = (p) => !!cloudInfo(p);
/** The rail entry a cloud provider's models are under. */
export const tabOf = (p) => cloudInfo(p)?.tab ?? p;

/** The rail's cloud entries: each provider's own, OpenAI's two as one "ChatGPT". */
export const CLOUD_RAIL = CLOUD_PROVIDERS.reduce((tabs, p) => {
  const id = tabOf(p.id);
  const had = tabs.find((t) => t.id === id);
  if (had) had.providers.push(p.id);
  else tabs.push({ id, label: p.short, providers: [p.id] });
  return tabs;
}, []);

/**
 * The ChatGPT entry: which ways are connected, and the way its pane shows —
 * the only one connected, else (both) `way` as the person left the switch.
 */
export function gptState(status, way) {
  const planOn = !!status?.keyed?.includes(CHATGPT);
  const keyOn = !!status?.keyed?.includes('openai');
  const shown = planOn && keyOn ? way : planOn ? CHATGPT : keyOn ? 'openai' : null;
  return { planOn, keyOn, shown };
}

/** "2.8 GB", "398 MB". */
export function fmtSize(mb) {
  const n = Number(mb) || 0;
  return n >= 1024 ? `${(n / 1024).toFixed(1)} GB` : `${Math.max(1, Math.round(n))} MB`;
}

export function sameUrl(a, b) {
  const t = (s) => String(s || '').trim().replace(/\/+$/, '').toLowerCase();
  return !!a && t(a) === t(b);
}

const tag = (t, text) => ({ t, text });
/** At most two tags a row. */
const cap = (tags) => tags.filter(Boolean).slice(0, 2);
/** "Reads images" (Snip to Chat): the model takes a screenshot (stores/visionTags.svelte.js). */
const imagesTag = (images) => images === true && tag('img', 'Reads images');
const imagesOf = (ctx, p, m) => (ctx.readsImages ? ctx.readsImages(p, m) : null);

/**
 * The rail, in its order: Recent, every cloud provider Yap supports (a dot
 * when it has a key: `ready`; `state` says it in words), Custom when a
 * server elsewhere is set up, and On this PC. `icon`: a mark's id
 * (modelIcons.js); `glyph`: one of Yap's own.
 */
export function railTabs(ctx) {
  const status = ctx.status;
  const servers = status?.servers || [];
  const remote = servers.filter((s) => !s.onThisPc);
  const pcReady = !!ctx.local?.installed || servers.some((s) => s.onThisPc) || (ctx.local?.own || []).length > 0;
  return [
    { id: 'recent', label: 'Recent', glyph: 'recent' },
    // ChatGPT's dot: green when either way (the plan, an OpenAI key) is connected.
    ...CLOUD_RAIL.map((t) => {
      const ready = t.providers.some((p) => status?.keyed?.includes(p));
      return { id: t.id, label: t.label, icon: providerIcon(t.providers[0]), ready, state: ready ? 'connected' : 'not set up', sepBefore: t.id === 'groq' };
    }),
    ...(remote.length ? [{ id: 'custom', label: 'Custom', glyph: 'server', ready: true, state: 'set up' }] : []),
    { id: 'pc', label: 'On this PC', glyph: 'pc', ready: pcReady, state: pcReady ? 'ready' : 'nothing installed yet', sepBefore: true },
  ];
}

/** The rail entry the picker opens on: the current model's provider (or the one the scope is set to). */
export function startTab(status) {
  const m = status?.ready ? status.model : null;
  const p = m?.provider || status?.provider || '';
  if (isCloud(p)) return tabOf(p);
  if (p === 'ondevice') return 'pc';
  if (p === 'local' || p === 'custom') {
    const s = (status?.servers || []).find((x) => x.provider === p && (!m || x.host === status.host));
    return s && !s.onThisPc ? 'custom' : 'pc';
  }
  return 'groq';
}

// ------------------------------------------------------------ on this PC

/** "Up to 256K · Blurb": first, so a one-line row never cuts it off. */
const withMax = (desc, max) => [max ? `Up to ${fmtTokens(max)}` : '', desc].filter(Boolean).join(' · ');

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
export function localRows(ctx) {
  const st = ctx.local;
  const status = ctx.status;
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
      action: m.installed ? 'pick' : ctx.downloading === m.id ? 'downloading' : 'download',
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
export function serverLine(s, ctx) {
  const l = ctx.servers?.[s.baseUrl];
  if (!l || (l.loading && !l.ok)) return { text: 'Asking the server for its models…' };
  if (l.ok) return { text: l.models.length === 1 ? '1 model on this server' : `${l.models.length} models on this server` };
  return { text: `Couldn't list the server's models: ${l.error}`, warn: true };
}

/** A server's rows: its models (when it was asked), the one set up for it, the Chat model on it. */
export function serverRows(s, ctx) {
  const status = ctx.status;
  const l = ctx.servers?.[s.baseUrl];
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
    const images = imagesOf(ctx, s.provider, m.id);
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

/**
 * A provider's rows, split as the picker shows them (core/latestModels.js):
 * `{ connected, split, latest, older, live, missing }` — `latest` the
 * newest of each family, `older` the rest under "More models (N)", newest
 * first (each marked `older`). `split` false: none of its models is in a
 * family, so `latest` and `older` are Settings' list and its "more".
 */
export function cloudRows(p, ctx) {
  const status = ctx.status;
  const entry = ctx.catalog?.[p];
  const connected = !!status?.keyed?.includes(p);
  const cur = connected && status?.ready && status.model?.provider === p ? status.model.id : '';
  const merged = mergeModels(p, entry, { current: cur });
  const where = cloudInfo(p)?.way || cloudInfo(p)?.short || p;
  // The context the provider's live list gives ("1M context"), as info:
  // only from the list Yap already has, never a call of its own.
  const live = entry?.models ?? [];
  const liveOf = (id) => live.find((m) => m.id === id || m.aliases?.includes(id) || m.base === id) ?? null;
  const descOf = (i, lm) => {
    const info = contextInfo(lm?.context ?? null);
    return !info || /context/i.test(i.desc || '') ? i.desc : [i.desc, info].filter(Boolean).join(' · ');
  };
  const row = (i) => {
    const images = imagesOf(ctx, p, i.value);
    const lm = liveOf(i.value);
    return {
      key: `${p}:${i.value}`,
      kind: 'cloud',
      provider: p,
      model: i.value,
      name: i.label,
      desc: descOf(i, lm),
      where,
      icon: familyIcon(i.label, i.value) ?? providerIcon(p),
      current: !!cur && i.value === cur,
      action: connected ? 'pick' : 'key',
      tools: i.tools !== false,
      images,
      // For Latest (core/latestModels.js): when the provider says it came
      // out, and whether it's on its way out or gone.
      created: lm?.created ?? null,
      deprecated: !!lm?.deprecated,
      missing: !!i.missing,
      tags: connected
        ? cap([
            i.missing && tag('gone', 'No longer offered'),
            i.tag === 'new' && tag('new', 'New'),
            imagesTag(images),
            i.tools === false && tag('warn', 'no note tools'),
          ])
        : [tag('key', p === CHATGPT ? 'Sign in' : 'Add key'), imagesTag(images)].filter(Boolean),
    };
  };
  const rows = merged.items.map(row);
  const more = merged.more.map(row);
  // The Chat model when no list has it (an id typed in Settings).
  if (cur && !rows.some((r) => r.current) && !more.some((r) => r.current)) {
    const label = status.model.name || labelFor(p, cur, entry) || 'Unknown model';
    rows.unshift(row({ value: cur, label, desc: 'The model id saved in Settings', tools: status.tools ? null : false }));
  }
  const split = latestModels(p, [...rows, ...more]);
  const latest = split ? split.latest : rows;
  const older = (split ? split.older : more).map((r) => ({ ...r, older: true }));
  return { connected, split: !!split, latest, older, live: merged.live, missing: merged.missing };
}

/** Every model a provider offers, as the picker lists them: Latest, then More models. */
export function allCloudRows(p, ctx) {
  const { latest, older } = cloudRows(p, ctx);
  return [...latest, ...older];
}

/** The pane's status line for a cloud provider. */
export function providerLine(p, ctx) {
  const status = ctx.status;
  const short = cloudInfo(p)?.short || p;
  if (!status?.keyed?.includes(p)) return { text: p === 'chatgpt' ? 'Not signed in' : 'Not set up' };
  const entry = ctx.catalog?.[p];
  if (p === 'chatgpt' && !entry?.checking) return { text: 'Using ChatGPT plan' };
  if (entry?.checking) return { text: `Checking ${short}'s models…` };
  if (mergeModels(p, entry).live) return { text: `Connected · checked ${ago(entry.fetchedAt)}` };
  if (entry?.error) return { text: `Couldn't check: ${entry.error}. Showing Yap's list.`, warn: true };
  return { text: 'Connected' };
}

// ------------------------------------------------------------- recent

/** Two Recent entries for the same model. */
export const sameEntry = (a, b) =>
  a.provider === b.provider && (a.model || '') === (b.model || '') && (a.file || '') === (b.file || '') && sameUrl(a.baseUrl || '-', b.baseUrl || '-');

/** The picker's rows → what Recent keeps of one. */
export function recentEntry(row) {
  return { provider: row.provider, model: row.model || '', file: row.file || '', baseUrl: row.baseUrl || '' };
}

/** Recent's entries (the caller keeps them), as each provider lists them now (gone ones left out). */
export function recentRows(entries, ctx) {
  const out = [];
  for (const e of entries || []) {
    let row = null;
    if (isCloud(e.provider)) {
      row = allCloudRows(e.provider, ctx).find((r) => r.model === e.model) ?? null;
    } else if (e.provider === 'ondevice') {
      row = localRows(ctx).installed.find((r) => r.file === e.file) ?? null;
    } else {
      const s = (ctx.status?.servers || []).find((x) => x.provider === e.provider && sameUrl(x.baseUrl, e.baseUrl));
      if (s) row = serverRows(s, ctx).find((r) => r.model === e.model) ?? null;
    }
    if (row && !out.some((r) => r.key === row.key)) out.push(row);
  }
  return out;
}

// ------------------------------------------------------------- search

/**
 * Every model the picker knows matching `q`, grouped by where it runs: a
 * provider's older models too, More models open or not (Latest first).
 */
export function searchGroups(q, ctx) {
  const terms = q.toLowerCase().split(/\s+/).filter(Boolean);
  if (terms.length === 0) return [];
  const match = (r, extra) => {
    // "images" (or "vision") finds the models that read them.
    const hay = `${r.name} ${r.model} ${r.file || ''} ${extra} ${r.images ? 'reads images vision screenshots' : ''}`.toLowerCase();
    return terms.every((t) => hay.includes(t));
  };
  const groups = [];
  for (const p of CLOUD_PROVIDERS) {
    // OpenAI's two ways: "openai", "gpt" or "chatgpt" find both, each its own group.
    const hits = allCloudRows(p.id, ctx).filter((r) => match(r, `${p.name} ${p.short} ${p.words || ''}`));
    if (hits.length) groups.push({ id: p.id, label: p.way || p.short, icon: providerIcon(p.id), rows: hits });
  }
  const { installed, available } = localRows(ctx);
  const servers = ctx.status?.servers || [];
  const pc = [...installed, ...available].filter((r) => match(r, 'on this pc on-device local private'));
  for (const s of servers.filter((x) => x.onThisPc)) {
    pc.push(...serverRows(s, ctx).filter((r) => match(r, `on this pc self-hosted local server ${s.host}`)));
  }
  if (pc.length) groups.push({ id: 'pc', label: 'On this PC', glyph: 'pc', rows: pc });
  const custom = servers
    .filter((x) => !x.onThisPc)
    .flatMap((s) => serverRows(s, ctx).filter((r) => match(r, `custom server ${s.host}`)));
  if (custom.length) groups.push({ id: 'custom', label: 'Custom', glyph: 'server', rows: custom });
  return groups;
}
