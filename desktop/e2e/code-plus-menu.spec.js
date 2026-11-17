// **A Code chat's + menu** (17 Nov 2026, after the Claude Code app's: desktop/src-tauri/src/agents/menu.rs,
// views/chat/codeMenu.svelte.js): Add files or photos (Ctrl+U), Take a screenshot, Add folder, Slash commands ›,
// Plugins › (Search plugins, the session's MCP servers and connectors with a switch each for this chat, the
// plugins with their skills, Manage plugins…), then Voice replies, their volume and Voice settings….
//
// - What's listed is what the chat's Claude Code offers in its workspace: before any run there, a look — Claude
//   Code started with no message and asked `initialize` and `mcp_status` (the stand-in's `_offers`), its plugins
//   from `claude plugin list --json` (`_plugins`) — then each run's own `system/init` and `commands_changed`.
// - "/" at the start of the box opens the same commands; a pick puts `/name ` in the box; the box says
//   "Type / for commands".
// - A server switched off is `--disallowedTools mcp__<name>` on the chat's runs; an added folder is `--add-dir`,
//   shown in the summary panel's Environment with its ×. Never a folder inside the workspace, nor Yap's own data.
// - Manage plugins… opens Settings → Yap Code → Plugins: each installed plugin switched with `claude plugin
//   enable|disable`.
//
// Chat is on the plan with Yap's stand-in for Claude Code (`yap.exe fake-claude <script>`); the workspace and
// folders are in this run's own folder. No one's real Claude Code, account or plugins.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, setConfig, settingsDialog, closeSettings, closeToasts, jsonLines } from './support/fixtures.js';
import { fakeClaude, planConfig } from './support/claude-plan.js';
import { RUNS } from './support/yap.js';
import { newCodeChat } from './support/code-chats.js';
import { commandQuery, matchCommands, commandGroups, pluginRows, serverRows } from '../src/lib/views/chat/codeMenu.svelte.js';

const HERE = path.join(RUNS, `code-plus-menu-spec-w${process.env.TEST_WORKER_INDEX ?? 0}`);
const SCRIPTS = path.join(HERE, 'scripts');
const GARDEN = path.join(HERE, 'folders', 'garden-planner');
const SEEDS = path.join(HERE, 'folders', 'seed-catalogue');
const PLUGINS_DIR = path.join(HERE, 'plugins');
const ARGS = path.join(HERE, 'args.jsonl');
const LOOKS = path.join(HERE, 'looks.jsonl');
const PLUGIN_STATE = path.join(HERE, 'plugins.json');
const PLUGIN_RUNS = path.join(HERE, 'plugin-runs.jsonl');

// ---- what the stand-in offers (as 2.1.296's `initialize` answers: made-up commands, servers and plugins) ----

const COMMANDS = [
  { name: 'tidy-beds', description: "Tidy the flower beds' notes", argumentHint: '[bed]' },
  { name: 'plant-plan', description: "Plan this season's planting", argumentHint: '' },
  { name: 'garden-tools:water', description: '(garden-tools) Water the seedlings on a schedule', argumentHint: '<bed>' },
  { name: 'garden-tools:prune', description: "(garden-tools) Prune what's overgrown", argumentHint: '', aliases: ['prune'] },
  { name: 'seed-log:log', description: '(seed-log) Keep a log of what was sown and when', argumentHint: '', aliases: ['log'] },
  { name: 'compact', description: 'Free up context by summarizing the conversation so far', argumentHint: '<optional custom summarization instructions>', builtin: true },
  { name: 'context', description: 'Show current context usage', argumentHint: '', builtin: true },
  { name: 'doctor', description: "Health-check the setup (Claude Code's terminal only)", argumentHint: '', builtin: true },
];
const SERVERS = [
  { name: 'claude.ai Example Docs', status: 'connected', scope: 'claudeai', source: 'claudeai' },
  { name: 'issue-tracker', status: 'connected', scope: 'user', source: 'user' },
  { name: 'plugin:garden-tools:garden', status: 'needs-auth', scope: 'dynamic', source: 'plugin' },
  // Yap's own endpoint's name: never one to switch off.
  { name: 'yap', status: 'connected', scope: 'user', source: 'user' },
];
const pluginAt = (name) => path.join(PLUGINS_DIR, name);
const PLUGIN_LIST = () => [
  { id: 'garden-tools@example-market', version: '1.2.0', scope: 'user', enabled: true, installPath: pluginAt('garden-tools'), projectEnabled: false },
  { id: 'seed-log@example-market', version: '0.4.0', scope: 'user', enabled: true, installPath: pluginAt('seed-log'), projectEnabled: false },
  { id: 'old-helper@example-market', version: '0.1.0', scope: 'user', enabled: false, installPath: pluginAt('old-helper'), projectEnabled: false },
  { id: 'team-kit@synced', version: '0.2.0', scope: 'synced', enabled: true, installPath: pluginAt('team-kit') },
];
const OFFERS = { _offers: { commands: COMMANDS, mcpServers: SERVERS, dump: LOOKS } };
const PLUGINS = { _plugins: { list: PLUGIN_LIST(), state: PLUGIN_STATE, dump: PLUGIN_RUNS } };

