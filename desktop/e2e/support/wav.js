// A tiny 16-bit mono PCM WAV (a quiet 440 Hz tone) for the Upload and
// meeting tests. `gaps` ([[fromSec, toSec], …]) are silent: pauses where the
// meeting recorder cuts its chunks, and the loudness pattern its echo check
// compares. `amplitude` is in 16-bit units. `voice`: a voice-like buzz
// instead of the tone (140 Hz and its harmonics, rising and falling like
// syllables), which a browser's noise suppression keeps as it keeps speech
// (the phone's hands-free talk listens for the end of it).
import fs from 'node:fs';

/** The voice-like buzz at `t` seconds, about ±1 at its loudest (phone/web/tests/support/audio.js's). */
function voiceAt(t) {
  const f0 = 140 + 20 * Math.sin(2 * Math.PI * 0.7 * t);
  let s = 0;
  for (let k = 1; k <= 12; k++) s += Math.sin(2 * Math.PI * f0 * k * t) / k;
  return (s / 2) * (0.55 + 0.45 * Math.sin(2 * Math.PI * 4 * t));
}

export function writeWav(file, { seconds = 1.5, rate = 16_000, gaps = [], amplitude = 3000, voice = false } = {}) {
  const samples = Math.round(seconds * rate);
  const buf = Buffer.alloc(44 + samples * 2);
  buf.write('RIFF', 0);
  buf.writeUInt32LE(36 + samples * 2, 4);
  buf.write('WAVE', 8);
  buf.write('fmt ', 12);
  buf.writeUInt32LE(16, 16); // fmt chunk size
  buf.writeUInt16LE(1, 20); // PCM
  buf.writeUInt16LE(1, 22); // mono
  buf.writeUInt32LE(rate, 24);
  buf.writeUInt32LE(rate * 2, 28); // byte rate
  buf.writeUInt16LE(2, 32); // block align
  buf.writeUInt16LE(16, 34); // bits per sample
  buf.write('data', 36);
  buf.writeUInt32LE(samples * 2, 40);
  for (let i = 0; i < samples; i++) {
    const silent = gaps.some(([a, b]) => i >= a * rate && i < b * rate);
    const shape = voice ? voiceAt(i / rate) : Math.sin((2 * Math.PI * 440 * i) / rate);
    const v = silent ? 0 : Math.max(-32768, Math.min(32767, Math.round(amplitude * shape)));
    buf.writeInt16LE(v, 44 + i * 2);
  }
  fs.writeFileSync(file, buf);
  return file;
}
