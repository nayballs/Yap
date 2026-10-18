// The main window, end to end: launch, every sidebar view, every Settings
// section, a persisted setting, Notes, Dictionary (corrections and
// snippets), the "press enter" voice command's switch, the signed-out
// Account page, onboarding and (with a microphone) a stub dictation.
import {
  test,
  expect,
  sidebar,
  openView,
  openSettings,
  closeSettings,
  settingsDialog,
  expectStore,
  pressHotkey,
} from './support/fixtures.js';

test('launches into Home, isolated in portable mode', async ({ yap, main, shot }) => {
  const greeting = main.getByRole('heading', { level: 1 });
  await expect(greeting).toContainText('to start yapping');
  // The seeded test hotkey (F24) shows as the keycap, i.e. our config loaded.
  await expect(greeting).toContainText('F24');
  expect(await yap.invoke('is_portable')).toBe(true);
  expect(yap.readConfig().hotkey).toBe('kb:135');
  await shot(main, '01-home');
});

const VIEWS = [
  ['Home', (main) => main.getByRole('heading', { level: 1, name: /start yapping/ })],
  ['Insights', (main) => main.getByRole('heading', { level: 1, name: 'Insights' })],
  ['Chat', (main) => main.getByPlaceholder('Type a message...')],
  ['Notes', (main) => main.getByRole('button', { name: 'Search notes' })],
  ['Meetings', (main) => main.getByRole('heading', { level: 1, name: 'Meetings' })],
  ['Upload', (main) => main.getByRole('heading', { level: 1, name: 'Upload' })],
  ['Dictionary', (main) => main.getByRole('heading', { level: 1, name: 'Dictionary' })],
  ['Integrations', (main) => main.getByRole('heading', { level: 1, name: 'Integrations' })],
];

test('every sidebar view renders', async ({ main, shot }) => {
  for (const [i, [label, marker]] of VIEWS.entries()) {
    await openView(main, label);
    await expect(marker(main)).toBeVisible();
    await shot(main, `02-view-${String(i + 1).padStart(2, '0')}-${label.toLowerCase()}`);
  }
});

const SECTIONS = [
  'General',
  'Voice Agent',
  'Speech-to-Text',
  'Language Models',
  'History',
  'Connectors',
  'MCP',
  'Advanced',
  'About',
];

test('every Settings section renders', async ({ main, shot }) => {
  for (const [i, section] of SECTIONS.entries()) {
    const dialog = await openSettings(main, section);
    await expect(dialog.getByRole('heading', { level: 1, name: section })).toBeVisible();
    const slug = section.toLowerCase().replace(/[^a-z]+/g, '-');
    await shot(main, `03-settings-${String(i + 1).padStart(2, '0')}-${slug}`);
  }
  await closeSettings(main);
});

test('a settings change is saved and survives a reload', async ({ yap, main, shot }) => {
  const toggle = (dialog) => dialog.getByRole('button', { name: 'Append trailing space' });
  let dialog = await openSettings(main, 'Advanced');
  await expect(toggle(dialog)).toHaveAttribute('aria-pressed', 'false');
  await toggle(dialog).click();
  await expect(toggle(dialog)).toHaveAttribute('aria-pressed', 'true');
  await expectStore(yap, 'config.json', (c) => c.appendTrailingSpace === true);

  // A fresh page load reads the setting back from disk.
  await main.reload({ waitUntil: 'load' });
  dialog = await openSettings(main, 'Advanced');
  await expect(toggle(dialog)).toHaveAttribute('aria-pressed', 'true');
  await shot(main, '04-settings-saved');

  await toggle(dialog).click();
  await expectStore(yap, 'config.json', (c) => c.appendTrailingSpace === false);
});

