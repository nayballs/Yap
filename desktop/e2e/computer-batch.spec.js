// Computer use, phase 3 (desktop/src-tauri/src/computer/batch.rs, open.rs, clipboard.rs, and the API's
// own toolset in ai/llm/anthropic.rs; docs/architecture/computer-use.md):
// - a plan chat's Claude Code runs several actions in one computer_batch (its coordinates from the
//   screenshot before it, a screenshot inside it coming back in place), puts text on the clipboard
//   with its grant, and opens an allowed app that isn't on screen;
// - a batch stops at its first error, and nothing after it runs;
// - Claude on an Anthropic key gets the API's own computer toolset in place of its members as
//   function tools: its calls and results carry toolset_name, a screenshot's picture in its own
//   result.
//
// - asked out loud from another app (the Voice Assistant), the access card is said in one line and
//   put on the Yap bar (Allow / Don't); a spoken yes allows it, and the turn goes on to act.
//
// Claude Code is this build's stand-in (`yap.exe fake-claude`), Anthropic the fake server, the
// Start menu a stand-in list (YAP_E2E_APPS), the desktop the stand-in (computer/sim.rs): nothing of
// the real screen, mouse, keyboard or clipboard. Fictional apps only.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, setConfig, jsonLines, inAnotherApp } from './support/fixtures.js';
import { fakeClaude, planConfig, SCRIPTS as PLAN_SCRIPTS } from './support/claude-plan.js';
import { RUNS } from './support/yap.js';
import { startFakeLlm } from './support/fake-llm.js';

const HERE = path.join(RUNS, 'computer-batch-spec');
const SCRIPTS = path.join(HERE, 'scripts');
const SID = '{{session_id}}';
const MODEL = 'claude-haiku-5-5';
const SERVER = 'yap-computer';
const CLAUDE_KEY = 'sk-ant-e2e-fake-1234567890';

