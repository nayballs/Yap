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
// - **Where links open** (16 Nov 2026, agents/editors.rs): Settings → Yap
//   Code → "Open file links in" — Yap's file view, VS Code, Cursor, another
//   editor found on this PC (here the stand-ins `YAP_E2E_EDITORS` names), or
//   Windows' default app; a click goes there at the line, the command a test
//   run keeps (`e2e_file_opens`) checked to the letter (paths quoted); an
//   editor that's gone and a file Windows would run open in Yap instead,
//   said so. A link's right-click menu: Open in Yap, Open in <editor>, Show
//   in folder, Copy path.
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
// - **One chip a file a turn** (16 Nov 2026): Claude writes report.md and shares it; the
//   answer's chip (the Claude app's file chip) is its one card — the turn's "Agent done ·
//   Changed report.md" names it as a link — a click opens it where its links open, its ⋯ is
//   a file link's menu.
//
// The chat is on the plan with Yap's stand-in for Claude Code (agents-direct.spec.js'
// way); the workspace and the folder "outside" it are in this run's own folder.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, setConfig, openSettings, closeSettings, closeToasts, jsonLines } from './support/fixtures.js';
import { fakeClaude, planConfig } from './support/claude-plan.js';
import { RUNS } from './support/yap.js';
import { newCodeChat, toSide, moreClearOf } from './support/code-chats.js';
import { looksLikePath, pathsIn, moreSpot } from '../src/lib/ui/fileLinks.js';
import { runnable, blockLang } from '../src/lib/ui/codeRun.js';
import { outputItems, splitPath } from '../src/lib/views/chat/outputs.js';
import { contextView, fmtTokens, fmtShare, shortPath, prsNote, prChecks } from '../src/lib/views/chat/contextBreakdown.js';

// Its workspaces and stand-ins, a worker's own (`--repeat-each` runs copies side by side).
const HERE = path.join(RUNS, `code-chat-files-spec-w${process.env.TEST_WORKER_INDEX ?? 0}`);
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
 * edit tool, so Claude Code's checkpoints never keep them (`command: true`) — and it answers. */
