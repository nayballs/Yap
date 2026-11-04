// The answer's sound on the phone: the browser's media player, the standard
// way (pieces cut at its pauses, played by <audio>), and Web Audio — one
// stream from a jitter buffer (player-worklet.js, dsp.js `Playout`) — as
// the automatic fallback where the browser refuses the media player (a play
// refused, an error, a piece that never starts: `mediaPlayer`); "Clearer
// sound in a headset" (on by default: the mic closed while Yap talks) and
// Sound details. The jitter buffer's own tests reach it through that
// fallback (`refuseMediaPlayer`). Yap on the PC mocked, its Done sending
// the question to answer out loud (as a PC from before words does;
// `CAN_BEFORE_WORDS`); the mic the looped 440 Hz tone.
import { test, expect } from '@playwright/test';
import {
  servePage,
  seedDevice,
  mockPc as mockAnyPc,
  open,
  ready,
  ask,
  snapshot,
  downFrame,
  voice,
  sentences,
  openSettings,
  closeSettings,
  mediaPlayer,
  refuseMediaPlayer,
  CAN_BEFORE_WORDS,
} from './support/harness.js';

/** A PC whose Done sends the spoken question, and says its answer. */
const mockPc = (page, opts = {}) => mockAnyPc(page, { can: CAN_BEFORE_WORDS, ...opts });

const player = async (page) => (await snapshot(page)).player;

/** Every audio track the page's getUserMedia handed out, to see them stop. */
async function watchTracks(page) {
  await page.addInitScript(() => {
    window.__tracks = [];
    const md = navigator.mediaDevices;
    const real = md.getUserMedia.bind(md);
    md.getUserMedia = async (c) => {
      const s = await real(c);
      window.__tracks.push(...s.getAudioTracks());
      return s;
    };
  });
  return {
    opened: () => page.evaluate(() => window.__tracks.length),
    live: () => page.evaluate(() => window.__tracks.filter((t) => t.readyState === 'live').length),
  };
}

/** "Clearer sound in a headset" as if the person had chosen it before (once: a later visit keeps what the page saved). */
async function chooseClearer(page, on) {
  await page.addInitScript((on) => {
    if (!sessionStorage.getItem('seeded-clearer')) {
      localStorage.setItem('yap.link.clearer', JSON.stringify(on));
      sessionStorage.setItem('seeded-clearer', '1');
    }
  }, on);
}

const CLEARER_HINT =
  "A headset's call mode can crackle, so this closes the mic while Yap talks and the headset uses its music mode. You can't talk over Yap by voice; Stop still works.";

/** An answer: speak_start, `n` frames of `ms` each (a steady tone), speak_end, idle. */
function speak(pc, n, ms = 100, end = true) {
  pc.send({ type: 'state', phase: 'speaking' });
  pc.send({ type: 'speak_start', rate: 24000 });
  for (let i = 0; i < n; i++) pc.sendBinary(downFrame(i, voice(24000, ms)));
  if (end) {
    pc.send({ type: 'speak_end' });
    pc.send({ type: 'state', phase: 'idle' });
  }
}

/** A long answer, still coming: `ms` of it in sentences (the media player plays its pieces as they come). */
function speakOn(pc, ms = 3000) {
  pc.send({ type: 'state', phase: 'speaking' });
  pc.send({ type: 'speak_start', rate: 24000 });
  for (const f of sentences(ms)) pc.sendBinary(f);
}

test.beforeEach(async ({ page }) => {
  await servePage(page);
  await seedDevice(page);
});

test('Web Audio (the fallback): playback starts once about 250 ms is waiting, not before', async ({ page }) => {
  await refuseMediaPlayer(page);
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await ask(page, pc);
  pc.send({ type: 'state', phase: 'speaking' });
  pc.send({ type: 'speak_start', rate: 24000 });
  // 200 ms, a frame at a time: still waiting, silent.
  for (let i = 0; i < 4; i++) pc.sendBinary(downFrame(i, voice(24000, 50)));
  await expect.poll(async () => (await player(page)).queued).toBeCloseTo(0.2, 6);
  await page.waitForTimeout(500);
  let p = await player(page);
  expect(p.state).toBe('buffering');
  expect(p.played).toBe(0);
  expect(p.level).toBe(0);
  await expect(page.getByTestId('phase')).toHaveText('Speaking');
  // 60 ms more (260 ms): it starts, with what was waiting.
  pc.sendBinary(downFrame(4, voice(24000, 60)));
  await expect.poll(async () => (await player(page)).state).toBe('playing');
  p = await player(page);
  expect(p.startBufferedMs).toBeGreaterThanOrEqual(250);
  expect(p.startBufferedMs).toBeLessThan(270);
  expect(p.startDelayMs).toBeGreaterThan(400); // from the first frame's arrival
  pc.send({ type: 'speak_end' });
  pc.send({ type: 'state', phase: 'idle' });
  await expect.poll(async () => (await player(page)).active, { timeout: 4000 }).toBe(false);
  p = await player(page);
  expect(p.played).toBeCloseTo(0.26, 3);
  expect(p.underruns).toBe(0);
});

