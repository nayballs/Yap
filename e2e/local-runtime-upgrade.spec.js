// An older llamafile engine on disk is replaced in the background
// (src-tauri/src/local_llm.rs `spawn_runtime_upgrade`): installs from before
// 22 Oct 2026 have 0.10.3 with no marker beside it, and the install path only
// ever downloaded a missing engine. While the on-device model is in use, the
// pinned engine is downloaded next to the old one (verified), put in place
// once nothing uses the model, and the old one is kept as
// llamafile-previous.exe until the new one has started once — the way back
// if it won't.
//
// The engines here are stand-ins: the "old" one a few bytes, the new one
// random bytes served from 127.0.0.1 (YAP_LLM_SOURCES); a test run never runs
// either, and only upgrades when YAP_E2E_RUNTIME_UPGRADE_MS says when to look.
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect } from './support/fixtures.js';
import { startFakeLocalModels } from './support/local-llm.js';
import { RUNS } from './support/yap.js';

const OLD_ENGINE = 'the old llamafile 0.10.3 (a stand-in)';

const test = base.extend({
  localModels: [
    async ({}, use) => {
      const fake = await startFakeLocalModels(path.join(RUNS, 'llm-sources-upgrade'), ['qwen2.5-1.5b']);
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({ localModels }, use) => {
      await use({
        name: 'local-runtime-upgrade',
        env: { ...localModels.env, YAP_E2E_RUNTIME_UPGRADE_MS: '1500' },
        config: { postProcessEnabled: true, ppProvider: 'ondevice' },
        prepare: (dataDir) => {
          const llm = path.join(dataDir, 'llm');
          fs.mkdirSync(llm, { recursive: true });
          // An engine with no marker: what every install had before markers.
          fs.writeFileSync(path.join(llm, 'llamafile.exe'), OLD_ENGINE);
          fs.writeFileSync(path.join(llm, 'qwen2.5-1.5b-instruct-q4_k_m.gguf'), '');
        },
      });
    },
    { scope: 'worker' },
  ],
});

const sha256 = (file) => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');

test('an old engine is replaced in the background, and kept until the new one has started', async ({ yap, localModels }) => {
  const llm = path.join(yap.dataDir, 'llm');
  const status = () => yap.invoke('local_llm_status');
  expect((await status()).runtime.pinned).toBe('0.10.6');

  // Downloaded and put in place by itself, nobody asking.
  await expect.poll(async () => (await status()).runtime.version, { timeout: 30_000 }).toBe('0.10.6');
  expect(localModels.requests).toContain('/llm/runtime');
  expect(sha256(path.join(llm, 'llamafile.exe'))).toBe(localModels.sources.runtime.sha256);
  // The marker says which engine it is, so no start hashes 368 MB again.
  const marker = JSON.parse(fs.readFileSync(path.join(llm, 'llamafile.json'), 'utf8'));
  expect(marker).toMatchObject({ version: '0.10.6', sha256: localModels.sources.runtime.sha256, unproven: true, refused: [] });
  // The old engine stays until the new one has started once (the way back).
  expect(fs.readFileSync(path.join(llm, 'llamafile-previous.exe'), 'utf8')).toBe(OLD_ENGINE);
  // Nothing half-done left behind, and it doesn't download it twice.
  expect(fs.existsSync(path.join(llm, 'llamafile-next.exe'))).toBe(false);
  expect(fs.existsSync(path.join(llm, 'llamafile-next.partial'))).toBe(false);
  await expect.poll(async () => (await status()).runtime.upgrade).toBe('');
  expect(localModels.requests.filter((r) => r === '/llm/runtime')).toHaveLength(1);
  // Still installed, still the same model.
  expect((await status()).installed).toBe(true);
});
