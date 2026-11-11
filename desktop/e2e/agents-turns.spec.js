// One agent per Code chat, its follow-ups as its turns (2026-11-07):
// desktop/src-tauri/src/agents/runner.rs (`follow_up`, `undo_turn`, per-turn
// backups), chat/tools/agents.rs (`decide`: start_agent in a Code chat carries
// the chat's agent on), chat/turn/report.rs (a report when it stops working), and the tab
// (views/chat/AgentTab.svelte + AgentTurn.svelte; core/agents.js `turnsOf`,
// `pillsByAnswer`, `agentTabName`). As Claude Code itself is one session per
// conversation, each follow-up in a Code chat ("could you add a minimap
// button for it as well?") continues the chat's agent with its context.
//
// Agents are driven as agents-ui.spec.js drives them: the scripted fake AI calls
// the agent tools, and Claude Code is this build's own stand-in (`yap.exe
// fake-claude <script>`, switched per turn with `e2e_claude_script`), whose
// `_dump_args` here appends each turn's arguments to a file of this run's
// own (`--session-id`, then `--resume`). No one's real Claude Code, account
// or files: the workspace is in this run's own folder.
//
// - Two requests in a Code chat: one agent, one tab with two turns (its
//   panel's two rows; its timeline the newest open, the earlier folded), two
//   reports, the second turn on `--resume` of the same session; a pill under
//   each answer and a report card each, all opening the same tab at their
//   turn's row. Light and dark.
// - A note while the agent works goes to it now (11 Nov 2026: Claude Code
//   takes a message written mid-work at its next tool call's end): said so,
//   shown in its tab, read at its next step, the same turn, the same process,
//   one report. One it never reads (its turn ended first) waits as its next
//   turn, on `--resume`, and the two are told in one report, never one each
//   (the chat spoke twice in a row with nobody asking in between).
// - Its work in the thread (11 Nov 2026, after Claude Code's own transcript):
//   what the agent says as it goes, and a line for each stretch of steps
//   ("Edited notes.md +1") that opens into them (views/chat/AgentWork.svelte,
//   core/agents.js `workOf`, `workSummary`).
// - "Start fresh" (`fresh: true`): a second agent, its tab named apart.
// - Undo this turn (refused while a later turn changed the same file:
//   "Undo the later change first"), and Undo all, asked first.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, setConfig } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { RUNS } from './support/yap.js';
import { newCodeChat, agentTimeline } from './support/code-chats.js';
import { turnsOf, titleOf, turnStatusWord, canUndoTurn, pillsByAnswer, agentOfReport, turnOfReport, agentTabName, workedFor, turnNow, workOf, workSummary } from '../src/lib/core/agents.js';

const HERE = path.join(RUNS, 'agents-turns-spec');
const SCRIPTS = path.join(HERE, 'scripts');
const GARDEN = path.join(HERE, 'folders', 'garden-planner');
const ARGS = path.join(HERE, 'args.jsonl');

const STARTED = "On it: I've sent an agent to garden-planner.";
const PASSED = "On it: I've passed that to the agent in garden-planner.";
const NOTED = 'Passed that on: the agent in garden-planner takes it in at its next step.';
const DONE1 = 'Done. notes.md has a watering schedule now.';
const DONE2 = 'Done. There is a minimap button for the beds, and notes.md says so.';
const NOTES0 = 'Draft notes\n';
const NOTES1 = 'Draft notes\nWatering: twice a week\n';
const NOTES2 = 'Draft notes\nWatering: twice a week\nMinimap: on\n';
const MINIMAP = 'export const minimap = true;\n';

// ---- the stand-in's scripts ----

const init = (mode = 'auto') => ({
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
/** An edit of `file` (backed up first by the run's hook), leaving `text`. */
const edit = (id, file, text, old_string, new_string) => [
  said(`${id}a`, [{ type: 'tool_use', id, name: 'Edit', input: { file_path: `{{cwd}}\\${file}`, old_string, new_string } }]),
  { _hook: { event: 'PreToolUse', input: { tool_name: 'Edit', tool_use_id: id, tool_input: { file_path: file, old_string, new_string } } } },
  { _write_file: { path: file, text } },
  toolResult(id, `The file ${file} has been updated.`),
];
/** Its first turn: a watering schedule in notes.md. */
const WATERING = [
  { _wait_user: true },
  { _dump_args: ARGS },
  init(),
  said('w1', [{ type: 'text', text: "I'll add the watering schedule to notes.md." }], 'tool_use'),
  ...edit('toolu_w', 'notes.md', NOTES1, 'Draft notes', 'Draft notes\nWatering: twice a week'),
  said('w2', [{ type: 'text', text: DONE1 }], 'end_turn'),
  result(DONE1, 0.03),
  { _wait_user: true },
];
/** Its next: notes.md again, and a new minimap.js. */
const MINIMAP_TURN = [
  { _wait_user: true },
  { _dump_args: ARGS },
  init(),
  said('m1', [{ type: 'text', text: 'Now the minimap button.' }], 'tool_use'),
  ...edit('toolu_m', 'notes.md', NOTES2, 'Watering: twice a week', 'Watering: twice a week\nMinimap: on'),
  said('m2', [{ type: 'tool_use', id: 'toolu_map', name: 'Write', input: { file_path: '{{cwd}}\\minimap.js', content: MINIMAP } }]),
  { _hook: { event: 'PreToolUse', input: { tool_name: 'Write', tool_use_id: 'toolu_map', tool_input: { file_path: 'minimap.js' } } } },
  { _write_file: { path: 'minimap.js', text: MINIMAP } },
  toolResult('toolu_map', 'File created successfully at: minimap.js'),
  said('m3', [{ type: 'text', text: DONE2 }], 'end_turn'),
  result(DONE2, 0.02),
  { _wait_user: true },
];
/**
 * A turn that takes its time (a note comes while it works): the note is with
 * it at its tool call's end, as Claude Code takes one (`{{notes}}`).
 */
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
/**
 * A turn with no tool call left when the note comes, whose Claude Code ends
 * with its result: the note is never read (it goes back to Yap).
 */
const NEVER_READS = [{ _wait_user: true }, { _dump_args: ARGS }, init(), said('n1', [{ type: 'text', text: 'Reading the notes.' }], 'end_turn'), { _sleep_ms: 6000 }, result('Read notes.md.', 0.01)];
/** Says what it was told. */
const ECHO = [{ _wait_user: true }, { _dump_args: ARGS }, init(), result('Got: {{user_text}}', 0.002), { _wait_user: true }];

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

/** Each turn's Claude Code arguments, as the stand-in wrote them. */
const argsSeen = () => (fs.existsSync(ARGS) ? fs.readFileSync(ARGS, 'utf8').split('\n').filter(Boolean).map((l) => JSON.parse(l).args) : []);
const after = (args, flag) => {
  const i = args.indexOf(flag);
  return i >= 0 ? args[i + 1] : undefined;
};
const read = (f) => fs.readFileSync(path.join(GARDEN, f), 'utf8');

// ---- the app ----

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
      await use({ name: 'agents-turns', config: { memoryLearnChats: false } });
    },
    { scope: 'worker' },
  ],
});

