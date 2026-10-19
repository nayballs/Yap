// The AI providers' live model lists (src-tauri/src/model_catalog.rs,
// src/lib/modelCatalog.js, ModelPicker.svelte, the Chat view's model chip):
// - Yap asks the provider itself, with the key already saved (Groq: Bearer;
//   Gemini: the key in x-goog-api-key, never the URL), and caches the list
//   in model_catalog.json without the key.
// - A model the provider added since Yap's list was made shows after the
//   curated ones with "New", in the Chat picker and in Settings; an older
//   one it lists waits behind "Show 1 more"; a curated model it no longer
//   lists is hidden.
// - The saved model missing from the list: a clear line on the chip and in
//   Settings, and still selectable.
// - A failed check: exactly Yap's own list, and Try again.
// - A key saved: that provider's list is checked at once.
// - A self-hosted server: its /models as the Model field's suggestions.
//
// Nothing leaves the PC: every provider is the local fake
// (support/fake-llm.js) through the debug-only YAP_MODELS_STANDIN, with
// test keys.
import { test as base, expect, openView, openSettings, closeSettings, expectStore } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';

const TEST_KEY = 'e2e-fake-key';
const GEMINI_KEY = 'e2e-fake-gemini-key';
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

// Groq's list today, for this spec: GPT-OSS 120B is gone (curated, hidden),
// "Brainy 9" just came out (New), ALLaM is older (Show more), Whisper
// isn't a chat model.
const GROQ_LIST = [
  groqModel('openai/gpt-oss-20b', 'GPT OSS 20B'),
  groqModel('qwen/qwen3.8-27b', 'Qwen/Qwen3.8-27B', { created: 1786984846 }),
  groqModel('acme/brainy-9', 'Brainy 9', { created: now() }),
  groqModel('allam-2-7b', 'ALLaM-2-7b', { created: 1737672203, supported_features: ['json_mode'] }),
  groqModel('whisper-large-v3', 'Whisper', { input_modalities: ['audio'], output_modalities: ['transcription'] }),
];

/** A model as Gemini's native /v1beta/models lists one (no dates). */
const geminiModel = (id, displayName, methods = ['generateContent', 'countTokens']) => ({
  name: `models/${id}`,
  displayName,
  description: displayName,
  inputTokenLimit: 1048576,
  supportedGenerationMethods: methods,
  thinking: true,
});

// Gemini 3 Flash (curated) is gone; Gemini 9 Ultra is new (not in the
// registry's `seen`); Gemini 2.5 Flash was there when the registry was made.
const GEMINI_LIST = [
  geminiModel('gemini-3.6-flash', 'Gemini 3.6 Flash'),
  geminiModel('gemini-3.8-flash', 'Gemini 3.8 Flash'),
  geminiModel('gemini-4-argon', 'Gemini 4 Argon'),
  geminiModel('gemini-9-ultra', 'Gemini 9 Ultra'),
  geminiModel('gemini-2.5-flash', 'Gemini 2.5 Flash'),
  geminiModel('gemini-3.8-flash-tts', 'Gemini 3.8 Flash TTS'),
  geminiModel('gemini-embedding-2', 'Gemini Embedding 2', ['embedContent']),
];

