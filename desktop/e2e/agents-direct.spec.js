// A Code chat that works directly (11 Nov 2026, desktop/src-tauri/src/agents/
// direct.rs), as Claude Code's own app does: on the person's Claude plan,
// what they type in a Code chat goes straight to the chat's Claude Code
// session in its workspace — their words its brief, each later message its
// next turn — with no model of Chat's in between. Before, Chat couldn't
// touch files: it worded every request into a brief for an agent ("On it:
// I've sent an agent…") and told what came of it second-hand.
//
// - The first message: a session with it as its brief, on the chat's own
//   model, at the chat's Mode; its words and steps show under the message
//   (views/chat/AgentWork.svelte), and its final words are the chat's answer
//   as they are (chat/turn/report.rs `own_words`). No "On it" bubble.
// - The next message: the same session's next turn, on `--resume`.
// - A message while it works: handed to it at once, the same turn.
// - A message with a screenshot (12 Nov 2026): straight to its session too,
//   the picture with its words, as Claude Code's own app sends a pasted
//   one — the first message, the next one (that turn's own picture only),
//   and a note while it works. Before, a screenshot made the message
//   Chat's own business: Chat's model read it and sent an agent.
// - Its words as it writes them (12 Nov 2026): the message being written is
//   the last paragraph of its work, growing; whole, it's the line that was
//   coming (or the chat's answer). Never a line of its log.
// - Claude Code's own question (AskUserQuestion, 12 Nov 2026) is a card in
//   the thread: its options as buttons (one question with one answer is
//   answered by the click), several questions sent together, the person's
//   own words, or "Let it decide". The stand-in asks the run's permission
//   prompt as Claude Code does (`_mcp approve`) and its answer is read back
//   from a file. Before, every question was refused at once.
// - Asked out loud with the Code chat in view (the Voice Assistant, 12 Nov
//   2026): straight to its session too. Yap says "On it." (its own words,
//   no model's), and the session's answer is read aloud when it's done.
//   Before, a spoken question was handed over by Chat's model.
// - "Work directly in Code chats" off: nothing goes to a session by itself
//   (Chat's model answers; plan-agents.spec.js covers its hand-off).
//
// Chat is on the plan with Yap's stand-in for Claude Code (claude-plan.spec.js'
// way: `yap.exe fake-claude <script>`, switched per turn with
// `e2e_claude_script`); the workspace is in this run's own folder. No one's
// real Claude Code, account or files.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, setConfig } from './support/fixtures.js';
import { fakeClaude, planConfig } from './support/claude-plan.js';
import { RUNS } from './support/yap.js';
import { newCodeChat } from './support/code-chats.js';
import { writePng } from './support/png.js';
import { workingDirectly, turnEnd, answeredAfter } from '../src/lib/core/agents.js';
import { sayingOver, questionAnswers, questionsAnswered } from '../src/lib/core/agentNow.js';

const HERE = path.join(RUNS, 'agents-direct-spec');
const SCRIPTS = path.join(HERE, 'scripts');
const GARDEN = path.join(HERE, 'folders', 'garden-planner');
const ARGS = path.join(HERE, 'args.jsonl');
const ANSWERS = path.join(HERE, 'answers.jsonl');

const DONE1 = 'Done. notes.md has a watering schedule now.';
const DONE2 = 'Done. There is a minimap button for the beds.';
const NOTES0 = 'Draft notes\n';
const NOTES1 = 'Draft notes\nWatering: twice a week\n';
const MINIMAP = 'export const minimap = true;\n';

// ---- the stand-in's scripts ----

const init = () => ({
  type: 'system',
  subtype: 'init',
  cwd: '{{cwd}}',
  session_id: '{{session_id}}',
  tools: ['Bash', 'Edit', 'Read', 'Write'],
  mcp_servers: [{ name: 'yap', status: 'connected' }],
  model: 'claude-haiku-5-5',
  permissionMode: 'auto',
  apiKeySource: 'none',
  claude_code_version: '2.1.296',
  capabilities: ['interrupt_receipt_v1'],
});
const said = (id, content, stop = 'tool_use') => ({
  type: 'assistant',
  message: { id, type: 'message', role: 'assistant', model: 'claude-haiku-5-5', content, stop_reason: stop },
  parent_tool_use_id: null,
  session_id: '{{session_id}}',
});
const toolResult = (id, text) => ({ type: 'user', message: { role: 'user', content: [{ type: 'tool_result', tool_use_id: id, content: text }] }, parent_tool_use_id: null, session_id: '{{session_id}}' });
const result = (text, cost) => ({
  type: 'result',
  subtype: 'success',
  is_error: false,
  duration_ms: 900,
  num_turns: 2,
  result: text,
  session_id: '{{session_id}}',
  total_cost_usd: cost,
  usage: { input_tokens: 4000, output_tokens: 120, cache_creation_input_tokens: 0, cache_read_input_tokens: 0 },
  permission_denials: [],
  terminal_reason: 'completed',
});
/** Its first turn: says what it will do, edits notes.md (backed up first), says it's done. */
const WATERING = [
  { _wait_user: true },
  { _dump_args: ARGS },
  init(),
  said('w1', [{ type: 'text', text: "I'll add the watering schedule to notes.md." }]),
  said('w1a', [{ type: 'tool_use', id: 'toolu_w', name: 'Edit', input: { file_path: '{{cwd}}\\notes.md', old_string: 'Draft notes', new_string: 'Draft notes\nWatering: twice a week' } }]),
  { _hook: { event: 'PreToolUse', input: { tool_name: 'Edit', tool_use_id: 'toolu_w', tool_input: { file_path: 'notes.md', old_string: 'Draft notes', new_string: 'Draft notes\nWatering: twice a week' } } } },
  { _write_file: { path: 'notes.md', text: NOTES1 } },
  toolResult('toolu_w', 'The file notes.md has been updated.'),
  said('w2', [{ type: 'text', text: DONE1 }], 'end_turn'),
  result(DONE1, 0.03),
  { _wait_user: true },
];
/** Its next: a new minimap.js. */
const MINIMAP_TURN = [
  { _wait_user: true },
  { _dump_args: ARGS },
  init(),
  said('m1', [{ type: 'tool_use', id: 'toolu_map', name: 'Write', input: { file_path: '{{cwd}}\\minimap.js', content: MINIMAP } }]),
  { _hook: { event: 'PreToolUse', input: { tool_name: 'Write', tool_use_id: 'toolu_map', tool_input: { file_path: 'minimap.js' } } } },
  { _write_file: { path: 'minimap.js', text: MINIMAP } },
  toolResult('toolu_map', 'File created successfully at: minimap.js'),
  said('m2', [{ type: 'text', text: DONE2 }], 'end_turn'),
  result(DONE2, 0.02),
  { _wait_user: true },
];
/** A turn that takes its time: a message that comes meanwhile is with it at its tool call's end (`{{notes}}`). */
const SLOWISH = [
  { _wait_user: true },
  { _dump_args: ARGS },
  init(),
  said('s1', [{ type: 'tool_use', id: 'toolu_read', name: 'Read', input: { file_path: '{{cwd}}\\notes.md' } }]),
  { _sleep_ms: 6000 },
  toolResult('toolu_read', 'Draft notes'),
  result('Read notes.md. Noted: {{notes}}', 0.01),
  { _wait_user: true },
];

