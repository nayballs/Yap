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
//   Environment (the workspace, its branch, a worktree, Remote Control's
//   switch), Changes (the branch's pull requests from the person's own `gh`:
//   here `yap.exe fake-gh`), Context (broken down from Claude Code's own
//   `/context`, run for the session without writing to it: the stand-in's
//   `_context`; with no breakdown, the one number), the chat's connectors,
//   Outputs (the files its work changed, each with a right-click menu) and
//   Background tasks (a run's clock and Stop, "N finished"). A merged pull
//   request's Archive archives the chat.
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
import { contextView, fmtTokens, fmtShare, shortPath, prsNote } from '../src/lib/views/chat/contextBreakdown.js';

const HERE = path.join(RUNS, 'code-chat-files-spec');
const SCRIPTS = path.join(HERE, 'scripts');
const GARDEN = path.join(HERE, 'folders', 'garden-planner');
const OUTSIDE = path.join(HERE, 'folders', 'outside');
// What the stand-ins were started with: Claude Code's `/context`, GitHub's `gh`.
const CONTEXT_DUMP = path.join(HERE, 'context-runs.jsonl');
const GH_DUMP = path.join(HERE, 'gh-runs.jsonl');

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
/** A command's files (15 Nov 2026, agents/changes.rs): a script it runs writes costs.csv and tidies docs/lab.md — no
 * edit tool, so no backup hook — and it answers. */
const COSTS = 'item,cost\nseeds,4.50\ncompost,12.00\n';
const LAB_TIDIED = LAB.replace('The beds', 'All the beds');
const COSTS_SAID = 'Done: costs.csv has the costs.';
const MAKES_COSTS = [
  { _wait_user: true },
  init(),
  said('c1', [{ type: 'tool_use', id: 'toolu_make', name: 'Bash', input: { command: 'node make-costs.js', description: 'Make the costs sheet' } }]),
  { _write_file: { path: 'costs.csv', text: COSTS } },
  { _write_file: { path: 'docs\\lab.md', text: LAB_TIDIED } },
  toolResult('toolu_make', 'Wrote costs.csv and tidied docs/lab.md'),
  said('c2', [{ type: 'text', text: COSTS_SAID }], 'end_turn'),
  result(COSTS_SAID),
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
  // Its repository's config, with a GitHub remote (a worktree's is its repository's: `commondir`).
  fs.writeFileSync(path.join(gitdir, 'commondir'), '../..\n');
  fs.writeFileSync(path.join(HERE, 'folders', 'repo.git', 'config'), '[core]\n\tbare = false\n[remote "origin"]\n\turl = https://github.com/example-org/garden-planner.git\n');
}

