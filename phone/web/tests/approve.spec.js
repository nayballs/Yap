// The cards an agent waits on, on the phone (Remote Control, wave 2;
// cards.js): a permission prompt (Allow once / Deny), a plan (Go ahead /
// Not yet), Yap's ask-first card (Start / Don't). **Each a fresh tap**: the
// buttons asleep for the card's first MIN_SHOWN_MS on screen (an early tap
// does nothing), `shownMs` sent with the answer, a script's click never
// counted; "Allowing…" until the PC says how it ended, and who when another
// phone or the PC answered first. The Needs-you banner on every screen,
// a tap to the card. Notifications in the background where the browser
// allows them, asked for only on a tap. Yap on the PC mocked with the
// wire's own examples (support/code-pc.js).
import { test, expect } from '@playwright/test';
import { servePage, seedDevice, open, ready, snapshot, openSettings } from './support/harness.js';
import { mockCodePc, out, approveCard, MIN_SHOWN_MS, CODE } from './support/code-pc.js';

test.beforeEach(async ({ page }) => {
  await servePage(page);
  await seedDevice(page);
});

async function openChat12(page) {
  await page.getByTestId('menu').click();
  await page.getByTestId('side-code').click();
  await page.locator('[data-testid="code-chat"][data-id="12"]').click();
  await expect(page.getByTestId('turn')).toHaveCount(2);
}
const card = (page, id) => page.locator(`[data-testid="code-card"][data-id="${id}"]`).first();

test('a permission card: the banner, the card in its thread, asleep at first (an early tap does nothing), then Allow once → Allowing… → Allowed', async ({ page }) => {
  const pc = await mockCodePc(page, { auto: { approve: false } });
  await open(page);
  await ready(page, pc);
  await openChat12(page);
  pc.send(approveCard());
  const c = card(page, 7);
  await expect(c).toBeVisible();
  await expect(c.getByTestId('card-summary')).toHaveText('Run a command: npm test');
  await expect(c.getByTestId('card-detail')).toHaveText('npm test');
  await expect(c).toContainText('The agent in garden-planner asks to:');
  await expect(c.getByTestId('card-time')).toContainText('Yap says no in');
  await expect(page.getByTestId('needs')).toBeVisible();
  await expect(page.getByTestId('needs')).toHaveText('A step in garden-planner needs you');
  await expect(page).toHaveTitle('(1) Yap Link');
  // Its first moment on screen: both asleep, and a real tap there does nothing.
  const yes = c.getByTestId('card-yes');
  await expect(yes).toBeDisabled();
  await expect(c.getByTestId('card-no')).toBeDisabled();
  const box = await yes.boundingBox();
  await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
  await page.waitForTimeout(80);
  expect(pc.of('approve')).toEqual([]);
  // Awake once it's been shown long enough.
  await expect(yes).toBeEnabled({ timeout: MIN_SHOWN_MS + 2000 });
  // A script's click never answers (only a trusted tap does).
  await yes.evaluate((b) => b.click());
  await page.waitForTimeout(80);
  expect(pc.of('approve')).toEqual([]);
  await yes.click();
  await expect.poll(() => pc.of('approve').length).toBe(1);
  const sent = pc.of('approve')[0];
  expect(sent).toMatchObject({ type: 'approve', id: 7, allow: true });
  expect(sent.shownMs).toBeGreaterThanOrEqual(MIN_SHOWN_MS);
  await expect(c.getByTestId('card-status')).toHaveText('Allowing…');
  await expect(c.getByTestId('card-yes')).toBeHidden();
  // The PC says how it ended.
  pc.send(out('approve_done', { id: 7, outcome: 'allowed', by: 'you', name: undefined }));
  await expect(c.getByTestId('card-status')).toHaveText('Allowed');
  await expect(c).toHaveAttribute('data-state', 'done');
  await expect(page.getByTestId('needs')).toBeHidden();
  await expect(page).toHaveTitle('Yap Link');
  expect(pc.codeSide.wireErrors).toEqual([]);
});

