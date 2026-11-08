// Settings → Connectors with the connectors preview on (wave 5a,
// desktop/src/lib/settings/connectors/): the page in the Claude app's layout
// (Yours | Discover, a search box, + Add ▾), driven as the person would.
// - a link added with its token (support/fake-mcp.js --http): what it found,
//   its row, its page — the address, its tools grouped, each one's Always
//   allow / Ask first / Off saved, a group's control, no Always allow for a
//   tool that can delete, "Last used" —, the token in no file and no field;
// - a program: its exact command line shown before anything runs, nothing
//   started until Approve and run, its secret variable in no file, Show
//   details' stderr;
// - a program that won't start, then Try again once it's fixed;
// - a link whose server signs in (support/fake-mcp-oauth.js): Sign in, the
//   browser (played by the test), Cancel shown meanwhile, connected; Sign
//   out revoking; Remove asking first;
// - Import from Claude Desktop (a stand-in profile's file, read only; Yap's
//   own entry left out; secrets straight to the vault) and Paste a setup;
// - the page's search entries landing; the page hidden with the switch off;
// - screenshots of Yours, a connector's page, + Add and Discover (the
//   bundled list) in light and dark.
// Fictional data only; nothing leaves this PC.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openSettings, settingsDialog, setConfig, mainOnScreen } from './support/fixtures.js';
import { RUNS, launchYap } from './support/yap.js';
import { startFakeMcpOauth } from './support/fake-mcp-oauth.js';
import { SETTINGS_INDEX } from '../src/lib/settings/settingsIndex.js';
import { FAKE, NODE, fakeHttp, read, stored, openConnectors, addVia, row, opened, nextOpened, approveInBrowser, noFieldHolds } from './support/connectors.js';

const DIR = path.join(RUNS, 'connectors-ui-fakes');
const TOKEN = 'tok-FAKE-51c0d2e9aa';
const SECRET = 'files-FAKE-3b8e71c4';
const IMPORTED = 'imp-FAKE-90d1c5ee';
const LINK_TOKEN = 'lnk-FAKE-77ab02f3';
const FILES = ['connectors.json', 'connector_tools.json', 'config.json', 'connections.json'];