/** Claude Code's `/context` for the chat's session, as 2.1.296 answers it (a fictional setup). */
const CATEGORIES = [
  ['System prompt', 4324, 'used'],
  ['System tools', 14959, 'used'],
  ['MCP tools', 2050, 'used'],
  ['MCP server instructions', 1031, 'used'],
  ['MCP tools (deferred)', 12410, 'deferred'],
  ['System tools (deferred)', 17928, 'deferred'],
  ['Custom agents', 220, 'used'],
  ['Memory files', 3612, 'used'],
  ['Skills', 1166, 'used'],
  ['Messages', 129046, 'used'],
  ['Autocompact buffer', 33000, 'buffer'],
];
const USED = CATEGORIES.filter((c) => c[2] === 'used').reduce((sum, c) => sum + c[1], 0);
const USAGE = {
  model: 'claude-haiku-5-5',
  total_tokens: USED,
  raw_max_tokens: 1_000_000,
  percentage: 16,
  categories: [...CATEGORIES, ['Free space', 1_000_000 - USED - 33000, 'free']].map(([name, tokens, kind]) => ({ name, tokens, kind })),
  mcp_tools: [
    { name: 'mcp__notes__search', server_name: 'notes', tokens: 640 },
    { name: 'mcp__notes__read', server_name: 'notes', tokens: 410 },
    { name: 'mcp__calendar__list_events', server_name: 'calendar', tokens: 1000 },
  ],
  memory_files: [
    { path: 'C:\\Users\\sam\\.claude\\CLAUDE.md', type: 'User', tokens: 700 },
    { path: 'C:\\Projects\\garden-planner\\CLAUDE.md', type: 'Project', tokens: 2912 },
  ],
  agents: [{ agent_type: 'reviewer', source: 'userSettings', tokens: 120 }],
  skills: [
    { name: 'release-notes', source: 'userSettings', tokens: 237 },
    { name: 'pdf', source: 'plugin', tokens: 904 },
  ],
};
/** `gh pr list --json …`'s answer for the branch: one open (its checks running), one merged. */
const PULLS = [
  { number: 117, title: 'Measure the beds in centimetres', state: 'MERGED', isDraft: false, url: 'https://github.com/example-org/garden-planner/pull/117', statusCheckRollup: [{ __typename: 'CheckRun', status: 'COMPLETED', conclusion: 'SUCCESS', name: 'check' }] },
  { number: 131, title: 'Round toInches to one decimal', state: 'OPEN', isDraft: false, url: 'https://github.com/example-org/garden-planner/pull/131', statusCheckRollup: [{ __typename: 'CheckRun', status: 'IN_PROGRESS', conclusion: '', name: 'e2e' }] },
];
function writeGh(name, body) {
  fs.mkdirSync(SCRIPTS, { recursive: true });
  const file = path.join(SCRIPTS, `${name}.json`);
  fs.writeFileSync(file, JSON.stringify(body));
  return file;
}
const dumped = (file) => (fs.existsSync(file) ? fs.readFileSync(file, 'utf8').trim().split('\n').filter(Boolean).map((l) => JSON.parse(l)) : []);
async function theme(yap, main, which) {
  await setConfig(yap, main, { theme: which });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe(which);
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
async function rounded(yap, main, lines = ROUNDS) {
  await script(yap, lines, 'rounds');
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
  // No gh on this PC unless a test says otherwise.
  await yap.invoke('e2e_gh_script', { script: null });
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
  // No breakdown (this stand-in's `/context` says nothing readable): the one number, never an error.
  await expect(summary.getByTestId('chat-summary-context').locator('.ctxpct')).toHaveText(/^\d+%$/);
  await expect(summary.getByTestId('chat-summary-context-head')).toHaveCount(0);
  // No gh on this PC: one quiet line.
  await expect(summary.getByTestId('chat-summary-changes-note')).toHaveText("Install GitHub's gh to see this branch's pull requests.");
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
  // With two tabs: the title bar's button shows it again (the row has no copy of it), and closes the panel while it shows.
  await expect(panel(main).getByTestId('chat-panel-tab')).toHaveText(['Summary', 'convert.js']);
  await expect(panel(main).getByTestId('chat-panel-summary')).toHaveCount(0);
  await expect(button).toHaveAttribute('aria-pressed', 'false');
  await button.click();
  await expect(summary).toBeVisible();
  await button.click();
  await expect(panel(main)).toHaveCount(0);
  await expect(button).toHaveAttribute('aria-pressed', 'false');
});

