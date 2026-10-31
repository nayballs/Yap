// Effort, per model, in Chat's model picker (src-tauri/src/ai/effort.rs,
// src/lib/views/chat/ChatModelPicker.svelte, ModelChip.svelte), as the Claude
// Code app's model menu has it:
// - the chip names the model and its effort ("Claude Opus 5.5 Medium": its
//   default until one is chosen);
// - the picker's Effort shows only the levels the model in use takes (five
//   for Opus 5.5, three for Groq's GPT-OSS, none for a model without a
//   reasoning control or the one on this PC);
// - a level chosen there is on the chip at once, kept for that model alone
//   (config.json `chatEffort`), and sent the provider's way — Claude on
//   Anthropic's Messages API with adaptive thinking, its summary folded
//   above the answer;
// - Fast mode for Claude Opus 5.5: off until switched on, then `speed:
//   "fast"`;
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
const picker = (main) => main.getByRole('dialog', { name: 'Switch the Chat model' });
const effortStrip = (main) => picker(main).getByTestId('chat-effort');
const levels = (main) => effortStrip(main).getByRole('tab');
const fastSwitch = (main) => effortStrip(main).getByRole('button', { name: 'Fast mode' });
const box = (main) => main.getByRole('textbox', { name: 'Message' });
const thread = (main) => main.locator('.msgs');
const thinkButton = (main) => main.getByRole('button', { name: 'Think it through' });

async function openPicker(main) {
  await openView(main, 'Chat');
  await chip(main).click();
  await expect(picker(main)).toBeVisible();
}

async function closePicker(main) {
  await main.keyboard.press('Escape');
  await expect(picker(main)).toBeHidden();
}

async function freshChat(main) {
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
}

async function send(main, text) {
  await box(main).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
}

test('effort: chosen in the picker for the model in use, on the chip, kept per model and sent', async ({ yap, main, fakeLlm, shot }) => {
  await openView(main, 'Chat');
  // Its default until one is chosen: medium on Opus 5.5.
  await expect(chip(main)).toHaveText('Claude Opus 5.5 Medium · Anthropic');
  await openPicker(main);
  await expect(effortStrip(main)).toContainText('Claude Opus 5.5');
  await expect(levels(main)).toHaveText(['Low', 'Medium', 'High', 'Extra', 'Max']);
  await expect(levels(main).filter({ hasText: 'Medium' })).toHaveAttribute('aria-selected', 'true');
  await expect(effortStrip(main).getByTestId('chat-effort-hint')).toHaveText("Medium: balanced, for everyday questions. The model's default.");
  // Fast mode: Opus 5.5's, off until switched on, its price said.
  await expect(fastSwitch(main)).toHaveAttribute('aria-pressed', 'false');
  await expect(effortStrip(main)).toContainText('twice the price');
  await shot(main, '01-effort-default');

  // Extra: on the chip at once, kept for this model.
  await levels(main).filter({ hasText: 'Extra' }).click();
  await expect(levels(main).filter({ hasText: 'Extra' })).toHaveAttribute('aria-selected', 'true');
  await expect(chipEffort(main)).toHaveText('Extra');
  await expect(effortStrip(main).getByTestId('chat-effort-hint')).toHaveText('Extra: thinks longer, for hard problems.');
  await expectStore(yap, 'config.json', (c) => c.chatEffort?.['anthropic:claude-opus-5-5'] === 'xhigh');
  await fastSwitch(main).click();
  await expect(fastSwitch(main)).toHaveAttribute('aria-pressed', 'true');
  await expect(chip(main)).toHaveText('Claude Opus 5.5 Extra · Fast · Anthropic');
  await expectStore(yap, 'config.json', (c) => c.fastMode === true);
  await shot(main, '02-effort-extra-fast');
  await closePicker(main);

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
  await shot(main, '03-answer-at-extra');

  // Per model: Sonnet 5.5 has its own (high, its default), and no fast mode.
  const cfg = await yap.invoke('get_config');
  await setConfig(yap, main, { llmScopes: { ...cfg.llmScopes, chat: claude(fakeLlm, 'claude-sonnet-5-5') } });
  await expect(chip(main)).toHaveText('Claude Sonnet 5.5 High · Anthropic');
  await openPicker(main);
  await expect(levels(main)).toHaveCount(5);
  await expect(fastSwitch(main)).toHaveCount(0);
  await closePicker(main);
  // A Claude with no effort control: no Effort at all.
  await setConfig(yap, main, { llmScopes: { ...cfg.llmScopes, chat: claude(fakeLlm, 'claude-haiku-4-5') } });
  await expect(chip(main)).toHaveText('Claude Haiku 4.5 · Anthropic');
  await openPicker(main);
  await expect(effortStrip(main)).toHaveCount(0);
  await closePicker(main);
  // Back on Opus 5.5: Extra, as it was left.
  await setConfig(yap, main, { llmScopes: { ...cfg.llmScopes, chat: claude(fakeLlm, 'claude-opus-5-5') } });
  await expect(chip(main)).toHaveText('Claude Opus 5.5 Extra · Fast · Anthropic');

  // Back to its default, fast mode off.
  await openPicker(main);
  await levels(main).filter({ hasText: 'Medium' }).click();
  await fastSwitch(main).click();
  await expect(chip(main)).toHaveText('Claude Opus 5.5 Medium · Anthropic');
  await expectStore(yap, 'config.json', (c) => c.chatEffort?.['anthropic:claude-opus-5-5'] === 'medium' && c.fastMode === false);
  await closePicker(main);
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
  await shot(main, '05-settings-effort');
  // Saved by Settings' auto-save, under the model's key; the chip follows.
  await expectStore(yap, 'config.json', (c) => c.chatEffort?.['anthropic:claude-opus-5-5'] === 'high');
  await closeSettings(main);
  await expect(chipEffort(main)).toHaveText('High');
  // Chosen in the picker: Settings has it next time it's opened.
  await openPicker(main);
  await levels(main).filter({ hasText: 'Low' }).click();
  await closePicker(main);
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
    await shot(main, '04-think-on-this-pc');
    await openPicker(main);
    await expect(effortStrip(main)).toHaveCount(0);
    await closePicker(main);
  } finally {
    await setConfig(yap, main, { llmScopes: { ...cfg.llmScopes, chat: claude(fakeLlm, 'claude-opus-5-5') } });
  }
  await freshChat(main);
  await expect(chip(main)).toContainText('Claude Opus 5.5');
  await expect(thinkButton(main)).toHaveCount(0);
});
