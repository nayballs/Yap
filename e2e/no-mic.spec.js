// Yap without a microphone (none plugged in, a headset that connects after
// login, a CI runner): it starts and stays usable, says so when asked to
// record, and Upload still transcribes files. A configured microphone that
// isn't there forces this path on any machine.
import path from 'node:path';
import { test, expect, openView, pressHotkey } from './support/fixtures.js';
import { writeWav } from './support/wav.js';

test.use({ yapOptions: { name: 'no-mic', config: { inputDevice: 'Missing microphone (e2e)' } } });

test('starts without a microphone and says so when asked to record', async ({ yap, main, shot }) => {
  await expect(main.getByRole('heading', { level: 1 })).toContainText('to start yapping');

  await pressHotkey(main);
  const toast = main.getByRole('status').filter({ hasText: 'No microphone found' });
  await expect(toast).toBeVisible();
  await expect(yap.overlay.getByText('No microphone found')).toBeVisible();
  await shot(main, '01-toast-no-microphone');
  await shot(yap.overlay, '02-overlay-no-microphone');
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
