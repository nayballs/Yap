// Toasts never cover a message box (desktop/src/lib/ui/toastClear.js, ToastHost.svelte; 16 Nov 2026 — Chat's
// notices stacked at the foot of the main window, over the box's Send, and Yap Lab had to close them to reach it):
// - Pure: the stack's place for a window, its own corner and the boxes marked `use:keepClear` — its corner when
//   nothing's under its column, lifted above every box it would cover (an edit above the composer too), the
//   window's top when there's no room above.
// - In Chat, at the window's least size and larger, light and dark: a notice raised while a message is typed sits
//   above the message box, Send clear and clickable — a taller box (lines typed) lifts it further; with no box in
//   the view (Home) it's back in its corner; a new chat's box in the middle of its start screen leaves the stack
//   in its corner, under the box and clear of it.
// Toasts are raised through the notice policy in test mode (`e2e_notice`); one question goes, to the local fake.
import { test, expect, openView, setConfig } from './support/fixtures.js';
import { placeToasts } from '../src/lib/ui/toastClear.js';
import { startFakeLlm } from './support/fake-llm.js';

test.use({
  yapOptions: {
    name: 'toast-placement',
    config: {
      meetingReminder: 'never',
      // A Chat model that's never asked (nothing is sent): the box as it is with one.
      llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: 'http://127.0.0.1:9/v1', model: 'fake-chat' } },
    },
  },
});

// ---------------------------------------------------------------- no app

const VIEW = { width: 1200, height: 800 };
const BASE = { right: 20, bottom: 20 };
const STACK = { width: 352, height: 120 };

test('the stack stays in its corner unless a box is under its column, then rises above it — or goes to the top', () => {
  // Nothing marked, or a box clear of the column (a wide window's centred composer): its corner.
  expect(placeToasts({ view: VIEW, base: BASE, stack: STACK, boxes: [] })).toEqual({ bottom: 20 });
  expect(placeToasts({ view: VIEW, base: BASE, stack: STACK, boxes: [{ left: 220, right: 820, top: 680, bottom: 784 }] })).toEqual({ bottom: 20 });
  // The composer under the column: just above it (12px), whatever its height.
  const composer = { left: 300, right: 1180, top: 680, bottom: 784 };
  expect(placeToasts({ view: VIEW, base: BASE, stack: STACK, boxes: [composer] })).toEqual({ bottom: 132 });
  expect(placeToasts({ view: VIEW, base: BASE, stack: STACK, boxes: [{ ...composer, top: 540 }] })).toEqual({ bottom: 272 });
  // An edit in place just above the composer, in the way once the stack's up: above both.
  const edit = { left: 400, right: 1100, top: 470, bottom: 600 };
  expect(placeToasts({ view: VIEW, base: BASE, stack: STACK, boxes: [composer, edit] })).toEqual({ bottom: 342 });
  // A box well above the stack's place (an edit high in the thread): nothing moves for it.
  expect(placeToasts({ view: VIEW, base: BASE, stack: STACK, boxes: [{ left: 400, right: 1100, top: 100, bottom: 220 }] })).toEqual({ bottom: 20 });
  // A window's own corner (the notepad lifts it over its footer): kept when nothing's in the way, never lowered.
  expect(placeToasts({ view: VIEW, base: { right: 20, bottom: 104 }, stack: STACK, boxes: [{ left: 0, right: 1200, top: 714, bottom: 784 }] })).toEqual({ bottom: 104 });
  // No room above the box for the stack under the title bar: the window's top.
  expect(placeToasts({ view: { width: 900, height: 420 }, base: BASE, top: 56, stack: { width: 352, height: 200 }, boxes: [{ left: 0, right: 900, top: 200, bottom: 404 }] })).toEqual({ top: 56 });
});

// ---------------------------------------------------------------- in Chat

const box = (main) => main.getByRole('textbox', { name: 'Message' });
const sendBtn = (main) => main.getByRole('button', { name: 'Send', exact: true });
const stack = (main) => main.getByTestId('toast-stack');
const toastOf = (main, text) => main.getByRole('status').filter({ hasText: text });

async function rectOf(loc) {
  return loc.evaluate((el) => {
    const r = el.getBoundingClientRect();
    return { left: r.left, top: r.top, right: r.right, bottom: r.bottom };
  });
}
const overlap = (a, b) => a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top;

/** Send is what's under its own centre (nothing over it), and Playwright could click it. */
async function sendIsClear(main) {
  const r = await rectOf(sendBtn(main));
  const hit = await main.evaluate(([x, y]) => !!document.elementFromPoint(x, y)?.closest('button[aria-label="Send"]'), [(r.left + r.right) / 2, (r.top + r.bottom) / 2]);
  await sendBtn(main).click({ trial: true, timeout: 2_000 });
  return hit;
}

async function theme(yap, main, which) {
  await setConfig(yap, main, { theme: which });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe(which);
}

async function closeToasts(main) {
  for (let i = 0; i < 6 && (await main.getByRole('status').count()); i++) {
    await main.getByRole('status').first().getByRole('button', { name: 'Close', exact: true }).click({ timeout: 2_000 }).catch(() => {});
    await main.waitForTimeout(250);
  }
}

