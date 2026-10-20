// The on-device model's context (src/lib/LocalContext.svelte,
// localContext.js; src-tauri/src/local_llm.rs, gguf.rs): how much text the
// one on-device model reads at once, for Chat, dictation cleanup and the
// meeting notes alike.
// - the Chat model picker's On this PC pane: "Context" with the choices up
//   to what the model was trained for (its GGUF header: "up to 256K" on its
//   row), each with what it takes on the graphics card, a warning on one
//   that won't fit, and the one-line note;
// - a change saves `localContext` (Settings' copy keeps it) and restarts
//   the running model — never while a dictation is going on;
// - Settings → Language Models → Local: the same control; the model chip's
//   tooltip names the context.
//
// The models are stand-in GGUF headers with Qwen3.5-4B's and Qwen2.5-1.5B's
// real shapes and no weights (support/gguf.js), the llamafile engine an
// empty placeholder that never runs, the running model a stand-in
// (`e2e_local_llm_serving`, which a context change "restarts":
// `e2e_local_llm_restarts`), a dictation the restart guard's stand-in
// (`e2e_local_llm_busy`), and the graphics card a stand-in 8 GB card
// (YAP_E2E_GPU): ~5.8 GB of room beside Windows and speech recognition.
import fs from 'node:fs';
import path from 'node:path';
import { test, expect, openView, openSettings, closeSettings, expectStore } from './support/fixtures.js';
import { qwen35_4b, qwen25_1_5b, writeModel } from './support/gguf.js';

const CARD = 'NVIDIA GeForce RTX 4060';
const BIG_FILE = 'Qwen_Qwen3.5-4B-Q4_K_M.gguf';

function models(dataDir) {
  const llm = path.join(dataDir, 'llm');
  fs.mkdirSync(llm, { recursive: true });
  fs.writeFileSync(path.join(llm, 'llamafile.exe'), '');
  writeModel(path.join(llm, BIG_FILE), qwen35_4b());
  writeModel(path.join(llm, 'qwen2.5-1.5b-instruct-q4_k_m.gguf'), qwen25_1_5b());
}

const ON_DEVICE_CHAT = { enabled: true, provider: 'ondevice', baseUrl: '', model: '', apiKey: '', apiKeys: {}, prompt: '', disableThinking: false };

test.use({
  yapOptions: {
    name: 'local-context',
    env: { YAP_E2E_GPU: `${CARD},7957` },
    config: {
      postProcessEnabled: true,
      ppProvider: 'ondevice',
      ppLocalModel: BIG_FILE,
      llmScopes: { chat: ON_DEVICE_CHAT },
    },
    prepare: models,
  },
});

const chip = (main) => main.locator('.modelchip .chip');
const picker = (main) => main.getByRole('dialog', { name: 'Switch the Chat model' });
const control = (scope) => scope.getByTestId('local-context');
const select = (scope) => control(scope).getByRole('combobox', { name: 'Context' });

async function openPicker(main) {
  await openView(main, 'Chat');
  await chip(main).click();
  await expect(picker(main)).toBeVisible();
  await expect(picker(main).getByRole('tab', { name: /^On this PC/ })).toHaveAttribute('aria-selected', 'true');
  return picker(main);
}

async function closePicker(main) {
  if (await picker(main).isVisible()) {
    await main.keyboard.press('Escape');
    await expect(picker(main)).toBeHidden();
  }
}

/** Back to 8K, the model "running" on the card at it, no dictation;
 * `usedMib`: what its log says it takes. */
async function fresh(yap, usedMib = null) {
  await yap.invoke('e2e_local_llm_busy', { busy: false });
  await yap.invoke('local_llm_set_context', { tokens: 8192 });
  await yap.invoke('e2e_local_llm_serving', { file: BIG_FILE, gpuLayers: 34, layers: 34, ctx: 8192, usedMib });
}

test.afterAll(async ({ yap }) => {
  await yap.invoke('e2e_local_llm_busy', { busy: false }).catch(() => {});
  await yap.invoke('e2e_local_llm_serving', {}).catch(() => {});
});

