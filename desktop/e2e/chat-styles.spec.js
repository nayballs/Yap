// Styles (desktop/src-tauri/src/chat/styles.rs; views/chat/Composer.svelte, chatStyles.svelte.js,
// StylesDialog.svelte; 15 Nov 2026, the Claude app's styles from Anthropic's help pages): how a chat's
// answers are written, picked per chat from the message box's + menu (Style ›, 16 Nov 2026).
//
// - Normal shows nothing in the box; Style › has Yap's four (the one in use ticked) and "Create & edit
//   styles…". Concise picked: a chip in the box says so (× back to Normal), the question's system prompt carries the style's
//   block, and the chat keeps it; a new chat starts with the last one picked.
// - A style of your own from a description: Chat's model writes it (the fake's `styleWrite`),
//   you read and edit it, save it; picked, the next question carries its words. Deleted (asked
//   first): the chats that had it go back to Normal. Light and dark.
// - Nothing in chats.json: the picks live in chat_styles.json, a file an older Yap never touches.
//
// The model is the scripted fake (support/fake-llm.js). Every detail is fictional.
import { test as base, expect, openView, setConfig, openSettings, closeSettings } from './support/fixtures.js';
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
// The style's chip in the box (none at Normal), and Style › in the box's + menu.
const chip = (main) => main.getByTestId('chat-style-on');
const plus = (main) => main.getByRole('button', { name: 'Chat options' });
const menu = (main) => main.getByTestId('chat-plus-style-menu');
/** + → Style ›: the chat's styles. */
async function openStyles(main) {
  await plus(main).click();
  await main.getByTestId('chat-plus-style').click();
  await expect(menu(main).getByRole('menuitemradio').first()).toBeVisible();
}
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

test('a chat’s style is picked from the box’s + menu, shown in the box, carried in its prompt and kept with it', async ({ yap, main, fakeLlm, shot }) => {
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
  // Normal: nothing in the box, and nothing under it but the model.
  await expect(chip(main)).toHaveCount(0);
  await expect(main.locator('.composerfoot').getByRole('button', { name: /^Style/ })).toHaveCount(0);
  await openStyles(main);
  await expect(main.getByTestId('chat-plus-style')).toContainText('Normal');
  await expect(menu(main).getByRole('menuitemradio')).toHaveText(['Normal', 'Concise', 'Explanatory', 'Formal']);
  await expect(menu(main).getByRole('menuitemradio', { name: 'Normal' })).toHaveAttribute('aria-checked', 'true');
  await expect(menu(main).getByRole('menuitem', { name: 'Create & edit styles…' })).toBeVisible();
  await shot(main, '01-style-menu');
  await menu(main).getByRole('menuitemradio', { name: 'Concise' }).click();
  await expect(menu(main)).toHaveCount(0);
  await expect(chip(main)).toContainText('Concise');

  await ask(main, fakeLlm, 'How do I repot a fern?', 'Water it, then lift it gently.');
  const sys = systemOf(askedWith(fakeLlm, 'How do I repot a fern?'));
  expect(sys).toContain('How to write in this chat (the user picked the "Concise" style; follow it in every answer here');
  expect(sys).toContain('Answer as briefly as the question allows: lead with the answer');
  // The chat keeps it (chat_styles.json, never chats.json).
  const [conv] = await yap.invoke('chats_list');
  expect(await yap.invoke('chat_style_of', { conversation: conv.id })).toBe('concise');
  expect(yap.readJson('chat_styles.json').chats[String(conv.id)]).toBe('concise');
  expect(JSON.stringify(yap.readJson('chats.json'))).not.toContain('concise');

  // A new chat starts with the last one picked; its chip's × goes back to Normal, which adds nothing.
  await main.getByRole('button', { name: 'New chat' }).click();
  await expect(chip(main)).toContainText('Concise');
  await chip(main).getByRole('button', { name: /back to Normal/ }).click();
  await expect(chip(main)).toHaveCount(0);
  await ask(main, fakeLlm, 'And a cactus?', 'Gloves, then a gritty mix.');
  expect(systemOf(askedWith(fakeLlm, 'And a cactus?'))).not.toContain('How to write in this chat');
  // The first chat still has Concise.
  await main.locator('.conv').filter({ hasText: 'How do I repot a fern?' }).click();
  await expect(chip(main)).toContainText('Concise');
  // Picked again from the menu: Normal there too.
  await openStyles(main);
  await expect(menu(main).getByRole('menuitemradio', { name: 'Concise' })).toHaveAttribute('aria-checked', 'true');
  await menu(main).getByRole('menuitemradio', { name: 'Normal' }).click();
  await expect(chip(main)).toHaveCount(0);
});

test('a style of your own: described, written by the model, edited, used and deleted', async ({ yap, main, fakeLlm, shot }) => {
  fakeLlm.reply('styleWrite', () => JSON.stringify({ name: 'Garden chat', instructions: 'Write warmly and briefly, like a note to a neighbour over the fence.' }));
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
  await openStyles(main);
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
  await expect(chip(main)).toContainText('Garden chat');

  await ask(main, fakeLlm, 'When should I sow beans?', 'Mid-spring, once it warms up.');
  expect(systemOf(askedWith(fakeLlm, 'When should I sow beans?'))).toContain('(the user picked the "Garden chat" style; follow it in every answer here, over any habit of your own about length or layout): Write warmly and briefly, like a note to a neighbour. No jargon.');
  const conv = (await yap.invoke('chats_list')).find((c) => c.title.includes('sow beans'));
  // Deleted, asked first: its chat goes back to Normal.
  await openStyles(main);
  await menu(main).getByRole('menuitem', { name: 'Create & edit styles…' }).click();
  await dialog(main).getByRole('button', { name: 'Delete Garden chat' }).click();
  const confirm = main.getByRole('alertdialog');
  await expect(confirm).toHaveAccessibleName('Delete “Garden chat”?');
  await confirm.getByRole('button', { name: 'Delete' }).click();
  await expect(dialog(main).getByRole('list', { name: 'Your styles' })).toHaveCount(0);
  await dialog(main).getByRole('button', { name: 'Done' }).click();
  await expect(chip(main)).toHaveCount(0);
  expect(await yap.invoke('chat_style_of', { conversation: conv.id })).toBe('normal');
});

test('Settings → Skills has Chat styles, found by Settings search: the same dialog, and Use sets the style new chats start with', async ({ yap, main }) => {
  await openView(main, 'Chat');
  const dlg = await openSettings(main, 'Skills');
  const row = dlg.locator('[data-setting="chat-styles"]');
  await expect(row).toContainText('New chats start in Normal');
  await row.getByRole('button', { name: 'Create & edit styles…' }).click();
  await expect(dialog(main)).toBeVisible();
  await dialog(main).getByRole('list', { name: "Yap's styles" }).getByRole('listitem').filter({ hasText: 'Formal' }).getByRole('button', { name: 'Use' }).click();
  await dialog(main).getByRole('button', { name: 'Done' }).click();
  await expect(row).toContainText('New chats start in Formal');
  expect(await yap.invoke('chat_style_of', { conversation: null })).toBe('formal');
  await closeSettings(main);
  // A new chat starts in it: its chip in the box.
  await main.getByRole('button', { name: 'New chat' }).click();
  await expect(chip(main)).toContainText('Formal');
  await chip(main).getByRole('button', { name: /back to Normal/ }).click();
  await expect(chip(main)).toHaveCount(0);
});
