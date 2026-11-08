// **Chat's status dots** (2026-11-07, after the Claude Code app's sidebar):
// each conversation in Chat's list says at a glance what it's doing — a
// soft pulse while it works, amber while something waits on the person, a
// blue dot when an answer came while it wasn't on screen, nothing when it's
// idle and seen. The rules, pure: stores/chatStatus.svelte.js keeps the
// inputs live and the list, its groups, the Chat | Code switch and the menu
// rail draw what comes out (views/chat/StatusDot.svelte).
//
// - **Needs you** (amber, steady; outranks the rest): an agent's permission
//   prompt (Allow once / Deny) or an agent asking (`asking`), an ask-first card
//   in its thread (`confirm`, Start / Don't, Allow once / Don't), or an agent's
//   turn that ended with a question for the person or a plan waiting for "go
//   ahead" — until they write in that chat after its report (`askedAt`), it
//   starts again, or a day has passed.
// - **Working** (a soft pulse): an agent of the chat's queued or running, or an
//   answer being written there (any turn: typed, the Voice Assistant's, a
//   phone's, an agent's report: Rust's list of the conversations answering).
//   A spell shorter than `WORKING_FLOOR_MS` never shows; two spells apart by
//   at most `WORKING_HOLD_MS` (an agent ending, then its report starting) are
//   one, so it never flickers off and on.
// - **New** (blue): its last answer (`answerAt`) is past the last one seen
//   there (`seen`, this window's storage), and it isn't the one on screen.
//   Opening it clears it.

/** A working spell this short never shows. */
export const WORKING_FLOOR_MS = 300;
/** A pause this short between two spells joins them (and a spell that ends shows this much longer). */
export const WORKING_HOLD_MS = 300;
/** An agent's question or plan still waits on the person for this long after its turn ended. */
export const WAITS_FOR_MS = 24 * 3600 * 1000;
/** This window's storage: `{v: 1, seen: {conversation: index}}`. */
export const SEEN_KEY = 'yap.chat.seen';

/** The states, strongest first. */
export const STATES = ['needs', 'working', 'new'];
const RANK = { needs: 3, working: 2, new: 1 };

/** An agent that isn't over: queued or running (asking is the person's). */
const working = (status) => status === 'queued' || status === 'running';

/** The index in a message id ("chat:<conversation>:<index>"), or -1. */
export function messageAt(id) {
  const m = /^chat:\d+:(\d+)$/.exec(String(id ?? ''));
  return m ? Number(m[1]) : -1;
}

/** The last answer's index among a thread's messages (by their saved ids), or -1. */
export function lastAnswerAt(messages) {
  let at = -1;
  for (const m of messages || []) if (m?.role === 'assistant' && m.id) at = Math.max(at, messageAt(m.id));
  return at;
}

/** The report marker (or else the request) of an agent's latest turn: its index in the chat, or -1. */
function latestTurnAt(agent) {
  const t = agent?.turnList?.length ? agent.turnList.at(-1) : null;
  const report = messageAt(t?.reportMessageId ?? agent?.reportMessageId);
  if (report >= 0) return report;
  return messageAt(t?.messageId ?? agent?.messageId);
}

/**
 * **Why a conversation waits on the person**, or null: `approve` (an agent's
 * step to allow), `confirm` (an ask-first card), `question` (an agent asks
 * something), `plan` (an agent's plan waits for "go ahead").
 * `conv`: its summary (`askedAt`), `agents` and `approvals` its own.
 */
function needsWhy({ conv, agents, approvals, confirms, now }) {
  if (approvals.length || agents.some((j) => j.status === 'asking')) return 'approve';
  if (confirms > 0) return 'confirm';
  for (const j of agents) {
    if (j.status !== 'done' || !(j.plan || j.asks)) continue;
    const ended = Number(j.endedTs) * 1000;
    if (ended && now - ended > WAITS_FOR_MS) continue;
    // The person wrote there after its report (or its request): answered.
    const at = latestTurnAt(j);
    if (conv && at >= 0 && Number(conv.askedAt ?? -1) > at) continue;
    return j.plan ? 'plan' : 'question';
  }
  return null;
}

/**
 * **The working sources now**, by conversation: `{conv: {since, why}}` —
 * an answer being written (`answering`: `{conv: sinceMs}`) or an agent queued
 * or running (`agentSince`: `{agentId: sinceMs}`, when it was first seen at
 * work). `why`: "agent" or "answer".
 */
