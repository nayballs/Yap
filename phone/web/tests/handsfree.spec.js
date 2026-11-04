// Hands-free talk (4 Nov; the Claude app's voice mode): the waveform in
// Send's place starts a conversation, as a call is — Yap listens, the wait
// chosen (Short 1 s, Normal 1.5 s, Long 2.5 s) of quiet after speech sends
// it (`start {mode: "ask"}`, frames, `stop`), Yap answers out loud, then it
// listens again by itself; End, 10 s with nothing said, the connection
// dropping, the page hidden or the mic failing end it; ■ while Yap answers
// goes back to listening.
//
// The mic: in Chromium (the "handsfree" project in playwright.config.js)
// support/audio.js's SPEECH_WAV played once for each stream the page opens
// — quiet, 1.2 s of a voice-like sound, then quiet — through Chromium's own
// capture (noise suppression and all). With "Clearer sound" on (the
// default) the page opens the mic afresh for each turn, so each turn hears
// it from the start; with it off, the same stream runs on into quiet. In
// Firefox, whose fake mic is a tone, the same made in the page (speechMic).
import { test, expect } from '@playwright/test';
import {
  servePage,
  seedDevice,
  mockPc,
  open,
  ready,
  snapshot,
  openSettings,
  closeSettings,
  downFrame,
  voice,
  speechMic,
} from './support/harness.js';
import { SPEECH_AT } from './support/audio.js';

test.beforeEach(async ({ page, browserName }) => {
  await servePage(page);
  await seedDevice(page);
  if (browserName === 'firefox') await speechMic(page);
});

/** The call button: "Talk hands-free", pressed while the call is on. */
const callBtn = (page) => page.getByRole('button', { name: 'Talk hands-free' });

/** "Clearer sound in a headset" as if chosen before. */
async function chooseClearer(page, on) {
  await page.addInitScript((on) => localStorage.setItem('yap.link.clearer', JSON.stringify(on)), on);
}

/** `ms` of quiet, as Int16. */
const quiet = (rate, ms) => new Int16Array(Math.round((rate * ms) / 1000));

/** The PC's answer said out loud: transcribing, heard, thinking, the answer, its voice (`ms`; sentences with a pause between, as a voice has). */
function answer(pc, { heard, text, id, ms = 600, end = true }) {
  pc.send({ type: 'state', phase: 'transcribing' });
  pc.send({ type: 'heard', text: heard });
  pc.send({ type: 'state', phase: 'thinking' });
  pc.send({ type: 'answer', text, final: true, id });
  pc.send({ type: 'state', phase: 'speaking' });
  pc.send({ type: 'speak_start', rate: 24000 });
  let seq = 0;
  for (let left = ms; left > 0; left -= 900) {
    pc.sendBinary(downFrame(seq++, voice(24000, Math.min(700, left))));
    if (left > 700) pc.sendBinary(downFrame(seq++, quiet(24000, 200)));
  }
  if (end) {
    pc.send({ type: 'speak_end' });
    pc.send({ type: 'state', phase: 'idle' });
  }
}

/** Waits for the call's `n`th question to go (`stop`), and gives the seconds of frames between its start and its stop. */
async function nthQuestion(pc, n) {
  await expect.poll(() => pc.of('start').length, { timeout: 10_000 }).toBe(n);
  const at = pc.log.findLastIndex((e) => e.text && e.text.type === 'start');
  await expect.poll(() => pc.of('stop').length, { timeout: 12_000 }).toBe(n);
  const stopAt = pc.log.findLastIndex((e) => e.text && e.text.type === 'stop');
  return pc.log.slice(at, stopAt).filter((e) => e.frame).length * 0.02;
}

