// The refined transcript after a meeting (src-tauri/src/meetings/meeting_refine.rs)
// and a failed chunk filled again (meetings/meeting.rs "Failed chunks"), with real
// speech-to-text: an `engines` build (`npm run test:real:build --
// --engines`) and Parakeet V3 — plus Whisper Large for the second test —
// copied read-only from your installed Yap's models (support/engines.js).
// A two-person call made of Windows' text-to-speech sentences plays as the
// meeting's two sides (YAP_E2E_MEETING_AUDIO, 2× real time; no audio
// device). No window is typed into and the keyboard focus is never taken.
// Skipped without the build or the models.
//
// 1. The call side's first chunk fails live (`e2e_meeting_fail`): the
//    meeting ends with a hole, the refine comes first and fills it — the
//    first sentence must be there, on the call's side, in its place.
// 2. The live pass runs on Whisper Large, Parakeet V3 is installed and
//    more accurate for English: the meeting is refined on Parakeet after
//    the summary step, and both sides still say what was said.
import fs from 'node:fs';
import path from 'node:path';
import { test, expect } from './support/robot.js';
import { ENGINES_EXE, MODEL_ID, WHISPER_ID, hasWhisper, linkModel, linkWhisper, sttUnavailable } from './support/engines.js';
import { speechWav } from './support/tts.js';
import { RUNS } from './support/targets.js';
import { launchYap } from '../e2e/support/yap.js';

const why = sttUnavailable();
test.skip(!!why, why ?? '');

const NAME = 'meeting-refine-real';
const audioDir = path.join(RUNS, `${NAME}-wavs`);
const options = (config, prepare = linkModel) => ({
  name: NAME,
  runsDir: RUNS,
  exe: ENGINES_EXE,
  prepare,
  config: { modelSize: MODEL_ID, streamingPartials: false, selectedLanguage: 'en', meetingRefine: 'auto', ...config },
  env: { YAP_E2E_MEETING_AUDIO: audioDir, YAP_E2E_MEETING_SPEED: '2' },
});

// A fictional supplier call: "them" (A) and "you" (B) taking turns.
const THEM = [
  'Good morning. The replacement printer parts arrive on Thursday, not Monday.',
  'The engineer can install them on Friday if someone lets him into the office.',
  'Then we are all set. I will confirm the slot by email this afternoon.',
];
const YOU = [
  'Thanks for the update. Thursday is fine for us.',
  'I can open the office at nine on Friday and stay until he finishes.',
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
const side = (t, source) => t.filter((s) => s.source === source && s.text && !s.echo && !s.failed).map((s) => s.text).join(' ');
const note = (yap, id) => yap.readJson('notes.json').notes.find((n) => n.id === id);
const PLAYED = 'e2e: the test audio has played to the end';
const refine = (yap, noteId) => yap.invoke('meeting_audio_note', { noteId }).then((a) => (a.job?.kind === 'refine' ? a.job : null));

/** A meeting of the call audio, ended. Its note's id. */
async function meeting(yap, title) {
  const { id: noteId } = await yap.invoke('note_create', { title, folder: 'Meetings' });
  await yap.invoke('meeting_start', { noteId });
  await expect.poll(() => fs.readFileSync(yap.logPath, 'utf8').includes(PLAYED), { timeout: 120_000 }).toBe(true);
  await yap.invoke('meeting_end', { origin: null });
  await expect.poll(() => yap.invoke('meeting_state').then((s) => s.recording), { timeout: 60_000 }).toBe(false);
  return noteId;
}

test('a chunk that failed live is filled by the refine, the right words in the right place', async () => {
  test.setTimeout(420_000);
  callAudio();
  const yap = await launchYap(options({}));
  try {
    // The call side's first chunk fails, as after a lost graphics card.
    await yap.invoke('e2e_meeting_fail', { side: 'them', from: 0, to: 5 });
    const noteId = await meeting(yap, 'Printer parts');
    await yap.invoke('e2e_meeting_fail', {});
    // The hole was there, and the refine comes first ("then the summary").
    await expect.poll(() => refine(yap, noteId).then((j) => j?.thenSummary), { timeout: 30_000 }).toBe(true);
    await expect.poll(() => refine(yap, noteId).then((j) => j?.state), { timeout: 240_000 }).toBe('done');
    const t = note(yap, noteId).transcript;
    expect(t.some((s) => s.failed)).toBe(false);
    // The first sentence — where the hole was — and everything else.
    expect(share([THEM[0]], side(t, 'them')), side(t, 'them')).toBeGreaterThanOrEqual(0.85);
    expect(share(THEM, side(t, 'them')), side(t, 'them')).toBeGreaterThanOrEqual(0.85);
    expect(share(YOU, side(t, 'you')), side(t, 'you')).toBeGreaterThanOrEqual(0.85);
    // In its place: the call's first line comes first.
    const firstThem = t.find((s) => s.source === 'them' && s.text);
    expect(share([THEM[0]], firstThem.text)).toBeGreaterThanOrEqual(0.7);
  } finally {
    await yap.stop().catch(() => {});
  }
});

test('a meeting recorded on Whisper Large is refined on the more accurate Parakeet V3', async () => {
  test.skip(!hasWhisper(), 'Whisper Large (ggml-large-v3-q5_0.bin) isn\'t in your models folder');
  test.setTimeout(600_000);
  callAudio();
  const both = (dataDir) => {
    linkModel(dataDir);
    linkWhisper(dataDir);
  };
  const yap = await launchYap(options({ modelSize: WHISPER_ID }, both));
  try {
    const noteId = await meeting(yap, 'Printer parts, refined');
    await expect.poll(() => refine(yap, noteId).then((j) => j?.state), { timeout: 420_000 }).toBe('done');
    const job = await refine(yap, noteId);
    expect(job.model).toBe('Parakeet V3');
    const t = note(yap, noteId).transcript;
    expect(share(THEM, side(t, 'them')), side(t, 'them')).toBeGreaterThanOrEqual(0.85);
    expect(share(YOU, side(t, 'you')), side(t, 'you')).toBeGreaterThanOrEqual(0.85);
    // The live pass's words agree with the refine's (the check that keeps
    // a model that misheard the language out did pass).
    expect(fs.readFileSync(yap.logPath, 'utf8')).toMatch(/Meeting refine: the new lines against the live ones share=0\.[6-9]|share=1/);
  } finally {
    await yap.stop().catch(() => {});
  }
});
