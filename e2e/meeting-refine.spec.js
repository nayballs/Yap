// A meeting's transcript you can trust (meeting.rs "Failed chunks",
// meeting_refine.rs, the recorder's slot), end to end:
// - a chunk that fails to transcribe is a marker, never silence ("Couldn't
//   transcribe 0:00–0:15 · Try again"), the person is told once, and Try
//   again puts that stretch's lines in its place;
// - when a meeting ends it's transcribed again by itself — automatically
//   when it helps (chunks failed: first, then the summary; a more accurate
//   model installed: after the summary, then "Transcript improved · Update
//   summary"), always, or never (Settings → Meetings);
// - a dictation in the middle of that refine is never kept waiting;
// - two meeting starts at the same moment give one recorder.
// No audio device is opened: the recorder plays you.wav / them.wav, 4× real
// time (e2e.rs); `e2e_meeting_fail` makes the live chunks over a stretch
// fail; the stub engine's lines say how much audio each chunk had and which
// model ("engine=parakeet", "engine=cohere"); each refine chunk takes 1.5 s
// (YAP_E2E_REFINE_CHUNK_MS), long enough for a dictation to land in it.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, expectStore, openView, setConfig, closeToasts } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { newMeetingNote, meetingNote } from './support/meetings.js';
import { writeWav } from './support/wav.js';
import { RUNS } from './support/yap.js';

const audioDir = path.join(RUNS, 'meeting-refine-wavs');