test('a short answer (under 250 ms, then speak_end) still plays', async ({ page }) => {
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await ask(page, pc);
  speak(pc, 1, 120);
  await expect.poll(async () => (await player(page)).active, { timeout: 4000 }).toBe(false);
  const p = await player(page);
  expect(p.played).toBeCloseTo(0.12, 3);
  expect(p.startDelayMs).not.toBeNull();
  await expect(page.getByTestId('phase')).toHaveText('Ready');
});

test('Web Audio (the fallback), a late frame: one underrun, faded out and back in, never a hard cut', async ({ page }) => {
  await refuseMediaPlayer(page);
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await ask(page, pc);
  speak(pc, 3, 100, false);
  await expect.poll(async () => (await player(page)).state).toBe('playing');
  // 300 ms play out; the next frame is late: the sound runs dry.
  await expect.poll(async () => (await player(page)).state, { timeout: 3000 }).toBe('starved');
  let p = await player(page);
  expect(p.underruns).toBe(1);
  expect(p.played).toBeCloseTo(0.3, 2);
  await expect(page.getByTestId('phase')).toHaveText('Speaking');
  // 200 ms more (over the 150 ms it waits for): back, faded in.
  for (let i = 3; i < 5; i++) pc.sendBinary(downFrame(i, voice(24000, 100)));
  await expect.poll(async () => (await player(page)).state).toBe('playing');
  pc.send({ type: 'speak_end' });
  pc.send({ type: 'state', phase: 'idle' });
  await expect.poll(async () => (await player(page)).active, { timeout: 4000 }).toBe(false);
  p = await player(page);
  expect(p.underruns).toBe(1);
  expect(p.gapMs).toBeGreaterThan(0);
  expect(p.played).toBeCloseTo(0.5, 3);
  // The tone's own steps are under 0.006 at 44.1/48 kHz; a cut at full
  // volume would be about 0.18.
  expect(p.maxStep).toBeLessThan(0.03);
  // Sound details say so (the settings sheet, ⋯).
  await openSettings(page);
  await page.getByText('Sound details').click();
  await expect(page.getByTestId('sound-gaps')).toHaveText('1 (the sound ran dry and paused)');
});

test('Clearer sound in a headset: on by default; a saved choice is kept', async ({ page }) => {
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  // Nobody's chosen yet: on, and nothing saved until they do (the settings sheet, ⋯).
  await openSettings(page);
  const sw = page.getByTestId('clearer');
  await expect(sw).toBeChecked();
  expect((await snapshot(page)).clearer).toBe(true);
  expect(await page.evaluate(() => localStorage.getItem('yap.link.clearer'))).toBeNull();
  await expect(page.getByText(CLEARER_HINT)).toBeVisible();

  // Turned off: saved, and still off on the next visit.
  await page.getByText('Clearer sound in a headset').click();
  await expect(sw).not.toBeChecked();
  expect(await page.evaluate(() => localStorage.getItem('yap.link.clearer'))).toBe('false');
  await open(page);
  await ready(page, pc);
  await expect(page.getByTestId('clearer')).not.toBeChecked();
  expect((await snapshot(page)).clearer).toBe(false);

  // On again: saved as on.
  await openSettings(page);
  await page.getByText('Clearer sound in a headset').click();
  expect(await page.evaluate(() => localStorage.getItem('yap.link.clearer'))).toBe('true');
  await open(page);
  await ready(page, pc);
  await expect(page.getByTestId('clearer')).toBeChecked();
});

