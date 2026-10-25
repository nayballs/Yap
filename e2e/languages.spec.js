// My languages (dictation/languages.rs, 2026-10-25): Settings → Dictation → My
// languages — the languages you speak picked from a searchable list, which
// installed speech model covers which (and the one recommended), the main
// shortcut's language, a language shortcut recorded and checked against the
// other shortcuts (and they against it), the Speech page's and Shortcuts
// page's links — then what a dictation does with them, through the test
// hooks: a language shortcut's dictation translated by the AI (the fake
// one, support/fake-llm.js), as said when the AI fails, the cleanup told
// the dictation's language; and the nudge, "You spoke French · Add it to My
// languages?", in the main window and on the Yap bar, rate-limited.
//
// The worker runs "Whisper Large v3" (the stub engine stands in for it), so
// the dictations can say which language they heard (`e2e_language_stage`,
// `e2e_language_heard`); Parakeet V2's folder is there too, installed.
// Nothing is pasted anywhere: dictations go to a stand-in window.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openSettings, closeSettings, settingsDialog, expectStore, setConfig, inAnotherApp } from './support/fixtures.js';
import { startFakeLlm, cleanupTranscript, transformText, transformInstruction } from './support/fake-llm.js';

const test = base.extend({
  yapOptions: [
    async ({}, use) => {
      await use({
        name: 'languages',
        config: { modelSize: 'large' },
        // Installed speech models (dictation/stt.rs: a folder or a file each).
        prepare: (dataDir) => {
          fs.mkdirSync(path.join(dataDir, 'models', 'parakeet-tdt-0.6b-v2-int8'), { recursive: true });
          fs.writeFileSync(path.join(dataDir, 'models', 'ggml-large-v3-q5_0.bin'), '');
        },
      });
    },
    { scope: 'worker' },
  ],
});

const SAID = 'hey can you send me the report by friday';
const SPANISH = 'Hola, ¿puedes enviarme el informe el viernes?';
const FRENCH = 'bonjour je voudrais réserver une table pour quatre personnes';

const log = (yap) => fs.readFileSync(yap.logPath, 'utf8');

/** A key as Chromium sends it (the shortcut picker reads keyCode). */
async function pressKey(page, key, vk) {
  const cdp = await page.context().newCDPSession(page);
  try {
    const ev = { key, code: key, windowsVirtualKeyCode: vk, nativeVirtualKeyCode: vk };
    await cdp.send('Input.dispatchKeyEvent', { type: 'rawKeyDown', ...ev });
    await cdp.send('Input.dispatchKeyEvent', { type: 'keyUp', ...ev });
  } finally {
    await cdp.detach().catch(() => {});
  }
}

test.afterEach(async ({ yap, main }) => {
  await yap.invoke('e2e_language_stage', { shortcut: null, heard: null, fresh: true }).catch(() => {});
  await yap.invoke('e2e_target_window', { on: false }).catch(() => {});
  await setConfig(yap, main, { myLanguages: [], languageShortcuts: [], selectedLanguage: 'auto', modelSize: 'large', languageNudge: true }).catch(() => {});
});

