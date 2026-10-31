// **Main models**: the short list Chat's model menu opens with
// (views/chat/ChatModelMenu.svelte), as the Claude Code app's model menu
// lists the latest Opus, Fable, Sonnet and Haiku and keeps the older ones
// under "More models ▸". The rule, in one place (pure: no Tauri, no Svelte;
// its tests are e2e/main-models.spec.js):
//
// - From each provider the person has connected (a key saved), the newest
//   model of each of its families: Anthropic's Opus, Fable, Sonnet and
//   Haiku; OpenAI's flagship and small models; Gemini's flagship (Pro, or a
//   named tier like Argon) and Flash; Groq's biggest and smallest GPT-OSS.
//   Through OpenRouter, the same families of a lab whose own key isn't
//   there (anthropic/…, openai/…, google/…). A connected provider none of
//   whose models is in a family: its first one, as Yap lists it.
// - "Newest" is read from the id: Opus 5.5 over Opus 4.8 (a dated snapshot
//   counts as its alias); OpenAI by generation, then its tier (Astra over
//   Sol over Terra; Luna over Mini over Nano), then the point release;
//   Gemini a release over a preview. Then the provider's own date, then
//   Yap's order. The families are names, never model ids, and the models
//   come from the providers' live lists (ai/model_catalog.rs, merged with
//   Yap's curated ones by modelCatalog.js): a new Opus a provider lists
//   tomorrow takes the place without a Yap update, and one it retired is
//   never offered here (a dead id fails silently: the live list drops it
//   first). A model being retired, or no longer offered, is never a main one.
// - Then the on-device model, when one is installed ("On this PC").
// - Then the model in use, always, even an older one (last, so the others
//   keep their numbers).
// - At most seven rows, each with its number key (1–7). Past that, each
//   provider keeps its first families in turn (Opus, then Sonnet, Haiku and
//   Fable; a flagship before a small model), the model in use kept.
//
// Everything else the connected providers offer is under More models
// (`moreModels`), grouped by provider, newest first.
//
// A row is the pickers' (chatModels.svelte.js): `{ key, provider, model,
// name, current?, created?, deprecated?, missing? }`.

/** The menu's providers, in its order: the strongest chat models first. */
export const MENU_ORDER = ['anthropic', 'openai', 'gemini', 'groq', 'openrouter'];

