// Yap Link's sound maths, with nothing of the page in it, so it runs in four
// places: the mic's AudioWorklet (mic-worklet.js), the answer's AudioWorklet
// (player-worklet.js), the page (link.js) and Node's test runner
// (e2e-link/dsp.test.mjs).
//
// - `Resampler`: one rate to another through a windowed-sinc filter, in a
//   stream (any block size in, every output sample once, the same samples
//   however the input is cut up). The mic: the phone's own rate (48 kHz,
//   44.1 kHz…) down to the 16 kHz the PC's speech models take. The answer:
//   Yap's voice, 24 kHz, up to the phone's output rate, carried on from one
//   frame to the next (never a clip at a time). Positions are kept in whole
//   numbers, so 48,000 samples in give exactly 16,000 out, and 24,000 give
//   exactly 44,100 at 44.1 kHz.
// - `Ring`: a first-in, first-out queue of samples that grows as it needs.
// - `Playout`: the answer's jitter buffer, on the audio thread: the sound
//   starts once 250 ms is waiting (or the answer's end came), running dry
//   fades out over 5 ms, waits for 150 ms more, then fades back in (each
//   such gap counted), Stop fades out over 5 ms.
// - `toInt16` / `fromInt16`: float samples to Int16, clamped, and back.
// - `rms` / `toDb`: a frame's level, for the meter and the voice detector.
// - `EnergyVad`: hands-free's end of speech, by energy: speech once it's
//   heard for a moment above the room's noise, kept while the level stays
//   a little above it (a soft last word isn't quiet), then the end after the
//   wait chosen (`WAITS`: 1 s, 1.5 s or 2.5 s) of quiet.
// - `elapsed`: a time as Chat's activity line says it ("12s", "1m 59s").

/** The rate the PC takes the phone's mic at. */
export const LINK_RATE = 16000;

/** One frame of the mic sent up: 20 ms at 16 kHz. */
export const FRAME_SAMPLES = 320;

/** The rate of Yap's voice coming down (`speak_start`'s, wire::DOWN_RATE). */
export const DOWN_RATE = 24000;

/** Hands-free's wait before sending, by choice, in ms. */
export const WAITS = { short: 1000, normal: 1500, long: 2500 };

/** A filter's phases worked out ahead at most (a rate pair with more is interpolated). */
const MAX_PHASES = 1024;

const gcd = (a, b) => {
  while (b) [a, b] = [b, a % b];
  return a;
};

