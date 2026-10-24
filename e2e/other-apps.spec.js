// Other apps' sound while you dictate, in Settings → Dictation (dictation/mute.rs,
// dictation/media_pause.rs): "Silence other apps while you dictate" where "Mute while
// recording" was — a config from before carries its choice over (this
// instance starts with the old `muteWhileRecording: true`) — and "Pause
// music and videos while you dictate", off by default. A test run never
// silences or pauses anything: a stub dictation with both on says so in
// the log. (The real thing, on the test's own sound only, is
// e2e-real/other-apps.real.js.)
import fs from 'node:fs';
import { test, expect, openSettings, closeSettings, expectStore, pressHotkey } from './support/fixtures.js';

test.use({ yapOptions: { name: 'other-apps', config: { muteWhileRecording: true } } });

test('the old "Mute while recording" is now "Silence other apps", with "Pause music and videos" beside it', async ({
  yap,
  main,
  shot,
}) => {
  const dialog = await openSettings(main, 'Dictation');
  const silence = dialog.getByRole('button', { name: 'Silence other apps while you dictate' });
  const pause = dialog.getByRole('button', { name: 'Pause music and videos while you dictate' });
  await silence.scrollIntoViewIfNeeded();
  // Carried over from `muteWhileRecording: true`; pausing is new, and off.
  await expect(silence).toHaveAttribute('aria-pressed', 'true');
  await expect(pause).toHaveAttribute('aria-pressed', 'false');
  await expect(dialog.getByText('Mute while recording')).toHaveCount(0);
  await expect(dialog).toContainText('Music, videos and games go quiet, then come back as they were');
  await expect(dialog).toContainText("What's playing pauses, and plays again when you're done");
  await shot(main, '01-settings-other-apps');

  // Saved under the new names; the old one is gone from config.json.
  await pause.click();
  await expectStore(yap, 'config.json', (c) => c.quietOtherApps === true && c.pauseMedia === true && !('muteWhileRecording' in c));
  await closeSettings(main);
});

test('a test run never silences or pauses another app, even with both on', async ({ yap, main }) => {
  const mics = await yap.invoke('list_audio_devices');
  test.skip(mics.length === 0, 'no microphone here; no-mic.spec.js covers that path');
  const cfg = await yap.invoke('get_config');
  await yap.invoke('save_config', { cfg: { ...cfg, quietOtherApps: true, pauseMedia: true } });

  const mark = fs.readFileSync(yap.logPath, 'utf8').length;
  await pressHotkey(main);
  await expect(yap.overlay.locator('.capsule')).toBeVisible();
  await main.waitForTimeout(1_200);
  await pressHotkey(main);
  await expect(main.getByText(/\[STT stub: received [\d.]+s of audio/).first()).toBeVisible({ timeout: 20_000 });
  const log = () => fs.readFileSync(yap.logPath, 'utf8').slice(mark);
  await expect.poll(log).toContain('other apps: left alone in a test run');
  expect(log()).not.toContain('other apps: quieted');
  expect(log()).not.toContain('other apps: paused media');
});
