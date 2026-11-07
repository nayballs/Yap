// Your Claude plan with no Claude Code on this PC (desktop/src/lib/settings/ClaudePlanProvider.svelte,
// ai/claude_code/cli.rs: a test run without YAP_CLAUDE_EXE finds none): Settings' card says
// so, with Claude Code's install guide and Check again, and no Sign in; Chat's picker offers
// "Use your Claude plan instead…" under the Anthropic key's box, saying it opens the guide.
import { test, expect, closeSettings, openView } from './support/fixtures.js';
import { claudeTab, planConfig } from './support/claude-plan.js';

// Chat on Claude without a key yet (Settings shows its Claude tab).
const KEYLESS = { enabled: true, provider: 'anthropic', baseUrl: 'https://api.anthropic.com/v1', model: 'claude-haiku-5-5', apiKey: '', apiKeys: {}, prompt: '', disableThinking: false };
test.use({ yapOptions: { name: 'claude-plan-missing', config: planConfig({}, { llmScopes: { chat: KEYLESS } }) } });

test("Settings' card says Claude Code isn't installed, with its install guide", async ({ main, shot }) => {
  const dialog = await claudeTab(main);
  const card = dialog.getByTestId('claude-plan-card');
  await expect(card.locator('[data-state="missing"]')).toContainText("Claude Code isn't installed on this PC.");
  await expect(card.getByRole('link', { name: 'Install Claude Code' })).toBeVisible();
  await expect(card.getByRole('button', { name: 'Sign in' })).toHaveCount(0);
  // The Anthropic key's way beside it.
  await expect(dialog.getByRole('button', { name: 'Use an API key instead' })).toBeVisible();
  await shot(main, '01-claude-plan-not-installed');
  await closeSettings(main);
});

test("Chat's picker: Claude's key box, and the plan's way under it", async ({ main, shot }) => {
  await openView(main, 'Chat');
  await main.locator('.modelchip .chip').click();
  const picker = main.getByRole('dialog', { name: 'Switch the Chat model' });
  await picker.getByRole('tab', { name: /^Claude/ }).click();
  const pane = picker.locator('.pane');
  await expect(pane.locator('.phead')).toContainText('Anthropic');
  await expect(pane.locator('.connect')).toContainText('Connect Claude');
  await expect(pane.getByTestId('claude-ways')).toHaveCount(0);
  await expect(pane.getByTestId('claude-plan-link')).toHaveText('Use your Claude plan instead…');
  await expect(pane.locator('.waynote')).toContainText("Claude Code isn't on this PC yet");
  await shot(main, '02-picker-claude-no-plan');
  await main.keyboard.press('Escape');
});
