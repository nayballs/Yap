// The Jobs screens (desktop/src/lib: views/chat/ModeChip, JobTab, JobPills,
// JobReportCard, JobConfirm, JobApprove, RunningJobs; bar/Overlay's "1 job
// running"; settings/JobsPage; stores/jobs.svelte.js, core/jobs.js) over the
// Jobs backend (desktop/src-tauri/src/jobs.rs). Jobs are driven as
// jobs.spec.js drives them: the scripted fake AI (`scriptChat`) calls
// `start_job`, and Claude Code is this build's own stand-in (`yap.exe
// fake-claude <script>`, switched per turn with `e2e_claude_script`). No
// one's real Claude Code, account or files: the job folders are in this
// run's own folder; Add folder…'s picker takes a folder the test names.
//
// - core/jobs.js, pure: status words, cost lines told apart by how they're
//   paid, lengths of time, steps from a log, the pills under the answer
//   that started a job, problems in plain words, the Mode keys.
// - The Mode chip: only with Jobs on; Claude Code's words; 1–5, ↑ ↓ Enter,
//   Esc; kept per conversation, a new chat's pick applied once its
//   conversation exists; Bypass permissions asks once, then the chip is
//   amber and switches freely. Light and dark.
// - A job's tab opening at once, its log, files, commands, what it wasn't
//   allowed, its words and cost; the pill under the answer; the report as a
//   job card; Undo job. Light and dark. A job whose edits went through with
//   no backup made (Claude Code's hooks didn't run): its tab says Undo can't
//   put the files back, and offers no Undo.
// - A slow job: its live line, the running list above the conversations,
//   "1 job running" on the Yap bar (its click opening the job), Stop,
//   Continue.
// - A job read like Claude Code: its to-do list (the running line its item
//   in progress once a call came back), what it says between steps
//   (markdown, a long one folded), commands by their descriptions and with
//   their `cd` into the folder gone, the steps following the live end (and
//   not when scrolled up: "↓"), Commands run with what each was for, and
//   Files changed opening to their diffs (an edit, a new file, a file
//   rewritten), replayed from the log. Light and dark.
// - The ask-first card as a job; a permission prompt's card in the thread
//   (Deny) and on the bar (Allow once); a job that wasn't signed in (Sign
//   in); one paid by API key.
// - Settings → Jobs: the switch, Claude Code's status, pay with, the
//   defaults, folders (add, cap, remove), asking and reports, History; the
//   Network connections note.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, openSettings, closeSettings, setConfig, inAnotherApp, settingsDialog } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { RUNS } from './support/yap.js';
import {
  statusWord,
  statusTone,
  isLive,
  money,
  costLine,
  duration,
  elapsed,
  stepsOf,
  latestStep,
  timelineOf,
  longSaid,
  todosOf,
  todoCount,
  diffsOf,
  diffTotals,
  diffTitle,
  moreLines,
  diffLine,
  commandsOf,
  messageIndex,
  pillsByAnswer,
  canUndo,
  NO_BACKUPS,
  jobOfReport,
  reportTitle,
  reportFolder,
  problemWords,
  modeForKey,
  modeLabel,
  modelLabel,
  stepKind,
  runningLine,
} from '../src/lib/core/jobs.js';

const HERE = path.join(RUNS, 'jobs-ui-spec');
const SCRIPTS = path.join(HERE, 'scripts');
const GARDEN = path.join(HERE, 'folders', 'garden-planner');
const RECIPES = path.join(HERE, 'folders', 'recipe-box');
const SHELF = path.join(HERE, 'folders', 'book-shelf');
const ANTHROPIC_KEY = 'fake-anthropic-key-for-tests';

const DONE = 'Done. I changed Draft to Final in notes.md and the tests pass.';

// ---- the stand-in's scripts (as jobs.spec.js has them) ----

