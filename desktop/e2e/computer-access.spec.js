// Computer use, phase 1: seeing and permission (desktop/src-tauri/src/computer.rs,
// computer/access.rs, integrations/claude_live/computer.rs). A plan chat's Claude Code asks for
// apps with mcp__yap-computer__request_access on Yap's `yap-computer` server; the person answers
// on the access card in the thread; then mcp__yap-computer__screenshot shows one display with
// every window of an app not allowed masked, its picture back as MCP image content and kept
// with the question.
//
// Claude Code is this build's stand-in (`yap.exe fake-claude`, ai/claude_code/fake.rs): `_mcp`
// with `server: "yap-computer"` calls Yap's endpoint as Claude Code would and writes what it got
// to a file this spec reads. The desktop is computer use's stand-in (`e2e_computer_scene`,
// computer/sim.rs) and the Start menu a stand-in list (YAP_E2E_APPS): nothing of the real screen,
// the person's apps or their Claude Code. Fictional apps and paths only.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, expectStore, openView, setConfig, jsonLines, openSettings, closeSettings } from './support/fixtures.js';
import { fakeClaude, planConfig, SCRIPTS as PLAN_SCRIPTS } from './support/claude-plan.js';
import { RUNS } from './support/yap.js';

const HERE = path.join(RUNS, 'computer-access-spec');
const SCRIPTS = path.join(HERE, 'scripts');
const SID = '{{session_id}}';
const MODEL = 'claude-haiku-5-5';
const SERVER = 'yap-computer';

// ---- the stand-in's script ----

