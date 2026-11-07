// An empty Code chat with Claude Code signed out here (desktop/src/lib/views/chat/CodeEmpty.svelte,
// core/codeChats.js `claudeCodeHint`, stores/claudePlan.svelte.js): a Yap of its own, its
// Claude Code this build's stand-in (`e2e_claude_script`) whose `claude auth status` says
// signed out (YAP_FAKE_CLAUDE_LOGGED_IN=0). jobs-code-chats.spec.js has the rest of the
// greeting (signed in, and Claude Code not on this PC).
// - The greeting's one line, "Claude Code isn't signed in on this PC yet.", with Sign in
//   (Claude Code's own `claude auth login`, in a terminal: a test run opens none), then
//   "Finish signing in…" with Check again. Light and dark.
// - Jobs paying with an API key need no sign-in: nothing to say.
// Nobody's real Claude Code, account or files: the workspace is in this run's own folder.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, setConfig } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { RUNS } from './support/yap.js';
import { newCodeChat } from './support/code-chats.js';

const HERE = path.join(RUNS, 'jobs-code-chats-signed-out-spec');
const GARDEN = path.join(HERE, 'folders', 'garden-planner');
const SCRIPT = path.join(HERE, 'scripts', 'idle.jsonl');

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
    async ({}, use) => {
      await use({ name: 'jobs-code-chats-signed-out', env: { YAP_FAKE_CLAUDE_LOGGED_IN: '0' }, config: { memoryLearnChats: false } });
    },
    { scope: 'worker' },
  ],
});

async function theme(yap, main, which) {
  await setConfig(yap, main, { theme: which });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe(which);
}

test('an empty code chat says to sign in, with Sign in; paying with a key, nothing to say', async ({ yap, main, fakeLlm, shot }) => {
  fs.mkdirSync(GARDEN, { recursive: true });
  fs.mkdirSync(path.dirname(SCRIPT), { recursive: true });
  // Never run: the stand-in only answers `--version` and `auth status` here.
  fs.writeFileSync(SCRIPT, JSON.stringify({ _wait_user: true }) + '\n');
  await yap.invoke('e2e_claude_script', { script: SCRIPT });
  await setConfig(yap, main, {
    jobsEnabled: true,
    jobsPay: 'plan',
    llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat', apiKey: '', apiKeys: {}, prompt: '', disableThinking: false } },
  });
  if (!(await yap.invoke('job_folders_list')).some((f) => f.name === 'garden-planner')) await yap.invoke('job_folders_add', { path: GARDEN });

  await openView(main, 'Chat');
  await newCodeChat(main, 'garden-planner');
  const empty = main.getByTestId('chat-code-empty');
  await expect(empty.getByRole('heading', { name: 'Work in garden-planner' })).toBeVisible();
  const hint = main.getByTestId('chat-code-claude');
  await expect(hint).toHaveAttribute('data-state', 'signed-out', { timeout: 15_000 });
  await expect(hint).toContainText("Claude Code isn't signed in on this PC yet.");
  await shot(main, '01-empty-code-chat-signed-out-light');
  await theme(yap, main, 'dark');
  await shot(main, '02-empty-code-chat-signed-out-dark');
  await theme(yap, main, 'light');

  // Sign in: Claude Code's own, in a terminal (a test run opens none); then Check again.
  await hint.getByRole('button', { name: 'Sign in' }).click();
  await expect(hint).toHaveAttribute('data-state', 'signing-in');
  await expect(hint).toContainText('Finish signing in to Claude Code in the terminal Yap opened.');
  await hint.getByRole('button', { name: 'Check again' }).click();
  await expect(hint.getByRole('button', { name: 'Check again' })).toBeEnabled();
  await expect(hint).toHaveAttribute('data-state', 'signing-in');

  // Jobs paying with an API key: no sign-in needed, nothing to say.
  await setConfig(yap, main, { jobsPay: 'key' });
  await expect(hint).toHaveCount(0);
  await setConfig(yap, main, { jobsPay: 'plan' });
  await expect(hint).toBeVisible();
});
