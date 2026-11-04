// Reading PNGs in the e2e suite (Snip to Chat's saved screenshots and the
// copies Yap sends a model): Playwright's own bundled pngjs, no new package.
import utilsBundle from 'playwright-core/lib/utilsBundle';

const { PNG } = utilsBundle;

/** A PNG's bytes → `{ width, height, data }` (RGBA). */
export function readPng(bytes) {
  const p = PNG.sync.read(bytes);
  return { width: p.width, height: p.height, data: p.data };
}

/** A `width` × `height` PNG of one colour `[r, g, b]`, as bytes. */
export function writePng(width, height, [r, g, b]) {
  const p = new PNG({ width, height });
  for (let i = 0; i < p.data.length; i += 4) {
    p.data[i] = r;
    p.data[i + 1] = g;
    p.data[i + 2] = b;
    p.data[i + 3] = 255;
  }
  return PNG.sync.write(p);
}

/** The RGB of one pixel. */
export function pixel(png, x, y) {
  const i = (y * png.width + x) * 4;
  return [png.data[i], png.data[i + 1], png.data[i + 2]];
}

/** Whether a pixel is `[r, g, b]` within `tolerance` per channel. */
export function near(got, want, tolerance = 1) {
  return got.every((v, c) => Math.abs(v - want[c]) <= tolerance);
}

/** A `data:image/png;base64,…` URL → `{ width, height, data }`. */
export function readDataUrl(url) {
  return readPng(Buffer.from(String(url).replace(/^data:image\/png;base64,/, ''), 'base64'));
}

/**
 * The worst channel difference between every pixel of `png` and `[r, g, b]`
 * (0 = all exactly that colour).
 */
export function worstError(png, [r, g, b]) {
  let worst = 0;
  for (let i = 0; i < png.data.length; i += 4) {
    worst = Math.max(worst, Math.abs(png.data[i] - r), Math.abs(png.data[i + 1] - g), Math.abs(png.data[i + 2] - b));
  }
  return worst;
}
