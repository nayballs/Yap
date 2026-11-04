// The network receipts (src-tauri/src/privacy/net.rs) see every request Rust makes.
// The one connection a page makes itself is the signed-in account's picture
// (an <img> from the sign-in provider): it's noted here, by its host only,
// once per picture while the window is open (the browser caches it after).
import { invoke } from '@tauri-apps/api/core';

const noted = new Set();

/** The account picture at `url` loaded: note it in the network receipts. */
export function noteAccountPicture(url) {
  if (!url || noted.has(url) || !('__TAURI_INTERNALS__' in window)) return;
  noted.add(url);
  invoke('connections_note', { purpose: 'account-picture', url }).catch(() => {});
}
