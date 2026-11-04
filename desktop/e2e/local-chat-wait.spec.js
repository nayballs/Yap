// A Chat answer from a model on this PC may take longer than a cloud
// model's 20 s (Qwen3.5 4B on a processor: ~25 s a turn, its slowest calls
// ~50 s; llm::chat_timeout gives a loopback server 2 minutes): the answer
// still comes, and meanwhile the Chat view says it's still working rather
// than looking hung. The "model on this PC" is a local fake
// (support/fake-llm.js) on 127.0.0.1 that holds its answer back 23 s.
import { test as base, expect, openView } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';

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
        name: 'local-chat-wait',
        config: {
          llmScopes: {
            // A self-hosted Qwen3.5 4B (Ollama's tag): on this PC, with tools.
            chat: { enabled: true, provider: 'local', baseUrl: fakeLlm.base, model: 'qwen3.5:4b', apiKey: '', apiKeys: {}, prompt: '', disableThinking: false },
          },
        },
      });
    },
    { scope: 'worker' },
  ],
});

test('a slow answer from a model on this PC comes, and says it is still working', async ({ main, fakeLlm, shot }) => {
  test.setTimeout(90_000);
  fakeLlm.reply('other', () => 'Canberra is the capital of Australia.');
  fakeLlm.delay('other', 23_000);
  try {
    await openView(main, 'Chat');
    await main.getByRole('textbox', { name: 'Message' }).fill("What's the capital of Australia?");
    await main.getByRole('button', { name: 'Send' }).click();
    const waiting = main.getByTestId('chat-still-working');
    await expect(waiting).toContainText('the model runs on this PC, so this can take a minute', { timeout: 15_000 });
    await shot(main, '01-still-working');
    // Past a cloud model's 20 s: the answer, not an error.
    await expect(main.locator('.cbubble.assistant').filter({ hasText: 'Canberra is the capital of Australia.' })).toBeVisible({
      timeout: 40_000,
    });
    await expect(waiting).toBeHidden();
    // Asked with the tools: a ≥ 4B local model gets them.
    const asked = fakeLlm.requests.filter((r) => r.kind === 'other').at(-1);
    expect(asked.tools).toContain('search_notes');
    expect(asked.model).toBe('qwen3.5:4b');
  } finally {
    fakeLlm.delay('other', 0);
  }
});
