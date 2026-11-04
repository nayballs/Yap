// Yap Link's voice, on the audio thread: the answer as one stream. The page
// resamples each down frame (24 kHz) to the output's rate as it comes, in one
// filter carried on from frame to frame (dsp.js `Resampler`), and posts the
// samples here; this plays them out of a jitter buffer (dsp.js `Playout`):
// starting once 250 ms is waiting, fading out over 5 ms when it runs dry and
// back in once 150 ms more has come, silent within 5 ms on Stop.
//
// Not an AudioBufferSourceNode per frame, as before: each clip was resampled
// on its own (a click at every join), and a late one left a hard gap.
//
// Messages from the page, each with the answer's `id`: {cmd: 'begin'},
// {cmd: 'push', samples: ArrayBuffer (Float32 at the context's rate)},
// {cmd: 'end'} (play out what's left), {cmd: 'stop'}. To the page: {id,
// stats, events, level} on every event and every ~50 ms while there's an
// answer (a few more after, so the page sees the silence), `events` being
// 'start', 'underrun', 'resume', 'done' (the end played out) and 'stopped'.
import { Playout } from './dsp.js';

const REPORT_S = 0.05;
const AFTER = 4; // reports once idle, so the last of the sound is seen gone

class YapPlayer extends AudioWorkletProcessor {
  constructor() {
    super();
    // `sampleRate` is the AudioWorkletGlobalScope's: the context's rate.
    this.p = new Playout(sampleRate);
    this.id = 0;
    this.every = Math.max(1, Math.round((REPORT_S * sampleRate) / 128));
    this.tick = 0;
    this.after = 0;
    this.port.onmessage = (e) => {
      const d = e.data || {};
      if (d.cmd === 'begin') {
        this.id = d.id;
        this.p.begin();
        this.after = AFTER;
        this.report();
        return;
      }
      if (d.id !== this.id) return; // an answer since replaced
      if (d.cmd === 'push' && d.samples) this.p.push(new Float32Array(d.samples));
      else if (d.cmd === 'end') this.p.end();
      else if (d.cmd === 'stop') this.p.stop();
    };
  }

  report() {
    this.tick = 0;
    this.port.postMessage({ id: this.id, stats: this.p.stats(), events: this.p.take(), level: this.p.level() });
  }

  process(_inputs, outputs) {
    const out = outputs[0];
    if (!out || !out[0]) return true;
    this.p.pull(out[0]);
    for (let c = 1; c < out.length; c++) out[c].set(out[0]);
    const idle = this.p.state === 'idle';
    if (!idle) this.after = AFTER;
    if (this.p.events.length) this.report();
    else if ((!idle || this.after > 0) && ++this.tick >= this.every) {
      if (idle) this.after--;
      this.report();
    }
    return true;
  }
}

registerProcessor('yap-player', YapPlayer);