test('Clearer sound off (a saved choice): talking over Yap uses the mic already open', async ({ page }) => {
  await chooseClearer(page, false);
  const tracks = await watchTracks(page);
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await expect(page.getByTestId('clearer')).not.toBeChecked();
  await ask(page, pc);
  speakOn(pc);
  await expect.poll(async () => (await player(page)).state).toBe('playing');
  // The mic open while Yap talks (the headset in call mode)…
  expect(await tracks.live()).toBe(1);
  const opened = await tracks.opened();
  // …so Talk listens at once, on the same mic: interrupt, then start.
  await page.getByTestId('talk').click();
  await expect.poll(() => pc.of('start').length).toBe(2);
  expect(pc.types().filter((t) => t !== 'ping').slice(-2)).toEqual(['interrupt', 'start']);
  await expect(page.getByTestId('phase')).toHaveText('Listening');
  expect(await tracks.opened()).toBe(opened);
  expect(await tracks.live()).toBe(1);
});

test('Clearer sound on (the default): talking over Yap opens the mic afresh, then listens', async ({ page }) => {
  const tracks = await watchTracks(page);
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await ask(page, pc);
  speakOn(pc);
  await expect.poll(async () => (await player(page)).state).toBe('playing');
  expect(await tracks.live()).toBe(0);
  const opened = await tracks.opened();
  await page.getByTestId('talk').click();
  await expect.poll(() => pc.of('start').length).toBe(2);
  expect(pc.types().filter((t) => t !== 'ping').slice(-2)).toEqual(['interrupt', 'start']);
  await expect(page.getByTestId('phase')).toHaveText('Listening');
  expect(await tracks.opened()).toBe(opened + 1);
  const frames = pc.frames.length;
  await expect.poll(() => pc.frames.length - frames).toBeGreaterThan(10);
});

test('Clearer sound in a headset: the mic let go while Yap talks, open again after', async ({ page }) => {
  await chooseClearer(page, false);
  const tracks = await watchTracks(page);
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  const sw = page.getByTestId('clearer');
  await expect(sw).not.toBeChecked();

  // Off: the mic stays open through the answer (the headset in call mode).
  await ask(page, pc);
  expect(await tracks.live()).toBe(1);
  pc.send({ type: 'state', phase: 'thinking' });
  // Switched on while Yap works on it: the mic's still open…
  await openSettings(page);
  await page.getByText('Clearer sound in a headset').click();
  await expect(sw).toBeChecked();
  await closeSettings(page);
  expect(await page.evaluate(() => localStorage.getItem('yap.link.clearer'))).toBe('true');
  expect(await tracks.live()).toBe(1);
  // …until the answer starts: then its tracks stop.
  speak(pc, 6, 100);
  await expect.poll(() => tracks.live()).toBe(0);
  expect((await snapshot(page)).micOpen).toBe(false);
  await expect.poll(async () => (await player(page)).active, { timeout: 4000 }).toBe(false);
  // It stays closed until Talk (hands-free talk opens it for its next turn: handsfree.spec.js).
  await page.waitForTimeout(300);
  expect(await tracks.live()).toBe(0);
  let last = (await player(page)).last;
  expect(last.micClosed).toBe(true);

  // Talk opens it again; sending the question lets it go at once (the headset
  // switches back while Yap works it out).
  const opened = await tracks.opened();
  await ask(page, pc);
  expect(await tracks.opened()).toBe(opened + 1);
  await expect.poll(() => tracks.live()).toBe(0);
  pc.send({ type: 'state', phase: 'thinking' });
  speak(pc, 4, 100);
  await expect.poll(async () => (await player(page)).active, { timeout: 4000 }).toBe(false);
  last = (await player(page)).last;
  expect(last.micOpen).toBe(false); // never open while it played
  expect(last.micClosed).toBe(true);
  await openSettings(page);
  await page.getByText('Sound details').click();
  await expect(page.getByTestId('sound-mic')).toHaveText('Closed');
  await expect(page.getByTestId('sound-mic-now')).toHaveText('Closed');

  // Remembered on the next visit.
  await open(page);
  await ready(page, pc);
  await expect(page.getByTestId('clearer')).toBeChecked();
});