test('notes: a new note keeps its text across a view switch', async ({ yap, main, shot }) => {
  await openView(main, 'Notes');
  await main.getByRole('button', { name: 'New note' }).first().click();
  await main.getByPlaceholder('Untitled Note').fill('Groceries');
  await main.getByPlaceholder('Start writing…').fill('Oat milk, coffee, and bread.');

  // Leave straight away: leaving Notes must flush the pending save.
  await openView(main, 'Home');
  await openView(main, 'Notes');
  const item = main.locator('.items .item', { hasText: 'Groceries' });
  await expect(item).toBeVisible();
  await item.click();
  await expect(main.getByPlaceholder('Untitled Note')).toHaveValue('Groceries');
  await expect(main.getByPlaceholder('Start writing…')).toHaveValue('Oat milk, coffee, and bread.');
  await expectStore(yap, 'notes.json', (s) =>
    JSON.stringify(s).includes('Oat milk, coffee, and bread.')
  );
  await shot(main, '05-notes');
});

test('dictionary: a new correction is saved', async ({ yap, main, shot }) => {
  await openView(main, 'Dictionary');
  await main.getByRole('button', { name: '+ Add correction' }).click();
  await main.getByPlaceholder('Power to Keep').last().fill('jaison');
  await main.getByPlaceholder('Parakeet').last().fill('JSON');
  await expectStore(yap, 'config.json', (c) =>
    c.dictionary.some((e) => e.from === 'jaison' && e.to === 'JSON')
  );

  await openView(main, 'Home');
  await openView(main, 'Dictionary');
  await expect(main.getByPlaceholder('Power to Keep').last()).toHaveValue('jaison');
  await shot(main, '06-dictionary');
});

test('dictionary: a snippet is added, kept, edited and removed', async ({ yap, main, shot }) => {
  await openView(main, 'Dictionary');
  const snippets = main.getByRole('region', { name: 'Snippets' });
  const corrections = main.getByRole('region', { name: 'Corrections' });
  await expect(snippets.getByText(/No snippets yet/)).toBeVisible();
  await shot(main, '06b-dictionary-snippets-empty');

  await snippets.getByRole('button', { name: '+ Add snippet' }).click();
  const phrase = snippets.getByRole('textbox', { name: 'Phrase you say' }).last();
  const text = snippets.getByRole('textbox', { name: 'Text Yap types' }).last();
  await expect(phrase).toBeFocused();
  await phrase.fill('my sign-off');
  await text.fill('Speak soon,\nSam');
  await expectStore(yap, 'config.json', (c) =>
    c.dictionary.some((e) => e.snippet === true && e.from === 'my sign-off' && e.to === 'Speak soon,\nSam')
  );
  // A snippet isn't a correction: not in that list.
  const correctionValues = () =>
    corrections.getByRole('textbox').evaluateAll((els) => els.map((el) => el.value));
  expect(await correctionValues()).not.toContain('my sign-off');
  await shot(main, '06c-dictionary-snippet');

  // Kept across a view switch, its text still on two lines.
  await openView(main, 'Home');
  await openView(main, 'Dictionary');
  await expect(phrase).toHaveValue('my sign-off');
  await expect(text).toHaveValue('Speak soon,\nSam');

  // Edited: a third line.
  await text.fill('Speak soon,\nSam\nThe Yap team');
  await expectStore(yap, 'config.json', (c) =>
    c.dictionary.some((e) => e.snippet && e.to === 'Speak soon,\nSam\nThe Yap team')
  );

  // Settings auto-saves its own copy of the config (with the dictionary):
  // that must keep the snippet a snippet.
  const dialog = await openSettings(main, 'Advanced');
  const space = dialog.getByRole('button', { name: 'Append trailing space' });
  const before = await space.getAttribute('aria-pressed');
  await space.click();
  await expectStore(yap, 'config.json', (c) => String(c.appendTrailingSpace) !== before);
  await space.click();
  await expectStore(yap, 'config.json', (c) => String(c.appendTrailingSpace) === before);
  expect(yap.readConfig().dictionary.filter((e) => e.from === 'my sign-off')).toEqual([
    { from: 'my sign-off', to: 'Speak soon,\nSam\nThe Yap team', fuzzy: true, snippet: true },
  ]);
  await closeSettings(main);

  await snippets.getByRole('button', { name: 'Remove snippet' }).last().click();
  await expectStore(yap, 'config.json', (c) => !c.dictionary.some((e) => e.from === 'my sign-off'));
  await expect(snippets.getByText(/No snippets yet/)).toBeVisible();
});