export class Resampler {
  /**
   * @param {number} inRate the input's rate (the mic's context; the voice's 24 kHz)
   * @param {number} outRate the rate wanted (16 kHz; the phone's output rate)
   * @param {number} zeroCrossings the filter's half-length in zero crossings
   */
  constructor(inRate, outRate = LINK_RATE, zeroCrossings = 16) {
    this.inRate = Math.round(inRate);
    this.outRate = Math.round(outRate);
    if (!(this.inRate > 0 && this.outRate > 0)) throw new RangeError('Resampler: rates must be positive');
    const ratio = this.inRate / this.outRate;
    // The cut-off as a fraction of the input's Nyquist: under the output's
    // Nyquist going down (no aliasing), under the input's going up (no
    // images). At 48 kHz → 16 kHz that's 106 taps: flat to ~6 kHz, −40 dB by
    // 10 kHz. At 24 kHz → 48 kHz, 36: flat to ~9 kHz, the images gone.
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
    // An output sample falls between two input samples at one of
    // outRate / gcd places (2 for 24 → 48 kHz, 147 for 24 → 44.1 kHz, 1 for
    // 48 → 16 kHz): each place's taps worked out once (a polyphase filter),
    // the same values the table gives, without the lookups.
    this.step = gcd(this.inRate, this.outRate);
    const phases = this.outRate / this.step;
    const taps = 2 * this.H;
    this.poly = null;
    if (phases <= MAX_PHASES) {
      this.poly = new Float32Array(phases * taps);
      for (let p = 0; p < phases; p++) {
        const frac = (p * this.step) / this.outRate;
        for (let k = 0; k < taps; k++) this.poly[p * taps + k] = this._kernel(k - this.H + 1 - frac);
      }
    }
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
    const { inRate, outRate, H, buf, poly, step } = this;
    const taps = 2 * H;
    const most = Math.ceil((this.len / inRate) * outRate) + 2;
    if (this.out.length < most) this.out = new Float32Array(most);
    const out = this.out;
    let made = 0;
    for (;;) {
      const num = this.n * inRate; // where output n sits, × outRate (exact)
      const centre = Math.floor(num / outRate);
      if (centre + H - this.base >= this.len) break;
      const rem = num - centre * outRate;
      const start = centre - H + 1 - this.base;
      let acc = 0;
      if (poly) {
        const at = (rem / step) * taps;
        for (let k = 0; k < taps; k++) acc += buf[start + k] * poly[at + k];
      } else {
        const frac = rem / outRate;
        for (let k = 0; k < taps; k++) acc += buf[start + k] * this._kernel(k - H + 1 - frac);
      }
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

/** Samples first in, first out, in a ring that doubles when it's full. */
export class Ring {
  constructor(capacity = 16384) {
    this.buf = new Float32Array(Math.max(16, capacity));
    this.head = 0; // the next sample out
    this.length = 0;
  }

  clear() {
    this.head = 0;
    this.length = 0;
  }

  /** @param {Float32Array} samples */
  write(samples) {
    const n = samples.length;
    if (this.length + n > this.buf.length) {
      let cap = this.buf.length;
      while (cap < this.length + n) cap *= 2;
      const grown = new Float32Array(cap);
      this.read(grown.subarray(0, this.length), true);
      this.buf = grown;
      this.head = 0;
    }
    const cap = this.buf.length;
    const tail = (this.head + this.length) % cap;
    const first = Math.min(n, cap - tail);
    this.buf.set(samples.subarray(0, first), tail);
    if (first < n) this.buf.set(samples.subarray(first), 0);
    this.length += n;
  }

  /** Copies the oldest `dst.length` samples into `dst`; `keep` leaves them in. */
  read(dst, keep = false) {
    const n = Math.min(dst.length, this.length);
    const cap = this.buf.length;
    const first = Math.min(n, cap - this.head);
    dst.set(this.buf.subarray(this.head, this.head + first));
    if (first < n) dst.set(this.buf.subarray(0, n - first), first);
    if (!keep) {
      this.head = (this.head + n) % cap;
      this.length -= n;
    }
    return n;
  }

  /** The oldest sample, taken out (0 when empty). */
  shift() {
    if (!this.length) return 0;
    const s = this.buf[this.head];
    this.head = this.head + 1 === this.buf.length ? 0 : this.head + 1;
    this.length--;
    return s;
  }
}

/**
 * The answer's voice as one stream, on the audio thread (player-worklet.js):
 * samples at the output's rate go in as they come (`push`, bursts from the
 * network), the speaker takes them out at its steady pace (`pull`, 128 at a
 * time).
 *
 * idle → `begin` → buffering: silent until `startMs` is waiting, or `end`
 * came (a short answer) → playing, faded in over `fadeMs`. Running dry
 * (nothing left and no `end` yet): the last `fadeMs` fades out, an underrun
 * is counted, and it's starved: silent until `rebufferMs` more is waiting
 * (or the end), then fades in again. A near miss (the buffer nearly empty,
 * then filled in time) only dips. `end`: what's left plays out (its last
 * `fadeMs` faded), then 'done' and idle. `stop`: silent within `fadeMs` (a
 * fade, not a cut), what's left thrown away, 'stopped' and idle.
 *
 * Events (`take()`): 'start', 'underrun', 'resume', 'done', 'stopped'.
 */
export class Playout {
  /**
   * @param {number} rate the output's rate (the AudioContext's)
   */
  constructor(rate, { startMs = 250, rebufferMs = 150, fadeMs = 5 } = {}) {
    this.rate = rate;
    this.startLen = Math.round((rate * startMs) / 1000);
    this.rebufferLen = Math.round((rate * rebufferMs) / 1000);
    this.fadeLen = Math.max(1, Math.round((rate * fadeMs) / 1000));
    this.ring = new Ring(Math.max(16384, this.startLen * 4));
    this.state = 'idle';
    this.events = [];
    this._fresh();
  }

  _fresh() {
    this.ring.clear();
    this.ended = false;
    this.gain = 0;
    this.prev = 0;
    this.stopLeft = 0;
    this.queued = 0; // samples pushed this answer
    this.played = 0; // samples taken out to play
    this.underruns = 0;
    this.waited = 0; // samples of silence between the first push and the start
    this.startDelay = -1; // that, once it started
    this.startBuffered = -1; // what was waiting when it started
    this.gapSamples = 0; // silence while starved
    this.maxStep = 0; // the largest jump between two samples out: a hard cut shows here
    this.peak = 0; // the loudest sample out since `level()`
  }

  /** A new answer: everything before it gone. */
  begin() {
    this._fresh();
    this.state = 'buffering';
  }

  /** @param {Float32Array} samples at `rate` */
  push(samples) {
    if (this.state === 'idle' || this.state === 'stopping' || this.ended || !samples.length) return;
    this.ring.write(samples);
    this.queued += samples.length;
  }

  /** No more samples for this answer: play out what's left. */
  end() {
    if (this.state !== 'idle' && this.state !== 'stopping') this.ended = true;
  }

  /** Silent at once (a `fadeMs` fade), the rest thrown away. */
  stop() {
    if (this.state === 'idle' || this.state === 'stopping') return;
    if (this.state === 'playing' && this.gain > 0) {
      this.state = 'stopping';
      this.stopLeft = this.fadeLen;
    } else this._finish('stopped');
  }

  _finish(event) {
    this.ring.clear();
    this.state = 'idle';
    this.gain = 0;
    this.events.push(event);
  }

  /** Seconds waiting to be played. */
  get buffered() {
    return this.ring.length / this.rate;
  }

  /** The events since the last call. */
  take() {
    const e = this.events;
    this.events = [];
    return e;
  }

  /** The loudest sample out since the last call. */
  level() {
    const p = this.peak;
    this.peak = 0;
    return p;
  }

  /** Fills `out` with the next samples to play (silence when there's none). */
  pull(out) {
    const ring = this.ring;
    const F = this.fadeLen;
    const up = 1 / F;
    let prev = this.prev;
    for (let i = 0; i < out.length; i++) {
      let s = 0;
      const state = this.state;
      if (state === 'buffering' || state === 'starved') {
        const avail = ring.length;
        const need = state === 'buffering' ? this.startLen : this.rebufferLen;
        if (avail >= need || (this.ended && avail > 0)) {
          if (state === 'buffering') {
            this.startDelay = this.waited;
            this.startBuffered = avail;
            this.events.push('start');
          } else this.events.push('resume');
          this.state = 'playing';
          this.gain = 0;
        } else if (this.ended) {
          // The end, and nothing (more) to play.
          this._finish('done');
        } else if (state === 'buffering') {
          if (this.queued > 0) this.waited++;
        } else this.gapSamples++;
      }
      if (this.state === 'playing') {
        s = ring.shift();
        const left = ring.length;
        let g = this.gain + up;
        if (g > 1) g = 1;
        // Nearly dry: fade so the last sample there is lands at silence.
        if (left < F && left / F < g) g = left / F;
        this.gain = g;
        s *= g;
        this.played++;
        if (left === 0) {
          if (this.ended) this._finish('done');
          else {
            this.state = 'starved';
            this.underruns++;
            this.events.push('underrun');
          }
        }
      } else if (this.state === 'stopping') {
        s = ring.shift();
        this.stopLeft--;
        const g = Math.min(this.gain, this.stopLeft / F);
        this.gain = g;
        s *= g;
        if (this.stopLeft <= 0) this._finish('stopped');
      }
      out[i] = s;
      const d = s > prev ? s - prev : prev - s;
      if (d > this.maxStep) this.maxStep = d;
      const a = s < 0 ? -s : s;
      if (a > this.peak) this.peak = a;
      prev = s;
    }
    this.prev = prev;
  }

  /** What the page shows and the tests read, in seconds and ms. */
  stats() {
    const ms = (n) => (n < 0 ? null : Math.round((n / this.rate) * 1000));
    return {
      state: this.state,
      queued: this.queued / this.rate,
      played: this.played / this.rate,
      buffered: this.buffered,
      underruns: this.underruns,
      startDelayMs: ms(this.startDelay),
      startBufferedMs: ms(this.startBuffered),
      gapMs: ms(this.gapSamples),
      maxStep: this.maxStep,
    };
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
 * for the first half-second and never above −30 dB. Speech starts 12 dB over
 * it, and over −45 dB at least. Once it has started, it carries on while the
 * level stays 6 dB over the noise, and over −52 dB (hysteresis): a soft last
 * word, or one trailing off, isn't taken for quiet.
 *
 * For that hold, the noise is also taken as the room's typical level before
 * speech began (the median of those frames, given half a second of them):
 * a room whose noise flutters (a TV, traffic) has peaks more than 6 dB over
 * its quietest moment, which would otherwise keep "speech" going for ever.
 */
export class EnergyVad {
  constructor({
    frameMs = 20,
    endMs = WAITS.normal,
    minSpeechMs = 160,
    marginDb = 12,
    floorDb = -45,
    holdMarginDb = 6,
    holdFloorDb = -52,
    windowMs = 3000,
  } = {}) {
    this.frameMs = frameMs;
    this.endFrames = Math.round(endMs / frameMs);
    this.minSpeechFrames = Math.max(1, Math.round(minSpeechMs / frameMs));
    this.settleFrames = Math.round(500 / frameMs);
    this.marginDb = marginDb;
    this.floorDb = floorDb;
    this.holdMarginDb = holdMarginDb;
    this.holdFloorDb = holdFloorDb;
    this.window = new Float32Array(Math.max(1, Math.round(windowMs / frameMs)));
    this.reset();
  }

  reset() {
    this.count = 0;
    this.noiseDb = -50;
    this.roomDb = -Infinity;
    this.speechRun = 0;
    this.quiet = 0;
    this.heard = false;
    this.ended = false;
  }

  /** The level speech has to clear to start, just now. */
  get thresholdDb() {
    return Math.max(this.noiseDb + this.marginDb, this.floorDb);
  }

  /** The level that keeps speech going once it has started, just now. */
  get holdDb() {
    return Math.max(Math.max(this.noiseDb, this.roomDb) + this.holdMarginDb, this.holdFloorDb);
  }

  /** The median of the window's frames before the speech's start (−∞ with under half a second of them). */
  _room() {
    const len = this.window.length;
    const n = Math.min(this.count, len) - 2 * this.minSpeechFrames;
    if (n < this.settleFrames) return -Infinity;
    const before = new Float32Array(n);
    for (let j = 0; j < n; j++) before[j] = this.window[(this.count - 1 - 2 * this.minSpeechFrames - j + len * 2) % len];
    before.sort();
    return Math.min(before[n >> 1], -30);
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
    if (!this.heard) {
      // A cough or a click isn't speech: it has to last.
      const loud = db > this.thresholdDb;
      this.speechRun = loud ? this.speechRun + 1 : Math.max(0, this.speechRun - 1);
      if (this.speechRun >= this.minSpeechFrames) {
        this.heard = true;
        this.quiet = 0;
        this.roomDb = this._room();
        return 'speech';
      }
      return null;
    }
    this.quiet = db > this.holdDb ? 0 : this.quiet + 1;
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
