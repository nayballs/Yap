// Settings' copy of the config and config.json, kept in step
// (Settings.svelte, 2026-10-23). Settings reads the config once and
// auto-saves its whole copy; other surfaces save config.json too (the
// Speech-to-Text page's model switch, the tray, onboarding, a notice's
// button…). `merge3` puts this copy's changes onto config.json as it is now,
// and takes in what changed there, without either side's change undoing the
// other's.

const plain = (v) => v !== null && typeof v === 'object' && !Array.isArray(v);

/** Whether two JSON values are the same (an object's key order, and keys
 *  that are `undefined`, aside). */
export function same(a, b) {
  if (a === b) return true;
  if (a === null || b === null || typeof a !== 'object' || typeof b !== 'object') return false;
  if (Array.isArray(a) !== Array.isArray(b)) return false;
  if (Array.isArray(a)) return a.length === b.length && a.every((x, i) => same(x, b[i]));
  const ka = Object.keys(a).filter((k) => a[k] !== undefined);
  const kb = Object.keys(b).filter((k) => b[k] !== undefined);
  return ka.length === kb.length && ka.every((k) => same(a[k], b[k]));
}

/**
 * Three-way merge of JSON values: `mine` (this copy) and `theirs`
 * (config.json now) both came from `base` (config.json as this copy last
 * agreed with it). What only one side changed comes from that side; where
 * both changed, objects merge key by key (one provider's key here, another's
 * there) and anything else keeps `mine` — the edit the person is making.
 */
export function merge3(base, mine, theirs) {
  if (same(mine, base)) return theirs;
  if (same(theirs, base) || same(mine, theirs)) return mine;
  if (plain(mine) && plain(theirs)) {
    const b = plain(base) ? base : {};
    const out = {};
    for (const k of new Set([...Object.keys(mine), ...Object.keys(theirs)])) {
      const v = merge3(b[k], mine[k], theirs[k]);
      if (v !== undefined) out[k] = v;
    }
    return out;
  }
  return mine;
}

/**
 * `base` with its holes filled from `pristine` (this copy as first set up:
 * Settings' defaults and its own fill-ins, `ensureScopes`), key by key. A
 * fill-in the person never touched then reads as no change of this copy's,
 * so a value saved elsewhere into that hole wins: the robot's Chat scope,
 * saved where config.json had none, was put back to Settings' default one
 * at its next auto-save (a real-app run, 2026-10-23) — as the Chat
 * picker's model would have been.
 */
export function fill(base, pristine) {
  if (base === undefined) return copy(pristine);
  if (!plain(base) || !plain(pristine)) return base;
  const out = { ...base };
  for (const k of Object.keys(pristine)) out[k] = fill(base[k], pristine[k]);
  return out;
}

/** A plain deep copy of a JSON value (Svelte's proxies read through). */
export function copy(v) {
  return v === undefined ? undefined : JSON.parse(JSON.stringify(v));
}
