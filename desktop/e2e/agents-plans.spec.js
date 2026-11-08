// An agent's plan and its go-ahead, after the maintainer's real use on 7 Nov
// 2026 (desktop/src-tauri/src: chat/tools/agents.rs `pick_mode`,
// `fixed_go_ahead`; agents/consent.rs `asks_lower_mode`,
// `plain_go_ahead`; agents/modes.rs `model_pick`; agents/runner.rs
// `plan_allowed`, `go_ahead`; integrations/claude_live/approve.rs's
// ExitPlanMode; agents/approve_card.rs; chat/turn/report.rs. Pages:
// views/chat/AgentApprove.svelte's plan card, AgentReportCard.svelte's Go ahead).
//
// - **The person chooses a lower Mode, never the chat's model**: with a
//   Bypass chip, "look into this and make our own version" runs at Bypass
//   even when the model asks for a plan (its result says why); "make a plan
//   first" runs as a plan.
// - **Allowing a plan means go ahead**: Plan mode's ExitPlanMode prompt is a
//   plan card — "The plan is ready", the plan in markdown, Go ahead / Not
//   yet — in the thread and on the Yap bar. Go ahead: once the plan turn
//   ends, its next turn starts at once at the chat's Mode with Yap's own
//   go-ahead (`--permission-mode`, `--resume` from the stand-in's
//   `_dump_args`), and the plan's report says it's building from it. Not
//   yet: it stays a plan.
// - **The report card's Go ahead** (and the bar's "The agent's plan is ready"
//   card's): the person's click carries the plan out, once.
//
// Agents are driven as agents-turns.spec.js drives them: the scripted fake AI
// calls start_agent, and Claude Code is this build's own stand-in (`yap.exe
// fake-claude <script>`, switched per turn with `e2e_claude_script`), whose
// `_mcp approve` asks the run's permission prompt as Claude Code does. No
// one's real Claude Code, account or files: the workspace is in this run's
// own folder. Light and dark.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, setConfig, inAnotherApp } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { RUNS } from './support/yap.js';
import { newCodeChat } from './support/code-chats.js';
import { reportTitle, planWaits, latestStep, PLAN_ALLOWED, PLAN_WAITING } from '../src/lib/core/agents.js';

const HERE = path.join(RUNS, 'agents-plans-spec');
const SCRIPTS = path.join(HERE, 'scripts');
const GARDEN = path.join(HERE, 'folders', 'garden-planner');
const ARGS = path.join(HERE, 'args.jsonl');

const STARTED = "On it: I've sent an agent to garden-planner.";
const PLAN_MD = [
  '## Watering schedule',
  '',
  'Add a **Watering** section to `notes.md`:',
  '',
  '1. Tomatoes twice a week, seedlings every morning',
  '2. A line for the rain barrel',
  '3. Run the tests',
  '',
  'Nothing else changes: the beds and the planting calendar stay as they are.',
].join('\n');
const PLAN_SAID = "Here's the plan: a watering schedule in notes.md. Shall I go ahead?";

// ---- the stand-in's scripts ----

