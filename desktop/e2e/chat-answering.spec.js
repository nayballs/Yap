// An answer being written belongs to its conversation (desktop/src/lib/views/chat/
// chatState.svelte.js `Answer`, 7 Nov 2026): the thread shows the open chat's
// answer only — its streamed text, its steps, the line at its foot ("Thinking…")
// — and the composer's ■ Stop and disabled Send are the open chat's only.
// Before, an answer in progress showed in whichever chat was open: ask in A,
// open B, and B said A's "Thinking…" with a Stop that stopped A.
//
// - Ask in A, open B while it's written: nothing of it in B, A's row Working;
//   back in A, the text so far and its line, nothing twice; it lands in A
//   (New in the list while B shows) — never pulling the view back to A.
// - Two chats answering at once (Rust ends a turn only for a newer question in
//   its own conversation, chat/stream.rs): B asks while A answers; ■ in A
//   stops A's answer only, Esc in B B's.
// - An agent's report being written in A (a turn Yap starts itself,
//   chat/turn/report.rs) while B shows: B unchanged, Send free.
// - A question asked out loud (the Voice Assistant) answered in A while B
//   shows: none of it, nor its ■, in B; back in A, its answer so far.
// The model is a local fake (support/fake-llm.js) streaming slowly; agents are
// driven as agents-ui.spec.js drives them (Claude Code is this build's own
// stand-in, the workspace in this run's own folder).
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, setConfig, mainOnScreen } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { RUNS } from './support/yap.js';
import { toSide, newCodeChat, sideSwitch } from './support/code-chats.js';

const HERE = path.join(RUNS, 'chat-answering-spec');
const SCRIPTS = path.join(HERE, 'scripts');
const GARDEN = path.join(HERE, 'folders', 'garden-planner');
const STARTED = "On it: I've sent an agent to garden-planner.";

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
      await use({ name: 'chat-answering', config: { memoryLearnChats: false, meetingReminder: 'never' } });
    },
    { scope: 'worker' },
  ],
});

const chatScope = (fakeLlm) => ({
  chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat', apiKey: '', apiKeys: {}, prompt: '', disableThinking: false },
});
const box = (main) => main.getByRole('textbox', { name: 'Message' });
const thread = (main) => main.locator('.msgs');
const streamingBubble = (main) => main.getByTestId('chat-streaming');
const activityLine = (main) => main.getByTestId('chat-activity');
const stopButton = (main) => main.getByRole('button', { name: 'Stop the answer' });
const sendButton = (main) => main.getByRole('button', { name: 'Send', exact: true });
const lastAnswer = (main) => thread(main).locator('.cbubble.assistant').last();
const row = (main, id) => main.locator(`.convs [data-conv="${id}"]`);
const dot = (main, id) => row(main, id).getByTestId('chat-status');
const activeConv = (main) => main.locator('.convs .conv.active');
const answering = (yap) => yap.invoke('chat_answering').then((a) => a.conversations);
/** The fake's streamed request for the question `q`. */
const requestFor = (fakeLlm, q) => fakeLlm.requests.filter((r) => r.stream && String(r.user ?? '').includes(q)).at(-1);
const normal = (s) => String(s ?? '').replace(/\s+/g, ' ').trim();

async function send(main, text) {
  await box(main).fill(text);
  await sendButton(main).click();
}

async function newChat(main) {
  await main.getByRole('button', { name: 'New chat' }).click();
  await expect(thread(main).locator('.cbubble')).toHaveCount(0);
}

/** The id of the conversation open (once its first question made it). */
async function openId(main) {
  await expect(activeConv(main)).toHaveAttribute('data-conv', /^\d+$/);
  return Number(await activeConv(main).getAttribute('data-conv'));
}

/** A question and its answer in the chat shown: the conversation's id. */
async function askAndWait(main, fakeLlm, question, answer) {
  fakeLlm.scriptChat([{ content: answer }]);
  await send(main, question);
  await expect(lastAnswer(main)).toContainText(answer);
  await expect(stopButton(main)).toHaveCount(0);
  return openId(main);
}

