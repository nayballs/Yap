// Code chats (desktop/src/lib/views/chat/codeChats.svelte.js and its parts:
// ChatSideSwitch, WorkspacePicker, CodeChips, CodeEdge + codeChat.css;
// core/codeChats.js) over desktop/src-tauri/src/jobs/code_chat.rs. With Jobs
// on, Chat has two sides, flipped by the switch at the top of its list, as
// the Claude app's sidebar has them: Chat (plain conversations, exactly as
// with Jobs off) and Code (conversations attached to one of the person's
// workspaces, the job folders, grouped by workspace). Jobs are driven as
// jobs-ui.spec.js drives them: the scripted fake AI calls the job tools, and
// Claude Code is this build's own stand-in. No one's real Claude Code,
// account or files: the workspaces are in this run's own folder.
//
// - core/codeChats.js, pure: the label, paths, sides, the Code side's groups,
//   the picker's rows.
// - The switch only with Jobs on; the Chat side plain (no Mode chip, nothing
//   job-like); the Code side's New code chat and its workspace picker (Add
//   workspace…, Manage workspaces…); a Code chat's composer: the workspace
//   chip, the Mode chip, the accent edge and its label. Light and dark.
// - A new code chat: its first question makes a Code chat (kept in
//   jobs.json, never chats.json), its prompt says so, and a job asked
//   without naming the workspace runs there; message_job without an id.
// - A plain chat that names a workspace becomes a Code chat, and Chat
//   follows it to the Code side; its next request needn't name it; Plain
//   chat turns it back, and a plain chat's rules are as before.
// - The Code side's groups: by workspace, newest first, folding, the path
//   on hover; the running jobs on top, never on the Chat side.
// - The moving light while a job runs: the Animate switch, reduced motion.
// - A removed workspace: its chats are plain again, said once.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, openSettings, closeSettings, setConfig, settingsDialog } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { RUNS } from './support/yap.js';
import { toSide, newCodeChat, workspaceMenu, sideSwitch } from './support/code-chats.js';
import { FEATURE_NAME, CODE_NAME, codeLabel, samePath, sideOf, groupByWorkspace, plainOnly, pickerRows } from '../src/lib/core/codeChats.js';

const HERE = path.join(RUNS, 'jobs-code-chats-spec');
const SCRIPTS = path.join(HERE, 'scripts');
const GARDEN = path.join(HERE, 'folders', 'garden-planner');
const RECIPES = path.join(HERE, 'folders', 'recipe-box');
const SHELF = path.join(HERE, 'folders', 'book-shelf');
const STARTED = "On it: I've started a job in garden-planner.";
const CODE_LINE = "This is a Code chat in the user's folder garden-planner";

// ---- the stand-in's scripts (as jobs-ui.spec.js has them) ----

