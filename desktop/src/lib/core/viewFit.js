// **Yap's page fills its window** (18 Nov 2026; shell/view_fit.rs): the
// maintainer snapped the main window into a Snap layouts zone, maximised,
// minimised and restored it, and its page went on drawing at the snapped
// size, the rest of the window empty, until Chat was left and opened again.
// Rust fits the webview to its window once the window's size settles; this
// is the page's own check, for whatever slips past that: its size in device
// pixels told to Rust (`main_view_fit`) on a resize, the window's focus and
// every few seconds while it has it. Off by more than a rounding, it looks
// again a moment later (a page mid-resize lags its window by a frame) and
// only then has the webview fitted to the window again.

/** How often the page looks while its window has the focus. */
export const EVERY_MS = 2000;
/** How long after a first look that's off it looks again (and fits). */
export const AGAIN_MS = 400;
/** How long after the last resize it looks. */
export const SETTLE_MS = 250;

/** The page's own size in device pixels: what it's laid out at. */
export function pageSize(win = window) {
  const r = win.devicePixelRatio || 1;
  return { width: Math.round(win.innerWidth * r), height: Math.round(win.innerHeight * r) };
}

/**
 * Watch the page's size against its window's. `invoke`: Tauri's; `appWindow`:
 * the window (its resize and focus events). The way to stop, back.
 */
export function watchViewFit({ invoke, appWindow, win = window, every = EVERY_MS, again = AGAIN_MS, settle = SETTLE_MS }) {
  let stopped = false;
  let busy = false;
  let settleTimer = 0;
  const offs = [];

  const ask = (fit) => invoke('main_view_fit', { ...pageSize(win), fit }).catch(() => 'fine');

  /** One look; off, a second one a moment later that fits. */
  async function look() {
    if (stopped || busy) return;
    busy = true;
    try {
      if ((await ask(false)) !== 'off') return;
      await new Promise((done) => setTimeout(done, again));
      if (!stopped) await ask(true);
    } finally {
      busy = false;
    }
  }

  const soon = () => {
    clearTimeout(settleTimer);
    settleTimer = setTimeout(look, settle);
  };

  win.addEventListener('resize', soon);
  win.addEventListener('focus', soon);
  const timer = setInterval(() => {
    if (win.document?.hasFocus?.()) look();
  }, every);
  appWindow?.onResized?.(soon).then((u) => (stopped ? u() : offs.push(u))).catch(() => {});
  appWindow?.onFocusChanged?.(({ payload }) => payload && soon()).then((u) => (stopped ? u() : offs.push(u))).catch(() => {});
  soon();

  return () => {
    stopped = true;
    clearTimeout(settleTimer);
    clearInterval(timer);
    win.removeEventListener('resize', soon);
    win.removeEventListener('focus', soon);
    offs.forEach((u) => u());
  };
}
