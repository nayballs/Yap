// AI apps over MCP, end to end. Settings → MCP (Wispr Flow's MCP page, local)
// adds Yap to an AI app's own config (in a stand-in user profile: never a
// real AI app), and a real `yap.exe mcp` session, started the way AI apps
// start it, reads the seeded meetings through this instance's local API.
// The dictation history never comes out.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, openSettings, closeSettings, expectStore } from './support/fixtures.js';
import { startMcp } from './support/mcp-client.js';
import { RUNS } from './support/yap.js';

const T0 = Date.UTC(2026, 9, 5, 13, 0, 0) / 1000;

const PLANNING = {
  id: 1,
  title: 'Q3 planning sync',
  noteType: 'meeting',
  folder: 'Meetings',
  source: 'meeting',
  participants: ['Priya', 'Tom'],
  content: 'Venue shortlist: Lisbon, Porto',
  enhancedContent:
    '## Action plan\n### Priya\n- [ ] send the revised budget (due: Friday)\n### Tom\n- [ ] book the venue\n## Decisions\n- The offsite is in Lisbon.',
  transcript: [
    { source: 'you', text: "Let's start with the budget for the offsite.", ts: T0 },
    { source: 'them', text: 'Priya here. I will send the revised budget by Friday.', ts: T0 + 20 },
    { source: 'you', text: 'Great. Tom, can you book the venue?', ts: T0 + 45 },
    { source: 'them', text: 'Tom: yes, I will book it this week.', ts: T0 + 60 },
    { source: 'them', text: 'We decided on Lisbon, the numbers work best there.', ts: T0 + 1500 },
  ],
  createdTs: T0 - 60,
  updatedTs: T0 + 1600,
};

// Long enough for several transcript pages.
const LOGISTICS = {
  id: 2,
  title: 'Offsite logistics',
  noteType: 'meeting',
  folder: 'Meetings',
  source: 'meeting',
  participants: [],
  transcript: Array.from({ length: 700 }, (_, i) => ({
    source: i % 2 ? 'them' : 'you',
    text:
      i === 501
        ? 'The catering budget is still the open question.'
        : `Line ${i}: we went through travel, rooms and the agenda for both days of the offsite.`,
    ts: T0 + 86_400 + i * 15,
  })),
  createdTs: T0 + 86_000,
  updatedTs: T0 + 97_000,
};

const GROCERIES = {
  id: 3,
  title: 'Groceries',
  noteType: 'personal',
  folder: 'Personal',
  source: 'manual',
  content: 'Oat milk, coffee and bread',
  createdTs: T0 - 7200,
  updatedTs: T0 - 7200,
};

// A dictation: it must never reach an AI app.
const DICTATION = { ts: T0 - 3600, raw: 'zebracorn password hint', text: 'Zebracorn password hint', model: 'parakeet', app: 'notepad.exe' };

// The stand-in user profile's AI apps.
const CLAUDE_TEXT = `${JSON.stringify(
  {
    mcpServers: { filesystem: { command: 'npx', args: ['-y', '@modelcontextprotocol/server-filesystem', 'C:\\Users\\me\\Desktop'] } },
    preferences: { menuBarEnabled: false },
  },
  null,
  2
)}\n`;
const GEMINI_TEXT = '{\n  // my theme\n  "theme": "GitHub"\n}\n';
const CODEX_TEXT = '# Codex settings\nmodel = "gpt-5-codex"\n\n[mcp_servers.docs]\ncommand = "npx"\nargs = ["-y", "docs-mcp"]\n';
const CLAUDE_CODE_TEXT = `${JSON.stringify({ numStartups: 12, projects: { 'C:/code/app': { allowedTools: [] } }, mcpServers: {} }, null, 2)}\n`;

const FILES = {
  claude: 'AppData/Roaming/Claude/claude_desktop_config.json',
  gemini: '.gemini/settings.json',
  codex: '.codex/config.toml',
  claudeCode: '.claude.json',
  cursor: '.cursor/mcp.json',
};