test('answered elsewhere: "Allowed from Kitchen tablet", "Denied on your PC", a tap too late, the PC turning down a tap, and no answer in time', async ({ page }) => {
  const pc = await mockCodePc(page, { auto: { approve: false } });
  await open(page);
  await ready(page, pc);
  await openChat12(page);
  pc.send(approveCard());
  pc.send(approveCard({ card: { id: 8, summary: 'Edit a file: notes.md', detail: 'notes.md', askedTs: 1730000130 } }));
  pc.send(approveCard({ card: { id: 9, summary: 'Run a command: npm run lint', detail: 'npm run lint', askedTs: 1730000140 } }));
  pc.send(approveCard({ card: { id: 10, summary: 'Run a command: npm run build', detail: 'npm run build', askedTs: 1730000150 } }));
  await expect(page.getByTestId('needs')).toHaveText('4 steps need you');
  // Another phone answered first.
  pc.send(out('approve_done', { id: 7, outcome: 'allowed', by: 'phone', name: 'Kitchen tablet' }));
  await expect(card(page, 7).getByTestId('card-status')).toHaveText('Allowed from Kitchen tablet');
  // The PC answered.
  pc.send(out('approve_done', { id: 8, outcome: 'denied', by: 'pc', name: undefined }));
  await expect(card(page, 8).getByTestId('card-status')).toHaveText('Denied on your PC');
  // Tapped here, but another got there first: the PC says so. (A card asleep
  // until it's been on screen: brought into view first.)
  await card(page, 9).scrollIntoViewIfNeeded();
  await expect(card(page, 9).getByTestId('card-yes')).toBeEnabled({ timeout: MIN_SHOWN_MS + 2000 });
  await card(page, 9).getByTestId('card-yes').click();
  await expect(card(page, 9).getByTestId('card-status')).toHaveText('Allowing…');
  pc.send({ type: 'error', message: 'Already allowed from Kitchen tablet.' });
  await expect(card(page, 9).getByTestId('card-status')).toHaveText('Already allowed from Kitchen tablet');
  await expect(page.getByTestId('error')).toBeHidden();
  // A tap the PC turned down: the card waits again, saying why.
  await card(page, 10).getByTestId('card-no').click();
  await expect(card(page, 10).getByTestId('card-status')).toHaveText('Denying…');
  pc.send({ type: 'error', message: 'Tap it again: the card had only just come.' });
  await expect(card(page, 10).getByTestId('card-error')).toHaveText('Tap it again: the card had only just come.');
  await expect(card(page, 10).getByTestId('card-no')).toBeVisible();
  // No answer in time.
  pc.send(out('approve_done', { id: 10, outcome: 'timedOut', by: null, name: undefined }));
  await expect(card(page, 10).getByTestId('card-status')).toHaveText('No answer in time, so the agent was told no.');
  await expect(page.getByTestId('needs')).toBeHidden();
  expect(pc.codeSide.wireErrors).toEqual([]);
});

test("a plan's card: the plan itself, Not yet and Go ahead", async ({ page }) => {
  const pc = await mockCodePc(page);
  await open(page);
  await ready(page, pc);
  await openChat12(page);
  const plan = '## The plan\n\n1. A table per bed\n2. A row for each day\n3. The tests for it\n4. A note in the README\n5. Nothing else';
  pc.send(approveCard({ card: { id: 11, tool: 'ExitPlanMode', summary: 'Go ahead with the plan', detail: plan } }));
  const c = card(page, 11);
  await expect(c).toHaveAttribute('data-kind', 'plan');
  await expect(c).toContainText('The plan is ready · garden-planner');
  await expect(c.getByTestId('card-plan')).toContainText('A table per bed');
  await expect(page.getByTestId('needs')).toHaveText('The plan in garden-planner is ready for you');
  await expect(c.getByTestId('card-no')).toHaveText('Not yet');
  await expect(c.getByTestId('card-yes')).toHaveText('Go ahead');
  await expect(c.getByTestId('card-no')).toBeEnabled({ timeout: MIN_SHOWN_MS + 2000 });
  await c.getByTestId('card-no').click();
  await expect.poll(() => pc.of('approve').at(-1)).toMatchObject({ id: 11, allow: false });
  await expect(c.getByTestId('card-status')).toHaveText('Kept as a plan');
  // Another plan: Go ahead.
  pc.send(approveCard({ card: { id: 12, tool: 'ExitPlanMode', summary: 'Go ahead with the plan', detail: '1. Only the herbs bed' } }));
  await expect(card(page, 12).getByTestId('card-yes')).toBeEnabled({ timeout: MIN_SHOWN_MS + 2000 });
  await card(page, 12).getByTestId('card-yes').click();
  await expect.poll(() => pc.of('approve').at(-1)).toMatchObject({ id: 12, allow: true });
  await expect(card(page, 12).getByTestId('card-status')).toHaveText('Going ahead with the plan');
  expect(pc.codeSide.wireErrors).toEqual([]);
});

