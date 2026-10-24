// Upload's job outlives its view (upload.svelte.js, code review 2026-10-23
// #8): leaving Upload mid-job used to unmount it with its progress, Cancel
// and result. The stub engine transcribes a chunk at once, so
// `e2e_upload_pace` holds each chunk a moment: the job is still going when
// the view comes back.
import path from 'node:path';
import { test, expect, openView } from './support/fixtures.js';
import { writeWav } from './support/wav.js';

test.use({ yapOptions: { name: 'upload' } });

/** Drop `file` on the Upload view, as Tauri reports a real drop. */
async function drop(yap, main, file) {
  await expect(async () => {
    await yap.invoke('plugin:event|emit', {
      event: 'tauri://drag-drop',
      payload: { paths: [file], position: { x: 600, y: 400 } },
    });
    await expect(main.getByText(path.basename(file))).toBeVisible({ timeout: 1_000 });
  }).toPass();
}

test('leaving Upload mid-job keeps its progress, Cancel and result', async ({ yap, main, shot }) => {
  // ~3 minutes of audio: three ~60 s chunks, each held 1.5 s.
  const wav = writeWav(path.join(yap.runDir, 'garden-walk.wav'), { seconds: 170 });
  await yap.invoke('e2e_upload_pace', { ms: 1_500 });
  try {
    await openView(main, 'Upload');
    await drop(yap, main, wav);
    await main.getByRole('button', { name: 'Transcribe', exact: true }).click();
    await expect(main.getByText('Transcribing… chunk 1 of 3')).toBeVisible({ timeout: 15_000 });

    await openView(main, 'Home');
    await openView(main, 'Upload');
    // Still going, as it was: the file, its progress and Cancel.
    await expect(main.getByText('garden-walk.wav')).toBeVisible();
    await expect(main.getByText(/Transcribing… chunk \d of 3/)).toBeVisible();
    await expect(main.getByRole('button', { name: 'Cancel' })).toBeVisible();
    await shot(main, '01-progress-kept');

    // Away again until it's done: the toast still comes, the result waits.
    await openView(main, 'Home');
    await expect(main.getByRole('status').filter({ hasText: 'Transcription complete' })).toBeVisible({ timeout: 20_000 });
    await openView(main, 'Upload');
    await expect(main.getByRole('heading', { name: 'Transcript' })).toBeVisible();
    await expect(main.getByText(/\[STT stub: received 60\.\ds of audio/).first()).toBeVisible();
    await shot(main, '02-result-kept');
    await main.getByRole('button', { name: 'Transcribe another file' }).click();
  } finally {
    await yap.invoke('e2e_upload_pace', { ms: 0 });
  }
});

test('Cancel still works after coming back to Upload', async ({ yap, main }) => {
  const wav = writeWav(path.join(yap.runDir, 'long-memo.wav'), { seconds: 170 });
  await yap.invoke('e2e_upload_pace', { ms: 2_000 });
  try {
    await openView(main, 'Upload');
    await drop(yap, main, wav);
    await main.getByRole('button', { name: 'Transcribe', exact: true }).click();
    await expect(main.getByText('Transcribing… chunk 1 of 3')).toBeVisible({ timeout: 15_000 });
    await openView(main, 'Notes');
    await openView(main, 'Upload');
    await main.getByRole('button', { name: 'Cancel' }).click();
    // Back to the file, ready to go again; no result, no toast.
    await expect(main.getByRole('button', { name: 'Transcribe', exact: true })).toBeVisible({ timeout: 10_000 });
    await expect(main.getByRole('heading', { name: 'Transcript' })).toHaveCount(0);
    await expect(main.getByRole('status').filter({ hasText: 'Transcription complete' })).toHaveCount(0);
    await main.getByRole('button', { name: 'Remove file' }).click();
  } finally {
    await yap.invoke('e2e_upload_pace', { ms: 0 });
  }
});
