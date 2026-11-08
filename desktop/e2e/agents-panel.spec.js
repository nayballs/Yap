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
// - **The crew** (2026-11-08: Yap's own pixel characters, one a model family,
//   core/crew.js and views/chat/crew/; the idea of a character a row after
//   savvy-progress's panel): who a model is, the props, the markup; a
//   seeded Code chat (`e2e_agents_seed`) with a row in each Mode and a
//   Haiku, a Sonnet, an Opus and a Fable working, needing you, done,
//   stopped and failed — each prop and pose on its avatar, rows folded with
//   ▸ opening one (remembered), the walk running on screen, paused off
//   screen and out of view, still under reduced motion; screenshots light
//   and dark, the avatars at 2×, and the whole crew's sheet (a test-only
//   overlay drawn with the very markup the panel uses).
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { test as base, expect, openView, setConfig, expectStore } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { RUNS, SCREENSHOTS } from './support/yap.js';
import { newCodeChat } from './support/code-chats.js';
import { money } from '../src/lib/core/agents.js';
import { characterFor, propOf, crewLabel, crewName, CREW } from '../src/lib/core/crew.js';
import { crewMarkup, crewSvg, CHARACTER_IDS, PROP_IDS } from '../src/lib/views/chat/crew/sprites.js';
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
  poseOf,
} from '../src/lib/core/agentPanel.js';

const CREW_CSS = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', 'src', 'lib', 'views', 'chat', 'crew', 'crew.css');

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
  // The character's pose: how the turn stands.
  expect(['running', 'asking', 'done', 'failed', 'stopped', 'interrupted'].map((s) => poseOf(s))).toEqual(['working', 'needs', 'done', 'failed', 'stopped', 'stopped']);
  expect(poseOf('queued', false)).toBe('queued');
  expect(poseOf('queued', true)).toBe('working');

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
  expect(rows[1]).toMatchObject({ modeWord: 'Auto', modeTone: 'auto', modelWord: 'Sonnet 5.5', effortWord: 'Medium', pose: 'working', ctx: 34, secs: 62, estimated: true });
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