const chatScope = (fakeLlm) => ({
  chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat', apiKey: '', apiKeys: {}, prompt: '', disableThinking: false },
});
const thread = (main) => main.locator('.msgs');
const lastAnswer = (main) => thread(main).locator('.cbubble.assistant').last();
const chipsOfLast = (main) => thread(main).locator('.toolchips').last().locator('.toolchip');
const agents = (yap) => yap.invoke('agent_list');
const script = (yap, lines, name) => yap.invoke('e2e_claude_script', { script: writeScript(name, lines) });
const tab = (main) => main.getByTestId('chat-agent');
const turnsIn = (main) => tab(main).getByTestId('chat-agent-turn');
const turnAt = (main, n) => tab(main).locator(`[data-testid="chat-agent-turn"][data-turn="${n}"]`);
/** Turn `n`'s row in the tab's panel (AgentPanel.svelte). */
const rowAt = (main, n) => tab(main).locator(`[data-testid="chat-agent-row"][data-turn="${n}"]`);
/** The latest tool result the chat model was given. */
const lastTool = (fakeLlm) => JSON.parse(fakeLlm.requests.filter((r) => r.messages.some((m) => m.role === 'tool')).at(-1).messages.filter((m) => m.role === 'tool').at(-1).content);

async function send(main, text) {
  await main.getByRole('textbox', { name: 'Message' }).fill(text);
  await main.getByRole('button', { name: 'Send', exact: true }).click();
}

async function closeTab(main) {
  if (await tab(main).count()) await tab(main).getByRole('button', { name: 'Close the panel' }).click();
}

/** A new code chat in garden-planner. */
async function codeChat(main) {
  await openView(main, 'Chat');
  await closeTab(main);
  await newCodeChat(main, 'garden-planner');
}

async function ended(yap, id, timeout = 30_000) {
  await expect
    .poll(async () => (await agents(yap)).find((j) => j.id === id)?.status, { timeout, message: `agent ${id} to end` })
    .toMatch(/^(done|failed|stopped)$/);
  return (await agents(yap)).find((j) => j.id === id);
}

async function newAgent(yap, before) {
  await expect.poll(async () => (await agents(yap)).length, { timeout: 20_000, message: 'an agent to start' }).toBe(before + 1);
  return (await agents(yap))[0];
}

async function messagesOf(yap, id, n) {
  await expect.poll(async () => (await yap.invoke('chat_get', { id })).messages.length, { timeout: 30_000, message: `${n} messages` }).toBe(n);
  return (await yap.invoke('chat_get', { id })).messages;
}

async function theme(yap, main, which) {
  await setConfig(yap, main, { theme: which });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe(which);
}

/**
 * **A Code chat's agent, two requests in**: a watering schedule, then "could
 * you add a minimap button for it as well?" — both reported. The agent, its
 * conversation, and how many agents there were before.
 */
async function twoTurns(yap, main, fakeLlm) {
  fs.rmSync(ARGS, { force: true });
  await script(yap, WATERING, 'watering');
  const before = (await agents(yap)).length;
  await codeChat(main);
  fakeLlm.scriptChat([
    { tools: [{ name: 'start_agent', args: { brief: 'Add a watering schedule to notes.md.', title: 'Add a watering schedule' } }] },
    { content: STARTED },
    { plain: true, content: 'Done: notes.md has a watering schedule.' },
  ]);
  await send(main, 'Add a watering schedule to notes.md.');
  const first = await ended(yap, (await newAgent(yap, before)).id);
  const conversation = first.conversation;
  await messagesOf(yap, conversation, 4);
  // The follow-up: the same agent, its next turn.
  await script(yap, MINIMAP_TURN, 'minimap');
  fakeLlm.scriptChat([
    { tools: [{ name: 'start_agent', args: { brief: 'Add a minimap button for the beds, and say so in notes.md.', title: 'Add a minimap button' } }] },
    { content: PASSED },
    { plain: true, content: 'Done: there is a minimap button for the beds now.' },
  ]);
  await send(main, 'could you add a minimap button for it as well?');
  await expect(chipsOfLast(main)).toHaveText(['Continued the agent in garden-planner']);
  await expect.poll(async () => (await agents(yap)).find((j) => j.id === first.id).turnList?.length, { timeout: 20_000 }).toBe(2);
  await expect.poll(async () => (await agents(yap)).find((j) => j.id === first.id).turnList[1].status, { timeout: 30_000 }).toBe('done');
  const agent = await ended(yap, first.id);
  await messagesOf(yap, conversation, 8);
  return { agent, conversation, before, first };
}

test.beforeEach(async ({ yap, main, fakeLlm }) => {
  freshFolder(GARDEN);
  await yap.invoke('e2e_claude_approve_auto', { allow: true });
  await setConfig(yap, main, {
    agentsEnabled: true,
    agentsAsk: 'new-or-web',
    agentsSpeak: 'never',
    agentsPay: 'plan',
    agentsMode: 'auto',
    webSearch: false,
    speakReplies: false,
    theme: 'light',
    llmScopes: chatScope(fakeLlm),
  });
  if (!(await yap.invoke('workspaces_list')).some((f) => f.name === 'garden-planner')) {
    await yap.invoke('workspaces_add', { path: GARDEN });
  }
});

// ---------------------------------------------------------------- no app