/** A run's init as 2.1.296 sends it, with what the session offers. */
const init = () => ({
  type: 'system',
  subtype: 'init',
  cwd: '{{cwd}}',
  session_id: '{{session_id}}',
  tools: ['Bash', 'Edit', 'Read', 'Write', 'mcp__claude_ai_Example_Docs__read'],
  mcp_servers: SERVERS.map(({ name, status, source }) => ({ name, status, source })),
  model: 'claude-haiku-5-5',
  permissionMode: 'auto',
  slash_commands: COMMANDS.map((c) => c.name).concat(['__plumbing']),
  terminal_slash_commands: ['doctor'],
  skills: ['plant-plan', 'garden-tools:water', 'garden-tools:prune', 'seed-log:log'],
  plugins: [
    { name: 'garden-tools', path: pluginAt('garden-tools'), source: 'garden-tools@example-market', version: '1.2.0' },
    { name: 'seed-log', path: pluginAt('seed-log'), source: 'seed-log@example-market', version: '0.4.0' },
    { name: 'cc-plugin-example', path: pluginAt('builtin'), source: 'cc-plugin-example@builtin', version: '0.0.1' },
  ],
  apiKeySource: 'none',
  claude_code_version: '2.1.296',
  capabilities: ['interrupt_receipt_v1'],
});
const said = (id, text) => ({ type: 'assistant', message: { id, type: 'message', role: 'assistant', model: 'claude-haiku-5-5', content: [{ type: 'text', text }], stop_reason: 'end_turn' }, parent_tool_use_id: null, session_id: '{{session_id}}' });
const result = (text) => ({ type: 'result', subtype: 'success', is_error: false, duration_ms: 400, num_turns: 1, result: text, session_id: '{{session_id}}', total_cost_usd: 0.01, usage: { input_tokens: 900, output_tokens: 20, cache_creation_input_tokens: 0, cache_read_input_tokens: 0 }, permission_denials: [], terminal_reason: 'completed' });
const ANSWER = 'Hello from the garden planner.';
/** Every turn: its arguments dumped, its init and commands, a one-line answer. */
const TURNS = [
  OFFERS,
  PLUGINS,
  { _wait_user: true },
  { _dump_args: ARGS },
  init(),
  { type: 'system', subtype: 'commands_changed', commands: COMMANDS },
  said('a1', ANSWER),
  result(ANSWER),
  { _wait_user: true },
  { _dump_args: ARGS },
  init(),
  said('a2', ANSWER),
  result(ANSWER),
  { _wait_user: true },
];

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
  fs.mkdirSync(path.join(GARDEN, 'src'), { recursive: true });
  fs.writeFileSync(path.join(SEEDS, 'catalogue.md'), '# Seeds\n');
  for (const [name, description] of [
    ['garden-tools', 'Tools for a garden planner'],
    ['seed-log', 'A log of what was sown'],
    ['old-helper', 'An old helper, switched off'],
  ]) {
    fs.mkdirSync(path.join(pluginAt(name), '.claude-plugin'), { recursive: true });
    fs.writeFileSync(path.join(pluginAt(name), '.claude-plugin', 'plugin.json'), JSON.stringify({ name, description, version: '1.0.0' }));
  }
  for (const f of [ARGS, LOOKS, PLUGIN_STATE, PLUGIN_RUNS]) fs.rmSync(f, { force: true });
}