test('Sound details: the rate, latency, the last answer, the mic and the browser', async ({ page }) => {
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  const sound = page.getByTestId('sound');
  await expect(sound).not.toHaveAttribute('open', '');
  await openSettings(page);
  await page.getByText('Sound details').click();
  await expect(page.getByTestId('sound-rate')).toHaveText('Shown once you’ve asked something');
  await expect(page.getByTestId('sound-start')).toHaveText('No answer yet');
  await expect(page.getByTestId('sound-mic-now')).toHaveText('Closed');
  await expect(page.getByTestId('sound-mic-step')).toHaveText('Not opened yet');
  await expect(page.getByTestId('sound-browser')).toHaveText(/^(Chrome|Edge|Firefox) \d+ on Windows$/);

  // While the mic opens, the step it's on; then the last one and the whole time.
  await closeSettings(page);
  await page.getByTestId('talk').click();
  await expect(page.getByTestId('sound-mic-step')).toHaveText(/^Waiting for sound from the mic: \d+ ms \(open after [\d,.]+ (ms|s)\)$/);
  await expect.poll(() => pc.frames.length).toBeGreaterThan(20);
  await page.getByTestId('talk').click();
  await expect.poll(() => pc.of('stop').length).toBe(1);
  expect((await snapshot(page)).micStep).toMatchObject({ name: 'live', state: 'open' });
  speak(pc, 6, 100);
  await expect.poll(async () => (await player(page)).active, { timeout: 4000 }).toBe(false);
  await openSettings(page);
  await expect(page.getByTestId('sound-rate')).toHaveText(/^(44,100|48,000) Hz$/);
  await expect(page.getByTestId('sound-latency')).toHaveText(/^(Base \d+ ms( · Output \d+ ms)?|Output \d+ ms|Not given by this browser)$/);
  await expect(page.getByTestId('sound-start')).toHaveText(/^0\.6 s played; started after \d+ ms, with \d{3} ms buffered$/);
  await expect(page.getByTestId('sound-gaps')).toHaveText('None');
  // Which way it played: the media player, the standard way.
  await expect(page.getByTestId('sound-via')).toHaveText('The media player (more sound kept ready)');
  // Clearer sound (on by default): the mic let go while it played.
  await expect(page.getByTestId('sound-mic')).toHaveText('Closed');
  await expect(page.getByTestId('sound-mic-now')).toHaveText('Closed');
  // Each row as a term and its value, for a screen reader too.
  const terms = await sound.locator('dt').allTextContents();
  expect(terms).toEqual([
    'Sample rate',
    'Latency',
    'Played through',
    'Last answer',
    'Gaps in it',
    'Microphone while it played',
    'Microphone now',
    'Opening the microphone',
    'Browser',
  ]);
});

test('Sound details, the mic open while it played (Clearer sound off)', async ({ page }) => {
  await chooseClearer(page, false);
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await openSettings(page);
  await page.getByText('Sound details').click();
  await closeSettings(page);
  await ask(page, pc);
  speak(pc, 6, 100);
  await expect.poll(async () => (await player(page)).active, { timeout: 4000 }).toBe(false);
  await expect(page.getByTestId('sound-mic')).toHaveText('Open (the headset in call mode)');
  await expect(page.getByTestId('sound-mic-now')).toHaveText('Open');
});

/** `ms` of quiet, as Int16. */
const quiet = (rate, ms) => new Int16Array(Math.round((rate * ms) / 1000));

/** Two sentences with a pause between (the media player's pieces are cut there): `speak_start`, then the frames. 1.6 s in all. */
function speakSentences(pc, end = true) {
  pc.send({ type: 'state', phase: 'speaking' });
  pc.send({ type: 'speak_start', rate: 24000 });
  pc.sendBinary(downFrame(0, voice(24000, 700)));
  pc.sendBinary(downFrame(1, quiet(24000, 200)));
  pc.sendBinary(downFrame(2, voice(24000, 700)));
  if (end) {
    pc.send({ type: 'speak_end' });
    pc.send({ type: 'state', phase: 'idle' });
  }
}

/** The answer's first sentence and its pause, playing through the media player; the rest to come. */
async function firstPiecePlaying(page, pc) {
  pc.send({ type: 'state', phase: 'speaking' });
  pc.send({ type: 'speak_start', rate: 24000 });
  pc.sendBinary(downFrame(0, voice(24000, 700)));
  pc.sendBinary(downFrame(1, quiet(24000, 200)));
  await expect.poll(async () => (await player(page)).state).toBe('playing');
  expect((await player(page)).via).toBe('media');
}

/** The rest of that answer: its second sentence, the end. */
function theRest(pc) {
  pc.sendBinary(downFrame(2, voice(24000, 700)));
  pc.send({ type: 'speak_end' });
  pc.send({ type: 'state', phase: 'idle' });
}