const init = (mode) => ({
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
const result = (text, extra = {}) => ({
  type: 'result',
  subtype: 'success',
  is_error: false,
  duration_ms: 900,
  num_turns: 1,
  result: text,
  session_id: '{{session_id}}',
  total_cost_usd: 0.002,
  usage: { input_tokens: 1000, output_tokens: 40, cache_creation_input_tokens: 0, cache_read_input_tokens: 0 },
  permission_denials: [],
  terminal_reason: 'completed',
  ...extra,
});
/** Says what it was told, for every turn. */
const ECHO = [{ _wait_user: true }, init('auto'), result('Got: {{user_text}}'), { _wait_user: true }, init('auto'), result('Got: {{user_text}}'), { _wait_user: true }];
/** Takes its time (a Stop ends it). */
const SLOW = [
  { _wait_user: true },
  init('auto'),
  { type: 'assistant', message: { id: 's1', type: 'message', role: 'assistant', model: 'claude-sonnet-5-5', content: [{ type: 'tool_use', id: 'toolu_read', name: 'Read', input: { file_path: '{{cwd}}\\notes.md' } }], stop_reason: 'tool_use' }, parent_tool_use_id: null, session_id: '{{session_id}}' },
  { _sleep_ms: 60000 },
  result('Finished slowly.'),
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
      await use({ name: 'jobs-code-chats', config: { memoryLearnChats: false } });
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
const modeChip = (main) => main.getByTestId('chat-mode-chip');
const wsChip = (main) => main.getByTestId('chat-workspace-chip');
const composer = (main) => main.locator('.composer');
const tab = (main) => main.getByTestId('chat-job');
const group = (main, name) => main.locator(`[data-testid="chat-workspace-group"][data-workspace="${name}"]`);
const row = (main, id) => main.locator(`.convs [data-conv="${id}"]`);
const folderOf = (yap, conversation) => yap.invoke('job_chat_folder_get', { conversation });

async function send(main, text) {
  await main.getByRole('textbox', { name: 'Message' }).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
}

async function closeTab(main) {
  if (await tab(main).count()) await tab(main).getByRole('button', { name: 'Close the panel' }).click();
}

/** A plain new chat, on the Chat side. */
async function plainChat(main) {
  await openView(main, 'Chat');
  await closeTab(main);
  await toSide(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
}

/** A new code chat in `name`. */
async function codeChat(main, name) {
  await openView(main, 'Chat');
  await closeTab(main);
  await newCodeChat(main, name);
}

async function shownConversation(main) {
  await expect(lastAnswer(main)).toHaveAttribute('data-msg-id', /^chat:\d+:\d+$/);
  return Number(/^chat:(\d+):/.exec(await lastAnswer(main).getAttribute('data-msg-id'))[1]);
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
  await expect.poll(async () => (await yap.invoke('chat_get', { id })).messages.length, { timeout: 20_000, message: `${n} messages` }).toBe(n);
  return (await yap.invoke('chat_get', { id })).messages;
}

async function theme(yap, main, which) {
  await setConfig(yap, main, { theme: which });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe(which);
}

/** A token's colour as the page resolves it. */
const tokenColour = (main, token) =>
  main.evaluate((t) => {
    const probe = document.createElement('span');
    probe.style.color = `var(${t})`;
    document.body.appendChild(probe);
    const c = getComputedStyle(probe).color;
    probe.remove();
    return c;
  }, token);

test.beforeEach(async ({ yap, main, fakeLlm }) => {
  freshFolder(GARDEN);
  freshFolder(RECIPES);
  await yap.invoke('e2e_claude_approve_auto', { allow: true });
  await setConfig(yap, main, {
    jobsEnabled: true,
    jobsAsk: 'new-or-web',
    jobsSpeak: 'never',
    jobsPay: 'plan',
    jobsMode: 'auto',
    jobsAnimate: true,
    motion: 'system',
    webSearch: false,
    speakReplies: false,
    llmScopes: chatScope(fakeLlm),
  });
  const have = (await yap.invoke('job_folders_list')).map((f) => f.name);
  if (!have.includes('garden-planner')) await yap.invoke('job_folders_add', { path: GARDEN });
  if (!have.includes('recipe-box')) await yap.invoke('job_folders_add', { path: RECIPES });
  await script(yap, ECHO, 'echo');
});

// ---------------------------------------------------------------- no app

test('core/codeChats.js: the label, paths, sides, the Code side’s groups, the picker’s rows', () => {
  // One name for the feature, one for the side (never "Claude Code" anything).
  expect(FEATURE_NAME).toBe('Jobs');
  expect(CODE_NAME).toBe('Code');
  expect(codeLabel('garden-planner')).toBe('Code · garden-planner');
  expect(samePath('D:\\Projects\\garden-planner', 'd:/projects/garden-planner/')).toBe(true);
  expect(samePath('D:\\Projects\\garden', 'D:\\Projects\\garden-planner')).toBe(false);
  expect(samePath('', '')).toBe(false);

  const garden = { name: 'garden-planner', path: 'D:\\Projects\\garden-planner' };
  const recipes = { name: 'recipe-box', path: 'D:\\Projects\\recipe-box' };
  const chats = { 1: garden, 3: recipes, 4: garden };
  expect([sideOf(1, chats), sideOf(2, chats), sideOf(null, chats)]).toEqual(['code', 'chat', 'chat']);
  const conversations = [
    { id: 1, updatedTs: 10 },
    { id: 2, updatedTs: 50 },
    { id: 3, updatedTs: 20 },
    { id: 4, updatedTs: 30 },
  ];
  expect(plainOnly(conversations, chats).map((c) => c.id)).toEqual([2]);
  // By workspace, the newest chat's group first, each newest first.
  const groups = groupByWorkspace(conversations, chats);
  expect(groups.map((g) => [g.label, g.items.map((c) => c.id)])).toEqual([
    ['garden-planner', [4, 1]],
    ['recipe-box', [3]],
  ]);
  expect(groups[0]).toMatchObject({ path: garden.path, collapsible: true, collapsed: false, count: 2 });
  // Folded: the header stays, with how many.
  const folded = groupByWorkspace(conversations, chats, { [garden.path]: true });
  expect(folded[0]).toMatchObject({ collapsed: true, count: 2, items: [] });
  // The picker: Rust's order, the one in use marked.
  expect(pickerRows([recipes, garden], { ...garden, path: 'd:/projects/garden-planner' }).map((w) => [w.name, w.on])).toEqual([
    ['recipe-box', false],
    ['garden-planner', true],
  ]);
});

// ---------------------------------------------------------------- the switch and the picker

test('the switch: only with Jobs on, a plain Chat side, the Code side’s picker, and a Code chat’s composer', async ({ yap, main, shot }) => {
  // Jobs off: Chat exactly as before.
  await setConfig(yap, main, { jobsEnabled: false });
  await openView(main, 'Chat');
  await closeTab(main);
  await expect(sideSwitch(main)).toHaveCount(0);
  await main.getByRole('button', { name: 'New chat' }).click();
  await expect(wsChip(main)).toHaveCount(0);
  await expect(modeChip(main)).toHaveCount(0);

  // Jobs on: the switch, Chat | Code, with their tooltips.
  await setConfig(yap, main, { jobsEnabled: true });
  await expect(sideSwitch(main)).toBeVisible();
  const chatSeg = sideSwitch(main).getByRole('radio', { name: 'Chat', exact: true });
  const codeSeg = sideSwitch(main).getByRole('radio', { name: 'Code', exact: true });
  await expect(chatSeg).toHaveAttribute('title', 'Chat');
  await expect(codeSeg).toHaveAttribute('title', 'Code');
  await toSide(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
  // The Chat side: a plain chat, nothing job-like.
  await expect(wsChip(main)).toHaveCount(0);
  await expect(modeChip(main)).toHaveCount(0);
  await expect(composer(main)).not.toHaveClass(/code-chat/);
  await expect(main.getByTestId('chat-code-label')).toHaveCount(0);
  await shot(main, '01-chat-side-plain-composer-light');

  // The Code side: New code chat opens the workspace picker.
  await codeSeg.click();
  await expect(sideSwitch(main)).toHaveAttribute('data-side', 'code');
  await expect(main.getByRole('button', { name: 'New chat', exact: true })).toHaveCount(0);
  await main.getByRole('button', { name: 'New code chat' }).click();
  const menu = workspaceMenu(main);
  await expect(menu).toBeVisible();
  await expect(menu.getByRole('menuitemradio')).toHaveCount(2);
  await expect(menu.getByRole('menuitemradio', { name: 'garden-planner' })).toHaveAttribute('title', GARDEN);
  await expect(menu.getByRole('menuitemradio', { name: 'recipe-box' })).toHaveAttribute('title', RECIPES);
  await expect(menu.getByRole('menuitem', { name: 'Add workspace…' })).toBeVisible();
  await expect(menu.getByRole('menuitem', { name: 'Manage workspaces…' })).toBeVisible();
  // (New code chat's picker never offers Plain chat.)
  await expect(menu.getByRole('menuitem', { name: /Plain chat/ })).toHaveCount(0);
  await expect(menu.getByRole('menuitemradio').first()).toBeFocused();
  await shot(main, '02-new-code-chat-picker-light');

  // Add workspace…: the folder picker (test mode: the folder this test names), added and picked.
  freshFolder(SHELF);
  await yap.invoke('job_folders_remove', { name: 'book-shelf' });
  await yap.invoke('e2e_job_folder', { path: SHELF });
  await menu.getByRole('menuitem', { name: 'Add workspace…' }).click();
  await expect(menu).toHaveCount(0);
  await expect(wsChip(main)).toHaveAttribute('data-workspace', 'book-shelf');
  expect((await yap.invoke('job_folders_list')).map((f) => f.name)).toContain('book-shelf');
  // Used last: first in the picker now (most recently used first).
  await main.getByRole('button', { name: 'New code chat' }).click();
  await expect(menu.getByRole('menuitemradio')).toHaveCount(3);
  await expect(menu.getByRole('menuitemradio').first()).toHaveAttribute('data-workspace', 'book-shelf');
  await menu.getByRole('menuitemradio', { name: 'garden-planner' }).click();

  // A Code chat's composer: the workspace chip, the Mode chip beside it, the accent edge and its label.
  await expect(wsChip(main)).toHaveText('garden-planner');
  await expect(wsChip(main)).toHaveAttribute('title', GARDEN);
  await expect(modeChip(main)).toBeVisible();
  await expect(composer(main)).toHaveClass(/code-chat/);
  await expect(main.getByTestId('chat-code-label')).toHaveText('Code · garden-planner');
  // The edge: the accent's line, the accent itself while the box has the focus.
  const edge = async () => main.evaluate(() => getComputedStyle(document.querySelector('.composer.code-chat > .inputbar')).borderTopColor);
  await main.getByRole('textbox', { name: 'Message' }).focus();
  expect(await edge()).toBe(await tokenColour(main, '--yap-code'));
  await main.getByRole('textbox', { name: 'Message' }).blur();
  expect(await edge()).toBe(await tokenColour(main, '--yap-code-line'));
  expect(await tokenColour(main, '--yap-code')).not.toBe(await tokenColour(main, '--yap-warning'));
  await shot(main, '03-code-composer-light');
  // The chip's own picker: switch the workspace, or Plain chat.
  await wsChip(main).click();
  await expect(menu.getByRole('menuitemradio', { name: 'garden-planner' })).toHaveAttribute('aria-checked', 'true');
  await expect(menu.getByRole('menuitem', { name: /Plain chat/ })).toBeVisible();
  await shot(main, '04-workspace-chip-picker-light');
  await main.keyboard.press('Escape');
  await expect(menu).toHaveCount(0);
  await expect(wsChip(main)).toBeFocused();

  // Manage workspaces…: Settings → Jobs.
  await wsChip(main).click();
  await menu.getByRole('menuitem', { name: 'Manage workspaces…' }).click();
  await expect(settingsDialog(main).getByRole('heading', { level: 1, name: 'Jobs' })).toBeVisible();
  await expect(settingsDialog(main).locator('#settings-jobs-folders')).toContainText('Workspaces');
  await closeSettings(main);

  // Dark: the Code composer, its picker, and the plain one.
  await theme(yap, main, 'dark');
  await expect(wsChip(main)).toHaveText('garden-planner');
  await main.getByRole('textbox', { name: 'Message' }).blur();
  expect(await edge()).toBe(await tokenColour(main, '--yap-code-line'));
  await shot(main, '05-code-composer-dark');
  await wsChip(main).click();
  await shot(main, '06-workspace-chip-picker-dark');
  await main.keyboard.press('Escape');
  await main.getByRole('button', { name: 'New code chat' }).click();
  await shot(main, '07-new-code-chat-picker-dark');
  await main.keyboard.press('Escape');
  await toSide(main, 'Chat');
  await expect(composer(main)).not.toHaveClass(/code-chat/);
  await shot(main, '08-chat-side-plain-composer-dark');
  await theme(yap, main, 'light');
});

// ---------------------------------------------------------------- a new code chat

test('a new code chat: its first question makes a Code chat, and a job asked without naming the workspace runs there', async ({ yap, main, fakeLlm }) => {
  const before = (await jobs(yap)).length;
  await codeChat(main, 'garden-planner');
  fakeLlm.scriptChat([
    { tools: [{ name: 'start_job', args: { brief: 'Tidy notes.md.' } }] },
    { content: STARTED },
    { plain: true, content: 'It tidied notes.md.' },
  ]);
  // Not a word about the folder: the chat's workspace, and the person's own request, are enough.
  await send(main, 'Tidy notes.md.');
  await expect(lastAnswer(main)).toHaveText(STARTED);
  await expect(chipsOfLast(main)).toHaveText(['Started a job in garden-planner']);
  const job = await ended(yap, (await newJob(yap, before)).id);
  expect(job).toMatchObject({ status: 'done', folder: 'garden-planner', path: GARDEN, brief: 'Tidy notes.md.', mode: 'auto' });
  // The model was told: the chat's line in the question's context.
  const asked = fakeLlm.requests.filter((r) => r.tools.includes('start_job') && JSON.stringify(r.messages).includes('Tidy notes.md.'))[0];
  expect(JSON.stringify(asked.messages)).toContain(CODE_LINE);
  expect(asked.system).toContain("this isn't a Code chat");
  // Kept in jobs.json, beside its Mode — never in chats.json.
  const conversation = job.conversation;
  expect(await folderOf(yap, conversation)).toMatchObject({ name: 'garden-planner', path: GARDEN });
  expect(yap.readJson('jobs.json').codeChats[String(conversation)].path).toBe(GARDEN);
  const saved = (yap.readJson('chats.json').conversations ?? []).find((c) => c.id === conversation);
  expect(JSON.stringify(saved ?? {})).not.toContain(GARDEN.replace(/\\/g, '\\\\'));
  // In the list: under its workspace, on the Code side (the job's tab closed: the list is back).
  await closeTab(main);
  await expect(group(main, 'garden-planner')).toHaveAttribute('title', GARDEN);
  await expect(row(main, conversation)).toBeVisible();
  await messagesOf(yap, conversation, 4);

  // message_job without an id: this chat's latest job, in its workspace.
  fakeLlm.scriptChat([
    { tools: [{ name: 'message_job', args: { text: 'Also sort the list.' } }] },
    { content: 'Told it.' },
    { plain: true, content: 'It sorted the list too.' },
  ]);
  await closeTab(main);
  await send(main, 'Also sort the list.');
  await expect(chipsOfLast(main)).toHaveText(['Told the job in garden-planner']);
  await expect.poll(async () => (await jobs(yap)).find((j) => j.id === job.id).turns, { timeout: 20_000 }).toBe(2);
  const told = await ended(yap, job.id);
  expect(told.text).toContain('Got: Also sort the list.');
  await messagesOf(yap, conversation, 8);
});

// ---------------------------------------------------------------- a plain chat becomes one

test('a plain chat that names a workspace becomes a Code chat, Chat follows it, and Plain chat turns it back', async ({ yap, main, fakeLlm, shot }) => {
  const before = (await jobs(yap)).length;
  await plainChat(main);
  await expect(modeChip(main)).toHaveCount(0);
  fakeLlm.scriptChat([
    { tools: [{ name: 'start_job', args: { folder: 'garden-planner', brief: 'Tidy notes.md.' } }] },
    { content: STARTED },
    { plain: true, content: 'It tidied notes.md.' },
  ]);
  await send(main, 'Start a job in garden-planner to tidy notes.md.');
  await expect(lastAnswer(main)).toHaveText(STARTED);
  const first = await ended(yap, (await newJob(yap, before)).id);
  const conversation = first.conversation;
  // A Code chat now, and Chat followed it to the Code side.
  await expect.poll(() => folderOf(yap, conversation)).toMatchObject({ name: 'garden-planner' });
  await expect(sideSwitch(main)).toHaveAttribute('data-side', 'code');
  await expect(wsChip(main)).toHaveText('garden-planner');
  await expect(modeChip(main)).toBeVisible();
  await expect(composer(main)).toHaveClass(/code-chat/);
  await messagesOf(yap, conversation, 4);
  await closeTab(main);
  await expect(group(main, 'garden-planner').locator('xpath=following-sibling::*[1]')).toBeVisible();
  await expect(row(main, conversation)).toBeVisible();
  await shot(main, '09-plain-chat-moved-to-code-light');

  // Its next request needn't name the workspace.
  fakeLlm.scriptChat([
    { tools: [{ name: 'start_job', args: { brief: 'Fix the headings in notes.md.' } }] },
    { content: STARTED },
    { plain: true, content: 'It fixed the headings.' },
  ]);
  await send(main, 'Now fix the headings in notes.md.');
  await expect(chipsOfLast(main)).toHaveText(['Started a job in garden-planner']);
  const second = await ended(yap, (await newJob(yap, before + 1)).id);
  expect(second).toMatchObject({ folder: 'garden-planner', path: GARDEN, conversation });
  await messagesOf(yap, conversation, 8);

  // Plain chat (the chip's picker): back to the Chat side, nothing job-like.
  await closeTab(main);
  await wsChip(main).click();
  await workspaceMenu(main).getByRole('menuitem', { name: /Plain chat/ }).click();
  await expect.poll(() => folderOf(yap, conversation)).toBeNull();
  await expect(sideSwitch(main)).toHaveAttribute('data-side', 'chat');
  await expect(wsChip(main)).toHaveCount(0);
  await expect(modeChip(main)).toHaveCount(0);
  await expect(composer(main)).not.toHaveClass(/code-chat/);
  await expect(row(main, conversation)).toBeVisible();
  // A plain chat's rules, as before: a job left unnamed isn't one.
  fakeLlm.scriptChat([
    { tools: [{ name: 'start_job', args: { brief: 'Tidy notes.md again.' } }] },
    { content: 'Which folder?' },
  ]);
  await send(main, 'Tidy notes.md again.');
  await expect(chipsOfLast(main)).toHaveText(["Didn't start a job: which folder?"]);
  const told = JSON.parse(fakeLlm.requests.at(-1).messages.filter((m) => m.role === 'tool').at(-1).content);
  expect(told.error).toContain('no folder was named');
  expect(told.error).toContain('never guess one');
  expect((await jobs(yap)).length).toBe(before + 2);
  const last = fakeLlm.requests.filter((r) => r.tools.includes('start_job')).at(-1);
  expect(JSON.stringify(last.messages)).not.toContain(CODE_LINE);
});

// ---------------------------------------------------------------- the sidebar

test('the Code side: grouped by workspace, newest first, folding, the running jobs on top', async ({ yap, main, fakeLlm, shot }) => {
  const asked = async (name, text) => {
    await codeChat(main, name);
    fakeLlm.scriptChat([{ content: `${text}.` }]);
    await send(main, text);
    await expect(lastAnswer(main)).toHaveText(`${text}.`);
    return shownConversation(main);
  };
  const one = await asked('garden-planner', 'Plan the beds');
  const two = await asked('recipe-box', 'List the soups');
  const three = await asked('garden-planner', 'Water the herbs');

  // Grouped by workspace, the newest group first, each newest first.
  const groups = main.getByTestId('chat-workspace-group');
  await expect(groups.first()).toHaveAttribute('data-workspace', 'garden-planner');
  await expect(group(main, 'recipe-box')).toBeVisible();
  const order = await main.evaluate(() => [...document.querySelectorAll('.convscroll [data-conv], .convscroll [data-testid="chat-workspace-group"]')].map((e) => e.dataset.workspace ?? Number(e.dataset.conv)));
  const at = (x) => order.indexOf(x);
  expect(at('garden-planner')).toBeLessThan(at(three));
  expect(at(three)).toBeLessThan(at(one));
  expect(at(one)).toBeLessThan(at('recipe-box'));
  expect(at('recipe-box')).toBeLessThan(at(two));
  await expect(group(main, 'garden-planner')).toHaveAttribute('title', GARDEN);
  await shot(main, '10-code-side-groups-light');

  // Folding: the header stays, with how many (earlier tests' chats there too); kept as it was.
  const listed = new Set((await yap.invoke('chats_list')).map((c) => c.id));
  const inGarden = Object.entries((await yap.invoke('job_code_chats')).chats).filter(([id, w]) => w.name === 'garden-planner' && listed.has(Number(id))).length;
  expect(inGarden).toBeGreaterThanOrEqual(2);
  await group(main, 'garden-planner').click();
  await expect(group(main, 'garden-planner')).toHaveAttribute('aria-expanded', 'false');
  await expect(row(main, one)).toHaveCount(0);
  await expect(row(main, three)).toHaveCount(0);
  await expect(group(main, 'garden-planner').locator('.gcount')).toHaveText(String(inGarden));
  await expect(row(main, two)).toBeVisible();
  await group(main, 'garden-planner').click();
  await expect(row(main, one)).toBeVisible();

  // The Chat side never shows them.
  await toSide(main, 'Chat');
  for (const id of [one, two, three]) await expect(row(main, id)).toHaveCount(0);
  await toSide(main, 'Code');

  // A job running: on top of the Code side, never on the Chat side (a dot on Code instead).
  await script(yap, SLOW, 'slow-list');
  const before = (await jobs(yap)).length;
  await main.locator(`.convs [data-conv="${three}"]`).click();
  fakeLlm.scriptChat([{ tools: [{ name: 'start_job', args: { brief: 'Read notes.md slowly.' } }] }, { content: STARTED }]);
  await send(main, 'Read notes.md slowly.');
  const slow = await newJob(yap, before);
  await expect.poll(async () => (await jobs(yap)).find((j) => j.id === slow.id)?.status).toBe('running');
  await closeTab(main);
  await expect(main.getByTestId('chat-running-jobs')).toBeVisible();
  await shot(main, '11-code-side-running-light');
  await theme(yap, main, 'dark');
  await shot(main, '12-code-side-running-dark');
  await theme(yap, main, 'light');
  await toSide(main, 'Chat');
  await expect(main.getByTestId('chat-running-jobs')).toHaveCount(0);
  await expect(main.getByTestId('chat-side-busy')).toBeVisible();
  await shot(main, '13-chat-side-while-a-job-runs-light');
  await yap.invoke('job_stop', { id: slow.id });
  await ended(yap, slow.id);
  await expect(main.getByTestId('chat-side-busy')).toHaveCount(0);
});

// ---------------------------------------------------------------- the moving light

test('the moving light while a job runs: Animate Code chats, and reduced motion', async ({ yap, main, fakeLlm, shot }) => {
  await script(yap, SLOW, 'slow-light');
  const before = (await jobs(yap)).length;
  await codeChat(main, 'garden-planner');
  const light = main.getByTestId('chat-code-light');
  await expect(light).toHaveCount(0);
  fakeLlm.scriptChat([{ tools: [{ name: 'start_job', args: { brief: 'Read notes.md slowly.' } }] }, { content: STARTED }]);
  await send(main, 'Read notes.md slowly.');
  const job = await newJob(yap, before);
  await expect.poll(async () => (await jobs(yap)).find((j) => j.id === job.id)?.status).toBe('running');
  await closeTab(main);
  // Running here, animation on: the light goes round the edge.
  await expect(light).toBeVisible();
  expect(await light.evaluate((el) => getComputedStyle(el).animationName)).toBe('yap-code-light');
  await shot(main, '14-code-composer-job-running-light');

  // Animate Code chats off: the edge stays, still.
  await setConfig(yap, main, { jobsAnimate: false });
  await expect(light).toHaveCount(0);
  await expect(composer(main)).toHaveClass(/code-chat/);
  await setConfig(yap, main, { jobsAnimate: true });
  await expect(light).toBeVisible();

  // Reduced motion (Settings → General → Appearance): never.
  await setConfig(yap, main, { motion: 'reduced' });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.motion)).toBe('reduced');
  await expect(light).toBeHidden();
  expect(await light.evaluate((el) => getComputedStyle(el).animationName)).toBe('none');
  await setConfig(yap, main, { motion: 'system' });

  // Over: no light.
  await yap.invoke('job_stop', { id: job.id });
  await ended(yap, job.id);
  await expect(light).toHaveCount(0);
  await expect(composer(main)).toHaveClass(/code-chat/);
});

// ---------------------------------------------------------------- a removed workspace

test('a workspace removed in Settings turns its chats back to plain, said once', async ({ yap, main, fakeLlm }) => {
  await codeChat(main, 'recipe-box');
  fakeLlm.scriptChat([{ content: 'Soups first.' }]);
  await send(main, 'Which recipes first?');
  await expect(lastAnswer(main)).toHaveText('Soups first.');
  const conversation = await shownConversation(main);
  expect(await folderOf(yap, conversation)).toMatchObject({ name: 'recipe-box' });

  const dialog = await openSettings(main, 'Jobs');
  await dialog.locator('[data-testid="jobs-folder"][data-name="recipe-box"]').getByRole('button', { name: 'Remove' }).click();
  await expect(main.getByText('“recipe-box” removed')).toBeVisible();
  // (Earlier tests may have left more of recipe-box's chats: "Its 2 Code chats are plain chats now.")
  await expect(main.getByText(/Its (\d+ )?Code chats? (is a plain chat|are plain chats) now\./)).toHaveCount(1);
  await closeSettings(main);

  // Plain again, and Chat follows it to the Chat side.
  expect(await folderOf(yap, conversation)).toBeNull();
  expect(yap.readJson('jobs.json').codeChats?.[String(conversation)]).toBeUndefined();
  await expect(sideSwitch(main)).toHaveAttribute('data-side', 'chat');
  await expect(row(main, conversation)).toBeVisible();
  await expect(wsChip(main)).toHaveCount(0);
  await expect(modeChip(main)).toHaveCount(0);
});