test('Settings → Dictation → My languages: picked from a search, which model covers which, the main shortcut, a language shortcut', async ({
  yap,
  main,
  shot,
}) => {
  const dialog = await openSettings(main, 'Dictation');
  const group = dialog.getByRole('group', { name: 'My languages' });
  await group.scrollIntoViewIfNeeded();
  // Nothing picked yet: the search, no model rows.
  await expect(group.locator('[data-setting="language-models"]')).toHaveCount(0);
  await shot(main, '01-my-languages-empty');

  // French, by its own name; English by a click.
  const search = group.getByRole('combobox', { name: 'Add a language' });
  await search.fill('franç');
  const list = group.getByRole('listbox', { name: 'Languages' });
  await expect(list.getByRole('option')).toHaveText([/French\s*Français/]);
  await search.press('Enter');
  await expect(group.getByRole('list', { name: 'Your languages' }).getByRole('listitem')).toHaveText(['French']);
  await expectStore(yap, 'config.json', (c) => JSON.stringify(c.myLanguages) === '["fr"]');
  await search.fill('eng');
  await list.getByRole('option', { name: /^English/ }).click();
  await expectStore(yap, 'config.json', (c) => JSON.stringify(c.myLanguages) === '["fr","en"]');
  await search.fill('zz');
  await expect(list).toContainText('No language called “zz”');
  await search.press('Escape');
  await expect(search).toHaveValue('');

  // Whisper (the worker's model) understands both, and says which you spoke.
  const models = group.locator('[data-setting="language-models"]');
  await expect(models.locator('[data-coverage]')).toHaveText('Whisper Large v3, your speech model, understands all of them.');
  const table = models.getByRole('table', { name: 'Which speech model understands which of your languages' });
  await expect(table.locator('tr[data-model="large"] td')).toHaveText(['✓', '✓']);
  await expect(table.locator('tr[data-model="parakeet-tdt-0.6b-v2"] td')).toHaveText(['—', '✓']);
  await expect(group.locator('[data-setting="main-language"]')).toContainText('Auto: Whisper hears which of your languages you speak');
  await expect(group.locator('[data-setting="language-nudge"]')).toContainText('After a dictation in another language, Yap offers to add it');
  await shot(main, '02-my-languages-whisper');

  // Parakeet V2 (English only): said, and a model for them all recommended —
  // an installed one first (Whisper here) — the model itself never changed.
  await setConfig(yap, main, { modelSize: 'parakeet-tdt-0.6b-v2' });
  await expect(models.locator('[data-coverage]')).toHaveText(
    'Parakeet V2, your speech model, understands English only. French needs another model.'
  );
  await expect(models.locator('[data-recommended="large"]')).toContainText("Whisper Large v3 understands all your languages, and it's installed.");
  await expect(group.locator('[data-setting="main-language"]')).toContainText('Parakeet V2 knows English only');
  await expect(group.locator('[data-setting="language-nudge"]')).toContainText("Only Whisper models say which language they heard. Parakeet V2 doesn't");
  expect((await yap.invoke('get_config')).modelSize).toBe('parakeet-tdt-0.6b-v2');
  await shot(main, '03-my-languages-parakeet-v2');
  // Open Speech-to-Text: its Languages row names them, and leads back.
  await models.getByRole('button', { name: 'Open Speech-to-Text' }).click();
  const speechRow = dialog.locator('[data-setting="language"]');
  await expect(speechRow).toContainText('French, English');
  await speechRow.click();
  await expect(group).toBeInViewport();
  await setConfig(yap, main, { modelSize: 'large' });

  // The main shortcut: Auto, or one of them.
  const main_ = group.getByRole('combobox', { name: "Main shortcut's language" });
  await expect(main_.locator('option')).toHaveText(['Auto', 'French', 'English']);
  await main_.selectOption('fr');
  await expectStore(yap, 'config.json', (c) => c.selectedLanguage === 'fr');
  await main_.selectOption('auto');
  await expectStore(yap, 'config.json', (c) => c.selectedLanguage === 'auto');

  // A language shortcut: English, pasted in Spanish, on F22 — not the
  // dictation hotkey's F24.
  await group.getByRole('button', { name: 'Add a language shortcut' }).click();
  const row = group.locator('[data-language-shortcut]');
  await expect(row).toHaveCount(1);
  await expect(row.getByRole('combobox', { name: 'Language you speak' })).toHaveValue('en');
  await row.getByRole('combobox', { name: 'Paste as' }).selectOption('es');
  await expect(group).toContainText('Translating uses your AI model');
  await row.getByRole('button', { name: 'Set a shortcut' }).click();
  await pressKey(main, 'F24', 135);
  await expect(row).toContainText('Already used by the dictation hotkey');
  await pressKey(main, 'F22', 133);
  await expectStore(yap, 'config.json', (c) => c.languageShortcuts?.[0]?.translateTo === 'es' && c.languageShortcuts[0].hotkey === 'kb:133');
  const id = (await yap.invoke('get_config')).languageShortcuts[0].id;
  await expect.poll(async () => (await yap.invoke('e2e_language_slots')).slots).toEqual([{ id, key: 'keyboard vkey 133' }]);
  await shot(main, '04-language-shortcut');

  // A transform can't take its key, and Shortcuts links to it.
  const commands = await openSettings(main, 'Commands');
  const polish = commands.locator('[data-transform="polish"]');
  await polish.scrollIntoViewIfNeeded();
  await polish.getByRole('button', { name: 'Set a shortcut' }).click();
  await pressKey(main, 'F22', 133);
  await expect(polish).toContainText('Already used by the English → Spanish shortcut');
  await pressKey(main, 'Escape', 27);
  const shortcuts = await openSettings(main, 'Shortcuts');
  const link = shortcuts.locator('[data-setting="shortcut-languages"]');
  await expect(link).toContainText('English → Spanish');
  await link.click();
  await expect(settingsDialog(main).locator('[data-setting="language-shortcuts"]')).toBeInViewport();

  // Deleted; French removed from the list.
  await settingsDialog(main).getByRole('button', { name: 'Delete the English → Spanish shortcut' }).click();
  await expectStore(yap, 'config.json', (c) => c.languageShortcuts.length === 0);
  await expect.poll(async () => (await yap.invoke('e2e_language_slots')).slots).toEqual([]);
  await settingsDialog(main).getByRole('button', { name: 'Remove French' }).click();
  await expectStore(yap, 'config.json', (c) => JSON.stringify(c.myLanguages) === '["en"]');

  // Settings search finds it.
  const box = settingsDialog(main).getByRole('searchbox', { name: 'Search settings' });
  await box.fill('bilingual');
  const found = settingsDialog(main).getByRole('listbox', { name: 'Settings found' });
  await expect(found.getByRole('option').filter({ hasText: 'Languages you speak' })).toBeVisible();
  await box.fill('');
  await closeSettings(main);
});

