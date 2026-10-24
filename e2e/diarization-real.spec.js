// The real speaker engine (Nemotron-3-Diarization on NeMo-Speech.cpp, as
// `yap.exe diarize`), driven through Yap: `diarize_wav` on a test meeting
// scored against its reference turns, and a meeting recording whose "Them"
// side is that meeting, labelled while it records and finished when it
// stops (speakers/diarize.rs, meetings/meeting.rs's tee) — its "Them" lines ending up with
// one voice per person (speakers/labels.rs).
//
// Skipped unless the model and runtime are on disk (it never downloads them):
//   YAP_E2E_DIARIZATION_MODELS  a folder laid out like <data>/models/diarization
//                               (nemotron-3-diarization/, nemo-speech-0.2.0-cpu/)
//   YAP_E2E_DIARIZATION_AUDIO   a folder of <name>.wav + <name>.rttm (the
//                               diarization spike's audio/conv; a <name>.json
//                               of its utterances' words scores the lines)
// Optional:
//   YAP_E2E_DIARIZATION_FILE    which meeting (default s3_clean)
//   YAP_E2E_DIARIZATION_SPEED   how fast it plays (default 20x)
//   YAP_E2E_DIARIZATION_STT     a Parakeet model folder, for an `engines`
//                               build (YAP_E2E_EXE): real lines with word
//                               times, split at their words, scored word by
//                               word against the reference (linked read-only
//                               into the run's Data/models/)
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
const SPEED = process.env.YAP_E2E_DIARIZATION_SPEED || '20';
const STT = process.env.YAP_E2E_DIARIZATION_STT;
const meetingDir = path.join(RUNS, 'diarization-real-wavs');

test.skip(!MODELS || !AUDIO, 'needs YAP_E2E_DIARIZATION_MODELS and YAP_E2E_DIARIZATION_AUDIO');

/** The speech model, hard-linked (or copied) into the run's Data/models/. */
function linkStt(dataDir) {
  if (!STT) return;
  const target = path.join(dataDir, 'models', path.basename(STT));
  fs.mkdirSync(target, { recursive: true });
  for (const f of fs.readdirSync(STT)) {
    const src = path.join(STT, f);
    if (f.startsWith('._') || !fs.statSync(src).isFile()) continue;
    try {
      fs.linkSync(src, path.join(target, f));
    } catch {
      fs.copyFileSync(src, path.join(target, f));
    }
  }
}

test.use({
  yapOptions: {
    name: 'diarization-real',
    env: {
      YAP_DIARIZATION_MODELS: MODELS ?? '',
      YAP_E2E_MEETING_AUDIO: meetingDir,
      YAP_E2E_MEETING_SPEED: SPEED,
    },
    config: { meetingDiarization: true, streamingPartials: false },
    prepare: linkStt,
  },
});

const voices = (segments) => new Set(segments.map((s) => `${s.stream ?? 0}:${s.speaker}`)).size;
const asSecs = (segments) => segments.map((s) => ({ start: s.startMs / 1000, end: s.endMs / 1000, speaker: `${s.stream ?? 0}:${s.speaker}` }));
const median = (xs) => (xs.length ? [...xs].sort((a, b) => a - b)[Math.floor(xs.length / 2)] : null);

/** Every way to give each of `hyp`'s voices a different reference speaker (or none). */
function bestMapping(counts, hyp, ref) {
  let best = 0;
  const walk = (i, used, total) => {
    if (i === hyp.length) {
      best = Math.max(best, total);
      return;
    }
    walk(i + 1, used, total); // this voice matches nobody
    for (const r of ref) {
      if (!used.has(r)) {
        used.add(r);
        walk(i + 1, used, total + (counts[hyp[i]]?.[r] ?? 0));
        used.delete(r);
      }
    }
  };
  walk(0, new Set(), 0);
  return best;
}

/**
 * The share of the "Them" lines' words put on the right person (the spike's
 * yapflow.py score): each word, at its time on the recording's clock, belongs
 * to the reference utterance around it (else the nearest); the lines' voices
 * are mapped one-to-one onto the reference speakers the best way.
 */
