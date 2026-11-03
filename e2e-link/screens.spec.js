// The page's states as named screenshots, light and dark, at a small and a
// middling phone, in test-results/link/screenshots/ (to look at, not
// compared): pairing, empty, typing with the keyboard up, answering,
// listening, the model sheet, the settings sheet. Also checks nothing
// overflows sideways, every control is a thumb's size (44 px or more), Yap's
// fonts and logos come from the page's own origin, and the composer and
// Talk are on the first screen, low enough for a thumb, upright and on its
// side.
import { test, expect } from '@playwright/test';
import { servePage, seedDevice, mockPc, open, ready, ask, type, openSettings, downFrame, voice } from './support/harness.js';

const DIR = 'test-results/link/screenshots';
/** A named screenshot, once what just changed has settled (transitions run 150 ms). */
async function shot(page, name) {
  await page.waitForTimeout(250);
  await page.screenshot({ path: `${DIR}/${name}.png` });
}
const SIZES = [
  [360, 780],
  [390, 844],
];

/** Controls smaller than a thumb (44 px tall), visible ones only. */
const tooSmall = (page) =>
  page.evaluate(() =>
    [...document.querySelectorAll('button:not([hidden]), select, .switch, .seg label, summary, textarea')]
      .filter((e) => e.offsetParent || e.closest('dialog[open]'))
      .filter((e) => e.getBoundingClientRect().height > 0)
      .map((e) => [e.dataset.testid || e.id || e.className || e.tagName, Math.round(e.getBoundingClientRect().height)])
      .filter(([, h]) => h < 44),
  );
const noSideways = (page) => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth);

for (const scheme of ['light', 'dark']) {
  for (const [w, h] of SIZES) {
    const at = `${w}x${h}-${scheme}`;
    test.describe(`${scheme} ${w}×${h}`, () => {
      test.use({ colorScheme: scheme, viewport: { width: w, height: h } });

      test(`pairing (${at})`, async ({ page }) => {
        await servePage(page);
        const pc = await mockPc(page, {
          onHello: (hello, conn, pc) => pc.sendTo(conn, { type: 'pair_wait', code: '4821' }),
        });
        await open(page, '#pair=tok-abc123');
        await expect(page.getByTestId('pair-code')).toHaveText('4821');
        await shot(page, `pairing-${at}`);
        expect(pc.conns.length).toBe(1);
        expect(await noSideways(page)).toBe(true);
        await page.evaluate(() => localStorage.clear());
        await open(page);
        await expect(page.getByTestId('pair-how')).toBeVisible();
        await shot(page, `pair-how-${at}`);
      });

      test(`chat (${at})`, async ({ page }) => {
        await servePage(page);
        await seedDevice(page);
        const pc = await mockPc(page);
        await open(page);
        await ready(page, pc);
        await page.evaluate(() => document.fonts.ready);
        await shot(page, `empty-${at}`);
        expect(await noSideways(page)).toBe(true);
        expect(await tooSmall(page)).toEqual([]);

        // A typed question and its answer, then one asked out loud.
        await type(page, pc, "What's on my calendar this afternoon?");
        pc.send({ type: 'heard', text: "What's on my calendar this afternoon?" });
        pc.send({ type: 'state', phase: 'thinking' });
        pc.send({ type: 'step', label: 'Checking your calendar · this afternoon' });
        await expect(page.locator('#step-time')).toHaveText(' · 1s', { timeout: 4000 });
        pc.send({
          type: 'answer',
          text: 'You have **two things** this afternoon:\n\n- The design review at 2 pm\n- A call with the print shop at 4\n\nBoth are in the *Studio* calendar.',
          final: true,
          id: 'chat:7:1',
        });
        pc.send({ type: 'state', phase: 'idle' });
        await expect(page.getByTestId('read-aloud')).toBeVisible();

        // Typing, the keyboard up: the page as tall as what's left over it
        // (Chrome resizes it for the keyboard).
        await page.getByTestId('input').fill('And tomorrow morning?');
        await page.setViewportSize({ width: w, height: h - 336 });
        await page.getByTestId('input').focus();
        await expect(page.getByTestId('send')).toBeVisible();
        await page.waitForTimeout(350);
        await shot(page, `typing-keyboard-${at}`);
        const box = await page.getByTestId('composer').boundingBox();
        expect(box.y + box.height).toBeLessThanOrEqual(h - 336);
        expect(await noSideways(page)).toBe(true);
        await page.getByTestId('input').fill('');
        await page.setViewportSize({ width: w, height: h });

        // Asked out loud, answered and spoken.
        await ask(page, pc, 300);
        pc.send({ type: 'state', phase: 'transcribing' });
        pc.send({ type: 'heard', text: 'Remind me what the print shop needs.' });
        pc.send({ type: 'state', phase: 'thinking' });
        pc.send({ type: 'answer', text: 'The proofs for the spring flyer, signed off by Friday.', final: false });
        pc.send({ type: 'state', phase: 'speaking' });
        pc.send({ type: 'speak_start', rate: 24000 });
        for (let i = 0; i < 40; i++) pc.sendBinary(downFrame(i, voice(24000, 100)));
        await expect(page.getByTestId('stop')).toBeVisible();
        // One Stop, the composer's; under the answer only "Speaking…".
        await expect(page.getByRole('button', { name: 'Stop' })).toHaveCount(1);
        await expect(page.getByTestId('turn').last().locator('.speaking')).toHaveText('Speaking…');
        await shot(page, `answering-${at}`);
        expect(await tooSmall(page)).toEqual([]);
        await page.getByTestId('stop').click();

        // Listening: the composer as the Yap bar's capsule.
        await page.getByTestId('talk').click();
        await expect(page.getByTestId('talk-time')).toHaveText('0:01', { timeout: 4000 });
        await shot(page, `listening-${at}`);
        expect(await tooSmall(page)).toEqual([]);
        await page.getByTestId('cancel').click();

        // Chat's picker.
        await page.getByTestId('model-chip').click();
        await expect(page.getByTestId('models')).toBeVisible();
        await page.waitForTimeout(300);
        await shot(page, `models-${at}`);
        await page.getByTestId('picker-rail').getByRole('tab', { name: /Claude/ }).click();
        await expect(page.getByTestId('picker-pane')).toContainText('Latest');
        await shot(page, `models-claude-${at}`);
        expect(await tooSmall(page)).toEqual([]);
        expect(await noSideways(page)).toBe(true);
        await page.keyboard.press('Escape');
        await expect(page.getByTestId('models')).toBeHidden();

        // The settings sheet, Sound details open.
        await openSettings(page);
        await page.getByText('Hands-free').click();
        await page.waitForTimeout(250);
        await shot(page, `settings-${at}`);
        expect(await tooSmall(page)).toEqual([]);
        await page.getByText('Sound details').click();
        await page.getByTestId('sound').scrollIntoViewIfNeeded();
        await shot(page, `settings-sound-${at}`);
        await page.getByText('Hands-free').click();
      });
    });
  }
}