test('the On this PC pane shows the context, what it takes, and each model’s maximum', async ({ yap, main, shot }) => {
  await fresh(yap);
  let st = await yap.invoke('local_llm_status');
  // What its log measured: a little under the estimate, as the real 4B's
  // 3,815 MiB against 3,821 (the stand-in has no weights: ~0.9 GB).
  const used = st.context.options.find((o) => o.tokens === 8192).estimateMib - 6;
  await yap.invoke('e2e_local_llm_serving', { file: BIG_FILE, gpuLayers: 34, layers: 34, ctx: 8192, usedMib: used });
  st = await yap.invoke('local_llm_status');
  // The stand-in header says what the model was trained for.
  expect(st.context).toMatchObject({ tokens: 8192, configured: 0, default: 8192, max: 262144, serving: 8192, onCard: true, pending: false, usedMib: used });
  expect(st.context.options.map((o) => o.tokens)).toEqual([4096, 8192, 16384, 32768, 65536, 131072, 262144]);
  // Each choice with what it takes; more context, more memory; the biggest
  // doesn't fit beside Windows and speech recognition on an 8 GB card.
  const est = st.context.options.map((o) => o.estimateMib);
  expect(est.every((m, i) => i === 0 || m > est[i - 1])).toBe(true);
  expect(st.context.options.at(-1).over).toBe(true);
  expect(st.context.options.find((o) => o.tokens === 8192).over).toBe(false);
  expect(st.curated.find((m) => m.id === 'qwen3.5-4b').maxContext).toBe(262144);
  expect(st.curated.find((m) => m.id === 'qwen2.5-1.5b').maxContext).toBe(32768);
  expect(st.curated.find((m) => m.id === 'gemma-2-2b').maxContext).toBe(8192);

  const pop = await openPicker(main);
  await expect(control(pop)).toBeVisible();
  await expect(select(pop)).toHaveValue('8192');
  // What the running model's log measured.
  await expect(control(pop).getByTestId('local-context-memory')).toHaveText(`Uses ${(used / 1024).toFixed(1)} GB on the graphics card`);
  await expect(control(pop)).toContainText(
    'Longer context: longer chats and meeting notes fit, more graphics memory, a little slower. This model goes up to 256K.'
  );
  await expect(select(pop).locator('option')).toHaveText([
    /^4K tokens · ≈ [\d.]+ GB$/,
    /^8K tokens · ≈ [\d.]+ GB$/,
    /^16K tokens · ≈ [\d.]+ GB$/,
    /^32K tokens · ≈ [\d.]+ GB$/,
    /^64K tokens · ≈ [\d.]+ GB$/,
    /^128K tokens · ≈ [\d.]+ GB$/,
    /^256K tokens · ≈ [\d.]+ GB · may not fit$/,
  ]);
  // Each model's row says how far it goes.
  const rows = pop.locator('.pane .mrow');
  await expect(rows.filter({ hasText: 'Qwen3.5 4B' }).first().locator('.ds')).toHaveText(/^Up to 256K · /);
  await expect(rows.filter({ hasText: 'Qwen2.5 1.5B Instruct' }).first().locator('.ds')).toHaveText(/^Up to 32K · /);
  await shot(main, '01-picker-context');

  // 32K: saved, and the running model restarted on it.
  await select(pop).selectOption('32768');
  await expectStore(yap, 'config.json', (c) => c.localContext === 32768);
  await expect.poll(() => yap.invoke('e2e_local_llm_restarts')).toBe(1);
  await expect.poll(async () => (await yap.invoke('local_llm_status')).context.serving).toBe(32768);
  await expect(select(pop)).toHaveValue('32768');
  await expect(control(pop).getByTestId('local-context-pending')).toBeHidden();
  // Not measured at 32K yet: the choice's own estimate says it.
  const at32 = st.context.options.find((o) => o.tokens === 32768).estimateMib;
  await expect(select(pop).locator('option:checked')).toHaveText(`32K tokens · ≈ ${(at32 / 1024).toFixed(1)} GB`);
  await expect(control(pop).getByTestId('local-context-memory')).toHaveCount(0);
  await shot(main, '02-picker-32k');

  // Too big for the card: said, not refused.
  await select(pop).selectOption('262144');
  await expect(control(pop).getByTestId('local-context-over')).toContainText(
    'More than fits on your graphics card (about 5.8 GB to spare): part of it would run on the processor, slowly.'
  );
  await shot(main, '03-picker-too-big');
  await select(pop).selectOption('32768');
  await expect(control(pop).getByTestId('local-context-over')).toBeHidden();
  await closePicker(main);

  // The chip's tooltip names it.
  await expect(chip(main)).toHaveAttribute('title', /Context: 32K tokens/);

  // Settings' copy kept it: its next (wholesale) save doesn't put 8K back.
  const cfg = await yap.invoke('get_config');
  await main.evaluate((p) => window.dispatchEvent(new CustomEvent('yap-config-patched', { detail: p })), {
    appendTrailingSpace: !cfg.appendTrailingSpace,
  });
  await expectStore(yap, 'config.json', (c) => c.appendTrailingSpace === !cfg.appendTrailingSpace);
  expect(yap.readJson('config.json').localContext).toBe(32768);
  await main.evaluate((p) => window.dispatchEvent(new CustomEvent('yap-config-patched', { detail: p })), {
    appendTrailingSpace: cfg.appendTrailingSpace,
  });
  await expectStore(yap, 'config.json', (c) => c.appendTrailingSpace === cfg.appendTrailingSpace);
});

