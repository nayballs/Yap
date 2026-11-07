// Your Claude plan's usage limits in this window (ai/claude_plan/usage.rs):
// the plan's limits as rows, extra usage, and each plan conversation's
// context, for Chat's usage ring (views/chat/PlanUsageRing.svelte) and
// Settings' plan card. Rust owns them, in memory only: `claude_plan_usage`
// reads them (`refresh`: Claude Code asked, at most once a minute — on a
// free warm session, else a one-off probe), and `yap-claude-plan-usage`
// brings every change (after each plan turn, a turn's rate-limit events).
//
// Asked only while something shows them (`watchPlanUsage`): when it mounts,
// and when this window gets the focus back. Never on a timer.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export const planUsage = $state({
  /** The plan's limits ({ subscription, limits, extra }); null: no ring. */
  usage: null,
  /** Each plan conversation's context, by its id: { used, window, known }. */
  contexts: {},
  loaded: false,
});

function apply(s) {
  if (!s || typeof s !== 'object') return;
  planUsage.usage = s.usage && typeof s.usage === 'object' ? s.usage : null;
  planUsage.contexts = s.contexts && typeof s.contexts === 'object' ? s.contexts : {};
  planUsage.loaded = true;
}

/** Things showing the numbers now (a ring, the card). */
let watchers = 0;
let hooked = false;
function hook() {
  if (hooked || typeof window === 'undefined') return;
  hooked = true;
  listen('yap-claude-plan-usage', (e) => apply(e.payload)).catch(() => {});
  window.addEventListener('focus', () => {
    if (watchers > 0) loadPlanUsage(true);
  });
}

/** The page's own once-a-minute (Rust keeps it too): IPC stays quiet. */
const EVERY_MS = 60_000;
let askedAt = 0;

/** The numbers (`refresh`: Claude Code asked, when it's due). */
export async function loadPlanUsage(refresh = false) {
  hook();
  if (refresh && Date.now() - askedAt < EVERY_MS) refresh = false;
  if (refresh) askedAt = Date.now();
  try {
    apply(await invoke('claude_plan_usage', { refresh }));
  } catch {
    /* not in Tauri */
  }
}

/** Something that shows the numbers mounted: asked now, and on each focus while it's up. Its undo. */
export function watchPlanUsage() {
  watchers += 1;
  loadPlanUsage(true);
  let done = false;
  return () => {
    if (!done) watchers -= 1;
    done = true;
  };
}
