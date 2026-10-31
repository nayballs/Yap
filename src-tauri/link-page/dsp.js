// Yap Link's sound maths, with nothing of the page in it, so it runs in three
// places: the mic's AudioWorklet (mic-worklet.js), the page (link.js) and
// Node's test runner (e2e-link/dsp.test.mjs).
//
// - `Resampler`: the mic at the phone's own rate (48 kHz, 44.1 kHz…) to the
//   16 kHz the PC's speech models take, through a windowed-sinc low-pass, in
//   a stream (any block size in, every output sample once). Positions are
//   kept in whole numbers, so 48,000 samples in give exactly 16,000 out.
// - `toInt16`: float samples to Int16, clamped.
// - `rms` / `toDb`: a frame's level, for the meter and the voice detector.
// - `EnergyVad`: hands-free's end of speech, by energy: speech once it's
//   heard for a moment above the room's noise, then the end after ~0.8 s
//   of quiet.
// - `elapsed`: a time as Chat's activity line says it ("12s", "1m 59s").

/** The rate the PC takes the phone's mic at. */
export const LINK_RATE = 16000;

/** One frame of the mic sent up: 20 ms at 16 kHz. */
export const FRAME_SAMPLES = 320;

export class Resampler {
  /**
   * @param {number} inRate the capture's rate (the AudioContext's)
   * @param {number} outRate the rate wanted (16 kHz)
   * @param {number} zeroCrossings the filter's half-length in zero crossings
   */
  constructor(inRate, outRate = LINK_RATE, zeroCrossings = 16) {
    this.inRate = Math.round(inRate);
    this.outRate = Math.round(outRate);
    if (!(this.inRate > 0 && this.outRate > 0)) throw new RangeError('Resampler: rates must be positive');
    const ratio = this.inRate / this.outRate;
    // The cut-off as a fraction of the input's Nyquist: under the output's
    // Nyquist going down (no aliasing), under the input's going up. At 48 kHz
    // that's 106 taps: flat to ~6 kHz, −40 dB by 10 kHz.
    const b = 0.9 / Math.max(1, ratio);
    this.H = Math.max(2, Math.ceil(zeroCrossings / b)); // half-width, input samples
    this.R = 128; // table steps per input sample
    const n = this.H * this.R;
    const table = new Float32Array(n + 2);
    for (let i = 0; i <= n; i++) {
      const d = i / this.R;
      const x = Math.PI * b * d;
      const sinc = d === 0 ? 1 : Math.sin(x) / x;
      const u = d / this.H;
      const blackman = 0.42 + 0.5 * Math.cos(Math.PI * u) + 0.08 * Math.cos(2 * Math.PI * u);
      table[i] = b * sinc * Math.max(0, blackman);
    }
    // Unity gain at DC.
    let sum = table[0];
    for (let k = 1; k < this.H; k++) sum += 2 * table[k * this.R];
    for (let i = 0; i <= n; i++) table[i] /= sum;
    this.table = table;
    this.reset();
  }

  /** Back to the start: no history, the next output is the first. */
  reset() {
    this.cap = 4096;
    this.buf = new Float32Array(this.cap);
    // H − 1 zeros before the first sample, so output 0 sits on input 0.
    this.len = this.H - 1;
    this.base = -(this.H - 1); // the absolute input index of buf[0]
    this.n = 0; // outputs made so far
    this.out = new Float32Array(0);
  }

  _kernel(d) {
    const ad = (d < 0 ? -d : d) * this.R;
    const i = ad | 0;
    if (i >= this.H * this.R) return 0;
    const t = ad - i;
    const a = this.table[i];
    return a + (this.table[i + 1] - a) * t;
  }

  _append(input) {
    const need = this.len + input.length;
    if (need > this.cap) {
      while (this.cap < need) this.cap *= 2;
      const grown = new Float32Array(this.cap);
      grown.set(this.buf.subarray(0, this.len));
      this.buf = grown;
    }
    this.buf.set(input, this.len);
    this.len = need;
  }

  /**
   * Takes a block of input, returns the output samples it completes (a view
   * into a buffer the next call reuses: copy it to keep it).
   * @param {Float32Array} input
   * @returns {Float32Array}
   */
  process(input) {
    this._append(input);
    const { inRate, outRate, H, buf } = this;
    const most = Math.ceil((this.len / inRate) * outRate) + 2;
    if (this.out.length < most) this.out = new Float32Array(most);
    const out = this.out;
    let made = 0;
    for (;;) {
      const num = this.n * inRate; // where output n sits, × outRate (exact)
      const centre = Math.floor(num / outRate);
      if (centre + H - this.base >= this.len) break;
      const frac = (num - centre * outRate) / outRate;
      const start = centre - H + 1 - this.base;
      let acc = 0;
      for (let k = 0; k < 2 * H; k++) acc += buf[start + k] * this._kernel(k - H + 1 - frac);
      out[made++] = acc;
      this.n++;
    }
    // Keep only what the next output still needs.
    const next = Math.floor((this.n * inRate) / outRate) - H + 1;
    const drop = next - this.base;
    if (drop > 0) {
      const keep = Math.max(0, this.len - drop);
      buf.copyWithin(0, Math.min(drop, this.len), this.len);
      this.len = keep;
      this.base += drop;
    }
    return out.subarray(0, made);
  }

