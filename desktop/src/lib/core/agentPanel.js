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
import { isLive, turnsOf, todosOf, statusWord, turnStatusWord, planWaitsNow, planNotUsed, PLAN_NOT_USED, PLAN_WAITS_S, money, modeLabel, modelLabel, titleOf } from './agents.js';
import { effortLabel } from './effort.js';

/** A plan still waits on the person this long after its turn ended (as Chat's status dots: core/chatStatus.js). */
export const WAITS_FOR_S = PLAN_WAITS_S;
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

/**
 * **The row's character's pose** (views/chat/crew/: Yap's crew): working
 * (walks, its Mode's prop moving), needs (stops, looks up, an amber "!"),
 * done (sits, ✓), stopped (grey, still), failed (a red badge), queued (faint).
 */
export function poseOf(status, started = true) {
  if (status === 'running') return 'working';
  if (status === 'asking') return 'needs';
  if (status === 'done') return 'done';
  if (status === 'failed') return 'failed';
  if (status === 'queued' && started) return 'working';
  if (status === 'queued') return 'queued';
  return 'stopped';
}

/** Turn `turn` is its agent's latest (an agent from before turns: its one). */
export const isLatestTurn = (agent, turn) => {
  const last = (agent?.turnList || []).at(-1);
  return !last || last.n === turn?.n;
};

/**
 * **Which group a turn is in** (2026-11-08: only what truly waits, live —
 * Chat's status dots agree, core/chatStatus.js): 'needs' (a permission
 * prompt it's blocked on, still waiting: `approval`; or its plan waiting for
 * Go ahead on its agent's latest turn, nothing newer begun in its chat:
 * core/agentNow.js `planWaitsNow`), 'running', 'finished' (a plan nobody
 * acted on too: "Plan not used"), or 'queued' (a request not sent yet:
 * behind the turn working, or never sent because the agent stopped first).
 * A turn that says it's asking with no prompt waiting is at work, as far as
 * anyone can tell: Running.
 */
export function stateOf(agent, turn, now = Date.now() / 1000, { approval = null } = {}) {
  const status = turn?.status;
  const started = Number(turn?.startedTs) > 0;
  if (status === 'asking' && approval) return 'needs';
  if (!started && (status === 'queued' || status === 'stopped' || status === 'interrupted')) return 'queued';
  if (isLive(status)) return 'running';
  if (status === 'done' && isLatestTurn(agent, turn) && planWaitsNow(agent, now)) return 'needs';
  return 'finished';
}

/**
 * **What a Needs you row waits for** (its card's own buttons, inline):
 * `{kind: 'approve', approval}` (Allow once / Deny, what it asks),
 * `{kind: 'plan', approval}` (a plan's prompt live: Go ahead / Not yet),
 * `{kind: 'goahead'}` (a plan its turn ended with: Go ahead / Not yet), or
 * null.
 */
export function needsOf(state, turn, approval) {
  if (state !== 'needs') return null;
  // (A question of Claude Code's own — AskUserQuestion — has its options on its card in the chat.)
  if (turn?.status === 'asking' && approval) return { kind: approval.tool === 'ExitPlanMode' ? 'plan' : approval.tool === 'AskUserQuestion' ? 'question' : 'approve', approval };
  return { kind: 'goahead' };
}

/**
 * **A row's third line**: "3/7 · Adding the minimap button" from its to-do
 * list; else the line it's on while it works (`line`), what waits on the
 * person, what stopped it, or how it ended ("Plan not used": `unused`).
 */
export function rowLine(agent, turn, { todos = [], line = '', state = '', unused = false } = {}) {
  const live = isLive(turn?.status);
  const c = todoCounts(todos, live);
  const status = turn?.status;
  if (state === 'queued') {
    return status === 'queued' ? 'Goes to the agent once it finishes this step.' : 'Not sent: the agent stopped first. Continue sends it.';
  }
  let words = '';
  if (status === 'asking' && state === 'needs') words = line || 'Waiting for you to allow a step';
  else if (live) words = (status !== 'asking' && line) || c.current || statusWord(status === 'asking' ? 'running' : status);
  else if (status === 'failed') words = turn?.error ? `Didn't finish · ${turn.error}` : "Didn't finish";
  else if (status === 'stopped' || status === 'interrupted') words = statusWord(status);
  else if (state === 'needs' && turn?.plan) words = 'Its plan is ready';
  else if (unused) words = PLAN_NOT_USED;
  else if (turn?.asks && isLatestTurn(agent, turn)) words = 'It has a question for you';
  if (!c.total) return words || statusWord(status);
  const count = `${c.done}/${c.total}`;
  if (live && status !== 'asking') return `${count} · ${c.current || line || statusWord(status)}`;
  if (words) return `${count} · ${words}`;
  return `${count} · ${c.done === c.total ? 'All done' : c.current || 'Done'}`;
}

/** The commands a turn ran (its events). */
export const commandCount = (events) => (events || []).filter((e) => typeof e?.command === 'string').length;

/**
 * **A row's character's pose**, as its row stands: Needs you looks up (a
 * plan waiting too, never sitting with a ✓), a plan nobody used stands
 * still and plain (`unused`: neither ✓ nor failed), a turn that says it's
 * asking with no prompt waiting is at work; else as its status
 * ([`poseOf`]).
 */
export function rowPose(state, turn, unused = false) {
  if (state === 'needs') return 'needs';
  if (unused) return 'unused';
  if (turn?.status === 'asking') return 'working';
  return poseOf(turn?.status, Number(turn?.startedTs) > 0);
}