const init = (mode, keySource = 'none') => ({
  type: 'system',
  subtype: 'init',
  cwd: '{{cwd}}',
  session_id: '{{session_id}}',
  tools: ['Bash', 'Edit', 'Read', 'Write'],
  mcp_servers: [{ name: 'yap', status: 'connected' }],
  model: 'claude-sonnet-5-5',
  permissionMode: mode,
  apiKeySource: keySource,
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
const result = (text, extra = {}) => ({
  type: 'result',
  subtype: 'success',
  is_error: false,
  duration_ms: 1200,
  num_turns: 3,
  result: text,
  session_id: '{{session_id}}',
  total_cost_usd: 0.041,
  usage: { input_tokens: 6100, output_tokens: 240, cache_creation_input_tokens: 0, cache_read_input_tokens: 3000 },
  permission_denials: [],
  terminal_reason: 'completed',
  ...extra,
});

/** Reads notes.md, backs it up and edits it, asks to run the tests, and reports. */
const EDIT = [
  { _wait_user: true },
  init('auto'),
  said('m1', [{ type: 'tool_use', id: 'toolu_read', name: 'Read', input: { file_path: '{{cwd}}\\notes.md' } }]),
  toolResult('toolu_read', 'Draft notes'),
  said('m2', [
    { type: 'text', text: "I'll update the notes file." },
    { type: 'tool_use', id: 'toolu_edit', name: 'Edit', input: { file_path: '{{cwd}}\\notes.md', old_string: 'Draft', new_string: 'Final' } },
  ]),
  { _hook: { event: 'PreToolUse', input: { tool_name: 'Edit', tool_use_id: 'toolu_edit', tool_input: { file_path: 'notes.md', old_string: 'Draft', new_string: 'Final' } } } },
  { _write_file: { path: 'notes.md', text: 'Final notes\n' } },
  toolResult('toolu_edit', 'The file notes.md has been updated.'),
  said('m3', [{ type: 'tool_use', id: 'toolu_bash', name: 'Bash', input: { command: 'npm test', description: 'Run the tests' } }]),
  { _mcp: { tool: 'approve', arguments: { tool_name: 'Bash', input: { command: 'npm test' }, tool_use_id: 'toolu_bash' } } },
  toolResult('toolu_bash', '12 passing'),
  said('m4', [{ type: 'text', text: DONE }], 'end_turn'),
  result(DONE, { permission_denials: [{ tool_name: 'Bash', tool_use_id: 'toolu_rm', tool_input: { command: 'rm -rf build' } }] }),
  { _wait_user: true },
];
/** The same edit with Claude Code's hooks off (a managed policy): the edit goes through, no backup is made. */
const EDIT_NO_HOOK = EDIT.filter((l) => !l._hook);
/** Says what it was told. */
const ECHO = [{ _wait_user: true }, init('auto'), result('Got: {{user_text}}', { total_cost_usd: 0.002 }), { _wait_user: true }];
/** The same, paid by the API key Yap gave it. */
const ECHO_KEY = [{ _wait_user: true }, init('auto', 'ANTHROPIC_API_KEY'), result('Got: {{user_text}}', { total_cost_usd: 0.002 }), { _wait_user: true }];
/** Takes its time (a Stop interrupts it). */
const SLOW = [
  { _wait_user: true },
  init('auto'),
  said('s1', [{ type: 'tool_use', id: 'toolu_read', name: 'Read', input: { file_path: '{{cwd}}\\notes.md' } }]),
  { _sleep_ms: 60000 },
  result('Finished slowly.'),
  { _wait_user: true },
];
// ---- a job as Claude Code tells it: its to-do list, what it says, its commands by what they're for, its diffs ----

/** A path as Git Bash writes it (`E:\a\b` → `/e/a/b`), as Claude Code's commands often do. */
const gitBash = (p) => p.replace(/^([A-Za-z]):[\\/]/, (_, d) => `/${d.toLowerCase()}/`).replace(/\\/g, '/');
const todo = (content, status, activeForm) => ({ content, status, activeForm });
const PLAN = [
  ['Look at how beds are stored', 'Looking at how beds are stored'],
  ['Add a watering schedule', 'Adding a watering schedule'],
  ['Write the planting guide', 'Writing the planting guide'],
];
/** The to-do list with the first `done` ticked and the next in progress. */
const todos = (done) => PLAN.map(([c, a], i) => todo(c, i < done ? 'completed' : i === done ? 'in_progress' : 'pending', a));
const PLANNER = 'export function water(bed) {\n  bed.water += 1;\n  return bed;\n}\n\nexport function plant(bed, seed) {\n  bed.seeds.push(seed);\n  return bed;\n}\n';
const WATER_OLD = 'export function water(bed) {\n  bed.water += 1;\n  return bed;\n}';
const WATER_NEW = 'export function water(bed, litres = 2) {\n  bed.water += litres;\n  bed.lastWatered = new Date().toISOString();\n  return bed;\n}';
const GUIDE = '# Planting guide\n\nTomatoes: after the last frost, in full sun.\nPeppers: two weeks after the tomatoes.\nBeans: straight into the bed in late spring.\n';
const NOTES = '# Notes\n\nWatering schedule added.\n';
const LONG_SAY =
  "Found how beds are stored: each bed is an object with a `water` count, and `water()` adds one every time it's called. Next I'm giving `water()` a number of litres (two by default) and recording when each bed was last watered, so the schedule can tell which beds are due. Then I'll write a short planting guide for the beds that are in the planner today.";
const FINISHED = 'Done. `water()` now takes litres and records when each bed was last watered, there is a new planting-guide.md, and notes.md says so.';
/** Bed files read one after another (the steps outgrow their box). */
const reads = (from, n) =>
  Array.from({ length: n }, (_, k) => {
    const id = `toolu_bed${from + k}`;
    const file = `beds\\bed-${String(from + k).padStart(2, '0')}.json`;
    return [said(`b${from + k}`, [{ type: 'tool_use', id, name: 'Read', input: { file_path: `{{cwd}}\\${file}` } }]), toolResult(id, '{"name":"Bed","water":0}')];
  }).flat();
/** Its first turn: a to-do list, what it says, a command by what it's for and one with a `cd` into the folder, many reads, a step to allow, more reads; then it works on (a Stop ends it). */
const looking = (folder) => [
  { _wait_user: true },
  init('auto'),
  said('t1', [
    { type: 'text', text: "I'll start by looking at how the planner stores its **beds**." },
    { type: 'tool_use', id: 'toolu_todo1', name: 'TodoWrite', input: { todos: todos(0) } },
  ]),
  toolResult('toolu_todo1', 'Todos have been modified successfully.'),
  said('t2', [{ type: 'tool_use', id: 'toolu_ls', name: 'Bash', input: { command: `cd "${gitBash(folder)}" && ls src`, description: 'List the source folder' } }]),
  toolResult('toolu_ls', 'planner.js'),
  said('t3', [{ type: 'tool_use', id: 'toolu_grep', name: 'Bash', input: { command: `cd "${gitBash(folder)}" && grep -n "water" "${gitBash(folder)}/src/planner.js" | head -5` } }]),
  toolResult('toolu_grep', '2:  bed.water += 1;'),
  ...reads(1, 24),
  said('t4', [{ type: 'tool_use', id: 'toolu_build', name: 'Bash', input: { command: 'npm run build', description: 'Build the planner' } }]),
  { _mcp: { tool: 'approve', arguments: { tool_name: 'Bash', input: { command: 'npm run build' }, tool_use_id: 'toolu_build' } } },
  toolResult('toolu_build', 'built'),
  ...reads(25, 10),
  { _sleep_ms: 60000 },
  result('Stopped.'),
  { _wait_user: true },
];
/** Its next turn: the list ticked off, an edit, a new file, a file rewritten, two more commands, and its summary. */
const finishing = [
  { _wait_user: true },
  init('auto'),
  said('f1', [{ type: 'tool_use', id: 'toolu_todo2', name: 'TodoWrite', input: { todos: todos(1) } }]),
  toolResult('toolu_todo2', 'Todos have been modified successfully.'),
  said('f2', [
    { type: 'text', text: LONG_SAY },
    { type: 'tool_use', id: 'toolu_edit', name: 'Edit', input: { file_path: '{{cwd}}\\src\\planner.js', old_string: WATER_OLD, new_string: WATER_NEW } },
  ]),
  { _hook: { event: 'PreToolUse', input: { tool_name: 'Edit', tool_use_id: 'toolu_edit', tool_input: { file_path: 'src\\planner.js' } } } },
  { _write_file: { path: 'src/planner.js', text: PLANNER.replace(WATER_OLD, WATER_NEW) } },
  toolResult('toolu_edit', 'The file src/planner.js has been updated.'),
  said('f3', [{ type: 'tool_use', id: 'toolu_test', name: 'Bash', input: { command: 'npm test', description: 'Run the tests' } }]),
  toolResult('toolu_test', '8 passing'),
  said('f4', [{ type: 'tool_use', id: 'toolu_check', name: 'Bash', input: { command: 'node --check src/planner.js', description: 'Check the planner still parses' } }]),
  toolResult('toolu_check', ''),
  said('f5', [{ type: 'tool_use', id: 'toolu_todo3', name: 'TodoWrite', input: { todos: todos(2) } }]),
  toolResult('toolu_todo3', 'Todos have been modified successfully.'),
  said('f6', [
    { type: 'text', text: 'Now the planting guide.' },
    { type: 'tool_use', id: 'toolu_guide', name: 'Write', input: { file_path: '{{cwd}}\\planting-guide.md', content: GUIDE } },
  ]),
  { _hook: { event: 'PreToolUse', input: { tool_name: 'Write', tool_use_id: 'toolu_guide', tool_input: { file_path: 'planting-guide.md' } } } },
  { _write_file: { path: 'planting-guide.md', text: GUIDE } },
  toolResult('toolu_guide', 'File created successfully at: planting-guide.md'),
  said('f7', [{ type: 'tool_use', id: 'toolu_notes', name: 'Write', input: { file_path: '{{cwd}}\\notes.md', content: NOTES } }]),
  { _hook: { event: 'PreToolUse', input: { tool_name: 'Write', tool_use_id: 'toolu_notes', tool_input: { file_path: 'notes.md' } } } },
  { _write_file: { path: 'notes.md', text: NOTES } },
  toolResult('toolu_notes', 'The file notes.md has been updated.'),
  said('f8', [{ type: 'tool_use', id: 'toolu_todo4', name: 'TodoWrite', input: { todos: todos(3) } }]),
  toolResult('toolu_todo4', 'Todos have been modified successfully.'),
  said('f9', [{ type: 'text', text: FINISHED }], 'end_turn'),
  result(FINISHED),
  { _wait_user: true },
];

/** Claude Code turned away: not signed in. */
const SIGNED_OUT = [
  { _wait_user: true },
  init('auto'),
  result('Failed to authenticate. API Error: 401 Invalid authentication credentials', { is_error: true, subtype: 'error_during_execution', terminal_reason: 'api_error', total_cost_usd: 0 }),
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
  fs.writeFileSync(path.join(dir, 'notes.md'), 'Draft notes\n');
}

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
      await use({ name: 'jobs-ui', config: { memoryLearnChats: false } });
    },
    { scope: 'worker' },
  ],
});

const chatScope = (fakeLlm) => ({
  chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat', apiKey: '', apiKeys: {}, prompt: '', disableThinking: false },
});
const thread = (main) => main.locator('.msgs');
const lastAnswer = (main) => thread(main).locator('.cbubble.assistant').last();
const jobs = (yap) => yap.invoke('job_list');
const script = (yap, lines, name) => yap.invoke('e2e_claude_script', { script: writeScript(name, lines) });
const chip = (main) => main.getByTestId('chat-mode-chip');
const modeMenu = (main) => main.getByRole('menu', { name: 'Mode' });
const tab = (main) => main.getByTestId('chat-job');

/** The Mode menu open from its chip, the keyboard in it. */
async function openMode(main) {
  await chip(main).click();
  await expect(modeMenu(main)).toBeVisible();
  await expect(modeMenu(main).locator('[role="menuitemradio"]:focus')).toHaveCount(1);
  return modeMenu(main);
}

async function send(main, text) {
  await main.getByRole('textbox', { name: 'Message' }).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
}

async function freshChat(main) {
  await openView(main, 'Chat');
  // A job's tab open beside the chat: the list stepped aside for it.
  if (await tab(main).count()) await tab(main).getByRole('button', { name: 'Close the panel' }).click();
  await main.getByRole('button', { name: 'New chat' }).click();
}

/** The conversation the thread shows, once its last answer is saved. */
async function shownConversation(main) {
  await expect(lastAnswer(main)).toHaveAttribute('data-msg-id', /^chat:\d+:\d+$/);
  return Number(/^chat:(\d+):/.exec(await lastAnswer(main).getAttribute('data-msg-id'))[1]);
}

/** The job `id` once it's over (done, failed or stopped). */
async function ended(yap, id, timeout = 30_000) {
  await expect
    .poll(async () => (await jobs(yap)).find((j) => j.id === id)?.status, { timeout, message: `job ${id} to end` })
    .toMatch(/^(done|failed|stopped)$/);
  return (await jobs(yap)).find((j) => j.id === id);
}

/** The job started since `before` jobs were listed. */
async function newJob(yap, before) {
  await expect.poll(async () => (await jobs(yap)).length, { message: 'a job to start' }).toBe(before + 1);
  return (await jobs(yap))[0];
}

/** Conversation `id`'s messages once there are `n`. */
async function messagesOf(yap, id, n) {
  await expect.poll(async () => (await yap.invoke('chat_get', { id })).messages.length, { timeout: 20_000, message: `${n} messages` }).toBe(n);
  return (await yap.invoke('chat_get', { id })).messages;
}

async function theme(yap, main, which) {
  await setConfig(yap, main, { theme: which });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe(which);
}


/**
 * A job started from a fresh chat with `brief`, run by `lines`; the job as
 * listed. `report`: the chat model's words when it's reported (null: it
 * won't be — a job the person stops). Every test waits for its reports, so
 * no scripted reply is left for the next one.
 */
async function startJob(yap, main, fakeLlm, { lines, name, brief, report = 'Done: it went fine.', ask = 'Start a job in garden-planner.' }) {
  await script(yap, lines, name);
  fakeLlm.scriptChat([
    { tools: [{ name: 'start_job', args: { folder: 'garden-planner', brief } }] },
    { content: "On it: I've started a job in garden-planner." },
    ...(report == null ? [] : [{ plain: true, content: report }]),
  ]);
  const before = (await jobs(yap)).length;
  await freshChat(main);
  await send(main, ask);
  return newJob(yap, before);
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
    llmScopes: chatScope(fakeLlm),
  });
  if (!(await yap.invoke('job_folders_list')).some((f) => f.name === 'garden-planner')) {
    await yap.invoke('job_folders_add', { path: GARDEN });
  }
});

