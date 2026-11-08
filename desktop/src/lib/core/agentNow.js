// Whether a Yap Code agent is still going and the line it's on now: the
// part of core/agents.js the main window's chunk needs at start-up
// (stores/agents.svelte.js), kept apart so the rest of agents.js loads with
// the views that show agents (frontend.md, "One chunk per window").
// core/agents.js re-exports all three. No Svelte state: pure.

/** Not over yet: queued, running, or waiting for the person to allow a step. */
export const isLive = (status) => status === 'queued' || status === 'running' || status === 'asking';

/** The line of an agent whose plan waits for the person's Go ahead (agents/runner.rs `on_tick`). */
export const PLAN_WAITING = 'Waiting for you: its plan is ready';

/**
 * **The line an agent is on now**, from its log: its latest step, or the line
 * it moved to without one (`now`: the to-do in progress once a call came
 * back, the line it was on before it asked). '' once it's over or before
 * any.
 */
export function latestStep(log, status) {
  if (!isLive(status)) return '';
  // Waiting on the person's Allow / Deny: that, whatever line came after it
  // (a to-do's update can land a moment after the wait began) — a plan's
  // Go ahead / Not yet as its own line (agents/runner.rs).
  if (status === 'asking') {
    const asked = [...(log || [])].reverse().find((e) => typeof e?.step === 'string' && /^Waiting for you\b/.test(e.step));
    return asked?.step === PLAN_WAITING ? PLAN_WAITING : 'Waiting for you to allow a step';
  }
  let line = '';
  for (const e of log || []) {
    if (typeof e?.now === 'string' && e.now.trim()) line = e.now.trim();
    else if (typeof e?.step === 'string' && e.step.trim()) line = e.step.trim();
  }
  return line;
}
