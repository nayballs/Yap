// Helpers for Settings → Connectors' specs (connectors.spec.js,
// connectors-discover.spec.js): the fake MCP servers (support/fake-mcp.js
// over HTTP, run as a child), Yap's data files, the page and its dialogs, and
// the browser's part of a sign-in (test mode keeps the address Yap would
// open: `e2e_opened_urls`). Fictional data only; nothing leaves this PC.
import fs from 'node:fs';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { expect, openSettings, settingsDialog } from './fixtures.js';

export const FAKE = fileURLToPath(new URL('./fake-mcp.js', import.meta.url));
export const NODE = process.execPath;

/** A file's JSON lines (the fakes' logs). */
export const lines = (file) =>
  fs.existsSync(file)
    ? fs
        .readFileSync(file, 'utf8')
        .split('\n')
        .filter(Boolean)
        .map((l) => JSON.parse(l))
    : [];

/** Yap's data file `file`, as text ('' when it isn't there). */
export const read = (yap, file) => {
  const p = path.join(yap.dataDir, file);
  return fs.existsSync(p) ? fs.readFileSync(p, 'utf8') : '';
};

/** connectors.json's connectors. */
export const stored = (yap) => JSON.parse(read(yap, 'connectors.json') || '{"connectors":[]}').connectors;

/** fake-mcp.js --http on a free port in `dir`, logging to its own file; `stop()` ends it. */
export async function fakeHttp(dir, name, args = []) {
  fs.mkdirSync(dir, { recursive: true });
  const portFile = path.join(dir, `${name}.port`);
  const logFile = path.join(dir, `${name}.log`);
  fs.rmSync(portFile, { force: true });
  fs.rmSync(logFile, { force: true });
  const child = spawn(NODE, [FAKE, '--http', '0', '--port-file', portFile, '--log', logFile, ...args], { stdio: 'ignore', windowsHide: true });
  await expect.poll(() => (fs.existsSync(portFile) ? fs.readFileSync(portFile, 'utf8') : ''), { message: `${name} to listen` }).not.toBe('');
  const port = Number(fs.readFileSync(portFile, 'utf8'));
  return { url: `http://127.0.0.1:${port}/mcp`, log: () => lines(logFile).filter((l) => l.kind === 'http'), stop: () => child.kill() };
}

/** Settings open on Connectors (the preview's page). */
export async function openConnectors(main) {
  const dialog = await openSettings(main, 'Connectors');
  await expect(dialog.getByRole('heading', { level: 1, name: 'Connectors' })).toBeVisible();
  return dialog;
}

/** + Add ▾ → `item` ("Custom connector…", "Program on this PC…", …): its dialog. */
export async function addVia(main, item, title) {
  const dialog = settingsDialog(main);
  await dialog.getByRole('button', { name: 'Add', exact: true }).click();
  await main.getByRole('menu', { name: 'Add a connector' }).getByRole('menuitem', { name: item }).click();
  const d = main.getByRole('dialog', { name: title });
  await expect(d).toBeVisible();
  return d;
}

/** Yours' row for connector `name`. */
export const row = (main, name) =>
  settingsDialog(main)
    .getByRole('list', { name: 'Your connectors' })
    .getByRole('listitem')
    .filter({ has: main.getByRole('button', { name: new RegExp(`^${name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}:`) }) });

/** The addresses test mode kept for "open in the browser". */
export const opened = (yap) => yap.invoke('e2e_opened_urls');

/** The next address Yap opens in the browser after `before` of them. */
export async function nextOpened(yap, before) {
  let url = null;
  await expect
    .poll(
      async () => {
        const urls = await opened(yap);
        url = urls.length > before ? urls.at(-1) : null;
        return !!url;
      },
      { timeout: 20_000, message: 'the browser to be opened on the sign-in' }
    )
    .toBe(true);
  return url;
}

/** The person allowing the sign-in in the browser: the fake's consent, then Yap's loopback page. */
export async function approveInBrowser(oauth, url) {
  const back = await oauth.approve(url);
  return (await fetch(back)).text();
}

/** No text field on the main window's page holds `secret` (a token the page was given). */
export async function noFieldHolds(main, secret) {
  const held = await main.evaluate((s) => [...document.querySelectorAll('input, textarea')].some((i) => i.value.includes(s)), secret);
  expect(held, 'a field still holds the secret').toBe(false);
  expect(await main.evaluate((s) => document.body.innerText.includes(s), secret), 'the page shows the secret').toBe(false);
}
