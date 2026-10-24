// Meeting audio kept on disk (src-tauri/src/meetings/meeting_audio.rs) with real
// speech-to-text: an `engines` build (`npm run test:real:build --
// --engines`) and Parakeet V3, copied read-only from your installed Yap's
// models (support/engines.js). A two-person call made of Windows'
// text-to-speech sentences plays as the meeting's two sides (YAP_E2E_MEETING_AUDIO,
// 2× real time; no audio device); the instance is killed mid-meeting, started
// again, and what never became lines is transcribed from the FLAC files on
// disk; then the whole meeting is transcribed again from them. Both passes
// must hear what was said, on the right side — the files are lossless, so
// the model hears what it heard live. No window is typed into and the
// keyboard focus is never taken. Skipped without the build or the model.
import fs from 'node:fs';
import path from 'node:path';
import { test, expect } from './support/robot.js';
import { ENGINES_EXE, MODEL_ID, linkModel, sttUnavailable } from './support/engines.js';
import { speechWav } from './support/tts.js';
import { RUNS } from './support/targets.js';
import { launchYap } from '../e2e/support/yap.js';

const why = sttUnavailable();
test.skip(!!why, why ?? '');

const NAME = 'meeting-audio-real';
const audioDir = path.join(RUNS, `${NAME}-wavs`);
const options = {
  name: NAME,
  runsDir: RUNS,
  exe: ENGINES_EXE,
  prepare: linkModel,
  config: { modelSize: MODEL_ID, streamingPartials: false },
  env: { YAP_E2E_MEETING_AUDIO: audioDir, YAP_E2E_MEETING_SPEED: '2' },
};

// A fictional delivery call: "them" (A) and "you" (B) taking turns.
const THEM = [
  'Thanks for joining. The delivery of the new shelves moves to Friday.',
  'Great, and the invoice should go to the finance inbox, not to me.',
  "Perfect, let's speak again next Tuesday morning.",
];
const YOU = [
  'That works for me. I will tell the warehouse team this afternoon.',
  'Understood. I will send it there with the purchase order number.',
];

/** The 16-bit samples of a WAV (its `data` chunk). */
function pcm16(file) {
  const b = fs.readFileSync(file);
  for (let i = 12; i + 8 <= b.length; ) {
    const id = b.toString('ascii', i, i + 4);
    const size = b.readUInt32LE(i + 4);
    if (id === 'data') return b.subarray(i + 8, i + 8 + size);
    i += 8 + size + (size & 1);
  }
  throw new Error(`no audio in ${file}`);
}
const silence = (secs) => Buffer.alloc(Math.round(secs * 16_000) * 2);
function writeWav(file, parts) {
  const data = Buffer.concat(parts);
  const h = Buffer.alloc(44);
  h.write('RIFF', 0);
  h.writeUInt32LE(36 + data.length, 4);
  h.write('WAVEfmt ', 8);
  h.writeUInt32LE(16, 16);
  h.writeUInt16LE(1, 20);
  h.writeUInt16LE(1, 22);
  h.writeUInt32LE(16_000, 24);
  h.writeUInt32LE(32_000, 28);
  h.writeUInt16LE(2, 32);
  h.writeUInt16LE(16, 34);
  h.write('data', 36);
  h.writeUInt32LE(data.length, 40);
  fs.writeFileSync(file, Buffer.concat([h, data]));
}

/** them.wav / you.wav: each sentence on its side, the other side silent meanwhile. */
function callAudio() {
  fs.mkdirSync(audioDir, { recursive: true });
  const them = [];
  const you = [];
  const turns = [THEM[0], YOU[0], THEM[1], YOU[1], THEM[2]];
  turns.forEach((sentence, i) => {
    const speech = pcm16(speechWav(sentence));
    const gap = silence(1.2);
    const quiet = Buffer.alloc(speech.length);
    (i % 2 === 0 ? them : you).push(speech, gap);
    (i % 2 === 0 ? you : them).push(quiet, gap);
  });
  writeWav(path.join(audioDir, 'them.wav'), them);
  writeWav(path.join(audioDir, 'you.wav'), you);
}

const words = (s) => s.toLowerCase().match(/[a-z]+/g) ?? [];
/** Share of `said`'s words found in `heard`. */
const share = (said, heard) => {
  const h = new Set(words(heard));
  const w = words(said.join(' '));
  return w.filter((x) => h.has(x)).length / w.length;
};
const side = (t, source) => t.filter((s) => s.source === source && s.text && !s.echo).map((s) => s.text).join(' ');
const note = (yap, id) => yap.readJson('notes.json').notes.find((n) => n.id === id);
const PLAYED = 'e2e: the test audio has played to the end';

/** A job on the note's audio, run to its end. */
async function runJob(yap, cmd, noteId) {
  await yap.invoke(cmd, { noteId });
  await expect
    .poll(() => yap.invoke('meeting_audio_note', { noteId }).then((a) => a.job?.state), { timeout: 180_000 })
    .toBe('done');
}

test('a meeting killed mid-call is recovered, then transcribed again, from its audio on disk', async () => {
  test.setTimeout(420_000);
  callAudio();
  let first = await launchYap(options);
  let second = null;
  try {
    const { id: noteId } = await first.invoke('note_create', { title: 'Delivery call', folder: 'Meetings' });
    await first.invoke('meeting_start', { noteId });
    await expect.poll(() => fs.readFileSync(first.logPath, 'utf8').includes(PLAYED), { timeout: 120_000 }).toBe(true);
    // The writer's last half second on disk, the last chunk not transcribed.
    await new Promise((r) => setTimeout(r, 1_500));
    await first.kill();

    second = await launchYap({ ...options, restart: true });
    await expect
      .poll(() => second.invoke('meeting_audio_note', { noteId }).then((a) => a.recoverableSecs), { timeout: 60_000 })
      .toBeGreaterThan(0);
    await runJob(second, 'meeting_recover', noteId);
    const recovered = note(second, noteId).transcript;
    // Everyone's words, on their own side.
    expect(share(THEM, side(recovered, 'them')), side(recovered, 'them')).toBeGreaterThanOrEqual(0.85);
    expect(share(YOU, side(recovered, 'you')), side(recovered, 'you')).toBeGreaterThanOrEqual(0.85);

    await runJob(second, 'meeting_transcribe_again', noteId);
    const again = note(second, noteId).transcript;
    expect(share(THEM, side(again, 'them')), side(again, 'them')).toBeGreaterThanOrEqual(0.85);
    expect(share(YOU, side(again, 'you')), side(again, 'you')).toBeGreaterThanOrEqual(0.85);
    // Lossless audio, the same model: the same words as before.
    const before = side(recovered, 'them') + ' ' + side(recovered, 'you');
    expect(share([before], side(again, 'them') + ' ' + side(again, 'you'))).toBeGreaterThanOrEqual(0.9);
  } finally {
    await second?.stop().catch(() => {});
    await first.stop().catch(() => {});
  }
});
