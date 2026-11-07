// Your Claude plan with Claude Code signed out here (desktop/src-tauri/src/ai/claude_plan.rs,
// desktop/src/lib/settings/ClaudePlanProvider.svelte, core/aiErrors.js `claudePlanProblem`):
// a stand-in plays Claude Code (support/claude-plan.js), `claude auth status` saying signed
// out (YAP_FAKE_CLAUDE_LOGGED_IN=0).
// - Settings' card: found but not signed in, Sign in (Claude Code's own `claude auth login`,
//   in a terminal: test mode opens none), "Finish signing in…", Check again; the key store's
//   marker gone (an earlier sign-in had set it).
// - A chat on the plan: "Claude Code isn't signed in on this PC…", with Sign in to Claude
//   Code; the question back in the box.
// The other problems, each with a Yap of its own: claude-plan-missing.spec.js (not
// installed), claude-plan-expired.spec.js (a sign-in that ended), claude-plan-limit.spec.js
// (the plan's limit).
import path from 'node:path';
import { test, expect, closeSettings, expectStore } from './support/fixtures.js';
import { RECORDINGS, fakeClaude, planConfig, messageBox, sendToast, claudeTab, askInNewChat } from './support/claude-plan.js';

test.use({
  yapOptions: {
    name: 'claude-plan-signed-out',
    env: fakeClaude(path.join(RECORDINGS, 'real-not-logged-in.jsonl'), { YAP_FAKE_CLAUDE_LOGGED_IN: '0' }),
    // As left by an earlier sign-in: the marker set, Chat on the plan.
    config: planConfig({ 'claude-plan': 'claude-plan' }),
  },
});

test("Settings' card: found but not signed in, Sign in, and the marker gone", async ({ yap, main, shot }) => {
  const dialog = await claudeTab(main);
  const card = dialog.getByTestId('claude-plan-card');
  await expect(card.locator('[data-state="signed-out"]')).toContainText("is on this PC, but it isn't signed in.", { timeout: 15_000 });
  // Claude Code said it's signed out: the key store's marker went.
  await expectStore(yap, 'config.json', (c) => !c.ppApiKeys?.['claude-plan']);
  await shot(main, '01-claude-plan-signed-out-card');
  await card.getByRole('button', { name: 'Sign in' }).click();
  await expect(card.locator('[data-state="signed-out"]')).toContainText('Finish signing in to Claude Code in the terminal Yap opened.');
  await expect(card.getByRole('button', { name: 'Check again' })).toBeVisible();
  await shot(main, '02-claude-plan-signing-in');
  await closeSettings(main);
});

test('a chat on the plan says to sign in, with Sign in to Claude Code', async ({ main, shot }) => {
  await askInNewChat(main, 'What is on today?');
  const t = sendToast(main);
  await expect(t).toContainText("Claude Code isn't signed in on this PC.", { timeout: 15_000 });
  await expect(t.getByRole('button', { name: 'Sign in to Claude Code' })).toBeVisible();
  await shot(main, '03-claude-plan-chat-not-ready');
  await t.getByRole('button', { name: 'Sign in to Claude Code' }).click();
  await expect(messageBox(main)).toHaveValue('What is on today?');
});
