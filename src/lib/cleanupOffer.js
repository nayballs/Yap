// The one gentle offer of AI cleanup (src-tauri/src/cleanup_styles.rs
// `offer`): to someone with cleanup off, on a PC whose graphics card runs
// the on-device model (or with that model already here), once they've done
// a few dictations. A tip in the main window — only while they're looking at
// it, never during a dictation, a meeting or the first-run setup (Rust asks
// the notice policy, notice.rs) — with "Set it up" (Settings → Language
// Models → Dictation cleanup) and "Not now". Shown once, whatever the
// answer: Rust remembers it (`cleanup_offer_seen`); "Show tips" off, never.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { toast } from './ui/toast.svelte.js';

let shown = false;

/** Watch for the moment: after a dictation is saved, and when the window comes to the front. */
export function initCleanupOffer({ openSettings }) {
  const check = async () => {
    if (shown) return;
    let s;
    try {
      s = await invoke('cleanup_offer_status');
    } catch {
      return;
    }
    if (!s?.offer || shown) return;
    shown = true;
    invoke('cleanup_offer_seen').catch(() => {});
    toast({
      kind: 'tip',
      chip: 'Tip',
      title: 'Let Yap tidy what you say',
      description: s.gpu
        ? `AI cleanup takes out the ums, fixes punctuation and writes each app's way: emails formal, chats casual. It runs on your ${s.gpu}, so nothing leaves this PC.`
        : "AI cleanup takes out the ums, fixes punctuation and writes each app's way: emails formal, chats casual. It runs on this PC, so nothing leaves it.",
      duration: 0,
      action: { label: 'Set it up', onClick: () => openSettings('cleanup#cleanup') },
      secondary: { label: 'Not now', onClick: () => {} },
    });
  };
  // A dictation was saved (history.rs): give its own toasts a moment.
  listen('yap-history-changed', () => setTimeout(check, 3000)).catch(() => {});
  // The window came to the front (Yap opened from the tray or the bar).
  try {
    getCurrentWindow()
      .onFocusChanged(({ payload }) => payload && setTimeout(check, 1500))
      .catch(() => {});
  } catch {
    /* not in Tauri */
  }
}
