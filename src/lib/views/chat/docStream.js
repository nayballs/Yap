// A document's page as Chat writes it (DocPanel.svelte, its own chunk): the
// pure parts, kept apart so they can be tested alone.
//
// - `shownSegments`: the text the page shows, in pieces — each section being
//   written a piece of its own (its key in `live`), the rest as saved — so
//   the page redraws only the piece that's growing (docPieces.js
//   `splitSections` cuts the rest at its sections, each drawn once).
// - `revealStep`: how much of a section's text is on the page by the next
//   frame. Chat's text comes in bursts (Rust sends what's written at most
//   every 50 ms, a provider that won't stream sends the section whole): the
//   page lets it flow in at a pace that follows how far behind it is, so it
//   reads line by line instead of in jumps, never lagging much (2026-10-31,
//   after Claude's own documents filling while you watch).
import { isPlaceholder } from '../../core/markdown.js';
import { closePartial } from '../../core/streaming.js';
import { findSection } from './docSections.js';

/** A section's text as it streams, under its own heading when it wrote one, else the key's. */
export function liveBlock(key, text) {
  const own = /^#{1,2}\s+\S/.test(text.trimStart());
  return own ? closePartial(text.trimStart().replace(/^#\s/, '## ')) : `## ${key}\n\n${closePartial(text)}`;
}

/**
 * A document's text as the panel shows it, in pieces (`[{md, live}]`,
 * joined with "\n" it's the whole): without its first line when that's the
 * title (the panel shows it), and each section being written in its place
 * as it streams (`live`: key → its text so far) — a placeholder's place
 * (`write_section`), the section it rewrites (or the section as saved, as
 * it goes on flowing in after it landed), or a new one at the end
 * (`rewrite_section`) — as a piece of its own (`live` its key; `null` for
 * the rest). Pure.
 */
export function shownSegments(content, title, live = {}) {
  let lines = String(content || '').split('\n');
  const first = lines[0]?.trim() ?? '';
  if (/^#\s+/.test(first) && first.replace(/^#\s+/, '').trim() === String(title || '').trim()) {
    lines = lines.slice(1);
  }
  // Each line, or a live section's block in its place.
  const items = lines.map((l) => ({ md: l, live: null }));
  for (const key of Object.keys(live)) {
    const block = { md: liveBlock(key, live[key] ?? ''), live: key };
    const plain = items.map((it) => (it.live == null ? it.md : null));
    const at = plain.findIndex((l) => l != null && isPlaceholder(l, key));
    if (at >= 0) {
      items.splice(at, 1, block);
      continue;
    }
    // (A live block stands in as a heading nothing is called: it ends a range.)
    const range = findSection(
      plain.map((l) => l ?? '## \u0000'),
      key,
      live[key]
    );
    if (range) items.splice(range[0], range[1] - range[0], block);
    else items.push({ md: '', live: null }, block);
  }
  // Runs of plain lines joined; the text's leading blank lines left out.
  const out = [];
  for (const it of items) {
    if (it.live != null || !out.length || out.at(-1).live != null) out.push({ ...it });
    else out.at(-1).md += `\n${it.md}`;
  }
  while (out.length > 1 && out[0].live == null && /^\n*$/.test(out[0].md)) out.shift();
  if (out.length && out[0].live == null) out[0].md = out[0].md.replace(/^\n+/, '');
  return out;
}

/** The text the panel shows, whole (`shownSegments` joined). Pure. */
export function shownText(content, title, live = {}) {
  return shownSegments(content, title, live)
    .map((s) => s.md)
    .join('\n');
}

/** Characters a second a section flows in at, at least. */
const MIN_RATE = 90;
/** Seconds: each second shows this share of what's waiting (1/τ), so a burst is caught up in about a second. */
const CATCH_UP = 0.3;

/**
 * How much of a section's text (`total` characters) is on the page after
 * `dtMs` more, from `have` (a fraction is kept: `revealCut` rounds down).
 * Pure.
 */
export function revealStep(have, total, dtMs) {
  if (have >= total) return total;
  const dt = Math.min(Math.max(Number(dtMs) || 0, 0), 250) / 1000;
  const n = Math.max(MIN_RATE * dt, ((total - have) * dt) / CATCH_UP);
  return Math.min(total, have + n);
}

/**
 * Where a section's revealed text ends: `have` rounded down, never between
 * the halves of a character outside the BMP (an emoji). Pure.
 */
export function revealCut(text, have) {
  let n = Math.min(text.length, Math.max(0, Math.floor(have)));
  const c = text.charCodeAt(n - 1);
  if (n < text.length && c >= 0xd800 && c <= 0xdbff) n += 1;
  return n;
}

/**
 * Where a section starts flowing in: after its own heading line (and the
 * blank lines under it) when its text has one — the heading is known
 * whole; flowing in "#", "##"… would flicker a heading of the wrong size —
 * else at its start. Pure.
 */
export function revealFrom(text) {
  const m = /^\s*#{1,2}\s+\S[^\n]*\n(?:[ \t]*\n)*/.exec(String(text ?? ''));
  return m ? m[0].length : 0;
}

/**
 * How much of a section is shown once its text is `text` (it came on, or
 * it landed as saved), from `have` of `prev` before: as much as before —
 * the saved text differs from what streamed only in small ways (its
 * heading written out, its citations linked) — unless the text is now
 * shorter than that (written afresh: a rewrite of it began), then from its
 * start. Never less than its heading. Pure.
 */
export function revealOn(have, text, prev = null) {
  const t = String(text ?? '');
  const from = revealFrom(t);
  if (have == null) return from;
  // Saved, it has the heading it streamed without (`## Gear` from its key).
  const h = prev != null && from > 0 && !/^\s*#/.test(prev) ? have + from : have;
  if (h > t.length) return from;
  return Math.max(h, from);
}
