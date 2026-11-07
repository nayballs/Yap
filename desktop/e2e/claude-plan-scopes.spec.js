// Your Claude plan in every scope (7 Nov 2026: desktop/src-tauri/src/ai/claude_plan.rs `Scope`,
// claude_plan/oneshot.rs, claude_plan/spare.rs; desktop/src/lib/settings/ClaudePlanProvider.svelte,
// WayNote.svelte): Dictation cleanup, Commands and Note formatting on the person's own Claude
// plan, through their own Claude Code, beside Chat. A stand-in plays Claude Code, signed in
// (`yap.exe fake-claude <script>`, ai/claude_code/fake.rs), from a script this spec writes:
// every call it serves appends its process id and the words it was sent to a file (`_dump_args`
// after `_wait_user`), so one Claude Code is seen to serve one call. Nothing goes on the internet.
// - Settings → Language Models → every scope's Claude tab: the plan's card ("Connected · Using
//   Claude plan"); what each way is ("Your plan … Expect slightly slower responses" against
//   "API key: quickest", side by side for Dictation cleanup and Commands, a line for Note
//   formatting and Chat); the selected model's row says which pays ("Using your Claude
//   plan"); the cleanup row's "Cloud providers" names both plans. With an Anthropic key
//   saved: "Your plan | API key", and "If your plan can't answer, use my API key" (off) on
//   the plan in every scope but Chat, never on the key's way. Light and dark.
// - A dictation cleaned on the plan: lean flags (no tools, no settings, not saved, Yap's own
//   system prompt, low effort, Haiku), its usage under "cleanup" as plan usage; a spare waits
//   behind it, and the next dictation goes to that spare: two dictations, two processes, each
//   answering only its own call.
// - A Command by name on the plan.
// - Spares end when idle, and when the scope leaves the plan.
// The fallback to the key and Sign in: claude-plan-scopes-signed-out.spec.js.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openSettings, closeSettings, expectStore, setConfig } from './support/fixtures.js';
import { fakeClaude } from './support/claude-plan.js';
import { RUNS } from './support/yap.js';

const DIR = path.join(RUNS, 'claude-plan-scopes-fake');
const SCRIPT = path.join(DIR, 'plan-scopes.jsonl');
const CALLS = path.join(DIR, 'calls.jsonl');
const CLEANED = 'Hello Imogen, the launch is on Friday.';
const SAID = 'hello imogen the launch is on friday';
const ANTHROPIC_KEY = 'sk-ant-e2e-fake-00005678';

/** Claude Code's part: one call per process, its id and words written down, one answer. */
function writeScript() {
  fs.rmSync(DIR, { recursive: true, force: true });
  fs.mkdirSync(DIR, { recursive: true });
  const lines = [
    { _wait_user: true },
    { _dump_args: CALLS },
    { type: 'system', subtype: 'init', session_id: '{{session_id}}', cwd: '{{cwd}}', tools: [], mcp_servers: [], model: 'claude-haiku-5-5', permissionMode: 'dontAsk', apiKeySource: 'none', claude_code_version: '2.1.296', capabilities: ['interrupt_receipt_v1'] },
    { type: 'stream_event', event: { type: 'content_block_delta', index: 0, delta: { type: 'text_delta', text: CLEANED } }, parent_tool_use_id: null, session_id: '{{session_id}}' },
    { type: 'result', subtype: 'success', is_error: false, result: CLEANED, session_id: '{{session_id}}', total_cost_usd: 0.0003, usage: { input_tokens: 120, output_tokens: 9 }, permission_denials: [], terminal_reason: 'completed', num_turns: 1 },
    // A second call in the same process would get this: it never should.
    { _wait_user: true },
    { type: 'result', subtype: 'success', is_error: false, result: 'A SECOND CALL IN ONE PROCESS', session_id: '{{session_id}}' },
  ];
  fs.writeFileSync(SCRIPT, lines.map((l) => JSON.stringify(l)).join('\n') + '\n');
}

const onPlan = (model) => ({ enabled: true, provider: 'claude-plan', baseUrl: 'claude-code://plan', model, apiKey: 'claude-plan', apiKeys: {}, prompt: '', disableThinking: false });

