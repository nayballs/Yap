// Deleting a speech model in Settings → Speech-to-Text (ModelManager.svelte)
// asks first, in Yap's own dialog. It used `window.confirm()`, which in a
// Tauri window goes to the dialog plugin this window isn't allowed: it threw,
// so the trash button did nothing at all (2026-10-18).
//
// A model counts as installed when its file or folder is in Data/models, so
// the spec seeds an empty Moonshine Base folder (never the active model: the
// default is Parakeet V3, which isn't installed here), and SenseVoice and
// Canary 180M Flash to switch between (the stub engine never reads them).
//
// Switching the speech model saves it in Rust (`set_active_model`), outside
// Settings' copy of the config: the next switch flipped anywhere in Settings
// put the old model back, until Settings adopted saves from elsewhere
// (2026-10-23).
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openSettings, closeSettings, expectStore } from './support/fixtures.js';

const MODEL_DIR = 'moonshine-base';
const SWITCH_DIRS = ['sense-voice-int8', 'canary-180m-flash'];

const test = base.extend({
  yapOptions: [
    async ({}, use) => {
      await use({
        name: 'speech-models',
        prepare: (dataDir) => {
          for (const dir of [MODEL_DIR, ...SWITCH_DIRS]) fs.mkdirSync(path.join(dataDir, 'models', dir), { recursive: true });
        },
      });
    },
    { scope: 'worker' },
  ],
});

test('a speech model switched outside Settings stays when a setting is changed in Settings', async ({ yap, main }) => {
  // On the Speech-to-Text page: Use SenseVoice (saved in Rust).
  let dialog = await openSettings(main, 'Speech-to-Text');
  const row = dialog.locator('.manager .mrow').filter({ hasText: 'SenseVoice' });
  await row.scrollIntoViewIfNeeded();
  await row.getByRole('button', { name: 'Use' }).click();
  await expectStore(yap, 'config.json', (c) => c.modelSize === 'sense-voice-int8');
  // A switch in Settings: the model stays.
  dialog = await openSettings(main, 'Dictation');
  const offer = dialog.getByRole('button', { name: 'Offer what you said after AI cleanup' });
  await offer.click();
  await expectStore(yap, 'config.json', (c) => c.asSaidOffer === false);
  await main.waitForTimeout(700);
  expect(yap.readConfig().modelSize).toBe('sense-voice-int8');

  // As the tray or onboarding does it (no page involved): Canary 180M Flash.
  await yap.invoke('set_active_model', { modelSize: 'canary-180m-flash' });
  await expectStore(yap, 'config.json', (c) => c.modelSize === 'canary-180m-flash');
  // The next switch keeps it too.
  await offer.click();
  await expectStore(yap, 'config.json', (c) => c.asSaidOffer === true);
  await main.waitForTimeout(700);
  expect(yap.readConfig().modelSize).toBe('canary-180m-flash');
  await closeSettings(main);
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
