// Your mouse's side buttons set up in one step (2026-10-31, Wispr Flow's
// "Use your mouse's side buttons?"; dictation/side_buttons.rs,
// settings/SideButtons.svelte): Settings → Shortcuts' dialog — the button
// for dictating, the one for Send, then what they'll do — writing the
// existing bindings (an extra key beside the keyboard shortcut,
// `moreHotkeys`), Undo and Remove on the page, a button another shortcut has
// moved only when asked, and the tip the first time an unbound side button
// is pressed, once.
//
// A test run has no hook (it must never see the developer's mouse): the
// side buttons are the hook's stand-in (`e2e_mouse_button`, the hook's own
// routing: the setup listening, a shortcut on the button, or the tip), and
// once the page's own mouse event (the dialog hears one when the hook
// doesn't). The real hook with real XBUTTON events is the robot's
// (e2e-real/side-buttons.real.js).
import fs from 'node:fs';
import { test as base, expect, openSettings, closeSettings, closeToasts, expectStore, setConfig } from './support/fixtures.js';

// A made-up microphone (dictation/mics.rs): a quiet tone, on any machine.
const MIC = { id: 'desk', name: 'Microphone (Contoso Desk Mic)' };

const test = base.extend({
  yapOptions: [
    async ({}, use) => use({ name: 'side-buttons', env: { YAP_E2E_MICS: JSON.stringify([MIC]) } }),
    { scope: 'worker' },
  ],
});

const BACK = 4;
const FORWARD = 5;
const TITLE = "Set up your mouse's side buttons";

/** A side button through the hook's stand-in: whether the hook would have swallowed it. */
const side = (yap, button, phase) => yap.invoke('e2e_mouse_button', { button, phase });
const bindings = (yap) => yap.invoke('e2e_hotkey_bindings');
const capsule = (yap) => yap.overlay.locator('.capsule');
const setupDialog = (main) => main.getByRole('dialog', { name: TITLE });
const row = (main) => main.locator('[data-setting="side-buttons"]');
const tip = (main) => main.getByRole('status').filter({ hasText: "Use your mouse's side buttons?" });
const log = (yap) => fs.readFileSync(yap.logPath, 'utf8');

test.afterEach(async ({ yap, main }) => {
  await yap.invoke('side_buttons_listen', { on: false }).catch(() => {});
  await yap.invoke('e2e_feature_tips', { on: false }).catch(() => {});
  await setConfig(yap, main, { moreHotkeys: {}, sendHotkey: '', copyLastHotkey: '', recordingMode: 'toggle' });
});

async function openSetup(main) {
  const dialog = await openSettings(main, 'Shortcuts');
  await row(main).scrollIntoViewIfNeeded();
  await row(main).getByRole('button', { name: /^(Set up|Change)…$/ }).click();
  await expect(setupDialog(main)).toBeVisible();
  return dialog;
}

