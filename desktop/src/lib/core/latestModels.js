// **Latest models**: the split in Chat's model picker (the provider rail,
// views/chat/ChatModelPicker.svelte) of the selected cloud provider's list
// into "Latest", the newest model of each of its families, and "More
// models (N)" under it, the rest, as the Claude Code app keeps the latest
// Opus, Fable, Sonnet and Haiku up front. The rule, in one place (pure: no
// Tauri, no Svelte; its tests are desktop/e2e/latest-models.spec.js):
//
// - Latest: the provider's newest model of each family, in this order —
//   Anthropic: Opus, Fable, Sonnet, Haiku; OpenAI: its flagship and its
//   small tier; Gemini: its flagship (Pro, or a named top tier like Argon)
//   and Flash; Groq: its biggest and its smallest GPT-OSS; OpenRouter: the
//   same families of the labs it carries (anthropic/…, openai/…, google/…),
//   Anthropic's, then OpenAI's, then Google's.
// - "Newest" is read from the id: Opus 5.5 over Opus 4.8 (a dated snapshot
//   counts as its alias); OpenAI by generation, then its tier (Astra over
//   Sol over Terra; Luna over Mini over Nano), then the point release;
//   Gemini a release over a preview. Then the provider's own date, then
//   Yap's order. The families are names, never model ids, and the rows are
//   the providers' live lists merged with Yap's curated ones
//   (core/modelCatalog.js): a new Opus a provider lists tomorrow takes the
//   place without a Yap update, and one it retired is never offered. A
//   model being retired (`deprecated`) or no longer offered (`missing`) is
//   never a latest one.
// - More models: every other model of the provider, newest first — by the
//   provider's dates when every row has one, else by the version in the id,
//   then the date, then Yap's order.
// - A provider none of whose models is in a family: no split (null); the
//   picker lists it as Yap always has.
// - The model in use stays where it is: ✓ in Latest, or in More models,
//   which then opens (the picker's, chatModels.svelte.js `moreIsOpen`; closed
//   by hand, the picker keeps that one row on show under it).
//
// A row is the picker's (chatModels.svelte.js): `{ key, model, created?,
// deprecated?, missing? }`.

const num = (s) => Number(s ?? 0) || 0;

