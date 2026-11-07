// Your Claude plan in Dictation cleanup and Commands with Claude Code signed out
// (desktop/src-tauri/src/ai/claude_plan.rs `fallback_key`, ai/llm.rs `post_chat_request`;
// desktop/src/lib/settings/ClaudePlanProvider.svelte, LanguageModelsPage.svelte): the stand-in
// says it isn't signed in (`YAP_FAKE_CLAUDE_LOGGED_IN=0`), so the key store has no plan marker.
// - Settings: the cleanup's banner says to sign in to Claude Code, with Sign in; the plan's card
//   in Dictation cleanup and Commands offers Sign in; "If your plan can't answer, use my API
//   key" shows (an Anthropic key is saved), off. Light and dark.
// - Off (the default): a dictation goes in as said, and a Command fails; nothing reaches the key.
// - On: that call goes once to the Anthropic key (here a local fake standing in for
//   api.anthropic.com: YAP_E2E_ANTHROPIC_API, debug builds only), on the same model, counted as
//   the key's.
// Nothing goes on the internet.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openSettings, closeSettings, expectStore, setConfig } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { RECORDINGS, fakeClaude } from './support/claude-plan.js';

const SAID = 'hello imogen the launch is on friday';
const BY_KEY = 'Hello, Imogen. The launch is on Friday.';
const ANTHROPIC_KEY = 'sk-ant-e2e-fake-00009012';
const signedOutPlan = (model) => ({ enabled: true, provider: 'claude-plan', baseUrl: 'claude-code://plan', model, apiKey: '', apiKeys: {}, prompt: '', disableThinking: false });

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
        name: 'claude-plan-scopes-signed-out',
        env: {
          ...fakeClaude(path.join(RECORDINGS, 'real-not-logged-in.jsonl'), { YAP_FAKE_CLAUDE_LOGGED_IN: '0' }),
          YAP_E2E_ANTHROPIC_API: fakeLlm.base,
          YAP_E2E_NET_ALIASES: `${new URL(fakeLlm.base).host}=api.anthropic.com`,
        },
        config: {
          postProcessEnabled: true,
          ppProvider: 'claude-plan',
          ppBaseUrl: 'claude-code://plan',
          ppApiKey: '',
          ppModel: 'claude-haiku-5-5',
          ppApiKeys: { anthropic: ANTHROPIC_KEY },
          agentName: 'Jarvis',
          meetingReminder: 'never',
          llmScopes: { voiceAgent: signedOutPlan('claude-haiku-5-5') },
        },
      });
    },
    { scope: 'worker' },
  ],
});

const log = (yap) => fs.readFileSync(yap.logPath, 'utf8');
const ofKind = (llm, kind) => llm.requests.filter((r) => r.kind === kind);

async function claudeTabOf(main, scope) {
  const dialog = await openSettings(main, 'Language Models');
  await dialog.getByRole('tab', { name: scope }).click();
  await dialog.getByRole('tab', { name: 'Claude' }).click();
  await expect(dialog.getByTestId('claude-provider')).toBeVisible();
  return dialog;
}

test('Settings: signed out, Sign in on the banner and the card; the fallback offered, off (light and dark)', async ({ yap, main, shot }) => {
  let dialog = await claudeTabOf(main, 'Dictation Cleanup');
  const banner = dialog.getByTestId('cleanup-key-banner');
  await expect(banner).toContainText('Sign in to Claude Code');
  await expect(banner).toContainText('until Claude Code is signed in on this PC Yap types your words as they are');
  await expect(banner.getByRole('button', { name: 'Sign in' })).toBeVisible();
  const tab = dialog.getByTestId('claude-provider');
  const card = tab.getByTestId('claude-plan-card');
  await expect(card.locator('[data-state="signed-out"]')).toContainText("it isn't signed in");
  await expect(card.locator('[data-setting="claude-plan-sign-in"]').getByRole('button', { name: 'Sign in' })).toBeVisible();
  const fallback = tab.locator('[data-setting="claude-plan-fallback"]');
  await expect(fallback.getByRole('button', { name: "If your plan can't answer, use my API key" })).toHaveAttribute('aria-pressed', 'false');
  await expect(tab.getByTestId('way-compare').locator('[aria-current="true"]')).toContainText('Your plan');
  await shot(main, '01-cleanup-signed-out');
  await setConfig(yap, main, { theme: 'dark' });
  await shot(main, '02-cleanup-signed-out-dark');
  await setConfig(yap, main, { theme: 'light' });
  // Commands: the card's Sign in too.
  dialog = await claudeTabOf(main, 'Commands');
  await expect(dialog.getByTestId('claude-plan-card').locator('[data-setting="claude-plan-sign-in"]')).toBeVisible();
  await expect(dialog.getByTestId('claude-provider').locator('[data-setting="claude-plan-fallback"]')).toBeVisible();
  // Sign in opens Claude Code's own sign-in (nothing, in a test run), and the card waits for it.
  await dialog.getByTestId('claude-plan-card').getByRole('button', { name: 'Sign in' }).click();
  await expect(dialog.getByTestId('claude-plan-card')).toContainText('Finish signing in to Claude Code in the terminal Yap opened.');
  await shot(main, '03-commands-signing-in');
  await closeSettings(main);
});

