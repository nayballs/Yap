// Yap's curated model lists (ppModels.js) merged with what each provider
// really offers right now (ai/model_catalog.rs, asked with the person's own
// key). ONE helper for every picker: Settings → Language Models (cleanup and
// every scope), the Chat view's model chip, the cleanup profiles' model
// field. Pure: no Tauri calls here (the store is modelCatalog.svelte.js).
//
// The rules:
// - Curated models the provider lists: shown as always, in curated order
//   (the recommended default stays first).
// - Models it lists that aren't curated: after them. Those released after
//   CURATED_AT (by the provider's date, or — for a provider whose list has
//   no dates, Gemini — not in the registry's `seen` and first seen since)
//   are tagged "New", newest first, for NEW_FOR_DAYS; the rest wait behind
//   "Show N more".
// - Curated models it no longer lists (only after a good fetch): hidden.
// - The saved model missing from a good list: kept, selectable, flagged
//   `missing`, so the page can say "Gemini no longer offers Gemini 3 Flash"
//   instead of failing silently (and nothing breaks if the list was wrong).
// - No key, offline, a failed fetch, a test run: exactly the curated list.
import { PP_CLOUD_MODELS, CURATED_AT, modelThinks } from './ppModels.js';
import { contextInfo } from './localContext.js';

const DAY = 24 * 60 * 60;
/** How long a model keeps its "New" tag. */
export const NEW_FOR_DAYS = 120;
const CURATED_AT_SECS = Date.parse(`${CURATED_AT}T00:00:00Z`) / 1000;

/**
 * Does live model `m` stand for the id `id`: itself, a dated snapshot of it
 * (`claude-haiku-4-5-20251001` for `claude-haiku-4-5`), or one folded into it?
 */
export function liveMatches(m, id) {
  return !!id && !!m && (m.id === id || m.base === id || (m.aliases ?? []).includes(id));
}

/** When the model came out, as far as Yap can tell (unix secs), or null. */
function releasedAt(provider, m) {
  if (m.created) return m.created;
  const seen = PP_CLOUD_MODELS[provider]?.seen;
  return seen && !seen.includes(m.id) && m.firstSeen ? m.firstSeen : null;
}

function isNew(provider, m, now) {
  if (m.deprecated) return false;
  const at = releasedAt(provider, m);
  return !!at && at >= CURATED_AT_SECS && now - at < NEW_FOR_DAYS * DAY;
}

/** A context size in words: "128K context" (131,072), "200K context", "1M context". */
const contextLine = contextInfo;

/**
 * The merged list for `provider`. `entry` = the provider's snapshot from
 * ai/model_catalog.rs (`{ ok, models, error, fetchedAt, checking }`, or
 * undefined: no key); `current` = the saved model id.
 *
 * Returns `{ live, error, fetchedAt, checking, items, more, missing }`:
 * `items` are what a picker shows (curated, then New, then the saved model
 * when it would otherwise be hidden); `more` the rest of the provider's
 * list; each `{ value, label, desc, tag: 'new'|null, missing, curated,
 * tools: true|false|null, thinks }`. `missing` = `{ value, label }` of the
 * saved model when a good list doesn't have it.
 */
export function mergeModels(provider, entry, { current = '', now = Date.now() / 1000 } = {}) {
  const curated = PP_CLOUD_MODELS[provider]?.models ?? [];
  const state = {
    error: entry?.error ?? null,
    fetchedAt: entry?.fetchedAt ?? 0,
    checking: !!entry?.checking,
  };
  // A row's value is the saved id when that's how this model was saved (a
  // dated snapshot of a curated alias), so the picker shows it selected.
  const valueOf = (c, m) => {
    const v = c?.value ?? m.id;
    return current && current !== v && m && liveMatches(m, current) ? current : v;
  };
  const row = (c, m, extra = {}) => ({
    value: valueOf(c, m),
    label: c?.label || m?.name || m?.id || '',
    desc: c?.desc ?? (m?.desc || contextLine(m?.context)),
    tag: null,
    missing: false,
    curated: !!c,
    tools: m?.tools ?? null,
    thinks: m?.thinks ?? (c ? modelThinks(c.value) : false),
    ...extra,
  });
  const curatedOnly = () => ({ ...state, live: false, items: curated.map((c) => row(c, null)), more: [], missing: null });

  const models = entry?.ok ? (entry.models ?? []) : [];
  if (models.length === 0) return curatedOnly();

  const used = new Set();
  const items = [];
  for (const c of curated) {
    const m = models.find((x) => liveMatches(x, c.value));
    if (m) {
      used.add(m.id);
      items.push(row(c, m));
    } else if (c.value === current) {
      items.push(row(c, null, { missing: true }));
    }
  }
  const fresh = [];
  const rest = [];
  for (const m of models) {
    if (used.has(m.id)) continue;
    (isNew(provider, m, now) ? fresh : rest).push(m);
  }
  fresh.sort((a, b) => (releasedAt(provider, b) ?? 0) - (releasedAt(provider, a) ?? 0));
  rest.sort(
    (a, b) =>
      Number(!!a.deprecated) - Number(!!b.deprecated) ||
      (b.created ?? 0) - (a.created ?? 0) ||
      (a.name || a.id).localeCompare(b.name || b.id)
  );
  items.push(...fresh.map((m) => row(null, m, { tag: 'new' })));
  let more = rest.map((m) => row(null, m, m.deprecated ? { desc: 'Being retired' } : {}));

  // The saved model is always on show: moved up from "more", or kept (and
  // flagged) when the provider no longer lists it.
  if (current && !items.some((i) => i.value === current)) {
    const at = rest.findIndex((m) => liveMatches(m, current));
    if (at >= 0) {
      items.push(more[at]);
      more = more.filter((_, i) => i !== at);
    } else {
      items.push(row(null, { id: current }, { missing: true }));
    }
  }
  // Never a picker with nothing to pick.
  if (items.every((i) => i.missing) && more.length === 0) return curatedOnly();
  if (items.every((i) => i.missing)) {
    items.push(...more);
    more = [];
  }
  const gone = items.find((i) => i.missing);
  return {
    ...state,
    live: true,
    items,
    more,
    missing: gone ? { value: gone.value, label: gone.label } : null,
  };
}

/**
 * The model a picker selects when the person switches to `provider`: the
 * first one it shows that the provider still offers.
 */
export function defaultFrom(merged, provider) {
  return merged.items.find((i) => !i.missing)?.value ?? PP_CLOUD_MODELS[provider]?.models[0]?.value ?? '';
}

/** Does `provider` offer `id` (curated or live, not gone)? */
export function offers(merged, id) {
  return !!id && [...merged.items, ...merged.more].some((i) => i.value === id && !i.missing);
}

/**
 * A model's name: the curated label, else the provider's own name from its
 * list, else the id.
 */
export function labelFor(provider, id, entry) {
  const own = PP_CLOUD_MODELS[provider]?.models.find((x) => x.value === id);
  if (own) return own.label;
  const live = entry?.ok ? entry.models?.find((m) => liveMatches(m, id)) : null;
  return live?.name || id || '';
}

/** "just now", "5 min ago", "3 h ago", "2 days ago". */
export function ago(secs, now = Date.now() / 1000) {
  const d = Math.max(0, now - secs);
  if (d < 90) return 'just now';
  if (d < 3600) return `${Math.round(d / 60)} min ago`;
  if (d < 2 * DAY) return `${Math.round(d / 3600)} h ago`;
  return `${Math.round(d / DAY)} days ago`;
}
