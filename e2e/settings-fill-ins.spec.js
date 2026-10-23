// Settings' copy of the config fills in what config.json doesn't have yet —
// its defaults, and a Chat scope for each tab (`ensureScopes`). A fill-in
// the person never touched must never win over a value saved elsewhere
// since: the robot's Chat scope, saved where config.json had none, was put
// back to Settings' default one at Settings' next auto-save, and the Voice
// Assistant then had no model (a real-app run, 2026-10-23). The Chat picker
// saving a model into a fresh config would have lost it the same way.
// (`settings/configMerge.js` `fill`.)
import { test as base, expect, openSettings, closeSettings, expectStore } from './support/fixtures.js';

const test = base.extend({
  yapOptions: [
    async ({}, use) => {
      // A fresh config: no Chat scope saved yet.
      await use({ name: 'settings-fill-ins' });
    },
    { scope: 'worker' },
  ],
});

const CHAT = {
  enabled: true,
  provider: 'groq',
  baseUrl: 'http://127.0.0.1:9/v1',
  model: 'openai/gpt-oss-20b',
  apiKey: 'e2e-fake-key',
  apiKeys: {},
  prompt: '',
  disableThinking: false,
};

test("a Chat scope saved where config.json had none survives Settings' next save", async ({ yap, main }) => {
  // Settings is up, with its own fill-ins and nothing saved.
  let dialog = await openSettings(main, 'Dictation');
  expect(yap.readConfig().llmScopes?.chat?.enabled ?? false).toBe(false);
  // Saved elsewhere, as the robot (or the Chat picker) does.
  const cfg = await yap.invoke('get_config');
  await yap.invoke('save_config', { cfg: { ...cfg, llmScopes: { ...(cfg.llmScopes || {}), chat: CHAT } } });
  await expectStore(yap, 'config.json', (c) => c.llmScopes?.chat?.enabled === true);
  // A switch flipped in Settings: the Chat scope stays.
  const offer = dialog.getByRole('button', { name: 'Offer what you said after AI cleanup' });
  await offer.click();
  await expectStore(yap, 'config.json', (c) => c.asSaidOffer === false);
  await main.waitForTimeout(700);
  const chat = yap.readConfig().llmScopes.chat;
  expect([chat.enabled, chat.baseUrl, chat.apiKey]).toEqual([true, CHAT.baseUrl, CHAT.apiKey]);
  await offer.click();
  await expectStore(yap, 'config.json', (c) => c.asSaidOffer === true);
  await closeSettings(main);
});
