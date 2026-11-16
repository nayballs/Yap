// Retry and its versions on the phone as named screenshots (16 Nov 2026),
// light and dark, at a small and a middling phone, in
// test-results/link/screenshots/ (to look at, not compared): a chat whose
// question was edited (‹ 2/2 › under its bubble) and whose answer was
// retried (‹ 3/3 › first under it, then Copy, ▶ and ↻), a ↻ under way (its
// answer written afresh, what followed gone with the old version), and the
// new answer in with its ‹ 4/4 ›. Also checks nothing overflows sideways and
// every control is a thumb's size (44 px or more). Chromium only, as the
// page's other screenshots.
import { test, expect } from '@playwright/test';
import { servePage, seedDevice, mockPc, open, ready, CAN } from './support/harness.js';

const DIR = 'test-results/link/screenshots';
async function shot(page, name) {
  await page.waitForTimeout(250);
  await page.screenshot({ path: `${DIR}/${name}.png` });
}
const SIZES = [
  [360, 780],
  [390, 844],
];
/** Controls smaller than a thumb (44 px tall), visible ones only (screens.spec.js's rule). */
const tooSmall = (page) =>
  page.evaluate(() =>
    [...document.querySelectorAll('button:not([hidden]), select, .switch, .seg label, summary, textarea')]
      .filter((e) => e.offsetParent || e.closest('dialog[open]'))
      .filter((e) => e.getBoundingClientRect().height > 0)
      .map((e) => [e.dataset.testid || e.id || e.className || e.tagName, Math.round(e.getBoundingClientRect().height)])
      .filter(([, h]) => h < 44),
  );
const noSideways = (page) => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth);

const QUESTION = 'What should I pack for three days of hiking in the rain?';
const THREAD = {
  title: 'Trip packing list',
  messages: [
    { role: 'user', text: QUESTION, version: { at: 0, shown: 2, count: 2 } },
    {
      role: 'assistant',
      text: 'Pack **layers** you can add and take off:\n\n- A waterproof jacket and overtrousers\n- Two merino base layers\n- Spare socks in a dry bag',
      id: 'chat:41:1',
      version: { at: 1, shown: 3, count: 3 },
      retry: true,
    },
    { role: 'user', text: 'Is a 35-litre rucksack enough?' },
    { role: 'assistant', text: 'Yes, for three days if you stay in huts.', id: 'chat:41:3', retry: true },
  ],
};

for (const scheme of ['light', 'dark']) {
  for (const [w, h] of SIZES) {
    const at = `${w}x${h}-${scheme}`;
    test.describe(`${scheme} ${w}×${h}`, () => {
      test.use({ colorScheme: scheme, viewport: { width: w, height: h } });

      test(`Retry and versions (${at})`, async ({ page }) => {
        await servePage(page);
        await seedDevice(page);
        await page.addInitScript(() => {
          if (!sessionStorage.getItem('in-chat')) {
            localStorage.setItem('yap.link.chat', '41');
            sessionStorage.setItem('in-chat', '1');
          }
        });
        const pc = await mockPc(page, { can: [...CAN, 'versions', 'retry'], threads: { 41: THREAD } });
        await open(page);
        await ready(page, pc);
        await expect(page.getByTestId('turn')).toHaveCount(2);
        await expect(page.getByTestId('answer-versions').first()).toBeVisible();
        await shot(page, `versions-thread-${at}`);
        expect(await tooSmall(page)).toEqual([]);
        expect(await noSideways(page)).toBe(true);

        // ↻ on the first answer: written afresh, what followed gone with the old version.
        await page.getByTestId('turn').first().getByTestId('retry').click();
        await expect.poll(() => pc.of('retry').length).toBe(1);
        pc.send({ type: 'state', phase: 'thinking' });
        pc.send({ type: 'step', label: 'Searching the web · rain hiking kit list' });
        pc.send({ type: 'answer', text: 'For three wet days, pack a', final: false });
        await expect(page.getByTestId('answer')).toHaveText('For three wet days, pack a');
        await shot(page, `versions-retrying-${at}`);
        expect(await noSideways(page)).toBe(true);

        // The new answer in, the thread again with its ‹ 4/4 ›.
        const text = 'For three wet days, pack a rain jacket, overtrousers, gaiters and a warm hat.';
        pc.send({ type: 'answer', text, final: true, id: 'chat:41:1', retry: true });
        pc.send({ type: 'state', phase: 'idle' });
        pc.threads[41] = {
          title: 'Trip packing list',
          messages: [
            { role: 'user', text: QUESTION, version: { at: 0, shown: 2, count: 2 } },
            { role: 'assistant', text, id: 'chat:41:1', version: { at: 1, shown: 4, count: 4 }, retry: true },
          ],
        };
        pc.send({ type: 'versions', chat: 41 });
        await expect(page.getByTestId('answer-versions').getByTestId('version-count')).toHaveText('4/4');
        await shot(page, `versions-retried-${at}`);
        expect(await tooSmall(page)).toEqual([]);
        expect(await noSideways(page)).toBe(true);
      });
    });
  }
}
