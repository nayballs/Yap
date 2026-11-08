// Connectors' marks (Settings → Connectors): a catalogue entry's `logo` names
// a file in desktop/src/assets/connectors/ (Simple Icons' marks, CC0, each in
// its brand's colour on a light tile in both themes, as the AI apps' logos
// are); without one, its initial on a tile of its `color`. A connector the
// person added by hand gets its initial too. Pure, no Svelte state.

const FILES = import.meta.glob('../../../assets/connectors/*.svg', { eager: true, query: '?url', import: 'default' });

/** name → the mark's URL. */
export const LOGOS = Object.fromEntries(Object.entries(FILES).map(([path, url]) => [path.split('/').pop().replace(/\.svg$/, ''), url]));

/** A mark's URL, or null for one Yap doesn't have. */
export function logoUrl(name) {
  return (name && LOGOS[name]) || null;
}

/** The first letter or digit of a name, upper case ("?" for none). */
export function initialOf(name) {
  const m = String(name ?? '').match(/[\p{L}\p{N}]/u);
  return m ? m[0].toLocaleUpperCase() : '?';
}

// Tiles for initials without a colour of their own: muted, ink-legible
// (white on each passes 4.5:1).
const TILES = ['#5b6b7a', '#7a5b6b', '#5b7a6b', '#6b5b7a', '#7a6b5b', '#4f6f8f'];

/** An initial's tile colour: the entry's own, else one picked by the name. */
export function tileOf(name, color) {
  if (color && /^#[0-9a-f]{6}$/i.test(color)) return color;
  let h = 0;
  for (const ch of String(name ?? '')) h = (h * 31 + ch.codePointAt(0)) >>> 0;
  return TILES[h % TILES.length];
}

/** A catalogue entry by its id, from Discover's list. */
export function entryOf(catalog, id) {
  return catalog?.connectors?.find((e) => e.id === id) ?? null;
}

/** Entries and cards whose name, publisher or line has every word of `q`. */
export function filterCatalog(items, q) {
  const words = String(q ?? '')
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean);
  if (!words.length) return items ?? [];
  return (items ?? []).filter((e) => {
    const hay = [e.name, e.publisher, e.blurb, e.id].filter(Boolean).join(' ').toLowerCase();
    return words.every((w) => hay.includes(w));
  });
}

/** The person's connectors whose name, address or status has every word of `q`. */
export function filterYours(list, q) {
  const words = String(q ?? '')
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean);
  if (!words.length) return list ?? [];
  return (list ?? []).filter((c) => {
    const hay = [c.name, c.address, c.status?.line].filter(Boolean).join(' ').toLowerCase();
    return words.every((w) => hay.includes(w));
  });
}

/**
 * A program's arguments as typed on one line: spaces between them, "double"
 * or 'single' quotes keeping one together (import.rs splits a pasted line
 * the same way).
 */
export function splitArgs(line) {
  const out = [];
  let cur = '';
  let quote = null;
  let any = false;
  for (const ch of String(line ?? '')) {
    if (quote) {
      if (ch === quote) quote = null;
      else cur += ch;
    } else if (ch === '"' || ch === "'") {
      quote = ch;
      any = true;
    } else if (/\s/.test(ch)) {
      if (any || cur) out.push(cur);
      cur = '';
      any = false;
    } else cur += ch;
  }
  if (any || cur) out.push(cur);
  return out;
}

/** Arguments back on one line, quoted where they need it. */
export function joinArgs(args) {
  return (args ?? []).map((a) => (a === '' || /[\s"']/.test(a) ? `"${a.replace(/"/g, '')}"` : a)).join(' ');
}

/** A link's host for a name ("mcp.example.com" → "Example"), or ''. */
export function nameFromUrl(url) {
  try {
    const host = new URL(String(url).trim()).hostname.replace(/^(mcp|api)\./, '');
    const word = host.split('.')[0] || host;
    return word ? word[0].toUpperCase() + word.slice(1) : '';
  } catch {
    return '';
  }
}

/** How an entry signs in, in the sheet's words. */
export function howItSignsIn(e) {
  if (!e) return '';
  if (e.signIn === 'oauth') return `${e.name} will ask you to sign in, in your browser.${e.token ? ' Or use a token instead.' : ''}`;
  if (e.signIn === 'token') return `${e.name} takes a token you make on its site.`;
  return `A program on this PC${e.needs ? `: it needs ${e.needs}` : ''}. Yap shows its exact command before it runs.`;
}
