// A meeting's microphone unplugged mid-recording (meetings/meeting.rs,
// "Following the microphone"; dictation/mics.rs): "You" moves to the next
// microphone in the order (else Windows' default) and the recording goes
// on — the gap silent, a "Your mic was disconnected" marker in the
// transcript where it happened, and the person told.
//
// No audio device is opened: the recorder plays you.wav / them.wav, 4× real
// time (shell/e2e.rs), and follows the made-up microphones (`YAP_E2E_MICS`,
// `e2e_mic_unplug`) as the real capture follows Windows' list.
import fs from 'node:fs';
import path from 'node:path';
import { test, expect, expectStore } from './support/fixtures.js';
import { newMeetingNote, meetingNote } from './support/meetings.js';
import { writeWav } from './support/wav.js';
import { RUNS } from './support/yap.js';

const audioDir = path.join(RUNS, 'meeting-mic-wavs');
const LAPTOP = { id: 'laptop', name: 'Microphone Array (Northwind Audio)' };
const USB = { id: 'usb', name: 'Microphone (Fabrikam USB Mic)' };

test.use({
  yapOptions: {
    name: 'meeting-mic',
    env: { YAP_E2E_MEETING_AUDIO: audioDir, YAP_E2E_MEETING_SPEED: '4', YAP_E2E_MICS: JSON.stringify([LAPTOP, USB]) },
    config: { micOrder: [{ id: `e2e-${USB.id}`, name: USB.name }] },
  },
});

const recording = (yap) => yap.invoke('meeting_state').then((s) => s.recording);
const transcriptLog = (main) => main.getByRole('log', { name: 'Meeting transcript' });
const logCount = (yap, text) => fs.readFileSync(yap.logPath, 'utf8').split(text).length - 1;

test.afterEach(async ({ yap }) => {
  if (await recording(yap)) {
    await yap.invoke('meeting_pause');
    await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
  }
});

test('the mic unplugged mid-meeting: "You" goes on with the next one, marked in the transcript', async ({ yap, main, shot }) => {
  fs.mkdirSync(audioDir, { recursive: true });
  writeWav(path.join(audioDir, 'you.wav'), { seconds: 30, gaps: [[4.5, 4.8], [9.5, 9.9], [14.2, 14.6], [19.5, 19.9], [24.5, 24.9]] });
  writeWav(path.join(audioDir, 'them.wav'), { seconds: 30, gaps: [[2, 3], [7, 8], [12, 13], [17, 18], [22, 23]] });
  await newMeetingNote(main, 'Headset unplugged');
  await main.getByRole('button', { name: 'Record' }).click();
  await expect.poll(() => recording(yap), { timeout: 10_000 }).toBe(true);
  await main.waitForTimeout(1_500); // ~6 s of the meeting

  await yap.invoke('e2e_mic_unplug', { id: USB.id });
  await expect
    .poll(() => logCount(yap, 'e2e: "You" goes on with the next microphone'), { message: 'the recorder moved to the next mic', timeout: 10_000 })
    .toBe(1);
  // Told where the person is looking, and the recording goes on.
  const notice = main.getByRole('status').filter({ hasText: 'Your microphone was disconnected' });
  await expect(notice).toBeVisible({ timeout: 10_000 });
  await expect(notice).toContainText('Yap is recording you with Northwind Audio now.');
  expect(await recording(yap)).toBe(true);

  // The marker, in the transcript where it happened.
  const log = transcriptLog(main);
  const marker = log.getByRole('separator', { name: 'Your mic was disconnected' });
  await expect(marker).toBeVisible({ timeout: 30_000 });
  await expect(marker).toHaveText('Your mic was disconnected · now Northwind Audio');
  await shot(main, '01-meeting-mic-switched');

  await expect
    .poll(() => logCount(yap, 'e2e: the test audio has played to the end'), { timeout: 30_000 })
    .toBeGreaterThan(0);
  await main.getByRole('button', { name: 'Pause recording' }).click();
  await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
  await expectStore(yap, 'notes.json', (s) => meetingNote(s, 'Headset unplugged')?.transcript?.some((seg) => seg.mic != null));
  const t = meetingNote(yap.readJson('notes.json'), 'Headset unplugged').transcript;
  const marks = t.filter((seg) => seg.mic != null);
  expect(marks).toEqual([{ source: 'you', text: '', ts: marks[0].ts, mic: LAPTOP.name }]);
  // "You" talked before and after it.
  const you = t.filter((seg) => seg.source === 'you' && seg.mic == null && seg.text);
  expect(you.length).toBeGreaterThanOrEqual(1);
  expect(t.some((seg) => seg.source === 'them' && seg.text)).toBe(true);
});
