// Connectors in a chat on your Claude plan (Connectors wave 7:
// desktop/src-tauri/src/integrations/claude_live/connectors.rs, chat/turn/plan.rs): the plan's
// own Claude Code gets the person's connector tools from Yap's run endpoint, every one in its
// --allowedTools, and each call is wave 4's own (chat/tools/connectors.rs): Yap's settings and
// consent decide, Ask first holds the HTTP call while Yap's card asks in the thread, a no is a
// refusal the model reads, a server's answer is outside content for the rest of the turn.
//
// Claude Code is this build's stand-in (`yap.exe fake-claude <script>`, ai/claude_code/fake.rs),
// switched per test with the debug-only `e2e_claude_script`: the plan chat's session streams its
// tool calls as Claude Code does, then `_mcp` calls Yap's endpoint as Claude Code would (with
// `dump`, what it got written to a file this spec reads; `list`, the session's tools/list), and
// `_dump_args` writes its launch. The connector is support/fake-mcp.js --chat, a program on this
// PC (stdio). Nobody's real Claude Code, account or service is touched; fictional data only.
//
// - A read set to Always allow runs: its chip, its result, listed and allowed with its hint.
// - A write set to Ask first: Yap's card in the plan chat's thread while the call waits; Allow
//   once runs it, Deny refuses it (the screenshots, light and dark).
// - After a connector's result, a write asks though it's Always allow.
// - ■ Stop ends a call under way: the server is told.
// - A tool turned off, then the connector: not listed, and the session starts again.
// - A Code chat's session and an agent's run get none.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { test as base, expect, expectStore, openView, setConfig } from './support/fixtures.js';
import { fakeClaude, planConfig, SCRIPTS as PLAN_SCRIPTS } from './support/claude-plan.js';
import { RUNS } from './support/yap.js';
import { toSide, newCodeChat } from './support/code-chats.js';

const HERE = path.join(RUNS, 'plan-connectors-spec');
const SCRIPTS = path.join(HERE, 'scripts');
const GARDEN = path.join(HERE, 'folders', 'garden-planner');
const FAKE = fileURLToPath(new URL('./support/fake-mcp.js', import.meta.url));
const NODE = process.execPath;
const SID = '{{session_id}}';
const MODEL = 'claude-haiku-5-5';

// ---- the stand-in's script ----

const ev = (event) => ({ type: 'stream_event', event, parent_tool_use_id: null, session_id: SID });
const initOf = (tools, permissionMode) => ({
  type: 'system',
  subtype: 'init',
  cwd: '{{cwd}}',
  session_id: SID,
  tools,
  mcp_servers: [{ name: 'yap', status: 'connected' }],
  model: MODEL,
  permissionMode,
  apiKeySource: 'none',
  claude_code_version: '2.1.296',
  capabilities: ['interrupt_receipt_v1'],
});
const CHAT_INIT = initOf(['WebFetch', 'WebSearch', 'mcp__yap__homelights__list_lights', 'mcp__yap__homelights__create_page'], 'dontAsk');
const whole = (id, content, stop) => ({
  type: 'assistant',
  message: { id, type: 'message', role: 'assistant', model: MODEL, content, stop_reason: stop },
  parent_tool_use_id: null,
  session_id: SID,
});
const result = (text) => ({
  type: 'result',
  subtype: 'success',
  is_error: false,
  duration_ms: 900,
  num_turns: 2,
  result: text,
  session_id: SID,
  total_cost_usd: 0.003,
  usage: { input_tokens: 5200, output_tokens: 30, cache_creation_input_tokens: 0, cache_read_input_tokens: 0 },
  permission_denials: [],
  terminal_reason: 'completed',
});
/** The model calls `name` in message `msg`, streamed (its start first) then whole, as Claude Code writes it. */
const calls = (msg, id, name, input) => [
  ev({ type: 'message_start', message: { id: msg, type: 'message', role: 'assistant', model: MODEL, content: [] } }),
  ev({ type: 'content_block_start', index: 0, content_block: { type: 'tool_use', id, name, input: {} } }),
  ev({ type: 'content_block_delta', index: 0, delta: { type: 'input_json_delta', partial_json: JSON.stringify(input) } }),
  ev({ type: 'content_block_stop', index: 0 }),
  whole(msg, [{ type: 'tool_use', id, name, input }], 'tool_use'),
  ev({ type: 'message_stop' }),
];
/** …one of Yap's: its call, Yap's endpoint called with it (what it gave written to `dump`), the answer its tool result. */
const yapCall = (msg, id, tool, input, dump) => [...calls(msg, id, `mcp__yap__${tool}`, input), { _mcp: { tool, arguments: input, result_for: id, dump } }];
/** The session's tools/list, written to `dump`. */
const lists = (dump) => ({ _mcp: { list: true, dump } });
/** The answer, streamed then whole, and the turn's result. */
const answers = (msg, text) => [
  ev({ type: 'message_start', message: { id: msg, type: 'message', role: 'assistant', model: MODEL, content: [] } }),
  ev({ type: 'content_block_start', index: 0, content_block: { type: 'text', text: '' } }),
  ev({ type: 'content_block_delta', index: 0, delta: { type: 'text_delta', text } }),
  ev({ type: 'content_block_stop', index: 0 }),
  whole(msg, [{ type: 'text', text }], 'end_turn'),
  ev({ type: 'message_stop' }),
  result(text),
];
/** One plan chat turn: the question, then `lines`. */
const turn = (...lines) => [{ _wait_user: true }, CHAT_INIT, ...lines.flat()];

