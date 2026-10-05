// A tiny 16-bit mono PCM WAV (a quiet 440 Hz tone) for the Upload and
// meeting tests. `gaps` ([[fromSec, toSec], …]) are silent: pauses where the
// meeting recorder cuts its chunks, and the loudness pattern its echo check
// compares. `amplitude` is in 16-bit units.
import fs from 'node:fs';

export function writeWav(file, { seconds = 1.5, rate = 16_000, gaps = [], amplitude = 3000 } = {}) {
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
    const v = silent ? 0 : Math.round(amplitude * Math.sin((2 * Math.PI * 440 * i) / rate));
    buf.writeInt16LE(v, 44 + i * 2);
  }
  fs.writeFileSync(file, buf);
  return file;
}