test('the crew: who a model is, the Modes’ props, an avatar’s markup and label', () => {
  // The Claude models by tier, Sonnet when the id names none (the panel's rows: Claude Code's models).
  expect(['claude-haiku-5-5', 'claude-sonnet-5-5', 'claude-opus-5-5', 'claude-fable-5', 'claude-next', ''].map((m) => characterFor('claude-plan', m))).toEqual([
    'haiku',
    'sonnet',
    'opus',
    'fable',
    'sonnet',
    'sonnet',
  ]);
  expect(characterFor('openrouter', 'anthropic/claude-opus-5')).toBe('opus');
  // Every other family by its words, else its provider's, else the generic one.
  expect(
    [
      ['openai', 'gpt-5-mini'],
      ['chatgpt', 'o4-mini'],
      ['groq', 'openai/gpt-oss-120b'],
      ['gemini', 'gemini-3-flash'],
      ['openrouter', 'google/gemma-4-31b'],
      ['xai', 'grok-5'],
      ['groq', 'llama-4-scout'],
      ['openrouter', 'qwen/qwen3.5-32b'],
      ['mistral', 'magistral-medium'],
      ['openrouter', 'deepseek/deepseek-r2'],
      ['ondevice', 'qwen3.5-4b'],
      ['local', 'llama3.1'],
      ['openai', 'some-new-model'],
      ['custom', 'some-new-model'],
      ['', ''],
    ].map(([p, m]) => characterFor(p, m)),
  ).toEqual(['gpt', 'gpt', 'gpt', 'gemini', 'gemini', 'grok', 'llama', 'qwen', 'mistral', 'deepseek', 'local', 'llama', 'gpt', 'generic', 'generic']);
  // The Modes' props.
  expect(['plan', 'auto', 'manual', 'acceptEdits', 'bypassPermissions', ''].map(propOf)).toEqual(['plan', 'auto', 'manual', 'edits', 'bypass', 'auto']);
  // The label: who, how it stands, its Mode.
  expect(crewLabel('opus', 'working', 'Auto')).toBe('Opus, working, Auto mode');
  expect(crewLabel('haiku', 'needs', 'Plan')).toBe('Haiku, needs you, Plan mode');
  expect(crewName('nobody')).toBe('Model');
  // Every character drawn, with every prop: its own SVG, its colours by class, never an image.
  expect([...CHARACTER_IDS].sort()).toEqual(Object.keys(CREW).sort());
  expect([...PROP_IDS].sort()).toEqual(['auto', 'bypass', 'edits', 'manual', 'plan']);
  for (const c of CHARACTER_IDS) {
    for (const p of PROP_IDS) {
      const svg = crewSvg(c, p);
      expect(svg).toMatch(/^<svg viewBox="0 0 18 16"/);
      expect(svg).not.toMatch(/<image|href=|#[0-9a-f]{6}/i);
      expect(svg).toContain('class="c-fl"');
      expect(svg).toContain('class="c-eyes"');
      expect(svg).toContain(p === 'bypass' ? 'class="c-helmet"' : 'class="c-prop"');
    }
  }
  // The markup: its data, its label, its badge; the same string for the same arguments (a redraw never restarts the walk).
  const at = (pose) => crewMarkup({ character: 'opus', prop: 'plan', pose, label: crewLabel('opus', pose, 'Plan'), testid: 'x' });
  expect(at('working')).toContain('role="img" aria-label="Opus, working, Plan mode" data-character="opus" data-prop="plan" data-pose="working"');
  expect(at('working')).toBe(at('working'));
  expect(at('done')).toContain('class="badge done"');
  expect(at('failed')).toContain('class="badge failed"');
  expect(at('needs')).toContain('class="bubble"');
  expect(at('stopped')).not.toMatch(/badge|bubble/);
  expect(crewMarkup({ character: 'nobody' })).toContain('data-character="generic"');
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
  // Its character: Sonnet, walking with Auto's flag.
  const avatar = row(main, 1).getByTestId('chat-agent-row-avatar');
  await expect(avatar).toHaveAttribute('data-pose', 'working');
  await expect(avatar).toHaveAttribute('data-character', 'sonnet');
  await expect(avatar).toHaveAttribute('data-prop', 'auto');
  await expect(avatar).toHaveAttribute('aria-label', 'Sonnet, working, Auto mode');
  // Its stats: the context's fill and tokens after its third call, an estimate, the time.
  await expect(row(main, 1).getByTestId('chat-agent-row-stats')).toHaveText(new RegExp(`^ctx \\d+% · ${fmtTokens(ctxOf(U[2]))} · ≈ \\$[0-9.]+ · \\d+:\\d\\d$`));
  // The header's tiles: what the calls so far used.
  const soFar = U.slice(0, 3).reduce((s, u) => s + ctxOf(u), 0);
  await expect(main.getByTestId('chat-agent-total-tokens').locator('.tv')).toHaveText(fmtTokens(soFar));
  await expect(main.getByTestId('chat-agent-total-cost').locator('.tv')).toHaveText(/^≈ \$/);
  await expect(main.getByTestId('chat-agent-total-cost').locator('.tc')).toHaveText('on the API');
  await expect(main.getByTestId('chat-agent-total-time').locator('.tv')).toHaveText(/^\d+:\d\d$/);
  // Folded, the running row too (its turn's own start opens none); ▸ opens it in place: its to-dos as they stand.
  await expect(row(main, 1)).toHaveAttribute('data-open', 'false');
  await expect(row(main, 1).getByTestId('chat-agent-row-more')).toHaveCount(0);
  await row(main, 1).locator('.rhead').click();
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
  // Its character stops and looks up, an amber "!" over it.
  await expect(avatar).toHaveAttribute('data-pose', 'needs');
  await expect(avatar.locator('.bubble')).toHaveText('!');
  await expect(avatar).toHaveAttribute('aria-label', 'Sonnet, needs you, Auto mode');
  await shot(main, '03-panel-needs-you');
  await thread(main).getByTestId('chat-agent-approve').getByRole('button', { name: 'Allow once' }).click();

  // Finished: ✓, and its totals as Claude Code's result says them.
  const agent = await ended(yap, id);
  expect(agent.status).toBe('done');
  await expect(group(main, 'finished').getByTestId('chat-agent-group-head')).toHaveText('Finished · 1');
  await expect(avatar).toHaveAttribute('data-pose', 'done');
  await expect(row(main, 1).locator('.badge')).toHaveText('✓');
  await expect(avatar.locator('.bubble')).toHaveCount(0);
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

// ---------------------------------------------------------------- the crew, seeded

/** A seeded turn of an agent: its Mode, model and status (agents/store.rs `TurnNote`). */
function seedTurn(now, n, title, mode, model, status, extra = {}) {
  const live = status === 'running' || status === 'asking';
  return {
    n,
    title,
    request: `${title}.`,
    status,
    mode,
    model,
    effort: 'medium',
    startedTs: now - 900 + n * 120,
    endedTs: live ? 0 : now - 840 + n * 120,
    cost: 0.02 * n,
    ...extra,
  };
}

/** What each seeded row shows: its agent, turn, character, prop and pose. */
const avatarOf = (main, id, n) => panel(main).locator(`[data-testid="chat-agent-row"][data-key="${id}:${n}"]`).getByTestId('chat-agent-row-avatar');
/** A computed style of the first `selector` in the avatar. */
const styleIn = (avatar, selector, prop) => avatar.evaluate((el, [s, p]) => getComputedStyle(el.querySelector(s))[p], [selector, prop]);

/** **The crew sheet** (test only, never in the app): every character working, the Modes' props and the poses, at 2×, drawn with the panel's own markup and CSS. */
async function showCrewSheet(main) {
  const css = fs.readFileSync(CREW_CSS, 'utf8');
  const cell = (character, prop, pose, caption) =>
    `<figure>${crewMarkup({ character, prop, pose, size: 2, label: crewLabel(character, pose, prop) })}<figcaption>${caption}</figcaption></figure>`;
  const props = { plan: 'Plan', auto: 'Auto', manual: 'Manual', edits: 'Accept edits', bypass: 'Bypass' };
  const html = `<style>${css}
    #crew-sheet{position:fixed;inset:0;z-index:2147483647;overflow:auto;box-sizing:border-box;padding:22px 26px;background:var(--yap-paper);color:var(--yap-paper-ink);font:13px var(--yap-font-reading, sans-serif)}
    #crew-sheet h2{margin:0 0 4px;font-size:17px}#crew-sheet h3{margin:16px 0 10px;font-size:12px;font-weight:600;color:var(--yap-paper-muted)}
    #crew-sheet .crow{display:flex;flex-wrap:wrap;gap:16px 22px}
    #crew-sheet figure{margin:0;display:flex;flex-direction:column;align-items:center;gap:8px;width:92px}
    #crew-sheet figcaption{font-size:11.5px;color:var(--yap-paper-text-2);text-align:center}</style>
    <div id="crew-sheet" data-testid="crew-sheet"><h2>Yap's crew</h2>
    <h3>Every character, working (each with a Mode's prop)</h3><div class="crow">${CHARACTER_IDS.map((c, i) => cell(c, PROP_IDS[i % PROP_IDS.length], 'working', `${crewName(c)} · ${props[PROP_IDS[i % PROP_IDS.length]]}`)).join('')}</div>
    <h3>The Modes' props (Sonnet, working)</h3><div class="crow">${PROP_IDS.map((p) => cell('sonnet', p, 'working', props[p])).join('')}</div>
    <h3>The poses (Opus, Plan)</h3><div class="crow">${['working', 'needs', 'done', 'stopped', 'failed', 'queued'].map((pose) => cell('opus', 'plan', pose, pose)).join('')}</div></div>`;
  await main.evaluate((h) => {
    const host = document.createElement('div');
    host.id = 'crew-sheet-host';
    host.innerHTML = h;
    document.body.appendChild(host);
  }, html);
  await expect(main.getByTestId('crew-sheet')).toBeVisible();
}

test('the crew: a character a row in each Mode and pose; rows folded, ▸ remembered; moving only on screen; the crew sheet', async ({ yap, main, fakeLlm, shot }) => {
  await main.emulateMedia({ reducedMotion: 'no-preference' });
  await openView(main, 'Chat');
  if (await tab(main).count()) await tab(main).getByRole('button', { name: 'Close the panel' }).click();
  // A Code chat to seed agents into (no Claude Code runs: e2e_agents_seed).
  await newCodeChat(main, 'garden-planner');
  fakeLlm.scriptChat([{ plain: true, content: 'Hello! What shall we build?' }]);
  await send(main, 'Hello.');
  await expect(thread(main)).toContainText('What shall we build?', { timeout: 20_000 });
  const conversation = Math.max(...(await yap.invoke('chats_list')).map((c) => c.id));
  const now = Math.floor(Date.now() / 1000);
  const A = `crewa${now.toString(16)}`;
  const B = `crewb${now.toString(16)}`;
  const base = { conversation, folder: 'garden-planner', path: GARDEN, effort: 'medium', pay: 'plan', updatedTs: now };
  await yap.invoke('e2e_agents_seed', {
    agents: [
      {
        ...base,
        id: A,
        brief: 'Plan the watering schedule.',
        model: 'claude-sonnet-5-5',
        mode: 'auto',
        status: 'running',
        createdTs: now - 1000,
        startedTs: now - 780,
        turns: 4,
        turnList: [
          seedTurn(now, 1, 'Plan the watering schedule', 'plan', 'claude-haiku-5-5', 'done', { todos: todos(7) }),
          seedTurn(now, 2, 'Write the planting guide', 'acceptEdits', 'claude-fable-5', 'stopped'),
          seedTurn(now, 3, 'Move the beds into their own file', 'bypassPermissions', 'claude-opus-5-5', 'failed', { error: 'Claude Code stopped before it finished.' }),
          seedTurn(now, 4, 'Add a minimap button', 'auto', 'claude-sonnet-5-5', 'running', { todos: todos(3) }),
        ],
      },
      {
        ...base,
        id: B,
        brief: 'Tidy notes.md.',
        model: 'claude-opus-5-5',
        mode: 'manual',
        status: 'asking',
        createdTs: now - 200,
        startedTs: now - 150,
        turns: 1,
        turnList: [seedTurn(now, 1, 'Tidy notes.md', 'manual', 'claude-opus-5-5', 'asking')],
      },
    ],
  });
  const open = () => yap.invoke('plugin:event|emit', { event: 'yap-agent-open', payload: { id: A, conversation } });
  await open();
  await expect(panel(main)).toBeVisible({ timeout: 20_000 });
  await expect(panel(main).getByTestId('chat-agent-row')).toHaveCount(5, { timeout: 20_000 });

  // A character a row: who the model is, its Mode's prop, how it stands — and its label.
  const ROWS = [
    [B, 1, 'opus', 'manual', 'needs', 'Manual'],
    [A, 4, 'sonnet', 'auto', 'working', 'Auto'],
    [A, 3, 'opus', 'bypass', 'failed', 'Bypass permissions'],
    [A, 2, 'fable', 'edits', 'stopped', 'Accept edits'],
    [A, 1, 'haiku', 'plan', 'done', 'Plan'],
  ];
  for (const [id, n, character, prop, pose, mode] of ROWS) {
    const avatar = avatarOf(main, id, n);
    await expect(avatar).toHaveAttribute('data-character', character);
    await expect(avatar).toHaveAttribute('data-prop', prop);
    await expect(avatar).toHaveAttribute('data-pose', pose);
    await expect(avatar).toHaveAttribute('role', 'img');
    await expect(avatar).toHaveAttribute('aria-label', crewLabel(character, pose, mode));
    await expect(avatar.locator(prop === 'bypass' ? '.c-helmet' : '.c-prop')).toHaveCount(1);
  }
  // Each pose's mark: needs you's "!" bubble, done's ✓, failed's red badge; stopped grey, no badge.
  await expect(avatarOf(main, B, 1).locator('.bubble')).toHaveText('!');
  await expect(avatarOf(main, A, 1).locator('.badge.done')).toHaveText('✓');
  await expect(avatarOf(main, A, 3).locator('.badge.failed')).toHaveText('×');
  await expect(avatarOf(main, A, 2).locator('.badge, .bubble')).toHaveCount(0);
  expect(await styleIn(avatarOf(main, A, 2), 'svg', 'filter')).toContain('grayscale(1)');
  // Needs you looks up; done sits, its eyes shut.
  expect(await styleIn(avatarOf(main, B, 1), '.c-eyes', 'transform')).toMatch(/matrix\(1, 0, 0, 1, 0, -1\)/);
  expect(await styleIn(avatarOf(main, A, 1), '.c-bod', 'transform')).toMatch(/matrix\(1, 0, 0, 1, 0, 1\)/);
  expect(await styleIn(avatarOf(main, A, 1), '.c-eu', 'opacity')).toBe('0');
  // Its group heads as savvy's: "Running · 1", Finished with Collapse.
  await expect(group(main, 'needs').getByTestId('chat-agent-group-head')).toHaveText('Needs you · 1');
  await expect(group(main, 'running').getByTestId('chat-agent-group-head')).toHaveText('Running · 1');
  await expect(group(main, 'finished').getByTestId('chat-agent-group-head')).toHaveText('Finished · 3');
  await expect(group(main, 'finished').getByTestId('chat-agent-group-head')).toHaveAttribute('data-act', 'Collapse');

  // Only the working one moves: its feet step, its flag flutters; the others are still.
  const working = avatarOf(main, A, 4);
  await expect(panel(main)).toHaveAttribute('data-crew-paused', 'false');
  expect(await styleIn(working, '.c-fl', 'animationName')).toBe('crew-step');
  expect(await styleIn(working, '.c-fl', 'animationPlayState')).toBe('running');
  expect(await styleIn(working, '.c-pfx .f2', 'animationName')).toBe('crew-f2');
  for (const [id, n] of [
    [B, 1],
    [A, 1],
    [A, 2],
    [A, 3],
  ]) {
    expect(await styleIn(avatarOf(main, id, n), '.c-fl', 'animationName')).toBe('none');
  }
  // Out of view (the panel hidden): paused where it is; back, moving again.
  await panel(main).evaluate((el) => (el.style.display = 'none'));
  await expect(panel(main)).toHaveAttribute('data-crew-paused', 'true');
  expect(await styleIn(working, '.c-fl', 'animationPlayState')).toBe('paused');
  await panel(main).evaluate((el) => (el.style.display = ''));
  await expect(panel(main)).toHaveAttribute('data-crew-paused', 'false');
  // Reduced motion (Yap's Motion setting): still frames.
  await setConfig(yap, main, { motion: 'reduced' });
  await expect.poll(() => styleIn(working, '.c-fl', 'animationName')).toBe('none');
  expect(await styleIn(working, '.c-pfx .f2', 'animationName')).toBe('none');
  await setConfig(yap, main, { motion: 'system' });
  await expect.poll(() => styleIn(working, '.c-fl', 'animationName')).toBe('crew-step');
  // And Windows' own (prefers-reduced-motion).
  await main.emulateMedia({ reducedMotion: 'reduce' });
  await expect.poll(() => styleIn(working, '.c-fl', 'animationName')).toBe('none');
  await main.emulateMedia({ reducedMotion: 'no-preference' });
  await expect.poll(() => styleIn(working, '.c-fl', 'animationName')).toBe('crew-step');

  // Rows start folded, the running one too; ▸ opens one, and it stays open when the tab comes back.
  for (const [id, n] of ROWS) await expect(panel(main).locator(`[data-testid="chat-agent-row"][data-key="${id}:${n}"]`)).toHaveAttribute('data-open', 'false');
  const running = panel(main).locator(`[data-testid="chat-agent-row"][data-key="${A}:4"]`);
  await running.locator('.rhead').click();
  await expect(running).toHaveAttribute('data-open', 'true');
  await expect(running.getByTestId('chat-agent-row-todos').locator('.todo')).toHaveCount(7);
  await tab(main).getByRole('button', { name: 'Close the panel' }).click();
  await expect(panel(main)).toHaveCount(0);
  await open();
  await expect(panel(main).locator(`[data-testid="chat-agent-row"][data-key="${A}:4"]`)).toHaveAttribute('data-open', 'true');
  await panel(main).locator(`[data-testid="chat-agent-row"][data-key="${A}:4"] .rhead`).click();
  await expect(panel(main).locator(`[data-testid="chat-agent-row"][data-key="${A}:4"]`)).toHaveAttribute('data-open', 'false');
  // Finished collapses, and says Expand.
  const finishedHead = group(main, 'finished').getByTestId('chat-agent-group-head');
  await finishedHead.click();
  await expect(finishedHead).toHaveAttribute('data-act', 'Expand');
  await expect(group(main, 'finished').getByTestId('chat-agent-row')).toHaveCount(0);
  await finishedHead.click();
  await expect(group(main, 'finished').getByTestId('chat-agent-row')).toHaveCount(3);

  // Screenshots: the five rows (a taller window, so they all show), light and
  // dark; the avatars close up at 2× (the page drawn at twice its pixels, light and dark).
  const size = main.viewportSize();
  await main.setViewportSize({ width: 1100, height: 980 });
  await expect(avatarOf(main, A, 1)).toBeInViewport();
  await shot(main, '08-crew-panel-light');
  const closeUp = async (name) => {
    const boxes = [];
    for (const [id, n] of ROWS) boxes.push(await avatarOf(main, id, n).boundingBox());
    const x = Math.max(0, Math.min(...boxes.map((b) => b.x)) - 10);
    const y = Math.max(0, Math.min(...boxes.map((b) => b.y)) - 10);
    const clip = { x, y, width: Math.max(...boxes.map((b) => b.x + b.width)) + 300 - x, height: Math.max(...boxes.map((b) => b.y + b.height)) + 40 - y, scale: 2 };
    const file = path.join(SCREENSHOTS, 'agents-panel', `${name}.png`);
    fs.mkdirSync(path.dirname(file), { recursive: true });
    const cdp = await main.context().newCDPSession(main);
    try {
      const { data } = await cdp.send('Page.captureScreenshot', { format: 'png', clip });
      fs.writeFileSync(file, Buffer.from(data, 'base64'));
    } finally {
      await cdp.detach().catch(() => {});
    }
  };
  // Held at their first frame for the close-ups (the walk would catch a foot mid-step).
  await setConfig(yap, main, { motion: 'reduced' });
  await closeUp('12-crew-avatars-2x-light');
  await theme(yap, main, 'dark');
  await shot(main, '09-crew-panel-dark');
  await closeUp('13-crew-avatars-2x-dark');
  await theme(yap, main, 'light');
  await setConfig(yap, main, { motion: 'system' });
  if (size) await main.setViewportSize(size);

  // The crew sheet: every character, every prop, every pose — at 2×, light and dark.
  await showCrewSheet(main);
  await shot(main, '10-crew-sheet-light');
  await theme(yap, main, 'dark');
  await shot(main, '11-crew-sheet-dark');
  await theme(yap, main, 'light');
  await main.evaluate(() => document.getElementById('crew-sheet-host')?.remove());

  // Tidy: the seeded agents stop "working".
  await yap.invoke('e2e_agents_seed', {
    agents: (await agents(yap)).filter((j) => j.id === A || j.id === B).map((j) => ({ ...j, status: 'stopped', turnList: (j.turnList || []).map((t) => ({ ...t, status: t.status === 'running' || t.status === 'asking' ? 'stopped' : t.status })) })),
  });
  await tab(main).getByRole('button', { name: 'Close the panel' }).click();
  await main.emulateMedia({ reducedMotion: null });
});