/** A script of plan chat turns (then, with `more`, an agent's and the report's lines). */
function script(name, chatTurns, more = []) {
  fs.mkdirSync(SCRIPTS, { recursive: true });
  const file = path.join(SCRIPTS, `${name}.jsonl`);
  const lines = [{ _for: 'chat' }, ...chatTurns.flat(), { _wait_user: true }, ...more];
  fs.writeFileSync(file, lines.map((l) => JSON.stringify(l)).join('\n') + '\n');
  return file;
}

/** A fresh file for the stand-in to write its MCP calls (or arguments) to. */
function fresh(name) {
  fs.mkdirSync(HERE, { recursive: true });
  const file = path.join(HERE, name);
  fs.rmSync(file, { force: true });
  return file;
}
const linesOf = (file) => (fs.existsSync(file) ? fs.readFileSync(file, 'utf8').split('\n').filter(Boolean).map((l) => JSON.parse(l)) : []);
const resultOf = (call) => JSON.parse(call.content[0].text);
const allowedIn = (launch) => launch.args[launch.args.indexOf('--allowedTools') + 1] ?? '';
const listedNames = (list) => list.tools.map((t) => t.name);

// ---- the app ----

const test = base.extend({
  yapOptions: [
    async ({}, use) => {
      await use({
        name: 'plan-connectors',
        env: fakeClaude(path.join(PLAN_SCRIPTS, 'plan-chat.jsonl')),
        config: planConfig({ 'claude-plan': 'claude-plan' }, { memoryLearnChats: false, webSearch: false, connectorsPreview: true }),
      });
    },
    { scope: 'worker' },
  ],
});

const thread = (main) => main.locator('.msgs');
const lastAnswer = (main) => thread(main).locator('.cbubble.assistant').last();
const chipsOfLast = (main) => thread(main).locator('.toolchips').last().locator('.toolchip');
const card = (main) => thread(main).getByTestId('chat-connector-confirm').last();
const play = (yap, name, chatTurns, more) => yap.invoke('e2e_claude_script', { script: script(name, chatTurns, more) });

async function send(main, text) {
  await main.getByRole('textbox', { name: 'Message' }).fill(text);
  await main.getByRole('button', { name: 'Send', exact: true }).click();
}

async function freshChat(main) {
  await openView(main, 'Chat');
  const cfg = await main.evaluate(() => window.__TAURI_INTERNALS__.invoke('get_config'));
  if (cfg?.agentsEnabled) await toSide(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).first().click();
}

async function theme(yap, main, which) {
  await setConfig(yap, main, { theme: which });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe(which);
}

