// Yap Link's sound maths (src-tauri/link-page/dsp.js) in Node: the resampler
// both worklets' sound goes through (the mic down to 16 kHz, Yap's voice up
// from 24 kHz), the answer's ring and jitter buffer, Int16, the hands-free
// voice detector and the times.
// Run: `node --test e2e-link/dsp.test.mjs` (part of `npm run test:link`).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  Resampler,
  Ring,
  Playout,
  toInt16,
  fromInt16,
  rms,
  toDb,
  EnergyVad,
  WAITS,
  elapsed,
  clock,
  FRAME_SAMPLES,
  DOWN_RATE,
} from '../src-tauri/link-page/dsp.js';

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

/** What the detector said, and when (ms from the first frame). */
function said(vad, levels) {
  const out = [];
  levels.forEach((l, i) => {
    const r = vad.push(l);
    if (r) out.push([r, i * 20]);
  });
  return out;
}

test('hands-free: speech heard, then the end after the wait (Normal, 1.5 s) of quiet', () => {
  const heard = said(new EnergyVad(), [...frames(-70, 600), ...frames(-22, 1500), ...frames(-70, 3000)]);
  assert.equal(heard.length, 2);
  assert.equal(heard[0][0], 'speech');
  assert.ok(heard[0][1] >= 600 && heard[0][1] < 900, `speech at ${heard[0][1]} ms`);
  assert.equal(heard[1][0], 'end');
  const quiet = heard[1][1] - 2100;
  assert.ok(quiet >= 1480 && quiet <= 1520, `end ${quiet} ms after the speech`);
});

test('hands-free: each wait choice ends after its own time of quiet', () => {
  assert.deepEqual(WAITS, { short: 1000, normal: 1500, long: 2500 });
  for (const [name, wait] of Object.entries(WAITS)) {
    const heard = said(new EnergyVad({ endMs: wait }), [...frames(-70, 500), ...frames(-24, 1200), ...frames(-70, 4000)]);
    assert.deepEqual(
      heard.map(([r]) => r),
      ['speech', 'end'],
      name,
    );
    const quiet = heard[1][1] - 1700;
    assert.ok(Math.abs(quiet - wait) <= 20, `${name}: end ${quiet} ms after the speech, wanted ${wait}`);
  }
});

test("hands-free: a soft last word isn't taken for quiet", () => {
  // Speech, then a soft trailing word: −48 dB is under the −45 dB a start
  // needs, over the −52 dB that keeps speech going. Its 1.4 s is longer than
  // the Short wait, so a detector without the hold would send mid-word.
  for (const wait of [WAITS.short, WAITS.normal]) {
    const heard = said(new EnergyVad({ endMs: wait }), [
      ...frames(-70, 500),
      ...frames(-22, 1000),
      ...frames(-48, 1400),
      ...frames(-70, 3000),
    ]);
    assert.deepEqual(
      heard.map(([r]) => r),
      ['speech', 'end'],
    );
    const quiet = heard[1][1] - 2900;
    assert.ok(quiet >= wait - 20 && quiet <= wait + 20, `the end ${quiet} ms after the soft word (wait ${wait})`);
  }
  // The same soft sound alone never starts it.
  const vad = new EnergyVad({ endMs: WAITS.short });
  assert.deepEqual(said(vad, [...frames(-70, 500), ...frames(-48, 2000)]), []);
  assert.ok(vad.holdDb < -48 && vad.thresholdDb > -48, `hold ${vad.holdDb}, start ${vad.thresholdDb}`);
});

test("hands-free: a room's noise, even a fluttering one, doesn't start it", () => {
  // A fan wavering between −44 and −36 dB, there from the start, for 5 s.
  let x = 7;
  const flutter = Array.from({ length: 250 }, () => {
    x = (x * 16807) % 2147483647;
    return 10 ** ((-44 + 8 * (x / 2147483647)) / 20);
  });
  const vad = new EnergyVad();
  assert.deepEqual(said(vad, flutter), []);
  assert.equal(vad.heard, false);
  // Speech over it starts it, and the fan alone after it is quiet (it's under
  // the hold, 6 dB over the noise).
  const heard = said(vad, [...frames(-18, 1000), ...flutter]);
  assert.deepEqual(
    heard.map(([r]) => r),
    ['speech', 'end'],
  );
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
  const heard = [...frames(-20, 1200), ...frames(-40, 1800)].map((l) => vad.push(l)).filter(Boolean);
  assert.deepEqual(heard, ['speech', 'end']);
});

