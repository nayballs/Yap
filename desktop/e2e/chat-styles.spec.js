// Styles (desktop/src-tauri/src/chat/styles.rs; views/chat/StyleChip.svelte, StylesDialog.svelte;
// 15 Nov 2026, the Claude app's styles from Anthropic's help pages): how a chat's answers are
// written, picked per chat from Style under the message box.
//
// - The chip says Normal; its menu has Yap's four (the one in use ticked) and "Create & edit
//   styles…". Concise picked: the chip says so, the question's system prompt carries the style's
//   block, and the chat keeps it; a new chat starts with the last one picked.
// - A style of your own from a description: Chat's model writes it (the fake's `styleWrite`),
//   you read and edit it, save it; picked, the next question carries its words. Deleted (asked
//   first): the chats that had it go back to Normal. Light and dark.
// - Nothing in chats.json: the picks live in chat_styles.json, a file an older Yap never touches.
//
// The model is the scripted fake (support/fake-llm.js). Every detail is fictional.
import { test as base, expect, openView, setConfig } from './support/fixtures.js';
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
        name: 'chat-styles',
        config: {
          postProcessEnabled: false,
          meetingReminder: 'never',
          webSearch: false,
          speakReplies: false,
          llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat', apiKey: '', apiKeys: {}, prompt: '', disableThinking: false } },
        },
      });
    },
    { scope: 'worker' },
  ],
});

const box = (main) => main.getByRole('textbox', { name: 'Message' });
const chip = (main) => main.getByTestId('chat-style-chip');
const menu = (main) => main.getByRole('menu').first();
const dialog = (main) => main.getByTestId('chat-styles-dialog');
const askedWith = (fakeLlm, text) => fakeLlm.requests.filter((r) => Array.isArray(r.messages) && JSON.stringify(r.messages).includes(text) && r.kind === 'other').at(-1);
const systemOf = (req) => String(req?.messages?.find((m) => m.role === 'system')?.content ?? '');

async function theme(yap, main, which) {
  await setConfig(yap, main, { theme: which });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe(which);
}

async function ask(main, fakeLlm, text, reply) {
  fakeLlm.scriptChat([{ content: reply }]);
  await box(main).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
  await expect(main.locator('.cbubble.assistant').last()).toContainText(reply);
}

test('a chat’s style is picked under the message box, carried in its prompt and kept with it', async ({ yap, main, fakeLlm, shot }) => {
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
  await expect(chip(main)).toHaveText('Normal');
  await chip(main).click();
  await expect(menu(main).getByRole('menuitemradio')).toHaveText(['Normal', 'Concise', 'Explanatory', 'Formal']);
  await expect(menu(main).getByRole('menuitemradio', { name: 'Normal' })).toHaveAttribute('aria-checked', 'true');
  await expect(menu(main).getByRole('menuitem', { name: 'Create & edit styles…' })).toBeVisible();
  await shot(main, '01-style-menu');
  await menu(main).getByRole('menuitemradio', { name: 'Concise' }).click();
  await expect(chip(main)).toHaveText('Concise');

  await ask(main, fakeLlm, 'How do I repot a fern?', 'Water it, then lift it gently.');
  const sys = systemOf(askedWith(fakeLlm, 'How do I repot a fern?'));
  expect(sys).toContain('How to write in this chat (the user picked the "Concise" style; follow it in every answer here');
  expect(sys).toContain('Answer as briefly as the question allows: lead with the answer');
  // The chat keeps it (chat_styles.json, never chats.json).
  const [conv] = await yap.invoke('chats_list');
  expect(await yap.invoke('chat_style_of', { conversation: conv.id })).toBe('concise');
  expect(yap.readJson('chat_styles.json').chats[String(conv.id)]).toBe('concise');
  expect(JSON.stringify(yap.readJson('chats.json'))).not.toContain('concise');

  // A new chat starts with the last one picked; Normal adds nothing.
  await main.getByRole('button', { name: 'New chat' }).click();
  await expect(chip(main)).toHaveText('Concise');
  await chip(main).click();
  await menu(main).getByRole('menuitemradio', { name: 'Normal' }).click();
  await expect(chip(main)).toHaveText('Normal');
  await ask(main, fakeLlm, 'And a cactus?', 'Gloves, then a gritty mix.');
  expect(systemOf(askedWith(fakeLlm, 'And a cactus?'))).not.toContain('How to write in this chat');
  // The first chat still has Concise.
  await main.locator('.conv').filter({ hasText: 'How do I repot a fern?' }).click();
  await expect(chip(main)).toHaveText('Concise');
});

