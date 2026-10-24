// Network receipts and Report a problem:
// - Settings → Privacy & data → Network connections (src/lib/settings/
//   ConnectionsView.svelte, src-tauri/src/net.rs): a Chat answer from "Groq"
//   — the fake AI standing in for api.groq.com through the debug-only
//   YAP_E2E_NET_ALIASES — shows as "Chat · Groq" to api.groq.com, on the
//   Internet; one from a server on this PC as "Chat · Custom endpoint" at
//   127.0.0.1, This PC. The filter shows each alone; Privacy & data's row
//   counts today's; a link (`privacy#connections-list`) opens the list;
//   light and dark. connections.json holds hosts and labels only: no key,
//   no path, nothing asked or answered. Clear (asked first) empties it.
// - Settings → About → Report a problem (ReportProblem.svelte,
//   diagnostics.rs): the file list and a preview of the settings summary,
//   the system lines and the connections before anything is saved; the
//   summary says a Groq key is set but never shows it, nor About you.
//   Save writes the file where the test says (Windows' Save dialog in real
//   use, `e2e_save_path` here); no key, name or note is in any file of it.
//   "Open a GitHub issue" opens the new-issue page with Yap's version only.
//
// Nothing leaves the PC: both AI servers are local fakes (support/
// fake-llm.js). Fictional people and fake keys only.
import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';
import { test as base, expect, openView, openSettings, closeSettings, expectStore, setConfig } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';

const GROQ_KEY = 'gsk_FAKEreceipts0123456789abcXYZ';
const NAME = 'Robin Example';
const NOTE = 'Grows leeks on Example Lane';

const hostPort = (base) => new URL(base).host;