test('core/agents.js: an agent’s turns from its log, their words, the pills a turn each, a report’s turn, tab names apart', () => {
  // An older agent (no turns kept): one, from the agent, its whole log.
  const old = { id: 'a', brief: 'Tidy notes.md. Keep the headings.', status: 'done', files: ['notes.md'], text: 'Done.', messageId: 'chat:5:0' };
  const one = turnsOf(old, [{ step: 'Reading notes.md' }, { step: 'Editing notes.md', file: 'notes.md' }]);
  expect(one.map((t) => [t.n, t.title, t.status, t.events.length])).toEqual([[1, 'Tidy notes.md', 'done', 2]]);
  expect(titleOf('Add a minimap button to the map view. It shows every bed.')).toBe('Add a minimap button to the map view');
  expect(titleOf('could you add a minimap button for it as well?')).toBe('could you add a minimap button for it as well');
  // Turns kept: each event with its turn; one without goes with the turn before it.
  const agent = {
    id: 'b',
    status: 'running',
    turnList: [
      { n: 1, title: 'Add a watering schedule', status: 'done', startedTs: 1, files: ['notes.md'] },
      { n: 2, title: 'Add a minimap button', status: 'running', startedTs: 2, files: [] },
      { n: 3, title: 'Add a legend', status: 'queued', startedTs: 0 },
    ],
  };
  const log = [{ turn: 1, step: 'Editing notes.md' }, { step: 'Got your message' }, { turn: 2, step: 'Writing minimap.js' }, { turn: 3, queued: 'Add a legend' }, { turn: 4, step: 'Starting Claude Code', status: 'running' }];
  const turns = turnsOf(agent, log);
  expect(turns.map((t) => [t.n, t.events.length])).toEqual([
    [1, 2],
    [2, 1],
    [3, 1],
    [4, 1],
  ]);
  expect(turns[3].title, 'a turn this window hasn’t read yet: its own, until it does').toBe('');
  expect(turns.map(turnStatusWord)).toEqual(['Done', 'Running', 'Queued', 'Running']);
  expect(turnStatusWord({ status: 'stopped', startedTs: 0 })).toBe('Not sent yet');
  // Undo this turn: over, its files, not undone, backups made.
  const over = { ...agent, status: 'done' };
  expect(canUndoTurn(over, agent.turnList[0])).toBe(true);
  expect(canUndoTurn(agent, agent.turnList[0]), 'not while it works').toBe(false);
  expect(canUndoTurn(over, agent.turnList[1]), 'no files').toBe(false);
  expect(canUndoTurn(over, { ...agent.turnList[0], undone: true })).toBe(false);
  expect(canUndoTurn({ ...over, noBackups: true }, agent.turnList[0])).toBe(false);
  // A pill a turn, under the answer after its question; one an agent an answer.
  const messages = [{ role: 'user' }, { role: 'assistant' }, { role: 'user', kind: 'agentReport' }, { role: 'assistant' }, { role: 'user' }, { role: 'assistant' }];
  const turned = { id: 'c', conversation: 9, createdTs: 1, turnList: [{ n: 1, messageId: 'chat:9:0', status: 'done', title: 'One' }, { n: 2, messageId: 'chat:9:4', status: 'running', title: 'Two' }] };
  const pills = pillsByAnswer([turned], 9, messages);
  expect([...pills.keys()]).toEqual([1, 5]);
  expect(pills.get(1).map((p) => [p.id, p.turn, p.turnStatus])).toEqual([['c', 1, 'done']]);
  expect(pills.get(5).map((p) => [p.id, p.turn, p.turnStatus, p.turnTitle])).toEqual([['c', 2, 'running', 'Two']]);
  // A report's agent and turn, by the turn's marker.
  const reported = { ...turned, folder: 'garden-planner', reportMessageId: 'chat:9:7', turnList: [{ ...turned.turnList[0], reportMessageId: 'chat:9:2' }, { ...turned.turnList[1], reportMessageId: 'chat:9:7' }] };
  expect(agentOfReport([reported], 'chat:9:2', 9)?.id).toBe('c');
  expect(turnOfReport(reported, 'chat:9:2')?.n).toBe(1);
  expect(turnOfReport(reported, 'chat:9:7')?.n).toBe(2);
  // One report for two turns (the second asked while the first worked): its latest.
  const together = { ...reported, turnList: reported.turnList.map((t) => ({ ...t, reportMessageId: 'chat:9:7' })) };
  expect(turnOfReport(together, 'chat:9:7')?.n).toBe(2);
  expect(agentOfReport([together], 'chat:9:7', 9)?.id).toBe('c');
  expect(turnOfReport(old, 'chat:5:2')).toBeNull();
  // Its work as the thread tells it: what it said, and a line for each
  // stretch of steps between its words.
  const itsLog = [
    { turn: 1, step: 'Starting Claude Code', ts: 1 },
    { turn: 1, said: "I'll look at the notes first.", ts: 2 },
    { turn: 1, step: 'Reading notes.md', ts: 3 },
    { turn: 1, step: 'Running npm test', command: 'npm test', about: 'Run the tests', ts: 4 },
    { turn: 1, step: 'Running npm run lint', command: 'npm run lint', ts: 5 },
    { turn: 1, step: 'Editing notes.md', file: 'notes.md', ts: 6 },
    { turn: 1, diff: { file: 'notes.md', lines: ['+Watering: twice a week'], added: 4, removed: 1, kind: 'edit' }, ts: 7 },
    { turn: 1, said: 'Now the legend.', ts: 8 },
    { turn: 1, step: 'Writing legend.md', file: 'legend.md', ts: 9 },
    { turn: 1, diff: { file: 'legend.md', lines: ['+Legend'], added: 12, removed: 0, kind: 'new' }, ts: 10 },
    { turn: 1, said: 'Done. The legend is in.', ts: 11 },
  ];
  const work = workOf(itsLog, 'Done. The legend is in.');
  // (Housekeeping before its first words isn't a stretch of work; its final words are its report's.)
  expect(work.map((w) => [w.kind, w.text ?? w.steps.length])).toEqual([
    ['said', "I'll look at the notes first."],
    ['work', 4],
    ['said', 'Now the legend.'],
    ['work', 1],
  ]);
  expect(workSummary(work[1])).toEqual([{ text: 'Ran 2 commands' }, { text: 'read a file' }, { text: 'edited notes.md', added: 4, removed: 1 }]);
  // One step: its own line, as done, with what it changed.
  expect(workSummary(work[3])).toEqual([{ text: 'Wrote legend.md', added: 12, removed: 0 }]);
  // The stretch it's on now: the step it's on, as it is.
  expect(workSummary(work[1], true)).toEqual([{ text: 'Editing notes.md' }]);
  // Still working: its words so far all show, and housekeeping alone is what it's on.
  expect(workOf(itsLog).at(-1)).toMatchObject({ kind: 'said', text: 'Done. The legend is in.' });
  expect(workOf([{ step: 'Starting Claude Code' }]).map((w) => workSummary(w))).toEqual([[{ text: 'Started Claude Code' }]]);
  // Several files: counted, their lines added up; searches and the web in words.
  const many = workOf([
    { step: 'Editing a.js', file: 'a.js' },
    { diff: { file: 'a.js', lines: [], added: 2, removed: 2, kind: 'edit' } },
    { step: 'Editing b.js', file: 'b.js' },
    { diff: { file: 'b.js', lines: [], added: 5, removed: 0, kind: 'edit' } },
    { step: 'Searching the files for "legend"' },
    { step: 'Searching the web for bed spacing' },
  ]);
  expect(workSummary(many[0])).toEqual([{ text: 'Searched the files' }, { text: 'looked on the web' }, { text: 'edited 2 files', added: 7, removed: 2 }]);
  expect(workOf([])).toEqual([]);
  // Two agent tabs in one folder are named apart: their first requests, else a number.
  const a = { kind: 'agent', agent: { id: 'j1', folder: 'garden-planner', turnList: [{ title: 'Add a watering schedule' }] } };
  const b = { kind: 'agent', agent: { id: 'j2', folder: 'garden-planner', turnList: [{ title: 'Start over on the planner' }] } };
  const c = { kind: 'agent', agent: { id: 'j3', folder: 'recipe-box' } };
  expect(agentTabName(c, [a, c])).toBe('Agent · recipe-box');
  expect(agentTabName(a, [a, b, c])).toBe('Agent · garden-planner · Add a watering schedule');
  expect(agentTabName(b, [a, b, c])).toBe('Agent · garden-planner · Start over on the planner');
  const refOnly = { kind: 'agent', agent: { id: 'j2', folder: 'garden-planner' } };
  expect(agentTabName(refOnly, [a, refOnly], (id) => (id === 'j2' ? b.agent : null))).toBe('Agent · garden-planner · Start over on the planner');
  const twin = { kind: 'agent', agent: { id: 'j4', folder: 'garden-planner', turnList: [{ title: 'Add a watering schedule' }] } };
  expect(agentTabName(twin, [a, twin])).toBe('Agent · garden-planner #2');
  // Two agents from before turns in one Code chat (as the maintainer had): their briefs' first words.
  const older = { kind: 'agent', agent: { id: 'j5', folder: 'garden-planner' } };
  expect(agentTabName(older, [a, older], (id) => (id === 'j5' ? { brief: 'Fix the beds page. Then run the tests.' } : null))).toBe('Agent · garden-planner · Fix the beds page');
  // How long it worked: its turns added up, not the hours between them; the running list, its turn now.
  const worked = { status: 'done', startedTs: 100, endedTs: 7300, turnList: [{ startedTs: 100, endedTs: 160 }, { startedTs: 7200, endedTs: 7300 }] };
  expect(workedFor(worked, 8000)).toBe('2 min');
  expect(workedFor({ ...worked, status: 'running', turnList: [worked.turnList[0], { startedTs: 7200 }] }, 7230)).toBe('1 min');
  expect(workedFor({ status: 'done', startedTs: 100, endedTs: 160 }, 999), 'an agent from before turns').toBe('1 min');
  expect(turnNow(worked)).toEqual({ startedTs: 7200, endedTs: 7300 });
  expect(turnNow({ turnList: [{ startedTs: 5 }, { status: 'queued', startedTs: 0 }] })).toEqual({ startedTs: 5 });
});

