// Hinglish (dictation/languages/hinglish.rs, 2026-10-29): Settings →
// Dictation → My languages offers "Write in Latin letters (Hinglish)" once
// Hindi is among your languages, and says which speech model hears Hindi
// (only Whisper's). Then what a Hindi dictation does, through the test
// hooks: Whisper's Devanagari spelt out in Latin letters with cleanup off;
// with it on, cleanup is given the Latin letters and told to keep Hinglish,
// and anything it still writes in Devanagari is spelt out too; with the
// switch off, Hindi stays in Devanagari, exactly as before.
//
// The worker runs "Whisper Large v3" (the stub engine stands in for it), so
// a dictation can say it heard Hindi (`e2e_language_stage`); Parakeet V2's
// folder is there too. Nothing is pasted anywhere: dictations go to a
// stand-in window. Every sentence is made up.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openSettings, closeSettings, settingsDialog, expectStore, setConfig } from './support/fixtures.js';
import { startFakeLlm, cleanupTranscript } from './support/fake-llm.js';

const DEVANAGARI = 'कल की मीटिंग तीन बजे है।';
const HINGLISH = 'Kal ki meeting teen baje hai.';

const test = base.extend({
  fakeLocal: [
    async ({}, use) => {
      const fake = await startFakeLlm();
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({ fakeLocal }, use) => {
      await use({
        name: 'hinglish',
        env: { YAP_E2E_GPU: 'none' },
        config: { modelSize: 'large', postProcessEnabled: false, ppProvider: 'custom', ppBaseUrl: fakeLocal.base, ppModel: 'fake-cleanup', ppApiKey: '' },
        prepare: (dataDir) => {
          fs.mkdirSync(path.join(dataDir, 'models', 'parakeet-tdt-0.6b-v2-int8'), { recursive: true });
          fs.writeFileSync(path.join(dataDir, 'models', 'ggml-large-v3-q5_0.bin'), '');
        },
      });
    },
    { scope: 'worker' },
  ],
});

const hasDevanagari = (s) => /[ऀ-ॿ]/.test(s);

test.afterEach(async ({ yap, main }) => {
  await yap.invoke('e2e_language_stage', { shortcut: null, heard: null, fresh: true }).catch(() => {});
  await yap.invoke('e2e_target_window', { on: false }).catch(() => {});
  await setConfig(yap, main, { myLanguages: [], hinglish: false, modelSize: 'large', postProcessEnabled: false }).catch(() => {});
});

test('Settings → My languages: Hindi offers Hinglish, and says which speech model hears Hindi', async ({ yap, main, shot }) => {
  const dialog = await openSettings(main, 'Dictation');
  const group = dialog.getByRole('group', { name: 'My languages' });
  await group.scrollIntoViewIfNeeded();
  // English alone: no Hinglish switch.
  const search = group.getByRole('combobox', { name: 'Add a language' });
  await search.fill('English');
  await main.keyboard.press('Enter');
  await expectStore(yap, 'config.json', (c) => c.myLanguages.join() === 'en');
  await expect(dialog.getByRole('button', { name: 'Write in Latin letters (Hinglish)' })).toHaveCount(0);

  // Hindi: the switch, off, with Whisper hearing Hindi.
  await search.fill('Hindi');
  await main.keyboard.press('Enter');
  await expectStore(yap, 'config.json', (c) => c.myLanguages.join() === 'en,hi');
  const toggle = dialog.getByRole('button', { name: 'Write in Latin letters (Hinglish)' });
  await expect(toggle).toHaveAttribute('aria-pressed', 'false');
  const row = dialog.locator('[data-setting="hinglish"]');
  await expect(row).toContainText('kal ki meeting teen baje hai');
  await expect(row).toContainText('Whisper Large v3 hears Hindi');
  await toggle.click();
  await expectStore(yap, 'config.json', (c) => c.hinglish === true);
  await row.scrollIntoViewIfNeeded();
  await shot(main, '01-hinglish-on');

  // Parakeet V2: it doesn't hear Hindi, and the switch says so.
  await setConfig(yap, main, { modelSize: 'parakeet-tdt-0.6b-v2' });
  await expect(row).toContainText("doesn't hear Hindi: only Whisper models do");
  await setConfig(yap, main, { theme: 'dark' });
  await row.scrollIntoViewIfNeeded();
  await shot(main, '02-hinglish-parakeet-dark');
  await setConfig(yap, main, { theme: 'light', modelSize: 'large' });

  // Settings search finds it.
  const box = settingsDialog(main).getByRole('searchbox', { name: 'Search settings' });
  await box.fill('hinglish');
  await expect(settingsDialog(main).getByRole('listbox', { name: 'Settings found' }).getByRole('option').filter({ hasText: 'Hinglish' })).toBeVisible();
  await box.fill('');

  // Hindi taken out: the switch goes with it (the setting stays as it was).
  await group.getByRole('button', { name: 'Remove Hindi' }).click();
  await expectStore(yap, 'config.json', (c) => c.myLanguages.join() === 'en');
  await expect(dialog.getByRole('button', { name: 'Write in Latin letters (Hinglish)' })).toHaveCount(0);
  await closeSettings(main);
});

test('a Hindi dictation in Latin letters: spelt out with cleanup off, cleaned as Hinglish with it on, Devanagari with the switch off', async ({ yap, main, fakeLocal }) => {
  await yap.invoke('e2e_target_window', { on: true });
  await setConfig(yap, main, { myLanguages: ['en', 'hi'], hinglish: true, postProcessEnabled: false });

  // Cleanup off: Yap spells it out itself.
  await yap.invoke('e2e_language_stage', { heard: [['hi', 0.97]] });
  let r = await yap.invoke('e2e_dictation', { text: DEVANAGARI });
  expect(r.text.trim()).toBe(HINGLISH);
  expect(r.raw).toBe(HINGLISH);

  // English words Whisper kept in Latin letters stay as they are.
  await yap.invoke('e2e_language_stage', { heard: [['hi', 0.95]] });
  r = await yap.invoke('e2e_dictation', { text: 'मैं Priya को call करूँगा' });
  expect(r.text.trim()).toMatch(/^Main Priya ko call kar/);
  expect(hasDevanagari(r.text)).toBe(false);

  // Cleanup on: it gets the Latin letters and the Hinglish line; what it
  // still writes in Devanagari is spelt out too.
  fakeLocal.reply('cleanup', (user) => `${cleanupTranscript(user)} ठीक है।`);
  await setConfig(yap, main, { postProcessEnabled: true });
  await yap.invoke('e2e_language_stage', { heard: [['hi', 0.97]] });
  r = await yap.invoke('e2e_dictation', { text: DEVANAGARI });
  const asked = fakeLocal.requests.filter((q) => q.kind === 'cleanup').at(-1);
  expect(cleanupTranscript(asked.user)).toBe(HINGLISH);
  expect(asked.user).toContain('write the cleaned text in Latin letters, the way Hinglish is typed');
  expect(asked.system).not.toContain('Latin letters');
  expect(r.text.trim()).toBe(`${HINGLISH} Theek hai.`);
  expect(hasDevanagari(r.text)).toBe(false);

  // The switch off: Devanagari, and cleanup's line as before Hinglish.
  fakeLocal.reply('cleanup', (user) => cleanupTranscript(user));
  await setConfig(yap, main, { hinglish: false });
  await yap.invoke('e2e_language_stage', { heard: [['hi', 0.97]] });
  r = await yap.invoke('e2e_dictation', { text: DEVANAGARI });
  expect(r.text.trim()).toBe(DEVANAGARI);
  const before = fakeLocal.requests.filter((q) => q.kind === 'cleanup').at(-1);
  expect(before.user).toContain('This transcript is in Hindi: write the cleaned text in Hindi, never translating it.');
  expect(before.user).not.toContain('Latin letters');
});
