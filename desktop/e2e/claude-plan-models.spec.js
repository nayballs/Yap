// Your Claude plan's models are Yap's own list (desktop/src-tauri/src/ai/claude_plan.rs
// `MODELS`), so the build that changes it shows the new one at once
// (ai/model_catalog.rs `keep_own_lists`), and a choice saved on the plan of a
// model the list moved on from follows its family (config.rs
// `migrate_plan_models`). 10 Nov 2026: Claude Fable 5.2 was in the list, yet
// the plan's picker went on showing Fable 5.1 — the pickers read each
// provider's list from model_catalog.json, kept a day, and the plan's was an
// hour old.
//
// This Yap starts as that one did: model_catalog.json with the plan's list
// from the build before (fetched an hour ago), and Chat saved on the plan's
// Claude Fable 5.1 at Extra. A stand-in plays Claude Code, signed in
// (claude-plan.spec.js' way); nothing goes on the internet.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { test as base, expect, openView, expectStore } from './support/fixtures.js';
import { EXE, RUNS } from './support/yap.js';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const SCRIPT = path.join(HERE, 'support', 'fake-claude', 'plan-chat.jsonl');
const CLAUDE_CONFIG = path.join(RUNS, 'claude-plan-models-claude-config');
const PLAN_CHAT = { enabled: true, provider: 'claude-plan', baseUrl: 'claude-code://plan', model: 'claude-fable-5-1', apiKey: 'claude-plan', apiKeys: {}, prompt: '', disableThinking: false };
/** The plan's list as the build before had it. */
const BEFORE = ['claude-haiku-5-5', 'claude-sonnet-5-5', 'claude-opus-5-5', 'claude-fable-5-1'];
const name = (id) => `Claude ${id.split('-')[1][0].toUpperCase()}${id.split('-')[1].slice(1)} ${id.split('-').slice(2).join('.')}`;

const test = base.extend({
  yapOptions: [
    async ({}, use) => {
      fs.rmSync(CLAUDE_CONFIG, { recursive: true, force: true });
      fs.mkdirSync(CLAUDE_CONFIG, { recursive: true });
      const hourAgo = Math.floor(Date.now() / 1000) - 3600;
      await use({
        name: 'claude-plan-models',
        env: { YAP_CLAUDE_EXE: EXE, YAP_CLAUDE_ARGS_PREFIX: JSON.stringify(['fake-claude', SCRIPT]), CLAUDE_CONFIG_DIR: CLAUDE_CONFIG },
        config: {
          postProcessEnabled: false,
          ppProvider: 'ondevice',
          meetingReminder: 'never',
          ppApiKeys: { 'claude-plan': 'claude-plan' },
          llmScopes: { chat: PLAN_CHAT },
          chatEffort: { 'claude-plan:claude-fable-5-1': 'xhigh' },
        },
        prepare: (dataDir) =>
          fs.writeFileSync(
            path.join(dataDir, 'model_catalog.json'),
            JSON.stringify({ providers: { 'claude-plan': { fetchedAt: hourAgo, checkedAt: hourAgo, models: BEFORE.map((id) => ({ id, name: name(id), firstSeen: hourAgo - 86_400 })) } } })
          ),
      });
    },
    { scope: 'worker' },
  ],
});

const chip = (main) => main.locator('.modelchip .chip');
const picker = (main) => main.getByRole('dialog', { name: 'Switch the Chat model' });
const pane = (main) => picker(main).locator('.pane');

test('a list from the build before and a saved Fable 5.1: the picker and the chip are on this build’s Fable at once', async ({ yap, main, shot }) => {
  await openView(main, 'Chat');
  // The saved choice followed its family, its effort with it.
  await expect(chip(main)).toHaveText(/Claude Fable 5\.2\s+Extra\s+· Using Claude plan/);
  await expectStore(yap, 'config.json', (c) => c.llmScopes.chat.model === 'claude-fable-5-2' && c.chatEffort['claude-plan:claude-fable-5-2'] === 'xhigh' && !('claude-plan:claude-fable-5-1' in c.chatEffort));
  // The picker: this build's list, though the stored one was an hour old.
  await chip(main).click();
  await expect(picker(main)).toBeVisible();
  await expect(pane(main).locator('.phead')).toContainText('Using Claude plan');
  await expect(pane(main).locator('.mrow .nm')).toHaveText(['Claude Opus 5.5', 'Claude Fable 5.2', 'Claude Sonnet 5.5', 'Claude Haiku 5.5']);
  await expect(pane(main).getByRole('button', { name: /^Claude Fable 5\.2/ })).toHaveAttribute('aria-pressed', 'true');
  await expect(pane(main)).not.toContainText('Fable 5.1');
  await expect(picker(main).getByTestId('chat-effort')).toContainText('Claude Fable 5.2');
  await shot(main, '01-plan-list-is-this-builds');
  await main.keyboard.press('Escape');
  // Stored again as this build has it: the models it had keep when they were
  // first seen.
  await expectStore(yap, 'model_catalog.json', (s) => {
    const list = s.providers['claude-plan'];
    return list.models.map((m) => m.id).join() === 'claude-haiku-5-5,claude-sonnet-5-5,claude-opus-5-5,claude-fable-5-2' && list.models[2].firstSeen < list.models[3].firstSeen;
  });
});