const test = base.extend({
  yapOptions: [
    async ({}, use) => {
      await use({
        name: 'code-plus-menu',
        env: fakeClaude(writeScript('turns', TURNS)),
        config: planConfig({ 'claude-plan': 'claude-plan' }, { memoryLearnChats: false }),
      });
    },
    { scope: 'worker' },
  ],
});

const plus = (main) => main.getByRole('button', { name: 'Chat options' });
const menu = (main) => main.getByTestId('code-plus-menu');
const box = (main) => main.getByRole('textbox', { name: 'Message' });
const agents = (yap) => yap.invoke('agent_list');
const argsSeen = () => jsonLines(ARGS).map((l) => l.args);
const after = (args, flag) => {
  const i = args.indexOf(flag);
  return i >= 0 ? args[i + 1] : undefined;
};
/** The values a variadic flag took (up to the next flag). */
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
async function send(yap, main, text) {
  const before = argsSeen().length;
  await box(main).fill(text);
  await main.getByRole('button', { name: 'Send', exact: true }).click();
  await expect.poll(() => argsSeen().length, { timeout: 20_000, message: 'its run to start' }).toBe(before + 1);
  await expect(main.locator('.msgs .cbubble.assistant').last()).toContainText(ANSWER, { timeout: 20_000 });
  return argsSeen().at(-1);
}

test.beforeEach(async ({ yap, main }) => {
  freshFolders();
  await yap.invoke('e2e_claude_script', { script: writeScript('turns', TURNS) });
  await yap.invoke('e2e_claude_approve_auto', { allow: true });
  await setConfig(yap, main, { agentsEnabled: true, agentsDirect: true, agentsAsk: 'new-or-web', agentsSpeak: 'never', agentsMode: 'auto', webSearch: false, speakReplies: false, assistantScreen: false, theme: 'light' });
  if (!(await yap.invoke('workspaces_list')).some((f) => f.name === 'garden-planner')) await yap.invoke('workspaces_add', { path: GARDEN });
});

test('the "/" list matches names (a plugin skill by its own part too), groups yours before the built-in ones, and the plugins and servers filter', () => {
  expect(commandQuery('/')).toBe('');
  expect(commandQuery('/garden-tools:wa')).toBe('garden-tools:wa');
  expect(commandQuery('/tidy beds')).toBeNull();
  expect(commandQuery('tidy')).toBeNull();
  const list = [
    { name: 'tidy-beds', description: 'Tidy the beds' },
    { name: 'garden-tools:water', description: 'Water', plugin: 'garden-tools' },
    { name: 'compact', description: 'Free up context', builtin: true },
    { name: 'plant-plan', description: 'Plan the tidy bit' },
  ];
  expect(matchCommands(list, 'wa').map((c) => c.name)).toEqual(['garden-tools:water']);
  // Another name of a command's counts as its own (/review for /code-review).
  expect(matchCommands([...list, { name: 'code-review', aliases: ['review'], description: 'Review the diff' }], 'rev')[0].name).toBe('code-review');
  expect(matchCommands(list, 'tidy').map((c) => c.name)).toEqual(['tidy-beds', 'plant-plan']);
  expect(matchCommands(list, '').length).toBe(4);
  expect(commandGroups(list).map((g) => [g.label, g.items.length, g.start])).toEqual([
    ['Commands and skills', 3, 0],
    ['Built in', 1, 3],
  ]);
  const offered = {
    commands: [...list, { name: 'garden-tools:prune', plugin: 'garden-tools', description: 'Prune' }, { name: 'seed-log:log', plugin: 'seed-log', description: 'Keep a log' }],
    plugins: [
      { name: 'garden-tools', description: 'Tools' },
      { name: 'seed-log', description: 'Seeds' },
      { name: 'team-kit', description: 'A kit' },
    ],
    servers: [
      { name: 'claude.ai Example Docs', label: 'Example Docs' },
      { name: 'issue-tracker', label: 'issue-tracker' },
    ],
  };
  expect(pluginRows(offered).map((p) => [p.name, p.skills.length, p.line])).toEqual([
    ['garden-tools', 2, 'Tools'],
    ['seed-log', 1, 'Keep a log'],
    ['team-kit', 0, 'A kit'],
  ]);
  expect(pluginRows(offered, 'prune').map((p) => p.name)).toEqual(['garden-tools']);
  expect(serverRows(offered, ['issue-tracker']).map((s) => [s.label, s.on])).toEqual([
    ['Example Docs', true],
    ['issue-tracker', false],
  ]);
  expect(serverRows(offered, [], 'docs').map((s) => s.label)).toEqual(['Example Docs']);
});

