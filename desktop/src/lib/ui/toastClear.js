// **Toasts never cover a message box** (16 Nov 2026: Chat's notices stacked at the foot of the main window,
// over the message box's Send — Yap Lab had to close them to reach it). A box marks itself `use:keepClear`
// (Chat's composer, its side question and an edit in place, the notepad's Ask); ToastHost lifts its stack above
// every marked box under the toasts' column, as Material's snackbars rise above a bottom bar and its button —
// or, with no room left above the box, takes the stack to the window's top. A box elsewhere in the window (a
// wide window's centred composer, clear of the column) moves nothing.

/** The marked boxes on screen. */
const boxes = new Set();
/** ToastHost's measure, told when a box comes, goes or changes size. */
const listeners = new Set();

function changed() {
  for (const fn of listeners) {
    try {
      fn();
    } catch {
      /* a listener's own problem */
    }
  }
}

/** Svelte action: toasts stay clear of this element. */
export function keepClear(node) {
  boxes.add(node);
  const ro = typeof ResizeObserver === 'function' ? new ResizeObserver(changed) : null;
  ro?.observe(node);
  changed();
  return {
    destroy() {
      boxes.delete(node);
      ro?.disconnect();
      changed();
    },
  };
}

/** `fn` runs whenever a marked box comes, goes or changes size; returns its unsubscribe. */
export function onKeepClear(fn) {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

/** The marked boxes' rectangles now (those laid out: a hidden one has none). */
export function clearRects() {
  const out = [];
  for (const node of boxes) {
    if (!node.isConnected) continue;
    const r = node.getBoundingClientRect();
    if (r.width > 0 && r.height > 0) out.push({ left: r.left, top: r.top, right: r.right, bottom: r.bottom });
  }
  return out;
}

/**
 * **Where the toasts go** (pure): `view` the window's `{width, height}`; `base` the stack's own place
 * (`{right, bottom}`, the window's `--yap-toast-right` / `--yap-toast-bottom`), `top` the highest it may go
 * (under the title bar), `gap` its room above a box; `stack` its `{width, height}` now; `boxes` the marked
 * boxes' rectangles. `{bottom}`: from the window's foot — its own, or lifted above every box under its column —
 * or `{top}` when that leaves no room above the box. Never over a box either way, unless the window has no room
 * anywhere (then the top, where a box rarely is).
 */
export function placeToasts({ view, base, top = 56, gap = 12, stack, boxes: rects = [] }) {
  const right = view.width - base.right;
  const left = right - stack.width;
  // The boxes in the stack's column.
  const column = rects.filter((r) => r.left < right && r.right > left);
  let bottom = base.bottom;
  // Up above each box the stack would cover, until it covers none (a box above the composer, an edit in place,
  // may be next).
  for (let i = 0; i <= column.length; i++) {
    const from = view.height - bottom - stack.height;
    const to = view.height - bottom;
    const hit = column.filter((r) => r.top < to && r.bottom > from);
    if (!hit.length) break;
    bottom = Math.max(bottom, Math.ceil(view.height - Math.min(...hit.map((r) => r.top)) + gap));
  }
  if (view.height - bottom - stack.height >= top) return { bottom };
  return { top };
}
