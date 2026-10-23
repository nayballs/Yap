// Reading PNGs in the e2e suite (Snip to Chat's saved screenshots and the
// copies Yap sends a model): Playwright's own bundled pngjs, no new package.
import utilsBundle from 'playwright-core/lib/utilsBundle';

const { PNG } = utilsBundle;

/** A PNG's bytes → `{ width, height, data }` (RGBA). */
export function readPng(bytes) {
  const p = PNG.sync.read(bytes);
  return { width: p.width, height: p.height, data: p.data };
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
