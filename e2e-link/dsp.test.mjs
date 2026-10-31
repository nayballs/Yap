// Yap Link's sound maths (src-tauri/link-page/dsp.js) in Node: the resampler
// the mic's worklet runs, Int16, the hands-free voice detector and the times.
// Run: `node --test e2e-link/dsp.test.mjs` (part of `npm run test:link`).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { Resampler, toInt16, fromInt16, rms, toDb, EnergyVad, elapsed, clock, FRAME_SAMPLES } from '../src-tauri/link-page/dsp.js';

/** `seconds` of a sine at `hz`, sampled at `rate`. */
function sine(rate, hz, seconds, amp = 0.5) {
  const n = Math.round(rate * seconds);
  const out = new Float32Array(n);
  for (let i = 0; i < n; i++) out[i] = amp * Math.sin((2 * Math.PI * hz * i) / rate);
  return out;
}

/** Through the resampler in blocks of `block` (the worklet's 128), then flushed. */
function run(rs, input, block = 128) {
  const parts = [];
  for (let i = 0; i < input.length; i += block) parts.push(Float32Array.from(rs.process(input.subarray(i, i + block))));
  parts.push(Float32Array.from(rs.flush()));
  const out = new Float32Array(parts.reduce((n, p) => n + p.length, 0));
  let at = 0;
  for (const p of parts) {
    out.set(p, at);
    at += p.length;
  }
  return out;
}

const peak = (a) => a.reduce((m, v) => Math.max(m, Math.abs(v)), 0);

test('48 kHz to 16 kHz: exactly a third as many samples, the same sine', () => {
  const out = run(new Resampler(48000, 16000), sine(48000, 1000, 1));
  assert.equal(out.length, 16000);
  // Away from the ends (the filter's zero padding), every sample is the 1 kHz sine at 16 kHz.
  let worst = 0;
  for (let n = 64; n < out.length - 64; n++) {
    const want = 0.5 * Math.sin((2 * Math.PI * 1000 * n) / 16000);
    worst = Math.max(worst, Math.abs(out[n] - want));
  }
  assert.ok(worst < 0.005, `worst error ${worst}`);
});

test('44.1 kHz (not a whole ratio) to 16 kHz: 16,000 samples a second, still in phase', () => {
  const out = run(new Resampler(44100, 16000), sine(44100, 440, 2));
  assert.equal(out.length, 32000);
  let worst = 0;
  for (let n = 64; n < out.length - 64; n++) {
    worst = Math.max(worst, Math.abs(out[n] - 0.5 * Math.sin((2 * Math.PI * 440 * n) / 16000)));
  }
  assert.ok(worst < 0.005, `worst error ${worst}`);
});

test('the block size makes no difference', () => {
  const input = sine(48000, 700, 0.5);
  const a = run(new Resampler(48000), input, 128);
  const b = run(new Resampler(48000), input, 4801);
  const c = run(new Resampler(48000), input, 1);
  assert.equal(a.length, b.length);
  assert.equal(a.length, c.length);
  for (let i = 0; i < a.length; i++) {
    assert.ok(Math.abs(a[i] - b[i]) < 1e-6 && Math.abs(a[i] - c[i]) < 1e-6, `sample ${i}`);
  }
});

test('what the 16 kHz stream can\'t hold is filtered out, not folded back', () => {
  // 12 kHz would alias to 4 kHz without the low-pass.
  const out = run(new Resampler(48000), sine(48000, 12000, 0.5));
  const middle = out.subarray(100, out.length - 100);
  assert.ok(toDb(peak(middle) / 0.5) < -40, `leak ${toDb(peak(middle) / 0.5).toFixed(1)} dB`);
  const near = run(new Resampler(48000), sine(48000, 10000, 0.5)).subarray(100, -100);
  assert.ok(toDb(peak(near) / 0.5) < -35, `10 kHz leak ${toDb(peak(near) / 0.5).toFixed(1)} dB`);
  // Speech's band comes through whole: 6 kHz within 1 dB.
  const pass = run(new Resampler(48000), sine(48000, 6000, 0.5)).subarray(100, -100);
  assert.ok(Math.abs(toDb(peak(pass) / 0.5)) < 1, `6 kHz ${toDb(peak(pass) / 0.5).toFixed(2)} dB`);
});

