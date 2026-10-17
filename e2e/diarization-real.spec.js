// The real speaker engine (Nemotron-3-Diarization on NeMo-Speech.cpp, as
// `yap.exe diarize`), driven through Yap: `diarize_wav` on a test meeting
// scored against its reference turns, and a meeting recording whose "Them"
// side is that meeting, labelled while it records and finished when it
// stops (diarize.rs, meeting.rs's tee).
//
// Skipped unless the model and runtime are on disk (it never downloads them):
//   YAP_E2E_DIARIZATION_MODELS  a folder laid out like <data>/models/diarization
//                               (nemotron-3-diarization/, nemo-speech-0.2.0-cpu/)
//   YAP_E2E_DIARIZATION_AUDIO   a folder of <name>.wav + <name>.rttm (the
//                               diarization spike's audio/conv)
import fs from 'node:fs';
import path from 'node:path';
import { test, expect, openView } from './support/fixtures.js';
import { newMeetingNote, meetingNote } from './support/meetings.js';
import { readRttm, frameError } from './support/diarization.js';
import { writeWav } from './support/wav.js';
import { RUNS } from './support/yap.js';

const MODELS = process.env.YAP_E2E_DIARIZATION_MODELS;
const AUDIO = process.env.YAP_E2E_DIARIZATION_AUDIO;
const FILE = process.env.YAP_E2E_DIARIZATION_FILE || 's3_clean';
const meetingDir = path.join(RUNS, 'diarization-real-wavs');

test.skip(!MODELS || !AUDIO, 'needs YAP_E2E_DIARIZATION_MODELS and YAP_E2E_DIARIZATION_AUDIO');

test.use({
  yapOptions: {
    name: 'diarization-real',
    env: {
      YAP_DIARIZATION_MODELS: MODELS ?? '',
      YAP_E2E_MEETING_AUDIO: meetingDir,
      YAP_E2E_MEETING_SPEED: '20',
    },
    config: { meetingDiarization: true },
  },
});

const voices = (segments) => new Set(segments.map((s) => `${s.stream ?? 0}:${s.speaker}`)).size;
const asSecs = (segments) => segments.map((s) => ({ start: s.startMs / 1000, end: s.endMs / 1000, speaker: `${s.stream ?? 0}:${s.speaker}` }));

test('diarize_wav runs the real engine and finds the voices', async ({ yap }) => {
  test.setTimeout(120_000);
  const ref = readRttm(path.join(AUDIO, `${FILE}.rttm`));
  const result = await yap.invoke('diarize_wav', { path: path.join(AUDIO, `${FILE}.wav`) });
  const err = frameError(ref, asSecs(result.segments));
  const people = new Set(ref.map((s) => s.speaker)).size;
  console.log(
    `${FILE}: ${result.speakers} voices for ${people}, error ${(err * 100).toFixed(1)}%, ` +
      `${(result.audioMs / 1000).toFixed(0)} s of audio in ${(result.computeMs / 1000).toFixed(1)} s ` +
      `(RTF ${result.rtf.toFixed(3)}, ${result.threads} threads), labels ${result.lagMsMedian} / ${result.lagMsMax} ms behind`
  );
  expect(result.speakers).toBe(people);
  expect(err).toBeLessThan(0.1);
  expect(result.rtf).toBeLessThan(0.2);
  // Speakers 1… in the order they first spoke.
  const order = [];
  for (const s of result.segments) if (!order.includes(s.speaker)) order.push(s.speaker);
  expect(order).toEqual(order.map((_, i) => i + 1));
});

test('a meeting recording labels "Them" as it goes and finishes when it stops', async ({ yap, main, shot }) => {
  test.setTimeout(180_000);
  fs.mkdirSync(meetingDir, { recursive: true });
  fs.copyFileSync(path.join(AUDIO, `${FILE}.wav`), path.join(meetingDir, 'them.wav'));
  writeWav(path.join(meetingDir, 'you.wav'), { seconds: 2, gaps: [[0, 2]] });
  const ref = readRttm(path.join(AUDIO, `${FILE}.rttm`));
  const people = new Set(ref.map((s) => s.speaker)).size;

  await newMeetingNote(main, 'Three voices');
  await main.getByRole('button', { name: 'Record' }).click();
  await expect.poll(() => yap.invoke('meeting_state').then((s) => s.recording)).toBe(true);
  const noteId = await expect
    .poll(() => meetingNote(yap.readJson('notes.json'), 'Three voices')?.id ?? null)
    .not.toBeNull()
    .then(() => meetingNote(yap.readJson('notes.json'), 'Three voices').id);

  // Live labels while it records (the engine trails by up to ~30 s of audio).
  await expect
    .poll(async () => (await yap.invoke('diarization_timeline', { noteId }))?.segments?.length ?? 0, { timeout: 60_000 })
    .toBeGreaterThan(0);
  const live = await yap.invoke('diarization_timeline', { noteId });
  expect(live.done).toBe(false);
  expect(live.labelledMs).toBeLessThanOrEqual(live.audioMs);

  // Played out (6 min at 20x) → pause → the final timeline.
  const played = () => fs.readFileSync(yap.logPath, 'utf8').includes('e2e: the test audio has played to the end');
  await expect.poll(played, { timeout: 60_000 }).toBe(true);
  await yap.invoke('meeting_pause');
  await expect.poll(() => yap.invoke('meeting_state').then((s) => s.recording), { timeout: 30_000 }).toBe(false);
  const done = await yap.invoke('diarization_timeline', { noteId });
  expect(done.done).toBe(true);
  expect(done.error).toBeNull();
  expect(done.restarts).toBe(0);
  expect(done.labelledMs).toBe(done.audioMs);
  // The recorder keeps going a moment past the file: the clock is the session's.
  expect(done.audioMs).toBeGreaterThanOrEqual(Math.floor(ref.at(-1).end * 1000));
  const err = frameError(ref, asSecs(done.segments));
  console.log(`meeting: ${voices(done.segments)} voices for ${people}, error ${(err * 100).toFixed(1)}%, ${done.segments.length} segments`);
  expect(voices(done.segments)).toBe(people);
  expect(err).toBeLessThan(0.1);

  // The recorder itself is unchanged: "Them" lines as always.
  const note = meetingNote(yap.readJson('notes.json'), 'Three voices');
  expect(note.transcript.some((s) => s.source === 'them')).toBe(true);
  const log = fs.readFileSync(yap.logPath, 'utf8');
  expect(log).toContain('Diarization: engine started');
  expect(log).toContain('Diarization: done');
  await openView(main, 'Notes');
  await shot(main, '01-meeting-with-speaker-engine');
});
