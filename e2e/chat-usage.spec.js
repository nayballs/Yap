// The AI usage tracker (src-tauri/src/ai/usage.rs) and Chat's context ring
// (src/lib/views/chat/ContextRing.svelte, ContextPanel.svelte; Insights'
// AiUsageCard.svelte), 2026-10-25:
// - Every AI call's tokens are counted from the answer's own `usage` (and
//   llama.cpp's `timings`), streamed or whole: a dictation's cleanup and a
//   Chat answer land in Insights → AI usage — today's total, by model with
//   an estimated cost (the fake stands in for api.groq.com through the
//   debug-only YAP_E2E_NET_ALIASES, so it counts as Groq's cloud), and by
//   feature — and Groq's "Usage today" meter in Settings still counts them.
// - The ring under Chat's message box shows the conversation's last answer
//   (prompt + answer) against the Chat model's context from Groq's model
//   list (a small 8K context on the stand-in GPT-OSS 20B): "39% of context
//   used" on hover; its panel's rows (context, the last answer and its
//   speed, today's and the week's use by model); past 80 % the quiet line
//   with New chat; a model switch in the picker changes the denominator;
//   New chat resets it; See all opens Insights' card.
// Nothing leaves the PC: every provider is the local fake (support/fake-llm.js)
// behind a test key. Fictional details only.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, openSettings, closeSettings, expectStore } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';

const TEST_KEY = 'gsk_FAKEusage0123456789abcXYZ';

/** A model as Groq's /openai/v1/models lists one (trimmed). */
const groqModel = (id, name, context) => ({
  id,
  object: 'model',
  created: 1754407957,
  owned_by: 'Fake',
  active: true,
  context_window: context,
  name,
  input_modalities: ['text'],
  output_modalities: ['text'],
  supported_features: ['tools', 'json_mode'],
});