const test = base.extend({
  profile: [
    async ({}, use, workerInfo) => {
      const root = path.join(RUNS, `mcp-profile-w${workerInfo.workerIndex}`);
      fs.rmSync(root, { recursive: true, force: true });
      const put = (rel, text) => {
        fs.mkdirSync(path.dirname(path.join(root, rel)), { recursive: true });
        fs.writeFileSync(path.join(root, rel), text);
      };
      put(FILES.claude, CLAUDE_TEXT);
      put(FILES.gemini, GEMINI_TEXT);
      put(FILES.codex, CODEX_TEXT);
      put(FILES.claudeCode, CLAUDE_CODE_TEXT);
      fs.mkdirSync(path.join(root, '.cursor'), { recursive: true }); // installed, no config yet
      // VS Code and Windsurf aren't installed.
      await use(root);
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({ profile }, use) => {
      await use({
        name: 'mcp',
        config: { bridgeEnabled: true },
        env: { YAP_MCP_CLIENT_ROOT: profile },
        data: {
          'notes.json': { folders: ['Personal', 'Meetings'], notes: [PLANNING, LOGISTICS, GROCERIES] },
          'history.json': [DICTATION],
        },
      });
    },
    { scope: 'worker' },
  ],
});

const read = (profile, rel) => fs.readFileSync(path.join(profile, rel), 'utf8');
const exe = (yap) => path.join(yap.appDir, 'yap.exe');
const toast = (main, title) => main.getByRole('status').filter({ hasText: title });

/** Settings → MCP (in the "Connections" group). */
async function openMcp(main) {
  const dialog = await openSettings(main, 'MCP');
  await expect(dialog.getByRole('heading', { level: 1, name: 'MCP' })).toBeVisible();
  await expect(dialog.getByRole('list', { name: 'AI apps', exact: true })).toBeVisible();
  return dialog;
}

// Wispr's four cards, then "All other apps".
const app = (page, name) => page.getByRole('list', { name: 'AI apps', exact: true }).getByRole('listitem', { name, exact: true });
const other = (page, name) => page.getByRole('list', { name: 'Other AI apps' }).getByRole('listitem', { name, exact: true });

test('Integrations points to Settings → MCP', async ({ main, shot }) => {
  await openView(main, 'Integrations');
  const card = main.getByRole('region', { name: 'AI apps (MCP)' });
  await expect(card).toContainText("They can't see your dictations");
  await expect(main.getByText(/^Running · 127\.0\.0\.1:\d+$/)).toBeVisible(); // the Local API it needs
  await shot(main, '01-integrations-go-to-mcp');
  await card.getByRole('button', { name: 'Go to MCP' }).click();
  const dialog = main.getByRole('dialog', { name: 'Settings' });
  await expect(dialog.getByRole('heading', { level: 1, name: 'MCP' })).toBeVisible();
  await expect(
    dialog.getByRole('navigation', { name: 'Settings sections' }).getByRole('button', { name: 'MCP' })
  ).toHaveAttribute('aria-current', 'page');
  await closeSettings(main);
});

test('Settings → MCP lists each AI app with what Yap found on this PC', async ({ main, shot }) => {
  const page = await openMcp(main);
  await expect(page).toContainText("Yap's MCP can't see your dictations, and everything stays on this PC.");
  // Wispr's order, each with its "Allow … to access" line and Add button.
  const names = await page
    .getByRole('list', { name: 'AI apps', exact: true })
    .getByRole('listitem')
    .evaluateAll((items) => items.map((i) => i.getAttribute('aria-label')));
  expect(names).toEqual(['Claude', 'ChatGPT', 'Gemini', 'Cursor']);
  for (const name of ['Claude', 'ChatGPT', 'Cursor']) {
    await expect(app(page, name)).toContainText(`Allow ${name} to access your meeting notes and transcripts`);
    await expect(app(page, name).getByRole('button', { name: `Add to ${name}` })).toBeVisible();
  }
  await expect(app(page, 'ChatGPT')).toContainText("ChatGPT on the web can't reach apps on your PC");
  await expect(app(page, 'Gemini')).toContainText("isn't plain JSON (line 2, column 3)");
  await shot(main, '02-settings-mcp');

  // All other apps: one click where Yap knows the app, else the command/JSON.
  await expect(other(page, 'Claude Code').getByRole('button', { name: 'Add to Claude Code' })).toBeVisible();
  for (const name of ['VS Code', 'Windsurf']) {
    await expect(other(page, name)).toContainText('Not installed');
    await expect(other(page, name).getByRole('button')).toHaveCount(0);
  }
  await expect(page.locator('code')).toContainText('yap.exe" mcp');
  await expect(page.locator('pre')).toContainText('"mcpServers"');
  const writes = page.getByRole('button', { name: 'Let AI apps save notes to Yap' });
  await expect(writes).toHaveAttribute('aria-pressed', 'false');
  await page.getByText('with no Yap server in between').scrollIntoViewIfNeeded();
  await shot(main, '03-all-other-apps');
});

test('Add to Claude merges into its config; Remove takes only Yap out', async ({ yap, main, profile, shot }) => {
  const page = await openMcp(main);
  await app(page, 'Claude').getByRole('button', { name: 'Add to Claude' }).click();
  await expect(toast(main, 'Added to Claude')).toBeVisible();
  await expect(toast(main, 'Added to Claude')).toContainText('Quit Claude');
  await expect(app(page, 'Claude')).toContainText('Added ✓');

  const cfg = JSON.parse(read(profile, FILES.claude));
  expect(Object.keys(cfg.mcpServers)).toEqual(['filesystem', 'yap']);
  expect(cfg.mcpServers.yap.command.toLowerCase()).toBe(exe(yap).toLowerCase());
  expect(cfg.mcpServers.yap.args).toEqual(['mcp']);
  expect(cfg.mcpServers.filesystem).toEqual(JSON.parse(CLAUDE_TEXT).mcpServers.filesystem);
  expect(cfg.preferences).toEqual({ menuBarEnabled: false });
  expect(read(profile, `${FILES.claude}.bak`)).toBe(CLAUDE_TEXT);
  await shot(main, '04-added-to-claude');

  await app(page, 'Claude').getByRole('button', { name: 'Remove Yap from Claude' }).click();
  await expect(app(page, 'Claude').getByRole('button', { name: 'Add to Claude' })).toBeVisible();
  expect(read(profile, FILES.claude)).toBe(CLAUDE_TEXT); // byte for byte
});

test('Cursor, ChatGPT and Claude Code each get their own format', async ({ yap, main, profile, shot }) => {
  const page = await openMcp(main);
  const rows = { Cursor: app(page, 'Cursor'), ChatGPT: app(page, 'ChatGPT'), 'Claude Code': other(page, 'Claude Code') };
  for (const [name, row] of Object.entries(rows)) {
    await row.getByRole('button', { name: `Add to ${name}` }).click();
    await expect(row).toContainText('Added ✓');
  }
  const cursor = JSON.parse(read(profile, FILES.cursor));
  expect(cursor.mcpServers.yap).toMatchObject({ type: 'stdio', args: ['mcp'] });
  expect(cursor.mcpServers.yap.command.toLowerCase()).toBe(exe(yap).toLowerCase());

  const codex = read(profile, FILES.codex);
  expect(codex.startsWith(CODEX_TEXT)).toBe(true); // comments and the other server kept
  expect(codex).toContain('[mcp_servers.yap]');
  expect(codex).toContain('args = ["mcp"]');

  const code = JSON.parse(read(profile, FILES.claudeCode));
  expect(code.numStartups).toBe(12);
  expect(code.projects).toEqual({ 'C:/code/app': { allowedTools: [] } });
  expect(code.mcpServers.yap).toMatchObject({ type: 'stdio', args: ['mcp'], env: {} });
  expect(fs.existsSync(path.join(profile, '.claude.json.lock'))).toBe(false);
  await shot(main, '05-cursor-chatgpt-claude-code-added');

  for (const [name, row] of Object.entries(rows)) {
    await row.getByRole('button', { name: `Remove Yap from ${name}` }).click();
    await expect(row.getByRole('button', { name: `Add to ${name}` })).toBeVisible();
  }
  expect(read(profile, FILES.codex)).toBe(CODEX_TEXT);
  expect(read(profile, FILES.claudeCode)).toBe(CLAUDE_CODE_TEXT);
  expect(JSON.parse(read(profile, FILES.cursor))).toEqual({ mcpServers: {} });
});

test("a config Yap can't read is left alone, with the reason", async ({ main, profile, shot }) => {
  const page = await openMcp(main);
  await app(page, 'Gemini').getByRole('button', { name: 'Add to Gemini' }).click();
  await expect(toast(main, "Couldn't add Yap to Gemini")).toBeVisible();
  await expect(toast(main, "Couldn't add Yap to Gemini")).toContainText('so Yap left it alone');
  expect(read(profile, FILES.gemini)).toBe(GEMINI_TEXT);
  expect(fs.existsSync(path.join(profile, `${FILES.gemini}.bak`))).toBe(false);
  await shot(main, '06-gemini-left-alone');
});

test('with the Local API off, Settings → MCP says so and turns it back on', async ({ yap, main, shot }) => {
  // Switched off where people do it: Integrations → Local API.
  await openView(main, 'Integrations');
  await main.locator('label.switch').click();
  await expect(main.getByText('Off', { exact: true })).toBeVisible();
  expect((await yap.invoke('bridge_status')).running).toBe(false);

  const page = await openMcp(main);
  const warning = page.getByRole('note');
  await expect(warning).toContainText('through its Local API, which is off');
  await shot(main, '07-local-api-off');

  await warning.getByRole('button', { name: 'Turn on the Local API' }).click();
  await expect(warning).toBeHidden();
  expect((await yap.invoke('bridge_status')).running).toBe(true);
  expect(fs.existsSync(yap.bridgeFile)).toBe(true);
  await expectStore(yap, 'config.json', (c) => c.bridgeEnabled === true);
  // Settings' own copy agrees, so its next auto-save keeps the API on.
  await page.getByRole('navigation', { name: 'Settings sections' }).getByRole('button', { name: /^Advanced/ }).click();
  await page.getByRole('button', { name: 'Append trailing space' }).click();
  await expectStore(yap, 'config.json', (c) => c.appendTrailingSpace === true);
  expect(yap.readConfig().bridgeEnabled).toBe(true);
  await page.getByRole('button', { name: 'Append trailing space' }).click();
  await expectStore(yap, 'config.json', (c) => c.appendTrailingSpace === false);
});

test('an AI app reads meetings through yap.exe mcp, and never dictations', async ({ yap }) => {
  const mcp = startMcp(exe(yap), { YAP_BRIDGE_FILE: yap.bridgeFile });
  try {
    const init = await mcp.initialize();
    expect(init.result.protocolVersion).toBe('2025-06-18');
    expect(init.result.serverInfo).toMatchObject({ name: 'yap', title: 'Yap' });
    expect(await mcp.toolNames()).toEqual([
      'list_meetings',
      'search_meetings',
      'get_meeting',
      'search_notes',
      'get_note',
      'list_folders',
    ]);

    const seen = [];
    const call = async (name, args) => {
      const r = await mcp.tool(name, args);
      seen.push(r.text);
      expect(r.isError, `${name}: ${r.text}`).toBe(false);
      return r.text;
    };

    let text = await call('list_meetings');
    expect(text).toMatch(/^2 meetings in Yap, newest first:/);
    expect(text.indexOf('Offsite logistics')).toBeLessThan(text.indexOf('Q3 planning sync'));
    expect(text).toContain('with Priya, Tom');
    expect(text).toContain('25 min');

    text = await call('search_meetings', { query: 'budget' });
    expect(text).toContain('[0:20] Them: Priya here. I will send the revised budget by Friday. (transcript page 1)');
    expect(text).toMatch(/Them: The catering budget is still the open question\. \(transcript page [2-9]\)/);
    expect(text).toContain('in the summary or notes: - [ ] send the revised budget (due: Friday)');

    text = await call('get_meeting', { id: 1 });
    expect(text).toContain('# Q3 planning sync');
    expect(text).toContain('Attendees: Priya, Tom');
    expect(text).toContain('## AI summary\n## Action plan');
    expect(text).toContain('## Notes typed during the meeting\nVenue shortlist: Lisbon, Porto');
    expect(text).toContain('[25:00] Them: We decided on Lisbon, the numbers work best there.');

    text = await call('get_meeting', { id: 2 });
    const pages = Number(text.match(/## Transcript, page 1 of (\d+)/)[1]);
    expect(pages).toBeGreaterThan(1);
    expect(text).toContain('transcript_page 2');
    text = await call('get_meeting', { id: 2, transcript_page: pages });
    expect(text).toContain(`## Transcript, page ${pages} of ${pages}`);
    expect(text).toContain('Line 699:');
    expect(text).not.toContain('Line 0:');

    text = await call('search_notes', { query: 'oat milk' });
    expect(text).toContain('**Groceries**');
    text = await call('get_note', { id: 3 });
    expect(text).toContain('Oat milk, coffee and bread');
    text = await call('list_folders');
    expect(text).toContain('- Meetings: 2 notes');
    expect(text).toContain('- Personal: 1 note');

    // The dictation is in history.json, but no tool reaches it.
    for (const query of ['zebracorn', 'password hint']) {
      expect(await call('search_notes', { query })).toBe(`No notes mention “${query}”.`);
      expect(await call('search_meetings', { query })).toBe(`No meetings mention “${query}”.`);
    }
    const results = seen.filter((t) => !/^No (notes|meetings) mention/.test(t));
    expect(results.join('\n')).not.toMatch(/zebracorn|password hint/i);
    expect(mcp.notJson).toEqual([]); // stdout carried protocol messages only
  } finally {
    expect(await mcp.close()).toBe(0);
  }
});

test('allowing writes adds create_note, and the saved note shows up in Notes', async ({ yap, main, shot }) => {
  const page = await openMcp(main);
  const writes = page.getByRole('button', { name: 'Let AI apps save notes to Yap' });
  await writes.click();
  await expect(writes).toHaveAttribute('aria-pressed', 'true');
  await expectStore(yap, 'config.json', (c) => c.mcpAllowWrites === true);

  const mcp = startMcp(exe(yap), { YAP_BRIDGE_FILE: yap.bridgeFile });
  let id;
  try {
    await mcp.initialize('2025-11-25');
    expect(await mcp.toolNames()).toContain('create_note');
    const r = await mcp.tool('create_note', {
      title: 'Ideas from Claude',
      content: '- Try the riverside venue\n- Ask Priya about the budget',
      folder: 'Ideas',
    });
    expect(r.isError).toBe(false);
    expect(r.text).toMatch(/^Saved “Ideas from Claude” to Yap \(note id \d+, folder Ideas\)\.$/);
    await expectStore(yap, 'notes.json', (s) =>
      s.notes.some((n) => n.title === 'Ideas from Claude' && n.source === 'mcp' && n.folder === 'Ideas')
    );
    id = yap.readJson('notes.json').notes.find((n) => n.title === 'Ideas from Claude').id;
  } finally {
    await mcp.close();
  }

  // NotesView refreshes on its own (the bridge emits yap-notes-changed).
  await closeSettings(main);
  await openView(main, 'Notes');
  await main.locator('.folder', { hasText: 'Ideas' }).click();
  await main.locator('.items .item', { hasText: 'Ideas from Claude' }).click();
  await expect(main.getByPlaceholder('Untitled Note')).toHaveValue('Ideas from Claude');
  await expect(main.getByPlaceholder('Start writing…')).toHaveValue(/Try the riverside venue/);
  await shot(main, '08-note-saved-by-an-ai-app');

  // Switched off again: the tool is gone and refuses.
  const again = await openMcp(main);
  await again.getByRole('button', { name: 'Let AI apps save notes to Yap' }).click();
  await expectStore(yap, 'config.json', (c) => c.mcpAllowWrites === false);
  const off = startMcp(exe(yap), { YAP_BRIDGE_FILE: yap.bridgeFile });
  try {
    await off.initialize();
    expect(await off.toolNames()).not.toContain('create_note');
    const r = await off.tool('create_note', { title: 'x', content: 'y' });
    expect(r.isError).toBe(true);
    expect(r.text).toContain('switched off in Yap');
  } finally {
    await off.close();
    await yap.invoke('note_delete', { id });
  }
});

test('with Yap closed, the tools ask for Yap', async ({ yap }) => {
  const mcp = startMcp(exe(yap), { YAP_BRIDGE_FILE: path.join(yap.runDir, 'no-yap-here.json') });
  try {
    await mcp.initialize();
    expect(await mcp.toolNames()).toHaveLength(6); // still listed, so the app shows Yap
    const r = await mcp.tool('list_meetings');
    expect(r.isError).toBe(true);
    expect(r.text).toContain('Open Yap to let your AI read your notes');
  } finally {
    expect(await mcp.close()).toBe(0);
  }
});
