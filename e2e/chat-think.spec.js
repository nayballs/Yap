// Think it through (src-tauri/src/chat/think.rs, Chat's Think button): one
// question, thought through by the model on this PC.
// - The button shows for the on-device Qwen3.5 only; a cloud model has its
//   effort instead (ai/effort.rs, the model picker: chat-effort.spec.js).
// - The question asks the sidecar its own way (`chat_template_kwargs`, a
//   thinking budget); the next one, without Think, asks nothing of the kind,
//   and the model's reasoning sent unasked shows none of it.
// - The thinking shows apart from the answer while it streams ("Thinking…",
//   folded), then folded above the answer ("Thought for 2 s"), and is kept
//   with it in chats.json — never in what's sent back to the model.
// - A cloud model (Groq's GPT-OSS) answers at its effort: no Think, its
//   reasoning folded above the answer all the same.
// The on-device model is an empty stand-in (support/local-llm.js), the
// started model and "Groq" a local fake (support/fake-llm.js); the real
// Qwen's fields were measured on llamafile (think.rs).
import { test as base, expect, openView, expectStore, setConfig } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { installStandInModel } from './support/local-llm.js';

const TEST_KEY = 'e2e-fake-key';
const QWEN = 'Qwen_Qwen3.5-4B-Q4_K_M.gguf';
const ON_DEVICE_CHAT = { enabled: true, provider: 'ondevice', baseUrl: '', model: '', apiKey: '', apiKeys: {}, prompt: '', disableThinking: false };

function groqChat(fakeLlm, model) {
  return { enabled: true, provider: 'groq', baseUrl: fakeLlm.base, model, apiKey: TEST_KEY, apiKeys: {}, prompt: '', disableThinking: false };
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
        name: 'chat-think',
        env: { YAP_E2E_GPU: 'NVIDIA GeForce RTX 4070,12282', YAP_E2E_LOCAL_LLM: fakeLlm.base, YAP_E2E_LOCAL_LLM_LOAD_MS: '200' },
        config: {
          postProcessEnabled: false,
          ppProvider: 'ondevice',
          ppLocalModel: QWEN,
          ppApiKeys: { groq: TEST_KEY },
          llmScopes: { chat: ON_DEVICE_CHAT },
          meetingReminder: 'never',
        },
        prepare: (dir) => installStandInModel(dir, QWEN),
      });
    },
    { scope: 'worker' },
  ],
});

const box = (main) => main.getByRole('textbox', { name: 'Message' });
const thread = (main) => main.locator('.msgs');
const thinkButton = (main) => main.getByRole('button', { name: 'Think it through' });

async function freshChat(main) {
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
}

async function send(main, text) {
  await box(main).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
}

