// **A Code chat's own /clear, /compact and /model** (17 Nov 2026, the Claude Code app's: desktop/src-tauri/src/agents/
// slash.rs, views/chat/Composer.svelte `submit`), the "+N folders" on its edge (CodeEdge.svelte), Slash commands ›'s
// search box, and the added folders on a Code chat that doesn't work directly (chat/turn/plan.rs).
//
// - /clear: Claude starts afresh — the chat's agents set aside, its next message a new session — the thread kept
//   under a "Context cleared" divider; asked first while a turn works (stopped, then cleared).
// - /compact [words]: Claude Code's own compaction of the session (`-p "/compact …" --resume <session>`, kept in it),
//   a quiet line while it runs, then what it came to; the chat's ring follows.
// - /model <name>: the chat's model chip switched, its effort kept; alone, the picker opens.
// - Anything else after "/" goes as typed.
//
// Chat is on the plan with Yap's stand-in for Claude Code (`yap.exe fake-claude <script>`); the workspace and folders
// are in this run's own folder. No one's real Claude Code, account or files.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, setConfig, closeToasts, jsonLines } from './support/fixtures.js';
import { fakeClaude, planConfig } from './support/claude-plan.js';
import { RUNS } from './support/yap.js';
import { newCodeChat } from './support/code-chats.js';
import { codeCommand, modelFor, roughTokens } from '../src/lib/views/chat/codeMenu.svelte.js';

const HERE = path.join(RUNS, `code-commands-spec-w${process.env.TEST_WORKER_INDEX ?? 0}`);
const SCRIPTS = path.join(HERE, 'scripts');
const GARDEN = path.join(HERE, 'folders', 'garden-planner');
const SEEDS = path.join(HERE, 'folders', 'seed-catalogue');
const ARGS = path.join(HERE, 'args.jsonl');
const COMPACTS = path.join(HERE, 'compacts.jsonl');

const COMMANDS = [
  { name: 'tidy-beds', description: "Tidy the flower beds' notes", argumentHint: '[bed]' },
  { name: 'garden-tools:water', description: '(garden-tools) Water the seedlings on a schedule', argumentHint: '<bed>' },
  { name: 'garden-tools:prune', description: "(garden-tools) Prune what's overgrown", argumentHint: '' },
  { name: 'clear', description: 'Start a new session with empty context', argumentHint: '[name]', builtin: true },
  { name: 'compact', description: 'Free up context by summarizing the conversation so far', argumentHint: '<optional custom summarization instructions>', builtin: true },
  { name: 'model', description: 'Set the AI model for Claude Code', argumentHint: '<model>', builtin: true },
];
const OFFERS = { _offers: { commands: COMMANDS, mcpServers: [] } };
const COMPACT = { _compact: { before: 54_000, after: 3_000, dump: COMPACTS } };

const init = () => ({ type: 'system', subtype: 'init', cwd: '{{cwd}}', session_id: '{{session_id}}', tools: ['Read'], mcp_servers: [], model: 'claude-haiku-5-5', permissionMode: 'auto', apiKeySource: 'none', claude_code_version: '2.1.296', capabilities: ['interrupt_receipt_v1'] });
const said = (id, text) => ({ type: 'assistant', message: { id, type: 'message', role: 'assistant', model: 'claude-haiku-5-5', content: [{ type: 'text', text }], stop_reason: 'end_turn', usage: { input_tokens: 30_000, output_tokens: 40, cache_creation_input_tokens: 0, cache_read_input_tokens: 0 } }, parent_tool_use_id: null, session_id: '{{session_id}}' });
const result = (text) => ({ type: 'result', subtype: 'success', is_error: false, duration_ms: 400, num_turns: 1, result: text, session_id: '{{session_id}}', total_cost_usd: 0.01, usage: { input_tokens: 30_000, output_tokens: 40, cache_creation_input_tokens: 0, cache_read_input_tokens: 0 }, permission_denials: [], terminal_reason: 'completed' });
const ANSWER = 'The beds are watered on Tuesdays.';
/** Turns that answer at once, their arguments dumped. */
const TURNS = [OFFERS, COMPACT, { _wait_user: true }, { _dump_args: ARGS }, init(), said('a1', ANSWER), result(ANSWER), { _wait_user: true }, { _dump_args: ARGS }, init(), said('a2', ANSWER), result(ANSWER), { _wait_user: true }];
/** A turn that takes its time (a /clear asks first while it works). */
const SLOW = [OFFERS, COMPACT, { _wait_user: true }, { _dump_args: ARGS }, init(), { _sleep_ms: 30_000 }, said('s1', ANSWER), result(ANSWER), { _wait_user: true }];
/** A plan chat's own session (a Code chat that doesn't work directly): its arguments dumped. */
const PLAN_CHAT = [OFFERS, { _wait_user: true }, { _dump_args: ARGS }, { ...init(), permissionMode: 'dontAsk' }, said('p1', ANSWER), result(ANSWER), { _wait_user: true }];