test("the ask-first card: the workspace, its Mode and brief, why Yap asks; Start / Don't", async ({ page }) => {
  const pc = await mockCodePc(page);
  await open(page);
  await ready(page, pc);
  await openChat12(page);
  pc.send(out('confirm_card'));
  const c = page.locator('[data-testid="code-card"][data-kind="confirm"]');
  await expect(c).toBeVisible();
  await expect(c).toContainText('Yap wants to send an agent to ‘garden-planner’ (Auto)');
  await expect(c.getByTestId('card-brief')).toHaveText('Add a watering schedule');
  await expect(c).toContainText('This chat read the web');
  await expect(page.getByTestId('needs')).toHaveText('Yap asks before sending an agent to garden-planner');
  await expect(c.getByTestId('card-no')).toHaveText("Don't");
  await expect(c.getByTestId('card-yes')).toHaveText('Start');
  await expect(c.getByTestId('card-yes')).toBeEnabled({ timeout: MIN_SHOWN_MS + 2000 });
  await c.getByTestId('card-yes').click();
  await expect.poll(() => pc.of('confirm').length).toBe(1);
  expect(pc.of('confirm')[0]).toMatchObject({ type: 'confirm', id: 3, yes: true });
  expect(pc.of('confirm')[0].shownMs).toBeGreaterThanOrEqual(MIN_SHOWN_MS);
  await expect(c.getByTestId('card-status')).toHaveText('Started');
  expect(pc.codeSide.wireErrors).toEqual([]);
});

test('the Needs-you banner shows on every screen, and a tap goes to its card — in another chat, opened first', async ({ page }) => {
  const pc = await mockCodePc(page);
  const wait = approveCard({ card: { id: 21, conversation: 14, run: 'ef56ab78', label: 'recipe-box', summary: 'Run a command: npm test', detail: 'npm test' } });
  pc.threads[14] = {
    title: 'Fix the failing tests',
    messages: [
      { role: 'user', text: "Find and fix what's failing" },
      { role: 'assistant', text: "On it: I've sent an agent to recipe-box.", id: 'chat:14:1' },
    ],
    code: { ...CODE, workspace: 'recipe-box', path: 'D:\\Projects\\recipe-box' },
    agents: [],
    cards: [{ kind: 'approve', card: wait.card, expiresTs: wait.expiresTs }],
  };
  await open(page);
  await ready(page, pc);
  // On a plain chat (the Chat side) too.
  await page.getByTestId('menu').click();
  await page.getByTestId('recent').first().click();
  pc.send(wait);
  await expect(page.getByTestId('needs')).toHaveText('A step in recipe-box needs you');
  // A tap: its chat, then its card in view.
  await page.getByTestId('needs').click();
  await expect.poll(() => pc.of('open_chat').at(-1)).toEqual({ type: 'open_chat', id: 14 });
  await expect(card(page, 21)).toBeInViewport();
  await expect(page.getByTestId('workspace-chip')).toHaveText('recipe-box');
  // In an agent's live view too.
  await page.getByTestId('menu').click();
  await page.locator('[data-testid="code-chat"][data-id="12"]').click();
  await page.getByTestId('agent-pill').click();
  await expect(page.getByTestId('agent-view')).toBeVisible();
  await expect(page.getByTestId('needs')).toBeVisible();
  expect((await snapshot(page)).code.cards).toEqual([expect.objectContaining({ kind: 'approve', id: 21, state: 'waiting' })]);
  expect(pc.codeSide.wireErrors).toEqual([]);
});

