// Computer use, phase 4: teach mode (desktop/src-tauri/src/computer/teach.rs, teach/tooltip.rs;
// docs/architecture/computer-use.md). A plan chat's Claude Code asks to guide the person
// (request_teach_access: the access card worded as a tour), then shows steps: each tooltip waits
// for Next, which runs the step's actions on the stand-in desktop and answers with a screenshot;
// a teach_batch left with Exit answers exited with the steps done; Esc on a tooltip is its Exit.
// Yap's main window steps aside for the tour (a test run only notes it) and the tour ends with
// the turn.
//
// The tooltip is a native window a test run never puts on the real screen: its words are read,
// and Next or Exit pressed, with `e2e_computer_teach`. Claude Code is this build's stand-in
// (`yap.exe fake-claude`), the Start menu a stand-in list (YAP_E2E_APPS). Fictional apps only.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, setConfig, jsonLines } from './support/fixtures.js';
import { fakeClaude, planConfig, SCRIPTS as PLAN_SCRIPTS } from './support/claude-plan.js';
import { RUNS } from './support/yap.js';

const HERE = path.join(RUNS, 'computer-teach-spec');
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
  tools: ['mcp__yap-computer__request_teach_access', 'mcp__yap-computer__screenshot', 'mcp__yap-computer__teach_step', 'mcp__yap-computer__teach_batch'],
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
const SCENE = {
  displays: [{ x: 0, y: 0, w: 1280, h: 720, primary: true }],
  windows: [{ x: 100, y: 80, w: 700, h: 460, exe: MAIL_EXE, title: 'Example Mail', widgets: [widget('field', 'Message', 20, 50), widget('button', 'Save draft', 20, 110)] }],
  cursor: [10, 10],
};
/** Each widget's middle on the screenshot (the frame is the display's own: 1280 fits). */
const MESSAGE = [100 + 20 + 100, 80 + 50 + 17];
const SAVE = [100 + 20 + 100, 80 + 110 + 17];
const ACCESS = { apps: ['Example Mail'], reason: 'writing a reply' };

