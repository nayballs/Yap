// Chat's model menu with Claude (src/lib/views/chat/ChatModelMenu.svelte,
// core/mainModels.js, src-tauri/src/ai/effort.rs, ModelChip.svelte), as the
// Claude Code app's model menu has it ("Opus 5.5 ✓, Fable 5.1 2, Sonnet 5.5
// 3, Haiku 5.5 4, Effort ▸ Extra, More models ▸, Fast mode"):
// - the main models: Anthropic's newest Opus, Fable, Sonnet and Haiku, then
//   the on-device model, each with its number key; the older Claudes under
//   More models; picked by a click, a number key or in More models;
// - the chip names the model and its effort ("Claude Opus 5.5 Medium": its
//   default until one is chosen);
// - Effort ▸ shows only the levels the model in use takes (five for Opus
//   5.5, none for a model without a reasoning control or the one on this
//   PC), the one in effect ticked and a line on what each does;
// - a level chosen there is on the chip at once, kept for that model alone
//   (config.json `chatEffort`), and sent the provider's way — Claude on
//   Anthropic's Messages API with adaptive thinking, its summary folded
//   above the answer;
// - Fast mode for Claude Opus 5.5 only: off until switched on, then `speed:
//   "fast"`;
// - the menu in light and dark;
// - Settings → Language Models → Chat has the same choice, and the two
//   agree;
// - Think is gone for a cloud model and there for the on-device one.
// The models are a local fake (support/fake-llm.js, speaking Anthropic's
// Messages API too); the on-device model an empty stand-in started as the
// fake (support/local-llm.js). Nothing goes on the internet.
import { test as base, expect, openView, openSettings, closeSettings, setConfig, expectStore } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { installStandInModel } from './support/local-llm.js';

const CLAUDE_KEY = 'sk-ant-e2e-fake-1234567890';
const QWEN = 'Qwen_Qwen3.5-4B-Q4_K_M.gguf';
const ON_DEVICE_CHAT = { enabled: true, provider: 'ondevice', baseUrl: '', model: '', apiKey: '', apiKeys: {}, prompt: '', disableThinking: false };

function claude(fakeLlm, model) {
  return { enabled: true, provider: 'anthropic', baseUrl: fakeLlm.base, model, apiKey: CLAUDE_KEY, apiKeys: {}, prompt: '', disableThinking: false };
}