/** The id as the families read it: lower case, no "models/", ":variant" or dated snapshot. */
function bare(id) {
  return String(id || '')
    .trim()
    .toLowerCase()
    .replace(/^models\//, '')
    .replace(/:[a-z0-9-]+$/, '')
    .replace(/-(?:\d{8}|\d{4}-\d{2}-\d{2})$/, '');
}

/** "anthropic/claude-opus-4.5" → ["anthropic", "claude-opus-4.5"] (OpenRouter's ids). */
function splitLab(id) {
  const s = String(id || '').trim().toLowerCase();
  const at = s.indexOf('/');
  return at < 0 ? ['', s] : [s.slice(0, at), s.slice(at + 1)];
}

/** A Claude family: claude-opus-5-5, claude-opus-4-5-20251101, claude-opus-4.5, claude-3-5-sonnet. */
function claude(fam) {
  const now = new RegExp(`^claude-${fam}-(\\d+)(?:[-.](\\d{1,2}))?$`);
  const old = new RegExp(`^claude-(\\d+)(?:[-.](\\d))?-${fam}$`);
  return {
    id: fam,
    key: (m) => {
      const r = now.exec(m) || old.exec(m);
      return r ? [num(r[1]), num(r[2])] : null;
    },
  };
}

// OpenAI: gpt-6-astra, gpt-6.1-sol, gpt-5.5, gpt-5.4-mini. Tiers by how
// capable (unknown ones and a plain id rank lowest); not chat-only, audio,
// search or Responses-only variants.
const GPT = /^gpt-(\d+)(?:\.(\d+))?(?:-([a-z]+))?$/;
const GPT_BIG = ['terra', 'sol', 'astra'];
const GPT_SMALL = ['nano', 'mini', 'luna'];
const GPT_NOT = ['chat', 'turbo', 'instruct', 'audio', 'realtime', 'search', 'codex', 'pro', 'image', 'transcribe', 'tts', 'preview', 'latest'];

// Gemini: gemini-4-argon, gemini-3.1-pro-preview, gemini-3.8-flash.
const GEMINI = /^gemini-(\d+)(?:\.(\d+))?-([a-z]+(?:-lite)?)(-preview)?(?:-\d{2}-\d{2,4})?$/;

// Groq's GPT-OSS: openai/gpt-oss-120b (a later generation: gpt-oss-2-120b).
const OSS = /^(?:openai\/)?gpt-oss-(?:(\d+)-)?(\d+)b$/;

/**
 * Each provider's families, in Latest's order. `key(id)`: how new (and how
 * fitting) a model is, compared left to right, or null when it isn't one.
 */
const FAMILIES = {
  anthropic: [claude('opus'), claude('fable'), claude('sonnet'), claude('haiku')],
  openai: [
    {
      id: 'flagship',
      key: (m) => {
        const r = GPT.exec(m);
        const tier = r?.[3] || '';
        if (!r || GPT_SMALL.includes(tier) || GPT_NOT.includes(tier)) return null;
        return [num(r[1]), GPT_BIG.indexOf(tier) + 1, num(r[2])];
      },
    },
    {
      id: 'small',
      key: (m) => {
        const r = GPT.exec(m);
        return r && GPT_SMALL.includes(r[3]) ? [num(r[1]), num(r[2]), GPT_SMALL.indexOf(r[3])] : null;
      },
    },
  ],
  gemini: [
    {
      id: 'flagship',
      key: (m) => {
        const r = GEMINI.exec(m);
        return r && !r[3].startsWith('flash') ? [num(r[1]), num(r[2]), r[4] ? 0 : 1] : null;
      },
    },
    {
      id: 'flash',
      key: (m) => {
        const r = GEMINI.exec(m);
        return r && r[3] === 'flash' ? [num(r[1]), num(r[2]), r[4] ? 0 : 1] : null;
      },
    },
  ],
  groq: [
    {
      id: 'big',
      key: (m) => {
        const r = OSS.exec(m);
        return r ? [num(r[1] || 1), num(r[2])] : null;
      },
    },
    {
      id: 'small',
      key: (m) => {
        const r = OSS.exec(m);
        return r ? [num(r[1] || 1), -num(r[2])] : null;
      },
    },
  ],
};

/** OpenRouter's labs whose families it carries, in Latest's order, and the provider each is. */
const LABS = [
  ['anthropic', 'anthropic'],
  ['openai', 'openai'],
  ['google', 'gemini'],
];

/** `provider`'s families, each reading a model id (`read(id)` → key | null). */
function familiesOf(provider) {
  if (provider !== 'openrouter') {
    return (FAMILIES[provider] || []).map((f) => ({ ...f, read: (id) => f.key(bare(id)) }));
  }
  return LABS.flatMap(([lab, own]) =>
    FAMILIES[own].map((f) => ({
      ...f,
      id: `${lab}:${f.id}`,
      read: (id) => {
        const [l, rest] = splitLab(id);
        return l === lab ? f.key(bare(rest)) : null;
      },
    }))
  );
}

/** Compare two keys, left to right: > 0 when `a` is newer. */
function cmpKey(a, b) {
  for (let i = 0; i < Math.max(a.length, b.length); i++) {
    const d = (a[i] ?? 0) - (b[i] ?? 0);
    if (d) return d;
  }
  return 0;
}

/**
 * The version an id says (`[major, minor]`: Claude, GPT, Gemini), else
 * `[0, 0]`: More models' "newest first" where a list has no dates.
 */
export function versionOf(provider, id) {
  const m = bare(provider === 'openrouter' ? splitLab(id)[1] : id);
  const r =
    /^claude-[a-z]+-(\d+)(?:[-.](\d{1,2}))?$/.exec(m) ||
    /^claude-(\d+)(?:[-.](\d))?-[a-z]+$/.exec(m) ||
    /^gpt-(\d+)(?:\.(\d+))?(?:$|-)/.exec(m) ||
    /^gemini-(\d+)(?:\.(\d+))?-/.exec(m);
  return r ? [num(r[1]), num(r[2])] : [0, 0];
}

const usable = (r) => r && !r.missing && !r.deprecated;
/** OpenRouter's ":free" / ":thinking" variant of an id. */
const variant = (id) => /:[a-z0-9-]+$/i.test(String(id || ''));

/** The newest usable row of each family, in the families' order (a row once). */
function picks(provider, rows) {
  const taken = new Set();
  const out = [];
  for (const f of familiesOf(provider)) {
    let best = null;
    rows.forEach((row) => {
      if (!usable(row) || taken.has(row.key)) return;
      const k = f.read(row.model);
      if (!k) return;
      // Newer by the id, then by the provider's date, then the plain id
      // over a variant of it; a tie keeps Yap's order.
      const d = best
        ? cmpKey(k, best.k) || (row.created ?? 0) - (best.row.created ?? 0) || Number(variant(best.row.model)) - Number(variant(row.model))
        : 1;
      if (d > 0) best = { row, k };
    });
    if (best) {
      taken.add(best.row.key);
      out.push(best.row);
    }
  }
  return out;
}

/** Newest first: by the provider's dates when every row has one, else by the version in the id. */
export function newestFirst(provider, list) {
  const dated = list.length > 0 && list.every((r) => r.created);
  return list
    .map((row, i) => ({ row, i, v: versionOf(provider, row.model) }))
    .sort((a, b) => {
      const byDate = (b.row.created ?? 0) - (a.row.created ?? 0);
      const byVersion = cmpKey(b.v, a.v);
      return (dated ? byDate || byVersion : byVersion || byDate) || a.i - b.i;
    })
    .map((x) => x.row);
}

/**
 * **The split.** `rows`: every model `provider` offers, as the picker lists
 * them (Yap's order: curated, New, the rest). `{ latest, older }` — the
 * newest of each family in the families' order, then every other one newest
 * first — or null when none of them is in a family (no split).
 */
export function latestModels(provider, rows = []) {
  const latest = picks(provider, rows);
  if (!latest.length) return null;
  const shown = new Set(latest.map((r) => r.key));
  return { latest, older: newestFirst(provider, rows.filter((r) => !shown.has(r.key))) };
}