test('the summary after the Claude app: context by category from /context, the branch’s pull requests, Remote Control, an output’s menu, Archive', async ({ yap, main, shot }) => {
  fs.rmSync(CONTEXT_DUMP, { force: true });
  fs.rmSync(GH_DUMP, { force: true });
  await yap.invoke('e2e_gh_script', { script: writeGh('gh-list', { stdout: PULLS, dump: GH_DUMP }) });
  const conversation = await rounded(yap, main, [{ _context: { usage: USAGE, markdown: '## Context Usage', dump: CONTEXT_DUMP } }, ...ROUNDS]);
  const agent = (await agents(yap)).find((a) => a.conversation === conversation);
  await main.getByTestId('chat-summary-button').click();
  const summary = main.getByTestId('chat-summary');

  // Context: Claude Code's own /context for the chat's session, drawn as the Claude app draws it.
  const context = summary.getByTestId('chat-summary-context');
  await expect(context.getByTestId('chat-summary-context-head')).toHaveText('16% used · 156k / 1M');
  await expect
    .poll(() => context.getByTestId('chat-summary-context-cat').evaluateAll((els) => els.map((e) => e.dataset.name)))
    .toEqual(['Messages', 'Memory files', 'System tools', 'MCP tools', 'System prompt', 'Skills', 'MCP server instructions', 'Custom agents', 'Autocompact buffer', 'Free space']);
  const messagesRow = context.locator('[data-testid="chat-summary-context-cat"][data-name="Messages"]');
  await expect(messagesRow).toContainText('129k');
  await expect(messagesRow).toContainText('12.9%');
  // The deferred ones: their tokens, no share of the window.
  const deferred = context.getByTestId('chat-summary-context-deferred');
  await expect(deferred).toHaveCount(2);
  await expect(deferred.first()).toContainText('MCP tools (deferred)');
  await expect(deferred.first().locator('.cp')).toHaveText('');
  // The lists behind them, folded, each with its count.
  const lists = context.getByTestId('chat-summary-context-list');
  await expect(lists.locator('summary')).toHaveText([/MCP tools\s*3/, /Memory files\s*2/, /Custom agents\s*1/, /Skills\s*2/]);
  await lists.first().locator('summary').click();
  await expect(lists.first().locator('.crow')).toHaveCount(3);
  await expect(lists.first().locator('.crow').first()).toContainText('mcp__notes__search');
  await expect(lists.first().locator('.crow').first()).toContainText('notes');
  // Run for the session, in its workspace, never written into it.
  const runs = dumped(CONTEXT_DUMP);
  expect(runs.length).toBeGreaterThanOrEqual(1);
  for (const r of runs) {
    expect(r.args.slice(0, 2)).toEqual(['-p', '/context']);
    expect(r.args[r.args.indexOf('--resume') + 1]).toBe(agent.session);
    expect(r.args).toContain('--no-session-persistence');
    expect(r.persist).toBe(false);
    expect(path.resolve(r.cwd).toLowerCase()).toBe(path.resolve(GARDEN).toLowerCase());
  }

  // Changes: the branch's pull requests, newest first, from the person's own gh.
  const changes = summary.getByTestId('chat-summary-changes');
  const pulls = changes.getByTestId('chat-summary-pr');
  await expect(pulls).toHaveCount(2);
  await expect(pulls.first()).toHaveAttribute('data-state', 'open');
  await expect(pulls.first()).toHaveAttribute('data-checks', 'running');
  await expect(pulls.first()).toContainText('Round toInches to one decimal');
  await expect(pulls.first()).toContainText('example-org/garden-planner #131');
  await expect(pulls.first()).toContainText('Checks running');
  await expect(pulls.nth(1)).toHaveAttribute('data-state', 'merged');
  await expect(pulls.nth(1)).toContainText('Checks passing');
  await expect(pulls.first().getByTestId('chat-summary-pr-archive')).toHaveCount(0);
  await expect(pulls.nth(1).getByTestId('chat-summary-pr-archive')).toBeVisible();
  const gh = dumped(GH_DUMP)[0];
  expect(gh.args).toEqual(expect.arrayContaining(['pr', 'list', '--head=claude/round-inches', '--state', 'all']));
  expect(gh.prompts).toBe(false);
  expect(path.resolve(gh.cwd).toLowerCase()).toBe(path.resolve(GARDEN).toLowerCase());

  // Remote Control's switch: the "Code · garden-planner" card's; with Settings → Phone's off, asked first.
  const rc = summary.getByTestId('chat-summary-remote-switch');
  await expect(rc).toHaveAttribute('aria-checked', 'false');
  await theme(yap, main, 'light');
  await shot(main, '07-the-summary-broken-down');
  await theme(yap, main, 'dark');
  await shot(main, '07-the-summary-broken-down-dark');
  await theme(yap, main, 'light');
  await rc.click();
  const ask = main.getByRole('alertdialog', { name: 'Turn on Remote Control?' });
  await expect(ask).toBeVisible();
  await ask.getByRole('button', { name: 'Cancel' }).click();
  await expect(ask).toHaveCount(0);
  await expect(rc).toHaveAttribute('aria-checked', 'false');

  // A pull request opens in the person's browser (a test run keeps the link).
  await pulls.first().locator('.propen').click();
  await expect.poll(() => yap.invoke('e2e_opened_urls')).toContainEqual('https://github.com/example-org/garden-planner/pull/131');

  // An output's right-click menu: Open, Show in folder, Copy path, Open in editor.
  const output = summary.getByTestId('chat-summary-outputs').getByTestId('chat-output').first();
  const menuItems = async () => {
    await output.click({ button: 'right' });
    const names = await main.getByRole('menu').getByRole('menuitem').allTextContents();
    await main.keyboard.press('Escape');
    return names.map((n) => n.trim());
  };
  await expect.poll(menuItems).toEqual(['Open', 'Show in folder', 'Copy path', 'Open in editor']);
  await output.click({ button: 'right' });
  await main.getByRole('menu').getByRole('menuitem', { name: 'Show in folder' }).click();
  await expect.poll(() => yap.invoke('e2e_opened_urls')).toContainEqual('reveal:src/convert.js');
  await output.click({ button: 'right' });
  await main.getByRole('menu').getByRole('menuitem', { name: 'Open in editor' }).click();
  await expect.poll(() => yap.invoke('e2e_opened_urls')).toContainEqual('editor:src/convert.js:0');

  // A merged pull request's Archive: this chat archived, as the list archives it.
  await pulls.nth(1).getByTestId('chat-summary-pr-archive').click();
  await expect.poll(async () => (await yap.invoke('chats_archived')).some((c) => c.id === conversation)).toBe(true);
  await expect(main.getByText('Chat archived')).toBeVisible();
  await yap.invoke('chat_archive', { ids: [conversation], archived: false });
  await yap.invoke('e2e_gh_script', { script: null });
});

