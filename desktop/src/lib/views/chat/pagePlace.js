// **Where a page beside the chat goes** (PageTab.svelte; chat/pages/view.rs
// `page_view_place`), the pure parts (15 Nov 2026).
//
// - `nextSeq`: each place the view sends has a number that only rises (the
//   view's clock in microseconds, so across its tabs and a reload too): Rust
//   applies the newest and drops one that gets there late (view.rs `Latest`).
// - While the stage moves (the split or the list's edge dragged, Yap's window
//   resized by its edge), the page's own window can't keep up with the layout
//   around it frame by frame, so a still of the page stands in for it in the
//   stage — drawn by Yap's own page, it moves with everything else — and the
//   live page is laid out under Yap's page meanwhile (`layer: 'under'`). A
//   still taken ready (the pointer coming over a handle, the window's edge
//   pressed) is used if it's recent and the page's size (`stillFits`); the
//   strip a wider stage leaves at its right is the page's own background
//   (`edgeColour`).

let last = 0;

/** The next place's number (rising: never the same twice, never smaller). */
export function nextSeq(now = performance.timeOrigin + performance.now()) {
  last = Math.max(last + 1, Math.floor(now * 1000));
  return last;
}

/** How long a still taken ready may wait for the press (ms). */
export const STILL_FRESH_MS = 2000;

/** The least time between two stills taken while the stage moves (ms): what's in the still keeps up within it. */
export const REFRESH_MS = 50;

/**
 * A still taken ready that can stand in for the page now: taken within
 * `STILL_FRESH_MS` and of the page's size on screen (`bounds`, physical px).
 */
export function stillFits(still, bounds, now = performance.now()) {
  if (!still || !bounds) return false;
  return now - still.at <= STILL_FRESH_MS && still.w === bounds.w && still.h === bounds.h;
}

/**
 * Where the still sits in the stage so its pixels are the screen's (CSS px):
 * the page's window starts at the stage's place rounded to a device pixel
 * (`measure`), so the still is moved by what that rounding moved it.
 */
export function pixelShift(rect, dpr) {
  const d = dpr || 1;
  return { x: (Math.round(rect.left * d) - rect.left * d) / d, y: (Math.round(rect.top * d) - rect.top * d) / d };
}

/**
 * **The page's background, as its right edge has it**: the colour most of a
 * column of pixels `inset` from the right shows (clear of a scrollbar), as
 * `rgb(r, g, b)`; `fallback` when there's no telling. `px` is RGBA, one
 * pixel a row.
 */
export function edgeColour(px, fallback = '') {
  const counts = new Map();
  let best = '';
  let most = 0;
  for (let i = 0; i + 3 < px.length; i += 4) {
    if (px[i + 3] < 250) continue;
    const c = `rgb(${px[i]}, ${px[i + 1]}, ${px[i + 2]})`;
    const n = (counts.get(c) || 0) + 1;
    counts.set(c, n);
    if (n > most) {
      most = n;
      best = c;
    }
  }
  // A colour on at least a third of the column is the background; else no telling.
  return most * 4 * 3 >= px.length ? best : fallback;
}
