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

const HERE = path.join(RUNS, 'agents-direct-spec');
const SCRIPTS = path.join(HERE, 'scripts');
const GARDEN = path.join(HERE, 'folders', 'garden-planner');
const ARGS = path.join(HERE, 'args.jsonl');

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
const script = (yap, lines, name) => yap.invoke('e2e_claude_script', { script: writeScript(name, lines) });
const tab = (main) => main.getByTestId('chat-agent');

async function send(main, text) {
  await main.getByRole('textbox', { name: 'Message' }).fill(text);
  await main.getByRole('button', { name: 'Send', exact: true }).click();
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
