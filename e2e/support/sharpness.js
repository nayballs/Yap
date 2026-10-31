// How crisply a picture is drawn on screen (Chat's screenshots:
// chat-shots.spec.js). A made-up screenshot of a dark app — rows of tiny
// one-pixel "glyphs", hairlines, buttons — goes into Chat; the page's own
// pixels where it's drawn (a device-scale capture) are compared with the
// ideal: the same part of the picture area-averaged to exactly that many
// device pixels. `error` is the mean difference per channel (0 = the ideal);
// `sharpness` the drawn picture's edge contrast over the ideal's (1 = as
// crisp as it can be, lower = softer). Playwright's own pngjs, no package.
import utilsBundle from 'playwright-core/lib/utilsBundle';

const { PNG } = utilsBundle;

/** A deterministic 0..1 sequence. */
function lcg(seed) {
  let s = seed >>> 0;
  return () => (s = (Math.imul(s, 1664525) + 1013904223) >>> 0) / 4294967296;
}

/**
 * A `width` × `height` "screenshot" of a dark editor: a title bar, a
 * sidebar, rows of 6 × 9 px glyphs drawn with one-pixel strokes (text's
 * detail, without a font), a table's hairlines and two buttons. RGBA.
 */
export function screenshotLike(width, height) {
  const data = Buffer.alloc(width * height * 4);
  const rect = (x, y, w, h, [r, g, b]) => {
    for (let yy = Math.max(0, y); yy < Math.min(height, y + h); yy++) {
      for (let xx = Math.max(0, x); xx < Math.min(width, x + w); xx++) {
        const i = (yy * width + xx) * 4;
        data[i] = r;
        data[i + 1] = g;
        data[i + 2] = b;
        data[i + 3] = 255;
      }
    }
  };
  const rnd = lcg(20261031);
  const glyph = (x, y, colour) => {
    const tall = rnd() < 0.3;
    const top = tall ? y - 3 : y;
    const h = tall ? 12 : 9;
    if (rnd() < 0.85) rect(x, top, 1, h, colour);
    if (rnd() < 0.5) rect(x + 5, y, 1, 9, colour);
    if (rnd() < 0.6) rect(x, y, 6, 1, colour);
    if (rnd() < 0.5) rect(x, y + 4, 6, 1, colour);
    if (rnd() < 0.6) rect(x, y + 8, 6, 1, colour);
    if (rnd() < 0.25) rect(x + 2, y + 2, 2, 2, colour);
  };
  const line = (x0, x1, y, colour) => {
    let x = x0;
    while (x < x1 - 8) {
      const letters = 2 + Math.floor(rnd() * 8);
      for (let i = 0; i < letters && x < x1 - 8; i++, x += 7) glyph(x, y, colour);
      x += 5;
    }
  };
  rect(0, 0, width, height, [30, 30, 30]);
  rect(0, 0, width, 32, [45, 45, 45]);
  line(12, 400, 12, [204, 204, 204]);
  rect(0, 32, 260, height - 32, [37, 37, 38]);
  rect(260, 32, 1, height - 32, [70, 70, 70]);
  for (let y = 48; y < height - 24; y += 22) line(16, 16 + 60 + Math.floor(rnd() * 160), y, [157, 157, 157]);
  for (let y = 56; y < height * 0.55; y += 18) line(290, 290 + Math.floor((width - 340) * (0.4 + rnd() * 0.6)), y, [212, 212, 212]);
  // A table: one-pixel rules, text in its cells.
  const tableTop = Math.floor(height * 0.6);
  for (let r = 0; r <= 6; r++) rect(290, tableTop + r * 24, width - 340, 1, [80, 80, 80]);
  for (let c = 0; c <= 4; c++) rect(290 + Math.floor(((width - 340) * c) / 4), tableTop, 1, 6 * 24, [80, 80, 80]);
  for (let r = 0; r < 6; r++) {
    for (let c = 0; c < 4; c++) {
      const x = 298 + Math.floor(((width - 340) * c) / 4);
      line(x, x + 60 + Math.floor(rnd() * 100), tableTop + 8 + r * 24, [190, 190, 190]);
    }
  }
  // Two buttons.
  rect(290, height - 70, 140, 30, [14, 99, 156]);
  line(304, 420, height - 60, [255, 255, 255]);
  rect(450, height - 70, 140, 30, [58, 58, 58]);
  line(464, 580, height - 60, [230, 230, 230]);
  return { width, height, data };
}

/** `{ width, height, data }` → PNG bytes. */
export function encode(img) {
  const p = new PNG({ width: img.width, height: img.height });
  img.data.copy(p.data);
  return PNG.sync.write(p);
}

/** A part of an image (whole pixels, clamped to it). */
export function crop(img, x, y, w, h) {
  x = Math.max(0, x);
  y = Math.max(0, y);
  w = Math.max(0, Math.min(w, img.width - x));
  h = Math.max(0, Math.min(h, img.height - y));
  const data = Buffer.alloc(w * h * 4);
  for (let r = 0; r < h; r++) img.data.copy(data, r * w * 4, ((y + r) * img.width + x) * 4, ((y + r) * img.width + x + w) * 4);
  return { width: w, height: h, data };
}

