// Jobs in the main window (desktop/src-tauri/src/jobs.rs): every job
// (`job_list`, newest first), kept in step with the app-wide `yap-job`
// event — its status, files, commands and cost as they change, and the
// line it's on now (`steps`: its latest step, or the line it moved to
// without one, `now`; a job's own Job carries none: read from its log
// for the ones running when this window opened) —; the permission prompts
// running jobs are waiting on (`claude_approve_pending`, then
// `yap-claude-approve` / `-done`), answered only by a click; a report
// landing (`yap-job-report`, which never switches the open chat); and the
// bar card's or a notification's Open (`yap-job-open`: `openWanted`, which
// ChatView takes once it's showing that chat). Started once per window
// (ControlPanel, Chat, Settings → Jobs: `initJobs`).
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { isLive, latestStep } from '../core/jobs.js';

export const jobs = $state({
  /** Every job, newest first. */
  list: [],
  /** id → the line it's on now (live jobs). */
  steps: {},
  /** The permission prompts waiting, oldest first (`Pending`). */
  approvals: [],
  /** `{id, conversation}`: a job to show (its chat and tab), from the bar's or a notification's Open. */
  openWanted: null,
  /** job id → the turn whose plan a report card's Go ahead sent on (its button stays used). */
  wentAhead: {},
  /** The latest report (`yap-job-report`), and how many came: Chat reads the open chat again. */
  report: null,
  reports: 0,
  loaded: false,
});

let started = false;
let refreshTimer = null;

/** Read every job again (their live lines too, for the ones not followed yet). */
export async function refreshJobs() {
  try {
    jobs.list = (await invoke('job_list')) || [];
    jobs.loaded = true;
  } catch {
    return;
  }
  for (const j of jobs.list) {
    if (!isLive(j.status) || jobs.steps[j.id]) continue;
    invoke('job_get', { id: j.id })
      .then((r) => {
        const step = latestStep(r?.log, r?.job?.status);
        if (step && !jobs.steps[j.id]) jobs.steps = { ...jobs.steps, [j.id]: step };
      })
      .catch(() => {});
  }
}

function refreshSoon(ms = 150) {
  clearTimeout(refreshTimer);
  refreshTimer = setTimeout(refreshJobs, ms);
}

/** One `yap-job` event: the job as it is now, and its line. */
function onJob(ev) {
  if (!ev?.id) return;
  const i = jobs.list.findIndex((j) => j.id === ev.id);
  if (i < 0) {
    // A new job: its whole record (its message, its pay, its times).
    refreshSoon();
  } else {
    const was = jobs.list[i];
    const next = { ...was, status: ev.status ?? was.status };
    if (ev.file && !(was.files || []).includes(ev.file)) next.files = [...(was.files || []), ev.file];
    if (ev.command) next.commands = [...(was.commands || []), ev.command];
    if (typeof ev.cost === 'number') next.cost = ev.cost;
    if (ev.problem) next.problem = ev.problem;
    jobs.list = jobs.list.map((j, k) => (k === i ? next : j));
    // Over now, or started again — or a turn new to this window (a
    // follow-up, one queued behind the turn working): as saved.
    const newTurn = Number.isInteger(ev.turn) && !(was.turnList || []).some((t) => t.n === ev.turn);
    if (was.status !== next.status || newTurn || ev.queued) refreshSoon();
  }
  if (!isLive(ev.status)) {
    if (jobs.steps[ev.id]) jobs.steps = Object.fromEntries(Object.entries(jobs.steps).filter(([k]) => k !== ev.id));
  } else if (ev.now || ev.step) {
    // The line it's on: a new step, or one it moved to without one (`now`).
    jobs.steps = { ...jobs.steps, [ev.id]: ev.now || ev.step };
  }
}

async function loadApprovals() {
  try {
    jobs.approvals = (await invoke('claude_approve_pending')) || [];
  } catch {
    /* none yet */
  }
}

/** A prompt answered here (the thread's card): gone at once, then told. */
export async function answerApproval(id, allow) {
  jobs.approvals = jobs.approvals.filter((a) => a.id !== id);
  try {
    await invoke('claude_approve_answer', { id, allow: !!allow });
  } catch {
    /* it had lapsed: its job says so */
  }
}

/**
 * **Go ahead with a job's plan** (its report card's button: the person's
 * click): its next turn now. `turn`: the plan's, its button used from now
 * on (`wentAhead`), whatever the card's fate.
 */
export async function goAheadWithPlan(id, turn = 0) {
  jobs.wentAhead = { ...jobs.wentAhead, [id]: turn };
  try {
    return await invoke('job_go_ahead', { id });
  } finally {
    refreshSoon(0);
  }
}

/** The job `id` as this window has it. */
export const jobById = (id) => jobs.list.find((j) => j.id === id) ?? null;

/** Load once per window and follow the app's events. */
export function initJobs() {
  if (started) return;
  started = true;
  refreshJobs();
  loadApprovals();
  listen('yap-job', (e) => onJob(e.payload)).catch(() => {});
  listen('yap-job-report', (e) => {
    jobs.report = e.payload ?? null;
    jobs.reports += 1;
    refreshSoon();
  }).catch(() => {});
  listen('yap-job-open', (e) => {
    if (e.payload?.id) jobs.openWanted = { id: e.payload.id, conversation: e.payload.conversation ?? null };
  }).catch(() => {});
  listen('yap-claude-approve', (e) => {
    const p = e.payload;
    if (p?.id != null && !jobs.approvals.some((a) => a.id === p.id)) jobs.approvals = [...jobs.approvals, p];
  }).catch(() => {});
  listen('yap-claude-approve-done', (e) => {
    const id = e.payload?.id;
    if (id != null) jobs.approvals = jobs.approvals.filter((a) => a.id !== id);
  }).catch(() => {});
}
