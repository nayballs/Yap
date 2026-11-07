// Your Claude plan at its limit (chat/turn/plan.rs, ai/claude_code/events.rs
// `Problem::RateLimited`, Chat's toast in views/chat/chatState.svelte.js): the stand-in plays
// a rejected rate limit (ai/claude_code/fixtures/2.1.296/synthetic-rate-limited.jsonl). Said
// plainly — "You've reached your Claude plan's limit" — with Use my API key for this chat
// (an Anthropic key is saved) and Manage usage; the key's way puts Chat on the Anthropic key,
// the same model.
import path from 'node:path';
import { test, expect, expectStore } from './support/fixtures.js';
import { RECORDINGS, fakeClaude, planConfig, messageBox, sendToast, askInNewChat } from './support/claude-plan.js';

test.use({
  yapOptions: {
    name: 'claude-plan-limit',
    env: fakeClaude(path.join(RECORDINGS, 'synthetic-rate-limited.jsonl')),
    config: planConfig({ 'claude-plan': 'claude-plan', anthropic: 'sk-ant-e2e-fake-00005678' }),
  },
});

test('the limit is said plainly, with Use my API key for this chat and Manage usage', async ({ yap, main, shot }) => {
  await askInNewChat(main, 'Summarise my week');
  const t = sendToast(main);
  await expect(t).toContainText("You've reached your Claude plan's limit (resets at ", { timeout: 15_000 });
  await expect(t.getByRole('button', { name: 'Use my API key for this chat' })).toBeVisible();
  await expect(t.getByRole('button', { name: 'Manage usage' })).toBeVisible();
  await shot(main, '01-claude-plan-limit');
  // The key's way: Chat on the Anthropic key, the same model. (The box
  // emptied first: the question would go again, to Anthropic itself.)
  await expect(messageBox(main)).toHaveValue('Summarise my week');
  await messageBox(main).fill('');
  await t.getByRole('button', { name: 'Use my API key for this chat' }).click();
  await expectStore(yap, 'config.json', (c) => c.llmScopes.chat.provider === 'anthropic' && c.llmScopes.chat.model === 'claude-haiku-5-5');
  await expect(main.locator('.modelchip .chip')).toContainText('Anthropic API key');
});