test("a Code chat's + menu lists what its Claude Code offers before its first message: commands, servers with switches, plugins with skills", async ({ yap, main, shot }) => {
  await codeChat(main);
  // The box says how to reach the commands.
  await expect(box(main)).toHaveAttribute('placeholder', 'Type / for commands');
  await plus(main).click();
  await expect(menu(main)).toBeVisible();
  // The Claude Code app's rows, in its order, then the voice group.
  await expect(menu(main).locator(':scope > .mrow, :scope > .optsub > .mrow').locator('.mlabel')).toHaveText([
    'Add files or photos',
    'Take a screenshot',
    'Add folder',
    'Slash commands',
    'Plugins',
    'Voice replies',
    'Voice settings…',
  ]);
  await expect(menu(main).getByRole('button', { name: 'Add files or photos' })).toContainText('Ctrl+U');
  await expect(menu(main).getByRole('slider', { name: 'Voice volume' })).toBeVisible();
  // Nothing was known of the workspace: Claude Code was looked at there, no message sent.
  await expect.poll(() => jsonLines(LOOKS).length, { timeout: 15_000 }).toBeGreaterThan(0);
  const look = jsonLines(LOOKS)[0];
  expect(path.resolve(look.cwd)).toBe(path.resolve(GARDEN));
  expect(look.args).toContain('--no-session-persistence');
  expect(argsSeen()).toHaveLength(0);
  await shot(main, '01-the-code-plus-menu-light');

  // Slash commands ›: the person's own and their plugins', then Claude Code's built-in ones; never a terminal-only one.
  await menu(main).getByRole('button', { name: 'Slash commands' }).click();
  const cmds = main.getByTestId('code-plus-commands-menu');
  await expect(cmds.locator('.mhead')).toHaveText(['Commands and skills', 'Built in'], { timeout: 15_000 });
  await expect(cmds.locator('.mcmd')).toHaveText(['/tidy-beds', '/plant-plan', '/garden-tools:water', '/garden-tools:prune', '/seed-log:log', '/compact', '/context']);
  // A plugin's own name is off its line.
  await expect(cmds.getByRole('menuitem', { name: /garden-tools:water/ })).toContainText('Water the seedlings on a schedule');
  await expect(cmds.getByRole('menuitem', { name: /garden-tools:water/ })).not.toContainText('(garden-tools)');
  await shot(main, '02-slash-commands-light');

  // Plugins ›: Search plugins, the servers (Yap's own endpoint never) with a switch each, the plugins.
  await menu(main).getByRole('button', { name: 'Plugins' }).hover();
  const plugins = main.getByTestId('code-plus-plugins-menu');
  await expect(plugins).toBeVisible();
  await expect(plugins.getByRole('searchbox', { name: 'Search plugins' })).toBeVisible();
  const servers = plugins.getByTestId('code-plus-server');
  await expect(servers.locator('.mlabel')).toHaveText(['Example Docs', 'issue-tracker', 'garden · Needs sign-in']);
  for (const s of await servers.all()) await expect(s).toHaveAttribute('aria-checked', 'true');
  const rows = plugins.getByTestId('code-plus-plugin');
  await expect(rows).toHaveCount(3);
  // Several skills: "2 skills ›"; one: its line; none: the plugin's own (a synced plugin with no manifest: its name).
  await expect(rows.nth(0)).toContainText('garden-tools');
  await expect(rows.nth(0)).toContainText('2 skills');
  await expect(rows.nth(1)).toContainText('Keep a log of what was sown and when');
  await expect(rows.nth(2)).toContainText('team-kit');
  await expect(plugins.getByRole('menuitem', { name: 'Manage plugins…' })).toBeVisible();
  await rows.nth(0).hover();
  const skills = main.getByTestId('code-plus-plugin-skills');
  await expect(skills.locator('.mcmd')).toHaveText(['/garden-tools:water', '/garden-tools:prune']);
  // Whole in the window: no room on the right, they open on the other side.
  await expect
    .poll(async () => {
      const [b, vw] = await Promise.all([skills.boundingBox(), main.evaluate(() => window.innerWidth)]);
      return b && b.x >= 0 && b.x + b.width <= vw;
    })
    .toBe(true);
  await shot(main, '03-plugins-and-their-skills-light');
  // Search: what holds the words.
  await plugins.getByRole('searchbox', { name: 'Search plugins' }).fill('seed');
  await expect(rows).toHaveCount(1);
  await expect(servers).toHaveCount(0);
  await plugins.getByRole('searchbox', { name: 'Search plugins' }).fill('');
  // A skill picked: `/name ` in the box, the menu closed.
  await rows.nth(0).hover();
  await skills.getByRole('menuitem', { name: /garden-tools:prune/ }).click();
  await expect(menu(main)).toHaveCount(0);
  await expect(box(main)).toHaveValue('/garden-tools:prune ');

  // Dark.
  await box(main).fill('');
  await theme(yap, main, 'dark');
  await plus(main).click();
  await menu(main).getByRole('button', { name: 'Plugins' }).hover();
  await expect(plugins.getByTestId('code-plus-server')).toHaveCount(3);
  await plugins.getByTestId('code-plus-plugin').first().hover();
  await expect(main.getByTestId('code-plus-plugin-skills')).toBeVisible();
  await shot(main, '04-plugins-and-their-skills-dark');
  await menu(main).getByRole('button', { name: 'Slash commands' }).hover();
  await expect(main.getByTestId('code-plus-commands-menu')).toBeVisible();
  await shot(main, '05-slash-commands-dark');
  await main.keyboard.press('Escape');
  await main.keyboard.press('Escape');
  await expect(menu(main)).toHaveCount(0);
  await theme(yap, main, 'light');
});