const COSTS = 'item,cost\nseeds,4.50\ncompost,12.00\n';
const LAB_TIDIED = LAB.replace('The beds', 'All the beds');
const COSTS_SAID = 'Done: costs.csv has the costs.';
const MAKES_COSTS = [
  { _wait_user: true },
  init(),
  said('c1', [{ type: 'tool_use', id: 'toolu_make', name: 'Bash', input: { command: 'node make-costs.js', description: 'Make the costs sheet' } }]),
  { _write_file: { path: 'costs.csv', text: COSTS, command: true } },
  { _write_file: { path: 'docs\\lab.md', text: LAB_TIDIED, command: true } },
  toolResult('toolu_make', 'Wrote costs.csv and tidied docs/lab.md'),
  said('c2', [{ type: 'text', text: COSTS_SAID }], 'end_turn'),
  result(COSTS_SAID),
  { _wait_user: true },
];
/** File links of every shape, in two answers (16 Nov 2026: a short link at the end of its line had its ⋯ over its end). */
const NOTES_MD = '# Notes\n\nThe north border has four beds.\n';
const LONG = 'src/garden/beds/north-border/raised-beds/twenty-twenty-six/measurements-of-every-raised-bed-in-the-north-border.js';
const SHAPES = [
  `The beds are in notes.md, and every measurement is in \`${LONG}\` now.`,
  '',
  '| File | What it holds |',
  '|---|---|',
  '| `notes.md` | the beds |',
  '| the converter | `src/convert.js` |',
  '',
  '- The lab notes: docs/lab.md',
].join('\n');
const ONE_LINE = 'Every bed is in notes.md';
const SHAPED = [{ _wait_user: true }, init(), said('s1', [{ type: 'text', text: SHAPES }], 'end_turn'), result(SHAPES), { _wait_user: true }];
const IN_ONE_LINE = [{ _wait_user: true }, init(), said('s2', [{ type: 'text', text: ONE_LINE }], 'end_turn'), result(ONE_LINE), { _wait_user: true }];
/** Writes a new report.md and shares it in the chat (Yap's `share_files`, over its endpoint), then answers. */
const REPORT_MD = '# Bed report\n\nFour beds in the north border.\n';
const SHARED_SAID = 'The bed report is ready.';
const WRITES_AND_SHARES = [
  { _wait_user: true },
  init(),
  said('w1', [{ type: 'tool_use', id: 'toolu_write', name: 'Write', input: { file_path: '{{cwd}}\\report.md', content: REPORT_MD } }]),
  { _hook: { event: 'PreToolUse', input: { tool_name: 'Write', tool_use_id: 'toolu_write', tool_input: { file_path: 'report.md' } } } },
  { _write_file: { path: 'report.md', text: REPORT_MD } },
  toolResult('toolu_write', 'File created successfully at: report.md'),
  said('w2', [{ type: 'tool_use', id: 'toolu_share', name: 'mcp__yap__share_files', input: { files: ['{{cwd}}\\report.md'], caption: 'The bed report' } }]),
  { _mcp: { tool: 'share_files', arguments: { files: ['{{cwd}}\\report.md'], caption: 'The bed report' }, result_for: 'toolu_share' } },
  said('w3', [{ type: 'text', text: SHARED_SAID }], 'end_turn'),
  result(SHARED_SAID),
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
/** `gh pr list --json …`'s answer for the branch: one open (its checks running), one open with a cancelled check, one merged. */
const PULLS = [
  { number: 117, title: 'Measure the beds in centimetres', state: 'MERGED', isDraft: false, url: 'https://github.com/example-org/garden-planner/pull/117', statusCheckRollup: [{ __typename: 'CheckRun', status: 'COMPLETED', conclusion: 'SUCCESS', name: 'check' }] },
  { number: 131, title: 'Round toInches to one decimal', state: 'OPEN', isDraft: false, url: 'https://github.com/example-org/garden-planner/pull/131', statusCheckRollup: [{ __typename: 'CheckRun', status: 'IN_PROGRESS', conclusion: '', name: 'e2e' }] },
  // A run its workflow's concurrency cancelled (16 Nov 2026: it read "Checks failing"), CodeQL's neutral, and a
  // check cancelled then run again and passing (only its latest run counts).
  {
    number: 129,
    title: 'Show the beds on a map',
    state: 'OPEN',
    isDraft: false,
    url: 'https://github.com/example-org/garden-planner/pull/129',
    statusCheckRollup: [
      { __typename: 'CheckRun', status: 'COMPLETED', conclusion: 'CANCELLED', name: 'e2e (1/2)', workflowName: 'E2E', startedAt: '2026-11-16T02:08:42Z' },
      { __typename: 'CheckRun', status: 'COMPLETED', conclusion: 'SUCCESS', name: 'e2e (2/2)', workflowName: 'E2E', startedAt: '2026-11-16T02:08:42Z' },
      { __typename: 'CheckRun', status: 'COMPLETED', conclusion: 'CANCELLED', name: 'check', workflowName: 'CI', startedAt: '2026-11-16T02:01:00Z' },
      { __typename: 'CheckRun', status: 'COMPLETED', conclusion: 'SUCCESS', name: 'check', workflowName: 'CI', startedAt: '2026-11-16T02:09:10Z' },
      { __typename: 'CheckRun', status: 'COMPLETED', conclusion: 'NEUTRAL', name: 'CodeQL', workflowName: '', startedAt: '2026-11-16T02:09:59Z' },
    ],
  },
];
function writeGh(name, body) {
  fs.mkdirSync(SCRIPTS, { recursive: true });
  const file = path.join(SCRIPTS, `${name}.json`);
  fs.writeFileSync(file, JSON.stringify(body));
  return file;
}
const dumped = (file) => jsonLines(file);
async function theme(yap, main, which) {
  await setConfig(yap, main, { theme: which });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe(which);
}

/** The editors "on this PC" (stand-ins: never started, a test run keeps the command instead). */
const EDITORS_FILE = path.join(HERE, 'editors.json');
const VS_CODE = 'C:\\Tools\\Microsoft VS Code\\Code.exe';
const ZED_CLI = 'C:\\Tools\\Zed\\bin\\zed.exe';
const STAND_INS = [
  { id: 'vscode', program: VS_CODE },
  { id: 'zed', program: ZED_CLI },
  { id: 'notepadpp', program: 'C:\\Tools\\Notepad++\\notepad++.exe' },
  { id: 'idea-ce', program: 'C:\\Tools\\IntelliJ IDEA Community Edition\\bin\\idea64.exe' },
  { id: 'notepad', program: 'C:\\Windows\\System32\\notepad.exe' },
];
function writeEditors(list) {
  fs.mkdirSync(HERE, { recursive: true });
  fs.writeFileSync(EDITORS_FILE, JSON.stringify(list));
}

const test = base.extend({
  yapOptions: [
    async ({}, use) => {
      writeEditors(STAND_INS);
      await use({
        name: 'code-chat-files',
        env: { ...fakeClaude(writeScript('idle', [{ _wait_user: true }])), YAP_E2E_EDITORS: EDITORS_FILE },
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
  await setConfig(yap, main, { agentsEnabled: true, agentsDirect: true, agentsAsk: 'new-or-web', agentsSpeak: 'never', agentsMode: 'auto', agentsOpenFiles: 'yap', webSearch: false, speakReplies: false, assistantScreen: false });
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

test('“Open file links in”: Yap, an editor at the line (its command quoted), Windows’ default app, an editor that’s gone; a link’s menu', async ({ yap, main, shot }) => {
  writeEditors(STAND_INS);
  await yap.invoke('e2e_file_opens', { clear: true });
  const conversation = await rounded(yap, main);
  const [convert] = await yap.invoke('code_files_check', { conversation, paths: ['src/convert.js'] });
  const lab = convert.abs.replace(/src\\convert\.js$/, 'docs\\lab.md');
  const ws = convert.abs.replace(/\\src\\convert\.js$/, '');
  const links = answer(main).locator('.md-filelink');
  const opens = () => yap.invoke('e2e_file_opens');
  const lastOpen = async () => (await opens()).at(-1) ?? null;
  const file = main.getByTestId('chat-file');

  // The setting: Yap's view first, VS Code and Cursor always (Cursor greyed out: not here), the others found, the default app last.
  const dialog = await openSettings(main, 'Yap Code');
  const row = dialog.locator('[data-setting="yap-code-open-files"]');
  const select = row.getByRole('combobox', { name: 'Open file links in' });
  await row.scrollIntoViewIfNeeded();
  await expect(select).toHaveValue('yap');
  await expect(select.locator('option')).toHaveText(["Yap's file view", 'VS Code', 'Cursor (not on this PC)', 'Zed', 'Notepad++', 'IntelliJ IDEA Community', 'Notepad', "Windows' default app"]);
  await expect(select.locator('option[value="cursor"]')).toBeDisabled();
  await expect(row).toContainText('Right-click a link');
  await shot(main, '02b-open-file-links-in-the-setting');
  await theme(yap, main, 'dark');
  await shot(main, '02c-open-file-links-in-the-setting-dark');
  await theme(yap, main, 'light');
  // Picked there: VS Code. Links now open in it, at the line, and Yap's view stays shut.
  await select.selectOption('vscode');
  await expect(row).toContainText('At the line, in VS Code');
  await expect.poll(async () => (await yap.invoke('get_config')).agentsOpenFiles).toBe('vscode');
  await closeSettings(main);
  await links.first().click();
  await expect.poll(lastOpen).toEqual({
    with: 'vscode',
    name: 'VS Code',
    program: VS_CODE,
    args: [`"${ws}"`, '-g', `"${convert.abs}:5"`],
    commandLine: `"${VS_CODE}" "${ws}" -g "${convert.abs}:5"`,
    rel: 'src/convert.js',
    line: 5,
  });
  await expect(file).toHaveCount(0);

  // Zed: its command line, the line after the path.
  await setConfig(yap, main, { agentsOpenFiles: 'zed' });
  await answer(main).locator('.md-filelink', { hasText: 'the lab notes' }).click();
  await expect.poll(lastOpen).toEqual(expect.objectContaining({ with: 'zed', program: ZED_CLI, args: [`"${lab}:3"`], rel: 'docs/lab.md', line: 3 }));
  // A JetBrains IDE: --line.
  await setConfig(yap, main, { agentsOpenFiles: 'idea-ce' });
  await links.first().click();
  await expect.poll(lastOpen).toEqual(expect.objectContaining({ with: 'idea-ce', name: 'IntelliJ IDEA Community', args: ['--line', '5', `"${convert.abs}"`] }));

  // Windows' default app: a file it opens goes there (the lab notes); one it would run (a .js script) opens in Yap, said so.
  await setConfig(yap, main, { agentsOpenFiles: 'default' });
  await answer(main).locator('.md-filelink', { hasText: 'the lab notes' }).click();
  await expect.poll(lastOpen).toEqual({ with: 'default', name: "Windows' default app", program: null, args: [], commandLine: null, rel: 'docs/lab.md', line: 3 });
  const before = (await opens()).length;
  await links.first().click();
  await expect(file).toHaveAttribute('data-rel', 'src/convert.js');
  await expect(main.getByRole('status').filter({ hasText: 'Opened in Yap instead' })).toContainText('.js file');
  expect((await opens()).length).toBe(before);
  await shot(main, '02f-a-script-opens-in-yap-instead');
  await closeToasts(main);
  await panel(main).getByRole('button', { name: 'Close the panel' }).first().click();

  // An editor that's gone (uninstalled since it was picked): Yap's view, and why.
  await setConfig(yap, main, { agentsOpenFiles: 'cursor' });
  const n = (await opens()).length;
  await links.first().click();
  await expect(file).toHaveAttribute('data-rel', 'src/convert.js');
  await expect(main.getByRole('status').filter({ hasText: 'Opened in Yap instead' })).toContainText("Cursor isn't on this PC");
  expect((await opens()).length).toBe(n);
  await closeToasts(main);
  await panel(main).getByRole('button', { name: 'Close the panel' }).first().click();

  // Rust's rules, whatever the page asks: never a file outside the workspace, never a program the page names.
  await expect(yap.invoke('code_file_open', { conversation, path: '../outside/secret.txt', line: null, with: 'vscode' })).rejects.toThrow(/isn't in this chat's workspace/);
  expect(await yap.invoke('code_file_open', { conversation, path: 'src/convert.js', line: null, with: 'C:\\Windows\\System32\\cmd.exe' })).toEqual({ how: 'yap' });
  expect((await opens()).length).toBe(n);

  // A link's menu (right-click): Open in Yap, Open in <the setting's editor, else the first found>, Show in folder, Copy path.
  await setConfig(yap, main, { agentsOpenFiles: 'yap' });
  const menuOf = async (link) => {
    await link.click({ button: 'right' });
    const menu = main.getByRole('menu');
    await expect(menu).toBeVisible();
    return menu;
  };
  let menu = await menuOf(links.first());
  await expect(menu.getByRole('menuitem')).toHaveText(['Open in Yap', 'Open in VS Code', 'Show in folder', 'Copy path']);
  await shot(main, '02d-a-file-links-menu');
  await theme(yap, main, 'dark');
  await shot(main, '02e-a-file-links-menu-dark');
  await theme(yap, main, 'light');
  await menu.getByRole('menuitem', { name: 'Open in VS Code' }).click();
  await expect.poll(lastOpen).toEqual(expect.objectContaining({ with: 'vscode', args: [`"${ws}"`, '-g', `"${convert.abs}:5"`] }));
  await expect(file).toHaveCount(0);
  menu = await menuOf(links.nth(1));
  await menu.getByRole('menuitem', { name: 'Show in folder' }).click();
  await expect.poll(() => yap.invoke('e2e_opened_urls')).toContainEqual('reveal:docs/lab.md');
  menu = await menuOf(links.first());
  await menu.getByRole('menuitem', { name: 'Copy path' }).click();
  await expect(main.getByRole('status').filter({ hasText: 'Path copied' })).toBeVisible();
  await closeToasts(main);
  // Picked Zed: the menu names Zed. Open in Yap: the file view, whatever the setting.
  await setConfig(yap, main, { agentsOpenFiles: 'zed' });
  await expect
    .poll(async () => {
      const m = await menuOf(links.first());
      const second = (await m.getByRole('menuitem').nth(1).textContent()).trim();
      await main.keyboard.press('Escape');
      return second;
    })
    .toBe('Open in Zed');
  menu = await menuOf(links.first());
  await menu.getByRole('menuitem', { name: 'Open in Yap' }).click();
  await expect(file).toHaveAttribute('data-rel', 'src/convert.js');
  await expect(file.locator('.fl.mark')).toHaveAttribute('data-no', '5');
  // The keyboard: Shift+F10 on the focused link.
  await links.nth(2).focus();
  await main.keyboard.press('Shift+F10');
  await expect(main.getByRole('menu').getByRole('menuitem').first()).toHaveText('Open in Yap');
  await main.keyboard.press('Escape');
  await setConfig(yap, main, { agentsOpenFiles: 'yap' });
  await panel(main).getByRole('button', { name: 'Close the panel' }).first().click();
});

test('a file link’s ⋯: on hover or focus, the link’s menu under it; the next Tab stop; nothing in the words moves', async ({ yap, main, shot }) => {
  writeEditors(STAND_INS);
  await yap.invoke('e2e_file_opens', { clear: true });
  const conversation = await rounded(yap, main);
  const [convert] = await yap.invoke('code_files_check', { conversation, paths: ['src/convert.js'] });
  const ws = convert.abs.replace(/\\src\\convert\.js$/, '');
  const links = answer(main).locator('.md-filelink');
  await expect(links).toHaveText(['src/convert.js:5', 'the lab notes', 'src/convert.js']);
  const more = answer(main).getByTestId('md-filelink-more');
  const file = main.getByTestId('chat-file');
  const words = await answer(main).locator('.cbody').evaluate((el) => el.textContent);
  const placeOf = (loc) => loc.evaluate((el) => {
    const r = el.getBoundingClientRect();
    return { left: r.left, right: r.right, top: r.top, bottom: r.bottom };
  });
  // None until a link is pointed at.
  await expect(more).toBeHidden();
  /** Where a link is in the answer (the thread may scroll as the pointer moves). */
  const inAnswer = (loc) =>
    loc.evaluate((el) => {
      const r = el.getBoundingClientRect();
      const a = el.closest('.cbody').getBoundingClientRect();
      return { left: r.left - a.left, top: r.top - a.top, width: r.width, height: r.height };
    });
  const before = await inAnswer(links.nth(1));

  // The pointer on a link: its ⋯ at the link's end, its middle on the link's line; the link where it was.
  await links.first().hover();
  await expect(more).toBeVisible();
  await expect(more).toHaveAccessibleName('More for src/convert.js');
  await expect(more).toHaveAttribute('aria-haspopup', 'menu');
  const link0 = await placeOf(links.first());
  const btn = await placeOf(more);
  expect(Math.abs(btn.left - link0.right)).toBeLessThanOrEqual(5);
  expect(Math.abs((btn.top + btn.bottom) / 2 - (link0.top + link0.bottom) / 2)).toBeLessThanOrEqual(2);
  expect(await inAnswer(links.nth(1))).toEqual(before);
  // Its icon only: the answer's words are as they were (a copy of them has no "⋯").
  expect(await answer(main).locator('.cbody').evaluate((el) => el.textContent)).toBe(words);
  await shot(main, '02g-a-file-links-more-on-hover');
  await theme(yap, main, 'dark');
  await shot(main, '02h-a-file-links-more-on-hover-dark');
  await theme(yap, main, 'light');

  // Onto it, and a click: the link's own menu, under the ⋯. Pointed at anew, from the link, as a person goes: with
  // the pointer left still through the screenshots and theme switches above, the ⋯ had now and then gone (its link
  // no longer under the pointer as the page saw it), and the hover onto it waited the test out (CI, 16 Nov 2026).
  await main.mouse.move(5, 5);
  await links.first().hover();
  await expect(more).toBeVisible();
  await more.hover();
  await expect(more).toBeVisible();
  await more.click();
  const menu = main.getByRole('menu');
  await expect(menu).toBeVisible();
  await expect(menu.getByRole('menuitem')).toHaveText(['Open in Yap', 'Open in VS Code', 'Show in folder', 'Copy path']);
  await expect(more).toHaveAttribute('aria-expanded', 'true');
  const menuBox = await placeOf(menu);
  const at = await placeOf(more);
  expect(menuBox.top).toBeGreaterThanOrEqual(at.bottom);
  await shot(main, '02i-a-file-links-more-menu');
  await theme(yap, main, 'dark');
  await shot(main, '02j-a-file-links-more-menu-dark');
  await theme(yap, main, 'light');
  // Its items do what the right-click's do: VS Code at the link's line.
  await menu.getByRole('menuitem', { name: 'Open in VS Code' }).click();
  await expect.poll(async () => (await yap.invoke('e2e_file_opens')).at(-1)).toEqual(expect.objectContaining({ with: 'vscode', args: [`"${ws}"`, '-g', `"${convert.abs}:5"`] }));
  await expect(file).toHaveCount(0);
  // The menu shut, the focus back on the ⋯ (so it stays); the focus and the pointer elsewhere, it goes.
  await expect(more).toHaveAttribute('aria-expanded', 'false');
  await main.getByRole('textbox', { name: 'Message' }).focus();
  await main.mouse.move(5, 5);
  await expect(more).toBeHidden();

  // The keyboard: the focused link's ⋯ is the next Tab stop; Enter opens the menu; Escape gives the ⋯ the focus
  // back; Shift+Tab goes back to its link.
  const focused = () => main.evaluate(() => document.activeElement?.dataset?.testid || document.activeElement?.textContent || '');
  await links.nth(1).focus();
  await expect(more).toBeVisible();
  await expect(more).toHaveAccessibleName('More for docs/lab.md');
  await main.keyboard.press('Tab');
  await expect.poll(focused).toBe('md-filelink-more');
  await main.keyboard.press('Enter');
  await expect(main.getByRole('menu').getByRole('menuitem').first()).toHaveText('Open in Yap');
  await main.keyboard.press('Escape');
  await expect(main.getByRole('menu')).toHaveCount(0);
  await expect.poll(focused).toBe('md-filelink-more');
  await main.keyboard.press('Shift+Tab');
  await expect.poll(focused).toBe('the lab notes');
  // Space on it too, and Open in Yap: the file view at the link's line.
  await main.keyboard.press('Tab');
  await main.keyboard.press('Space');
  await main.getByRole('menu').getByRole('menuitem', { name: 'Open in Yap' }).click();
  await expect(file).toHaveAttribute('data-rel', 'docs/lab.md');
  await expect(file.locator('.fl.mark')).toHaveAttribute('data-no', '3');
  // The focus elsewhere: no ⋯.
  await main.getByRole('textbox', { name: 'Message' }).focus();
  await expect(more).toBeHidden();
  // A link a turn's own words carry (as Claude works) has one too.
  const said = thread(main).getByTestId('chat-agent-work-said');
  await said.locator('.md-filelink').first().hover();
  await expect(said.getByTestId('md-filelink-more')).toBeVisible();
  await main.mouse.move(5, 5);
  await panel(main).getByRole('button', { name: 'Close the panel' }).first().click();
});

test('a file link’s ⋯ never covers its link: a short one, one that wraps, one in a table, one at the end of its line', async ({ yap, main, shot }) => {
  fs.writeFileSync(path.join(GARDEN, 'notes.md'), NOTES_MD);
  fs.mkdirSync(path.dirname(path.join(GARDEN, LONG)), { recursive: true });
  fs.writeFileSync(path.join(GARDEN, LONG), 'export const beds = 4;\n');
  await script(yap, SHAPED, 'shapes');
  const before = (await agents(yap)).length;
  await openView(main, 'Chat');
  if (await panel(main).count()) await panel(main).getByRole('button', { name: 'Close the panel' }).first().click();
  await newCodeChat(main, 'garden-planner');
  await send(main, 'Where are the beds measured? See @notes.md');
  await expect.poll(async () => (await agents(yap)).length, { timeout: 20_000 }).toBe(before + 1);
  const agent = (await agents(yap))[0];
  const turnDone = (n) =>
    expect
      .poll(async () => {
        const j = (await agents(yap)).find((a) => a.id === agent.id);
        return [j?.status, j?.turnList?.length];
      }, { timeout: 30_000 })
      .toEqual(['done', n]);
  await turnDone(1);
  await expect(answer(main)).toContainText('every measurement');
  await expect(thread(main).getByTestId('chat-streaming')).toHaveCount(0);
  const shaped = answer(main);
  const links = shaped.locator('.md-filelink');
  await expect(links).toHaveText(['notes.md', LONG, 'notes.md', 'src/convert.js', 'docs/lab.md']);
  /** Every link's line boxes, from the answer's corner: what must not move while a ⋯ shows. */
  const places = (bubble) =>
    bubble.locator('.cbody').evaluate((body) => {
      const a = body.getBoundingClientRect();
      const r1 = (v) => Math.round(v * 10) / 10;
      return [...body.querySelectorAll('.md-filelink')].map((el) => [...el.getClientRects()].map((r) => [r1(r.left - a.left), r1(r.top - a.top), r1(r.width), r1(r.height)]));
    });
  const words = await shaped.locator('.cbody').evaluate((el) => el.textContent);
  const still = await places(shaped);
  // The long one wraps across lines.
  expect(still[1].length).toBeGreaterThan(1);
  // Each in turn: its ⋯ clear of every line of it, and nothing in the words moved.
  const at = [];
  for (let i = 0; i < still.length; i++) {
    const got = await moreClearOf(links.nth(i));
    at.push(got.at);
    expect(await places(shaped)).toEqual(still);
    if (i === 1) {
      // The long one's: after the end of its last line, on that line.
      const last = got.lines.at(-1);
      expect(got.more.left).toBeGreaterThanOrEqual(last.right);
      expect(Math.abs((got.more.top + got.more.bottom) / 2 - (last.top + last.bottom) / 2)).toBeLessThanOrEqual(2);
      await shot(main, '02s-a-file-links-more-after-a-long-link');
      await theme(yap, main, 'dark');
      await shot(main, '02t-a-file-links-more-after-a-long-link-dark');
      await theme(yap, main, 'light');
    }
    if (i === 3) {
      // In the table's last column: after it, past the table's edge.
      await shot(main, '02u-a-file-links-more-in-a-table');
      await theme(yap, main, 'dark');
      await shot(main, '02v-a-file-links-more-in-a-table-dark');
      await theme(yap, main, 'light');
    }
  }
  expect(at).toEqual(['after', 'after', 'after', 'after', 'after']);
  expect(await shaped.locator('.cbody').evaluate((el) => el.textContent)).toBe(words);

  // A one-line answer ending in a short link: its end is the answer's edge, so the ⋯ goes past it, never over the
  // link — and a click on the link's end opens the file, not the menu (16 Nov 2026: it had the ⋯ over its end).
  await main.mouse.move(5, 5);
  await script(yap, IN_ONE_LINE, 'in-one-line');
  await send(main, 'And in one line?');
  await turnDone(2);
  await expect(answer(main).locator('.cbody')).toHaveText(ONE_LINE);
  await expect(thread(main).getByTestId('chat-streaming')).toHaveCount(0);
  const oneLine = answer(main);
  const short = oneLine.locator('.md-filelink');
  await expect(short).toHaveText('notes.md');
  const edge = await short.evaluate((el) => el.closest('.cbody').getBoundingClientRect().right - el.getBoundingClientRect().right);
  expect(Math.abs(edge)).toBeLessThanOrEqual(1);
  const stillShort = await places(oneLine);
  const got = await moreClearOf(short);
  expect(got.at).toBe('after');
  expect(got.more.left).toBeGreaterThanOrEqual(got.lines[0].right);
  expect(await places(oneLine)).toEqual(stillShort);
  await shot(main, '02w-a-file-links-more-after-a-short-link-at-its-lines-end');
  await theme(yap, main, 'dark');
  await shot(main, '02x-a-file-links-more-after-a-short-link-at-its-lines-end-dark');
  await theme(yap, main, 'light');
  const { width, height } = await short.boundingBox();
  await short.click({ position: { x: width - 2, y: height / 2 } });
  const file = main.getByTestId('chat-file');
  await expect(file).toHaveAttribute('data-rel', 'notes.md');
  await expect(main.getByRole('menu')).toHaveCount(0);

  // The panel open, the thread narrow: the person's own @-mention ends their message at the thread's edge, with no
  // room after it in view — so its ⋯ goes under its end, still clear of it.
  await main.mouse.move(5, 5);
  const pill = thread(main).locator('.cbubble.user').first().locator('.md-filelink');
  await expect(pill).toHaveText('@notes.md');
  const room = await pill.evaluate((el) => el.closest('.msgs').getBoundingClientRect().right - el.getBoundingClientRect().right);
  expect(room).toBeLessThan(25);
  const atPill = await moreClearOf(pill);
  expect(atPill.at).toBe('below');
  expect(atPill.more.top).toBeGreaterThanOrEqual(atPill.lines[0].bottom);
  await shot(main, '02y-a-mentions-more-under-it-at-the-threads-edge');
  await theme(yap, main, 'dark');
  await shot(main, '02z-a-mentions-more-under-it-at-the-threads-edge-dark');
  await theme(yap, main, 'light');
  await main.mouse.move(5, 5);
  await panel(main).getByRole('button', { name: 'Close the panel' }).first().click();
});

test('one chip a file a turn: a file written and shared is the answer’s chip, the report’s line a link; a click opens it, its ⋯ a link’s menu', async ({ yap, main, shot }) => {
  writeEditors(STAND_INS);
  await yap.invoke('e2e_file_opens', { clear: true });
  await script(yap, WRITES_AND_SHARES, 'writes-and-shares');
  const before = (await agents(yap)).length;
  await openView(main, 'Chat');
  if (await panel(main).count()) await panel(main).getByRole('button', { name: 'Close the panel' }).first().click();
  await newCodeChat(main, 'garden-planner');
  await send(main, 'Write up the beds in a report and send it to me.');
  await expect.poll(async () => (await agents(yap)).length, { timeout: 20_000 }).toBe(before + 1);
  const agent = (await agents(yap))[0];
  await expect.poll(async () => (await agents(yap)).find((j) => j.id === agent.id)?.status, { timeout: 30_000 }).toBe('done');
  await expect(answer(main)).toContainText(SHARED_SAID);
  const reportFile = path.join(GARDEN, 'report.md');
  // Kept twice — the turn's report has the new file, the answer shares it — and shown once: the answer's chip.
  const kept = (await yap.invoke('chat_get', { id: agent.conversation })).messages;
  expect(kept.map((m) => m.kind || m.role)).toEqual(['user', 'agentReport', 'assistant']);
  expect(kept[1].files?.[0]?.items?.map((i) => i.name)).toEqual(['report.md']);
  expect(kept[2].files?.[0]?.items?.map((i) => i.name)).toEqual(['report.md']);
  const chips = thread(main).getByTestId('chat-file-card');
  await expect(chips).toHaveCount(1);
  await expect(chips.getByTestId('chat-file-name')).toHaveText('report.md');
  await expect(chips.getByTestId('chat-file-badge')).toHaveText('MD');
  await expect(thread(main).getByTestId('chat-share-caption')).toHaveText('The bed report');
  expect((await chips.boundingBox()).y).toBeGreaterThan((await answer(main).boundingBox()).y);
  // The report's line names it, a link (no card of its own).
  const report = thread(main).getByTestId('chat-agent-report');
  await expect(report).toHaveCount(1);
  await expect(report.getByTestId('chat-agent-report-file')).toHaveText('report.md');
  await expect(report.getByTestId('chat-file-card')).toHaveCount(0);
  await shot(main, '02r-one-chip-a-file-a-turn');
  await theme(yap, main, 'dark');
  await shot(main, '02s-one-chip-a-file-a-turn-dark');
  await theme(yap, main, 'light');

  // A click on the chip: where its links open (Yap's file view here), as a link to the workspace's file.
  await chips.getByTestId('chat-file-open').click();
  const file = main.getByTestId('chat-file');
  await expect(file).toHaveAttribute('data-rel', 'report.md');
  await expect(file.getByTestId('chat-file-body')).toContainText('Four beds in the north border.');
  await panel(main).getByRole('button', { name: 'Close the panel' }).first().click();
  // The report's link opens it the same way.
  await report.getByTestId('chat-agent-report-file').click();
  await expect(file).toHaveAttribute('data-rel', 'report.md');
  await panel(main).getByRole('button', { name: 'Close the panel' }).first().click();

  // Its ⋯ (on hover): a file link's menu — Open in Yap, Open in <editor>, Show in folder, Copy path.
  await chips.hover();
  await chips.getByTestId('chat-file-more').click();
  const menu = main.getByRole('menu');
  await expect(menu.getByRole('menuitem')).toHaveText(['Open in Yap', 'Open in VS Code', 'Show in folder', 'Copy path']);
  await shot(main, '02t-a-chips-menu');
  await menu.getByRole('menuitem', { name: 'Open in VS Code' }).click();
  await expect.poll(async () => (await yap.invoke('e2e_file_opens')).at(-1)).toEqual(expect.objectContaining({ with: 'vscode', rel: 'report.md' }));
  await chips.hover();
  await chips.getByTestId('chat-file-more').click();
  await main.evaluate(() => {
    window.__copied = [];
    navigator.clipboard.writeText = async (t) => void window.__copied.push(t);
  });
  await menu.getByRole('menuitem', { name: 'Copy path' }).click();
  await expect.poll(() => main.evaluate(() => window.__copied)).toEqual([reportFile]);
  // "Open file links in" an editor: a click on the chip goes there too.
  await setConfig(yap, main, { agentsOpenFiles: 'vscode' });
  await yap.invoke('e2e_file_opens', { clear: true });
  await chips.getByTestId('chat-file-open').click();
  await expect.poll(async () => (await yap.invoke('e2e_file_opens')).at(-1) ?? null).toEqual(expect.objectContaining({ with: 'vscode', rel: 'report.md' }));
  await expect(main.getByTestId('chat-file')).toHaveCount(0);
  await setConfig(yap, main, { agentsOpenFiles: 'yap' });
});

test('an Outputs row opens where “Open file links in” says, safely: workspace files only, a file Windows would run in Yap', async ({ yap, main, shot }) => {
  writeEditors(STAND_INS);
  await yap.invoke('e2e_file_opens', { clear: true });
  const conversation = await rounded(yap, main);
  const [convert] = await yap.invoke('code_files_check', { conversation, paths: ['src/convert.js'] });
  const ws = convert.abs.replace(/\\src\\convert\.js$/, '');
  const opens = () => yap.invoke('e2e_file_opens');
  const file = main.getByTestId('chat-file');
  await main.getByTestId('chat-summary-button').click();
  const output = main.getByTestId('chat-summary').getByTestId('chat-summary-outputs').getByTestId('chat-output').first();
  await expect(output).toContainText('convert.js');
  const menuItems = async () => {
    await output.click({ button: 'right' });
    const names = await main.getByRole('menu').getByRole('menuitem').allTextContents();
    await main.keyboard.press('Escape');
    return names.map((n) => n.trim());
  };

  // VS Code: a click on the row opens it there (the workspace's window, the file in it), not in Yap's view.
  await setConfig(yap, main, { agentsOpenFiles: 'vscode' });
  await output.click();
  await expect.poll(async () => (await opens()).at(-1)).toEqual({
    with: 'vscode',
    name: 'VS Code',
    program: VS_CODE,
    args: [`"${ws}"`, `"${convert.abs}"`],
    commandLine: `"${VS_CODE}" "${ws}" "${convert.abs}"`,
    rel: 'src/convert.js',
    line: null,
  });
  await expect(file).toHaveCount(0);
  // Its menu: Open (there), Open in Yap (Yap's view one click away), Show in folder, Copy path, Open in VS Code.
  await expect.poll(menuItems).toEqual(['Open', 'Open in Yap', 'Show in folder', 'Copy path', 'Open in VS Code']);
  await output.click({ button: 'right' });
  await shot(main, '07c-an-outputs-menu-when-links-open-in-an-editor');
  await main.getByRole('menu').getByRole('menuitem', { name: 'Open in Yap' }).click();
  await expect(file).toHaveAttribute('data-rel', 'src/convert.js');
  await main.getByTestId('chat-summary-button').click();

  // Windows' default app: convert.js is a script Windows would run, so it opens in Yap's view instead, said so.
  await setConfig(yap, main, { agentsOpenFiles: 'default' });
  const n = (await opens()).length;
  await output.click();
  await expect(file).toHaveAttribute('data-rel', 'src/convert.js');
  await expect(main.getByRole('status').filter({ hasText: 'Opened in Yap instead' })).toContainText('.js file');
  expect((await opens()).length).toBe(n);
  await closeToasts(main);
  await main.getByTestId('chat-summary-button').click();

  // Yap's view (the default): as before, and no Open in Yap twice.
  await setConfig(yap, main, { agentsOpenFiles: 'yap' });
  await expect.poll(menuItems).toEqual(['Open', 'Show in folder', 'Copy path', 'Open in VS Code']);
  await output.click();
  await expect(file).toHaveAttribute('data-rel', 'src/convert.js');
  expect((await opens()).length).toBe(n);
  await panel(main).getByRole('button', { name: 'Close the panel' }).first().click();
});

test('the file view’s Open in <editor> stays one line at the Lab’s sizes: the folder gives way first, then the label', async ({ yap, main, shot }) => {
  // (The window as it was, put back after.)
  const [w0, h0] = await main.evaluate(() => [window.innerWidth, window.innerHeight]);
  writeEditors(STAND_INS);
  // The Lab's own window (1280 × 860) and its narrowest (860 × 600), the longest editor's name of the stand-ins.
  await setConfig(yap, main, { agentsOpenFiles: 'idea-ce' });
  await main.setViewportSize({ width: 1280, height: 860 });
  await rounded(yap, main);
  const links = answer(main).locator('.md-filelink');
  await links.first().click({ button: 'right' });
  await main.getByRole('menu').getByRole('menuitem', { name: 'Open in Yap' }).click();
  const file = main.getByTestId('chat-file');
  await expect(file).toHaveAttribute('data-rel', 'src/convert.js');
  const button = file.getByTestId('chat-file-open-editor');
  await expect(button).toHaveAccessibleName('Open in IntelliJ IDEA Community');
  const split = main.getByRole('separator', { name: 'Resize the panel' });
  /** The button's height and whether it and ✕ are inside the panel; its label's width shown and whole. */
  const measure = () =>
    file.evaluate((el) => {
      const panelBox = el.getBoundingClientRect();
      const b = el.querySelector('[data-testid="chat-file-open-editor"]').getBoundingClientRect();
      const close = el.querySelector('[aria-label="Close the panel"]').getBoundingClientRect();
      const label = el.querySelector('.flabel');
      const name = el.querySelector('[data-testid="chat-file-name"]');
      return {
        panel: Math.round(panelBox.width),
        height: Math.round(b.height),
        inside: b.right <= panelBox.right + 0.5 && close.right <= panelBox.right + 0.5,
        label: label.offsetParent ? Math.round(label.getBoundingClientRect().width) : 0,
        whole: !!label.offsetParent && label.scrollWidth <= label.clientWidth,
        name: name.scrollWidth <= name.clientWidth && name.getBoundingClientRect().right <= b.left,
      };
    });
  // (The panel's width as it was, put back after: it's remembered.)
  const was = Number(await split.getAttribute('aria-valuenow'));
  const sizes = [];
  for (const [w, h] of [
    [1280, 860],
    [860, 600],
  ]) {
    await main.setViewportSize({ width: w, height: h });
    // The panel as it opens, then at its narrowest (End on the split; none when the panel covers the chat).
    for (const narrowest of [false, true]) {
      if (narrowest) {
        if (!(await split.count())) continue;
        await split.focus();
        await main.keyboard.press('End');
      }
      await main.waitForTimeout(200);
      const m = await measure();
      sizes.push({ w, narrowest, ...m });
      // One line (the button's own 28 px), the button and ✕ inside the panel.
      expect(m.height, JSON.stringify(m)).toBeLessThanOrEqual(30);
      expect(m.inside, JSON.stringify(m)).toBe(true);
      // The file's name whole as the panel opens (the folder, then the editor's name, gave way first).
      if (!narrowest) expect(m.name, JSON.stringify(m)).toBe(true);
      await shot(main, `02k-the-file-view-${w}${narrowest ? '-narrowest' : ''}`);
      if (narrowest && w === 860) {
        await theme(yap, main, 'dark');
        await shot(main, '02l-the-file-view-860-narrowest-dark');
        await theme(yap, main, 'light');
      }
    }
    if (await split.count()) {
      await split.focus();
      await main.keyboard.press('End');
      for (let i = 0; i < Math.round((was - 320) / 24); i++) await main.keyboard.press('ArrowLeft');
    }
  }
  // At the Lab's own size, as the panel opens, the longest name ends in "…" but reads ("Open in IntelliJ ID…");
  // "Open in VS Code" shows whole.
  expect(sizes[0].label, JSON.stringify(sizes)).toBeGreaterThan(80);
  console.log(`file view header: ${JSON.stringify(sizes)}`);
  await main.setViewportSize({ width: 1280, height: 860 });
  await setConfig(yap, main, { agentsOpenFiles: 'vscode' });
  await expect(button).toHaveAccessibleName('Open in VS Code');
  await expect.poll(async () => (await measure()).whole).toBe(true);
  expect((await measure()).name).toBe(true);
  await shot(main, '02m-the-file-view-1280-vs-code');
  await main.setViewportSize({ width: w0, height: h0 });
  await setConfig(yap, main, { agentsOpenFiles: 'yap' });
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
  // (Context's fold as a person first sees it, whatever an earlier run left.)
  await main.evaluate(() => localStorage.removeItem('yap.summary.contextOpen'));
  await main.getByTestId('chat-summary-button').click();
  const summary = main.getByTestId('chat-summary');

  // Context: Claude Code's own /context for the chat's session, drawn as the Claude app draws it — folded at first,
  // "Context ›" with its share and a small bar; opened, the breakdown (and it stays open).
  const context = summary.getByTestId('chat-summary-context');
  const toggle = context.getByTestId('chat-summary-context-toggle');
  await expect(toggle).toHaveAttribute('aria-expanded', 'false');
  await expect(context.getByTestId('chat-summary-context-mini')).toHaveText('16%');
  await expect(context.getByTestId('chat-summary-context-cat')).toHaveCount(0);
  await shot(main, '07a-context-folded');
  await toggle.click();
  await expect(toggle).toHaveAttribute('aria-expanded', 'true');
  await expect(context.getByTestId('chat-summary-context-mini')).toHaveCount(0);
  await expect(context.getByTestId('chat-summary-context-head').locator('.ctxused')).toHaveText('16% used');
  await expect(context.getByTestId('chat-summary-context-head').locator('.ctxof')).toHaveText('156k / 1M');
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
  await expect(deferred.first().locator('.cp')).toHaveText('—');
  // The categories with more behind them say what they are on hover.
  await expect(context.locator('[data-name="Memory files"] .tipped')).toHaveAttribute('title', /CLAUDE\.md/);
  await expect(context.locator('[data-name="Messages"] .tipped')).toHaveCount(0);
  // The lists behind them, folded, each with its count.
  const lists = context.getByTestId('chat-summary-context-list');
  await expect(lists.locator('summary')).toHaveText([/MCP tools\s*3/, /Memory files\s*2/, /Custom agents\s*1/, /Skills\s*2/]);
  await lists.first().locator('summary').click();
  await expect(lists.first().locator('.crow')).toHaveCount(3);
  await expect(lists.first().locator('.crow').first()).toContainText('mcp__notes__search');
  await expect(lists.first().locator('.crow').first()).toContainText('notes');
  // Opened, it stays open: the summary shut and shown again.
  await main.getByTestId('chat-summary-button').click();
  await expect(summary).toHaveCount(0);
  await main.getByTestId('chat-summary-button').click();
  await expect(toggle).toHaveAttribute('aria-expanded', 'true');
  await expect(context.getByTestId('chat-summary-context-cat').first()).toBeVisible();
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
  await expect(pulls).toHaveCount(3);
  await expect(pulls.first()).toHaveAttribute('data-state', 'open');
  await expect(pulls.first()).toHaveAttribute('data-checks', 'running');
  await expect(pulls.first()).toContainText('Round toInches to one decimal');
  await expect(pulls.first()).toContainText('example-org/garden-planner #131');
  await expect(pulls.first().getByTestId('chat-summary-pr-checks')).toHaveText('Checks running · 0 of 1 passed');
  // A cancelled check is cancelled, as GitHub says it, never "failing"; skipped and neutral ones passed.
  const cancelled = pulls.nth(1);
  await expect(cancelled).toHaveAttribute('data-checks', 'cancelled');
  await expect(cancelled.getByTestId('chat-summary-pr-checks')).toHaveText('Checks cancelled · 3 of 4 passed');
  await expect(cancelled.getByTestId('chat-summary-pr-checks')).toHaveAttribute('title', '1 cancelled, 1 neutral, 2 successful checks');
  await expect(changes).not.toContainText('Checks failing');
  await expect(pulls.nth(2)).toHaveAttribute('data-state', 'merged');
  await expect(pulls.nth(2).getByTestId('chat-summary-pr-checks')).toHaveText('Checks passing');
  await expect(pulls.nth(2).getByTestId('chat-summary-pr-checks')).toHaveAttribute('title', '1 successful check');
  await expect(pulls.first().getByTestId('chat-summary-pr-archive')).toHaveCount(0);
  await expect(pulls.nth(2).getByTestId('chat-summary-pr-archive')).toBeVisible();
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
  // Folded again, in the dark too.
  await toggle.click();
  await expect(toggle).toHaveAttribute('aria-expanded', 'false');
  await shot(main, '07b-context-folded-dark');
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

  // An output's right-click menu: Open, Show in folder, Copy path, Open in <editor> (the first found: VS Code).
  const output = summary.getByTestId('chat-summary-outputs').getByTestId('chat-output').first();
  const menuItems = async () => {
    await output.click({ button: 'right' });
    const names = await main.getByRole('menu').getByRole('menuitem').allTextContents();
    await main.keyboard.press('Escape');
    return names.map((n) => n.trim());
  };
  await expect.poll(menuItems).toEqual(['Open', 'Show in folder', 'Copy path', 'Open in VS Code']);
  await output.click({ button: 'right' });
  await main.getByRole('menu').getByRole('menuitem', { name: 'Show in folder' }).click();
  await expect.poll(() => yap.invoke('e2e_opened_urls')).toContainEqual('reveal:src/convert.js');
  await output.click({ button: 'right' });
  await main.getByRole('menu').getByRole('menuitem', { name: 'Open in VS Code' }).click();
  await expect.poll(() => yap.invoke('e2e_opened_urls')).toContainEqual('editor:src/convert.js:0');

  // A merged pull request's Archive: this chat archived, as the list archives it.
  await pulls.nth(2).getByTestId('chat-summary-pr-archive').click();
  await expect.poll(async () => (await yap.invoke('chats_archived')).some((c) => c.id === conversation)).toBe(true);
  await expect(main.getByText('Chat archived')).toBeVisible();
  await yap.invoke('chat_archive', { ids: [conversation], archived: false });
  await yap.invoke('e2e_gh_script', { script: null });
  // Yap Lab's read-only gh (agents/prs/lab.rs) is a live lab's only: the suite never reaches the person's own gh.
  await expect(yap.invoke('e2e_gh_lab', { on: true })).rejects.toThrow(/live Yap Lab/);
});

test('a file a command made is in Outputs and opens as a sheet; Files changed says a command made or changed it; Rewind leaves it, as the Claude app', async ({ yap, main, shot }) => {
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
  // Found by comparing the workspace before and after the turn: the new file and the one the command changed.
  const agent = (await agents(yap)).find((j) => j.id === id);
  expect(agent.turnList[0].files).toEqual(['costs.csv', 'docs\\lab.md']);
  expect(agent.turnList[0].byCommand).toEqual([
    { file: 'costs.csv', new: true },
    { file: 'docs\\lab.md', new: false },
  ]);
  expect(agent.turnList[0].created).toEqual(['costs.csv']);
  // The report says what changed, with no word of backups or Undo.
  const marker = (await yap.invoke('chat_get', { id: agent.conversation })).messages.find((m) => m.kind === 'agentReport');
  expect(marker.text).toContain('Files its commands made: costs.csv.');
  expect(marker.text).toContain('Files its commands changed: docs\\lab.md.');
  expect(marker.text).not.toMatch(/backup|Undo/);

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

  // Its turn's Files changed: "New · By a command" and "By a command"; no Undo anywhere (as the Claude app).
  await thread(main).getByTestId('chat-agent-pill').first().click();
  const tab = main.getByTestId('chat-agent');
  await expect(tab).toBeVisible();
  await expect(tab.getByTestId('chat-agent-file')).toHaveCount(2);
  await expect(tab.locator('[data-testid="chat-agent-file"][data-file="costs.csv"]')).toHaveAttribute('data-by-command', 'new');
  await expect(tab.locator('[data-testid="chat-agent-file"][data-file="costs.csv"] .tag')).toHaveText(['New', 'By a command']);
  await expect(tab.locator('[data-testid="chat-agent-file"][data-file="docs\\\\lab.md"]')).toHaveAttribute('data-by-command', 'changed');
  await expect(tab.locator('[data-testid="chat-agent-file"][data-file="docs\\\\lab.md"] .tag')).toHaveText(['By a command']);
  await expect(tab.getByRole('button', { name: /^Undo/ })).toHaveCount(0);
  await shot(main, '11-files-changed-by-a-command');
  await theme(yap, main, 'dark');
  await shot(main, '11-files-changed-by-a-command-dark');
  await theme(yap, main, 'light');
  await tab.getByRole('button', { name: 'Close the panel' }).first().click();

  // Rewind to before the message, files too: Claude Code's checkpoints never kept a command's files, so they stay as
  // they are, and it says so (ask Claude to undo it), as the Claude app's Rewind.
  const index = (await yap.invoke('chat_get', { id: agent.conversation })).messages.findIndex((m) => m.role === 'user' && !m.kind);
  const r = await yap.invoke('agent_rewind', { conversation: agent.conversation, index, files: true });
  expect(r.filesBack).toBe(false);
  expect(r.problems).toEqual(['What its commands changed stays as it is (costs.csv, docs\\lab.md): ask Claude to undo it.']);
  expect(fs.readFileSync(path.join(GARDEN, 'costs.csv'), 'utf8')).toBe(COSTS);
  expect(fs.readFileSync(path.join(GARDEN, 'docs', 'lab.md'), 'utf8')).toBe(LAB_TIDIED);
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
  // A link's ⋯ (`moreSpot`): never over a line of the link, wholly in what shows of the thread.
  const box = (left, top, right, bottom) => ({ left, top, right, bottom });
  const size = { w: 22, h: 18 };
  const room = box(0, 0, 600, 400);
  // Room after its end: there, its middle on the line's.
  expect(moreSpot([box(100, 100, 160, 116)], size, room)).toEqual({ at: 'after', x: 163, y: 99 });
  // A short link at the edge of what shows: under its end, its right edge at the link's.
  expect(moreSpot([box(540, 100, 590, 116)], size, room)).toEqual({ at: 'below', x: 568, y: 117 });
  // …the last line too: over its end.
  expect(moreSpot([box(540, 380, 590, 396)], size, room)).toEqual({ at: 'above', x: 568, y: 361 });
  // A link that wraps, its last line ending at the edge: not over its first line's part, under the end instead.
  const wrapped = [box(400, 100, 598, 116), box(0, 120, 597, 136)];
  expect(moreSpot(wrapped, size, room).at).toBe('below');
  // Wrapped onto a last line it fills, the room's foot under it: before its start (the words before it).
  expect(moreSpot([box(400, 364, 598, 380), box(0, 382, 597, 398)], size, room)).toEqual({ at: 'before', x: 375, y: 363 });
  // Nowhere wholly in view: the first place that, pulled into view, stays clear of the link…
  expect(moreSpot([box(2, 100, 598, 116)], size, box(0, 90, 600, 134))).toEqual({ at: 'below', x: 576, y: 116 });
  // …and with none, after its end as it is: cut off by the thread's edge, never over the link.
  expect(moreSpot([box(0, 0, 600, 16)], size, box(0, 0, 600, 20))).toEqual({ at: 'after', x: 603, y: -1 });
  expect(moreSpot([], size, room)).toBe(null);
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
  // A pull request's checks in GitHub's words (agents/prs.rs `checks`, `counts`): cancelled is cancelled, never failing;
  // skipped and neutral passed ("8 of 10 checks passed"); every state on hover, in GitHub's order.
  const n = (o) => ({ failing: 0, cancelled: 0, pending: 0, skipped: 0, neutral: 0, successful: 0, ...o });
  expect(prChecks({ checks: 'cancelled', counts: n({ cancelled: 2, neutral: 1, successful: 7 }) })).toEqual({ word: 'Checks cancelled', detail: '8 of 10 passed', title: '2 cancelled, 1 neutral, 7 successful checks' });
  expect(prChecks({ checks: 'failing', counts: n({ failing: 1, cancelled: 1, neutral: 1, successful: 7 }) })).toEqual({ word: 'Checks failing', detail: '8 of 10 passed', title: '1 failing, 1 cancelled, 1 neutral, 7 successful checks' });
  expect(prChecks({ checks: 'passing', counts: n({ skipped: 1, neutral: 1, successful: 3 }) })).toEqual({ word: 'Checks passing', detail: '', title: '1 skipped, 1 neutral, 3 successful checks' });
  expect(prChecks({ checks: 'skipped', counts: n({ skipped: 1 }) })).toEqual({ word: 'Checks skipped', detail: '', title: '1 skipped check' });
  expect(prChecks({ checks: 'neutral', counts: n({ neutral: 2 }) }).word).toBe('Checks neutral');
  expect(prChecks({ checks: 'running', counts: n({ pending: 2, successful: 1 }) })).toEqual({ word: 'Checks running', detail: '1 of 3 passed', title: '2 pending, 1 successful checks' });
  expect(prChecks({ checks: 'none', counts: n({}) })).toEqual({ word: '', detail: '', title: '' });
  // (An older Yap's answer, with no counts: its word alone.)
  expect(prChecks({ checks: 'failing' })).toEqual({ word: 'Checks failing', detail: '', title: 'Checks failing' });
});