/** Says how many pictures its message came with (the stand-in's `{{user_pictures}}`). */
const SEES = [{ _wait_user: true }, { _dump_args: ARGS }, init(), said('p1', [{ type: 'text', text: 'Pictures with it: {{user_pictures}}.' }], 'end_turn'), result('Pictures with it: {{user_pictures}}.', 0.01), { _wait_user: true }];
/** A turn that takes its time, then says how many pictures its messages came with: its first, and a note's meanwhile. */
const SEES_LATER = [
  { _wait_user: true },
  init(),
  said('l1', [{ type: 'tool_use', id: 'toolu_read', name: 'Read', input: { file_path: '{{cwd}}\\notes.md' } }]),
  { _sleep_ms: 6000 },
  toolResult('toolu_read', 'Draft notes'),
  { _dump_args: ARGS },
  result('Pictures with it: {{user_pictures}}. Noted: {{notes}}', 0.01),
  { _wait_user: true },
];

/** A piece of Claude Code's stream (`--include-partial-messages`). */
const piece = (event) => ({ type: 'stream_event', event, parent_tool_use_id: null, session_id: '{{session_id}}' });
const starts = (id) => piece({ type: 'message_start', message: { id, type: 'message', role: 'assistant', model: 'claude-haiku-5-5', content: [] } });
const writes = (text) => piece({ type: 'content_block_delta', index: 0, delta: { type: 'text_delta', text } });
const FIRST_WORDS = "I'll read the notes first, then add the schedule.";
const LAST_WORDS = 'Done: nothing needed changing.';
/** A turn whose words come a piece at a time: a message before a step, then its final words. */
const WRITES_AS_IT_GOES = [
  { _wait_user: true },
  init(),
  starts('msg_1'),
  piece({ type: 'content_block_start', index: 0, content_block: { type: 'text', text: '' } }),
  writes("I'll read the notes "),
  { _sleep_ms: 300 },
  writes('first, then'),
  { _sleep_ms: 4000 },
  writes(' add the schedule.'),
  piece({ type: 'content_block_stop', index: 0 }),
  said('w1', [{ type: 'text', text: FIRST_WORDS }]),
  said('w1a', [{ type: 'tool_use', id: 'toolu_read', name: 'Read', input: { file_path: '{{cwd}}\\notes.md' } }]),
  { _sleep_ms: 1200 },
  toolResult('toolu_read', 'Draft notes'),
  starts('msg_2'),
  writes('Done: nothing'),
  { _sleep_ms: 4000 },
  writes(' needed changing.'),
  said('w2', [{ type: 'text', text: LAST_WORDS }], 'end_turn'),
  result(LAST_WORDS, 0.01),
  { _wait_user: true },
];

const ONE_QUESTION = [{ question: 'Which bed should get the watering line first?', header: 'Bed', options: [{ label: 'North bed', description: 'The tomatoes' }, { label: 'South bed', description: 'The herbs' }], multiSelect: false }];
const TWO_QUESTIONS = [
  { question: 'How often should it say to water?', header: 'How often', options: [{ label: 'Twice a week', description: '' }, { label: 'Every day', description: 'In hot weather' }], multiSelect: false },
  { question: 'Which sections should the notes have?', header: 'Sections', options: [{ label: 'Watering', description: '' }, { label: 'Feeding', description: '' }, { label: 'Pests', description: '' }], multiSelect: true },
];
/** A turn that asks the person (Claude Code's AskUserQuestion, through the run's permission prompt as Claude Code sends it), then carries on. */
const asks = (questions) => [
  { _wait_user: true },
  init(),
  said('q1', [{ type: 'tool_use', id: 'toolu_q', name: 'AskUserQuestion', input: { questions } }]),
  { _mcp: { tool: 'approve', arguments: { tool_name: 'AskUserQuestion', input: { questions }, tool_use_id: 'toolu_q' }, result_for: 'toolu_q', dump: ANSWERS } },
  said('q2', [{ type: 'text', text: 'Thanks: carrying on.' }], 'end_turn'),
  result('Thanks: carrying on.', 0.01),
  { _wait_user: true },
];
/** What Claude Code was told for each question so far (the permission prompt's own answers). */
const decisions = () => (fs.existsSync(ANSWERS) ? fs.readFileSync(ANSWERS, 'utf8').split('\n').filter(Boolean).map((l) => JSON.parse(JSON.parse(l).content[0].text)) : []);

