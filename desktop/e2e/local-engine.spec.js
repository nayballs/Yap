// The on-device model's engine kept up to date (src-tauri/src/ai/local_llm/
// engine.rs, engine/updater.rs; 2026-11-01): Settings → Language Models shows
// "Engine: llama.cpp b11960"; Check now reads the releases, takes the newest
// build at least 3 days old (never a newer one), downloads it (its digest
// checked), tries it against the engine running now and switches to it —
// "Updated to b11980 · faster on this PC", the build before kept for Go
// back; going back refuses the build left; a build that fails its trial is
// refused and Settings says why; "Keep the engine up to date" is kept in the
// engine's own marker, never config.json.
//
// The releases are a stand-in on 127.0.0.1 (support/engine-feed.js), the
// builds a few bytes that never run, and the trials scripted
// (YAP_E2E_ENGINE_TRIAL): a test run never starts a server, and never looks
// by itself (no YAP_E2E_ENGINE_CHECK_MS).
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openSettings, closeSettings } from './support/fixtures.js';
import { startEngineFeed } from './support/engine-feed.js';

const DEFAULT_GGUF = 'qwen2.5-1.5b-instruct-q4_k_m.gguf';

const test = base.extend({
  feed: [
    async ({}, use) => {
      // b11980 is 4 days old; b11990 (1 day) is too new to be taken.
      const feed = await startEngineFeed([
        { build: 11980, daysAgo: 4 },
        { build: 11990, daysAgo: 1 },
      ]);
      await use(feed);
      await feed.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({ feed }, use) => {
      await use({
        name: 'local-engine',
        env: {
          ...feed.env,
          // No card: the processor's build.
          YAP_E2E_GPU: 'none',
          YAP_E2E_ENGINE_TRIAL: JSON.stringify({ current: { tps: 100 }, b11980: { tps: 112 }, b11985: { fail: 'tool' } }),
        },
        config: { postProcessEnabled: true, ppProvider: 'ondevice' },
        prepare: (dataDir) => {
          // Yap on llama.cpp b11960 (the processor's build), the default model.
          const llm = path.join(dataDir, 'llm');
          const build = path.join(llm, 'engine', 'b11960-cpu');
          fs.mkdirSync(build, { recursive: true });
          fs.writeFileSync(path.join(build, 'llama-server.exe'), 'stand-in b11960 (never run)');
          fs.writeFileSync(path.join(llm, DEFAULT_GGUF), '');
          const current = { build: 11960, variant: 'cpu', sha256: '0'.repeat(64) };
          fs.writeFileSync(path.join(llm, 'engine', 'engine.json'), JSON.stringify({ current }, null, 2));
        },
      });
    },
    { scope: 'worker' },
  ],
});

const marker = (yap) => JSON.parse(fs.readFileSync(path.join(yap.dataDir, 'llm', 'engine', 'engine.json'), 'utf8'));

/** Settings → Language Models, the engine's box. */
async function openEngine(main) {
  await openSettings(main, 'About');
  const dialog = await openSettings(main, 'Language Models');
  const box = dialog.getByTestId('local-engine');
  await box.scrollIntoViewIfNeeded();
  await expect(box).toBeVisible();
  return box;
}

test('Check now takes the newest build three days old after its trial, and Go back refuses it', async ({ yap, main, feed, shot }) => {
  const box = await openEngine(main);
  const line = box.getByTestId('local-engine-line');
  await expect(line).toHaveText(/Engine: llama\.cpp b11960/);
  await expect(box.getByRole('button', { name: 'Keep the engine up to date' })).toHaveAttribute('aria-pressed', 'true');
  expect(feed.requests).toEqual([]);
  await shot(main, '01-engine-before');

  await box.getByRole('button', { name: 'Check now' }).click();
  await expect(line).toHaveText(/Engine: llama\.cpp b11980.*up to date/, { timeout: 30_000 });
  await expect(box.getByTestId('local-engine-event')).toHaveText('Updated to b11980 · faster on this PC');
  await shot(main, '02-engine-updated');

  // The 3-day-old build's processor zip, never the newer one nor another kind.
  expect(feed.downloads()).toEqual(['/download/b11980/llama-b11980-bin-win-cpu-x64.zip']);
  const m = marker(yap);
  expect(m.current).toMatchObject({ build: 11980, variant: 'cpu' });
  expect(m.previous).toMatchObject({ build: 11960 });
  expect(m.unproven).toBe(true);
  expect(m.latest).toBe(11980);
  // Unpacked: the server and its DLL, not the other tools; the zip gone.
  const dir = path.join(yap.dataDir, 'llm', 'engine', 'b11980-cpu');
  expect(fs.readFileSync(path.join(dir, 'llama-server.exe'), 'utf8')).toContain('b11980');
  expect(fs.existsSync(path.join(dir, 'ggml.dll'))).toBe(true);
  expect(fs.existsSync(path.join(dir, 'llama-cli.exe'))).toBe(false);
  expect(fs.readdirSync(path.join(yap.dataDir, 'llm', 'engine', 'downloads'))).toEqual([]);
  // The setting and the state live in the engine's marker, never config.json.
  expect(fs.readFileSync(path.join(yap.dataDir, 'config.json'), 'utf8')).not.toMatch(/b119[68]0/);
  // Network connections name both requests.
  const receipts = JSON.stringify(await yap.invoke('connections_summary'));
  expect(receipts).toContain('On-device AI · engine update check');
  expect(receipts).toContain('On-device AI engine download (llama.cpp)');

  // Go back: b11960 again, b11980 never taken again.
  await box.getByRole('button', { name: 'Go back to b11960' }).click();
  await expect(line).toHaveText(/Engine: llama\.cpp b11960/);
  await expect(box.getByTestId('local-engine-event')).toHaveText("Went back to b11960 · b11980 won't be installed again");
  expect(marker(yap).refused).toContain('b11980-cpu');
  await box.getByRole('button', { name: 'Check now' }).click();
  await expect(line).toHaveText(/Engine: llama\.cpp b11960.*up to date/, { timeout: 30_000 });
  expect(feed.downloads()).toHaveLength(1);
  await shot(main, '03-engine-gone-back');
  await closeSettings(main);
});

test('a build that fails its trial is refused and Settings says why; the switch is kept in the marker', async ({ yap, main, feed, shot }) => {
  // A newer build, 3.5 days old, whose tool calls fail on this PC.
  feed.add({ build: 11985, daysAgo: 3.5 });
  const box = await openEngine(main);
  const line = box.getByTestId('local-engine-line');
  await box.getByRole('button', { name: 'Check now' }).click();
  await expect(box.getByTestId('local-engine-event')).toHaveText("b11985 couldn't use tools here: staying on b11960", { timeout: 30_000 });
  // A refused build isn't "available": what runs is the newest Yap takes.
  await expect(line).toHaveText(/Engine: llama\.cpp b11960 \(processor\) · up to date/);
  expect(marker(yap).refused).toContain('b11985-cpu');
  expect(marker(yap).current).toMatchObject({ build: 11960 });
  // Its folder doesn't stay.
  expect(fs.existsSync(path.join(yap.dataDir, 'llm', 'engine', 'b11985-cpu'))).toBe(false);
  await shot(main, '04-engine-refused');

  // "Keep the engine up to date" off: in the marker.
  const toggle = box.getByRole('button', { name: 'Keep the engine up to date' });
  await toggle.click();
  await expect(toggle).toHaveAttribute('aria-pressed', 'false');
  await expect.poll(() => marker(yap).paused).toBe(true);
  await toggle.click();
  await expect.poll(() => marker(yap).paused).toBe(false);
  await closeSettings(main);
});