const test = base.extend({
  fakeLlm: [
    async ({}, use) => {
      const fake = await startFakeLlm();
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({ fakeLlm }, use) => {
      await use({
        name: 'chat-effort',
        env: { YAP_E2E_GPU: 'NVIDIA GeForce RTX 4070,12282', YAP_E2E_LOCAL_LLM: fakeLlm.base, YAP_E2E_LOCAL_LLM_LOAD_MS: '200' },
        config: {
          postProcessEnabled: false,
          ppProvider: 'ondevice',
          ppLocalModel: QWEN,
          ppApiKeys: { anthropic: CLAUDE_KEY },
          llmScopes: { chat: claude(fakeLlm, 'claude-opus-5-5') },
          meetingReminder: 'never',
        },
        prepare: (dir) => installStandInModel(dir, QWEN),
      });
    },
    { scope: 'worker' },
  ],
});

const chip = (main) => main.locator('.modelchip .chip');
const chipEffort = (main) => main.getByTestId('chip-effort');
const menu = (main) => main.getByRole('menu', { name: 'Chat model' });
const models = (main) => menu(main).getByRole('menuitemradio');
const effortItem = (main) => menu(main).getByRole('menuitem', { name: /^Effort/ });
const effortMenu = (main) => main.getByRole('menu', { name: 'Effort' });
const levels = (main) => effortMenu(main).getByRole('menuitemradio');
const level = (main, name) => effortMenu(main).getByRole('menuitemradio', { name, exact: true });
const fastSwitch = (main) => menu(main).getByRole('menuitemcheckbox', { name: 'Enable fast mode' });
const moreMenu = (main) => main.getByRole('menu', { name: 'More models' });
const box = (main) => main.getByRole('textbox', { name: 'Message' });
const thread = (main) => main.locator('.msgs');
const thinkButton = (main) => main.getByRole('button', { name: 'Think it through' });

async function openMenu(main) {
  await openView(main, 'Chat');
  await chip(main).click();
  await expect(menu(main)).toBeVisible();
}

/** Esc out of a submenu, then the menu. */
async function closeMenu(main) {
  for (let i = 0; i < 3 && (await menu(main).isVisible()); i++) await main.keyboard.press('Escape');
  await expect(menu(main)).toBeHidden();
}

async function openEffort(main) {
  await effortItem(main).click();
  await expect(effortMenu(main)).toBeVisible();
}

async function freshChat(main) {
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
}

async function send(main, text) {
  await box(main).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
}

async function pickTheme(main, yap, label) {
  const dialog = await openSettings(main, 'General');
  const group = dialog.getByRole('group', { name: 'Appearance' });
  await group.getByRole('tablist', { name: 'Theme' }).getByRole('tab', { name: label, exact: true }).click();
  await expectStore(yap, 'config.json', (c) => c.theme === label.toLowerCase(), `the theme ${label} to be saved`);
  await closeSettings(main);
}

test('the menu: the newest Claudes with their numbers, the one in use ticked, the older ones under More models', async ({ yap, main, fakeLlm, shot }) => {
  await openView(main, 'Chat');
  await expect(chip(main)).toHaveText('Claude Opus 5.5 Medium · Anthropic');
  await openMenu(main);
  // As Claude Code lists them, then the model on this PC.
  await expect(models(main).locator('.nm')).toHaveText(['Claude Opus 5.5', 'Claude Fable 5.1', 'Claude Sonnet 5.5', 'Claude Haiku 5.5', 'Qwen3.5 4B']);
  await expect(models(main).locator('.where')).toHaveText(['Anthropic', 'Anthropic', 'Anthropic', 'Anthropic', 'On this PC']);
  await expect(models(main).first()).toHaveAttribute('aria-checked', 'true');
  await expect(models(main).first().locator('.tick')).toBeVisible();
  await expect(models(main).locator('kbd')).toHaveText(['2', '3', '4', '5']);
  await expect(menu(main).getByTestId('menu-effort')).toHaveText('Medium');
  await expect(fastSwitch(main)).toHaveAttribute('aria-checked', 'false');
  await expect(fastSwitch(main)).toContainText('twice the price per token');
  await shot(main, '01-menu-light');

  // More models: the older Claudes, newest first.
  await menu(main).getByRole('menuitem', { name: 'More models' }).hover();
  const older = moreMenu(main).getByRole('group', { name: 'Anthropic' }).getByRole('menuitemradio');
  await expect(older.locator('.nm')).toHaveText([
    'Claude Opus 4.8',
    'Claude Opus 4.7',
    'Claude Sonnet 4.6',
    'Claude Opus 4.6',
    'Claude Haiku 4.5',
    'Claude Opus 4.5',
  ]);
  await shot(main, '02-menu-more-models');

  // Number keys: 3 is Sonnet 5.5 (its own effort, high by default).
  await models(main).first().focus();
  await main.keyboard.press('3');
  await expect(menu(main)).toBeHidden();
  await expect(chip(main)).toHaveText('Claude Sonnet 5.5 High · Anthropic');
  await expectStore(yap, 'config.json', (c) => c.llmScopes.chat.model === 'claude-sonnet-5-5');
  // Same provider: still the test's own endpoint.
  expect(yap.readJson('config.json').llmScopes.chat.baseUrl).toBe(fakeLlm.base);

  // One from More models: Haiku 4.5 has no effort control, so no Effort.
  await openMenu(main);
  await menu(main).getByRole('menuitem', { name: 'More models' }).click();
  await moreMenu(main).getByRole('menuitemradio', { name: /^Claude Haiku 4\.5/ }).click();
  await expect(menu(main)).toBeHidden();
  await expect(chip(main)).toHaveText('Claude Haiku 4.5 · Anthropic');
  await openMenu(main);
  await expect(effortItem(main)).toHaveCount(0);
  await expect(fastSwitch(main)).toHaveCount(0);
  // In use: last, ticked; the others keep their numbers.
  await expect(models(main).locator('.nm')).toHaveText([
    'Claude Opus 5.5',
    'Claude Fable 5.1',
    'Claude Sonnet 5.5',
    'Claude Haiku 5.5',
    'Qwen3.5 4B',
    'Claude Haiku 4.5',
  ]);
  await expect(models(main).last()).toHaveAttribute('aria-checked', 'true');
  // 1: back on Opus 5.5.
  await main.keyboard.press('1');
  await expect(chip(main)).toHaveText('Claude Opus 5.5 Medium · Anthropic');
  await expectStore(yap, 'config.json', (c) => c.llmScopes.chat.model === 'claude-opus-5-5');
});

test('effort: chosen in the menu for the model in use, on the chip, kept per model and sent', async ({ yap, main, fakeLlm, shot }) => {
  await openView(main, 'Chat');
  // Its default until one is chosen: medium on Opus 5.5.
  await expect(chip(main)).toHaveText('Claude Opus 5.5 Medium · Anthropic');
  await openMenu(main);
  await openEffort(main);
  await expect(levels(main)).toHaveText(['Low', 'Medium', 'High', 'Extra', 'Max']);
  await expect(level(main, 'Medium')).toHaveAttribute('aria-checked', 'true');
  await expect(level(main, 'Medium')).toBeFocused();
  await expect(effortMenu(main).getByTestId('chat-effort-hint')).toHaveText("Medium: balanced, for everyday questions. The model's default.");
  // The line follows the level in focus.
  await main.keyboard.press('ArrowDown');
  await expect(effortMenu(main).getByTestId('chat-effort-hint')).toHaveText('High: thinks more, for detailed work.');
  await shot(main, '03-effort-submenu');

  // Extra: the menu closes, the chip says it at once, kept for this model.
  await level(main, 'Extra').click();
  await expect(menu(main)).toBeHidden();
  await expect(chipEffort(main)).toHaveText('Extra');
  await expectStore(yap, 'config.json', (c) => c.chatEffort?.['anthropic:claude-opus-5-5'] === 'xhigh');
  // Fast mode: Opus 5.5's, off until switched on; the menu stays open.
  await openMenu(main);
  await expect(menu(main).getByTestId('menu-effort')).toHaveText('Extra');
  await fastSwitch(main).click();
  await expect(fastSwitch(main)).toHaveAttribute('aria-checked', 'true');
  await expect(menu(main)).toBeVisible();
  await expect(chip(main)).toHaveText('Claude Opus 5.5 Extra · Fast · Anthropic');
  await expectStore(yap, 'config.json', (c) => c.fastMode === true);
  await shot(main, '04-effort-extra-fast');
  await closeMenu(main);

  // Asked at Extra: adaptive thinking, its summary folded above the answer,
  // room to think, fast. No Think button for a cloud model.
  await freshChat(main);
  await expect(thinkButton(main)).toHaveCount(0);
  const before = fakeLlm.requests.length;
  fakeLlm.scriptChat([{ reasoning: 'Three rooms, then the hall: four coats in all.', content: 'Four coats of paint.', plain: true }]);
  await send(main, 'How many coats of paint for the hall and three rooms?');
  await expect(thread(main)).toContainText('Four coats of paint.', { timeout: 15_000 });
  const asked = fakeLlm.requests.slice(before).find((r) => r.api === 'messages' && r.user?.includes('coats of paint'));
  expect(asked.think.outputConfig).toEqual({ effort: 'xhigh' });
  expect(asked.think.thinking).toEqual({ type: 'adaptive', display: 'summarized' });
  expect(asked.maxTokens).toBe(64000);
  expect(asked.body.speed).toBe('fast');
  const thought = main.getByTestId('chat-thought').last();
  await expect(thought).toContainText(/Thought/);
  await thought.locator('summary').click();
  await expect(thought).toContainText('four coats in all');
  await shot(main, '05-answer-at-extra');

  // Per model: Sonnet 5.5 has its own (high, its default), and no fast mode.
  const cfg = await yap.invoke('get_config');
  await setConfig(yap, main, { llmScopes: { ...cfg.llmScopes, chat: claude(fakeLlm, 'claude-sonnet-5-5') } });
  await expect(chip(main)).toHaveText('Claude Sonnet 5.5 High · Anthropic');
  await openMenu(main);
  await expect(fastSwitch(main)).toHaveCount(0);
  await expect(menu(main).getByText('Fast mode', { exact: true })).toHaveCount(0);
  await openEffort(main);
  await expect(levels(main)).toHaveCount(5);
  await expect(level(main, 'High')).toHaveAttribute('aria-checked', 'true');
  await closeMenu(main);
  // Back on Opus 5.5: Extra, as it was left.
  await setConfig(yap, main, { llmScopes: { ...cfg.llmScopes, chat: claude(fakeLlm, 'claude-opus-5-5') } });
  await expect(chip(main)).toHaveText('Claude Opus 5.5 Extra · Fast · Anthropic');

  // Back to its default, fast mode off.
  await openMenu(main);
  await openEffort(main);
  await level(main, 'Medium').click();
  await expect(menu(main)).toBeHidden();
  await openMenu(main);
  await fastSwitch(main).click();
  await expect(chip(main)).toHaveText('Claude Opus 5.5 Medium · Anthropic');
  await expectStore(yap, 'config.json', (c) => c.chatEffort?.['anthropic:claude-opus-5-5'] === 'medium' && c.fastMode === false);
  await closeMenu(main);
});

test('the menu in dark', async ({ yap, main, shot }) => {
  await pickTheme(main, yap, 'Dark');
  try {
    await openMenu(main);
    // The warm dark theme's surface, from its tokens.
    const [r, g, b] = (await menu(main).evaluate((el) => getComputedStyle(el).backgroundColor)).match(/\d+/g).map(Number);
    expect(r + g + b).toBeLessThan(240);
    await shot(main, '06-menu-dark');
    await openEffort(main);
    await shot(main, '07-effort-submenu-dark');
    await closeMenu(main);
    await openMenu(main);
    await menu(main).getByRole('menuitem', { name: 'More models' }).hover();
    await expect(moreMenu(main)).toBeVisible();
    await shot(main, '08-more-models-dark');
    await closeMenu(main);
  } finally {
    await pickTheme(main, yap, 'System');
  }
});

test('Settings → Language Models → Chat has the same effort for the Chat model', async ({ yap, main, shot }) => {
  await openView(main, 'Chat');
  await expect(chipEffort(main)).toHaveText('Medium');
  const dialog = await openSettings(main, 'Language Models');
  await dialog.getByRole('tab', { name: 'Chat & Voice Assistant' }).click();
  const choice = dialog.getByRole('tablist', { name: 'Effort' });
  await expect(choice.getByRole('tab')).toHaveText(['Low', 'Medium', 'High', 'Extra', 'Max']);
  await expect(choice.getByRole('tab', { name: 'Medium' })).toHaveAttribute('aria-selected', 'true');
  await choice.getByRole('tab', { name: 'High' }).click();
  await expect(dialog.getByTestId('settings-effort-hint')).toHaveText('High: thinks more, for detailed work.');
  await shot(main, '09-settings-effort');
  // Saved by Settings' auto-save, under the model's key; the chip follows.
  await expectStore(yap, 'config.json', (c) => c.chatEffort?.['anthropic:claude-opus-5-5'] === 'high');
  await closeSettings(main);
  await expect(chipEffort(main)).toHaveText('High');
  // Chosen in the menu: Settings has it next time it's opened.
  await openMenu(main);
  await openEffort(main);
  await level(main, 'Low').click();
  await expect(menu(main)).toBeHidden();
  const again = await openSettings(main, 'Language Models');
  await again.getByRole('tab', { name: 'Chat & Voice Assistant' }).click();
  await expect(again.getByRole('tablist', { name: 'Effort' }).getByRole('tab', { name: 'Low' })).toHaveAttribute('aria-selected', 'true');
  await again.getByRole('tablist', { name: 'Effort' }).getByRole('tab', { name: 'Medium' }).click();
  await expectStore(yap, 'config.json', (c) => c.chatEffort?.['anthropic:claude-opus-5-5'] === 'medium');
  await closeSettings(main);
});

test('Think: hidden for a cloud model, shown for the one on this PC, which has no effort', async ({ yap, main, fakeLlm, shot }) => {
  await freshChat(main);
  await expect(chip(main)).toContainText('Claude Opus 5.5');
  await expect(box(main)).toBeVisible();
  await expect(thinkButton(main)).toHaveCount(0);

  const cfg = await yap.invoke('get_config');
  await setConfig(yap, main, { llmScopes: { ...cfg.llmScopes, chat: ON_DEVICE_CHAT } });
  try {
    await freshChat(main);
    await expect(chip(main)).toHaveText('Qwen3.5 4B · On this PC');
    await expect(thinkButton(main)).toBeVisible({ timeout: 15_000 });
    await shot(main, '10-think-on-this-pc');
    // The menu: the model on this PC ticked, no Effort, no Fast mode.
    await openMenu(main);
    await expect(menu(main).getByRole('menuitemradio', { name: /^Qwen3\.5 4B/ })).toHaveAttribute('aria-checked', 'true');
    await expect(effortItem(main)).toHaveCount(0);
    await expect(fastSwitch(main)).toHaveCount(0);
    await closeMenu(main);
  } finally {
    await setConfig(yap, main, { llmScopes: { ...cfg.llmScopes, chat: claude(fakeLlm, 'claude-opus-5-5') } });
  }
  await freshChat(main);
  await expect(chip(main)).toContainText('Claude Opus 5.5');
  await expect(thinkButton(main)).toHaveCount(0);
});