function writeScript(name, lines) {
  fs.mkdirSync(SCRIPTS, { recursive: true });
  const file = path.join(SCRIPTS, `${name}.jsonl`);
  fs.writeFileSync(file, lines.map((l) => JSON.stringify(l)).join('\n') + '\n');
  return file;
}
function freshFolder(dir) {
  fs.rmSync(dir, { recursive: true, force: true });
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(path.join(dir, 'notes.md'), NOTES0);
}
const argsSeen = () => (fs.existsSync(ARGS) ? fs.readFileSync(ARGS, 'utf8').split('\n').filter(Boolean).map((l) => JSON.parse(l).args) : []);
/** The pictures each turn's messages came with, as the stand-in saw them (each its type and size, never its data). */
const picturesSeen = () => (fs.existsSync(ARGS) ? fs.readFileSync(ARGS, 'utf8').split('\n').filter(Boolean).map((l) => JSON.parse(l).user_pictures) : []);
const after = (args, flag) => {
  const i = args.indexOf(flag);
  return i >= 0 ? args[i + 1] : undefined;
};

// ---- the app ----

const test = base.extend({
  yapOptions: [
    async ({}, use) => {
      await use({
        name: 'agents-direct',
        // (Each test loads the script it needs: e2e_claude_script.)
        env: fakeClaude(writeScript('idle', [{ _wait_user: true }])),
        config: planConfig({ 'claude-plan': 'claude-plan' }, { memoryLearnChats: false }),
      });
    },
    { scope: 'worker' },
  ],
});

const thread = (main) => main.locator('.msgs');
const agents = (yap) => yap.invoke('agent_list');
/** A question asked out loud (the Voice Assistant's shortcut, its words given). */
const askAloud = (yap, text) => yap.invoke('e2e_assistant_ask', { text, listenMs: 0 });
/** What Yap has said out loud so far. */
const spoken = async (yap) => (await yap.invoke('tts_e2e_spoken')).map((s) => s.text);
const script = (yap, lines, name) => yap.invoke('e2e_claude_script', { script: writeScript(name, lines) });
const tab = (main) => main.getByTestId('chat-agent');

async function send(main, text) {
  await main.getByRole('textbox', { name: 'Message' }).fill(text);
  await main.getByRole('button', { name: 'Send', exact: true }).click();
}
const attachments = (main) => main.getByTestId('chat-attachments');
/** A picture file dropped on Chat: it waits in the box, as a snip does (`count` of them there now). */
async function dropPicture(yap, main, name, colour, count = 1) {
  const dir = path.join(HERE, 'to-drop');
  fs.mkdirSync(dir, { recursive: true });
  const file = path.join(dir, name);
  fs.writeFileSync(file, writePng(96, 60, colour));
  await yap.invoke('e2e_drag_drop', { paths: [file] });
  await expect(attachments(main).locator('.shotchip')).toHaveCount(count);
}
async function codeChat(main) {
  await openView(main, 'Chat');
  if (await tab(main).count()) await tab(main).getByRole('button', { name: 'Close the panel' }).click();
  await newCodeChat(main, 'garden-planner');
}
async function ended(yap, id, timeout = 30_000) {
  await expect.poll(async () => (await agents(yap)).find((j) => j.id === id)?.status, { timeout, message: `agent ${id} to end` }).toMatch(/^(done|failed|stopped)$/);
  return (await agents(yap)).find((j) => j.id === id);
}
async function newAgent(yap, before) {
  await expect.poll(async () => (await agents(yap)).length, { timeout: 20_000, message: 'a session to start' }).toBe(before + 1);
  return (await agents(yap))[0];
}
async function messagesOf(yap, id, n) {
  await expect.poll(async () => (await yap.invoke('chat_get', { id })).messages.length, { timeout: 30_000, message: `${n} messages` }).toBe(n);
  return (await yap.invoke('chat_get', { id })).messages;
}
/** What the thread shows, top to bottom: the person's messages, a session's work, its report line, an answer. */
const laidOut = (main) =>
  thread(main)
    .locator('.cbubble.user, .cbubble.assistant, [data-testid="chat-agent-work"], [data-testid="chat-agent-report"]')
    .evaluateAll((els) => els.map((e) => (e.dataset.testid === 'chat-agent-work' ? 'work' : e.dataset.testid === 'chat-agent-report' ? 'report' : e.classList.contains('user') ? 'you' : 'answer')));

test.beforeEach(async ({ yap, main }) => {
  freshFolder(GARDEN);
  fs.rmSync(ARGS, { force: true });
  await yap.invoke('e2e_claude_approve_auto', { allow: true });
  await setConfig(yap, main, { agentsEnabled: true, agentsDirect: true, agentsAsk: 'new-or-web', agentsSpeak: 'never', agentsMode: 'auto', webSearch: false, speakReplies: false });
  if (!(await yap.invoke('workspaces_list')).some((f) => f.name === 'garden-planner')) await yap.invoke('workspaces_add', { path: GARDEN });
});

