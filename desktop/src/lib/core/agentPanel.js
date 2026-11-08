// **The Agent panel's rows, groups and numbers** (views/chat/AgentPanel.svelte,
// the face of an agent's tab; Yap Code: desktop/src-tauri/src/agents/), and the
// thread's to-do card (views/chat/AgentTodoCard.svelte). One row a turn of the
// Code chat's agent (core/agents.js `turnsOf`), in groups — Needs you,
// Running, Finished, Queued — each row its title, its Mode · model · effort,
// "3/7 · the to-do in progress" (else the line it's on), its context's fill,
// tokens, cost and time, and a thin bar: its to-dos done, else its context's
// fill in grey. The header's totals add the rows up. What each turn used
// comes from Rust (agents/usage.rs: `turnList[].usage`, live as `yap-agent`'s
// `usage`). No Svelte state: pure, tested by desktop/e2e/agents-panel.spec.js.
//
// The layout, the stats line, the progress bar's context fallback and the
// number formats are adapted from savvy-progress's agents panel
// (hooks/register.tsx: `fmtTokens`, `fmtTime`, `ctxOf`, `progressOf`,
// `totals`), github.com/JohnnyVizz/claude-kit, under its MIT License:
//
//   MIT License
//
//   Copyright (c) 2026 johnnyvizz
//
//   Permission is hereby granted, free of charge, to any person obtaining a
//   copy of this software and associated documentation files (the
//   "Software"), to deal in the Software without restriction, including
//   without limitation the rights to use, copy, modify, merge, publish,
//   distribute, sublicense, and/or sell copies of the Software, and to permit
//   persons to whom the Software is furnished to do so, subject to the
//   following conditions:
//
//   The above copyright notice and this permission notice shall be included
//   in all copies or substantial portions of the Software.
//
//   THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
//   IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
//   FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL
//   THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
//   LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
//   FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
//   DEALINGS IN THE SOFTWARE.
import { isLive, turnsOf, todosOf, timelineOf, statusWord, turnStatusWord, planWaits, money, modeLabel, modelLabel, titleOf } from './agents.js';
import { effortLabel } from './effort.js';
import { familyIcon } from './modelIcons.js';

/** A question or a plan still waits on the person this long after its turn ended (as Chat's status dots: core/chatStatus.js). */
export const WAITS_FOR_S = 24 * 3600;
/** The latest steps a row opens to. */
export const LATEST_STEPS = 8;

/** Tokens as the panel says them: "980", "12k", "1.2M". */
export function fmtTokens(n) {
  const v = Math.max(0, Number(n) || 0);
  const k = Math.round(v / 1e3);
  if (v >= 1e6 || k >= 1000) return `${(v / 1e6).toFixed(1)}M`;
  if (v >= 1e3) return `${k}k`;
  return `${Math.round(v)}`;
}

/** A length of time as a clock: "0:42", "4:02", "1:05:09". */
export function fmtClock(secs) {
  const s = Math.max(0, Math.round(Number(secs) || 0));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const ss = String(s % 60).padStart(2, '0');
  return h ? `${h}:${String(m).padStart(2, '0')}:${ss}` : `${m}:${ss}`;
}

/** Every token a turn used, read and written (0: none said). */
export function tokensOf(usage) {
  if (!usage) return 0;
  return (Number(usage.input) || 0) + (Number(usage.output) || 0) + (Number(usage.cacheRead) || 0) + (Number(usage.cacheWrite) || 0);
}

/** **How full a turn's context is**, 0–100 (its latest call against the model's window), or null when not known. */
export function ctxPercent(usage) {
  const used = Number(usage?.context) || 0;
  const window = Number(usage?.window) || 0;
  if (!used || !window) return null;
  return Math.min(100, Math.round((used / window) * 100));
}

/** A to-do list's counts: `{done, total, current}` — `current` the one in progress (its `activeForm` while `live`), else null. */
export function todoCounts(todos, live = false) {
  const list = (todos || []).filter((t) => t && typeof t.content === 'string');
  const doing = list.find((t) => t.status === 'in_progress');
  return {
    done: list.filter((t) => t.status === 'completed').length,
    total: list.length,
    current: doing ? (live && doing.activeForm) || doing.content : null,
  };
}

/**
 * **A row's bar**: `{value, kind}` (0–1) — its to-dos done out of all of them
 * ('todos'); with no list, a done turn full ('done'), else its context's fill
 * ('context', drawn grey).
 */
export function progressOf(turn, todos) {
  const c = todoCounts(todos);
  if (c.total) return { value: c.done / c.total, kind: 'todos' };
  if (turn?.status === 'done') return { value: 1, kind: 'done' };
  const ctx = ctxPercent(turn?.usage);
  return { value: ctx == null ? 0 : ctx / 100, kind: 'context' };
}

