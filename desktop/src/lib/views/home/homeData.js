// Home's data in one round trip (`home_snapshot`: its history page, stats,
// the settings it shows and the installed models), asked for before
// anything else the main window asks Rust for (2026-10-24). Home used to
// ask in four calls (get_history, get_stats, get_config, installed_models)
// queued behind the ~60 the window makes as it starts — 50 of them
// listeners — so it drew last (frontend.md, "Home's first paint").
import { invoke } from '@tauri-apps/api/core';

const LIMIT = 200;
let early = null;

/** Start fetching Home's data now (ControlPanel, first thing). */
export function prefetchHome() {
  early ??= invoke('home_snapshot', { limit: LIMIT });
  return early;
}

/** Home's data: what `prefetchHome` asked for the first time, fresh after. */
export function homeSnapshot() {
  if (early) {
    const p = early;
    early = null;
    return p;
  }
  return invoke('home_snapshot', { limit: LIMIT });
}
