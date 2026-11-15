// A Code chat's files and the panel beside it (15 Nov 2026, after the Claude
// app's Code tab; desktop/src-tauri/src/agents/files.rs, run_block.rs):
//
// - **File links**: a path in a Code chat's answer — inline code
//   (`src/convert.js:3`), a link to a file (`[lab notes](docs/lab.md:2)`), a
//   path in the words — is a link when it names a file in the chat's
//   workspace, and opens a read-only view of it beside the chat, scrolled to
//   the line and that line marked, with Copy path and Open in editor. A path
//   outside the workspace (`../outside/secret.txt`) is never a link, nor
//   readable by asking Rust for it. A row's file (an edit's path, a read's
//   name) opens it too.
// - **▶ Run** on a one-line shell block: under Auto it asks first, then runs
//   the line in the workspace, its output in the Runs tab beside the chat
//   (Stop, Run again, Add to message); under Bypass permissions nothing asks.
//   A block of two lines has no ▶; neither has a Chat (non-Code) chat.
// - **The summary** (the title bar's button, or the list button in the panel's row of tabs):
//   Environment (the workspace, its branch, a worktree), Context, the chat's
//   connectors, Outputs (the files its work changed) and Background tasks (a
//   run's clock and Stop, "N finished").
//
// The chat is on the plan with Yap's stand-in for Claude Code (agents-direct.spec.js'
// way); the workspace and the folder "outside" it are in this run's own folder.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, setConfig } from './support/fixtures.js';
import { fakeClaude, planConfig } from './support/claude-plan.js';
import { RUNS } from './support/yap.js';
import { newCodeChat, toSide } from './support/code-chats.js';
import { looksLikePath, pathsIn } from '../src/lib/ui/fileLinks.js';
import { runnable, blockLang } from '../src/lib/ui/codeRun.js';
import { outputItems, splitPath } from '../src/lib/views/chat/outputs.js';

const HERE = path.join(RUNS, 'code-chat-files-spec');
const SCRIPTS = path.join(HERE, 'scripts');
const GARDEN = path.join(HERE, 'folders', 'garden-planner');
const OUTSIDE = path.join(HERE, 'folders', 'outside');

const CONVERT0 = 'export function toCm(inches) {\n  return inches * 2.54;\n}\n\nexport function toInches(cm) {\n  return cm / 2.54;\n}\n';
const CONVERT1 = CONVERT0.replace('return cm / 2.54;', 'return Math.round((cm / 2.54) * 10) / 10;');
const LAB = '# Lab notes\n\nThe beds are measured in centimetres.\n';
const SAID = "I'll round toInches in src/convert.js.";
const ANSWER = [
  'Done: `src/convert.js:5` rounds now, and [the lab notes](docs/lab.md:3) say why.',
  'It reads src/convert.js only, never `../outside/secret.txt`.',
  '',
  '```powershell',
  'Write-Output "garden-run-ok"; Get-Content docs/lab.md -TotalCount 1',
  '```',
  '',
  '```bash',
  'cd src',
  'ls',
  '```',
].join('\n');

