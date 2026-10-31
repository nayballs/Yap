// The fake microphone's sounds, as WAV files Chromium plays as the mic
// (`--use-file-for-fake-audio-capture`). Written once, before the browsers
// start (global-setup.js), under test-results/ (git ignores it).
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
export const AUDIO_DIR = path.resolve(here, '../../test-results/link/audio');
/** A steady 440 Hz tone (looped): the frames' rate is read from its pitch. */
export const TONE_WAV = path.join(AUDIO_DIR, 'tone-440.wav');
/** Quiet, then 1.2 s of speech-like sound, then quiet (played once). */
export const SPEECH_WAV = path.join(AUDIO_DIR, 'speech-then-quiet.wav');
export const TONE_HZ = 440;
/** When the "speech" starts and ends in SPEECH_WAV, in seconds. */
export const SPEECH_AT = { from: 0.4, to: 1.6, total: 6 };

/** 16-bit mono PCM WAV bytes. */
export function wav(samples, rate) {
  const data = Buffer.alloc(samples.length * 2);
  for (let i = 0; i < samples.length; i++) {
    const s = Math.max(-1, Math.min(1, samples[i]));
    data.writeInt16LE(Math.round(s * 32767), i * 2);
  }
  const h = Buffer.alloc(44);
  h.write('RIFF', 0);
  h.writeUInt32LE(36 + data.length, 4);
  h.write('WAVE', 8);
  h.write('fmt ', 12);
  h.writeUInt32LE(16, 16);
  h.writeUInt16LE(1, 20); // PCM
  h.writeUInt16LE(1, 22); // mono
  h.writeUInt32LE(rate, 24);
  h.writeUInt32LE(rate * 2, 28);
  h.writeUInt16LE(2, 32);
  h.writeUInt16LE(16, 34);
  h.write('data', 36);
  h.writeUInt32LE(data.length, 40);
  return Buffer.concat([h, data]);
}

/**
 * A voice-like sound: a 140 Hz buzz with its harmonics, its loudness rising
 * and falling like syllables (so noise suppression keeps it, as it keeps
 * speech), with short gaps between "words".
 */
function speechLike(t) {
  const f0 = 140 + 20 * Math.sin(2 * Math.PI * 0.7 * t);
  let s = 0;
  for (let k = 1; k <= 12; k++) s += Math.sin(2 * Math.PI * f0 * k * t) / k;
  const syllables = 0.55 + 0.45 * Math.sin(2 * Math.PI * 4 * t);
  return 0.18 * s * syllables;
}

export function writeAudio() {
  fs.mkdirSync(AUDIO_DIR, { recursive: true });
  const rate = 48000;
  const tone = new Float32Array(rate * 4);
  for (let i = 0; i < tone.length; i++) tone[i] = 0.3 * Math.sin((2 * Math.PI * TONE_HZ * i) / rate);
  fs.writeFileSync(TONE_WAV, wav(tone, rate));

  const speech = new Float32Array(Math.round(rate * SPEECH_AT.total));
  for (let i = 0; i < speech.length; i++) {
    const t = i / rate;
    if (t >= SPEECH_AT.from && t < SPEECH_AT.to) speech[i] = speechLike(t);
  }
  fs.writeFileSync(SPEECH_WAV, wav(speech, rate));
}
