// Dictation cleanup's profiles and app rules (Smart routing,
// SmartRouting.svelte): ids, an app's default label, and the one-off
// migration of rules from before profiles.
import { appLabel } from '../core/appNames.js';

let idSeq = 1;
export function newId() {
  return globalThis.crypto?.randomUUID?.() ?? `p_${Date.now()}_${idSeq++}`;
}

// The app's own name for a default label ("slack.exe" → "Slack",
// "winword.exe" → "Word"; appNames.js).
export function prettyAppLabel(app) {
  return appLabel(app) || app;
}

/**
 * Migrate any legacy inline-body rules (pre-profiles) into named profiles so
 * the whole app now speaks "profiles". Runs once on load (Settings' mount).
 * Returns whether it changed anything (so the caller can persist — otherwise
 * it'd re-migrate and duplicate profiles on every load until the next save).
 */
export function migrateRoutesToProfiles(cfg) {
  if (!Array.isArray(cfg.cleanupProfiles)) cfg.cleanupProfiles = [];
  let migrated = false;
  for (const r of cfg.appRoutes || []) {
    if (!r.profileId && (r.prompt || '').trim()) {
      const id = newId();
      cfg.cleanupProfiles = [
        ...cfg.cleanupProfiles,
        {
          id,
          name: r.label || prettyAppLabel(r.app),
          prompt: r.prompt,
          provider: '',
          baseUrl: '',
          model: '',
          apiKey: '',
        },
      ];
      r.profileId = id;
      r.prompt = '';
      migrated = true;
    }
  }
  return migrated;
}