const init = () => ({ type: 'system', subtype: 'init', cwd: '{{cwd}}', session_id: '{{session_id}}', tools: ['Bash', 'Edit', 'Read', 'Write'], mcp_servers: [{ name: 'yap', status: 'connected' }], model: 'claude-haiku-5-5', permissionMode: 'auto', apiKeySource: 'none', claude_code_version: '2.1.296', capabilities: ['interrupt_receipt_v1'] });
const said = (id, content, stop = 'tool_use', usage = undefined) => ({ type: 'assistant', message: { id, type: 'message', role: 'assistant', model: 'claude-haiku-5-5', content, stop_reason: stop, ...(usage ? { usage } : {}) }, parent_tool_use_id: null, session_id: '{{session_id}}' });
const toolResult = (id, text) => ({ type: 'user', message: { role: 'user', content: [{ type: 'tool_result', tool_use_id: id, content: text }] }, parent_tool_use_id: null, session_id: '{{session_id}}' });
const result = (text) => ({ type: 'result', subtype: 'success', is_error: false, duration_ms: 900, num_turns: 2, result: text, session_id: '{{session_id}}', total_cost_usd: 0.02, usage: { input_tokens: 4000, output_tokens: 120, cache_creation_input_tokens: 0, cache_read_input_tokens: 0 }, permission_denials: [], terminal_reason: 'completed' });
/** Reads the lab notes, edits convert.js, answers with paths and two shell blocks. */
const ROUNDS = [
  { _wait_user: true },
  init(),
  said('r1', [{ type: 'text', text: SAID }]),
  said('r2', [{ type: 'tool_use', id: 'toolu_read', name: 'Read', input: { file_path: '{{cwd}}\\docs\\lab.md' } }]),
  toolResult('toolu_read', LAB),
  said('r3', [{ type: 'tool_use', id: 'toolu_edit', name: 'Edit', input: { file_path: '{{cwd}}\\src\\convert.js', old_string: 'return cm / 2.54;', new_string: 'return Math.round((cm / 2.54) * 10) / 10;' } }]),
  { _hook: { event: 'PreToolUse', input: { tool_name: 'Edit', tool_use_id: 'toolu_edit', tool_input: { file_path: 'src\\convert.js', old_string: 'return cm / 2.54;', new_string: 'return Math.round((cm / 2.54) * 10) / 10;' } } } },
  { _write_file: { path: 'src\\convert.js', text: CONVERT1 } },
  toolResult('toolu_edit', 'The file src/convert.js has been updated.'),
  // (Its last call's tokens: how full the context is, for the summary.)
  said('r4', [{ type: 'text', text: ANSWER }], 'end_turn', { input_tokens: 30_000, output_tokens: 120, cache_creation_input_tokens: 0, cache_read_input_tokens: 0 }),
  result(ANSWER),
  { _wait_user: true },
];
/** A plain chat's answer: the same words (no workspace, so nothing is a link or runs). */
const PLAIN = [{ _wait_user: true }, init(), said('p1', [{ type: 'text', text: ANSWER }], 'end_turn'), result(ANSWER), { _wait_user: true }];

function writeScript(name, lines) {
  fs.mkdirSync(SCRIPTS, { recursive: true });
  const file = path.join(SCRIPTS, `${name}.jsonl`);
  fs.writeFileSync(file, lines.map((l) => JSON.stringify(l)).join('\n') + '\n');
  return file;
}
function freshFolders() {
  fs.rmSync(path.join(HERE, 'folders'), { recursive: true, force: true });
  fs.mkdirSync(path.join(GARDEN, 'src'), { recursive: true });
  fs.mkdirSync(path.join(GARDEN, 'docs'), { recursive: true });
  fs.mkdirSync(OUTSIDE, { recursive: true });
  fs.writeFileSync(path.join(GARDEN, 'src', 'convert.js'), CONVERT0);
  fs.writeFileSync(path.join(GARDEN, 'docs', 'lab.md'), LAB);
  fs.writeFileSync(path.join(OUTSIDE, 'secret.txt'), 'not the workspace’s\n');
  // A linked worktree's `.git` (a file pointing into its repository's worktrees), on a branch.
  const gitdir = path.join(HERE, 'folders', 'repo.git', 'worktrees', 'garden-planner');
  fs.mkdirSync(gitdir, { recursive: true });
  fs.writeFileSync(path.join(gitdir, 'HEAD'), 'ref: refs/heads/claude/round-inches\n');
  fs.writeFileSync(path.join(GARDEN, '.git'), `gitdir: ${gitdir}\n`);
}

