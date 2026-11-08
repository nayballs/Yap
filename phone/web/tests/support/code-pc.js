// Remote Control's PC, mocked (the Code side, wave 2): Yap on the PC as
// support/harness.js plays it, plus the Code side's messages — every one
// the PC sends made from **core/codeWire.js's own examples** (the file the
// PC's wire.rs is held to by its tests, served to the page as it is), and
// every one the page sends checked against the wire's example of its type
// (`conforms`): so the page, this mock and the PC can't drift apart.
// Fictional, as everything in this public repo.
import * as wire from '../../../../desktop/src/lib/core/codeWire.js';
import { mockPc, CAN, RECENTS, THREADS } from './harness.js';

export const { EXAMPLES, CODE_CAN, MIN_SHOWN_MS, PHONE_MODES, PHONE_TO_PC, PC_TO_PHONE } = wire;

/** What a PC with Remote Control says it can (`ready.can`). */
export const CAN_CODE = [...CAN, ...CODE_CAN];

const clone = (x) => JSON.parse(JSON.stringify(x));
const plain = (x) => x && typeof x === 'object' && !Array.isArray(x);
function merge(a, b) {
  for (const [k, v] of Object.entries(b || {})) {
    if (v === undefined) delete a[k];
    else if (plain(v) && plain(a[k])) merge(a[k], v);
    else a[k] = clone(v);
  }
  return a;
}

/** **A message the PC sends**: the wire's own example of `type`, with `over` merged in (arrays replaced). */
export function out(type, over = {}) {
  if (!EXAMPLES.out[type]) throw new Error(`${type} isn't one of the wire's PC → phone messages`);
  return merge(clone(EXAMPLES.out[type]), over);
}

/** Fields a phone message may leave out (the wire's examples have them). */
const OPTIONAL = { agent_diff: ['turn'], agent_undo: ['turn'] };

/**
 * **Does a message from the page keep to the wire?** Its type is one of
 * the Code side's, and its fields are the example's, of the same kinds
 * (null: it does). Pure.
 */
export function conforms(msg) {
  const ex = EXAMPLES.in[msg?.type];
  if (!ex) return `${msg?.type} isn't one of the wire's phone → PC messages`;
  for (const [k, v] of Object.entries(msg)) {
    if (!(k in ex)) return `${msg.type}.${k} isn't in the wire's example`;
    if (typeof v !== typeof ex[k]) return `${msg.type}.${k} is a ${typeof v}, the wire's a ${typeof ex[k]}`;
  }
  for (const k of Object.keys(ex)) {
    if (!(k in msg) && !(OPTIONAL[msg.type] || []).includes(k)) return `${msg.type} has no ${k}`;
  }
  return null;
}

// ---- the Code side's data (fictional) ----

const EX_LIST = EXAMPLES.out.code_list;
/** The agent started four minutes ago (its times are the screens' "4 min"). */
export const T0 = Math.floor(Date.now() / 1000) - 240;
/** garden-planner (the wire's example) and a second workspace. */
export const WORKSPACES = [...clone(EX_LIST.workspaces), { name: 'recipe-box', path: 'D:\\Projects\\recipe-box', allowed: true, usedTs: 1729990000 }];
/** The wire's example agent, done with its first turn (its report in chat 12). */
export const AGENT_ID = EX_LIST.agents[0].id;
export function brief(over = {}) {
  return merge(clone(EX_LIST.agents[0]), {
    status: 'done',
    now: null,
    startedTs: T0,
    endedTs: T0 + 190,
    reportMessageId: 'chat:12:2',
    turnList: [{ n: 1, title: 'Add a watering schedule', status: 'done', messageId: 'chat:12:0', reportMessageId: 'chat:12:2' }],
    ...over,
  });
}
/** The Code chats: the wire's example (chat 12 in garden-planner) and one in recipe-box. */
export const CHATS = [...clone(EX_LIST.chats), { id: 14, title: 'Fix the failing tests', ts: 1729990100, workspace: 'recipe-box' }];

/** A Code chat's `thread.code`, as the PC sends it (link/code.rs `thread_code`). */
export const CODE = { workspace: 'garden-planner', path: 'D:\\Projects\\garden-planner', mode: 'auto', modeLabel: 'Auto', allowed: true, phoneMax: 'auto' };