const ev = (event) => ({ type: 'stream_event', event, parent_tool_use_id: null, session_id: SID });
const CHAT_INIT = {
  type: 'system',
  subtype: 'init',
  cwd: '{{cwd}}',
  session_id: SID,
  tools: ['WebFetch', 'WebSearch', 'mcp__yap-computer__request_access', 'mcp__yap-computer__screenshot'],
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
/** One of Yap's computer-use tools: its call, Yap's yap-computer server called with it, the answer its tool result. */
const computerCall = (msg, id, tool, input, dump) => [...calls(msg, id, `mcp__${SERVER}__${tool}`, input), { _mcp: { tool, arguments: input, result_for: id, dump, server: SERVER } }];
const answers = (msg, text) => [
  ev({ type: 'message_start', message: { id: msg, type: 'message', role: 'assistant', model: MODEL, content: [] } }),
  ev({ type: 'content_block_start', index: 0, content_block: { type: 'text', text: '' } }),
  ev({ type: 'content_block_delta', index: 0, delta: { type: 'text_delta', text } }),
  ev({ type: 'content_block_stop', index: 0 }),
  whole(msg, [{ type: 'text', text }], 'end_turn'),
  ev({ type: 'message_stop' }),
  result(text),
];
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
const imagesIn = (call) => call.content.filter((b) => b.type === 'image');
const resultOf = (call) => JSON.parse(call.content[0].text);

// ---- the app ----

const NOTEPAD_EXE = 'C:\\Program Files\\WindowsApps\\Microsoft.WindowsNotepad_11.2410.21.0_x64__8wekyb3d8bbwe\\Notepad\\Notepad.exe';
const START_MENU = [
  { id: 'Microsoft.WindowsNotepad_8wekyb3d8bbwe!App', name: 'Notepad' },
  { id: 'Microsoft.WindowsCalculator_8wekyb3d8bbwe!App', name: 'Calculator' },
  { id: 'C:\\Program Files\\Vaultkeeper\\KeePass.exe', name: 'KeePass', target: 'C:\\Program Files\\Vaultkeeper\\KeePass.exe' },
];
/** One 1280 × 720 display: Notepad under another app's window, and one of Yap's own. */
const SCENE = {
  displays: [{ x: 0, y: 0, w: 1280, h: 720, primary: true }],
  windows: [
    { x: 900, y: 400, w: 300, h: 250, own: true, class: 'Tauri Window' },
    { x: 600, y: 100, w: 400, h: 300, exe: 'C:\\Program Files\\Example Mail\\mail.exe', class: 'MailWindow' },
    { x: 100, y: 80, w: 700, h: 500, exe: NOTEPAD_EXE, aumid: 'microsoft.windowsnotepad_8wekyb3d8bbwe', class: 'Notepad' },
  ],
  cursor: [200, 200],
};

const test = base.extend({
  yapOptions: [
    async ({}, use) => {
      fs.mkdirSync(HERE, { recursive: true });
      const apps = path.join(HERE, 'start-menu.json');
      fs.writeFileSync(apps, JSON.stringify(START_MENU, null, 2));
      await use({
        name: 'computer-access',
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
const screenshotsOfLast = (main) => main.getByRole('group', { name: 'Screenshots' }).last().getByRole('button', { name: /^Screenshot,/ });
const play = (yap, name, chatTurns) => yap.invoke('e2e_claude_script', { script: script(name, chatTurns) });

async function send(main, text) {
  await main.getByRole('textbox', { name: 'Message' }).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
}

async function freshChat(main) {
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).first().click();
}

test.beforeEach(async ({ yap, main }) => {
  await setConfig(yap, main, { agentsEnabled: false, computerUse: true, assistantScreen: true, assistantScreenCloud: true });
  await yap.invoke('e2e_computer_scene', { scene: SCENE });
});

test('a plan chat asks to use apps on a card; allowed, its screenshot shows only their windows, kept with the question', async ({ yap, main, shot }) => {
  const DUMP = fresh('allowed.jsonl');
  const SAID = 'Your note in Notepad says to buy bread.';
  await play(yap, 'allowed', [
    turn(
      computerCall('m1', 'toolu_access', 'request_access', { apps: ['Notepad', 'KeePass', 'Paintbrush Deluxe'], reason: 'To read the note you have open.' }, DUMP),
      computerCall('m2', 'toolu_shot', 'screenshot', {}, DUMP),
      answers('m3', SAID)
    ),
  ]);
  await freshChat(main);
  await send(main, 'What does my note in Notepad say?');

  // The card: the app found, why, and the ones that can never be allowed (a password manager, one not in the Start menu).
  const card = accessCard(main);
  await expect(card).toHaveAttribute('data-state', 'asking', { timeout: 20_000 });
  await expect(card.getByTestId('chat-computer-apps')).toHaveText(/Notepad/);
  await expect(card.getByTestId('chat-computer-reason')).toHaveText('To read the note you have open.');
  await expect(card.getByTestId('chat-computer-denied')).toContainText('KeePass: Yap never looks at a password manager.');
  await expect(card.getByTestId('chat-computer-denied')).toContainText('Paintbrush Deluxe: Not in the Start menu');
  await shot(main, '01-the-access-card-asks');
  await card.getByRole('button', { name: 'Allow' }).click();

  // Allowed: the result names it; the screenshot comes back as image content, the other app and Yap masked.
  await expect(lastAnswer(main)).toHaveText(SAID, { timeout: 20_000 });
  await expect(card).toHaveAttribute('data-state', 'allowed');
  await expect(card).toContainText('Allowed in this chat: Notepad.');
  await expect.poll(() => jsonLines(DUMP).length).toBe(2);
  const [access, taken] = jsonLines(DUMP);
  expect(access).toMatchObject({ tool: 'request_access', isError: false });
  expect(resultOf(access).data.granted).toEqual([expect.objectContaining({ name: 'Notepad', tier: 'full' })]);
  expect(resultOf(access).data.denied.map((d) => d.name)).toEqual(['KeePass', 'Paintbrush Deluxe']);
  expect(taken).toMatchObject({ tool: 'screenshot', isError: false });
  const shotResult = resultOf(taken);
  expect(shotResult).toMatchObject({ success: true, data: { display: 'Display 1', frame: { w: 1280, h: 720 }, windows: [{ app: 'Notepad', x: 100, y: 80, w: 700, h: 500 }] } });
  expect(shotResult.data.masked.map((m) => m.what).sort()).toEqual(['Yap', 'not allowed']);
  expect(imagesIn(taken)).toHaveLength(1);
  expect(imagesIn(taken)[0].mimeType).toBe('image/png');
  // Shown with the question, as it was taken, and kept with it.
  await expect(screenshotsOfLast(main)).toHaveCount(1);
  await expectStore(yap, 'chats.json', (s) =>
    s.conversations.some((c) => c.messages.some((m) => m.role === 'user' && m.text === 'What does my note in Notepad say?' && m.images?.filter((i) => i.source === 'screen').length === 1))
  );
  // The card is kept with the answer, answered.
  await expectStore(yap, 'chats.json', (s) =>
    s.conversations.some((c) => c.messages.some((m) => m.role === 'assistant' && m.confirms?.some((k) => k.act === 'access' && k.state === 'allowed' && k.access?.apps?.[0]?.name === 'Notepad')))
  );
  await shot(main, '02-allowed-and-looked');
});

test("Don't on the card lets nothing in: the screenshot after it is refused", async ({ yap, main }) => {
  const DUMP = fresh('refused.jsonl');
  const SAID = "I can't look at Notepad without your OK.";
  await play(yap, 'refused', [turn(computerCall('m1', 'toolu_access', 'request_access', { apps: ['Calculator'], reason: 'To check the sum.' }, DUMP), computerCall('m2', 'toolu_shot', 'screenshot', {}, DUMP), answers('m3', SAID))]);
  await freshChat(main);
  await send(main, 'Is the sum on my calculator right?');
  const card = accessCard(main);
  await expect(card).toHaveAttribute('data-state', 'asking', { timeout: 20_000 });
  await card.getByRole('button', { name: "Don't" }).click();
  await expect(lastAnswer(main)).toHaveText(SAID, { timeout: 20_000 });
  await expect(card).toContainText('Not allowed');
  const [access, taken] = jsonLines(DUMP);
  expect(access).toMatchObject({ tool: 'request_access', isError: true });
  expect(resultOf(access).error).toContain("didn't allow");
  expect(taken).toMatchObject({ tool: 'screenshot', isError: true });
  expect(resultOf(taken).error).toContain('request_access first');
  expect(imagesIn(taken)).toEqual([]);
  await expect(screenshotsOfLast(main)).toHaveCount(0);
});

test('the Settings switch is off by default and needs both screen switches', async ({ yap, main, shot }) => {
  await setConfig(yap, main, { computerUse: false, assistantScreenCloud: false });
  const dialog = await openSettings(main, 'Voice Assistant');
  const toggle = dialog.getByRole('button', { name: 'Let Yap’s AI use apps on your PC' });
  const cloud = dialog.getByRole('button', { name: 'With cloud models too' });
  await toggle.scrollIntoViewIfNeeded();
  await expect(toggle).toHaveAttribute('aria-pressed', 'false');
  await expect(toggle).toBeDisabled();
  await cloud.click();
  await expect(toggle).toBeEnabled();
  await toggle.click();
  await expect(toggle).toHaveAttribute('aria-pressed', 'true');
  await expect.poll(async () => (await yap.invoke('get_config')).computerUse).toBe(true);
  await shot(main, '03-the-settings-switch');
  await closeSettings(main);
});
