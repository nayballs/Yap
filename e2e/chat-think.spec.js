// Think it through (src-tauri/src/chat/think.rs, Chat's Think button):
// one question, thought through.
// - The button shows only for a model that can think on request (Groq's
//   gpt-oss here; not Qwen3.8), follows a switch of model, and goes off once
//   its question is sent.
// - The question asks the provider its own way (gpt-oss: `reasoning_effort:
//   "high"`); the next one, without Think, asks nothing of the kind.
// - The thinking shows apart from the answer while it streams ("Thinking…",
//   folded), then folded above the answer ("Thought for 2 s"), and is kept
//   with it in chats.json — never in what's sent back to the model.
// The on-device Qwen's own fields (`chat_template_kwargs`, a thinking
// budget) are unit-tested and were measured on the real llamafile
// (think.rs); "Groq" here is a local fake (support/fake-llm.js).
import { test as base, expect, openView, expectStore, setConfig } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';

const TEST_KEY = 'e2e-fake-key';

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
        config: {
          ppApiKeys: { groq: TEST_KEY },
          llmScopes: { chat: groqChat(fakeLlm, 'openai/gpt-oss-20b') },
          meetingReminder: 'never',
        },
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

test('Think: one question thought through, its thinking folded above the answer and kept', async ({ yap, main, fakeLlm, shot }) => {
  await freshChat(main);
  await expect(thinkButton(main)).toBeVisible();
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
    },
  ]);
  await send(main, 'A bat and a ball cost £1.10. The bat costs £1 more. How much is the ball?');
  await expect(main.getByTestId('chat-thinking')).toBeVisible({ timeout: 10_000 });
  await expect(main.getByTestId('chat-thinking')).toContainText('The bat costs');
  await shot(main, '02-thinking');
  await expect(thread(main)).toContainText('The ball costs 5p.', { timeout: 10_000 });

  // Asked its way: gpt-oss at high effort.
  const asked = fakeLlm.requests.slice(before).find((r) => r.user?.includes('How much is the ball?'));
  expect(asked.think.reasoningEffort).toBe('high');
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

  // The next question: no thinking asked for, and the earlier thinking never sent.
  const next = fakeLlm.requests.length;
  fakeLlm.scriptChat([{ content: 'Yes.' }]);
  await send(main, 'Are you sure?');
  await expect(thread(main)).toContainText('Yes.');
  const plain = fakeLlm.requests.slice(next).find((r) => r.user?.includes('Are you sure?'));
  expect(plain.think.reasoningEffort).toBeNull();
  expect(JSON.stringify(plain.messages)).not.toContain('so the ball is 5p');

  // Reopened later: still folded above it.
  await openView(main, 'Home');
  await openView(main, 'Chat');
  await main.locator('.conv', { hasText: 'A bat and a ball' }).first().click();
  await expect(main.getByTestId('chat-thought').first()).toContainText(/Thought/);
});

test("a model that can't think on request has no Think button; switching to one brings it", async ({ yap, main, fakeLlm }) => {
  await setConfig(yap, main, { llmScopes: { chat: groqChat(fakeLlm, 'qwen/qwen3.8-27b') } });
  try {
    await freshChat(main);
    await expect(box(main)).toBeVisible();
    await expect(thinkButton(main)).toHaveCount(0);
  } finally {
    await setConfig(yap, main, { llmScopes: { chat: groqChat(fakeLlm, 'openai/gpt-oss-20b') } });
  }
  await freshChat(main);
  await expect(thinkButton(main)).toBeVisible({ timeout: 10_000 });
});
