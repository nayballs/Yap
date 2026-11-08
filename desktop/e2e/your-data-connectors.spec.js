// Your data with connectors present (Connectors wave 8: desktop/src-tauri/src/integrations/connectors/privacy.rs,
// privacy/reset.rs, export.rs and diagnostics.rs), the preview on:
// - Download your data: connectors/connectors.json lists the connectors, how each is
//   reached (a link without its query), its header's name and each tool's setting, and
//   no vault value (a sign-in's tokens, a client's secret, a header's value) is anywhere
//   in the zip;
// - Report a problem: settings.json counts the connectors (links, programs, signed in,
//   failing), never a name, an address or a header;
// - Delete some of your data → "Connector sign-ins": the browser sign-in ends (its tokens
//   gone from the vault: test mode's connector-secrets.e2e.json), the connectors, their
//   tools' settings, Yap's registration and a pasted header stay;
// - Reset app: the same, listed under what goes, the connectors under what stays.
// Seeded straight into the data folder: a link signed in in the browser (no server is
// ever contacted: it has no revocation endpoint, so Yap ends the sign-in here) and a link
// with a header. Fictional names, made-up secrets.
import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';
import { test as base, expect, openSettings, settingsDialog, closeSettings, expectStore } from './support/fixtures.js';

const NOW = Math.floor(Date.now() / 1000);
const ACCESS = 'at-FAKE-yourdata-access-0001';
const REFRESH = 'rt-FAKE-yourdata-refresh-0002';
const DCR_SECRET = 'cs-FAKE-yourdata-registered-0003';
const HEADER = 'hdr-FAKE-yourdata-header-0004';
const SECRETS = [ACCESS, REFRESH, DCR_SECRET, HEADER];
const VAULT = 'connector-secrets.e2e.json';

const GRANT = {
  grant: { access: ACCESS, refresh: REFRESH, expires: NOW + 3_600, received: NOW, scopes: ['read'], issuer: 'https://auth.example.com', resource: 'https://notes.example.com/mcp', clientId: 'dcr-FAKE-1' },
  registeredSecret: DCR_SECRET,
};

const SEED = {
  'connectors.json': {
    version: 1,
    connectors: [
      {
        id: 'c1',
        slug: 'notes',
        name: 'Allotment notes',
        enabled: true,
        origin: { kind: 'custom' },
        transport: { kind: 'remote', url: 'https://notes.example.com/mcp?workspace=FAKE-ws-7' },
        auth: {
          kind: 'oauth',
          issuer: 'https://auth.example.com',
          resource: 'https://notes.example.com/mcp',
          client: { kind: 'dcr', clientId: 'dcr-FAKE-1' },
          scopes: ['read'],
          signedIn: true,
          registration: { issuer: 'https://auth.example.com', clientId: 'dcr-FAKE-1', authMethod: 'client_secret_post', redirectUris: ['http://127.0.0.1:51439/connectors/callback'], registered: NOW },
        },
        tools: { search: { setting: 'allow', hash: '1a2b' }, create_page: { setting: 'ask', hash: '3c4d' } },
        added: NOW - 600,
      },
      {
        id: 'c2',
        slug: 'tracker',
        name: 'Garden tracker',
        enabled: true,
        origin: { kind: 'custom' },
        transport: { kind: 'remote', url: 'https://tracker.example.com/mcp', headers: [{ name: 'X-Api-Key' }] },
        auth: { kind: 'header' },
        tools: { get_issue: { setting: 'allow', hash: '5e6f' } },
        added: NOW - 300,
      },
    ],
  },
  [VAULT]: {
    'yap-connector-c1-1': JSON.stringify(GRANT),
    'yap-connector-c2-1': JSON.stringify({ headers: { 'X-Api-Key': HEADER } }),
  },
};

const test = base.extend({
  yapOptions: [
    async ({}, use) => {
      await use({ name: 'your-data-connectors', env: { YAP_CONNECTORS: '1' }, data: SEED });
    },
    { scope: 'worker' },
  ],
});

/** The .zip's files by name (stored or deflated). */
function unzip(file) {
  const zip = fs.readFileSync(file);
  const files = {};
  let at = 0;
  while (zip.readUInt32LE(at) === 0x04034b50) {
    const method = zip.readUInt16LE(at + 8);
    const packed = zip.readUInt32LE(at + 18);
    const nameLen = zip.readUInt16LE(at + 26);
    const extraLen = zip.readUInt16LE(at + 28);
    const name = zip.subarray(at + 30, at + 30 + nameLen).toString();
    const start = at + 30 + nameLen + extraLen;
    const body = zip.subarray(start, start + packed);
    files[name] = method === 8 ? zlib.inflateRawSync(body) : body;
    at = start + packed;
  }
  return files;
}

const group = (main) => settingsDialog(main).getByRole('group', { name: 'Your data' });
const vault = (yap) => {
  const p = path.join(yap.dataDir, VAULT);
  return fs.existsSync(p) ? JSON.parse(fs.readFileSync(p, 'utf8')) : {};
};
const secretsOf = (yap, id) => JSON.parse(vault(yap)[`yap-connector-${id}-1`] ?? '{}');

