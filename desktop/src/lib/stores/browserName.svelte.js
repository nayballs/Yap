// The browser Yap's links open in, by name, for buttons ("Open in Brave";
// shell/browsers.rs `browser_label`): Settings → General → Links' choice, or
// Windows' default when Yap can tell which it is, else "your browser". Read
// once, and again when the settings are saved (`yap-config-saved`) or a
// button asks (`refreshBrowserName`).
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export const browserName = $state({ label: 'your browser' });

let asked = null;
let following = false;

/** Ask Rust again (Settings changed the choice). */
export function refreshBrowserName() {
  asked = invoke('browser_label')
    .then((label) => {
      if (typeof label === 'string' && label.trim()) browserName.label = label.trim();
    })
    .catch(() => {});
  return asked;
}

/** Read it the first time something shows it, and follow the settings. */
export function useBrowserName() {
  if (!asked) refreshBrowserName();
  if (!following && typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window) {
    following = true;
    listen('yap-config-saved', () => refreshBrowserName()).catch(() => {});
  }
  return browserName;
}

/** "Open in Brave", "Open in your browser". */
export const openInLabel = (label) => `Open in ${label || 'your browser'}`;
