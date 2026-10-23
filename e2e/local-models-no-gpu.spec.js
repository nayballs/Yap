// local-models.spec.js, on a PC without a graphics card for the 4B
// (`YAP_E2E_GPU=none`): the 1.5B is recommended and nothing is offered. Each
// card is its own Yap instance, so its own file.
//
// The on-device model picked for this PC (src-tauri/src/gpu.rs,
// local_llm.rs): with a graphics card that can run it, Qwen3.5 4B is
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

test.use({ yapOptions: { name: 'local-no-gpu', env: { YAP_E2E_GPU: 'none' }, config: onDevice, prepare: placeholders } });

test('the 1.5B is recommended and nothing is offered', async ({ yap, main, shot }) => {
  const status = await yap.invoke('local_llm_status');
  expect(status.gpuCapable).toBe(false);
  expect(status.recommendedId).toBe('qwen2.5-1.5b');
  expect(status.upgrade).toBeNull();
  const dialog = await openLocalModels(main);
  await expect(card(dialog, 'Qwen2.5 1.5B').locator('.lb-badge')).toHaveText('Recommended for this PC');
  await expect(dialog.getByTestId('local-upgrade')).toHaveCount(0);
  // The 1.5B on the processor is never "slow".
  await yap.invoke('e2e_local_llm_serving', { file: 'qwen2.5-1.5b-instruct-q4_k_m.gguf', gpuLayers: 0, layers: 29 });
  try {
    expect((await yap.invoke('local_llm_status')).slowOnThisPc).toBe(false);
  } finally {
    await yap.invoke('e2e_local_llm_serving', {});
  }
  await shot(main, '04-no-gpu-recommends-1.5b');
  await closeSettings(main);
});