test("a card waiting for the agent open in the live view shows there, and is answered there", async ({ page }) => {
  const pc = await mockCodePc(page);
  await open(page);
  await ready(page, pc);
  await openChat12(page);
  await page.getByTestId('agent-pill').click();
  await expect(page.getByTestId('agent-view')).toBeVisible();
  pc.send(approveCard());
  const c = page.getByTestId('agent-cards').locator('[data-testid="code-card"][data-id="7"]');
  await expect(c).toBeVisible();
  await expect(c.getByTestId('card-yes')).toBeEnabled({ timeout: MIN_SHOWN_MS + 2000 });
  await c.getByTestId('card-yes').click();
  await expect.poll(() => pc.of('approve').length).toBe(1);
  await expect(c.getByTestId('card-status')).toHaveText('Allowed');
});

/** A stand-in for the browser's notifications: what's asked and shown (`throws`: Chrome on Android's). */
async function fakeNotifications(page, { throws = false } = {}) {
  await page.addInitScript((throws) => {
    window.__notes = [];
    window.__asked = 0;
    class FakeNotification {
      static permission = 'default';
      static async requestPermission() {
        window.__asked++;
        FakeNotification.permission = 'granted';
        return 'granted';
      }
      constructor(title, opts = {}) {
        if (throws) throw new TypeError("Failed to construct 'Notification': Illegal constructor. Use ServiceWorkerRegistration.showNotification() instead.");
        window.__notes.push({ title, body: opts.body, tag: opts.tag });
      }
      close() {}
    }
    window.Notification = FakeNotification;
  }, throws);
}
const hide = (page) =>
  page.evaluate(() => {
    Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => 'hidden' });
    document.dispatchEvent(new Event('visibilitychange'));
  });

test('notifications: asked for only on a tap, then a step that needs you and a finished agent while the page is in the background', async ({ page }) => {
  await fakeNotifications(page);
  const pc = await mockCodePc(page);
  await open(page);
  await ready(page, pc);
  await expect.poll(async () => (await snapshot(page)).code.on).toBe(true);
  // Never asked on load.
  expect(await page.evaluate(() => window.__asked)).toBe(0);
  await openSettings(page);
  await expect(page.getByTestId('notify-row')).toBeVisible();
  await expect(page.getByTestId('notify')).not.toBeChecked();
  await page.getByTestId('notify-row').click();
  await expect(page.getByTestId('notify')).toBeChecked();
  expect(await page.evaluate(() => window.__asked)).toBe(1);
  await expect(page.getByTestId('notify-line')).toContainText('On: while this page is open in the background');
  expect(await page.evaluate(() => window.__notes.map((n) => n.title))).toEqual(['Notifications are on']);
  await page.getByTestId('settings-sheet').getByRole('button', { name: 'Done' }).click();
  // In the front: no notification (the banner and a buzz instead).
  pc.send(approveCard({ card: { id: 31 } }));
  await expect(page.getByTestId('needs')).toBeVisible();
  expect(await page.evaluate(() => window.__notes.length)).toBe(1);
  // In the background: one per card, no buttons; and the agent's report.
  await hide(page);
  pc.send(approveCard({ card: { id: 32, summary: 'Edit a file: notes.md' } }));
  pc.send(approveCard({ card: { id: 32, summary: 'Edit a file: notes.md' } }));
  pc.send(out('report', { conversation: 12, status: 'done' }));
  await expect.poll(() => page.evaluate(() => window.__notes.map((n) => n.title))).toEqual(['Notifications are on', 'A step in garden-planner needs you', 'Agent done · garden-planner']);
  expect(await page.evaluate(() => window.__notes[1].body)).toBe('Open Yap Link to answer it.');
});

test("notifications where the browser can't show a page's own (Chrome on Android): the sheet says so, and the banner stays", async ({ page }) => {
  await fakeNotifications(page, { throws: true });
  const pc = await mockCodePc(page);
  await open(page);
  await ready(page, pc);
  await expect.poll(async () => (await snapshot(page)).code.on).toBe(true);
  await openSettings(page);
  await page.getByTestId('notify-row').click();
  await expect(page.getByTestId('notify-line')).toContainText('shows notifications only from an installed web app');
  await expect(page.getByTestId('notify')).not.toBeChecked();
  expect((await snapshot(page)).code.notify).toEqual({ want: false, on: false });
});
