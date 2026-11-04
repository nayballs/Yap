// Mic-quality tips and the laptop's lid (dictation/mic_tips.rs, dictation/lid.rs,
// src/lib/notices/micTips.js):
// - after a dictation with a weak signal — quiet, clipped — "Yap didn't hear
//   you clearly" with one concrete tip and one button; a clear one says
//   nothing; rarely (not again the same day); "Show tips" off, none;
// - a Bluetooth headset in its call profile (made-up, at 16 kHz): "Bluetooth
//   headsets sound worse while their mic is on", the PC's mic suggested,
//   with Microphones → Settings → Dictation → Microphone;
// - the lid closed: the laptop's own microphone gives way to the next one in
//   the order; when it's all there is, "Your laptop's lid is closed, so its
//   microphone can't hear you", with Microphones.
//
// No real device is touched: made-up microphones (`YAP_E2E_MICS`, `e2e_mics`),
// a made-up lid (`e2e_lid`), and dictations played in the mic's place
// (`e2e_dictate_audio`: a tone in syllables at the amplitude asked for). Test
// mode judges a dictation's signal only once a spec asks (`e2e_mic_tips`).
import { test, expect, settingsDialog, closeSettings, setConfig } from './support/fixtures.js';

const LAPTOP = { id: 'laptop', name: 'Microphone Array (Northwind Audio)', builtIn: true };
const USB = { id: 'usb', name: 'Microphone (Fabrikam USB Mic)' };
const BUDS = { id: 'buds', name: 'Headset (Contoso Buds Hands-Free AG Audio)', bluetooth: true };
const id = (m) => `e2e-${m.id}`;

test.use({
  yapOptions: {
    name: 'mic-tips',
    env: { YAP_MIC_WARM_MS: '500', YAP_E2E_MICS: JSON.stringify([LAPTOP, USB]) },
  },
});

const toast = (main, text) => main.getByRole('status').filter({ hasText: text });
const NOT_CLEAR = "Yap didn't hear you clearly";

/** Made-up microphones `devices` in the order `order`; tips on and due; no lid. */
async function setUp(yap, main, devices, order) {
  await yap.invoke('e2e_lid', { closed: null });
  await yap.invoke('e2e_mics', { devices });
  await setConfig(yap, main, { micOrder: order.map((m) => ({ id: id(m), name: m.name })), micNotNow: [], showTips: true });
  await yap.invoke('e2e_mic_tips', { on: true });
  await expect.poll(async () => (await yap.invoke('e2e_mic_status')).open, { timeout: 5_000 }).toBe(false);
}

test.afterEach(async ({ yap }) => {
  await yap.invoke('e2e_mic_tips', { on: false }).catch(() => {});
  await yap.invoke('e2e_lid', { closed: null }).catch(() => {});
});

test('a quiet dictation gets one tip, a clear one none, and not another the same day', async ({ yap, main, shot }) => {
  await setUp(yap, main, [LAPTOP, USB], [USB]);
  // Clear speech: nothing to say.
  await yap.invoke('e2e_dictate_audio', { seconds: 3, amplitude: 0.3 });
  await main.waitForTimeout(2_500);
  await expect(toast(main, NOT_CLEAR)).toHaveCount(0);

  // Far from the mic.
  await yap.invoke('e2e_dictate_audio', { seconds: 3, amplitude: 0.004 });
  const tip = toast(main, NOT_CLEAR);
  await expect(tip).toBeVisible({ timeout: 10_000 });
  await expect(tip).toContainText('Move closer to the microphone');
  await expect(tip.getByRole('button', { name: 'Sound settings' })).toBeVisible();
  await shot(main, '01-quiet');
  await tip.getByRole('button', { name: 'Sound settings' }).click();
  await expect(tip).toHaveCount(0);

  // Rare: a clipped one straight after says nothing.
  await yap.invoke('e2e_dictate_audio', { seconds: 3, amplitude: 1.0 });
  await main.waitForTimeout(2_500);
  await expect(toast(main, NOT_CLEAR)).toHaveCount(0);
});

test('"Show tips" off: no mic tips; on again, a clipped dictation is told to turn the level down', async ({ yap, main }) => {
  await setUp(yap, main, [LAPTOP, USB], [USB]);
  await setConfig(yap, main, { showTips: false });
  await yap.invoke('e2e_dictate_audio', { seconds: 3, amplitude: 1.0 });
  await main.waitForTimeout(2_500);
  await expect(toast(main, NOT_CLEAR)).toHaveCount(0);

  await setConfig(yap, main, { showTips: true });
  await yap.invoke('e2e_dictate_audio', { seconds: 3, amplitude: 1.0 });
  const tip = toast(main, NOT_CLEAR);
  await expect(tip).toBeVisible({ timeout: 10_000 });
  await expect(tip).toContainText('so loud that your words distort');
});