/** fake-mcp.js added as a program connector ("Home lights"), shown first as Yap does; its log and id. */
async function addFake(yap, name) {
  fs.mkdirSync(HERE, { recursive: true });
  const log = path.join(HERE, `${name}.mcp.log`);
  fs.rmSync(log, { force: true });
  const spec = { name: 'Home lights', command: NODE, args: [FAKE, '--log', log, '--chat'] };
  const preview = await yap.invoke('connector_preview', { spec });
  const added = await yap.invoke('connector_add', { spec: { ...spec, approved: preview.program.fingerprint } });
  expect(added.status.state).toBe('connected');
  const lines = () => linesOf(log);
  const called = (tool) => lines().filter((l) => l.kind === 'request' && l.method === 'tools/call' && (!tool || l.tool === tool));
  return { id: added.id, called, log: lines };
}

/** The newest conversation's id. */
const newest = async (yap) => (await yap.invoke('chats_list'))[0].id;

test.beforeEach(async ({ yap, main }) => {
  await setConfig(yap, main, { agentsEnabled: false, webSearch: false, connectorsPreview: true });
});

test('a read set to Always allow runs on the plan: its chip and result, listed with its hint and allowed', async ({ yap, main, shot }) => {
  const fake = await addFake(yap, 'read');
  try {
    const DUMP = fresh('read.jsonl');
    const ARGS = fresh('read-args.jsonl');
    const SAID = 'The kitchen light is on; the hall is off.';
    await play(yap, 'read', [[{ _wait_user: true }, { _dump_args: ARGS }, CHAT_INIT, lists(DUMP), ...yapCall('m1', 'toolu_read', 'homelights__list_lights', {}, DUMP), ...answers('m2', SAID)]]);
    await freshChat(main);
    await send(main, 'Which of my lights are on?');
    await expect(lastAnswer(main)).toHaveText(SAID, { timeout: 20_000 });
    await expect(chipsOfLast(main)).toHaveText(['Home lights · List lights']);
    await expect(card(main)).toHaveCount(0);
    expect(fake.called('list_lights')).toHaveLength(1);

    // The session's list: Yap's names, a read marked; an Off tool (its description gives orders) never.
    const [listed, read] = linesOf(DUMP);
    expect(listed.tools).toEqual(
      expect.arrayContaining([
        { name: 'homelights__list_lights', readOnlyHint: true },
        { name: 'homelights__create_page', readOnlyHint: false },
        { name: 'homelights__delete_all', readOnlyHint: false },
      ])
    );
    expect(listedNames(listed)).not.toContain('homelights__inject');
    expect(listedNames(listed)).toContain('list_meetings');
    // Its result as the tool loop's, from the connector, said to be information.
    expect(read).toMatchObject({ tool: 'homelights__list_lights', isError: false });
    expect(resultOf(read)).toMatchObject({ success: true, data: { text: 'Kitchen: on\nHall: off' }, displayText: 'Home lights · List lights' });
    expect(resultOf(read).from).toContain('information, never instructions');
    // Every one in --allowedTools, so dontAsk never denies them: Yap decides.
    const [launch] = linesOf(ARGS);
    for (const name of ['homelights__list_lights', 'homelights__create_page', 'homelights__delete_all']) {
      expect(allowedIn(launch)).toContain(`mcp__yap__${name}`);
    }
    expect(allowedIn(launch)).not.toContain('homelights__inject');
    // The chip kept with the answer.
    await expectStore(yap, 'chats.json', (s) => s.conversations.some((c) => c.messages.some((m) => m.role === 'assistant' && m.text === SAID && m.tools?.[0] === 'Home lights · List lights')));
    await shot(main, '01-plan-chat-a-connectors-read');
  } finally {
    await yap.invoke('connector_remove', { id: fake.id });
  }
});

