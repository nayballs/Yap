// The picture tab's zoom and pan (DocPanel's own chunk, PictureTab.svelte):
// where a picture sits on its stage at a scale. No Svelte state: pure.
//
// A view is `{ s, x, y }`: `s` the picture's device pixels per image pixel
// (1 = 1:1, one image pixel per device pixel), `x`/`y` its top-left corner
// on the stage in CSS px. At `ratio` device pixels per CSS px the picture is
// `w * s / ratio` CSS px wide. Its corner always sits on a whole device pixel,
// so at 1:1 every image pixel is one of the screen's.

/** The most it zooms in: this many device pixels per image pixel. */
export const MAX_SCALE = 8;
/** One step of + / − (and a key's). */
export const STEP = 1.25;
/** Room left round the picture when it fits (CSS px). */
export const FIT_PAD = 12;

const snap = (v, r) => Math.round(v * r) / r;

/** The scale at which `w` × `h` fits `roomW` × `roomH` CSS px: never above 1 (a small picture stays 1:1). */
export function fitScale(w, h, roomW, roomH, ratio = 1) {
  if (!(w > 0 && h > 0)) return 1;
  const r = ratio > 0 ? ratio : 1;
  // On whole device pixels, as chatImages.js `placed` asks for its thumbnail.
  const bw = Math.max(1, Math.round(Math.max(1, roomW) * r));
  const bh = Math.max(1, Math.round(Math.max(1, roomH) * r));
  const s = Math.min(bw / w, bh / h, 1);
  const dw = Math.max(1, Math.round(w * s));
  return dw / w;
}

/** The least and most a picture zooms: from fitting the stage to `MAX_SCALE`. */
export function limits(fit) {
  return { min: fit, max: Math.max(MAX_SCALE, fit) };
}

/**
 * `view` kept on the stage (`stageW` × `stageH` CSS px): a picture smaller
 * than the stage across is centred that way; a bigger one can't be dragged
 * off it (no empty stage past its edge).
 */
export function clampView(view, w, h, stageW, stageH, ratio = 1) {
  const r = ratio > 0 ? ratio : 1;
  const cw = (w * view.s) / r;
  const ch = (h * view.s) / r;
  const axis = (pos, size, room) => (size <= room ? (room - size) / 2 : Math.min(0, Math.max(room - size, pos)));
  return { s: view.s, x: snap(axis(view.x, cw, stageW), r), y: snap(axis(view.y, ch, stageH), r) };
}

/** The picture fitted to the stage, centred. */
export function fitView(w, h, stageW, stageH, ratio = 1) {
  const s = fitScale(w, h, stageW - 2 * FIT_PAD, stageH - 2 * FIT_PAD, ratio);
  return clampView({ s, x: 0, y: 0 }, w, h, stageW, stageH, ratio);
}

/**
 * `view` zoomed to scale `s` (kept within `limits(fit)`) around the stage
 * point `px`, `py` (CSS px): the image pixel under it stays under it.
 */
export function zoomAt(view, s, px, py, { w, h, stageW, stageH, ratio = 1, fit }) {
  const r = ratio > 0 ? ratio : 1;
  const { min, max } = limits(fit ?? view.s);
  const next = Math.min(max, Math.max(min, s));
  const ix = (px - view.x) / (view.s / r);
  const iy = (py - view.y) / (view.s / r);
  return clampView({ s: next, x: px - (ix * next) / r, y: py - (iy * next) / r }, w, h, stageW, stageH, r);
}

/** `view` dragged by `dx`, `dy` CSS px. */
export function panBy(view, dx, dy, { w, h, stageW, stageH, ratio = 1 }) {
  return clampView({ s: view.s, x: view.x + dx, y: view.y + dy }, w, h, stageW, stageH, ratio);
}

/** Whether `s` is the fit (a hair either way). */
export const isFit = (s, fit) => Math.abs(s - fit) < 1e-6;

/**
 * Double-click's scale: fitted, 1:1 (a picture that fits at 1:1 already
 * goes to 2×); zoomed, back to the fit.
 */
export function toggleScale(s, fit) {
  if (!isFit(s, fit)) return fit;
  return fit < 1 - 1e-6 ? 1 : Math.min(2, limits(fit).max);
}

/** The scale one step in (`dir` 1) or out (-1) from `s`, within the limits. */
export function stepScale(s, dir, fit) {
  const { min, max } = limits(fit);
  return Math.min(max, Math.max(min, dir > 0 ? s * STEP : s / STEP));
}

/** The wheel's scale from `s` for a wheel event's `deltaY` (`lines`: deltaMode 1). */
export function wheelScale(s, deltaY, lines = false) {
  const d = Math.max(-300, Math.min(300, lines ? deltaY * 33 : deltaY));
  return s * Math.exp(-d * 0.0022);
}

/** "Fit", or the scale against 1:1 ("250%"). */
export function zoomLabel(s, fit) {
  return isFit(s, fit) ? 'Fit' : `${Math.round(s * 100)}%`;
}
