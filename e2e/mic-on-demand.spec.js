// The dictation's microphone opens on demand (mic.rs, 2026-10-22): closed
// while Yap idles, opened by a dictation, kept warm a moment after (here
// YAP_MIC_WARM_MS, 2.5 s, instead of a minute), then closed — and kept open
// by "Keep the microphone ready" or onboarding's mic check. The dictations
// here play a tone in the mic's place (`e2e_dictate_audio`) but hold and
// open the machine's real microphone as any dictation does, so those tests
// skip without one (CI). The model loading in the background at startup:
// model-load.spec.js.
import { test, expect, openSettings, settingsDialog, closeSettings, expectStore } from './support/fixtures.js';

const WARM_MS = 2_500;

const mic = (yap) => yap.invoke('e2e_mic_status');

async function needsMic(yap) {
  const mics = await yap.invoke('list_audio_devices');
  test.skip(mics.length === 0, 'no microphone here');
}

test.use({ yapOptions: { name: 'mic-on-demand', env: { YAP_MIC_WARM_MS: String(WARM_MS) } } });

test('is closed while Yap idles', async ({ yap }) => {
  expect(await mic(yap)).toMatchObject({ open: false, opens: 0, warmMs: WARM_MS });
});

test('a dictation opens it; it stays warm a moment for the next one, then closes', async ({ yap }) => {
  await needsMic(yap);
  const before = await mic(yap);
  const first = await yap.invoke('e2e_dictate_audio', { seconds: 1.5 });
  expect(first?.raw).toContain('STT stub');
  let now = await mic(yap);
  expect(now.opens).toBe(before.opens + 1);
  // How the open went: asked at the recording's start, its first audio
  // soon after (a USB mic: tens of ms).
  expect(now.last.openMs).toBeLessThan(2_000);
  expect(now.last.firstAudioMs).toBeLessThan(3_000);
  expect(now.last.bluetooth).toBe(false);
  // Warm: the next dictation starts on the open stream.
  expect(now.open).toBe(true);
  const second = await yap.invoke('e2e_dictate_audio', { seconds: 1 });
  expect(second?.raw).toContain('STT stub');
  now = await mic(yap);
  expect(now.opens).toBe(before.opens + 1);
  // Nothing for the warm window: closed.
  await expect.poll(async () => (await mic(yap)).open, { timeout: WARM_MS + 5_000 }).toBe(false);
  expect((await mic(yap)).closes).toBe(before.closes + 1);
});

test('onboarding\'s mic check holds it open while it runs', async ({ yap }) => {
  await needsMic(yap);
  await yap.invoke('set_mic_test', { on: true });
  await expect.poll(async () => (await mic(yap)).open, { timeout: 5_000 }).toBe(true);
  // Its level meter is fed while nothing records.
  await yap.invoke('set_mic_test', { on: false });
  await expect.poll(async () => (await mic(yap)).open, { timeout: WARM_MS + 5_000 }).toBe(false);
});

test('"Keep the microphone ready" keeps it open; off again, it closes', async ({ yap, main, shot }) => {
  await openSettings(main, 'Dictation');
  const row = settingsDialog(main).locator('[data-setting="keep-mic-ready"]');
  await expect(row).toContainText('Keep the microphone ready');
  await expect(row).toContainText('Dictation starts instantly and catches your first word');
  const toggle = row.getByRole('button', { name: 'Keep the microphone ready' });
  await expect(toggle).toHaveAttribute('aria-pressed', 'false');
  await row.scrollIntoViewIfNeeded();
  await shot(main, '01-keep-mic-ready-off');

  await toggle.click();
  await expectStore(yap, 'config.json', (c) => c.keepMicReady === true);
  const mics = await yap.invoke('list_audio_devices');
  if (mics.length) await expect.poll(async () => (await mic(yap)).open, { timeout: 5_000 }).toBe(true);
  await shot(main, '02-keep-mic-ready-on');

  await toggle.click();
  await expectStore(yap, 'config.json', (c) => c.keepMicReady === false);
  await expect.poll(async () => (await mic(yap)).open, { timeout: 5_000 }).toBe(false);
  await closeSettings(main);
});

test('Settings search finds it by what it does', async ({ main }) => {
  await openSettings(main, 'Dictation');
  const search = settingsDialog(main).getByRole('searchbox', { name: 'Search settings' });
  await search.fill('bluetooth');
  const found = settingsDialog(main).getByRole('listbox', { name: 'Settings found' });
  await expect(found.getByRole('option').first()).toContainText('Keep the microphone ready');
  await search.fill('');
  await closeSettings(main);
});
