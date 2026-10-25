// Yap's on-device model loaded when it's needed, its memory freed when it's
// idle (src-tauri/src/ai/local_llm/on_demand.rs, 2026-10-25):
// - launched with on-device cleanup, the model isn't loaded; Settings →
//   Language Models says "Not loaded · loads when needed", with "Load the
//   on-device model when Yap starts" off and "Free its memory after it's
//   unused for" 15 minutes;
// - a dictation starts loading it as it starts recording, and its cleanup
//   waits for it ("Loading the AI model…" on the bar, with Skip cleanup) and
//   uses it;
// - unused for the set time (shortened here), it's freed — never while a
//   dictation is going on — and the next dictation loads it again;
// - "Load when Yap starts" on: it's loaded at launch (and turning it on
//   loads it); "Load now" and "Free memory" in Settings;
// - cloud models only: never loaded, the setting on or not.
//
// The model's files are empty stand-ins (support/local-llm.js), and the
// started model is a fake AI on 127.0.0.1 (YAP_E2E_LOCAL_LLM, "loading" for
// YAP_E2E_LOCAL_LLM_LOAD_MS): no llamafile ever runs.
import fs from 'node:fs';
import { test as base, expect, openSettings, closeSettings, expectStore, setConfig } from './support/fixtures.js';
import { startFakeLlm, cleanupTranscript } from './support/fake-llm.js';
import { installStandInModel } from './support/local-llm.js';
import { launchYap } from './support/yap.js';

const LOAD_MS = 1500;
const DEFAULT_ID = 'qwen2.5-1.5b-instruct-q4_k_m';

