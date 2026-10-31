// The Chat view's model chip (src/lib/views/chat/ModelChip.svelte, src-tauri/src/
// ai/ai_model.rs): it names the model the next message goes to, resolved the
// way chat_send resolves it; says "no note tools" for a local model under
// 4B (whose answers come from a plain search of the notes); says "No AI
// model" when nothing is set up, and opens the picker on the provider to
// connect; and its picker (ChatModelPicker.svelte, more in
// chat-model-picker.spec.js) switches the Chat scope's model, which
// Settings' always-mounted config copy adopts so its auto-save keeps it.
// Each answer shows the model that wrote it. The "Groq" here is a local fake
// (support/fake-llm.js) behind a test key: nothing leaves the PC.
import { test as base, expect, openView, openSettings, closeSettings, settingsDialog, expectStore, setConfig } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';

const TEST_KEY = 'e2e-fake-key';

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
        name: 'chat-model',
        config: {
          ppApiKeys: { groq: TEST_KEY },
          llmScopes: { chat: groqChat(fakeLlm, 'openai/gpt-oss-20b') },
        },
      });
    },
    { scope: 'worker' },
  ],
});

/** A whole Chat scope, as config.json holds one (config.rs `LlmScope`). */
function chatScope(fields) {
  return { enabled: true, apiKey: '', apiKeys: {}, prompt: '', disableThinking: false, ...fields };
}
function groqChat(fakeLlm, model) {
  return chatScope({ provider: 'groq', baseUrl: fakeLlm.base, model, apiKey: TEST_KEY });
}

const chip = (main) => main.locator('.modelchip .chip');
const popover = (main) => main.getByRole('dialog', { name: 'Switch the Chat model' });
const models = (main) => popover(main).locator('.pane .mrow');

async function send(main, text) {
  await main.getByRole('textbox', { name: 'Message' }).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
}