test('hands-free talk: listens, sends when you stop, answers out loud, then listens again and sends the next', async ({ page }) => {
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  // The waveform, in Send's place while there's nothing to send.
  const call = callBtn(page);
  await expect(call).toBeVisible();
  await expect(call).toHaveAttribute('aria-pressed', 'false');
  await expect(page.getByTestId('send')).toBeHidden();
  await call.click();
  await expect(call).toHaveAttribute('aria-pressed', 'true');
  await expect(call).toContainText('End');
  await expect(page.getByTestId('composer')).toHaveAttribute('data-call', 'true');
  // The question: an ordinary spoken one, sent by the quiet after the speech.
  const first = await nthQuestion(pc, 1);
  expect(pc.of('start')[0]).toEqual({ type: 'start', mode: 'ask' });
  expect(first).toBeGreaterThan(SPEECH_AT.to + 1.2);
  expect(first).toBeLessThan(SPEECH_AT.to + 2.6);
  await expect(page.getByTestId('phase')).toHaveText('Thinking');
  await expect(page.locator('#live-label')).toHaveText('Thinking');
  // Yap answers out loud, through the media player.
  answer(pc, { heard: 'When does the last train leave?', text: 'The last train leaves at 11:40.', id: 'chat:5:1', ms: 1600 });
  await expect(page.getByTestId('heard').first()).toHaveText('When does the last train leave?');
  await expect.poll(async () => (await snapshot(page)).player.state).toBe('playing');
  await expect(page.getByTestId('phase')).toHaveText('Speaking');
  await expect(page.locator('#live-label')).toHaveText('Speaking');
  expect((await snapshot(page)).player.via).toBe('media');
  // ■ is there while it answers, and End.
  await expect(page.getByTestId('stop')).toBeVisible();
  // Clearer sound (the default): the mic let go while Yap speaks.
  expect((await snapshot(page)).micOpen).toBe(false);
  // Played out: it listens again by itself, and sends the next question.
  const second = await nthQuestion(pc, 2);
  expect(second).toBeGreaterThan(SPEECH_AT.to + 1.2);
  expect((await snapshot(page)).player.last.micOpen).toBe(false);
  expect((await snapshot(page)).player.last.played).toBeGreaterThan(1.4);
  // Nothing was cancelled or interrupted on the way; the call's still on.
  expect(pc.of('cancel')).toHaveLength(0);
  expect(pc.of('interrupt')).toHaveLength(0);
  expect((await snapshot(page)).call).toBe(true);
  // Both questions in the thread, said.
  answer(pc, { heard: 'And the first one tomorrow?', text: 'The first leaves at 5:52.', id: 'chat:5:3' });
  await expect(page.getByTestId('heard')).toHaveCount(2);
  await expect(page.getByRole('img', { name: 'Asked by voice' })).toHaveCount(2);
  await expect.poll(() => pc.of('start').length, { timeout: 10_000 }).toBe(3);
});

test('End ends it: what it was listening to is cancelled, the composer back, the mic let go', async ({ page }) => {
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await callBtn(page).click();
  await expect.poll(() => pc.of('start').length).toBe(1);
  await expect(page.getByTestId('phase')).toHaveText('Listening');
  // "Listening" only once the mic is open: the page is sending its frames.
  await expect.poll(() => pc.frames.length).toBeGreaterThan(5);
  expect((await snapshot(page)).micOpen).toBe(true);
  await callBtn(page).click();
  await expect(callBtn(page)).toHaveAttribute('aria-pressed', 'false');
  await expect.poll(() => pc.of('cancel').length).toBe(1);
  expect(pc.of('stop')).toHaveLength(0);
  const s = await snapshot(page);
  expect(s.call).toBe(false);
  expect(s.micOpen).toBe(false);
  // The composer is back: the box, Talk, the waveform; no question left behind.
  await expect(page.getByTestId('input')).toBeVisible();
  await expect(page.getByTestId('talk')).toBeVisible();
  await expect(page.getByTestId('turn')).toHaveCount(0);
  await expect(page.getByTestId('note')).toBeHidden();
  await page.waitForTimeout(3500);
  expect(pc.of('start')).toHaveLength(1);
});

test('nothing said for 10 s once it listens: it ends quietly, with a note', async ({ page }) => {
  // Clearer sound off: the mic stays open, so the second turn hears the quiet after the speech.
  await chooseClearer(page, false);
  test.setTimeout(60_000);
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await callBtn(page).click();
  await nthQuestion(pc, 1);
  answer(pc, { heard: 'Is the pool open late?', text: 'Until 9 pm on Thursdays.', id: 'chat:7:1' });
  await expect.poll(() => pc.of('start').length, { timeout: 10_000 }).toBe(2);
  const listening = Date.now();
  await expect(page.getByTestId('note')).toHaveText('Hands-free talk ended: nothing was said for 10 seconds.', { timeout: 15_000 });
  expect((Date.now() - listening) / 1000).toBeGreaterThan(9);
  await expect(callBtn(page)).toHaveAttribute('aria-pressed', 'false');
  expect(pc.of('cancel')).toHaveLength(1);
  expect(pc.of('stop')).toHaveLength(1);
  // The empty question goes; the one answered stays.
  await expect(page.getByTestId('turn')).toHaveCount(1);
  expect((await snapshot(page)).micOpen).toBe(false);
});