test('a language shortcut that translates, the AI failing, and cleanup told the language', async ({ yap, main, shot }) => {
  const llm = await startFakeLlm();
  llm.reply('cleanup', (user) => cleanupTranscript(user));
  llm.reply('transform', (user) => (/into Spanish/.test(transformInstruction(user)) ? SPANISH : transformText(user)));
  const asked = (kind) => llm.requests.filter((r) => r.kind === kind);
  await setConfig(yap, main, {
    postProcessEnabled: true,
    ppProvider: 'custom',
    ppBaseUrl: llm.base,
    ppModel: 'fake-cleanup',
    ppApiKey: '',
    myLanguages: ['en', 'fr'],
    languageShortcuts: [{ id: 'l-es', language: 'en', translateTo: 'es', hotkey: 'kb:133' }],
  });
  await yap.invoke('e2e_target_window', { on: true });
  try {
    // The shortcut's dictation: translated, not cleaned up; what was said
    // one "Use what I said" away.
    let cleanups = asked('cleanup').length;
    await yap.invoke('e2e_language_stage', { shortcut: 'l-es', heard: null });
    let r = await yap.invoke('e2e_dictation', { text: SAID });
    expect(r.text.trim()).toBe(SPANISH);
    expect(r.asSaid?.trim()).toBe(SAID);
    expect(asked('cleanup').length).toBe(cleanups);
    const t = asked('transform').at(-1);
    expect(transformText(t.user)).toBe(SAID);
    expect(transformInstruction(t.user)).toMatch(/^The text was dictated in English, as speech-to-text wrote it\. Translate it into Spanish/);
    expect(t.system).toMatch(/^You transform text/);
    expect(log(yap)).toContain('Translate on a shortcut: translated');

    // The AI down: pasted as said, and said so.
    llm.failNext('transform', 1);
    await yap.invoke('e2e_language_stage', { shortcut: 'l-es', heard: null });
    r = await yap.invoke('e2e_dictation', { text: SAID });
    expect(r.text.trim()).toBe(SAID);
    await expect(main.getByRole('status').filter({ hasText: "Couldn't translate into Spanish" })).toBeVisible();
    await shot(main, '05-could-not-translate');

    // The main shortcut: cleanup as ever, told the languages it may be in…
    cleanups = asked('cleanup').length;
    r = await yap.invoke('e2e_dictation', { text: SAID });
    expect(asked('cleanup').length).toBe(cleanups + 1);
    expect(asked('cleanup').at(-1).user).toContain(
      'This transcript may be in English or French: keep it in the language it is in, never translating it, and leave out its filler words (euh, bah).'
    );
    // …and, when Whisper heard French, that it's French.
    await yap.invoke('e2e_language_stage', { shortcut: null, heard: [['fr', 0.97], ['en', 0.02]] });
    r = await yap.invoke('e2e_dictation', { text: FRENCH });
    const c = asked('cleanup').at(-1);
    expect(cleanupTranscript(c.user)).toBe(FRENCH);
    expect(c.user).toContain('This transcript is in French: write the cleaned text in French, never translating it, and leave out its filler words (euh, bah).');
    expect(r.text.trim()).toBe(FRENCH);
    // English heard: no language line (the region's spelling is English's).
    await yap.invoke('e2e_language_stage', { shortcut: null, heard: [['en', 0.99]] });
    await yap.invoke('e2e_dictation', { text: SAID });
    expect(asked('cleanup').at(-1).user).not.toContain('This transcript');
  } finally {
    await setConfig(yap, main, { postProcessEnabled: false, ppBaseUrl: '' });
    await llm.close();
  }
});