test('Settings → Shortcuts: Back heard for dictating, Forward for Send, saved beside F24, and Undo', async ({ yap, main, shot }) => {
  await openSettings(main, 'Shortcuts');
  await expect(row(main)).toContainText("Your mouse's side buttons");
  await expect(row(main)).toContainText('Dictate with one and send with the other');
  await openSetup(main);
  const sb = setupDialog(main);

  // 1. The button for dictating, heard through the hook (and swallowed:
  // Back doesn't go back while the dialog asks).
  await expect(sb).toContainText('Press the button you want for dictating');
  await expect(sb).toContainText('Step 1 of 3');
  await shot(main, '01-press-for-dictating');
  // (Once the dialog has asked the hook to listen.)
  await expect.poll(() => side(yap, BACK)).toBe(true);

  // 2. The one for Send: Back again is refused, Forward heard by the page itself.
  await expect(sb).toContainText('Press the one for Send');
  await expect(sb.getByRole('button', { name: /^Back/ })).toBeDisabled();
  await expect(sb.getByRole('button', { name: /^Back/ })).toContainText('Dictates');
  await side(yap, BACK);
  await expect(sb).toContainText('Back dictates. Press the other one, or Skip.');
  await main.evaluate(() => window.dispatchEvent(new MouseEvent('mousedown', { button: 1, bubbles: true })));
  await expect(sb).toContainText("That's the middle button");
  await main.evaluate(() => window.dispatchEvent(new MouseEvent('mousedown', { button: 4, bubbles: true })));

  // 3. What they'll do: toggle as the dictation key does now, then push-to-talk.
  const summary = sb.locator('[data-summary]');
  await expect(summary).toHaveText('Back: press to start and stop dictating · Forward: send');
  await expect(sb).toContainText("Back won't go back in your browser while it's set to dictate.");
  await expect(sb).toContainText("Forward won't go forward in your browser while it's set to send.");
  await sb.getByRole('tab', { name: 'Hold to talk' }).click();
  await expect(summary).toHaveText('Back: hold to dictate, double-tap for hands-free · Forward: send');
  await expect(sb).toContainText('F24 changes too: hold it to dictate, double-tap for hands-free.');
  await shot(main, '02-summary');
  // Not listening any more: Back goes back again until it's set up.
  await expect.poll(() => side(yap, BACK)).toBe(false);

  await sb.getByRole('button', { name: 'Set up', exact: true }).click();
  await expect(sb).toBeHidden();
  await expect(main.getByRole('status').filter({ hasText: 'Side buttons set up' })).toBeVisible();

  // The bindings: Back one more dictation key beside F24, Forward Send's first.
  await expectStore(
    yap,
    'config.json',
    (c) =>
      c.hotkey === 'kb:135' &&
      JSON.stringify(c.moreHotkeys?.hotkey) === '["mouse:4"]' &&
      c.sendHotkey === 'mouse:5' &&
      c.recordingMode === 'pushToTalk'
  );
  await expect.poll(async () => (await bindings(yap)).actions.hotkey.map((b) => b.key).slice(0, 2)).toEqual([
    'keyboard vkey 135',
    'mouse button 4',
  ]);
  expect((await bindings(yap)).actions.sendHotkey[0].key).toBe('mouse button 5');
  const dictate = main.locator('[data-setting="shortcut-dictation"]');
  await expect(dictate.getByRole('button', { name: /^Mouse 4\s*Change$/ })).toBeVisible();
  await expect(main.locator('[data-setting="shortcut-send"]').getByRole('button', { name: /^Mouse 5\s*Change$/ })).toBeVisible();
  await expect(row(main)).toContainText('Back: hold to dictate, double-tap for hands-free · Forward: send');
  await closeToasts(main);
  await shot(main, '03-set-up');

  // Undo on the page: everything as it was.
  await row(main).getByRole('button', { name: 'Undo' }).click();
  await expectStore(
    yap,
    'config.json',
    (c) => !c.moreHotkeys?.hotkey && (c.sendHotkey ?? '') === '' && c.recordingMode === 'toggle' && c.hotkey === 'kb:135'
  );
  await expect(row(main)).toContainText('Dictate with one and send with the other');
  await expect(row(main).getByRole('button', { name: 'Undo' })).toHaveCount(0);
  await expect.poll(async () => (await bindings(yap)).actions.sendHotkey[0].key).toBe('none');
  await closeSettings(main);
});

test('set up, Back dictates (toggle and push-to-talk) and Forward sends; removed, Back goes back again', async ({ yap, main }) => {
  await yap.invoke('side_buttons_apply', { choice: { dictate: BACK, send: FORWARD, hold: false } });
  await expect.poll(async () => (await bindings(yap)).actions.hotkey[1].key).toBe('mouse button 4');
  const before = (yap.readJson('history.json') || []).length;

  // Toggle: a click starts it, the next stops it.
  expect(await side(yap, BACK)).toBe(true);
  await expect(capsule(yap)).toBeVisible();
  await main.waitForTimeout(1_200);
  await side(yap, BACK);
  await expectStore(yap, 'history.json', (h) => h.length === before + 1 && JSON.stringify(h).includes('STT stub'));
  await expect(capsule(yap)).toBeHidden({ timeout: 15_000 });

  // Push-to-talk: held while it records, let go to transcribe.
  await yap.invoke('side_buttons_apply', { choice: { dictate: BACK, send: FORWARD, hold: true } });
  await expectStore(yap, 'config.json', (c) => c.recordingMode === 'pushToTalk');
  await side(yap, BACK, 'press');
  await expect(capsule(yap)).toBeVisible();
  await main.waitForTimeout(1_200);
  await side(yap, BACK, 'release');
  await expectStore(yap, 'history.json', (h) => h.length === before + 2);
  await expect(capsule(yap)).toBeHidden({ timeout: 15_000 });

  // Forward: the send key, pressed (a test run presses nothing in another app).
  const mark = log(yap).length;
  expect(await side(yap, FORWARD)).toBe(true);
  await expect.poll(() => log(yap).slice(mark)).toContain('e2e: not pressing the send key in another app');

  // Removed from the Shortcuts page: Back goes back as ever, and dictates nothing.
  await openSettings(main, 'Shortcuts');
  await row(main).scrollIntoViewIfNeeded();
  await expect(row(main)).toContainText('Back: hold to dictate');
  await row(main).getByRole('button', { name: 'Remove' }).click();
  await expectStore(yap, 'config.json', (c) => !c.moreHotkeys?.hotkey && (c.sendHotkey ?? '') === '' && c.hotkey === 'kb:135');
  await expect(row(main)).toContainText('Dictate with one and send with the other');
  expect(await side(yap, BACK)).toBe(false);
  expect(await side(yap, FORWARD)).toBe(false);
  await main.waitForTimeout(800);
  await expect(capsule(yap)).toBeHidden();
  expect((yap.readJson('history.json') || []).length).toBe(before + 2);
  // …and the toast's Undo puts them back.
  await main.getByRole('status').filter({ hasText: 'Side buttons removed' }).getByRole('button', { name: 'Undo' }).click();
  await expectStore(yap, 'config.json', (c) => JSON.stringify(c.moreHotkeys?.hotkey) === '["mouse:4"]' && c.sendHotkey === 'mouse:5');
  await closeToasts(main);
  await closeSettings(main);
});

