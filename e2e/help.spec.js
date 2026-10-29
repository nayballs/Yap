// The main window's Help (2026-10-29; views/help/, shell/help.rs): a "?"
// in the title bar beside the bell, with Keyboard shortcuts (also Ctrl+/),
// Microphone help (Settings → Dictation → the mic quiz), Languages, Report
// a problem, What's new (the release page without notes here) and GitHub.
// The shortcut sheet lists every binding the input hook knows, as set now:
// the seeded dictation key (F24), more keys beside it, "Not set" for the
// unbound ones, a transform's key; Esc closes it, "Change them in
// Settings" opens Settings → Shortcuts. Settings' mic quiz runs on made-up
// microphones.
import { test, expect, settingsDialog, closeSettings, setConfig } from './support/fixtures.js';

const help = (main) => main.getByRole('button', { name: 'Help', exact: true });
const menu = (main) => main.getByRole('menu', { name: 'Help' });
const sheet = (main) => main.getByRole('dialog', { name: 'Keyboard shortcuts' });

test('the Help menu: every entry goes where it says', async ({ yap, main, shot }) => {
  await help(main).click();
  await expect(menu(main).getByRole('menuitem')).toHaveText([
    /Keyboard shortcuts/,
    'Microphone help',
    'Languages',
    'Report a problem',
    "What's new",
    'GitHub',
  ]);
  await shot(main, '01-help-menu');
  // ↓ moves through it; Esc closes it and the focus goes back to "?".
  await main.keyboard.press('ArrowDown');
  await expect(menu(main).getByRole('menuitem', { name: 'Microphone help' })).toBeFocused();
  await main.keyboard.press('Escape');
  await expect(menu(main)).toBeHidden();
  await expect(help(main)).toBeFocused();

  // Microphone help: Settings → Dictation at the mic quiz.
  await help(main).click();
  await menu(main).getByRole('menuitem', { name: 'Microphone help' }).click();
  const dialog = settingsDialog(main);
  await expect(dialog.getByRole('heading', { level: 1, name: 'Dictation' })).toBeVisible();
  await expect(dialog.locator('[data-setting="mic-quiz"]')).toBeInViewport();
  await closeSettings(main);

  // Languages: My languages.
  await help(main).click();
  await menu(main).getByRole('menuitem', { name: 'Languages' }).click();
  await expect(dialog.locator('#settings-languages')).toBeInViewport();
  await closeSettings(main);

  // Report a problem: About's.
  await help(main).click();
  await menu(main).getByRole('menuitem', { name: 'Report a problem' }).click();
  await expect(dialog.getByRole('heading', { level: 1, name: 'About' })).toBeVisible();
  await expect(dialog.locator('#settings-report')).toBeInViewport();
  await closeSettings(main);

  // What's new: no release notes on this PC (no update found), so the
  // release page; GitHub: Yap's.
  await help(main).click();
  await menu(main).getByRole('menuitem', { name: "What's new" }).click();
  await expect.poll(() => yap.invoke('e2e_opened_urls')).toContainEqual(expect.stringContaining('github.com/nayballs/Yap/releases'));
  await help(main).click();
  await menu(main).getByRole('menuitem', { name: 'GitHub' }).click();
  await expect.poll(() => yap.invoke('e2e_opened_urls')).toContain('https://github.com/nayballs/Yap');
  // A click outside closes it.
  await help(main).click();
  await main.locator('main.view').click({ position: { x: 20, y: 20 } });
  await expect(menu(main)).toBeHidden();
});

