// One round trip for a read several parts of a window ask for at the same
// moment (2026-10-24): as the main window starts, Appearance, Settings, the
// speech-model list and Home each asked for the settings (get_config ×4) and
// the installed models (×2), every one a sync command holding the window's
// thread in turn. A call made while the same one is still on its way gets
// its answer (a copy of it: callers change what they're given); a call after
// it has come back asks again, so nothing is ever older than the moment it
// was asked for (frontend.md, "Home's first paint").
import { invoke } from '@tauri-apps/api/core';

const inFlight = new Map();

/** `invoke(cmd)` (no arguments), shared with the same call still on its way. */
export function shared(cmd) {
  let p = inFlight.get(cmd);
  if (!p) {
    p = invoke(cmd);
    inFlight.set(cmd, p);
    const done = () => inFlight.delete(cmd);
    p.then(done, done);
  }
  return p.then((v) => (v && typeof v === 'object' ? structuredClone(v) : v));
}

/** The saved settings (`get_config`). */
export const getConfig = () => shared('get_config');

/** The installed speech models' ids (`installed_models`). */
export const installedModels = () => shared('installed_models');
