// **The Code side** (Remote Control, wave 2; docs/architecture/link.md
// "Remote Control", phone/web/README.md "The Code side"): Yap Code from the
// phone. What the PC says of it — the workspaces and their Code chats
// (`code_list`), what the status dots are made from (`code_status`) — kept
// here, and the dots worked out as the desktop works them out
// (core/chatStatus.js, with the phone's own "seen", `yap.link.seen`); the
// drawer's Code side (the agents working first, then a group per workspace,
// + New code chat); the sheets a Code chat's chips open (the Mode, up to
// Auto, never Bypass; the Environment); and the pure helpers the thread
// uses to find an answer's agent pills and a report's agent.
//
// Shown only while the PC says Remote Control is on for this phone
// (`ready.can` has "code", `ready.code.on`: core/codeWire.js `codeOn`). The
// PC checks every message anyway: this side's view is never trusted there.

import { codeOn, PHONE_MODES, PHONE_MAX_MODE, setMode } from './core/codeWire.js';
import {
  chatStatuses,
  trackWorking,
  workingNow,
  strongest,
  statusWords,
  markSeen,
  seedSeen,
  readSeen,
  writeSeen,
  nextChange,
  messageAt,
} from './core/chatStatus.js';
import { groupByWorkspace, CODE_NAME } from './core/codeChats.js';
import {
  isLive,
  statusWord,
  statusTone,
  elapsed,
  turnNow,
  pillsByAnswer,
  agentOfReport,
  turnOfReport,
  reportTitle,
  reportFolder,
  planWaits,
  modeLabel,
} from './core/agents.js';
import { h, svg, fill, ICONS } from './dom.js';

/** This phone's "seen" (`{v: 1, seen: {conversation: index}}`), as Chat keeps its own (`yap.chat.seen`). */
export const SEEN_KEY = 'yap.link.seen';

/**
 * What each Mode does, in Claude Code's words (agents/modes.rs `says`: the
 * PC's chip says the same). The phone offers these four, lowest first, and
 * never Bypass permissions (core/codeWire.js `PHONE_MODES`).
 */
export const MODE_SAYS = {
  auto: 'Claude handles permission decisions',
  manual: 'Always ask before making changes',
  acceptEdits: 'Automatically accept all file edits',
  plan: 'Create a plan before making changes',
};
/** The Mode sheet's order: Claude Code's menu, Bypass left out. */
export const MODE_MENU = ['auto', 'manual', 'acceptEdits', 'plan'];
export const BYPASS = 'bypassPermissions';
/** Said where the phone can't go higher. */
export const PHONE_MAX_WORDS = 'From your phone, agents run at most at Auto: Bypass permissions is for the PC only.';

/** A Mode's place, lowest first (Bypass above them all). Pure. */
export const modeRank = (m) => (m === BYPASS ? PHONE_MODES.length : PHONE_MODES.indexOf(m));

/**
 * **The Mode sheet's rows** for a Code chat's `code` (`thread.code`): each
 * of the phone's Modes, in Claude Code's order, with its words; one above
 * the most a phone may pick there (`phoneMax`: Auto, or the workspace's
 * cap) greyed with why. Pure.
 */
export function modeRows(code) {
  const max = code?.phoneMax && PHONE_MODES.includes(code.phoneMax) ? code.phoneMax : PHONE_MAX_MODE;
  return MODE_MENU.map((mode) => {
    const over = modeRank(mode) > modeRank(max);
    return {
      mode,
      label: modeLabel(mode),
      says: MODE_SAYS[mode],
      on: code?.mode === mode,
      allowed: !over,
      why: over ? (code?.cap ? `Never above ${modeLabel(code.cap)} in ${code.workspace}` : PHONE_MAX_WORDS) : '',
    };
  });
}

// ---- the thread's helpers (pure) ----

/**
 * **Each thread message's index in its chat** (`chat:<conv>:<index>`): an
 * answer says its own (`id`); the others are counted from their nearest
 * answer (the PC sends a run of the chat's latest messages, in order). A
 * thread with no answers: their places, when the PC sent the whole chat
 * (`more` false), else unknown (-1). Pure.
 */
