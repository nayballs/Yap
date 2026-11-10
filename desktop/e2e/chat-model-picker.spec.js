// The Chat model picker, a provider rail (desktop/src/lib/views/chat/ChatModelPicker.svelte,
// chatModels.svelte.js, ModelChip.svelte; desktop/src-tauri/src/ai/ai_model.rs):
// - the chip names the model (the provider's mark, "· Groq"); the picker
//   opens on its provider, the rail lists Recent, every cloud provider with
//   a dot (connected or not), and On this PC; a cloud provider's rows are
//   its Latest (the newest of each family: core/latestModels.js) and "More
//   models (N)", the older ones in place below it, newest first (with "New");
// - Claude with a stand-in list of twelve: its four latest, More models (8);
//   the keyboard reaching More models (aria-expanded); an older one picked
//   (the effort strip follows it), opened again with More models open;
//   closed by hand, the one in use stays on show under it; search finds an
//   older one with it closed; remembered while Yap runs; light and dark
//   pictures;
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
//   search, Enter, Esc;
// - ChatGPT, one entry for OpenAI's two ways (2026-11-02): nothing connected,
//   a connect card (Continue with ChatGPT, "Use an API key instead" opening
//   the key field, no model list); an OpenAI model found by search leads to
//   that field; a key saved, OpenAI's Latest and More models with "Use your
//   ChatGPT plan instead…"; picked, the chip says "OpenAI API key", Recent
//   and search say which way (the plan's states: chatgpt.spec.js).
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
// A stand-in Anthropic list: the four latest, eight older (dated snapshots
// among them, and one Yap's own list doesn't have).
const ANTHROPIC_LIST = [
  claudeModel('claude-opus-5-5', 'Claude Opus 5.5', '2026-08-01T00:00:00Z'),
  claudeModel('claude-fable-5-2', 'Claude Fable 5.2', '2026-11-04T00:00:00Z'),
  claudeModel('claude-fable-5-1', 'Claude Fable 5.1', '2026-09-15T00:00:00Z'),
  claudeModel('claude-sonnet-5-5', 'Claude Sonnet 5.5', '2026-08-01T00:00:00Z'),
  claudeModel('claude-haiku-5-5', 'Claude Haiku 5.5', '2026-08-20T00:00:00Z'),
  claudeModel('claude-opus-4-8', 'Claude Opus 4.8', '2026-05-01T00:00:00Z'),
  claudeModel('claude-opus-4-7', 'Claude Opus 4.7', '2026-03-01T00:00:00Z'),
  claudeModel('claude-sonnet-4-6', 'Claude Sonnet 4.6', '2026-01-15T00:00:00Z'),
  claudeModel('claude-opus-4-6', 'Claude Opus 4.6', '2026-01-10T00:00:00Z'),
  claudeModel('claude-opus-4-5-20251101', 'Claude Opus 4.5', '2025-11-01T00:00:00Z'),
  claudeModel('claude-haiku-4-5-20251001', 'Claude Haiku 4.5', '2025-10-01T00:00:00Z'),
  claudeModel('claude-sonnet-4-5-20250929', 'Claude Sonnet 4.5', '2025-09-29T00:00:00Z'),
];
const CLAUDE_LATEST = ['Claude Opus 5.5', 'Claude Fable 5.2', 'Claude Sonnet 5.5', 'Claude Haiku 5.5'];
const OPENAI_KEY = 'sk-proj-e2e-fake-0000004321';
/** As OpenAI's /v1/models lists one: Yap's own nine, nothing new. */
const OPENAI_LIST = ['gpt-5.6-luna', 'gpt-5-mini', 'gpt-5.6-terra', 'gpt-5.6-sol', 'gpt-6-luna', 'gpt-6.1-sol', 'gpt-6-astra', 'gpt-5.5', 'gpt-5.4-mini'].map(
  (id) => ({ id, object: 'model', created: 1760000000, owned_by: 'openai' })
);
const CLAUDE_OLDER = [
  'Claude Fable 5.1',
  'Claude Opus 4.8',
  'Claude Opus 4.7',
  'Claude Sonnet 4.6',
  'Claude Opus 4.6',
  'Claude Opus 4.5',
  'Claude Haiku 4.5',
  'Claude Sonnet 4.5',
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
      fake.listModels('openai', OPENAI_LIST);
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
const latest = (main) => pane(main).getByRole('group', { name: 'Latest' });
const moreButton = (main) => pane(main).getByRole('button', { name: /^More models \(\d+\)$/ });
const older = (main, short) => pane(main).getByRole('group', { name: `More ${short} models` });

/** The selected provider's More models open or closed, whatever it was. */
async function setMore(main, open) {
  const b = moreButton(main);
  if ((await b.getAttribute('aria-expanded')) !== String(open)) await b.click();
  await expect(b).toHaveAttribute('aria-expanded', String(open));
}

/** General → Appearance → Theme (System, Light or Dark), saved. */
async function pickTheme(yap, main, label) {
  const dialog = await openSettings(main, 'General');
  const theme = dialog.getByRole('group', { name: 'Appearance' }).getByRole('tablist', { name: 'Theme' });
  await theme.getByRole('tab', { name: label, exact: true }).click();
  await expectStore(yap, 'config.json', (c) => c.theme === label.toLowerCase(), `the theme ${label} to be saved`);
  await closeSettings(main);
}

async function openPicker(main) {
  await openView(main, 'Chat');
  await chip(main).click();
  await expect(picker(main)).toBeVisible();
  return picker(main);
}

test('the chip opens a provider rail on its provider, with the rows Settings shows', async ({ yap, main, shot }) => {
  await openView(main, 'Chat');
  await expect(chip(main)).toHaveText('GPT-OSS 20B Medium · Groq');
  await expect(chip(main).locator('.mark')).toHaveAttribute('data-icon', 'groq');
  const pop = await openPicker(main);

  // The rail: Recent, every cloud provider with its state, On this PC.
  await expect(pop.getByRole('tablist', { name: 'Providers' })).toBeVisible();
  await expect(pop.getByRole('tablist', { name: 'Providers' }).getByRole('tab')).toHaveText([
    'Recent',
    'Groq, connected',
    'Gemini, not set up',
    'Claude, not set up',
    // OpenAI's two ways (the ChatGPT plan, an API key): one entry.
    'ChatGPT, not set up',
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
  // Latest: Groq's biggest and smallest GPT-OSS, each with its family's mark.
  const rows = latest(main).locator('.mrow');
  await expect(rows.locator('.nm')).toHaveText(['GPT-OSS 120B', 'GPT-OSS 20B']);
  await expect(rows.nth(1)).toHaveAttribute('aria-pressed', 'true');
  await expect(rows.nth(0)).toHaveAttribute('aria-pressed', 'false');
  await expect(rows.nth(1).locator('.mark')).toHaveAttribute('data-icon', 'openai-oss');
  // The context Groq's own list gives (131,072 tokens), as info.
  await expect(rows.nth(1).locator('.ds')).toHaveText(/ · 128K context$/);
  // More models: closed (the model in use is a latest one); open, the rest
  // in place below it, newest first: what Groq added since (New) on top.
  await expect(moreButton(main)).toHaveText('More models (2)');
  await expect(moreButton(main)).toHaveAttribute('aria-expanded', 'false');
  await expect(pane(main).locator('.mrow .nm')).toHaveText(['GPT-OSS 120B', 'GPT-OSS 20B']);
  await setMore(main, true);
  const more = older(main, 'Groq').locator('.mrow');
  await expect(more.locator('.nm')).toHaveText(['Brainy 9', 'Qwen3.8 27B']);
  await expect(more.nth(0).locator('.tag.new')).toHaveText('New');
  await expect(more.nth(0).locator('.mark')).toHaveAttribute('data-icon', 'groq');
  await expect(more.nth(1).locator('.mark')).toHaveAttribute('data-icon', 'qwen');
  await expect(more.nth(0).locator('.ds')).toHaveText('128K context');
  await expect(pop).toContainText('Changes the model for Chat only. Dictation cleanup stays as it is.');
  await shot(main, '01-picker-groq');
  await setMore(main, false);
  await expect(older(main, 'Groq')).toHaveCount(0);

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
  await expect(chip(main)).toHaveText('GPT-OSS 120B Medium · Groq');
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
  // Its curated models, greyed until there's a key: the four latest, the
  // older seven under More models.
  const locked = pane(main).locator('.mrow.locked');
  await expect(locked).toHaveCount(4);
  await expect(latest(main).locator('.nm')).toHaveText(CLAUDE_LATEST);
  await expect(moreButton(main)).toHaveText('More models (7)');
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
  await expect(head).toContainText('Connected · checked just now');
  await expect(latest(main).locator('.nm')).toHaveText(CLAUDE_LATEST);
  await expect(moreButton(main)).toHaveText('More models (8)');
  await expect(pane(main).locator('.mrow.locked')).toHaveCount(0);
  await expect(head.locator('.keymask')).toHaveText('sk-…7890');
  await expect(tab(main, 'Claude')).toHaveText('Claude, connected');
  expect(await pop.innerHTML()).not.toContain(ANTHROPIC_KEY);
  expect(JSON.stringify(yap.readJson('model_catalog.json'))).not.toContain(ANTHROPIC_KEY);

  await pane(main).getByRole('button', { name: /^Claude Sonnet 5\.5/ }).click();
  await expect(chip(main)).toHaveText('Claude Sonnet 5.5 High · Anthropic API key');
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

test('Claude: Latest has the four newest, More models the older ones, which can be picked and searched', async ({ yap, main, fakeLlm, shot }) => {
  // Anthropic connected (as the test above leaves it, or here when this runs alone).
  const cfg = await yap.invoke('get_config');
  if (cfg.ppApiKeys?.anthropic !== ANTHROPIC_KEY) await setConfig(yap, main, { ppApiKeys: { ...cfg.ppApiKeys, anthropic: ANTHROPIC_KEY } });
  await backToGroq(yap, main, fakeLlm);
  const effort = picker(main).getByTestId('chat-effort');
  const levels = effort.getByRole('tab');
  try {
    await pickTheme(yap, main, 'Light');
    const pop = await openPicker(main);
    await tab(main, 'Claude').click();
    await expect(pop.locator('.phead')).toContainText('Connected · checked');

    // Latest: the newest Opus, Fable, Sonnet and Haiku; More models (8) closed under it.
    await expect(latest(main).locator('.nm')).toHaveText(CLAUDE_LATEST);
    await expect(latest(main).locator('.mrow .mark').first()).toHaveAttribute('data-icon', 'claude');
    await expect(moreButton(main)).toHaveText('More models (8)');
    await setMore(main, false);
    await expect(pane(main).locator('.mrow')).toHaveCount(4);
    await shot(main, '10-picker-latest-light');

    // The keyboard: ↓ from the last of Latest to More models, Enter opens
    // it in place (announced: aria-expanded), ↓ on into the older ones.
    await latest(main).getByRole('button', { name: /^Claude Haiku 5\.5/ }).focus();
    await main.keyboard.press('ArrowDown');
    await expect(moreButton(main)).toBeFocused();
    await main.keyboard.press('Enter');
    await expect(moreButton(main)).toHaveAttribute('aria-expanded', 'true');
    await expect(moreButton(main)).toHaveAttribute('aria-controls', 'chat-model-more-anthropic');
    // The older ones, newest first, in the rows' own style (marks, tags).
    const more = older(main, 'Claude');
    await expect(more.locator('.nm')).toHaveText(CLAUDE_OLDER);
    await expect(more.locator('.mrow').first().locator('.mark')).toHaveAttribute('data-icon', 'claude');
    await expect(more.locator('.mrow').first().locator('.tag.img')).toHaveText('Reads images');
    await main.keyboard.press('ArrowDown');
    await expect(more.getByRole('button', { name: /^Claude Fable 5\.1/ })).toBeFocused();
    await main.keyboard.press('ArrowUp');
    await expect(moreButton(main)).toBeFocused();
    await shot(main, '11-picker-more-models-light');

    // Pick an older one: Chat uses it; the effort strip follows it.
    await more.getByRole('button', { name: /^Claude Opus 4\.6/ }).click();
    await expect(pop).toBeHidden();
    await expect(chip(main)).toHaveText('Claude Opus 4.6 High · Anthropic API key');
    await expectStore(yap, 'config.json', (c) => c.llmScopes.chat.provider === 'anthropic' && c.llmScopes.chat.model === 'claude-opus-4-6');

    // Opened again: on Claude, More models open, the one in use ✓ where it is.
    await pickTheme(yap, main, 'Dark');
    await chip(main).click();
    await expect(tab(main, 'Claude')).toHaveAttribute('aria-selected', 'true');
    await expect(moreButton(main)).toHaveAttribute('aria-expanded', 'true');
    const inUse = older(main, 'Claude').getByRole('button', { name: /^Claude Opus 4\.6/ });
    await expect(inUse).toHaveAttribute('aria-pressed', 'true');
    await expect(inUse).toBeFocused();
    await expect(effort).toContainText('Claude Opus 4.6');
    await expect(levels).toHaveText(['Low', 'Medium', 'High', 'Max']);
    await expect(levels.filter({ hasText: 'High' })).toHaveAttribute('aria-selected', 'true');
    await expect(effort.getByRole('button', { name: 'Fast mode' })).toHaveCount(0);
    await shot(main, '12-picker-more-models-dark');

    // Closed by hand: the one in use stays on show under it, ✓; search still
    // finds every older one, and leaves More models as it was.
    await setMore(main, false);
    await expect(older(main, 'Claude')).toHaveCount(0);
    await expect(pane(main).locator('.mrow .nm')).toHaveText([...CLAUDE_LATEST, 'Claude Opus 4.6']);
    await expect(pane(main).getByRole('button', { name: /^Claude Opus 4\.6/ })).toHaveAttribute('aria-pressed', 'true');
    await expect(moreButton(main)).toHaveText('More models (8)');
    await shot(main, '13-picker-latest-dark');
    const search = picker(main).getByRole('searchbox', { name: 'Search models and providers' });
    await search.fill('opus 4.7');
    await expect(pane(main).getByRole('group').locator('.gh')).toHaveText(['Claude']);
    await expect(pane(main).locator('.mrow .nm')).toHaveText(['Claude Opus 4.7']);
    await search.fill('claude sonnet');
    await expect(pane(main).getByRole('group', { name: 'Claude' }).locator('.nm')).toHaveText(['Claude Sonnet 5.5', 'Claude Sonnet 4.6', 'Claude Sonnet 4.5']);
    await picker(main).getByRole('button', { name: 'Clear the search' }).click();
    await expect(moreButton(main)).toHaveAttribute('aria-expanded', 'false');
    // Picked from the search: More models open next time, where it is.
    await search.fill('opus 4.7');
    await search.press('Enter');
    await expect(picker(main)).toBeHidden();
    await expect(chip(main)).toHaveText('Claude Opus 4.7 High · Anthropic API key');
    await chip(main).click();
    await expect(moreButton(main)).toHaveAttribute('aria-expanded', 'true');
    await expect(older(main, 'Claude').getByRole('button', { name: /^Claude Opus 4\.7/ })).toHaveAttribute('aria-pressed', 'true');
    await expect(levels).toHaveText(['Low', 'Medium', 'High', 'Extra', 'Max']);

    // A latest one: the strip follows again (Fast mode is Opus 5.5's), and
    // More models stays as it was left, while Yap runs.
    await latest(main).getByRole('button', { name: /^Claude Opus 5\.5/ }).click();
    await expect(chip(main)).toHaveText('Claude Opus 5.5 Medium · Anthropic API key');
    await chip(main).click();
    await expect(latest(main).getByRole('button', { name: /^Claude Opus 5\.5/ })).toHaveAttribute('aria-pressed', 'true');
    await expect(levels).toHaveText(['Low', 'Medium', 'High', 'Extra', 'Max']);
    await expect(effort.getByRole('button', { name: 'Fast mode' })).toHaveAttribute('aria-pressed', 'false');
    await expect(moreButton(main)).toHaveAttribute('aria-expanded', 'true');
    await setMore(main, false);
    await main.keyboard.press('Escape');
    await chip(main).click();
    await expect(moreButton(main)).toHaveAttribute('aria-expanded', 'false');
    await main.keyboard.press('Escape');
    await expect(picker(main)).toBeHidden();
  } finally {
    if (await picker(main).isVisible()) await main.keyboard.press('Escape');
    await pickTheme(yap, main, 'System');
    await backToGroq(yap, main, fakeLlm);
  }
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
  await expect(pop.getByRole('tablist', { name: 'Providers' }).getByRole('tab', { selected: true })).toHaveCount(0);
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
  // Groq's More models closed (an earlier test may have left it open).
  await openPicker(main);
  await setMore(main, false);
  await main.keyboard.press('Escape');
  await expect(picker(main)).toBeHidden();
  await chip(main).focus();
  await main.keyboard.press('Enter');
  await expect(picker(main)).toBeVisible();
  const row = (name) => pane(main).getByRole('button', { name: new RegExp(`^${name}`) });
  // The current model has the focus, with the amber ring.
  await expect(row('GPT-OSS 20B')).toBeFocused();
  await expect(row('GPT-OSS 20B')).toHaveCSS('outline-style', 'solid');
  // ↓ to More models (a stop of its own, with the ring), Space opens it, ↓
  // into the older ones; Space again closes it.
  await main.keyboard.press('ArrowDown');
  await expect(moreButton(main)).toBeFocused();
  await expect(moreButton(main)).toHaveCSS('outline-style', 'solid');
  await main.keyboard.press('Space');
  await expect(moreButton(main)).toHaveAttribute('aria-expanded', 'true');
  await main.keyboard.press('ArrowDown');
  await expect(row('Brainy 9')).toBeFocused();
  await main.keyboard.press('ArrowUp');
  await main.keyboard.press('Space');
  await expect(moreButton(main)).toHaveAttribute('aria-expanded', 'false');
  await expect(row('Brainy 9')).toHaveCount(0);
  await main.keyboard.press('ArrowUp');
  await expect(row('GPT-OSS 20B')).toBeFocused();
  await main.keyboard.press('ArrowUp');
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
  await expect(chip(main)).toHaveText('GPT-OSS 120B Medium · Groq');

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

test('ChatGPT: one entry for the plan and an OpenAI key — nothing connected, then a key', async ({ yap, main, fakeLlm, shot }) => {
  await backToGroq(yap, main, fakeLlm);
  const box = () => picker(main).getByLabel('OpenAI API key');
  try {
    const pop = await openPicker(main);
    await tab(main, 'ChatGPT').click();
    await expect(tab(main, 'ChatGPT')).toHaveText('ChatGPT, not set up');
    await expect(tab(main, 'ChatGPT').locator('.mark')).toHaveAttribute('data-icon', 'openai');
    const head = pop.locator('.phead');
    await expect(head.locator('.ptitle')).toHaveText('ChatGPT');
    await expect(head).toContainText('Not set up');

    // Nothing connected: the plan first, the key quieter under it; no models.
    const card = pop.getByTestId('gpt-connect');
    await expect(card).toContainText('Use your ChatGPT plan');
    await expect(card).toContainText('no cost per message');
    await expect(card.getByRole('button', { name: 'Continue with ChatGPT' })).toBeVisible();
    const toggle = card.getByRole('button', { name: 'Use an API key instead' });
    await expect(toggle).toHaveAttribute('aria-expanded', 'false');
    await expect(box()).toHaveCount(0);
    await expect(pane(main).locator('.mrow')).toHaveCount(0);
    await expect(pane(main).getByTestId('gpt-ways')).toHaveCount(0);
    await shot(main, '14-picker-chatgpt-nothing-connected');
    await toggle.click();
    await expect(toggle).toHaveAttribute('aria-expanded', 'true');
    await expect(box()).toBeFocused();
    await shot(main, '15-picker-chatgpt-key-field');
    await toggle.click();
    await expect(box()).toHaveCount(0);

    // Search finds OpenAI's models by "chatgpt" too, saying which way pays;
    // one (greyed: no key) leads to the ChatGPT entry, its key field open.
    const search = pop.getByRole('searchbox', { name: 'Search models and providers' });
    await search.fill('chatgpt astra');
    await expect(pane(main).getByRole('group').locator('.gh')).toHaveText(['OpenAI API key']);
    await expect(pane(main).locator('.mrow.locked .tag.key')).toHaveText('Add key');
    await pane(main).locator('.mrow.locked').first().click();
    await expect(tab(main, 'ChatGPT')).toHaveAttribute('aria-selected', 'true');
    await expect(toggle).toHaveAttribute('aria-expanded', 'true');
    await expect(box()).toBeFocused();

    // Get a key ↗: OpenAI's key page. Saved: a key only, OpenAI's models.
    await card.getByRole('button', { name: 'Get a key ↗' }).click();
    await expect.poll(() => yap.invoke('e2e_opened_urls')).toContain('https://platform.openai.com/api-keys');
    await box().fill(OPENAI_KEY);
    await card.getByRole('button', { name: 'Save key' }).click();
    await expectStore(yap, 'config.json', (c) => c.ppApiKeys?.openai === OPENAI_KEY);
    await expect(card).toBeHidden();
    await expect(tab(main, 'ChatGPT')).toHaveText('ChatGPT, connected');
    await expect(tab(main, 'ChatGPT').locator('.st')).not.toHaveClass(/off/);
    await expect(head).toContainText('Connected · checked just now');
    await expect(head.locator('.keymask')).toHaveText('sk-…4321');
    await expect(latest(main).locator('.nm')).toHaveText(['GPT-6 Astra', 'GPT-6 Luna']);
    await expect(moreButton(main)).toHaveText('More models (7)');
    await expect(pane(main).getByTestId('gpt-ways')).toHaveCount(0);
    // The other way, under the models.
    await expect(pane(main).getByRole('button', { name: 'Use your ChatGPT plan instead…' })).toBeVisible();
    expect(await pop.innerHTML()).not.toContain(OPENAI_KEY);
    await shot(main, '16-picker-chatgpt-key-only');

    // Picked: provider "openai" as ever; the chip says the key pays.
    await latest(main).getByRole('button', { name: /^GPT-6 Astra/ }).click();
    await expect(pop).toBeHidden();
    await expect(chip(main)).toContainText('GPT-6 Astra');
    await expect(chip(main)).toContainText('· OpenAI API key');
    await expect(main.locator('.modelchip').getByRole('link', { name: 'Manage usage' })).toHaveCount(0);
    await expectStore(yap, 'config.json', (c) => c.llmScopes.chat.provider === 'openai' && c.llmScopes.chat.model === 'gpt-6-astra');
    expect(yap.readJson('config.json').llmScopes.chat.apiKey).toBe(OPENAI_KEY);

    // Opened again: on ChatGPT; Recent says which way.
    await chip(main).click();
    await expect(tab(main, 'ChatGPT')).toHaveAttribute('aria-selected', 'true');
    await expect(latest(main).getByRole('button', { name: /^GPT-6 Astra/ })).toHaveAttribute('aria-pressed', 'true');
    await tab(main, 'Recent').click();
    await expect(pane(main).locator('.mrow').first()).toContainText('GPT-6 Astra');
    await expect(pane(main).locator('.mrow').first()).toContainText('OpenAI API key · ');
    await main.keyboard.press('Escape');
    await expect(picker(main)).toBeHidden();
  } finally {
    if (await picker(main).isVisible()) await main.keyboard.press('Escape');
    const { openai: _drop, ...keys } = (await yap.invoke('get_config')).ppApiKeys || {};
    await setConfig(yap, main, { ppApiKeys: keys });
    await backToGroq(yap, main, fakeLlm);
  }
});