/** Chat 12 as `open_chat` gives it: the request, Yap's answer, the agent's report marker and its answer. */
export function codeThread(over = {}) {
  return {
    title: 'Watering schedule',
    messages: [
      { role: 'user', text: 'Add a watering schedule to notes.md' },
      { role: 'assistant', text: "On it: I've sent an agent to garden-planner.", id: 'chat:12:1' },
      {
        role: 'user',
        text: "The agent in garden-planner: it's done.\nFiles it changed: notes.md (backed up first).\nWhat it said: I added a watering schedule.",
        kind: 'agentReport',
      },
      { role: 'assistant', text: 'Done: I added a watering schedule to notes.md, with a row for each bed.', id: 'chat:12:3' },
    ],
    code: { ...CODE },
    agents: [brief()],
    cards: [],
    ...over,
  };
}

/** The agent's record as `agent_open`'s snapshot has it (agents.json's, as `agent_get` gives it). */
export function record(over = {}) {
  return {
    id: AGENT_ID,
    conversation: 12,
    messageId: 'chat:12:0',
    origin: 'typed',
    folder: 'garden-planner',
    path: 'D:\\Projects\\garden-planner',
    brief: 'Add a watering schedule to notes.md',
    model: 'claude-sonnet-5-5',
    effort: '',
    mode: 'auto',
    pay: 'plan',
    session: 'sess-1',
    status: 'running',
    createdTs: T0 - 10,
    startedTs: T0,
    endedTs: 0,
    updatedTs: T0 + 2,
    files: ['notes.md'],
    commands: ['npm test'],
    denials: [],
    text: '',
    cost: 0.04,
    turns: 1,
    plan: false,
    planAllowed: false,
    asks: false,
    reported: false,
    undone: false,
    noBackups: false,
    swept: false,
    diffLines: 6,
    turnList: [
      {
        n: 1,
        title: 'Add a watering schedule',
        request: 'Add a watering schedule to notes.md',
        messageId: 'chat:12:0',
        status: 'running',
        mode: 'auto',
        model: 'claude-sonnet-5-5',
        effort: '',
        startedTs: T0,
        endedTs: 0,
        cost: 0.04,
        files: ['notes.md'],
        denials: [],
        text: '',
        plan: false,
        planAllowed: false,
        asks: false,
        undone: false,
      },
    ],
    modelAsked: false,
    effortAsked: false,
    ...over,
  };
}

/** Its log as `agent_open` numbers it (`{line, event}`), diff lines left out (counts kept), as the PC strips them. */
export function logLines(n = 6) {
  const ev = (line, e) => ({ line, event: { id: AGENT_ID, status: 'running', folder: 'garden-planner', conversation: 12, turn: 1, ts: T0 + line, ...e } });
  const all = [
    ev(1, { step: 'Reading notes.md', file: null }),
    ev(2, { said: 'I will add a table with a row for each bed and the days to water it.' }),
    ev(3, { todos: [{ content: 'Read the notes', status: 'completed' }, { content: 'Add the schedule', status: 'in_progress', activeForm: 'Adding the schedule' }, { content: 'Run the tests', status: 'pending' }] }),
    ev(4, { step: 'Editing notes.md', file: 'notes.md' }),
    ev(5, { diff: { file: 'notes.md', kind: 'edit', lines: null, added: 4, removed: 1, more: true } }),
    ev(6, { step: 'Running npm test', command: 'npm test', about: 'Run the tests' }),
  ];
  return all.slice(0, n);
}

/** notes.md's diff, as `agent_diff` gives it. */
export const DIFF = ['@', ' # Garden notes', '-Water when dry', '+| Bed | Days |', '+| --- | --- |', '+| Tomatoes | Mon, Thu |', '+| Herbs | Daily |'];

/**
 * **Yap on the PC with Remote Control**: the mock PC of harness.js, its
 * `ready` saying the Code side is on (`code`: `{on: true}`, or off with
 * why), and the Code side's messages answered as the PC answers them
 * (`auto`: each by its type, true by default; a test sets one false to
 * answer it itself). Every Code message from the page is checked against
 * the wire (`pc.wireErrors`).
 */
