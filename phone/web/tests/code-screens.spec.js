// Remote Control's Code side as named screenshots (wave 2), light and dark,
// at a small and a middling phone, in test-results/link/screenshots/ (to
// look at, not compared): the drawer's Code side (the agents working, the
// workspaces with their dots), a Code chat's thread (a pill, a report card,
// the workspace and Mode chips), a permission card waiting with the
// Needs-you banner, the Mode sheet, the agent's live view (its to-dos,
// steps, files) and a diff opened, a plan's card. Also checks nothing
// overflows sideways and every control is a thumb's size (44 px or more).
import { test, expect } from '@playwright/test';
import { servePage, seedDevice, open, ready } from './support/harness.js';
import { mockCodePc, brief, approveCard, MIN_SHOWN_MS, T0 } from './support/code-pc.js';

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

const RUNNING = brief({ status: 'running', now: 'Adding the schedule', endedTs: 0, reportMessageId: undefined, turnList: [{ n: 1, title: 'Add a watering schedule', status: 'running', messageId: 'chat:12:0', startedTs: T0 }] });
const OTHER = brief({ id: 'ef56ab78', conversation: 14, folder: 'recipe-box', status: 'asking', now: null, startedTs: T0 + 60, endedTs: 0, messageId: 'chat:14:0', reportMessageId: undefined, turnList: [{ n: 1, title: 'Fix the failing tests', status: 'asking', messageId: 'chat:14:0', startedTs: T0 + 60 }] });

for (const scheme of ['light', 'dark']) {
  for (const [w, h] of SIZES) {
    const at = `${w}x${h}-${scheme}`;
    test.describe(`${scheme} ${w}×${h}`, () => {
      test.use({ colorScheme: scheme, viewport: { width: w, height: h } });

      test(`the Code side (${at})`, async ({ page }) => {
        await servePage(page);
        await seedDevice(page);
        const pc = await mockCodePc(page, { agents: [RUNNING, OTHER], thread: { agents: [RUNNING] } });
        await open(page);
        await ready(page, pc);
        // The dots: chat 12 working, chat 14 waiting on you.
        pc.status({ agents: [RUNNING, OTHER], approvals: [{ id: 21, conversation: 14, agent: 'ef56ab78' }] });
        await page.getByTestId('menu').click();
        await page.getByTestId('side-code').click();
        await expect(page.getByTestId('code-running-agent')).toHaveCount(2);
        await expect(page.locator('[data-testid="code-chat"][data-id="14"] [data-testid="code-chat-dot"]')).toHaveAttribute('data-state', 'needs');
        await shot(page, `code-drawer-${at}`);
        expect(await tooSmall(page)).toEqual([]);
        expect(await noSideways(page)).toBe(true);

        // A Code chat: its pill, its report, its chips.
        await page.locator('[data-testid="code-chat"][data-id="12"]').click();
        await expect(page.getByTestId('report-card')).toBeVisible();
        await shot(page, `code-thread-${at}`);
        expect(await tooSmall(page)).toEqual([]);
        expect(await noSideways(page)).toBe(true);

        // The agent's live view from its pill (its top: status, cost, the line, to-dos), then a diff opened.
        await page.getByTestId('agent-pill').click();
        await expect(page.getByTestId('agent-step')).toHaveCount(3);
        await page.getByTestId('agent-body').evaluate((b) => b.scrollTo(0, 0));
        await shot(page, `agent-view-${at}`);
        expect(await tooSmall(page)).toEqual([]);
        expect(await noSideways(page)).toBe(true);
        await page.getByTestId('agent-file').click();
        await expect(page.getByTestId('agent-diff').getByTestId('diff-line')).toHaveCount(7);
        await page.getByTestId('agent-diff').scrollIntoViewIfNeeded();
        await shot(page, `agent-diff-${at}`);
        expect(await noSideways(page)).toBe(true);

        // A step waiting while it's open: the card at its live end, the banner.
        pc.send(approveCard({ card: { askedTs: Math.floor(Date.now() / 1000) - 40 }, expiresTs: Math.floor(Date.now() / 1000) + 560 }));
        const inView = page.getByTestId('agent-cards').locator('[data-testid="card-yes"]');
        await expect(inView).toBeEnabled({ timeout: MIN_SHOWN_MS + 2000 });
        await shot(page, `agent-view-card-${at}`);
        expect(await tooSmall(page)).toEqual([]);
        expect(await noSideways(page)).toBe(true);

        // Back in the chat: the card in its thread.
        await page.getByTestId('agent-close').click();
        await page.locator('#thread-cards [data-testid="code-card"]').scrollIntoViewIfNeeded();
        await expect(page.locator('#thread-cards [data-testid="card-yes"]')).toBeEnabled();
        await shot(page, `code-approve-${at}`);
        expect(await tooSmall(page)).toEqual([]);
        expect(await noSideways(page)).toBe(true);

        // The Mode sheet.
        await page.getByTestId('mode-chip').click();
        await expect(page.getByTestId('mode-sheet')).toBeVisible();
        await shot(page, `code-mode-${at}`);
        expect(await tooSmall(page)).toEqual([]);
        await page.getByTestId('mode-sheet').getByRole('button', { name: 'Done' }).click();
      });

      test(`a plan's card (${at})`, async ({ page }) => {
        await servePage(page);
        await seedDevice(page);
        const pc = await mockCodePc(page);
        await open(page);
        await ready(page, pc);
        await page.getByTestId('menu').click();
        await page.getByTestId('side-code').click();
        await page.locator('[data-testid="code-chat"][data-id="12"]').click();
        pc.send(
          approveCard({
            card: {
              id: 11,
              tool: 'ExitPlanMode',
              summary: 'Go ahead with the plan',
              detail: '## Watering schedule\n\n1. A table in notes.md: a row for each bed\n2. The days to water each one\n3. A test that the table parses\n4. Nothing else changes',
              askedTs: Math.floor(Date.now() / 1000) - 20,
            },
            expiresTs: Math.floor(Date.now() / 1000) + 580,
          }),
        );
        await expect(page.locator('[data-testid="code-card"][data-id="11"] [data-testid="card-yes"]')).toBeEnabled({ timeout: MIN_SHOWN_MS + 2000 });
        await shot(page, `code-plan-${at}`);
        expect(await tooSmall(page)).toEqual([]);
        expect(await noSideways(page)).toBe(true);
      });
    });
  }
}