function writeScript(name, lines) {
  fs.mkdirSync(SCRIPTS, { recursive: true });
  const file = path.join(SCRIPTS, `${name}.jsonl`);
  fs.writeFileSync(file, lines.map((l) => JSON.stringify(l)).join('\n') + '\n');
  return file;
}
function freshFolders() {
  for (const dir of [GARDEN, SEEDS]) {
    fs.rmSync(dir, { recursive: true, force: true });
    fs.mkdirSync(dir, { recursive: true });
  }
  fs.writeFileSync(path.join(GARDEN, 'notes.md'), '# Garden notes\n');
  fs.writeFileSync(path.join(SEEDS, 'catalogue.md'), '# Seeds\n');
  for (const f of [ARGS, COMPACTS]) fs.rmSync(f, { force: true });
}

const test = base.extend({
  yapOptions: [
    async ({}, use) => {
      await use({ name: 'code-commands', env: fakeClaude(writeScript('turns', TURNS)), config: planConfig({ 'claude-plan': 'claude-plan' }, { memoryLearnChats: false }) });
    },
    { scope: 'worker' },
  ],
});

const box = (main) => main.getByRole('textbox', { name: 'Message' });
const plus = (main) => main.getByRole('button', { name: 'Chat options' });
const menu = (main) => main.getByTestId('code-plus-menu');
const agents = (yap) => yap.invoke('agent_list');
const argsSeen = () => jsonLines(ARGS).map((l) => l.args);
const after = (args, flag) => {
  const i = args.indexOf(flag);
  return i >= 0 ? args[i + 1] : undefined;
};
const valuesOf = (args, flag) => {
  const i = args.indexOf(flag);
  if (i < 0) return [];
  const out = [];
  for (let j = i + 1; j < args.length && !args[j].startsWith('--'); j++) out.push(args[j]);
  return out;
};
async function theme(yap, main, which) {
  await setConfig(yap, main, { theme: which });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe(which);
}
async function codeChat(main) {
  await openView(main, 'Chat');
  const doc = main.getByTestId('chat-doc');
  if (await doc.count()) await doc.getByRole('button', { name: 'Close the panel' }).first().click();
  await newCodeChat(main, 'garden-planner');
}
/** A message sent, its run started and answered. Its arguments. */
async function send(yap, main, text) {
  const before = argsSeen().length;
  await box(main).fill(text);
  await main.getByRole('button', { name: 'Send', exact: true }).click();
  await expect.poll(() => argsSeen().length, { timeout: 20_000, message: 'its run to start' }).toBe(before + 1);
  return argsSeen().at(-1);
}
async function idle(yap) {
  await expect.poll(async () => (await agents(yap)).every((a) => !['queued', 'running', 'asking'].includes(a.status)), { timeout: 30_000 }).toBe(true);
}
/** Words typed in the box and Enter, as the person does. */
async function typed(main, text) {
  await box(main).fill(text);
  await box(main).press('Enter');
}

test.beforeEach(async ({ yap, main }) => {
  freshFolders();
  await yap.invoke('e2e_claude_script', { script: writeScript('turns', TURNS) });
  await yap.invoke('e2e_claude_approve_auto', { allow: true });
  await setConfig(yap, main, { agentsEnabled: true, agentsDirect: true, agentsAsk: 'new-or-web', agentsSpeak: 'never', agentsMode: 'auto', webSearch: false, speakReplies: false, assistantScreen: false, theme: 'light' });
  if (!(await yap.invoke('workspaces_list')).some((f) => f.name === 'garden-planner')) await yap.invoke('workspaces_add', { path: GARDEN });
});