test('hands-free: speech from the very first frame is still heard', () => {
  const vad = new EnergyVad();
  const heard = [...frames(-22, 1000), ...frames(-65, 1800)].map((l) => vad.push(l)).filter(Boolean);
  assert.deepEqual(heard, ['speech', 'end']);
});

// ------------------------------------------------- Yap's voice, 24 kHz up

/** Through the resampler in the chunk sizes given (cycled), then flushed. */
function runChunks(rs, input, sizes) {
  const parts = [];
  for (let i = 0, k = 0; i < input.length; k++) {
    const n = sizes[k % sizes.length];
    parts.push(Float32Array.from(rs.process(input.subarray(i, i + n))));
    i += n;
  }
  parts.push(Float32Array.from(rs.flush()));
  const out = new Float32Array(parts.reduce((n, p) => n + p.length, 0));
  let at = 0;
  for (const p of parts) {
    out.set(p, at);
    at += p.length;
  }
  return out;
}

for (const outRate of [48000, 44100]) {
  test(`24 kHz to ${outRate / 1000} kHz: carried on across frames, the right length, the same sine`, () => {
    assert.equal(DOWN_RATE, 24000);
    const input = sine(24000, 1000, 1, 0.4);
    // Whole, then in 7 odd-sized pieces (the last takes what's left).
    const whole = runChunks(new Resampler(24000, outRate), input, [input.length]);
    const odd = [2311, 4801, 977, 6007, 3, 5501, 4400];
    assert.equal(odd.reduce((a, b) => a + b, 0), 24000);
    const pieces = runChunks(new Resampler(24000, outRate), input, odd);
    assert.equal(whole.length, outRate, 'a second in, a second out');
    assert.equal(pieces.length, whole.length);
    let joins = 0;
    for (let i = 0; i < whole.length; i++) joins = Math.max(joins, Math.abs(whole[i] - pieces[i]));
    assert.ok(joins < 1e-6, `pieces differ by ${joins}`);
    // Away from the ends, every sample is the 1 kHz sine at the new rate.
    let worst = 0;
    for (let n = 100; n < whole.length - 100; n++) {
      worst = Math.max(worst, Math.abs(whole[n] - 0.4 * Math.sin((2 * Math.PI * 1000 * n) / outRate)));
    }
    assert.ok(worst < 0.004, `worst error ${worst}`);
    // A higher voice too (its images above 12 kHz filtered away).
    const high = runChunks(new Resampler(24000, outRate), sine(24000, 5000, 0.5, 0.4), odd);
    let off = 0;
    for (let n = 100; n < high.length - 100; n++) {
      off = Math.max(off, Math.abs(high[n] - 0.4 * Math.sin((2 * Math.PI * 5000 * n) / outRate)));
    }
    assert.ok(off < 0.004, `5 kHz worst error ${off}`);
  });
}

test('the ring: first in, first out, round the end and grown when full', () => {
  const r = new Ring(16);
  const seq = (from, n) => Float32Array.from({ length: n }, (_, i) => from + i);
  r.write(seq(0, 10));
  const a = new Float32Array(6);
  assert.equal(r.read(a), 6);
  assert.deepEqual([...a], [0, 1, 2, 3, 4, 5]);
  r.write(seq(10, 10)); // wraps round
  assert.equal(r.length, 14);
  r.write(seq(20, 30)); // grows, the order kept
  assert.equal(r.length, 44);
  const all = new Float32Array(44);
  assert.equal(r.read(all), 44);
  assert.deepEqual([...all], [...seq(6, 44)]);
  assert.equal(r.shift(), 0, 'empty: silence');
  assert.equal(r.length, 0);
});