test('a message typed in a Code chat on the plan goes straight to its session, and its own words are the answer', async ({ yap, main, shot }) => {
  await script(yap, WATERING, 'watering');
  const before = (await agents(yap)).length;
  await codeChat(main);
  await send(main, 'Add a watering schedule to notes.md.');
  // Its session: their words its brief, on the chat's own model, marked as theirs.
  const agent = await newAgent(yap, before);
  expect(agent).toMatchObject({ folder: 'garden-planner', brief: 'Add a watering schedule to notes.md.', model: 'claude-haiku-5-5', mode: 'auto' });
  expect(agent.turnList[0]).toMatchObject({ n: 1, direct: true, request: 'Add a watering schedule to notes.md.', title: 'Add a watering schedule to notes.md' });
  const done = await ended(yap, agent.id);
  expect(fs.readFileSync(path.join(GARDEN, 'notes.md'), 'utf8')).toBe(NOTES1);
  const args = argsSeen();
  expect(args).toHaveLength(1);
  expect(after(args[0], '--model')).toBe('claude-haiku-5-5');
  expect(after(args[0], '--permission-mode')).toBe('auto');
  expect(args[0]).toContain('--replay-user-messages');
  // The chat: their message, its report line, and its own final words — no
  // "On it" of Chat's, and no second-hand telling.
  const messages = await messagesOf(yap, done.conversation, 3);
  expect(messages.map((m) => m.kind || m.role)).toEqual(['user', 'agentReport', 'assistant']);
  expect(messages[2].text).toBe(DONE1);
  expect(messages[2].model).toMatchObject({ provider: 'claude-plan' });
  // The thread: the message, its session's work under it, then the answer.
  await expect(thread(main).locator('.cbubble.assistant')).toHaveText([DONE1]);
  // (Its work is drawn once its log is read: a moment after the thread.)
  await expect.poll(() => laidOut(main)).toEqual(['you', 'work', 'report', 'answer']);
  const work = thread(main).getByTestId('chat-agent-work');
  await expect(work.getByTestId('chat-agent-work-said')).toHaveText(["I'll add the watering schedule to notes.md."]);
  await expect(work.getByTestId('chat-agent-work-steps').locator('.wtext')).toHaveText([/^Edited notes\.md\s*\+1$/]);
  await expect(thread(main).getByTestId('chat-agent-report').locator('.jtitle')).toHaveText('Agent done · garden-planner');
  // Its tab opened beside the chat as it started.
  await expect(tab(main)).toBeVisible();
  await shot(main, '01-a-code-chat-works-directly');

  // The next message: the same session's next turn, on --resume.
  await script(yap, MINIMAP_TURN, 'minimap');
  await send(main, 'could you add a minimap button for it as well?');
  await expect.poll(async () => (await agents(yap)).find((j) => j.id === agent.id).turnList.length, { timeout: 20_000 }).toBe(2);
  const again = await (async () => {
    await expect.poll(async () => (await agents(yap)).find((j) => j.id === agent.id).turnList[1].status, { timeout: 30_000 }).toBe('done');
    return ended(yap, agent.id);
  })();
  expect((await agents(yap)).length).toBe(before + 1);
  expect(again.turnList[1]).toMatchObject({ n: 2, direct: true, request: 'could you add a minimap button for it as well?' });
  expect(after(argsSeen()[1], '--resume')).toBe(again.session);
  expect(after(argsSeen()[1], '--model')).toBe('claude-haiku-5-5');
  const then = await messagesOf(yap, done.conversation, 6);
  expect(then.map((m) => m.kind || m.role)).toEqual(['user', 'agentReport', 'assistant', 'user', 'agentReport', 'assistant']);
  expect(then[5].text).toBe(DONE2);
  await expect(thread(main).locator('.cbubble.assistant')).toHaveText([DONE1, DONE2]);
  await expect.poll(() => laidOut(main)).toEqual(['you', 'work', 'report', 'answer', 'you', 'work', 'report', 'answer']);
  await shot(main, '02-its-next-message-its-next-turn');
});

test('a message while it works is handed to it at once: the same turn, one answer', async ({ yap, main }) => {
  await script(yap, SLOWISH, 'slowish');
  const before = (await agents(yap)).length;
  await codeChat(main);
  await send(main, 'Read notes.md.');
  const agent = await newAgent(yap, before);
  await expect.poll(async () => (await agents(yap)).find((j) => j.id === agent.id).status, { timeout: 20_000 }).toBe('running');
  await send(main, 'Keep the headings as they are.');
  // With it now: the thread says so, live, under the first message.
  await expect(thread(main).getByTestId('chat-agent-work').locator('[data-busy="true"] .wtext')).toHaveText('Your note is with the agent: it reads it at its next step');
  const done = await ended(yap, agent.id, 40_000);
  expect(done.turnList.map((t) => [t.n, t.status, !!t.direct])).toEqual([[1, 'done', true]]);
  expect(done.turnList[0].request).toBe('Read notes.md.\n\nKeep the headings as they are.');
  expect(argsSeen()).toHaveLength(1);
  // Both messages, one report line, one answer in its own words.
  const messages = await messagesOf(yap, done.conversation, 4);
  expect(messages.map((m) => m.kind || m.role)).toEqual(['user', 'user', 'agentReport', 'assistant']);
  expect(messages[3].text).toContain('Read notes.md. Noted: Keep the headings as they are.');
  await expect.poll(() => laidOut(main)).toEqual(['you', 'work', 'you', 'report', 'answer']);
});