const test = base.extend({
  oauth: [
    async ({}, use) => {
      const fake = await startFakeMcpOauth();
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({}, use) => {
      await use({ name: 'connectors-ui', env: { YAP_CONNECTORS: '1', YAP_E2E_CONNECTOR_START_MS: '6000' } });
    },
    { scope: 'worker' },
  ],
});

const servers = [];
test.afterAll(() => servers.forEach((s) => s.stop()));
const http = async (name, args) => {
  const s = await fakeHttp(DIR, name, args);
  servers.push(s);
  return s;
};

/** Everything Yap logged: its stdout and its log files. */
function logged(yap) {
  let out = fs.existsSync(yap.logPath) ? fs.readFileSync(yap.logPath, 'utf8') : '';
  const dir = path.join(yap.dataDir, 'logs');
  if (fs.existsSync(dir)) for (const f of fs.readdirSync(dir)) out += fs.readFileSync(path.join(dir, f), 'utf8');
  return out;
}

const tool = (dialog, name) => dialog.getByRole('radiogroup', { name: `${name}: when Chat may use it` });

test('Yours: a link added with its token, its tools set one by one and by group, the token in no file', async ({ yap, main, shot }) => {
  await main.setViewportSize({ width: 1200, height: 820 });
  const srv = await http('token', ['--token', TOKEN]);
  const dialog = await openConnectors(main);
  await expect(dialog.getByRole('tab', { name: 'Yours', selected: true })).toBeVisible();
  await expect(dialog.getByRole('tab', { name: 'Discover' })).toBeVisible();
  // Yap's calendars first, then the person's connectors.
  await expect(dialog.getByRole('heading', { name: 'Calendars' })).toBeVisible();
  await expect(dialog.getByRole('group', { name: 'Google Calendar' })).toBeVisible();
  await expect(dialog.getByText('No connectors yet.')).toBeVisible();

  const add = await addVia(main, 'Custom connector…', 'Add a custom connector');
  await add.getByLabel('Name').fill('Home lights');
  await add.getByLabel('Server address').fill(srv.url);
  await add.getByLabel(/^Token/).fill(TOKEN);
  await shot(main, 'add-link');
  await add.getByRole('button', { name: 'Add', exact: true }).click();
  // What it found: the server's own name and its tools.
  await expect(add.getByText('Connected to fake-mcp')).toBeVisible();
  await expect(add.getByRole('list', { name: 'Its tools' })).toContainText('List lights');
  // Sent: the page forgot the token.
  await noFieldHolds(main, TOKEN);
  await add.getByRole('button', { name: 'Done' }).click();
  await expect(add).toBeHidden();
  await expect(row(main, 'Home lights')).toContainText('Connected · 9 tools');
  // Which way Chat's model gets them (connectors_budget), said under the list.
  const budget = await yap.invoke('connectors_budget');
  expect(['all', 'search', 'deferred', 'none']).toContain(budget.mode);
  await expect(dialog.getByTestId('connectors-budget')).toHaveAttribute('data-mode', budget.mode);
  await expect(dialog.getByTestId('connectors-budget')).not.toBeEmpty();
  // "Let connectors work during meetings": off, then on, saved.
  const meetings = dialog.getByRole('button', { name: 'Let connectors work during meetings' });
  await expect(meetings).toHaveAttribute('aria-pressed', 'false');
  await meetings.click();
  await expect.poll(async () => (await yap.invoke('get_config')).connectorsInMeetings).toBe(true);
  await meetings.click();
  await expect.poll(async () => (await yap.invoke('get_config')).connectorsInMeetings).toBe(false);

  // Its page.
  await row(main, 'Home lights').getByRole('button', { name: /^Home lights:/ }).click();
  await expect(dialog.getByRole('heading', { level: 1, name: 'Home lights' })).toBeVisible();
  await expect(dialog.getByTestId('connector-status')).toHaveText('Connected · 9 tools');
  await expect(dialog.getByTestId('connector-address')).toHaveText(srv.url);
  await expect(dialog.getByText('Sends a token (kept in Windows Credential Manager), only to this server.')).toBeVisible();
  for (const g of ['Reads', 'Changes things', 'Can delete']) await expect(dialog.getByRole('heading', { level: 3, name: new RegExp(`^${g}`) })).toBeVisible();
  const id = stored(yap).find((c) => c.name === 'Home lights').id;
  const setting = (t) => stored(yap).find((c) => c.id === id).tools[t].setting;

  // One tool's setting, saved.
  await tool(dialog, 'turn_off').getByRole('radio', { name: 'Always allow' }).click();
  await expect(tool(dialog, 'turn_off').getByRole('radio', { name: 'Always allow' })).toHaveAttribute('aria-checked', 'true');
  await expect.poll(() => setting('turn_off')).toBe('allow');
  await tool(dialog, 'List lights').getByRole('radio', { name: 'Off' }).click();
  await expect.poll(() => setting('list_lights')).toBe('off');
  // A tool that can delete never offers Always allow, nor does its group.
  await expect(tool(dialog, 'delete_all').getByRole('radio')).toHaveText(['Ask first', 'Off']);
  await expect(dialog.getByRole('radiogroup', { name: 'All of Can delete' }).getByRole('radio')).toHaveText(['Ask first', 'Off']);
  // A group's control: every read Yap offers as it is, at once (the one
  // whose description reads like orders, and the one it can't offer, stay Off).
  await dialog.getByRole('radiogroup', { name: 'All of Reads' }).getByRole('radio', { name: 'Ask first' }).click();
  await expect.poll(() => ['list_lights', 'slow', 'crash', 'big_schema', 'rug_pull', 'picture'].map(setting)).toEqual(Array(6).fill('ask'));
  expect(setting('inject')).toBe('off');
  expect(setting('needs_input')).toBe('off');
  await expect(tool(dialog, 'needs_input').getByRole('radio')).toHaveText(['Off']);
  await expect(dialog.locator('[data-tool="inject"]')).toContainText('Its description reads like instructions to the model');

  // Where Yap uses it: Chat, the Voice Assistant, meetings, and the phone (its reads only, wave 6: off by default).
  await dialog.getByRole('button', { name: 'Use it with the Voice Assistant' }).click();
  await expect.poll(() => stored(yap).find((c) => c.id === id).where).toMatchObject({ chat: true, voice: false, meetings: false, phone: false });
  await dialog.getByRole('button', { name: 'Use it during meetings' }).click();
  await expect.poll(() => stored(yap).find((c) => c.id === id).where.meetings).toBe(true);
  const phone = dialog.getByRole('button', { name: 'Use it from your phone' });
  await expect(phone).toBeEnabled();
  await expect(dialog.locator('[data-setting="connector-where"]')).toContainText('Its reads set to Always allow, when you ask from your paired phone.');
  await phone.click();
  await expect.poll(() => stored(yap).find((c) => c.id === id).where.phone).toBe(true);
  await phone.click();
  await expect.poll(() => stored(yap).find((c) => c.id === id).where.phone).toBe(false);

  // "Last used", from a call.
  await expect(dialog.getByTestId('connector-used')).toContainText('Not used yet');
  await yap.invoke('connector_call_tool', { id, tool: 'turn_off', args: { area: 'hall' } });
  await expect(dialog.getByTestId('connector-used')).toContainText('Last used just now');
  await dialog.getByRole('button', { name: 'Back to Connectors' }).scrollIntoViewIfNeeded();
  await shot(main, 'connector-page');
  await setConfig(yap, main, { theme: 'dark' });
  await shot(main, 'connector-page-dark');
  await setConfig(yap, main, { theme: 'light' });

  // The token is in no file Yap keeps (only the test run's stand-in vault), nor the log.
  for (const f of FILES) expect(read(yap, f), f).not.toContain(TOKEN);
  expect(read(yap, 'connector-secrets.e2e.json')).toContain(TOKEN);
  expect(logged(yap)).not.toContain(TOKEN);
  await noFieldHolds(main, TOKEN);
  await dialog.getByRole('button', { name: 'Back to Connectors' }).click();
  await expect(dialog.getByRole('heading', { level: 1, name: 'Connectors' })).toBeVisible();
});

test('a program: its exact command shown before anything runs, nothing started until Approve and run', async ({ yap, main, shot }) => {
  await main.setViewportSize({ width: 1200, height: 820 });
  fs.mkdirSync(DIR, { recursive: true });
  const log = path.join(DIR, 'program.log');
  fs.rmSync(log, { force: true });
  const dialog = await openConnectors(main);
  const add = await addVia(main, 'Program on this PC…', 'Add a program on this PC');
  await add.getByLabel('Name').fill('Fake files');
  await add.getByLabel(/^Command/).fill(NODE);
  await add.getByLabel(/^Arguments/).fill(`"${FAKE}" --log "${log}" --stderr`);
  await add.getByRole('button', { name: '+ Add a variable' }).click();
  await add.getByLabel('Variable name').fill('FILES_TOKEN');
  await add.getByLabel('Value of FILES_TOKEN').fill(SECRET);
  await add.getByRole('button', { name: 'Review' }).click();

  // The consent panel: the whole command line, resolved, before it runs.
  await expect(add.getByText('This runs a program on your PC with your permissions.')).toBeVisible();
  const line = add.getByTestId('command-line');
  await expect(line).toContainText('fake-mcp.js');
  await expect(line).toContainText('--stderr');
  await expect(add.getByText('🔒 kept in Credential Manager')).toBeVisible();
  await expect(add.getByText(SECRET)).toHaveCount(0);
  expect(fs.existsSync(log), 'nothing ran before Approve').toBe(false);
  await shot(main, 'add-program');
  await setConfig(yap, main, { theme: 'dark' });
  await shot(main, 'add-program-dark');
  await setConfig(yap, main, { theme: 'light' });

  await add.getByRole('button', { name: 'Approve and run' }).click();
  await expect(add.getByText(/^Connected to/)).toBeVisible({ timeout: 30_000 });
  expect(fs.existsSync(log), 'it ran once approved').toBe(true);
  await noFieldHolds(main, SECRET);
  await add.getByRole('button', { name: 'Open its page' }).click();
  await expect(add).toBeHidden();
  await expect(dialog.getByRole('heading', { level: 1, name: 'Fake files' })).toBeVisible();
  await expect(dialog.getByTestId('connector-command')).toContainText('fake-mcp.js');
  await expect(dialog.getByText('FILES_TOKEN')).toBeVisible();
  await dialog.getByText('Show details').click();
  await expect(dialog.getByTestId('connector-stderr')).toContainText('fake-mcp: listening on stdio');

  const c = stored(yap).find((x) => x.name === 'Fake files');
  expect(c.transport.env).toEqual([{ name: 'FILES_TOKEN', secret: true }]);
  for (const f of FILES) expect(read(yap, f), f).not.toContain(SECRET);
  expect(logged(yap)).not.toContain(SECRET);
});

test("a program that won't start says why, and Try again brings it back once it's fixed", async ({ yap, main }) => {
  fs.mkdirSync(DIR, { recursive: true });
  const broken = path.join(DIR, 'broken.flag');
  fs.writeFileSync(broken, '1');
  const spec = { name: 'Flaky lights', command: NODE, args: [FAKE, '--fail-if', broken] };
  const p = await yap.invoke('connector_preview', { spec });
  const v = await yap.invoke('connector_add', { spec: { ...spec, approved: p.program.fingerprint } });
  expect(v.status.state).toBe('failed');
  await openConnectors(main);
  const r = row(main, 'Flaky lights');
  await expect(r).toContainText("Couldn't start");
  await expect(r).toContainText('stopped before it answered');
  // Still broken: it says so again.
  await r.getByRole('button', { name: 'Try again' }).click();
  await expect(r.getByRole('button', { name: 'Try again' })).toBeEnabled();
  await expect(r).toContainText("Couldn't start");
  // Fixed: back.
  fs.rmSync(broken);
  await r.getByRole('button', { name: 'Try again' }).click();
  await expect(r).toContainText('Connected · 9 tools', { timeout: 30_000 });
  await expect(r.getByRole('button', { name: 'Try again' })).toHaveCount(0);
  // Off with its switch, on again.
  await r.getByRole('button', { name: 'Use Flaky lights' }).click();
  await expect(r).toContainText('Off');
  await expect.poll(() => stored(yap).find((c) => c.name === 'Flaky lights').enabled).toBe(false);
  await r.getByRole('button', { name: 'Use Flaky lights' }).click();
  await expect.poll(() => stored(yap).find((c) => c.name === 'Flaky lights').enabled).toBe(true);
});

test('a server that signs in: the 401, Sign in through the browser, Cancel meanwhile, then Sign out and Remove asking first', async ({ yap, main, oauth }) => {
  oauth.reset();
  const dialog = await openConnectors(main);
  const add = await addVia(main, 'Custom connector…', 'Add a custom connector');
  await add.getByLabel('Name').fill('Lights');
  await add.getByLabel('Server address').fill(oauth.url());
  await add.getByRole('button', { name: 'Add', exact: true }).click();
  // The server answered 401 with a challenge: a sign-in, never a token error.
  await expect(add.getByText(/asks you to sign in, in your browser/)).toBeVisible();
  const before = (await opened(yap)).length;
  await add.getByRole('button', { name: 'Sign in', exact: true }).click();
  const url = await nextOpened(yap, before);
  expect(url.startsWith(`${oauth.base}/authorize?`)).toBe(true);
  await expect(add.getByText(/Finish signing in in your browser/)).toBeVisible();
  await expect(add.getByRole('button', { name: 'Cancel', exact: true })).toBeVisible();
  const page = await approveInBrowser(oauth, url);
  expect(page).toContain("You're signed in to Lights.");
  await expect(add.getByText(/^Connected to/)).toBeVisible({ timeout: 20_000 });
  await add.getByRole('button', { name: 'Open its page' }).click();
  await expect(dialog.getByTestId('connector-status')).toHaveText('Connected · 3 tools');
  await expect(dialog.getByText(/^Signed in at 127\.0\.0\.1/)).toBeVisible();
  // Its own client ID, tucked under Advanced.
  await expect(dialog.getByText('Advanced: your own client ID')).toBeVisible();
  await expect(dialog.getByLabel('Your own client ID')).toBeHidden();

  // Sign out: revoked at the server, "Needs sign-in".
  await dialog.getByRole('button', { name: 'Sign out' }).click();
  await expect(dialog.getByTestId('connector-status')).toContainText('Needs sign-in');
  expect(oauth.log.some((l) => l.revoked)).toBe(true);
  await expect(dialog.getByRole('button', { name: 'Sign in', exact: true })).toBeVisible();

  // A sign-in started and cancelled from its page.
  const before2 = (await opened(yap)).length;
  await dialog.getByRole('button', { name: 'Sign in', exact: true }).click();
  await nextOpened(yap, before2);
  await expect(dialog.getByTestId('connector-status')).toHaveText('Signing in… Finish in your browser.');
  await dialog.getByRole('button', { name: 'Cancel', exact: true }).click();
  await expect(dialog.getByTestId('connector-status')).toContainText('Needs sign-in');

  // Remove asks first; Cancel keeps it.
  await dialog.getByRole('button', { name: 'Remove…' }).click();
  const confirm = main.getByRole('alertdialog', { name: 'Remove Lights?' });
  await expect(confirm).toContainText('You can add it again at any time.');
  await confirm.getByRole('button', { name: 'Cancel' }).click();
  await expect(confirm).toBeHidden();
  expect(stored(yap).some((c) => c.name === 'Lights')).toBe(true);
  await dialog.getByRole('button', { name: 'Remove…' }).click();
  await confirm.getByRole('button', { name: 'Remove', exact: true }).click();
  await expect(dialog.getByRole('heading', { level: 1, name: 'Connectors' })).toBeVisible();
  await expect.poll(() => stored(yap).some((c) => c.name === 'Lights')).toBe(false);
  await expect(row(main, 'Lights')).toHaveCount(0);
});

test("Import from Claude Desktop: each server shown whole, Yap's own left out, its file untouched, secrets to the vault; and Paste a setup", async ({ yap, main, shot }) => {
  await main.setViewportSize({ width: 1200, height: 820 });
  fs.mkdirSync(DIR, { recursive: true });
  const link = await http('imported', ['--token', LINK_TOKEN]);
  const importLog = path.join(DIR, 'imported.log');
  fs.rmSync(importLog, { force: true });
  const claudeDir = path.join(yap.runDir, 'ai-apps', 'AppData', 'Roaming', 'Claude');
  fs.mkdirSync(claudeDir, { recursive: true });
  const claudeFile = path.join(claudeDir, 'claude_desktop_config.json');
  const original = JSON.stringify(
    {
      mcpServers: {
        'fake-files': { command: NODE, args: [FAKE, '--log', importLog], env: { FAKE_API_KEY: IMPORTED, LOG_LEVEL: 'info' } },
        'fake-link': { type: 'http', url: link.url, headers: { Authorization: `Bearer ${LINK_TOKEN}` } },
        yap: { command: 'C:\\Program Files\\Yap\\yap.exe', args: ['mcp'] },
      },
      globalShortcut: '',
    },
    null,
    2
  );
  fs.writeFileSync(claudeFile, original);

  await openConnectors(main);
  const imp = await addVia(main, 'Import from Claude Desktop…', 'Import from Claude Desktop');
  const found = imp.getByRole('list', { name: 'Servers found' });
  await expect(found.getByRole('checkbox')).toHaveCount(2);
  await expect(found).toContainText('fake-files');
  await expect(found).toContainText('fake-link');
  await expect(imp.getByText("One entry left out: Yap's own, or not a server Yap can add.")).toBeVisible();
  await expect(imp.getByTestId('command-line')).toContainText('fake-mcp.js');
  await expect(found).toContainText('FAKE_API_KEY');
  await expect(found).toContainText('LOG_LEVEL');
  await expect(found).toContainText('Sends a token: kept in Windows Credential Manager.');
  // No secret on the page, and nothing ran.
  await noFieldHolds(main, IMPORTED);
  await noFieldHolds(main, LINK_TOKEN);
  expect(fs.existsSync(importLog)).toBe(false);
  await shot(main, 'import');
  await imp.getByRole('button', { name: 'Approve and add 2' }).click();
  await expect(imp.getByText(/^Connected to/)).toHaveCount(2, { timeout: 30_000 });
  await imp.getByRole('button', { name: 'Done' }).click();

  // Claude Desktop's file is as it was; the secrets went to the vault, not Yap's files.
  expect(fs.readFileSync(claudeFile, 'utf8')).toBe(original);
  const mine = stored(yap).filter((c) => c.origin?.kind === 'import');
  expect(mine.map((c) => [c.name, c.origin.from]).sort()).toEqual([
    ['fake-files', 'claude-desktop'],
    ['fake-link', 'claude-desktop'],
  ]);
  expect(mine.find((c) => c.name === 'fake-link').auth.kind).toBe('bearer');
  for (const f of FILES) {
    expect(read(yap, f), f).not.toContain(IMPORTED);
    expect(read(yap, f), f).not.toContain(LINK_TOKEN);
  }
  const vault = read(yap, 'connector-secrets.e2e.json');
  expect(vault).toContain(IMPORTED);
  expect(vault).toContain(LINK_TOKEN);
  expect(link.log().every((l) => l.auth)).toBe(true);

  // Paste a setup: read, shown, approved.
  const paste = await addVia(main, 'Paste a setup…', 'Paste a setup');
  const setup = JSON.stringify({ mcpServers: { 'pasted-files': { command: NODE, args: [FAKE] } } });
  await paste.getByLabel(/^Your setup/).fill(setup);
  await paste.getByRole('button', { name: 'Read it' }).click();
  await expect(paste.getByRole('list', { name: 'Servers found' }).getByRole('checkbox')).toHaveCount(1);
  await expect(paste.getByTestId('command-line')).toContainText('fake-mcp.js');
  await expect(paste.getByText('The program you tick will run on your PC with your permissions.')).toBeVisible();
  await paste.getByRole('button', { name: 'Approve and add 1' }).click();
  await expect(paste.getByText(/^Connected to/)).toBeVisible({ timeout: 30_000 });
  await paste.getByRole('button', { name: 'Done' }).click();
  await expect(row(main, 'pasted-files')).toContainText('Connected · 9 tools');
  // The same again: already added, not ticked.
  const again = await addVia(main, 'Paste a setup…', 'Paste a setup');
  await again.getByLabel(/^Your setup/).fill(setup);
  await again.getByRole('button', { name: 'Read it' }).click();
  await expect(again.getByText('Added already')).toBeVisible();
  await expect(again.getByRole('checkbox', { name: 'Add pasted-files' })).not.toBeChecked();
  await again.getByRole('button', { name: 'Close' }).click();
});

test("the page's search entries land on their rows, and the search finds them", async ({ main }) => {
  await main.setViewportSize({ width: 1200, height: 820 });
  const dialog = await openSettings(main, 'General');
  const missing = [];
  for (const e of SETTINGS_INDEX.filter((x) => x.page === 'connectors')) {
    await main.evaluate((t) => window.dispatchEvent(new CustomEvent('yap-settings-goto', { detail: t })), { page: e.page, anchor: e.anchor });
    try {
      await expect(dialog.locator(`[data-setting="${e.anchor}"], #settings-${e.anchor}`).first()).toBeAttached({ timeout: 5_000 });
    } catch {
      missing.push(`${e.label} (${e.anchor})`);
    }
  }
  expect(missing, 'entries with no row').toEqual([]);
  const box = dialog.getByRole('searchbox', { name: 'Search settings' });
  await box.fill('import claude desktop');
  await expect(dialog.getByRole('listbox', { name: 'Settings found' })).toContainText('Import from Claude Desktop');
  await box.fill('');
});

test('screenshots: Yours and Discover (the bundled list), light and dark', async ({ yap, main, shot, oauth }) => {
  await main.setViewportSize({ width: 1200, height: 820 });
  // Yours with one of each: connected with a token, waiting for a sign-in, a
  // program that won't start, one turned off.
  for (const c of (await yap.invoke('connectors_list')).connectors) await yap.invoke('connector_remove', { id: c.id });
  oauth.reset();
  const notes = await http('shots', ['--token', TOKEN]);
  await yap.invoke('connector_add', { spec: { name: 'Team notes', url: notes.url, token: TOKEN } });
  await yap.invoke('connector_add', { spec: { name: 'Garden sensors', url: oauth.url() } });
  const broken = path.join(DIR, 'shots-broken.flag');
  fs.writeFileSync(broken, '1');
  const spec = { name: 'Recipe files', command: NODE, args: [FAKE, '--fail-if', broken] };
  const p = await yap.invoke('connector_preview', { spec });
  await yap.invoke('connector_add', { spec: { ...spec, approved: p.program.fingerprint } });
  const off = await yap.invoke('connector_add', { spec: { name: 'Old wiki', url: notes.url, token: TOKEN } });
  await yap.invoke('connector_set_enabled', { id: off.id, enabled: false });
  const dialog = await openConnectors(main);
  await expect(row(main, 'Team notes')).toContainText('Connected · 9 tools');
  await expect(row(main, 'Garden sensors').getByRole('button', { name: 'Sign in', exact: true })).toBeVisible();
  await expect(row(main, 'Recipe files').getByRole('button', { name: 'Try again' })).toBeVisible();
  await expect(row(main, 'Old wiki')).toContainText('Off');
  await expect(dialog.getByRole('list', { name: 'Your connectors' })).toBeVisible();
  await shot(main, 'yours');
  await setConfig(yap, main, { theme: 'dark' });
  await shot(main, 'yours-dark');
  await setConfig(yap, main, { theme: 'light' });

  await dialog.getByRole('tab', { name: 'Discover' }).click();
  const list = dialog.getByRole('list', { name: 'Connectors you can add' });
  // The bundled list: the official servers Yap can use today; none claims a review.
  for (const name of ['Notion', 'Linear', 'GitHub', 'Atlassian', 'Home Assistant']) await expect(list).toContainText(name);
  await expect(list.getByText('Tested with Yap')).toHaveCount(0);
  await expect(dialog.getByRole('list', { name: 'Coming later' })).toContainText('Slack');
  await expect(dialog.getByText(/^More coming\./)).toBeVisible();
  await shot(main, 'discover');
  await setConfig(yap, main, { theme: 'dark' });
  await shot(main, 'discover-dark');
  await setConfig(yap, main, { theme: 'light' });
  // A card's sheet: who makes it, how it signs in, where the data goes.
  await list.getByRole('button', { name: 'Add Notion' }).click();
  const sheet = main.getByRole('dialog', { name: 'Notion' });
  await expect(sheet).toContainText('by Notion Labs');
  await expect(sheet).toContainText('Notion will ask you to sign in, in your browser.');
  await expect(sheet).toContainText('What Notion returns goes to the model you chat with.');
  await expect(sheet.getByRole('button', { name: 'Continue to sign in' })).toBeVisible();
  await shot(main, 'discover-sheet');
  await sheet.getByRole('button', { name: 'Cancel' }).click();
  await expect(sheet).toBeHidden();
  // Nothing was added or contacted.
  expect(stored(yap).some((c) => c.origin?.catalog === 'notion')).toBe(false);
});

test('with the preview switched off, Connectors is the calendars page alone', async () => {
  const off = await launchYap({ name: 'connectors-ui-off' });
  try {
    await mainOnScreen(off);
    const dialog = await openSettings(off.main, 'Connectors');
    await expect(dialog.getByRole('group', { name: 'Google Calendar' })).toBeVisible();
    await expect(dialog.getByText('A calendar gives you a reminder before each meeting')).toBeVisible();
    await expect(dialog.getByRole('tab', { name: 'Discover' })).toHaveCount(0);
    await expect(dialog.getByRole('button', { name: 'Add', exact: true })).toHaveCount(0);
    // Its search entries aren't found either.
    const box = dialog.getByRole('searchbox', { name: 'Search settings' });
    await box.fill('discover connectors');
    await expect(dialog.getByRole('listbox', { name: 'Settings found' })).not.toContainText('Discover connectors');
    await box.fill('');
    expect(fs.existsSync(path.join(off.dataDir, 'connectors.json'))).toBe(false);
    // "Connectors (preview)" (wave 6): shown, off by default; on, the connectors' page; off again, the calendars'.
    const preview = dialog.getByRole('button', { name: 'Connectors (preview)' });
    await expect(preview).toBeVisible();
    await expect(preview).toHaveAttribute('aria-pressed', 'false');
    await preview.click();
    await expect.poll(() => off.readConfig()?.connectorsPreview).toBe(true);
    await expect(dialog.getByRole('tab', { name: 'Discover' })).toBeVisible({ timeout: 10_000 });
    await dialog.getByRole('button', { name: 'Connectors (preview)' }).click();
    await expect.poll(() => off.readConfig()?.connectorsPreview).toBe(false);
    await expect(dialog.getByRole('tab', { name: 'Discover' })).toHaveCount(0, { timeout: 10_000 });
  } finally {
    await off.stop();
  }
});
