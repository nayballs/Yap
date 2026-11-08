// Yap Code's agents in the main window (desktop/src-tauri/src/agents.rs): every agent
// (`agent_list`, newest first), kept in step with the app-wide `yap-agent`
// event — its status, files, commands and cost as they change, and the
// line it's on now (`steps`: its latest step, or the line it moved to
// without one, `now`; an agent's own Agent carries none: read from its log
// for the ones running when this window opened) —; the permission prompts
// running agents are waiting on (`claude_approve_pending`, then
// `yap-claude-approve` / `-done`), answered only by a click; a report
// landing (`yap-agent-report`, which never switches the open chat); and the
// bar card's or a notification's Open (`yap-agent-open`: `openWanted`, which
// ChatView takes once it's showing that chat). Started once per window
// (ControlPanel, Chat, Settings → Yap Code: `initAgents`).
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { isLive, latestStep } from '../core/agents.js';

export const agents = $state({
  /** Every agent, newest first. */
  list: [],
  /** id → the line it's on now (live agents). */
  steps: {},
  /** The permission prompts waiting, oldest first (`Pending`). */
  approvals: [],
  /** `{id, conversation}`: an agent to show (its chat and tab), from the bar's or a notification's Open. */
  openWanted: null,
  /** agent id → the turn whose plan a report card's Go ahead sent on (its button stays used). */
  wentAhead: {},
  /** The latest report (`yap-agent-report`), and how many came: Chat reads the open chat again. */
  report: null,
  reports: 0,
  loaded: false,
});

let started = false;
let refreshTimer = null;

/** Read every agent again (their live lines too, for the ones not followed yet). */
export async function refreshAgents() {
  try {
    agents.list = (await invoke('agent_list')) || [];
    agents.loaded = true;
  } catch {
    return;
  }
  for (const j of agents.list) {
    if (!isLive(j.status) || agents.steps[j.id]) continue;
    invoke('agent_get', { id: j.id })
      .then((r) => {
        const step = latestStep(r?.log, r?.agent?.status);
        if (step && !agents.steps[j.id]) agents.steps = { ...agents.steps, [j.id]: step };
      })
      .catch(() => {});
  }
}

function refreshSoon(ms = 150) {
  clearTimeout(refreshTimer);
  refreshTimer = setTimeout(refreshAgents, ms);
}

/** One `yap-agent` event: the agent as it is now, and its line. */
function onAgent(ev) {
  if (!ev?.id) return;
  const i = agents.list.findIndex((j) => j.id === ev.id);
  if (i < 0) {
    // A new agent: its whole record (its message, its pay, its times).
    refreshSoon();
  } else {
    const was = agents.list[i];
    const next = { ...was, status: ev.status ?? was.status };
    if (ev.file && !(was.files || []).includes(ev.file)) next.files = [...(was.files || []), ev.file];
    if (ev.command) next.commands = [...(was.commands || []), ev.command];
    if (typeof ev.cost === 'number') next.cost = ev.cost;
    if (ev.problem) next.problem = ev.problem;
    agents.list = agents.list.map((j, k) => (k === i ? next : j));
    // Over now, or started again — or a turn new to this window (a
    // follow-up, one queued behind the turn working): as saved.
    const newTurn = Number.isInteger(ev.turn) && !(was.turnList || []).some((t) => t.n === ev.turn);
    if (was.status !== next.status || newTurn || ev.queued) refreshSoon();
  }
  if (!isLive(ev.status)) {
    if (agents.steps[ev.id]) agents.steps = Object.fromEntries(Object.entries(agents.steps).filter(([k]) => k !== ev.id));
  } else if (ev.now || ev.step) {
    // The line it's on: a new step, or one it moved to without one (`now`).
    agents.steps = { ...agents.steps, [ev.id]: ev.now || ev.step };
  }
}

async function loadApprovals() {
  try {
    agents.approvals = (await invoke('claude_approve_pending')) || [];
  } catch {
    /* none yet */
  }
}

/** A prompt answered here (the thread's card): gone at once, then told. */
export async function answerApproval(id, allow) {
  agents.approvals = agents.approvals.filter((a) => a.id !== id);
  try {
    await invoke('claude_approve_answer', { id, allow: !!allow });
  } catch {
    /* it had lapsed: its agent says so */
  }
}

/**
 * **Go ahead with an agent's plan** (its report card's button: the person's
 * click): its next turn now. `turn`: the plan's, its button used from now
 * on (`wentAhead`), whatever the card's fate.
 */
export async function goAheadWithPlan(id, turn = 0) {
  agents.wentAhead = { ...agents.wentAhead, [id]: turn };
  try {
    return await invoke('agent_go_ahead', { id });
  } finally {
    refreshSoon(0);
  }
}

/** The agent `id` as this window has it. */
export const agentById = (id) => agents.list.find((j) => j.id === id) ?? null;

/** Load once per window and follow the app's events. */
export function initAgents() {
  if (started) return;
  started = true;
  refreshAgents();
  loadApprovals();
  listen('yap-agent', (e) => onAgent(e.payload)).catch(() => {});
  listen('yap-agent-report', (e) => {
    agents.report = e.payload ?? null;
    agents.reports += 1;
    refreshSoon();
  }).catch(() => {});
  listen('yap-agent-open', (e) => {
    if (e.payload?.id) agents.openWanted = { id: e.payload.id, conversation: e.payload.conversation ?? null };
  }).catch(() => {});
  listen('yap-claude-approve', (e) => {
    const p = e.payload;
    if (p?.id != null && !agents.approvals.some((a) => a.id === p.id)) agents.approvals = [...agents.approvals, p];
  }).catch(() => {});
  listen('yap-claude-approve-done', (e) => {
    const id = e.payload?.id;
    if (id != null) agents.approvals = agents.approvals.filter((a) => a.id !== id);
  }).catch(() => {});
}