test.afterEach(async ({ yap, main }) => {
  await closeToasts(main).catch(() => {});
  await main.setViewportSize({ width: 1200, height: 800 });
  await setConfig(yap, main, { theme: 'light' }).catch(() => {});
});

const SIZES = [
  { width: 860, height: 600, name: 'least' },
  { width: 1100, height: 760, name: 'mid' },
  { width: 1600, height: 900, name: 'wide' },
];

test('a notice raised while a message is typed sits above the message box: Send stays clear at every size, light and dark', async ({ yap, main, shot }) => {
  test.setTimeout(120_000);
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).first().click();
  let n = 0;
  for (const which of ['light', 'dark']) {
    await theme(yap, main, which);
    for (const size of SIZES) {
      await main.setViewportSize({ width: size.width, height: size.height });
      await box(main).fill('Can you look up the opening times for the harbour museum on Saturday?');
      await expect(sendBtn(main)).toBeVisible();
      const title = `Notice ${++n}`;
      expect(await yap.invoke('e2e_notice', { surface: 'window', kind: `e2e-toast-${n}`, level: 'info', title, body: 'Raised while you type: it stays clear of the message box.', timeoutMs: 60_000 })).toBe(true);
      await expect(toastOf(main, title)).toBeVisible();
      // (Its entrance and any lift settled.)
      await main.waitForTimeout(450);
      const composer = await rectOf(main.locator('.composer'));
      await expect.poll(async () => overlap(await rectOf(stack(main)), composer), { message: `${size.name} ${which}: the stack over the box` }).toBe(false);
      expect(await sendIsClear(main)).toBe(true);
      // Still in the window, under nothing but its title bar.
      const s = await rectOf(stack(main));
      expect(s.top).toBeGreaterThanOrEqual(40);
      expect(s.right).toBeLessThanOrEqual(size.width);
      await shot(main, `${String(n).padStart(2, '0')}-toast-over-chat-${size.name}-${which}`);
      await closeToasts(main);
      await expect(main.getByRole('status')).toHaveCount(0);
    }
  }
});

test('a taller box lifts the stack further, and with no box in view it goes back to its corner', async ({ yap, main, shot }) => {
  await main.setViewportSize({ width: 860, height: 700 });
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).first().click();
  // A new chat's box sits in the middle of the thread (its start screen, 16 Nov 2026): a notice keeps its corner,
  // under the box and clear of it, however many lines are typed.
  await box(main).fill('A short line.');
  expect(await yap.invoke('e2e_notice', { surface: 'window', kind: 'e2e-toast-start', level: 'info', title: 'A notice on the start screen', body: 'Below the box.', timeoutMs: 60_000 })).toBe(true);
  await expect(toastOf(main, 'A notice on the start screen')).toBeVisible();
  await main.waitForTimeout(450);
  await box(main).fill(['Things for Saturday:', 'the harbour museum', 'the market', 'lunch at the pier', 'the lighthouse walk', 'back by six'].join('\n'));
  await main.waitForTimeout(450);
  expect(overlap(await rectOf(stack(main)), await rectOf(main.locator('.composer')))).toBe(false);
  expect(Math.round(700 - (await rectOf(stack(main))).bottom)).toBe(20);
  await closeToasts(main);
  // A chat with a message: its box at the foot, where the stack has to rise above it. (The fake answers once.)
  const fake = await startFakeLlm();
  const before = await yap.invoke('get_config');
  try {
    await setConfig(yap, main, { llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: fake.base, model: 'fake-chat', apiKey: '', apiKeys: {}, prompt: '', disableThinking: false } } });
    fake.scriptChat([{ content: 'Noted.' }]);
    await box(main).fill('Plan my Saturday.');
    await box(main).press('Enter');
    await expect(main.locator('.cbubble.assistant').last()).toContainText('Noted.');
  } finally {
    await setConfig(yap, main, { llmScopes: before.llmScopes });
    await fake.close();
  }
  await box(main).fill('A short line.');
  expect(await yap.invoke('e2e_notice', { surface: 'window', kind: 'e2e-toast-tall', level: 'info', title: 'A notice that stays', body: 'Until it is closed.', timeoutMs: 60_000 })).toBe(true);
  await expect(toastOf(main, 'A notice that stays')).toBeVisible();
  await main.waitForTimeout(450);
  const low = await rectOf(stack(main));
  // Six lines typed: the box grows up, the stack with it, Send still clear.
  await box(main).fill(['Things for Saturday:', 'the harbour museum', 'the market', 'lunch at the pier', 'the lighthouse walk', 'back by six'].join('\n'));
  await expect.poll(async () => (await rectOf(stack(main))).bottom, { timeout: 5_000 }).toBeLessThan(low.bottom - 20);
  // (Once its glide up is over.)
  await expect.poll(async () => overlap(await rectOf(stack(main)), await rectOf(main.locator('.composer'))), { timeout: 5_000 }).toBe(false);
  expect(await sendIsClear(main)).toBe(true);
  await shot(main, '07-toast-above-a-tall-box-light');
  // Home has no message box: the stack's own corner (20px from the foot).
  await openView(main, 'Home');
  await expect.poll(async () => Math.round(main.viewportSize().height - (await rectOf(stack(main))).bottom), { timeout: 5_000 }).toBe(20);
  await shot(main, '08-toast-in-its-corner-without-a-box-light');
});