/** How long a turn has worked (`now`: unix seconds): from its start to its end, or to now while it's live. */
export function turnSecs(turn, now = Date.now() / 1000) {
  const from = Number(turn?.startedTs) || 0;
  if (!from) return 0;
  const to = isLive(turn.status) ? now : Number(turn.endedTs) || from;
  return Math.max(0, to - from);
}

/** **What a turn cost**: Claude Code's own figure, and what it hasn't priced yet estimated (`usage.estimate`). */
export function turnCost(turn) {
  return (Number(turn?.cost) || 0) + (Number(turn?.usage?.estimate) || 0);
}

/** A cost as the panel says it: "≈ $0.04" on a plan (what the API would charge) or an estimate, "$0.04" on a key; '' for none. */
export function costWords(usd, { pay = 'plan', estimated = false } = {}) {
  if (!(Number(usd) > 0)) return '';
  return pay === 'key' && !estimated ? money(usd) : `≈ ${money(usd)}`;
}

/** A Claude model in the panel's few words: "claude-sonnet-5-5" → "Sonnet 5.5". */
export const shortModel = (id) => modelLabel(id).replace(/^Claude /, '');

/** The Mode's tone (its colour): auto, manual, edits, plan, bypass. */
export function modeTone(mode) {
  if (mode === 'bypassPermissions') return 'bypass';
  if (mode === 'acceptEdits') return 'edits';
  if (mode === 'plan' || mode === 'manual') return mode;
  return 'auto';
}

/** The avatar ring's state: running (moving), asking (amber), done (✓), stopped (grey), failed (red), queued (dashed). */
export function ringOf(status, started = true) {
  if (status === 'running') return 'running';
  if (status === 'asking') return 'asking';
  if (status === 'done') return 'done';
  if (status === 'failed') return 'failed';
  if (status === 'queued' && started) return 'running';
  if (status === 'queued') return 'queued';
  return 'stopped';
}

/**
 * **Which group a turn is in**: 'needs' (a card waits on the person: a step
 * to allow, a plan waiting for Go ahead, a question it asked), 'running',
 * 'finished', or 'queued' (a request not sent yet: behind the turn working,
 * or never sent because the agent stopped first).
 */
export function stateOf(agent, turn, now = Date.now() / 1000) {
  const status = turn?.status;
  const started = Number(turn?.startedTs) > 0;
  if (status === 'asking') return 'needs';
  if (!started && (status === 'queued' || status === 'stopped' || status === 'interrupted')) return 'queued';
  if (isLive(status)) return 'running';
  const last = (agent?.turnList || []).at(-1);
  const latest = !last || last.n === turn?.n;
  const ended = Number(turn?.endedTs) || 0;
  const recent = !ended || now - ended <= WAITS_FOR_S;
  if (latest && recent && !isLive(agent?.status) && status === 'done') {
    if (agent?.turnList?.length ? planWaits(agent, turn) : planWaits(agent, null, true)) return 'needs';
    if (turn.asks) return 'needs';
  }
  return 'finished';
}

/**
 * **A row's third line**: "3/7 · Adding the minimap button" from its to-do
 * list; else the line it's on while it works (`line`), what waits on the
 * person, what stopped it, or how it ended.
 */
export function rowLine(agent, turn, { todos = [], line = '', state = '' } = {}) {
  const live = isLive(turn?.status);
  const c = todoCounts(todos, live);
  const status = turn?.status;
  if (state === 'queued') {
    return status === 'queued' ? 'Goes to the agent once it finishes this step.' : 'Not sent: the agent stopped first. Continue sends it.';
  }
  let words = '';
  if (status === 'asking') words = line || 'Waiting for you to allow a step';
  else if (live) words = line || c.current || statusWord(status);
  else if (status === 'failed') words = turn?.error ? `Didn't finish · ${turn.error}` : "Didn't finish";
  else if (status === 'stopped' || status === 'interrupted') words = statusWord(status);
  else if (state === 'needs' && turn?.plan) words = 'Its plan is ready';
  else if (state === 'needs' && turn?.asks) words = 'It has a question for you';
  if (!c.total) return words || statusWord(status);
  const count = `${c.done}/${c.total}`;
  if (live && status !== 'asking') return `${count} · ${c.current || line || statusWord(status)}`;
  if (words) return `${count} · ${words}`;
  return `${count} · ${c.done === c.total ? 'All done' : c.current || 'Done'}`;
}

/** A turn's latest steps (its own events), newest last: `[{step, file, command, ts}]`. */
export function latestSteps(events, n = LATEST_STEPS) {
  return timelineOf(events)
    .filter((t) => t.kind === 'step')
    .slice(-n);
}