export function chatIndexes(messages, more = false) {
  const list = Array.isArray(messages) ? messages : [];
  const out = list.map((m) => messageAt(m?.id));
  let next = -1;
  let nextAt = -1;
  for (let i = out.length - 1; i >= 0; i--) {
    if (out[i] >= 0) {
      next = out[i];
      nextAt = i;
    } else if (next >= 0) out[i] = next - (nextAt - i);
  }
  let prev = -1;
  let prevAt = -1;
  for (let i = 0; i < out.length; i++) {
    if (out[i] >= 0) {
      prev = out[i];
      prevAt = i;
    } else if (prev >= 0) out[i] = prev + (i - prevAt);
  }
  if (out.every((x) => x < 0) && !more) return list.map((_, i) => i);
  return out.map((x) => (x >= 0 ? x : -1));
}

/**
 * The thread's turns as the chat's messages (`pillsByAnswer`'s input): at
 * each turn's question and answer index, its role (a report marker's
 * kind), the rest left empty. Pure.
 */
export function sparseMessages(turns) {
  const out = [];
  for (const t of turns || []) {
    if (t.qAt >= 0) out[t.qAt] = { role: 'user', kind: t.report ? 'agentReport' : '' };
    if (t.aAt >= 0) out[t.aAt] = { role: 'assistant' };
  }
  return out;
}

/**
 * **A Code chat's agents**: the thread's (`thread.agents`), each as the PC
 * said it last (`code_status`, `code_list`), and any the PC listed since
 * for that chat. Pure.
 */
export function chatAgents(threadAgents, known, conversation) {
  const out = new Map();
  for (const a of threadAgents || []) if (a?.id) out.set(a.id, a);
  for (const a of known?.values?.() ?? known ?? []) {
    if (a?.id && (out.has(a.id) || a.conversation === conversation)) out.set(a.id, { ...(out.get(a.id) || {}), ...a });
  }
  return [...out.values()];
}

/** The agent pills under each answer: a Map from the answer's index to its pills (core/agents.js `pillsByAnswer`). Pure. */
export function pillsOf(agents, conversation, turns) {
  if (typeof conversation !== 'number') return new Map();
  return pillsByAnswer(agents, conversation, sparseMessages(turns));
}

/**
 * **A report card's facts** (`t`: a report turn — its marker's words in
 * `marker`, its index `qAt`): the agent it's about (as the desktop's card
 * finds it), its line ("Agent done", "The agent's plan is ready"…), its
 * dot's tone, the workspace, what it changed, and whether its plan still
 * waits for Go ahead. An agent in brief says its latest turn's plan and
 * question as the agent's own (`plan`, `asks`), so an earlier turn's
 * report reads from its marker's words. Pure.
 */
export function reportOf(agents, conversation, t) {
  const text = t?.marker || t?.q || '';
  const folder = reportFolder(text);
  const markerId = typeof conversation === 'number' && t?.qAt >= 0 ? `chat:${conversation}:${t.qAt}` : '';
  const agent = agentOfReport(agents, markerId, conversation, folder);
  const turn = turnOfReport(agent, markerId);
  const last = agent?.turnList?.at?.(-1) ?? null;
  const isLast = !!turn && !!last && turn.n === last.n;
  const latest = !!agent && !!markerId && agent.reportMessageId === markerId;
  const as = isLast ? { ...turn, plan: agent.plan, planAllowed: agent.planAllowed, asks: agent.asks } : turn ? null : latest ? agent : null;
  const title = reportTitle(as, text);
  const tone = as ? statusTone(as.status) : turn ? statusTone(turn.status) : /didn't finish/.test(title) ? 'failed' : 'done';
  const changed = (/^Files it changed: (.+)$/m.exec(text)?.[1] ?? '').replace(/\s*\(backed up first\)\.?$/, '');
  const waits = !!agent && planWaits(agent, isLast ? as : turn, latest);
  return { agent, turn, title, tone, folder: agent?.folder || folder, changed, waits };
}

