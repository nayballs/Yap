// Onboarding on a small PC (ai/hardware.rs, docs/benchmarks.md; 2026-10-24):
// no graphics card (`YAP_E2E_GPU=none`), 4 processor threads
// (`YAP_E2E_CPU_THREADS`) and 8 GB (`YAP_E2E_RAM_MIB`) stand in for a
// budget laptop. Speech is said to run on the processor (and does, before
// this PC's own timing: Settings → Speech-to-Text → Runs on), and the AI
// cleanup step offers a cloud provider first — quicker here — with what
// each adds to a dictation, the private model still one click away.
// Its neighbour local-models-no-gpu.spec.js covers Language Models.
import { test, expect, openSettings, closeSettings } from './support/fixtures.js';

test.use({
  yapOptions: {
    name: 'onboarding-weak-pc',
    env: { YAP_E2E_GPU: 'none', YAP_E2E_CPU_THREADS: '4', YAP_E2E_RAM_MIB: '8192' },
  },
});

test('a 4-thread PC without a graphics card: speech on the processor, cloud cleanup offered first with both times', async ({ yap, main, shot }) => {
  const advice = await yap.invoke('hardware_advice');
  expect(advice).toMatchObject({ tier: 'small', speechModel: 'parakeet-tdt-0.6b-v3', speechDevice: 'cpu', cleanup: 'cloud', localModel: 'qwen2.5-1.5b' });
  expect(advice.localMs).toBeGreaterThan(advice.cloudMs);

  const onboarding = yap.onboarding;
  const dialog = await openSettings(main, 'About');
  await dialog.getByRole('button', { name: /Show setup guide again/ }).click();
  await expect(onboarding.getByRole('heading', { level: 1, name: 'Welcome to Yap' })).toBeVisible();
  await expect(onboarding.locator('.sub').first()).toContainText('on its processor: no graphics card needed');
  await shot(onboarding, '01-model-step-processor');

  await onboarding.getByRole('button', { name: "I'll choose later" }).click();
  await onboarding.getByRole('button', { name: 'Continue →' }).click();
  await expect(onboarding.getByRole('heading', { level: 1, name: 'Make it sound polished' })).toBeVisible();
  // The cloud card is picked, and says why; the private one isn't "Recommended".
  const cloud = onboarding.locator('.mode-card').filter({ hasText: 'Cloud' });
  const local = onboarding.locator('.mode-card').filter({ hasText: 'Private' });
  await expect(cloud).toHaveClass(/\bsel\b/);
  await expect(cloud.locator('.mode-badge')).toHaveText('Quicker on this PC');
  await expect(local.locator('.mode-badge')).toHaveCount(0);
  const times = onboarding.getByTestId('cleanup-times');
  await expect(times).toHaveAttribute('data-pick', 'cloud');
  await expect(times).toContainText(`private cleanup adds about ${(advice.localMs / 1000).toFixed(1)} s`);
  await expect(times).toContainText(`a cloud provider about ${(advice.cloudMs / 1000).toFixed(1)} s`);
  await shot(onboarding, '02-cleanup-step-cloud-first');
  // The private model is one click away, the light one recommended.
  await local.click();
  await expect(onboarding.locator('.llm-row[data-model="qwen2.5-1.5b"] .llm-badge')).toHaveText('Recommended for this PC');
  await shot(onboarding, '03-cleanup-step-private');
  await yap.invoke('close_onboarding');
  await closeSettings(main);
});

test('before this PC has timed it, the speech model runs on the processor', async ({ yap, main, shot }) => {
  const status = await yap.invoke('stt_speed_status');
  expect(status.hasGpu).toBe(false);
  expect(status.measured).toBeNull();
  expect(status.device).toBe('cpu');
  const dialog = await openSettings(main, 'Speech-to-Text');
  const row = dialog.locator('[data-setting="runs-on"]');
  await row.scrollIntoViewIfNeeded();
  await expect(row).toContainText(/Runs on: processor/);
  await shot(main, '04-runs-on-processor');
  await closeSettings(main);
});
