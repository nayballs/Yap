// The page's states as named screenshots, light and dark, phone-sized, in
// test-results/link/screenshots/ (to look at, not compared). Also checks
// nothing overflows sideways and every control is at least 48 px.
import { test, expect } from '@playwright/test';
import { servePage, seedDevice, mockPc, open, ready, ask, downFrame, voice, ORIGIN, PC, MODELS } from './support/harness.js';

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

      // No sideways scroll, and the controls are thumb-sized (the wait
      // choice and Sound details too, shown for the check).
      await page.getByText('Hands-free').click();
      await page.getByText('Sound details').click();
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
      const small = await page.evaluate(() =>
        [...document.querySelectorAll('button:not([hidden]), select, .switch, .seg label, summary')]
          .filter((e) => e.offsetParent)
          .map((e) => [e.id || e.className || e.tagName, e.getBoundingClientRect().height])
          .filter(([, h]) => h < 48),
      );
      expect(small).toEqual([]);
      // The wait's three, and the mode switch's three.
      expect(await page.locator('#wait .seg label').count()).toBe(3);
      expect(await page.locator('#mode .seg label').count()).toBe(3);
      await page.getByTestId('wait').scrollIntoViewIfNeeded();
      await page.screenshot({ path: `${DIR}/options-${scheme}.png`, fullPage: true });
      await page.getByText('Hands-free').click();
      await page.getByText('Sound details').click();

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

      // Sound details after that answer.
      await page.getByText('Sound details').click();
      await expect(page.getByTestId('sound-start')).toContainText('started');
      await page.getByTestId('sound').scrollIntoViewIfNeeded();
      await page.screenshot({ path: `${DIR}/sound-details-${scheme}.png`, fullPage: true });
    });

    test(`dictating (${scheme})`, async ({ page, context }) => {
      await context.grantPermissions(['clipboard-read', 'clipboard-write'], { origin: ORIGIN });
      await servePage(page);
      await seedDevice(page);
      const pc = await mockPc(page);
      await open(page);
      await ready(page, pc);
      await page.getByRole('radio', { name: 'Dictate to PC', exact: true }).click();
      await page.screenshot({ path: `${DIR}/dictate-pc-ready-${scheme}.png` });
      await ask(page, pc, 400);
      pc.send({ type: 'state', phase: 'transcribing' });
      await expect(page.getByTestId('phase')).toHaveText('Typing it on Office PC');
      await page.screenshot({ path: `${DIR}/dictate-pc-typing-${scheme}.png` });
      pc.send({ type: 'heard', text: 'Thanks for the update. I’ll send the figures over by Friday afternoon.', to: 'pc' });
      pc.send({ type: 'pasted', ok: true, app: 'Outlook' });
      pc.send({ type: 'state', phase: 'idle' });
      await expect(page.getByTestId('result-line')).toHaveText('Pasted into Outlook');
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
      await page.screenshot({ path: `${DIR}/dictate-pc-pasted-${scheme}.png` });
      await ask(page, pc, 300);
      pc.send({ type: 'heard', text: 'Hello there.', to: 'pc' });
      pc.send({ type: 'pasted', ok: false, reason: 'No text box in front: copied on the PC instead' });
      pc.send({ type: 'state', phase: 'idle' });
      await expect(page.getByTestId('result')).toHaveAttribute('data-status', 'failed');
      await page.screenshot({ path: `${DIR}/dictate-pc-not-pasted-${scheme}.png` });

      await page.getByRole('radio', { name: 'Dictate to phone', exact: true }).click();
      await ask(page, pc, 300);
      pc.send({ type: 'heard', text: 'Milk, eggs, bread and something for the barbecue on Saturday.', to: 'phone' });
      pc.send({ type: 'state', phase: 'idle' });
      await expect(page.getByTestId('result-line')).toHaveText('Copied');
      await expect(page.getByTestId('copy')).toBeVisible();
      const copy = await page.getByTestId('copy').boundingBox();
      expect(copy.height).toBeGreaterThanOrEqual(48);
      await page.screenshot({ path: `${DIR}/dictate-phone-copied-${scheme}.png` });

      // Off for this phone: the hint under the switch.
      pc.send({ type: 'ready', pc: PC, model: MODELS[0], models: MODELS, mayType: false });
      await page.getByRole('radio', { name: 'Dictate to PC', exact: true }).click();
      await expect(page.getByTestId('mode-hint')).toBeVisible();
      await page.screenshot({ path: `${DIR}/dictate-pc-not-allowed-${scheme}.png` });
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
