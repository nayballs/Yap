// A dictation's alerts (alert.rs): things the person has to act on, shown
// as a toast in the main window while it's on screen, else a card on the
// Yap bar (else a Windows notification, which a test run never posts).
//
// - Windows' microphone privacy switches (mic_privacy.rs): "Windows is
//   blocking microphone access for desktop apps" with Open settings, when a
//   recording starts and in place of the dead-mic watchdog's "No sound from
//   …". A test run reads no registry: `e2e_mic_privacy` stands in. (Without
//   a microphone at all: no-mic.spec.js.)
import fs from 'node:fs';
import { test, expect, closeToasts, expectStore, pressHotkey } from './support/fixtures.js';

const now = Math.floor(Date.now() / 1000);
const DICTATION = {
  ts: now - 60,
  raw: 'deploy it to staging',
  text: 'Deploy it to staging.',
  model: 'parakeet-tdt-0.6b-v3',
  app: 'powershell.exe',
};

test.use({ yapOptions: { name: 'dictation-alerts', data: { 'history.json': [DICTATION] } } });

const logCount = (yap, text) => fs.readFileSync(yap.logPath, 'utf8').split(text).length - 1;
const dictations = (yap) => (yap.readJson('history.json') || []).length;
const capsule = (yap) => yap.overlay.locator('.capsule');

async function needsMic(yap) {
  const mics = await yap.invoke('list_audio_devices');
  test.skip(mics.length === 0, 'no microphone here; recording needs one');
}

/** The main window away (closed to the tray), so alerts go to the bar. */
async function mainAway(yap) {
  await yap.invoke('plugin:window|close', { label: 'settings' });
  await expect.poll(() => yap.invoke('plugin:window|is_visible', { label: 'settings' })).toBe(false);
}

test.afterEach(async ({ yap }) => {
  await yap.invoke('e2e_mic_privacy', { blocked: null }).catch(() => {});
  await yap.invoke('e2e_dictation_mic', { silent: false }).catch(() => {});
});

test("Windows' microphone privacy: said when a recording starts, with Open settings", async ({ yap, main, shot }) => {
  await needsMic(yap);
  await yap.invoke('e2e_mic_privacy', { blocked: 'desktopApps' });
  // A blocked mic gives digital silence: the real mic's audio zeroed, as
  // Windows would.
  await yap.invoke('e2e_dictation_mic', { silent: true });
  const before = dictations(yap);
  await pressHotkey(main);
  // At once, not after the watchdog's five seconds: the capsule's line…
  await expect(capsule(yap).getByRole('alert')).toHaveText('Windows is blocking the microphone', { timeout: 2_000 });
  await expect(capsule(yap).locator('.dot.rec')).toBeVisible();
  // …and the toast, naming the switch, with Open settings.
  const toast = main.getByRole('status').filter({ hasText: 'Windows is blocking microphone access for desktop apps' });
  await expect(toast).toBeVisible();
  await expect(toast).toContainText('Let desktop apps access your microphone');
  await expect(toast).toContainText('Microphone'); // its chip
  await shot(main, '03-mic-blocked-toast');
  await shot(yap.overlay, '04-mic-blocked-capsule');

  // Past the watchdog's report: no "No sound from…" on top of it.
  await main.waitForTimeout(5_500);
  await expect(main.getByRole('status').filter({ hasText: 'No sound from' })).toHaveCount(0);
  expect(logCount(yap, 'Dictation mic: No sound from')).toBe(0);

  // Open settings: Windows Settings → Privacy & security → Microphone (a
  // test run only logs it).
  await toast.getByRole('button', { name: 'Open settings' }).click();
  await expect.poll(() => logCount(yap, 'e2e: not opening ms-settings:privacy-microphone')).toBe(1);

  // Stopped without a sound: nothing transcribed, the overlay ends on why.
  await pressHotkey(main);
  await expect(capsule(yap)).toContainText('Windows is blocking the microphone');
  await expect(capsule(yap)).toBeHidden({ timeout: 10_000 });
  expect(dictations(yap)).toBe(before);
  await closeToasts(main);
});

test('a dead mic with the privacy switch off is blamed on the switch, on the bar when the window is away', async ({ yap, shot }) => {
  await needsMic(yap);
  await yap.invoke('e2e_dictation_mic', { silent: true });
  await mainAway(yap);
  try {
    // The bar's Dictate; the switch turned off mid-recording: the watchdog
    // finds it, not "No sound from <mic>".
    await yap.invoke('toggle_recording');
    await expect(capsule(yap)).toBeVisible();
    await yap.invoke('e2e_mic_privacy', { blocked: 'apps' });
    const card = yap.overlay.getByRole('status').filter({ hasText: 'Windows is blocking microphone access for desktop apps' });
    await expect(card).toBeVisible({ timeout: 8_000 });
    await expect(card).toContainText('Let apps access your microphone');
    await expect(capsule(yap).getByRole('alert')).toHaveText('Windows is blocking the microphone');
    await yap.overlay.waitForTimeout(300);
    await shot(yap.overlay, '05-mic-blocked-card');
    expect(logCount(yap, 'Dictation mic: No sound from')).toBe(0);
    // Its Open settings.
    const before = logCount(yap, 'e2e: not opening ms-settings:privacy-microphone');
    await card.getByRole('button', { name: 'Open settings' }).click();
    await expect.poll(() => logCount(yap, 'e2e: not opening ms-settings:privacy-microphone')).toBe(before + 1);
    await yap.invoke('cancel_recording');
    await expect(capsule(yap)).toBeHidden({ timeout: 10_000 });
  } finally {
    await yap.invoke('open_settings');
  }
});
