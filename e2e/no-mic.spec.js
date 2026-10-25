// Yap without a microphone (none plugged in, a headset that connects after
// login, a CI runner): it starts and stays usable, says so when asked to
// record, and Upload still transcribes files. Made-up microphones, none of
// them connected (`YAP_E2E_MICS`, dictation/mics.rs), force this path on any
// machine (a configured one that isn't there falls back to Windows' default
// since 25 Oct).
import path from 'node:path';
import { test, expect, openView, pressHotkey } from './support/fixtures.js';
import { writeWav } from './support/wav.js';

test.use({
  yapOptions: { name: 'no-mic', env: { YAP_E2E_MICS: '[]' }, config: { inputDevice: 'Missing microphone (e2e)' } },
});

test('starts without a microphone and says so when asked to record', async ({ yap, main, shot }) => {
  await expect(main.getByRole('heading', { level: 1 })).toContainText('to start yapping');

  await pressHotkey(main);
  // A dictation's own error is said where the dictation is — the overlay —
  // and only there (notices/notice.rs: one place; no toast in the main window).
  await expect(yap.overlay.getByText('No microphone found')).toBeVisible();
  await shot(yap.overlay, '02-overlay-no-microphone');
  await expect(main.getByRole('status').filter({ hasText: 'No microphone found' })).toHaveCount(0);
});

// No device because Windows' privacy switches block the microphone: Yap
// says that, with Open settings, instead of "No microphone found"
// (dictation/mic_privacy.rs; a test run reads no registry — `e2e_mic_privacy`).
test("when Windows' privacy settings block the mic, it says so instead", async ({ yap, main, shot }) => {
  await yap.invoke('e2e_mic_privacy', { blocked: 'device' });
  try {
    await pressHotkey(main);
    const toast = main.getByRole('status').filter({ hasText: 'Windows is blocking microphone access for desktop apps' });
    await expect(toast).toBeVisible();
    await expect(toast).toContainText('Turn on “Microphone access”');
    await expect(toast.getByRole('button', { name: 'Open settings' })).toBeVisible();
    await expect(yap.overlay.getByText('Windows is blocking the microphone')).toBeVisible();
    await shot(yap.overlay, '06-overlay-mic-blocked'); // (the error clears after 4 s)
    await expect(main.getByRole('status').filter({ hasText: 'No microphone found' })).toHaveCount(0);
    await shot(main, '05-toast-mic-blocked');
  } finally {
    await yap.invoke('e2e_mic_privacy', { blocked: null });
  }
});

test('Upload still transcribes a file', async ({ yap, main, shot }) => {
  await openView(main, 'Upload');
  const wav = writeWav(path.join(yap.runDir, 'voice-memo.wav'));
  // CDP can't drag a file in from Explorer: hand the webview the same
  // drag-drop event Tauri sends for a real drop. (Repeated until the view's
  // drop listener, registered asynchronously on mount, picks it up.)
  await expect(async () => {
    await yap.invoke('plugin:event|emit', {
      event: 'tauri://drag-drop',
      payload: { paths: [wav], position: { x: 600, y: 400 } },
    });
    await expect(main.getByText('voice-memo.wav')).toBeVisible({ timeout: 1_000 });
  }).toPass();
  await main.getByRole('button', { name: 'Transcribe', exact: true }).click();

  await expect(main.getByRole('heading', { name: 'Transcript' })).toBeVisible({ timeout: 20_000 });
  await expect(main.getByText(/\[STT stub: received 1\.5s of audio/)).toBeVisible();
  await shot(main, '03-upload-transcribed');
});
