// The page's states as named screenshots, light and dark, phone-sized, in
// test-results/link/screenshots/ (to look at, not compared). Also checks
// nothing overflows sideways and every control is at least 48 px.
import { test, expect } from '@playwright/test';
import { servePage, seedDevice, mockPc, open, ready, ask, downFrame, voice } from './support/harness.js';

const DIR = 'test-results/link/screenshots';

for (const scheme of ['light', 'dark']) {
  test.describe(scheme, () => {
    test.use({ colorScheme: scheme });

    test(`pairing (${scheme})`, async ({ page }) => {
      await servePage(page);
      const pc = await mockPc(page, {
        onHello: (hello, conn, pc) => pc.sendTo(conn, { type: 'pair_wait', code: '4821' }),
      });
      await open(page, '#pair=tok-abc123');
      await expect(page.getByTestId('pair-code')).toHaveText('4821');
      await page.screenshot({ path: `${DIR}/pair-code-${scheme}.png` });
      expect(pc.conns.length).toBe(1);

      await page.evaluate(() => localStorage.clear());
      await open(page);
      await expect(page.getByTestId('pair-how')).toBeVisible();
      await page.screenshot({ path: `${DIR}/pair-how-${scheme}.png` });
    });

    test(`talking (${scheme})`, async ({ page }) => {
      await servePage(page);
      await seedDevice(page);
      const pc = await mockPc(page);
      await open(page);
      await ready(page, pc);
      await page.screenshot({ path: `${DIR}/ready-${scheme}.png` });

      // No sideways scroll, and the controls are thumb-sized.
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
      const small = await page.evaluate(() =>
        [...document.querySelectorAll('button:not([hidden]), select, .switch')]
          .filter((e) => e.offsetParent)
          .map((e) => [e.id || e.className, e.getBoundingClientRect().height])
          .filter(([, h]) => h < 48),
      );
      expect(small).toEqual([]);

      await page.getByTestId('talk').click();
      await expect(page.getByTestId('talk-time')).toHaveText('0:01', { timeout: 4000 });
      await page.screenshot({ path: `${DIR}/listening-${scheme}.png` });
      await page.getByTestId('talk').click();
      await expect.poll(() => pc.of('stop').length).toBe(1);
      pc.send({ type: 'state', phase: 'thinking' });
      pc.send({ type: 'heard', text: "What's on my calendar this afternoon?" });
      pc.send({ type: 'step', label: 'Checking your calendar · this afternoon' });
      await expect(page.locator('#step-time')).toHaveText(' · 1s', { timeout: 4000 });
      await page.screenshot({ path: `${DIR}/working-${scheme}.png` });

      pc.send({
        type: 'answer',
        text: 'You have two things this afternoon: the design review at 2 pm, and a call with the print shop at 4.',
        final: true,
      });
      pc.send({ type: 'state', phase: 'speaking' });
      pc.send({ type: 'speak_start', rate: 24000 });
      for (let i = 0; i < 40; i++) pc.sendBinary(downFrame(i, voice(24000, 100)));
      await expect(page.getByTestId('stop')).toBeVisible();
      await page.screenshot({ path: `${DIR}/speaking-${scheme}.png` });
      await page.getByTestId('stop').click();

      pc.send({ type: 'error', message: "Couldn't reach the AI model. Check Yap on your PC." });
      await page.getByTestId('model-chip').click();
      await expect(page.getByTestId('models')).toBeVisible();
      await page.screenshot({ path: `${DIR}/models-${scheme}.png` });
      await page.keyboard.press('Escape');
      await expect(page.getByTestId('models')).toBeHidden();
      await page.screenshot({ path: `${DIR}/error-${scheme}.png` });
    });
  });
}

test('reconnecting, and the step on a narrow phone', async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 640 });
  await servePage(page);
  await seedDevice(page);
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await ask(page, pc, 200);
  pc.send({ type: 'step', label: 'Reading tide-tables.example/harbour/today-and-tomorrow-full-week' });
  await expect(page.getByTestId('step')).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  pc.autoReady = false;
  pc.close();
  await expect(page.getByTestId('conn')).toHaveText('Reconnecting to Office PC…');
  await page.screenshot({ path: `${DIR}/reconnecting-narrow.png` });
});
