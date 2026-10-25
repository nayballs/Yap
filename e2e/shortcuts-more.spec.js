// Several shortcuts for one action (2026-10-25, Wispr Flow's "Add another";
// dictation/input_hook.rs `moreHotkeys`): Settings → Shortcuts' "+ Add
// another", the same clash rules as every shortcut, the guided mouse-button
// pick, and two keys for dictation both working. A test run has no global
// hook (it must never see the developer's keys), so the keys reach the app
// through the main window's in-page fallback — the hook's own path into
// the pipeline — and `e2e_hotkey_bindings` shows what the hook was given.
// The real hook, with real keys, is the robot's (e2e-real/hotkeys.real.js).
import { test as base, expect, openSettings, closeSettings, expectStore, setConfig } from './support/fixtures.js';

// Made-up microphones (dictation/mics.rs): a quiet tone, on any machine.
const MIC = { id: 'desk', name: 'Microphone (Contoso Desk Mic)' };

const test = base.extend({
  yapOptions: [
    async ({}, use) => use({ name: 'shortcuts-more', env: { YAP_E2E_MICS: JSON.stringify([MIC]) } }),
    { scope: 'worker' },
  ],
});

/** A key as a keyboard sends it, through CDP (Playwright's names stop at F12). */
async function pressKey(page, key, vk, types = ['rawKeyDown', 'keyUp']) {
  const cdp = await page.context().newCDPSession(page);
  try {
    const ev = { key, code: key, windowsVirtualKeyCode: vk, nativeVirtualKeyCode: vk };
    for (const type of types) await cdp.send('Input.dispatchKeyEvent', { type, ...ev });
  } finally {
    await cdp.detach().catch(() => {});
  }
}

const bindings = (yap) => yap.invoke('e2e_hotkey_bindings');
const capsule = (yap) => yap.overlay.locator('.capsule');

test.afterEach(async ({ yap, main }) => {
  await setConfig(yap, main, { moreHotkeys: {}, recordingMode: 'toggle', tripleTapMeeting: false });
});

test('Settings → Shortcuts: "+ Add another" gives an action a second key, with the same clash rules', async ({ yap, main, shot }) => {
  const dialog = await openSettings(main, 'Shortcuts');
  const dictate = dialog.locator('[data-setting="shortcut-dictation"]');
  const meeting = dialog.locator('[data-setting="shortcut-meeting"]');
  await expect(dictate.getByRole('button', { name: '+ Add another' })).toBeVisible();
  // An action with no shortcut yet has nothing to add to.
  await expect(dialog.locator('[data-setting="shortcut-rewrite"]').getByRole('button', { name: '+ Add another' })).toHaveCount(0);

  // The dictation hotkey's own key again: refused.
  await dictate.getByRole('button', { name: '+ Add another' }).click();
  await pressKey(main, 'F24', 135);
  await expect(dictate).toContainText("Already used by another of this action's shortcuts");
  // The meeting shortcut's (Win+Alt+M) — a key of another action: refused.
  // F23: taken, and the hook has it beside F24.
  await pressKey(main, 'F23', 134);
  await expectStore(yap, 'config.json', (c) => JSON.stringify(c.moreHotkeys?.hotkey) === '["kb:134"]');
  await expect(dictate.getByRole('button', { name: /^F23\s*Change$/ })).toBeVisible();
  await expect(dictate.getByRole('button', { name: /^F24\s*Change$/ })).toBeVisible();
  await expect
    .poll(async () => (await bindings(yap)).actions.hotkey.map((b) => b.key).slice(0, 2))
    .toEqual(['keyboard vkey 135', 'keyboard vkey 134']);
  await shot(main, '01-dictate-with-two-keys');

  // Another action can't have F23 now.
  await meeting.getByRole('button', { name: '+ Add another' }).click();
  await pressKey(main, 'F23', 134);
  await expect(meeting).toContainText('Already used by the dictation hotkey');
  await pressKey(main, 'Escape', 27);

  // …nor can a language shortcut or a transform: the same list.
  await expect(dialog.getByText('Each can have up to three more keys or mouse buttons')).toBeVisible();

  // The guided mouse-button pick: left and right refused, a side button taken.
  await dictate.getByRole('button', { name: '+ Add another' }).click();
  await dictate.getByRole('button', { name: 'Mouse button' }).click();
  await expect(dictate).toContainText('Press the mouse button you want to use');
  await main.evaluate(() => window.dispatchEvent(new MouseEvent('mousedown', { button: 2, bubbles: true })));
  await expect(dictate).toContainText('Left and right click are for clicking');
  await shot(main, '02-mouse-button-guide');
  await main.evaluate(() => window.dispatchEvent(new MouseEvent('mousedown', { button: 3, bubbles: true })));
  await expectStore(yap, 'config.json', (c) => JSON.stringify(c.moreHotkeys?.hotkey) === '["kb:134","mouse:4"]');
  await expect.poll(async () => (await bindings(yap)).actions.hotkey[2].key).toBe('mouse button 4');

  // ✕ on F23 removes it; the mouse button stays.
  await dictate.getByRole('button', { name: 'Remove hotkey' }).nth(1).click();
  await expectStore(yap, 'config.json', (c) => JSON.stringify(c.moreHotkeys?.hotkey) === '["mouse:4"]');
  await expect.poll(async () => (await bindings(yap)).actions.hotkey.map((b) => b.key).slice(0, 3)).toEqual([
    'keyboard vkey 135',
    'mouse button 4',
    'none',
  ]);
  await closeSettings(main);
});

