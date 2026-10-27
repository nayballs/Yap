// The Chat model picker, a provider rail (src/lib/views/chat/ChatModelPicker.svelte,
// chatModels.svelte.js, ModelChip.svelte; src-tauri/src/ai/ai_model.rs):
// - the chip names the model (the provider's mark, "· Groq"); the picker
//   opens on its provider, the rail lists Recent, every cloud provider with
//   a dot (connected or not), and On this PC; the rows are the live + curated
//   list Settings shows, with "New";
// - a provider without a key connects right there: the key saved where
//   Settings keeps it (and Settings' copy keeps it), its live list asked at
//   once, its models pickable, the key only ever shown masked;
// - search looks through every provider and this PC;
// - On this PC: Yap's on-device models ("Recommended for this PC" from the
//   graphics card), one downloaded in its row with a progress bar, then
//   picked; an installed one uninstalled from its right-click menu (off
//   while it's in use; back under Download, or gone for a GGUF of your
//   own); the processor warning and its switch to the 1.5B; a self-hosted
//   server's models;
// - the keyboard: arrows through the rail and the models, Ctrl+K, typing to
//   search, Enter, Esc.
//
// Nothing leaves the PC: the providers are the local fake
// (support/fake-llm.js) through YAP_MODELS_STANDIN with test keys, the
// graphics card a stand-in (YAP_E2E_GPU), the downloads small fakes from
// 127.0.0.1 (support/local-llm.js, YAP_LLM_SOURCES), the llamafile engine
// and the bundled model empty placeholder files that never run.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, openSettings, closeSettings, expectStore, setConfig } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { startFakeLocalModels } from './support/local-llm.js';
import { RUNS } from './support/yap.js';

const TEST_KEY = 'e2e-fake-key';
const ANTHROPIC_KEY = 'sk-ant-e2e-fake-1234567890';
const CARD = 'NVIDIA GeForce RTX 5070 Ti';
const BIG_FILE = 'Qwen_Qwen3.5-4B-Q4_K_M.gguf';
const now = () => Math.floor(Date.now() / 1000);

/** A model as Groq's /openai/v1/models lists one (trimmed). */
const groqModel = (id, name, extra = {}) => ({
  id,
  object: 'model',
  created: 1754407957,
  owned_by: 'Fake',
  active: true,
  context_window: 131072,
  name,
  input_modalities: ['text'],
  output_modalities: ['text'],
  supported_features: ['tools', 'json_mode'],
  ...extra,
});
const GROQ_LIST = [
  groqModel('openai/gpt-oss-20b', 'GPT OSS 20B'),
  groqModel('openai/gpt-oss-120b', 'GPT OSS 120B'),
  groqModel('qwen/qwen3.8-27b', 'Qwen/Qwen3.8-27B', { created: 1786984846 }),
  groqModel('acme/brainy-9', 'Brainy 9', { created: now() }),
];
/** As Anthropic's /v1/models lists one. */
const claudeModel = (id, display_name, created_at) => ({ id, type: 'model', display_name, created_at, max_input_tokens: 200000 });
const ANTHROPIC_LIST = [
  claudeModel('claude-haiku-4-5-20251001', 'Claude Haiku 4.5', '2025-10-01T00:00:00Z'),
  claudeModel('claude-sonnet-5-5', 'Claude Sonnet 5.5', '2026-08-01T00:00:00Z'),
  claudeModel('claude-opus-5-5', 'Claude Opus 5.5', '2026-08-01T00:00:00Z'),
  claudeModel('claude-nova-6', 'Claude Nova 6', new Date().toISOString()),
];

/** The placeholder engine + bundled 1.5B in <data>/llm (installed, never run). */
function placeholders(dataDir) {
  const llm = path.join(dataDir, 'llm');
  fs.mkdirSync(llm, { recursive: true });
  fs.writeFileSync(path.join(llm, 'llamafile.exe'), '');
  fs.writeFileSync(path.join(llm, 'qwen2.5-1.5b-instruct-q4_k_m.gguf'), '');
}