test('the media player, the standard way: an answer in pieces, played to its end, Sound details say so; no switch for it', async ({ page }) => {
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  // No setting for it now: it's how Yap's voice plays.
  await openSettings(page);
  await expect(page.getByText('Play the voice through the media player')).toHaveCount(0);
  await page.getByText('Sound details').click();
  await expect(page.getByTestId('sound-via')).toHaveText('The media player (more sound kept ready)');
  await closeSettings(page);

  await ask(page, pc);
  // Readied within the tap: it played a moment of silence there.
  await expect.poll(async () => (await snapshot(page)).voice.unlocked).toBe(true);
  speakSentences(pc);
  await expect.poll(async () => (await player(page)).state).toBe('playing');
  let p = await player(page);
  expect(p.via).toBe('media');
  await expect(page.getByTestId('phase')).toHaveText('Speaking');
  await expect.poll(async () => (await player(page)).active, { timeout: 6000 }).toBe(false);
  p = await player(page);
  expect(p.played).toBeGreaterThan(1.45);
  expect(p.played).toBeLessThan(1.75);
  expect(p.underruns).toBe(0);
  expect(p.pieces).toBe(0);
  expect(p.state).toBe('idle');
  await expect(page.getByTestId('phase')).toHaveText('Ready');
  await openSettings(page);
  await expect(page.getByTestId('sound-via')).toHaveText('The media player (more sound kept ready)');
  await expect(page.getByTestId('sound-start')).toHaveText(/^1\.[5-7] s played; started after \d+ ms, with \d+ ms buffered$/);
  await expect(page.getByTestId('sound-gaps')).toHaveText('None');
  expect((await snapshot(page)).voice).toMatchObject({ via: 'media', why: '' });
});

test.describe('a phone that chose before 4 Nov', () => {
  // A real phone's Firefox (Gecko's Android user agent).
  test.use({ userAgent: 'Mozilla/5.0 (Android 15; Mobile; rv:144.0) Gecko/144.0 Firefox/144.0' });

  test('"Play the voice through the media player" turned off then: ignored, the media player all the same', async ({ page }) => {
    await page.addInitScript(() => {
      if (!sessionStorage.getItem('chose-media')) {
        localStorage.setItem('yap.link.media', 'false');
        sessionStorage.setItem('chose-media', '1');
      }
    });
    const pc = await mockPc(page);
    await open(page);
    await ready(page, pc);
    // Tidied away, never read.
    expect(await page.evaluate(() => localStorage.getItem('yap.link.media'))).toBeNull();
    await ask(page, pc);
    speakSentences(pc);
    await expect.poll(async () => (await player(page)).state).toBe('playing');
    expect((await player(page)).via).toBe('media');
  });
});

test('the media player: Stop is silent at once and drops the rest', async ({ page }) => {
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await ask(page, pc);
  speakSentences(pc, false);
  pc.sendBinary(downFrame(3, quiet(24000, 200)));
  pc.sendBinary(downFrame(4, voice(24000, 2000)));
  await expect.poll(async () => (await player(page)).state).toBe('playing');
  await page.getByTestId('stop').click();
  await expect.poll(async () => (await player(page)).active).toBe(false);
  const p = await player(page);
  expect(p.pieces).toBe(0);
  expect(p.played).toBeLessThan(1);
  expect(p.state).toBe('idle');
  await expect(page.getByTestId('phase')).not.toHaveText('Speaking');
  // Nothing plays on: what it had played stays put.
  await page.waitForTimeout(600);
  expect((await player(page)).played).toBe(p.played);
});

test('the media player: a piece late, a gap counted, then on to the end', async ({ page }) => {
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await ask(page, pc);
  pc.send({ type: 'state', phase: 'speaking' });
  pc.send({ type: 'speak_start', rate: 24000 });
  // One sentence and its pause: a piece, played; the next one is late.
  pc.sendBinary(downFrame(0, voice(24000, 700)));
  pc.sendBinary(downFrame(1, quiet(24000, 200)));
  await expect.poll(async () => (await player(page)).state, { timeout: 4000 }).toBe('starved');
  expect((await player(page)).underruns).toBe(1);
  await expect(page.getByTestId('phase')).toHaveText('Speaking');
  pc.sendBinary(downFrame(2, voice(24000, 500)));
  pc.send({ type: 'speak_end' });
  pc.send({ type: 'state', phase: 'idle' });
  await expect.poll(async () => (await player(page)).active, { timeout: 4000 }).toBe(false);
  const p = await player(page);
  expect(p.underruns).toBe(1);
  expect(p.played).toBeGreaterThan(1.3);
  await openSettings(page);
  await page.getByText('Sound details').click();
  await expect(page.getByTestId('sound-gaps')).toHaveText('1 (the sound ran dry and paused)');
});

