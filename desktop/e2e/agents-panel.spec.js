// The Agent panel (2026-11-08, after savvy-progress's agents panel, MIT,
// github.com/JohnnyVizz/claude-kit): views/chat/AgentPanel.svelte, the face of
// an agent's tab (AgentTab.svelte), with core/agentPanel.js's rows, groups and
// numbers; what each turn used from Claude Code's stream
// (desktop/src-tauri/src/agents/usage.rs: its assistant messages' `usage`, the
// result's sums and `modelUsage`), kept with the turn in agents.json; and the
// thread's to-do card (views/chat/AgentTodoCard.svelte) in a Code chat.
//
// Agents are driven as agents-ui.spec.js drives them: the scripted fake AI
// calls `start_agent`, and Claude Code is this build's own stand-in (`yap.exe
// fake-claude <script>`, `e2e_claude_script`), whose script's messages carry
// TodoWrite calls and each call's usage. No one's real Claude Code, account or
// files: the workspace is in this run's own folder.
//
// - core/agentPanel.js, pure: tokens and time as the panel says them, the
//   context's fill, a row's progress (its to-dos, else its context in grey),
//   its third line, which group a turn is in, the rows, groups and totals.
// - A Code chat's agent, live: the tab opens on the panel; its row in
//   Running with "3/7 · Writing the planting guide", its bar at 3/7, the
//   header's tiles growing with each call; the thread's to-do card live
//   under the pill; a step to allow moves it to Needs you; Allow once, and
//   it finishes: ✓, its totals from Claude Code's result (and its window),
//   kept in agents.json; the card folded to "7 to-dos · all done".
// - A follow-up: Finished · 2, the header adding both turns up; ▸ opens a row
//   in place (its to-dos, latest steps, counts), ↑ ↓ between rows, Enter
//   folds; Open the timeline and Back. Light, dark and narrow.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, setConfig, expectStore } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { RUNS } from './support/yap.js';
import { newCodeChat } from './support/code-chats.js';
import { money } from '../src/lib/core/agents.js';
import {
  fmtTokens,
  fmtClock,
  tokensOf,
  ctxPercent,
  todoCounts,
  progressOf,
  turnCost,
  costWords,
  stateOf,
  rowLine,
  panelRows,
  panelGroups,
  panelTotals,
  statsLine,
  todoSummary,
  shortModel,
  ringOf,
} from '../src/lib/core/agentPanel.js';

const HERE = path.join(RUNS, 'agents-panel-spec');
const SCRIPTS = path.join(HERE, 'scripts');
const GARDEN = path.join(HERE, 'folders', 'garden-planner');

const STARTED = "On it: I've sent an agent to garden-planner.";
const PASSED = "On it: I've passed that to the agent in garden-planner.";
const DONE = 'Done. The planner waters by the litre, has a minimap button and a planting guide, and the tests pass.';

// ---- the stand-in's script ----

const init = () => ({
  type: 'system',
  subtype: 'init',
  cwd: '{{cwd}}',
  session_id: '{{session_id}}',
  tools: ['Bash', 'Edit', 'Read', 'Write', 'TodoWrite'],
  mcp_servers: [{ name: 'yap', status: 'connected' }],
  model: 'claude-sonnet-5-5',
  permissionMode: 'auto',
  apiKeySource: 'none',
  claude_code_version: '2.1.296',
  capabilities: ['interrupt_receipt_v1'],
});
/** A model call's message, with what it used. */
const said = (id, content, usage, stop = 'tool_use') => ({
  type: 'assistant',
  message: { id, type: 'message', role: 'assistant', model: 'claude-sonnet-5-5', content, stop_reason: stop, usage },
  parent_tool_use_id: null,
  session_id: '{{session_id}}',
});
const toolResult = (id, text) => ({
  type: 'user',
  message: { role: 'user', content: [{ type: 'tool_result', tool_use_id: id, content: text }] },
  parent_tool_use_id: null,
  session_id: '{{session_id}}',
});
const use = (input, write, read, output) => ({ input_tokens: input, cache_creation_input_tokens: write, cache_read_input_tokens: read, output_tokens: output });
const result = (text, cost, usage) => ({
  type: 'result',
  subtype: 'success',
  is_error: false,
  duration_ms: 1200,
  num_turns: 6,
  result: text,
  session_id: '{{session_id}}',
  total_cost_usd: cost,
  usage,
  modelUsage: { 'claude-sonnet-5-5': { inputTokens: usage.input_tokens, outputTokens: usage.output_tokens, contextWindow: 1_000_000 } },
  permission_denials: [],
  terminal_reason: 'completed',
});

