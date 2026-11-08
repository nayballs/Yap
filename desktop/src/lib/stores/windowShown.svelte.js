// **Is this window on screen?** (shown, not minimised): what a page that
// animates while it's seen asks — the Agent panel's crew pauses while it
// isn't (views/chat/AgentPanel.svelte). WebView2 keeps
// `document.visibilityState` "visible" while its window is hidden (CLAUDE.md's
// gotchas; updates.svelte.js), so the window itself is asked: when its focus
// changes (hiding or minimising it takes the focus away, the tray and "Open
// Yap" give it back) and every 2 s while anything follows it (a window
// shown without the focus, `show_quietly`, sends no focus event). Followed
// only while someone asks (`followWindowShown`), so an idle window asks
// nothing.
import { getCurrentWindow } from '@tauri-apps/api/window';

export const windowShown = $state({ shown: true });

const POLL_MS = 2000;
let followers = 0;
let timer = null;
let unFocus = null;

async function check() {
  try {
    const win = getCurrentWindow();
    const [visible, minimized] = await Promise.all([win.isVisible(), win.isMinimized()]);
    const shown = visible && !minimized && document.visibilityState !== 'hidden';
    if (windowShown.shown !== shown) windowShown.shown = shown;
  } catch {
    if (!windowShown.shown) windowShown.shown = true;
  }
}

/** Follow this window's being on screen (`windowShown.shown`); returns the stop. */
export function followWindowShown() {
  followers += 1;
  if (followers === 1) {
    check();
    timer = setInterval(check, POLL_MS);
    document.addEventListener('visibilitychange', check);
    const pending = getCurrentWindow()
      .onFocusChanged(() => check())
      .catch(() => null);
    unFocus = () => pending.then((u) => u?.());
  }
  let stopped = false;
  return () => {
    if (stopped) return;
    stopped = true;
    followers -= 1;
    if (followers > 0) return;
    clearInterval(timer);
    timer = null;
    document.removeEventListener('visibilitychange', check);
    unFocus?.();
    unFocus = null;
  };
}
