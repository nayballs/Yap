// What the Claude plan's specs share (claude-plan*.spec.js): Claude Code played by a
// stand-in (`yap.exe fake-claude <script>`, desktop/src-tauri/src/ai/claude_code/fake.rs:
// YAP_CLAUDE_EXE and YAP_CLAUDE_ARGS_PREFIX), from Yap's own recordings
// (ai/claude_code/fixtures/2.1.296/) or this folder's scripts; never the person's own Claude
// Code, never their account.
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { openSettings, openView } from './fixtures.js';
import { EXE } from './yap.js';

const HERE = path.dirname(fileURLToPath(import.meta.url));
/** Claude Code's recordings (signed out, a rate limit…). */
export const RECORDINGS = path.join(HERE, '..', '..', 'src-tauri', 'src', 'ai', 'claude_code', 'fixtures', '2.1.296');
/** This folder's scripts. */
export const SCRIPTS = path.join(HERE, 'fake-claude');

/** The environment that makes Yap find the stand-in, playing `script` (a path). */
export function fakeClaude(script, extra = {}) {
  return { YAP_CLAUDE_EXE: EXE, YAP_CLAUDE_ARGS_PREFIX: JSON.stringify(['fake-claude', script]), ...extra };
}

/** Chat's scope on the plan's Claude Haiku 5.5, as config.json keeps it. */
export const PLAN_CHAT = { enabled: true, provider: 'claude-plan', baseUrl: 'claude-code://plan', model: 'claude-haiku-5-5', apiKey: 'claude-plan', apiKeys: {}, prompt: '', disableThinking: false };

/** A test Yap's config with Chat on the plan and `keys` in the key store. */
export const planConfig = (keys, more = {}) => ({ postProcessEnabled: false, ppProvider: 'ondevice', meetingReminder: 'never', ppApiKeys: keys, llmScopes: { chat: PLAN_CHAT }, ...more });

export const messageBox = (main) => main.getByRole('textbox', { name: 'Message' });
/** Chat's "Couldn't send" toast. */
export const sendToast = (main) => main.locator('.toast').filter({ hasText: "Couldn't send" }).last();

/** Settings → Language Models → Chat & Voice Assistant → Claude. */
export async function claudeTab(main) {
  const dialog = await openSettings(main, 'Language Models');
  await dialog.getByRole('tab', { name: 'Chat & Voice Assistant' }).click();
  await dialog.getByRole('tab', { name: 'Claude' }).click();
  return dialog;
}

/** A question in a new chat. */
export async function askInNewChat(main, text) {
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
  await messageBox(main).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
}