test('"/" at the start of the box opens the same commands: typing narrows them, Enter or a click puts one in the box', async ({ yap, main, shot }) => {
  await codeChat(main);
  await box(main).click();
  await box(main).pressSequentially('/');
  const list = main.getByTestId('code-command-menu');
  await expect(list).toBeVisible();
  await expect(list.getByRole('option').first()).toContainText('/tidy-beds', { timeout: 15_000 });
  await expect(list.locator('.ghead')).toHaveText(['Commands and skills', 'Built in']);
  await expect(list).not.toContainText('/doctor');
  // Yap's own Skills menu never opens in a Code chat.
  await expect(main.getByTestId('skill-menu')).toHaveCount(0);
  await shot(main, '06-slash-in-the-box-light');
  await box(main).pressSequentially('pl');
  await expect(list.getByRole('option')).toHaveText([/\/plant-plan/]);
  await box(main).press('Enter');
  await expect(list).toHaveCount(0);
  await expect(box(main)).toHaveValue('/plant-plan ');
  // A plugin's skill by its own part; ↓ then Tab.
  await box(main).fill('');
  await box(main).pressSequentially('/wa');
  await expect(list.getByRole('option').first()).toContainText('/garden-tools:water');
  await expect(list.getByRole('option').first()).toContainText('<bed>');
  await box(main).press('Tab');
  await expect(box(main)).toHaveValue('/garden-tools:water ');
  // Esc closes it for the words as they are.
  await box(main).fill('');
  await box(main).pressSequentially('/co');
  await expect(list).toBeVisible();
  await box(main).press('Escape');
  await expect(list).toHaveCount(0);
  // A command typed in full goes as typed: straight to its session, never one of Yap's Skills. (/clear, /compact and
  // /model are Yap's own to do: code-commands.spec.js.)
  await box(main).fill('');
  await box(main).pressSequentially('/tidy-beds');
  await box(main).press('Enter');
  await expect.poll(() => argsSeen().length, { timeout: 20_000 }).toBe(1);
  const said = (await yap.invoke('chat_get', { id: (await agents(yap))[0].conversation })).messages[0].text;
  expect(said).toBe('/tidy-beds');
  await theme(yap, main, 'dark');
  await box(main).click();
  await box(main).pressSequentially('/');
  await expect(list).toBeVisible();
  await shot(main, '07-slash-in-the-box-dark');
  await box(main).press('Escape');
  await box(main).fill('');
  await theme(yap, main, 'light');
});