const test = base.extend({
  fakeLlm: [
    async ({}, use) => {
      const fake = await startFakeLlm();
      fake.listModels('groq', GROQ_LIST);
      fake.listModels('gemini', GEMINI_LIST);
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({ fakeLlm }, use) => {
      await use({
        name: 'model-catalog',
        env: { YAP_MODELS_STANDIN: fakeLlm.modelsStandin },
        config: {
          postProcessEnabled: true,
          ppProvider: 'groq',
          ppBaseUrl: fakeLlm.base,
          ppModel: 'openai/gpt-oss-20b',
          ppApiKey: TEST_KEY,
          ppApiKeys: { groq: TEST_KEY },
          llmScopes: { chat: groqChat(fakeLlm, 'openai/gpt-oss-20b') },
        },
      });
    },
    { scope: 'worker' },
  ],
});

/** A whole Chat scope, as config.json holds one (config.rs `LlmScope`). */
function groqChat(fakeLlm, model) {
  return { enabled: true, provider: 'groq', baseUrl: fakeLlm.base, model, apiKey: TEST_KEY, apiKeys: {}, prompt: '', disableThinking: false };
}

/** Change config.json as another surface would, and tell Settings' copy. */
async function setConfig(yap, main, patch) {
  const cfg = await yap.invoke('get_config');
  await yap.invoke('save_config', { cfg: { ...cfg, ...patch } });
  await main.evaluate((p) => window.dispatchEvent(new CustomEvent('yap-config-patched', { detail: p })), patch);
  // Let Settings' own auto-save (400 ms after a change) land first.
  await main.waitForTimeout(500);
}

const chip = (main) => main.locator('.modelchip .chip');
const popover = (main) => main.getByRole('dialog', { name: 'Switch the Chat model' });
/** A model row in the Chat picker's pane (a button), by the start of its name. */
const chatRow = (pop, name) => pop.locator('.pane').getByRole('button', { name: new RegExp(`^${name}`) });
const picker = (dialog, provider) => dialog.locator(`.picker[data-provider="${provider}"]`);
/** The rows a picker shows before "Show more". */
const rows = (p) => p.locator('.list').first().locator('.srow');

async function languageModels(main, tab) {
  const dialog = await openSettings(main, 'Language Models');
  await dialog.getByRole('tab', { name: tab }).click();
  return dialog;
}

test('the Chat picker shows a model Groq added as New and hides one it dropped', async ({ yap, main, fakeLlm, shot }) => {
  await openView(main, 'Chat');
  await expect(chip(main)).toHaveText('GPT-OSS 20B · Groq');
  await chip(main).click();
  const pop = popover(main);
  await expect(pop).toBeVisible();
  // Curated first (GPT-OSS 120B gone), then what Groq added since; the
  // older one it lists behind "Show 1 more", as in Settings.
  await expect(pop.locator('.pane .mrow .nm')).toHaveText(['GPT-OSS 20B', 'Qwen3.8 27B', 'Brainy 9']);
  await expect(chatRow(pop, 'Brainy 9').locator('.tag.new')).toHaveText('New');
  await expect(pop.locator('.phead')).toContainText('Connected · checked just now');
  await pop.getByRole('button', { name: 'Show 1 more Groq model' }).click();
  await expect(pop.locator('.pane .mrow .nm')).toHaveText(['GPT-OSS 20B', 'Qwen3.8 27B', 'Brainy 9', 'ALLaM-2-7b']);
  await shot(main, '01-chat-picker-new-model');

  // Groq was asked with the saved key, through the stand-in.
  const asked = fakeLlm.modelRequests.find((r) => r.provider === 'groq');
  expect(asked.path).toBe('/openai/v1/models');
  expect(asked.headers.authorization).toBe(`Bearer ${TEST_KEY}`);
  // The cache: the list, never the key.
  await expectStore(yap, 'model_catalog.json', (c) => c.providers?.groq?.models?.some((m) => m.id === 'acme/brainy-9'));
  const cached = yap.readJson('model_catalog.json');
  expect(cached.providers.groq.models.map((m) => m.id)).toEqual(['openai/gpt-oss-20b', 'qwen/qwen3.8-27b', 'acme/brainy-9', 'allam-2-7b']);
  expect(JSON.stringify(cached)).not.toContain(TEST_KEY);

  // The new model can be picked, and the next answer comes from it.
  await chatRow(pop, 'Brainy 9').click();
  await expect(chip(main)).toHaveText('Brainy 9 · Groq');
  await expectStore(yap, 'config.json', (c) => c.llmScopes.chat.model === 'acme/brainy-9');
  await main.getByRole('button', { name: 'New chat' }).click();
  fakeLlm.scriptChat([{ content: 'Hello from Brainy.' }]);
  await main.getByPlaceholder('Type a message...').fill('Who are you?');
  await main.getByRole('button', { name: 'Send' }).click();
  await expect(main.locator('.msgs')).toContainText('Hello from Brainy.');
  await expect(main.locator('.msgs .byline')).toHaveText(['Brainy 9 · Groq']);
  expect(fakeLlm.requests.at(-1).model).toBe('acme/brainy-9');

  await setConfig(yap, main, { llmScopes: { ...(await yap.invoke('get_config')).llmScopes, chat: groqChat(fakeLlm, 'openai/gpt-oss-20b') } });
});

test('Settings lists the live models: New after the curated ones, older ones behind Show more', async ({ main, shot }) => {
  const dialog = await languageModels(main, 'Dictation Cleanup');
  const groq = picker(dialog, 'groq');
  await groq.scrollIntoViewIfNeeded();
  await expect(rows(groq)).toHaveCount(3);
  await expect(rows(groq).nth(0)).toContainText('GPT-OSS 20B');
  await expect(rows(groq).nth(0)).toContainText('Active');
  await expect(rows(groq).nth(1)).toContainText('Qwen3.8 27B');
  await expect(rows(groq).nth(2).locator('.slabel')).toHaveText('Brainy 9');
  await expect(rows(groq).nth(2).locator('.snew')).toHaveText('New');
  await expect(groq).not.toContainText('GPT-OSS 120B');
  await expect(groq).toContainText("Groq's current models, checked just now.");
  await shot(main, '02-settings-new-model');

  await groq.getByRole('button', { name: 'Show 1 more Groq model' }).click();
  const more = groq.locator('.list').nth(1).locator('.srow');
  await expect(more).toHaveCount(1);
  await expect(more.first()).toContainText('ALLaM-2-7b');
  await expect(groq).not.toContainText('Whisper');
  await closeSettings(main);
});

test('a saved model the provider no longer offers says so, and stays selectable', async ({ yap, main, fakeLlm, shot }) => {
  await setConfig(yap, main, { llmScopes: { ...(await yap.invoke('get_config')).llmScopes, chat: groqChat(fakeLlm, 'openai/gpt-oss-120b') } });
  await openView(main, 'Chat');
  await expect(chip(main)).toHaveText('GPT-OSS 120B · Groq · no longer offered');
  await chip(main).click();
  const pop = popover(main);
  await expect(pop.getByRole('alert')).toHaveText('Groq no longer offers GPT-OSS 120B. Pick another model.');
  const gone = chatRow(pop, 'GPT-OSS 120B');
  await expect(gone).toHaveAttribute('aria-pressed', 'true');
  await expect(gone.locator('.tag.gone')).toHaveText('No longer offered');
  await shot(main, '03-chip-no-longer-offered');
  await main.keyboard.press('Escape');
  await expect(pop).toBeHidden();

  // Settings → Chat: the same line, the row kept and selected.
  const dialog = await languageModels(main, 'Chat');
  const groq = picker(dialog, 'groq');
  await groq.scrollIntoViewIfNeeded();
  await expect(groq.getByRole('alert')).toHaveText('Groq no longer offers GPT-OSS 120B. Pick another model.');
  const selected = groq.locator('.srow.sel');
  await expect(selected).toContainText('GPT-OSS 120B');
  await expect(selected.locator('.sgone')).toHaveText('No longer offered');
  await shot(main, '04-settings-no-longer-offered');

  // Picking another one clears it.
  await rows(groq).filter({ hasText: 'GPT-OSS 20B' }).click();
  await expect(groq.getByRole('alert')).toHaveCount(0);
  await expect(groq).not.toContainText('GPT-OSS 120B');
  await expectStore(yap, 'config.json', (c) => c.llmScopes.chat.model === 'openai/gpt-oss-20b');
  await closeSettings(main);
  await expect(chip(main)).toHaveText('GPT-OSS 20B · Groq');
});

test('a check that fails shows Yap’s own list, and Try again brings the live one back', async ({ main, fakeLlm, shot }) => {
  const dialog = await languageModels(main, 'Dictation Cleanup');
  const groq = picker(dialog, 'groq');
  await groq.scrollIntoViewIfNeeded();
  await expect(rows(groq)).toHaveCount(3);

  fakeLlm.failModels('groq', 1);
  await groq.getByRole('button', { name: 'Check now' }).click();
  await expect(groq).toContainText("Couldn't check Groq's models: Groq answered HTTP 500. Showing Yap's list.");
  // Exactly the curated list: GPT-OSS 120B back, nothing New, no "more".
  await expect(rows(groq).locator('.slabel')).toHaveText(['GPT-OSS 20B', 'GPT-OSS 120B', 'Qwen3.8 27B']);
  await expect(groq.locator('.snew')).toHaveCount(0);
  await expect(groq.getByRole('button', { name: /more Groq/ })).toHaveCount(0);
  await shot(main, '05-settings-check-failed');

  await groq.getByRole('button', { name: 'Try again' }).click();
  await expect(rows(groq).locator('.slabel')).toHaveText(['GPT-OSS 20B', 'Qwen3.8 27B', 'Brainy 9']);
  await expect(groq).toContainText("Groq's current models, checked just now.");
  await closeSettings(main);
});

test('a key saved for Gemini checks its list at once, the key in a header, never the URL', async ({ yap, main, fakeLlm, shot }) => {
  const before = fakeLlm.modelRequests.length;
  const cfg = await yap.invoke('get_config');
  await setConfig(yap, main, { ppApiKeys: { ...cfg.ppApiKeys, gemini: GEMINI_KEY } });
  await expect.poll(() => fakeLlm.modelRequests.slice(before).some((r) => r.provider === 'gemini')).toBe(true);
  const asked = fakeLlm.modelRequests.slice(before).find((r) => r.provider === 'gemini');
  expect(asked.path).toBe('/v1beta/models');
  expect(asked.headers['x-goog-api-key']).toBe(GEMINI_KEY);
  expect(asked.headers.authorization).toBeUndefined();
  expect(asked.url).not.toContain(GEMINI_KEY);

  // Settings → Dictation Cleanup → Gemini: the curated ones it lists, then
  // the one Google added (it gives no dates: new to Yap's list = New);
  // Gemini 3 Flash (curated) is gone; Gemini 2.5 Flash waits behind "more".
  const dialog = await languageModels(main, 'Dictation Cleanup');
  await dialog.getByRole('tab', { name: 'Gemini' }).click();
  const gemini = picker(dialog, 'gemini');
  await gemini.scrollIntoViewIfNeeded();
  await expect(rows(gemini).locator('.slabel')).toHaveText(['Gemini 3.6 Flash', 'Gemini 3.8 Flash', 'Gemini 4 Argon', 'Gemini 9 Ultra']);
  await expect(rows(gemini).nth(3).locator('.snew')).toHaveText('New');
  // Switching provider picked its first model it still offers.
  await expect(rows(gemini).nth(0)).toContainText('Active');
  await expect(gemini).not.toContainText('Gemini 3 Flash');
  await expect(gemini.getByRole('button', { name: 'Show 1 more Gemini model' })).toBeVisible();
  await shot(main, '06-settings-gemini-new-model');

  // Back to Groq for what follows: its first model again.
  await dialog.getByRole('tab', { name: 'Groq' }).click();
  await expectStore(yap, 'config.json', (c) => c.ppProvider === 'groq' && c.ppModel === 'openai/gpt-oss-20b');
  await closeSettings(main);
});

test("a self-hosted server's models are the Model field's suggestions", async ({ yap, main, fakeLlm, shot }) => {
  const dialog = await languageModels(main, 'Dictation Cleanup');
  await dialog.locator('.mode').filter({ hasText: 'Self-Hosted' }).click();
  const baseUrl = dialog.getByPlaceholder('http://localhost:11434/v1');
  await baseUrl.fill(fakeLlm.base);
  const line = dialog.getByTestId('server-models');
  // Its /models, an embedding model left out.
  await expect(line).toContainText('3 models on this server: fake-chat, llama3.1:8b, qwen2.5-7b-instruct');
  await expect(dialog.locator('#pp-local-models option')).toHaveCount(3);
  const model = dialog.getByPlaceholder('llama3.1');
  await model.fill('fake-chat');
  await line.scrollIntoViewIfNeeded();
  await shot(main, '07-settings-self-hosted-models');
  // A name the server doesn't list is pointed out.
  await model.fill('llama9');
  await expect(line).toContainText("The server doesn't list “llama9”.");
  await expect(fakeLlm.modelRequests.some((r) => r.provider === 'server' && r.path === '/v1/models')).toBe(true);

  // Back to the cloud for the next test.
  await dialog.locator('.mode').filter({ hasText: 'Cloud Providers' }).click();
  await expectStore(yap, 'config.json', (c) => c.ppProvider === 'groq');
  await closeSettings(main);
});
