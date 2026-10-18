// Deleting a speech model in Settings → Speech-to-Text (ModelManager.svelte)
// asks first, in Yap's own dialog. It used `window.confirm()`, which in a
// Tauri window goes to the dialog plugin this window isn't allowed: it threw,
// so the trash button did nothing at all (2026-10-18).
//
// A model counts as installed when its file or folder is in Data/models, so
// the spec seeds an empty Moonshine Base folder (never the active model: the
// default is Parakeet V3, which isn't installed here).
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openSettings } from './support/fixtures.js';

const MODEL_DIR = 'moonshine-base';

const test = base.extend({
  yapOptions: [
    async ({}, use) => {
      await use({
        name: 'speech-models',
        prepare: (dataDir) => fs.mkdirSync(path.join(dataDir, 'models', MODEL_DIR), { recursive: true }),
      });
    },
    { scope: 'worker' },
  ],
});

test('deleting a speech model asks first, Cancel keeps it, Delete removes it', async ({ yap, main, shot }) => {
  const folder = path.join(yap.dataDir, 'models', MODEL_DIR);
  expect(fs.existsSync(folder)).toBe(true);

  const dialog = await openSettings(main, 'Speech-to-Text');
  const row = dialog.locator('.manager .mrow').filter({ hasText: 'Moonshine Base' });
  await row.scrollIntoViewIfNeeded();
  await expect(row.getByRole('button', { name: 'Download' })).toHaveCount(0);

  // The trash button asks; nothing is deleted yet.
  await row.hover();
  await row.getByRole('button', { name: 'Delete model' }).click();
  const ask = main.getByRole('alertdialog', { name: 'Delete Moonshine Base?' });
  await expect(ask).toBeVisible();
  await expect(ask).toContainText('You can download it again later.');
  await expect(ask.getByRole('button', { name: 'Cancel' })).toBeFocused();
  await shot(main, '01-delete-speech-model-asks');

  // Cancel (and Esc) keep it.
  await ask.getByRole('button', { name: 'Cancel' }).click();
  await expect(ask).toBeHidden();
  expect(fs.existsSync(folder)).toBe(true);
  await row.hover();
  await row.getByRole('button', { name: 'Delete model' }).click();
  await expect(ask).toBeVisible();
  await main.keyboard.press('Escape');
  await expect(ask).toBeHidden();
  expect(fs.existsSync(folder)).toBe(true);

  // Delete removes it, and the row offers the download again.
  await row.hover();
  await row.getByRole('button', { name: 'Delete model' }).click();
  await ask.getByRole('button', { name: 'Delete', exact: true }).click();
  await expect(ask).toBeHidden();
  await expect.poll(() => fs.existsSync(folder)).toBe(false);
  await expect(row.getByRole('button', { name: 'Download' })).toBeVisible();
  await shot(main, '02-speech-model-deleted');
});