test('a message with a screenshot goes straight to its session too: the picture with its words, each turn its own', async ({ yap, main, shot }) => {
  await script(yap, SEES, 'sees');
  const before = (await agents(yap)).length;
  await codeChat(main);
  await dropPicture(yap, main, 'beds.png', [40, 120, 200]);
  await send(main, 'Make the beds look like this.');
  // Its session, their words its brief: no model of Chat's read the picture and worded one.
  const agent = await newAgent(yap, before);
  expect(agent).toMatchObject({ folder: 'garden-planner', brief: 'Make the beds look like this.' });
  expect(agent.turnList[0]).toMatchObject({ n: 1, direct: true, request: 'Make the beds look like this.' });
  expect(agent.turnList[0].shots).toHaveLength(1);
  const done = await ended(yap, agent.id);
  // Claude Code got the picture itself, with the words.
  expect(picturesSeen()).toHaveLength(1);
  expect(picturesSeen()[0]).toHaveLength(1);
  expect(picturesSeen()[0][0].type).toBe('image/png');
  expect(picturesSeen()[0][0].bytes).toBeGreaterThan(50);
  const messages = await messagesOf(yap, done.conversation, 3);
  expect(messages.map((m) => m.kind || m.role)).toEqual(['user', 'agentReport', 'assistant']);
  expect(messages[0].images.map((i) => i.id)).toEqual(agent.turnList[0].shots);
  expect(messages[2].text).toBe('Pictures with it: 1.');
  // The thread: their message with its screenshot, its session's own answer.
  await expect(thread(main).locator('.cbubble.user img')).toHaveCount(1);
  await expect(thread(main).locator('.cbubble.assistant')).toHaveText(['Pictures with it: 1.']);
  await expect(attachments(main)).toBeHidden();
  await shot(main, '10-a-screenshot-goes-straight-to-its-session');

  // The next message, with two: its next turn on --resume, with its own pictures — never the first turn's again.
  await dropPicture(yap, main, 'legend.png', [200, 120, 40]);
  await dropPicture(yap, main, 'path.png', [90, 160, 90], 2);
  await send(main, 'And the legend and the path like these.');
  await expect.poll(async () => (await agents(yap)).find((j) => j.id === agent.id).turnList.length, { timeout: 20_000 }).toBe(2);
  await expect.poll(async () => (await agents(yap)).find((j) => j.id === agent.id).turnList[1].status, { timeout: 30_000 }).toBe('done');
  const again = await ended(yap, agent.id);
  expect((await agents(yap)).length).toBe(before + 1);
  expect(again.turnList[1]).toMatchObject({ n: 2, direct: true, request: 'And the legend and the path like these.' });
  expect(again.turnList[1].shots).toHaveLength(2);
  expect(after(argsSeen()[1], '--resume')).toBe(again.session);
  expect(picturesSeen()[1].map((p) => p.type)).toEqual(['image/png', 'image/png']);
  const then = await messagesOf(yap, done.conversation, 6);
  expect(then[5].text).toBe('Pictures with it: 2.');

  await expect(thread(main).locator('.cbubble.assistant')).toHaveText(['Pictures with it: 1.', 'Pictures with it: 2.']);
});

test('a note with a screenshot while it works is handed to it at once, the picture with it', async ({ yap, main }) => {
  await script(yap, SEES_LATER, 'sees-later');
  const before = (await agents(yap)).length;
  await codeChat(main);
  await send(main, 'Read notes.md.');
  const agent = await newAgent(yap, before);
  await expect.poll(async () => (await agents(yap)).find((j) => j.id === agent.id).status, { timeout: 20_000 }).toBe('running');
  await dropPicture(yap, main, 'headings.png', [120, 40, 160]);
  await send(main, 'Keep the headings like this.');
  const done = await ended(yap, agent.id, 40_000);
  // The same turn, one process; the note's picture reached it with the note.
  expect(done.turnList.map((t) => [t.n, t.status, !!t.direct])).toEqual([[1, 'done', true]]);
  expect(picturesSeen()).toHaveLength(1);
  expect(picturesSeen()[0].map((p) => p.type)).toEqual(['image/png']);
  const messages = await messagesOf(yap, done.conversation, 4);
  expect(messages.map((m) => m.kind || m.role)).toEqual(['user', 'user', 'agentReport', 'assistant']);
  expect(messages[1].images).toHaveLength(1);
  expect(messages[3].text).toContain('Pictures with it: 1. Noted: Keep the headings like this.');
});

test('its words show as it writes them: the message so far, then the whole line; its final words become the answer', async ({ yap, main, shot }) => {
  await script(yap, WRITES_AS_IT_GOES, 'writes');
  const before = (await agents(yap)).length;
  await codeChat(main);
  await send(main, 'Add a watering schedule to notes.md.');
  const agent = await newAgent(yap, before);
  const work = thread(main).getByTestId('chat-agent-work');
  const writing = work.getByTestId('chat-agent-work-saying');
  // The message it's writing, so far: under their message before any step, no line of steps yet.
  await expect(writing).toHaveText("I'll read the notes first, then", { timeout: 20_000 });
  await expect(work.getByTestId('chat-agent-work-said')).toHaveCount(0);
  // (Its start-up line was only what it was on until it wrote.)
  await expect(work.getByTestId('chat-agent-work-steps')).toHaveCount(0);
  await shot(main, '30-its-words-as-it-writes-them');
  // Whole: the line that was coming, and what was being written is let go.
  await expect(work.getByTestId('chat-agent-work-said')).toHaveText([FIRST_WORDS], { timeout: 20_000 });
  await expect(writing).toHaveCount(0);
  // Its final words come the same way, after its step…
  await expect(writing).toHaveText('Done: nothing', { timeout: 20_000 });
  await expect(work.getByTestId('chat-agent-work-steps').locator('.wtext')).toHaveText(['Read notes.md']);
  await expect(work.locator('[data-busy="true"]')).toHaveCount(0);
  // …and are the chat's answer once it's done: never left behind in its work.
  const done = await ended(yap, agent.id, 40_000);
  await expect(thread(main).locator('.cbubble.assistant')).toHaveText([LAST_WORDS]);
  await expect(writing).toHaveCount(0);
  await expect(work.getByTestId('chat-agent-work-said')).toHaveText([FIRST_WORDS]);
  // Nothing half-written is a line of its log: its words are there whole, once.
  const log = (await yap.invoke('agent_get', { id: done.id })).log;
  expect(log.filter((e) => e.said).map((e) => e.said)).toEqual([FIRST_WORDS, LAST_WORDS]);
  expect(JSON.stringify(log)).not.toContain('first, then"');
  expect(log.some((e) => 'saying' in e)).toBe(false);
});