// ---------------------------------------------------------------- no app

test('core/jobs.js: status, cost by how it is paid, time, steps, pills, problems and the Mode keys', () => {
  expect(statusWord('asking')).toBe('Waiting for you');
  expect(statusWord('failed')).toBe("Didn't finish");
  expect(statusTone('running')).toBe('live');
  expect(statusTone('interrupted')).toBe('idle');
  expect([isLive('queued'), isLive('asking'), isLive('done')]).toEqual([true, true, false]);
  // Undo: once it's over, with files changed and their backups there.
  const changed = { status: 'done', files: ['notes.md'] };
  expect(canUndo(changed)).toBe(true);
  expect(canUndo({ ...changed, status: 'running' })).toBe(false);
  expect(canUndo({ ...changed, files: [] })).toBe(false);
  expect(canUndo({ ...changed, undone: true })).toBe(false);
  expect(canUndo({ ...changed, swept: true })).toBe(false);
  expect(canUndo({ ...changed, noBackups: true })).toBe(false);
  expect(canUndo(null)).toBe(false);
  expect(NO_BACKUPS).toBe("No backups were made, so Undo can't put these files back.");

  // Plan and key, told apart.
  // (In a plan chat's answers' words: core/aiModel.js paidLine, core/money.js.)
  expect(money(0.041)).toBe('$0.04');
  expect(money(0.0042)).toBe('$0.0042');
  expect(money(0.004)).toBe('$0.004');
  expect(money(0.00002)).toBe('under $0.0001');
  expect(costLine({ pay: 'plan', cost: 0.0042 })).toBe('Your Claude plan · ≈ $0.0042 on the API');
  expect(costLine({ pay: 'plan', cost: 0.041 })).toBe('Your Claude plan · ≈ $0.04 on the API');
  expect(costLine({ pay: 'plan', cost: 0 })).toBe('Your Claude plan');
  expect(costLine({ pay: 'key', cost: 0.0042 })).toBe('API key · $0.0042');
  expect(costLine({ pay: 'key', cost: 0 })).toBe('API key');

  expect([duration(12), duration(185), duration(3900), duration(7200)]).toEqual(['12 s', '3 min', '1 h 5 min', '2 h']);
  expect(elapsed({ status: 'running', startedTs: 1000 }, 1090)).toBe('1 min');
  expect(elapsed({ status: 'done', startedTs: 1000, endedTs: 1030 }, 99999)).toBe('30 s');
  expect(elapsed({ status: 'queued', startedTs: 0 }, 1000)).toBe('');

  const log = [
    { status: 'running', step: 'Starting Claude Code', ts: 1 },
    { status: 'running', step: 'Reading notes.md', ts: 2 },
    { status: 'running', text: "I'll update it.", ts: 3 },
    { status: 'running', step: 'Editing notes.md', file: 'notes.md', ts: 4 },
    { status: 'running', step: 'Editing notes.md', file: 'notes.md', ts: 5 },
    { status: 'running', step: 'Running npm test', command: 'npm test', ts: 6 },
    { status: 'running', step: 'Running npm test', ts: 7 },
  ];
  expect(stepsOf(log).map((s) => s.step)).toEqual(['Starting Claude Code', 'Reading notes.md', 'Editing notes.md', 'Editing notes.md', 'Running npm test']);
  expect(latestStep(log, 'running')).toBe('Running npm test');
  expect(latestStep(log, 'done')).toBe('');
  expect(stepKind({ step: 'Editing notes.md', file: 'notes.md' })).toBe('edit');
  expect(stepKind({ step: 'Running npm test' })).toBe('command');
  expect(stepKind({ step: 'Searching the web · tide tables' })).toBe('web');
  expect(stepKind({ step: 'Reading docs.example.com/guide/start' })).toBe('web');
  expect(stepKind({ step: 'Reading src\\app.ts' })).toBe('read');
  expect(stepKind({ step: 'Waiting for you to allow a step' })).toBe('ask');
  // A command's step by what it's for is still a command.
  expect(stepKind({ step: 'Listing the source folder', command: 'ls src' })).toBe('command');

  // The timeline: steps and what it said, in order; its final words not twice.
  const told = [
    { status: 'running', step: 'Starting Claude Code', ts: 1 },
    { status: 'running', said: "I'll look at the **beds** first.", ts: 2 },
    { status: 'running', todos: [{ content: 'Look at the beds', status: 'in_progress', activeForm: 'Looking at the beds' }], now: 'Looking at the beds', ts: 3 },
    { status: 'running', step: 'Listing the source folder', command: 'ls src', about: 'List the source folder', ts: 4 },
    { status: 'running', now: 'Looking at the beds', ts: 5 },
    { status: 'running', step: 'Editing src\\planner.js', file: 'src\\planner.js', ts: 6 },
    { status: 'running', diff: { file: 'src\\planner.js', kind: 'edit', lines: ['-a', '+b', '@', ' c'], added: 1, removed: 1 }, ts: 7 },
    { status: 'running', step: 'Writing guide.md', file: 'guide.md', ts: 8 },
    { status: 'running', diff: { file: 'guide.md', kind: 'new', lines: ['+# Guide'], added: 1, removed: 0, more: 40 }, ts: 9 },
    { status: 'running', diff: { file: 'src\\planner.js', kind: 'edit', lines: ['-d', '+e'], added: 1, removed: 1, all: true }, ts: 10 },
    { status: 'running', said: 'All done.', ts: 11 },
    { status: 'running', text: 'All done.', cost: 0.01, ts: 12 },
  ];
  const tl = timelineOf(told, 'All done.');
  expect(tl.map((t) => (t.kind === 'said' ? `said: ${t.text}` : t.step))).toEqual([
    'Starting Claude Code',
    "said: I'll look at the **beds** first.",
    'Listing the source folder',
    'Editing src\\planner.js',
    'Writing guide.md',
  ]);
  expect(tl[2]).toMatchObject({ command: 'ls src', about: 'List the source folder' });
  expect(timelineOf(told).filter((t) => t.kind === 'said').map((t) => t.text)).toEqual(["I'll look at the **beds** first.", 'All done.']);
  // An older Yap's log: its words as it worked came as `text`; its result's (with a cost) never.
  expect(timelineOf(log).filter((t) => t.kind === 'said').map((t) => t.text)).toEqual(["I'll update it."]);
  expect([longSaid('short'), longSaid('x'.repeat(300)), longSaid('a\nb\nc\nd\ne')]).toEqual([false, true, true]);
  // The line it's on: the latest step, or the one it moved to without one.
  expect(latestStep(told.slice(0, 5), 'running')).toBe('Looking at the beds');
  expect(latestStep(told.slice(0, 4), 'running')).toBe('Listing the source folder');
  // Its to-do list, the latest.
  expect(todosOf(told)).toEqual([{ content: 'Look at the beds', status: 'in_progress', activeForm: 'Looking at the beds' }]);
  expect(todosOf(log)).toEqual([]);
  expect(todoCount([{ status: 'completed' }, { status: 'in_progress' }, { status: 'pending' }])).toBe('1 of 3 done');
  // What changed in each file, in order.
  const diffs = diffsOf(told);
  expect([...diffs.keys()]).toEqual(['src\\planner.js', 'guide.md']);
  expect(diffs.get('src\\planner.js')).toHaveLength(2);
  expect(diffTotals(diffs.get('src\\planner.js'))).toEqual({ added: 2, removed: 2, made: null });
  expect(diffTotals(diffs.get('guide.md'))).toEqual({ added: 1, removed: 0, made: 'new' });
  expect([diffTitle({ kind: 'new', added: 12 }), diffTitle({ kind: 'rewrite', added: 1 }), diffTitle({ kind: 'write', added: 3 }), diffTitle({ kind: 'edit', added: 3 })]).toEqual([
    'New file · 12 lines',
    'Rewritten · 1 line',
    'Written · 3 lines',
    '',
  ]);
  expect([moreLines(40), moreLines(1), moreLines(0), moreLines(1800)]).toEqual(['…40 more lines', '…1 more line', '', '…1,800 more lines']);
  expect(['-a', '+b', ' c', '@'].map(diffLine)).toEqual([
    { mark: 'del', text: 'a' },
    { mark: 'add', text: 'b' },
    { mark: 'ctx', text: 'c' },
    { mark: 'gap', text: '' },
  ]);
  // The commands it ran, with what each was for.
  expect(commandsOf({ commands: ['ls src', 'npm test'] }, told)).toEqual([
    { command: 'ls src', about: 'List the source folder' },
    { command: 'npm test', about: '' },
  ]);
  expect(commandsOf(null, told)).toEqual([]);

  // A job's pill goes under the first answer after its question.
  expect(messageIndex('chat:12:4', 12)).toBe(4);
  expect(messageIndex('chat:12:4', 13)).toBe(-1);
  const messages = [{ role: 'user' }, { role: 'assistant' }, { role: 'user', kind: 'jobReport' }, { role: 'assistant' }, { role: 'user' }, { role: 'assistant' }];
  const list = [
    { id: 'a', conversation: 12, messageId: 'chat:12:0', createdTs: 1 },
    { id: 'b', conversation: 12, messageId: 'chat:12:4', createdTs: 2 },
    { id: 'c', conversation: 13, messageId: 'chat:13:0', createdTs: 3 },
  ];
  const pills = pillsByAnswer(list, 12, messages);
  expect([...pills.keys()]).toEqual([1, 5]);
  expect(pills.get(1).map((j) => j.id)).toEqual(['a']);

  // The report card: its job, its title, its folder.
  const reported = { id: 'a', conversation: 12, folder: 'garden-planner', status: 'done', reportMessageId: 'chat:12:2', createdTs: 1 };
  expect(jobOfReport([reported], 'chat:12:2', 12)?.id).toBe('a');
  expect(jobOfReport([reported], 'chat:12:9', 12, 'garden-planner')?.id).toBe('a');
  expect(jobOfReport([reported], 'chat:12:9', 12, '')).toBeNull();
  expect(reportTitle({ status: 'done' })).toBe('Job done');
  expect(reportTitle({ status: 'done', plan: true })).toBe("Job's plan is ready");
  expect(reportTitle({ status: 'done', asks: true })).toBe('Job has a question');
  expect(reportTitle({ status: 'failed' })).toBe("Job didn't finish");
  expect(reportTitle({ status: 'running' }, "The job in garden-planner: it didn't finish.")).toBe("Job didn't finish");
  expect(reportTitle(null, 'The job in garden-planner: it made a plan and changed nothing yet.')).toBe("Job's plan is ready");
  expect(reportFolder("The job in garden-planner: it's done.")).toBe('garden-planner');

  // Problems: one button each, or what to check.
  expect(problemWords({ kind: 'notSignedIn', message: "Claude Code isn't signed in." })).toMatchObject({ action: 'signIn' });
  const limited = problemWords({ kind: 'rateLimited', message: "You've reached your Claude plan's limit.", resetsAt: 1_794_067_200 });
  expect(limited.action).toBeNull();
  expect(limited.check).toMatch(/^It resets at .+\. Press Continue then\.$/);
  expect(problemWords({ kind: 'billing', message: '' }).check).toContain('billing');
  expect(problemWords({ kind: 'paysAnotherWay', message: '' }).check).toContain('Settings → Jobs');
  expect(problemWords(null)).toBeNull();

  // The Mode chip's keys, in Claude Code's order.
  const modes = ['auto', 'manual', 'acceptEdits', 'plan', 'bypassPermissions'].map((mode) => ({ mode }));
  expect(['1', '2', '3', '4', '5', '6'].map((k) => modeForKey(modes, k))).toEqual(['auto', 'manual', 'acceptEdits', 'plan', 'bypassPermissions', null]);
  expect(modeLabel('acceptEdits')).toBe('Accept edits');
  expect(modelLabel('claude-sonnet-5-5')).toBe('Claude Sonnet 5.5');
  expect(modelLabel('claude-opus-5-5')).toBe('Claude Opus 5.5');
  expect(modelLabel('gpt-x')).toBe('gpt-x');
  expect([runningLine(1), runningLine(3)]).toEqual(['1 job running', '3 jobs running']);
});