test("the box's three commands, a model by its name, and a token count in a word", () => {
  expect(codeCommand('/clear')).toEqual({ name: 'clear', arg: '' });
  expect(codeCommand('  /COMPACT keep the test plan ')).toEqual({ name: 'compact', arg: 'keep the test plan' });
  expect(codeCommand('/model haiku')).toEqual({ name: 'model', arg: 'haiku' });
  expect(codeCommand('/model')).toEqual({ name: 'model', arg: '' });
  // "/clear the cache" is words for Claude; anything else after "/" goes as typed.
  expect(codeCommand('/clear the cache')).toBeNull();
  expect(codeCommand('/tidy-beds north')).toBeNull();
  expect(codeCommand('/compacted')).toBeNull();
  expect(codeCommand('clear')).toBeNull();
  const rows = [
    { value: 'claude-opus-5-5', label: 'Claude Opus 5.5' },
    { value: 'claude-sonnet-5-5', label: 'Claude Sonnet 5.5' },
    { value: 'claude-haiku-5-5', label: 'Claude Haiku 5.5' },
    { value: 'claude-haiku-4-5', label: 'Claude Haiku 4.5' },
  ];
  expect(modelFor(rows, 'haiku').value).toBe('claude-haiku-5-5');
  expect(modelFor(rows, 'Haiku 4.5').value).toBe('claude-haiku-4-5');
  expect(modelFor(rows, 'claude-sonnet-5-5').value).toBe('claude-sonnet-5-5');
  expect(modelFor(rows, 'gpt')).toBeNull();
  expect(roughTokens(54_135)).toBe('54k');
  expect(roughTokens(2_900)).toBe('3k');
  expect(roughTokens(1_200_000)).toBe('1.2M');
  expect(roughTokens(600)).toBe('600');
});

test('/clear: Claude starts afresh (a new session for the next message), the thread kept under a divider; asked first while it works', async ({ yap, main, shot }) => {
  await codeChat(main);
  const first = await send(yap, main, 'When are the beds watered?');
  await idle(yap);
  const session = after(first, '--session-id');
  expect(session).toBeTruthy();
  const conversation = (await agents(yap))[0].conversation;
  await typed(main, '/clear');
  await expect(main.locator('.toast').filter({ hasText: 'Context cleared' })).toBeVisible();
  await expect(box(main)).toHaveValue('');
  const divider = main.getByTestId('chat-cleared');
  await expect(divider).toHaveCount(1);
  await expect(divider).toContainText('Context cleared');
  // Nothing went to Claude Code: Yap did it.
  expect(argsSeen()).toHaveLength(1);
  // The next message: a new session, never the old one resumed.
  const next = await send(yap, main, 'And the herbs?');
  expect(after(next, '--resume')).toBeUndefined();
  expect(after(next, '--session-id')).toBeTruthy();
  expect(after(next, '--session-id')).not.toBe(session);
  await idle(yap);
  expect((await agents(yap)).filter((a) => a.conversation === conversation)).toHaveLength(2);
  // The divider sits between the first answer and the next message.
  const order = await main
    .locator('.msgs')
    .locator('.cbubble.user, [data-testid="chat-cleared"]')
    .evaluateAll((els) => els.map((e) => (e.dataset.testid === 'chat-cleared' ? 'divider' : e.textContent.trim().slice(0, 20))));
  expect(order).toEqual(['When are the beds wa', 'divider', 'And the herbs?']);
  await closeToasts(main);
  await shot(main, '01-cleared-light');
  await theme(yap, main, 'dark');
  await shot(main, '02-cleared-dark');
  await theme(yap, main, 'light');

  // While a turn works: asked first, the turn stopped, then cleared.
  await yap.invoke('e2e_claude_script', { script: writeScript('slow', SLOW) });
  await box(main).fill('Plan next season in detail.');
  await main.getByRole('button', { name: 'Send', exact: true }).click();
  await expect.poll(async () => (await agents(yap)).some((a) => a.status === 'running'), { timeout: 20_000 }).toBe(true);
  await typed(main, '/clear');
  const ask = main.getByRole('alertdialog', { name: 'Clear the conversation?' });
  await expect(ask).toBeVisible();
  await shot(main, '03-clear-asks-while-working-light');
  await ask.getByRole('button', { name: 'Stop and clear' }).click();
  await expect(ask).toHaveCount(0, { timeout: 20_000 });
  await expect(divider).toHaveCount(2, { timeout: 20_000 });
  await idle(yap);
});