/** The confirms as `chatStatuses` counts them (`{conv: n}`). Pure. */
function countByConv(list) {
  const out = {};
  for (const c of list || []) if (c?.conversation != null) out[c.conversation] = (out[c.conversation] || 0) + 1;
  return out;
}

/** An agent's line in the running list (RunningAgents.svelte's). */
export const runningLineOf = (a) => (a.status === 'asking' ? 'Waiting for you to allow a step' : a.now || a.turnTitle || statusWord(a.status));

/** How long it has been at its request now (its turn's time). */
export function tookOf(a, now = Date.now() / 1000) {
  const t = turnNow(a);
  return elapsed(t ? { ...t, status: a.status } : a, now);
}

// ---- the dot ----

/** A status dot (StatusDot.svelte's): working pulses (in the Code teal on the Code side), needs you amber, new blue. */
export function dot(s, { code = false, place = 'inline', testid = 'status-dot' } = {}) {
  const words = statusWords(s, s?.count ?? 1);
  return h('span', {
    class: `sdot ${place}${code ? ' teal' : ''}`,
    testid,
    data: { state: s?.state ?? '', why: s?.why ?? '' },
    role: s?.state && place === 'inline' ? 'img' : null,
    'aria-label': (place === 'inline' && words) || null,
    'aria-hidden': s?.state && place === 'inline' ? null : 'true',
    title: words || null,
  });
}

// ---- the Code side's state ----

/**
 * The Code side. `send(msg)` goes to the PC; `onChange()` redraws what
 * shows it (the drawer, the switch's dot, the banner); `shown()` is the
 * conversation on screen (an id or null); `openChat(id, title, workspace)`,
 * `openAgent(brief)` and `newCodeChat(workspace)` are the page's;
 * `pcName()` names the PC.
 */