// ---------------------------------------------------------------- the Mode chip

test('the Mode chip: only with Jobs on, Claude Code’s words, keys 1–5, kept per conversation', async ({ yap, main, fakeLlm, shot }) => {
  await setConfig(yap, main, { jobsEnabled: false });
  await freshChat(main);
  await expect(chip(main)).toHaveCount(0);
  await setConfig(yap, main, { jobsEnabled: true });
  await expect(chip(main)).toBeVisible();
  await expect(chip(main)).toHaveText('Auto');

  // Its menu, worded as Claude Code's.
  const menu = await openMode(main);
  await expect(menu.locator('.mtitle')).toHaveText('Mode');
  const items = menu.getByRole('menuitemradio');
  await expect(items.locator('.mname')).toHaveText(['AutoRecommended', 'Manual', 'Accept edits', 'Plan', 'Bypass permissions']);
  await expect(items.locator('.msays')).toHaveText([
    'Claude handles permission decisions',
    'Always ask before making changes',
    'Automatically accept all file edits',
    'Create a plan before making changes',
    'Accepts all permissions',
  ]);
  await expect(items.locator('.mkey')).toHaveText(['1', '2', '3', '4', '5']);
  await expect(items.nth(0)).toHaveAttribute('aria-checked', 'true');
  await expect(items.nth(0)).toBeFocused();
  await shot(main, '01-mode-menu-light');

  // A new chat: the pick waits for its conversation.
  await main.keyboard.press('2');
  await expect(menu).toHaveCount(0);
  await expect(chip(main)).toHaveText('Manual');
  await expect(chip(main)).toBeFocused();
  fakeLlm.scriptChat([{ content: 'Hello there.' }]);
  await send(main, 'Hello');
  await expect(lastAnswer(main)).toHaveText('Hello there.');
  const first = await shownConversation(main);
  await expect.poll(async () => (await yap.invoke('job_mode_get', { conversation: first })).mode).toBe('manual');

  // ↓ and Enter: the next one; Esc closes without a change.
  await openMode(main);
  await expect(modeMenu(main).getByRole('menuitemradio').nth(1)).toBeFocused();
  await main.keyboard.press('ArrowDown');
  await main.keyboard.press('Enter');
  await expect(chip(main)).toHaveText('Accept edits');
  expect((await yap.invoke('job_mode_get', { conversation: first })).mode).toBe('acceptEdits');
  await openMode(main);
  await main.keyboard.press('Escape');
  await expect(modeMenu(main)).toHaveCount(0);
  await expect(chip(main)).toHaveText('Accept edits');

  // Another chat starts at Settings' default; the first keeps its own.
  await main.getByRole('button', { name: 'New chat' }).click();
  await expect(chip(main)).toHaveText('Auto');
  fakeLlm.scriptChat([{ content: 'Second.' }]);
  await send(main, 'Another one');
  await expect(lastAnswer(main)).toHaveText('Second.');
  const second = await shownConversation(main);
  expect(second).not.toBe(first);
  expect((await yap.invoke('job_mode_get', { conversation: second })).mode).toBe('auto');
  await main.locator(`.convs [data-conv="${first}"]`).click();
  await expect(chip(main)).toHaveText('Accept edits');
  // A new chat's pick never goes to a conversation opened from the list.
  await main.getByRole('button', { name: 'New chat' }).click();
  await expect(chip(main)).toHaveText('Auto');
  await openMode(main);
  await main.keyboard.press('4');
  await expect(chip(main)).toHaveText('Plan');
  await main.locator(`.convs [data-conv="${second}"]`).click();
  await expect(chip(main)).toHaveText('Auto');
  expect((await yap.invoke('job_mode_get', { conversation: second })).mode).toBe('auto');

  // Dark.
  await theme(yap, main, 'dark');
  await openMode(main);
  await shot(main, '02-mode-menu-dark');
  await main.keyboard.press('Escape');
  await theme(yap, main, 'light');
});

test('Bypass permissions asks once, then the chip is amber and switches freely', async ({ yap, main, fakeLlm, shot }) => {
  expect((await yap.invoke('job_mode_get', { conversation: null })).bypassConfirmed).toBe(false);
  await freshChat(main);
  fakeLlm.scriptChat([{ content: 'Ready.' }]);
  await send(main, 'Ready?');
  await expect(lastAnswer(main)).toHaveText('Ready.');
  const conversation = await shownConversation(main);

  // The first time: the warning.
  await openMode(main);
  await main.keyboard.press('5');
  const dialog = main.getByRole('alertdialog', { name: 'Use Bypass permissions?' });
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText('Claude Code can run anything on your PC without asking. Use it only in folders you trust.');
  await dialog.getByRole('button', { name: 'Cancel' }).click();
  await expect(dialog).toHaveCount(0);
  await expect(chip(main)).toHaveText('Auto');
  expect((await yap.invoke('job_mode_get', { conversation })).mode).toBe('auto');

  // OK: confirmed, set, amber.
  await (await openMode(main)).getByRole('menuitemradio', { name: /Bypass permissions/ }).click();
  await dialog.getByRole('button', { name: 'Use Bypass permissions' }).click();
  await expect(chip(main)).toHaveAttribute('data-mode', 'bypassPermissions');
  await expect(chip(main)).toHaveText('Bypass permissions');
  await expect(chip(main)).toHaveClass(/bypass/);
  expect(await yap.invoke('job_mode_get', { conversation })).toMatchObject({ mode: 'bypassPermissions', bypassConfirmed: true });
  const amber = async () =>
    main.evaluate(() => {
      const probe = document.createElement('span');
      probe.style.color = 'var(--yap-warning)';
      document.body.appendChild(probe);
      const want = getComputedStyle(probe).color;
      probe.remove();
      return [getComputedStyle(document.querySelector('[data-testid="chat-mode-chip"] .clabel')).color, want];
    });
  let [got, want] = await amber();
  expect(got).toBe(want);
  await shot(main, '03-mode-bypass-light');
  await theme(yap, main, 'dark');
  [got, want] = await amber();
  expect(got).toBe(want);
  await shot(main, '04-mode-bypass-dark');
  await theme(yap, main, 'light');

  // From now on it switches freely, like Claude Code's.
  await openMode(main);
  await main.keyboard.press('1');
  await expect(chip(main)).toHaveText('Auto');
  await expect(chip(main)).not.toHaveClass(/bypass/);
  await openMode(main);
  await main.keyboard.press('5');
  await expect(main.getByRole('alertdialog')).toHaveCount(0);
  await expect(chip(main)).toHaveAttribute('data-mode', 'bypassPermissions');
  await yap.invoke('job_mode_set', { conversation, mode: 'auto' });
});