const test = base.extend({
  yapOptions: [
    async ({}, use) => {
      await use({
        name: 'code-chat-files',
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
const panel = (main) => main.getByTestId('chat-doc');
const answer = (main) => thread(main).locator('.cbubble.assistant').last();

async function send(main, text) {
  await main.getByRole('textbox', { name: 'Message' }).fill(text);
  await main.getByRole('button', { name: 'Send', exact: true }).click();
}
/** A Code chat in garden-planner, asked to round the converter: its turn done, its answer shown. Its conversation. */
async function rounded(yap, main) {
  await script(yap, ROUNDS, 'rounds');
  const before = (await agents(yap)).length;
  await openView(main, 'Chat');
  if (await panel(main).count()) await panel(main).getByRole('button', { name: 'Close the panel' }).first().click();
  await newCodeChat(main, 'garden-planner');
  await send(main, 'Round toInches to one decimal.');
  await expect.poll(async () => (await agents(yap)).length, { timeout: 20_000 }).toBe(before + 1);
  const agent = (await agents(yap))[0];
  await expect.poll(async () => (await agents(yap)).find((j) => j.id === agent.id)?.status, { timeout: 30_000 }).toBe('done');
  await expect(answer(main)).toContainText('rounds now');
  expect(fs.readFileSync(path.join(GARDEN, 'src', 'convert.js'), 'utf8')).toBe(CONVERT1);
  return agent.conversation;
}

test.beforeEach(async ({ yap, main }) => {
  freshFolders();
  await yap.invoke('e2e_claude_approve_auto', { allow: true });
  await setConfig(yap, main, { agentsEnabled: true, agentsDirect: true, agentsAsk: 'new-or-web', agentsSpeak: 'never', agentsMode: 'auto', webSearch: false, speakReplies: false, assistantScreen: false });
  if (!(await yap.invoke('workspaces_list')).some((f) => f.name === 'garden-planner')) await yap.invoke('workspaces_add', { path: GARDEN });
});

test('a path in a Code chat’s answer opens the file beside the chat at its line; one outside the workspace never does', async ({ yap, main, shot }) => {
  const conversation = await rounded(yap, main);
  const links = answer(main).locator('.md-filelink');
  // Inline code, a link to a file and a path in the words: each a link once Rust found it in the workspace.
  await expect(links).toHaveText(['src/convert.js:5', 'the lab notes', 'src/convert.js']);
  await expect(links.first()).toHaveAttribute('title', 'Open src/convert.js at line 5');
  // Outside the workspace: plain text, and Rust won't find it for anyone who asks.
  await expect(answer(main).locator('code', { hasText: '../outside/secret.txt' })).not.toHaveClass(/md-filelink/);
  expect(await yap.invoke('code_files_check', { conversation, paths: ['../outside/secret.txt', path.join(OUTSIDE, 'secret.txt'), 'src/convert.js:5'] })).toEqual([null, null, expect.objectContaining({ rel: 'src/convert.js', line: 5 })]);
  await expect(yap.invoke('code_file_read', { conversation, path: '../outside/secret.txt' })).rejects.toThrow(/isn't in this chat's workspace/);
  await shot(main, '01-file-links-in-an-answer');

  // Opened: read only, numbered, coloured, at line 5, that line marked.
  await links.first().click();
  const file = main.getByTestId('chat-file');
  await expect(file).toHaveAttribute('data-rel', 'src/convert.js');
  await expect(file.getByTestId('chat-file-name')).toHaveText('convert.js');
  await expect(file.getByTestId('chat-file-line')).toHaveText('Line 5');
  await expect(file.locator('.fl.mark')).toHaveAttribute('data-no', '5');
  await expect(file.locator('.fl.mark .ft')).toHaveText('export function toInches(cm) {');
  await expect(file.locator('.fl')).toHaveCount(CONVERT1.trimEnd().split('\n').length);
  await expect(file.getByTestId('chat-file-body')).toHaveAttribute('data-coloured', 'true');
  await expect(file.getByTestId('chat-file-facts')).toContainText('7 lines');
  // Nothing in it is editable.
  await expect(file.locator('[contenteditable], textarea, input')).toHaveCount(0);
  await shot(main, '02-the-file-beside-the-chat');
  // Open in editor: the editor at the line (a test run keeps it instead).
  await file.getByTestId('chat-file-open-editor').click();
  await expect.poll(() => yap.invoke('e2e_opened_urls')).toContainEqual('editor:src/convert.js:5');

  // The link to a file: its own tab, at its line.
  await answer(main).locator('.md-filelink', { hasText: 'the lab notes' }).click();
  await expect(file).toHaveAttribute('data-rel', 'docs/lab.md');
  await expect(file.locator('.fl.mark')).toHaveAttribute('data-no', '3');
  await expect(panel(main).getByTestId('chat-panel-tab')).toHaveText(['convert.js', 'lab.md']);

  // A row's file: the read's name, at the first line it read; the edit's path over its diff.
  const work = thread(main).getByTestId('chat-agent-work');
  await work.getByTestId('chat-agent-work-steps').first().locator('.wline').click();
  const rowFile = work.getByTestId('chat-agent-work-file');
  await expect(rowFile.first()).toHaveText('lab.md');
  await rowFile.first().click();
  await expect(file).toHaveAttribute('data-rel', 'docs/lab.md');
  // What Claude said as it worked: its path a link too.
  await expect(work.getByTestId('chat-agent-work-said').locator('.md-filelink')).toHaveText(['src/convert.js']);
  await panel(main).getByRole('button', { name: 'Close the panel' }).first().click();
});

test('▶ Run on a one-line shell block: asked first under Auto, run in the workspace, its output beside the chat; Bypass asks nothing', async ({ yap, main, shot }) => {
  const conversation = await rounded(yap, main);
  // One ▶: the PowerShell line; the two-line bash block has none.
  const run = answer(main).getByTestId('md-code-run');
  await expect(run).toHaveCount(1);
  await expect(answer(main).locator('pre.md-code').nth(1)).toHaveAttribute('data-run', 'no');

  // Auto: asked first. Cancel runs nothing.
  await run.click();
  const ask = main.getByRole('alertdialog');
  await expect(ask).toContainText('Run this command?');
  await expect(ask).toContainText('Write-Output "garden-run-ok"');
  await expect(ask).toContainText('garden-planner');
  await expect(ask).toContainText("this chat's Mode is Auto");
  await shot(main, '03-run-asks-first-under-auto');
  await ask.getByRole('button', { name: 'Cancel' }).click();
  expect(await yap.invoke('code_runs', { conversation })).toEqual([]);
  // Rust refuses a run nobody was asked about, whatever the page says.
  await expect(yap.invoke('code_run_start', { conversation, command: 'Write-Output sneaky', lang: 'powershell', asked: false })).rejects.toThrow(/Asked first/);

  // Run: it runs in the workspace (it reads docs/lab.md there), its output in the Runs tab.
  await run.click();
  await main.getByRole('alertdialog').getByRole('button', { name: 'Run' }).click();
  const runs = main.getByTestId('chat-runs');
  await expect(runs).toBeVisible();
  const first = runs.getByTestId('chat-run').first();
  await expect(first).toHaveAttribute('data-state', 'done', { timeout: 30_000 });
  await expect(first.getByTestId('chat-run-output')).toHaveText('garden-run-ok\n# Lab notes');
  await expect(first.getByTestId('chat-run-state')).toContainText('Exit 0');
  await shot(main, '04-its-output-in-the-runs-tab');
  // Add to message: the line and its output in the message box, for Claude to see.
  await first.getByTestId('chat-run-add').click();
  await expect(main.getByRole('textbox', { name: 'Message' })).toHaveValue(/I ran this in garden-planner:[\s\S]*\$ Write-Output "garden-run-ok"[\s\S]*garden-run-ok[\s\S]*\(exit 0\)/);
  await main.getByRole('textbox', { name: 'Message' }).fill('');

  // Bypass permissions: nothing asks.
  await yap.invoke('agent_bypass_confirmed');
  await yap.invoke('agent_mode_set', { conversation, mode: 'bypassPermissions' });
  await run.click();
  await expect(runs.getByTestId('chat-run')).toHaveCount(2);
  await expect(main.getByRole('alertdialog')).toHaveCount(0);
  await expect(runs.getByTestId('chat-run').nth(1)).toHaveAttribute('data-state', 'done', { timeout: 30_000 });

  // A long one: its clock and Stop, here and in the summary's Background tasks.
  const long = await yap.invoke('code_run_start', { conversation, command: 'Start-Sleep -Seconds 60', lang: 'powershell', asked: true });
  await expect(runs.locator(`[data-id="${long.id}"]`)).toHaveAttribute('data-state', 'running');
  await main.getByTestId('chat-summary-button').click();
  const tasks = main.getByTestId('chat-summary-tasks');
  await expect(tasks.locator('[data-testid="chat-summary-task"][data-kind="run"]')).toContainText('Start-Sleep -Seconds 60');
  await expect(tasks.getByTestId('chat-summary-finished')).toHaveText(/2 finished/);
  await shot(main, '05-a-run-in-background-tasks');
  await tasks.getByTestId('chat-summary-task-stop').click();
  await expect.poll(async () => (await yap.invoke('code_runs', { conversation })).find((r) => r.id === long.id)?.state, { timeout: 15_000 }).toBe('stopped');
  await expect(tasks.getByTestId('chat-summary-finished')).toHaveText(/3 finished/);
  await yap.invoke('agent_mode_set', { conversation, mode: 'auto' });
  await panel(main).getByRole('button', { name: 'Close the panel' }).first().click();
});

test('the summary: the workspace, its branch and worktree, context, connectors, the files it changed, background tasks', async ({ yap, main, shot }) => {
  await rounded(yap, main);
  // The title bar's button (a Code chat's only) opens it, and closes it again.
  const button = main.getByTestId('chat-summary-button');
  await expect(button).toBeVisible();
  await button.click();
  const summary = main.getByTestId('chat-summary');
  await expect(summary).toBeVisible();
  await expect(button).toHaveAttribute('aria-pressed', 'true');
  const env = summary.getByTestId('chat-summary-environment');
  await expect(env).toContainText('garden-planner');
  await expect(env.getByTestId('chat-summary-branch')).toContainText('claude/round-inches');
  await expect(env.getByTestId('chat-summary-worktree')).toHaveText('Worktree');
  await expect(summary.getByTestId('chat-summary-context').locator('.ctxpct')).toHaveText(/^\d+%$/);
  await expect(summary.getByTestId('chat-summary-connectors')).toContainText('None in this chat');
  // Outputs: the file its work changed, which opens beside the chat.
  const outputs = summary.getByTestId('chat-summary-outputs').getByTestId('chat-output');
  await expect(outputs).toHaveCount(1);
  await expect(outputs.first()).toHaveAttribute('data-kind', 'changed');
  await expect(outputs.first()).toContainText('convert.js');
  await expect(summary.getByTestId('chat-summary-tasks')).toContainText('Nothing running.');
  await shot(main, '06-the-summary');
  await outputs.first().click();
  await expect(main.getByTestId('chat-file')).toHaveAttribute('data-rel', 'src/convert.js');
  // With two tabs, the row: its list button shows it again; the title bar's closes the panel while it shows.
  await expect(panel(main).getByTestId('chat-panel-tab')).toHaveText(['Summary', 'convert.js']);
  await panel(main).getByTestId('chat-panel-summary').click();
  await expect(summary).toBeVisible();
  await button.click();
  await expect(panel(main)).toHaveCount(0);
  await expect(button).toHaveAttribute('aria-pressed', 'false');
});

test('a Chat (non-Code) chat: the same answer has no file links and no ▶, and no summary button', async ({ yap, main }) => {
  await script(yap, PLAIN, 'plain');
  await openView(main, 'Chat');
  await toSide(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
  await send(main, 'Where is the converter?');
  await expect(answer(main)).toContainText('rounds now', { timeout: 30_000 });
  await expect(answer(main).locator('code', { hasText: 'src/convert.js:5' })).toBeVisible();
  // (Given its moment to look: none, ever.)
  await main.waitForTimeout(500);
  await expect(answer(main).locator('.md-filelink')).toHaveCount(0);
  await expect(answer(main).getByTestId('md-code-run')).toHaveCount(0);
  // The link to a file is its words only.
  await expect(answer(main).locator('a.md-file', { hasText: 'the lab notes' })).not.toHaveAttribute('href', /.*/);
  await expect(main.getByTestId('chat-summary-button')).toHaveCount(0);
});

test('ui/fileLinks.js, ui/codeRun.js and outputs.js: what may be a path, which blocks run, the outputs list', () => {
  expect(['src/convert.js', 'docs/lab.md:105', 'src/app.ts:12:4', 'CLAUDE.md', 'C:\\Projects\\a.rs', 'src\\x.rs#L4'].map(looksLikePath)).toEqual([true, true, true, true, true, true]);
  expect(['npm test', 'v1.2', 'hello', 'a b.js', '', 'src/'].map(looksLikePath)).toEqual([false, false, false, false, false, false]);
  expect(pathsIn('See src/convert.js:5, then CLAUDE.md. Not https://example.org/a.js or v1.2.').map((p) => p.path)).toEqual(['src/convert.js:5', 'CLAUDE.md']);
  expect(runnable('npm test\n', 'bash')).toBe(true);
  expect(runnable('$ npm test', 'sh')).toBe(true);
  expect(runnable('Get-ChildItem', 'powershell')).toBe(true);
  expect(runnable('cd src\nls', 'bash')).toBe(false);
  expect(runnable('npm test', 'js')).toBe(false);
  expect(runnable('$', 'bash')).toBe(false);
  expect(blockLang('lang-Bash x')).toBe('bash');
  expect(blockLang('')).toBe('');
  expect(splitPath('src\\lib\\a.js')).toEqual({ name: 'a.js', folder: 'src/lib' });
  const agents = [
    { id: 'a', turnList: [{ n: 1, endedTs: 10, files: ['src/a.js', 'README.md'] }, { n: 2, endedTs: 30, files: ['src/a.js'] }] },
    { id: 'b', files: ['docs/old.md'], updatedTs: 5 },
  ];
  const messages = [{ role: 'assistant', files: [{ items: [{ name: 'plan.png', path: 'C:\\x\\plan.png', kind: 'image', id: 'img1' }, { name: 'todo.md', path: 'C:\\x\\todo.md', kind: 'markdown' }] }] }];
  const items = outputItems({ agents, messages });
  expect(items.map((i) => [i.kind, i.name, i.label])).toEqual([
    ['changed', 'a.js', 'Changed'],
    ['changed', 'README.md', 'Changed'],
    ['changed', 'old.md', 'Changed'],
    ['shared', 'plan.png', 'Picture'],
    ['shared', 'todo.md', 'Shared'],
  ]);
  expect(items[0].rel).toBe('src/a.js');
  expect(items[3].picture).toBe(true);
});