test("a server switched off in the + menu is disallowed on the chat's runs, and stays off for that chat", async ({ yap, main }) => {
  await codeChat(main);
  await plus(main).click();
  await menu(main).getByRole('button', { name: 'Plugins' }).click();
  const plugins = main.getByTestId('code-plus-plugins-menu');
  const tracker = plugins.getByTestId('code-plus-server').filter({ hasText: 'issue-tracker' });
  const docs = plugins.getByTestId('code-plus-server').filter({ hasText: 'Example Docs' });
  await expect(tracker).toHaveAttribute('aria-checked', 'true', { timeout: 15_000 });
  await tracker.click();
  await docs.click();
  await expect(tracker).toHaveAttribute('aria-checked', 'false');
  await expect(docs).toHaveAttribute('aria-checked', 'false');
  // Back on: only the tracker stays off.
  await docs.click();
  await expect(docs).toHaveAttribute('aria-checked', 'true');
  await main.keyboard.press('Escape');
  await main.keyboard.press('Escape');
  // The new chat's first message: its run has the tracker's tools disallowed (the documented rule for a whole server).
  const first = await send(yap, main, 'Hello');
  expect(after(first, '--disallowedTools')?.split(',')).toContain('mcp__issue-tracker');
  expect(after(first, '--disallowedTools')).not.toContain('Example_Docs');
  const conversation = (await agents(yap))[0].conversation;
  expect((await yap.invoke('code_menu', { conversation })).serversOff).toEqual(['issue-tracker']);
  // The next turn too; switched back on, gone from the one after.
  const second = await send(yap, main, 'And again');
  expect(after(second, '--disallowedTools')?.split(',')).toContain('mcp__issue-tracker');
  await yap.invoke('code_menu_server', { conversation, server: 'issue-tracker', on: true });
  await yap.invoke('e2e_claude_script', { script: writeScript('turns', TURNS) });
  const third = await send(yap, main, 'Once more');
  expect(after(third, '--disallowedTools') ?? '').not.toContain('issue-tracker');
  // Yap's own endpoint is never one to switch off.
  await expect(yap.invoke('code_menu_server', { conversation, server: 'yap', on: false })).rejects.toThrow(/can't be switched/);
  // The run's own init kept what it offers there.
  const after1 = await yap.invoke('code_menu', { conversation });
  expect(after1.offered.from).toBe('init');
  expect(after1.offered.commands.find((c) => c.name === 'tidy-beds').hint).toBe('[bed]');
  expect(after1.offered.plugins.map((p) => p.name)).toEqual(['garden-tools', 'seed-log']);
});

test('Add folder gives the chat\'s Claude Code another folder (--add-dir), shown in the summary panel with its ×; never one inside the workspace', async ({ yap, main, shot }) => {
  await codeChat(main);
  // One inside the workspace: refused, said.
  await yap.invoke('e2e_workspace', { path: path.join(GARDEN, 'src') });
  await plus(main).click();
  await menu(main).getByRole('button', { name: 'Add folder' }).click();
  await expect(main.locator('.toast').filter({ hasText: "Couldn't add that folder" })).toContainText("in this chat's workspace already");
  await closeToasts(main);
  // A folder beside it: added (before the chat's first message, kept for it).
  await yap.invoke('e2e_workspace', { path: SEEDS });
  await plus(main).click();
  await menu(main).getByRole('button', { name: 'Add folder' }).click();
  await expect(main.locator('.toast').filter({ hasText: 'Claude Code can use seed-catalogue in this chat too' })).toBeVisible();
  await closeToasts(main);
  await plus(main).click();
  await expect(menu(main).getByRole('button', { name: 'Add folder' })).toContainText('1 added');
  await main.keyboard.press('Escape');
  const first = await send(yap, main, 'What seeds are in the catalogue?');
  expect(valuesOf(first, '--add-dir').map((p) => path.resolve(p))).toEqual([path.resolve(SEEDS)]);
  const conversation = (await agents(yap))[0].conversation;

  // The summary panel's Environment: the workspace, then the folder with its ×.
  await main.getByTestId('chat-summary-button').click();
  const env = main.getByTestId('chat-summary-environment');
  const row = env.getByTestId('chat-summary-folder');
  await expect(row).toHaveCount(1);
  await expect(row).toContainText('seed-catalogue');
  await shot(main, '08-an-added-folder-in-the-summary-light');
  await theme(yap, main, 'dark');
  await row.hover();
  await shot(main, '09-an-added-folder-in-the-summary-dark');
  await theme(yap, main, 'light');
  await row.getByRole('button', { name: 'Take seed-catalogue off this chat' }).click();
  await expect(row).toHaveCount(0);
  expect((await yap.invoke('code_menu', { conversation })).folders).toEqual([]);
  const second = await send(yap, main, 'And now?');
  expect(second).not.toContain('--add-dir');
  // Added again after the first message: the panel follows.
  await yap.invoke('e2e_workspace', { path: SEEDS });
  await plus(main).click();
  await menu(main).getByRole('button', { name: 'Add folder' }).click();
  await expect(row).toHaveCount(1);
  await closeToasts(main);
  await main.getByRole('button', { name: 'Close the panel' }).first().click();
});

test("Manage plugins… opens Settings → Yap Code → Plugins: each installed one with its line, switched with Claude Code's own command", async ({ yap, main, shot }) => {
  await codeChat(main);
  await plus(main).click();
  await menu(main).getByRole('button', { name: 'Plugins' }).click();
  await main.getByTestId('code-manage-plugins').click();
  const dialog = settingsDialog(main);
  await expect(dialog).toBeVisible();
  const group = dialog.getByRole('group', { name: 'Plugins' });
  await expect(group).toBeInViewport({ timeout: 15_000 });
  const rows = group.getByTestId('yap-code-plugin');
  await expect(rows).toHaveCount(4);
  await expect(rows.nth(0)).toContainText('garden-tools');
  await expect(rows.nth(0)).toContainText('Tools for a garden planner');
  await expect(rows.nth(0)).toContainText('Everywhere · 1.2.0 · example-market');
  await expect(rows.nth(2).getByRole('button', { name: 'old-helper in Claude Code' })).toHaveAttribute('aria-pressed', 'false');
  // A synced plugin is switched where it comes from.
  await expect(rows.nth(3).getByRole('button', { name: 'team-kit in Claude Code' })).toBeDisabled();
  await shot(main, '10-settings-plugins-light');
  await rows.nth(0).getByRole('button', { name: 'garden-tools in Claude Code' }).click();
  await expect.poll(() => jsonLines(PLUGIN_RUNS).map((l) => l.args)).toEqual([['disable', 'garden-tools@example-market', '--scope', 'user']]);
  await expect(rows.nth(0).getByRole('button', { name: 'garden-tools in Claude Code' })).toHaveAttribute('aria-pressed', 'false');
  // Every Code chat's menu is looked at again: the switched-off plugin's gone from the next look.
  await theme(yap, main, 'dark');
  await shot(main, '11-settings-plugins-dark');
  await theme(yap, main, 'light');
  await closeSettings(main);
  await plus(main).click();
  await menu(main).getByRole('button', { name: 'Plugins' }).click();
  await expect(main.getByTestId('code-plus-plugins-menu').getByTestId('code-plus-plugin')).toHaveCount(2, { timeout: 15_000 });
  await main.keyboard.press('Escape');
  await main.keyboard.press('Escape');
});
