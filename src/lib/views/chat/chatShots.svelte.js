// Snip to Chat's screenshots waiting in Chat's message box, as Rust keeps
// them (snip/waiting.rs, 2026-10-25): a snip, a paste or a drop puts them
// there, the box's ✕ and "New chat" take them out, and a question sent —
// typed, or asked out loud with the Voice Assistant — takes them. Rust says
// every change (`yap-chat-waiting`), so the box and a spoken question
// always agree; the page changes its copy at once too, so the box never
// lags. Kept while you look at another view: ChatView is made again each
// time Chat opens. `{ list: [{ id, w, h, bytes, source }], copied }`:
// `copied` is the id of a snip just put on the clipboard too ("Copy every
// snip to the clipboard", `yap-snip-copied`), for the box's short line.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export const waitingShots = $state({ list: [], copied: null });

let following = false;
let copiedTimer = null;

/** Follow Rust's box (once per window), and read it now. */
export function followWaitingShots() {
  if (!following) {
    following = true;
    listen('yap-chat-waiting', (e) => {
      waitingShots.list = e.payload?.images || [];
    }).catch(() => {});
    listen('yap-snip-copied', (e) => {
      const id = e.payload?.id;
      if (!id) return;
      waitingShots.copied = id;
      clearTimeout(copiedTimer);
      copiedTimer = setTimeout(() => {
        if (waitingShots.copied === id) waitingShots.copied = null;
      }, 6000);
    }).catch(() => {});
  }
  invoke('chat_waiting_get')
    .then((w) => {
      waitingShots.list = w?.images || [];
    })
    .catch(() => {});
}

/** The box's ✕ on one. */
export function removeWaiting(id) {
  waitingShots.list = waitingShots.list.filter((a) => a.id !== id);
  invoke('chat_waiting_remove', { id }).catch(() => {});
}

/** "New chat", "Send without the screenshot": the box empties. */
export function clearWaiting() {
  waitingShots.list = [];
  invoke('chat_waiting_clear').catch(() => {});
}