const PLAN = [
  ['Look at how beds are stored', 'Looking at how beds are stored'],
  ['Add a watering schedule', 'Adding a watering schedule'],
  ['Add a minimap button', 'Adding a minimap button'],
  ['Write the planting guide', 'Writing the planting guide'],
  ['Update notes.md', 'Updating notes.md'],
  ['Run the tests', 'Running the tests'],
  ['Tidy up', 'Tidying up'],
];
/** The to-do list with the first `done` ticked and the next in progress. */
const todos = (done) => PLAN.map(([content, activeForm], i) => ({ content, status: i < done ? 'completed' : i === done ? 'in_progress' : 'pending', activeForm }));
const todoWrite = (id, done) => ({ type: 'tool_use', id, name: 'TodoWrite', input: { todos: todos(done) } });

// Each call's usage: what it read new, wrote into the cache, read from it, and wrote.
const U = [use(2000, 8000, 0, 150), use(300, 500, 10_000, 120), use(200, 400, 10_500, 180), use(100, 0, 11_300, 90), use(50, 300, 11_400, 200), use(20, 0, 11_700, 160)];
const SUM = U.reduce((s, u) => ({
  input_tokens: s.input_tokens + u.input_tokens,
  cache_creation_input_tokens: s.cache_creation_input_tokens + u.cache_creation_input_tokens,
  cache_read_input_tokens: s.cache_read_input_tokens + u.cache_read_input_tokens,
  output_tokens: s.output_tokens + u.output_tokens,
}));
/** `t` as a pattern that matches it word for word. */
const esc = (t) => t.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
/** The context after a call: its prompt and answer. */
const ctxOf = (u) => u.input_tokens + u.cache_creation_input_tokens + u.cache_read_input_tokens + u.output_tokens;

/** Its first turn: a 7-item plan, three done, a pause, a step to allow, the rest, done. */
const BUILD = [
  { _wait_user: true },
  init(),
  said('p1', [{ type: 'text', text: "I'll plan it out first." }, todoWrite('toolu_todo0', 0)], U[0]),
  toolResult('toolu_todo0', 'Todos have been modified successfully.'),
  said('p2', [{ type: 'tool_use', id: 'toolu_read', name: 'Read', input: { file_path: '{{cwd}}\\beds.json' } }], U[1]),
  toolResult('toolu_read', '{"beds":[]}'),
  said('p3', [todoWrite('toolu_todo3', 3)], U[2]),
  toolResult('toolu_todo3', 'Todos have been modified successfully.'),
  { _sleep_ms: 12000 },
  said('p4', [{ type: 'tool_use', id: 'toolu_bash', name: 'Bash', input: { command: 'npm test', description: 'Run the tests' } }], U[3]),
  { _mcp: { tool: 'approve', arguments: { tool_name: 'Bash', input: { command: 'npm test' }, tool_use_id: 'toolu_bash' } } },
  toolResult('toolu_bash', '12 passing'),
  said('p5', [todoWrite('toolu_todo7', 7)], U[4]),
  toolResult('toolu_todo7', 'Todos have been modified successfully.'),
  said('p6', [{ type: 'text', text: DONE }], U[5], 'end_turn'),
  result(DONE, 0.1234, SUM),
  { _wait_user: true },
];
/** Its next turn: says what it was told, with no to-do list. */
const ECHO_USE = use(40, 0, 12_000, 60);
const ECHO = [
  { _wait_user: true },
  init(),
  said('e1', [{ type: 'text', text: 'Got: {{user_text}}' }], ECHO_USE, 'end_turn'),
  result('Got: {{user_text}}', 0.002, ECHO_USE),
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
      await use({ name: 'agents-panel', config: { memoryLearnChats: false } });
    },
    { scope: 'worker' },
  ],
});