// ---------------------------------------------------------------- a job, end to end

test('a job: its tab opens at once and follows it, the pill, the report card, Undo', async ({ yap, main, fakeLlm, shot }) => {
  const REPORT = "Done. I changed notes.md from Draft to Final and the tests pass. There's a backup if you want it back.";
  const started = await startJob(yap, main, fakeLlm, {
    lines: EDIT,
    name: 'edit',
    brief: 'Change Draft to Final in notes.md, then run the tests.',
    report: REPORT,
    ask: 'Start a job in garden-planner: change Draft to Final in notes.md and run the tests.',
  });
  // The tab opened from the turn's own event.
  await expect(tab(main)).toBeVisible();
  await expect(tab(main).locator('.jobtitle')).toHaveText('garden-planner');
  const job = await ended(yap, started.id);
  expect(job.status).toBe('done');

  await expect(tab(main)).toHaveAttribute('data-status', 'done');
  await expect(main.getByTestId('chat-job-status')).toHaveText('Done');
  await expect(main.getByTestId('chat-job-meta')).toContainText('Auto');
  await expect(main.getByTestId('chat-job-meta')).toContainText('Claude Sonnet 5.5 Medium');
  await expect(main.getByTestId('chat-job-elapsed')).toContainText('ran');
  const steps = main.getByTestId('chat-job-steps').locator('.stext');
  for (const s of ['Starting Claude Code', 'Reading notes.md', 'Editing notes.md', 'Running the tests']) await expect(steps.filter({ hasText: s }).first()).toBeVisible();
  // What it said on the way; its final words only once, below.
  await expect(main.getByTestId('chat-job-said')).toHaveText(["I'll update the notes file."]);
  await expect(main.getByTestId('chat-job-now')).toHaveCount(0);
  await expect(main.getByTestId('chat-job-files').locator('.fname')).toHaveText(['notes.md']);
  await expect(main.getByTestId('chat-job-command').locator('.cabout')).toHaveText(['Run the tests']);
  await expect(main.getByTestId('chat-job-command').locator('.cmd')).toHaveText(['npm test']);
  await expect(main.getByTestId('chat-job-denials').locator('li')).toHaveText(['Run a command: rm -rf build']);
  await expect(main.getByTestId('chat-job-text')).toContainText(DONE);
  await expect(main.getByTestId('chat-job-cost')).toHaveText('Your Claude plan · ≈ $0.04 on the API');
  await expect(tab(main).getByRole('button', { name: 'Stop' })).toHaveCount(0);
  await expect(tab(main).getByRole('button', { name: 'Open in terminal' })).toBeVisible();

  // The report: a job card, then the chat model's own words.
  await messagesOf(yap, job.conversation, 4);
  const report = thread(main).getByTestId('chat-job-report');
  await expect(report).toBeVisible();
  await expect(report.locator('.jtitle')).toHaveText('Job done · garden-planner');
  await expect(report.locator('.jline')).toHaveText('Changed notes.md');
  await expect(lastAnswer(main)).toHaveText(REPORT);
  // Never a question bubble.
  await expect(thread(main).locator('.cbubble.user')).toHaveCount(1);
  // The pill under the answer that started it.
  const pill = thread(main).getByTestId('chat-job-pill');
  await expect(pill).toHaveCount(1);
  await expect(pill).toHaveAttribute('data-status', 'done');
  await expect(pill.locator('.jname')).toHaveText('garden-planner');
  await shot(main, '05-job-tab-done-light');
  await theme(yap, main, 'dark');
  await shot(main, '06-job-tab-done-dark');
  await theme(yap, main, 'light');

  // Closing the tab closes only the tab; the pill and the report card open it again.
  await tab(main).getByRole('button', { name: 'Close the panel' }).click();
  await expect(tab(main)).toHaveCount(0);
  await report.getByRole('button', { name: 'Open the job in garden-planner' }).click();
  await expect(tab(main)).toBeVisible();
  await tab(main).getByRole('button', { name: 'Close the panel' }).click();
  await pill.click();
  await expect(tab(main)).toBeVisible();

  // Undo job: asked first, then what was put back.
  await tab(main).getByRole('button', { name: 'Undo job' }).click();
  const confirm = main.getByRole('alertdialog', { name: 'Undo this job?' });
  await expect(confirm).toBeVisible();
  await confirm.getByRole('button', { name: 'Undo job' }).click();
  await expect(main.getByTestId('chat-job-undone')).toHaveText('Undone: notes.md put back.');
  expect(fs.readFileSync(path.join(GARDEN, 'notes.md'), 'utf8')).toBe('Draft notes\n');
  await expect(tab(main).getByRole('button', { name: 'Undo job' })).toHaveCount(0);
  await expect.poll(async () => (await jobs(yap)).find((j) => j.id === job.id).undone).toBe(true);
});

test('a job whose edits made no backup (hooks off): its tab says Undo can’t put them back, and offers no Undo', async ({ yap, main, fakeLlm, shot }) => {
  const started = await startJob(yap, main, fakeLlm, {
    lines: EDIT_NO_HOOK,
    name: 'edit-no-hook',
    brief: 'Change Draft to Final in notes.md, then run the tests.',
    report: "Done. I changed notes.md from Draft to Final; no backup was made, so Undo can't put it back.",
    ask: 'Start a job in garden-planner: change Draft to Final in notes.md and run the tests.',
  });
  const job = await ended(yap, started.id);
  expect(job.status).toBe('done');
  // The edit went through and nothing was backed up: marked.
  expect(fs.readFileSync(path.join(GARDEN, 'notes.md'), 'utf8')).toBe('Final notes\n');
  expect(job.noBackups).toBe(true);
  await expect(tab(main)).toHaveAttribute('data-status', 'done');
  await expect(main.getByTestId('chat-job-files').locator('.fname')).toHaveText(['notes.md']);
  await expect(main.getByTestId('chat-job-no-backups')).toHaveText(NO_BACKUPS);
  await expect(tab(main).getByRole('button', { name: 'Undo job' })).toHaveCount(0);
  // The report's facts say so too (the model reads them), never "backed up".
  const messages = await messagesOf(yap, job.conversation, 4);
  const marker = messages.find((m) => m.kind === 'jobReport');
  expect(marker.text).toContain("Files it changed: notes.md (no backups were made, so Undo can't put them back).");
  expect(marker.text).not.toContain('backed up first');
  await shot(main, '06b-job-tab-no-backups');
  // A job whose hook ran (the one before) is never marked.
  expect((await jobs(yap)).filter((j) => j.id !== job.id && j.files?.length).every((j) => !j.noBackups)).toBe(true);
});

test('a slow job: its live line, the running list, “1 job running” on the bar, Stop and Continue', async ({ yap, main, fakeLlm, shot }) => {
  const started = await startJob(yap, main, fakeLlm, { lines: SLOW, name: 'slow', brief: 'Read notes.md slowly.', report: null });
  await expect(tab(main)).toBeVisible();
  await expect(main.getByTestId('chat-job-now')).toHaveText('Reading notes.md', { timeout: 20_000 });
  await expect(main.getByTestId('chat-job-status')).toHaveText('Running');
  await expect(main.getByTestId('chat-job-elapsed')).toContainText('so far');
  await expect(tab(main).getByRole('button', { name: 'Stop' })).toBeVisible();
  await expect(tab(main).getByRole('button', { name: 'Open in terminal' })).toHaveCount(0);
  await expect(main.getByTestId('chat-job-cost')).toHaveText('Your Claude plan');
  await shot(main, '07-job-tab-running');

  // The Yap bar: "1 job running".
  await expect.poll(async () => (await yap.invoke('bar_status')).jobs).toBe(1);
  const line = yap.overlay.getByTestId('bar-jobs');
  await expect(line).toHaveText('1 job running');
  await shot(yap.overlay, '08-bar-jobs-running');

  // The running list above the conversations (the panel closed: the list is back).
  await tab(main).getByRole('button', { name: 'Close the panel' }).click();
  const row = main.getByTestId('chat-running-job');
  await expect(row).toHaveCount(1);
  await expect(row.locator('.jfolder')).toHaveText('garden-planner');
  await expect(row.locator('.jstep')).toHaveText('Reading notes.md');
  await shot(main, '09-running-list');
  // Its row opens the job.
  await row.click();
  await expect(tab(main)).toBeVisible();

  // The bar's line opens Chat on it, from another chat.
  await tab(main).getByRole('button', { name: 'Close the panel' }).click();
  await main.getByRole('button', { name: 'New chat' }).click();
  await line.click();
  await expect(tab(main)).toBeVisible();
  await expect.poll(() => main.evaluate(() => document.querySelector('.cbubble.user')?.textContent?.trim() ?? '')).toBe('Start a job in garden-planner.');

  // Stop: it stops, nothing reports it; the list and the bar let it go.
  await tab(main).getByRole('button', { name: 'Stop' }).click();
  const stopped = await ended(yap, started.id);
  expect(stopped.status).toBe('stopped');
  await expect(main.getByTestId('chat-job-status')).toHaveText('Stopped');
  await expect(tab(main).getByRole('button', { name: 'Continue' })).toBeVisible();
  await expect(tab(main).getByRole('button', { name: 'Open in terminal' })).toBeVisible();
  await expect.poll(async () => (await yap.invoke('bar_status')).jobs).toBe(0);
  await expect(line).toHaveCount(0);
  await tab(main).getByRole('button', { name: 'Open in terminal' }).click();

  // Continue: the same session carries on.
  await script(yap, ECHO, 'echo');
  fakeLlm.scriptChat([{ plain: true, content: 'It carried on and finished.' }]);
  await tab(main).getByRole('button', { name: 'Continue' }).click();
  const carried = await ended(yap, started.id);
  expect(carried).toMatchObject({ status: 'done', turns: 2, session: stopped.session });
  await expect(main.getByTestId('chat-job-status')).toHaveText('Done');
  await expect(main.getByTestId('chat-job-text')).toContainText('Got: Carry on with the task from where you stopped.');
  await expect(main.getByTestId('chat-running-job')).toHaveCount(0);
  await messagesOf(yap, carried.conversation, 4);
});