const test = base.extend({
  fakeLlm: [
    async ({}, use) => {
      const fake = await startFakeLlm();
      fake.listModels('groq', GROQ_LIST);
      fake.listModels('anthropic', ANTHROPIC_LIST);
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  localModels: [
    async ({}, use) => {
      const fake = await startFakeLocalModels(path.join(RUNS, 'llm-sources'));
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({ fakeLlm, localModels }, use) => {
      await use({
        name: 'chat-model-picker',
        env: { YAP_MODELS_STANDIN: fakeLlm.modelsStandin, YAP_E2E_GPU: `${CARD},15995`, ...localModels.env },
        config: {
          postProcessEnabled: false,
          ppProvider: 'groq',
          ppBaseUrl: fakeLlm.base,
          ppModel: 'openai/gpt-oss-20b',
          ppApiKey: TEST_KEY,
          ppApiKeys: { groq: TEST_KEY },
          llmScopes: { chat: groqChat(fakeLlm, 'openai/gpt-oss-20b') },
        },
        prepare: placeholders,
      });
    },
    { scope: 'worker' },
  ],
});

/** A whole Chat scope, as config.json holds one (config.rs `LlmScope`). */
function groqChat(fakeLlm, model) {
  return { enabled: true, provider: 'groq', baseUrl: fakeLlm.base, model, apiKey: TEST_KEY, apiKeys: {}, prompt: '', disableThinking: false };
}

/** Chat back on the fake Groq's GPT-OSS 20B, the on-device model the default. */
async function backToGroq(yap, main, fakeLlm) {
  const cfg = await yap.invoke('get_config');
  await setConfig(yap, main, { ppLocalModel: '', llmScopes: { ...cfg.llmScopes, chat: groqChat(fakeLlm, 'openai/gpt-oss-20b') } });
}

const chip = (main) => main.locator('.modelchip .chip');
const picker = (main) => main.getByRole('dialog', { name: 'Switch the Chat model' });
const pane = (main) => picker(main).locator('.pane');
const tab = (main, name) => picker(main).getByRole('tab', { name: new RegExp(`^${name}`) });

async function openPicker(main) {
  await openView(main, 'Chat');
  await chip(main).click();
  await expect(picker(main)).toBeVisible();
  return picker(main);
}

test('the chip opens a provider rail on its provider, with the rows Settings shows', async ({ yap, main, shot }) => {
  await openView(main, 'Chat');
  await expect(chip(main)).toHaveText('GPT-OSS 20B · Groq');
  await expect(chip(main).locator('.mark')).toHaveAttribute('data-icon', 'groq');
  const pop = await openPicker(main);

  // The rail: Recent, every cloud provider with its state, On this PC.
  await expect(pop.getByRole('tablist', { name: 'Providers' })).toBeVisible();
  await expect(pop.getByRole('tab')).toHaveText([
    'Recent',
    'Groq, connected',
    'Gemini, not set up',
    'Claude, not set up',
    'OpenAI, not set up',
    'OpenRouter, not set up',
    'On this PC, ready',
  ]);
  await expect(tab(main, 'Groq')).toHaveAttribute('aria-selected', 'true');
  await expect(tab(main, 'Groq').locator('.st')).not.toHaveClass(/off/);
  await expect(tab(main, 'OpenRouter').locator('.st')).toHaveClass(/off/);
  await expect(tab(main, 'OpenRouter').locator('.mark')).toHaveAttribute('data-icon', 'openrouter');

  // Groq's pane: its mark on a tile, the live list's status, the key masked.
  const head = pop.locator('.phead');
  await expect(head).toContainText('Groq');
  await expect(head).toContainText('Connected · checked just now');
  await expect(head.locator('.keymask')).toHaveText('e2e…-key');
  // Curated first, then what Groq added since (New); each with its family's mark.
  const rows = pane(main).locator('.mrow');
  await expect(rows.locator('.nm')).toHaveText(['GPT-OSS 20B', 'GPT-OSS 120B', 'Qwen3.8 27B', 'Brainy 9']);
  await expect(rows.nth(0)).toHaveAttribute('aria-pressed', 'true');
  await expect(rows.nth(1)).toHaveAttribute('aria-pressed', 'false');
  await expect(rows.nth(3).locator('.tag.new')).toHaveText('New');
  await expect(rows.nth(0).locator('.mark')).toHaveAttribute('data-icon', 'openai-oss');
  await expect(rows.nth(2).locator('.mark')).toHaveAttribute('data-icon', 'qwen');
  await expect(rows.nth(3).locator('.mark')).toHaveAttribute('data-icon', 'groq');
  // The context Groq's own list gives (131,072 tokens), as info.
  await expect(rows.nth(0).locator('.ds')).toHaveText(/ · 128K context$/);
  await expect(rows.nth(3).locator('.ds')).toHaveText('128K context');
  await expect(pop).toContainText('Changes the model for Chat only. Dictation cleanup stays as it is.');
  await shot(main, '01-picker-groq');

  // Another provider on the rail: one without a key shows how to connect it.
  await tab(main, 'Gemini').click();
  await expect(tab(main, 'Gemini')).toHaveAttribute('aria-selected', 'true');
  await expect(head).toContainText('Google Gemini');
  await expect(head).toContainText('Not set up');
  await expect(pop.getByLabel('Google Gemini API key')).toBeVisible();
  await expect(pane(main).locator('.mrow.locked').first().locator('.tag.key')).toHaveText('Add key');
  // A greyed model leads to the key field.
  await pane(main).locator('.mrow.locked').first().click();
  await expect(pop.getByLabel('Google Gemini API key')).toBeFocused();

  // Pick another Groq model: the Chat scope only.
  await tab(main, 'Groq').click();
  await pane(main).getByRole('button', { name: /^GPT-OSS 120B/ }).click();
  await expect(pop).toBeHidden();
  await expect(chip(main)).toHaveText('GPT-OSS 120B · Groq');
  await expectStore(yap, 'config.json', (c) => c.llmScopes.chat.model === 'openai/gpt-oss-120b');
  expect(yap.readJson('config.json').ppModel).toBe('openai/gpt-oss-20b');

  // Recent: the last ones used, newest first.
  await chip(main).click();
  await tab(main, 'Recent').click();
  await expect(pane(main).locator('.mrow .nm')).toHaveText(['GPT-OSS 120B', 'GPT-OSS 20B']);
  await expect(pane(main).locator('.mrow').first()).toContainText('Groq · ');
  await main.keyboard.press('Escape');
  await expect(picker(main)).toBeHidden();

  // A click outside closes it too.
  await chip(main).click();
  await expect(picker(main)).toBeVisible();
  await main.getByRole('button', { name: 'New chat' }).click();
  await expect(picker(main)).toBeHidden();
});

test('a provider without a key connects right in the picker', async ({ yap, main, fakeLlm, shot }) => {
  const pop = await openPicker(main);
  await tab(main, 'Claude').click();
  const head = pop.locator('.phead');
  await expect(head).toContainText('Anthropic');
  await expect(head).toContainText('Not set up');
  const box = pop.locator('.connect');
  await expect(box).toContainText('Connect Claude');
  // Its curated models, greyed until there's a key.
  const locked = pane(main).locator('.mrow.locked');
  await expect(locked).toHaveCount(10);
  await expect(locked.first().locator('.tag.key')).toHaveText('Add key');
  await expect(locked.first().locator('.mark')).toHaveAttribute('data-icon', 'claude');
  await shot(main, '02-picker-connect-claude');

  // "Get a key ↗" goes to the provider's key page (a test run records it).
  await box.getByRole('button', { name: 'Get a key ↗' }).click();
  await expect.poll(() => yap.invoke('e2e_opened_urls')).toContain('https://console.anthropic.com/settings/keys');

  // Save it: config.json as Settings saves keys, the list asked at once.
  const before = fakeLlm.modelRequests.length;
  await box.getByLabel('Anthropic API key').fill(ANTHROPIC_KEY);
  await box.getByRole('button', { name: 'Save key' }).click();
  await expect(box).toBeHidden();
  await expectStore(yap, 'config.json', (c) => c.ppApiKeys?.anthropic === ANTHROPIC_KEY);
  await expect.poll(() => fakeLlm.modelRequests.slice(before).some((r) => r.provider === 'anthropic')).toBe(true);
  const asked = fakeLlm.modelRequests.slice(before).find((r) => r.provider === 'anthropic');
  expect(asked.headers['x-api-key']).toBe(ANTHROPIC_KEY);

  // Its live models, pickable; the key masked, never in full.
  await expect(pane(main).locator('.mrow .nm')).toHaveText(['Claude Haiku 4.5', 'Claude Sonnet 5.5', 'Claude Opus 5.5', 'Claude Nova 6']);
  await expect(pane(main).locator('.mrow.locked')).toHaveCount(0);
  await expect(pane(main).locator('.mrow').nth(3).locator('.tag.new')).toHaveText('New');
  await expect(head).toContainText('Connected · checked just now');
  await expect(head.locator('.keymask')).toHaveText('sk-…7890');
  await expect(tab(main, 'Claude')).toHaveText('Claude, connected');
  expect(await pop.innerHTML()).not.toContain(ANTHROPIC_KEY);
  expect(JSON.stringify(yap.readJson('model_catalog.json'))).not.toContain(ANTHROPIC_KEY);

  await pane(main).getByRole('button', { name: /^Claude Sonnet 5\.5/ }).click();
  await expect(chip(main)).toHaveText('Claude Sonnet 5.5 · Anthropic');
  await expect(chip(main).locator('.mark')).toHaveAttribute('data-icon', 'claude');
  await expectStore(yap, 'config.json', (c) => c.llmScopes.chat.provider === 'anthropic' && c.llmScopes.chat.model === 'claude-sonnet-5-5');
  expect(yap.readJson('config.json').llmScopes.chat.apiKey).toBe(ANTHROPIC_KEY);

  // Settings' copy kept the key: a save from Settings (any setting) keeps it.
  const general = await openSettings(main, 'Meetings');
  const live = general.getByRole('button', { name: 'Show live transcript' });
  const was = (await live.getAttribute('aria-pressed')) === 'true';
  await live.click();
  await expectStore(yap, 'config.json', (c) => c.meetingLiveTranscript === !was);
  expect(yap.readJson('config.json').ppApiKeys.anthropic).toBe(ANTHROPIC_KEY);
  expect(yap.readJson('config.json').llmScopes.chat.model).toBe('claude-sonnet-5-5');
  await live.click();
  await expectStore(yap, 'config.json', (c) => c.meetingLiveTranscript === was);
  await closeSettings(main);
  await backToGroq(yap, main, fakeLlm);
});

test('search looks through every provider and this PC', async ({ yap, main, fakeLlm, shot }) => {
  await backToGroq(yap, main, fakeLlm);
  const pop = await openPicker(main);
  const search = pop.getByRole('searchbox', { name: 'Search models and providers' });
  await search.fill('qwen');
  await expect(pane(main).locator('.rhead')).toHaveText('Results for “qwen”');
  const groups = pane(main).getByRole('group');
  await expect(groups.locator('.gh')).toHaveText(['Groq', 'OpenRouter', 'On this PC']);
  await expect(groups.nth(0).locator('.nm')).toHaveText(['Qwen3.8 27B']);
  // A provider without a key still shows what it has, greyed.
  await expect(groups.nth(1).locator('.mrow.locked .nm')).toHaveText(['Qwen3 32B']);
  // This PC: the installed one first, the one recommended for it next.
  await expect(groups.nth(2).locator('.nm')).toHaveText([
    'Qwen2.5 1.5B Instruct',
    'Qwen3.5 4B',
    'Qwen3.5 0.8B',
    'Qwen3.5 2B',
    'Qwen3.5 9B',
  ]);
  // While searching no provider is selected.
  await expect(pop.getByRole('tab', { selected: true })).toHaveCount(0);
  await shot(main, '04-picker-search');

  // By provider and words: Gemini's Flash models (no key: greyed).
  await search.fill('gemini flash');
  await expect(groups.locator('.gh')).toHaveText(['Gemini']);
  await expect(groups.first().locator('.mrow.locked').first()).toContainText('Flash');
  await search.fill('nothing like this');
  await expect(pane(main)).toContainText('No model matches “nothing like this”.');

  // Enter picks the first one that can be picked.
  await search.fill('qwen3.8');
  await search.press('Enter');
  await expect(pop).toBeHidden();
  await expect(chip(main)).toHaveText('Qwen3.8 27B · Groq');
  await expectStore(yap, 'config.json', (c) => c.llmScopes.chat.model === 'qwen/qwen3.8-27b');
  await backToGroq(yap, main, fakeLlm);
});

test('On this PC: the recommended model downloads in its row, then Chat uses it', async ({ yap, main, localModels, shot }) => {
  const pop = await openPicker(main);
  await tab(main, 'On this PC').click();
  const head = pop.locator('.phead');
  await expect(head).toContainText('On this PC');
  await expect(head).toContainText('Private: nothing leaves this PC');
  await expect(head).toContainText(`Runs on your graphics card (${CARD})`);
  // One model runs on this PC: the footer never says "Chat only" here (the
  // pane says it's shared with cleanup when it is).
  await expect(pop.locator('.foot')).not.toContainText('Chat only');

  const installed = pane(main).getByRole('group', { name: 'Installed' });
  await expect(installed.locator('.nm')).toHaveText(['Qwen2.5 1.5B Instruct']);
  await expect(installed.locator('.mrow').first().locator('.tag.warn')).toHaveText('no note tools');
  const available = pane(main).getByRole('group', { name: 'Download' });
  const big = available.locator('.dlrow').first();
  await expect(big.locator('.nm')).toHaveText('Qwen3.5 4B');
  await expect(big.locator('.tag.rec')).toHaveText('Recommended for this PC');
  await expect(big.locator('.mark')).toHaveAttribute('data-icon', 'qwen');

  // The download, in the row: held half-way for the picture.
  localModels.hold();
  await big.getByRole('button', { name: /^Download Qwen3\.5 4B \(2\.\d GB\)$/ }).click();
  const bar = big.getByRole('progressbar', { name: 'Downloading Qwen3.5 4B' });
  await expect(bar).toBeVisible();
  await expect.poll(async () => Number(await bar.getAttribute('aria-valuenow')), { timeout: 15_000 }).toBeGreaterThanOrEqual(40);
  await expect(big.locator('.pline')).toContainText(/Downloading… \d+% · \d+ MB of 4 MB/);
  // Mid-download it offers Cancel (never Uninstall: it isn't installed yet).
  await expect(big.getByRole('button', { name: 'Cancel the download of Qwen3.5 4B' })).toBeVisible();
  // Only one at a time.
  await expect(available.locator('.dlrow').nth(1).getByRole('button')).toBeDisabled();
  await shot(main, '03-picker-on-this-pc-download');
  localModels.release();

  // Downloaded: installed, and it can be picked.
  await expect(installed.locator('.nm')).toHaveText(['Qwen2.5 1.5B Instruct', 'Qwen3.5 4B'], { timeout: 15_000 });
  expect(fs.existsSync(path.join(yap.dataDir, 'llm', BIG_FILE))).toBe(true);
  expect(localModels.requests).toContain('/llm/qwen3.5-4b.gguf');
  await installed.getByRole('button', { name: /^Qwen3\.5 4B/ }).click();
  await expect(pop).toBeHidden();
  await expect(chip(main)).toHaveText('Qwen3.5 4B · On this PC');
  await expectStore(yap, 'config.json', (c) => c.ppLocalModel === BIG_FILE && c.llmScopes.chat.provider === 'ondevice');

  // Open again: on this PC, the 4B in use.
  await chip(main).click();
  await expect(tab(main, 'On this PC')).toHaveAttribute('aria-selected', 'true');
  const inUse = pane(main).getByRole('group', { name: 'Installed' }).getByRole('button', { name: /^Qwen3\.5 4B/ });
  await expect(inUse).toHaveAttribute('aria-pressed', 'true');
  await expect(inUse.locator('.tag.use')).toHaveText('In use');
  await expect(inUse).toBeFocused();
  await main.keyboard.press('Escape');
});

test('a big model left on the processor says so, and the switch goes to the 1.5B', async ({ yap, main, fakeLlm, shot }) => {
  await yap.invoke('e2e_local_llm_serving', { file: BIG_FILE, gpuLayers: 0, layers: 34 });
  try {
    const pop = await openPicker(main);
    await expect(tab(main, 'On this PC')).toHaveAttribute('aria-selected', 'true');
    const slow = pop.getByTestId('picker-slow');
    await expect(slow).toContainText('Runs on the processor on this PC: answers will be slow — Qwen2.5 1.5B Instruct is quicker.');
    await shot(main, '05-picker-processor-warning');
    await slow.getByRole('button', { name: 'Use Qwen2.5 1.5B Instruct' }).click();
    await expect(pop).toBeHidden();
    await expect(chip(main)).toHaveText('Qwen2.5 1.5B Instruct · On this PC · no note tools');
    await expectStore(yap, 'config.json', (c) => c.ppLocalModel === '' && c.llmScopes.chat.provider === 'ondevice');
  } finally {
    await yap.invoke('e2e_local_llm_serving', {});
    await backToGroq(yap, main, fakeLlm);
  }
});

test('an installed model uninstalls from its right-click menu, never the one in use', async ({ yap, main, fakeLlm, shot }) => {
  const llm = path.join(yap.dataDir, 'llm');
  const OWN = 'my-notes-model.gguf';
  // The 4B (downloaded above, or a stand-in when this runs alone) and a
  // GGUF of the person's own; nothing ever runs them.
  fs.writeFileSync(path.join(llm, BIG_FILE), Buffer.alloc(3 * 1048576));
  fs.writeFileSync(path.join(llm, OWN), Buffer.alloc(2 * 1048576));
  const installed = () => pane(main).getByRole('group', { name: 'Installed' });
  const menu = main.getByRole('menu');
  const confirm = main.getByRole('alertdialog');
  try {
    // Chat on the 4B: in use, so its Uninstall… is off and says what uses it.
    await openPicker(main);
    await tab(main, 'On this PC').click();
    // Every description in full, wrapped: never cut off (2026-10-27).
    const long = pane(main).locator('.ds', { hasText: 'Newer and smarter than the default' });
    await expect(long).toHaveText(/still fine without a graphics card$/);
    expect(await long.evaluate((el) => getComputedStyle(el).whiteSpace !== 'nowrap' && el.scrollWidth <= el.clientWidth)).toBe(true);
    await installed().getByRole('button', { name: /^Qwen3\.5 4B/ }).click();
    await expect(chip(main)).toHaveText('Qwen3.5 4B · On this PC');
    await openPicker(main);
    await installed().getByRole('button', { name: /^Qwen3\.5 4B/ }).click({ button: 'right' });
    await expect(menu.getByRole('menuitem')).toHaveText(['Uninstall…']);
    await expect(menu.getByRole('menuitem', { name: 'Uninstall…' })).toHaveAttribute('aria-disabled', 'true');
    await expect(menu.locator('.note')).toHaveText('Chat uses it. Switch that to another model first');
    await shot(main, '07-picker-uninstall-off-in-use');
    await main.keyboard.press('Escape');
    await expect(menu).toBeHidden();
    await expect(picker(main)).toBeVisible();
    // Rust refuses it too, in plain words, and keeps the file.
    await expect(yap.invoke('local_llm_uninstall', { file: BIG_FILE })).rejects.toThrow('Chat uses Qwen3.5 4B. Switch that to another model first.');
    expect(fs.existsSync(path.join(llm, BIG_FILE))).toBe(true);

    // Chat on the 1.5B: the 4B can go. Right-click → Uninstall… → confirm.
    await installed().getByRole('button', { name: /^Qwen2\.5 1\.5B/ }).click();
    await expect(chip(main)).toHaveText('Qwen2.5 1.5B Instruct · On this PC · no note tools');
    await openPicker(main);
    await installed().getByRole('button', { name: /^Qwen3\.5 4B/ }).click({ button: 'right' });
    await expect(menu.getByRole('menuitem')).toHaveText(['Use', 'Uninstall…']);
    await menu.getByRole('menuitem', { name: 'Uninstall…' }).click();
    await expect(confirm.getByRole('heading')).toHaveText('Uninstall Qwen3.5 4B?');
    await expect(confirm).toContainText('This deletes its 3 MB file from this PC. You can download it again.');
    await shot(main, '08-picker-uninstall-confirm');
    await confirm.getByRole('button', { name: 'Uninstall' }).click();
    await expect(confirm).toBeHidden();
    // Gone from the disk; the picker stays open, the row back under Download.
    expect(fs.existsSync(path.join(llm, BIG_FILE))).toBe(false);
    await expect(picker(main)).toBeVisible();
    await expect(installed().locator('.nm')).toHaveText(['Qwen2.5 1.5B Instruct', 'my-notes-model']);
    const available = pane(main).getByRole('group', { name: 'Download' });
    const again = available.getByRole('button', { name: /^Download Qwen3\.5 4B/ });
    await expect(again).toBeVisible();
    await expect(again).toBeFocused();

    // A model of the person's own: named by its file, and gone from the list.
    await installed().getByRole('button', { name: /^my-notes-model/ }).click({ button: 'right' });
    await menu.getByRole('menuitem', { name: 'Uninstall…' }).click();
    await expect(confirm.getByRole('heading')).toHaveText(`Uninstall ${OWN}?`);
    await expect(confirm).toContainText("This deletes its 2 MB file from this PC. It isn't on Yap's download list, so Yap can't download it again.");
    await confirm.getByRole('button', { name: 'Uninstall' }).click();
    await expect(confirm).toBeHidden();
    await expect(installed().locator('.nm')).toHaveText(['Qwen2.5 1.5B Instruct']);
    expect(fs.existsSync(path.join(llm, OWN))).toBe(false);
    await shot(main, '09-picker-after-uninstall');
    await main.keyboard.press('Escape');

    // Only a bare .gguf name in the models folder: never the engine, a
    // path, or anything outside it.
    for (const bad of ['..\\config.json', '../config.json', 'llamafile.exe', 'C:x.gguf', `..\\llm\\${OWN}`]) {
      await expect(yap.invoke('local_llm_uninstall', { file: bad })).rejects.toThrow("That isn't one of the on-device models on this PC.");
    }
    expect(fs.existsSync(path.join(llm, 'llamafile.exe'))).toBe(true);
    expect(fs.existsSync(path.join(yap.dataDir, 'config.json'))).toBe(true);
  } finally {
    fs.rmSync(path.join(llm, OWN), { force: true });
    await backToGroq(yap, main, fakeLlm);
  }
});

test('the keyboard: arrows through the rail and the models, Ctrl+K, Enter, Esc', async ({ yap, main, fakeLlm }) => {
  await backToGroq(yap, main, fakeLlm);
  await openView(main, 'Chat');
  await chip(main).focus();
  await main.keyboard.press('Enter');
  await expect(picker(main)).toBeVisible();
  const row = (name) => pane(main).getByRole('button', { name: new RegExp(`^${name}`) });
  // The current model has the focus, with the amber ring.
  await expect(row('GPT-OSS 20B')).toBeFocused();
  await expect(row('GPT-OSS 20B')).toHaveCSS('outline-style', 'solid');
  await main.keyboard.press('ArrowDown');
  await expect(row('GPT-OSS 120B')).toBeFocused();
  // ← to the rail; ↑/↓ switch providers as they go; → back to the models.
  await main.keyboard.press('ArrowLeft');
  await expect(tab(main, 'Groq')).toBeFocused();
  await main.keyboard.press('ArrowDown');
  await expect(tab(main, 'Gemini')).toBeFocused();
  await expect(tab(main, 'Gemini')).toHaveAttribute('aria-selected', 'true');
  await expect(picker(main).locator('.phead')).toContainText('Google Gemini');
  await main.keyboard.press('ArrowUp');
  await expect(tab(main, 'Groq')).toHaveAttribute('aria-selected', 'true');
  // Back where it was in Groq's list.
  await main.keyboard.press('ArrowRight');
  await expect(row('GPT-OSS 120B')).toBeFocused();

  // Ctrl+K: the search; ↓ into the results, Enter picks.
  await main.keyboard.press('Control+k');
  const search = picker(main).getByRole('searchbox');
  await expect(search).toBeFocused();
  await main.keyboard.type('120');
  await expect(pane(main).getByRole('group').locator('.gh')).toHaveText(['Groq']);
  await main.keyboard.press('ArrowDown');
  await expect(row('GPT-OSS 120B')).toBeFocused();
  await main.keyboard.press('Enter');
  await expect(picker(main)).toBeHidden();
  await expect(chip(main)).toBeFocused();
  await expect(chip(main)).toHaveText('GPT-OSS 120B · Groq');

  // Typing on a model searches; Esc closes and the chip has the focus back.
  await main.keyboard.press('Enter');
  await expect(row('GPT-OSS 120B')).toBeFocused();
  await main.keyboard.type('bra');
  await expect(search).toBeFocused();
  await expect(search).toHaveValue('bra');
  await expect(pane(main).locator('.mrow .nm')).toHaveText(['Brainy 9']);
  await main.keyboard.press('Escape');
  await expect(picker(main)).toBeHidden();
  await expect(chip(main)).toBeFocused();
  await backToGroq(yap, main, fakeLlm);
});

test("a self-hosted server's models show under On this PC, and Chat can use one", async ({ yap, main, fakeLlm, shot }) => {
  // It's an Ollama: it says the context each model runs with (read-only).
  fakeLlm.serveOllama({
    loaded: { 'llama3.1:8b': 8192 },
    show: { 'llama3.1:8b': { max: 131072 }, 'qwen2.5-7b-instruct': { max: 32768, numCtx: 16384 }, 'fake-chat': { max: 4096 } },
  });
  await setConfig(yap, main, { ppProvider: 'local', ppBaseUrl: fakeLlm.base, ppModel: 'fake-chat', ppApiKey: '' });
  try {
    const pop = await openPicker(main);
    await tab(main, 'On this PC').click();
    const host = new URL(fakeLlm.base).host;
    const server = pane(main).getByRole('group', { name: `Self-hosted · ${host}` });
    await expect(server.locator('.gh')).toContainText('3 models on this server');
    await expect(server.locator('.nm')).toHaveText(['fake-chat', 'llama3.1:8b', 'qwen2.5-7b-instruct']);
    await expect(server.locator('.mrow').first()).toContainText('Set up in Settings');
    // The loaded model's context, num_ctx from its Modelfile, else its maximum.
    await expect(server.locator('.mrow .ds')).toHaveText([
      `Up to 4K · Set up in Settings · ${host}`,
      `8K context, set in Ollama · ${host}`,
      `16K context, set in Ollama · ${host}`,
    ]);
    await server.scrollIntoViewIfNeeded();
    await shot(main, '06-picker-self-hosted-context');
    await server.getByRole('button', { name: /^llama3\.1:8b/ }).click();
    await expect(pop).toBeHidden();
    await expect(chip(main)).toHaveText('llama3.1:8b · On this PC');
    await expect(chip(main).locator('.mark')).toHaveAttribute('data-icon', 'llama');
    await expectStore(yap, 'config.json', (c) => c.llmScopes.chat.provider === 'local' && c.llmScopes.chat.model === 'llama3.1:8b');
    expect(yap.readJson('config.json').llmScopes.chat.baseUrl).toBe(fakeLlm.base);

    // The next answer comes from it.
    fakeLlm.reply('other', () => 'Hello from the server.');
    await main.getByRole('button', { name: 'New chat' }).click();
    await main.getByRole('textbox', { name: 'Message' }).fill('Who answers?');
    await main.getByRole('button', { name: 'Send' }).click();
    await expect(main.locator('.msgs')).toContainText('Hello from the server.');
    expect(fakeLlm.requests.at(-1).model).toBe('llama3.1:8b');
  } finally {
    fakeLlm.serveOllama(null);
    await setConfig(yap, main, { ppProvider: 'groq', ppBaseUrl: fakeLlm.base, ppModel: 'openai/gpt-oss-20b', ppApiKey: TEST_KEY });
    await backToGroq(yap, main, fakeLlm);
  }
});

test('a custom server elsewhere gets its own place on the rail', async ({ yap, main, fakeLlm }) => {
  const cfg = await yap.invoke('get_config');
  const custom = {
    enabled: false,
    provider: 'custom',
    baseUrl: 'https://llm.example.invalid/v1',
    model: 'big-model',
    apiKey: 'custom-test-key',
    apiKeys: {},
    prompt: '',
    disableThinking: false,
  };
  await setConfig(yap, main, { llmScopes: { ...cfg.llmScopes, noteFormatting: custom } });
  try {
    const pop = await openPicker(main);
    await expect(tab(main, 'Custom')).toHaveText('Custom, set up');
    await tab(main, 'Custom').click();
    const server = pane(main).getByRole('group', { name: 'llm.example.invalid' });
    // A test run never asks a server elsewhere; the model set up for it is there.
    await expect(server.locator('.gh')).toContainText("Couldn't list the server's models");
    await expect(server.locator('.nm')).toHaveText(['big-model']);
    await server.getByRole('button', { name: /^big-model/ }).click();
    await expect(pop).toBeHidden();
    await expect(chip(main)).toHaveText('big-model · Custom');
    await expectStore(yap, 'config.json', (c) => c.llmScopes.chat.provider === 'custom' && c.llmScopes.chat.baseUrl === custom.baseUrl);
    expect(yap.readJson('config.json').llmScopes.chat.apiKey).toBe('custom-test-key');
    // Picked again on the rail: it opens there.
    await chip(main).click();
    await expect(tab(main, 'Custom')).toHaveAttribute('aria-selected', 'true');
    await main.keyboard.press('Escape');
  } finally {
    // Note Formatting as it was.
    const { noteFormatting: _drop, ...rest } = (await yap.invoke('get_config')).llmScopes;
    const was = cfg.llmScopes.noteFormatting;
    await setConfig(yap, main, { llmScopes: was ? { ...rest, noteFormatting: was } : rest });
    await backToGroq(yap, main, fakeLlm);
  }
});