/**
 * **The panel's rows**: one a turn of each of `agents` (the Code chat's: its
 * agent, and any other it has), each with what the panel shows of it, and
 * one an ask-first card waiting in its chat (`cards`: Send / Don't).
 * `logs`: agent id → its log (`yap-agent` events; none read yet: its turns'
 * own notes); `steps`: agent id → the line it's on now (stores/agents.svelte.js);
 * `approvals`: the permission prompts waiting (`{id, run, tool, summary,
 * detail}`: stores/agents.svelte.js), the live truth of a step to allow;
 * `dismissed`: show the rows dismissed too (each marked `dismissed`).
 */
export function panelRows(agents, { logs = {}, steps = {}, now = Date.now() / 1000, approvals = [], cards = [], dismissed = false } = {}) {
  const rows = [];
  for (const card of cards || []) {
    if (card?.id == null || card.state !== 'asking') continue;
    const j = card.agent || {};
    rows.push({
      key: `confirm:${card.id}`,
      kind: 'confirm',
      card,
      agentId: null,
      agent: null,
      turn: null,
      n: null,
      title: titleOf(j.brief || card.text) || card.label || 'An agent',
      request: j.brief || card.text || '',
      state: 'needs',
      needs: { kind: 'confirm', card },
      status: '',
      statusWord: 'Waiting for your OK',
      pose: 'needs',
      mode: j.mode || '',
      modeWord: j.modeLabel || modeLabel(j.mode),
      modeTone: modeTone(j.mode),
      model: '',
      modelWord: '',
      effortWord: '',
      sizing: '',
      todos: [],
      line: j.continues ? `Yap wants to tell the agent in ${card.label}` : `Yap wants to send an agent to ${card.label}`,
      progress: { value: 0, kind: 'context' },
      ctx: null,
      context: 0,
      tokens: 0,
      cost: 0,
      estimated: false,
      pay: 'plan',
      secs: 0,
      events: [],
      files: 0,
      commands: 0,
      createdTs: Number.MAX_SAFE_INTEGER,
      unused: false,
      dismissed: false,
    });
  }
  for (const agent of agents || []) {
    if (!agent?.id) continue;
    const turns = turnsOf(agent, logs[agent.id] || []);
    const newest = [...turns].reverse().find((t) => Number(t.startedTs) > 0)?.n ?? turns.at(-1)?.n;
    const asked = (approvals || []).find((a) => a?.run === agent.id) ?? null;
    for (const t of turns) {
      if (t.dismissed && !dismissed) continue;
      const live = isLive(t.status);
      const heard = todosOf(t.events);
      const todos = heard.length ? heard : t.todos || [];
      const approval = t.status === 'asking' && t.n === newest ? asked : null;
      const state = stateOf(agent, t, now, { approval });
      const unused = planNotUsed(t, state === 'needs');
      const line = live && t.n === newest ? steps[agent.id] || '' : '';
      const usage = t.usage || null;
      const cost = turnCost(t);
      const model = t.model || agent.model || '';
      rows.push({
        key: `${agent.id}:${t.n}`,
        kind: 'turn',
        agentId: agent.id,
        agent,
        turn: t,
        n: t.n,
        title: t.title || titleOf(t.request) || agent.folder || 'Agent',
        request: t.request || t.title || '',
        state,
        needs: needsOf(state, t, approval),
        unused,
        dismissed: !!t.dismissed,
        status: t.status,
        statusWord: unused ? PLAN_NOT_USED : turnStatusWord(t),
        pose: rowPose(state, t, unused),
        mode: t.mode || agent.mode || '',
        modeWord: modeLabel(t.mode || agent.mode),
        modeTone: modeTone(t.mode || agent.mode),
        model,
        modelWord: model ? shortModel(model) : '',
        effortWord: effortLabel(t.effort || agent.effort),
        // Why Chat sized it so (start_agent's `sizing`, agents/sizing.rs): its ▸ says it.
        sizing: t.sizing || '',
        todos,
        line: rowLine(agent, t, { todos, line, state, unused }),
        // (A plan nobody used never reads as done: its context in grey.)
        progress: unused && !todos.length ? { value: (ctxPercent(usage) ?? 0) / 100, kind: 'context' } : progressOf(t, todos),
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

/** The group a row shows in: its state's, a dismissed one shown again in Finished. */
export const groupOf = (r) => (r.dismissed ? 'finished' : r.state);

/** **The rows in their groups**, newest first in each; groups with none left out: `[{id, label, rows}]`. */
export function panelGroups(rows) {
  const newestFirst = [...(rows || [])].sort((a, b) => b.createdTs - a.createdTs || (b.n ?? 0) - (a.n ?? 0));
  return GROUPS.map(([id, label]) => ({ id, label, rows: newestFirst.filter((r) => groupOf(r) === id) })).filter((g) => g.rows.length);
}

/**
 * **A row's menu** (its ⋯ and right-click), what it offers: `stop` (its agent
 * at work: Stop, asked first), `finish` (Move to finished: in Needs you or
 * Running, but nothing runs its agent — `agent.active`, from the runner —
 * or a plan waiting), `dismiss` (hide it: never a turn at work), `restore`
 * (a dismissed one shown again), `goAhead` (a plan nobody used: Go ahead,
 * as in its details). None of them stops or changes an agent's work but
 * Stop. Pure.
 */
export function rowActions(r) {
  if (!r || r.kind !== 'turn') return { stop: false, finish: false, dismiss: false, restore: false, goAhead: false };
  const live = isLive(r.status);
  const active = !!r.agent?.active;
  return {
    stop: live && active,
    finish: (r.state === 'needs' || r.state === 'running') && !active && r.needs?.kind !== 'approve' && r.needs?.kind !== 'plan' && r.needs?.kind !== 'question',
    dismiss: !live && !r.dismissed,
    restore: r.dismissed,
    goAhead: r.unused && !isLive(r.agent?.status),
  };
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