/** Nothing of an answer in progress in the chat shown: no bubble, no line, no ■; Send free. */
async function nothingAnswering(main) {
  await expect(streamingBubble(main)).toHaveCount(0);
  await expect(activityLine(main)).toHaveCount(0);
  await expect(stopButton(main)).toHaveCount(0);
  await box(main).fill('Typed while another chat answers');
  await expect(sendButton(main)).toBeEnabled();
  await box(main).fill('');
}

async function messagesOf(yap, id, n) {
  await expect.poll(async () => (await yap.invoke('chat_get', { id })).messages.length, { timeout: 30_000, message: `${n} messages` }).toBe(n);
  return (await yap.invoke('chat_get', { id })).messages;
}

const LONG = [
  'Tomatoes go in the sunniest bed, at the back by the fence.',
  'Put the basil in front of them, where it gets the morning light.',
  'The beans can climb the trellis on the left side of the garden.',
  'Leave a gap for the courgettes, because they spread out a lot by midsummer.',
  'Water everything in the evening, after the heat of the day has gone.',
  'Mulch the beds with straw so the soil keeps its water for longer.',
  'Check the leaves for slugs after rain, and pick them off by hand.',
  'By the end of August the first tomatoes should be ready to pick.',
].join(' ');

test.describe('answers in their own chats', () => {
  test.beforeEach(async ({ yap, main, fakeLlm }) => {
    await setConfig(yap, main, { agentsEnabled: false, webSearch: false, speakReplies: false, llmScopes: chatScope(fakeLlm) });
    await openView(main, 'Chat');
  });

  test('an answer shows only in its chat: away it goes on, back it picks up where it is, it lands there', async ({ yap, main, fakeLlm, shot }) => {
    await newChat(main);
    const boxes = await askAndWait(main, fakeLlm, 'Plan my window boxes', 'Herbs on the kitchen sill.');
    await newChat(main);
    fakeLlm.scriptChat([{ content: LONG, stream: { gapMs: 260 } }]);
    await send(main, 'Where do the tomatoes go?');
    await expect(streamingBubble(main)).toContainText('Tomatoes go in the sunniest bed');
    await expect(activityLine(main)).toBeVisible();
    const garden = await openId(main);

    // Over to the window boxes while it's written: nothing of it there.
    await row(main, boxes).click();
    await expect(lastAnswer(main)).toContainText('Herbs on the kitchen sill.');
    await nothingAnswering(main);
    await expect(thread(main).locator('.cbubble')).toHaveCount(2);
    await expect(dot(main, garden)).toHaveAttribute('data-state', 'working');
    expect(await answering(yap)).toContain(garden);
    await shot(main, '01-another-chat-while-one-answers');

    // Back: the answer so far and its line, the question once, still coming.
    await row(main, garden).click();
    const live = streamingBubble(main);
    await expect(live).toBeVisible();
    await expect(activityLine(main)).toBeVisible();
    await expect(stopButton(main)).toBeVisible();
    await expect(thread(main).locator('.cbubble.user')).toHaveCount(1);
    await expect(thread(main).locator('.cbubble.assistant')).toHaveCount(1);
    const sofar = normal(await live.locator('.cbody').textContent());
    expect(sofar.length).toBeGreaterThan(10);
    expect(normal(LONG).startsWith(sofar), `"${sofar}" is how the answer starts`).toBe(true);
    // It grows on from there.
    await expect.poll(async () => normal(await live.locator('.cbody').textContent()).length).toBeGreaterThan(sofar.length);
    await shot(main, '02-back-mid-answer');

    // Away again: it lands in its own chat, the window boxes stay in view.
    await row(main, boxes).click();
    await nothingAnswering(main);
    const saved = await messagesOf(yap, garden, 2);
    expect(normal(saved[1].text)).toBe(normal(LONG));
    await expect(dot(main, garden)).toHaveAttribute('data-state', 'new');
    await expect(activeConv(main)).toHaveAttribute('data-conv', String(boxes));
    await expect(thread(main).locator('.cbubble')).toHaveCount(2);
    await expect(thread(main)).not.toContainText('Tomatoes');
    // The switches never cut the stream: asked once, streamed whole.
    expect(fakeLlm.requests.filter((r) => String(r.user ?? '').includes('Where do the tomatoes go?'))).toHaveLength(1);
    expect(requestFor(fakeLlm, 'Where do the tomatoes go?').ended).toBe(true);

    // Opened: the whole answer, once.
    await row(main, garden).click();
    await expect(lastAnswer(main)).toContainText('the first tomatoes should be ready to pick.');
    await expect(thread(main).locator('.cbubble.user')).toHaveCount(1);
    await expect(thread(main).locator('.cbubble.assistant')).toHaveCount(1);
    await expect(streamingBubble(main)).toHaveCount(0);
    await expect(dot(main, garden)).toHaveAttribute('data-state', '');
  });

  test('Chat left and opened again mid-answer: the answer shows in its chat once it is done', async ({ yap, main, fakeLlm }) => {
    await newChat(main);
    fakeLlm.scriptChat([{ content: LONG, stream: { gapMs: 200 } }]);
    await send(main, 'What goes where in the beds?');
    await expect(streamingBubble(main)).toContainText('Tomatoes go in the sunniest bed');
    const garden = await openId(main);
    await openView(main, 'Home');
    await openView(main, 'Chat');
    await row(main, garden).click();
    await expect(thread(main).locator('.cbubble.user')).toHaveCount(1);
    // Done: read again, its answer in place, once.
    await messagesOf(yap, garden, 2);
    await expect(lastAnswer(main)).toContainText('the first tomatoes should be ready to pick.');
    await expect(thread(main).locator('.cbubble.assistant')).toHaveCount(1);
  });

  test('two chats answering at once: B asks while A answers, ■ in A stops A only, Esc in B stops B', async ({ yap, main, fakeLlm, shot }) => {
    const A = 'Tell me about the garden beds';
    const B = 'Tell me about the window boxes';
    await newChat(main);
    fakeLlm.scriptChat([{ content: 'The garden beds face south. They get sun all day and drain well after rain.', stream: { gapMs: 120, hangAfter: 5 } }]);
    await send(main, A);
    await expect(streamingBubble(main)).toContainText('The garden beds');
    const a = await openId(main);
    await expect.poll(() => requestFor(fakeLlm, A) != null).toBe(true);

    // A new chat asks while A is still being answered.
    await newChat(main);
    await nothingAnswering(main);
    // (Its first sentence and the start of the next before it hangs: an
    // answer's opening is shown once a sentence of it is in, stream.rs `releases`.)
    fakeLlm.scriptChat([{ content: 'The window boxes dry out. They hang on the kitchen side and need water daily.', stream: { gapMs: 120, hangAfter: 5 } }]);
    await send(main, B);
    const live = streamingBubble(main);
    await expect(live).toContainText('The window boxes');
    await expect(live).not.toContainText('garden beds');
    await expect(activityLine(main)).toHaveCount(1);
    const b = await openId(main);
    await expect.poll(() => answering(yap)).toEqual(expect.arrayContaining([a, b]));
    await shot(main, '03-two-chats-answering');

    // In A: A's answer, A's ■ — and it stops A's only.
    await row(main, a).click();
    await expect(streamingBubble(main)).toContainText('The garden beds');
    await expect(streamingBubble(main)).not.toContainText('window boxes');
    await stopButton(main).click();
    await expect(streamingBubble(main)).toHaveCount(0);
    await expect(lastAnswer(main)).toContainText('The garden beds');
    await expect(thread(main).locator('.byline.stopped')).toHaveText('· Stopped');
    await expect.poll(() => requestFor(fakeLlm, A).aborted).toBe(true);
    await expect.poll(() => answering(yap)).not.toContain(a);
    expect(await answering(yap)).toContain(b);
    expect(requestFor(fakeLlm, B).aborted).toBeFalsy();
    await expect(dot(main, b)).toHaveAttribute('data-state', 'working');

    // B is still being answered: its ■, and Esc stops it.
    await row(main, b).click();
    await expect(streamingBubble(main)).toContainText('The window boxes');
    await expect(stopButton(main)).toBeVisible();
    await box(main).focus();
    await main.keyboard.press('Escape');
    await expect(streamingBubble(main)).toHaveCount(0);
    await expect(thread(main).locator('.byline.stopped')).toHaveText('· Stopped');
    await expect.poll(() => requestFor(fakeLlm, B).aborted).toBe(true);
    await expect.poll(() => answering(yap)).not.toContain(b);
  });

  test('a question asked out loud answered in A: none of it in B, its answer so far back in A', async ({ yap, main, fakeLlm, shot }) => {
    await yap.invoke('assistant_stop');
    await yap.invoke('tts_stop');
    await newChat(main);
    const boxes = await askAndWait(main, fakeLlm, 'Plan my window boxes', 'Herbs on the kitchen sill.');
    await newChat(main);
    const garden = await askAndWait(main, fakeLlm, 'Plan my garden beds', 'Start with the sunniest bed.');
    await mainOnScreen(yap);
    await yap.invoke('e2e_focus', { focused: true });
    fakeLlm.scriptChat([{ content: LONG, stream: { gapMs: 260 } }]);
    await yap.invoke('e2e_assistant_ask', { text: 'where do the tomatoes go', listenMs: 0 });
    // Asked in the chat open: its answer comes there, with its ■.
    await expect(streamingBubble(main)).toContainText('Tomatoes go in the sunniest bed', { timeout: 10_000 });
    await expect(stopButton(main)).toBeVisible();
    expect(await openId(main)).toBe(garden);

    // Over to the window boxes: none of it, not its ■.
    await row(main, boxes).click();
    await expect(lastAnswer(main)).toContainText('Herbs on the kitchen sill.');
    await nothingAnswering(main);
    await expect(dot(main, garden)).toHaveAttribute('data-state', 'working');
    await shot(main, '04-spoken-answer-elsewhere');

    // Back: its answer so far and its line.
    await row(main, garden).click();
    const live = streamingBubble(main);
    await expect(live).toBeVisible();
    await expect(activityLine(main)).toBeVisible();
    await expect(stopButton(main)).toBeVisible();
    const sofar = normal(await live.locator('.cbody').textContent());
    expect(normal(LONG).startsWith(sofar), `"${sofar}" is how the answer starts`).toBe(true);
    await expect(thread(main).locator('.cbubble.user')).toHaveCount(2);

    // Done, in its chat, once (read aloud: its ■ stays here, never in B).
    await expect(lastAnswer(main)).toContainText('the first tomatoes should be ready to pick.', { timeout: 20_000 });
    await expect(streamingBubble(main)).toHaveCount(0);
    await expect(thread(main).locator('.cbubble.assistant')).toHaveCount(2);
    await row(main, boxes).click();
    await expect(stopButton(main)).toHaveCount(0);
    await yap.invoke('assistant_stop');
    await yap.invoke('tts_stop');
  });
});

