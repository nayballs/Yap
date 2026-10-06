// Call detection when the call can't be recorded (meeting_detect.rs): a
// configured microphone that isn't there makes "Record notes" fail on any
// machine — it must say why and leave no empty meeting note behind. (See
// meeting-detect.spec.js for the flow that records.)
import { test, expect, mainOnScreen } from './support/fixtures.js';

test.use({
  yapOptions: { name: 'meeting-detect-no-mic', config: { inputDevice: 'Missing microphone (e2e)' } },
});

const toast = (main, title) => main.getByRole('status').filter({ hasText: title });

test('"Record notes" says why it can\'t record, and leaves no empty note', async ({ yap, main, shot }) => {
  await mainOnScreen(yap); // the prompt goes in-app only while it's on screen
  await yap.invoke('meeting_detect_simulate', { appId: 'webex', active: true });
  const prompt = toast(main, 'Webex call detected');
  await expect(prompt).toBeVisible();
  await prompt.getByRole('button', { name: 'Record notes' }).click();

  const failed = toast(main, "Couldn't record the call");
  await expect(failed).toBeVisible({ timeout: 15_000 });
  await expect(failed).toContainText('Missing microphone (e2e)');
  expect(await yap.invoke('meeting_state')).toMatchObject({ recording: false });
  const notes = yap.readJson('notes.json')?.notes ?? [];
  expect(notes.filter((n) => n.source === 'meeting')).toHaveLength(0);
  await shot(main, '01-record-failed-no-microphone');

  await yap.invoke('meeting_detect_simulate', { appId: 'webex', active: false });
  expect((await yap.invoke('meeting_detect_status')).prompt).toBeNull();
});