test('/compact: Claude Code compacts the session itself, a quiet line while it runs, then what it came to', async ({ yap, main, shot }) => {
  await codeChat(main);
  const first = await send(yap, main, 'When are the beds watered?');
  await idle(yap);
  const conversation = (await agents(yap))[0].conversation;
  await typed(main, '/compact keep the watering plan');
  const line = main.getByTestId('code-compact-line');
  await expect(line).toContainText('Compacted: about 54k tokens of context down to 3k', { timeout: 20_000 });
  await shot(main, '04-compacted-light');
  // Claude Code's own /compact, on the chat's session and model, saved into it.
  const run = jsonLines(COMPACTS).at(-1);
  expect(run.args.slice(0, 2)).toEqual(['-p', '/compact keep the watering plan']);
  expect(after(run.args, '--resume')).toBe(after(first, '--session-id'));
  expect(after(run.args, '--model')).toBe('claude-haiku-5-5');
  expect(run.persist).toBe(true);
  expect(path.resolve(run.cwd)).toBe(path.resolve(GARDEN));
  // The chat's ring: the compacted size.
  await expect.poll(async () => (await yap.invoke('claude_plan_usage', {})).contexts?.[String(conversation)]?.used, { timeout: 10_000 }).toBe(3_000);
  // The next message carries on the same session.
  const next = await send(yap, main, 'And the herbs?');
  expect(after(next, '--resume')).toBe(after(first, '--session-id'));
  await idle(yap);
  await theme(yap, main, 'dark');
  await typed(main, '/compact');
  await expect(line).toContainText('Compacted', { timeout: 20_000 });
  await shot(main, '05-compacted-dark');
  await theme(yap, main, 'light');
});

test('/model: the chat model chip switched, its effort kept; alone, the picker opens; one it has no such model for is said', async ({ yap, main, shot }) => {
  await codeChat(main);
  await yap.invoke('chat_set_effort', { provider: 'claude-plan', model: 'claude-haiku-5-5', level: 'high' });
  await typed(main, '/model sonnet');
  await expect.poll(async () => (await yap.invoke('ai_model_status', { scope: 'chat' })).model?.id, { timeout: 10_000 }).toBe('claude-sonnet-5-5');
  const status = await yap.invoke('ai_model_status', { scope: 'chat' });
  expect(status.effort?.chosen || status.effort?.default).toBe('high');
  await expect(main.locator('.toast').filter({ hasText: 'Claude Sonnet 5.5 now' })).toBeVisible();
  await expect(main.locator('.modelchip .chip')).toContainText('Claude Sonnet 5.5');
  await expect(box(main)).toHaveValue('');
  // Nothing went to Claude Code.
  expect(argsSeen()).toHaveLength(0);
  await shot(main, '06-model-switched-light');
  await closeToasts(main);
  // No such model: said, the words kept to fix.
  await typed(main, '/model gpt-9');
  await expect(main.locator('.toast').filter({ hasText: 'No model called "gpt-9"' })).toBeVisible();
  await expect(box(main)).toHaveValue('/model gpt-9');
  await closeToasts(main);
  // Alone: the picker.
  await typed(main, '/model');
  await expect(main.getByRole('dialog', { name: 'Switch the Chat model' })).toBeVisible();
  await expect(box(main)).toHaveValue('');
  await main.keyboard.press('Escape');
  // Back to Haiku for the rest (its effort kept too).
  await typed(main, '/model haiku');
  await expect.poll(async () => (await yap.invoke('ai_model_status', { scope: 'chat' })).model?.id, { timeout: 10_000 }).toBe('claude-haiku-5-5');
  await closeToasts(main);
});