test('a style of your own: described, written by the model, edited, used and deleted', async ({ yap, main, fakeLlm, shot }) => {
  fakeLlm.reply('styleWrite', () => JSON.stringify({ name: 'Garden chat', instructions: 'Write warmly and briefly, like a note to a neighbour over the fence.' }));
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
  await chip(main).click();
  await menu(main).getByRole('menuitem', { name: 'Create & edit styles…' }).click();
  await expect(dialog(main)).toBeVisible();
  await expect(dialog(main).getByRole('list', { name: "Yap's styles" }).getByRole('listitem')).toHaveCount(4);
  await dialog(main).getByRole('button', { name: 'Create a style' }).click();
  await dialog(main).getByRole('textbox', { name: 'The style you want' }).fill('warm and brief, like talking to a neighbour');
  await dialog(main).getByRole('button', { name: 'Write the style' }).click();
  await expect(dialog(main).getByRole('textbox', { name: 'How to write' })).toHaveValue('Write warmly and briefly, like a note to a neighbour over the fence.');
  await expect(dialog(main).getByRole('textbox', { name: 'Name' })).toHaveValue('Garden chat');
  const asked = fakeLlm.requests.filter((r) => r.kind === 'styleWrite').at(-1);
  expect(asked.user).toContain('warm and brief, like talking to a neighbour');
  // Edited before it's saved.
  await dialog(main).getByRole('textbox', { name: 'How to write' }).fill('Write warmly and briefly, like a note to a neighbour. No jargon.');
  await shot(main, '02-style-written');
  await dialog(main).getByRole('button', { name: 'Save style' }).click();
  const yours = dialog(main).getByRole('list', { name: 'Your styles' });
  await expect(yours.getByRole('listitem')).toHaveText([/Garden chat\s*Write warmly and briefly, like a note to a neighbour\. No jargon\./]);
  await theme(yap, main, 'dark');
  await shot(main, '03-styles-dialog-dark');
  await theme(yap, main, 'light');
  await yours.getByRole('button', { name: 'Use' }).click();
  await dialog(main).getByRole('button', { name: 'Done' }).click();
  await expect(chip(main)).toHaveText('Garden chat');

  await ask(main, fakeLlm, 'When should I sow beans?', 'Mid-spring, once it warms up.');
  expect(systemOf(askedWith(fakeLlm, 'When should I sow beans?'))).toContain('(the user picked the "Garden chat" style; follow it in every answer here, over any habit of your own about length or layout): Write warmly and briefly, like a note to a neighbour. No jargon.');
  const conv = (await yap.invoke('chats_list')).find((c) => c.title.includes('sow beans'));
  // Deleted, asked first: its chat goes back to Normal.
  await chip(main).click();
  await menu(main).getByRole('menuitem', { name: 'Create & edit styles…' }).click();
  await dialog(main).getByRole('button', { name: 'Delete Garden chat' }).click();
  const confirm = main.getByRole('alertdialog');
  await expect(confirm).toHaveAccessibleName('Delete “Garden chat”?');
  await confirm.getByRole('button', { name: 'Delete' }).click();
  await expect(dialog(main).getByRole('list', { name: 'Your styles' })).toHaveCount(0);
  await dialog(main).getByRole('button', { name: 'Done' }).click();
  await expect(chip(main)).toHaveText('Normal');
  expect(await yap.invoke('chat_style_of', { conversation: conv.id })).toBe('normal');
});