test('■ while Yap answers: the answer stops and it listens again; ■ while it thinks: called off, and the same', async ({ page }) => {
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await callBtn(page).click();
  await nthQuestion(pc, 1);
  // A long answer, still coming.
  answer(pc, { heard: 'Tell me about the coastal path.', text: 'It runs for 30 miles…', id: 'chat:8:1', ms: 6000, end: false });
  await expect.poll(async () => (await snapshot(page)).player.state).toBe('playing');
  const stop = page.getByRole('button', { name: 'Stop' });
  await expect(stop).toBeVisible();
  await stop.click();
  await expect.poll(() => pc.of('interrupt').length).toBe(1);
  // Straight back to listening: the call carries on.
  await expect.poll(() => pc.of('start').length).toBe(2);
  expect(pc.types().filter((t) => t !== 'ping').slice(-2)).toEqual(['interrupt', 'start']);
  pc.send({ type: 'state', phase: 'idle' });
  pc.send({ type: 'state', phase: 'listening' });
  await expect(page.getByTestId('phase')).toHaveText('Listening');
  expect((await snapshot(page)).player.active).toBe(false);
  expect((await snapshot(page)).call).toBe(true);

  // The next one sent; ■ while Yap works it out calls it off, and it listens again.
  await nthQuestion(pc, 2);
  pc.send({ type: 'state', phase: 'transcribing' });
  pc.send({ type: 'heard', text: 'How steep is it?' });
  pc.send({ type: 'state', phase: 'thinking' });
  await expect(page.getByTestId('phase')).toHaveText('Thinking');
  await page.getByRole('button', { name: 'Stop the answer' }).click();
  await expect.poll(() => pc.of('cancel').length).toBe(1);
  await expect.poll(() => pc.of('start').length).toBe(3);
  await expect(page.getByTestId('answer').last()).toHaveText('Cancelled');
  expect((await snapshot(page)).call).toBe(true);
});

test('a dropped connection ends it, with a note, and lets the mic go', async ({ page }) => {
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await callBtn(page).click();
  await expect.poll(() => pc.frames.length).toBeGreaterThan(5);
  pc.autoReady = false;
  pc.close();
  await expect(page.getByTestId('note')).toHaveText('Hands-free talk ended: the connection to Office PC dropped.');
  await expect(page.getByTestId('error')).toBeHidden();
  const s = await snapshot(page);
  expect(s.call).toBe(false);
  expect(s.micOpen).toBe(false);
  await expect(callBtn(page)).toHaveAttribute('aria-pressed', 'false');
  // Back: the waveform's there to start again.
  pc.autoReady = true;
  await expect(page.getByTestId('conn')).toHaveText('Connected to Office PC', { timeout: 10_000 });
  await expect(callBtn(page)).toBeEnabled();
});

test('the page going into the background ends it, with a note', async ({ page }) => {
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await callBtn(page).click();
  await expect.poll(() => pc.frames.length).toBeGreaterThan(5);
  await page.evaluate(() => {
    Object.defineProperty(document, 'visibilityState', { value: 'hidden', configurable: true });
    document.dispatchEvent(new Event('visibilitychange'));
  });
  await expect(page.getByTestId('note')).toHaveText('Hands-free talk ended when this page went into the background.');
  await expect.poll(() => pc.of('cancel').length).toBe(1);
  expect((await snapshot(page)).call).toBe(false);
});

test('a mic that won\'t open ends it, the error saying why', async ({ page }) => {
  await page.addInitScript(() => {
    navigator.mediaDevices.getUserMedia = () => Promise.reject(new DOMException('Requested device not found', 'NotFoundError'));
  });
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await callBtn(page).click();
  await expect(page.getByTestId('error')).toHaveText('No microphone found on this phone.');
  await expect(callBtn(page)).toHaveAttribute('aria-pressed', 'false');
  expect(pc.of('start')).toHaveLength(0);
});

test('a question it didn\'t catch: it listens again, once; a second in a row ends it', async ({ page }) => {
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await callBtn(page).click();
  await nthQuestion(pc, 1);
  pc.send({ type: 'state', phase: 'transcribing' });
  pc.send({ type: 'error', message: "Yap didn't catch a question. Try again." });
  pc.send({ type: 'state', phase: 'idle' });
  await expect(page.getByTestId('note')).toHaveText("Yap didn't catch that, so it's listening again.");
  await expect(page.getByTestId('error')).toBeHidden();
  await nthQuestion(pc, 2);
  pc.send({ type: 'state', phase: 'transcribing' });
  pc.send({ type: 'error', message: "Yap didn't catch a question. Try again." });
  pc.send({ type: 'state', phase: 'idle' });
  await expect(page.getByTestId('error')).toHaveText("Yap didn't catch a question. Try again.");
  await expect(callBtn(page)).toHaveAttribute('aria-pressed', 'false');
  await page.waitForTimeout(1500);
  expect(pc.of('start')).toHaveLength(2);
  // No question left in the thread for nothing.
  await expect(page.getByTestId('turn')).toHaveCount(0);
});

