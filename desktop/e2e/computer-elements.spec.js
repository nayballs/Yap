// Computer use, phase 5: elements instead of pixels (desktop/src-tauri/src/computer/elements.rs,
// computer/uia/tree.rs; docs/architecture/computer-use.md). A plan chat's Claude Code reads an
// allowed window as its accessibility tree (read_window: each element by a ref, what it takes,
// where it is on the screenshot), then sets a box's text, ticks a check box and clicks a button by
// ref — on the stand-in desktop, without the mouse moving — and a Send by ref asks first on the
// card like a click does (Don't: nothing pressed).
//
// Claude Code is this build's stand-in (`yap.exe fake-claude`), the Start menu a stand-in list
// (YAP_E2E_APPS): nothing of the real screen, mouse or keyboard. Fictional apps only.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, setConfig, jsonLines } from './support/fixtures.js';
import { fakeClaude, planConfig, SCRIPTS as PLAN_SCRIPTS } from './support/claude-plan.js';
import { RUNS } from './support/yap.js';

const HERE = path.join(RUNS, 'computer-elements-spec');
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
  tools: ['mcp__yap-computer__request_access', 'mcp__yap-computer__read_window', 'mcp__yap-computer__set_value', 'mcp__yap-computer__click_element', 'mcp__yap-computer__toggle_element'],
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
/** The mail app with a message box, Send, a check box and Save draft: e2…e5 in read_window's refs. */
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
      widgets: [widget('field', 'Message', 20, 50), widget('button', 'Send', 20, 110), widget('check', 'Request a receipt', 20, 170), widget('button', 'Save draft', 20, 230)],
    },
  ],
  cursor: [10, 10],
};
const ACCESS = { apps: ['Example Mail'], reason: 'To write your reply.' };

const test = base.extend({
  yapOptions: [
    async ({}, use) => {
      fs.mkdirSync(HERE, { recursive: true });
      const apps = path.join(HERE, 'start-menu.json');
      fs.writeFileSync(apps, JSON.stringify(START_MENU, null, 2));
      await use({
        name: 'computer-elements',
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

async function send(main, text) {
  await main.getByRole('textbox', { name: 'Message' }).fill(text);
  await main.getByRole('button', { name: 'Send', exact: true }).click();
}
async function freshChat(main) {
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).first().click();
}

test.beforeEach(async ({ yap, main }) => {
  await setConfig(yap, main, { agentsEnabled: false, computerUse: true, assistantScreen: true, assistantScreenCloud: true });
  await yap.invoke('e2e_computer_scene', { scene: SCENE });
});

test('a window read by ref: its text set, a box ticked and a button clicked without the mouse; Send by ref asks first', async ({ yap, main, shot }) => {
  const DUMP = fresh('elements.jsonl');
  const SAID = 'I wrote the reply, asked for a receipt and saved the draft. You said not to send it.';
  await play(yap, 'elements', [
    turn(
      use('request_access', ACCESS, DUMP),
      use('screenshot', {}, DUMP),
      use('read_window', { app: 'Example Mail' }, DUMP),
      use('set_value', { ref: 'e2', text: 'Running late', action_summary: 'Writes the reply' }, DUMP),
      use('toggle_element', { ref: 'e4', action_summary: 'Asks for a receipt' }, DUMP),
      use('click_element', { ref: 'e5', action_summary: 'Saves the draft' }, DUMP),
      use('click_element', { ref: 'e3', action_summary: 'Sends the reply' }, DUMP),
      answers(SAID),
    ),
  ]);
  await freshChat(main);
  await send(main, 'Reply "Running late", ask for a receipt, save it, then send it');
  await accessCard(main).getByRole('button', { name: 'Allow' }).click({ timeout: 20_000 });
  // Send by ref asks on the card, as a click on it does: Don't.
  const sendCard = main.getByTestId('chat-page-confirm').last();
  await expect(sendCard).toHaveAttribute('data-state', 'asking', { timeout: 20_000 });
  await expect(sendCard).toContainText('Send');
  await shot(main, '01-send-by-ref-asks');
  await sendCard.getByRole('button', { name: "Don't" }).click();
  await expect(lastAnswer(main)).toHaveText(SAID, { timeout: 20_000 });
  const [, , read, set, ticked, saved, sent] = jsonLines(DUMP);
  const tree = resultOf(read).data.tree;
  expect(tree).toContain('e1 window "Example Mail"');
  expect(tree).toMatch(/e2 edit "Message" \[set_value\] at \[\d+, \d+\]/);
  expect(tree).toContain('e4 check box "Request a receipt" (not ticked) [toggle]');
  expect(resultOf(set)).toMatchObject({ success: true, displayText: 'Writes the reply' });
  expect(resultOf(ticked)).toMatchObject({ success: true, displayText: 'Asks for a receipt' });
  expect(resultOf(saved)).toMatchObject({ success: true, displayText: 'Saves the draft' });
  expect(resultOf(sent).success).toBe(false);
  const app = (await state(yap)).windows[0];
  expect(app.widgets.map((w) => w.text)).toEqual(['Running late', '', 'on', '']);
  const s = await state(yap);
  expect(s.log).toEqual(['set Message', 'ticked Request a receipt', 'pressed Save draft']);
  expect(s.cursor, 'the mouse never moved').toEqual([10, 10]);
});