test('Tell the job…: the person’s words go to the job, its next turn', async ({ yap, main, fakeLlm }) => {
  const started = await startJob(yap, main, fakeLlm, { lines: ECHO, name: 'echo-tell', brief: 'Tidy notes.md.' });
  const job = await ended(yap, started.id);
  await messagesOf(yap, job.conversation, 4);
  fakeLlm.scriptChat([{ plain: true, content: 'It also sorted the list.' }]);
  const box = tab(main).getByRole('textbox', { name: 'Tell the job' });
  await box.fill('Also sort the list.');
  await box.press('Enter');
  await expect(box).toHaveValue('');
  await expect.poll(async () => (await jobs(yap)).find((j) => j.id === job.id).turns, { timeout: 20_000 }).toBe(2);
  const told = await ended(yap, job.id);
  expect(told.text).toContain('Got: Also sort the list.');
  await expect(main.getByTestId('chat-job-text')).toContainText('Got: Also sort the list.');
  await messagesOf(yap, job.conversation, 6);
});

// ---------------------------------------------------------------- read like Claude Code

test('a job reads like Claude Code: its to-do list, what it says, commands by what they’re for, the live end, diffs', async ({ yap, main, fakeLlm, shot }) => {
  fs.mkdirSync(path.join(GARDEN, 'src'), { recursive: true });
  fs.writeFileSync(path.join(GARDEN, 'src', 'planner.js'), PLANNER);
  // Its step to allow waits for the test (the steps outgrow their box first).
  await yap.invoke('e2e_claude_approve_auto', { allow: null });
  const started = await startJob(yap, main, fakeLlm, { lines: looking(GARDEN), name: 'looking', brief: 'Add a watering schedule to the planner and write a planting guide.', report: null });
  await expect(tab(main)).toBeVisible();
  await expect.poll(async () => (await jobs(yap)).find((j) => j.id === started.id).status, { timeout: 20_000 }).toBe('asking');

  const box = main.getByTestId('chat-job-steps');
  const stepTexts = box.locator('.step .stext');
  const scroll = () => box.evaluate((el) => ({ top: el.scrollTop, gap: el.scrollHeight - el.clientHeight - el.scrollTop, over: el.scrollHeight > el.clientHeight }));
  // Steps by what they're for; a command's cd into the folder gone, its paths relative.
  await expect(stepTexts.filter({ hasText: 'Listing the source folder' })).toHaveCount(1);
  await expect(stepTexts.filter({ hasText: 'Running grep -n "water" src/planner.js | head -5' })).toHaveCount(1);
  await expect(stepTexts.filter({ hasText: 'Reading beds\\bed-24.json' })).toHaveCount(1);
  await expect(main.getByTestId('chat-job-now')).toHaveText('Waiting for you to allow a step');
  // What it said, in markdown, in its place among the steps.
  const said1 = main.getByTestId('chat-job-said').first();
  await expect(said1).toHaveText("I'll start by looking at how the planner stores its beds.");
  await expect(said1.locator('strong')).toHaveText('beds');
  // The live end: the steps outgrew their box, which stays at the newest.
  await expect.poll(async () => (await scroll()).over).toBe(true);
  await expect.poll(async () => (await scroll()).gap).toBeLessThanOrEqual(8);
  await expect(box).toHaveAttribute('data-pinned', 'true');
  await expect(main.getByTestId('chat-job-follow')).toHaveCount(0);
  // Scrolled up to read: "↓" offered, and it never moves as more come in.
  await box.evaluate((el) => el.scrollTo(0, 0));
  await expect(box).toHaveAttribute('data-pinned', 'false');
  await expect(main.getByTestId('chat-job-follow')).toBeVisible();
  const before = await stepTexts.count();
  const prompt = (await yap.invoke('claude_approve_pending')).find((p) => p.run === started.id);
  await yap.invoke('claude_approve_answer', { id: prompt.id, allow: true });
  await expect(stepTexts.filter({ hasText: 'Reading beds\\bed-34.json' })).toHaveCount(1);
  expect(await stepTexts.count()).toBeGreaterThan(before + 9);
  expect((await scroll()).top).toBe(0);
  await expect(main.getByTestId('chat-job-follow')).toBeVisible();
  // "↓": back at the newest, following again.
  await main.getByTestId('chat-job-follow').click();
  await expect.poll(async () => (await scroll()).gap).toBeLessThanOrEqual(8);
  await expect(box).toHaveAttribute('data-pinned', 'true');
  await expect(main.getByTestId('chat-job-follow')).toHaveCount(0);

  // Nothing more specific running: the line is the to-do in progress.
  await expect(main.getByTestId('chat-job-now')).toHaveText('Looking at how beds are stored');
  const list = main.getByTestId('chat-job-todos');
  await expect(list.locator('h3 .count')).toHaveText('0 of 3 done');
  await expect(list.locator('.ttext')).toHaveText(PLAN.map(([c]) => c));
  expect(await list.locator('.todo').evaluateAll((els) => els.map((e) => [e.dataset.status, e.querySelector('.tmark').textContent]))).toEqual([
    ['in_progress', '●'],
    ['pending', '○'],
    ['pending', '○'],
  ]);
  // (Allowed: running again once its next tick sees no prompt waiting.)
  await expect(main.getByTestId('chat-job-status')).toHaveText('Running');
  await main.locator('.jobbody').evaluate((el) => el.scrollTo(0, 0));
  await shot(main, '17-job-running-todos-light');
  await theme(yap, main, 'dark');
  await shot(main, '18-job-running-todos-dark');
  await theme(yap, main, 'light');
  // The running list says the same line.
  await tab(main).getByRole('button', { name: 'Close the panel' }).click();
  await expect(main.getByTestId('chat-running-job').locator('.jstep')).toHaveText('Looking at how beds are stored');
  await main.getByTestId('chat-running-job').click();
  await expect(tab(main)).toBeVisible();

  // Stop, then Continue: its next turn finishes the list.
  await tab(main).getByRole('button', { name: 'Stop' }).click();
  expect((await ended(yap, started.id)).status).toBe('stopped');
  await script(yap, finishing, 'finishing');
  fakeLlm.scriptChat([{ plain: true, content: 'It added the watering schedule and wrote a planting guide.' }]);
  await tab(main).getByRole('button', { name: 'Continue' }).click();
  const job = await ended(yap, started.id);
  expect(job.status).toBe('done');
  await expect(tab(main)).toHaveAttribute('data-status', 'done');
  await expect(main.getByTestId('chat-job-now')).toHaveCount(0);

  // The list ticked off.
  await expect(list.locator('h3 .count')).toHaveText('3 of 3 done');
  expect(await list.locator('.todo .tmark').allTextContents()).toEqual(['✓', '✓', '✓']);
  // What it said, in order, the long one folded; its summary only once, below.
  const said = main.getByTestId('chat-job-said');
  await expect(said).toHaveCount(3);
  await expect(said.nth(2)).toHaveText('Now the planting guide.');
  const long = said.nth(1);
  await expect(long).toHaveClass(/folded/);
  await long.getByRole('button', { name: 'Show more' }).click();
  await expect(long).not.toHaveClass(/folded/);
  await expect(long.getByRole('button', { name: 'Show less' })).toHaveAttribute('aria-expanded', 'true');
  await expect(main.getByTestId('chat-job-text')).toContainText('now takes litres');
  for (const s of ['Editing src\\planner.js', 'Running the tests', 'Writing planting-guide.md', 'Writing notes.md']) await expect(stepTexts.filter({ hasText: s })).toHaveCount(1);

  // Commands run: what each was for above it, the command itself (its cd gone), a few then "Show 1 more".
  const commands = main.getByTestId('chat-job-command');
  await expect(commands).toHaveCount(3);
  await expect(commands.nth(0).locator('.cabout')).toHaveText('List the source folder');
  await expect(commands.nth(0).locator('.cmd')).toHaveText('ls src');
  await expect(commands.nth(1).locator('.cabout')).toHaveCount(0);
  await expect(commands.nth(1).locator('.cmd')).toHaveText(`grep -n "water" "${gitBash(GARDEN)}/src/planner.js" | head -5`);
  await main.getByTestId('chat-job-commands').getByRole('button', { name: 'Show 2 more' }).click();
  await expect(commands.locator('.cabout')).toHaveText(['List the source folder', 'Build the planner', 'Run the tests', 'Check the planner still parses']);
  await expect(commands).toHaveCount(5);

  // Files changed: each with its counts, opening to what changed.
  const file = (f) => main.locator(`[data-testid="chat-job-file"][data-file="${f.replace(/\\/g, '\\\\')}"]`);
  await expect(main.getByTestId('chat-job-files').locator('.fname')).toHaveText(['src\\planner.js', 'planting-guide.md', 'notes.md']);
  await expect(file('src\\planner.js').locator('.counts .plus')).toHaveText('+3');
  await expect(file('src\\planner.js').locator('.counts .minus')).toHaveText('−2');
  await expect(file('planting-guide.md').locator('.counts .minus')).toHaveCount(0);
  await expect(file('planting-guide.md').locator('.tag')).toHaveText('New');
  await file('src\\planner.js').locator('.frow').click();
  const edit = file('src\\planner.js').getByTestId('chat-job-diff');
  await expect(edit).toHaveAttribute('data-kind', 'edit');
  await expect(edit.locator('.dl.del .dt')).toHaveText(['export function water(bed) {', '  bed.water += 1;']);
  await expect(edit.locator('.dl.add .dt')).toHaveText(['export function water(bed, litres = 2) {', '  bed.water += litres;', '  bed.lastWatered = new Date().toISOString();']);
  await expect(edit.locator('.dl.ctx .dt')).toHaveText(['  return bed;', '}']);
  await file('planting-guide.md').locator('.frow').click();
  const made = file('planting-guide.md').getByTestId('chat-job-diff');
  await expect(made.locator('.dtitle')).toHaveText('New file · 5 lines');
  await expect(made.locator('.dl.add .dt').first()).toHaveText('# Planting guide');
  await file('notes.md').locator('.frow').click();
  await expect(file('notes.md').getByTestId('chat-job-diff').locator('.dtitle')).toHaveText('Rewritten · 3 lines');
  // Red and green, in Yap's own colours, light and dark.
  const tints = () =>
    edit.evaluate((el) => {
      const bg = (sel) => getComputedStyle(el.querySelector(sel)).backgroundColor;
      return { del: bg('.dl.del'), add: bg('.dl.add'), ctx: bg('.dl.ctx') };
    });
  for (const which of ['light', 'dark']) {
    if (which === 'dark') await theme(yap, main, 'dark');
    const t = await tints();
    expect(t.del).not.toBe(t.ctx);
    expect(t.add).not.toBe(t.ctx);
    expect(t.del).not.toBe(t.add);
    await file('src\\planner.js').scrollIntoViewIfNeeded();
    await shot(main, `${which === 'light' ? '19' : '20'}-job-done-diffs-${which}`);
  }
  await theme(yap, main, 'light');

  // Replayed from its log: the same, the tab opened afresh.
  await tab(main).getByRole('button', { name: 'Close the panel' }).click();
  await messagesOf(yap, job.conversation, 4);
  await thread(main).getByTestId('chat-job-pill').click();
  await expect(main.getByTestId('chat-job-said')).toHaveCount(3);
  await expect(main.getByTestId('chat-job-todos').locator('h3 .count')).toHaveText('3 of 3 done');
  await file('src\\planner.js').locator('.frow').click();
  await expect(file('src\\planner.js').getByTestId('chat-job-diff').locator('.dl.add')).toHaveCount(3);
  // Its log carries the diffs and the list (what the tab replays).
  const { log } = await yap.invoke('job_get', { id: job.id });
  expect(log.filter((e) => e.diff).map((e) => [e.diff.file, e.diff.kind])).toEqual([
    ['src\\planner.js', 'edit'],
    ['planting-guide.md', 'new'],
    ['notes.md', 'rewrite'],
  ]);
  expect(log.filter((e) => e.todos)).toHaveLength(4);
});

