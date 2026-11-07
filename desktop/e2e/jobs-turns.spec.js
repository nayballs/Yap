// One job per Code chat, its follow-ups as its turns (2026-11-07):
// desktop/src-tauri/src/jobs/runner.rs (`follow_up`, `undo_turn`, per-turn
// backups), chat/tools/jobs.rs (`decide`: start_job in a Code chat carries
// the chat's job on), chat/turn/report.rs (a report a turn), and the tab
// (views/chat/JobTab.svelte + JobTurn.svelte; core/jobs.js `turnsOf`,
// `pillsByAnswer`, `jobTabName`). As Claude Code itself is one session per
// conversation, each follow-up in a Code chat ("could you add a minimap
// button for it as well?") continues the chat's job with its context.
//
// Jobs are driven as jobs-ui.spec.js drives them: the scripted fake AI calls
// the job tools, and Claude Code is this build's own stand-in (`yap.exe
// fake-claude <script>`, switched per turn with `e2e_claude_script`), whose
// `_dump_args` here appends each turn's arguments to a file of this run's
// own (`--session-id`, then `--resume`). No one's real Claude Code, account
// or files: the workspace is in this run's own folder.
//
// - Two requests in a Code chat: one job, one tab with two turns (the
//   newest open, the earlier folded), two reports, the second turn on
//   `--resume` of the same session; a pill under each answer and a report
//   card each, all opening the same tab at their turn. Light and dark.
// - A request while the job works: queued as its next turn (said so, shown
//   so), sent once the turn ends, never a second job.
// - "Start fresh" (`fresh: true`): a second job, its tab named apart.
// - Undo this turn (refused while a later turn changed the same file:
//   "Undo the later change first"), and Undo all, asked first.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, setConfig } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { RUNS } from './support/yap.js';
import { newCodeChat } from './support/code-chats.js';
import { turnsOf, titleOf, turnStatusWord, canUndoTurn, pillsByAnswer, jobOfReport, turnOfReport, jobTabName } from '../src/lib/core/jobs.js';

const HERE = path.join(RUNS, 'jobs-turns-spec');
const SCRIPTS = path.join(HERE, 'scripts');
const GARDEN = path.join(HERE, 'folders', 'garden-planner');
const ARGS = path.join(HERE, 'args.jsonl');

const STARTED = "On it: I've started a job in garden-planner.";
const PASSED = "On it: I've passed that to the job in garden-planner.";
const QUEUED = "The job in garden-planner is still working: I'll pass that on when it finishes this step.";
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
/** A turn that takes its time (a request comes while it works). */
const SLOWISH = [
  { _wait_user: true },
  { _dump_args: ARGS },
  init(),
  said('s1', [{ type: 'tool_use', id: 'toolu_read', name: 'Read', input: { file_path: '{{cwd}}\\notes.md' } }]),
  { _sleep_ms: 6000 },
  toolResult('toolu_read', 'Draft notes'),
  result('Read notes.md.', 0.01),
  { _wait_user: true },
];
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
      await use({ name: 'jobs-turns', config: { memoryLearnChats: false } });
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
const jobs = (yap) => yap.invoke('job_list');
const script = (yap, lines, name) => yap.invoke('e2e_claude_script', { script: writeScript(name, lines) });
const tab = (main) => main.getByTestId('chat-job');
const turnsIn = (main) => tab(main).getByTestId('chat-job-turn');
const turnAt = (main, n) => tab(main).locator(`[data-testid="chat-job-turn"][data-turn="${n}"]`);
/** The latest tool result the chat model was given. */
const lastTool = (fakeLlm) => JSON.parse(fakeLlm.requests.filter((r) => r.messages.some((m) => m.role === 'tool')).at(-1).messages.filter((m) => m.role === 'tool').at(-1).content);

async function send(main, text) {
  await main.getByRole('textbox', { name: 'Message' }).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
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
    .poll(async () => (await jobs(yap)).find((j) => j.id === id)?.status, { timeout, message: `job ${id} to end` })
    .toMatch(/^(done|failed|stopped)$/);
  return (await jobs(yap)).find((j) => j.id === id);
}