export function createCode({ send, onChange, shown, openChat, openAgent, newCodeChat, pcName, store = globalThis.localStorage }) {
  const C = {
    on: false,
    listed: false,
    workspaces: [],
    chats: [],
    /** Every agent the PC has said, by id (briefs: `code_list`, `code_status`, a thread's). */
    agents: new Map(),
    /** The latest `code_status`: what the dots are made from. */
    status: null,
    answeringSince: {},
    agentSince: {},
    spells: {},
    /** What this phone saw of each chat: its last answer and question (from its thread). */
    known: {},
    /** The drawer's plain chats (their dots: an answer being written there). */
    plain: [],
    seen: loadSeen(),
    timer: 0,
    tick: 0,
  };

  function loadSeen() {
    try {
      return readSeen(store?.getItem(SEEN_KEY) ?? '');
    } catch {
      return null;
    }
  }
  function saveSeen() {
    try {
      store?.setItem(SEEN_KEY, writeSeen(C.seen || {}));
    } catch {
      /* private browsing: kept for this visit */
    }
  }

  const changed = () => onChange?.();

  /** `ready` (again): the Code side on or off for this phone. */
  function setReady(ready) {
    const was = C.on;
    C.on = codeOn(ready);
    if (C.on && !was) {
      C.listed = false;
      send({ type: 'code_list' });
    }
    if (!C.on) {
      C.status = null;
      C.spells = {};
      clearTimeout(C.timer);
    }
    if (was !== C.on) changed();
  }

  function mergeAgents(briefs) {
    for (const b of briefs || []) {
      if (!b?.id) continue;
      C.agents.set(b.id, { ...(C.agents.get(b.id) || {}), ...b });
    }
  }

  /** `code_list`: the workspaces, their Code chats, their agents. */
  function onList(m) {
    C.workspaces = Array.isArray(m.workspaces) ? m.workspaces.filter((w) => w && typeof w.name === 'string') : [];
    C.chats = Array.isArray(m.chats) ? m.chats.filter((c) => c && typeof c.id === 'number') : [];
    mergeAgents(m.agents);
    C.listed = true;
    // A first visit: what's there now counts as seen (nothing old lights up).
    if (!C.seen) {
      C.seen = seedSeen(conversations());
      saveSeen();
    }
    recompute();
  }

  /** `code_status`: the dots' makings, on connect and on change. */
  function onStatus(m) {
    if (C.status && Number(m.seq) > 0 && Number(m.seq) < Number(C.status.seq)) return; // an older one, late
    C.status = {
      seq: Number(m.seq) || 0,
      answering: Array.isArray(m.answering) ? m.answering.filter((x) => typeof x === 'number') : [],
      agents: Array.isArray(m.agents) ? m.agents.filter((a) => a?.id) : [],
      approvals: Array.isArray(m.approvals) ? m.approvals : [],
      confirms: Array.isArray(m.confirms) ? m.confirms : [],
    };
    mergeAgents(C.status.agents);
    const now = Date.now();
    const answering = {};
    for (const id of C.status.answering) answering[id] = C.answeringSince[id] ?? now;
    C.answeringSince = answering;
    const since = {};
    for (const a of C.status.agents) if (a.status === 'queued' || a.status === 'running') since[a.id] = C.agentSince[a.id] ?? now;
    C.agentSince = since;
    recompute();
  }

  /** The agents the dots read: the PC's latest list, else what's known. */
  const liveAgents = () => (C.status ? C.status.agents : [...C.agents.values()]);

  /**
   * The Code chats as `chatStatuses` reads them: `answerAt` (its last
   * answer as this phone knows it — the answer after its latest report, or
   * one its thread showed) and `askedAt` (the person's last words there).
   */
  function conversations() {
    const byConv = new Map();
    const see = (id) => {
      if (!byConv.has(id)) byConv.set(id, { id, answerAt: -1, askedAt: -1 });
      return byConv.get(id);
    };
    for (const c of C.chats) see(c.id);
    for (const id of C.plain) see(id);
    for (const a of C.agents.values()) {
      if (typeof a.conversation !== 'number') continue;
      const conv = see(a.conversation);
      for (const t of a.turnList?.length ? a.turnList : [{ reportMessageId: a.reportMessageId }]) {
        const at = messageAt(t?.reportMessageId);
        if (at >= 0) conv.answerAt = Math.max(conv.answerAt, at + 1);
      }
    }
    for (const [id, k] of Object.entries(C.known)) {
      const conv = see(Number(id));
      conv.answerAt = Math.max(conv.answerAt, k.answerAt ?? -1);
      conv.askedAt = Math.max(conv.askedAt, k.askedAt ?? -1);
    }
    for (const conv of byConv.values()) {
      const s = C.seen?.[String(conv.id)];
      if (Number.isInteger(s)) conv.answerAt = Math.max(conv.answerAt, s);
    }
    return [...byConv.values()];
  }

  function recompute() {
    const now = Date.now();
    C.spells = trackWorking(C.spells, workingNow({ answering: C.answeringSince, agents: liveAgents(), agentSince: C.agentSince, now }), now);
    clearTimeout(C.timer);
    const at = nextChange(C.spells, now);
    if (at != null) C.timer = setTimeout(recompute, Math.max(16, at - now));
    changed();
  }

  /** Each conversation's dot now: `{id: {state, why}}`. */
  function statuses() {
    if (!C.on) return {};
    return chatStatuses({
      conversations: conversations(),
      agents: liveAgents(),
      approvals: C.status?.approvals || [],
      confirms: countByConv(C.status?.confirms),
      spells: C.spells,
      seen: C.seen || {},
      shown: shown(),
      now: Date.now(),
    });
  }

  /**
   * **A thread this phone showed** (`thread`, or a message added to it):
   * its last answer and question, and seen up to its last answer.
   */
  function sawThread(id, { answerAt = -1, askedAt = -1 } = {}) {
    if (typeof id !== 'number') return;
    const k = C.known[id] || { answerAt: -1, askedAt: -1 };
    C.known[id] = { answerAt: Math.max(k.answerAt, answerAt), askedAt: Math.max(k.askedAt, askedAt) };
    const next = markSeen(C.seen || {}, id, C.known[id].answerAt);
    if (next !== C.seen) {
      C.seen = next;
      saveSeen();
    }
    changed();
  }

  /** A workspace by its name. */
  const workspaceOf = (name) => C.workspaces.find((w) => w.name === name) || null;
  /** Is conversation `id` a Code chat here (its workspace's name)? */
  const workspaceOfChat = (id) => C.chats.find((c) => c.id === id)?.workspace || '';

  /** The agents working now (the running list): not over. */
  const running = () => liveAgents().filter((a) => isLive(a.status));

  /** The Code side's dot on the drawer's switch: the strongest of its chats'. */
  function sideDot(side, statusesNow, plainIds = []) {
    const ids = side === 'code' ? [...new Set([...C.chats.map((c) => c.id), ...liveAgents().map((a) => a.conversation).filter((x) => typeof x === 'number')])] : plainIds;
    return strongest(statusesNow, ids);
  }

  // ---- the drawer's Code side ----

  /** The Code side's list into `root`: the agents working, then each workspace's chats; `close()` closes the drawer. */
  function renderList(root, close) {
    const st = statuses();
    const frag = document.createDocumentFragment();
    const now = Date.now() / 1000;
    const work = running();
    if (work.length) {
      const ul = h('ul', { class: 'running', 'aria-label': 'Agents working', testid: 'code-running' });
      for (const a of work) {
        const took = tookOf(a, now);
        ul.append(
          h(
            'li',
            {},
            h(
              'button',
              {
                type: 'button',
                class: 'runrow',
                testid: 'code-running-agent',
                data: { id: a.id, status: a.status },
                on: {
                  click: () => {
                    close();
                    openAgent(a);
                  },
                },
              },
              h('span', { class: `adot tone-${statusTone(a.status)}`, 'aria-hidden': 'true' }),
              h(
                'span',
                { class: 'runtext' },
                h('span', { class: 'runtop' }, h('span', { class: 'runfolder', text: a.folder }), took ? h('span', { class: 'runtime', text: took }) : null),
                h('span', { class: 'runline', text: runningLineOf(a) }),
              ),
            ),
          ),
        );
      }
      frag.append(h('h3', { class: 'recents-h', text: 'Agents working' }), ul);
    }
    const chats = {};
    for (const c of C.chats) {
      const ws = workspaceOf(c.workspace);
      chats[c.id] = { name: c.workspace, path: ws?.path || c.workspace };
    }
    const groups = groupByWorkspace(
      C.chats.map((c) => ({ id: c.id, title: c.title, updatedTs: c.ts })),
      chats,
    );
    for (const g of groups) {
      const ul = h('ul', { class: 'recents', 'aria-label': g.label });
      for (const c of g.items) {
        const s = st[String(c.id)] ?? null;
        ul.append(
          h(
            'li',
            {},
            h(
              'button',
              {
                type: 'button',
                class: 'recent',
                testid: 'code-chat',
                data: { id: c.id },
                'aria-current': c.id === shown() ? 'true' : null,
                on: {
                  click: () => {
                    close();
                    if (c.id !== shown()) openChat(c.id, c.title, g.label);
                  },
                },
              },
              svg(ICONS.folder),
              h('span', { class: 'recent-title', text: c.title || 'New chat' }),
              dot(s, { code: true, testid: 'code-chat-dot' }),
            ),
          ),
        );
      }
      frag.append(
        h(
          'div',
          { class: 'wsgroup', testid: 'code-workspace', data: { name: g.label } },
          h(
            'div',
            { class: 'wshead' },
            h('h3', { class: 'recents-h', text: g.label, title: g.path }),
            h(
              'button',
              {
                type: 'button',
                class: 'iconbtn small',
                testid: 'code-workspace-new',
                'aria-label': `New code chat in ${g.label}`,
                on: {
                  click: () => {
                    close();
                    newCodeChat(g.label);
                  },
                },
              },
              svg(ICONS.plus),
            ),
          ),
          ul,
        ),
      );
    }
    if (!groups.length && !work.length) {
      frag.append(
        h('p', {
          class: 'recents-empty',
          testid: 'code-empty',
          text: !C.listed
            ? 'Asking your PC…'
            : C.workspaces.length
              ? 'No Code chats yet. Start one with + New code chat.'
              : `Add a workspace in Yap on ${pcName()} first: Settings → Yap Code.`,
        }),
      );
    }
    root.replaceChildren(frag);
  }

  // ---- the workspace sheet (+ New code chat) ----

  /** The workspaces to start a Code chat in, most recently used first, into `root`. */
  function renderWorkspaces(root, pick) {
    const list = [...C.workspaces].sort((a, b) => (b.usedTs || 0) - (a.usedTs || 0));
    if (!list.length) {
      root.replaceChildren(h('p', { class: 'hint', testid: 'workspaces-empty', text: `Add a workspace in Yap on ${pcName()} first: Settings → Yap Code. Agents work only in folders you chose there.` }));
      return;
    }
    root.replaceChildren(
      h(
        'ul',
        { class: 'wslist' },
        list.map((w) =>
          h(
            'li',
            {},
            h(
              'button',
              { type: 'button', class: 'wsrow', testid: 'workspace', data: { name: w.name }, on: { click: () => pick(w.name) } },
              svg(ICONS.folder),
              h('span', { class: 'wstext' }, h('span', { class: 'wsname', text: w.name }), h('span', { class: 'wspath', text: w.path || '' })),
            ),
          ),
        ),
      ),
    );
  }

  // ---- the Mode sheet ----

  /**
   * The Mode sheet for a Code chat into `root`: its four Modes (above the
   * phone's most greyed, with why), the one in use ticked, and a chat set
   * to Bypass permissions on the PC said so. `pick(mode)` sends it.
   */
  function renderModes(root, code, pick, { readOnly = false } = {}) {
    const rows = modeRows(code);
    const bypass = code?.mode === BYPASS;
    fill(
      root,
      bypass ? h('p', { class: 'modenote caution', testid: 'mode-bypass', text: `This chat is set to Bypass permissions on ${pcName()}. ${PHONE_MAX_WORDS} You can pick a lower Mode here.` }) : null,
      h(
        'ul',
        { class: 'modelist', role: 'radiogroup', 'aria-label': 'Mode' },
        rows.map((r) =>
          h(
            'li',
            {},
            h(
              'button',
              {
                type: 'button',
                class: 'moderow',
                role: 'radio',
                'aria-checked': String(r.on),
                testid: 'mode-row',
                data: { mode: r.mode },
                disabled: readOnly || !r.allowed,
                on: { click: () => !r.on && pick(r.mode) },
              },
              h(
                'span',
                { class: 'modetext' },
                h('span', { class: 'modelabel' }, r.label, r.mode === 'auto' ? h('span', { class: 'modetag', text: 'Recommended' }) : null),
                h('span', { class: 'modesays', text: r.why || r.says }),
              ),
              h('span', { class: 'modecheck', 'aria-hidden': 'true', text: r.on ? '✓' : '' }),
            ),
          ),
        ),
      ),
      h('p', { class: 'hint', testid: 'mode-hint', text: readOnly ? `A new chat's Mode is set on ${pcName()} until its first request.` : 'From your phone, agents run at most at Auto.' }),
    );
  }

  /** `set_mode` for chat `chat` (the PC refuses anything higher than the phone may). */
  const sendMode = (chat, mode) => send(setMode(chat, mode));

  return {
    get on() {
      return C.on;
    },
    get listed() {
      return C.listed;
    },
    get workspaces() {
      return C.workspaces;
    },
    get chats() {
      return C.chats;
    },
    get agents() {
      return C.agents;
    },
    get status() {
      return C.status;
    },
    setReady,
    /** The drawer's plain chats (`chats`, those without a workspace). */
    setPlain(ids) {
      C.plain = (ids || []).filter((id) => typeof id === 'number');
    },
    onList,
    onStatus,
    mergeAgents,
    statuses,
    sideDot,
    sawThread,
    running,
    workspaceOf,
    workspaceOfChat,
    renderList,
    renderWorkspaces,
    renderModes,
    sendMode,
    /** Ask the PC for the list again (the drawer opening, a chat made, a report). */
    refresh: () => C.on && send({ type: 'code_list' }),
    /** The label on a Code chat's chip: "garden-planner". */
    label: (code) => (code?.workspace ? code.workspace : CODE_NAME),
  };
}
