// Chat's status dots (desktop/src/lib/core/chatStatus.js, the rules, pure;
// stores/chatStatus.svelte.js, live; views/chat/StatusDot.svelte, drawn by
// ConversationList, ChatSideSwitch and ControlPanel's rail), after the
// Claude Code app's sidebar: a soft pulse while a chat works, amber while
// something waits on the person, blue when an answer came while it wasn't on
// screen. Over Rust's list of the conversations answering
// (chat/stream.rs `chat_answering`, `yap-chat-answering`) and Chat's list
// (`chats_list`'s `answerAt` / `askedAt`).
//
// - core/chatStatus.js, no app: each state, which wins, the floor under a
//   short working spell and the hold that joins two, what "seen" keeps, the
//   words.
// - An answer being written in a chat you've left: its row pulses, then is
//   blue once the answer's in; opening it clears it. Reduced motion: no
//   pulse. Light and dark.
// - A job running in a Code chat you've left: its row pulses in the Code
//   teal; a folded workspace shows it; the Chat | Code switch's Code shows it
//   from the Chat side; the rail's Chat from another view.
// - A permission prompt: amber in the row and on the rail (from every view),
//   cleared on Allow once.
// - An ask-first card: amber, cleared on Don't.
// - A job's report landing while you're in another chat: blue ("a job's
//   report is in"), cleared on opening it.
// Jobs are driven as jobs-ui.spec.js drives them: the scripted fake AI calls
// the job tools, Claude Code is this build's own stand-in, the workspaces
// are in this run's own folder.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, setConfig, sidebar } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { RUNS } from './support/yap.js';
import { toSide, newCodeChat, sideSwitch } from './support/code-chats.js';
import {
  WORKING_FLOOR_MS,
  WORKING_HOLD_MS,
  WAITS_FOR_MS,
  messageAt,
  lastAnswerAt,
  workingNow,
  trackWorking,
  showsWorking,
  nextChange,
  seenAt,
  chatStatuses,
  strongest,
  any,
  statusWords,
  markSeen,
  seedSeen,
  pruneSeen,
  readSeen,
  writeSeen,
} from '../src/lib/core/chatStatus.js';

const HERE = path.join(RUNS, 'chat-status-spec');
const SCRIPTS = path.join(HERE, 'scripts');
const GARDEN = path.join(HERE, 'folders', 'garden-planner');
const RECIPES = path.join(HERE, 'folders', 'recipe-box');
const STARTED = "On it: I've started a job in garden-planner.";

// ---------------------------------------------------------------- no app