test('Think: one question thought through on this PC, its thinking folded above the answer and kept', async ({ yap, main, fakeLlm, shot }) => {
  await freshChat(main);
  await expect(thinkButton(main)).toBeVisible({ timeout: 15_000 });
  await expect(thinkButton(main)).toHaveAttribute('aria-pressed', 'false');
  await thinkButton(main).click();
  await expect(thinkButton(main)).toHaveAttribute('aria-pressed', 'true');
  await shot(main, '01-think-on');

  // The answer thinks first (slowly, so "Thinking…" shows with its thoughts).
  const before = fakeLlm.requests.length;
  fakeLlm.scriptChat([
    {
      reasoning: 'The bat costs a pound more than the ball. Together they cost 1.10, so the ball is 5p.',
      content: 'The ball costs 5p.',
      stream: { gapMs: 120 },
      plain: true,
    },
  ]);
  await send(main, 'A bat and a ball cost £1.10. The bat costs £1 more. How much is the ball?');
  await expect(main.getByTestId('chat-thinking')).toBeVisible({ timeout: 15_000 });
  await expect(main.getByTestId('chat-thinking')).toContainText('The bat costs');
  await shot(main, '02-thinking');
  await expect(thread(main)).toContainText('The ball costs 5p.', { timeout: 15_000 });

  // Asked the sidecar's way: thinking on for this request, to a budget.
  const asked = fakeLlm.requests.slice(before).find((r) => r.user?.includes('How much is the ball?'));
  expect(asked.think.chatTemplateKwargs).toEqual({ enable_thinking: true });
  expect(asked.think.thinkingBudget).toBe(1500);
  expect(asked.think.reasoningEffort).toBeNull();
  // The button's off again: for that question only.
  await expect(thinkButton(main)).toHaveAttribute('aria-pressed', 'false');

  // Folded above the answer, its time said; the thinking kept with it.
  const thought = main.getByTestId('chat-thought').last();
  await expect(thought).toContainText(/Thought (for \d+ s|it through)/);
  await thought.locator('summary').click();
  await expect(thought).toContainText('Together they cost 1.10');
  await shot(main, '03-thought-folded');
  await expectStore(yap, 'chats.json', (s) =>
    s.conversations.some((c) => c.messages.some((m) => m.role === 'assistant' && m.text === 'The ball costs 5p.' && m.thinking?.text?.includes('so the ball is 5p')))
  );

  // The next question: no thinking asked for, and the earlier thinking never
  // sent. Reasoning sent anyway, unasked: none of it shows.
  await main.evaluate(() => {
    window.__sawThinking = false;
    window.__thinkingWatch?.disconnect();
    window.__thinkingWatch = new MutationObserver(() => {
      if (document.querySelector('[data-testid="chat-thinking"]') || document.querySelectorAll('[data-testid="chat-thought"]').length > 1) {
        window.__sawThinking = true;
      }
    });
    window.__thinkingWatch.observe(document.body, { subtree: true, childList: true, characterData: true });
  });
  const next = fakeLlm.requests.length;
  fakeLlm.scriptChat([{ reasoning: 'They want the sum checked again: 5p and £1.05 make £1.10, so yes.', content: 'Yes.', stream: { gapMs: 120 }, plain: true }]);
  await send(main, 'Are you sure?');
  await expect(thread(main)).toContainText('Yes.', { timeout: 15_000 });
  expect(await main.evaluate(() => window.__sawThinking)).toBe(false);
  await main.evaluate(() => window.__thinkingWatch?.disconnect());
  const again = fakeLlm.requests.slice(next).find((r) => r.user?.includes('Are you sure?'));
  expect(again.think.chatTemplateKwargs).toBeNull();
  expect(JSON.stringify(again.messages)).not.toContain('so the ball is 5p');

  // Reopened later: still folded above it.
  await openView(main, 'Home');
  await openView(main, 'Chat');
  await main.locator('.conv', { hasText: 'A bat and a ball' }).first().click();
  await expect(main.getByTestId('chat-thought').first()).toContainText(/Thought/);
});

test('a cloud model has no Think button: it answers at its effort, its reasoning folded above the answer', async ({ yap, main, fakeLlm }) => {
  await setConfig(yap, main, { llmScopes: { chat: groqChat(fakeLlm, 'openai/gpt-oss-20b') } });
  try {
    await freshChat(main);
    await expect(main.getByTestId('chip-effort')).toHaveText('Medium');
    await expect(thinkButton(main)).toHaveCount(0);
    const before = fakeLlm.requests.length;
    fakeLlm.scriptChat([{ reasoning: 'The user wants a quick sum: 2 and 2 is 4.', content: 'Four.', stream: { gapMs: 60 } }]);
    await send(main, 'What is two and two?');
    await expect(thread(main)).toContainText('Four.', { timeout: 15_000 });
    const asked = fakeLlm.requests.slice(before).find((r) => r.user?.includes('two and two'));
    expect(asked.think.reasoningEffort).toBe('medium');
    expect(asked.think.chatTemplateKwargs).toBeNull();
    const thought = main.getByTestId('chat-thought').last();
    await expect(thought).toContainText(/Thought/);
    await thought.locator('summary').click();
    await expect(thought).toContainText('2 and 2 is 4');
  } finally {
    await setConfig(yap, main, { llmScopes: { chat: ON_DEVICE_CHAT } });
  }
  // Back on this PC: Think again.
  await freshChat(main);
  await expect(thinkButton(main)).toBeVisible({ timeout: 15_000 });
});
