// Computer use, phase 2: input (desktop/src-tauri/src/computer/act.rs, checks.rs, watch.rs,
// indicator.rs; docs/architecture/computer-use.md). A plan chat's Claude Code clicks and types in an
// allowed app over Yap's yap-computer server; the stand-in desktop (computer/sim.rs) takes the input
// and says what it did (`e2e_computer_state`). A click on Send asks first on the card; Esc (the
// person's input, pretended: `e2e_computer_touch`) stops the action under way and the rest of the
// turn's input; the Yap bar says "Yap is using your PC" while it drives.
//
// Claude Code is this build's stand-in (`yap.exe fake-claude`), the Start menu a stand-in list
// (YAP_E2E_APPS): nothing of the real screen, mouse, keyboard or Claude Code. Fictional apps only.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, setConfig, jsonLines } from './support/fixtures.js';
import { fakeClaude, planConfig, SCRIPTS as PLAN_SCRIPTS } from './support/claude-plan.js';
import { RUNS } from './support/yap.js';

const HERE = path.join(RUNS, 'computer-input-spec');
const SCRIPTS = path.join(HERE, 'scripts');
const SID = '{{session_id}}';
const MODEL = 'claude-haiku-5-5';
const SERVER = 'yap-computer';

const ev = (event) => ({ type: 'stream_event', event, parent_tool_use_id: null, session_id: SID });
const CHAT_INIT = {
  type: 'system',
  subtype: 'init',
  cwd: '{{cwd}}',
  session_id: SID,
  tools: ['mcp__yap-computer__request_access', 'mcp__yap-computer__screenshot', 'mcp__yap-computer__left_click', 'mcp__yap-computer__type'],
  mcp_servers: [
    { name: 'yap', status: 'connected' },
    { name: SERVER, status: 'connected' },
  ],
  model: MODEL,
  permissionMode: 'dontAsk',
  apiKeySource: 'none',
  claude_code_version: '2.1.296',
  capabilities: ['interrupt_receipt_v1'],
};
const whole = (id, content, stop) => ({ type: 'assistant', message: { id, type: 'message', role: 'assistant', model: MODEL, content, stop_reason: stop }, parent_tool_use_id: null, session_id: SID });
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
const calls = (msg, id, name, input) => [
  ev({ type: 'message_start', message: { id: msg, type: 'message', role: 'assistant', model: MODEL, content: [] } }),
  ev({ type: 'content_block_start', index: 0, content_block: { type: 'tool_use', id, name, input: {} } }),
  ev({ type: 'content_block_delta', index: 0, delta: { type: 'input_json_delta', partial_json: JSON.stringify(input) } }),
  ev({ type: 'content_block_stop', index: 0 }),
  whole(msg, [{ type: 'tool_use', id, name, input }], 'tool_use'),
  ev({ type: 'message_stop' }),
];
let n = 0;
/** One of Yap's computer-use tools called, what it gave written to `dump`. */
const use = (tool, input, dump) => {
  n += 1;
  return [...calls(`m${n}`, `toolu_${n}`, `mcp__${SERVER}__${tool}`, input), { _mcp: { tool, arguments: input, result_for: `toolu_${n}`, dump, server: SERVER } }];
};
const answers = (text) => {
  n += 1;
  const msg = `m${n}`;
  return [
    ev({ type: 'message_start', message: { id: msg, type: 'message', role: 'assistant', model: MODEL, content: [] } }),
    ev({ type: 'content_block_start', index: 0, content_block: { type: 'text', text: '' } }),
    ev({ type: 'content_block_delta', index: 0, delta: { type: 'text_delta', text } }),
    ev({ type: 'content_block_stop', index: 0 }),
    whole(msg, [{ type: 'text', text }], 'end_turn'),
    ev({ type: 'message_stop' }),
    result(text),
  ];
};
const turn = (...lines) => [{ _wait_user: true }, CHAT_INIT, ...lines.flat()];

function script(name, chatTurns) {
  fs.mkdirSync(SCRIPTS, { recursive: true });
  const file = path.join(SCRIPTS, `${name}.jsonl`);
  const lines = [{ _for: 'chat' }, ...chatTurns.flat(), { _wait_user: true }];
  fs.writeFileSync(file, lines.map((l) => JSON.stringify(l)).join('\n') + '\n');
  return file;
}
function fresh(name) {
  fs.mkdirSync(HERE, { recursive: true });
  const file = path.join(HERE, name);
  fs.rmSync(file, { force: true });
  return file;
}
const resultOf = (call) => JSON.parse(call.content[0].text);

const MAIL_EXE = 'C:\\Apps\\ExampleMail\\mail.exe';
const START_MENU = [{ id: MAIL_EXE, name: 'Example Mail', target: MAIL_EXE }];
const widget = (kind, name, x, y) => ({ kind, name, x, y, w: 200, h: 34 });
/** One 1280 × 720 display, the mail app in front with a message box, Send, a PIN. */
const SCENE = {
  displays: [{ x: 0, y: 0, w: 1280, h: 720, primary: true }],
  windows: [
    {
      x: 100,
      y: 80,
      w: 700,
      h: 460,
      exe: MAIL_EXE,
      title: 'Example Mail',
      widgets: [widget('field', 'Message', 20, 50), widget('button', 'Send', 20, 110), widget('password', 'PIN', 20, 170)],
    },
  ],
  cursor: [10, 10],
};
/** Each widget's middle on the screenshot (the frame is the display's own: 1280 fits). */
const MESSAGE = [100 + 20 + 100, 80 + 50 + 17];
const SEND = [100 + 20 + 100, 80 + 110 + 17];