const test = base.extend({
  fakeLlm: [
    async ({}, use) => {
      const fake = await startFakeLlm();
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({ fakeLlm }, use) => {
      await use({
        name: 'meeting-refine',
        env: { YAP_E2E_MEETING_AUDIO: audioDir, YAP_E2E_MEETING_SPEED: '4', YAP_E2E_REFINE_CHUNK_MS: '1500' },
        config: {
          // "STT stub" is in every stub line: "Heard" marks the live pass.
          dictionary: [{ from: 'STT stub', to: 'Heard', fuzzy: false }],
          // The meeting's language: English (Cohere and Parakeet speak it).
          selectedLanguage: 'en',
          meetingRefine: 'never',
          llmScopes: {
            noteFormatting: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-notes' },
          },
        },
      });
    },
    { scope: 'worker' },
  ],
});

/** The next recording's audio: `seconds` of tone per side, silent in `you` / `them` ([[from, to], …]). */
function meetingAudio({ seconds = 24, you = [[0, 1], [8, 13], [20, 24]], them = THEM_PAUSES } = {}) {
  fs.mkdirSync(audioDir, { recursive: true });
  writeWav(path.join(audioDir, 'you.wav'), { seconds, gaps: you });
  writeWav(path.join(audioDir, 'them.wav'), { seconds, gaps: them });
}

/** Pauses every ~5 s on the call side, where the recorder cuts. */
const THEM_PAUSES = [
  [4.5, 4.8],
  [9.5, 9.9],
  [14.2, 14.6],
  [19.5, 19.9],
];

const recording = (yap) => yap.invoke('meeting_state').then((s) => s.recording);
const logText = (yap) => fs.readFileSync(yap.logPath, 'utf8');
const logCount = (yap, text) => logText(yap).split(text).length - 1;
const PLAYED = 'e2e: the test audio has played to the end';
const noteOf = (yap, title) => meetingNote(yap.readJson('notes.json'), title);
const jobOf = (yap, noteId) => yap.invoke('meeting_audio_note', { noteId }).then((a) => a.job);
/** Seconds of audio a stub line says it received. */
const secsOf = (seg) => Number(/received ([\d.]+)s/.exec(seg.text)?.[1] ?? NaN);
const sum = (xs) => xs.reduce((a, b) => a + b, 0);
const talk = (t, source) => t.filter((s) => s.source === source && s.text && !s.failed && !s.dictated);

/** A model "installed" for the stub engine (its folder is what Yap looks for). */
const cohere = (yap) => path.join(yap.dataDir, 'models', 'cohere-int8');

/** A new meeting note (with a line typed: never "Started by mistake?"), recorded until its test audio has played. Its id. */
async function record(main, yap, title) {
  await newMeetingNote(main, title);
  await main.getByPlaceholder('Start writing…').fill('Agenda: the delivery dates');
  const before = logCount(yap, PLAYED);
  await main.getByRole('button', { name: 'Record' }).click();
  await expect
    .poll(() => logCount(yap, PLAYED), { message: 'the test audio to play out', timeout: 30_000 })
    .toBeGreaterThan(before);
  return noteOf(yap, title).id;
}

/** End the meeting (as "End meeting & summarise"). */
async function end(yap) {
  await yap.invoke('meeting_end', { origin: 'settings' });
  await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
}

/** The note's action plan written (its summary job done). */
async function summarised(yap, noteId) {
  await expect
    .poll(() => yap.invoke('meeting_summary_status', { noteId }).then((s) => s?.state), { timeout: 30_000 })
    .toBe('done');
}

test.afterEach(async ({ yap }) => {
  await yap.invoke('e2e_meeting_fail', {}).catch(() => {});
  if (await recording(yap)) {
    await yap.invoke('meeting_pause');
    await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
  }
  fs.rmSync(cohere(yap), { recursive: true, force: true });
});

test('a chunk that fails is marked, said once, and Try again fills it', async ({ yap, main, shot }) => {
  await setConfig(yap, main, { meetingRefine: 'never' });
  meetingAudio();
  // The first chunk of each side fails, as after a lost graphics card.
  await yap.invoke('e2e_meeting_fail', { side: 'both', from: 0, to: 10 });
  const toldBefore = logCount(yap, "telling the person part of it couldn't be transcribed");
  const told = main.getByRole('status').filter({ hasText: "Yap couldn't transcribe part of this meeting" });
  const noteId = await record(main, yap, 'Failed chunks');
  await main.getByRole('button', { name: 'Pause recording' }).click();
  await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
  await yap.invoke('e2e_meeting_fail', {});

  // Told once, in one place (the main window: the person is looking at it).
  expect(logCount(yap, "telling the person part of it couldn't be transcribed") - toldBefore).toBe(1);
  await expect(told).toHaveCount(1);
  await expect(told).toContainText('use Try again');

  // A marker over each side's stretch — not silence — the rest transcribed.
  await expectStore(yap, 'notes.json', (s) => (meetingNote(s, 'Failed chunks')?.transcript || []).filter((x) => x.failed).length === 2);
  const live = noteOf(yap, 'Failed chunks').transcript;
  const marks = live.filter((s) => s.failed);
  expect(marks.map((m) => m.source).sort()).toEqual(['them', 'you']);
  for (const m of marks) {
    expect(m.startMs).toBe(0);
    expect(m.endMs).toBeGreaterThan(10_000);
    expect(m.text || '').toBe('');
  }
  const failedSecs = (marks.find((m) => m.source === 'them').endMs - 0) / 1000;
  expect(sum(talk(live, 'them').map(secsOf))).toBeCloseTo(24 - failedSecs, 0);

  // The Notes view says so, with Try again.
  const log = main.getByRole('log', { name: 'Meeting transcript' });
  const failed = log.getByRole('note').filter({ hasText: "Couldn't transcribe" });
  await expect(failed).toHaveCount(2);
  await expect(failed.first()).toContainText(/Couldn't transcribe 0:00–0:\d\d · Try again/);
  await closeToasts(main);
  await shot(main, '01-failed-chunk-marker');
  // So does the notepad.
  await yap.invoke('notepad_open', { noteId });
  const pad = yap.notepad;
  await pad.getByRole('tab', { name: /Transcript/ }).click();
  await expect(pad.getByRole('note').filter({ hasText: "Couldn't transcribe" })).toHaveCount(2);
  await shot(pad, '02-failed-chunk-notepad');

  // Try again on each: that stretch from the audio on disk, in its place.
  await failed.first().getByRole('button', { name: 'Try again' }).click();
  await expectStore(yap, 'notes.json', (s) => (meetingNote(s, 'Failed chunks')?.transcript || []).filter((x) => x.failed).length === 1, 'one stretch filled');
  await expect(failed).toHaveCount(1);
  await failed.first().getByRole('button', { name: 'Try again' }).click();
  await expectStore(yap, 'notes.json', (s) => !(meetingNote(s, 'Failed chunks')?.transcript || []).some((x) => x.failed), 'both filled');
  await expect(failed).toHaveCount(0);
  const filled = noteOf(yap, 'Failed chunks').transcript;
  expect(sum(talk(filled, 'them').map(secsOf))).toBeCloseTo(24, 0);
  expect(talk(filled, 'you').length).toBeGreaterThan(talk(live, 'you').length);
  // In time order: the call's first line is from the start again.
  expect(filled.find((s) => s.source === 'them').startMs).toBeLessThan(1_000);
  await shot(main, '03-tried-again');
  await yap.invoke('meeting_delete', { noteId });
});

test('when a meeting ends, its transcript is improved: automatically when it helps, always, or never', async ({ yap, main, fakeLlm, shot }) => {
  test.setTimeout(240_000);
  const refined = (noteId) => jobOf(yap, noteId).then((j) => (j?.kind === 'refine' ? j.state : null));

  // Never: the live transcript stays, whatever is installed.
  fs.mkdirSync(cohere(yap), { recursive: true });
  await setConfig(yap, main, { meetingRefine: 'never' });
  meetingAudio();
  let noteId = await record(main, yap, 'Never refined');
  await end(yap);
  await summarised(yap, noteId);
  await main.waitForTimeout(1_000);
  expect(await refined(noteId)).toBeNull();
  await yap.invoke('meeting_delete', { noteId });

  // Automatically, with nothing better than the live model: nothing.
  fs.rmSync(cohere(yap), { recursive: true, force: true });
  await setConfig(yap, main, { meetingRefine: 'auto' });
  meetingAudio();
  noteId = await record(main, yap, 'Nothing better');
  await end(yap);
  await summarised(yap, noteId);
  await main.waitForTimeout(1_000);
  expect(await refined(noteId)).toBeNull();
  await yap.invoke('meeting_delete', { noteId });

  // Automatically, with a more accurate model installed (Cohere): the
  // summary first, then the refine on Cohere, then "Update summary".
  fs.mkdirSync(cohere(yap), { recursive: true });
  meetingAudio();
  noteId = await record(main, yap, 'Better model');
  await end(yap);
  await summarised(yap, noteId);
  const plansBefore = fakeLlm.requests.filter((r) => r.kind === 'actionPlan').length;
  await expect.poll(() => refined(noteId), { timeout: 15_000 }).toBe('running');
  await openView(main, 'Notes');
  const line = main.getByRole('status').filter({ hasText: 'Improving the transcript' });
  await expect(line).toBeVisible();
  await expect(line.getByRole('button', { name: 'Cancel' })).toBeVisible();
  await shot(main, '04-improving');
  await expect.poll(() => refined(noteId), { timeout: 60_000 }).toBe('done');
  const better = noteOf(yap, 'Better model');
  expect(talk(better.transcript, 'them').every((s) => s.text.includes('engine=cohere'))).toBe(true);
  expect(sum(talk(better.transcript, 'them').map(secsOf))).toBeCloseTo(24, 0);
  expect(better.enhancedContent).toBeTruthy();
  const offer = main.getByRole('status').filter({ hasText: 'Transcript improved' });
  await expect(offer).toBeVisible();
  await shot(main, '05-update-summary');
  await offer.getByRole('button', { name: 'Update summary' }).click();
  await expect(offer).toHaveCount(0);
  await summarised(yap, noteId);
  await expect.poll(() => fakeLlm.requests.filter((r) => r.kind === 'actionPlan').length).toBeGreaterThan(plansBefore);
  // The new summary read the improved lines.
  const last = fakeLlm.requests.filter((r) => r.kind === 'actionPlan').at(-1);
  expect(String(last.user)).toContain('engine=cohere');
  await yap.invoke('meeting_delete', { noteId });

  // Always: on the live model when it's the best one there is.
  fs.rmSync(cohere(yap), { recursive: true, force: true });
  await setConfig(yap, main, { meetingRefine: 'always' });
  meetingAudio();
  noteId = await record(main, yap, 'Always refined');
  await end(yap);
  await expect.poll(() => refined(noteId), { timeout: 60_000 }).toBe('done');
  const always = await jobOf(yap, noteId);
  expect(always.model).toBe('Parakeet V3');
  expect(logText(yap)).toContain('reason=Always');
  await yap.invoke('meeting_delete', { noteId });

  // Automatically, chunks failed: the refine first, the summary after it.
  await setConfig(yap, main, { meetingRefine: 'auto' });
  meetingAudio();
  await yap.invoke('e2e_meeting_fail', { side: 'them', from: 0, to: 10 });
  noteId = await record(main, yap, 'Holes');
  await end(yap);
  await yap.invoke('e2e_meeting_fail', {});
  await expect.poll(() => jobOf(yap, noteId).then((j) => j?.state === 'running' && j.thenSummary), { timeout: 15_000 }).toBe(true);
  expect(await yap.invoke('meeting_summary_status', { noteId })).toBeNull();
  await openView(main, 'Notes');
  await expect(main.getByRole('status').filter({ hasText: 'Improving the transcript, then the summary' })).toBeVisible();
  await closeToasts(main);
  await shot(main, '06-refine-first');
  await expect.poll(() => refined(noteId), { timeout: 60_000 }).toBe('done');
  await summarised(yap, noteId);
  const holes = noteOf(yap, 'Holes');
  expect(holes.transcript.some((s) => s.failed)).toBe(false);
  expect(sum(talk(holes.transcript, 'them').map(secsOf))).toBeCloseTo(24, 0);
  expect(holes.enhancedContent).toBeTruthy();
  await yap.invoke('meeting_delete', { noteId });
});

test('a dictation in the middle of a refine is never kept waiting', async ({ yap, main }) => {
  const dictate = async () => {
    const t0 = Date.now();
    const out = await yap.invoke('e2e_dictate_audio', { seconds: 2 });
    return { ms: Date.now() - t0, out };
  };
  // How long a dictation takes here, with nothing else going on (the
  // first loads the stub model).
  await dictate();
  const quiet = await dictate();
  expect(quiet.out?.raw).toContain('received');

  await setConfig(yap, main, { meetingRefine: 'always' });
  meetingAudio();
  const noteId = await record(main, yap, 'Busy refine');
  await end(yap);
  await expect
    .poll(() => jobOf(yap, noteId).then((j) => j?.kind === 'refine' && j.state === 'running' && !j.waiting), { timeout: 30_000 })
    .toBe(true);
  // A refine chunk (1.5 s) is under way, holding the warm engine.
  await main.waitForTimeout(500);
  const waitsBefore = logCount(yap, 'Speech engine: in use; waiting');
  const during = await dictate();
  expect(during.out?.raw).toContain('received');
  // Its transcription never waited for the engine: the refine finished its
  // chunk while the dictation recorded, then waited for it.
  expect(logCount(yap, 'Speech engine: in use; waiting') - waitsBefore).toBe(0);
  expect(during.ms).toBeLessThan(quiet.ms + 700);
  // And the refine carried on afterwards.
  await expect.poll(() => jobOf(yap, noteId).then((j) => j?.state), { timeout: 60_000 }).toBe('done');
  await yap.invoke('meeting_delete', { noteId });
});

test('two meeting starts at the same moment give one recorder', async ({ yap, main }) => {
  meetingAudio();
  const a = await yap.invoke('note_create', { title: 'Shortcut start', folder: 'Meetings' });
  const b = await yap.invoke('note_create', { title: 'Prompt start', folder: 'Meetings' });
  const players = logCount(yap, 'e2e: meeting audio from test files');
  const results = await yap.invoke('e2e_meeting_start_twice', { first: a.id, second: b.id });
  const ok = results.filter((r) => 'Ok' in r);
  expect(ok).toHaveLength(1);
  expect(results.find((r) => 'Err' in r).Err).toBe('A meeting is already being recorded');
  const state = await yap.invoke('meeting_state');
  expect(state.recording).toBe(true);
  const winner = results[0] === ok[0] ? a.id : b.id;
  const loser = winner === a.id ? b.id : a.id;
  expect(state.noteId).toBe(winner);
  // One recorder: one capture (the test player), one note's audio.
  expect(logCount(yap, 'e2e: meeting audio from test files') - players).toBe(1);
  expect(fs.existsSync(path.join(yap.dataDir, 'meeting-audio', String(winner)))).toBe(true);
  expect(fs.existsSync(path.join(yap.dataDir, 'meeting-audio', String(loser)))).toBe(false);
  await yap.invoke('meeting_pause');
  await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
  // Its note can't be deleted while it records; now it can.
  await yap.invoke('note_delete', { id: winner });
  await yap.invoke('note_delete', { id: loser });
  await openView(main, 'Home');
});