const test = base.extend({
  yapOptions: [
    async ({}, use) => {
      writeScript();
      await use({
        name: 'claude-plan-scopes',
        env: fakeClaude(SCRIPT),
        config: {
          postProcessEnabled: true,
          ppProvider: 'claude-plan',
          ppBaseUrl: 'claude-code://plan',
          ppApiKey: 'claude-plan',
          ppModel: 'claude-haiku-5-5',
          ppApiKeys: { 'claude-plan': 'claude-plan' },
          agentName: 'Jarvis',
          meetingReminder: 'never',
          llmScopes: { voiceAgent: onPlan('claude-haiku-5-5'), noteFormatting: onPlan('claude-sonnet-5-5'), chat: onPlan('claude-haiku-5-5') },
        },
      });
    },
    { scope: 'worker' },
  ],
});

/** Every call the stand-in served: `{ pid, args, cwd, user_text }`. */
const calls = () => (fs.existsSync(CALLS) ? fs.readFileSync(CALLS, 'utf8').trim().split('\n').filter(Boolean).map((l) => JSON.parse(l)) : []);
const after = (argv, flag) => argv[argv.indexOf(flag) + 1];
const spares = (yap, opts = {}) => yap.invoke('e2e_claude_spares', opts).then((r) => r.waiting);
const log = (yap) => fs.readFileSync(yap.logPath, 'utf8');

/** Settings → Language Models → a scope's tab → Cloud providers → Claude. */
async function claudeTabOf(main, scope) {
  const dialog = await openSettings(main, 'Language Models');
  await dialog.getByRole('tab', { name: scope }).click();
  await dialog.getByRole('tab', { name: 'Claude' }).click();
  await expect(dialog.getByTestId('claude-provider')).toBeVisible();
  return dialog;
}

