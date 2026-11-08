// PNGs for Yap Lab (docs/lab.md): read a size, stitch a tall thread shot
// out of its pieces, draw the made-up pictures a script seeds a workspace
// with. Playwright's own bundled pngjs (as desktop/e2e/support/png.js):
// no new package. Pure but for the codec.
import utilsBundle from 'playwright-core/lib/utilsBundle';

const { PNG } = utilsBundle;

/** A PNG's width and height from its header, without decoding it. Pure. */
export function pngSize(bytes) {
  const b = Buffer.from(bytes);
  if (b.length < 24 || b.readUInt32BE(0) !== 0x89504e47 || b.toString('latin1', 12, 16) !== 'IHDR') throw new Error('Not a PNG');
  return { width: b.readUInt32BE(16), height: b.readUInt32BE(20) };
}

/** A PNG's bytes → `{ width, height, data }` (RGBA). */
export function readPng(bytes) {
  const p = PNG.sync.read(Buffer.from(bytes));
  return { width: p.width, height: p.height, data: p.data };
}

/** `{ width, height, data }` (RGBA) → a PNG's bytes. */
export function writePng({ width, height, data }) {
  const p = new PNG({ width, height });
  data.copy(p.data);
  return PNG.sync.write(p);
}

/**
 * One tall PNG from pieces stacked top to bottom: each `{ png, from, to }`
 * keeps rows `from` up to (not including) `to` of its picture (all of it
 * without them). The pieces' widths can differ by a pixel or two (a capture
 * rounded): the result is the narrowest. Pure but for the codec.
 */
export function stitch(pieces) {
  const parts = pieces.map((p) => {
    const img = readPng(p.png);
    const from = Math.max(0, Math.min(img.height, Math.round(p.from ?? 0)));
    const to = Math.max(from, Math.min(img.height, Math.round(p.to ?? img.height)));
    return { img, from, to };
  });
  const width = Math.min(...parts.map((p) => p.img.width));
  const height = parts.reduce((n, p) => n + (p.to - p.from), 0);
  const data = Buffer.alloc(width * height * 4);
  let y = 0;
  for (const { img, from, to } of parts) {
    for (let row = from; row < to; row++, y++) {
      img.data.copy(data, y * width * 4, row * img.width * 4, row * img.width * 4 + width * 4);
    }
  }
  return writePng({ width, height, data });
}

/** "#rrggbb" (or "#rgb") → [r, g, b]. Pure. */
export function colour(hex, fallback = [128, 128, 128]) {
  const m = /^#?([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(String(hex ?? '').trim());
  if (!m) return fallback;
  const h = m[1].length === 3 ? [...m[1]].map((c) => c + c).join('') : m[1];
  return [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16));
}

/**
 * A made-up picture: `shape` ("circle", "square", "triangle", "stripes")
 * in `color` on `background`, `width` × `height` (at most 2000 each). For a
 * script's workspace: nothing from outside. Pure but for the codec.
 */
export function drawPng({ width = 320, height = 200, shape = 'circle', color = '#d1495b', background = '#edf2f4' } = {}) {
  const w = Math.max(8, Math.min(2000, Math.round(width)));
  const h = Math.max(8, Math.min(2000, Math.round(height)));
  const fg = colour(color, [209, 73, 91]);
  const bg = colour(background, [237, 242, 244]);
  const data = Buffer.alloc(w * h * 4);
  const cx = w / 2;
  const cy = h / 2;
  const r = Math.min(w, h) * 0.35;
  const inside = (x, y) => {
    switch (shape) {
      case 'square':
        return Math.abs(x - cx) <= r && Math.abs(y - cy) <= r;
      case 'triangle': {
        // Point up: the base at cy + r, the tip at cy - r.
        const t = (y - (cy - r)) / (2 * r);
        return t >= 0 && t <= 1 && Math.abs(x - cx) <= t * r;
      }
      case 'stripes':
        return Math.floor(x / Math.max(4, w / 10)) % 2 === 0;
      default:
        return (x - cx) ** 2 + (y - cy) ** 2 <= r * r;
    }
  };
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const [cr, cg, cb] = inside(x + 0.5, y + 0.5) ? fg : bg;
      const i = (y * w + x) * 4;
      data[i] = cr;
      data[i + 1] = cg;
      data[i + 2] = cb;
      data[i + 3] = 255;
    }
  }
  return writePng({ width: w, height: h, data });
}

/** The mean luma (0–255) of rows `from`–`to` of a decoded PNG (all of it by default). Pure. */
export function meanLuma(img, from = 0, to = img.height) {
  let sum = 0;
  let n = 0;
  for (let y = Math.max(0, from); y < Math.min(img.height, to); y++) {
    for (let x = 0; x < img.width; x++) {
      const i = (y * img.width + x) * 4;
      sum += 0.2126 * img.data[i] + 0.7152 * img.data[i + 1] + 0.0722 * img.data[i + 2];
      n++;
    }
  }
  return n ? sum / n : 0;
}

/** How many distinct colours rows `from`–`to` hold (capped): 1 is a blank band. Pure. */
export function coloursIn(img, from = 0, to = img.height, cap = 64) {
  const seen = new Set();
  for (let y = Math.max(0, from); y < Math.min(img.height, to) && seen.size < cap; y++) {
    for (let x = 0; x < img.width && seen.size < cap; x++) {
      const i = (y * img.width + x) * 4;
      seen.add((img.data[i] << 16) | (img.data[i + 1] << 8) | img.data[i + 2]);
    }
  }
  return seen.size;
}
