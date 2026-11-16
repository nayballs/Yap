// The reading card's sentence as it's drawn (16 Nov 2026): Rust sends it as
// parts, its words numbered and what's between them (voice/read_aloud.rs
// `parts`). A formula in it — an answer's maths read aloud, `$…$`, `\(…\)`,
// `$$…$$`, `\[…\]`, found as core/markdown.js `mathAt` finds it, so a price
// ("$5 and $10") stays words — is one piece, drawn by KaTeX on the card
// (ui/markdownVisuals.js), with the words it holds: it's lit while any of
// them is said. Everything else is as Rust sent it.
import { mathAt } from '../core/markdown.js';

/**
 * `parts` (`[{ text, w? }]`) as pieces to draw: `{ text, w }` (`w` a word's
 * number, or `null` between words), or a formula `{ text, maths: true,
 * display, ws }` (its source as written, the words in it). Pure.
 */
export function readingPieces(parts) {
  const list = Array.isArray(parts) ? parts : [];
  const text = list.map((p) => String(p.text ?? '')).join('');
  if (!/[$\\]/.test(text)) return list.map((p) => ({ text: String(p.text ?? ''), w: p.w ?? null }));
  // Each character's word (UTF-16 units, as `text` is indexed).
  const wordAt = [];
  for (const p of list) for (let i = 0; i < String(p.text ?? '').length; i++) wordAt.push(p.w ?? null);
  const out = [];
  for (let i = 0; i < text.length; ) {
    const c = text[i];
    const m = c === '$' || c === '\\' ? mathAt(text, i) : null;
    if (m) {
      const ws = [...new Set(wordAt.slice(i, m.end).filter((w) => w != null))];
      out.push({ text: text.slice(i, m.end), maths: true, display: m.display, ws });
      i = m.end;
      // A formula on lines of its own ends its sentence: the full stop the
      // reading gave it would sit alone on the line under it.
      if (m.display && !/[\p{L}\p{N}$\\]/u.test(text.slice(i))) break;
      continue;
    }
    const w = wordAt[i];
    const last = out.at(-1);
    if (last && !last.maths && last.w === w) last.text += c;
    else out.push({ text: c, w });
    i++;
  }
  return out;
}