test('a language shortcut through the speech engine: its dictation translated', async ({ yap, main }) => {
  const mics = await yap.invoke('list_audio_devices');
  test.skip(mics.length === 0, 'no microphone here; recording needs one');
  const llm = await startFakeLlm();
  llm.reply('transform', (user) => (/into Spanish/.test(transformInstruction(user)) ? SPANISH : transformText(user)));
  await setConfig(yap, main, {
    postProcessEnabled: false,
    ppProvider: 'custom',
    ppBaseUrl: llm.base,
    ppModel: 'fake-cleanup',
    ppApiKey: '',
    myLanguages: ['en', 'fr'],
    languageShortcuts: [{ id: 'l-es', language: 'en', translateTo: 'es', hotkey: 'kb:133' }],
  });
  await yap.invoke('e2e_target_window', { on: true });
  try {
    // Armed as its key's press arms it: the recording it starts takes it.
    await yap.invoke('e2e_language_stage', { shortcut: 'l-es', heard: null });
    const r = await yap.invoke('e2e_dictate_audio', { seconds: 2.5 });
    expect(r?.raw).toContain('STT stub');
    expect(r?.text.trim()).toBe(SPANISH);
    expect((await yap.invoke('e2e_language_slots')).shortcut).toBe('l-es');
    // The next one, on the main shortcut: no translation.
    const transforms = llm.requests.filter((x) => x.kind === 'transform').length;
    const plain = await yap.invoke('e2e_dictate_audio', { seconds: 1.5 });
    expect(plain?.text).toContain('STT stub');
    expect(llm.requests.filter((x) => x.kind === 'transform').length).toBe(transforms);
  } finally {
    await setConfig(yap, main, { ppBaseUrl: '' });
    await llm.close();
  }
});

test('the nudge: "You spoke French", added from the main window; on the bar; at most one in ten minutes; never too short', async ({
  yap,
  main,
  shot,
}) => {
  await setConfig(yap, main, { myLanguages: ['en'] });
  await yap.invoke('e2e_language_stage', { shortcut: null, heard: null, fresh: true });

  // Whisper sure it was French: offered in the main window (it's in front).
  const heard = await yap.invoke('e2e_language_heard', { heard: [['fr', 0.97], ['en', 0.02]], secs: 4 });
  expect(heard).toMatchObject({ top: 'fr', picked: 'en' });
  const toast = main.getByRole('status').filter({ hasText: 'You spoke French · Add it to My languages?' });
  await expect(toast).toBeVisible();
  await expect(toast).toContainText('Yap wrote this dictation as English, one of your languages.');
  await shot(main, '06-nudge-toast');
  await toast.getByRole('button', { name: 'Add French' }).click();
  await expectStore(yap, 'config.json', (c) => JSON.stringify(c.myLanguages) === '["en","fr"]');
  await expect(main.getByRole('status').filter({ hasText: 'French is in My languages' })).toBeVisible();

  // Within ten minutes of that: not again, whatever was heard.
  await yap.invoke('e2e_language_heard', { heard: [['de', 0.96]], secs: 4 });
  await expect.poll(() => log(yap)).toMatch(/Language nudge: not offered.*too soon after the last one|too soon after the last one.*Language nudge: not offered/);

  // In another app: a card on the Yap bar; Not now leaves My languages alone.
  await yap.invoke('e2e_language_stage', { shortcut: null, heard: null, fresh: true });
  await inAnotherApp(yap);
  await yap.invoke('e2e_language_heard', { heard: [['de', 0.96]], secs: 4 });
  const card = yap.overlay.locator('.card[data-region="card:language-nudge"]');
  await expect(card).toContainText('You spoke German · Add it to My languages?');
  await expect(card.getByRole('button', { name: 'Add German' })).toBeVisible();
  await yap.overlay.waitForTimeout(250);
  await shot(yap.overlay, '07-nudge-bar-card');
  await card.getByRole('button', { name: 'Not now' }).click();
  await expect(card).toBeHidden();
  expect((await yap.invoke('get_config')).myLanguages).toEqual(['en', 'fr']);

  // Too short to tell, or not sure enough: never asked.
  await yap.invoke('e2e_language_stage', { shortcut: null, heard: null, fresh: true });
  await yap.invoke('e2e_language_heard', { heard: [['it', 0.97]], secs: 1.2 });
  await yap.invoke('e2e_language_heard', { heard: [['it', 0.6], ['en', 0.3]], secs: 5 });
  await expect.poll(() => log(yap)).toContain('too short to tell');
  expect(log(yap)).toContain('not sure enough');
  await expect(yap.overlay.locator('.card[data-region="card:language-nudge"]')).toHaveCount(0);

  // Switched off: nothing.
  await setConfig(yap, main, { languageNudge: false });
  const before = log(yap).length;
  await yap.invoke('e2e_language_heard', { heard: [['it', 0.98]], secs: 5 });
  await main.waitForTimeout(500);
  expect(log(yap).slice(before)).not.toContain('Language nudge: offered');
});