async function newJob(yap, before) {
  await expect.poll(async () => (await jobs(yap)).length, { timeout: 20_000, message: 'a job to start' }).toBe(before + 1);
  return (await jobs(yap))[0];
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
 * **A Code chat's job, two requests in**: a watering schedule, then "could
 * you add a minimap button for it as well?" — both reported. The job, its
 * conversation, and how many jobs there were before.
 */
async function twoTurns(yap, main, fakeLlm) {
  fs.rmSync(ARGS, { force: true });
  await script(yap, WATERING, 'watering');
  const before = (await jobs(yap)).length;
  await codeChat(main);
  fakeLlm.scriptChat([
    { tools: [{ name: 'start_job', args: { brief: 'Add a watering schedule to notes.md.', title: 'Add a watering schedule' } }] },
    { content: STARTED },
    { plain: true, content: 'Done: notes.md has a watering schedule.' },
  ]);
  await send(main, 'Add a watering schedule to notes.md.');
  const first = await ended(yap, (await newJob(yap, before)).id);
  const conversation = first.conversation;
  await messagesOf(yap, conversation, 4);
  // The follow-up: the same job, its next turn.
  await script(yap, MINIMAP_TURN, 'minimap');
  fakeLlm.scriptChat([
    { tools: [{ name: 'start_job', args: { brief: 'Add a minimap button for the beds, and say so in notes.md.', title: 'Add a minimap button' } }] },
    { content: PASSED },
    { plain: true, content: 'Done: there is a minimap button for the beds now.' },
  ]);
  await send(main, 'could you add a minimap button for it as well?');
  await expect(chipsOfLast(main)).toHaveText(['Continued the job in garden-planner']);
  await expect.poll(async () => (await jobs(yap)).find((j) => j.id === first.id).turnList?.length, { timeout: 20_000 }).toBe(2);
  await expect.poll(async () => (await jobs(yap)).find((j) => j.id === first.id).turnList[1].status, { timeout: 30_000 }).toBe('done');
  const job = await ended(yap, first.id);
  await messagesOf(yap, conversation, 8);
  return { job, conversation, before, first };
}

test.beforeEach(async ({ yap, main, fakeLlm }) => {
  freshFolder(GARDEN);
  await yap.invoke('e2e_claude_approve_auto', { allow: true });
  await setConfig(yap, main, {
    jobsEnabled: true,
    jobsAsk: 'new-or-web',
    jobsSpeak: 'never',
    jobsPay: 'plan',
    jobsMode: 'auto',
    webSearch: false,
    speakReplies: false,
    theme: 'light',
    llmScopes: chatScope(fakeLlm),
  });
  if (!(await yap.invoke('job_folders_list')).some((f) => f.name === 'garden-planner')) {
    await yap.invoke('job_folders_add', { path: GARDEN });
  }
});

// ---------------------------------------------------------------- no app

test('core/jobs.js: a job’s turns from its log, their words, the pills a turn each, a report’s turn, tab names apart', () => {
  // An older job (no turns kept): one, from the job, its whole log.
  const old = { id: 'a', brief: 'Tidy notes.md. Keep the headings.', status: 'done', files: ['notes.md'], text: 'Done.', messageId: 'chat:5:0' };
  const one = turnsOf(old, [{ step: 'Reading notes.md' }, { step: 'Editing notes.md', file: 'notes.md' }]);
  expect(one.map((t) => [t.n, t.title, t.status, t.events.length])).toEqual([[1, 'Tidy notes.md', 'done', 2]]);
  expect(titleOf('Add a minimap button to the map view. It shows every bed.')).toBe('Add a minimap button to the map view');
  expect(titleOf('could you add a minimap button for it as well?')).toBe('could you add a minimap button for it as well');
  // Turns kept: each event with its turn; one without goes with the turn before it.
  const job = {
    id: 'b',
    status: 'running',
    turnList: [
      { n: 1, title: 'Add a watering schedule', status: 'done', startedTs: 1, files: ['notes.md'] },
      { n: 2, title: 'Add a minimap button', status: 'running', startedTs: 2, files: [] },
      { n: 3, title: 'Add a legend', status: 'queued', startedTs: 0 },
    ],
  };
  const log = [{ turn: 1, step: 'Editing notes.md' }, { step: 'Got your message' }, { turn: 2, step: 'Writing minimap.js' }, { turn: 3, queued: 'Add a legend' }, { turn: 4, step: 'Starting Claude Code', status: 'running' }];
  const turns = turnsOf(job, log);
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
  const over = { ...job, status: 'done' };
  expect(canUndoTurn(over, job.turnList[0])).toBe(true);
  expect(canUndoTurn(job, job.turnList[0]), 'not while it works').toBe(false);
  expect(canUndoTurn(over, job.turnList[1]), 'no files').toBe(false);
  expect(canUndoTurn(over, { ...job.turnList[0], undone: true })).toBe(false);
  expect(canUndoTurn({ ...over, noBackups: true }, job.turnList[0])).toBe(false);
  // A pill a turn, under the answer after its question; one a job an answer.
  const messages = [{ role: 'user' }, { role: 'assistant' }, { role: 'user', kind: 'jobReport' }, { role: 'assistant' }, { role: 'user' }, { role: 'assistant' }];
  const turned = { id: 'c', conversation: 9, createdTs: 1, turnList: [{ n: 1, messageId: 'chat:9:0', status: 'done', title: 'One' }, { n: 2, messageId: 'chat:9:4', status: 'running', title: 'Two' }] };
  const pills = pillsByAnswer([turned], 9, messages);
  expect([...pills.keys()]).toEqual([1, 5]);
  expect(pills.get(1).map((p) => [p.id, p.turn, p.turnStatus])).toEqual([['c', 1, 'done']]);
  expect(pills.get(5).map((p) => [p.id, p.turn, p.turnStatus, p.turnTitle])).toEqual([['c', 2, 'running', 'Two']]);
  // A report's job and turn, by the turn's marker.
  const reported = { ...turned, folder: 'garden-planner', reportMessageId: 'chat:9:7', turnList: [{ ...turned.turnList[0], reportMessageId: 'chat:9:2' }, { ...turned.turnList[1], reportMessageId: 'chat:9:7' }] };
  expect(jobOfReport([reported], 'chat:9:2', 9)?.id).toBe('c');
  expect(turnOfReport(reported, 'chat:9:2')?.n).toBe(1);
  expect(turnOfReport(reported, 'chat:9:7')?.n).toBe(2);
  expect(turnOfReport(old, 'chat:5:2')).toBeNull();
  // Two job tabs in one folder are named apart: their first requests, else a number.
  const a = { kind: 'job', job: { id: 'j1', folder: 'garden-planner', turnList: [{ title: 'Add a watering schedule' }] } };
  const b = { kind: 'job', job: { id: 'j2', folder: 'garden-planner', turnList: [{ title: 'Start over on the planner' }] } };
  const c = { kind: 'job', job: { id: 'j3', folder: 'recipe-box' } };
  expect(jobTabName(c, [a, c])).toBe('Job · recipe-box');
  expect(jobTabName(a, [a, b, c])).toBe('Job · garden-planner · Add a watering schedule');
  expect(jobTabName(b, [a, b, c])).toBe('Job · garden-planner · Start over on the planner');
  const refOnly = { kind: 'job', job: { id: 'j2', folder: 'garden-planner' } };
  expect(jobTabName(refOnly, [a, refOnly], (id) => (id === 'j2' ? b.job : null))).toBe('Job · garden-planner · Start over on the planner');
  const twin = { kind: 'job', job: { id: 'j4', folder: 'garden-planner', turnList: [{ title: 'Add a watering schedule' }] } };
  expect(jobTabName(twin, [a, twin])).toBe('Job · garden-planner #2');
});

// ---------------------------------------------------------------- one job, its turns

test('two requests in a Code chat: one job, one tab with two turns, two reports, the second on --resume', async ({ yap, main, fakeLlm, shot }) => {
  const { job, conversation, before, first } = await twoTurns(yap, main, fakeLlm);
  // One job, its two turns, one session.
  expect((await jobs(yap)).length).toBe(before + 1);
  expect(job.session).toBe(first.session);
  expect(job.turnList.map((t) => [t.n, t.title, t.status])).toEqual([
    [1, 'Add a watering schedule', 'done'],
    [2, 'Add a minimap button', 'done'],
  ]);
  expect(job.turnList.map((t) => t.messageId)).toEqual([`chat:${conversation}:0`, `chat:${conversation}:4`]);
  expect(job.turnList[1].request).toBe('Add a minimap button for the beds, and say so in notes.md.');
  expect(job.turnList[1].files).toEqual(['notes.md', 'minimap.js']);
  expect(job.turnList[0].cost).toBeCloseTo(0.03, 5);
  expect(job.turnList[1].cost).toBeCloseTo(0.02, 5);
  expect(job.cost).toBeCloseTo(0.05, 5);
  expect(job.text).toBe(DONE2);
  expect(read('notes.md')).toBe(NOTES2);
  // The follow-up's tool result: the same job, its next turn.
  expect(lastTool(fakeLlm)).toMatchObject({ success: true, data: { id: job.id, turn: 2, say: PASSED } });
  // The second turn resumed the first's session (Claude Code's own `--resume`).
  const args = argsSeen();
  expect(args).toHaveLength(2);
  expect(after(args[0], '--session-id')).toBe(job.session);
  expect(args[0]).not.toContain('--resume');
  expect(after(args[1], '--resume')).toBe(job.session);
  expect(args[1]).not.toContain('--session-id');
  // Each turn's own backups.
  const backup = path.join(yap.dataDir, 'jobs', job.id, 'backup');
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
    ['user', 'jobReport'],
    ['assistant', ''],
    ['user', ''],
    ['assistant', ''],
    ['user', 'jobReport'],
    ['assistant', ''],
  ]);
  expect(messages[2].text).toContain('Files it changed: notes.md (backed up first).');
  expect(messages[6].text).toContain('Files it changed: notes.md, minimap.js (backed up first).');
  expect(messages[6].text).toContain(DONE2);
  expect(job.turnList.map((t) => t.reportMessageId)).toEqual([`chat:${conversation}:2`, `chat:${conversation}:6`]);

  // One tab: the panel shows the job alone (no row of tabs), its two turns.
  await expect(tab(main)).toBeVisible();
  await expect(main.getByTestId('chat-panel-tabs')).toHaveCount(0);
  await expect(tab(main)).toHaveAttribute('data-turns', '2');
  await expect(turnsIn(main)).toHaveCount(2);
  await expect(turnAt(main, 1)).toHaveAttribute('data-open', 'false');
  await expect(turnAt(main, 2)).toHaveAttribute('data-open', 'true');
  await expect(turnAt(main, 1).locator('.ttitle')).toHaveText('Add a watering schedule');
  await expect(turnAt(main, 2).locator('.ttitle')).toHaveText('Add a minimap button');
  await expect(turnAt(main, 1).getByTestId('chat-job-turn-facts')).toContainText('Done');
  await expect(turnAt(main, 1).getByTestId('chat-job-turn-facts')).toContainText('≈ $0.03');
  await expect(turnAt(main, 2).getByTestId('chat-job-turn-facts')).toContainText('≈ $0.02');
  await expect(main.getByTestId('chat-job-cost')).toHaveText('Your Claude plan · ≈ $0.05 on the API in all');
  await expect(main.getByTestId('chat-job-turn-count')).toHaveText('· 2 turns');
  // The newest open: its steps, files, words; the earlier folded.
  await expect(main.getByTestId('chat-job-text')).toHaveCount(1);
  await expect(turnAt(main, 2).getByTestId('chat-job-text')).toContainText(DONE2);
  await expect(turnAt(main, 2).getByTestId('chat-job-files').locator('.fname')).toHaveText(['notes.md', 'minimap.js']);
  await expect(turnAt(main, 2).getByTestId('chat-job-said')).toHaveText(['Now the minimap button.']);
  await expect(tab(main).getByRole('button', { name: 'Undo all' })).toBeVisible();
  await expect(turnAt(main, 2).getByTestId('chat-job-undo-turn')).toBeVisible();
  // The earlier one opens to its own.
  await turnAt(main, 1).getByTestId('chat-job-turn-head').click();
  await expect(turnAt(main, 1)).toHaveAttribute('data-open', 'true');
  await expect(turnAt(main, 1).getByTestId('chat-job-text')).toContainText(DONE1);
  await expect(turnAt(main, 1).getByTestId('chat-job-said')).toHaveText(["I'll add the watering schedule to notes.md."]);
  await expect(turnAt(main, 1).getByTestId('chat-job-files').locator('.fname')).toHaveText(['notes.md']);
  await main.locator('.jobbody').evaluate((el) => el.scrollTo(0, 0));
  await shot(main, '01-job-tab-two-turns-light');
  await theme(yap, main, 'dark');
  await shot(main, '02-job-tab-two-turns-dark');
  await theme(yap, main, 'light');
  await turnAt(main, 1).getByTestId('chat-job-turn-head').click();
  await expect(turnAt(main, 1)).toHaveAttribute('data-open', 'false');

  // A pill under each answer, a report card each: all the same tab, at their turn.
  const pills = thread(main).getByTestId('chat-job-pill');
  await expect(pills).toHaveCount(2);
  await expect(pills.nth(0)).toHaveAttribute('data-turn', '1');
  await expect(pills.nth(1)).toHaveAttribute('data-turn', '2');
  await expect(pills.nth(1).locator('.jturn')).toHaveText('· Add a minimap button');
  await closeTab(main);
  await pills.nth(0).click();
  await expect(tab(main)).toBeVisible();
  await expect(turnAt(main, 1)).toHaveAttribute('data-open', 'true');
  await expect(main.getByTestId('chat-panel-tabs')).toHaveCount(0);
  const reports = thread(main).getByTestId('chat-job-report');
  await expect(reports).toHaveCount(2);
  await expect(reports.nth(1)).toHaveAttribute('data-turn', '2');
  await expect(reports.nth(1).locator('.jtitle')).toHaveText('Job done · garden-planner');
  await expect(reports.nth(1).locator('.jline')).toHaveText('Changed notes.md, minimap.js');
  await closeTab(main);
  await reports.nth(1).getByRole('button', { name: 'Open the job in garden-planner' }).click();
  await expect(turnAt(main, 2)).toHaveAttribute('data-open', 'true');
  await expect(main.getByTestId('chat-panel-tabs')).toHaveCount(0);
  // The chat's prompt said so: its job, a follow-up continues it.
  const asked = fakeLlm.requests.filter((r) => r.tools.includes('start_job') && JSON.stringify(r.messages).includes('as well?'))[0];
  expect(JSON.stringify(asked.messages)).toContain(`This chat's job there (id ${job.id}) is done, after 1 turn: the user's next request there continues it.`);
});