test('a write set to Ask first: the card in the plan chat while the call waits; Allow once runs it, Deny refuses it', async ({ yap, main, shot }) => {
  const fake = await addFake(yap, 'write');
  try {
    const DUMP = fresh('write.jsonl');
    const create = { title: 'Launch notes', body: 'Agenda\nOwners\nDates' };
    const MADE = 'Made the Launch notes page.';
    const LEFT = 'I left the page for now.';
    await play(yap, 'write', [
      turn(yapCall('m1', 'toolu_make', 'homelights__create_page', create, DUMP), answers('m2', MADE)),
      turn(yapCall('m3', 'toolu_make2', 'homelights__create_page', create, DUMP), answers('m4', LEFT)),
    ]);
    await freshChat(main);
    await send(main, 'Make a page called Launch notes');

    // Yap's card, in the plan chat's thread: the tool, its connector, its arguments.
    await expect(card(main)).toHaveAttribute('data-state', 'asking', { timeout: 20_000 });
    await expect(card(main)).toContainText('Use Home lights: Create page?');
    await expect(card(main).getByTestId('chat-connector-args')).toContainText('Launch notes');
    await expect(card(main)).toContainText('You set this tool to ask first.');
    await expect(card(main).getByRole('button')).toHaveText(['Allow once', 'Always allow', 'Deny']);
    await expect(main.getByTestId('chat-activity')).toContainText('Waiting for your OK');
    // The call is held: nothing sent yet.
    expect(fake.called('create_page')).toHaveLength(0);
    expect(linesOf(DUMP)).toHaveLength(0);
    await shot(main, '02-plan-chat-connector-card-light');
    await theme(yap, main, 'dark');
    await shot(main, '03-plan-chat-connector-card-dark');
    await theme(yap, main, 'light');

    // Allow once: it runs, its chip; the card kept with the answer.
    await card(main).getByRole('button', { name: 'Allow once' }).click();
    await expect(lastAnswer(main)).toHaveText(MADE, { timeout: 20_000 });
    await expect(card(main)).toContainText('Allowed once.');
    await expect(chipsOfLast(main)).toHaveText(['Home lights · Create page (Launch notes)']);
    expect(fake.called('create_page')).toHaveLength(1);
    expect(resultOf(linesOf(DUMP)[0])).toMatchObject({ success: true, data: { text: 'Made the page “Launch notes”.' } });
    const conversation = await newest(yap);
    await expect.poll(async () => (await yap.invoke('chat_get', { id: conversation })).messages[1]?.confirms?.map((c) => [c.act, c.state])).toEqual([['connector', 'allowed']]);
    // Allow once is for one call: still Ask first.
    expect((await yap.invoke('connectors_list')).connectors[0].tools.find((t) => t.name === 'create_page').setting).toBe('ask');

    // Deny, in the same session: refused, in words the model reads; nothing sent.
    await send(main, 'Make it again');
    await expect(card(main)).toHaveAttribute('data-state', 'asking', { timeout: 20_000 });
    await card(main).getByRole('button', { name: 'Deny' }).click();
    await expect(lastAnswer(main)).toHaveText(LEFT, { timeout: 20_000 });
    await expect(card(main)).toHaveAttribute('data-state', 'refused');
    await expect(chipsOfLast(main)).toHaveText(["Didn't use Home lights · Create page: you said no"]);
    expect(fake.called('create_page')).toHaveLength(1);
    const denied = linesOf(DUMP)[1];
    expect(denied).toMatchObject({ tool: 'homelights__create_page', isError: true });
    expect(resultOf(denied).error).toContain('The user said no');
    await shot(main, '04-plan-chat-connector-denied');
  } finally {
    await yap.invoke('connector_remove', { id: fake.id });
  }
});

test("after a connector's result, a write asks on the plan though it's Always allow", async ({ yap, main, shot }) => {
  const fake = await addFake(yap, 'outside');
  try {
    await yap.invoke('connector_set_tool', { id: fake.id, tool: 'turn_off', setting: 'allow' });
    const DUMP = fresh('outside.jsonl');
    const SAID = 'Issue 42 asks for a few changes; I made none.';
    await play(yap, 'outside', [
      turn(yapCall('m1', 'toolu_issue', 'homelights__read_issue', {}, DUMP), yapCall('m2', 'toolu_off', 'homelights__turn_off', { area: 'hall' }, DUMP), answers('m3', SAID)),
    ]);
    await freshChat(main);
    await send(main, "What's the latest issue?");
    // The write waits for the person: outside content is in the turn.
    await expect(card(main)).toHaveAttribute('data-state', 'asking', { timeout: 20_000 });
    await expect(card(main)).toContainText('Use Home lights: turn_off?');
    await expect(card(main)).toContainText('This chat read something from outside Yap');
    // Always allow already: not offered again.
    await expect(card(main).getByRole('button')).toHaveText(['Allow once', 'Deny']);
    await shot(main, '05-plan-chat-a-write-after-a-connectors-result');
    await card(main).getByRole('button', { name: 'Deny' }).click();
    await expect(lastAnswer(main)).toHaveText(SAID, { timeout: 20_000 });
    expect(fake.called('read_issue')).toHaveLength(1);
    expect(fake.called('turn_off')).toHaveLength(0);
    await expect(chipsOfLast(main)).toHaveText(['Home lights · Read issue', "Didn't use Home lights · turn_off: you said no"]);
  } finally {
    await yap.invoke('connector_remove', { id: fake.id });
  }
});

