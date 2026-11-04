// Mic-quality tips on real speech (dictation/mic_tips.rs): the thresholds held
// against what a dictation really sounds like, not a test tone. Windows' own
// voice says a sentence (support/tts.js), written as a WAV and played in the
// mic's place through the real start/stop path (`realtest_dictate_wav`):
// - as it is, clear speech: judged fine, no tip;
// - 32 dB quieter (someone across the room, or the level turned down):
//   "quiet";
// - with a hiss nearly as loud as the voice under it (a busy café): "noisy";
// - turned up 12 dB past full scale: "clipped".
// The judgement is read back with `e2e_mic_tips` (the voice and the room in
// dBFS, the clipped share); the tip itself is the e2e suite's
// (e2e/mic-tips.spec.js). Played, never heard: nothing goes to the speakers.
import fs from 'node:fs';
import path from 'node:path';
import { test, expect } from './support/robot.js';
import { speechWav } from './support/tts.js';

test.use({ robotOptions: { name: 'mic-tips' } });

/** A 16-bit mono WAV's samples (-1…1) and rate. */
function readWav(file) {
  const b = fs.readFileSync(file);
  const rate = b.readUInt32LE(24);
  const at = b.indexOf('data', 12);
  const len = b.readUInt32LE(at + 4);
  const samples = new Float32Array(len / 2);
  for (let i = 0; i < samples.length; i++) samples[i] = b.readInt16LE(at + 8 + i * 2) / 32768;
  return { rate, samples };
}

/** `samples` as a 16-bit mono WAV at `rate`. */
function writeWav(file, { rate, samples }) {
  const data = Buffer.alloc(samples.length * 2);
  samples.forEach((s, i) => data.writeInt16LE(Math.round(Math.max(-1, Math.min(1, s)) * 32767), i * 2));
  const head = Buffer.alloc(44);
  head.write('RIFF', 0);
  head.writeUInt32LE(36 + data.length, 4);
  head.write('WAVEfmt ', 8);
  head.writeUInt32LE(16, 16);
  head.writeUInt16LE(1, 20);
  head.writeUInt16LE(1, 22);
  head.writeUInt32LE(rate, 24);
  head.writeUInt32LE(rate * 2, 28);
  head.writeUInt16LE(2, 32);
  head.writeUInt16LE(16, 34);
  head.write('data', 36);
  head.writeUInt32LE(data.length, 40);
  fs.writeFileSync(file, Buffer.concat([head, data]));
  return file;
}

/** `src` changed by `gainDb`, with a steady hiss of peak `hiss` under it. */
function variant(src, name, gainDb, hiss = 0) {
  const { rate, samples } = readWav(src);
  const g = 10 ** (gainDb / 20);
  let seed = 7;
  const out = samples.map((s) => {
    seed = (seed * 1103515245 + 12345) % 2 ** 31;
    return s * g + (seed / 2 ** 31 - 0.5) * 2 * hiss;
  });
  return writeWav(path.join(path.dirname(src), `${path.basename(src, '.wav')}-${name}.wav`), { rate, samples: out });
}

test('real speech: clear is fine; quiet, noisy and clipped are each told apart', async ({ robot }) => {
  test.setTimeout(240_000);
  const { yap, pad } = robot;
  await yap.invoke('e2e_mic_tips', { on: true });
  const speech = speechWav('Please send the leek order to Example Lane by Friday, and copy in the allotment group.');
  const judge = async (file) => {
    await pad.set('box', '');
    await robot.dictateWav(pad, file);
    const j = await yap.invoke('e2e_mic_tips', {});
    console.log(`${path.basename(file)}: voice ${j.voiceDb?.toFixed(1)} dBFS, room ${j.floorDb?.toFixed(1)} dBFS, clipped ${(j.clipped * 100).toFixed(2)} % → ${j.trouble ?? 'fine'}`);
    return j;
  };
  try {
    expect((await judge(speech)).trouble).toBeNull();
    expect((await judge(variant(speech, 'quiet', -32))).trouble).toBe('quiet');
    expect((await judge(variant(speech, 'noisy', 0, 0.25))).trouble).toBe('noisy');
    expect((await judge(variant(speech, 'clipped', 12))).trouble).toBe('clipped');
  } finally {
    await yap.invoke('e2e_mic_tips', { on: false });
  }
});
