// The Sources row under an answer (2026-10-31): one chip a page, the pages
// it cites or opened first, a few shown and the rest behind "+N more". A
// strong model's guide listed 46 sources under one line ("It's open on the
// right."), most of them search results nobody opened, many titled only
// "Divinity: Original Sin 2 · steamcommunity.com". The Rust side marks a
// page read `opened` and one the answer or its document cites `cited`
// (web/turn.rs, chat/chats.rs `Source`); an older answer without them has
// its `[n]` citations read from its text. As the Claude app's research
// lists its sources: the top few, then "N other sources".

/** How many chips the row shows before "+N more". */
export const FEW = 4;

/**
 * A page's address as one page, as the Rust side compares them
 * (web/turn.rs `normal_url`): no "www.", no fragment, no `utm_` parameters,
 * no trailing slash. '' when there's no address (a meeting's passage).
 */
export function pageKey(url) {
  if (!url) return '';
  try {
    const u = new URL(url.trim());
    u.hash = '';
    u.hostname = u.hostname.replace(/^www\./i, '').toLowerCase();
    for (const k of [...u.searchParams.keys()]) if (k.startsWith('utm_')) u.searchParams.delete(k);
    u.pathname = u.pathname.replace(/\/+$/, '') || '/';
    return u.toString().replace(/\/(\?|$)/, '$1');
  } catch {
    return url.trim().replace(/\/+$/, '');
  }
}

/** The `[n]` citations in an answer's text ("[1]", "[2, 3]", "[2–4]"). */
export function citedIn(text) {
  const out = new Set();
  for (const m of String(text || '').matchAll(/\[(\d{1,2}(?:\s*[,–-]\s*\d{1,2})*)\](?!\()/g)) {
    const nums = m[1].split(/[,–-]/).map((n) => Number(n.trim()));
    const range = /[–-]/.test(m[1]) && nums.length === 2 && nums[0] < nums[1] && nums[1] - nums[0] <= 10;
    const all = range ? Array.from({ length: nums[1] - nums[0] + 1 }, (_, i) => nums[0] + i) : nums;
    for (const n of all) if (n > 0) out.add(n);
  }
  return out;
}

/** A page's own name in its address that says nothing about it. */
const BARE = /^(index|default|home|view|viewtopic|showthread|filedetails|details|page|post|posts|thread|threads|topic|wiki|article|articles|guide|guides|\d+|[0-9a-f-]{16,})$/i;

/**
 * What a page's address says it is, for a title several pages share: its
 * last part ("…/wiki/Fort+Joy" → "Fort Joy", "…/posts/lone-wolf-mage-build"
 * → "lone wolf mage build"); '' when that says nothing
 * ("…/filedetails/?id=123").
 */
export function addressTitle(url) {
  try {
    const last = new URL(url).pathname.split('/').filter(Boolean).at(-1) ?? '';
    const words = decodeURIComponent(last.replace(/\+/g, ' '))
      .replace(/\.(html?|php|aspx?)$/i, '')
      .replace(/[-_]+/g, ' ')
      .trim();
    if (words && !BARE.test(words) && /\p{L}{3}/u.test(words)) return words.length > 60 ? `${words.slice(0, 59)}…` : words;
  } catch {
    /* not an address */
  }
  return '';
}

/**
 * **The row**: each page once (by its address), the ones cited (`cited`,
 * or `[n]` in `text`) and opened first, then the rest, each part in number
 * order. A title several pages share gives way to what the address says,
 * where it says something. `{ all, few, more }`: every chip in order, the
 * first [`FEW`] of them, and how many "+N more" hides (0: nothing hidden —
 * one more than a few is shown rather than "+1 more").
 */
export function sourceRow(sources, text = '') {
  const cited = citedIn(text);
  const seen = new Set();
  const once = [];
  for (const s of sources || []) {
    const key = s.url ? pageKey(s.url) : `${s.note ?? ''}:${s.event ?? ''}:${s.n}`;
    if (seen.has(key)) continue;
    seen.add(key);
    once.push(s);
  }
  const titles = new Map();
  for (const s of once) if (s.url && s.title) titles.set(s.title, (titles.get(s.title) || 0) + 1);
  const shown = once.map((s) => {
    const shared = s.url && s.title && titles.get(s.title) > 1;
    const better = shared ? addressTitle(s.url) : '';
    return { ...s, label: better || s.title || s.site || '', key: !!(s.cited || s.opened || cited.has(s.n)) };
  });
  const byN = (a, b) => (a.n ?? 0) - (b.n ?? 0);
  const all = [...shown.filter((s) => s.key).sort(byN), ...shown.filter((s) => !s.key).sort(byN)];
  const hide = all.length > FEW + 1;
  return { all, few: hide ? all.slice(0, FEW) : all, more: hide ? all.length - FEW : 0 };
}