test('a request while the job works queues as its next turn: said so, shown so, sent once the turn ends', async ({ yap, main, fakeLlm, shot }) => {
  fs.rmSync(ARGS, { force: true });
  await script(yap, SLOWISH, 'slowish');
  const before = (await jobs(yap)).length;
  await codeChat(main);
  fakeLlm.scriptChat([
    { tools: [{ name: 'start_job', args: { brief: 'Read notes.md.', title: 'Read the notes' } }] },
    { content: STARTED },
  ]);
  await send(main, 'Read notes.md.');
  const job = await newJob(yap, before);
  await expect.poll(async () => (await jobs(yap)).find((j) => j.id === job.id).status, { timeout: 20_000 }).toBe('running');
  // Its next turn will say what it's told.
  await script(yap, ECHO, 'echo-next');
  fakeLlm.scriptChat([
    { tools: [{ name: 'start_job', args: { brief: 'Add a legend to the minimap.', title: 'Add a legend' } }] },
    { content: QUEUED },
    { plain: true, content: 'It read notes.md.' },
    { plain: true, content: 'It added the legend.' },
  ]);
  await send(main, 'Add a legend to the minimap too.');
  await expect(chipsOfLast(main)).toHaveText(['Queued for the job in garden-planner']);
  expect(lastTool(fakeLlm)).toMatchObject({ success: true, data: { id: job.id, turn: 2, say: QUEUED } });
  // Waiting in the tab, behind the turn working; still one job.
  await expect(turnAt(main, 2)).toHaveAttribute('data-status', 'queued');
  await expect(turnAt(main, 2).locator('.ttitle')).toHaveText('Add a legend');
  await expect(turnAt(main, 2).locator('.tfacts')).toHaveText('Queued');
  await expect(turnAt(main, 1)).toHaveAttribute('data-open', 'true');
  await expect(turnAt(main, 1).getByTestId('chat-job-now')).toBeVisible();
  await shot(main, '03-job-tab-a-turn-queued');
  expect((await jobs(yap)).length).toBe(before + 1);
  // The turn ends: reported, and the request goes as the next turn, on --resume.
  const done = await (async () => {
    await expect.poll(async () => (await jobs(yap)).find((j) => j.id === job.id).turnList?.[1]?.status, { timeout: 40_000 }).toBe('done');
    return ended(yap, job.id);
  })();
  expect(done.turnList.map((t) => [t.n, t.status])).toEqual([
    [1, 'done'],
    [2, 'done'],
  ]);
  expect(done.turnList[1].text).toContain('Got: Add a legend to the minimap.');
  const args = argsSeen();
  expect(args).toHaveLength(2);
  expect(after(args[1], '--resume')).toBe(done.session);
  expect((await jobs(yap)).length).toBe(before + 1);
  const messages = await messagesOf(yap, done.conversation, 8);
  expect(messages.filter((m) => m.kind === 'jobReport')).toHaveLength(2);
  await expect(turnAt(main, 2)).toHaveAttribute('data-open', 'true');
  await expect(turnAt(main, 1)).toHaveAttribute('data-open', 'false');
});