test('"+1 folder" on the edge once a folder is added: its card on hover, the summary on a click', async ({ yap, main, shot }) => {
  await codeChat(main);
  const folders = main.getByTestId('chat-code-folders');
  await expect(folders).toHaveCount(0);
  await yap.invoke('e2e_workspace', { path: SEEDS });
  await plus(main).click();
  await menu(main).getByRole('button', { name: 'Add folder' }).click();
  await expect(folders).toHaveText('+1 folder');
  await closeToasts(main);
  await folders.hover();
  const card = main.getByTestId('chat-code-folders-card');
  await expect(card).toBeVisible();
  await expect(card).toContainText('seed-catalogue');
  await shot(main, '07-plus-one-folder-light');
  await theme(yap, main, 'dark');
  await folders.hover();
  await shot(main, '08-plus-one-folder-dark');
  await theme(yap, main, 'light');
  await folders.click();
  const summary = main.getByTestId('chat-summary');
  await expect(summary).toBeVisible();
  await expect(summary.getByTestId('chat-summary-folder')).toContainText('seed-catalogue');
  // Clicked again with the summary open: it stays.
  await folders.click();
  await expect(summary).toBeVisible();
  await summary.getByRole('button', { name: 'Take seed-catalogue off this chat' }).click();
  await expect(folders).toHaveCount(0);
  await main.getByRole('button', { name: 'Close the panel' }).first().click();
});

test('Slash commands › has a search box: by name and line, ↓ into the list, Enter picks the first', async ({ yap, main, shot }) => {
  await codeChat(main);
  await plus(main).click();
  await menu(main).getByRole('button', { name: 'Slash commands' }).click();
  const sub = main.getByTestId('code-plus-commands-menu');
  const search = sub.getByRole('searchbox', { name: 'Search commands' });
  await expect(sub.locator('.mcmd').first()).toHaveText('/tidy-beds', { timeout: 15_000 });
  await search.fill('water');
  await expect(sub.locator('.mcmd')).toHaveText(['/garden-tools:water']);
  // By its line too.
  await search.fill('overgrown');
  await expect(sub.locator('.mcmd')).toHaveText(['/garden-tools:prune']);
  await search.fill('nothing like it');
  await expect(sub).toContainText('No command holds "nothing like it"');
  await search.fill('summariz');
  await expect(sub.locator('.mcmd')).toHaveText(['/compact']);
  await shot(main, '09-commands-search-light');
  await theme(yap, main, 'dark');
  await shot(main, '10-commands-search-dark');
  await theme(yap, main, 'light');
  // ↓ from the box to the list; Enter in the box picks the first.
  await search.fill('garden');
  await search.press('ArrowDown');
  await expect(sub.getByRole('menuitem').first()).toBeFocused();
  await search.focus();
  await search.press('Enter');
  await expect(menu(main)).toHaveCount(0);
  await expect(box(main)).toHaveValue('/garden-tools:water ');
  // The "/" list in the box filters as it's typed.
  await box(main).fill('');
  await box(main).pressSequentially('/pru');
  await expect(main.getByTestId('code-command-menu').getByRole('option').first()).toContainText('/garden-tools:prune');
  await box(main).press('Escape');
  await box(main).fill('');
});

test("a Code chat that doesn't work directly: its added folders on the plan's own session too", async ({ yap, main }) => {
  await setConfig(yap, main, { agentsDirect: false });
  await yap.invoke('e2e_claude_script', { script: writeScript('plan-chat', PLAN_CHAT) });
  try {
    await codeChat(main);
    await yap.invoke('e2e_workspace', { path: SEEDS });
    await plus(main).click();
    await menu(main).getByRole('button', { name: 'Add folder' }).click();
    await expect(main.getByTestId('chat-code-folders')).toHaveText('+1 folder');
    await closeToasts(main);
    // Typed /clear here goes as typed (Chat's own model's turn: nothing of Yap's done).
    const run = await send(yap, main, 'What seeds are in the catalogue?');
    expect(valuesOf(run, '--add-dir').map((p) => path.resolve(p))).toEqual([path.resolve(SEEDS)]);
    expect(run).toContain('--strict-mcp-config');
  } finally {
    await setConfig(yap, main, { agentsDirect: true });
  }
});