test("Settings: every scope's Claude tab offers the plan, says what each way is and which pays (light and dark)", async ({ yap, main, shot }) => {
  // Dictation cleanup: the plan's card, both ways side by side, the plan in use.
  let dialog = await claudeTabOf(main, 'Dictation Cleanup');
  await expect(dialog.locator('.mode').filter({ hasText: 'Cloud providers' }).locator('.m-desc')).toHaveText('Your ChatGPT or Claude plan, or Groq, OpenAI, Anthropic, Gemini or OpenRouter on your own API key');
  let tab = dialog.getByTestId('claude-provider');
  await expect(tab).toHaveAttribute('data-way', 'claude-plan');
  await expect(tab.getByTestId('claude-plan-card').locator('[data-state="ready"]')).toContainText('Connected · Using Claude plan');
  const compare = tab.getByTestId('way-compare');
  await expect(compare.locator('[aria-current="true"]')).toContainText('Your planIn use');
  await expect(compare).toContainText("Uses your plan's limits. Expect slightly slower responses: it goes through Claude Code on this PC.");
  await expect(compare).toContainText('Quickest. Billed per use on your Anthropic account.');
  await expect(tab.getByTestId('model-paid-by')).toHaveText('Using your Claude plan');
  await expect(tab.locator('.srow.sel')).toContainText('Claude Haiku 5.5');
  // Only a chat's sessions are kept by Claude Code: no such line here.
  await expect(tab.getByTestId('claude-plan-copies')).toHaveCount(0);
  // No Anthropic key: no switch, no fallback to offer.
  await expect(tab.locator('[data-setting="claude-ways"]')).toHaveCount(0);
  await expect(tab.locator('[data-setting="claude-plan-fallback"]')).toHaveCount(0);
  await expect(tab.getByRole('button', { name: 'Use an Anthropic API key instead…' })).toBeVisible();
  await shot(main, '01-cleanup-claude-plan');
  await setConfig(yap, main, { theme: 'dark' });
  await shot(main, '02-cleanup-claude-plan-dark');
  await setConfig(yap, main, { theme: 'light' });

  // Commands: the same, side by side.
  dialog = await claudeTabOf(main, 'Commands');
  tab = dialog.getByTestId('claude-provider');
  await expect(tab).toHaveAttribute('data-way', 'claude-plan');
  await expect(tab.getByTestId('way-compare').locator('[aria-current="true"]')).toContainText('Your plan');
  await expect(tab.getByTestId('model-paid-by')).toHaveText('Using your Claude plan');
  await shot(main, '03-commands-claude-plan');

  // Note formatting and Chat: a line for the way in use.
  dialog = await claudeTabOf(main, 'Note Formatting');
  tab = dialog.getByTestId('claude-provider');
  await expect(tab.getByTestId('way-note')).toHaveText("Your Claude plan, through Claude Code on this PC: uses your plan's limits. Expect slightly slower responses.");
  await expect(tab.locator('.srow.sel')).toContainText('Claude Sonnet 5.5');
  dialog = await claudeTabOf(main, 'Chat & Voice Assistant');
  tab = dialog.getByTestId('claude-provider');
  await expect(tab.getByTestId('way-note')).toContainText("uses your plan's limits");
  await expect(tab.getByTestId('claude-plan-copies')).toBeVisible();
  await closeSettings(main);

  // An Anthropic key too: "Your plan | API key", and the fallback (off) on the plan.
  await setConfig(yap, main, { ppApiKeys: { 'claude-plan': 'claude-plan', anthropic: ANTHROPIC_KEY } });
  dialog = await claudeTabOf(main, 'Dictation Cleanup');
  tab = dialog.getByTestId('claude-provider');
  const ways = tab.locator('[data-setting="claude-ways"]');
  await expect(ways.getByRole('tab')).toHaveText(['Your plan', 'API key']);
  await expect(ways.getByRole('tab', { name: 'Your plan' })).toHaveAttribute('aria-selected', 'true');
  const fallback = tab.locator('[data-setting="claude-plan-fallback"]');
  await expect(fallback).toContainText("If your plan can't answer, use my API key");
  await expect(fallback.getByRole('button', { name: "If your plan can't answer, use my API key" })).toHaveAttribute('aria-pressed', 'false');
  await fallback.scrollIntoViewIfNeeded();
  await shot(main, '04-cleanup-both-ways-fallback-off');
  await setConfig(yap, main, { theme: 'dark' });
  await shot(main, '05-cleanup-both-ways-fallback-off-dark');
  await setConfig(yap, main, { theme: 'light' });
  // The key's way: quickest, its row says the key pays, no fallback to offer.
  await ways.getByRole('tab', { name: 'API key' }).click();
  await expect(tab).toHaveAttribute('data-way', 'anthropic');
  await expect(tab.getByTestId('way-compare').locator('[aria-current="true"]')).toContainText('API keyIn use');
  await expect(tab.getByTestId('model-paid-by')).toHaveText('Anthropic API key');
  await expect(fallback).toHaveCount(0);
  await shot(main, '06-cleanup-api-key');
  await expectStore(yap, 'config.json', (c) => c.ppProvider === 'anthropic' && c.ppApiKey === ANTHROPIC_KEY);
  await ways.getByRole('tab', { name: 'Your plan' }).click();
  await expectStore(yap, 'config.json', (c) => c.ppProvider === 'claude-plan' && c.ppApiKey === 'claude-plan' && c.ppModel === 'claude-haiku-5-5');
  // Commands and Note formatting offer it too; Chat never (its limit message has Use my API key for this chat).
  dialog = await claudeTabOf(main, 'Commands');
  await expect(dialog.getByTestId('claude-provider').locator('[data-setting="claude-plan-fallback"]')).toBeVisible();
  dialog = await claudeTabOf(main, 'Note Formatting');
  await expect(dialog.getByTestId('claude-provider').locator('[data-setting="claude-plan-fallback"]')).toBeVisible();
  dialog = await claudeTabOf(main, 'Chat & Voice Assistant');
  await expect(dialog.getByTestId('claude-provider').locator('[data-setting="claude-ways"]')).toBeVisible();
  await expect(dialog.getByTestId('claude-provider').locator('[data-setting="claude-plan-fallback"]')).toHaveCount(0);
  await closeSettings(main);
  await setConfig(yap, main, { ppApiKeys: { 'claude-plan': 'claude-plan' } });
});

