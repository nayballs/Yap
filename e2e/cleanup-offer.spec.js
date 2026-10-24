// The one gentle offer of AI cleanup (ai/cleanup_styles.rs `offer`,
// cleanupOffer.js, 2026-10-24): someone with cleanup off, on a PC whose
// graphics card runs the on-device model, gets one tip in the main window
// after a dictation — "Let Yap tidy what you say", with Set it up (Settings →
// Language Models) and Not now — and never again, whatever the answer.
// Existing installs keep cleanup as they had it: nothing turns it on. And
// onboarding recommends cleanup on this PC here (cleanup-styles.spec.js has
// the PC without a card).
//
// The card is a stand-in (YAP_E2E_GPU); a test run offers the tip only when
// the spec asks (YAP_E2E_CLEANUP_OFFER), so no other spec meets it.
import { test as base, expect, openSettings, closeSettings, expectStore, setConfig } from './support/fixtures.js';

const NOW = Math.floor(Date.now() / 1000);
const CARD = 'Test Graphics 9000';
const HISTORY = Array.from({ length: 6 }, (_, i) => ({
  ts: NOW - 3600 + i * 60,
  raw: `so the plan for item ${i + 1} is fine`,
  text: `So the plan for item ${i + 1} is fine.`,
  model: 'parakeet-tdt-0.6b-v3',
  app: 'notepad.exe',
}));

const test = base.extend({
  yapOptions: [
    async ({}, use) => {
      await use({
        name: 'cleanup-offer',
        env: { YAP_E2E_GPU: `${CARD},12282`, YAP_E2E_CLEANUP_OFFER: '1' },
        data: { 'history.json': HISTORY },
      });
    },
    { scope: 'worker' },
  ],
});

const offerToast = (main) => main.getByRole('status').filter({ hasText: 'Let Yap tidy what you say' });

test('cleanup off on a PC the on-device model fits: one tip after a dictation, never again', async ({ yap, main, shot }) => {
  const cfg = await yap.invoke('get_config');
  // Nothing turned it on.
  expect(cfg.postProcessEnabled).toBe(false);
  expect(cfg.cleanupOfferSeen).toBe(false);
  expect((await yap.invoke('cleanup_offer_status')).offer).toBe(true);

  // A dictation is saved: the tip, naming the card it runs on.
  await yap.invoke('e2e_dictation', { text: 'and one more thing' });
  const tip = offerToast(main);
  await expect(tip).toBeVisible({ timeout: 15_000 });
  await expect(tip).toContainText(`It runs on your ${CARD}, so nothing leaves this PC`);
  await shot(main, '01-cleanup-offer');
  await expectStore(yap, 'config.json', (c) => c.cleanupOfferSeen === true);

  // Set it up: Settings → Language Models, cleanup still off until chosen.
  await tip.getByRole('button', { name: 'Set it up' }).click();
  const dialog = main.getByRole('dialog', { name: 'Settings' });
  await expect(dialog.getByRole('button', { name: 'Clean up dictations with AI' })).toBeVisible();
  expect((await yap.invoke('get_config')).postProcessEnabled).toBe(false);
  await closeSettings(main);

  // Never again: another dictation, no tip.
  expect((await yap.invoke('cleanup_offer_status')).offer).toBe(false);
  await yap.invoke('e2e_dictation', { text: 'and another thing' });
  await main.waitForTimeout(4_500);
  await expect(offerToast(main)).toHaveCount(0);
});

test('no tip with tips off or with cleanup on', async ({ yap, main }) => {
  await setConfig(yap, main, { cleanupOfferSeen: false, showTips: false });
  expect((await yap.invoke('cleanup_offer_status')).offer).toBe(false);
  await setConfig(yap, main, { showTips: true, postProcessEnabled: true });
  expect((await yap.invoke('cleanup_offer_status')).offer).toBe(false);
  await setConfig(yap, main, { postProcessEnabled: false });
  expect((await yap.invoke('cleanup_offer_status')).offer).toBe(true);
  await setConfig(yap, main, { cleanupOfferSeen: true });
});

test('onboarding: this PC runs the on-device model, so cleanup on this PC is recommended', async ({ yap, main, shot }) => {
  const onboarding = yap.onboarding;
  const dialog = await openSettings(main, 'About');
  await dialog.getByRole('button', { name: /Show setup guide again/ }).click();
  await expect(onboarding.getByRole('heading', { level: 1, name: 'Welcome to Yap' })).toBeVisible();
  await onboarding.getByRole('button', { name: "I'll choose later" }).click();
  await onboarding.getByRole('button', { name: 'Continue →' }).click();
  await expect(onboarding.getByRole('heading', { level: 1, name: 'Make it sound polished' })).toBeVisible();
  const local = onboarding.locator('.mode-card[data-recommended="yes"]');
  await expect(local).toContainText('Private');
  await expect(local).toContainText('Recommended for this PC');
  await expect(local).toHaveClass(/sel/);
  await expect(onboarding.locator('.llm-row[data-model="qwen3.5-4b"] .llm-badge')).toHaveText('Recommended for this PC');
  await shot(onboarding, '02-onboarding-recommends-this-pc');
  await yap.invoke('close_onboarding');
  await closeSettings(main);
});