const test = base.extend({
  yapOptions: [
    async ({}, use) => {
      fs.mkdirSync(HERE, { recursive: true });
      const apps = path.join(HERE, 'start-menu.json');
      fs.writeFileSync(apps, JSON.stringify(START_MENU, null, 2));
      await use({
        name: 'computer-input',
        env: { ...fakeClaude(path.join(PLAN_SCRIPTS, 'plan-chat.jsonl')), YAP_E2E_APPS: apps },
        config: planConfig({ 'claude-plan': 'claude-plan' }, { memoryLearnChats: false, webSearch: false, assistantScreen: true, assistantScreenCloud: true, assistantScreenAsked: true, computerUse: true }),
      });
    },
    { scope: 'worker' },
  ],
});

const thread = (main) => main.locator('.msgs');
const lastAnswer = (main) => thread(main).locator('.cbubble.assistant').last();
const accessCard = (main) => main.getByTestId('chat-computer-access').last();
const play = (yap, name, chatTurns) => yap.invoke('e2e_claude_script', { script: script(name, chatTurns) });
const state = (yap) => yap.invoke('e2e_computer_state');
const mail = async (yap) => (await state(yap)).windows.find((w) => w.title === 'Example Mail');

async function send(main, text) {
  await main.getByRole('textbox', { name: 'Message' }).fill(text);
  await main.getByRole('button', { name: 'Send', exact: true }).click();
}
async function freshChat(main) {
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).first().click();
}
async function allow(main) {
  const card = accessCard(main);
  await expect(card).toHaveAttribute('data-state', 'asking', { timeout: 20_000 });
  await card.getByRole('button', { name: 'Allow' }).click();
}
const ACCESS = { apps: ['Example Mail'], reason: 'To write your reply.' };

test.beforeEach(async ({ yap, main }) => {
  await setConfig(yap, main, { agentsEnabled: false, computerUse: true, assistantScreen: true, assistantScreenCloud: true });
  await yap.invoke('e2e_computer_scene', { scene: SCENE });
});

test('a plan chat clicks into a field and types; the app has the words, and the bar says Yap is using your PC', async ({ yap, main, shot }) => {
  const DUMP = fresh('typed.jsonl');
  const SAID = 'I wrote "Running late" in the message box.';
  await play(yap, 'typed', [
    turn(
      use('request_access', ACCESS, DUMP),
      use('screenshot', {}, DUMP),
      use('left_click', { coordinate: MESSAGE, action_summary: 'Clicks into the message box' }, DUMP),
      use('type', { text: 'Running late', action_summary: 'Types the reply' }, DUMP),
      answers(SAID),
    ),
  ]);
  await freshChat(main);
  await send(main, 'Write "Running late" in my mail');
  await allow(main);
  await expect(lastAnswer(main)).toHaveText(SAID, { timeout: 20_000 });
  // The bar said so while it drove (it lingers a few seconds after the last action).
  await expect(yap.overlay.getByTestId('bar-computer')).toBeVisible({ timeout: 5_000 });
  await expect(yap.overlay.getByTestId('bar-computer')).toContainText('Yap is using your PC');
  await expect(yap.overlay.getByTestId('bar-computer')).toContainText('Esc to stop');
  const [, , clicked, typed] = jsonLines(DUMP);
  expect(resultOf(clicked)).toMatchObject({ success: true, displayText: 'Clicks into the message box' });
  expect(resultOf(typed)).toMatchObject({ success: true, displayText: 'Types the reply' });
  const app = await mail(yap);
  expect(app.widgets[0].text).toBe('Running late');
  expect((await state(yap)).log).toEqual(['focused Message', 'typed into Message']);
  await shot(main, '01-clicked-and-typed');
  // …and goes once it's done.
  await expect(yap.overlay.getByTestId('bar-computer')).toBeHidden({ timeout: 10_000 });
});

test("a click on Send asks first on the card; Don't presses nothing", async ({ yap, main, shot }) => {
  const DUMP = fresh('send.jsonl');
  const SAID = "You said no, so I didn't send it.";
  await play(yap, 'send', [turn(use('request_access', ACCESS, DUMP), use('screenshot', {}, DUMP), use('left_click', { coordinate: SEND, action_summary: 'Sends the reply' }, DUMP), answers(SAID))]);
  await freshChat(main);
  await send(main, 'Send my reply');
  await allow(main);
  const card = main.getByTestId('chat-page-confirm').last();
  await expect(card).toHaveAttribute('data-state', 'asking', { timeout: 20_000 });
  await expect(card).toContainText('Send');
  await expect(card).toContainText('Example Mail');
  await shot(main, '02-send-asks-first');
  await card.getByRole('button', { name: "Don't" }).click();
  await expect(lastAnswer(main)).toHaveText(SAID, { timeout: 20_000 });
  const clicked = jsonLines(DUMP)[2];
  expect(resultOf(clicked).success).toBe(false);
  expect(resultOf(clicked).error).toContain('said no');
  expect((await state(yap)).log).not.toContain('pressed Send');
});