base.describe('core/chatStatus.js', () => {
  const T = 1_000_000;
  const conv = (id, answerAt = -1, askedAt = -1) => ({ id, answerAt, askedAt });

  base('each state: working, needs you, new — and nothing when idle and seen', () => {
    const conversations = [conv(1, 1, 0), conv(2, 3, 2), conv(3, 5, 4), conv(4, 1, 0)];
    const spells = trackWorking({}, workingNow({ answering: { 1: T - 1000 }, now: T }), T);
    const s = chatStatuses({ conversations, spells, seen: { 1: 1, 2: 3, 3: 1, 4: 1 }, now: T });
    expect(s['1']).toEqual({ state: 'working', why: 'answer' });
    // Seen up to its last answer: nothing.
    expect(s['2']).toBeUndefined();
    // An answer past the last seen: new.
    expect(s['3']).toEqual({ state: 'new', why: 'answer' });
    expect(s['4']).toBeUndefined();

    // A job queued or running: working ("job").
    const jobs = [{ id: 'j1', conversation: 4, status: 'running' }];
    const jspells = trackWorking({}, workingNow({ jobs, jobSince: { j1: T - 5000 }, now: T }), T);
    expect(chatStatuses({ conversations, jobs, spells: jspells, seen: { 4: 1 }, now: T })['4']).toEqual({ state: 'working', why: 'job' });
    const queued = [{ id: 'j1', conversation: 4, status: 'queued' }];
    expect(workingNow({ jobs: queued, jobSince: { j1: T - 5000 }, now: T })['4']).toEqual({ since: T - 5000, why: 'job' });

    // Needs you: a step to allow (the prompt, or the job asking), an ask-first card.
    const base = { conversations, seen: { 1: 1, 2: 3, 3: 5, 4: 1 }, now: T };
    expect(chatStatuses({ ...base, approvals: [{ id: 7, conversation: 2 }] })['2']).toEqual({ state: 'needs', why: 'approve' });
    expect(chatStatuses({ ...base, jobs: [{ id: 'j', conversation: 2, status: 'asking' }] })['2']).toEqual({ state: 'needs', why: 'approve' });
    expect(chatStatuses({ ...base, confirms: { 3: 1 } })['3']).toEqual({ state: 'needs', why: 'confirm' });
    expect(chatStatuses({ ...base, confirms: { 3: 0 } })['3']).toBeUndefined();

    // A job's turn ended with a question, or a plan waiting for "go ahead":
    // needs you until the person writes there after its report.
    const asks = { id: 'q', conversation: 3, status: 'done', asks: true, endedTs: T / 1000 - 60, turnList: [{ n: 1, messageId: 'chat:3:0', reportMessageId: 'chat:3:4' }] };
    const plan = { ...asks, id: 'p', asks: false, plan: true };
    const replied = [conv(1, 1, 0), conv(2, 3, 2), conv(3, 5, 6), conv(4, 1, 0)];
    expect(chatStatuses({ ...base, conversations: [conv(3, 5, 2)], jobs: [asks] })['3']).toEqual({ state: 'needs', why: 'question' });
    expect(chatStatuses({ ...base, conversations: [conv(3, 5, 2)], jobs: [plan] })['3']).toEqual({ state: 'needs', why: 'plan' });
    expect(chatStatuses({ ...base, conversations: replied, jobs: [plan] })['3']).toBeUndefined();
    // Its report still being written: its request is the mark (the person's own message is no answer).
    const unreported = { ...plan, turnList: [{ n: 1, messageId: 'chat:3:2' }] };
    expect(chatStatuses({ ...base, conversations: [conv(3, 1, 2)], jobs: [unreported] })['3']).toEqual({ state: 'needs', why: 'plan' });
    // A day on, it's let go; a failed or running job never asks this way.
    expect(chatStatuses({ ...base, conversations: [conv(3, 5, 2)], jobs: [{ ...plan, endedTs: (T - WAITS_FOR_MS - 1000) / 1000 }] })['3']).toBeUndefined();
    expect(chatStatuses({ ...base, conversations: [conv(3, 5, 2)], jobs: [{ ...plan, status: 'failed' }] })['3']).toBeUndefined();

    // The answer right after a job's report marker: "a job's report is in".
    const reported = { id: 'r', conversation: 3, status: 'done', turnList: [{ n: 1, reportMessageId: 'chat:3:4' }] };
    expect(chatStatuses({ ...base, seen: { 3: 3 }, jobs: [reported] })['3']).toEqual({ state: 'new', why: 'report' });
    // (A job from before turns: its own marker.)
    expect(chatStatuses({ ...base, seen: { 3: 3 }, jobs: [{ id: 'o', conversation: 3, status: 'done', reportMessageId: 'chat:3:4' }] })['3']).toEqual({ state: 'new', why: 'report' });
  });

  base('which wins: needs you over working over new; a group’s and a side’s strongest', () => {
    const conversations = [conv(1, 3, 2), conv(2, 3, 2), conv(3, 3, 2)];
    const spells = { 1: { from: T - 5000, to: null, why: 'answer' }, 2: { from: T - 5000, to: null, why: 'job' } };
    const s = chatStatuses({ conversations, spells, approvals: [{ conversation: 2 }], seen: {}, now: T });
    // 1: working and new → working; 2: working and needs → needs; 3: new.
    expect([s['1'].state, s['2'].state, s['3'].state]).toEqual(['working', 'needs', 'new']);
    expect(strongest(s, [1, 3])).toEqual({ state: 'working', why: 'answer', count: 1 });
    expect(strongest(s, [1, 2, 3])).toMatchObject({ state: 'needs', count: 1 });
    expect(strongest(s, [3])).toMatchObject({ state: 'new', count: 1 });
    expect(strongest({ 4: { state: 'new', why: 'answer' }, 5: { state: 'new', why: 'report' } }, [4, 5])).toMatchObject({ state: 'new', count: 2 });
    expect(strongest(s, [9])).toBeNull();
    expect(strongest(s, [])).toBeNull();
    expect([any(s, 'needs'), any(s, 'working'), any({}, 'needs')]).toEqual([true, true, false]);
    // Before Chat's list is read: what the jobs and spells say, never "new".
    const early = chatStatuses({ conversations: null, spells, approvals: [{ conversation: 2 }], now: T });
    expect(early).toEqual({ 1: { state: 'working', why: 'answer' }, 2: { state: 'needs', why: 'approve' } });
  });

  base('clearing on open: the chat on screen is never new, and seen stays seen', () => {
    const conversations = [conv(1, 5, 4), conv(2, 7, 6)];
    expect(chatStatuses({ conversations, seen: {}, now: T })).toMatchObject({ 1: { state: 'new' }, 2: { state: 'new' } });
    // On screen: not new.
    expect(chatStatuses({ conversations, seen: {}, shown: 1, now: T })['1']).toBeUndefined();
    // Seen up to its last answer, then left: still not new; a later answer is.
    let seen = markSeen({}, 1, 5);
    expect(seen).toEqual({ 1: 5 });
    expect(chatStatuses({ conversations, seen, now: T })['1']).toBeUndefined();
    expect(chatStatuses({ conversations: [conv(1, 7, 6)], seen, now: T })['1']).toEqual({ state: 'new', why: 'answer' });
    // Never back; nothing new for nothing.
    expect(markSeen(seen, 1, 3)).toBe(seen);
    expect(markSeen(seen, 1, -1)).toBe(seen);
    expect(markSeen(seen, null, 4)).toBe(seen);
    seen = markSeen(seen, 2, 7);
    expect(seen).toEqual({ 1: 5, 2: 7 });
    // A first run: everything already there counts as seen.
    expect(seedSeen([conv(1, 5), conv(2, -1), conv(3, 0)])).toEqual({ 1: 5, 3: 0 });
    // A deleted chat let go (its id may be given again); a seen past the
    // last answer is another chat with that id: none seen there.
    expect(pruneSeen(seen, [conv(2, 7)])).toEqual({ 2: 7 });
    expect(pruneSeen(seen, [conv(1, 5), conv(2, 7)])).toBe(seen);
    expect(seenAt({ 1: 9 }, conv(1, 2))).toBe(-1);
    expect(seenAt({ 1: 2 }, conv(1, 2))).toBe(2);
    expect(seenAt({}, conv(1, 2))).toBe(-1);
    // This window's storage, round trip; anything else read as none.
    expect(readSeen(writeSeen({ 1: 5, 12: 0 }))).toEqual({ 1: 5, 12: 0 });
    expect(readSeen(null)).toBeNull();
    expect(readSeen('not json')).toBeNull();
    expect(readSeen(JSON.stringify({ v: 2, seen: {} }))).toBeNull();
    expect(readSeen(JSON.stringify({ v: 1, seen: { 1: 3, x: 2, 2: -1, 3: 1.5 } }))).toEqual({ 1: 3 });
    // A thread's last saved answer, by its ids.
    expect(messageAt('chat:12:7')).toBe(7);
    expect(messageAt(null)).toBe(-1);
    expect(lastAnswerAt([{ role: 'user', id: 'chat:1:0' }, { role: 'assistant', id: 'chat:1:1' }, { role: 'user', id: 'chat:1:2' }, { role: 'assistant', id: null, streaming: true }])).toBe(1);
    expect(lastAnswerAt([])).toBe(-1);
  });

  base(`the floor: a working spell under ${WORKING_FLOOR_MS} ms never shows; one ${WORKING_HOLD_MS} ms from the next is one`, () => {
    const answering = (since) => workingNow({ answering: { 1: since }, now: since });
    // It starts: not yet (under the floor).
    let spells = trackWorking({}, answering(T), T);
    expect(spells['1']).toEqual({ from: T, to: null, why: 'answer' });
    expect(showsWorking(spells['1'], T + 100)).toBe(false);
    expect(nextChange(spells, T + 100)).toBe(T + WORKING_FLOOR_MS);
    // Over after 200 ms: never shown.
    spells = trackWorking(spells, {}, T + 200);
    expect(spells['1'].to).toBe(T + 200);
    for (const at of [T + 200, T + 300, T + 450]) expect(showsWorking(spells['1'], at)).toBe(false);
    expect(chatStatuses({ conversations: [conv(1, -1)], spells, now: T + 350 })['1']).toBeUndefined();
    // …and let go once its hold is past.
    expect(trackWorking(spells, {}, T + 200 + WORKING_HOLD_MS + 1)).toEqual({});

    // A longer one shows from the floor on, and a little after it ends.
    spells = trackWorking({}, answering(T), T);
    expect(showsWorking(spells['1'], T + WORKING_FLOOR_MS)).toBe(true);
    spells = trackWorking(spells, {}, T + 2000);
    expect(showsWorking(spells['1'], T + 2000 + WORKING_HOLD_MS)).toBe(true);
    expect(showsWorking(spells['1'], T + 2000 + WORKING_HOLD_MS + 1)).toBe(false);
    expect(nextChange(spells, T + 2000)).toBe(T + 2000 + WORKING_HOLD_MS + 1);

    // A job ending and its report starting 100 ms later: one spell, never off between.
    let joined = trackWorking({}, workingNow({ jobs: [{ id: 'j', conversation: 1, status: 'running' }], jobSince: { j: T }, now: T }), T);
    joined = trackWorking(joined, {}, T + 5000);
    expect(showsWorking(joined['1'], T + 5100)).toBe(true);
    joined = trackWorking(joined, workingNow({ answering: { 1: T + 5100 }, now: T + 5100 }), T + 5100);
    expect(joined['1']).toEqual({ from: T, to: null, why: 'answer' });
    expect(showsWorking(joined['1'], T + 5100)).toBe(true);
    // Apart by more than the hold: a new spell, under the floor again.
    let apart = trackWorking({}, answering(T), T);
    apart = trackWorking(apart, {}, T + 1000);
    apart = trackWorking(apart, workingNow({ answering: { 1: T + 1000 + WORKING_HOLD_MS + 50 }, now: T + 1000 + WORKING_HOLD_MS + 50 }), T + 1000 + WORKING_HOLD_MS + 50);
    expect(apart['1'].from).toBe(T + 1000 + WORKING_HOLD_MS + 50);
    expect(showsWorking(apart['1'], T + 1000 + WORKING_HOLD_MS + 60)).toBe(false);
    expect(nextChange({}, T)).toBeNull();
  });

  base('what a dot says', () => {
    expect(statusWords({ state: 'needs', why: 'approve' })).toBe('Needs you: a job waits for you to allow a step');
    expect(statusWords({ state: 'needs', why: 'confirm' })).toBe('Needs you: Yap is waiting for your OK');
    expect(statusWords({ state: 'needs', why: 'plan' })).toBe('Needs you: a job’s plan waits for you to go ahead');
    expect(statusWords({ state: 'needs', why: 'question' })).toBe('Needs you: a job has a question for you');
    expect(statusWords({ state: 'working', why: 'job' })).toBe('Working: a job is running');
    expect(statusWords({ state: 'working', why: 'answer' })).toBe('Working: writing an answer');
    expect(statusWords({ state: 'working', why: '' })).toBe('Working');
    expect(statusWords({ state: 'new', why: 'report' })).toBe('New: a job’s report is in');
    expect(statusWords({ state: 'new', why: 'answer' })).toBe('New: an answer you haven’t seen');
    expect(statusWords({ state: 'needs', why: 'approve' }, 2)).toBe('2 chats need you');
    expect(statusWords({ state: 'working' }, 3)).toBe('3 chats working');
    expect(statusWords({ state: 'new' }, 2)).toBe('2 chats with something new');
    expect(statusWords(null)).toBe('');
  });
});