test('a file a command made is in Outputs and opens as a sheet; Files changed says which a command changed with no backup; Undo deletes the new one and leaves the other', async ({ yap, main, shot }) => {
  test.setTimeout(120_000);
  await script(yap, MAKES_COSTS, 'makes-costs');
  const before = (await agents(yap)).length;
  await openView(main, 'Chat');
  if (await panel(main).count()) await panel(main).getByRole('button', { name: 'Close the panel' }).first().click();
  await newCodeChat(main, 'garden-planner');
  await send(main, 'Make costs.csv with the seed and compost costs.');
  await expect.poll(async () => (await agents(yap)).length, { timeout: 20_000 }).toBe(before + 1);
  const id = (await agents(yap))[0].id;
  await expect.poll(async () => (await agents(yap)).find((j) => j.id === id)?.status, { timeout: 30_000 }).toBe('done');
  await expect(answer(main)).toContainText(COSTS_SAID);
  // Found by comparing the workspace before and after the turn: the new file and the one the command changed, each said
  // as it is (no edit tool ran, so nothing went through the backup hook).
  const agent = (await agents(yap)).find((j) => j.id === id);
  expect(agent.turnList[0].files).toEqual(['costs.csv', 'docs\\lab.md']);
  expect(agent.turnList[0].byCommand).toEqual([
    { file: 'costs.csv', new: true, kept: false },
    { file: 'docs\\lab.md', new: false, kept: false },
  ]);
  expect(agent.noBackups).toBe(false);
  // The report never claims a backup that isn't there.
  const marker = (await yap.invoke('chat_get', { id: agent.conversation })).messages.find((m) => m.kind === 'agentReport');
  expect(marker.text).toContain('Files its commands made: costs.csv (new: Undo deletes them).');
  expect(marker.text).toContain("Files its commands changed with no backup: docs\\lab.md (Undo can't put them back).");
  expect(marker.text).not.toContain('backed up first');

  // Outputs: the workbook a command made ("New") and the file it changed; the new one opens as a sheet.
  await main.getByTestId('chat-summary-button').click();
  const outputs = main.getByTestId('chat-summary').getByTestId('chat-summary-outputs').getByTestId('chat-output');
  await expect(outputs).toHaveCount(2);
  await expect(outputs.nth(0)).toContainText('costs.csv');
  await expect(outputs.nth(0).locator('.olabel')).toHaveText('New');
  await expect(outputs.nth(1)).toContainText('lab.md');
  await expect(outputs.nth(1).locator('.olabel')).toHaveText('Changed');
  await shot(main, '10-a-commands-file-in-outputs');
  await outputs.nth(0).click();
  await expect(main.getByTestId('chat-sheet')).toHaveAttribute('data-name', 'costs.csv');
  await expect(main.getByTestId('chat-sheet')).toContainText('compost');
  await panel(main).getByRole('button', { name: 'Close the panel' }).first().click();

  // Its turn's Files changed: "New · By a command", and "By a command · No backup" with the line that says why.
  await thread(main).getByTestId('chat-agent-pill').first().click();
  const tab = main.getByTestId('chat-agent');
  await expect(tab).toBeVisible();
  const files = tab.getByTestId('chat-agent-file');
  await expect(files).toHaveCount(2);
  await expect(tab.locator('[data-testid="chat-agent-file"][data-file="costs.csv"]')).toHaveAttribute('data-by-command', 'new');
  await expect(tab.locator('[data-testid="chat-agent-file"][data-file="costs.csv"] .tag')).toHaveText(['New', 'By a command']);
  await expect(tab.locator('[data-testid="chat-agent-file"][data-file="docs\\\\lab.md"]')).toHaveAttribute('data-by-command', 'unbacked');
  await expect(tab.getByTestId('chat-agent-file-unbacked')).toHaveText('No backup');
  await expect(tab.getByTestId('chat-agent-command-unbacked')).toHaveText("Undo can't put back docs\\lab.md: a command changed it, and Yap had no copy of it from before.");
  await shot(main, '11-files-changed-by-a-command');
  await theme(yap, main, 'dark');
  await shot(main, '11-files-changed-by-a-command-dark');
  await theme(yap, main, 'light');

  // Undo: asked first, saying what it can't put back; the new file deleted, the changed one left as the command left it.
  await tab.getByRole('button', { name: "Undo the agent's changes" }).click();
  const confirm = main.getByRole('alertdialog', { name: "Undo the agent's changes?" });
  await expect(confirm).toContainText("Undo can't put back docs\\lab.md");
  await expect(confirm).not.toContainText('Yap kept their backups for this.');
  await confirm.getByRole('button', { name: "Undo the agent's changes" }).click();
  await expect(main.getByTestId('chat-agent-undone')).toHaveText('Undone: costs.csv deleted (the agent made it); lab.md left as it is (a command changed it, and Yap had no copy of it from before).');
  expect(fs.existsSync(path.join(GARDEN, 'costs.csv'))).toBe(false);
  expect(fs.readFileSync(path.join(GARDEN, 'docs', 'lab.md'), 'utf8')).toBe(LAB_TIDIED);
  await shot(main, '12-undo-what-a-command-made');
  await tab.getByRole('button', { name: 'Close the panel' }).first().click();
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
  // A file a command made says "New" and is marked; one it changed stays "Changed" (agents/changes.rs).
  const byCommand = outputItems({ agents: [{ id: 'c', turnList: [{ n: 1, endedTs: 40, files: ['costs.xlsx', 'data\\beds.csv'], byCommand: [{ file: 'costs.xlsx', new: true, kept: false }, { file: 'data\\beds.csv', new: false, kept: false }] }] }] });
  expect(byCommand.map((i) => [i.name, i.label, !!i.command])).toEqual([
    ['costs.xlsx', 'New', true],
    ['beds.csv', 'Changed', true],
  ]);
  // contextBreakdown.js: the context's numbers as the Claude app writes them, the rows in its order.
  expect([628, 4324, 32164, 156408, 934836, 1_000_000].map(fmtTokens)).toEqual(['628', '4.3k', '32.2k', '156k', '935k', '1M']);
  expect(fmtShare(129046, 1_000_000)).toBe('12.9%');
  expect(shortPath('C:\\Users\\sam\\.claude\\CLAUDE.md')).toBe('…\\.claude\\CLAUDE.md');
  expect(shortPath('docs/CLAUDE.md')).toBe('docs/CLAUDE.md');
  const view = contextView({ window: 200_000, used: 30_000, percent: 15, categories: [{ name: 'Free space', tokens: 137_000, kind: 'free' }, { name: 'Something new', tokens: 1000, kind: 'used' }, { name: 'Messages', tokens: 29_000, kind: 'used' }, { name: 'Autocompact buffer', tokens: 33_000, kind: 'buffer' }, { name: 'MCP tools (deferred)', tokens: 9000, kind: 'deferred' }], skills: [{ name: 'pdf', source: 'Plugin', tokens: 904 }] });
  expect(view.label).toBe('15% used · 30k / 200k');
  expect(view.rows.map((r) => [r.name, r.colour])).toEqual([['Messages', 'messages'], ['Something new', 'other'], ['Autocompact buffer', 'buffer'], ['Free space', 'free']]);
  expect(view.segments.map((g) => g.key)).toEqual(['Messages', 'Something new', 'Autocompact buffer']);
  expect(view.deferred.map((d) => d.name)).toEqual(['MCP tools (deferred)']);
  expect(view.lists.map((l) => [l.key, l.count])).toEqual([['skills', 1]]);
  expect(contextView(null)).toBe(null);
  expect(contextView({ window: 0, categories: [{ name: 'Messages', tokens: 1, kind: 'used' }] })).toBe(null);
  // The Changes section's quiet lines; no git: no section.
  expect(prsNote({ status: 'no-git' })).toBe(null);
  expect(prsNote({ status: 'ok', list: [] })).toBe('No pull requests for this branch yet.');
  expect(prsNote({ status: 'ok', list: [{ number: 1 }] })).toBe('');
  expect(prsNote({ status: 'signed-out' })).toMatch(/gh auth login/);
});