export async function mockCodePc(page, opts = {}) {
  const code = {
    workspaces: clone(opts.workspaces || WORKSPACES),
    chats: clone(opts.chats || CHATS),
    agents: opts.agents ? clone(opts.agents) : [brief()],
    record: opts.record || record(),
    log: opts.log || logLines(),
    more: !!opts.more,
    older: opts.older || [],
    diff: opts.diff || DIFF,
    auto: { approve: true, confirm: true, act: true, open: true, mode: true, list: true, ...(opts.auto || {}) },
    wireErrors: [],
  };
  const threads = { ...THREADS, 12: codeThread(opts.thread || {}), ...(opts.threads || {}) };
  const recents = opts.recents || [...RECENTS, { id: 12, title: 'Watering schedule', ts: 1_793_800_000, code: 'garden-planner' }, { id: 14, title: 'Fix the failing tests', ts: 1_793_790_000, code: 'recipe-box' }];
  const pc = await mockPc(page, {
    ...opts,
    can: opts.can || CAN_CODE,
    code: opts.ready === undefined ? { on: true } : opts.ready,
    threads,
    recents,
    onText: (msg, conn, pc) => {
      if (!PHONE_TO_PC.includes(msg.type)) return;
      const err = conforms(msg);
      if (err) code.wireErrors.push(err);
      const say = (m) => pc.sendTo(conn, m);
      const acted = (action, message) => code.auto.act && say(out('acted', { action, id: msg.id, message }));
      switch (msg.type) {
        case 'code_list':
          if (code.auto.list) say(out('code_list', { workspaces: code.workspaces, chats: code.chats, agents: code.agents }));
          break;
        case 'code_new':
          say({ type: 'thread', id: null, title: '', messages: [], more: false, code: { ...CODE, workspace: msg.workspace, path: code.workspaces.find((w) => w.name === msg.workspace)?.path || '' } });
          break;
        case 'agent_open':
          if (code.auto.open) say(out('agent_snapshot', { agent: code.record, events: code.log, more: code.more, seq: 0 }));
          break;
        case 'agent_log':
          say(out('agent_log', { id: msg.id, events: code.older, more: false }));
          break;
        case 'agent_diff':
          say(out('agent_diff', { id: msg.id, file: msg.file, lines: code.diff, more: false, added: 4, removed: 1 }));
          break;
        case 'agent_stop':
          acted('stop', 'Stopping the agent in garden-planner.');
          break;
        case 'agent_continue':
          acted('continue', 'Continuing the agent in garden-planner.');
          break;
        case 'agent_go_ahead':
          acted('go_ahead', 'The agent in garden-planner is carrying out its plan.');
          break;
        case 'agent_message':
          acted('message', 'Passed that to the agent in garden-planner.');
          break;
        case 'agent_undo':
          if (!msg.sure) say({ type: 'error', message: "Confirm it on the phone first: Undo puts the agent's files back." });
          else acted('undo', 'Put back 1 file in garden-planner.');
          break;
        case 'approve':
          if (msg.shownMs < MIN_SHOWN_MS) say({ type: 'error', message: 'Tap it again: the card had only just come.' });
          else if (code.auto.approve) say(out('approve_done', { id: msg.id, outcome: msg.allow ? 'allowed' : 'denied', by: 'you', name: undefined }));
          break;
        case 'confirm':
          if (msg.shownMs < MIN_SHOWN_MS) say({ type: 'error', message: 'Tap it again: the card had only just come.' });
          else if (code.auto.confirm) say(out('confirm_done', { id: msg.id, outcome: msg.yes ? 'allowed' : 'refused', by: 'you' }));
          break;
        case 'set_mode':
          if (!PHONE_MODES.includes(msg.mode)) say({ type: 'error', message: 'From your phone, agents run at most at Auto: Bypass permissions is for the PC only.' });
          else if (code.auto.mode) say(out('mode', { chat: msg.chat, mode: msg.mode, label: { plan: 'Plan', manual: 'Manual', acceptEdits: 'Accept edits', auto: 'Auto' }[msg.mode], cap: undefined }));
          break;
        default:
          break;
      }
    },
  });
  pc.codeSide = code;
  /** The dots' makings, as the PC sends them on connect and on change. */
  pc.status = (over = {}) => pc.send(out('code_status', { seq: (pc.seq = (pc.seq || 0) + 1), answering: [], agents: code.agents, approvals: [], confirms: [], ...over }));
  return pc;
}

/** A permission card for chat 12's agent (the wire's example), `over` merged in. */
export const approveCard = (over = {}) => out('approve_card', over);