// ---------------------------------------------------------------- the app

const init = (mode) => ({
  type: 'system',
  subtype: 'init',
  cwd: '{{cwd}}',
  session_id: '{{session_id}}',
  tools: ['Bash', 'Edit', 'Read', 'Write'],
  mcp_servers: [{ name: 'yap', status: 'connected' }],
  model: 'claude-sonnet-5-5',
  permissionMode: mode,
  apiKeySource: 'none',
  claude_code_version: '2.1.296',
  capabilities: ['interrupt_receipt_v1'],
});
const said = (id, content, stop = 'tool_use') => ({
  type: 'assistant',
  message: { id, type: 'message', role: 'assistant', model: 'claude-sonnet-5-5', content, stop_reason: stop },
  parent_tool_use_id: null,
  session_id: '{{session_id}}',
});
const toolResult = (id, text) => ({
  type: 'user',
  message: { role: 'user', content: [{ type: 'tool_result', tool_use_id: id, content: text }] },
  parent_tool_use_id: null,
  session_id: '{{session_id}}',
});
const result = (text, extra = {}) => ({
  type: 'result',
  subtype: 'success',
  is_error: false,
  duration_ms: 900,
  num_turns: 1,
  result: text,
  session_id: '{{session_id}}',
  total_cost_usd: 0.002,
  usage: { input_tokens: 1000, output_tokens: 40, cache_creation_input_tokens: 0, cache_read_input_tokens: 0 },
  permission_denials: [],
  terminal_reason: 'completed',
  ...extra,
});
/** Takes its time (a Stop ends it). */
const SLOW = [
  { _wait_user: true },
  init('auto'),
  said('s1', [{ type: 'tool_use', id: 'toolu_read', name: 'Read', input: { file_path: '{{cwd}}\\notes.md' } }]),
  { _sleep_ms: 60000 },
  result('Finished slowly.'),
  { _wait_user: true },
];
/** Works a few seconds, then is done. */
const A_WHILE = [
  { _wait_user: true },
  init('auto'),
  said('w1', [{ type: 'tool_use', id: 'toolu_read', name: 'Read', input: { file_path: '{{cwd}}\\notes.md' } }]),
  { _sleep_ms: 3000 },
  toolResult('toolu_read', 'Draft notes'),
  result('Read notes.md: it says Draft notes.'),
  { _wait_user: true },
];
/** Asks to run the tests (a permission prompt), then is done. */
const ASKS = [
  { _wait_user: true },
  init('auto'),
  said('a1', [{ type: 'tool_use', id: 'toolu_bash', name: 'Bash', input: { command: 'npm test', description: 'Run the tests' } }]),
  { _mcp: { tool: 'approve', arguments: { tool_name: 'Bash', input: { command: 'npm test' }, tool_use_id: 'toolu_bash' } } },
  toolResult('toolu_bash', '12 passing'),
  result('The tests pass.'),
  { _wait_user: true },
];