test('Claude Code’s own question is a card: an option answers it, several are sent together, or it is left to decide', async ({ yap, main, shot }) => {
  fs.rmSync(ANSWERS, { force: true });
  // (Asked as in the app: nothing answers for the person.)
  await yap.invoke('e2e_claude_approve_auto', { allow: null });
  await script(yap, asks(ONE_QUESTION), 'asks-one');
  const before = (await agents(yap)).length;
  await codeChat(main);
  await send(main, 'Add a watering line.');
  const agent = await newAgent(yap, before);
  const statusNow = async () => (await agents(yap)).find((j) => j.id === agent.id).status;
  const card = thread(main).locator('[data-testid="chat-agent-approve"][data-kind="question"]');
  await expect(card).toBeVisible({ timeout: 20_000 });
  await expect(card).toContainText('The agent has a question');
  await expect(card.getByTestId('chat-agent-question')).toHaveCount(1);
  await expect(card.getByTestId('chat-agent-question').locator('.qtext')).toContainText('Which bed should get the watering line first?');
  await expect(card.getByTestId('chat-agent-question').locator('.qhead')).toHaveText('Bed');
  await expect(card.getByTestId('chat-agent-question-option').locator('.qlabel')).toHaveText(['North bed', 'South bed']);
  await expect(card.getByTestId('chat-agent-question-option').locator('.qdesc')).toHaveText(['The tomatoes', 'The herbs']);
  // One question, one answer: no Send; it waits on the person, and says what for.
  await expect(card.getByRole('button', { name: 'Send', exact: true })).toHaveCount(0);
  await expect.poll(statusNow, { timeout: 20_000 }).toBe('asking');
  await expect(thread(main).getByTestId('chat-agent-work').locator('[data-busy="true"] .wtext')).toHaveText('Waiting for you: it has a question');
  await shot(main, '40-a-question-from-claude-code');
  // The click on an option is the answer, under the question's own words.
  await card.getByRole('button', { name: /South bed/ }).click();
  await expect(card).toHaveCount(0);
  await expect.poll(() => decisions().length, { timeout: 20_000 }).toBe(1);
  expect(decisions()[0]).toEqual({ behavior: 'allow', updatedInput: { questions: ONE_QUESTION, answers: { [ONE_QUESTION[0].question]: 'South bed' } } });
  await ended(yap, agent.id);
  await expect(thread(main).locator('.cbubble.assistant')).toHaveText(['Thanks: carrying on.']);

  // Several questions: each answered, then sent together — one option, several options, or their own words.
  await script(yap, asks(TWO_QUESTIONS), 'asks-two');
  await send(main, 'And tidy the notes.');
  await expect(card).toBeVisible({ timeout: 20_000 });
  const sets = card.getByTestId('chat-agent-question');
  const sendAnswers = card.getByRole('button', { name: 'Send', exact: true });
  await expect(sets).toHaveCount(2);
  await expect(sets.nth(1)).toContainText('Pick any that fit.');
  await expect(sendAnswers).toBeDisabled();
  await sets.nth(0).getByRole('button', { name: /Every day/ }).click();
  await expect(sets.nth(0).getByRole('button', { name: /Every day/ })).toHaveAttribute('aria-pressed', 'true');
  await expect(sendAnswers).toBeDisabled();
  await sets.nth(1).getByRole('button', { name: 'Watering', exact: true }).click();
  await sets.nth(1).getByRole('button', { name: 'Feeding', exact: true }).click();
  await sets.nth(1).getByRole('button', { name: 'Pests', exact: true }).click();
  // (Picked again: put back.)
  await sets.nth(1).getByRole('button', { name: 'Feeding', exact: true }).click();
  await expect(sets.nth(1).getByRole('button', { name: 'Feeding', exact: true })).toHaveAttribute('aria-pressed', 'false');
  await expect(sendAnswers).toBeEnabled();
  // Their own words for the first: what they meant, whatever was picked.
  await sets.nth(0).getByRole('button', { name: 'Something else…' }).click();
  await sets.nth(0).getByRole('textbox', { name: 'Your own answer' }).fill('Every other day');
  await expect(sets.nth(0).getByRole('button', { name: /Every day/ })).toHaveAttribute('aria-pressed', 'false');
  await shot(main, '41-several-questions-answered-together');
  await sendAnswers.click();
  await expect(card).toHaveCount(0);
  await expect.poll(() => decisions().length, { timeout: 20_000 }).toBe(2);
  expect(decisions()[1].behavior).toBe('allow');
  expect(decisions()[1].updatedInput.questions).toEqual(TWO_QUESTIONS);
  expect(decisions()[1].updatedInput.answers).toEqual({ [TWO_QUESTIONS[0].question]: 'Every other day', [TWO_QUESTIONS[1].question]: 'Watering, Pests' });
  await expect.poll(async () => (await agents(yap)).find((j) => j.id === agent.id).turnList[1]?.status, { timeout: 30_000 }).toBe('done');

  // "Let it decide": Claude chooses for itself, and is told to say what it chose.
  await script(yap, asks(ONE_QUESTION), 'asks-again');
  await send(main, 'One more thing.');
  await expect(card).toBeVisible({ timeout: 20_000 });
  await card.getByRole('button', { name: 'Let it decide' }).click();
  await expect(card).toHaveCount(0);
  await expect.poll(() => decisions().length, { timeout: 20_000 }).toBe(3);
  expect(decisions()[2].behavior).toBe('deny');
  expect(decisions()[2].message).toContain('The user left this one to you in Yap.');
  await expect.poll(async () => (await agents(yap)).find((j) => j.id === agent.id).turnList[2]?.status, { timeout: 30_000 }).toBe('done');
  // Its work says it asked: a step like any other.
  const log = (await yap.invoke('agent_get', { id: agent.id })).log;
  expect(log.filter((e) => e.step === 'Asking you a question')).toHaveLength(3);
  expect(log.some((e) => e.step === 'Waiting for you: it has a question')).toBe(true);
});

