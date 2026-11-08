// Settings → Connectors → Discover (wave 5b): the curated catalogue, here a
// fake one (YAP_E2E_CONNECTOR_CATALOG) whose entries point at this PC's fakes:
// a server that signs in in the browser (support/fake-mcp-oauth.js), one that
// takes a token (support/fake-mcp.js --http --token), and a program.
// - the cards: who makes each, **Tested with Yap** only where the catalogue
//   has a date, + and ✓ once added, "Coming later" with Why?, the quiet
//   "More coming" line, the search filtering them;
// - + on a sign-in one: its sheet, Continue, the browser (played by the
//   test), Cancel meanwhile, connected, ✓; added from the catalogue (its
//   own address, whatever the page sends), named by it in the receipts;
// - + on a token one: the masked field with its help, the token in no file,
//   the entry's own stricter defaults for its tools;
// - + on a program: + Add's program form filled in, its command shown before
//   it runs.
// Fictional data only; nothing leaves this PC.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, settingsDialog } from './support/fixtures.js';
import { RUNS } from './support/yap.js';
import { startFakeMcpOauth } from './support/fake-mcp-oauth.js';
import { FAKE, NODE, fakeHttp, read, stored, openConnectors, row, opened, nextOpened, approveInBrowser, noFieldHolds } from './support/connectors.js';

const DIR = path.join(RUNS, 'connectors-discover-fakes');
const TOKEN = 'notes-FAKE-c41f09b2d7';

/** The fake catalogue: three entries on this PC's fakes, one card for later. */
const catalogue = (oauthUrl, notesUrl) => ({
  version: 3,
  connectors: [
    {
      id: 'lights-cloud',
      name: 'Lights Cloud',
      publisher: 'Example Home Co',
      blurb: 'The lights in a fictional home, signed in through the browser.',
      logo: null,
      color: '#3d6b35',
      url: oauthUrl,
      signIn: 'oauth',
      tested: '2026-11-01',
      notes: 'A stand-in for the tests.',
      docs: 'https://example.com/lights-cloud',
    },
    {
      id: 'fake-notes',
      name: 'Fake Notes',
      publisher: 'Example Notes Ltd',
      blurb: 'Notes in a fictional workspace, with a pasted token.',
      logo: null,
      url: notesUrl,
      signIn: 'token',
      token: { help: 'Make a token on the Fake Notes settings page, then paste it here.', url: 'https://example.com/tokens' },
      tested: null,
      defaults: { turn_off: 'off' },
    },
    {
      id: 'fake-files',
      name: 'Fake Files',
      publisher: 'Example Tools',
      blurb: 'Files on this PC, through a program.',
      logo: null,
      signIn: 'local',
      command: NODE,
      args: [FAKE],
      needs: 'Node.js',
      tested: null,
    },
  ],
  later: [{ id: 'fake-chat', name: 'Fake Chat', blurb: 'Messages in a fictional chat app.', logo: null, why: 'It lets only the apps in its own marketplace sign in.' }],
});