test('a button another shortcut has is named and moved only when asked; Swap and Skip', async ({ yap, main, shot }) => {
  await setConfig(yap, main, { copyLastHotkey: 'mouse:5' });
  await openSetup(main);
  const sb = setupDialog(main);
  // Back and Forward as offered, picked without pressing them.
  await sb.getByRole('button', { name: 'Next' }).click();
  await expect(sb).toContainText('Press the one for Send');
  await sb.getByRole('button', { name: 'Next' }).click();

  const summary = sb.locator('[data-summary]');
  await expect(sb).toContainText('Forward is set to Copy last dictation.');
  await expect(summary).toHaveText('Back: press to start and stop dictating');
  const move = sb.getByRole('checkbox', { name: /Move Forward to Send/ });
  await expect(move).not.toBeChecked();
  await shot(main, '04-taken');
  await move.check();
  await expect(summary).toHaveText('Back: press to start and stop dictating · Forward: send');

  // Swap: Forward dictates (moved from Copy last), Back sends.
  await sb.getByRole('button', { name: 'Swap them' }).click();
  await expect(summary).toHaveText('Forward: press to start and stop dictating · Back: send');
  await expect(sb.getByRole('checkbox', { name: /Move Forward to Dictate/ })).toBeChecked();
  await sb.getByRole('button', { name: 'Swap them' }).click();
  await sb.getByRole('button', { name: 'Set up', exact: true }).click();
  await expect(sb).toBeHidden();
  await expectStore(
    yap,
    'config.json',
    (c) => (c.copyLastHotkey ?? '') === '' && c.sendHotkey === 'mouse:5' && JSON.stringify(c.moreHotkeys?.hotkey) === '["mouse:4"]'
  );
  await closeToasts(main);

  // Again, Skip for dictating: Forward sends already, nothing to change.
  await row(main).getByRole('button', { name: 'Change…' }).click();
  await expect(sb).toBeVisible();
  await sb.getByRole('button', { name: 'Skip' }).click();
  await expect(sb).toContainText('Press the one for Send');
  await sb.getByRole('button', { name: 'Next' }).click();
  await expect(summary).toHaveText('Forward: send');
  await expect(sb).toContainText('Forward sends already.');
  await expect(sb.getByRole('tab', { name: 'Hold to talk' })).toHaveCount(0);
  await expect(sb.getByRole('button', { name: 'Set up', exact: true })).toBeDisabled();
  await sb.getByRole('button', { name: 'Cancel' }).click();
  await expect(sb).toBeHidden();
  await closeSettings(main);
});

test('the tip: the first unbound side button pressed offers the setup, once; Set up opens the dialog', async ({ yap, main, shot }) => {
  await yap.invoke('e2e_feature_tips', { on: true });
  // Unbound: Back goes back as ever (not swallowed), and Yap offers the setup.
  expect(await side(yap, BACK)).toBe(false);
  const t = tip(main);
  await expect(t).toBeVisible();
  await expect(t).toContainText('Dictate with one and send with the other');
  await shot(main, '05-tip');
  await expectStore(yap, 'hints.json', (h) => !!h.features?.sideButtons);

  await t.getByRole('button', { name: 'Set up' }).click();
  await expect(setupDialog(main)).toBeVisible();
  await expect(setupDialog(main)).toContainText('Press the button you want for dictating');
  await setupDialog(main).getByRole('button', { name: 'Cancel' }).click();
  await expect(setupDialog(main)).toBeHidden();
  await closeSettings(main);
  await closeToasts(main);

  // Once: another press says nothing.
  expect(await side(yap, FORWARD)).toBe(false);
  await main.waitForTimeout(2_000);
  await expect(tip(main)).toHaveCount(0);
});