export function workingNow({ answering = {}, agents = [], agentSince = {}, now = Date.now() } = {}) {
  const out = {};
  for (const [id, since] of Object.entries(answering || {})) out[id] = { since: Math.min(Number(since) || now, now), why: 'answer' };
  for (const j of agents || []) {
    if (j?.conversation == null || !working(j.status)) continue;
    const since = Math.min(Number(agentSince?.[j.id]) || now, now);
    const was = out[j.conversation];
    out[j.conversation] = { since: was ? Math.min(was.since, since) : since, why: 'agent' };
  }
  return out;
}

/**
 * **Working spells**, followed: `prev` and the sources now (`workingNow`) →
 * `{conv: {from, to, why}}` (`to` null while it works). A spell that starts
 * again within `WORKING_HOLD_MS` of its end carries on; one ended longer
 * ago is let go.
 */
export function trackWorking(prev = {}, raw = {}, now = Date.now()) {
  const next = {};
  for (const [id, r] of Object.entries(raw || {})) {
    const p = prev?.[id];
    const going = p && (p.to == null || now - p.to <= WORKING_HOLD_MS);
    next[id] = { from: going ? p.from : r.since, to: null, why: r.why };
  }
  for (const [id, p] of Object.entries(prev || {})) {
    if (next[id]) continue;
    if (p.to == null) next[id] = { ...p, to: now };
    else if (now - p.to <= WORKING_HOLD_MS) next[id] = p;
  }
  return next;
}

/** A spell shows: it has worked at least `WORKING_FLOOR_MS`, and isn't over by more than `WORKING_HOLD_MS`. */
export function showsWorking(spell, now = Date.now()) {
  if (!spell) return false;
  const end = spell.to ?? now;
  if (end - spell.from < WORKING_FLOOR_MS) return false;
  return spell.to == null || now - spell.to <= WORKING_HOLD_MS;
}

/** When a spell's dot may next change by the clock alone (ms), or null: the store's timer. */
export function nextChange(spells = {}, now = Date.now()) {
  let at = null;
  const soonest = (t) => {
    if (t > now && (at == null || t < at)) at = t;
  };
  for (const s of Object.values(spells || {})) {
    if (s.to == null) soonest(s.from + WORKING_FLOOR_MS);
    else soonest(s.to + WORKING_HOLD_MS + 1);
  }
  return at;
}

/**
 * **The last answer seen there**: `seen[id]`, unless it's past the
 * conversation's last answer (a deleted chat's id given to a new one), then
 * none (-1). A conversation never opened here: -1.
 */
export function seenAt(seen, conv) {
  const s = seen?.[conv?.id];
  if (!Number.isInteger(s)) return -1;
  return s > Number(conv?.answerAt ?? -1) ? -1 : s;
}

/**
 * **The state of each conversation**: `{id: {state, why}}` for those with
 * one (none: idle and seen).
 *
 * - `conversations`: Chat's list (`{id, answerAt, askedAt}`), or null while
 *   it isn't read yet (then every conversation the agents, cards and spells
 *   name, never "new");
 * - `agents`, `approvals` (`{conversation}`), `confirms` (`{conv: count}`);
 * - `spells` (`trackWorking`), `seen` (`{conv: index}`), `shown` (the
 *   conversation on screen, or null), `now`.
 *
 * `why`: needs → approve | confirm | question | plan; working → agent |
 * answer; new → report | answer.
 */