const init = (mode) => ({
  type: 'system',
  subtype: 'init',
  cwd: '{{cwd}}',
  session_id: '{{session_id}}',
  tools: ['Bash', 'Edit', 'Read', 'Write', 'ExitPlanMode'],
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
/** Plan mode: it reads notes.md, then asks ExitPlanMode's permission (the card), then ends its turn with the plan. */
const PLANNING = [
  { _wait_user: true },
  { _dump_args: ARGS },
  init('plan'),
  said('p1', [{ type: 'text', text: "I'll read notes.md first." }, { type: 'tool_use', id: 'toolu_read', name: 'Read', input: { file_path: '{{cwd}}\\notes.md' } }]),
  toolResult('toolu_read', 'Draft notes'),
  said('p2', [{ type: 'tool_use', id: 'toolu_plan', name: 'ExitPlanMode', input: { plan: PLAN_MD } }]),
  { _mcp: { tool: 'approve', arguments: { tool_name: 'ExitPlanMode', input: { plan: PLAN_MD }, tool_use_id: 'toolu_plan' }, result_for: 'toolu_plan' } },
  said('p3', [{ type: 'text', text: PLAN_SAID }], 'end_turn'),
  result(PLAN_SAID, 0.01),
  { _wait_user: true },
];
/** Says what it was told (the go-ahead's turn). */
const ECHO = [{ _wait_user: true }, { _dump_args: ARGS }, init('auto'), result('Got: {{user_text}}', 0.002), { _wait_user: true }];

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

/** Each turn's Claude Code arguments, as the stand-in wrote them. */
const argsSeen = () => (fs.existsSync(ARGS) ? fs.readFileSync(ARGS, 'utf8').split('\n').filter(Boolean).map((l) => JSON.parse(l).args) : []);
const after = (args, flag) => {
  const i = args.indexOf(flag);
  return i >= 0 ? args[i + 1] : undefined;
};

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
      await use({ name: 'agents-plans', config: { memoryLearnChats: false } });
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
const planCard = (main) => thread(main).locator('[data-testid="chat-agent-approve"][data-kind="plan"]');
/** The latest tool result the chat model was given. */
const lastTool = (fakeLlm) => JSON.parse(fakeLlm.requests.filter((r) => r.messages.some((m) => m.role === 'tool')).at(-1).messages.filter((m) => m.role === 'tool').at(-1).content);
const agentOf = async (yap, id) => (await agents(yap)).find((j) => j.id === id);

async function send(main, text) {
  await main.getByRole('textbox', { name: 'Message' }).fill(text);
  await main.getByRole('button', { name: 'Send', exact: true }).click();
}

/** A new code chat in garden-planner. */
async function codeChat(main) {
  await openView(main, 'Chat');
  if (await tab(main).count()) await tab(main).getByRole('button', { name: 'Close the panel' }).click();
  await newCodeChat(main, 'garden-planner');
}

async function newAgent(yap, before) {
  await expect.poll(async () => (await agents(yap)).length, { timeout: 20_000, message: 'an agent to start' }).toBe(before + 1);
  return (await agents(yap))[0];
}

async function ended(yap, id, timeout = 30_000) {
  await expect.poll(async () => (await agentOf(yap, id))?.status, { timeout, message: `agent ${id} to end` }).toMatch(/^(done|failed|stopped)$/);
  return agentOf(yap, id);
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
 * **A plan agent in a new code chat**: the person asks for a plan first, the
 * model's start_agent asks for Plan mode, and the stand-in plans until its
 * ExitPlanMode card is answered. `reports`: the chat model's words for the
 * reports to come. The agent, as listed.
 */
async function planAgent(yap, main, fakeLlm, reports) {
  fs.rmSync(ARGS, { force: true });
  await script(yap, PLANNING, 'planning');
  const before = (await agents(yap)).length;
  await codeChat(main);
  fakeLlm.scriptChat([
    { tools: [{ name: 'start_agent', args: { brief: 'Add a watering schedule to notes.md.', title: 'Plan a watering schedule', mode: 'plan' } }] },
    { content: STARTED },
    ...reports.map((content) => ({ plain: true, content })),
  ]);
  await send(main, 'Make a plan first: add a watering schedule to notes.md.');
  const agent = await newAgent(yap, before);
  await expect.poll(async () => (await agentOf(yap, agent.id)).status, { timeout: 20_000 }).toBe('asking');
  // The go-ahead's turn (if it comes) is the stand-in's echo: switched now,
  // while the plan's process already runs.
  await script(yap, ECHO, 'echo');
  return agent;
}

/** The agent's go-ahead turn, once it's done: the agent, and each turn's arguments. */
async function wentAhead(yap, id) {
  await expect.poll(async () => (await agentOf(yap, id)).turnList?.length, { timeout: 20_000, message: 'its go-ahead turn' }).toBe(2);
  await expect.poll(async () => (await agentOf(yap, id)).turnList[1].status, { timeout: 30_000 }).toBe('done');
  const agent = await ended(yap, id);
  return { agent, args: argsSeen() };
}

test.beforeEach(async ({ yap, main, fakeLlm }) => {
  freshFolder(GARDEN);
  await yap.invoke('e2e_claude_approve_auto', { allow: null });
  await setConfig(yap, main, {
    agentsEnabled: true,
    agentsAsk: 'new-or-web',
    agentsSpeak: 'never',
    agentsPay: 'plan',
    agentsMode: 'auto',
    webSearch: false,
    speakReplies: false,
    theme: 'light',
    barTheme: 'light',
    llmScopes: chatScope(fakeLlm),
  });
  if (!(await yap.invoke('workspaces_list')).some((f) => f.name === 'garden-planner')) {
    await yap.invoke('workspaces_add', { path: GARDEN });
  }
});

// ---------------------------------------------------------------- no app

test('core/agents.js: a plan allowed reads as carried out; a waiting plan’s Go ahead only while it’s the latest turn', () => {
  expect(reportTitle({ status: 'done', plan: true, planAllowed: true })).toBe(PLAN_ALLOWED);
  expect(reportTitle({ status: 'done', plan: true })).toBe("The agent's plan is ready");
  expect(reportTitle(null, 'The agent in garden-planner: it made a plan, and the user allowed it in Yap: it’s carrying the plan out now.')).toBe(PLAN_ALLOWED);
  expect(reportTitle(null, 'The agent in garden-planner: it made a plan and changed nothing yet.')).toBe("The agent's plan is ready");
  const t1 = { n: 1, plan: true, planAllowed: false };
  const agent = { status: 'done', plan: true, turnList: [t1] };
  expect(planWaits(agent, t1)).toBe(true);
  expect(planWaits({ ...agent, status: 'running' }, t1), 'working on it').toBe(false);
  expect(planWaits({ ...agent, plan: false, turnList: [t1, { n: 2 }] }, t1), 'a later turn').toBe(false);
  expect(planWaits({ ...agent, planAllowed: true, turnList: [{ ...t1, planAllowed: true }] }, { ...t1, planAllowed: true }), 'allowed already').toBe(false);
  expect(planWaits({ status: 'done', plan: true }, null, true), 'an agent from before turns, its latest report').toBe(true);
  expect(planWaits(null, t1)).toBe(false);
  // The agent's line while its plan waits: its own, not "allow a step".
  expect(latestStep([{ step: 'Reading notes.md' }, { step: PLAN_WAITING }], 'asking')).toBe(PLAN_WAITING);
  expect(latestStep([{ step: PLAN_WAITING }, { now: 'Reading notes.md' }, { step: 'Waiting for you to allow a step' }], 'asking')).toBe('Waiting for you to allow a step');
});

// ---------------------------------------------------------------- the Mode

test('a Bypass chip never stops for a plan: the model’s plan is a read-only run at Bypass, “make a plan first” too, no card', async ({ yap, main, fakeLlm }) => {
  // (8 Nov 2026, the maintainer: "if I have bypass permissions, I shouldn't be
  // accepting plans". Under Auto, "make a plan first" is still a plan: the
  // plan card's tests below.)
  await yap.invoke('agent_bypass_confirmed');
  await setConfig(yap, main, { agentsMode: 'bypassPermissions' });
  try {
    fs.rmSync(ARGS, { force: true });
    await script(yap, ECHO, 'echo');
    let before = (await agents(yap)).length;
    await codeChat(main);
    await expect(main.getByTestId('chat-mode-chip')).toContainText('Bypass permissions');
    // The maintainer's words on 7 Nov, and the model's own Plan: never a plan
    // to approve under Bypass — read only at Bypass, the result says why.
    fakeLlm.scriptChat([
      { tools: [{ name: 'start_agent', args: { brief: 'Look into the seed tracker and how we could build our own version combined with the garden planner.', title: 'Our own seed tracker', mode: 'plan' } }] },
      { content: STARTED },
      { plain: true, content: 'It looked into the seed tracker: here is how ours could work.' },
    ]);
    await send(main, 'can we look into this seed tracker and make our own version of it combined with the garden planner');
    const look = await ended(yap, (await newAgent(yap, before)).id);
    expect(look).toMatchObject({ status: 'done', mode: 'bypassPermissions', plan: false, asks: false });
    expect(look.turnList[0]).toMatchObject({ readOnly: true, mode: 'bypassPermissions' });
    expect(after(argsSeen().at(-1), '--permission-mode')).toBe('bypassPermissions');
    expect(after(argsSeen().at(-1), '--disallowedTools')).toBe('Edit,Write,MultiEdit,NotebookEdit,EnterPlanMode');
    expect(look.text).toContain("This is read only: don't change, create or delete any files");
    const told = lastTool(fakeLlm);
    expect(told.success).toBe(true);
    expect(told.data).toMatchObject({ mode: 'Bypass permissions', read_only: true });
    expect(told.data.note).toContain("No plan mode: this chat's Mode is Bypass permissions, so an agent never stops for a plan.");
    await messagesOf(yap, look.conversation, 4);

    // The person asks for a plan in so many words: still no plan to approve
    // under Bypass — read only, its report the plan; no card at any point.
    before = (await agents(yap)).length;
    await codeChat(main);
    fakeLlm.scriptChat([
      { tools: [{ name: 'start_agent', args: { brief: 'Add a watering schedule to notes.md.', mode: 'plan' } }] },
      { content: STARTED },
      { plain: true, content: 'Its plan: a watering schedule in notes.md. Say go ahead and it builds it.' },
    ]);
    await send(main, 'Make a plan first: add a watering schedule to notes.md.');
    const plan = await ended(yap, (await newAgent(yap, before)).id);
    expect(plan).toMatchObject({ status: 'done', mode: 'bypassPermissions', plan: false, planAllowed: false });
    expect(plan.turnList[0].readOnly).toBe(true);
    expect(after(argsSeen().at(-1), '--permission-mode')).toBe('bypassPermissions');
    expect(lastTool(fakeLlm).data.note).toContain('No plan mode');
    await messagesOf(yap, plan.conversation, 4);
    await expect(planCard(main)).toHaveCount(0);
    await expect(thread(main).getByTestId('chat-agent-confirm')).toHaveCount(0);
    await expect(thread(main).getByTestId('chat-agent-go-ahead')).toHaveCount(0);
    expect(fs.readFileSync(path.join(GARDEN, 'notes.md'), 'utf8')).toBe('Draft notes\n');

    // "Go ahead": its next turn at Bypass, its tools back (no longer read only).
    fakeLlm.scriptChat([
      { tools: [{ name: 'message_agent', args: { text: 'go ahead' } }] },
      { content: 'Going ahead.' },
      { plain: true, content: 'Done: it added the watering schedule.' },
    ]);
    await send(main, 'Go ahead.');
    await expect.poll(async () => (await agentOf(yap, plan.id)).turnList?.[1]?.status, { timeout: 30_000 }).toBe('done');
    const built = await ended(yap, plan.id);
    expect(built.turnList[1]).toMatchObject({ mode: 'bypassPermissions' });
    expect(built.turnList[1].readOnly).toBeFalsy();
    expect(after(argsSeen().at(-1), '--disallowedTools')).toBe('EnterPlanMode');
    await messagesOf(yap, plan.conversation, 8);
  } finally {
    await setConfig(yap, main, { agentsMode: 'auto' });
  }
});

test('a Bypass Code chat: “catch up on this folder” runs read only with no card and no question, and reports back', async ({ yap, main, fakeLlm, shot }) => {
  await yap.invoke('agent_bypass_confirmed');
  await setConfig(yap, main, { agentsMode: 'bypassPermissions', agentsAsk: 'always' });
  // Cards ask as in the app (none answered by the test): any card would wait.
  await yap.invoke('e2e_claude_approve_auto', { allow: null });
  const PROMPTS = path.join(HERE, 'catch-up-approve.jsonl');
  fs.rmSync(PROMPTS, { force: true });
  const CAUGHT_UP = "I'm caught up: garden-planner keeps the beds and a planting calendar in notes.md; nothing's changed.";
  try {
    fs.rmSync(ARGS, { force: true });
    await script(
      yap,
      [
        { _wait_user: true },
        { _dump_args: ARGS },
        init('bypassPermissions'),
        said('c1', [{ type: 'text', text: "I'll look around first." }, { type: 'tool_use', id: 'toolu_log', name: 'Bash', input: { command: 'git log --oneline -5', description: 'See the recent commits' } }]),
        // A prompt Claude Code still sends in Bypass (its own safety checks):
        // answered no at once in Yap, with no card.
        { _mcp: { tool: 'approve', arguments: { tool_name: 'Bash', input: { command: 'git log --oneline -5' }, tool_use_id: 'toolu_log' }, dump: PROMPTS } },
        toolResult('toolu_log', 'a1b2c3 Add the planting calendar'),
        said('c2', [{ type: 'tool_use', id: 'toolu_read', name: 'Read', input: { file_path: '{{cwd}}\\notes.md' } }]),
        toolResult('toolu_read', 'Draft notes'),
        said('c3', [{ type: 'text', text: CAUGHT_UP }], 'end_turn'),
        result(CAUGHT_UP, 0.008),
        { _wait_user: true },
      ],
      'catch-up'
    );
    const before = (await agents(yap)).length;
    await codeChat(main);
    await expect(main.getByTestId('chat-mode-chip')).toContainText('Bypass permissions');
    const BRIEF = 'Get caught up with the garden-planner folder: read it and say what it is and where it stands. Change nothing.';
    fakeLlm.scriptChat([
      { tools: [{ name: 'start_agent', args: { brief: BRIEF, title: 'Catch up on garden-planner', read_only: true } }] },
      { content: STARTED },
      { plain: true, content: "It's caught up: garden-planner keeps the beds and a planting calendar in notes.md, and it changed nothing." },
    ]);
    await send(main, 'Hey Claude, can you get yourself caught up with this folder? When you get to speed with it, let me know.');
    const agent = await ended(yap, (await newAgent(yap, before)).id);
    // Bypass, read only, no plan, never asking: Settings' "always" skipped too.
    expect(agent).toMatchObject({ status: 'done', mode: 'bypassPermissions', plan: false, brief: BRIEF });
    expect(agent.turnList[0]).toMatchObject({ readOnly: true, title: 'Catch up on garden-planner' });
    const args = argsSeen().at(-1);
    expect(after(args, '--permission-mode')).toBe('bypassPermissions');
    expect(after(args, '--disallowedTools')).toBe('Edit,Write,MultiEdit,NotebookEdit,EnterPlanMode');
    expect(args).not.toContain('--dangerously-skip-permissions');
    expect(agent.text).toBe(CAUGHT_UP);
    // Claude Code's own prompt: answered no, nobody asked, never a card.
    const prompt = fs.readFileSync(PROMPTS, 'utf8');
    expect(prompt).toContain('runs in Bypass permissions in Yap, so nobody is asked');
    expect(await yap.invoke('claude_approve_pending')).toEqual([]);
    // Reported back in the chat, with no card or question on the way.
    const messages = await messagesOf(yap, agent.conversation, 4);
    expect(messages.map((m) => m.kind ?? '')).toEqual(['', '', 'agentReport', '']);
    expect(messages[1].confirms ?? []).toEqual([]);
    expect(lastTool(fakeLlm).data).toMatchObject({ mode: 'Bypass permissions', read_only: true });
    await expect(thread(main).getByTestId('chat-agent-confirm')).toHaveCount(0);
    await expect(thread(main).locator('[data-testid="chat-agent-approve"]')).toHaveCount(0);
    await expect(thread(main).getByTestId('chat-agent-report')).toHaveCount(1);
    expect(fs.readFileSync(path.join(GARDEN, 'notes.md'), 'utf8')).toBe('Draft notes\n');
    await shot(main, '07-bypass-catch-up-reported-light');
    await theme(yap, main, 'dark');
    await shot(main, '08-bypass-catch-up-reported-dark');
    await theme(yap, main, 'light');
  } finally {
    await setConfig(yap, main, { agentsMode: 'auto', agentsAsk: 'new-or-web' });
  }
});

// ---------------------------------------------------------------- allowing a plan

test('the plan card in the thread: Go ahead carries the plan out at once, at the chat’s Mode, on the same session', async ({ yap, main, fakeLlm, shot }) => {
  const started = await planAgent(yap, main, fakeLlm, ['It made a plan and is building it now.', 'Done: notes.md has its watering schedule.']);
  // The card reads as a plan: its words, the plan itself, Go ahead / Not yet.
  const card = planCard(main);
  await expect(card).toHaveCount(1);
  await expect(card.locator('.aq')).toContainText("The agent's plan is ready · garden-planner");
  const plan = card.getByTestId('chat-agent-plan');
  await expect(plan.getByRole('heading', { name: 'Watering schedule' })).toBeVisible();
  await expect(plan.locator('li')).toHaveCount(3);
  await expect(plan.locator('code')).toHaveText('notes.md');
  await expect(card.getByRole('button', { name: 'Show more' })).toBeVisible();
  await expect(card.getByRole('button', { name: 'Go ahead' })).toBeVisible();
  await expect(card.getByRole('button', { name: 'Not yet' })).toBeVisible();
  await expect(main.getByTestId('chat-agent-status')).toHaveText('Waiting for you');
  await expect(tab(main).getByText(PLAN_WAITING).first()).toBeVisible();
  await shot(main, '01-plan-card-light');
  await card.getByRole('button', { name: 'Show more' }).click();
  await expect(card.getByRole('button', { name: 'Show less' })).toBeVisible();
  await expect(plan).toContainText('Nothing else changes');
  await theme(yap, main, 'dark');
  await shot(main, '02-plan-card-dark');
  await theme(yap, main, 'light');

  // Go ahead: the plan's turn ends, and its next turn starts by itself.
  await card.getByRole('button', { name: 'Go ahead' }).click();
  await expect(card).toHaveCount(0);
  const { agent, args } = await wentAhead(yap, started.id);
  const [planned, go] = agent.turnList;
  expect(planned).toMatchObject({ n: 1, status: 'done', mode: 'plan', plan: true, planAllowed: true, asks: false });
  expect(go).toMatchObject({ n: 2, status: 'done', mode: 'auto', title: 'Go ahead with the plan', plan: false });
  expect(agent).toMatchObject({ mode: 'auto', plan: false, planAllowed: false });
  expect(go.text).toContain('Got: Go ahead with the plan.');
  expect(go.text).toContain('Go ahead: carry out the plan.');
  // Each turn's Claude Code: the plan's in Plan mode, the go-ahead's at the chip's Auto, the same session.
  expect(args).toHaveLength(2);
  expect(after(args[0], '--permission-mode')).toBe('plan');
  expect(after(args[1], '--permission-mode')).toBe('auto');
  expect(after(args[1], '--resume')).toBe(after(args[0], '--session-id'));

  // Two reports: the plan's says it's building from it, never "go ahead?".
  const messages = await messagesOf(yap, agent.conversation, 6);
  const markers = messages.filter((m) => m.kind === 'agentReport');
  expect(markers).toHaveLength(2);
  expect(markers.some((m) => m.text.includes('it made a plan, and the user allowed it in Yap: it\'s carrying the plan out now.'))).toBe(true);
  const reportRules = fakeLlm.requests.filter((r) => r.tools.length === 0).map((r) => r.system || '');
  expect(reportRules.some((s) => s.includes("it's building from the plan now") && s.includes("don't ask whether to go ahead"))).toBe(true);
  const cards = thread(main).getByTestId('chat-agent-report');
  await expect(cards).toHaveCount(2);
  await expect(cards.locator('.jtitle')).toContainText([`${PLAN_ALLOWED} · garden-planner`, 'Agent done · garden-planner']);
  await expect(thread(main).getByTestId('chat-agent-go-ahead')).toHaveCount(0);
});

test('Not yet keeps it a plan; the report card’s Go ahead carries it out, once', async ({ yap, main, fakeLlm, shot }) => {
  const started = await planAgent(yap, main, fakeLlm, ['It made a plan: a watering schedule in notes.md. Shall it go ahead?', 'Done: notes.md has its watering schedule.']);
  await planCard(main).getByRole('button', { name: 'Not yet' }).click();
  await expect(planCard(main)).toHaveCount(0);
  let agent = await ended(yap, started.id);
  expect(agent).toMatchObject({ status: 'done', mode: 'plan', plan: true, planAllowed: false, asks: true });
  const messages = await messagesOf(yap, agent.conversation, 4);
  expect(messages[2].text).toContain('it made a plan and changed nothing yet');
  // Nothing carried on by itself.
  await main.waitForTimeout(1_500);
  expect((await agentOf(yap, started.id)).turnList).toHaveLength(1);
  expect(argsSeen()).toHaveLength(1);
  expect(fs.readFileSync(path.join(GARDEN, 'notes.md'), 'utf8')).toBe('Draft notes\n');

  // The report card: "The agent's plan is ready", with Go ahead.
  const report = thread(main).getByTestId('chat-agent-report');
  await expect(report.locator('.jtitle')).toHaveText("The agent's plan is ready · garden-planner");
  const goAhead = report.getByTestId('chat-agent-go-ahead');
  await expect(goAhead).toHaveText('Go ahead');
  await expect(goAhead).toBeEnabled();
  await shot(main, '03-plan-report-go-ahead-light');
  await theme(yap, main, 'dark');
  await shot(main, '04-plan-report-go-ahead-dark');
  await theme(yap, main, 'light');
  // The person's click: the go-ahead's turn, at the chip's Auto.
  await goAhead.click();
  await expect(goAhead).toBeDisabled();
  await expect(goAhead).toHaveText('Going ahead');
  const went = await wentAhead(yap, started.id);
  agent = went.agent;
  expect(agent.turnList[1]).toMatchObject({ title: 'Go ahead with the plan', mode: 'auto', status: 'done' });
  expect(agent.turnList[1].text).toContain('Got: Go ahead with the plan.');
  expect(after(went.args.at(-1), '--permission-mode')).toBe('auto');
  // Once: the button stays used, and a second go-ahead is refused.
  await expect(goAhead).toBeDisabled();
  await expect(yap.invoke('agent_go_ahead', { id: started.id })).rejects.toThrow(/no plan waiting/);
  await messagesOf(yap, agent.conversation, 6);
  expect((await agentOf(yap, started.id)).turnList).toHaveLength(2);
});

// ---------------------------------------------------------------- on the Yap bar

test('on the Yap bar: the plan card’s Go ahead carries it out; after Not yet, “Agent’s plan is ready” has Go ahead too', async ({ yap, main, fakeLlm, shot }) => {
  const barCards = async () => (await yap.invoke('bar_status')).cards;
  /** The bar's card `id` once it's up (another may hold the bar's one slot a while: it waits its turn). */
  const cardUp = async (id, title = null) => {
    await expect.poll(async () => (await barCards()).some((c) => c.id === id && (!title || c.title === title)), { timeout: 25_000, message: `the bar's ${id} card` }).toBe(true);
    return (await barCards()).find((c) => c.id === id);
  };
  // In another app: the plan card is on the bar, a short line of it.
  let started = await planAgent(yap, main, fakeLlm, ['It made a plan and is building it now.', 'Done: notes.md has its watering schedule.']);
  await inAnotherApp(yap);
  let barCard = await cardUp('agentApprove');
  expect(barCard).toMatchObject({ title: "The agent's plan is ready in garden-planner", body: 'Watering schedule', icon: 'agent' });
  expect([barCard.primary.label, barCard.secondary.label]).toEqual(['Go ahead', 'Not yet']);
  const onBar = yap.overlay.locator('.card').filter({ hasText: "The agent's plan is ready in garden-planner" });
  await expect(onBar).toBeVisible();
  await shot(yap.overlay, '05-plan-bar-card-light');
  await setConfig(yap, main, { barTheme: 'dark' });
  await expect.poll(() => yap.overlay.evaluate(() => document.documentElement.dataset.barTheme)).toBe('dark');
  await shot(yap.overlay, '06-plan-bar-card-dark');
  await setConfig(yap, main, { barTheme: 'light' });
  // Go ahead on the bar: carried out at once, at the chat's Mode.
  await yap.invoke('bar_card_action', { id: 'agentApprove', action: barCard.primary.id });
  let { agent, args } = await wentAhead(yap, started.id);
  expect(agent.turnList[0]).toMatchObject({ plan: true, planAllowed: true });
  expect(after(args.at(-1), '--permission-mode')).toBe('auto');
  await messagesOf(yap, agent.conversation, 6);
  // (No "The agent's plan is ready" for a plan carried out: the next turn's report follows.)

  // Not yet on the bar, then the report's card: "The agent's plan is ready", Go ahead.
  started = await planAgent(yap, main, fakeLlm, ['It made a plan: a watering schedule in notes.md. Shall it go ahead?', 'Done: notes.md has its watering schedule.']);
  barCard = await cardUp('agentApprove');
  expect(barCard.title).toBe("The agent's plan is ready in garden-planner");
  await yap.invoke('bar_card_action', { id: 'agentApprove', action: barCard.secondary.id });
  agent = await ended(yap, started.id);
  expect(agent).toMatchObject({ plan: true, planAllowed: false });
  await messagesOf(yap, agent.conversation, 4);
  const done = await cardUp('agentDone', "The agent's plan is ready: garden-planner");
  expect([done.primary.label, done.secondary?.label]).toEqual(['Go ahead', 'Open']);
  await yap.invoke('bar_card_action', { id: 'agentDone', action: done.primary.id });
  ({ agent, args } = await wentAhead(yap, started.id));
  expect(agent.turnList[1]).toMatchObject({ title: 'Go ahead with the plan', mode: 'auto' });
  await messagesOf(yap, agent.conversation, 6);
});