const chatScope = (fakeLlm) => ({
  chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat', apiKey: '', apiKeys: {}, prompt: '', disableThinking: false },
});
const thread = (main) => main.locator('.msgs');
const agents = (yap) => yap.invoke('agent_list');
const script = (yap, lines, name) => yap.invoke('e2e_claude_script', { script: writeScript(name, lines) });
const tab = (main) => main.getByTestId('chat-agent');
const panel = (main) => main.getByTestId('chat-agent-panel');
const group = (main, id) => panel(main).locator(`[data-testid="chat-agent-group"][data-group="${id}"]`);
const row = (main, n) => panel(main).locator(`[data-testid="chat-agent-row"][data-turn="${n}"]`);
const card = (main, n = 1) => thread(main).locator(`[data-testid="chat-agent-todo-card"][data-turn="${n}"]`);

async function send(main, text) {
  await main.getByRole('textbox', { name: 'Message' }).fill(text);
  await main.getByRole('button', { name: 'Send', exact: true }).click();
}

async function ended(yap, id, timeout = 40_000) {
  await expect
    .poll(async () => (await agents(yap)).find((j) => j.id === id)?.status, { timeout, message: `agent ${id} to end` })
    .toMatch(/^(done|failed|stopped)$/);
  return (await agents(yap)).find((j) => j.id === id);
}

async function messagesOf(yap, id, n) {
  await expect.poll(async () => (await yap.invoke('chat_get', { id })).messages.length, { timeout: 30_000, message: `${n} messages` }).toBe(n);
  return (await yap.invoke('chat_get', { id })).messages;
}

async function theme(yap, main, which) {
  await setConfig(yap, main, { theme: which });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe(which);
}

test.beforeEach(async ({ yap, main, fakeLlm }) => {
  freshFolder(GARDEN);
  await yap.invoke('e2e_claude_approve_auto', { allow: true });
  await setConfig(yap, main, {
    agentsEnabled: true,
    agentsAsk: 'never',
    agentsSpeak: 'never',
    agentsPay: 'plan',
    agentsMode: 'auto',
    webSearch: false,
    speakReplies: false,
    llmScopes: chatScope(fakeLlm),
  });
  if (!(await yap.invoke('workspaces_list')).some((f) => f.name === 'garden-planner')) {
    await yap.invoke('workspaces_add', { path: GARDEN });
  }
});

// ---------------------------------------------------------------- no app

