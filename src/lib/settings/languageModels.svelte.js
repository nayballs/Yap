// Settings → Language Models' state that outlives the page: Settings is
// always mounted, the page only while it's shown, and what's here stays as
// you left it when you come back (the tab, the key field open, an app half
// typed in Smart routing) — as it did when it was all Settings.svelte's.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { CLOUD_PROVIDER_IDS, modeOf } from '../core/providers.js';
import { config } from './configSync.svelte.js';

export const lm = $state({
  // The tab: a scope key (settings/scopes.js); `cleanup#<scope>` opens one.
  scope: 'cleanup',
  // Dictation cleanup's where-it-runs (OpenWhispr's split: a mode, then the
  // provider's tab). UI-only: the saved state is cfg.ppProvider (`providerUi`).
  mode: 'cloud',
  cloudProvider: 'groq',
  // The API key's field open; else it shows masked (OpenWhispr's).
  keyEditing: false,
  // The Chat tab remounts when its scope changed outside it: its provider
  // pills read the scope once (ScopeProviderConfig).
  chatScopeRev: 0,
  // The cleanup guardrails (llm::BASE_PROMPT), for Prompt Studio's View.
  basePrompt: '',
  // Smart routing: apps you've dictated into (from history), and the app
  // being added.
  recentApps: [],
  newRouteApp: '',
  // The day's Groq usage (camelCase from get_groq_usage / the groq-usage event).
  usage: { day: 0, tokens: 0, tokenCap: 500000, requests: 0, requestCap: 14400 },
});

/** The cleanup tab's mode and cloud tab, from the saved provider id. */
export function providerUi(cfg) {
  lm.mode = modeOf(cfg.ppProvider);
  if (CLOUD_PROVIDER_IDS.includes(cfg.ppProvider)) lm.cloudProvider = cfg.ppProvider;
}

/** The cleanup guardrails, read once (Settings' mount). */
export async function loadBasePrompt() {
  try {
    lm.basePrompt = await invoke('get_base_prompt');
  } catch {
    /* older backend — View shows the body alone */
  }
}

/**
 * Apps the person has actually dictated into, from local history, so Smart
 * routing's "Add rule" suggests real targets instead of asking them to type
 * "slack.exe". Read when Language Models opens, and after a rule goes.
 */
export async function loadRecentApps() {
  try {
    const entries = await invoke('get_history', { limit: 200 });
    const bound = new Set((config.cfg.appRoutes || []).map((r) => (r.app || '').toLowerCase()));
    const seen = new Map();
    for (const e of entries || []) {
      const app = (e.app || '').trim();
      if (!app || bound.has(app.toLowerCase())) continue;
      if (!seen.has(app.toLowerCase())) seen.set(app.toLowerCase(), app);
    }
    lm.recentApps = [...seen.values()].sort((a, b) => a.localeCompare(b));
  } catch {
    lm.recentApps = [];
  }
}

export async function refreshUsage() {
  try {
    lm.usage = await invoke('get_groq_usage');
  } catch {
    /* backend not ready / stub — leave defaults */
  }
}

/** The usage as dictations come in; resolves to the unlisten. */
export function listenUsage() {
  return listen('groq-usage', (e) => {
    if (e?.payload) lm.usage = e.payload;
  });
}