// ---------------------------------------------------------------- one agent, its turns

test('two requests in a Code chat: one agent, one tab with two turns, two reports, the second on --resume', async ({ yap, main, fakeLlm, shot }) => {
  const { agent, conversation, before, first } = await twoTurns(yap, main, fakeLlm);
  // One agent, its two turns, one session.
  expect((await agents(yap)).length).toBe(before + 1);
  expect(agent.session).toBe(first.session);
  expect(agent.turnList.map((t) => [t.n, t.title, t.status])).toEqual([
    [1, 'Add a watering schedule', 'done'],
    [2, 'Add a minimap button', 'done'],
  ]);
  expect(agent.turnList.map((t) => t.messageId)).toEqual([`chat:${conversation}:0`, `chat:${conversation}:4`]);
  expect(agent.turnList[1].request).toBe('Add a minimap button for the beds, and say so in notes.md.');
  expect(agent.turnList[1].files).toEqual(['notes.md', 'minimap.js']);
  expect(agent.turnList[0].cost).toBeCloseTo(0.03, 5);
  expect(agent.turnList[1].cost).toBeCloseTo(0.02, 5);
  expect(agent.cost).toBeCloseTo(0.05, 5);
  expect(agent.text).toBe(DONE2);
  expect(read('notes.md')).toBe(NOTES2);
  // The follow-up's tool result: the same agent, its next turn.
  expect(lastTool(fakeLlm)).toMatchObject({ success: true, data: { id: agent.id, turn: 2, say: PASSED } });
  // The second turn resumed the first's session (Claude Code's own `--resume`).
  const args = argsSeen();
  expect(args).toHaveLength(2);
  expect(after(args[0], '--session-id')).toBe(agent.session);
  expect(args[0]).not.toContain('--resume');
  expect(after(args[1], '--resume')).toBe(agent.session);
  expect(args[1]).not.toContain('--session-id');
  // Each turn's own backups.
  const backup = path.join(yap.dataDir, 'agents', agent.id, 'backup');
  const manifest = (n) => JSON.parse(fs.readFileSync(path.join(backup, String(n), 'manifest.json'), 'utf8')).files.map((f) => [path.basename(f.original), f.state]);
  expect(manifest(1)).toEqual([['notes.md', 'copied']]);
  expect(manifest(2)).toEqual([
    ['notes.md', 'copied'],
    ['minimap.js', 'created'],
  ]);
  // Two reports, one a turn.
  const messages = await messagesOf(yap, conversation, 8);
  expect(messages.map((m) => [m.role, m.kind ?? ''])).toEqual([
    ['user', ''],
    ['assistant', ''],
    ['user', 'agentReport'],
    ['assistant', ''],
    ['user', ''],
    ['assistant', ''],
    ['user', 'agentReport'],
    ['assistant', ''],
  ]);
  expect(messages[2].text).toContain('Files it changed: notes.md (backed up first).');
  expect(messages[6].text).toContain('Files it changed: notes.md, minimap.js (backed up first).');
  expect(messages[6].text).toContain(DONE2);
  expect(agent.turnList.map((t) => t.reportMessageId)).toEqual([`chat:${conversation}:2`, `chat:${conversation}:6`]);

  // One tab: the panel shows the agent alone (no row of tabs), its two turns.
  await expect(tab(main)).toBeVisible();
  await expect(main.getByTestId('chat-panel-tabs')).toHaveCount(0);
  await expect(tab(main)).toHaveAttribute('data-turns', '2');
  // Its panel: a row a turn, the newest first, both finished.
  await expect(tab(main).getByTestId('chat-agent-row')).toHaveCount(2);
  await expect(tab(main).getByTestId('chat-agent-row').first()).toHaveAttribute('data-turn', '2');
  await expect(tab(main).locator('[data-testid="chat-agent-group"][data-group="finished"] [data-testid="chat-agent-group-head"]')).toHaveText('Finished · 2');
  await expect(rowAt(main, 1).getByTestId('chat-agent-row-title')).toHaveText('Add a watering schedule');
  await expect(rowAt(main, 2).getByTestId('chat-agent-row-title')).toHaveText('Add a minimap button');
  // Its whole timeline: a section a turn.
  await agentTimeline(main);
  await expect(turnsIn(main)).toHaveCount(2);
  await expect(turnAt(main, 1)).toHaveAttribute('data-open', 'false');
  await expect(turnAt(main, 2)).toHaveAttribute('data-open', 'true');
  await expect(turnAt(main, 1).locator('.ttitle')).toHaveText('Add a watering schedule');
  await expect(turnAt(main, 2).locator('.ttitle')).toHaveText('Add a minimap button');
  await expect(turnAt(main, 1).getByTestId('chat-agent-turn-facts')).toContainText('Done');
  await expect(turnAt(main, 1).getByTestId('chat-agent-turn-facts')).toContainText('≈ $0.03');
  await expect(turnAt(main, 2).getByTestId('chat-agent-turn-facts')).toContainText('≈ $0.02');
  await expect(main.getByTestId('chat-agent-cost')).toHaveText('Your Claude plan · ≈ $0.05 on the API in all');
  await expect(main.getByTestId('chat-agent-turn-count')).toHaveText('· 2 turns');
  // The newest open: its steps, files, words; the earlier folded.
  await expect(main.getByTestId('chat-agent-text')).toHaveCount(1);
  await expect(turnAt(main, 2).getByTestId('chat-agent-text')).toContainText(DONE2);
  await expect(turnAt(main, 2).getByTestId('chat-agent-files').locator('.fname')).toHaveText(['notes.md', 'minimap.js']);
  await expect(turnAt(main, 2).getByTestId('chat-agent-said')).toHaveText(['Now the minimap button.']);
  await expect(tab(main).getByRole('button', { name: 'Undo all' })).toBeVisible();
  await expect(turnAt(main, 2).getByTestId('chat-agent-undo-turn')).toBeVisible();
  // The earlier one opens to its own.
  await turnAt(main, 1).getByTestId('chat-agent-turn-head').click();
  await expect(turnAt(main, 1)).toHaveAttribute('data-open', 'true');
  await expect(turnAt(main, 1).getByTestId('chat-agent-text')).toContainText(DONE1);
  await expect(turnAt(main, 1).getByTestId('chat-agent-said')).toHaveText(["I'll add the watering schedule to notes.md."]);
  await expect(turnAt(main, 1).getByTestId('chat-agent-files').locator('.fname')).toHaveText(['notes.md']);
  await main.locator('.agentbody').evaluate((el) => el.scrollTo(0, 0));
  await shot(main, '01-agent-tab-two-turns-light');
  await theme(yap, main, 'dark');
  await shot(main, '02-agent-tab-two-turns-dark');
  await theme(yap, main, 'light');
  await turnAt(main, 1).getByTestId('chat-agent-turn-head').click();
  await expect(turnAt(main, 1)).toHaveAttribute('data-open', 'false');

  // A pill under each answer, a report card each: all the same tab, at their turn.
  const pills = thread(main).getByTestId('chat-agent-pill');
  await expect(pills).toHaveCount(2);
  await expect(pills.nth(0)).toHaveAttribute('data-turn', '1');
  await expect(pills.nth(1)).toHaveAttribute('data-turn', '2');
  await expect(pills.nth(1).locator('.jturn')).toHaveText('· Add a minimap button');
  await closeTab(main);
  await pills.nth(0).click();
  await expect(tab(main)).toBeVisible();
  await expect(rowAt(main, 1)).toHaveAttribute('data-open', 'true');
  // Turn 2's row was opened earlier (the timeline's Open): remembered while the window stays open.
  await expect(rowAt(main, 2)).toHaveAttribute('data-open', 'true');
  await expect(main.getByTestId('chat-panel-tabs')).toHaveCount(0);
  const reports = thread(main).getByTestId('chat-agent-report');
  await expect(reports).toHaveCount(2);
  await expect(reports.nth(1)).toHaveAttribute('data-turn', '2');
  await expect(reports.nth(1).locator('.jtitle')).toHaveText('Agent done · garden-planner');
  // Its work, in the thread, under the answer that sent each turn: what it
  // said as it went (never its final words: the report tells those), and a
  // line for each stretch of steps, which opens into them.
  const works = thread(main).getByTestId('chat-agent-work');
  await expect(works).toHaveCount(2);
  await expect(works.nth(0)).toHaveAttribute('data-turn', '1');
  await expect(works.nth(0).getByTestId('chat-agent-work-said')).toHaveText(["I'll add the watering schedule to notes.md."]);
  await expect(works.nth(0).getByTestId('chat-agent-work-steps').locator('.wtext')).toHaveText([/^Edited notes\.md\s*\+1$/]);
  await expect(works.nth(1).getByTestId('chat-agent-work-said')).toHaveText(['Now the minimap button.']);
  await expect(works.nth(1).getByTestId('chat-agent-work-steps').locator('.wtext')).toHaveText([/^Edited 2 files\s*\+2$/]);
  await expect(works.nth(1).locator('.wstep')).toHaveCount(0);
  await works.nth(1).getByRole('button', { name: /Edited 2 files/ }).click();
  await expect(works.nth(1).locator('.wstep .stext')).toHaveText(['Editing notes.md', 'Writing minimap.js']);
  await shot(main, '10-its-work-in-the-thread');
  await works.nth(1).getByRole('button', { name: /Edited 2 files/ }).click();
  await expect(works.nth(1).locator('.wstep')).toHaveCount(0);
  await expect(reports.nth(1).locator('.jline')).toHaveText('Changed notes.md, minimap.js');
  await closeTab(main);
  await reports.nth(1).getByRole('button', { name: 'Open the agent in garden-planner' }).click();
  await expect(rowAt(main, 2)).toHaveAttribute('data-open', 'true');
  // Its Open: the timeline at that turn.
  await agentTimeline(main, 2);
  await expect(turnAt(main, 2)).toHaveAttribute('data-open', 'true');
  await expect(main.getByTestId('chat-panel-tabs')).toHaveCount(0);
  // The chat's prompt said so: its agent, a follow-up continues it.
  const asked = fakeLlm.requests.filter((r) => r.tools.includes('start_agent') && JSON.stringify(r.messages).includes('as well?'))[0];
  expect(JSON.stringify(asked.messages)).toContain(`This chat's agent there (id ${agent.id}) is done, after 1 turn: the user's next request there continues it.`);
});

