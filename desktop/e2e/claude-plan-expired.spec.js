// Your Claude plan when Claude Code says it's signed in but its request is refused (a
// sign-in that ended: chat/turn/plan.rs, ai/claude_code/events.rs `Problem::NotSignedIn`):
// the stand-in plays Claude Code's real "Not logged in" recording
// (ai/claude_code/fixtures/2.1.296/real-not-logged-in.jsonl). The turn's refusal is never an
// answer: "Claude Code isn't signed in on this PC. Sign in to Claude Code, then try again.",
// with Sign in to Claude Code; nothing saved as an answer, the question back in the box.
import path from 'node:path';
import { test, expect } from './support/fixtures.js';
import { RECORDINGS, fakeClaude, planConfig, messageBox, sendToast, askInNewChat } from './support/claude-plan.js';

test.use({
  yapOptions: {
    name: 'claude-plan-expired',
    env: fakeClaude(path.join(RECORDINGS, 'real-not-logged-in.jsonl')),
    config: planConfig({ 'claude-plan': 'claude-plan' }),
  },
});

test("the turn's refusal says to sign in, and never shows as an answer", async ({ yap, main, shot }) => {
  await askInNewChat(main, 'What is on today?');
  const t = sendToast(main);
  await expect(t).toContainText("Claude Code isn't signed in on this PC. Sign in to Claude Code, then try again.", { timeout: 15_000 });
  await expect(t.getByRole('button', { name: 'Sign in to Claude Code' })).toBeVisible();
  await shot(main, '01-claude-plan-chat-signed-out');
  const conv = yap.readJson('chats.json').conversations.at(-1);
  expect(conv.messages.filter((m) => m.role === 'assistant')).toHaveLength(0);
  await expect(messageBox(main)).toHaveValue('What is on today?');
});