const test = base.extend({
  yapOptions: [
    async ({}, use) => {
      fs.mkdirSync(HERE, { recursive: true });
      const apps = path.join(HERE, 'start-menu.json');
      fs.writeFileSync(apps, JSON.stringify(START_MENU, null, 2));
      await use({
        name: 'computer-teach',
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
const teachNow = (yap, press) => yap.invoke('e2e_computer_teach', press ? { press } : {});
const message = async (yap) => (await state(yap)).windows.find((w) => w.title === 'Example Mail').widgets[0].text;

async function send(main, text) {
  await main.getByRole('textbox', { name: 'Message' }).fill(text);
  await main.getByRole('button', { name: 'Send', exact: true }).click();
}
async function freshChat(main) {
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).first().click();
}
/** The tooltip showing step `step` of `of`: its words. */
async function tooltip(yap, step, of) {
  await expect.poll(async () => (await teachNow(yap)).tip, { timeout: 20_000, message: `the tooltip for step ${step} of ${of}` }).toMatchObject({ step, of });
  return (await teachNow(yap)).tip;
}

test.beforeEach(async ({ yap, main }) => {
  await setConfig(yap, main, { agentsEnabled: false, computerUse: true, assistantScreen: true, assistantScreenCloud: true });
  await yap.invoke('e2e_computer_scene', { scene: SCENE });
});

test('a guided tour: the card worded as one, a step whose Next runs it, a batch left with Exit', async ({ yap, main, shot }) => {
  const DUMP = fresh('tour.jsonl');
  const SAID = 'You saw where the message goes and how it is typed.';
  const step = {
    explanation: 'This is the message box: what you type here is your reply.',
    next_preview: "Next: I'll click it and type Hello.",
    anchor: MESSAGE,
    actions: [
      { action: 'left_click', coordinate: MESSAGE, action_summary: 'Clicks into the message box' },
      { action: 'type', text: 'Hello', action_summary: 'Types Hello' },
    ],
  };
  const batch = {
    steps: [
      { explanation: 'Add a little more.', next_preview: "Next: I'll type ' there'.", actions: [{ action: 'type', text: ' there', action_summary: 'Types more' }] },
      { explanation: 'Save draft keeps it for later.', next_preview: "Next: I'll click Save draft.", anchor: SAVE, actions: [{ action: 'left_click', coordinate: SAVE, action_summary: 'Clicks Save draft' }] },
    ],
  };
  await play(yap, 'tour', [turn(use('request_teach_access', ACCESS, DUMP), use('screenshot', {}, DUMP), use('teach_step', step, DUMP), use('teach_batch', batch, DUMP), answers(SAID))]);
  await freshChat(main);
  await send(main, 'Show me how to write a reply');
  const card = accessCard(main);
  await expect(card).toHaveAttribute('data-state', 'asking', { timeout: 20_000 });
  await expect(card).toContainText('Let Yap’s AI guide you through this?');
  await expect(card).toContainText('writing a reply');
  await expect(card).toContainText('a tooltip shows each step');
  await expect(card.locator('input[type=checkbox]')).toHaveCount(0);
  await shot(main, '01-teach-card');
  await card.getByRole('button', { name: 'Allow' }).click();
  // The step's tooltip: its words, where it points; Yap's window stepped aside.
  const first = await tooltip(yap, 1, 1);
  expect(first).toMatchObject({ explanation: step.explanation, nextPreview: step.next_preview, anchor: MESSAGE });
  expect((await teachNow(yap)).tour).toMatchObject({ mainHidden: true, exited: false });
  expect(await message(yap)).toBe('', 'nothing done before Next');
  expect((await teachNow(yap, 'next')).pressed).toBe(true);
  // The batch: Next on its first step, Exit on its second.
  const second = await tooltip(yap, 1, 2);
  expect(second.anchor).toBeNull();
  expect(await message(yap)).toBe('Hello');
  await teachNow(yap, 'next');
  const third = await tooltip(yap, 2, 2);
  expect(third.explanation).toBe('Save draft keeps it for later.');
  await teachNow(yap, 'exit');
  await expect(lastAnswer(main)).toHaveText(SAID, { timeout: 20_000 });
  const [allowed, , stepped, batched] = jsonLines(DUMP);
  expect(resultOf(allowed)).toMatchObject({ success: true });
  expect(resultOf(stepped)).toMatchObject({ success: true, data: { screenshot: { display: 'Display 1' } } });
  // The step's screenshot after its actions comes in its result.
  expect(stepped.content.filter((c) => c.type === 'image')).toHaveLength(1);
  expect(resultOf(batched)).toMatchObject({ success: true, data: { exited: true, stepsCompleted: 1 } });
  expect(await message(yap)).toBe('Hello there');
  expect((await state(yap)).log).not.toContain('pressed Save draft');
  // The turn's end closed the tour.
  await expect.poll(async () => (await teachNow(yap)).tour).toBeNull();
});

test('Esc on a tooltip is its Exit; a step after it answers exited', async ({ yap, main }) => {
  const DUMP = fresh('esc.jsonl');
  const SAID = 'OK, we stopped there.';
  const step = { explanation: 'This is the message box.', next_preview: "Next: I'll type in it.", actions: [{ action: 'type', text: 'never', action_summary: 'Types' }] };
  await play(yap, 'esc', [turn(use('request_teach_access', ACCESS, DUMP), use('screenshot', {}, DUMP), use('teach_step', step, DUMP), use('teach_step', step, DUMP), answers(SAID))]);
  await freshChat(main);
  await send(main, 'Teach me to reply');
  await accessCard(main).getByRole('button', { name: 'Allow' }).click({ timeout: 20_000 });
  await tooltip(yap, 1, 1);
  expect(await yap.invoke('e2e_computer_touch', { kind: 'esc' })).toBe(true);
  await expect(lastAnswer(main)).toHaveText(SAID, { timeout: 20_000 });
  const [, , left, again] = jsonLines(DUMP);
  expect(resultOf(left)).toMatchObject({ success: true, data: { exited: true } });
  expect(resultOf(again)).toMatchObject({ data: { exited: true } });
  expect(await message(yap)).toBe('');
});