test('a note while the agent works goes to it now: said so, read at its next step, the same turn, one report', async ({ yap, main, fakeLlm, shot }) => {
  fs.rmSync(ARGS, { force: true });
  await script(yap, SLOWISH, 'slowish');
  const before = (await agents(yap)).length;
  await codeChat(main);
  fakeLlm.scriptChat([
    { tools: [{ name: 'start_agent', args: { brief: 'Read notes.md.', title: 'Read the notes' } }] },
    { content: STARTED },
  ]);
  await send(main, 'Read notes.md.');
  const agent = await newAgent(yap, before);
  await expect.poll(async () => (await agents(yap)).find((j) => j.id === agent.id).status, { timeout: 20_000 }).toBe('running');
  fakeLlm.scriptChat([
    { tools: [{ name: 'start_agent', args: { brief: 'Keep the headings as they are.', title: 'Keep the headings' } }] },
    { content: NOTED },
    { plain: true, content: 'Done. It read notes.md and kept the headings.' },
  ]);
  await send(main, 'Keep the headings as they are.');
  await expect(chipsOfLast(main)).toHaveText(['Told the agent in garden-planner']);
  // Handed over: the turn working's, never a turn of its own.
  expect(lastTool(fakeLlm)).toMatchObject({ success: true, data: { id: agent.id, turn: 1, say: NOTED } });
  expect(lastTool(fakeLlm).data.note).toContain('it takes them into the work in hand at its next step');
  await expect(tab(main).locator('[data-testid="chat-agent-group"][data-group="queued"]')).toHaveCount(0);
  await expect(tab(main).getByTestId('chat-agent-row')).toHaveCount(1);
  await expect(rowAt(main, 1)).toHaveAttribute('data-state', 'running');
  await expect(rowAt(main, 1).getByTestId('chat-agent-row-line')).toHaveText('Your note is with the agent: it reads it at its next step');
  // …and in the thread: the line it's on, live.
  const working = thread(main).getByTestId('chat-agent-work');
  await expect(working).toHaveAttribute('data-live', 'true');
  await expect(working.locator('[data-busy="true"] .wtext')).toHaveText('Your note is with the agent: it reads it at its next step');
  await shot(main, '03-agent-tab-a-note-with-the-agent');
  // Its tool call ends: it reads the note, and answers once for both.
  const done = await ended(yap, agent.id, 40_000);
  expect(done.turnList.map((t) => [t.n, t.status])).toEqual([[1, 'done']]);
  expect(done.turnList[0].request).toBe('Read notes.md.\n\nKeep the headings as they are.');
  expect(done.turnList[0].text).toContain('Read notes.md. Noted: Keep the headings as they are.');
  // One Claude Code process, started so it says what it has read.
  const args = argsSeen();
  expect(args).toHaveLength(1);
  expect(args[0]).toContain('--replay-user-messages');
  expect((await agents(yap)).length).toBe(before + 1);
  // One report, of one turn.
  const messages = await messagesOf(yap, done.conversation, 6);
  expect(messages.map((m) => m.kind || m.role)).toEqual(['user', 'assistant', 'user', 'assistant', 'agentReport', 'assistant']);
  expect(messages[4].text).not.toContain('This report covers');
  expect(messages[4].text).toContain('Its own words: Read notes.md. Noted: Keep the headings as they are.');
  await expect(thread(main).getByTestId('chat-agent-report')).toHaveCount(1);
  await expect(lastAnswer(main)).toHaveText('Done. It read notes.md and kept the headings.');
  // Its tab: the note read, in the turn's steps.
  await expect(thread(main).getByTestId('chat-agent-work')).toHaveAttribute('data-live', 'false');
  await expect(thread(main).getByTestId('chat-agent-work').locator('.wtext')).toHaveText(['Read notes.md']);
  await agentTimeline(main, 1);
  await expect(turnAt(main, 1)).toContainText('Read your note');
  await shot(main, '06-a-note-read-one-report');
});