test('a new context waits for the dictation to finish, then the model restarts', async ({ yap, main, shot }) => {
  await fresh(yap);
  const before = await yap.invoke('e2e_local_llm_restarts');
  await yap.invoke('e2e_local_llm_busy', { busy: true });
  const pop = await openPicker(main);
  await select(pop).selectOption('16384');
  // Saved at once; the model keeps its context while the dictation goes on.
  await expectStore(yap, 'config.json', (c) => c.localContext === 16384);
  await expect(control(pop).getByTestId('local-context-pending')).toContainText('Restarting the model with the new context');
  await shot(main, '04-waits-for-dictation');
  await main.waitForTimeout(1_500);
  expect(await yap.invoke('e2e_local_llm_restarts')).toBe(before);
  expect((await yap.invoke('local_llm_status')).context.serving).toBe(8192);
  // The dictation ends: the restart goes ahead (after a moment's settle).
  await yap.invoke('e2e_local_llm_busy', { busy: false });
  await expect.poll(() => yap.invoke('e2e_local_llm_restarts'), { timeout: 10_000 }).toBe(before + 1);
  await expect.poll(async () => (await yap.invoke('local_llm_status')).context.serving).toBe(16384);
  await expect(control(pop).getByTestId('local-context-pending')).toBeHidden();
  await closePicker(main);
});

test('Settings → Language Models → Local has the same control', async ({ yap, main, shot }) => {
  await fresh(yap);
  await openSettings(main, 'About');
  const dialog = await openSettings(main, 'Language Models');
  const browser = dialog.locator('.localbrowser');
  await expect(browser).toBeVisible();
  await expect(select(browser)).toHaveValue('8192');
  await expect(control(browser).getByTestId('local-context-memory')).toHaveText(/^≈ \d\.\d GB on the graphics card$/);
  await expect(control(browser)).toContainText('Shared by Chat, dictation cleanup and meeting notes.');
  // The model cards say their maximum next to their size.
  await expect(browser.locator('.lb-card').filter({ hasText: 'Qwen3.5 4B' }).locator('.lb-size')).toContainText('up to 256K');
  await expect(browser.locator('.lb-card').filter({ hasText: 'Qwen2.5 1.5B' }).locator('.lb-size')).toContainText('up to 32K');
  await control(browser).scrollIntoViewIfNeeded();
  await shot(main, '05-settings-context');
  await select(browser).selectOption('65536');
  await expectStore(yap, 'config.json', (c) => c.localContext === 65536);
  await expect.poll(async () => (await yap.invoke('local_llm_status')).context.serving).toBe(65536);
  // Back to the default: kept as 0 (follows a future default).
  await select(browser).selectOption('8192');
  await expectStore(yap, 'config.json', (c) => c.localContext === 0);
  await closeSettings(main);
});

/** Change config.json as another surface would, and tell Settings' copy. */
async function setConfig(yap, main, patch) {
  const cfg = await yap.invoke('get_config');
  await yap.invoke('save_config', { cfg: { ...cfg, ...patch } });
  await main.evaluate((p) => window.dispatchEvent(new CustomEvent('yap-config-patched', { detail: p })), patch);
  await main.waitForTimeout(600);
}

test('the 1.5B goes up to its own maximum, and a bigger setting is held to it', async ({ yap, main }) => {
  await fresh(yap);
  await yap.invoke('local_llm_set_context', { tokens: 65536 });
  await main.waitForTimeout(600);
  // Switch the model to the 1.5B (its header: trained for 32K).
  await setConfig(yap, main, { ppLocalModel: '' });
  const st = await yap.invoke('local_llm_status');
  expect(st.context).toMatchObject({ configured: 65536, tokens: 32768, max: 32768 });
  expect(st.context.options.map((o) => o.tokens)).toEqual([4096, 8192, 16384, 32768]);
  // Its estimates: no recurrent state, 28 KiB of KV a token.
  const est = Object.fromEntries(st.context.options.map((o) => [o.tokens, o.estimateMib]));
  expect(est[32768] - est[8192]).toBeGreaterThan(650);
  // Back to the 4B and 8K for the rest.
  await setConfig(yap, main, { ppLocalModel: BIG_FILE });
  await yap.invoke('local_llm_set_context', { tokens: 8192 });
  await expectStore(yap, 'config.json', (c) => c.localContext === 0 && c.ppLocalModel === BIG_FILE);
});