const ev = (event) => ({ type: 'stream_event', event, parent_tool_use_id: null, session_id: SID });
const CHAT_INIT = {
  type: 'system',
  subtype: 'init',
  cwd: '{{cwd}}',
  session_id: SID,
  tools: ['mcp__yap-computer__request_access', 'mcp__yap-computer__screenshot', 'mcp__yap-computer__computer_batch', 'mcp__yap-computer__open_application', 'mcp__yap-computer__write_clipboard'],
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
const NOTES_EXE = 'C:\\Apps\\ExampleNotes\\notes.exe';
const START_MENU = [
  { id: MAIL_EXE, name: 'Example Mail', target: MAIL_EXE },
  { id: NOTES_EXE, name: 'Example Notes', target: NOTES_EXE },
];
const widget = (kind, name, x, y) => ({ kind, name, x, y, w: 200, h: 34 });
/** One 1280 × 720 display, the mail app in front with a message and a subject box. */
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
      widgets: [widget('field', 'Message', 20, 50), widget('field', 'Subject', 20, 110)],
    },
  ],
  cursor: [10, 10],
};
/** Each widget's middle on the screenshot (the frame is the display's own: 1280 fits). */
const MESSAGE = [100 + 20 + 100, 80 + 50 + 17];
const SUBJECT = [100 + 20 + 100, 80 + 110 + 17];
/** The desktop, outside every window: masked (File Explorer isn't allowed). */
const DESKTOP = [1100, 650];

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
      fs.mkdirSync(HERE, { recursive: true });
      const apps = path.join(HERE, 'start-menu.json');
      fs.writeFileSync(apps, JSON.stringify(START_MENU, null, 2));
      await use({
        name: 'computer-batch',
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
const field = async (yap, name) => (await state(yap)).windows.find((w) => w.title === 'Example Mail').widgets.find((g) => g.name === name).text;

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

test.beforeEach(async ({ yap, main }) => {
  await setConfig(yap, main, { agentsEnabled: false, computerUse: true, assistantScreen: true, assistantScreenCloud: true });
  await yap.invoke('e2e_computer_scene', { scene: SCENE });
  await yap.invoke('e2e_opened_apps', { clear: true });
});

test('a plan chat batches a click, typing and a screenshot, copies to the clipboard and opens an allowed app', async ({ yap, main, shot }) => {
  const DUMP = fresh('batch.jsonl');
  const SAID = 'I wrote it, copied the subject and opened your notes.';
  const ACCESS = { apps: ['Example Mail', 'Example Notes'], reason: 'To write your reply and keep a note.', clipboardWrite: true };
  await play(yap, 'batch', [
    turn(
      use('request_access', ACCESS, DUMP),
      use('screenshot', {}, DUMP),
      use(
        'computer_batch',
        {
          actions: [
            { action: 'left_click', coordinate: MESSAGE, action_summary: 'Clicks into the message box' },
            { action: 'type', text: 'Running late', action_summary: 'Types the reply' },
            { action: 'left_click', coordinate: SUBJECT, action_summary: 'Clicks into the subject' },
            { action: 'type', text: 'Today', action_summary: 'Types the subject' },
            { action: 'screenshot', scale: 0.5 },
          ],
        },
        DUMP,
      ),
      use('write_clipboard', { text: 'Today' }, DUMP),
      use('open_application', { app: 'Example Notes' }, DUMP),
      answers(SAID),
    ),
  ]);
  await freshChat(main);
  await send(main, 'Write "Running late" in my mail, subject Today, and open my notes');
  await allow(main);
  await expect(lastAnswer(main)).toHaveText(SAID, { timeout: 20_000 });
  const [, , batch, copied, opened] = jsonLines(DUMP);
  expect(resultOf(batch)).toMatchObject({ success: true, data: { completed: 5 } });
  expect(resultOf(batch).displayTexts.slice(0, 4)).toEqual(['Clicks into the message box', 'Types the reply', 'Clicks into the subject', 'Types the subject']);
  // The screenshot inside the batch comes back in its result, in place.
  expect(batch.content.filter((c) => c.type === 'image')).toHaveLength(1);
  expect(await field(yap, 'Message')).toBe('Running late');
  expect(await field(yap, 'Subject')).toBe('Today');
  expect(resultOf(copied)).toMatchObject({ success: true, displayText: 'Copied to the clipboard' });
  expect((await state(yap)).clipboard).toBe('Today');
  expect(resultOf(opened)).toMatchObject({ success: true, displayText: 'Opened Example Notes' });
  // Opened from the Start menu's own entry (recorded in a test run), its window in front.
  expect((await yap.invoke('e2e_opened_apps')).map((a) => a.name)).toEqual(['Example Notes']);
  expect((await state(yap)).windows[0].title).toBe('Example Notes');
  await shot(main, '01-batch-clipboard-open');
});

test('a batch stops at its first error, and nothing after it runs', async ({ yap, main }) => {
  const DUMP = fresh('stopped.jsonl');
  const SAID = "I couldn't click there: it isn't one of your allowed apps.";
  await play(yap, 'stopped', [
    turn(
      use('request_access', { apps: ['Example Mail'], reason: 'To write your reply.' }, DUMP),
      use('screenshot', {}, DUMP),
      use(
        'computer_batch',
        {
          actions: [
            { action: 'left_click', coordinate: MESSAGE, action_summary: 'Clicks into the message box' },
            { action: 'left_click', coordinate: DESKTOP, action_summary: 'Clicks the desktop' },
            { action: 'type', text: 'never', action_summary: 'Types a word' },
          ],
        },
        DUMP,
      ),
      use('write_clipboard', { text: 'never' }, DUMP),
      answers(SAID),
    ),
  ]);
  await freshChat(main);
  await send(main, 'Write in my mail');
  await allow(main);
  await expect(lastAnswer(main)).toHaveText(SAID, { timeout: 20_000 });
  const [, , batch, copied] = jsonLines(DUMP);
  const stopped = resultOf(batch);
  expect(stopped.success).toBe(false);
  expect(stopped.error).toMatch(/^Batch stopped at actions\[1\] \(left_click\): .* 1 completed, 1 remaining\.$/);
  expect(await field(yap, 'Message')).toBe('');
  expect((await state(yap)).log).toEqual(['focused Message']);
  // No clipboardWrite asked for: refused.
  expect(resultOf(copied).error).toContain('clipboardWrite');
});

/**
 * A display point in the frame of the last screenshot the request carries (its result's `frame`: a
 * model on this PC's, as the fake's local address makes it, is smaller than the display).
 */
function inFrame(record, [x, y]) {
  const texts = record.body.messages
    .flatMap((m) => (Array.isArray(m.content) ? m.content : []))
    .filter((b) => b.type === 'tool_result')
    .flatMap((b) => (Array.isArray(b.content) ? b.content : [{ text: b.content }]))
    .map((c) => c.text ?? '');
  const frames = texts.flatMap((t) => {
    try {
      const f = JSON.parse(t).data?.frame;
      return f ? [f] : [];
    } catch {
      return [];
    }
  });
  const k = frames.length ? frames.at(-1).w / SCENE.displays[0].w : 1;
  return [Math.round(x * k), Math.round(y * k)];
}

test("Claude on an API key gets the API's own computer toolset; its calls and results carry toolset_name", async ({ yap, main, fakeLlm }) => {
  const cfg = await yap.invoke('get_config');
  const claude = { enabled: true, provider: 'anthropic', baseUrl: fakeLlm.base, model: 'claude-opus-5-5', apiKey: CLAUDE_KEY, apiKeys: {}, prompt: '', disableThinking: false };
  await setConfig(yap, main, { llmScopes: { ...cfg.llmScopes, chat: claude } });
  try {
    const before = fakeLlm.requests.length;
    fakeLlm.scriptChat([
      { tools: [{ name: 'request_access', args: { apps: ['Example Mail'], reason: 'To write your reply.' } }] },
      { tools: [{ name: 'screenshot', args: {}, toolset: true }] },
      {
        tools: [
          { name: 'left_click', args: (record) => ({ coordinate: inFrame(record, MESSAGE) }), toolset: true },
          { name: 'type', args: { text: 'On my way' }, toolset: true },
        ],
      },
      { content: 'Done: it says "On my way".' },
    ]);
    await freshChat(main);
    await send(main, 'Write "On my way" in my mail');
    await allow(main);
    await expect(lastAnswer(main)).toContainText('On my way', { timeout: 20_000 });
    expect(await field(yap, 'Message')).toBe('On my way');
    const steps = fakeLlm.requests.slice(before).filter((r) => r.api === 'messages' && r.kind !== 'chatSummary' && r.tools.includes('request_access'));
    expect(steps.length).toBeGreaterThanOrEqual(4);
    // The toolset in place of its members; Yap's other computer-use tools beside it.
    expect(steps[0].serverTools.map((t) => t.type)).toContain('computer_toolset_20260801');
    expect(steps[0].tools).not.toContain('screenshot');
    expect(steps[0].tools).not.toContain('left_click');
    expect(steps[0].tools).toEqual(expect.arrayContaining(['request_access', 'open_application', 'computer_batch']));
    // The screenshot's result: toolset_name echoed, its picture inside it.
    const results = (r) => r.body.messages.flatMap((m) => (Array.isArray(m.content) ? m.content : [])).filter((b) => b.type === 'tool_result');
    const shotResult = results(steps[2]).find((b) => b.toolset_name === 'computer');
    expect(shotResult, JSON.stringify(results(steps[2]).map((b) => Object.keys(b)))).toBeTruthy();
    expect(shotResult.content.some((c) => c.type === 'image')).toBe(true);
    // The click's and the typing's results too; request_access's is an ordinary one.
    const last = results(steps[3]);
    expect(last.filter((b) => b.toolset_name === 'computer')).toHaveLength(3);
    expect(last.filter((b) => !b.toolset_name)).toHaveLength(1);
    expect(last.some((b) => b.is_error)).toBe(false);
  } finally {
    await setConfig(yap, main, { llmScopes: cfg.llmScopes });
  }
});

test('asked out loud from another app: the access card is said and on the bar, and a spoken yes lets it act', async ({ yap, main, fakeLlm, shot }) => {
  const cfg = await yap.invoke('get_config');
  const claude = { enabled: true, provider: 'anthropic', baseUrl: fakeLlm.base, model: 'claude-opus-5-5', apiKey: CLAUDE_KEY, apiKeys: {}, prompt: '', disableThinking: false };
  await setConfig(yap, main, { llmScopes: { ...cfg.llmScopes, chat: claude }, speakReplies: true });
  await yap.invoke('tts_e2e_spoken', { clear: true });
  await inAnotherApp(yap);
  const spoken = async () => (await yap.invoke('tts_e2e_spoken')).map((s) => s.text).join('\n');
  const barCard = async () => (await yap.invoke('bar_status')).cards.find((c) => c.id === 'askFirst') ?? null;
  try {
    fakeLlm.scriptChat([
      { tools: [{ name: 'request_access', args: { apps: ['Example Mail'], reason: 'To write your reply.' } }] },
      { tools: [{ name: 'screenshot', args: {}, toolset: true }] },
      {
        tools: [
          { name: 'left_click', args: (record) => ({ coordinate: inFrame(record, MESSAGE) }), toolset: true },
          { name: 'type', args: { text: 'See you soon' }, toolset: true },
        ],
      },
      { content: 'I wrote "See you soon" in your mail.' },
    ]);
    await yap.invoke('e2e_assistant_ask', { text: 'Write see you soon in my mail', listenMs: 0 });
    await expect.poll(spoken, { timeout: 20_000 }).toContain('Let me use Example Mail, to write your reply? Say yes, or press Allow.');
    await expect.poll(barCard, { timeout: 15_000 }).not.toBeNull();
    const card = await barCard();
    expect(card).toMatchObject({ title: 'Let Yap use your apps?', icon: 'assistant' });
    expect(card.body).toBe('Example Mail (clicks and typing). To write your reply. Esc or your own mouse stops it.');
    expect([card.primary.label, card.secondary.label]).toEqual(['Allow', "Don't"]);
    await expect(yap.overlay.locator('.card').filter({ hasText: 'Let Yap use your apps?' })).toBeVisible();
    await shot(yap.overlay, '02-voice-access-card');
    // The spoken yes: the same conversation's next question.
    await yap.invoke('e2e_assistant_ask', { text: 'Yes', listenMs: 0 });
    await expect.poll(spoken, { timeout: 20_000 }).toContain('I wrote "See you soon" in your mail.');
    expect(await field(yap, 'Message')).toBe('See you soon');
    await expect.poll(barCard).toBeNull();
  } finally {
    await setConfig(yap, main, { llmScopes: cfg.llmScopes, speakReplies: cfg.speakReplies });
  }
});