test('asked out loud with the Code chat in view: straight to its session, “On it.” said, its own answer read aloud', async ({ yap, main }) => {
  await script(yap, WATERING, 'watering');
  await setConfig(yap, main, { speakReplies: true, agentsSpeak: 'voice' });
  await yap.invoke('tts_e2e_spoken', { clear: true });
  const before = (await agents(yap)).length;
  await codeChat(main);
  // A typed message first, so the Code chat exists and is the one in view.
  await send(main, 'Add a watering schedule to notes.md.');
  const agent = await newAgent(yap, before);
  const first = await ended(yap, agent.id);
  await messagesOf(yap, first.conversation, 3);
  // Typed: its answer isn't read aloud ("Read agent reports aloud": when asked by voice).
  expect(await spoken(yap)).toEqual([]);

  // Asked out loud, the chat in view: its session's next turn — no model of Chat's, and Yap's own "On it.".
  await script(yap, MINIMAP_TURN, 'minimap');
  await askAloud(yap, 'Could you add a minimap button for it as well?');
  await expect.poll(async () => (await agents(yap)).find((j) => j.id === agent.id).turnList.length, { timeout: 20_000 }).toBe(2);
  await expect.poll(() => spoken(yap), { timeout: 20_000 }).toEqual(expect.arrayContaining(['On it.']));
  expect((await agents(yap)).length).toBe(before + 1);
  await expect.poll(async () => (await agents(yap)).find((j) => j.id === agent.id).turnList[1].status, { timeout: 30_000 }).toBe('done');
  const again = (await agents(yap)).find((j) => j.id === agent.id);
  expect(again.turnList[1]).toMatchObject({ n: 2, direct: true });
  expect(again.turnList[1].request.toLowerCase()).toContain('minimap button');
  expect(after(argsSeen()[1], '--resume')).toBe(again.session);
  // Its own words are the chat's answer, and — this turn was asked out loud — read aloud.
  const then = await messagesOf(yap, first.conversation, 6);
  expect(then.map((m) => m.kind || m.role)).toEqual(['user', 'agentReport', 'assistant', 'user', 'agentReport', 'assistant']);
  expect(then[3].voice).toBe(true);
  expect(then[5].text).toBe(DONE2);
  await expect.poll(() => spoken(yap), { timeout: 30_000 }).toEqual(expect.arrayContaining([DONE2]));
  // The thread: the spoken question, its session's work under it, the answer.
  await expect(thread(main).locator('.cbubble.assistant')).toHaveText([DONE1, DONE2]);
  await expect.poll(() => laidOut(main)).toEqual(['you', 'work', 'report', 'answer', 'you', 'work', 'report', 'answer']);
});