test('Download your data: the connectors and their settings, never a secret', async ({ yap, main }, testInfo) => {
  await openSettings(main, 'Privacy & data');
  const g = group(main);
  await g.scrollIntoViewIfNeeded();
  const file = testInfo.outputPath('Yap data.zip');
  await yap.invoke('e2e_save_path', { path: file });
  await g.getByRole('button', { name: 'Download…' }).click();
  await expect(g.getByRole('heading', { name: /^Saved Yap data\.zip/ })).toBeVisible({ timeout: 30_000 });
  await g.getByRole('button', { name: 'Done' }).click();
  await closeSettings(main);

  const files = unzip(file);
  expect(Object.keys(files)).toContain('connectors/connectors.json');
  const list = JSON.parse(files['connectors/connectors.json'].toString('utf8')).connectors;
  expect(list.map((c) => c.name)).toEqual(['Allotment notes', 'Garden tracker']);
  expect(list[0].reached.address).toBe('https://notes.example.com/mcp');
  expect(list[0].signsIn).toMatchObject({ how: 'in the browser', signedIn: true });
  expect(list[0].tools).toEqual([
    { name: 'create_page', setting: 'ask', kind: null },
    { name: 'search', setting: 'allow', kind: null },
  ]);
  expect(list[1].reached.headerNames).toEqual(['X-Api-Key']);
  expect(files['README.txt'].toString('utf8')).toContain('connectors/connectors.json');
  // No vault value in any file of the zip (unpacked), nor the link's query.
  const all = Object.values(files).map((b) => b.toString('utf8')).join('\n');
  for (const s of [...SECRETS, 'FAKE-ws-7']) expect(all.includes(s), s).toBe(false);
});

test('Report a problem: the connectors are counts, never a name or an address', async ({ yap }) => {
  const preview = await yap.invoke('diagnostics_preview');
  const settings = JSON.parse(preview.settings);
  expect(settings.connectors).toMatchObject({ count: 2, links: 2, programs: 0, signedIn: 1, withTokenOrHeaders: 1, failing: 0 });
  for (const never of ['Allotment notes', 'Garden tracker', 'notes.example.com', 'tracker.example.com', 'X-Api-Key', ...SECRETS]) {
    expect(preview.settings.includes(never), never).toBe(false);
  }
});

test('Delete some of your data: connector sign-ins end, the connectors and their keys stay', async ({ yap, main, shot }) => {
  await openSettings(main, 'Privacy & data');
  const g = group(main);
  await g.scrollIntoViewIfNeeded();
  await g.getByRole('button', { name: 'Delete…', exact: true }).click();
  const p = main.getByRole('dialog', { name: 'Delete some of your data' });
  await expect(p).toBeVisible();
  await expect(p.locator('.amount[data-kind="connectorSignIns"]')).toHaveText('1 sign-in');
  await expect(p).toContainText("Your connectors' sign-ins in the browser, signed out where the service allows it");
  await p.getByRole('checkbox', { name: /^Connector sign-ins/ }).check();
  await shot(main, '01-connector-sign-ins');
  await p.getByRole('button', { name: 'Delete…', exact: true }).click();
  const confirm = main.getByRole('alertdialog');
  await expect(confirm.getByRole('list', { name: 'What goes' })).toHaveText('Connector sign-ins · 1 sign-in');
  await confirm.getByRole('button', { name: 'Delete', exact: true }).click();
  await expect(main.getByRole('status').filter({ hasText: 'Deleted from this PC' })).toContainText('Connector sign-ins');
  await closeSettings(main);

  // The sign-in's tokens are gone; Yap's registration (its secret) and the other's header stay.
  await expect.poll(() => secretsOf(yap, 'c1').grant ?? null).toBe(null);
  expect(secretsOf(yap, 'c1').registeredSecret).toBe(DCR_SECRET);
  expect(secretsOf(yap, 'c2').headers).toEqual({ 'X-Api-Key': HEADER });
  // Both connectors stay, with their tools' settings; the first says signed out.
  await expectStore(yap, 'connectors.json', (s) => s.connectors.length === 2 && !s.connectors[0].auth.signedIn && !s.connectors[0].auth.scopes);
  const c1 = yap.readJson('connectors.json').connectors[0];
  expect(c1.tools).toEqual({ search: { setting: 'allow', hash: '1a2b' }, create_page: { setting: 'ask', hash: '3c4d' } });
  expect(c1.auth.registration.clientId).toBe('dcr-FAKE-1');
  const view = (await yap.invoke('connectors_list')).connectors[0];
  expect(view.status.signIn).toBe('needs-sign-in');
});

test('Reset app: connector sign-ins go, the connectors stay', async ({ yap, main }) => {
  // Signed in again (as another Yap sharing the data folder would have left it).
  const v = vault(yap);
  v['yap-connector-c1-1'] = JSON.stringify(GRANT);
  fs.writeFileSync(path.join(yap.dataDir, VAULT), JSON.stringify(v, null, 2));
  await openSettings(main, 'Privacy & data');
  const g = group(main);
  await g.scrollIntoViewIfNeeded();
  await g.getByRole('button', { name: 'Reset app…' }).click();
  const dialog = main.getByRole('alertdialog');
  await expect(dialog.getByRole('list', { name: 'What goes' })).toContainText('Connector sign-ins · 1 sign-in');
  await expect(dialog.getByRole('list', { name: 'What stays' })).toContainText("Your connectors, their tools' settings and any token you pasted");
  await dialog.getByRole('button', { name: 'Reset app' }).click();
  await expect(main.getByRole('status').filter({ hasText: 'Yap is reset' })).toBeVisible();
  await closeSettings(main);
  await expect.poll(() => secretsOf(yap, 'c1').grant ?? null).toBe(null);
  expect(secretsOf(yap, 'c2').headers).toEqual({ 'X-Api-Key': HEADER });
  await expectStore(yap, 'connectors.json', (s) => s.connectors.map((c) => c.name).join() === 'Allotment notes,Garden tracker');
});