// ---------------------------------------------------------------- asking

test('the ask-first card reads as a job: folder, path, mode, the whole brief, why, Start', async ({ yap, main, fakeLlm, shot }) => {
  freshFolder(RECIPES);
  await yap.invoke('job_folders_remove', { name: 'recipe-box' });
  await script(yap, ECHO, 'echo-confirm');
  const before = (await jobs(yap)).length;
  const BRIEF = 'Tidy notes.md: sort the recipes by course, keep every one of them, and fix the headings so each starts with a capital letter.';
  fakeLlm.scriptChat([
    { tools: [{ name: 'start_job', args: { folder: RECIPES, brief: BRIEF } }] },
    { content: "On it: I've started a job in recipe-box." },
    { plain: true, content: 'It tidied notes.md in recipe-box.' },
  ]);
  await freshChat(main);
  await send(main, `Start a job in ${RECIPES} to tidy notes.md.`);
  const card = thread(main).getByTestId('chat-job-confirm');
  await expect(card).toHaveAttribute('data-state', 'asking');
  await expect(card.locator('.jq')).toHaveText('Yap wants to start a job in ‘recipe-box’ (Auto)');
  await expect(card.locator('.jpath')).toHaveText(RECIPES);
  await expect(card.locator('.jfacts dd')).toHaveText('Auto');
  await expect(card.getByTestId('chat-job-brief')).toHaveText(BRIEF);
  await expect(card.locator('.jwhy')).toHaveText('Jobs haven’t worked in this folder before. Only you can say yes.');
  expect((await jobs(yap)).length).toBe(before);
  await shot(main, '10-job-confirm');
  await card.getByRole('button', { name: 'Start' }).click();
  await expect(card).toHaveAttribute('data-state', 'allowed');
  await expect(card.locator('.jdone')).toHaveText('Started.');
  const job = await ended(yap, (await newJob(yap, before)).id);
  expect(job).toMatchObject({ folder: 'recipe-box', status: 'done' });
  await messagesOf(yap, job.conversation, 4);
});

test('a permission prompt: a card in the thread (Deny) and on the Yap bar (Allow once), never answered by itself', async ({ yap, main, fakeLlm, shot }) => {
  await yap.invoke('e2e_claude_approve_auto', { allow: null });
  let started = await startJob(yap, main, fakeLlm, { lines: EDIT, name: 'edit-ask', brief: 'Change Draft to Final in notes.md, then run the tests.' });
  await expect.poll(async () => (await jobs(yap)).find((j) => j.id === started.id).status, { timeout: 20_000 }).toBe('asking');
  const card = thread(main).getByTestId('chat-job-approve');
  await expect(card).toHaveCount(1);
  await expect(card.locator('.aq')).toContainText('The job in garden-planner asks to:');
  await expect(card.locator('.aq strong')).toHaveText('Run a command: npm test');
  await expect(card.locator('.adetail')).toHaveText('npm test');
  await expect(main.getByTestId('chat-job-status')).toHaveText('Waiting for you');
  await shot(main, '11-approve-card');
  await card.getByRole('button', { name: 'Deny' }).click();
  await expect(card).toHaveCount(0);
  let job = await ended(yap, started.id);
  expect(job.status).toBe('done');
  await messagesOf(yap, job.conversation, 4);

  // In another app: the bar asks too (Allow once from there).
  started = await startJob(yap, main, fakeLlm, { lines: EDIT, name: 'edit-ask-2', brief: 'Change Draft to Final in notes.md, then run the tests.' });
  await inAnotherApp(yap);
  await expect.poll(async () => (await jobs(yap)).find((j) => j.id === started.id).status, { timeout: 20_000 }).toBe('asking');
  await expect.poll(async () => (await yap.invoke('bar_status')).cards.map((c) => c.id), { timeout: 10_000 }).toEqual(['jobApprove']);
  const barCard = (await yap.invoke('bar_status')).cards[0];
  expect(barCard).toMatchObject({ title: 'Allow a step in garden-planner?', body: 'Run a command: npm test', icon: 'job' });
  expect(barCard.primary.label).toBe('Allow once');
  expect(barCard.secondary.label).toBe('Deny');
  await expect(yap.overlay.locator('.card').filter({ hasText: 'Allow a step in garden-planner?' })).toBeVisible();
  await shot(yap.overlay, '12-approve-bar-card');
  // Nobody answered yet: it waits.
  expect((await yap.invoke('claude_approve_pending')).filter((p) => p.run === started.id)).toHaveLength(1);
  await yap.invoke('bar_card_action', { id: 'jobApprove', action: barCard.primary.id });
  job = await ended(yap, started.id);
  expect(job.status).toBe('done');
  await expect.poll(async () => (await yap.invoke('bar_status')).cards.map((c) => c.id)).not.toContain('jobApprove');
  await expect(thread(main).getByTestId('chat-job-approve')).toHaveCount(0);
  await messagesOf(yap, job.conversation, 4);
});

// ---------------------------------------------------------------- problems and paying

