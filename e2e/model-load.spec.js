// Yap loads its speech model in the background at startup (pipeline.rs
// `Pipeline::start`, off the startup thread: the tray, the bar and the
// window come first). A dictation started before it's ready waits for it —
// the bar says "Loading the speech model…" — and goes through, never fails.
// YAP_E2E_MODEL_LOAD_MS slows the (stub) load down; the dictation plays a
// tone in the mic's place (`e2e_dictate_audio`), so no microphone is needed.
import fs from 'node:fs';
import { test, expect, expectStore } from './support/fixtures.js';

const capsule = (yap) => yap.overlay.locator('.capsule');

test.use({ yapOptions: { name: 'model-load', env: { YAP_E2E_MODEL_LOAD_MS: '20000' } } });

test('waits for it, saying so on the bar, and goes through', async ({ yap, shot }) => {
  // The window and the bar are up while the model still loads.
  expect(fs.readFileSync(yap.logPath, 'utf8')).not.toContain('Speech model loaded in the background');
  const dictation = yap.invoke('e2e_dictate_audio', { seconds: 1 });
  await expect(capsule(yap)).toContainText('Loading the speech model…', { timeout: 10_000 });
  await shot(yap.overlay, '01-loading-the-model');
  const delivered = await dictation;
  expect(delivered?.raw).toContain('STT stub');
  expect(fs.readFileSync(yap.logPath, 'utf8')).toContain('Speech model loaded in the background');
  await expectStore(yap, 'history.json', (h) => JSON.stringify(h).includes('STT stub'));
});
