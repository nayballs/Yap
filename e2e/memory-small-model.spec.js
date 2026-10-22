// Memory with a Chat model too small to learn by itself (a 1.5B model
// without tools: memory/learn.rs `learns_by_itself`): Settings says so, a
// quiet chat isn't read (no model call), and "Tell Yap what to change"
// still works with it.
import { test as base, expect, openSettings, closeSettings, expectStore } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';

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
        name: 'memory-small-model',
        config: {
          llmScopes: {
            chat: { enabled: true, provider: 'local', baseUrl: fakeLlm.base, model: 'qwen2.5-1.5b-instruct' },
          },
        },
      });
    },
    { scope: 'worker' },
  ],
});

test('a small Chat model: no learning by itself, said so; "Tell Yap what to change" still works', async ({ yap, main, fakeLlm, shot }) => {
  const dialog = await openSettings(main, 'Memory');
  const note = dialog.getByRole('note').filter({ hasText: 'too small to pick out what to remember' });
  await expect(note).toBeVisible();
  await shot(main, '01-small-model');

  // A chat about the person, "gone quiet": nothing read, no model call.
  const sent = await yap.invoke('chat_send', { conversationId: null, text: 'My sister Rochelle lives in York.' });
  const before = fakeLlm.requests.filter((r) => r.kind === 'memory').length;
  const learned = await yap.invoke('e2e_memory_learn', { conversationId: sent.conversationId });
  expect(learned.done).toEqual([]);
  expect(fakeLlm.requests.filter((r) => r.kind === 'memory').length).toBe(before);

  // Asked for, it's done.
  fakeLlm.reply('memory', (user) =>
    user.includes('Beans')
      ? JSON.stringify({ changes: [{ action: 'add', id: 0, section: 'profile', name: '', about: '', facts: ['Has a dog called Beans'], remove_facts: [], say: '' }] })
      : '{"changes": []}'
  );
  await dialog.getByRole('textbox', { name: 'Tell Yap what to change' }).fill("My dog's name is Beans");
  await dialog.getByRole('button', { name: 'Suggest changes' }).click();
  await dialog.getByRole('button', { name: 'Apply' }).click();
  await expectStore(yap, 'memory.json', (m) => m.items.some((i) => i.details?.includes('Has a dog called Beans')));
  // A model on this PC without tools answers under the changes' grammar.
  const asked = fakeLlm.requests.filter((r) => r.kind === 'memory').at(-1);
  expect(asked.model).toBe('qwen2.5-1.5b-instruct');
  await closeSettings(main);
});
