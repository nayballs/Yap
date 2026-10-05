// Meetings recorded in a Yap without an AI model: the recorder's live
// You/Them transcript, the end-of-meeting summary step explaining how to set
// up AI instead of failing, and the call's audio leaking into the mic
// (speakers, no headphones) kept out of the "You" side. No audio device is
// opened: in test mode the recorder plays you.wav / them.wav from a folder
// (src-tauri/src/e2e.rs), here 4× faster than real time. The long-meeting
// summary is meeting-summary.spec.js.
import path from 'node:path';
import fs from 'node:fs';
import { test, expect, settingsDialog, closeSettings, expectStore } from './support/fixtures.js';
import { newMeetingNote, meetingNote } from './support/meetings.js';
import { writeWav } from './support/wav.js';
import { RUNS } from './support/yap.js';

const audioDir = path.join(RUNS, 'meeting-audio');

test.use({
  yapOptions: {
    name: 'meeting-noai',
    env: { YAP_E2E_MEETING_AUDIO: audioDir, YAP_E2E_MEETING_SPEED: '4' },
  },
});

/**
 * The next recording's audio: 20 s of tone per side, silent in `gaps`
 * ([[fromSec, toSec], …]). Each side pauses in 15.7–19.7 s, where the
 * recorder cuts its first chunk (the stub engine only reports durations).
 */
function meetingAudio({ you, them, youAmplitude = 3000 }) {
  fs.mkdirSync(audioDir, { recursive: true });
  writeWav(path.join(audioDir, 'you.wav'), { seconds: 20, gaps: you, amplitude: youAmplitude });
  writeWav(path.join(audioDir, 'them.wav'), { seconds: 20, gaps: them });
}

test('live You/Them segments, then the summary step says how to set up AI', async ({
  yap,
  main,
  shot,
}) => {
  // Two people taking turns: their pauses never line up.
  meetingAudio({
    you: [[3, 3.3], [8, 8.3], [16, 16.4]],
    them: [[5.5, 5.8], [11, 11.3], [16.8, 17.2]],
  });
  await newMeetingNote(main, 'Standup', ['Priya']);
  await main.getByRole('button', { name: 'Record' }).click();
  await expect(main.getByRole('button', { name: 'End meeting & summarise' })).toBeVisible();

  // Chunks are cut every ~15 s of audio, in a pause, and transcribed.
  const log = main.getByRole('log', { name: 'Meeting transcript' });
  await expect(log.locator('.bubble.you')).toHaveCount(1, { timeout: 20_000 });
  await expect(log.locator('.bubble.them')).toHaveCount(1);
  await expect(log.locator('.bubble.you')).toContainText(/\[STT stub: received 1\d\.\ds of audio/);
  await shot(main, '01-live-you-them-segments');

  await main.getByRole('button', { name: 'End meeting & summarise' }).click();
  // The last few seconds are transcribed, then the summary step needs AI.
  const card = main.locator('.aicard');
  await expect(card).toContainText('Your meeting is saved.', { timeout: 20_000 });
  await expect(card).toContainText('No groq API key');
  await expect(main.getByRole('button', { name: 'Resume' })).toBeVisible();
  await expect(log.locator('.bubble')).toHaveCount(4);
  await expectStore(yap, 'notes.json', (s) => meetingNote(s, 'Standup')?.transcript?.length === 4);
  const note = meetingNote(yap.readJson('notes.json'), 'Standup');
  expect(note.noteType).toBe('meeting');
  expect(note.enhancedContent ?? '').toBe('');
  expect(note.transcript.some((s) => s.echo)).toBe(false);
  // Segments are dated by when their audio was captured, in order.
  const ts = note.transcript.map((s) => s.ts);
  expect(ts).toEqual([...ts].sort((a, b) => a - b));
  await shot(main, '02-no-ai-explains-setup');

  await card.getByRole('button', { name: 'Open Language Models' }).click();
  await expect(
    settingsDialog(main).getByRole('heading', { level: 1, name: 'Language Models' })
  ).toBeVisible();
  await closeSettings(main);
});

test('the call leaking from the speakers into the mic is kept out of "You"', async ({
  yap,
  main,
  shot,
}) => {
  // The mic hears the call 120 ms late and quieter: same pauses, shifted.
  const them = [[2, 2.3], [4.5, 4.8], [7, 7.2], [9.5, 9.9], [12, 12.3], [16, 16.4]];
  meetingAudio({
    them,
    you: them.map(([a, b]) => [a + 0.12, b + 0.12]),
    youAmplitude: 900,
  });
  await newMeetingNote(main, 'On speakers');
  await main.getByRole('button', { name: 'Record' }).click();

  const log = main.getByRole('log', { name: 'Meeting transcript' });
  await expect(log.locator('.bubble.them')).toHaveCount(1, { timeout: 20_000 });
  const toggle = main.getByRole('button', { name: /Show 1 line your mic picked up/ });
  await expect(toggle).toBeVisible();
  await expect(log.locator('.bubble.you')).toHaveCount(0);
  await shot(main, '03-echo-hidden');

  await toggle.click();
  await expect(log.locator('.bubble.you.echo')).toContainText('from the speakers');
  await shot(main, '04-echo-shown');

  await main.getByRole('button', { name: 'Pause recording' }).click();
  await expect(main.getByRole('button', { name: 'Resume' })).toBeVisible({ timeout: 20_000 });
  // Kept in the note (nothing is lost), flagged so summaries skip it.
  await expectStore(yap, 'notes.json', (s) =>
    meetingNote(s, 'On speakers')?.transcript?.some((seg) => seg.source === 'you' && seg.echo)
  );
  // A pause doesn't summarise.
  await expect(main.locator('.aicard')).toHaveCount(0);
});