/** The commands a turn ran (its events). */
export const commandCount = (events) => (events || []).filter((e) => typeof e?.command === 'string').length;

/**
 * **The panel's rows**: one a turn of each of `agents` (the Code chat's: its
 * agent, and any other it has), each with what the panel shows of it.
 * `logs`: agent id → its log (`yap-agent` events; none read yet: its turns'
 * own notes); `steps`: agent id → the line it's on now (stores/agents.svelte.js).
 */
export function panelRows(agents, { logs = {}, steps = {}, now = Date.now() / 1000 } = {}) {
  const rows = [];
  for (const agent of agents || []) {
    if (!agent?.id) continue;
    const turns = turnsOf(agent, logs[agent.id] || []);
    const newest = [...turns].reverse().find((t) => Number(t.startedTs) > 0)?.n ?? turns.at(-1)?.n;
    for (const t of turns) {
      const live = isLive(t.status);
      const heard = todosOf(t.events);
      const todos = heard.length ? heard : t.todos || [];
      const state = stateOf(agent, t, now);
      const line = live && t.n === newest ? steps[agent.id] || '' : '';
      const usage = t.usage || null;
      const cost = turnCost(t);
      const model = t.model || agent.model || '';
      rows.push({
        key: `${agent.id}:${t.n}`,
        agentId: agent.id,
        agent,
        turn: t,
        n: t.n,
        title: t.title || titleOf(t.request) || agent.folder || 'Agent',
        request: t.request || t.title || '',
        state,
        status: t.status,
        statusWord: turnStatusWord(t),
        ring: ringOf(t.status, Number(t.startedTs) > 0),
        mode: t.mode || agent.mode || '',
        modeWord: modeLabel(t.mode || agent.mode),
        modeTone: modeTone(t.mode || agent.mode),
        model,
        modelWord: model ? shortModel(model) : '',
        icon: familyIcon(model) || 'claude',
        effortWord: effortLabel(t.effort || agent.effort),
        todos,
        line: rowLine(agent, t, { todos, line, state }),
        progress: progressOf(t, todos),
        ctx: ctxPercent(usage),
        context: Number(usage?.context) || 0,
        tokens: tokensOf(usage),
        cost,
        estimated: Number(usage?.estimate) > 0,
        pay: agent.pay || 'plan',
        secs: turnSecs(t, now),
        events: t.events || [],
        files: t.files?.length ?? 0,
        commands: commandCount(t.events),
        createdTs: Number(agent.createdTs) || 0,
      });
    }
  }
  return rows;
}

/** The groups' order and names. */
export const GROUPS = [
  ['needs', 'Needs you'],
  ['running', 'Running'],
  ['finished', 'Finished'],
  ['queued', 'Queued'],
];

/** **The rows in their groups**, newest first in each; groups with none left out: `[{id, label, rows}]`. */
export function panelGroups(rows) {
  const newestFirst = [...(rows || [])].sort((a, b) => b.createdTs - a.createdTs || b.n - a.n);
  return GROUPS.map(([id, label]) => ({ id, label, rows: newestFirst.filter((r) => r.state === id) })).filter((g) => g.rows.length);
}

/**
 * **The header's totals**: `{cost, approx, tokens, secs, known}` — every
 * row's cost (approximate on a plan, or while any of it is estimated), its
 * tokens (`known`: some turn said its usage) and how long the turns worked
 * (the time between them left out).
 */
export function panelTotals(rows) {
  const list = rows || [];
  return {
    cost: list.reduce((s, r) => s + r.cost, 0),
    approx: list.some((r) => r.pay !== 'key' || r.estimated),
    tokens: list.reduce((s, r) => s + r.tokens, 0),
    known: list.some((r) => r.tokens > 0),
    secs: list.reduce((s, r) => s + r.secs, 0),
  };
}

/** A row's stats at its right: "ctx 34% · 340k · ≈ $1.29 · 4:02" (each part only when known). */
export function statsLine(r) {
  const parts = [];
  if (r.ctx != null) parts.push(`ctx ${r.ctx}%`);
  if (r.context) parts.push(fmtTokens(r.context));
  const cost = costWords(r.cost, { pay: r.pay, estimated: r.estimated });
  if (cost) parts.push(cost);
  if (Number(r.turn?.startedTs) > 0) parts.push(fmtClock(r.secs));
  return parts.join(' · ');
}

/** The thread's to-do card, folded: "7 to-dos · all done", "7 to-dos · 5 done", "1 to-do · none done yet". */
export function todoSummary(todos) {
  const c = todoCounts(todos);
  const n = c.total === 1 ? '1 to-do' : `${c.total} to-dos`;
  if (c.done === c.total) return `${n} · all done`;
  return c.done ? `${n} · ${c.done} done` : `${n} · none done yet`;
}
