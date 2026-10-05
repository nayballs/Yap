// Call detection (src-tauri/src/meeting_detect.rs): a call starting offers to
// take notes, "Record notes" creates a meeting note and records it, the call
// ending offers to stop and summarise, and the Settings toggle turns it off.
// Test mode reads no registry: the debug-only `meeting_detect_simulate` hook
// stands in for an app taking or letting go of the mic (no debounce), and a
// recording opens no audio device (it's silent here; e2e.rs), so it runs the
// same with or without a microphone. Each test uses its own call app — "Not
// now" quiets an app for 5 minutes, and the tests share one instance.
import {
  test,
  expect,
  openView,
  openSettings,
  closeSettings,
  closeToasts,
  expectStore,
} from './support/fixtures.js';

test.use({ yapOptions: { name: 'meeting-detect' } });

const toast = (main, title) => main.getByRole('status').filter({ hasText: title });
const simulate = (yap, appId, active) => yap.invoke('meeting_detect_simulate', { appId, active });
const meetingNotes = (yap) => (yap.readJson('notes.json')?.notes ?? []).filter((n) => n.source === 'meeting');

test('a call starting offers to take notes, and "Not now" leaves it alone', async ({ yap, main, shot }) => {
  await simulate(yap, 'teams', true);
  const prompt = toast(main, 'Teams call detected');
  await expect(prompt).toBeVisible();
  await expect(prompt).toContainText("Let people know you're taking notes.");
  await expect(prompt.getByRole('button', { name: 'Record notes' })).toBeVisible();
  await expect(prompt.getByRole('button', { name: 'Not now' })).toBeVisible();
  await shot(main, '01-prompt-call-detected');

  await prompt.getByRole('button', { name: 'Not now' }).click();
  await expect(prompt).toHaveCount(0);
  const status = await yap.invoke('meeting_detect_status');
  expect(status.prompt).toBeNull();
  expect(status.calls.map((c) => c.app)).toEqual(['teams']);
  expect(await yap.invoke('meeting_state')).toMatchObject({ recording: false });
  expect(meetingNotes(yap)).toHaveLength(0);

  // Asked once per call: the call ending and the next Teams call within
  // 5 minutes stay quiet.
  await simulate(yap, 'teams', false);
  await simulate(yap, 'teams', true);
  await main.waitForTimeout(1_000);
  await expect(toast(main, 'Teams call')).toHaveCount(0);
  await simulate(yap, 'teams', false);
  expect((await yap.invoke('meeting_detect_status')).calls).toEqual([]);
});

test('a call that ends before anyone answers takes its prompt with it', async ({ yap, main }) => {
  await simulate(yap, 'zoom', true);
  const prompt = toast(main, 'Zoom call detected');
  await expect(prompt).toBeVisible();
  await simulate(yap, 'zoom', false);
  await expect(prompt).toHaveCount(0);
  expect((await yap.invoke('meeting_detect_status')).prompt).toBeNull();
});

test('"Record notes" records the call into a meeting note; the call ending offers to stop and summarise', async ({
  yap,
  main,
  shot,
}) => {
  await simulate(yap, 'meet', true);
  const prompt = toast(main, 'Google Meet call detected');
  await expect(prompt).toBeVisible();
  await prompt.getByRole('button', { name: 'Record notes' }).click();

  // The new meeting note opens in Notes, recording. (When it can't record,
  // it says why and leaves no note: meeting-detect-no-mic.spec.js.)
  const title = main.getByPlaceholder('Untitled Note');
  await expect(title).toHaveValue(/^Google Meet call · \d{1,2} [A-Z][a-z]{2}, \d{2}:\d{2}$/, {
    timeout: 15_000,
  });
  await expect(main.getByRole('navigation', { name: 'Main' }).getByRole('button', { name: 'Notes', exact: true }))
    .toHaveAttribute('aria-current', 'page');
  await expect(main.getByRole('button', { name: 'End meeting & summarise' })).toBeVisible();
  await expect(toast(main, 'Recording notes')).toBeVisible();
  const notes = meetingNotes(yap);
  expect(notes).toHaveLength(1);
  expect(notes[0]).toMatchObject({ folder: 'Meetings', noteType: 'meeting', source: 'meeting' });
  expect(await yap.invoke('meeting_state')).toMatchObject({ recording: true, noteId: notes[0].id });
  await shot(main, '02-recording-started');

  // The call ends: stop and summarise?
  await simulate(yap, 'meet', false);
  const ended = toast(main, 'Google Meet call ended');
  await expect(ended).toBeVisible();
  await expect(ended.getByRole('button', { name: 'Stop and summarise' })).toBeVisible();
  await expect(ended.getByRole('button', { name: 'Keep recording' })).toBeVisible();
  await shot(main, '03-prompt-call-ended');

  // Stopping happens in the note, as "End meeting & summarise": the action
  // plan step runs (and, the test recording being silent, says there's
  // nothing to summarise yet).
  await ended.getByRole('button', { name: 'Stop and summarise' }).click();
  await expect(toast(main, 'Meeting ended')).toBeVisible();
  await expect.poll(() => yap.invoke('meeting_state').then((s) => s.recording), { timeout: 20_000 }).toBe(false);
  await expect(toast(main, 'Nothing to summarise yet')).toBeVisible({ timeout: 20_000 });
  await expect(title).toHaveValue(notes[0].title);
  await shot(main, '04-stopped-and-summarised');
  expect((await yap.invoke('meeting_detect_status')).prompt).toBeNull();
  await closeToasts(main);
});