test('core/agentPanel.js: tokens and time, the context, progress, the third line, groups and totals', () => {
  // As savvy-progress says them.
  expect([fmtTokens(0), fmtTokens(980), fmtTokens(11_280), fmtTokens(999_600), fmtTokens(1_234_567)]).toEqual(['0', '980', '11k', '1.0M', '1.2M']);
  expect([fmtClock(0), fmtClock(42), fmtClock(242), fmtClock(3909)]).toEqual(['0:00', '0:42', '4:02', '1:05:09']);
  expect(shortModel('claude-sonnet-5-5')).toBe('Sonnet 5.5');
  // The context: its latest call against the window; not known, none.
  const usage = { input: 100, output: 20, cacheRead: 3000, cacheWrite: 80, context: 340_000, window: 1_000_000 };
  expect(tokensOf(usage)).toBe(3200);
  expect(tokensOf(null)).toBe(0);
  expect(ctxPercent(usage)).toBe(34);
  expect(ctxPercent({ context: 5, window: 0 })).toBeNull();
  expect(ctxPercent({ context: 2_000_000, window: 1_000_000 })).toBe(100);
  // To-dos: done of all, the one in progress (its activeForm while it works).
  const list = todos(3);
  expect(todoCounts(list)).toEqual({ done: 3, total: 7, current: 'Write the planting guide' });
  expect(todoCounts(list, true).current).toBe('Writing the planting guide');
  // The bar: its to-dos; no list: done full, else its context in grey.
  expect(progressOf({ status: 'running' }, list)).toEqual({ value: 3 / 7, kind: 'todos' });
  expect(progressOf({ status: 'done' }, [])).toEqual({ value: 1, kind: 'done' });
  expect(progressOf({ status: 'running', usage }, [])).toEqual({ value: 0.34, kind: 'context' });
  expect(progressOf({ status: 'running' }, [])).toEqual({ value: 0, kind: 'context' });
  // Cost: Claude Code's figure and what it hasn't priced yet; told apart by how it's paid.
  expect(turnCost({ cost: 0.1, usage: { estimate: 0.02 } })).toBeCloseTo(0.12);
  expect(costWords(0.04)).toBe(`≈ ${money(0.04)}`);
  expect(costWords(0.04, { pay: 'key' })).toBe(money(0.04));
  expect(costWords(0.04, { pay: 'key', estimated: true })).toBe(`≈ ${money(0.04)}`);
  expect(costWords(0)).toBe('');
  // The ring: how the turn stands.
  expect(['running', 'asking', 'done', 'failed', 'stopped', 'interrupted'].map((s) => ringOf(s))).toEqual(['running', 'asking', 'done', 'failed', 'stopped', 'stopped']);
  expect(ringOf('queued', false)).toBe('queued');

  // Which group, and the third line.
  const now = 2_000_000_000;
  const agent = { id: 'a1', status: 'running', folder: 'garden-planner', pay: 'plan', model: 'claude-sonnet-5-5', effort: 'medium', mode: 'auto', createdTs: 1 };
  const t1 = { n: 1, title: 'Build the planner', status: 'done', startedTs: now - 600, endedTs: now - 360, cost: 0.12, todos: todos(7), usage: { ...usage, context: 11_880 } };
  const t2 = { n: 2, title: 'Add a minimap', status: 'running', startedTs: now - 62, cost: 0, todos: todos(3), usage: { ...usage, estimate: 0.047 } };
  const t3 = { n: 3, title: 'And a legend', request: 'And a legend.', status: 'queued', startedTs: 0 };
  const running = { ...agent, turnList: [t1, t2, t3] };
  expect([stateOf(running, t1, now), stateOf(running, t2, now), stateOf(running, t3, now)]).toEqual(['finished', 'running', 'queued']);
  expect(stateOf(running, { ...t2, status: 'asking' }, now)).toBe('needs');
  expect(rowLine(running, t2, { todos: t2.todos, line: 'Writing the planting guide' })).toBe('3/7 · Writing the planting guide');
  expect(rowLine(running, { ...t2, status: 'asking' }, { todos: t2.todos, line: 'Waiting for you to allow a step' })).toBe('3/7 · Waiting for you to allow a step');
  expect(rowLine(running, t1, { todos: t1.todos })).toBe('7/7 · All done');
  expect(rowLine(running, { ...t2, todos: [] }, { line: 'Reading beds.json' })).toBe('Reading beds.json');
  expect(rowLine(running, { ...t1, status: 'failed', error: 'Claude Code stopped before it finished.' }, {})).toBe("Didn't finish · Claude Code stopped before it finished.");
  expect(rowLine(running, { ...t1, status: 'stopped' }, {})).toBe('Stopped');
  expect(rowLine(running, t3, { state: 'queued' })).toBe('Goes to the agent once it finishes this step.');
  // A plan waiting for its go-ahead, or a question, on its latest turn: needs you (a day at most).
  const planned = { ...agent, status: 'done', plan: true, turnList: [{ ...t1, plan: true }] };
  expect(stateOf(planned, planned.turnList[0], now)).toBe('needs');
  expect(rowLine(planned, { ...planned.turnList[0], todos: [] }, { state: 'needs' })).toBe('Its plan is ready');
  expect(stateOf(planned, planned.turnList[0], now + 2 * 86_400)).toBe('finished');
  const asked = { ...agent, status: 'done', turnList: [{ ...t1, asks: true }] };
  expect(stateOf(asked, asked.turnList[0], now)).toBe('needs');

  // The rows, their groups (newest first) and the header's totals.
  const rows = panelRows([running], { steps: { a1: 'Writing the planting guide' }, now });
  expect(rows.map((r) => [r.n, r.state, r.line])).toEqual([
    [1, 'finished', '7/7 · All done'],
    [2, 'running', '3/7 · Writing the planting guide'],
    [3, 'queued', 'Goes to the agent once it finishes this step.'],
  ]);
  expect(rows[1]).toMatchObject({ modeWord: 'Auto', modeTone: 'auto', modelWord: 'Sonnet 5.5', effortWord: 'Medium', icon: 'claude', ring: 'running', ctx: 34, secs: 62, estimated: true });
  expect(statsLine(rows[1])).toBe(`ctx 34% · 340k · ≈ ${money(0.047)} · 1:02`);
  expect(statsLine(rows[0])).toBe(`ctx 1% · 12k · ≈ ${money(0.12)} · 4:00`);
  expect(statsLine(rows[2])).toBe('');
  expect(panelGroups(rows).map((g) => [g.id, g.label, g.rows.map((r) => r.n)])).toEqual([
    ['running', 'Running', [2]],
    ['finished', 'Finished', [1]],
    ['queued', 'Queued', [3]],
  ]);
  const totals = panelTotals(rows);
  expect(totals).toMatchObject({ approx: true, known: true, tokens: 6400, secs: 240 + 62 });
  expect(totals.cost).toBeCloseTo(0.167);
  // An older agent (no turns kept): one row from the agent, no usage said.
  const older = panelRows([{ id: 'b2', status: 'done', folder: 'garden-planner', brief: 'Tidy notes.md.', cost: 0.01, startedTs: now - 30, endedTs: now }], { now });
  expect(older).toHaveLength(1);
  expect(older[0]).toMatchObject({ title: 'Tidy notes.md', state: 'finished', tokens: 0, ctx: null, line: 'Done' });
  expect(panelTotals(older).known).toBe(false);
  // The thread's card, folded.
  expect([todoSummary(todos(7)), todoSummary(todos(5)), todoSummary(todos(0)), todoSummary(todos(7).slice(0, 1))]).toEqual([
    '7 to-dos · all done',
    '7 to-dos · 5 done',
    '7 to-dos · none done yet',
    '1 to-do · all done',
  ]);
});