test('two keys for dictation: either starts it and either stops it', async ({ yap, main }) => {
  // (Settings' copy takes it in too: its in-page fallback reads it.)
  await setConfig(yap, main, { recordingMode: 'toggle', moreHotkeys: { hotkey: ['kb:134'] } });
  await expect.poll(async () => (await bindings(yap)).actions.hotkey[1].key).toBe('keyboard vkey 134');
  const before = (yap.readJson('history.json') || []).length;

  // F23 (the second key) starts it, F24 (the first) stops it…
  await pressKey(main, 'F23', 134);
  await expect(capsule(yap)).toBeVisible();
  await main.waitForTimeout(1_200);
  await pressKey(main, 'F24', 135);
  await expectStore(yap, 'history.json', (h) => h.length === before + 1 && JSON.stringify(h).includes('STT stub'));
  await expect(capsule(yap)).toBeHidden({ timeout: 15_000 });

  // …and the other way round.
  await pressKey(main, 'F24', 135);
  await expect(capsule(yap)).toBeVisible();
  await main.waitForTimeout(1_200);
  await pressKey(main, 'F23', 134);
  await expectStore(yap, 'history.json', (h) => h.length === before + 2);
  await expect(capsule(yap)).toBeHidden({ timeout: 15_000 });
});

test('Settings → Dictation: push-to-talk offers triple-tap for meeting notes, off by default', async ({ yap, main, shot }) => {
  const dialog = await openSettings(main, 'Dictation');
  await expect(dialog.locator('[data-setting="triple-tap-meeting"]')).toHaveCount(0);
  await dialog.getByRole('tab', { name: 'Push-to-talk' }).click();
  const row = dialog.locator('[data-setting="triple-tap-meeting"]');
  const toggle = row.getByRole('button', { name: 'Triple-tap for meeting notes' });
  await expect(row).toContainText('Tap F24 three times quickly');
  await expect(toggle).toHaveAttribute('aria-pressed', 'false');
  await toggle.click();
  await expectStore(yap, 'config.json', (c) => c.tripleTapMeeting === true && c.recordingMode === 'pushToTalk');
  await shot(main, '03-triple-tap-setting');
  await toggle.click();
  await dialog.getByRole('tab', { name: 'Toggle' }).click();
  await expectStore(yap, 'config.json', (c) => c.tripleTapMeeting === false && c.recordingMode === 'toggle');
  await closeSettings(main);
});