test('core/agents.js: the Code chat’s own session at work is what the message box’s ■ stops', () => {
  // A question card's answers: each question's own words → the one label, the list picked, or the person's own words.
  const qs = [
    { question: 'How often?', options: [{ label: 'Twice a week' }, { label: 'Every day' }], multiSelect: false },
    { question: 'Which sections?', options: [{ label: 'Watering' }, { label: 'Pests' }], multiSelect: true },
  ];
  expect(questionAnswers(qs, {})).toEqual({});
  expect(questionsAnswered(qs, {})).toBe(false);
  expect(questionAnswers(qs, { 0: { labels: ['Every day'] } })).toEqual({ 'How often?': 'Every day' });
  expect(questionsAnswered(qs, { 0: { labels: ['Every day'] } })).toBe(false);
  expect(questionAnswers(qs, { 0: { labels: ['Every day'], other: ' Every other day ' }, 1: { labels: ['Watering', 'Pests'] } })).toEqual({ 'How often?': 'Every other day', 'Which sections?': ['Watering', 'Pests'] });
  // Where several may be picked, their own words join the picks.
  expect(questionAnswers(qs, { 1: { labels: ['Pests'], other: 'Pruning' } })).toEqual({ 'Which sections?': ['Pests', 'Pruning'] });
  expect(questionAnswers(qs, { 1: { other: 'Pruning' } })).toEqual({ 'Which sections?': ['Pruning'] });
  expect(questionsAnswered(qs, { 0: { other: 'Daily' }, 1: { labels: ['Pests'] } })).toBe(true);
  // Never a label the question doesn't have; blank words are no answer.
  expect(questionAnswers(qs, { 0: { labels: ['Hourly'], other: '   ' } })).toEqual({});
  expect(questionsAnswered([], {})).toBe(false);

  const own = (status, conversation = 4) => ({ id: 'a1', conversation, status, turnList: [{ n: 1, direct: true }] });
  for (const status of ['queued', 'running', 'asking']) expect(workingDirectly([own(status)], 4)?.id).toBe('a1');
  for (const status of ['done', 'failed', 'stopped', 'interrupted']) expect(workingDirectly([own(status)], 4)).toBeNull();
  // Another chat's; an agent Chat sent out (its latest turn isn't the person's own message); no chat open.
  expect(workingDirectly([own('running', 5)], 4)).toBeNull();
  expect(workingDirectly([{ id: 'a2', conversation: 4, status: 'running', turnList: [{ n: 1, direct: true }, { n: 2 }] }], 4)).toBeNull();
  expect(workingDirectly([{ id: 'a3', conversation: 4, status: 'running' }], 4)).toBeNull();
  expect(workingDirectly([own('running')], null)).toBeNull();
  expect(workingDirectly(null, 4)).toBeNull();
  // What it was writing is let go when the message is whole, it moves on to a step, or it's over —
  // never for what a turn has used, its to-do list or the line it's on.
  expect(sayingOver({ id: 'a1', status: 'running', said: 'Done.' })).toBe(true);
  expect(sayingOver({ id: 'a1', status: 'running', text: 'Done.' })).toBe(true);
  expect(sayingOver({ id: 'a1', status: 'running', step: 'Reading notes.md' })).toBe(true);
  expect(sayingOver({ id: 'a1', status: 'done' })).toBe(true);
  expect(sayingOver({ id: 'a1', status: 'running', usage: { input: 10 } })).toBe(false);
  expect(sayingOver({ id: 'a1', status: 'running', todos: [] })).toBe(false);
  expect(sayingOver({ id: 'a1', status: 'asking', now: 'Waiting for you to allow a step' })).toBe(false);

  // A turn's end is its own last event's: over, and its final words, before the store's record says so.
  const going = [{ status: 'running', step: 'Reading notes.md' }, { status: 'running', said: 'Done.' }];
  expect(turnEnd(going)).toEqual({ over: false, text: '' });
  expect(turnEnd([...going, { status: 'done', text: ' Done. ', cost: 0.01 }])).toEqual({ over: true, text: 'Done.' });
  expect(turnEnd([...going, { status: 'done', text: 'Done.' }, { status: 'done', changed: 'finished' }])).toEqual({ over: true, text: 'Done.' });
  expect(turnEnd([...going, { status: 'stopped' }])).toEqual({ over: true, text: '' });
  // Carried on after it ended: at work again.
  expect(turnEnd([...going, { status: 'stopped' }, { status: 'queued', step: 'Picking up where it left off' }])).toEqual({ over: false, text: '' });
  expect(turnEnd([])).toEqual({ over: false, text: '' });
  expect(turnEnd(null)).toEqual({ over: false, text: '' });

  // Its final words are the answer's alone once the answer is in the thread: an assistant message after
  // the question, before the next message that started a turn (a note while it worked started none).
  const you = { role: 'user' };
  const report = { role: 'assistant', kind: 'agentReport' };
  const answer = { role: 'assistant', text: 'Done.' };
  expect(answeredAfter([you], 0)).toBe(false);
  expect(answeredAfter([you, report], 0)).toBe(false);
  expect(answeredAfter([you, report, answer], 0)).toBe(true);
  expect(answeredAfter([you, you, report, answer], 0)).toBe(true);
  expect(answeredAfter([you, report, { ...answer, streaming: true }], 0)).toBe(false);
  // The next question's answer isn't this one's.
  expect(answeredAfter([you, you, report, answer], 0, (k) => k === 1)).toBe(false);
  expect(answeredAfter([you, report, answer, you, report, answer], 3)).toBe(true);
  expect(answeredAfter(null, 0)).toBe(false);
});

test('while its session works the message box has ■ Stop where Send was; Esc in the empty box stops it too', async ({ yap, main, shot }) => {
  await script(yap, SLOWISH, 'slowish');
  const before = (await agents(yap)).length;
  await codeChat(main);
  const box = main.getByRole('textbox', { name: 'Message' });
  const stop = main.getByTestId('chat-stop-work');
  // Nothing at work: no ■.
  await expect(stop).toHaveCount(0);
  await send(main, 'Read notes.md.');
  const agent = await newAgent(yap, before);
  await expect.poll(async () => (await agents(yap)).find((j) => j.id === agent.id).status, { timeout: 20_000 }).toBe('running');
  await expect(stop).toBeVisible();
  await expect(stop).toHaveAccessibleName('Stop the work');
  // Something typed: Send, as ever (it would go to the session as a note).
  await box.fill('Keep the headings.');
  await expect(stop).toHaveCount(0);
  await expect(main.getByRole('button', { name: 'Send', exact: true })).toBeVisible();
  await box.fill('');
  await shot(main, '20-stop-where-send-was');
  await stop.click();
  expect((await ended(yap, agent.id)).status).toBe('stopped');
  await expect(stop).toHaveCount(0);

  // Its next message carries it on; Esc in the empty box stops that too.
  await send(main, 'Read it again.');
  await expect.poll(async () => (await agents(yap)).find((j) => j.id === agent.id).status, { timeout: 20_000 }).toBe('running');
  await expect(stop).toBeVisible();
  await box.focus();
  await box.press('Escape');
  expect((await ended(yap, agent.id)).status).toBe('stopped');
  expect((await agents(yap)).length).toBe(before + 1);
});

test('“Work directly in Code chats” off: the message goes to Chat, never to a session by itself', async ({ yap, main }) => {
  await setConfig(yap, main, { agentsDirect: false });
  // (What Chat's own session says, when it's asked.)
  await script(yap, [{ _wait_user: true }, result('Chat answered this one itself.', 0), { _wait_user: true }], 'chat-answers');
  const before = (await agents(yap)).length;
  await codeChat(main);
  await send(main, 'Add a watering schedule to notes.md.');
  // Chat's own session answered (the stand-in's words for it), and no session was started for the workspace.
  await expect(thread(main).locator('.cbubble.assistant')).toHaveText(['Chat answered this one itself.'], { timeout: 30_000 });
  expect((await agents(yap)).length).toBe(before);
  expect(argsSeen()).toHaveLength(0);
});
