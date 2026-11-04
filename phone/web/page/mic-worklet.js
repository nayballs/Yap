// Yap Link's microphone, on the audio thread: the mic at the phone's own
// rate (the AudioContext's, usually 48 kHz), mixed to mono, resampled to
// 16 kHz and cut into 20 ms frames of Int16 for the PC.
//
// Not `new AudioContext({sampleRate: 16000})`: iOS delivers only ~12,000
// samples a second from a 16 kHz context (WebKit bug 251350), and a phone's
// mic runs at 48 kHz anyway (phone-remote design, §3.2).
//
// Messages from the page: {cmd: 'start'} begins a question's frames from a
// clean filter; {cmd: 'stop'} sends the last part-frame (padded with
// silence to a whole frame) marked `last`, then nothing until the next
// start. To the page: {frame: ArrayBuffer (Int16 × 320), level, last}, and
// a bare {last: true} when a stop finds no part-frame to send.
import { Resampler, toInt16, rms, FRAME_SAMPLES, LINK_RATE } from './dsp.js';

class YapMic extends AudioWorkletProcessor {
  constructor() {
    super();
    // `sampleRate` is the AudioWorkletGlobalScope's: the context's rate.
    this.rs = new Resampler(sampleRate, LINK_RATE);
    this.frame = new Int16Array(FRAME_SAMPLES);
    this.fill = 0;
    this.on = false;
    this.mono = new Float32Array(128);
    this.port.onmessage = (e) => {
      const cmd = e.data && e.data.cmd;
      if (cmd === 'start') {
        this.rs.reset();
        this.fill = 0;
        this.on = true;
      } else if (cmd === 'stop') {
        // The part-frame, if there is one; either way the page hears `last`.
        if (this.on && this.fill > 0) this.send(true);
        else this.port.postMessage({ last: true });
        this.on = false;
      }
    };
  }

  /** Posts the frame (zero-padded when it's the last part of one). */
  send(last) {
    if (last) this.frame.fill(0, this.fill);
    const level = rms(this.frame.subarray(0, Math.max(1, this.fill)));
    const buf = this.frame.buffer;
    this.port.postMessage({ frame: buf, level, last: !!last }, [buf]);
    this.frame = new Int16Array(FRAME_SAMPLES);
    this.fill = 0;
  }

  process(inputs) {
    const input = inputs[0];
    if (!this.on || !input || input.length === 0 || !input[0]) return true;
    const n = input[0].length;
    if (this.mono.length < n) this.mono = new Float32Array(n);
    const mono = this.mono.subarray(0, n);
    if (input.length === 1) mono.set(input[0]);
    else {
      mono.fill(0);
      for (const ch of input) for (let i = 0; i < n; i++) mono[i] += ch[i] / input.length;
    }
    const out = this.rs.process(mono);
    let i = 0;
    while (i < out.length) {
      const take = Math.min(out.length - i, FRAME_SAMPLES - this.fill);
      toInt16(out.subarray(i, i + take), this.frame, this.fill);
      this.fill += take;
      i += take;
      if (this.fill === FRAME_SAMPLES) this.send(false);
    }
    return true;
  }
}

registerProcessor('yap-mic', YapMic);