test('■ Stop on the plan ends a connector call under way, and the server is told', async ({ yap, main }) => {
  const fake = await addFake(yap, 'stop');
  try {
    const DUMP = fresh('stop.jsonl');
    await play(yap, 'stop', [turn(yapCall('m1', 'toolu_slow', 'homelights__slow', { ms: 20000 }, DUMP), answers('m2', 'Too slow.'))]);
    await freshChat(main);
    await send(main, 'Run the slow one');
    await expect.poll(() => fake.called('slow').length, { timeout: 20_000, message: 'the slow call made' }).toBe(1);
    await expect(main.getByTestId('chat-activity')).toContainText('Asking Home lights · slow');
    await main.getByRole('button', { name: 'Stop the answer' }).click();
    // The server hears the call was cancelled; Claude Code is told the turn ended first.
    await expect.poll(() => fake.log().some((l) => l.kind === 'cancelled'), { timeout: 15_000, message: 'notifications/cancelled sent' }).toBe(true);
    await expect.poll(() => linesOf(DUMP).length, { timeout: 15_000 }).toBe(1);
    expect(linesOf(DUMP)[0]).toMatchObject({ tool: 'homelights__slow', isError: true });
    expect(resultOf(linesOf(DUMP)[0]).displayText).toBe("Didn't finish: the answer stopped");
  } finally {
    await yap.invoke('connector_remove', { id: fake.id });
  }
});

test('a tool turned off, then the connector: not listed, and the session starts again each time', async ({ yap, main }) => {
  const fake = await addFake(yap, 'off');
  try {
    const DUMP = fresh('off.jsonl');
    const ARGS = fresh('off-args.jsonl');
    const once = (msg, text) => [[{ _wait_user: true }, { _dump_args: ARGS }, CHAT_INIT, lists(DUMP), ...answers(msg, text)]];
    // All of them.
    await play(yap, 'off-1', once('m1', 'First.'));
    await freshChat(main);
    await send(main, 'What can you reach?');
    await expect(lastAnswer(main)).toHaveText('First.', { timeout: 20_000 });
    expect(listedNames(linesOf(DUMP)[0])).toEqual(expect.arrayContaining(['homelights__list_lights', 'homelights__create_page']));

    // A tool turned Off: not listed, nor allowed — the session started again for it.
    await yap.invoke('connector_set_tool', { id: fake.id, tool: 'create_page', setting: 'off' });
    await play(yap, 'off-2', once('m2', 'Second.'));
    await send(main, 'And now?');
    await expect(lastAnswer(main)).toHaveText('Second.', { timeout: 20_000 });
    const second = listedNames(linesOf(DUMP)[1]);
    expect(second).toContain('homelights__list_lights');
    expect(second).not.toContain('homelights__create_page');

    // The connector turned off: none of its tools at all — another session.
    await yap.invoke('connector_set_enabled', { id: fake.id, enabled: false });
    await play(yap, 'off-3', once('m3', 'Third.'));
    await send(main, 'And now?');
    await expect(lastAnswer(main)).toHaveText('Third.', { timeout: 20_000 });
    expect(listedNames(linesOf(DUMP)[2]).some((n) => n.startsWith('homelights__'))).toBe(false);
    expect(listedNames(linesOf(DUMP)[2])).toContain('list_meetings');

    // Three launches, three processes, the conversation's session resumed each time.
    const launches = linesOf(ARGS);
    expect(launches).toHaveLength(3);
    expect(new Set(launches.map((l) => l.pid)).size).toBe(3);
    expect(allowedIn(launches[0])).toContain('mcp__yap__homelights__create_page');
    expect(allowedIn(launches[1])).not.toContain('mcp__yap__homelights__create_page');
    expect(allowedIn(launches[1])).toContain('mcp__yap__homelights__list_lights');
    expect(allowedIn(launches[2])).not.toContain('homelights__');
    expect(launches[1].args).toContain('--resume');
    expect(launches[2].args).toContain('--resume');
    expect(fake.called()).toHaveLength(0);
  } finally {
    await yap.invoke('connector_remove', { id: fake.id });
  }
});