test('a dictation is cleaned on the plan, lean; the next goes to the spare waiting behind it, its own process', async ({ yap }) => {
  const before = calls().length;
  let out = await yap.invoke('e2e_dictation', { text: SAID });
  expect(out.raw).toBe(SAID);
  expect(out.text).toBe(CLEANED);
  await expect.poll(() => calls().length).toBe(before + 1);
  const first = calls().at(-1);
  // Lean, never saved, Yap's own prompt, low effort, Haiku, in Yap's empty folder.
  expect(first.args).toContain('--no-session-persistence');
  expect(first.args).toContain('--strict-mcp-config');
  expect(after(first.args, '--tools')).toBe('');
  expect(after(first.args, '--setting-sources')).toBe('');
  expect(after(first.args, '--model')).toBe('claude-haiku-5-5');
  expect(after(first.args, '--effort')).toBe('low');
  expect(after(first.args, '--system-prompt-file')).toMatch(/own-prompt\.txt$/);
  expect(first.args).not.toContain('--mcp-config');
  expect(first.cwd.replace(/\\/g, '/')).toMatch(/\/claude\/ask$/);
  expect(first.user_text).toContain(SAID);
  // A spare waits behind it, for the next dictation.
  await expect.poll(() => spares(yap), { timeout: 15_000 }).toContain('cleanup');

  out = await yap.invoke('e2e_dictation', { text: SAID });
  expect(out.text).toBe(CLEANED);
  await expect.poll(() => calls().length).toBe(before + 2);
  const second = calls().at(-1);
  expect(second.pid).not.toBe(first.pid);
  expect(calls().filter((c) => c.pid === second.pid)).toHaveLength(1);
  await expect.poll(() => log(yap)).toMatch(/Claude plan: a one-shot call answered.*scope="?cleanup"?.*warm=true/);
  // And another spare behind that one.
  await expect.poll(() => spares(yap), { timeout: 15_000 }).toContain('cleanup');
  // Counted under cleanup, as the plan's (its API value, never spend).
  await expect.poll(async () => (await yap.invoke('ai_usage_summary')).today.features.map((f) => f.feature)).toContain('cleanup');
  expect((await yap.invoke('ai_usage_summary')).today.planTokens).toBeGreaterThan(0);
});

test('a Command by name runs on the plan', async ({ yap }) => {
  const before = calls().length;
  const out = await yap.invoke('e2e_command', { text: 'Hey Jarvis, write a thank you note to Sam' });
  expect(out.text).toBe(CLEANED);
  await expect.poll(() => calls().length).toBe(before + 1);
  const call = calls().at(-1);
  expect(call.user_text).toContain('write a thank you note to Sam');
  expect(after(call.args, '--effort')).toBe('low');
  await expect.poll(() => log(yap)).toMatch(/Claude plan: a one-shot call answered.*scope="?commands"?/);
  await expect.poll(() => spares(yap), { timeout: 15_000 }).toContain('commands');
});

test('spares end when idle, and when the scope leaves the plan', async ({ yap, main }) => {
  await expect.poll(() => spares(yap), { timeout: 15_000 }).toEqual(expect.arrayContaining(['cleanup', 'commands']));
  // Commands off the plan: its spare ends at the save.
  const commands = yap.readJson('config.json').llmScopes.voiceAgent;
  await setConfig(yap, main, { llmScopes: { ...yap.readJson('config.json').llmScopes, voiceAgent: { ...commands, provider: 'groq', baseUrl: 'https://api.groq.com/openai/v1', model: 'openai/gpt-oss-20b', apiKey: '' } } });
  await expect.poll(() => spares(yap)).not.toContain('commands');
  expect(await spares(yap)).toContain('cleanup');
  // Idle past the limit: ended.
  await spares(yap, { idleMs: 1, sweep: true });
  expect(await spares(yap)).toEqual([]);
  await spares(yap, { idleMs: 0 });
  await setConfig(yap, main, { llmScopes: { ...yap.readJson('config.json').llmScopes, voiceAgent: commands } });
});