test('“start fresh”: a second job in the workspace, its tab named apart from the first', async ({ yap, main, fakeLlm }) => {
  const { job, conversation } = await twoTurns(yap, main, fakeLlm);
  const before = (await jobs(yap)).length;
  await script(yap, ECHO, 'echo-fresh');
  fakeLlm.scriptChat([
    { tools: [{ name: 'start_job', args: { brief: 'Start over on the planner: list what it has.', title: 'Start over on the planner', fresh: true } }] },
    { content: STARTED },
    { plain: true, content: 'It listed what the planner has.' },
  ]);
  await send(main, 'Start a new job: start over on the planner and list what it has.');
  await expect(chipsOfLast(main)).toHaveText(['Started a job in garden-planner']);
  const fresh = await ended(yap, (await newJob(yap, before)).id);
  expect(fresh.id).not.toBe(job.id);
  expect(fresh.session).not.toBe(job.session);
  expect(fresh).toMatchObject({ conversation, folder: 'garden-planner', turns: 1 });
  expect(after(argsSeen().at(-1), '--session-id')).toBe(fresh.session);
  await messagesOf(yap, conversation, 12);
  // Both jobs' tabs in the row, never two that read the same.
  await thread(main).getByTestId('chat-job-pill').first().click();
  const names = await main.getByTestId('chat-panel-tab').evaluateAll((els) => els.filter((e) => e.dataset.key?.startsWith('job:')).map((e) => e.getAttribute('title')));
  expect(names).toEqual(['Job · garden-planner · Add a watering schedule', 'Job · garden-planner · Start over on the planner']);
  // The fresh job is the chat's job from now on: its newest in the workspace.
  await closeTab(main);
  await script(yap, ECHO, 'echo-after-fresh');
  fakeLlm.scriptChat([
    { tools: [{ name: 'start_job', args: { brief: 'Also list the seeds.', title: 'List the seeds' } }] },
    { content: PASSED },
    { plain: true, content: 'It listed the seeds.' },
  ]);
  await send(main, 'Also list the seeds.');
  await expect(chipsOfLast(main)).toHaveText(['Continued the job in garden-planner']);
  await expect.poll(async () => (await jobs(yap)).find((j) => j.id === fresh.id).turnList?.length, { timeout: 20_000 }).toBe(2);
  expect((await jobs(yap)).length).toBe(before + 1);
  await ended(yap, fresh.id);
  await messagesOf(yap, conversation, 16);
});

