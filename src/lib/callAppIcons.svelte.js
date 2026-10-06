// The call apps' own icons, as installed on this PC: { appId: data URL }
// from `meeting_app_icons` (src-tauri/src/app_icons.rs), for CallAppIcon.
// Each window asks the first time it draws one, and again when it draws one
// 10 minutes later (an app installed or updated meanwhile). Rust keeps the
// icons in <data>/icons and remakes one only when its app changes, so asking
// again is cheap. An app without an entry draws its bundled mark.
import { invoke } from '@tauri-apps/api/core';

export const installedIcons = $state({});

const FRESH_MS = 10 * 60_000;
let loadedAt = 0;
let loading = null;

/** Ask Rust for the icons, unless this window did lately or is asking now. */
export function loadInstalledIcons() {
  if (loading || (loadedAt && Date.now() - loadedAt < FRESH_MS)) return loading;
  loading = invoke('meeting_app_icons')
    .then((icons) => {
      for (const id of Object.keys(installedIcons)) if (!(id in icons)) delete installedIcons[id];
      Object.assign(installedIcons, icons);
    })
    .catch(() => {})
    .finally(() => {
      loadedAt = Date.now();
      loading = null;
    });
  return loading;
}