test('a note the agent never reads waits as its next turn, and both are told in one report', async ({ yap, main, fakeLlm, shot }) => {
  fs.rmSync(ARGS, { force: true });
  await script(yap, NEVER_READS, 'never-reads');
  const before = (await agents(yap)).length;
  await codeChat(main);
  fakeLlm.scriptChat([
    { tools: [{ name: 'start_agent', args: { brief: 'Read notes.md.', title: 'Read the notes' } }] },
    { content: STARTED },
  ]);
  await send(main, 'Read notes.md.');
  const agent = await newAgent(yap, before);
  await expect.poll(async () => (await agents(yap)).find((j) => j.id === agent.id).status, { timeout: 20_000 }).toBe('running');
  // Its next turn will say what it's told.
  await script(yap, ECHO, 'echo-next');
  fakeLlm.scriptChat([
    { tools: [{ name: 'start_agent', args: { brief: 'Add a legend to the minimap.', title: 'Add a legend' } }] },
    { content: NOTED },
    { plain: true, content: 'Done. It read notes.md, and the legend is in.' },
  ]);
  await send(main, 'Add a legend to the minimap too.');
  await expect(chipsOfLast(main)).toHaveText(['Told the agent in garden-planner']);
  expect(lastTool(fakeLlm)).toMatchObject({ success: true, data: { id: agent.id, turn: 1, say: NOTED } });
  // The turn ends with the note unread: it goes as the next turn, on --resume.
  const done = await (async () => {
    await expect.poll(async () => (await agents(yap)).find((j) => j.id === agent.id).turnList?.[1]?.status, { timeout: 40_000 }).toBe('done');
    return ended(yap, agent.id);
  })();
  expect(done.turnList.map((t) => [t.n, t.status, t.title])).toEqual([
    [1, 'done', 'Read the notes'],
    [2, 'done', 'Add a legend'],
  ]);
  expect(done.turnList[0].request).toBe('Read notes.md.');
  expect(done.turnList[1].text).toContain('Got: Add a legend to the minimap.');
  const args = argsSeen();
  expect(args).toHaveLength(2);
  expect(after(args[1], '--resume')).toBe(done.session);
  expect((await agents(yap)).length).toBe(before + 1);
  // One report once it stops working, for both requests: one card, one answer.
  const reports = () => thread(main).getByTestId('chat-agent-report');
  const messages = await messagesOf(yap, done.conversation, 6);
  expect(messages.map((m) => m.kind || m.role)).toEqual(['user', 'assistant', 'user', 'assistant', 'agentReport', 'assistant']);
  const marker = messages[4].text;
  expect(marker).toContain('This report covers 2 requests it worked through one after the other');
  expect(marker).toContain('Request 1, "Read the notes". Its own words: Read notes.md.');
  expect(marker).toContain('Request 2, "Add a legend". Its own words: Got: Add a legend to the minimap.');
  expect(messages[5].text).toBe('Done. It read notes.md, and the legend is in.');
  // The chat's model was told to answer for both in one message.
  const told = fakeLlm.requests.filter((r) => JSON.stringify(r.messages).includes('This report covers 2 requests')).at(-1);
  expect(JSON.stringify(told.messages)).toContain('This one report covers 2 requests');
  expect(told.tools).toEqual([]);
  // Both turns are that report's: its card reads as the latest, and opens there.
  const after2 = (await agents(yap)).find((j) => j.id === agent.id);
  const markerId = `chat:${done.conversation}:4`;
  expect(after2.turnList.map((t) => [t.reportMessageId, !!t.reportHeld])).toEqual([
    [markerId, false],
    [markerId, false],
  ]);
  await expect(reports()).toHaveCount(1);
  await expect(reports()).toHaveAttribute('data-turn', '2');
  await expect(reports().locator('.jtitle')).toHaveText('Agent done · garden-planner');
  await expect(lastAnswer(main)).toHaveText('Done. It read notes.md, and the legend is in.');
  await shot(main, '07-one-report-for-both-requests');
  await agentTimeline(main, 2);
  await expect(turnAt(main, 2)).toHaveAttribute('data-open', 'true');
  await expect(turnAt(main, 1)).toHaveAttribute('data-open', 'false');
});