  /** The last samples the filter was still waiting on (call once, at the end). */
  flush() {
    return this.process(new Float32Array(this.H));
  }
}

/**
 * Float samples (−1…1) to Int16, clamped, into `dst` at `at`.
 * @param {Float32Array} src
 * @param {Int16Array} dst
 * @param {number} at
 */
export function toInt16(src, dst, at = 0) {
  for (let i = 0; i < src.length; i++) {
    let s = src[i];
    s = s > 1 ? 1 : s < -1 ? -1 : s;
    dst[at + i] = s < 0 ? Math.round(s * 0x8000) : Math.round(s * 0x7fff);
  }
  return dst;
}

/** Int16 samples to floats (−1…1). */
export function fromInt16(src, dst = new Float32Array(src.length)) {
  for (let i = 0; i < src.length; i++) dst[i] = src[i] / 0x8000;
  return dst;
}

/** The root-mean-square of Int16 samples, as a fraction of full scale. */
export function rms(int16) {
  if (!int16.length) return 0;
  let sum = 0;
  for (let i = 0; i < int16.length; i++) {
    const s = int16[i] / 0x8000;
    sum += s * s;
  }
  return Math.sqrt(sum / int16.length);
}

/** A level in dB below full scale (floored at −100). */
export const toDb = (level) => 20 * Math.log10(Math.max(level, 1e-5));

/**
 * End of speech by energy, one frame at a time. `push(level)` returns
 * 'speech' the moment speech is first heard, 'end' once it has been quiet
 * for `endMs` after speech, else null.
 *
 * The room's noise is the quietest frame of the last 3 s (speech always has
 * gaps quieter than itself; a fan doesn't), taken as no louder than −50 dB
 * for the first half-second and never above −30 dB. Speech is 12 dB over
 * it, and over −45 dB at least.
 */
export class EnergyVad {
  constructor({ frameMs = 20, endMs = 800, minSpeechMs = 160, marginDb = 12, floorDb = -45, windowMs = 3000 } = {}) {
    this.frameMs = frameMs;
    this.endFrames = Math.round(endMs / frameMs);
    this.minSpeechFrames = Math.max(1, Math.round(minSpeechMs / frameMs));
    this.settleFrames = Math.round(500 / frameMs);
    this.marginDb = marginDb;
    this.floorDb = floorDb;
    this.window = new Float32Array(Math.max(1, Math.round(windowMs / frameMs)));
    this.reset();
  }

  reset() {
    this.count = 0;
    this.noiseDb = -50;
    this.speechRun = 0;
    this.quiet = 0;
    this.heard = false;
    this.ended = false;
  }

  /** The level speech has to clear just now. */
  get thresholdDb() {
    return Math.max(this.noiseDb + this.marginDb, this.floorDb);
  }

  _follow(db) {
    this.window[this.count % this.window.length] = db;
    this.count++;
    const n = Math.min(this.count, this.window.length);
    let min = Infinity;
    for (let i = 0; i < n; i++) if (this.window[i] < min) min = this.window[i];
    if (this.count <= this.settleFrames) min = Math.min(min, -50);
    this.noiseDb = Math.min(min, -30);
  }

  push(level) {
    if (this.ended) return null;
    const db = toDb(level);
    this._follow(db);
    const loud = db > this.thresholdDb;
    if (!this.heard) {
      // A cough or a click isn't speech: it has to last.
      this.speechRun = loud ? this.speechRun + 1 : Math.max(0, this.speechRun - 1);
      if (this.speechRun >= this.minSpeechFrames) {
        this.heard = true;
        this.quiet = 0;
        return 'speech';
      }
      return null;
    }
    this.quiet = loud ? 0 : this.quiet + 1;
    if (this.quiet >= this.endFrames) {
      this.ended = true;
      return 'end';
    }
    return null;
  }
}

/** A time as Chat's activity line says it: "12s", "1m 59s", "1h 3m". */
export function elapsed(seconds) {
  const s = Math.max(0, Math.floor(seconds || 0));
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m ${s % 60}s`;
  return `${Math.floor(s / 3600)}h ${Math.floor((s % 3600) / 60)}m`;
}

/** A question's length on the Talk button: "0:07", "1:59". */
export function clock(seconds) {
  const s = Math.max(0, Math.floor(seconds || 0));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
}