// ---- An agent's report being written in another chat

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
const result = (text) => ({
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
});
/** Reads notes.md, then is done. */
const READS = [
  { _wait_user: true },
  init('auto'),
  said('w1', [{ type: 'tool_use', id: 'toolu_read', name: 'Read', input: { file_path: '{{cwd}}\\notes.md' } }]),
  { _sleep_ms: 500 },
  toolResult('toolu_read', 'Draft notes'),
  result('Read notes.md: it says Draft notes.'),
  { _wait_user: true },
];

function writeScript(name, lines) {
  fs.mkdirSync(SCRIPTS, { recursive: true });
  const file = path.join(SCRIPTS, `${name}.jsonl`);
  fs.writeFileSync(file, lines.map((l) => JSON.stringify(l)).join('\n') + '\n');
  return file;
}

test.describe('an agent’s report', () => {
  test.beforeEach(async ({ yap, main, fakeLlm }) => {
    fs.rmSync(GARDEN, { recursive: true, force: true });
    fs.mkdirSync(GARDEN, { recursive: true });
    fs.writeFileSync(path.join(GARDEN, 'notes.md'), 'Draft notes\n');
    await yap.invoke('e2e_claude_approve_auto', { allow: true });
    await setConfig(yap, main, {
      agentsEnabled: true,
      agentsAsk: 'new-or-web',
      agentsSpeak: 'never',
      agentsPay: 'plan',
      agentsMode: 'auto',
      webSearch: false,
      speakReplies: false,
      llmScopes: chatScope(fakeLlm),
    });
    const have = (await yap.invoke('workspaces_list')).map((f) => f.name);
    if (!have.includes('garden-planner')) await yap.invoke('workspaces_add', { path: GARDEN });
  });

  test('a report being written in A while B shows: B unchanged, its Send free; it lands in A', async ({ yap, main, fakeLlm, shot }) => {
    await openView(main, 'Chat');
    await toSide(main, 'Chat');
    await newChat(main);
    const boxes = await askAndWait(main, fakeLlm, 'Plan my window boxes', 'Herbs on the kitchen sill.');

    // A code chat starts an agent; its report waits for the fake model's gate.
    await newCodeChat(main, 'garden-planner');
    await yap.invoke('e2e_claude_script', { script: writeScript('reads', READS) });
    const before = (await yap.invoke('agent_list')).length;
    const gatedBefore = fakeLlm.requests.filter((r) => r.gated).length;
    let release;
    const gate = new Promise((r) => (release = r));
    fakeLlm.scriptChat([
      { tools: [{ name: 'start_agent', args: { brief: 'Read notes.md.' } }] },
      { content: STARTED },
      { plain: true, content: 'Done: notes.md says Draft notes.', gate },
    ]);
    await send(main, 'Read notes.md.');
    await expect.poll(async () => (await yap.invoke('agent_list')).length, { timeout: 20_000 }).toBe(before + 1);
    await expect(lastAnswer(main)).toContainText(STARTED);
    const garden = await openId(main);
    const agent = (await yap.invoke('agent_list'))[0];

    // Over to the window boxes while the agent works and its report is written.
    if (await main.getByTestId('chat-agent').count()) await main.getByTestId('chat-agent').getByRole('button', { name: 'Close the panel' }).click();
    await toSide(main, 'Chat');
    await row(main, boxes).click();
    await expect(lastAnswer(main)).toContainText('Herbs on the kitchen sill.');
    await expect.poll(async () => (await yap.invoke('agent_list')).find((j) => j.id === agent.id)?.status, { timeout: 30_000 }).toBe('done');
    // (The report's model call, held at the gate.)
    await expect.poll(() => fakeLlm.requests.filter((r) => r.gated).length, { timeout: 20_000 }).toBeGreaterThan(gatedBefore);
    await expect.poll(() => answering(yap), { timeout: 10_000 }).toContain(garden);
    await nothingAnswering(main);
    await expect(thread(main).locator('.cbubble')).toHaveCount(2);
    await expect(sideSwitch(main).getByTestId('chat-side-status')).toHaveAttribute('data-state', 'working');
    await shot(main, '05-report-written-elsewhere');

    // It lands in A: B unchanged, still in view; A's is new.
    release();
    const saved = await messagesOf(yap, garden, 4);
    expect(saved[3].text).toContain('Done: notes.md says Draft notes.');
    await expect(activeConv(main)).toHaveAttribute('data-conv', String(boxes));
    await expect(thread(main).locator('.cbubble')).toHaveCount(2);
    await expect(thread(main)).not.toContainText('Draft notes');
    await nothingAnswering(main);
    await expect(sideSwitch(main).getByTestId('chat-side-status')).toHaveAttribute('data-state', 'new');
  });
});