test('the media player refused from the start (as an iPhone may be, without a tap): Web Audio says it all, and every answer after; said in Sound details only', async ({
  page,
}) => {
  await refuseMediaPlayer(page);
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await ask(page, pc);
  // The tap's moment of silence was refused: the browser said no.
  await expect.poll(async () => (await snapshot(page)).voice.refused).toBe('NotAllowedError');
  speakSentences(pc);
  await expect.poll(async () => (await player(page)).state).toBe('playing');
  let p = await player(page);
  expect(p.via).toBe('worklet');
  expect(p.loaded).toBe(true);
  await expect.poll(async () => (await player(page)).active, { timeout: 6000 }).toBe(false);
  p = await player(page);
  // The whole answer, never silent.
  expect(p.played).toBeCloseTo(1.6, 1);
  expect(p.underruns).toBe(0);
  // Never a note about it (no "turn it off in ⋯"): Sound details say which way, and why.
  await expect(page.getByTestId('note')).toBeHidden();
  await expect(page.getByTestId('error')).toBeHidden();
  await openSettings(page);
  await page.getByText('Sound details').click();
  await expect(page.getByTestId('sound-via')).toHaveText('Web Audio: the media player was refused (NotAllowedError)');
  await closeSettings(page);
  // The rest of the visit: Web Audio, even with the media player willing now.
  await page.evaluate(() => (window.__media.refuse = false));
  await ask(page, pc);
  speak(pc, 4, 100);
  await expect.poll(async () => (await player(page)).active, { timeout: 4000 }).toBe(false);
  p = await player(page);
  expect(p.via).toBe('worklet');
  expect(p.played).toBeCloseTo(0.4, 2);
  expect((await snapshot(page)).voice).toEqual({ via: 'worklet', why: 'was refused (NotAllowedError)', unlocked: false, refused: 'NotAllowedError' });
});

test('refused mid-answer: the rest goes through Web Audio at once, nothing lost or said twice', async ({ page }) => {
  const media = await mediaPlayer(page);
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await ask(page, pc);
  await firstPiecePlaying(page, pc);
  // The browser stops letting it play: the next piece is refused.
  await media.set({ refuse: true });
  theRest(pc);
  await expect.poll(async () => (await player(page)).via, { timeout: 4000 }).toBe('worklet');
  await expect.poll(async () => (await player(page)).active, { timeout: 6000 }).toBe(false);
  const p = await player(page);
  // All of it, once: the media player's piece, then the rest through Web Audio.
  expect(p.played).toBeGreaterThan(1.55);
  expect(p.played).toBeLessThan(1.65);
  expect(p.last).toMatchObject({ via: 'worklet', fellBack: 'was refused (NotAllowedError)' });
  await expect(page.getByTestId('note')).toBeHidden();
  await openSettings(page);
  await page.getByText('Sound details').click();
  await expect(page.getByTestId('sound-via')).toHaveText('Web Audio: the media player was refused (NotAllowedError)');
  await expect(page.getByTestId('sound-start')).toHaveText(/^1\.6 s played; started after \d+ ms, with \d+ ms buffered$/);
});

test("a piece that never starts: after 2 s Web Audio takes over from there, and the answer's said to its end", async ({ page }) => {
  const media = await mediaPlayer(page);
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await ask(page, pc);
  await firstPiecePlaying(page, pc);
  // The next piece's play is never answered.
  await media.set({ hang: true });
  theRest(pc);
  await expect.poll(async () => (await player(page)).via, { timeout: 5000 }).toBe('worklet');
  await expect.poll(async () => (await player(page)).active, { timeout: 6000 }).toBe(false);
  const p = await player(page);
  expect(p.played).toBeGreaterThan(1.55);
  expect(p.played).toBeLessThan(1.65);
  expect(p.last).toMatchObject({ via: 'worklet', fellBack: "didn't start" });
  expect((await snapshot(page)).voice.via).toBe('worklet');
});

test("an <audio> error as a piece plays: Web Audio from where it got to", async ({ page }) => {
  await mediaPlayer(page, { error: true });
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await ask(page, pc);
  speakSentences(pc);
  await expect.poll(async () => (await player(page)).via, { timeout: 4000 }).toBe('worklet');
  await expect.poll(async () => (await player(page)).active, { timeout: 6000 }).toBe(false);
  const p = await player(page);
  // What the media player played, then the rest from there: about the whole answer.
  expect(p.played).toBeGreaterThan(1.5);
  expect(p.played).toBeLessThan(1.7);
  expect(p.last).toMatchObject({ via: 'worklet', fellBack: 'stopped with an error' });
});
