// The answer's sound on the phone: one stream from a jitter buffer
// (player-worklet.js, dsp.js `Playout`), "Clearer sound in a headset" (the
// mic closed while Yap talks) and Sound details. Yap on the PC mocked; the
// mic the looped 440 Hz tone.
import { test, expect } from '@playwright/test';
import { servePage, seedDevice, mockPc, open, ready, ask, snapshot, downFrame, voice } from './support/harness.js';

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

/** An answer: speak_start, `n` frames of `ms` each, speak_end, idle. */
function speak(pc, n, ms = 100, end = true) {
  pc.send({ type: 'state', phase: 'speaking' });
  pc.send({ type: 'speak_start', rate: 24000 });
  for (let i = 0; i < n; i++) pc.sendBinary(downFrame(i, voice(24000, ms)));
  if (end) {
    pc.send({ type: 'speak_end' });
    pc.send({ type: 'state', phase: 'idle' });
  }
}

test.beforeEach(async ({ page }) => {
  await servePage(page);
  await seedDevice(page);
});

test('playback starts once about 250 ms is waiting, not before', async ({ page }) => {
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

test('a late frame: one underrun, faded out and back in, never a hard cut', async ({ page }) => {
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
  // Sound details say so.
  await page.getByText('Sound details').click();
  await expect(page.getByTestId('sound-gaps')).toHaveText('1 (the sound ran dry and paused)');
});

test('Clearer sound in a headset: the mic let go while Yap talks, open again after', async ({ page }) => {
  const tracks = await watchTracks(page);
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  const sw = page.getByTestId('clearer');
  await expect(sw).not.toBeChecked();
  await expect(page.getByText("Closes the mic while Yap talks, so the headset can use its music mode. You can't talk over Yap by voice; Stop still works.")).toBeVisible();

  // Off: the mic stays open through the answer (the headset in call mode).
  await ask(page, pc);
  expect(await tracks.live()).toBe(1);
  pc.send({ type: 'state', phase: 'thinking' });
  // Switched on while Yap works on it: the mic's still open…
  await page.getByText('Clearer sound in a headset').click();
  await expect(sw).toBeChecked();
  expect(await page.evaluate(() => localStorage.getItem('yap.link.clearer'))).toBe('true');
  expect(await tracks.live()).toBe(1);
  // …until the answer starts: then its tracks stop.
  speak(pc, 6, 100);
  await expect.poll(() => tracks.live()).toBe(0);
  expect((await snapshot(page)).micOpen).toBe(false);
  await expect.poll(async () => (await player(page)).active, { timeout: 4000 }).toBe(false);
  // Hands-free off: it stays closed until Talk.
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

  // Hands-free on: once the answer has played out, it opens again by itself,
  // ready for a follow-up.
  await page.getByText('Hands-free').click();
  speak(pc, 4, 100);
  await expect.poll(async () => (await player(page)).state).toBe('playing');
  expect(await tracks.live()).toBe(0);
  await expect.poll(async () => (await player(page)).active, { timeout: 4000 }).toBe(false);
  await expect.poll(() => tracks.live(), { timeout: 5000 }).toBe(1);
  expect((await snapshot(page)).micOpen).toBe(true);
  last = (await player(page)).last;
  expect(last.micOpen).toBe(false); // never open while it played
  await page.getByText('Sound details').click();
  await expect(page.getByTestId('sound-mic')).toHaveText('Closed');
  await expect(page.getByTestId('sound-mic-now')).toHaveText('Open');

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
  await page.getByText('Sound details').click();
  await expect(page.getByTestId('sound-rate')).toHaveText('Shown once you’ve asked something');
  await expect(page.getByTestId('sound-start')).toHaveText('No answer yet');
  await expect(page.getByTestId('sound-mic-now')).toHaveText('Closed');
  await expect(page.getByTestId('sound-browser')).toHaveText(/^(Chrome|Edge) \d+ on Windows$/);

  await ask(page, pc);
  speak(pc, 6, 100);
  await expect.poll(async () => (await player(page)).active, { timeout: 4000 }).toBe(false);
  await expect(page.getByTestId('sound-rate')).toHaveText(/^(44,100|48,000) Hz$/);
  await expect(page.getByTestId('sound-latency')).toHaveText(/^Base \d+ ms( · Output \d+ ms)?$/);
  await expect(page.getByTestId('sound-start')).toHaveText(/^0\.6 s played; started after \d+ ms, with \d{3} ms buffered$/);
  await expect(page.getByTestId('sound-gaps')).toHaveText('None');
  await expect(page.getByTestId('sound-mic')).toHaveText('Open (the headset in call mode)');
  await expect(page.getByTestId('sound-mic-now')).toHaveText('Open');
  // Each row as a term and its value, for a screen reader too.
  const terms = await sound.locator('dt').allTextContents();
  expect(terms).toEqual([
    'Sample rate',
    'Latency',
    'Last answer',
    'Gaps in it',
    'Microphone while it played',
    'Microphone now',
    'Browser',
  ]);
});
