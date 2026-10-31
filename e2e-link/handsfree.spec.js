// Hands-free: Talk once, speak, and the end of speech sends `stop` by
// itself, after the wait chosen (Short 1 s, Normal 1.5 s, Long 2.5 s). The
// fake mic (the "handsfree" project in playwright.config.js) plays
// support/audio.js's SPEECH_WAV once: quiet, 1.2 s of a voice-like sound,
// then quiet.
import { test, expect } from '@playwright/test';
import { servePage, seedDevice, mockPc, open, ready, snapshot } from './support/harness.js';
import { SPEECH_AT } from './support/audio.js';

test.beforeEach(async ({ page }) => {
  await servePage(page);
  await seedDevice(page);
});

/** One hands-free question: Talk, then the seconds of frames sent before the page sent stop. */
async function askHandsFree(page, pc) {
  const starts = pc.of('start').length;
  const stops = pc.of('stop').length;
  const before = pc.frames.length;
  await page.getByTestId('talk').click();
  await expect.poll(() => pc.of('start').length).toBe(starts + 1);
  await expect.poll(() => pc.of('stop').length, { timeout: 10_000 }).toBe(stops + 1);
  return (pc.frames.length - before) * 0.02;
}

test('hands-free: the end of speech sends stop by itself, about 1.5 s after it (Normal)', async ({ page }) => {
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  const sw = page.getByTestId('handsfree');
  await expect(sw).not.toBeChecked();
  // The wait choice shows with hands-free on.
  await expect(page.getByTestId('wait')).toBeHidden();
  await page.getByText('Hands-free').click();
  await expect(sw).toBeChecked();
  expect(await page.evaluate(() => localStorage.getItem('yap.link.handsFree'))).toBe('true');
  await expect(page.getByTestId('wait')).toBeVisible();
  await expect(page.getByRole('radio', { name: 'Normal 1.5 s' })).toBeChecked();

  const seconds = await askHandsFree(page, pc);
  // Stopped by the page, not by a tap: after the speech ended, not long after.
  expect(seconds).toBeGreaterThan(SPEECH_AT.to + 1.3);
  expect(seconds).toBeLessThan(SPEECH_AT.to + 2.4);
  // The speech is in what was sent.
  const loud = pc.frames.filter((f) => f.samples.some((s) => Math.abs(s) > 2000)).length;
  expect(loud * 0.02).toBeGreaterThan(0.8);
  await expect(page.getByTestId('phase')).toHaveText('Working it out');
  expect((await snapshot(page)).local).toBe('idle');

  // Remembered on the next visit.
  await open(page);
  await ready(page, pc);
  await expect(page.getByTestId('handsfree')).toBeChecked();
});

for (const [name, label, wait] of [
  ['short', 'Short 1 s', 1],
  ['long', 'Long 2.5 s', 2.5],
]) {
  test(`hands-free: the wait choice (${name}) is remembered and used`, async ({ page }) => {
    const pc = await mockPc(page);
    await open(page);
    await ready(page, pc);
    await page.getByText('Hands-free').click();
    await page.getByRole('radio', { name: label }).check();
    await expect(page.getByRole('radio', { name: label })).toBeChecked();
    await expect(page.getByRole('radio', { name: 'Normal 1.5 s' })).not.toBeChecked();
    expect(await page.evaluate(() => JSON.parse(localStorage.getItem('yap.link.wait')))).toBe(name);

    // Remembered: the page opened again has it, and uses it.
    await open(page);
    await ready(page, pc);
    await expect(page.getByRole('radio', { name: label })).toBeChecked();
    expect((await snapshot(page)).waitMs).toBe(wait * 1000);
    const seconds = await askHandsFree(page, pc);
    expect(seconds).toBeGreaterThan(SPEECH_AT.to + wait - 0.2);
    expect(seconds).toBeLessThan(SPEECH_AT.to + wait + 0.9);
  });
}

test('hands-free off: quiet after speech keeps listening until Talk is tapped', async ({ page }) => {
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await expect(page.getByTestId('handsfree')).not.toBeChecked();
  await page.getByTestId('talk').click();
  await expect.poll(() => pc.frames.length, { timeout: 8000 }).toBeGreaterThan((SPEECH_AT.to + 3) / 0.02);
  expect(pc.of('stop').length).toBe(0);
  await expect(page.getByTestId('phase')).toHaveText('Listening');
  await page.getByTestId('talk').click();
  await expect.poll(() => pc.of('stop').length).toBe(1);
});
