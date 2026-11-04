// External-link helper, ported from OpenWhispr's utils/externalLinks.ts.
// A plain target="_blank" anchor does nothing inside a Tauri webview (new-window
// requests are denied), so every external link goes through Rust's
// `open_external` (the opener plugin; web and mail links only). A test run
// records the link there instead of opening a browser on the PC running it.
// Outside Tauri (plain-browser dev) it falls back to window.open.
import { invoke } from '@tauri-apps/api/core';

export async function openExternalLink(url) {
  if ('__TAURI_INTERNALS__' in window) {
    try {
      await invoke('open_external', { url });
    } catch (e) {
      console.warn("Couldn't open link:", e);
    }
    return;
  }
  window.open(url, '_blank', 'noopener,noreferrer');
}

export function createExternalLinkHandler(url) {
  return (e) => {
    e.preventDefault();
    openExternalLink(url);
  };
}