test('one the PC heard and couldn\'t answer ends it, the error saying why', async ({ page }) => {
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await callBtn(page).click();
  await nthQuestion(pc, 1);
  pc.send({ type: 'state', phase: 'transcribing' });
  pc.send({ type: 'heard', text: 'Add milk to the list.' });
  pc.send({ type: 'error', message: 'The Voice Assistant is off during meetings.' });
  pc.send({ type: 'state', phase: 'idle' });
  await expect(page.getByTestId('error')).toHaveText('The Voice Assistant is off during meetings.');
  await expect(callBtn(page)).toHaveAttribute('aria-pressed', 'false');
  await page.waitForTimeout(1000);
  expect(pc.of('start')).toHaveLength(1);
});

test('the wait before sending: in the settings, Normal at first, its choice remembered', async ({ page }) => {
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await openSettings(page);
  // No Hands-free switch now (its own button), nor the media player's.
  await expect(page.getByTestId('settings-sheet').getByRole('switch')).toHaveCount(1);
  await expect(page.getByTestId('settings-sheet').getByText('Hands-free', { exact: true })).toHaveCount(0);
  await expect(page.getByTestId('wait')).toBeVisible();
  await expect(page.getByText('When you talk hands-free (the waveform by the mic), Yap sends what you said after this much quiet.')).toBeVisible();
  await expect(page.getByRole('radio', { name: 'Normal 1.5 s' })).toBeChecked();
  await closeSettings(page);
  expect((await snapshot(page)).waitMs).toBe(1500);
});

for (const [name, label, wait] of [
  ['short', 'Short 1 s', 1],
  ['long', 'Long 2.5 s', 2.5],
]) {
  test(`the wait (${name}): remembered, and the call waits that long`, async ({ page }) => {
    const pc = await mockPc(page);
    await open(page);
    await ready(page, pc);
    await openSettings(page);
    await page.getByRole('radio', { name: label }).check();
    await expect(page.getByRole('radio', { name: 'Normal 1.5 s' })).not.toBeChecked();
    expect(await page.evaluate(() => JSON.parse(localStorage.getItem('yap.link.wait')))).toBe(name);
    // Remembered: the page opened again has it, and the call uses it.
    await open(page);
    await ready(page, pc);
    await openSettings(page);
    await expect(page.getByRole('radio', { name: label })).toBeChecked();
    await closeSettings(page);
    expect((await snapshot(page)).waitMs).toBe(wait * 1000);
    await callBtn(page).click();
    const seconds = await nthQuestion(pc, 1);
    expect(seconds).toBeGreaterThan(SPEECH_AT.to + wait - 0.3);
    expect(seconds).toBeLessThan(SPEECH_AT.to + wait + 1.0);
  });
}

test('Talk is always the words in the box: no wait, no sending by itself, a stored Hands-free switch ignored', async ({ page }) => {
  // From before 4 Nov: the Hands-free switch on, the media player off.
  await page.addInitScript(() => {
    if (!sessionStorage.getItem('old-settings')) {
      localStorage.setItem('yap.link.handsFree', 'true');
      localStorage.setItem('yap.link.media', 'false');
      sessionStorage.setItem('old-settings', '1');
    }
  });
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  // Ignored, and tidied away.
  expect(await page.evaluate(() => [localStorage.getItem('yap.link.handsFree'), localStorage.getItem('yap.link.media')])).toEqual([null, null]);
  expect((await snapshot(page)).voice.via).toBe('media');
  await page.getByTestId('talk').click();
  await expect.poll(() => pc.of('start')).toEqual([{ type: 'start', mode: 'words' }]);
  // Quiet after the speech doesn't send it: Talk waits for Done.
  await expect.poll(() => pc.frames.length, { timeout: 8000 }).toBeGreaterThan((SPEECH_AT.to + 3) / 0.02);
  expect(pc.of('stop').length).toBe(0);
  await expect(page.getByTestId('phase')).toHaveText('Listening');
  await page.getByTestId('talk').click();
  await expect.poll(() => pc.of('stop').length).toBe(1);
  await expect(page.getByTestId('phase')).toHaveText('Transcribing…');
});