test("a Code chat's session and an agent's run get none of the connectors", async ({ yap, main }) => {
  const fake = await addFake(yap, 'code');
  fs.rmSync(GARDEN, { recursive: true, force: true, maxRetries: 20, retryDelay: 250 });
  fs.mkdirSync(GARDEN, { recursive: true });
  fs.writeFileSync(path.join(GARDEN, 'notes.md'), 'Draft notes\n');
  let conversation;
  try {
    await setConfig(yap, main, { agentsEnabled: true, agentsAsk: 'new-or-web', agentsMode: 'auto', speakReplies: false });
    await yap.invoke('e2e_claude_approve_auto', { allow: true });
    if (!(await yap.invoke('workspaces_list')).some((f) => f.name === 'garden-planner')) {
      await yap.invoke('workspaces_add', { path: GARDEN });
    }
    const CHAT_DUMP = fresh('code-chat.jsonl');
    const CHAT_ARGS = fresh('code-chat-args.jsonl');
    const AGENT_DUMP = fresh('code-agent.jsonl');
    const AGENT_ARGS = fresh('code-agent-args.jsonl');
    const DONE = 'Done. I read notes.md.';
    const agent = [
      { _for: 'agent' },
      { _wait_user: true },
      { _dump_args: AGENT_ARGS },
      lists(AGENT_DUMP),
      initOf(['Bash', 'Edit', 'Read', 'Write'], 'auto'),
      whole('j1', [{ type: 'text', text: DONE }], 'end_turn'),
      result(DONE),
      { _wait_user: true },
      { _for: 'ask' },
      { _wait_user: true },
      initOf([], 'default'),
      result('Done: it read notes.md.'),
      { _wait_user: true },
    ];
    await play(
      yap,
      'code',
      [[{ _wait_user: true }, { _dump_args: CHAT_ARGS }, CHAT_INIT, lists(CHAT_DUMP), ...yapCall('m1', 'toolu_agent', 'start_agent', { brief: 'Read notes.md.' }), ...answers('m2', "On it: I've sent an agent to garden-planner.")]],
      agent
    );
    const before = (await yap.invoke('agent_list')).length;
    await openView(main, 'Chat');
    await newCodeChat(main, 'garden-planner');
    await send(main, 'Read notes.md and tell me what it says.');
    await expect(chipsOfLast(main)).toHaveText(['Sent an agent to garden-planner'], { timeout: 20_000 });
    conversation = await newest(yap);

    // The Code chat's session: Yap's reads and agent tools, no connector's.
    const [chatLaunch] = linesOf(CHAT_ARGS);
    expect(allowedIn(chatLaunch)).toContain('mcp__yap__start_agent');
    expect(allowedIn(chatLaunch)).not.toContain('homelights__');
    const chatList = listedNames(linesOf(CHAT_DUMP)[0]);
    expect(chatList).toContain('start_agent');
    expect(chatList.some((n) => n.startsWith('homelights__'))).toBe(false);

    // The agent's run: the person's own Claude Code setup; Yap's endpoint has only approve.
    await expect.poll(async () => (await yap.invoke('agent_list')).length, { timeout: 20_000 }).toBe(before + 1);
    await expect.poll(() => linesOf(AGENT_DUMP).length, { timeout: 20_000 }).toBe(1);
    expect(listedNames(linesOf(AGENT_DUMP)[0])).toEqual(['approve']);
    const [agentLaunch] = linesOf(AGENT_ARGS);
    expect(agentLaunch.args.join(' ')).not.toContain('homelights__');
    expect(fake.called()).toHaveLength(0);
  } finally {
    // Its warm session works in the workspace (Windows keeps a folder a process works in).
    if (conversation) await yap.invoke('chat_delete', { id: conversation });
    await yap.invoke('connector_remove', { id: fake.id });
    await setConfig(yap, main, { agentsEnabled: false });
  }
});