test('settings: "press enter" to send is on by default and can be switched off', async ({ yap, main, shot }) => {
  const toggle = (dialog) => dialog.getByRole('button', { name: /^Say “press enter” to send/ });
  const dialog = await openSettings(main, 'Advanced');
  await expect(toggle(dialog)).toHaveAttribute('aria-pressed', 'true');
  // It presses the auto-submit key, so that choice shows with it on.
  await expect(dialog.getByText('Auto-submit key')).toBeVisible();
  await shot(main, '04b-settings-press-enter');

  await toggle(dialog).click();
  await expect(toggle(dialog)).toHaveAttribute('aria-pressed', 'false');
  await expectStore(yap, 'config.json', (c) => c.voiceEnter === false);
  if (!yap.readConfig().autoSubmit) await expect(dialog.getByText('Auto-submit key')).toBeHidden();

  await toggle(dialog).click();
  await expectStore(yap, 'config.json', (c) => c.voiceEnter === true);
});

test('account: signed out, with the account service unreachable', async ({ main, shot }) => {
  // The sidebar's "Sign in" button opens Settings → Account. (Never click a
  // sign-in button here: those open the system browser.)
  await sidebar(main).getByRole('button', { name: /^Sign in/ }).click();
  const dialog = settingsDialog(main);
  await expect(dialog.getByRole('heading', { level: 1, name: 'Account' })).toBeVisible();
  await expect(dialog.getByRole('heading', { name: 'Sign in to Yap' })).toBeVisible();
  // The page asks the (unreachable) service which sign-in methods exist;
  // without an answer it offers them all rather than nothing.
  await expect(dialog.getByRole('button', { name: 'Continue with Google' })).toBeVisible();
  await expect(dialog.getByRole('button', { name: 'Sign in with your phone' })).toBeVisible();
  await expect(dialog.getByRole('button', { name: 'Sign out' })).toHaveCount(0);
  await main.waitForTimeout(1_000); // let the failed methods check settle
  await expect(dialog.getByRole('heading', { name: 'Sign in to Yap' })).toBeVisible();
  await shot(main, '07-account-signed-out');
});

test('onboarding opens on its first step', async ({ yap, main, shot }) => {
  const onboarding = yap.onboarding;
  const before = await onboarding.evaluate(() => performance.timeOrigin);
  const dialog = await openSettings(main, 'About');
  await dialog.getByRole('button', { name: /Show setup guide again/ }).click();

  // open_onboarding shows the window and reloads the wizard.
  await expect
    .poll(() => yap.invoke('plugin:window|is_visible', { label: 'onboarding' }))
    .toBe(true);
  await expect
    .poll(() => onboarding.evaluate(() => performance.timeOrigin).catch(() => before))
    .not.toBe(before);
  await expect(onboarding.getByRole('heading', { level: 1, name: 'Welcome to Yap' })).toBeVisible();
  // Enabled once the wizard has loaded the config. (Never click it: it
  // downloads a 456 MB model.)
  await expect(onboarding.getByRole('button', { name: /^Download Parakeet V3/ })).toBeEnabled();
  await shot(onboarding, '08-onboarding-welcome');

  await yap.invoke('close_onboarding');
  await expect
    .poll(() => yap.invoke('plugin:window|is_visible', { label: 'onboarding' }))
    .toBe(false);
});

test('dictation (stub engine) lands in the Home feed', async ({ yap, main, shot }) => {
  const mics = await yap.invoke('list_audio_devices');
  test.skip(mics.length === 0, 'no microphone here; no-mic.spec.js covers that path');

  // Test mode never pastes the result anywhere (src-tauri/src/e2e.rs).
  await pressHotkey(main);
  await expect(yap.overlay.locator('.capsule')).toBeVisible();
  await main.waitForTimeout(1_500);
  await shot(yap.overlay, '09-overlay-recording');
  await pressHotkey(main);

  await expect(main.getByText(/\[STT stub: received [\d.]+s of audio/).first()).toBeVisible({
    timeout: 20_000,
  });
  await expectStore(yap, 'history.json', (h) => JSON.stringify(h).includes('STT stub'));
  await shot(main, '09-home-after-dictation');
});