test('a question sent just before an agent’s report is answered: its card still shows, and that answer tells what the agent did', async ({ yap, main, fakeLlm, shot }) => {
  // 11 Nov 2026, a Yap Lab run on the real Claude Code: a question sent in
  // the seconds between an agent finishing and its report. The report's own
  // answer gives way to it (as to any newer question) — but its card never
  // showed (the view skipped reading the chat again while its own question
  // was answered), and nobody said what the agent had done.
  fs.rmSync(ARGS, { force: true });
  // (An agent that takes a few seconds: Chat's own answer is in before it ends.)
  await script(yap, SLOWISH, 'slowish-overtaken');
  const before = (await agents(yap)).length;
  await codeChat(main);
  fakeLlm.scriptChat([
    { tools: [{ name: 'start_agent', args: { brief: 'Read notes.md.', title: 'Read the notes' } }] },
    { content: STARTED },
    // The report's own answer: held back (below), and never said.
    { plain: true, content: 'Never said: a question came first.' },
    { content: 'The agent read notes.md and changed nothing. The footer is fine as it is.' },
  ]);
  await send(main, 'Read notes.md.');
  const agent = await newAgent(yap, before);
  await expect(lastAnswer(main)).toHaveText(STARTED);
  // Every answer takes a while from here: the report's is still on its way
  // when the next question is sent.
  fakeLlm.delay('other', 4000);
  try {
    const done = await ended(yap, agent.id, 40_000);
    await expect.poll(async () => (await yap.invoke('chat_get', { id: done.conversation })).messages.some((m) => m.kind === 'agentReport'), { timeout: 20_000, message: 'the report’s marker in the chat' }).toBe(true);
    await send(main, 'And is the footer fine?');
    // Answered: the report's card is in the thread, above the question, and
    // the answer is the question's — which told the report first.
    await expect(lastAnswer(main)).toHaveText('The agent read notes.md and changed nothing. The footer is fine as it is.', { timeout: 30_000 });
    const messages = await messagesOf(yap, done.conversation, 5);
    expect(messages.map((m) => m.kind || m.role)).toEqual(['user', 'assistant', 'agentReport', 'user', 'assistant']);
    const reports = thread(main).getByTestId('chat-agent-report');
    await expect(reports).toHaveCount(1);
    await expect(reports.locator('.jtitle')).toHaveText('Agent done · garden-planner');
    const order = await thread(main).locator('[data-testid="chat-agent-report"], .cbubble.user').evaluateAll((els) => els.map((e) => (e.dataset.testid === 'chat-agent-report' ? 'report' : e.textContent.trim())));
    expect(order).toEqual(['Read notes.md.', 'report', 'And is the footer fine?']);
    const asked = fakeLlm.requests.filter((r) => JSON.stringify(r.messages).includes('And is the footer fine?')).at(-1);
    expect(JSON.stringify(asked.messages)).toContain("The user hasn't been told about it yet");
    expect(JSON.stringify(asked.messages)).not.toContain('Never said');
    // The pill of its turn is under the first answer again (the thread's ids are the saved ones).
    await expect(thread(main).getByTestId('chat-agent-pill')).toHaveCount(1);
    await shot(main, '08-a-question-overtook-the-report');
    // Once answered, the report reads as any other: nothing to tell twice.
    fakeLlm.delay('other', 0);
    fakeLlm.scriptChat([{ content: 'Yes.' }]);
    await send(main, 'Thanks. Anything else?');
    await expect(lastAnswer(main)).toHaveText('Yes.');
    const later = fakeLlm.requests.filter((r) => JSON.stringify(r.messages).includes('Anything else?')).at(-1);
    expect(JSON.stringify(later.messages)).not.toContain("hasn't been told about it yet");
    expect(JSON.stringify(later.messages)).toContain('Report from the agent you started (not from the person): ');
  } finally {
    fakeLlm.delay('other', 0);
  }
});

test('asked how it is getting on just as the agent finishes: that answer tells the result, and the report adds its card alone', async ({ yap, main, fakeLlm, shot }) => {
  // 11 Nov 2026, a Yap Lab run on the real Claude Code: "Stop the agent." as
  // it finished. Chat's model looked (agent_status), its answer told the
  // whole result — and then the report said it all again.
  fs.rmSync(ARGS, { force: true });
  await script(yap, SLOWISH, 'slowish-asked');
  const before = (await agents(yap)).length;
  const reportsAsked = () => fakeLlm.requests.filter((r) => JSON.stringify(r.messages).includes('An agent you sent earlier')).length;
  const askedBefore = reportsAsked();
  await codeChat(main);
  fakeLlm.scriptChat([
    { tools: [{ name: 'start_agent', args: { brief: 'Read notes.md.', title: 'Read the notes' } }] },
    { content: STARTED },
  ]);
  await send(main, 'Read notes.md.');
  const agent = await newAgent(yap, before);
  await expect(lastAnswer(main)).toHaveText(STARTED);
  await expect.poll(async () => (await agents(yap)).find((j) => j.id === agent.id).status, { timeout: 20_000 }).toBe('running');
  // The model takes its time: when it looks, the agent has ended, and its
  // report is waiting for this answer.
  fakeLlm.delay('other', 7000);
  try {
    fakeLlm.scriptChat([{ tools: [{ name: 'agent_status', args: {} }] }, { content: 'It has just finished: it read notes.md and changed nothing.' }]);
    await send(main, 'How is it getting on?');
    await expect(lastAnswer(main)).toHaveText('It has just finished: it read notes.md and changed nothing.', { timeout: 40_000 });
    // It was told the agent had just finished, and to say so itself.
    const looked = lastTool(fakeLlm);
    expect(looked.data.agents[0]).toMatchObject({ id: agent.id, status: 'done', just_finished: true });
    expect(looked.data.note).toContain('Yap shows its card under your answer and says nothing more.');
    // The report: its card, under that answer, and no second answer.
    const done = await ended(yap, agent.id);
    const messages = await messagesOf(yap, done.conversation, 5);
    expect(messages.map((m) => m.kind || m.role)).toEqual(['user', 'assistant', 'user', 'assistant', 'agentReport']);
    expect(messages[4].text.endsWith('(You told the user this in your answer just before it.)')).toBe(true);
    await expect(thread(main).getByTestId('chat-agent-report')).toHaveCount(1);
    await expect(thread(main).getByTestId('chat-agent-report').locator('.jtitle')).toHaveText('Agent done · garden-planner');
    await expect(lastAnswer(main)).toHaveText('It has just finished: it read notes.md and changed nothing.');
    await shot(main, '09-the-answer-told-it-the-card-alone');
    // The next question: nothing more was said meanwhile, the report's own
    // model was never asked, and that card isn't news to tell.
    fakeLlm.delay('other', 0);
    fakeLlm.scriptChat([{ content: 'Yes.' }]);
    await send(main, 'Thanks. All good?');
    await expect(lastAnswer(main)).toHaveText('Yes.');
    const after = await messagesOf(yap, done.conversation, 7);
    expect(after.map((m) => m.kind || m.role)).toEqual(['user', 'assistant', 'user', 'assistant', 'agentReport', 'user', 'assistant']);
    expect(reportsAsked()).toBe(askedBefore);
    const next = fakeLlm.requests.filter((r) => JSON.stringify(r.messages).includes('All good?')).at(-1);
    expect(JSON.stringify(next.messages)).toContain('(You told the user this in your answer just before it.)');
    expect(JSON.stringify(next.messages)).not.toContain("hasn't been told about it yet");
  } finally {
    fakeLlm.delay('other', 0);
  }
});

