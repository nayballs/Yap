// The Yap bar's ^ menu extras (src-tauri/src/bar/menu.rs, src/lib/bar/Overlay.svelte;
// Wispr Flow's Flow Bar menu, by behaviour): the microphone in use, with the
// others connected in the order Yap tries them (picking one puts it first in
// Settings → Dictation → Microphone's order), Paste last transcript (into the
// app you're in: the bar never takes the focus), and the language shortcuts'
// languages (a dictation in one, as its shortcut starts).
//
// No real device is touched: made-up microphones (`YAP_E2E_MICS`, dictation/mics.rs)
// stand in for Windows' list, a dictation seeded in history is the last
// transcript, and a test run pastes into nothing (it logs what it would).
import fs from 'node:fs';
import { test, expect, expectStore } from './support/fixtures.js';

const now = Math.floor(Date.now() / 1000);
const LAPTOP = { id: 'laptop', name: 'Microphone Array (Northwind Audio)' };
const HEADSET = { id: 'headset', name: 'Headset Microphone (Contoso H2)' };
const DICTATION = { ts: now - 60, raw: 'see you at ten', text: 'See you at ten.', model: 'parakeet-tdt-0.6b-v3', app: 'notepad.exe' };

test.use({
  yapOptions: {
    name: 'bar-menu',
    // The laptop's mic is Windows' default; nothing in the order yet.
    env: { YAP_E2E_MICS: JSON.stringify([LAPTOP, HEADSET]) },
    config: { myLanguages: ['en', 'fr'], languageShortcuts: [{ id: 'l-fr', language: 'fr', translateTo: '', hotkey: '' }] },
    data: { 'history.json': [DICTATION] },
  },
});

const simulate = (yap, args) => yap.invoke('bar_simulate', args);
const logText = (yap) => fs.readFileSync(yap.logPath, 'utf8');

async function openMenu(yap) {
  const bar = yap.overlay;
  await simulate(yap, { pointer: 'pill' });
  await expect.poll(() => yap.invoke('bar_debug').then((d) => d.interactive)).toBe(true);
  await bar.getByRole('button', { name: 'More' }).click();
  const menu = bar.getByRole('menu');
  await expect(menu).toBeVisible();
  return menu;
}

test.afterEach(async ({ yap }) => {
  await simulate(yap, { pointer: 'away' }).catch(() => {});
  // The menu closes a moment after the pointer leaves: gone before the next
  // test's More (which would close it again).
  await expect(yap.overlay.getByRole('menu')).toHaveCount(0);
});

test('the menu shows the microphone in use and picks another, first in the order', async ({ yap, shot }) => {
  const menu = await openMenu(yap);
  // Windows' default is in use: Yap has no order yet.
  const inUse = menu.getByRole('menuitem', { name: 'Microphone: Northwind Audio' });
  await expect(inUse).toBeVisible();
  await expect(inUse).toHaveAttribute('aria-expanded', 'false');
  // The fixed items are all still there.
  for (const item of ['Open Yap', 'New meeting note', 'Settings', 'Hide the bar for 1 hour', 'Turn off the bar']) {
    await expect(menu.getByRole('menuitem', { name: item, exact: true })).toBeVisible();
  }
  await inUse.click();
  const mics = menu.getByRole('group', { name: 'Microphones' }).getByRole('menuitemradio');
  await expect(mics).toHaveText([/Northwind Audio/, /Contoso H2/]);
  await expect(mics.nth(0)).toHaveAttribute('aria-checked', 'true');
  await expect(mics.nth(1)).toHaveAttribute('aria-checked', 'false');
  await yap.overlay.waitForTimeout(200);
  await shot(yap.overlay, '01-menu-microphones');

  // The headset: first in the order now, so the next dictation uses it.
  await mics.nth(1).click();
  await expect(menu).toBeHidden();
  await expectStore(yap, 'config.json', (c) => c.micOrder?.[0]?.id === 'e2e-headset' && c.micOrder[0].name === HEADSET.name);
  const again = await openMenu(yap);
  await expect(again.getByRole('menuitem', { name: 'Microphone: Contoso H2' })).toBeVisible();
});

test('Paste last transcript goes into the app you\'re in; a language dictates in it', async ({ yap, shot }) => {
  const menu = await openMenu(yap);
  const paste = menu.getByRole('menuitem', { name: 'Paste last transcript' });
  await expect(paste).toBeVisible();
  const french = menu.getByRole('menuitem', { name: 'Dictate in French' });
  await expect(french).toBeVisible();
  await shot(yap.overlay, '02-menu-paste-and-languages');

  await paste.click();
  await expect(menu).toBeHidden();
  await expect.poll(() => logText(yap)).toContain('e2e: not pasting the last transcript');

  // A dictation in French: it records (a made-up mic), started by the menu
  // as its shortcut would, and the next press of ✓ stops it.
  const again = await openMenu(yap);
  await again.getByRole('menuitem', { name: 'Dictate in French' }).click();
  await expect(yap.overlay.locator('.capsule')).toBeVisible();
  await expect.poll(() => logText(yap)).toContain('Dictation: started with a language shortcut');
  await yap.invoke('bar_action', { action: 'dictation-cancel' });
  await expect.poll(() => logText(yap)).toMatch(/Recording cancelled/);
});
