// Your Claude plan in this window (ai/claude_plan.rs): the person's own
// Claude Code on this PC — found, its version, signed in — for Settings'
// Claude tab (ClaudePlanProvider.svelte), Chat's model picker and the
// "Sign in" a plan chat's problems offer. Rust owns it: `claude_plan_status`
// probes Claude Code (at most 30 s old, `refresh` to ask again) and keeps the
// key store's marker true, which follows through `yap-config-saved` like any
// other saved setting. Signing in is Claude Code's own (`claude auth login`
// in a terminal Yap opens): when this window gets the focus back after it,
// the status is asked again.
import { invoke } from '@tauri-apps/api/core';

export const claudePlan = $state({
  loaded: false,
  /** A probe is under way. */
  checking: false,
  installed: false,
  version: null,
  minVersion: '2.1.259',
  tooOld: false,
  loggedIn: false,
  /** Installed, new enough, signed in: answers can come on the plan. */
  ready: false,
  /** Signed in with an API key, not a plan. */
  onAKey: false,
  plan: null,
  error: null,
  usageUrl: 'https://claude.ai/settings/usage',
  installUrl: 'https://code.claude.com/docs/en/setup',
  /** A sign-in terminal was opened: the next focus asks again. */
  signingIn: false,
  /** A command's own failure (opening the terminal…). */
  failed: null,
});

function apply(s) {
  if (!s || typeof s !== 'object') return;
  Object.assign(claudePlan, {
    loaded: true,
    installed: !!s.installed,
    version: s.version ?? null,
    minVersion: s.minVersion || claudePlan.minVersion,
    tooOld: !!s.tooOld,
    loggedIn: !!s.loggedIn,
    ready: !!s.ready,
    onAKey: !!s.onAKey,
    plan: s.plan ?? null,
    error: s.error ?? null,
    usageUrl: s.usageUrl || claudePlan.usageUrl,
    installUrl: s.installUrl || claudePlan.installUrl,
  });
  if (s.loggedIn) claudePlan.signingIn = false;
}

let focusHooked = false;
/** After a sign-in in the terminal: the window's focus back asks again. */
function hookFocus() {
  if (focusHooked || typeof window === 'undefined') return;
  focusHooked = true;
  window.addEventListener('focus', () => {
    if (claudePlan.signingIn || (claudePlan.loaded && !claudePlan.ready)) loadClaudePlan(true);
  });
}

let inFlight = null;
/** When Claude Code was last asked afresh. */
let probedAt = 0;
/** A fresh look within this long of the last reads that one (Claude Code is two process starts). */
const FRESH_MS = 5000;

/**
 * Claude Code's status (`refresh`: probe again now, not the 30 s copy —
 * at most every 5 s: a card that mounts again and again, or the window's
 * focus coming and going, never starts Claude Code over and over; `now`:
 * the person's own Check again, always).
 */
export async function loadClaudePlan(refresh = false, now = false) {
  hookFocus();
  if (refresh && !now && Date.now() - probedAt < FRESH_MS) refresh = false;
  if (inFlight && !refresh) return inFlight;
  if (refresh) probedAt = Date.now();
  claudePlan.checking = true;
  inFlight = (async () => {
    try {
      apply(await invoke('claude_plan_status', { refresh }));
    } catch {
      /* not in Tauri */
    } finally {
      claudePlan.checking = false;
      inFlight = null;
    }
  })();
  return inFlight;
}

/** Sign in: Claude Code's own sign-in, in a terminal (never Yap's). */
export async function signInClaudePlan() {
  claudePlan.failed = null;
  try {
    await invoke('claude_plan_sign_in');
    claudePlan.signingIn = true;
    hookFocus();
  } catch (e) {
    claudePlan.failed = String(e?.message ?? e);
  }
}