function writeScript(name, lines) {
  fs.mkdirSync(SCRIPTS, { recursive: true });
  const file = path.join(SCRIPTS, `${name}.jsonl`);
  fs.writeFileSync(file, lines.map((l) => JSON.stringify(l)).join('\n') + '\n');
  return file;
}

function freshFolder(dir) {
  fs.rmSync(dir, { recursive: true, force: true });
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(path.join(dir, 'notes.md'), 'Draft notes\n');
}

const test = base.extend({
  fakeLlm: [
    async ({}, use) => {
      const fake = await startFakeLlm();
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({}, use) => {
      await use({ name: 'chat-status', config: { memoryLearnChats: false } });
    },
    { scope: 'worker' },
  ],
});

const chatScope = (fakeLlm) => ({
  chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat', apiKey: '', apiKeys: {}, prompt: '', disableThinking: false },
});
const thread = (main) => main.locator('.msgs');
const lastAnswer = (main) => thread(main).locator('.cbubble.assistant').last();
const jobs = (yap) => yap.invoke('job_list');
const script = (yap, lines, name) => yap.invoke('e2e_claude_script', { script: writeScript(name, lines) });
const tab = (main) => main.getByTestId('chat-job');
const row = (main, id) => main.locator(`.convs [data-conv="${id}"]`);
const dot = (main, id) => row(main, id).getByTestId('chat-status');
const group = (main, name) => main.locator(`[data-testid="chat-workspace-group"][data-workspace="${name}"]`);
const railDot = (main) => sidebar(main).getByTestId('nav-chat-status');
const railChat = (main) => sidebar(main).getByRole('button', { name: 'Chat', exact: true });

async function send(main, text) {
  await main.getByRole('textbox', { name: 'Message' }).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
}

async function closeTab(main) {
  if (await tab(main).count()) await tab(main).getByRole('button', { name: 'Close the panel' }).click();
}

/** A plain new chat, on the Chat side. */
async function plainChat(main) {
  await openView(main, 'Chat');
  await closeTab(main);
  await toSide(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
}

/** A new code chat in `name`. */
async function codeChat(main, name) {
  await openView(main, 'Chat');
  await closeTab(main);
  await newCodeChat(main, name);
}

async function shownConversation(main) {
  await expect(lastAnswer(main)).toHaveAttribute('data-msg-id', /^chat:\d+:\d+$/);
  return Number(/^chat:(\d+):/.exec(await lastAnswer(main).getAttribute('data-msg-id'))[1]);
}

/** A question and its answer in the chat shown: the conversation's id. */
async function askAndWait(main, fakeLlm, question, answer) {
  fakeLlm.scriptChat([{ content: answer }]);
  await send(main, question);
  await expect(lastAnswer(main)).toContainText(answer);
  return shownConversation(main);
}

async function ended(yap, id, timeout = 30_000) {
  await expect
    .poll(async () => (await jobs(yap)).find((j) => j.id === id)?.status, { timeout, message: `job ${id} to end` })
    .toMatch(/^(done|failed|stopped)$/);
  return (await jobs(yap)).find((j) => j.id === id);
}

async function newJob(yap, before) {
  await expect.poll(async () => (await jobs(yap)).length, { timeout: 20_000, message: 'a job to start' }).toBe(before + 1);
  return (await jobs(yap))[0];
}

async function messagesOf(yap, id, n) {
  await expect.poll(async () => (await yap.invoke('chat_get', { id })).messages.length, { timeout: 30_000, message: `${n} messages` }).toBe(n);
  return (await yap.invoke('chat_get', { id })).messages;
}

async function theme(yap, main, which) {
  await setConfig(yap, main, { theme: which });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe(which);
}

/** A token's colour as the page resolves it. */
const tokenColour = (main, token) =>
  main.evaluate((t) => {
    const probe = document.createElement('span');
    probe.style.color = `var(${t})`;
    document.body.appendChild(probe);
    const c = getComputedStyle(probe).color;
    probe.remove();
    return c;
  }, token);
const dotColour = (loc) => loc.evaluate((el) => getComputedStyle(el).backgroundColor);
const dotAnimation = (loc) => loc.evaluate((el) => getComputedStyle(el).animationName);

test.describe('the dots', () => {
  test.beforeEach(async ({ yap, main, fakeLlm }) => {
    freshFolder(GARDEN);
    freshFolder(RECIPES);
    await yap.invoke('e2e_claude_approve_auto', { allow: true });
    await setConfig(yap, main, {
      jobsEnabled: true,
      jobsAsk: 'new-or-web',
      jobsSpeak: 'never',
      jobsPay: 'plan',
      jobsMode: 'auto',
      jobsAnimate: true,
      motion: 'system',
      theme: 'light',
      webSearch: false,
      speakReplies: false,
      llmScopes: chatScope(fakeLlm),
    });
    const have = (await yap.invoke('job_folders_list')).map((f) => f.name);
    if (!have.includes('garden-planner')) await yap.invoke('job_folders_add', { path: GARDEN });
    if (!have.includes('recipe-box')) await yap.invoke('job_folders_add', { path: RECIPES });
  });

  test('an answer written in a chat you left: it pulses, then is new; opening it clears it; reduced motion keeps it still', async ({ yap, main, fakeLlm, shot }) => {
    await plainChat(main);
    const garden = await askAndWait(main, fakeLlm, 'Plan my garden beds', 'Start with the sunniest bed.');
    // The chat on screen: nothing to say.
    await expect(dot(main, garden)).toHaveAttribute('data-state', '');
    await main.getByRole('button', { name: 'New chat' }).click();
    const boxes = await askAndWait(main, fakeLlm, 'Plan my window boxes', 'Herbs on the kitchen sill.');

    // Back to the garden chat; a question whose answer waits (the fake model's gate)…
    await row(main, garden).click();
    await expect(lastAnswer(main)).toContainText('Start with the sunniest bed.');
    let release;
    const gate = new Promise((r) => (release = r));
    fakeLlm.scriptChat([{ content: 'Tomatoes go in the sunniest bed.', gate }]);
    await send(main, 'Where do the tomatoes go?');
    await expect.poll(() => yap.invoke('chat_answering').then((a) => a.conversations)).toContain(garden);
    // …and over to the window boxes while it's written.
    await row(main, boxes).click();
    await expect(lastAnswer(main)).toContainText('Herbs on the kitchen sill.');
    await expect(dot(main, garden)).toHaveAttribute('data-state', 'working');
    await expect(dot(main, garden)).toHaveAttribute('title', 'Working: writing an answer');
    await expect(dot(main, garden)).toHaveAttribute('aria-label', 'Working: writing an answer');
    await expect.poll(() => dotColour(dot(main, garden))).toBe(await tokenColour(main, '--yap-primary'));
    expect(await dotAnimation(dot(main, garden))).toMatch(/status-pulse/);
    await expect(dot(main, boxes)).toHaveAttribute('data-state', '');
    await shot(main, '01-row-working-light');

    // Reduced motion: a steady dot.
    await setConfig(yap, main, { motion: 'reduced' });
    await expect.poll(() => main.evaluate(() => document.documentElement.dataset.motion)).toBe('reduced');
    expect(await dotAnimation(dot(main, garden))).toBe('none');
    await expect(dot(main, garden)).toHaveAttribute('data-state', 'working');
    await setConfig(yap, main, { motion: 'system' });

    // The rail's Chat from another view: working.
    await openView(main, 'Home');
    await expect(railDot(main)).toHaveAttribute('data-state', 'working');
    await expect(railChat(main)).toHaveAttribute('aria-describedby', 'nav-chat-status-words');
    await expect(sidebar(main).locator('#nav-chat-status-words')).toHaveText('Working');
    await shot(main, '02-rail-working-light');
    await openView(main, 'Chat');
    // (Not on Chat itself: the list says it.)
    await expect(railDot(main)).toHaveCount(0);
    await toSide(main, 'Chat');
    await row(main, boxes).click();

    // The answer lands while the window boxes show: new.
    release();
    await messagesOf(yap, garden, 4);
    await expect(dot(main, garden)).toHaveAttribute('data-state', 'new');
    await expect(dot(main, garden)).toHaveAttribute('title', 'New: an answer you haven’t seen');
    await expect.poll(() => dotColour(dot(main, garden))).toBe(await tokenColour(main, '--yap-status-new'));
    await shot(main, '03-row-new-light');
    await theme(yap, main, 'dark');
    await expect.poll(() => dotColour(dot(main, garden))).toBe(await tokenColour(main, '--yap-status-new'));
    await shot(main, '04-row-new-dark');
    await theme(yap, main, 'light');
    // Kept in this window's storage.
    const kept = await main.evaluate(() => JSON.parse(localStorage.getItem('yap.chat.seen')));
    expect(kept.v).toBe(1);
    expect(kept.seen[String(boxes)]).toBe(1);

    // Opened: seen, gone; left again: still gone.
    await row(main, garden).click();
    await expect(lastAnswer(main)).toContainText('Tomatoes go in the sunniest bed.');
    await expect(dot(main, garden)).toHaveAttribute('data-state', '');
    await row(main, boxes).click();
    await expect(dot(main, garden)).toHaveAttribute('data-state', '');
    expect((await main.evaluate(() => JSON.parse(localStorage.getItem('yap.chat.seen')))).seen[String(garden)]).toBe(3);
  });

  test('a job running in a Code chat you left: the Code teal, a folded workspace, the switch’s Code, the rail', async ({ yap, main, fakeLlm, shot }) => {
    await codeChat(main, 'recipe-box');
    const recipes = await askAndWait(main, fakeLlm, 'What is in this folder?', 'Just notes.md.');
    await codeChat(main, 'garden-planner');
    await script(yap, SLOW, 'slow');
    const before = (await jobs(yap)).length;
    fakeLlm.scriptChat([{ tools: [{ name: 'start_job', args: { brief: 'Read notes.md slowly.' } }] }, { content: STARTED }]);
    await send(main, 'Read notes.md slowly.');
    const job = await newJob(yap, before);
    await expect.poll(async () => (await jobs(yap)).find((j) => j.id === job.id)?.status).toBe('running');
    const garden = await shownConversation(main);
    await closeTab(main);
    // On screen, it works too.
    await expect(dot(main, garden)).toHaveAttribute('data-state', 'working');
    await row(main, recipes).click();
    await closeTab(main);
    await expect(dot(main, garden)).toHaveAttribute('data-state', 'working');
    await expect(dot(main, garden)).toHaveAttribute('title', 'Working: a job is running');
    await expect.poll(() => dotColour(dot(main, garden))).toBe(await tokenColour(main, '--yap-code'));
    await shot(main, '05-code-row-working-light');
    await theme(yap, main, 'dark');
    await expect.poll(() => dotColour(dot(main, garden))).toBe(await tokenColour(main, '--yap-code'));
    await shot(main, '06-code-row-working-dark');
    await theme(yap, main, 'light');

    // Folded, its workspace says it beside its name.
    await group(main, 'garden-planner').click();
    await expect(group(main, 'garden-planner')).toHaveAttribute('aria-expanded', 'false');
    const gdot = group(main, 'garden-planner').getByTestId('chat-group-status');
    await expect(gdot).toHaveAttribute('data-state', 'working');
    await expect(group(main, 'recipe-box').getByTestId('chat-group-status')).toHaveCount(0);
    await shot(main, '07-folded-group-working-light');
    await group(main, 'garden-planner').click();
    await expect(row(main, garden)).toBeVisible();

    // The Chat side: the switch's Code says it.
    await toSide(main, 'Chat');
    const sideDot = sideSwitch(main).getByTestId('chat-side-status');
    await expect(sideDot).toHaveAttribute('data-state', 'working');
    await expect(sideDot).toHaveAttribute('title', 'Working: a job is running');
    await expect(sideSwitch(main).getByRole('radio', { name: 'Code', exact: true })).toHaveAttribute('aria-describedby', 'chat-side-status-words');
    await expect(main.locator('#chat-side-status-words')).toHaveText('Working: a job is running');
    await expect.poll(() => dotColour(sideDot)).toBe(await tokenColour(main, '--yap-code'));
    await shot(main, '08-switch-code-working-light');
    await theme(yap, main, 'dark');
    await shot(main, '09-switch-code-working-dark');
    await theme(yap, main, 'light');

    // Another view: the rail's Chat pulses.
    await openView(main, 'Notes');
    await expect(railDot(main)).toHaveAttribute('data-state', 'working');
    await expect(railChat(main)).toBeVisible();
    // Narrow, its tooltip says so too.
    await sidebar(main).getByTestId('nav-rail-toggle').click();
    await expect(railChat(main)).toHaveAttribute('data-tip', 'Chat · Working');
    await shot(main, '10-rail-narrow-working-light');
    await sidebar(main).getByTestId('nav-rail-toggle').click();

    // Stopped (no report for a stop): nothing more to say anywhere.
    await yap.invoke('job_stop', { id: job.id });
    await ended(yap, job.id);
    await expect(railDot(main)).toHaveCount(0);
    await openView(main, 'Chat');
    await toSide(main, 'Code');
    await expect(dot(main, garden)).toHaveAttribute('data-state', '');
  });

  test('a permission prompt: amber in the row and on the rail, cleared on Allow once', async ({ yap, main, fakeLlm, shot }) => {
    await yap.invoke('e2e_claude_approve_auto', { allow: null });
    await codeChat(main, 'garden-planner');
    await script(yap, ASKS, 'asks');
    const before = (await jobs(yap)).length;
    fakeLlm.scriptChat([{ tools: [{ name: 'start_job', args: { brief: 'Run the tests.' } }] }, { content: STARTED }, { plain: true, content: 'Done: the tests pass.' }]);
    await send(main, 'Run the tests.');
    const job = await newJob(yap, before);
    await expect.poll(async () => (await jobs(yap)).find((j) => j.id === job.id)?.status, { timeout: 20_000 }).toBe('asking');
    const garden = await shownConversation(main);
    await closeTab(main);
    await expect(dot(main, garden)).toHaveAttribute('data-state', 'needs');
    await expect(dot(main, garden)).toHaveAttribute('title', 'Needs you: a job waits for you to allow a step');
    await expect.poll(() => dotColour(dot(main, garden))).toBe(await tokenColour(main, '--yap-warning'));
    expect(await dotAnimation(dot(main, garden))).toBe('none');
    // On Chat too, the rail says it.
    await expect(railDot(main)).toHaveAttribute('data-state', 'needs');
    await shot(main, '11-row-needs-you-light');
    await theme(yap, main, 'dark');
    await shot(main, '12-row-needs-you-dark');
    await theme(yap, main, 'light');
    await openView(main, 'Home');
    await expect(railDot(main)).toHaveAttribute('data-state', 'needs');
    await expect(sidebar(main).locator('#nav-chat-status-words')).toHaveText('Needs you');
    await expect.poll(() => dotColour(railDot(main))).toBe(await tokenColour(main, '--yap-warning'));
    await shot(main, '13-rail-needs-you-light');

    // Allow once, in its thread: no longer waiting on you.
    await openView(main, 'Chat');
    await toSide(main, 'Code');
    await row(main, garden).click();
    await closeTab(main);
    const card = thread(main).getByTestId('chat-job-approve');
    await expect(card).toHaveCount(1);
    await card.getByRole('button', { name: 'Allow once' }).click();
    await expect(card).toHaveCount(0);
    await expect(dot(main, garden)).not.toHaveAttribute('data-state', 'needs');
    await ended(yap, job.id);
    await messagesOf(yap, garden, 4);
    // Its report landed here, on screen: seen, nothing to say.
    await expect(dot(main, garden)).toHaveAttribute('data-state', '');
    await expect(railDot(main)).toHaveCount(0);
  });

  test('an ask-first card: amber while it waits, cleared on Don’t', async ({ yap, main, fakeLlm }) => {
    await setConfig(yap, main, { jobsAsk: 'always' });
    await codeChat(main, 'garden-planner');
    await script(yap, SLOW, 'slow-ask');
    fakeLlm.scriptChat([{ tools: [{ name: 'start_job', args: { brief: 'Read notes.md slowly.' } }] }, { content: "Okay, I won't start it." }]);
    await send(main, 'Read notes.md slowly.');
    const card = thread(main).getByTestId('chat-job-confirm');
    await expect(card).toHaveAttribute('data-state', 'asking');
    const garden = Number(await main.locator('.convs .conv.active').getAttribute('data-conv'));
    await expect(dot(main, garden)).toHaveAttribute('data-state', 'needs');
    await expect(dot(main, garden)).toHaveAttribute('title', 'Needs you: Yap is waiting for your OK');
    await expect(railDot(main)).toHaveAttribute('data-state', 'needs');
    await card.getByRole('button', { name: 'Don’t' }).or(card.getByRole('button', { name: "Don't" })).click();
    await expect(card).toHaveAttribute('data-state', 'refused');
    await expect(lastAnswer(main)).toContainText("Okay, I won't start it.");
    await expect(dot(main, garden)).toHaveAttribute('data-state', '');
    await expect(railDot(main)).toHaveCount(0);
  });

  test('a job’s report landing while you’re in another chat: new, cleared on opening it', async ({ yap, main, fakeLlm, shot }) => {
    await codeChat(main, 'recipe-box');
    const recipes = await askAndWait(main, fakeLlm, 'What is in this folder?', 'Just notes.md.');
    await codeChat(main, 'garden-planner');
    await script(yap, A_WHILE, 'a-while');
    const before = (await jobs(yap)).length;
    fakeLlm.scriptChat([{ tools: [{ name: 'start_job', args: { brief: 'Read notes.md.' } }] }, { content: STARTED }, { plain: true, content: 'Done: notes.md says Draft notes.' }]);
    await send(main, 'Read notes.md.');
    const job = await newJob(yap, before);
    const garden = await shownConversation(main);
    // Over to the recipes while it works.
    await closeTab(main);
    await row(main, recipes).click();
    await expect(dot(main, garden)).toHaveAttribute('data-state', 'working');
    await ended(yap, job.id);
    await messagesOf(yap, garden, 4);
    await expect(dot(main, garden)).toHaveAttribute('data-state', 'new');
    await expect(dot(main, garden)).toHaveAttribute('data-why', 'report');
    await expect(dot(main, garden)).toHaveAttribute('title', 'New: a job’s report is in');
    await shot(main, '14-report-new-light');
    // Folded: the workspace says it.
    await group(main, 'garden-planner').click();
    await expect(group(main, 'garden-planner').getByTestId('chat-group-status')).toHaveAttribute('data-state', 'new');
    await shot(main, '15-folded-group-new-light');
    await theme(yap, main, 'dark');
    await shot(main, '16-folded-group-new-dark');
    await theme(yap, main, 'light');
    await group(main, 'garden-planner').click();
    // The Chat side: the switch's Code says it.
    await toSide(main, 'Chat');
    await expect(sideSwitch(main).getByTestId('chat-side-status')).toHaveAttribute('data-state', 'new');
    // The rail never says "new".
    await openView(main, 'Home');
    await expect(railDot(main)).toHaveCount(0);
    // Opened: gone, everywhere.
    await openView(main, 'Chat');
    await toSide(main, 'Code');
    await row(main, garden).click();
    await expect(lastAnswer(main)).toContainText('Done: notes.md says Draft notes.');
    await expect(dot(main, garden)).toHaveAttribute('data-state', '');
    await toSide(main, 'Chat');
    await expect(sideSwitch(main).getByTestId('chat-side-status')).toHaveCount(0);
  });
});
