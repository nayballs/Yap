// Memory with a small Chat model (a 1.5B model without tools: memory/learn.rs
// `Path::Small`, memory/small.rs): Settings says it learns carefully; a quiet
// chat is read a sentence at a time — only what the person said about
// themselves, one short call each under the small form's JSON schema, every
// answer checked against their words; a chat of questions makes no call;
// "Tell Yap what to change" goes the same way, and "Forget …" needs no model.
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

const memoryCalls = (fakeLlm) => fakeLlm.requests.filter((r) => r.kind === 'memory');

test('a small Chat model learns carefully, a sentence at a time; Tell and Forget work', async ({ yap, main, fakeLlm, shot }) => {
  const dialog = await openSettings(main, 'Memory');
  const note = dialog.getByRole('note').filter({ hasText: 'learns carefully' });
  await expect(note).toBeVisible();
  await shot(main, '01-small-model');

  // The model's answer for a sentence: the person's sister, or (for anything
  // else) someone the sentence never names — which Yap must refuse.
  fakeLlm.reply('memory', (user) => {
    if (user.includes('Marisol')) return JSON.stringify({ section: 'people', title: 'Marisol', fact: 'Lives in Lisbon' });
    if (user.includes('Beans')) return JSON.stringify({ section: 'profile', title: '', fact: 'Has a dog called Beans' });
    return JSON.stringify({ section: 'people', title: 'Tom', fact: 'Brother' });
  });

  // Questions only: no sentence about the person, no model call.
  const before = memoryCalls(fakeLlm).length;
  const asked = await yap.invoke('chat_send', { conversationId: null, text: "What's the capital of Canada? Can you recommend a film?" });
  const nothing = await yap.invoke('e2e_memory_learn', { conversationId: asked.conversationId });
  expect(nothing.done).toEqual([]);
  expect(memoryCalls(fakeLlm).length).toBe(before);

  // About the person: one short call a sentence, under the small form's schema.
  const sent = await yap.invoke('chat_send', {
    conversationId: null,
    text: 'My sister Marisol lives in Lisbon. I work night shifts at the library. Any tips for sleeping in the day?',
  });
  const learned = await yap.invoke('e2e_memory_learn', { conversationId: sent.conversationId });
  const calls = memoryCalls(fakeLlm).slice(before);
  expect(calls.map((c) => c.user)).toEqual(['My sister Marisol lives in Lisbon', 'I work night shifts at the library']);
  expect(calls[0].responseFormat?.json_schema?.name).toBe('memory_item');
  expect(calls[0].model).toBe('qwen2.5-1.5b-instruct');
  // Marisol is kept, as the sentence says who she is; "Tom", whom it never
  // names, isn't — what the person said of themselves is, in their words.
  expect(learned.done.map((d) => d.title)).toEqual(['Marisol', 'Profile']);
  await expectStore(yap, 'memory.json', (m) =>
    m.items.some((i) => i.title === 'Marisol' && i.summary === 'Sister' && i.details?.includes('Lives in Lisbon')) &&
    m.items.some((i) => i.section === 'profile' && i.details?.includes('Works night shifts at the library')) &&
    !m.items.some((i) => i.title === 'Tom')
  );

  // "Tell Yap what to change": the same small call; the dog is found in
  // the person's own words ("my dog's name is Beans": Beans, Dog).
  await dialog.getByRole('textbox', { name: 'Tell Yap what to change' }).fill("My dog's name is Beans");
  await dialog.getByRole('button', { name: 'Suggest changes' }).click();
  await dialog.getByRole('button', { name: 'Apply' }).click();
  await expectStore(yap, 'memory.json', (m) => m.items.some((i) => i.title === 'Beans' && i.summary === 'Dog'));
  expect(memoryCalls(fakeLlm).at(-1).user).toBe("My dog's name is Beans");

  // "Forget …": Yap's own, no model call.
  const calledBefore = memoryCalls(fakeLlm).length;
  await dialog.getByRole('textbox', { name: 'Tell Yap what to change' }).fill('Forget everything about Marisol');
  await dialog.getByRole('button', { name: 'Suggest changes' }).click();
  await dialog.getByRole('button', { name: 'Apply' }).click();
  await expectStore(yap, 'memory.json', (m) => !m.items.some((i) => i.title === 'Marisol'));
  expect(memoryCalls(fakeLlm).length).toBe(calledBefore);
  await closeSettings(main);
});
