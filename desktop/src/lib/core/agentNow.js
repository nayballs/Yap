// Whether a Yap Code agent is still going and the line it's on now: the
// part of core/agents.js the main window's chunk needs at start-up
// (stores/agents.svelte.js), kept apart so the rest of agents.js loads with
// the views that show agents (frontend.md, "One chunk per window").
// core/agents.js re-exports the first three. No Svelte state: pure.

/** Not over yet: queued, running, or waiting for the person to allow a step. */
export const isLive = (status) => status === 'queued' || status === 'running' || status === 'asking';

/**
 * **An event that ends the message an agent was writing** (its words as
 * it writes them: `yap-agent-saying`, stores/agentLogs.svelte.js): the
 * message whole (its `said` line, its final `text`), a step it moved on
 * to, or the agent over. Pure.
 */
export const sayingOver = (ev) => !!(ev?.said || ev?.text || ev?.step || !isLive(ev?.status));

/** The line of an agent whose plan waits for the person's Go ahead (agents/runner.rs `on_tick`). */
export const PLAN_WAITING = 'Waiting for you: its plan is ready';

/** A plan still waits on the person this long after its turn ended (seconds). */
export const PLAN_WAITS_S = 24 * 3600;

/**
 * **An agent's plan waits for the person's go-ahead, now** (2026-11-08: an
 * old plan stayed "Needs you" for ever, below a newer turn already
 * running). Its latest turn ended as a plan (`plan`, mirrored on the agent
 * as agents.json keeps it), not allowed, not set aside — "Plan not used":
 * a newer turn began in its chat (agents/store.rs `set_plans_aside`), or
 * the person put it aside or dismissed it — the agent isn't at work, and
 * it ended within the day (`now`: unix seconds). The one rule the Agent
 * panel, Chat's status dots and the phone's share (an agent in brief, the
 * phone's, has the same fields).
 */
export function planWaitsNow(agent, now = Date.now() / 1000) {
  if (!agent || agent.status !== 'done' || !agent.plan || agent.planAllowed || agent.planUnused) return false;
  const last = agent.turnList?.length ? agent.turnList.at(-1) : null;
  // (A read-only turn — "catch up", "have a look" — reports what it found: never a plan to approve.)
  if (last?.dismissed || last?.planUnused || last?.readOnly) return false;
  const ended = Number(agent.endedTs) || Number(last?.endedTs) || 0;
  return !ended || now - ended <= PLAN_WAITS_S;
}

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
