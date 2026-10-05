// A meeting recorded without an AI model: the recorder's live You/Them
// transcript, then the end-of-meeting summary step explaining how to set up
// AI instead of failing. No audio device is opened: in test mode the
// recorder plays you.wav / them.wav (src-tauri/src/e2e.rs), here 4× faster
// than real time. The long-meeting summary is meeting-summary.spec.js.
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

test.beforeAll(() => {
  // 20 s of "you" and of "them" with a pause at 16 s, where the recorder
  // cuts its first chunk (the stub engine only reports durations).
  fs.mkdirSync(audioDir, { recursive: true });
  for (const who of ['you', 'them']) {
    writeWav(path.join(audioDir, `${who}.wav`), { seconds: 20, gaps: [[16, 16.4]] });
  }
});

test('live You/Them segments, then the summary step says how to set up AI', async ({
  yap,
  main,
  shot,
}) => {
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