const test = base.extend({
  fake: [
    async ({}, use) => {
      const fake = await startFakeLlm();
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({ fake }, use) => {
      await use({
        name: 'local-on-demand',
        env: { YAP_E2E_GPU: 'none', YAP_E2E_LOCAL_LLM: fake.base, YAP_E2E_LOCAL_LLM_LOAD_MS: String(LOAD_MS) },
        config: { postProcessEnabled: true, ppProvider: 'ondevice' },
        prepare: (dir) => installStandInModel(dir),
      });
    },
    { scope: 'worker' },
  ],
});

const loads = (yap) => yap.invoke('e2e_local_llm_loads');
const status = (yap) => yap.invoke('local_llm_status');
const cleanups = (fake) => fake.requests.filter((r) => r.kind === 'cleanup');

async function dictate(yap, text) {
  await yap.invoke('e2e_style_app', { exe: 'notepad.exe', title: 'notes.txt - Notepad' });
  try {
    return await yap.invoke('e2e_dictation', { text });
  } finally {
    await yap.invoke('e2e_style_app', { exe: null, title: null });
  }
}

/** Settings → Language Models (coming from another page, so it reads the model's state as it opens). */
async function openLocal(main) {
  await openSettings(main, 'About');
  const dialog = await openSettings(main, 'Language Models');
  const box = dialog.getByTestId('local-load');
  await expect(box).toBeVisible();
  return { dialog, box };
}

test('launched with on-device cleanup, the model is not loaded; Settings says it loads when needed', async ({ yap, main, shot }) => {
  // Longer than a load takes: nothing started it.
  await main.waitForTimeout(LOAD_MS + 1000);
  expect(await loads(yap)).toMatchObject({ loads: [], starts: 0, running: false });
  expect(await status(yap)).toMatchObject({ installed: true, running: false, loading: false });

  const { box } = await openLocal(main);
  await expect(box.getByTestId('local-state')).toHaveText(/Not loaded · loads when needed/);
  await expect(box.getByRole('button', { name: 'Load the on-device model when Yap starts' })).toHaveAttribute('aria-pressed', 'false');
  await expect(box).toContainText('Off: it loads the first time you use it');
  await expect(box.getByRole('combobox', { name: "Free its memory after it's unused for" })).toHaveValue(/15/);
  await box.scrollIntoViewIfNeeded();
  await shot(main, '01-not-loaded');
  await closeSettings(main);
});

test('a dictation starts loading it as it starts recording; its cleanup waits for it and uses it', async ({ yap, fake, shot }) => {
  fake.reply('cleanup', () => 'Cleaned on this PC.');
  try {
    const before = cleanups(fake).length;
    const capsule = yap.overlay.locator('.capsule');
    const t0 = Date.now();
    const run = dictate(yap, 'so um this is a test of the model on this pc');
    // The cleanup waiting for it: the bar says so, with Skip cleanup.
    await expect(capsule).toContainText('Loading the AI model…', { timeout: 5_000 });
    await expect(capsule.getByRole('button', { name: 'Skip cleanup' })).toBeVisible();
    await shot(yap.overlay, '02-bar-loading');
    const r = await run;
    const took = Date.now() - t0;
    // Loaded for the dictation, from its start (not when the cleanup came to it).
    expect(await loads(yap)).toMatchObject({ loads: ['dictation'], starts: 1, running: true });
    // The cleanup waited for it, then went to it.
    expect(took).toBeGreaterThanOrEqual(LOAD_MS - 200);
    expect(cleanups(fake).length).toBe(before + 1);
    expect(cleanups(fake).at(-1).model).toBe(DEFAULT_ID);
    expect(r.text.trim()).toBe('Cleaned on this PC.');

    // Up now: the next dictation goes straight to it, no load.
    const t1 = Date.now();
    await dictate(yap, 'and again');
    expect(Date.now() - t1).toBeLessThan(LOAD_MS);
    expect(await loads(yap)).toMatchObject({ loads: ['dictation'], starts: 1 });
    expect(cleanups(fake).length).toBe(before + 2);
  } finally {
    fake.reply('cleanup', cleanupTranscript);
  }
});

test('unused for the set time it is freed, never during a dictation; the next dictation loads it again', async ({ yap, main, shot }) => {
  expect((await status(yap)).running).toBe(true);
  // A dictation going on: kept, however long.
  await yap.invoke('e2e_local_llm_busy', { busy: true });
  await yap.invoke('e2e_local_llm_idle_ms', { ms: 800 });
  try {
    await main.waitForTimeout(2500);
    expect((await status(yap)).running).toBe(true);
    // Over: freed, and said in the log.
    await yap.invoke('e2e_local_llm_busy', { busy: false });
    await expect.poll(async () => (await status(yap)).running, { timeout: 10_000 }).toBe(false);
    expect(fs.readFileSync(yap.logPath, 'utf8')).toContain('freeing its memory');
  } finally {
    await yap.invoke('e2e_local_llm_busy', { busy: false });
    await yap.invoke('e2e_local_llm_idle_ms', { ms: 0 });
  }

  const { box } = await openLocal(main);
  await expect(box.getByTestId('local-state')).toHaveText(/Not loaded · loads when needed/);
  await closeSettings(main);

  // The next dictation loads it again.
  await dictate(yap, 'back again');
  expect(await loads(yap)).toMatchObject({ loads: ['dictation', 'dictation'], starts: 2, running: true });

  // "Never": kept.
  await setConfig(yap, main, { localModelIdleMins: 0 });
  await yap.invoke('e2e_local_llm_idle_ms', { ms: 300 });
  try {
    await main.waitForTimeout(1500);
    expect((await status(yap)).running).toBe(true);
  } finally {
    await yap.invoke('e2e_local_llm_idle_ms', { ms: 0 });
    await setConfig(yap, main, { localModelIdleMins: 15 });
  }
  await shot(main, '03-after-idle');
});

test('Settings: Free memory, Load now, and "Load when Yap starts" saved (turning it on loads it)', async ({ yap, main, shot }) => {
  const { box } = await openLocal(main);
  const state = box.getByTestId('local-state');
  await expect(state).toHaveText(/Loaded · ready/);
  await box.scrollIntoViewIfNeeded();
  await shot(main, '04-loaded');

  await box.getByRole('button', { name: 'Free memory' }).click();
  await expect(state).toHaveText(/Not loaded · loads when needed/);
  expect((await status(yap)).running).toBe(false);

  // Load now: "Loading…", then loaded.
  await box.getByRole('button', { name: 'Load now' }).click();
  await expect(state).toHaveText(/Loading…|Loaded · ready/);
  await expect(state).toHaveText(/Loaded · ready/, { timeout: 10_000 });
  await box.getByRole('button', { name: 'Free memory' }).click();
  await expect(state).toHaveText(/Not loaded · loads when needed/);

  // On: saved, and it loads now (the model is what cleanup uses).
  const starts = (await loads(yap)).starts;
  await box.getByRole('button', { name: 'Load the on-device model when Yap starts' }).click();
  await expectStore(yap, 'config.json', (c) => c.localModelAtLaunch === true);
  await expect.poll(async () => (await loads(yap)).starts, { timeout: 10_000 }).toBe(starts + 1);
  await expect(state).toHaveText(/Loaded · ready/, { timeout: 10_000 });
  // The idle choice saved.
  await box.getByRole('combobox', { name: "Free its memory after it's unused for" }).selectOption({ label: '1 hour' });
  await expectStore(yap, 'config.json', (c) => c.localModelIdleMins === 60);
  await box.scrollIntoViewIfNeeded();
  await shot(main, '05-load-at-launch-on');

  // Back as it was.
  await box.getByRole('button', { name: 'Load the on-device model when Yap starts' }).click();
  await box.getByRole('combobox', { name: "Free its memory after it's unused for" }).selectOption({ label: '15 minutes' });
  await expectStore(yap, 'config.json', (c) => c.localModelAtLaunch === false && c.localModelIdleMins === 15);
  await closeSettings(main);
});

test('"Load when Yap starts" on: loaded at launch; cloud models only: never loaded, the setting on or not', async ({ fake }) => {
  // On, with on-device cleanup: loaded at launch.
  let other = await launchYap({
    name: 'local-on-demand-launch',
    env: { YAP_E2E_GPU: 'none', YAP_E2E_LOCAL_LLM: fake.base, YAP_E2E_LOCAL_LLM_LOAD_MS: '300' },
    config: { postProcessEnabled: true, ppProvider: 'ondevice', localModelAtLaunch: true },
    prepare: (dir) => installStandInModel(dir),
  });
  try {
    await expect.poll(async () => (await loads(other)).running, { timeout: 15_000 }).toBe(true);
    // At launch, not on demand.
    expect((await loads(other)).loads).toEqual([]);
  } finally {
    await other.stop();
  }

  // A cloud model for cleanup (and everything else), the model here and
  // the setting on: never loaded — not at launch, not by a dictation.
  const cloud = await startFakeLlm();
  other = await launchYap({
    name: 'local-on-demand-cloud',
    env: { YAP_E2E_GPU: 'none', YAP_E2E_LOCAL_LLM: fake.base, YAP_E2E_LOCAL_LLM_LOAD_MS: '300' },
    config: { postProcessEnabled: true, ppProvider: 'custom', ppBaseUrl: cloud.base, ppModel: 'fake-cleanup', ppApiKey: '', localModelAtLaunch: true },
    prepare: (dir) => installStandInModel(dir),
  });
  try {
    await new Promise((r) => setTimeout(r, 1500));
    expect(await loads(other)).toMatchObject({ loads: [], starts: 0, running: false });
    await dictate(other, 'a dictation for the cloud');
    expect(cloud.requests.filter((r) => r.kind === 'cleanup').at(-1)?.model).toBe('fake-cleanup');
    expect(await loads(other)).toMatchObject({ loads: [], starts: 0, running: false });
  } finally {
    await other.stop();
    await cloud.close();
  }
});