test("off: a dictation goes in as said and a Command fails, nothing on the key; on: the key answers that call", async ({ yap, main, fakeLlm }) => {
  fakeLlm.reply('cleanup', () => BY_KEY);
  fakeLlm.reply('rewrite', () => 'Thank you, Sam, for all your help.');
  // Off, the default.
  let out = await yap.invoke('e2e_dictation', { text: SAID });
  expect(out.text).toBe(SAID);
  let cmd = await yap.invoke('e2e_command', { text: 'Hey Jarvis, write a thank you note to Sam' });
  expect(cmd.outcome).toBe('failed');
  expect(ofKind(fakeLlm, 'cleanup')).toHaveLength(0);
  expect(ofKind(fakeLlm, 'rewrite')).toHaveLength(0);

  // On, for cleanup (Settings' switch).
  const dialog = await claudeTabOf(main, 'Dictation Cleanup');
  await dialog.locator('[data-setting="claude-plan-fallback"]').getByRole('button', { name: "If your plan can't answer, use my API key" }).click();
  await closeSettings(main);
  await expectStore(yap, 'config.json', (c) => c.ppPlanFallback === true);
  out = await yap.invoke('e2e_dictation', { text: SAID });
  expect(out.text).toBe(BY_KEY);
  const asked = ofKind(fakeLlm, 'cleanup').at(-1);
  expect(asked.model).toBe('claude-haiku-5-5');
  expect(asked.authorization).toBe(`Bearer ${ANTHROPIC_KEY}`);
  // Claude Code isn't ready here, so it was never started for it.
  expect(log(yap)).not.toMatch(/Claude plan: a one-shot call/);
  // Counted as the key's, not the plan's.
  await expect.poll(async () => (await yap.invoke('ai_usage_summary')).today.models.map((m) => m.provider)).toContain('anthropic');
  expect((await yap.invoke('ai_usage_summary')).today.planTokens).toBe(0);
  // Commands still off: as before.
  cmd = await yap.invoke('e2e_command', { text: 'Hey Jarvis, write a thank you note to Sam' });
  expect(cmd.outcome).toBe('failed');
  // On for Commands too: the key writes it.
  const commands = yap.readJson('config.json').llmScopes.voiceAgent;
  await setConfig(yap, main, { llmScopes: { ...yap.readJson('config.json').llmScopes, voiceAgent: { ...commands, planFallback: true } } });
  cmd = await yap.invoke('e2e_command', { text: 'Hey Jarvis, write a thank you note to Sam' });
  expect(cmd.text).toBe('Thank you, Sam, for all your help.');
  expect(ofKind(fakeLlm, 'rewrite').at(-1).model).toBe('claude-haiku-5-5');

  // Connected, but Claude Code's request refused (a sign-in that ended: its real
  // "Not logged in" recording): that call goes to the key then, and the marker follows.
  await setConfig(yap, main, { ppApiKey: 'claude-plan', ppApiKeys: { anthropic: ANTHROPIC_KEY, 'claude-plan': 'claude-plan' } });
  const n = ofKind(fakeLlm, 'cleanup').length;
  out = await yap.invoke('e2e_dictation', { text: SAID });
  expect(out.text).toBe(BY_KEY);
  expect(ofKind(fakeLlm, 'cleanup')).toHaveLength(n + 1);
  await expect.poll(() => log(yap)).toMatch(/Claude plan couldn't answer: this call goes to the Anthropic API key, as the scope asks scope="?cleanup/);
  await expectStore(yap, 'config.json', (c) => !c.ppApiKeys?.['claude-plan']);
  await setConfig(yap, main, { ppPlanFallback: false, ppApiKey: '', llmScopes: { ...yap.readJson('config.json').llmScopes, voiceAgent: commands } });
});