test('a job Claude Code turned away: said plainly, with Sign in; the report card says it didn’t finish', async ({ yap, main, fakeLlm, shot }) => {
  const started = await startJob(yap, main, fakeLlm, { lines: SIGNED_OUT, name: 'signed-out', brief: 'Tidy notes.md.', report: "The job didn't finish: Claude Code isn't signed in." });
  const job = await ended(yap, started.id);
  expect(job.status).toBe('failed');
  expect(job.problem?.kind).toBe('notSignedIn');
  const problem = main.getByTestId('chat-job-problem');
  await expect(problem).toHaveAttribute('data-kind', 'notSignedIn');
  // Claude Code's problem in Rust's own words (claude_code/events.rs `Problem::message`).
  await expect(problem.locator('.pline')).toContainText("Claude Code isn't signed in");
  await expect(problem.locator('.pcheck')).toHaveText('Sign in to Claude Code, then press Continue.');
  await expect(problem.getByRole('button', { name: 'Sign in' })).toBeVisible();
  await problem.getByRole('button', { name: 'Sign in' }).click();
  await expect(tab(main).getByRole('button', { name: 'Continue' })).toBeVisible();
  await messagesOf(yap, job.conversation, 4);
  await expect(thread(main).getByTestId('chat-job-report').locator('.jtitle')).toHaveText("Job didn't finish · garden-planner");
  await shot(main, '13-job-problem');
});

test('a job paid by the API key says so, and History tells plan and key apart', async ({ yap, main, fakeLlm, shot }) => {
  const cfg = await yap.invoke('get_config');
  await setConfig(yap, main, { jobsPay: 'key', ppApiKeys: { ...(cfg.ppApiKeys || {}), anthropic: ANTHROPIC_KEY } });
  const started = await startJob(yap, main, fakeLlm, { lines: ECHO_KEY, name: 'echo-key', brief: 'Tidy notes.md.' });
  const job = await ended(yap, started.id);
  expect(job).toMatchObject({ status: 'done', pay: 'key' });
  await expect(main.getByTestId('chat-job-cost')).toHaveText('API key · $0.002');
  await expect(main.getByTestId('chat-job-cost')).toHaveAttribute('data-pay', 'key');
  await shot(main, '14-job-tab-key');
  await messagesOf(yap, job.conversation, 4);
  await setConfig(yap, main, { jobsPay: 'plan', ppApiKeys: cfg.ppApiKeys || {} });

  const dialog = await openSettings(main, 'Jobs');
  const rows = dialog.getByTestId('jobs-history-row');
  await expect(rows.first()).toBeVisible();
  const costs = await dialog.getByTestId('jobs-history-cost').allTextContents();
  expect(costs[0]).toBe('API key · $0.002');
  // The jobs before it ran on the plan: said as the plan's.
  for (const c of costs.slice(1)) expect(c).toMatch(/^Your Claude plan( · ≈ \$[\d.]+ on the API)?( · Undone)?$/);
  await closeSettings(main);
});

// ---------------------------------------------------------------- Settings → Jobs

test('Settings → Jobs: the switch, Claude Code, pay with, defaults, folders, asking, reports, History', async ({ yap, main, shot }) => {
  freshFolder(SHELF);
  await yap.invoke('job_folders_remove', { name: 'book-shelf' });
  // Claude Code here is this build's stand-in (its status probed afresh).
  await script(yap, ECHO, 'echo-settings');
  await setConfig(yap, main, { jobsEnabled: false });
  const dialog = await openSettings(main, 'Jobs');
  await expect(dialog.getByRole('heading', { level: 1, name: 'Jobs' })).toBeVisible();

  // The switch, with its one line.
  const enabled = dialog.locator('[data-setting="jobs-enabled"]');
  await expect(enabled).toContainText('Let Chat and the Voice Assistant hand tasks to your own Claude Code, in folders you choose.');
  await enabled.getByRole('button', { name: 'Jobs' }).click();
  await expect.poll(async () => (await yap.invoke('get_config')).jobsEnabled).toBe(true);

  // Claude Code: this build's stand-in, signed in.
  const cc = dialog.getByTestId('jobs-claude-code');
  await expect(cc).toHaveAttribute('data-state', 'ready');
  await expect(dialog.locator('[data-setting="jobs-claude-code"]')).toContainText('Claude Code 2.1.296 · signed in');

  // Pay with: the plan, or the key (none in Language Models yet).
  const pay = dialog.locator('[data-setting="jobs-pay"]');
  await expect(pay).toContainText("Jobs use your Claude plan's limits");
  await pay.getByRole('tab', { name: 'API key' }).click();
  await expect(pay).toContainText("There's no Claude API key in Language Models yet");
  await expect.poll(async () => (await yap.invoke('get_config')).jobsPay).toBe('key');
  await pay.getByRole('tab', { name: 'Your Claude plan' }).click();
  await expect.poll(async () => (await yap.invoke('get_config')).jobsPay).toBe('plan');

  // Defaults.
  await dialog.getByRole('combobox', { name: 'Model', exact: true }).selectOption('claude-opus-5-5');
  await dialog.getByRole('combobox', { name: 'Effort', exact: true }).selectOption('high');
  await dialog.getByRole('combobox', { name: 'Mode', exact: true }).selectOption('plan');
  await expect.poll(async () => {
    const c = await yap.invoke('get_config');
    return [c.jobsModel, c.jobsEffort, c.jobsMode];
  }).toEqual(['claude-opus-5-5', 'high', 'plan']);

  // Folders: garden-planner; Add folder… (the picker stood in for), a cap, Remove.
  const folder = (name) => dialog.locator(`[data-testid="jobs-folder"][data-name="${name}"]`);
  await expect(folder('garden-planner')).toBeVisible();
  await expect(dialog.locator('[data-setting="jobs-folder"]').filter({ has: main.locator('[data-testid="jobs-folder"][data-name="garden-planner"]') })).toContainText(GARDEN);
  await expect(dialog.getByText("This folder's own Claude settings and MCP servers also run.")).toBeVisible();
  // Test mode opens no dialog: the next pick takes this folder (job_folders_pick).
  await yap.invoke('e2e_job_folder', { path: SHELF });
  await dialog.getByRole('button', { name: 'Add folder…' }).click();
  await expect(folder('book-shelf')).toBeVisible();
  expect((await yap.invoke('job_folders_list')).map((f) => f.name)).toContain('book-shelf');
  await folder('book-shelf').getByRole('combobox', { name: 'Never above, in book-shelf' }).selectOption('manual');
  await expect.poll(async () => (await yap.invoke('job_folders_list')).find((f) => f.name === 'book-shelf')?.cap).toBe('manual');
  // Bypass's warning, in red, only where Bypass can run (once it's confirmed).
  const bypassOk = (await yap.invoke('job_mode_get', { conversation: null })).bypassConfirmed;
  const warnings = dialog.getByTestId('jobs-bypass-warning');
  if (bypassOk) {
    await expect(warnings.first()).toContainText('Claude Code can run anything on your PC without asking.');
    await expect(dialog.locator('[data-setting="jobs-folder"]').filter({ has: main.locator('[data-testid="jobs-folder"][data-name="book-shelf"]') }).getByTestId('jobs-bypass-warning')).toHaveCount(0);
  } else {
    await expect(warnings).toHaveCount(0);
  }

  // Asking and reports.
  await dialog.getByRole('combobox', { name: 'Ask before starting a job' }).selectOption('always');
  await dialog.getByRole('combobox', { name: 'Read job reports aloud' }).selectOption('always');
  await expect.poll(async () => {
    const c = await yap.invoke('get_config');
    return [c.jobsAsk, c.jobsSpeak];
  }).toEqual(['always', 'always']);
  await expect(dialog.locator('[data-setting="jobs-limits"]')).toHaveText('Backups of changed files are kept 7 days. One job at a time per folder, at most 3 at once.');
  await expect(dialog.locator('[data-setting="jobs-backups-hooks"]')).toHaveText(
    "Undo needs Claude Code's hooks, which back up each file before a job changes it. If a policy set by your organisation turns hooks off, no backups are made, and the job's tab says so.",
  );
  await expect(dialog.getByTestId('jobs-connections-note')).toHaveText("Jobs connect to Anthropic through Claude Code itself, so they aren't in Network connections.");
  await shot(main, '15-settings-jobs-light');
  await theme(yap, main, 'dark');
  await shot(main, '16-settings-jobs-dark');
  await theme(yap, main, 'light');

  // Remove.
  await folder('book-shelf').getByRole('button', { name: 'Remove' }).click();
  await expect(folder('book-shelf')).toHaveCount(0);
  expect((await yap.invoke('job_folders_list')).map((f) => f.name)).not.toContain('book-shelf');

  // History: Open goes to Chat on that job.
  const list = await jobs(yap);
  if (list.length) {
    await dialog.getByTestId('jobs-history-row').first().getByRole('button', { name: 'Open' }).click();
    await expect(settingsDialog(main)).toBeHidden();
    await expect(tab(main)).toBeVisible();
    await expect(tab(main).locator('.jobtitle')).toHaveText(list[0].folder);
  } else {
    await closeSettings(main);
  }

  // Network connections says so too.
  await setConfig(yap, main, { jobsModel: 'claude-sonnet-5-5', jobsEffort: 'medium', jobsMode: 'auto', jobsAsk: 'new-or-web', jobsSpeak: 'never' });
  await openSettings(main, 'Privacy');
  await main.evaluate(() => window.dispatchEvent(new CustomEvent('yap-settings-goto', { detail: 'privacy#connections-list' })));
  await expect(settingsDialog(main).getByTestId('connections-jobs-note')).toHaveText("Jobs connect to Anthropic through Claude Code itself, so they aren't in Network connections.");
  await closeSettings(main);
});