/** The most rows the menu's main list has. */
export const MAX_MAIN = 7;

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
function claude(fam, prio) {
  const now = new RegExp(`^claude-${fam}-(\\d+)(?:[-.](\\d{1,2}))?$`);
  const old = new RegExp(`^claude-(\\d+)(?:[-.](\\d))?-${fam}$`);
  return {
    id: fam,
    prio,
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
 * Each provider's families, in the menu's order. `key(id)`: how new (and
 * how fitting) a model is, compared left to right, or null when it isn't
 * one; `prio`: which a full menu keeps first.
 */
const FAMILIES = {
  anthropic: [claude('opus', 0), claude('fable', 3), claude('sonnet', 1), claude('haiku', 2)],
  openai: [
    {
      id: 'flagship',
      prio: 0,
      key: (m) => {
        const r = GPT.exec(m);
        const tier = r?.[3] || '';
        if (!r || GPT_SMALL.includes(tier) || GPT_NOT.includes(tier)) return null;
        return [num(r[1]), GPT_BIG.indexOf(tier) + 1, num(r[2])];
      },
    },
    {
      id: 'small',
      prio: 1,
      key: (m) => {
        const r = GPT.exec(m);
        return r && GPT_SMALL.includes(r[3]) ? [num(r[1]), num(r[2]), GPT_SMALL.indexOf(r[3])] : null;
      },
    },
  ],
  gemini: [
    {
      id: 'flagship',
      prio: 0,
      key: (m) => {
        const r = GEMINI.exec(m);
        return r && !r[3].startsWith('flash') ? [num(r[1]), num(r[2]), r[4] ? 0 : 1] : null;
      },
    },
    {
      id: 'flash',
      prio: 1,
      key: (m) => {
        const r = GEMINI.exec(m);
        return r && r[3] === 'flash' ? [num(r[1]), num(r[2]), r[4] ? 0 : 1] : null;
      },
    },
  ],
  groq: [
    {
      id: 'big',
      prio: 0,
      key: (m) => {
        const r = OSS.exec(m);
        return r ? [num(r[1] || 1), num(r[2])] : null;
      },
    },
    {
      id: 'small',
      prio: 1,
      key: (m) => {
        const r = OSS.exec(m);
        return r ? [num(r[1] || 1), -num(r[2])] : null;
      },
    },
  ],
};

/** OpenRouter's labs whose families it carries, and the provider each is. */
const LABS = { anthropic: 'anthropic', openai: 'openai', google: 'gemini' };

/** `provider`'s families, each reading a model id (`read(id)` → key | null). */
function familiesOf(provider, connected) {
  if (provider !== 'openrouter') {
    return (FAMILIES[provider] || []).map((f) => ({ ...f, read: (id) => f.key(bare(id)) }));
  }
  const out = [];
  for (const [lab, own] of Object.entries(LABS)) {
    // Its own key is there: those come from the provider itself.
    if (connected.includes(own)) continue;
    for (const f of FAMILIES[own]) {
      out.push({
        ...f,
        id: `${lab}:${f.id}`,
        read: (id) => {
          const [l, rest] = splitLab(id);
          return l === lab ? f.key(bare(rest)) : null;
        },
      });
    }
  }
  return out;
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

/** One provider's picks: `{ row, prio, fam }` per family it has, in the menu's order. */
function picksOf(provider, rows, connected) {
  const taken = new Set();
  const out = [];
  familiesOf(provider, connected).forEach((f, fam) => {
    let best = null;
    rows.forEach((row, i) => {
      if (!usable(row) || taken.has(row.key)) return;
      const k = f.read(row.model);
      if (!k) return;
      const c = { row, k, i };
      const d = best ? cmpKey(k, best.k) || (row.created ?? 0) - (best.row.created ?? 0) : 1;
      if (d > 0) best = c;
    });
    if (best) {
      taken.add(best.row.key);
      out.push({ row: best.row, prio: f.prio, fam });
    }
  });
  // None of its models is in a family: its first, as Yap lists it.
  if (!out.length) {
    const first = rows.find(usable);
    if (first) out.push({ row: first, prio: 9, fam: 0 });
  }
  return out;
}

/** The connected providers in the menu's order, with their rows. */
function connectedRows(rows, connected) {
  return MENU_ORDER.filter((p) => connected.includes(p) && rows[p]?.length).map((p) => ({ p, rows: rows[p] }));
}

/**
 * **The menu's main models.** `rows`: provider → its rows as the pickers
 * list them (unconnected providers' are ignored); `connected`: the
 * providers with a key; `onDevice`: the on-device model's row when one is
 * installed; `current`: the model in use's row (null: none). Rows, in the
 * menu's order.
 */
export function mainModels({ rows = {}, connected = [], onDevice = null, current = null, max = MAX_MAIN } = {}) {
  const lists = connectedRows(rows, connected);
  const all = lists.flatMap(({ p, rows: rs }, pi) => picksOf(p, rs, connected).map((c) => ({ ...c, pi })));
  const isCurrent = (r) => !!current && !!r && r.key === current.key;
  const curPick = all.find((c) => isCurrent(c.row));
  const extra = current && !curPick && !isCurrent(onDevice) ? current : null;
  const room = Math.max(1, max - (onDevice ? 1 : 0) - (extra ? 1 : 0));
  let kept = all;
  if (all.length > room) {
    // The model in use first, then each provider's first families in turn.
    const order = [...all].sort((a, b) => Number(b === curPick) - Number(a === curPick) || a.prio - b.prio || a.pi - b.pi || a.fam - b.fam);
    const keep = new Set(order.slice(0, room));
    kept = all.filter((c) => keep.has(c));
  }
  return [...kept.map((c) => c.row), onDevice, extra].filter(Boolean);
}

/** Newest first: by the provider's dates when every row has one, else by the version in the id. */
function newestFirst(provider, list) {
  const dated = list.every((r) => r.created);
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
 * **More models**: every other model of the connected providers, grouped
 * (`{ id, rows }`: a provider, "pc" for this PC's other models and
 * self-hosted servers, then a server elsewhere by its host), newest first
 * in each. `main`: what the menu shows already.
 */
export function moreModels({ rows = {}, connected = [], pc = [], servers = [], main = [] } = {}) {
  const shown = new Set(main.map((r) => r.key));
  const rest = (list) => list.filter((r) => r && !shown.has(r.key) && !r.missing);
  const groups = connectedRows(rows, connected).map(({ p, rows: rs }) => ({ id: p, rows: newestFirst(p, rest(rs)) }));
  groups.push({ id: 'pc', rows: rest(pc) });
  for (const s of servers) groups.push({ id: s.id, label: s.label, rows: rest(s.rows) });
  return groups.filter((g) => g.rows.length);
}