export function chatStatuses({ conversations = null, agents = [], approvals = [], confirms = {}, spells = {}, seen = {}, shown = null, now = Date.now() } = {}) {
  const byConv = (list) => {
    const m = new Map();
    for (const x of list || []) {
      if (x?.conversation == null) continue;
      const k = String(x.conversation);
      if (!m.has(k)) m.set(k, []);
      m.get(k).push(x);
    }
    return m;
  };
  const agentsOf = byConv(agents);
  const approvalsOf = byConv(approvals);
  const convs = conversations
    ? conversations
    : [...new Set([...agentsOf.keys(), ...approvalsOf.keys(), ...Object.keys(confirms || {}), ...Object.keys(spells || {})])].map((id) => ({ id: Number(id) }));
  const out = {};
  for (const conv of convs) {
    const id = String(conv.id);
    const mine = agentsOf.get(id) || [];
    const needs = needsWhy({ conv: conversations ? conv : null, agents: mine, approvals: approvalsOf.get(id) || [], confirms: Number(confirms?.[id]) || 0, now });
    if (needs) {
      out[id] = { state: 'needs', why: needs };
      continue;
    }
    const spell = spells?.[id];
    if (showsWorking(spell, now)) {
      out[id] = { state: 'working', why: spell.why || 'answer' };
      continue;
    }
    if (!conversations || String(shown) === id) continue;
    const answer = Number(conv.answerAt ?? -1);
    if (answer >= 0 && answer > seenAt(seen, conv)) {
      // The answer right after an agent's report marker: the report is in.
      const report = mine.some((j) => (j.turnList || [{ reportMessageId: j.reportMessageId }]).some((t) => messageAt(t?.reportMessageId) === answer - 1));
      out[id] = { state: 'new', why: report ? 'report' : 'answer' };
    }
  }
  return out;
}

/** **The strongest state among `ids`** (needs > working > new): `{state, why, count}`, or null. */
export function strongest(statuses, ids) {
  let best = null;
  let count = 0;
  for (const id of ids || []) {
    const s = statuses?.[String(id)];
    if (!s) continue;
    if (!best || RANK[s.state] > RANK[best.state]) {
      best = s;
      count = 1;
    } else if (s.state === best.state) count += 1;
  }
  return best ? { ...best, count } : null;
}

/** Any conversation in this state. */
export const any = (statuses, state) => Object.values(statuses || {}).some((s) => s.state === state);

/** **What a dot says** (its tooltip and accessible name). `count`: how many share it (a group, a side). */
export function statusWords(s, count = 1) {
  if (!s?.state) return '';
  const many = count > 1;
  switch (s.state) {
    case 'needs':
      if (many) return `${count} chats need you`;
      if (s.why === 'approve') return 'Needs you: an agent waits for you to allow a step';
      if (s.why === 'confirm') return 'Needs you: Yap is waiting for your OK';
      if (s.why === 'plan') return 'Needs you: an agent’s plan waits for you to go ahead';
      if (s.why === 'question') return 'Needs you: an agent has a question for you';
      return 'Needs you';
    case 'working':
      if (many) return `${count} chats working`;
      if (s.why === 'agent') return 'Working: an agent is running';
      return s.why === 'answer' ? 'Working: writing an answer' : 'Working';
    case 'new':
      if (many) return `${count} chats with something new`;
      return s.why === 'report' ? 'New: an agent’s report is in' : 'New: an answer you haven’t seen';
    default:
      return '';
  }
}

// ---- seen, kept in this window's storage ----

/** **Seen up to `at`** in conversation `id` (never back). A new object, or `seen` itself when nothing changed. */
export function markSeen(seen, id, at) {
  const k = String(id);
  const was = seen?.[k];
  if (id == null || !Number.isInteger(at) || at < 0 || (Number.isInteger(was) && was >= at)) return seen;
  return { ...(seen || {}), [k]: at };
}

/** **A first run**: every conversation's answers count as seen (nothing old lights up). */
export function seedSeen(conversations) {
  const out = {};
  for (const c of conversations || []) if (Number(c?.answerAt) >= 0) out[String(c.id)] = Number(c.answerAt);
  return out;
}

/** The conversations gone from the list let go (their ids may be given again). `seen` itself when nothing changed. */
export function pruneSeen(seen, conversations) {
  if (!conversations) return seen;
  const ids = new Set(conversations.map((c) => String(c.id)));
  const keys = Object.keys(seen || {});
  if (keys.every((k) => ids.has(k))) return seen;
  return Object.fromEntries(keys.filter((k) => ids.has(k)).map((k) => [k, seen[k]]));
}

/** `seen` from this window's storage's text (null: none kept yet). */
export function readSeen(text) {
  try {
    const v = JSON.parse(text);
    if (!v || v.v !== 1 || typeof v.seen !== 'object' || !v.seen) return null;
    return Object.fromEntries(Object.entries(v.seen).filter(([k, n]) => /^\d+$/.test(k) && Number.isInteger(n) && n >= 0));
  } catch {
    return null;
  }
}

/** `seen` as this window's storage keeps it. */
export const writeSeen = (seen) => JSON.stringify({ v: 1, seen: seen || {} });