function wordAccuracy(lines, utterances) {
  const counts = {};
  let total = 0;
  for (const s of lines) {
    if (!s.words?.length || s.words.length !== s.text.split(/\s+/).filter(Boolean).length) continue;
    for (const [a, b] of s.words) {
      const t = (s.startMs + (a + b) / 2) / 1000;
      const around = utterances.find((u) => u.start <= t && t <= u.end);
      const near =
        around ??
        utterances.reduce((best, u) => {
          const d = t < u.start ? u.start - t : t - u.end;
          return !best || d < best.d ? { u, d } : best;
        }, null)?.u;
      if (!near) continue;
      const v = s.speaker || 'them';
      counts[v] ??= {};
      counts[v][near.speaker] = (counts[v][near.speaker] ?? 0) + 1;
      total += 1;
    }
  }
  const hyp = Object.keys(counts);
  const ref = [...new Set(utterances.map((u) => u.speaker))];
  return { total, right: bestMapping(counts, hyp, ref), counts };
}

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
  test.setTimeout(STT ? 900_000 : 180_000);
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

  // Live labels while it records (the engine trails by up to ~30 s of
  // audio), and the lines taking voices as they go: how far behind the
  // labels are, and how many lines have a voice before the stop.
  const played = () => fs.readFileSync(yap.logPath, 'utf8').includes('e2e: the test audio has played to the end');
  const behind = [];
  // For lines first seen without a voice: how far the audio had got past
  // the line's end when its voice showed up (the audio_ms of the timeline
  // that gave it: the engine reports when its labels move on). A split line
  // is found by its first part (its side, date and place).
  const pastEnd = [];
  const firstSeen = new Map();
  const voiced = new Set();
  let liveLines = { them: 0, voiced: 0 };
  let sawLive = false;
  const deadline = Date.now() + (STT ? 840_000 : 120_000);
  while (!played() && Date.now() < deadline) {
    const tl = await yap.invoke('diarization_timeline', { noteId });
    if (tl?.segments?.length) {
      sawLive = true;
      expect(tl.done).toBe(false);
      expect(tl.labelledMs).toBeLessThanOrEqual(tl.audioMs);
      behind.push(tl.audioMs - tl.labelledMs);
    }
    const n = await yap.invoke('note_get', { id: noteId });
    const them = n.transcript.filter((s) => s.source === 'them' && s.text?.trim());
    liveLines = { them: them.length, voiced: them.filter((s) => s.speaker).length };
    for (const s of them) {
      if (!tl) break;
      const key = `${s.ts}:${s.startMs}`;
      // (A split line's later parts are new keys, seen already voiced.)
      if (!firstSeen.has(key)) firstSeen.set(key, { end: s.endMs, voiced: !!s.speaker });
      const first = firstSeen.get(key);
      if (s.speaker && !first.voiced && !voiced.has(key)) {
        voiced.add(key);
        pastEnd.push(tl.audioMs - first.end);
      }
    }
    await new Promise((r) => setTimeout(r, 250));
  }
  expect(played(), 'the test audio played to the end').toBe(true);
  expect(sawLive, 'live labels while recording').toBe(true);
  expect(liveLines.voiced, 'lines got voices while it recorded').toBeGreaterThan(0);

  // Played out → pause → the final timeline, and every line labelled.
  await yap.invoke('meeting_pause');
  await expect.poll(() => yap.invoke('meeting_state').then((s) => s.recording), { timeout: STT ? 300_000 : 30_000 }).toBe(false);
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

  // The "Them" lines: one voice per person, nearly every line labelled.
  const note = meetingNote(yap.readJson('notes.json'), 'Three voices');
  const them = note.transcript.filter((s) => s.source === 'them' && !s.echo && s.text?.trim());
  const labelled = them.filter((s) => s.speaker?.startsWith('them-'));
  const ids = new Set(labelled.map((s) => s.speaker));
  const withWords = them.filter((s) => s.words?.length).length;
  console.log(
    `lines: ${them.length} "Them" lines (${withWords} with word times), ${labelled.length} labelled ` +
      `(${liveLines.voiced} of ${liveLines.them} before the stop), voices ${[...ids].join(', ')}; ` +
      `labels behind the audio while recording: median ${median(behind)} ms, max ${Math.max(0, ...behind)} ms; ` +
      `a line's voice showed up ${median(pastEnd)} ms (median; max ${Math.max(0, ...pastEnd)}) of audio after ` +
      `its last word (${pastEnd.length} lines seen without one first)`
  );
  expect(them.length).toBeGreaterThan(0);
  expect(ids.size).toBe(people);
  expect(labelled.length / them.length).toBeGreaterThan(0.9);
  expect(note.speakers?.them?.name).toBeUndefined();

  // With real word times: how many words went to the right person.
  const words = path.join(AUDIO, `${FILE}.json`);
  if (withWords && fs.existsSync(words)) {
    const utterances = JSON.parse(fs.readFileSync(words, 'utf8'));
    const { total, right, counts } = wordAccuracy(them, utterances);
    console.log(`words: ${right} of ${total} on the right person (${((right / total) * 100).toFixed(1)}%) ${JSON.stringify(counts)}`);
    expect(total).toBeGreaterThan(0);
    expect(right / total).toBeGreaterThan(0.75);
  }

  const log = fs.readFileSync(yap.logPath, 'utf8');
  expect(log).toContain('Diarization: engine started');
  expect(log).toContain('Diarization: done');
  expect(log).toContain('Speakers: the recording\'s call side told apart');
  await openView(main, 'Notes');
  await shot(main, '01-meeting-with-speaker-engine');
  await yap.invoke('notepad_open', { noteId });
  await expect.poll(() => yap.invoke('plugin:window|is_visible', { label: 'notepad' })).toBe(true);
  await yap.notepad.getByRole('tab', { name: 'Transcript' }).click();
  await shot(yap.notepad, '02-notepad-voices-from-the-engine');
});

