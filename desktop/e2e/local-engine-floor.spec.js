// A fresh install whose engine won't start isn't left dead
// (desktop/src-tauri/src/ai/local_llm/engine.rs `first_start_failed`, engine/updater.rs
// `rescue`; 2026-11-01): the floor's CUDA build (an NVIDIA card) never starts
// and there's nothing to go back to, so it's refused and the floor's next
// builds are downloaded, checked and tried out of sight in turn — Vulkan
// (its trial fails: refused), then the processor's (kept) — each refusal in
// engine.json, so nothing is downloaded again at every launch. When the
// processor's build won't start either, none runs: Settings' engine line and
// Load now say so plainly, and Try again goes through them once more.
//
// The builds are stand-ins that can't run (a few bytes named
// llama-server.exe: a real start fails to launch them, as a build that won't
// start here would), their downloads from 127.0.0.1 (support/local-llm.js,
// YAP_LLM_SOURCES "engine" / "cudart"), the out-of-sight trials scripted
// (YAP_E2E_ENGINE_TRIAL). The card is the suite's NVIDIA stand-in: CUDA →
// Vulkan → the processor's.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openSettings, closeSettings } from './support/fixtures.js';
import { startFakeLocalModels } from './support/local-llm.js';
import { RUNS } from './support/yap.js';

const test = base.extend({
  localModels: [
    async ({}, use) => {
      const fake = await startFakeLocalModels(path.join(RUNS, 'llm-sources-floor'), ['qwen2.5-1.5b']);
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({ localModels }, use) => {
      await use({
        name: 'local-engine-floor',
        env: { ...localModels.env, YAP_E2E_ENGINE_TRIAL: JSON.stringify({ 'b11982-vulkan': { fail: 'start' } }) },
        config: { postProcessEnabled: true, ppProvider: 'ondevice' },
        prepare: (dataDir) => {
          // A fresh install: the floor's CUDA build, not started yet, no llamafile.
          const llm = path.join(dataDir, 'llm');
          const build = path.join(llm, 'engine', 'b11982-cuda12');
          fs.mkdirSync(build, { recursive: true });
          fs.writeFileSync(path.join(build, 'llama-server.exe'), 'a stand-in that cannot run');
          fs.writeFileSync(path.join(llm, 'qwen2.5-1.5b-instruct-q4_k_m.gguf'), '');
          const current = { build: 11982, variant: 'cuda12', sha256: '0'.repeat(64) };
          fs.writeFileSync(path.join(llm, 'engine', 'engine.json'), JSON.stringify({ current, unproven: true }, null, 2));
        },
      });
    },
    { scope: 'worker' },
  ],
});

const marker = (yap) => JSON.parse(fs.readFileSync(path.join(yap.dataDir, 'llm', 'engine', 'engine.json'), 'utf8'));

async function openEngine(main) {
  await openSettings(main, 'About');
  const dialog = await openSettings(main, 'Language Models');
  const box = dialog.getByTestId('local-engine');
  await box.scrollIntoViewIfNeeded();
  await expect(box).toBeVisible();
  return { dialog, box };
}

test("the floor's CUDA build won't start: Vulkan is tried, then the processor's build runs", async ({ yap, main, localModels, shot }) => {
  // The model's first start: the CUDA build doesn't launch.
  const err = await yap.invoke('local_llm_start').then(() => '', (e) => String(e));
  expect(err).toContain('setting up another build');
  await expect.poll(() => marker(yap).current?.variant ?? null, { timeout: 30_000 }).toBe('cpu');
  const m = marker(yap);
  expect(m.refused).toEqual(expect.arrayContaining(['b11982-cuda12', 'b11982-vulkan']));
  expect(m.refused).not.toContain('b11982-cpu');
  expect(m.unproven).toBe(true);
  expect(m.event).toMatchObject({ kind: 'fallback', variant: 'cpu', tried: ['cuda12', 'vulkan'] });
  expect(m.pick).toMatchObject({ variant: 'cpu' });
  // Vulkan's and the processor's zips downloaded (checked); the refused one's folder gone.
  expect(localModels.requests.filter((r) => r === '/llm/engine.zip')).toHaveLength(2);
  const engine = path.join(yap.dataDir, 'llm', 'engine');
  expect(fs.existsSync(path.join(engine, 'b11982-cpu', 'llama-server.exe'))).toBe(true);
  expect(fs.existsSync(path.join(engine, 'b11982-vulkan'))).toBe(false);

  const { box } = await openEngine(main);
  await expect(box.getByTestId('local-engine-line')).toHaveText(/Engine: llama\.cpp b11982 \(processor\)/);
  await expect(box.getByTestId('local-engine-event')).toHaveText('CUDA and Vulkan didn\'t start here, so the engine runs on the processor: answers will be slower');
  await shot(main, '01-fell-back-to-the-processor');
  await closeSettings(main);
});

test('none of the builds starts: Settings and Load now say so plainly, and Try again goes through them again', async ({ yap, main, localModels, shot }) => {
  const { dialog, box } = await openEngine(main);
  // Load now: the processor's build doesn't launch either.
  await dialog.getByTestId('local-load').getByRole('button', { name: 'Load now' }).click();
  await expect(dialog.locator('.ondevice-err')).toContainText("The on-device engine won't start on this PC");
  const line = box.getByTestId('local-engine-line');
  await expect(line).toHaveText(/Engine: none runs on this PC/);
  await expect(box.getByTestId('local-engine-event')).toHaveText(
    "None of the engine's builds started on this PC (CUDA, Vulkan and the processor's build tried). Updating the graphics driver may help; then Try again."
  );
  const m = marker(yap);
  expect(m.current ?? null).toBeNull();
  expect(m.refused).toEqual(expect.arrayContaining(['b11982-cuda12', 'b11982-vulkan', 'b11982-cpu']));
  expect(m.event).toMatchObject({ kind: 'noEngine' });
  // Still "installed": a start says why instead of offering a download again.
  expect((await yap.invoke('local_llm_status')).installed).toBe(true);
  const before = localModels.requests.length;
  // Another start downloads nothing: every one of them is refused here.
  const again = await yap.invoke('local_llm_start').then(() => '', (e) => String(e));
  expect(again).toContain("won't start on this PC");
  expect(localModels.requests.length).toBe(before);
  await shot(main, '02-none-runs');

  // Try again: the floor's builds from the first (CUDA, with NVIDIA's runtime).
  await box.getByRole('button', { name: 'Try again' }).click();
  await expect.poll(() => marker(yap).current?.variant ?? null, { timeout: 30_000 }).toBe('cuda12');
  expect(localModels.requests).toEqual(expect.arrayContaining(['/llm/cudart.zip']));
  expect(marker(yap).refused ?? []).not.toContain('b11982-cuda12');
  await expect(line).toHaveText(/Engine: llama\.cpp b11982 \(CUDA\)/);
  await expect(box.getByTestId('local-engine-event')).toHaveText('The engine runs on CUDA here');
  await shot(main, '03-tried-again');
  await closeSettings(main);
});
