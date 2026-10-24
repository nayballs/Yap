// The on-device model picked for this PC (src-tauri/src/ai/gpu.rs,
// ai/local_llm.rs): with a graphics card that can run it, Qwen3.5 4B is
// "Recommended for this PC" in Settings → Language Models and in the
// onboarding's AI cleanup step (picked there by default), and someone on the
// 1.5B is offered it once, quietly (never downloaded by itself; "No thanks"
// is remembered). Without one, the 1.5B is recommended and nothing is
// offered. A big model whose layers stayed on the processor says so, and
// points at the 1.5B.
//
// The card is a stand-in (`YAP_E2E_GPU`), the runtime and the default model
// are empty placeholder files (so "installed"; nothing ever runs or
// downloads), and the running sidecar's placement is staged with
// `e2e_local_llm_serving`.
import fs from 'node:fs';
import path from 'node:path';
import { test, expect, openSettings, closeSettings, expectStore } from './support/fixtures.js';

const CARD = 'NVIDIA GeForce RTX 5070 Ti';

/** Placeholder runtime + default model in <data>/llm (installed, never run). */
function placeholders(dataDir) {
  const llm = path.join(dataDir, 'llm');
  fs.mkdirSync(llm, { recursive: true });
  fs.writeFileSync(path.join(llm, 'llamafile.exe'), '');
  fs.writeFileSync(path.join(llm, 'qwen2.5-1.5b-instruct-q4_k_m.gguf'), '');
}

const onDevice = { postProcessEnabled: true, ppProvider: 'ondevice' };

const browser = (dialog) => dialog.locator('.localbrowser');
const card = (dialog, name) => browser(dialog).locator('.lb-card').filter({ hasText: name });

/** Settings → Language Models, coming from another section (it reads the
 * on-device model's state as it opens). */
async function openLocalModels(main) {
  await openSettings(main, 'About');
  const dialog = await openSettings(main, 'Language Models');
  await expect(browser(dialog)).toBeVisible();
  return dialog;
}

test.use({
  yapOptions: { name: 'local-gpu', env: { YAP_E2E_GPU: `${CARD},15995` }, config: onDevice, prepare: placeholders },
});

test('Settings recommends the 4B for this PC and offers it once', async ({ yap, main, shot }) => {
  const status = await yap.invoke('local_llm_status');
  expect(status.gpuCapable).toBe(true);
  expect(status.recommendedId).toBe('qwen3.5-4b');
  expect(status.gpu.name).toBe(CARD);
  const big = status.curated.find((m) => m.id === 'qwen3.5-4b');
  expect(big).toMatchObject({ recommended: true, needsGpu: true, isDefault: false, tools: true, installed: false });
  // The 1.5B stays the default an empty pick means, without the tools.
  expect(status.curated.find((m) => m.id === 'qwen2.5-1.5b')).toMatchObject({ recommended: false, isDefault: true, tools: false });

  const dialog = await openLocalModels(main);
  await expect(card(dialog, 'Qwen3.5 4B').locator('.lb-badge')).toHaveText('Recommended for this PC');
  await expect(card(dialog, 'Qwen2.5 1.5B').locator('.lb-badge')).toHaveCount(0);

  // On the 1.5B: the bigger model offered in a quiet line, never fetched by itself.
  const offer = dialog.getByTestId('local-upgrade');
  await expect(offer).toContainText(`Your ${CARD} can run Qwen3.5 4B`);
  await expect(offer.getByRole('button', { name: 'Download Qwen3.5 4B' })).toBeEnabled();
  await shot(main, '01-recommended-and-offer');
  expect(fs.existsSync(path.join(yap.dataDir, 'llm', 'Qwen_Qwen3.5-4B-Q4_K_M.gguf'))).toBe(false);

  // "No thanks": gone, and remembered.
  await offer.getByRole('button', { name: 'No thanks' }).click();
  await expect(offer).toBeHidden();
  await expectStore(yap, 'config.json', (c) => c.localLlmUpgradeDismissed === true);
  expect((await yap.invoke('local_llm_status')).upgrade).toBeNull();
  await closeSettings(main);
});

test('a big model left on the processor says so and points at the 1.5B', async ({ yap, main, shot }) => {
  await yap.invoke('e2e_local_llm_serving', { file: 'Qwen_Qwen3.5-4B-Q4_K_M.gguf', gpuLayers: 0, layers: 34 });
  try {
    const dialog = await openLocalModels(main);
    const slow = dialog.getByTestId('local-slow');
    await expect(slow).toContainText('Runs on the processor on this PC: answers will be slow — Qwen2.5 1.5B Instruct is quicker.', {
      timeout: 10_000,
    });
    await expect(slow.getByRole('button', { name: 'Use Qwen2.5 1.5B Instruct' })).toBeVisible();
    await shot(main, '02-runs-on-the-processor');

    // Part of it on the card: said with the layers.
    await yap.invoke('e2e_local_llm_serving', { file: 'Qwen_Qwen3.5-4B-Q4_K_M.gguf', gpuLayers: 12, layers: 34 });
    await closeSettings(main);
    const again = await openLocalModels(main);
    await expect(again.getByTestId('local-slow')).toContainText('Runs partly on the processor on this PC (12 of 34 layers on the graphics card)', {
      timeout: 10_000,
    });

    // All of it on the card: nothing to say.
    await yap.invoke('e2e_local_llm_serving', { file: 'Qwen_Qwen3.5-4B-Q4_K_M.gguf', gpuLayers: 34, layers: 34 });
    const status = await yap.invoke('local_llm_status');
    expect(status.placement).toMatchObject({ kind: 'gpu', gpuLayers: 34, layers: 34 });
    expect(status.slowOnThisPc).toBe(false);
  } finally {
    await yap.invoke('e2e_local_llm_serving', {});
    await closeSettings(main);
  }
});

test('onboarding picks the 4B for this PC', async ({ yap, main, shot }) => {
  const onboarding = yap.onboarding;
  const dialog = await openSettings(main, 'About');
  await dialog.getByRole('button', { name: /Show setup guide again/ }).click();
  await expect(onboarding.getByRole('heading', { level: 1, name: 'Welcome to Yap' })).toBeVisible();
  await onboarding.getByRole('button', { name: "I'll choose later" }).click();
  await onboarding.getByRole('button', { name: 'Continue →' }).click();
  // Cleanup is on already (this instance's config): the picker behind it.
  await onboarding.getByRole('button', { name: /Choose a different model or provider/ }).click();
  const row = onboarding.locator('.llm-row[data-model="qwen3.5-4b"]');
  await expect(row.locator('.llm-badge')).toHaveText('Recommended for this PC');
  await expect(row.locator('input[type="radio"]')).toBeChecked();
  // First in the list, and the reason under it.
  await expect(onboarding.locator('.llm-row').first()).toHaveAttribute('data-model', 'qwen3.5-4b');
  await expect(onboarding.locator('.llm-why')).toContainText(`Your ${CARD} runs Qwen3.5 4B quickly`);
  await expect(onboarding.getByRole('button', { name: /^Download Qwen3\.5 4B \(2\.\d GB\) & enable$/ })).toBeVisible();
  await shot(onboarding, '03-onboarding-recommends-4b');
  await yap.invoke('close_onboarding');
  await closeSettings(main);
});