test('Undo this turn: refused while a later turn changed the same file, then each turn back; Undo all, asked first', async ({ yap, main, fakeLlm, shot }) => {
  const { job } = await twoTurns(yap, main, fakeLlm);
  await expect(tab(main)).toBeVisible();
  // The earlier turn first: refused, and why.
  await turnAt(main, 1).getByTestId('chat-job-turn-head').click();
  await turnAt(main, 1).getByTestId('chat-job-undo-turn').click();
  await expect(turnAt(main, 1).getByTestId('chat-job-turn-refused')).toHaveText('Undo the later change first: “Add a minimap button” changed notes.md too.');
  expect(read('notes.md')).toBe(NOTES2);
  await shot(main, '04-undo-the-later-change-first');
  // The later turn: back to before it.
  await turnAt(main, 2).getByTestId('chat-job-undo-turn').click();
  await expect(turnAt(main, 2).getByTestId('chat-job-undone')).toHaveText('Undone: notes.md put back; minimap.js deleted (the job made it).');
  expect(read('notes.md')).toBe(NOTES1);
  expect(fs.existsSync(path.join(GARDEN, 'minimap.js'))).toBe(false);
  await expect(turnAt(main, 2).getByTestId('chat-job-undo-turn')).toHaveCount(0);
  // Now the earlier one goes.
  await turnAt(main, 1).getByTestId('chat-job-undo-turn').click();
  await expect(turnAt(main, 1).getByTestId('chat-job-undone')).toHaveText('Undone: notes.md put back.');
  expect(read('notes.md')).toBe(NOTES0);
  await expect.poll(async () => (await jobs(yap)).find((j) => j.id === job.id)).toMatchObject({ undone: true });
  const undone = (await jobs(yap)).find((j) => j.id === job.id);
  expect(undone.turnList.map((t) => t.undone)).toEqual([true, true]);
  await expect(tab(main).getByRole('button', { name: 'Undo all' })).toHaveCount(0);

  // Undo all: asked first, then back to before the first turn in one go.
  freshFolder(GARDEN);
  const again = await twoTurns(yap, main, fakeLlm);
  expect(read('notes.md')).toBe(NOTES2);
  await tab(main).getByRole('button', { name: 'Undo all' }).click();
  const confirm = main.getByRole('alertdialog', { name: 'Undo every turn of this job?' });
  await expect(confirm).toBeVisible();
  await shot(main, '05-undo-all-asked-first');
  await confirm.getByRole('button', { name: 'Undo all' }).click();
  await expect(main.getByTestId('chat-job-undone-all')).toHaveText('Undone: notes.md put back; minimap.js deleted (the job made it).');
  expect(read('notes.md')).toBe(NOTES0);
  expect(fs.existsSync(path.join(GARDEN, 'minimap.js'))).toBe(false);
  await expect.poll(async () => (await jobs(yap)).find((j) => j.id === again.job.id)).toMatchObject({ undone: true });
});