test('“start fresh”: a second agent in the workspace, its tab named apart from the first', async ({ yap, main, fakeLlm }) => {
  const { agent, conversation } = await twoTurns(yap, main, fakeLlm);
  const before = (await agents(yap)).length;
  await script(yap, ECHO, 'echo-fresh');
  fakeLlm.scriptChat([
    { tools: [{ name: 'start_agent', args: { brief: 'Start over on the planner: list what it has.', title: 'Start over on the planner', fresh: true } }] },
    { content: STARTED },
    { plain: true, content: 'It listed what the planner has.' },
  ]);
  await send(main, 'Start a new agent: start over on the planner and list what it has.');
  await expect(chipsOfLast(main)).toHaveText(['Sent an agent to garden-planner']);
  const fresh = await ended(yap, (await newAgent(yap, before)).id);
  expect(fresh.id).not.toBe(agent.id);
  expect(fresh.session).not.toBe(agent.session);
  expect(fresh).toMatchObject({ conversation, folder: 'garden-planner', turns: 1 });
  expect(after(argsSeen().at(-1), '--session-id')).toBe(fresh.session);
  await messagesOf(yap, conversation, 12);
  // Both agents' tabs in the row, never two that read the same.
  await thread(main).getByTestId('chat-agent-pill').first().click();
  // (Once the agents list has the fresh agent's turns: a moment after its tab opens.)
  const names = () => main.getByTestId('chat-panel-tab').evaluateAll((els) => els.filter((e) => e.dataset.key?.startsWith('agent:')).map((e) => e.getAttribute('title')));
  await expect.poll(names).toEqual(['Agent · garden-planner · Add a watering schedule', 'Agent · garden-planner · Start over on the planner']);
  // The fresh agent is the chat's agent from now on: its newest in the workspace.
  await closeTab(main);
  await script(yap, ECHO, 'echo-after-fresh');
  fakeLlm.scriptChat([
    { tools: [{ name: 'start_agent', args: { brief: 'Also list the seeds.', title: 'List the seeds' } }] },
    { content: PASSED },
    { plain: true, content: 'It listed the seeds.' },
  ]);
  await send(main, 'Also list the seeds.');
  await expect(chipsOfLast(main)).toHaveText(['Continued the agent in garden-planner']);
  await expect.poll(async () => (await agents(yap)).find((j) => j.id === fresh.id).turnList?.length, { timeout: 20_000 }).toBe(2);
  expect((await agents(yap)).length).toBe(before + 1);
  await ended(yap, fresh.id);
  await messagesOf(yap, conversation, 16);
});

test('Undo this turn: refused while a later turn changed the same file, then each turn back; Undo all, asked first', async ({ yap, main, fakeLlm, shot }) => {
  const { agent } = await twoTurns(yap, main, fakeLlm);
  await expect(tab(main)).toBeVisible();
  await agentTimeline(main);
  // The earlier turn first: refused, and why.
  await turnAt(main, 1).getByTestId('chat-agent-turn-head').click();
  await turnAt(main, 1).getByTestId('chat-agent-undo-turn').click();
  await expect(turnAt(main, 1).getByTestId('chat-agent-turn-refused')).toHaveText('Undo the later change first: “Add a minimap button” changed notes.md too.');
  expect(read('notes.md')).toBe(NOTES2);
  await shot(main, '04-undo-the-later-change-first');
  // The later turn: back to before it.
  await turnAt(main, 2).getByTestId('chat-agent-undo-turn').click();
  await expect(turnAt(main, 2).getByTestId('chat-agent-undone')).toHaveText('Undone: notes.md put back; minimap.js deleted (the agent made it).');
  expect(read('notes.md')).toBe(NOTES1);
  expect(fs.existsSync(path.join(GARDEN, 'minimap.js'))).toBe(false);
  await expect(turnAt(main, 2).getByTestId('chat-agent-undo-turn')).toHaveCount(0);
  // Now the earlier one goes.
  await turnAt(main, 1).getByTestId('chat-agent-undo-turn').click();
  await expect(turnAt(main, 1).getByTestId('chat-agent-undone')).toHaveText('Undone: notes.md put back.');
  expect(read('notes.md')).toBe(NOTES0);
  await expect.poll(async () => (await agents(yap)).find((j) => j.id === agent.id)).toMatchObject({ undone: true });
  const undone = (await agents(yap)).find((j) => j.id === agent.id);
  expect(undone.turnList.map((t) => t.undone)).toEqual([true, true]);
  await expect(tab(main).getByRole('button', { name: 'Undo all' })).toHaveCount(0);

  // Undo all: asked first, then back to before the first turn in one go.
  freshFolder(GARDEN);
  const again = await twoTurns(yap, main, fakeLlm);
  expect(read('notes.md')).toBe(NOTES2);
  await tab(main).getByRole('button', { name: 'Undo all' }).click();
  const confirm = main.getByRole('alertdialog', { name: 'Undo every turn of this agent?' });
  await expect(confirm).toBeVisible();
  await shot(main, '05-undo-all-asked-first');
  await confirm.getByRole('button', { name: 'Undo all' }).click();
  await expect(main.getByTestId('chat-agent-undone-all')).toHaveText('Undone: notes.md put back; minimap.js deleted (the agent made it).');
  expect(read('notes.md')).toBe(NOTES0);
  expect(fs.existsSync(path.join(GARDEN, 'minimap.js'))).toBe(false);
  await expect.poll(async () => (await agents(yap)).find((j) => j.id === again.agent.id)).toMatchObject({ undone: true });
});