// ---------------------------------------------------------------- the panel, live

test('the Agent panel: a Code chat’s agent live, needing you, finished; the to-do card; ▸, Open, keys; light, dark, narrow', async ({ yap, main, fakeLlm, shot }) => {
  await yap.invoke('e2e_claude_approve_auto', { allow: null });
  await script(yap, BUILD, 'build');
  await openView(main, 'Chat');
  if (await tab(main).count()) await tab(main).getByRole('button', { name: 'Close the panel' }).click();
  await newCodeChat(main, 'garden-planner');
  const before = (await agents(yap)).length;
  fakeLlm.scriptChat([
    { tools: [{ name: 'start_agent', args: { brief: 'Build the planner: watering by the litre, a minimap button, a planting guide.', title: 'Build the planner' } }] },
    { content: STARTED },
    { plain: true, content: 'Done: the planner is built and the tests pass.' },
  ]);
  await send(main, 'Build the planner: watering by the litre, a minimap button and a planting guide.');
  await expect.poll(async () => (await agents(yap)).length, { timeout: 20_000 }).toBe(before + 1);
  const id = (await agents(yap))[0].id;

  // The tab opens on the panel: the agent's Mode, model, effort and path, its tiles.
  await expect(panel(main)).toBeVisible();
  await expect(main.getByTestId('chat-agent-panel-meta')).toHaveText(/Auto\s*· Claude Sonnet 5\.5\s*· Medium/);
  // Running: its row with "3/7 · the to-do in progress", its bar at 3/7, opened (its turn's own start).
  await expect(group(main, 'running')).toBeVisible({ timeout: 20_000 });
  await expect(group(main, 'running').getByTestId('chat-agent-group-head')).toHaveText('Running · 1');
  await expect(row(main, 1).getByTestId('chat-agent-row-line')).toHaveText('3/7 · Writing the planting guide', { timeout: 20_000 });
  await expect(row(main, 1)).toHaveAttribute('data-state', 'running');
  await expect(row(main, 1).getByTestId('chat-agent-row-title')).toHaveText('Build the planner');
  await expect(row(main, 1).getByTestId('chat-agent-row-sub')).toHaveText('Auto · Sonnet 5.5 · Medium');
  await expect(row(main, 1).getByTestId('chat-agent-row-bar')).toHaveAttribute('data-kind', 'todos');
  await expect(row(main, 1).getByTestId('chat-agent-row-bar')).toHaveAttribute('data-value', String(Math.round((3 / 7) * 100)));
  await expect(row(main, 1).getByTestId('chat-agent-row-avatar')).toHaveAttribute('data-ring', 'running');
  await expect(row(main, 1).getByTestId('chat-agent-row-avatar')).toHaveAttribute('data-icon', 'claude');
  // Its stats: the context's fill and tokens after its third call, an estimate, the time.
  await expect(row(main, 1).getByTestId('chat-agent-row-stats')).toHaveText(new RegExp(`^ctx \\d+% · ${fmtTokens(ctxOf(U[2]))} · ≈ \\$[0-9.]+ · \\d+:\\d\\d$`));
  // The header's tiles: what the calls so far used.
  const soFar = U.slice(0, 3).reduce((s, u) => s + ctxOf(u), 0);
  await expect(main.getByTestId('chat-agent-total-tokens').locator('.tv')).toHaveText(fmtTokens(soFar));
  await expect(main.getByTestId('chat-agent-total-cost').locator('.tv')).toHaveText(/^≈ \$/);
  await expect(main.getByTestId('chat-agent-total-cost').locator('.tc')).toHaveText('on the API');
  await expect(main.getByTestId('chat-agent-total-time').locator('.tv')).toHaveText(/^\d+:\d\d$/);
  // Opened in place: its to-dos as they stand.
  await expect(row(main, 1)).toHaveAttribute('data-open', 'true');
  await expect(row(main, 1).getByTestId('chat-agent-row-todos').locator('.todo')).toHaveCount(7);
  await expect(row(main, 1).getByTestId('chat-agent-row-todos').locator('.todo[data-status="completed"]')).toHaveCount(3);

  // The thread's to-do card under the pill, live.
  const live = card(main);
  await expect(live).toBeVisible();
  await expect(live).toHaveAttribute('data-live', 'true');
  await expect(live).toHaveAttribute('data-open', 'true');
  await expect(live.locator('.item')).toHaveCount(7);
  await expect(live.locator('.item[data-status="completed"]')).toHaveCount(3);
  await expect(live.locator('.item[data-status="in_progress"] .text')).toHaveText('Write the planting guide');
  await expect(live.getByTestId('chat-agent-todo-now')).toHaveText('Writing the planting guide');
  await expect(live.locator('.ccount')).toHaveText('3 of 7 done');
  await shot(main, '01-panel-running-light');
  await theme(yap, main, 'dark');
  await shot(main, '02-panel-running-dark');
  await theme(yap, main, 'light');

  // A step to allow: Needs you, amber.
  await expect.poll(async () => (await agents(yap)).find((j) => j.id === id).status, { timeout: 30_000 }).toBe('asking');
  await expect(group(main, 'needs').getByTestId('chat-agent-group-head')).toHaveText('Needs you · 1');
  await expect(group(main, 'running')).toHaveCount(0);
  await expect(row(main, 1).getByTestId('chat-agent-row-line')).toHaveText('3/7 · Waiting for you to allow a step');
  await expect(row(main, 1).getByTestId('chat-agent-row-avatar')).toHaveAttribute('data-ring', 'asking');
  await shot(main, '03-panel-needs-you');
  await thread(main).getByTestId('chat-agent-approve').getByRole('button', { name: 'Allow once' }).click();

  // Finished: ✓, and its totals as Claude Code's result says them.
  const agent = await ended(yap, id);
  expect(agent.status).toBe('done');
  await expect(group(main, 'finished').getByTestId('chat-agent-group-head')).toHaveText('Finished · 1');
  await expect(row(main, 1).getByTestId('chat-agent-row-avatar')).toHaveAttribute('data-ring', 'done');
  await expect(row(main, 1).locator('.badge')).toHaveText('✓');
  await expect(row(main, 1).getByTestId('chat-agent-row-line')).toHaveText('7/7 · All done');
  await expect(row(main, 1).getByTestId('chat-agent-row-bar')).toHaveAttribute('data-value', '100');
  const total = SUM.input_tokens + SUM.output_tokens + SUM.cache_read_input_tokens + SUM.cache_creation_input_tokens;
  await expect(main.getByTestId('chat-agent-total-tokens').locator('.tv')).toHaveText(fmtTokens(total));
  await expect(main.getByTestId('chat-agent-total-cost').locator('.tv')).toHaveText(`≈ ${money(0.1234)}`);
  // The window Claude Code said (1M): its latest call's prompt and answer against it.
  await expect(row(main, 1).getByTestId('chat-agent-row-stats')).toHaveText(new RegExp(`^ctx 1% · ${fmtTokens(ctxOf(U[5]))} · ${esc(`≈ ${money(0.1234)}`)} · \\d+:\\d\\d$`));
  // Kept with the turn (a restart shows the same): its usage and its to-do list in agents.json.
  await expectStore(yap, 'agents.json', (j) => {
    const t = j.agents.find((a) => a.id === id)?.turnList?.[0];
    return (
      !!t &&
      t.usage?.input === SUM.input_tokens &&
      t.usage?.cacheRead === SUM.cache_read_input_tokens &&
      t.usage?.context === ctxOf(U[5]) &&
      t.usage?.window === 1_000_000 &&
      !t.usage?.estimate &&
      t.todos?.length === 7 &&
      t.todos.every((x) => x.status === 'completed')
    );
  });
  // The card stays as its record, folded.
  const messages = await messagesOf(yap, agent.conversation, 4);
  expect(messages.some((m) => m.kind === 'agentReport')).toBe(true);
  await expect(card(main)).toHaveAttribute('data-live', 'false');
  await expect(card(main)).toHaveAttribute('data-open', 'false');
  await expect(card(main).getByTestId('chat-agent-todo-summary')).toHaveText('7 to-dos · all done');
  await card(main).getByTestId('chat-agent-todo-head').click();
  await expect(card(main)).toHaveAttribute('data-open', 'true');
  await expect(card(main).locator('.item[data-status="completed"]')).toHaveCount(7);
  await card(main).getByTestId('chat-agent-todo-head').click();
  await expect(card(main)).toHaveAttribute('data-open', 'false');

  // A follow-up: the agent's next turn, a row of its own.
  await script(yap, ECHO, 'echo');
  fakeLlm.scriptChat([
    { tools: [{ name: 'start_agent', args: { brief: 'Add a legend to the minimap.', title: 'Add a legend' } }] },
    { content: PASSED },
    { plain: true, content: 'Done: the minimap has a legend.' },
  ]);
  await send(main, 'could you add a legend to the minimap as well?');
  await expect.poll(async () => (await agents(yap)).find((j) => j.id === id).turnList?.length, { timeout: 20_000 }).toBe(2);
  const both = await ended(yap, id);
  await expect.poll(async () => (await agents(yap)).find((j) => j.id === id).turnList[1].status, { timeout: 30_000 }).toBe('done');
  await messagesOf(yap, both.conversation, 8);
  await expect(group(main, 'finished').getByTestId('chat-agent-group-head')).toHaveText('Finished · 2');
  // Newest first; no list: "Done" and a full bar.
  await expect(group(main, 'finished').getByTestId('chat-agent-row')).toHaveCount(2);
  await expect(group(main, 'finished').getByTestId('chat-agent-row').first()).toHaveAttribute('data-turn', '2');
  await expect(row(main, 2).getByTestId('chat-agent-row-line')).toHaveText('Done');
  await expect(row(main, 2).getByTestId('chat-agent-row-bar')).toHaveAttribute('data-kind', 'done');
  // The header adds both turns up.
  await expect(main.getByTestId('chat-agent-total-cost').locator('.tv')).toHaveText(`≈ ${money(0.1234 + 0.002)}`);
  await expect(main.getByTestId('chat-agent-total-tokens').locator('.tv')).toHaveText(fmtTokens(total + ctxOf(ECHO_USE)));
  // No to-do list in the second turn: no card for it.
  await expect(card(main, 2)).toHaveCount(0);

  // The keys: ↑ ↓ between the rows' heads, Enter folds and opens.
  const head2 = row(main, 2).locator('.rhead');
  const head1 = row(main, 1).locator('.rhead');
  await head2.focus();
  await main.keyboard.press('ArrowDown');
  await expect(head1).toBeFocused();
  await main.keyboard.press('ArrowUp');
  await expect(head2).toBeFocused();
  const was = await row(main, 2).getAttribute('data-open');
  await main.keyboard.press('Enter');
  await expect(row(main, 2)).toHaveAttribute('data-open', was === 'true' ? 'false' : 'true');
  await main.keyboard.press('Space');
  await expect(row(main, 2)).toHaveAttribute('data-open', String(was === 'true'));
  // ▸ opens a row in place: its to-dos, its latest steps, how many commands and files.
  if ((await row(main, 1).getAttribute('data-open')) !== 'true') await head1.click();
  const more = row(main, 1).getByTestId('chat-agent-row-more');
  await expect(more.getByTestId('chat-agent-row-todos').locator('.todo')).toHaveCount(7);
  await expect(more.getByTestId('chat-agent-row-steps').locator('li')).toContainText(['Reading beds.json', 'Running the tests']);
  await expect(more.getByTestId('chat-agent-row-counts')).toHaveText('1 command · 0 files changed');
  // A group folds.
  await group(main, 'finished').getByTestId('chat-agent-group-head').click();
  await expect(group(main, 'finished').getByTestId('chat-agent-row')).toHaveCount(0);
  await group(main, 'finished').getByTestId('chat-agent-group-head').click();
  await expect(group(main, 'finished').getByTestId('chat-agent-row')).toHaveCount(2);
  await shot(main, '04-panel-finished-light');
  await theme(yap, main, 'dark');
  await shot(main, '05-panel-finished-dark');
  await theme(yap, main, 'light');

  // Open: the turn's whole timeline (the tab's other view), and Back.
  await more.getByTestId('chat-agent-row-open').click();
  await expect(panel(main)).toHaveCount(0);
  const turn1 = tab(main).locator('[data-testid="chat-agent-turn"][data-turn="1"]');
  await expect(turn1).toHaveAttribute('data-open', 'true');
  await expect(turn1.getByTestId('chat-agent-todos')).toBeVisible();
  await expect(main.getByTestId('chat-agent-cost')).toHaveText(`Your Claude plan · ≈ ${money(0.1234 + 0.002)} on the API in all`);
  await main.getByTestId('chat-agent-back').click();
  await expect(panel(main)).toBeVisible();

  // A pill opens the panel at its turn: that row open, in view.
  await tab(main).getByRole('button', { name: 'Close the panel' }).click();
  await thread(main).locator('[data-testid="chat-agent-pill"][data-turn="2"]').click();
  await expect(panel(main)).toBeVisible();
  await expect(row(main, 2)).toHaveAttribute('data-open', 'true');

  // Narrow: the rows still read, the stats under the line where they must.
  const size = main.viewportSize();
  await main.setViewportSize({ width: 860, height: 760 });
  await expect(panel(main)).toBeVisible();
  await expect(row(main, 1).getByTestId('chat-agent-row-line')).toBeVisible();
  await shot(main, '06-panel-narrow-light');
  await theme(yap, main, 'dark');
  await shot(main, '07-panel-narrow-dark');
  await theme(yap, main, 'light');
  if (size) await main.setViewportSize(size);
  await yap.invoke('e2e_claude_approve_auto', { allow: true });
});
