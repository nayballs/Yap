// Yap Link's dictation from the phone, for real (docs/architecture/link.md,
// "Dictation from the phone"): a "phone" — a headless Chromium on this PC
// with a fake microphone playing a spoken WAV, opening the page Yap serves
// over its own HTTPS — paired with the robot's Yap, then "Dictate to PC"
// with the test pad in front. The words go the pipeline's own way (the
// window in front captured when Talk is pressed, the warm engine, the paste
// and its read check) into the pad's box, and the phone says where they
// went. On the stub build the "transcript" says how much audio it got; on an
// engines build, the words. The headless browser never takes the focus.
import fs from 'node:fs';
import { chromium } from '@playwright/test';
import { test, expect } from './support/robot.js';
import { speechWav } from './support/tts.js';

test.use({ robotOptions: { name: 'phone-dictation', config: { linkEnabled: true }, env: { YAP_E2E_LINK_PORT: '0' } } });

const SAID = 'Please send the minutes to the whole team by Friday.';

/** Playwright's own Chromium if it's installed, else Edge (as e2e/link.spec.js). */
function channel() {
  if (process.env.YAP_LINK_CHANNEL) return process.env.YAP_LINK_CHANNEL;
  try {
    return fs.existsSync(chromium.executablePath()) ? 'chromium' : 'msedge';
  } catch {
    return 'msedge';
  }
}

/** The WAV's length in seconds (16-bit mono PCM, as `speechWav` writes it). */
function seconds(file) {
  const b = fs.readFileSync(file);
  return (b.length - 44) / 2 / b.readUInt32LE(24);
}

test("a phone's dictation lands in the test pad, and the phone says where", async ({ robot }) => {
  test.setTimeout(150_000);
  const { yap, pad } = robot;
  await expect.poll(async () => (await yap.invoke('link_status')).running, { timeout: 15_000 }).toBe(true);
  const wav = speechWav(SAID);
  const browser = await chromium.launch({
    channel: channel(),
    headless: true,
    args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream', `--use-file-for-fake-audio-capture=${wav}%noloop`],
  });
  try {
    const context = await browser.newContext({ ignoreHTTPSErrors: true, viewport: { width: 390, height: 844 }, permissions: ['microphone'] });
    const page = await context.newPage();
    const snapshot = () => page.evaluate(() => window.__yapLink.snapshot());

    // Paired: the link Settings would show, the yes Settings would ask for.
    const { port } = await yap.invoke('link_status');
    const pair = await yap.invoke('link_pair_start', { address: null });
    await page.goto(pair.url.replace(/^https:\/\/[^/]+/, `https://127.0.0.1:${port}`));
    await expect.poll(async () => (await yap.invoke('link_status')).asks.length, { timeout: 15_000 }).toBe(1);
    const [question] = (await yap.invoke('link_status')).asks;
    expect(await yap.invoke('link_pair_answer', { id: question.id, allow: true })).toBe(true);
    await expect.poll(async () => (await snapshot()).ready, { timeout: 15_000 }).toBe(true);
    expect((await snapshot()).mayType).toBe(true);

    // The pad in front when Talk is pressed: that's where it goes.
    await pad.set('box', '');
    await page.getByRole('radio', { name: 'Dictate to PC', exact: true }).click();
    await pad.focus();
    const mark = robot.logMark();
    await page.getByTestId('talk').click();
    await expect.poll(async () => (await snapshot()).local, { timeout: 10_000 }).toBe('listening');
    await page.waitForTimeout(Math.round(seconds(wav) * 1000) + 800);
    await page.getByTestId('talk').click();

    await expect(page.getByTestId('result-line')).toHaveText(/^Pasted into /, { timeout: 90_000 });
    const heard = (await page.getByTestId('result-text').textContent()).trim();
    expect(heard.length).toBeGreaterThan(0);
    // What landed is what the phone was told.
    await expect.poll(() => pad.text('box')).toContain(heard);
    const log = robot.logSince(mark);
    expect(log).toContain('Recording started from a phone');
    expect(log).toContain('Recording stopped, transcribing');
    expect(log).toContain('Yap Link: the phone');

    // Dictate to phone (the WAV has played: the stub still hears audio, a
    // real model nothing): whatever comes back, nothing more in the pad.
    const before = await pad.text('box');
    await page.getByRole('radio', { name: 'Dictate to phone', exact: true }).click();
    await pad.focus();
    await page.getByTestId('talk').click();
    await expect.poll(async () => (await snapshot()).local, { timeout: 10_000 }).toBe('listening');
    await page.waitForTimeout(1_500);
    await page.getByTestId('talk').click();
    await expect
      .poll(
        async () => {
          const s = await snapshot();
          return s.local === 'idle' && s.phase === 'idle';
        },
        { timeout: 90_000 },
      )
      .toBe(true);
    await page.waitForTimeout(500);
    expect(await pad.text('box')).toBe(before);
    await context.close();
  } finally {
    await browser.close();
  }
});