test("a Bluetooth headset in hands-free mode: try the PC's mic, with Microphones", async ({ yap, main, shot }) => {
  await setUp(yap, main, [LAPTOP, BUDS], [BUDS]);
  await yap.invoke('e2e_dictate_audio', { seconds: 3, amplitude: 0.3 });
  expect((await yap.invoke('e2e_mic_status')).last.device).toBe(BUDS.name);
  const tip = toast(main, 'Bluetooth headsets sound worse while their mic is on');
  await expect(tip).toBeVisible({ timeout: 10_000 });
  await expect(tip).toContainText(/try the PC's mic/i);
  await shot(main, '02-bluetooth');
  await tip.getByRole('button', { name: 'Microphones' }).click();
  await expect(settingsDialog(main).locator('[data-setting="microphone"]')).toBeVisible();
  await closeSettings(main);
});

test("a Bluetooth headset with the lid closed: never told to try the laptop's own mic", async ({ yap, main }) => {
  await setUp(yap, main, [LAPTOP, BUDS], [BUDS]);
  await yap.invoke('e2e_lid', { closed: true });
  await yap.invoke('e2e_dictate_audio', { seconds: 3, amplitude: 0.3 });
  expect((await yap.invoke('e2e_mic_status')).last.device).toBe(BUDS.name);
  await main.waitForTimeout(2_500);
  await expect(toast(main, /try the PC's mic/i)).toHaveCount(0);
});

test("the lid closed: the laptop's own mic gives way to the next one, and comes back when it opens", async ({ yap, main }) => {
  await setUp(yap, main, [LAPTOP, USB], [LAPTOP, USB]);
  await yap.invoke('e2e_lid', { closed: true });
  await yap.invoke('e2e_dictate_audio', { seconds: 1 });
  expect((await yap.invoke('e2e_mic_status')).last.device).toBe(USB.name);
  await expect(toast(main, 'lid is closed')).toHaveCount(0);

  await yap.invoke('e2e_lid', { closed: false });
  await expect.poll(async () => (await yap.invoke('e2e_mic_status')).open, { timeout: 5_000 }).toBe(false);
  await yap.invoke('e2e_dictate_audio', { seconds: 1 });
  expect((await yap.invoke('e2e_mic_status')).last.device).toBe(LAPTOP.name);
});

test("the lid closed and the laptop's mic all there is: said once, with Microphones", async ({ yap, main, shot }) => {
  await setUp(yap, main, [LAPTOP], []);
  await yap.invoke('e2e_lid', { closed: true });
  await yap.invoke('e2e_dictate_audio', { seconds: 1 });
  const alert = toast(main, "Your laptop's lid is closed, so its microphone can't hear you");
  await expect(alert).toBeVisible({ timeout: 10_000 });
  await shot(main, '03-lid-closed');
  await alert.getByRole('button', { name: 'Microphones' }).click();
  await expect(settingsDialog(main).locator('[data-setting="microphone"]')).toBeVisible();
  await closeSettings(main);
  // Once per closing.
  await yap.invoke('e2e_dictate_audio', { seconds: 1 });
  await main.waitForTimeout(1_500);
  await expect(alert).toHaveCount(0);
});

test("the lid closed while the laptop's mic is kept ready: said at the next dictation too", async ({ yap, main }) => {
  await setUp(yap, main, [LAPTOP], []);
  await setConfig(yap, main, { keepMicReady: true });
  try {
    // Open before the lid closes, and never moved off it (it's all there is).
    await expect.poll(async () => (await yap.invoke('e2e_mic_status')).open, { timeout: 5_000 }).toBe(true);
    await yap.invoke('e2e_lid', { closed: true });
    await yap.invoke('e2e_dictate_audio', { seconds: 1 });
    expect((await yap.invoke('e2e_mic_status')).last.device).toBe(LAPTOP.name);
    await expect(toast(main, "Your laptop's lid is closed, so its microphone can't hear you")).toBeVisible({ timeout: 10_000 });
  } finally {
    await setConfig(yap, main, { keepMicReady: false });
  }
});