test('the chip names the chat model, switches it, and every answer keeps its own', async ({
  yap,
  main,
  fakeLlm,
  shot,
}) => {
  await setConfig(yap, main, {
    ppApiKeys: { groq: TEST_KEY },
    llmScopes: { ...(await yap.invoke('get_config')).llmScopes, chat: groqChat(fakeLlm, 'openai/gpt-oss-20b') },
  });
  await openView(main, 'Chat');
  await expect(chip(main)).toHaveText('GPT-OSS 20B Medium · Groq');
  await expect(chip(main).locator('.mark')).toHaveAttribute('data-icon', 'groq');
  await shot(main, '01-chip-groq');

  // An answer, and the model that wrote it under it.
  fakeLlm.scriptChat([{ content: 'Your launch is on **Friday**.' }]);
  await send(main, 'When is the launch?');
  const thread = main.locator('.msgs');
  await expect(thread).toContainText('Your launch is on Friday.');
  await expect(thread.locator('.byline')).toHaveText(['GPT-OSS 20B · Groq']);

  // Where it runs comes from the endpoint: this "Groq" is the local fake.
  await expect(chip(main)).toHaveAttribute('title', /^Runs on this PC \(127\.0\.0\.1:/);

  // The picker, on Groq: its models (Yap's list: no live one in this run).
  await chip(main).click();
  const pop = popover(main);
  await expect(pop).toBeVisible();
  await expect(pop.getByRole('tab', { name: /^Groq/ })).toHaveAttribute('aria-selected', 'true');
  await expect(models(main).locator('.nm')).toHaveText(['GPT-OSS 20B', 'GPT-OSS 120B', 'Qwen3.8 27B']);
  await expect(pop.getByRole('button', { name: /^GPT-OSS 20B/ })).toHaveAttribute('aria-pressed', 'true');
  await shot(main, '02-popover');

  // Switch: the Chat scope only, dictation cleanup untouched.
  await pop.getByRole('button', { name: /^GPT-OSS 120B/ }).click();
  await expect(pop).toBeHidden();
  await expect(chip(main)).toHaveText('GPT-OSS 120B Medium · Groq');
  await expectStore(yap, 'config.json', (c) => c.llmScopes.chat.model === 'openai/gpt-oss-120b');
  const saved = yap.readJson('config.json');
  expect(saved.llmScopes.chat).toMatchObject({ enabled: true, provider: 'groq', baseUrl: fakeLlm.base });
  expect(saved.ppModel).toBe('openai/gpt-oss-20b');

  // The next answer comes from it, and says so; the first one still says its own.
  fakeLlm.scriptChat([{ content: 'It moved to Monday.' }]);
  await send(main, 'And now?');
  await expect(thread).toContainText('It moved to Monday.');
  await expect(thread.locator('.byline')).toHaveText(['GPT-OSS 20B · Groq', 'GPT-OSS 120B · Groq']);
  expect(fakeLlm.requests.at(-1).model).toBe('openai/gpt-oss-120b');
  await shot(main, '03-answers-keep-their-model');

  // Kept with the conversation: opened again, the lines are still there.
  await main.getByRole('button', { name: 'New chat' }).click();
  await main.locator('.conv').filter({ hasText: 'When is the launch?' }).click();
  await expect(main.locator('.msgs .byline')).toHaveText(['GPT-OSS 20B · Groq', 'GPT-OSS 120B · Groq']);
  const conv = yap.readJson('chats.json').conversations.find((c) => c.title === 'When is the launch?');
  expect(conv.messages.map((m) => m.model?.id ?? null)).toEqual([
    null,
    'openai/gpt-oss-20b',
    null,
    'openai/gpt-oss-120b',
  ]);

  // Settings adopted it: Language Models → Chat shows it, and a save from
  // Settings (any setting) keeps it.
  await chip(main).click();
  await popover(main).getByRole('button', { name: 'Language model settings…' }).click();
  const dialog = settingsDialog(main);
  await expect(dialog).toBeVisible();
  await expect(dialog.getByRole('heading', { name: 'Language Models' })).toBeVisible();
  await expect(dialog.getByRole('tab', { name: 'Chat' })).toHaveAttribute('aria-selected', 'true');
  await expect(dialog.locator('.srow.sel')).toContainText('GPT-OSS 120B');
  await shot(main, '04-settings-chat-scope');

  const general = await openSettings(main, 'Meetings');
  const live = general.getByRole('button', { name: 'Show live transcript' });
  const was = (await live.getAttribute('aria-pressed')) === 'true';
  await live.click();
  await expectStore(yap, 'config.json', (c) => c.meetingLiveTranscript === !was);
  expect(yap.readJson('config.json').llmScopes.chat.model).toBe('openai/gpt-oss-120b');
  await live.click();
  await expectStore(yap, 'config.json', (c) => c.meetingLiveTranscript === was);
  expect(yap.readJson('config.json').llmScopes.chat.model).toBe('openai/gpt-oss-120b');
  await closeSettings(main);
});

test('a small local model says it has no note tools, and answers from notes search', async ({
  yap,
  main,
  fakeLlm,
  shot,
}) => {
  const cfg = await yap.invoke('get_config');
  await setConfig(yap, main, {
    ppApiKeys: { groq: TEST_KEY },
    llmScopes: {
      ...cfg.llmScopes,
      chat: chatScope({ provider: 'local', baseUrl: fakeLlm.base, model: 'qwen2.5-1.5b-instruct' }),
    },
  });
  await openView(main, 'Chat');
  await expect(chip(main)).toHaveText('qwen2.5-1.5b-instruct · On this PC · no note tools');
  await expect(chip(main).locator('.mark')).toHaveAttribute('data-icon', 'qwen');
  await shot(main, '05-chip-no-note-tools');

  await expect(chip(main)).toHaveAttribute('title', /too small for Yap's note tools/);
  await chip(main).click();
  const pop = popover(main);
  // A server on this PC: the picker opens on On this PC, the server's models
  // under it, the current one first (its list doesn't have it).
  await expect(pop.getByRole('tab', { name: /^On this PC/ })).toHaveAttribute('aria-selected', 'true');
  const server = pop.getByRole('group', { name: `Self-hosted · ${new URL(fakeLlm.base).host}` });
  await expect(server.locator('.nm')).toHaveText(['qwen2.5-1.5b-instruct', 'fake-chat', 'llama3.1:8b', 'qwen2.5-7b-instruct']);
  const current = server.getByRole('button', { name: /^qwen2\.5-1\.5b-instruct/ });
  await expect(current).toHaveAttribute('aria-pressed', 'true');
  await expect(current.locator('.tag.warn')).toHaveText('no note tools');
  await shot(main, '06-popover-no-note-tools');
  await main.keyboard.press('Escape');
  await expect(pop).toBeHidden();

  // The answer: a plain chat call (no tools offered), from that model.
  fakeLlm.reply('other', () => 'From your notes: the venue is booked.');
  const before = fakeLlm.requests.length;
  await main.getByRole('button', { name: 'New chat' }).click();
  await send(main, 'Is the venue booked?');
  const thread = main.locator('.msgs');
  await expect(thread).toContainText('From your notes: the venue is booked.');
  const call = fakeLlm.requests.slice(before).at(-1);
  expect(call.tools).toEqual([]);
  expect(call.model).toBe('qwen2.5-1.5b-instruct');
  await expect(thread.locator('.byline')).toHaveText(['qwen2.5-1.5b-instruct · On this PC']);
});

test('with no AI model set up the chip says so, and the picker opens on the provider to connect', async ({ yap, main, shot }) => {
  const cfg = await yap.invoke('get_config');
  await setConfig(yap, main, {
    ppProvider: 'groq',
    ppApiKey: '',
    ppApiKeys: {},
    llmScopes: { ...cfg.llmScopes, chat: { ...cfg.llmScopes.chat, enabled: false, apiKey: '', apiKeys: {} } },
  });
  await openView(main, 'Chat');
  await expect(chip(main)).toHaveText('No AI model');
  await shot(main, '07-chip-no-ai-model');

  // Connecting one is right there: Groq (cleanup's provider), its key field.
  await chip(main).click();
  const pop = popover(main);
  await expect(pop.getByRole('tab', { name: /^Groq/ })).toHaveAttribute('aria-selected', 'true');
  await expect(pop.locator('.connect')).toContainText('Connect Groq');
  await expect(pop.getByLabel('Groq API key')).toBeFocused();

  // And Settings is one click away: Language Models → Chat.
  await pop.getByRole('button', { name: 'Language model settings…' }).click();
  const dialog = settingsDialog(main);
  await expect(dialog).toBeVisible();
  await expect(dialog.getByRole('heading', { name: 'Language Models' })).toBeVisible();
  await expect(dialog.getByRole('tab', { name: 'Chat' })).toHaveAttribute('aria-selected', 'true');
  await expect(popover(main)).toHaveCount(0);
  await closeSettings(main);
});

// Found by the maintainer (2026-10-18): Enter sent the message and threw the caret out
// of the box (it was disabled while the answer came), so every next message
// needed a click back in. Now the caret stays and you can type straight on.
test('Enter sends and the caret stays in the box for the next message', async ({ yap, main, fakeLlm }) => {
  await setConfig(yap, main, {
    ppApiKeys: { groq: TEST_KEY },
    llmScopes: { ...(await yap.invoke('get_config')).llmScopes, chat: groqChat(fakeLlm, 'openai/gpt-oss-20b') },
  });
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
  const box = main.getByRole('textbox', { name: 'Message' });
  const thread = main.locator('.msgs');
  await box.click();

  fakeLlm.scriptChat([{ content: 'First answer.' }]);
  await main.keyboard.type('First question');
  await main.keyboard.press('Enter');
  await expect(box).toBeFocused();
  await expect(thread).toContainText('First answer.');
  await expect(box).toBeFocused();

  // No click: the next message goes straight in.
  fakeLlm.scriptChat([{ content: 'Second answer.' }]);
  await main.keyboard.type('Second question');
  await expect(box).toHaveValue('Second question');
  await main.keyboard.press('Enter');
  await expect(thread).toContainText('Second answer.');
  await expect(box).toBeFocused();
  await expect(box).toHaveValue('');
});

// Chat answers show fenced code as a code block and links as links, and a
// link opens in the browser (the opener plugin), never in Yap's window. Seen
// in the Voice Agent screenshots (2026-10-18): ``` and [text](url) showed raw.
test('an answer shows code blocks and links, and a link opens in the browser', async ({ yap, main, fakeLlm, shot }) => {
  await setConfig(yap, main, {
    ppApiKeys: { groq: TEST_KEY },
    llmScopes: { ...(await yap.invoke('get_config')).llmScopes, chat: groqChat(fakeLlm, 'openai/gpt-oss-20b') },
  });
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
  fakeLlm.scriptChat([
    {
      content:
        'See [the plan](https://docs.example.com/plan?a=1&b=2) or https://example.org/app.\n\n```js\nconsole.log("<hi>");\n```\n\nNot a link: [x](javascript:alert(1))',
    },
  ]);
  await send(main, 'Where is the plan?');
  const answer = main.locator('.cbubble.assistant').last();
  await expect(answer.locator('pre.md-code code')).toHaveText('console.log("<hi>");');
  const link = answer.getByRole('link', { name: 'the plan' });
  await expect(link).toHaveAttribute('href', 'https://docs.example.com/plan?a=1&b=2');
  await expect(answer.getByRole('link', { name: 'https://example.org/app' })).toBeVisible();
  await expect(answer).toContainText('[x](javascript:alert(1))');
  await expect(answer.getByRole('link')).toHaveCount(2);
  await shot(main, '05-answer-code-and-links');

  // The click goes to Rust's open_external, which a test run records
  // instead of opening a browser; Yap's window stays on the app.
  const before = main.url();
  await link.click();
  await expect.poll(() => yap.invoke('e2e_opened_urls')).toContain('https://docs.example.com/plan?a=1&b=2');
  expect(main.url()).toBe(before);
});

// Groq's gpt-oss answers with tables; Chat showed the raw pipes (2026-10-21).
test('an answer with a table shows a table', async ({ main, fakeLlm, shot }) => {
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
  fakeLlm.scriptChat([
    {
      content:
        'Here are the plans:\n\n| Plan | Price | Usage |\n|:---|---:|:---:|\n| Plus | $8 | 1x |\n| Premium | $30 | 4x |\n| Super | $100 | 15x |\n\nAnnual billing is ten months for twelve.',
    },
  ]);
  await send(main, 'Compare the plans');
  const answer = main.locator('.cbubble.assistant').last();
  const table = answer.getByRole('table');
  await expect(table.getByRole('columnheader')).toHaveText(['Plan', 'Price', 'Usage']);
  await expect(table.getByRole('row')).toHaveCount(4);
  await expect(answer).not.toContainText('|');
  await shot(main, '06-answer-table');
});