const test = base.extend({
  fakeLlm: [
    async ({}, use) => {
      const fake = await startFakeLlm();
      // A small context on the stand-in GPT-OSS 20B, so a chat fills it.
      fake.listModels('groq', [groqModel('openai/gpt-oss-20b', 'GPT OSS 20B', 8192), groqModel('openai/gpt-oss-120b', 'GPT OSS 120B', 131072)]);
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({ fakeLlm }, use) => {
      await use({
        name: 'chat-usage',
        env: { YAP_MODELS_STANDIN: fakeLlm.modelsStandin, YAP_E2E_NET_ALIASES: `${new URL(fakeLlm.base).host}=api.groq.com` },
        config: {
          postProcessEnabled: true,
          ppProvider: 'groq',
          ppBaseUrl: fakeLlm.base,
          ppModel: 'openai/gpt-oss-20b',
          ppApiKey: TEST_KEY,
          ppApiKeys: { groq: TEST_KEY },
          llmScopes: {
            chat: { enabled: true, provider: 'groq', baseUrl: fakeLlm.base, model: 'openai/gpt-oss-20b', apiKey: TEST_KEY, apiKeys: {}, prompt: '', disableThinking: false },
          },
        },
      });
    },
    { scope: 'worker' },
  ],
});

const ring = (main) => main.getByTestId('context-ring');
const panel = (main) => main.getByRole('dialog', { name: 'Context and AI usage' });
const chip = (main) => main.locator('.modelchip .chip');
const lastAnswer = (main) => main.locator('.cbubble.assistant').last();

async function send(main, text) {
  await main.getByRole('textbox', { name: 'Message' }).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
}

/** The answers' counts, as the fake sends them: `{ usage, timings }`. */
const counts = (prompt, answer, timings) => ({
  usage: { prompt_tokens: prompt, completion_tokens: answer, total_tokens: prompt + answer },
  ...(timings ? { timings } : {}),
});

async function pickTheme(main, yap, label) {
  const dialog = await openSettings(main, 'General');
  const group = dialog.getByRole('group', { name: 'Appearance' });
  await group.getByRole('tablist', { name: 'Theme' }).getByRole('tab', { name: label, exact: true }).click();
  await expectStore(yap, 'config.json', (c) => c.theme === label.toLowerCase(), `the theme ${label} to be saved`);
  await closeSettings(main);
}

test('a cleanup and a chat answer are counted: Insights → AI usage, and Groq’s meter still right', async ({ yap, main, fakeLlm, shot }) => {
  // A dictation's cleanup (whole) and a Chat answer (streamed), each with
  // the provider's own counts.
  fakeLlm.usage('cleanup', counts(400, 20));
  fakeLlm.usage('other', counts(1000, 100));
  await yap.invoke('e2e_dictation', { text: 'remind me to book the venue for the offsite' });
  await expect.poll(() => fakeLlm.requests.filter((r) => r.kind === 'cleanup').length).toBe(1);

  await openView(main, 'Chat');
  await send(main, 'What is on my list for the offsite?');
  await expect(lastAnswer(main)).toContainText('OK');
  // Streamed, with the usage asked for in the stream.
  const asked = fakeLlm.requests.filter((r) => r.kind === 'other').at(-1);
  expect(asked.stream).toBe(true);
  expect(asked.streamOptions).toEqual({ include_usage: true });

  // Counted on this PC: today's summary has both, by feature, at Groq's price.
  await expect
    .poll(async () => (await yap.invoke('ai_usage_summary')).today.tokens, { message: 'both calls counted' })
    .toBe(1_520);
  const sum = await yap.invoke('ai_usage_summary');
  expect(sum.today.features.map((f) => [f.feature, f.tokens])).toEqual([
    ['chat', 1_100],
    ['cleanup', 420],
  ]);
  expect(sum.today.models).toHaveLength(1);
  expect(sum.today.models[0]).toMatchObject({ provider: 'groq', model: 'openai/gpt-oss-20b', here: false, calls: 2 });
  // 1,000 × $0.075/M + 100 × $0.30/M = 105 millionths, and 400 / 20 → 30 + 6.
  expect(sum.today.models[0].costMicros).toBe(141);

  // Insights → AI usage.
  await openView(main, 'Insights');
  const card = main.getByTestId('ai-usage-card');
  await expect(card).toBeVisible();
  await expect(card.getByTestId('ai-usage-total-today')).toContainText('1.5K');
  await expect(card.getByTestId('ai-usage-total-today')).toContainText('tokens · today');
  await expect(card).toContainText('Nothing leaves this PC.');
  await card.getByRole('button', { name: 'Today' }).click();
  const models = card.getByTestId('ai-usage-models');
  await expect(models).toContainText('GPT-OSS 20B');
  await expect(models).toContainText('Groq');
  await expect(models).toContainText('≈ $0.0001');
  const features = card.getByTestId('ai-usage-features');
  await expect(features.locator('.uname')).toHaveText(['Chat', 'AI cleanup']);
  await card.scrollIntoViewIfNeeded();
  await shot(main, '01-insights-ai-usage');

  // ai_usage.json: counts only, never the words or the key.
  await expectStore(yap, 'ai_usage.json', (s) => Object.values(s.days).flat().some((r) => r.feature === 'cleanup' && r.prompt === 400));
  const stored = fs.readFileSync(path.join(yap.dataDir, 'ai_usage.json'), 'utf8');
  for (const leak of [TEST_KEY, 'venue', 'offsite', 'OK']) expect(stored).not.toContain(leak);

  // Groq's "Usage today" meter (groq_usage.json), as before: both calls.
  const dialog = await openSettings(main, 'Language Models');
  const meter = dialog.locator('#settings-usage');
  await expect(meter).toContainText('1.5k / 200.0k');
  await expect(meter).toContainText('2 / 1000');
  await closeSettings(main);
  const groq = yap.readJson('groq_usage.json');
  expect(groq).toMatchObject({ tokens: 1_520, requests: 2 });
  expect(groq.providers.groq).toEqual({ tokens: 1_520, requests: 2 });
});

test('the context ring: the chat’s share of the model’s context, its panel, a model switch and New chat', async ({ yap, main, fakeLlm, shot }) => {
  // Groq's list (through the stand-in): GPT-OSS 20B with an 8K context (4K
  // until 31 Oct 2026: Chat's own prompt and tools no longer leave room in 4K).
  await yap.invoke('llm_models', { provider: 'groq', scope: 'chat', force: true });
  await openView(main, 'Chat');
  await main.locator('.convs').getByRole('button', { name: 'New chat', exact: true }).click();
  await expect(ring(main)).toHaveCount(0);

  // An answer: 2,400 prompt + 800 answer tokens of 8,192 = 39 %, written at
  // 120 tokens a second (llama.cpp's timings).
  fakeLlm.usage('other', counts(2_400, 800, { prompt_n: 2_400, prompt_ms: 1_200, predicted_n: 800, predicted_ms: 6_666.7, predicted_per_second: 120 }));
  await send(main, 'When is the launch?');
  await expect(lastAnswer(main)).toContainText('OK');
  await expect(ring(main)).toHaveAttribute('data-percent', '39');
  const button = ring(main).getByRole('button', { name: '39% of context used' });
  await expect(button).toBeVisible();
  await button.hover();
  await expect(main.getByTestId('context-tip')).toHaveText('39% of context used');
  await shot(main, '02-ring-hover');

  // Its panel: this chat, then today's and the week's use by model.
  await button.click();
  const p = panel(main);
  await expect(p).toBeVisible();
  await expect(p.getByTestId('context-used')).toHaveText('3.2K of 8K tokens');
  await expect(p.getByRole('meter', { name: 'Context used' })).toHaveAttribute('aria-valuenow', '39');
  await expect(p.getByTestId('context-last-answer')).toHaveText('800 tokens · 120 tokens/s');
  await expect(p.getByTestId('context-long')).toHaveCount(0);
  await expect(p.getByTestId('usage-today')).toContainText('GPT-OSS 20B');
  await expect(p.getByTestId('usage-today')).toContainText('· Groq');
  await expect(p.getByTestId('usage-today')).toContainText('≈ $');
  await expect(p.getByTestId('usage-week')).toBeVisible();
  await expect(p).toContainText('Counted on this PC: nothing leaves it.');
  await shot(main, '03-ring-panel');
  await main.keyboard.press('Escape');
  await expect(p).toBeHidden();
  await expect(button).toBeFocused();

  // Past 80 %: 7,000 + 400 of 8,192 = 90 %, warmer, and the quiet line.
  fakeLlm.usage('other', counts(7_000, 400));
  await send(main, 'And the week after?');
  await expect(ring(main)).toHaveAttribute('data-percent', '90');
  await ring(main).getByRole('button').click();
  const long = panel(main).getByTestId('context-long');
  await expect(long).toContainText('Nearly full: a long answer may be cut short. A new chat answers best.');
  await expect(long.getByRole('button', { name: 'New chat' })).toBeVisible();
  await shot(main, '04-ring-full-light');
  await main.keyboard.press('Escape');

  // Dark (Settings over Chat: the conversation stays open).
  await pickTheme(main, yap, 'Dark');
  await ring(main).getByRole('button').click();
  await expect(panel(main).getByTestId('context-long')).toBeVisible();
  await shot(main, '05-ring-full-dark');
  await main.keyboard.press('Escape');
  await pickTheme(main, yap, 'System');

  // A switch in the model menu to GPT-OSS 120B (128K): the same chat is 6 %.
  const picker = main.getByRole('menu', { name: 'Chat model' });
  await chip(main).click();
  await picker.getByRole('menuitemradio', { name: /^GPT-OSS 120B/ }).click();
  await expect(chip(main)).toHaveText('GPT-OSS 120B Medium · Groq');
  await expect(ring(main)).toHaveAttribute('data-percent', '6');
  await expect(ring(main).getByRole('button', { name: '6% of context used' })).toBeVisible();

  // Back on the 20B it's 90 % again; the line's New chat starts afresh: no ring.
  if (await picker.isVisible()) await main.keyboard.press('Escape');
  await chip(main).click();
  await picker.getByRole('menuitemradio', { name: /^GPT-OSS 20B/ }).click();
  await expect(chip(main)).toHaveText('GPT-OSS 20B Medium · Groq');
  await expect(ring(main)).toHaveAttribute('data-percent', '90');
  if (await picker.isVisible()) await main.keyboard.press('Escape');
  await ring(main).getByRole('button').click();
  await panel(main).getByTestId('context-long').getByRole('button', { name: 'New chat' }).click();
  await expect(ring(main)).toHaveCount(0);
  await expect(main.locator('.cbubble')).toHaveCount(0);

  // The new chat's first answer: 5,200 + 42 of 8,192 = 64 %.
  fakeLlm.usage('other', counts(5_200, 42));
  await send(main, 'Who is coming?');
  await expect(lastAnswer(main)).toContainText('OK');
  await expect(ring(main)).toHaveAttribute('data-percent', '64');

  // See all: Insights' AI usage card, in view.
  await ring(main).getByRole('button').click();
  await panel(main).getByRole('button', { name: 'See all' }).click();
  await expect(main.getByRole('navigation', { name: 'Main' }).getByRole('button', { name: 'Insights', exact: true })).toHaveAttribute('aria-current', 'page');
  await expect(main.getByTestId('ai-usage-card')).toBeInViewport();
  await shot(main, '06-see-all-insights');
  fakeLlm.usage('other', null);
});