const test = base.extend({
  fakeCloud: [
    async ({}, use) => {
      const fake = await startFakeLlm();
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  fakeLocal: [
    async ({}, use) => {
      const fake = await startFakeLlm();
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({ fakeCloud }, use) => {
      await use({
        name: 'receipts',
        env: { YAP_E2E_NET_ALIASES: `${hostPort(fakeCloud.base)}=api.groq.com` },
        config: {
          ppApiKeys: { groq: GROQ_KEY },
          aboutName: NAME,
          aboutMe: NOTE,
          llmScopes: { chat: groqChat(fakeCloud) },
        },
      });
    },
    { scope: 'worker' },
  ],
});

function chatScope(fields) {
  return { enabled: true, apiKey: '', apiKeys: {}, prompt: '', disableThinking: false, ...fields };
}
function groqChat(fake) {
  return chatScope({ provider: 'groq', baseUrl: fake.base, model: 'openai/gpt-oss-20b', apiKey: GROQ_KEY });
}
function localChat(fake) {
  return chatScope({ provider: 'custom', baseUrl: fake.base, model: 'fake-chat' });
}

async function ask(main, text) {
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
  await main.getByRole('textbox', { name: 'Message' }).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
}

/** Open a Settings page by a link, as the app's own links do. */
const goto = (main, target) =>
  main.evaluate((t) => window.dispatchEvent(new CustomEvent('yap-settings-goto', { detail: t })), target);

async function pickTheme(main, yap, label) {
  const dialog = await openSettings(main, 'General');
  const group = dialog.getByRole('group', { name: 'Appearance' });
  await group.getByRole('tablist', { name: 'Theme' }).getByRole('tab', { name: label, exact: true }).click();
  await expectStore(yap, 'config.json', (c) => c.theme === label.toLowerCase(), `the theme ${label} to be saved`);
}

/** The .zip's files by name, inflated (local headers carry the sizes). */
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
    files[name] = (method === 8 ? zlib.inflateRawSync(body) : body).toString();
    at = start + packed;
  }
  return files;
}

test('Network connections: a cloud call and one on this PC, by purpose and host; the filter; Clear', async ({
  yap,
  main,
  fakeCloud,
  fakeLocal,
  shot,
}) => {
  await yap.invoke('connections_clear');

  // A question to "Groq" (the fake standing in for api.groq.com)…
  fakeCloud.scriptChat([{ content: 'Leeks like rich, damp soil.' }]);
  await ask(main, 'What soil do leeks like?');
  await expect(main.locator('.msgs')).toContainText('Leeks like rich, damp soil.');
  // …and one to a server on this PC.
  const cfg = await yap.invoke('get_config');
  await setConfig(yap, main, { llmScopes: { ...cfg.llmScopes, chat: localChat(fakeLocal) } });
  fakeLocal.scriptChat([{ content: 'Plant them in late spring.' }]);
  await ask(main, 'When do I plant leeks?');
  await expect(main.locator('.msgs')).toContainText('Plant them in late spring.');

  // Privacy & data counts today's.
  const dialog = await openSettings(main, 'Privacy & data');
  const row = dialog.locator('[data-setting="connections"]');
  await expect(row).toContainText(/\d+ today/);
  await expect(row).toContainText(/left this PC today/);
  await row.click();
  await expect(dialog.getByRole('heading', { name: 'Network connections', level: 1 })).toBeVisible();
  await expect(dialog.locator('#connections-explain')).toContainText('What was in it is never kept');

  const list = dialog.getByRole('list', { name: 'Connections by purpose' });
  const cloud = list.getByRole('button', { name: /^Chat · Groq/ });
  const local = list.getByRole('button', { name: /^Chat · Custom endpoint/ });
  await expect(cloud).toContainText('Internet');
  await expect(cloud).toContainText(/\d+ requests? · [\d.]+ KB sent · [\d.]+ KB received/);
  await expect(local).toContainText('This PC');
  await cloud.click();
  await expect(cloud).toHaveAttribute('aria-expanded', 'true');
  await expect(dialog.getByRole('list', { name: 'Chat · Groq: hosts' })).toContainText('api.groq.com');
  await expect(dialog.getByRole('list', { name: 'Chat · Groq: hosts' })).toContainText('for Chat');
  await local.click();
  await expect(dialog.getByRole('list', { name: 'Chat · Custom endpoint: hosts' })).toContainText('127.0.0.1');
  await shot(main, '01-connections-light');

  // The filter: each alone.
  const tabs = dialog.getByRole('tablist', { name: 'Show' });
  await tabs.getByRole('tab', { name: /^On this PC/ }).click();
  await expect(local).toBeVisible();
  await expect(cloud).toHaveCount(0);
  await tabs.getByRole('tab', { name: /^Left this PC/ }).click();
  await expect(cloud).toBeVisible();
  await expect(local).toHaveCount(0);
  await tabs.getByRole('tab', { name: /^All/ }).click();

  // Dark, through a link straight to the list.
  await pickTheme(main, yap, 'Dark');
  await goto(main, 'privacy#connections-list');
  await expect(dialog.getByRole('heading', { name: 'Network connections', level: 1 })).toBeVisible();
  await list.getByRole('button', { name: /^Chat · Groq/ }).click();
  await shot(main, '02-connections-dark');
  await pickTheme(main, yap, 'System');

  // On disk: hosts and labels, never a key, a path or the words.
  await expectStore(yap, 'connections.json', (s) => s.entries.some((e) => e.host === 'api.groq.com' && e.place === 'internet'));
  await expectStore(yap, 'connections.json', (s) => s.entries.some((e) => e.label === 'Chat · Custom endpoint' && e.place === 'pc'));
  const stored = fs.readFileSync(path.join(yap.dataDir, 'connections.json'), 'utf8');
  for (const leak of [GROQ_KEY, '/v1', 'chat/completions', 'leeks', 'Leeks', 'spring']) expect(stored).not.toContain(leak);

  // Clear: asked first.
  await goto(main, 'privacy#connections-list');
  await dialog.getByRole('button', { name: 'Clear list…' }).click();
  const confirm = main.getByRole('alertdialog');
  await expect(confirm).toContainText('Clear the list of connections?');
  await confirm.getByRole('button', { name: 'Clear list' }).click();
  await expect(list.getByRole('button', { name: /^Chat · / })).toHaveCount(0);
  await expectStore(yap, 'connections.json', (s) => !s.entries.some((e) => e.label.startsWith('Chat · ')));
  await shot(main, '03-cleared');
  await dialog.locator('.page-back').click();
  await expect(dialog.getByRole('heading', { name: 'Privacy & data', level: 1 })).toBeVisible();
  await closeSettings(main);
});

test('Report a problem: see what is in the file first, then save it; no key, name or note in it', async ({ yap, main, shot }) => {
  const dialog = await openSettings(main, 'About');
  const group = dialog.locator('#settings-report');
  await group.getByRole('button', { name: 'Report a problem…' }).click();
  const panel = group.getByRole('region', { name: "What's in the file" });
  const files = panel.getByRole('list', { name: 'Files in the diagnostic file' });
  for (const name of ['README.txt', 'system.txt', 'settings.json', 'connections.txt', 'errors.txt']) await expect(files).toContainText(name);
  await expect(files).toContainText(/logs\/yap\.log\.[\d-]+\.txt/);

  // The settings summary: what's set, never what it is.
  const peek = panel.getByLabel(/^Preview: /);
  await expect(peek).toContainText(/"ppApiKeys": \[\s*"groq"\s*\]/);
  await expect(peek).toContainText('"aboutName": "set"');
  await expect(peek).toContainText('"hotkey"');
  await expect(peek).not.toContainText(GROQ_KEY);
  await expect(peek).not.toContainText(NAME);
  await expect(peek).not.toContainText('Example Lane');
  await shot(main, '04-report-preview');
  const tabs = panel.getByRole('tablist', { name: 'Preview' });
  await tabs.getByRole('tab', { name: 'System' }).click();
  await expect(peek).toContainText(/Yap: [\d.]+.*dev build/);
  await expect(peek).toContainText('Windows:');
  await tabs.getByRole('tab', { name: 'Connections' }).click();
  await expect(peek).toContainText('Network connections Yap made in the last 7 days');

  // Save: where the test says, in place of Windows' Save dialog.
  const file = path.join(yap.runDir, 'diagnostics.zip');
  fs.rmSync(file, { force: true });
  await yap.invoke('e2e_save_path', { path: file });
  await panel.getByRole('button', { name: 'Save…' }).click();
  const saved = group.getByRole('region', { name: /^Saved / });
  await expect(saved).toContainText('Nothing was uploaded');
  await shot(main, '05-report-saved');
  const zipped = unzip(file);
  expect(Object.keys(zipped).slice(0, 5)).toEqual(['README.txt', 'system.txt', 'settings.json', 'connections.txt', 'errors.txt']);
  expect(Object.keys(zipped).some((n) => n.startsWith('logs/'))).toBe(true);
  const all = Object.values(zipped).join('\n');
  for (const leak of [GROQ_KEY, NAME, 'Example Lane', 'leeks']) expect(all).not.toContain(leak);
  expect(JSON.parse(zipped['settings.json']).aboutMe).toBe('set');
  // The logs: info and above only.
  const log = Object.entries(zipped).find(([n]) => n.startsWith('logs/'))[1];
  expect(log).not.toMatch(/^\S+\s+DEBUG /m);

  // GitHub's new-issue page: a title and Yap's version, never the file.
  await saved.getByRole('button', { name: 'Open a GitHub issue' }).click();
  await expect
    .poll(async () => (await yap.invoke('e2e_opened_urls')).find((u) => u.startsWith('https://github.com/nayballs/Yap/issues/new?')))
    .toBeTruthy();
  const issue = (await yap.invoke('e2e_opened_urls')).find((u) => u.startsWith('https://github.com/nayballs/Yap/issues/new?'));
  const body = new URL(issue).searchParams.get('body');
  expect(body).toContain('Yap: ');
  expect(body).not.toContain(GROQ_KEY);
  expect(body).not.toContain(NAME);
  await saved.getByRole('button', { name: 'Done' }).click();
  await expect(group.getByRole('button', { name: 'Report a problem…' })).toBeVisible();
  await closeSettings(main);
});