test('DC through at unity gain; reset starts again', () => {
  const rs = new Resampler(48000);
  const out = run(rs, new Float32Array(4800).fill(0.25));
  for (let n = 40; n < out.length - 40; n++) assert.ok(Math.abs(out[n] - 0.25) < 1e-3, `sample ${n}: ${out[n]}`);
  rs.reset();
  assert.equal(run(rs, sine(48000, 1000, 0.1)).length, 1600);
});

test('16 kHz in passes through', () => {
  const out = run(new Resampler(16000, 16000), sine(16000, 1000, 0.25));
  assert.equal(out.length, 4000);
  for (let n = 40; n < out.length - 40; n++) {
    assert.ok(Math.abs(out[n] - 0.5 * Math.sin((2 * Math.PI * 1000 * n) / 16000)) < 0.01);
  }
});

test('Int16: clamped, rounded, and back', () => {
  const d = toInt16(Float32Array.from([0, 0.5, -0.5, 1, -1, 1.5, -1.5]), new Int16Array(7));
  assert.deepEqual([...d], [0, 16384, -16384, 32767, -32768, 32767, -32768]);
  const back = fromInt16(Int16Array.from([16384, -32768]));
  assert.equal(back[0], 0.5);
  assert.equal(back[1], -1);
  assert.ok(Math.abs(rms(Int16Array.from([16384, -16384])) - 0.5) < 1e-9);
  assert.equal(FRAME_SAMPLES, 320); // 20 ms at 16 kHz
});

/** Frames at a level in dBFS, as the worklet reports them. */
const frames = (db, ms) => new Array(Math.round(ms / 20)).fill(10 ** (db / 20));

test('hands-free: speech heard, then the end after 0.8 s of quiet', () => {
  const vad = new EnergyVad();
  const said = [];
  const levels = [...frames(-70, 600), ...frames(-22, 1500), ...frames(-70, 2000)];
  levels.forEach((l, i) => {
    const r = vad.push(l);
    if (r) said.push([r, i * 20]);
  });
  assert.equal(said.length, 2);
  assert.equal(said[0][0], 'speech');
  assert.ok(said[0][1] >= 600 && said[0][1] < 900, `speech at ${said[0][1]} ms`);
  assert.equal(said[1][0], 'end');
  const quiet = said[1][1] - 2100;
  assert.ok(quiet >= 780 && quiet <= 900, `end ${quiet} ms after the speech`);
});

test('hands-free: a click is not speech, a pause mid-sentence is not the end', () => {
  const vad = new EnergyVad();
  for (const l of [...frames(-70, 400), ...frames(-15, 60), ...frames(-70, 2000)]) assert.equal(vad.push(l), null);
  assert.equal(vad.heard, false);

  const v2 = new EnergyVad();
  const out = [...frames(-60, 300), ...frames(-25, 800), ...frames(-60, 500), ...frames(-25, 800)].map((l) => v2.push(l));
  assert.deepEqual(
    out.filter(Boolean),
    ['speech'],
    'a half-second pause keeps listening',
  );
});

test('hands-free: a noisy room raises the bar', () => {
  const vad = new EnergyVad();
  // A steady fan at −40 dB isn't speech…
  for (const l of frames(-40, 3000)) assert.equal(vad.push(l), null);
  assert.ok(vad.thresholdDb > -40, `threshold ${vad.thresholdDb}`);
  // …but speech over it is, and so is its end.
  const said = [...frames(-20, 1200), ...frames(-40, 1200)].map((l) => vad.push(l)).filter(Boolean);
  assert.deepEqual(said, ['speech', 'end']);
});

test('hands-free: speech from the very first frame is still heard', () => {
  const vad = new EnergyVad();
  const said = [...frames(-22, 1000), ...frames(-65, 1200)].map((l) => vad.push(l)).filter(Boolean);
  assert.deepEqual(said, ['speech', 'end']);
});

test("the times as Chat's activity line and the Talk button say them", () => {
  assert.equal(elapsed(4.7), '4s');
  assert.equal(elapsed(119), '1m 59s');
  assert.equal(elapsed(3720), '1h 2m');
  assert.equal(clock(7.9), '0:07');
  assert.equal(clock(120), '2:00');
});