test('a resumed meeting starts the engine again, and its voices carry on numbering', async ({ yap }) => {
  test.setTimeout(STT ? 1_200_000 : 240_000);
  fs.mkdirSync(meetingDir, { recursive: true });
  fs.copyFileSync(path.join(AUDIO, `${FILE}.wav`), path.join(meetingDir, 'them.wav'));
  writeWav(path.join(meetingDir, 'you.wav'), { seconds: 2, gaps: [[0, 2]] });
  const people = new Set(readRttm(path.join(AUDIO, `${FILE}.rttm`)).map((s) => s.speaker)).size;
  const note = await yap.invoke('note_create', { title: 'Resumed meeting', folder: 'Meetings' });
  const count = (text) => fs.readFileSync(yap.logPath, 'utf8').split(text).length - 1;
  const playedTimes = () => count('e2e: the test audio has played to the end');
  const startedTimes = () => count('Diarization: engine started');
  const recordOnce = async () => {
    const [played, started] = [playedTimes(), startedTimes()];
    await yap.invoke('meeting_start', { noteId: note.id });
    await expect.poll(() => yap.invoke('meeting_state').then((s) => s.recording)).toBe(true);
    await expect.poll(playedTimes, { timeout: STT ? 840_000 : 120_000, intervals: [500] }).toBeGreaterThan(played);
    await yap.invoke('meeting_pause');
    await expect.poll(() => yap.invoke('meeting_state').then((s) => s.recording), { timeout: STT ? 300_000 : 30_000 }).toBe(false);
    expect(startedTimes(), 'an engine for this recording').toBe(started + 1);
  };
  await recordOnce();
  const first = await yap.invoke('note_get', { id: note.id });
  const firstVoices = new Set(first.transcript.filter((s) => s.speaker?.startsWith('them-')).map((s) => s.speaker));
  await recordOnce();
  const both = await yap.invoke('note_get', { id: note.id });
  const after = both.transcript.slice(first.transcript.length).filter((s) => s.source === 'them' && s.text?.trim());
  const secondVoices = new Set(after.filter((s) => s.speaker?.startsWith('them-')).map((s) => s.speaker));
  const n = (id) => Number(id.slice('them-'.length));
  console.log(`resumed: first recording ${[...firstVoices].join(', ')}; second ${[...secondVoices].join(', ')}; ${both.breaks?.length ?? 0} stops`);
  // A new recording is a new engine session: its voices are new numbers,
  // after the first recording's (never merged by themselves).
  expect(firstVoices.size).toBe(people);
  expect(secondVoices.size).toBe(people);
  expect(Math.min(...[...secondVoices].map(n))).toBeGreaterThan(Math.max(...[...firstVoices].map(n)));
  // The first recording's lines kept their voices.
  expect(both.transcript.slice(0, first.transcript.length).map((s) => s.speaker)).toEqual(first.transcript.map((s) => s.speaker));
});
