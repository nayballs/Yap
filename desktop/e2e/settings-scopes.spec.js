// Settings fills in the Language Models scopes config.json hasn't got
// (`ensureScopes`: Commands, Note Formatting, Chat), so each tab has a scope
// to bind to. They must be filled in once and stay. With `llmScopes`
// holding only the Chat scope (the Chat picker saved it, nothing ever saved
// the other two; a fresh config's `{}` too), every merge took config.json's
// scopes back without the fill-ins (they read as no change of Settings'
// copy, and config.json had nothing there), Settings filled them in again —
// a change of its copy — and the auto-save ran again 400 ms later, for
// good: the Chat tab remounted every 400 ms, and the Claude plan card on it
// asked Claude Code again each time (2026-11-07). A gap config.json still
// has keeps Settings' fill-in, saved with the next save; a gap another
// surface filled since takes that surface's value
// (`settings/configMerge.js` `fill`; settings-fill-ins.spec.js).
import { test as base, expect, openSettings, closeSettings, expectStore } from './support/fixtures.js';

// The scope an older install saved: the only one in its config.json.
const CHAT = {
  enabled: true,
  provider: 'groq',
  baseUrl: 'http://127.0.0.1:9/v1',
  model: 'openai/gpt-oss-120b',
  apiKey: 'e2e-fake-key',
  apiKeys: {},
  prompt: 'Answer in one short paragraph.',
  disableThinking: false,
};

// Note Formatting, set up on another surface while Settings is open.
const NOTES = {
  enabled: true,
  provider: 'ondevice',
  baseUrl: '',
  model: '',
  apiKey: '',
  apiKeys: {},
  prompt: 'Short bullet points under a heading per topic.',
  disableThinking: true,
};

const test = base.extend({
  yapOptions: [
    async ({}, use) => {
      await use({ name: 'settings-scopes', config: { llmScopes: { chat: CHAT }, agentName: 'Nova' } });
    },
    { scope: 'worker' },
  ],
});

/** Count the main window's IPC calls (a fetch to http://ipc.localhost/ on Windows) from now on. */
function countIpc(main) {
  return main.evaluate(() => {
    window.__ipcCounts = {};
    if (window.__ipcCounted) return;
    window.__ipcCounted = true;
    const realFetch = window.fetch.bind(window);
    window.fetch = (input, init) => {
      const url = typeof input === 'string' ? input : input?.url || '';
      if (url.startsWith('http://ipc.localhost/')) {
        const cmd = decodeURIComponent(url.slice('http://ipc.localhost/'.length));
        window.__ipcCounts[cmd] = (window.__ipcCounts[cmd] || 0) + 1;
      }
      return realFetch(input, init);
    };
  });
}

test("the scopes config.json lacks are filled in once: one save, then no more, and the Chat tab stays put", async ({ yap, main, shot }) => {
  // Settings is up on its fill-ins; opening it saved nothing.
  let dialog = await openSettings(main, 'Dictation');
  expect(Object.keys(yap.readConfig().llmScopes)).toEqual(['chat']);
  // Note Formatting saved elsewhere, into one of the gaps.
  const cfg = await yap.invoke('get_config');
  await yap.invoke('save_config', { cfg: { ...cfg, llmScopes: { ...cfg.llmScopes, noteFormatting: NOTES } } });
  await expectStore(yap, 'config.json', (c) => c.llmScopes?.noteFormatting?.prompt === NOTES.prompt);
  // A switch the person flips: Settings' auto-save.
  const offer = dialog.getByRole('button', { name: 'Offer what you said after AI cleanup' });
  const was = (await offer.getAttribute('aria-pressed')) === 'true';
  await offer.click();
  await expectStore(yap, 'config.json', (c) => !!c.asSaidOffer === !was);

  // Language Models → Chat, once things have settled: it isn't mounted
  // again, and nothing reads, merges or saves the config over and over.
  dialog = await openSettings(main, 'Language Models');
  await dialog.getByRole('tab', { name: 'Chat & Voice Assistant' }).click();
  const group = dialog.locator('#settings-scope-chat');
  await expect(group).toBeVisible();
  await main.waitForTimeout(1000);
  await group.evaluate((el) => (el.dataset.e2eFirst = '1'));
  await countIpc(main);
  await main.waitForTimeout(2500);
  const counts = await main.evaluate(() => window.__ipcCounts);
  expect.soft(counts.save_config ?? 0, 'no saves while nothing changes').toBe(0);
  expect.soft(counts.get_config ?? 0, 'no read-merge every 400 ms').toBeLessThanOrEqual(1);
  await expect(dialog.locator('#settings-scope-chat[data-e2e-first]'), 'the Chat tab never remounted').toHaveCount(1);
  await shot(main, 'chat-tab-settled');

  // The gap nobody filled has Settings' Commands scope (off, its default
  // prompt), saved; the one filled elsewhere keeps that value; the older
  // install's Chat scope and the person's own settings are as they were.
  const saved = yap.readConfig();
  expect(Object.keys(saved.llmScopes).sort()).toEqual(['chat', 'noteFormatting', 'voiceAgent']);
  expect(saved.llmScopes.voiceAgent).toMatchObject({ enabled: false, provider: 'groq' });
  expect(saved.llmScopes.voiceAgent.prompt).toContain('{{agentName}}');
  expect(saved.llmScopes.noteFormatting).toEqual(NOTES);
  expect(saved.llmScopes.chat).toEqual(CHAT);
  expect(saved.agentName).toBe('Nova');
  expect(!!saved.asSaidOffer).toBe(!was);

  // Back as it was: one more save, and it's still quiet after it.
  dialog = await openSettings(main, 'Dictation');
  await offer.click();
  await expectStore(yap, 'config.json', (c) => !!c.asSaidOffer === was);
  await main.waitForTimeout(700);
  await countIpc(main);
  await main.waitForTimeout(1500);
  const after = await main.evaluate(() => window.__ipcCounts);
  expect(after.save_config ?? 0).toBe(0);
  expect(after.get_config ?? 0).toBeLessThanOrEqual(1);
  expect(yap.readConfig().llmScopes.chat).toEqual(CHAT);
  await closeSettings(main);
});