/** A Playout pulled in 128-sample quanta, as the audio thread does. */
function quanta(p, ms) {
  const n = Math.round((p.rate * ms) / 1000 / 128);
  const out = new Float32Array(n * 128);
  for (let q = 0; q < n; q++) p.pull(out.subarray(q * 128, (q + 1) * 128));
  return out;
}
const RATE = 48000;
const AMP = 0.3;
/** A sine carried on from where the last piece of it ended. */
function voiceFrom(at, ms, hz = 220) {
  const n = Math.round((RATE * ms) / 1000);
  return Float32Array.from({ length: n }, (_, i) => AMP * Math.sin((2 * Math.PI * hz * (at + i)) / RATE));
}
/** The largest jump between neighbouring samples. */
const maxJump = (a) => a.reduce((m, v, i) => (i ? Math.max(m, Math.abs(v - a[i - 1])) : m), 0);
const NATURAL = AMP * 2 * Math.PI * (220 / RATE); // the sine's own largest step

test('playout: silent until 250 ms is waiting, then faded in', () => {
  const p = new Playout(RATE);
  p.begin();
  assert.equal(p.state, 'buffering');
  p.push(voiceFrom(0, 200));
  const before = quanta(p, 100);
  assert.ok(before.every((s) => s === 0), 'nothing before 250 ms');
  assert.equal(p.state, 'buffering');
  p.push(voiceFrom(9600, 60));
  const after = quanta(p, 50);
  assert.equal(p.state, 'playing');
  assert.deepEqual(p.take(), ['start']);
  const st = p.stats();
  assert.ok(st.startBufferedMs >= 259 && st.startBufferedMs <= 261, `buffered ${st.startBufferedMs}`);
  assert.ok(st.startDelayMs >= 99 && st.startDelayMs <= 103, `waited ${st.startDelayMs}`);
  // A fade in over 5 ms: the first samples small, none a jump.
  assert.ok(Math.abs(after[0]) < 0.01);
  assert.ok(maxJump(after) <= NATURAL + AMP / p.fadeLen + 1e-6, `jump ${maxJump(after)}`);
});

test('playout: running dry fades out over 5 ms, counts an underrun, waits for 150 ms more, fades back in', () => {
  const p = new Playout(RATE);
  p.begin();
  p.push(voiceFrom(0, 300));
  const first = quanta(p, 400);
  assert.equal(p.state, 'starved');
  assert.deepEqual(p.take(), ['start', 'underrun']);
  assert.equal(p.underruns, 1);
  // The last 5 ms before the gap: a fade down to silence, not a cut.
  const end = 300 * 48;
  const tail = first.subarray(end - p.fadeLen, end);
  for (let i = 0; i < tail.length; i++) {
    const g = (p.fadeLen - 1 - i) / p.fadeLen;
    assert.ok(Math.abs(tail[i]) <= AMP * g + 1e-6, `sample ${i} of the fade: ${tail[i]}`);
  }
  assert.ok(first.subarray(end).every((s) => s === 0), 'then silence');
  // 100 ms more isn't enough to start again…
  p.push(voiceFrom(end, 100));
  assert.ok(quanta(p, 50).every((s) => s === 0));
  assert.equal(p.state, 'starved');
  // …150 ms is: faded back in.
  p.push(voiceFrom(end + 4800, 60));
  const back = quanta(p, 100);
  assert.equal(p.state, 'playing');
  assert.deepEqual(p.take(), ['resume']);
  assert.ok(Math.abs(back[0]) < 0.01, 'from silence');
  assert.ok(back.some((s) => Math.abs(s) > AMP * 0.9), 'up to full');
  // No hard cut anywhere in it.
  assert.ok(p.stats().maxStep <= NATURAL + AMP / p.fadeLen + 1e-6, `largest step ${p.stats().maxStep}`);
  assert.ok(p.stats().gapMs >= 49, `gap ${p.stats().gapMs} ms`);
  assert.equal(p.underruns, 1);
});