test('"Keep recording" carries the notes into a rejoined huddle, which asks again when it ends', async ({
  yap,
  main,
}) => {
  await simulate(yap, 'slack', true);
  await toast(main, 'Slack huddle detected').getByRole('button', { name: 'Record notes' }).click();
  await expect.poll(() => yap.invoke('meeting_state').then((s) => s.recording), { timeout: 15_000 }).toBe(true);
  await simulate(yap, 'slack', false);
  const ended = toast(main, 'Slack huddle ended');
  await ended.getByRole('button', { name: 'Keep recording' }).click();
  await expect(ended).toHaveCount(0);

  // Rejoined: no "detected" prompt (Yap is already recording it)…
  await simulate(yap, 'slack', true);
  await main.waitForTimeout(1_000);
  await expect(toast(main, 'Slack huddle detected')).toHaveCount(0);
  expect(await yap.invoke('meeting_state')).toMatchObject({ recording: true });
  // …and its end asks again.
  await simulate(yap, 'slack', false);
  await toast(main, 'Slack huddle ended').getByRole('button', { name: 'Stop and summarise' }).click();
  await expect.poll(() => yap.invoke('meeting_state').then((s) => s.recording), { timeout: 20_000 }).toBe(false);
  await expect(toast(main, 'Nothing to summarise yet')).toBeVisible({ timeout: 20_000 });
  await closeToasts(main);
});

test('the Settings toggle turns call detection off', async ({ yap, main, shot }) => {
  const dialog = await openSettings(main, 'General');
  const meetings = dialog.getByRole('group', { name: 'Meetings' });
  const toggle = meetings.getByRole('button', { name: 'Detect calls and offer to take notes' });
  await expect(toggle).toHaveAttribute('aria-pressed', 'true');
  await expect(meetings.getByText("Recording a call? Let people know you're taking notes.")).toBeVisible();
  await meetings.scrollIntoViewIfNeeded();
  await shot(main, '05-settings-meetings');

  await toggle.click();
  await expect(toggle).toHaveAttribute('aria-pressed', 'false');
  await expectStore(yap, 'config.json', (c) => c.meetingDetection === false);
  await expect.poll(() => yap.invoke('meeting_detect_status').then((s) => s.enabled)).toBe(false);
  await closeSettings(main);

  await simulate(yap, 'discord', true);
  await main.waitForTimeout(1_000);
  await expect(toast(main, 'Discord call')).toHaveCount(0);
  expect((await yap.invoke('meeting_detect_status')).calls).toEqual([]);

  // Back on: the next call is offered again.
  await openSettings(main, 'General');
  await toggle.click();
  await expectStore(yap, 'config.json', (c) => c.meetingDetection === true);
  await expect.poll(() => yap.invoke('meeting_detect_status').then((s) => s.enabled)).toBe(true);
  await closeSettings(main);
  await simulate(yap, 'discord', true);
  await expect(toast(main, 'Discord call detected')).toBeVisible();
  await simulate(yap, 'discord', false);
  await expect(toast(main, 'Discord call detected')).toHaveCount(0);
  await openView(main, 'Home');
});
