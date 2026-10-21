// The Yap bar's cards with a REAL mouse (Nathan, 2026-10-19: a card's ✕
// "wouldn't let me click"). The bar is a click-through window that takes
// clicks only where the cursor is on something it draws (bar.rs polls the
// cursor), so the e2e suite's simulated pointer can't prove a real click
// lands. Here the robot's Yap follows the real cursor
// (YAP_E2E_REAL_CURSOR=1), a demo card goes up, the pointer goes onto its ✕
// or a button, and a real left click (SendInput, the helper's `barclick`)
// goes out — only when Windows puts the robot's own Yap under the pointer.
// The card must close, and the app in front must keep the focus.
import { test, expect } from './support/robot.js';
import { helper } from './support/targets.js';

test.use({ robotOptions: { name: 'robot-bar', env: { YAP_E2E_REAL_CURSOR: '1' } } });

/** The middle of `locator` on screen, in physical pixels. */
async function onScreen(yap, locator) {
  const pos = await yap.invoke('plugin:window|outer_position', { label: 'overlay' });
  const scale = await yap.invoke('plugin:window|scale_factor', { label: 'overlay' });
  const b = await locator.boundingBox();
  return { x: Math.round(pos.x + (b.x + b.width / 2) * scale), y: Math.round(pos.y + (b.y + b.height / 2) * scale) };
}

async function click(yap, locator) {
  const { x, y } = await onScreen(yap, locator);
  const r = helper('barclick', yap.child.pid, x, y, 700);
  expect(r.ok, r.reason ?? 'clicked').toBe(true);
  expect(r.fgAfter, 'the app in front kept the focus').toBe(r.fgBefore);
  return r;
}

// The answer besides the ✕: a button on the card, or (a call card) an item
// of its ^ menu, opened with a real click too.
const CARDS = [
  { kind: 'update', text: 'Yap 0.2.0 is ready', button: 'Later' },
  { kind: 'call', text: 'Discord call detected', menu: 'More answers', button: 'Not now' },
];

for (const c of CARDS) {
  test(`a real click on the ${c.kind} card's ✕ closes it, and so does "${c.button}"`, async ({ robot }) => {
    const { yap } = robot;
    const bar = yap.overlay;
    const card = bar.getByRole('status').filter({ hasText: c.text });
    // The app in front: the robot's test pad (never Yap's bar).
    await robot.pad.focus();

    await yap.invoke('bar_simulate', { card: c.kind });
    await expect(card).toBeVisible();
    await bar.waitForTimeout(600); // placed, its regions reported
    await click(yap, card.getByRole('button', { name: 'Close' }));
    await expect(card).toHaveCount(0, { timeout: 3_000 });

    await yap.invoke('bar_simulate', { card: c.kind });
    await expect(card).toBeVisible();
    await bar.waitForTimeout(600);
    if (c.menu) {
      await click(yap, card.getByRole('button', { name: c.menu }));
      await expect(card.getByRole('menuitem', { name: c.button })).toBeVisible();
      await bar.waitForTimeout(300); // the menu's region reported
      await click(yap, card.getByRole('menuitem', { name: c.button }));
    } else {
      await click(yap, card.getByRole('button', { name: c.button }));
    }
    await expect(card).toHaveCount(0, { timeout: 3_000 });
  });
}

// Straight after the card appears: the pointer arrives as the card fades in
// (where Nathan's click was), with only a short rest before the click.
test('a card clicked the moment it appears still closes', async ({ robot }) => {
  const { yap } = robot;
  const bar = yap.overlay;
  const card = bar.getByRole('status').filter({ hasText: 'Yap 0.2.0 is ready' });
  await robot.pad.focus();
  await yap.invoke('bar_simulate', { card: 'update' });
  await expect(card).toBeVisible();
  const { x, y } = await onScreen(yap, card.getByRole('button', { name: 'Close' }));
  const r = helper('barclick', yap.child.pid, x, y, 350);
  expect(r.ok, r.reason ?? 'clicked').toBe(true);
  await expect(card).toHaveCount(0, { timeout: 3_000 });
});