test("answering Yap's Send card by mouse is the person's answer, never taking over: Allow once sends", async ({ yap, main }) => {
  const DUMP = fresh('send-allowed.jsonl');
  const SAID = 'I sent your reply.';
  await play(yap, 'send-allowed', [turn(use('request_access', ACCESS, DUMP), use('screenshot', {}, DUMP), use('left_click', { coordinate: SEND, action_summary: 'Sends the reply' }, DUMP), answers(SAID))]);
  await freshChat(main);
  await send(main, 'Send my reply');
  await allow(main);
  const card = main.getByTestId('chat-page-confirm').last();
  await expect(card).toHaveAttribute('data-state', 'asking', { timeout: 20_000 });
  // The person's mouse on its way to the card, their click and a key on it: answering it, never taking
  // over (before 20 Nov 2026 the watch, armed for the click, took them as the person stopping Yap).
  for (const kind of ['move', 'button', 'key', 'esc']) expect(await yap.invoke('e2e_computer_touch', { kind }), kind).toBe(false);
  await card.getByRole('button', { name: 'Allow once' }).click();
  await expect(lastAnswer(main)).toHaveText(SAID, { timeout: 20_000 });
  const clicked = jsonLines(DUMP)[2];
  expect(resultOf(clicked)).toMatchObject({ success: true, displayText: 'Sends the reply' });
  expect((await state(yap)).log).toContain('pressed Send');
});

test("Esc while Claude thinks between two actions stops the next one, and the bar's line goes at once", async ({ yap, main }) => {
  const DUMP = fresh('esc-between.jsonl');
  const SAID = 'You pressed Esc, so I stopped.';
  await play(yap, 'esc-between', [
    turn(
      use('request_access', ACCESS, DUMP),
      use('screenshot', {}, DUMP),
      use('left_click', { coordinate: MESSAGE, action_summary: 'Clicks into the message box' }, DUMP),
      // Claude thinking about its next step.
      { _sleep_ms: 3000 },
      use('type', { text: 'Running late', action_summary: 'Types the reply' }, DUMP),
      answers(SAID),
    ),
  ]);
  await freshChat(main);
  await send(main, 'Write my reply');
  await allow(main);
  await expect(yap.overlay.getByTestId('bar-computer')).toBeVisible({ timeout: 20_000 });
  await expect.poll(() => yap.invoke('e2e_computer_touch', { kind: 'esc' }), { timeout: 10_000 }).toBe(true);
  // The line goes at once, not after its linger…
  await expect(yap.overlay.getByTestId('bar-computer')).toBeHidden({ timeout: 2_000 });
  await expect(lastAnswer(main)).toHaveText(SAID, { timeout: 20_000 });
  // …and the next action never runs (found by a real-API run on 20 Nov 2026: it did).
  const [, , clicked, typed] = jsonLines(DUMP);
  expect(resultOf(clicked).success).toBe(true);
  expect(resultOf(typed).success).toBe(false);
  expect(resultOf(typed).displayText).toBe('Stopped: you pressed Esc');
  expect((await state(yap)).log).toEqual(['focused Message']);
});

test('Esc stops the action under way, and the rest of the turn is refused', async ({ yap, main }) => {
  const DUMP = fresh('esc.jsonl');
  const SAID = 'You took over, so I stopped.';
  await play(yap, 'esc', [
    turn(
      use('request_access', ACCESS, DUMP),
      use('screenshot', {}, DUMP),
      use('hold_key', { text: 'shift', duration: 8, action_summary: 'Holds Shift' }, DUMP),
      use('left_click', { coordinate: MESSAGE, action_summary: 'Clicks into the message box' }, DUMP),
      answers(SAID),
    ),
  ]);
  await freshChat(main);
  await send(main, 'Select the message');
  await allow(main);
  // While it holds Shift, the person presses Esc.
  await expect(yap.overlay.getByTestId('bar-computer')).toBeVisible({ timeout: 20_000 });
  await expect.poll(() => yap.invoke('e2e_computer_touch', { kind: 'esc' }), { timeout: 10_000 }).toBe(true);
  await expect(lastAnswer(main)).toHaveText(SAID, { timeout: 20_000 });
  const [, , held, clicked] = jsonLines(DUMP);
  expect(resultOf(held).error).toContain('pressed Esc');
  expect(resultOf(held).displayText).toBe('Stopped: you pressed Esc');
  expect(resultOf(clicked).error).toContain('took over');
  // Nothing more reached the app, and the indicator went at once.
  expect((await state(yap)).log).toEqual([]);
  await expect(yap.overlay.getByTestId('bar-computer')).toBeHidden({ timeout: 3_000 });
});