const test = base.extend({
  oauth: [
    async ({}, use) => {
      const fake = await startFakeMcpOauth();
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  notes: [
    async ({}, use) => {
      const srv = await fakeHttp(DIR, 'notes', ['--token', TOKEN]);
      await use(srv);
      srv.stop();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({ oauth, notes }, use) => {
      fs.mkdirSync(DIR, { recursive: true });
      const file = path.join(DIR, 'connector_catalog.json');
      fs.writeFileSync(file, JSON.stringify(catalogue(oauth.url(), notes.url), null, 2));
      await use({ name: 'connectors-discover', env: { YAP_CONNECTORS: '1', YAP_E2E_CONNECTOR_START_MS: '6000', YAP_E2E_CONNECTOR_CATALOG: file } });
    },
    { scope: 'worker' },
  ],
});

/** Discover's cards. */
async function discover(main) {
  const dialog = await openConnectors(main);
  await dialog.getByRole('tab', { name: 'Discover' }).click();
  const list = dialog.getByRole('list', { name: 'Connectors you can add' });
  await expect(list).toBeVisible();
  return { dialog, list };
}

const card = (list, id) => list.locator(`[data-entry="${id}"]`);

test('Discover: the cards, Tested with Yap only where it is, Coming later, and the search', async ({ main, shot }) => {
  await main.setViewportSize({ width: 1200, height: 820 });
  const { dialog, list } = await discover(main);
  await expect(list.getByRole('listitem')).toHaveCount(3);
  await expect(card(list, 'lights-cloud')).toContainText('by Example Home Co');
  await expect(card(list, 'lights-cloud')).toContainText('Tested with Yap');
  await expect(card(list, 'fake-notes')).not.toContainText('Tested with Yap');
  await expect(card(list, 'fake-files')).not.toContainText('Tested with Yap');
  for (const name of ['Lights Cloud', 'Fake Notes', 'Fake Files']) await expect(list.getByRole('button', { name: `Add ${name}` })).toBeVisible();
  // Coming later: why, on asking.
  const later = dialog.getByRole('list', { name: 'Coming later' });
  await expect(later).toContainText('Fake Chat');
  await expect(later).not.toContainText('marketplace');
  await later.getByRole('button', { name: 'Why?' }).click();
  await expect(later).toContainText('It lets only the apps in its own marketplace sign in.');
  // No registry search yet: said quietly.
  await expect(dialog.getByText(/^More coming\./)).toBeVisible();
  await shot(main, 'discover-fake');

  // The search filters the cards.
  const box = dialog.getByRole('searchbox', { name: 'Search connectors' });
  await box.fill('notes');
  await expect(list.getByRole('listitem')).toHaveCount(1);
  await expect(list).toContainText('Fake Notes');
  await box.fill('example home');
  await expect(list.getByRole('listitem')).toHaveCount(1);
  await expect(list).toContainText('Lights Cloud');
  await box.fill('zzqx');
  await expect(dialog.getByText(/Nothing in Yap's list matches “zzqx”/)).toBeVisible();
  await box.fill('');
  await expect(list.getByRole('listitem')).toHaveCount(3);
});

test('+ on one that signs in: its sheet, the browser, Cancel meanwhile, then connected and ✓', async ({ yap, main, oauth }) => {
  oauth.reset();
  const { dialog, list } = await discover(main);
  await list.getByRole('button', { name: 'Add Lights Cloud' }).click();
  const sheet = main.getByRole('dialog', { name: 'Lights Cloud' });
  await expect(sheet).toContainText('by Example Home Co');
  await expect(sheet).toContainText('Tested with Yap');
  await expect(sheet).toContainText('Lights Cloud will ask you to sign in, in your browser.');
  await expect(sheet).toContainText('Yap keeps the sign-in in Windows Credential Manager');
  const before = (await opened(yap)).length;
  await sheet.getByRole('button', { name: 'Continue to sign in' }).click();
  // The browser opens by itself on the catalogue's server.
  const url = await nextOpened(yap, before);
  expect(url.startsWith(`${oauth.base}/authorize?`)).toBe(true);
  await expect(sheet.getByText(/Finish signing in in your browser/)).toBeVisible();
  await expect(sheet.getByRole('button', { name: 'Cancel', exact: true })).toBeVisible();
  expect(await approveInBrowser(oauth, url)).toContain("You're signed in to Lights Cloud.");
  await expect(sheet.getByText(/^Connected to/)).toBeVisible({ timeout: 20_000 });
  await sheet.getByRole('button', { name: 'Done' }).click();
  await expect(sheet).toBeHidden();
  // ✓ on its card, which opens its page.
  const added = list.getByRole('button', { name: 'Lights Cloud is added: open it' });
  await expect(added).toBeVisible();
  await expect(list.getByRole('button', { name: 'Add Lights Cloud' })).toHaveCount(0);
  await added.click();
  await expect(dialog.getByRole('heading', { level: 1, name: 'Lights Cloud' })).toBeVisible();
  await expect(dialog.getByText('by Example Home Co')).toBeVisible();
  await expect(dialog.getByTestId('connector-status')).toHaveText('Connected · 3 tools');

  // Added from the catalogue, at its own address; named by it in the receipts.
  const c = stored(yap).find((x) => x.origin?.catalog === 'lights-cloud');
  expect(c).toMatchObject({ name: 'Lights Cloud', origin: { kind: 'catalog', catalog: 'lights-cloud', catalogVersion: 3 }, transport: { kind: 'remote', url: oauth.url() } });
  const labels = async () => (await yap.invoke('connections_summary')).groups.map((g) => g.label);
  await expect.poll(labels, { timeout: 15_000 }).toEqual(expect.arrayContaining(['Connector sign-in · Lights Cloud', 'Connector · Lights Cloud']));

  // The page sends only the catalogue's id: an address of its own is ignored.
  const other = await yap.invoke('connector_preview', { spec: { name: 'x', catalog: 'lights-cloud', url: 'https://mcp.example.com/mcp' } });
  expect(other.host).toBe(new URL(oauth.url()).hostname);
  // A catalogue id Yap doesn't have is refused.
  expect(await yap.invoke('connector_add', { spec: { name: 'x', catalog: 'not-in-the-list' } }).then(() => 'added', (e) => String(e?.message ?? e))).toContain("isn't in Yap's list");
});

test('+ on one that takes a token: the masked field and its help, Connect, ✓; the token in no file; its own defaults', async ({ yap, main, notes }) => {
  const { list } = await discover(main);
  await list.getByRole('button', { name: 'Add Fake Notes' }).click();
  const sheet = main.getByRole('dialog', { name: 'Fake Notes' });
  await expect(sheet).toContainText('Fake Notes takes a token you make on its site.');
  const field = sheet.getByLabel('Fake Notes token');
  await expect(field).toHaveAttribute('type', 'password');
  await expect(sheet).toContainText('Make a token on the Fake Notes settings page, then paste it here.');
  await expect(sheet.getByRole('button', { name: 'Make a token on example.com ↗' })).toBeVisible();
  // No token, no connector.
  await sheet.getByRole('button', { name: 'Connect' }).click();
  await expect(sheet.getByRole('alert')).toHaveText('Paste your Fake Notes token first.');
  expect(stored(yap).some((c) => c.origin?.catalog === 'fake-notes')).toBe(false);
  await field.fill(TOKEN);
  await sheet.getByRole('button', { name: 'Connect' }).click();
  await expect(sheet.getByText(/^Connected to fake-mcp/)).toBeVisible({ timeout: 20_000 });
  await noFieldHolds(main, TOKEN);
  await sheet.getByRole('button', { name: 'Done' }).click();
  await expect(list.getByRole('button', { name: 'Fake Notes is added: open it' })).toBeVisible();

  const c = stored(yap).find((x) => x.origin?.catalog === 'fake-notes');
  expect(c.auth.kind).toBe('bearer');
  // The entry's own defaults: stricter than the kind's (Changes → Ask first).
  expect(c.tools.turn_off.setting).toBe('off');
  expect(c.tools.list_lights.setting).toBe('allow');
  for (const f of ['connectors.json', 'connector_tools.json', 'config.json', 'connections.json']) expect(read(yap, f), f).not.toContain(TOKEN);
  expect(notes.log().every((l) => l.auth)).toBe(true);
  await expect.poll(async () => (await yap.invoke('connections_summary')).groups.map((g) => g.label), { timeout: 15_000 }).toContain('Connector · Fake Notes');
});

test('+ on a program: the program form filled in, its command shown before it runs, then ✓', async ({ yap, main }) => {
  const { list } = await discover(main);
  await list.getByRole('button', { name: 'Add Fake Files' }).click();
  const sheet = main.getByRole('dialog', { name: 'Fake Files' });
  await expect(sheet).toContainText('A program on this PC: it needs Node.js.');
  await sheet.getByRole('button', { name: 'Continue' }).click();
  const add = main.getByRole('dialog', { name: 'Add a program on this PC' });
  await expect(add).toBeVisible();
  await expect(add.getByLabel('Name')).toHaveValue('Fake Files');
  await expect(add.getByLabel(/^Command/)).toHaveValue(NODE);
  await expect(add.getByLabel(/^Arguments/)).toHaveValue(new RegExp('fake-mcp\\.js'));
  await add.getByRole('button', { name: 'Review' }).click();
  await expect(add.getByText('This runs a program on your PC with your permissions.')).toBeVisible();
  await expect(add.getByTestId('command-line')).toContainText('fake-mcp.js');
  expect(stored(yap).some((c) => c.origin?.catalog === 'fake-files'), 'nothing added before Approve').toBe(false);
  await add.getByRole('button', { name: 'Approve and run' }).click();
  await expect(add.getByText(/^Connected to/)).toBeVisible({ timeout: 30_000 });
  await add.getByRole('button', { name: 'Done' }).click();
  await expect(list.getByRole('button', { name: 'Fake Files is added: open it' })).toBeVisible();
  expect(stored(yap).find((c) => c.origin?.catalog === 'fake-files').transport.kind).toBe('local');
  await settingsDialog(main).getByRole('tab', { name: 'Yours' }).click();
  await expect(row(main, 'Fake Files')).toContainText('Connected · 9 tools');
});
