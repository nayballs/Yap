// Markdown files open in Yap, from the command line (notes/md_file.rs,
// `args.rs`; md-file.spec.js has the window itself): `yap.exe --autostart
// launch-plan.md not-there.md readme.txt`, started in the app's folder. The
// relative path is from there; the flag, a file that isn't there and one
// that isn't Markdown are never opened. A dev or test build has no
// single-instance, so the path is read at start (lib.rs) — the same reading
// a release build's second launch hands the running Yap. Fictional text.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect } from './support/fixtures.js';

const TEXT = '# Launch plan\n\nOpened from the command line.\n';

const test = base.extend({
  yapOptions: [
    {
      name: 'md-file-launch',
      args: ['--autostart', 'launch-plan.md', 'not-there.md', 'readme.txt'],
      prepare: (dataDir) => {
        // Beside the app: the folder it starts in.
        fs.writeFileSync(path.join(dataDir, '..', 'launch-plan.md'), `﻿${TEXT.replace(/\n/g, '\r\n')}`, 'utf8');
        fs.writeFileSync(path.join(dataDir, '..', 'readme.txt'), 'Not Markdown.\n', 'utf8');
      },
    },
    { scope: 'worker', option: true },
  ],
});

const labelOf = (page) => page.evaluate(() => window.__TAURI_INTERNALS__?.metadata?.currentWindow?.label ?? null).catch(() => null);

/** The pages of the Markdown files' windows open now, by label. */
async function docPages(yap) {
  const out = new Map();
  for (const page of yap.browser.contexts()[0].pages()) {
    const l = await labelOf(page);
    if (l?.startsWith('doc-')) out.set(l, page);
  }
  return out;
}

test('a Markdown file named at launch opens in its window, and only that', async ({ yap, main, shot }) => {
  await expect.poll(async () => [...(await docPages(yap)).keys()].length, { timeout: 30_000 }).toBe(1);
  const [[label, doc]] = [...(await docPages(yap)).entries()];
  doc.on('pageerror', (err) => yap.pageErrors.push({ label, message: err.stack || String(err) }));
  await expect(doc.getByRole('heading', { level: 1, name: 'launch-plan.md' })).toBeVisible({ timeout: 15_000 });
  await expect(doc.getByText(yap.appDir, { exact: true })).toBeVisible();
  const view = doc.getByRole('group', { name: 'View' });
  await view.getByRole('button', { name: 'Source' }).click();
  await expect(doc.getByRole('textbox', { name: 'Markdown source' })).toHaveValue(TEXT);
  await shot(doc, '01-launched');
  // The same file through the command: its window, not a second one.
  expect(await yap.invoke('open_markdown', { path: path.join(yap.appDir, 'launch-plan.md') })).toBe(label);
  expect([...(await docPages(yap)).keys()]).toEqual([label]);
  await doc.getByRole('button', { name: 'Close', exact: true }).click();
  await expect.poll(async () => (await docPages(yap)).size).toBe(0);
});