test("Yap's fonts and Chat's logos come from the PC, and nothing from anywhere else", async ({ page }) => {
  const served = await servePage(page);
  await seedDevice(page);
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await page.evaluate(() => document.fonts.ready);
  const fonts = await page.evaluate(() => [...document.fonts].filter((f) => f.status === 'loaded').map((f) => f.family));
  expect(fonts.map((f) => f.replace(/"/g, ''))).toEqual(expect.arrayContaining(['Figtree Variable', 'EB Garamond Variable']));
  expect(await page.evaluate(() => getComputedStyle(document.body).fontFamily)).toMatch(/^"?Figtree Variable/);
  expect(served.filter((s) => s.file && s.file.startsWith('fonts/')).map((s) => s.file).sort()).toEqual(
    expect.arrayContaining(['fonts/eb-garamond.woff2', 'fonts/figtree.woff2']),
  );
  await page.getByTestId('model-chip').click();
  await expect(page.getByTestId('picker-rail')).toBeVisible();
  await expect.poll(() => served.filter((s) => s.file && s.file.startsWith('providers/')).length).toBeGreaterThan(3);
  // Chat's own code, from the PC.
  expect(served.map((s) => s.file)).toEqual(expect.arrayContaining(['picker.js', 'core/chatPicker.js', 'core/latestModels.js']));
  expect(served.filter((s) => s.outside || s.missing)).toEqual([]);
});

// The first screen at a small, a middling and a large phone, and on its side:
// no sideways scroll, the composer on screen, Talk low enough for a thumb.
for (const [w, h] of [
  [360, 780],
  [390, 844],
  [430, 932],
  [844, 390],
]) {
  test(`Talk in the thumb's reach at ${w}×${h}`, async ({ page }) => {
    await page.setViewportSize({ width: w, height: h });
    await servePage(page);
    await seedDevice(page);
    const pc = await mockPc(page);
    await open(page);
    await ready(page, pc);
    expect(await noSideways(page)).toBe(true);
    const talk = await page.getByTestId('talk').boundingBox();
    expect(talk.y + talk.height).toBeLessThanOrEqual(h);
    expect(talk.height).toBeGreaterThanOrEqual(44);
    // Its middle in the lower part of the screen, on the right (a thumb's).
    expect(talk.y + talk.height / 2).toBeGreaterThan(h * (h > w ? 0.75 : 0.55));
    expect(talk.x + talk.width / 2).toBeGreaterThan(w / 2);
    const input = await page.getByTestId('input').boundingBox();
    expect(input.y + input.height).toBeLessThanOrEqual(h);
    if (w > h) {
      await type(page, pc, 'Is the harbour walk open today?');
      pc.send({ type: 'answer', text: 'Yes: the harbour walk is open from 8 am until dusk.', final: true, id: 'chat:2:1' });
      pc.send({ type: 'state', phase: 'idle' });
      await shot(page, `landscape-${w}x${h}`);
    }
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
  expect(await noSideways(page)).toBe(true);
  pc.autoReady = false;
  pc.close();
  await expect(page.getByTestId('conn')).toHaveText('Reconnecting to Office PC…');
  await shot(page, `reconnecting-narrow`);
});

test('large text: the composer and the chip still fit', async ({ page }) => {
  await servePage(page);
  await seedDevice(page);
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await page.evaluate(() => (document.documentElement.style.fontSize = '130%'));
  await type(page, pc, 'What time does the library close?');
  pc.send({ type: 'answer', text: 'The library closes at **6 pm** today.', final: true, id: 'chat:4:1' });
  pc.send({ type: 'state', phase: 'idle' });
  expect(await noSideways(page)).toBe(true);
  const talk = await page.getByTestId('talk').boundingBox();
  expect(talk.y + talk.height).toBeLessThanOrEqual(844);
  await shot(page, `large-text-390x844`);
});