/** Each output cell's share of the input pixels over [from, from + size). */
function coverage(from, size, out) {
  const step = size / out;
  return Array.from({ length: out }, (_, o) => {
    const a = from + o * step;
    const b = a + step;
    const parts = [];
    for (let i = Math.floor(a); i < Math.ceil(b); i++) {
      const w = (Math.min(b, i + 1) - Math.max(a, i)) / step;
      if (w > 0) parts.push([i, w]);
    }
    return parts;
  });
}

/** The part `sx, sy, sw, sh` (fractional) of `img` area-averaged to `dw` × `dh`. */
export function areaAverage(img, sx, sy, sw, sh, dw, dh) {
  const across = coverage(sx, sw, dw);
  const down = coverage(sy, sh, dh);
  const clampX = (i) => Math.min(img.width - 1, Math.max(0, i));
  const clampY = (i) => Math.min(img.height - 1, Math.max(0, i));
  const mid = new Float32Array(dw * img.height * 4);
  for (let y = 0; y < img.height; y++) {
    for (let x = 0; x < dw; x++) {
      for (const [i, w] of across[x]) {
        const s = (y * img.width + clampX(i)) * 4;
        for (let c = 0; c < 4; c++) mid[(y * dw + x) * 4 + c] += img.data[s + c] * w;
      }
    }
  }
  const data = Buffer.alloc(dw * dh * 4);
  for (let y = 0; y < dh; y++) {
    for (let x = 0; x < dw; x++) {
      const acc = [0, 0, 0, 0];
      for (const [j, w] of down[y]) for (let c = 0; c < 4; c++) acc[c] += mid[(clampY(j) * dw + x) * 4 + c] * w;
      for (let c = 0; c < 4; c++) data[(y * dw + x) * 4 + c] = Math.max(0, Math.min(255, Math.round(acc[c])));
    }
  }
  return { width: dw, height: dh, data };
}

/** Whether pixel x, y is inside the comparison (off the frame and its rounded corners). */
function inside(x, y, w, h, edge, corner) {
  if (x < edge || y < edge || x >= w - edge || y >= h - edge) return false;
  const nearX = x < corner || x >= w - corner;
  const nearY = y < corner || y >= h - corner;
  return !(nearX && nearY);
}

/** The edge contrast of an image's grey: mean |dx| + |dy| over the comparison. */
function edges(img, edge, corner) {
  const grey = (x, y) => {
    const i = (y * img.width + x) * 4;
    return 0.2126 * img.data[i] + 0.7152 * img.data[i + 1] + 0.0722 * img.data[i + 2];
  };
  let sum = 0;
  let n = 0;
  for (let y = 0; y < img.height - 1; y++) {
    for (let x = 0; x < img.width - 1; x++) {
      if (!inside(x, y, img.width - 1, img.height - 1, edge, corner)) continue;
      sum += Math.abs(grey(x + 1, y) - grey(x, y)) + Math.abs(grey(x, y + 1) - grey(x, y));
      n++;
    }
  }
  return n ? sum / n : 0;
}

/**
 * The drawn picture against the ideal (same size): `{ error, sharpness }`,
 * leaving out `edge` pixels all round (the frame's ring) and `corner`-pixel
 * squares in the corners (their rounding).
 */
export function compare(drawn, ideal, edge, corner) {
  let sum = 0;
  let n = 0;
  for (let y = 0; y < drawn.height; y++) {
    for (let x = 0; x < drawn.width; x++) {
      if (!inside(x, y, drawn.width, drawn.height, edge, corner)) continue;
      const i = (y * drawn.width + x) * 4;
      for (let c = 0; c < 3; c++) sum += Math.abs(drawn.data[i + c] - ideal.data[i + c]);
      n += 3;
    }
  }
  const ideallySharp = edges(ideal, edge, corner);
  return {
    error: n ? Math.round((sum / n) * 100) / 100 : 0,
    sharpness: ideallySharp ? Math.round((edges(drawn, edge, corner) / ideallySharp) * 1000) / 1000 : 0,
  };
}

/** An image made `k` times bigger, pixel for pixel (to look at). */
export function enlarge(img, k) {
  const w = img.width * k;
  const h = img.height * k;
  const data = Buffer.alloc(w * h * 4);
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const from = (Math.floor(y / k) * img.width + Math.floor(x / k)) * 4;
      img.data.copy(data, (y * w + x) * 4, from, from + 4);
    }
  }
  return { width: w, height: h, data };
}

/** Two images side by side, `gap` px of `[r, g, b]` between (and round the shorter). */
export function sideBySide(a, b, gap = 8, [r, g, bl] = [128, 128, 128]) {
  const w = a.width + gap + b.width;
  const h = Math.max(a.height, b.height);
  const data = Buffer.alloc(w * h * 4);
  for (let i = 0; i < w * h; i++) {
    data[i * 4] = r;
    data[i * 4 + 1] = g;
    data[i * 4 + 2] = bl;
    data[i * 4 + 3] = 255;
  }
  for (const [img, ox] of [
    [a, 0],
    [b, a.width + gap],
  ]) {
    for (let y = 0; y < img.height; y++) img.data.copy(data, (y * w + ox) * 4, y * img.width * 4, (y + 1) * img.width * 4);
  }
  return { width: w, height: h, data };
}