test('the shortcut sheet: every binding as set now, Ctrl+/ to open it, Esc to close', async ({ yap, main, shot }) => {
  const before = await yap.invoke('get_config');
  const transform = { ...(before.transforms?.[0] ?? { id: 'polish', builtin: 'polish', name: '', instruction: '', language: '' }), hotkey: 'kb:ctrl+alt+80' };
  await setConfig(yap, main, {
    moreHotkeys: { hotkey: ['mouse:4'] },
    meetingHotkey: 'kb:alt+win+77',
    snipHotkey: '',
    transforms: [transform, ...(before.transforms ?? []).slice(1)],
  });
  try {
    await main.keyboard.press('Control+/');
    const s = sheet(main);
    await expect(s).toBeVisible();
    await expect(s).toBeFocused();
    // Every one of the input hook's bindings, in its order.
    const rows = s.getByRole('region', { name: 'Anywhere on your PC' }).locator('li');
    const keys = await rows.evaluateAll((els) => els.map((e) => e.dataset.shortcut));
    const sheetData = await yap.invoke('shortcut_sheet');
    expect(keys).toEqual(sheetData.fixed.map((b) => b.key));
    expect(keys.slice(0, 2)).toEqual(['hotkey', 'editHotkey']);
    // Dictate: the seeded F24, the mouse button beside it.
    const dictate = s.locator('[data-shortcut="hotkey"]');
    await expect(dictate.locator('.label')).toHaveText('Dictate');
    await expect(dictate.locator('kbd')).toHaveText(['F24', 'Mouse 4']);
    await expect(dictate.locator('.desc')).toContainText('Press to start');
    await expect(s.locator('[data-shortcut="meetingHotkey"] kbd')).toHaveText(['Win', 'Alt', 'M']);
    await expect(s.locator('[data-shortcut="snipHotkey"] .unset')).toHaveText('Not set');
    // The transform with a key.
    await expect(s.getByRole('region', { name: 'Transforms' }).locator('kbd')).toHaveText(['Ctrl', 'Alt', 'P']);
    // Yap's own keys.
    await expect(s.getByRole('region', { name: "In Yap's window" })).toContainText('This sheet');
    await shot(main, '02-shortcut-sheet');

    // Esc closes; Ctrl+/ again opens and closes it.
    await main.keyboard.press('Escape');
    await expect(s).toBeHidden();
    await main.keyboard.press('Control+/');
    await expect(s).toBeVisible();
    await main.keyboard.press('Control+/');
    await expect(s).toBeHidden();

    // From the menu; push-to-talk says "Hold"; Change them → Settings → Shortcuts.
    await setConfig(yap, main, { recordingMode: 'pushToTalk' });
    await help(main).click();
    await menu(main).getByRole('menuitem', { name: /Keyboard shortcuts/ }).click();
    await expect(s.locator('[data-shortcut="hotkey"] .desc')).toContainText('Hold it and talk');
    await s.getByRole('button', { name: 'Change them in Settings' }).click();
    await expect(s).toBeHidden();
    await expect(settingsDialog(main).getByRole('heading', { level: 1, name: 'Shortcuts' })).toBeVisible();
    await closeSettings(main);
  } finally {
    await setConfig(yap, main, {
      moreHotkeys: before.moreHotkeys ?? {},
      meetingHotkey: before.meetingHotkey,
      snipHotkey: before.snipHotkey,
      transforms: before.transforms ?? [],
      recordingMode: before.recordingMode,
    });
  }
});

test("Settings' mic quiz: the best of the made-up microphones, put first", async ({ yap, main, shot }) => {
  await yap.invoke('e2e_mics', { devices: [{ id: 'array', name: 'Microphone Array (Northwind Audio)', default: true }, { id: 'usb', name: 'Microphone (Fabrikam USB Mic)' }] });
  await yap.invoke('e2e_mic_quiz', { profiles: { array: [0.004, 0.0002], usb: [0.25, 0.001] } });
  try {
    await help(main).click();
    await menu(main).getByRole('menuitem', { name: 'Microphone help' }).click();
    const dialog = settingsDialog(main);
    await dialog.getByRole('button', { name: 'Find my best microphone' }).click();
    const quiz = dialog.getByTestId('mic-quiz');
    await quiz.getByRole('button', { name: 'Start the mic quiz' }).click();
    await expect(quiz).toHaveAttribute('data-phase', 'done', { timeout: 15_000 });
    await expect(quiz.locator('[data-mic="e2e-usb"] .badge.best')).toBeVisible();
    await expect(quiz.locator('[data-mic="e2e-array"]')).toHaveAttribute('data-quality', 'quiet');
    await shot(main, '03-settings-mic-quiz');
    await quiz.locator('[data-mic="e2e-usb"]').getByRole('button', { name: 'Use this one' }).click();
    // Settings' own copy, saved by its auto-save.
    await expect.poll(async () => (await yap.invoke('get_config')).micOrder?.[0]?.id).toBe('e2e-usb');
    await closeSettings(main);
  } finally {
    await setConfig(yap, main, { micOrder: [] });
    await yap.invoke('e2e_mics', { devices: null });
  }
});