test('playout: a frame just in time only dips, without a gap', () => {
  const p = new Playout(RATE);
  p.begin();
  p.push(voiceFrom(0, 300));
  // 298 ms played: 2 ms left, the fade under way.
  const n = Math.floor((298 * 48) / 128);
  for (let q = 0; q < n; q++) p.pull(new Float32Array(128));
  assert.equal(p.state, 'playing');
  p.push(voiceFrom(300 * 48, 300));
  quanta(p, 100);
  assert.equal(p.state, 'playing');
  assert.equal(p.underruns, 0);
  assert.ok(p.stats().maxStep <= NATURAL + AMP / p.fadeLen + 1e-6);
});

test('playout: the end plays out what is left (even under 250 ms), then done', () => {
  const p = new Playout(RATE);
  p.begin();
  p.push(voiceFrom(0, 100));
  p.end();
  const out = quanta(p, 200);
  assert.deepEqual(p.take(), ['start', 'done']);
  assert.equal(p.state, 'idle');
  assert.equal(p.stats().played, 0.1);
  assert.equal(Math.abs(out[100 * 48 - 1]), 0, 'the last sample faded to silence');
  assert.ok(out.subarray(100 * 48).every((s) => s === 0));
  assert.equal(p.underruns, 0);
  // An answer that never had any sound ends too.
  p.begin();
  p.end();
  quanta(p, 10);
  assert.deepEqual(p.take(), ['done']);
});

test('playout: stop is silent within 5 ms, a fade not a cut, and the rest is dropped', () => {
  const p = new Playout(RATE);
  p.begin();
  p.push(voiceFrom(0, 2000));
  quanta(p, 500);
  p.take();
  p.stop();
  assert.equal(p.state, 'stopping');
  const out = quanta(p, 50);
  assert.deepEqual(p.take(), ['stopped']);
  assert.equal(p.state, 'idle');
  assert.ok(out.subarray(p.fadeLen).every((s) => s === 0), 'silent after 5 ms');
  assert.ok(p.stats().maxStep <= NATURAL + AMP / p.fadeLen + 1e-6, `largest step ${p.stats().maxStep}`);
  assert.equal(p.ring.length, 0);
  // Frames still on the way are dropped.
  p.push(voiceFrom(0, 100));
  assert.ok(quanta(p, 50).every((s) => s === 0));
  // Stop before any sound: idle at once.
  p.begin();
  p.push(voiceFrom(0, 100));
  p.stop();
  assert.equal(p.state, 'idle');
  assert.deepEqual(p.take(), ['stopped']);
});

test('playout: a new answer starts clean', () => {
  const p = new Playout(RATE);
  p.begin();
  p.push(voiceFrom(0, 300));
  quanta(p, 500);
  assert.equal(p.underruns, 1);
  p.begin();
  const st = p.stats();
  assert.equal(st.state, 'buffering');
  assert.equal(st.underruns, 0);
  assert.equal(st.queued, 0);
  assert.equal(st.startDelayMs, null);
  assert.equal(p.ring.length, 0);
});

test('the voice end to end: frames resampled one after another, then played without a seam', () => {
  // Yap's 0.5 s frames at 24 kHz, a sine carried across them, into the
  // playout at 48 kHz as the page and the worklet do it.
  const rs = new Resampler(24000, RATE);
  const p = new Playout(RATE);
  p.begin();
  const src = sine(24000, 220, 2, AMP);
  for (let i = 0; i < src.length; i += 12000) p.push(Float32Array.from(rs.process(src.subarray(i, i + 12000))));
  p.push(Float32Array.from(rs.flush()));
  p.end();
  const out = quanta(p, 2200);
  assert.deepEqual(p.take(), ['start', 'done']);
  assert.equal(p.underruns, 0);
  // Inside it, sample for sample the sine at 48 kHz: no click at the joins.
  let worst = 0;
  for (let n = 400; n < 2 * RATE - 400; n++) worst = Math.max(worst, Math.abs(out[n] - AMP * Math.sin((2 * Math.PI * 220 * n) / RATE)));
  assert.ok(worst < 0.003, `worst error ${worst}`);
});

test("the times as Chat's activity line and the Talk button say them", () => {
  assert.equal(elapsed(4.7), '4s');
  assert.equal(elapsed(119), '1m 59s');
  assert.equal(elapsed(3720), '1h 2m');
  assert.equal(clock(7.9), '0:07');
  assert.equal(clock(120), '2:00');
});
