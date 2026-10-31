// Every big download's progress and Cancel (src-tauri/src/shell/download.rs,
// src/lib/core/downloads.js, ui/DownloadProgress.svelte), through the
// on-device models' stand-in downloads (support/local-llm.js: small fakes
// served slowly from 127.0.0.1, held half-way on request; YAP_LLM_SOURCES).
// While one downloads its row says how far, how fast and how long is left;
// Cancel stops it at once, the half-downloaded file goes (nothing an older
// Yap could take for a model), nothing is said as a failure, and the same
// model downloads whole afterwards. The progress events carry the speed,
// the time left and what Cancel cancels.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openSettings, closeSettings, openView, openModelPicker } from './support/fixtures.js';
import { startFakeLocalModels } from './support/local-llm.js';
import { RUNS } from './support/yap.js';

const CARD = 'NVIDIA GeForce RTX 5070 Ti';
const SMALL = 'Qwen_Qwen3.5-2B-Q4_K_M.gguf';
const BIG_FILE = 'Qwen_Qwen3.5-4B-Q4_K_M.gguf';

/** Placeholder runtime + default model in <data>/llm (installed, never run). */
function placeholders(dataDir) {
  const llm = path.join(dataDir, 'llm');
  fs.mkdirSync(llm, { recursive: true });
  fs.writeFileSync(path.join(llm, 'llamafile.exe'), '');
  fs.writeFileSync(path.join(llm, 'qwen2.5-1.5b-instruct-q4_k_m.gguf'), '');
}

const test = base.extend({
  localModels: [
    async ({}, use) => {
      const fake = await startFakeLocalModels(path.join(RUNS, 'download-sources'));
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({ localModels }, use) => {
      await use({
        name: 'downloads',
        env: { YAP_E2E_GPU: `${CARD},15995`, ...localModels.env },
        config: { postProcessEnabled: true, ppProvider: 'ondevice' },
        prepare: placeholders,
      });
    },
    { scope: 'worker' },
  ],
});

const llmDir = (yap) => path.join(yap.dataDir, 'llm');
const partials = (yap) => fs.readdirSync(llmDir(yap)).filter((f) => f.endsWith('.partial'));
const card = (dialog, name) => dialog.locator('.localbrowser .lb-card').filter({ hasText: name });

async function openLocalModels(main) {
  await openSettings(main, 'About');
  const dialog = await openSettings(main, 'Language Models');
  await expect(dialog.locator('.localbrowser')).toBeVisible();
  return dialog;
}

test('Settings: a download shows its speed and time left, and Cancel leaves no half file', async ({ yap, main, localModels, shot }) => {
  const dialog = await openLocalModels(main);
  const row = card(dialog, 'Qwen3.5 2B');
  localModels.hold();
  await row.getByRole('button', { name: 'Download' }).click();

  // How far, how fast, how long: once it's going (held half-way).
  const progress = row.getByTestId('download-progress');
  await expect(progress).toBeVisible();
  await expect(progress).toContainText('Downloading Qwen3.5 2B…');
  const line = row.getByTestId('download-line');
  await expect(line).toHaveText(/^\d+ MB of 4 MB · [\d.]+ (MB|KB)\/s · (about \d+ min left|less than a minute left|almost done)$/, { timeout: 15_000 });
  await expect(progress.getByRole('progressbar')).toHaveAttribute('aria-valuenow', /^(4\d|5\d)$/);
  // The bytes so far, waiting under a temporary name.
  await expect.poll(() => partials(yap)).toEqual(['Qwen_Qwen3.5-2B-Q4_K_M.partial']);
  await shot(main, '01-settings-download-progress');

  // Cancel: at once, no error, and no half file.
  await progress.getByRole('button', { name: /^Cancel the download/ }).click();
  await expect(progress).toBeHidden({ timeout: 10_000 });
  await expect(row.getByRole('button', { name: 'Download' })).toBeEnabled();
  await expect(dialog.getByRole('alert')).toHaveCount(0);
  await expect.poll(() => partials(yap)).toEqual([]);
  expect(fs.existsSync(path.join(llmDir(yap), SMALL))).toBe(false);
  // Nothing left to cancel.
  expect(await yap.invoke('download_cancel', { id: 'llm:qwen3.5-2b' })).toBe(false);
  localModels.release();
  await shot(main, '02-settings-download-cancelled');

  // And it downloads whole after all.
  await row.getByRole('button', { name: 'Download' }).click();
  // (A model downloaded from Settings is the one in use.)
  await expect(row).toContainText('✓ Active', { timeout: 20_000 });
  expect(fs.existsSync(path.join(llmDir(yap), SMALL))).toBe(true);
  expect(partials(yap)).toEqual([]);
  await closeSettings(main);
});

test('Chat model picker: Cancel in the row, with how fast it was going', async ({ yap, main, localModels, shot }) => {
  const pop = await openModelPicker(main);
  await pop.getByRole('tab', { name: /^On this PC/ }).click();
  const big = pop.locator('.pane').getByRole('group', { name: 'Download' }).locator('.dlrow').filter({ hasText: 'Qwen3.5 4B' });
  localModels.hold();
  await big.getByRole('button', { name: /^Download Qwen3\.5 4B/ }).click();
  await expect(big.locator('.pline')).toContainText(/Downloading… \d+% · \d+ MB of 4 MB · [\d.]+ (MB|KB)\/s/, { timeout: 15_000 });
  await shot(main, '03-picker-download-progress');
  await big.getByRole('button', { name: 'Cancel the download of Qwen3.5 4B' }).click();
  await expect(big.getByRole('button', { name: /^Download Qwen3\.5 4B/ })).toBeEnabled({ timeout: 10_000 });
  await expect(big.locator('.rowerr')).toHaveCount(0);
  await expect.poll(() => partials(yap)).toEqual([]);
  expect(fs.existsSync(path.join(llmDir(yap), BIG_FILE))).toBe(false);
  localModels.release();
  await main.keyboard.press('Escape');
});
