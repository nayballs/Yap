// Hands-free: Talk once, speak, and the end of speech sends `stop` by
// itself. The fake mic (the "handsfree" project in playwright.config.js)
// plays support/audio.js's SPEECH_WAV once: quiet, 1.2 s of a voice-like
// sound, then quiet.
import { test, expect } from '@playwright/test';
import { servePage, seedDevice, mockPc, open, ready, snapshot } from './support/harness.js';
import { SPEECH_AT } from './support/audio.js';

test.beforeEach(async ({ page }) => {
  await servePage(page);
  await seedDevice(page);
});

test('hands-free: the end of speech sends stop by itself, about 0.8 s after it', async ({ page }) => {
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  const sw = page.getByTestId('handsfree');
  await expect(sw).not.toBeChecked();
  await page.getByText('Hands-free').click();
  await expect(sw).toBeChecked();
  expect(await page.evaluate(() => localStorage.getItem('yap.link.handsFree'))).toBe('true');

  await page.getByTestId('talk').click();
  await expect.poll(() => pc.of('start').length).toBe(1);
  await expect.poll(() => pc.of('stop').length, { timeout: 8000 }).toBe(1);
  // Stopped by the page, not by a tap: after the speech ended, not long after.
  const seconds = pc.frames.length * 0.02;
  expect(seconds).toBeGreaterThan(SPEECH_AT.to + 0.6);
  expect(seconds).toBeLessThan(SPEECH_AT.to + 1.6);
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

test('hands-free off: quiet after speech keeps listening until Talk is tapped', async ({ page }) => {
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await expect(page.getByTestId('handsfree')).not.toBeChecked();
  await page.getByTestId('talk').click();
  await expect.poll(() => pc.frames.length, { timeout: 8000 }).toBeGreaterThan((SPEECH_AT.to + 1.5) / 0.02);
  expect(pc.of('stop').length).toBe(0);
  await expect(page.getByTestId('phase')).toHaveText('Listening');
  await page.getByTestId('talk').click();
  await expect.poll(() => pc.of('stop').length).toBe(1);
});
