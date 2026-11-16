// The rows under Chat's messages, as the Claude app's (16 Nov 2026: desktop/src/lib/views/chat/ChatMessage.svelte,
// ChatThread.svelte, ui/actionTip.js, core/whenSaid.js; chat/commands.rs `chat_fork` / `chat_chapter_pin`,
// chat/cards/chapters.rs `pin_title`).
// - Under an answer: Copy, Fork from here, Pin as chapter, Read aloud (a speaker), then its time ("just now") and its
//   model — never 👍 / 👎 or ↻ Retry (taken out the same day). Under the person's message, on hover: Copy and Edit.
//   Each button's tooltip as the app's: its name, and what it does. Light and dark.
// - Their size: in the app's proportions to the answer's own text, as measured on its screenshots (each square about
//   1.6 of the text, its icon about 1.04, the time about 0.82), at any reading size.
// - Copy copies the answer as text; Read aloud reads it (■ to stop).
// - Fork from here: a new chat with the conversation up to that answer, opened; the original as it was.
// - Pin as chapter: the answer's turn starts a chapter, named by Chat's model as its own chapters are — a divider
//   above the answer (under its question) and an entry in the contents, the pin filled; a second click takes it off.
// - A chat saved with the thumbs of that day (`feedback` on an answer, `chat_feedback.json`) still opens, and the
//   file is left as it was.
//
// The model is the local fake (support/fake-llm.js), scripted answer by answer. Every detail is fictional.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, setConfig } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { launchYap } from './support/yap.js';

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
        name: 'chat-answer-row',
        config: {
          llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat' } },
          meetingReminder: 'never',
          webSearch: false,
          speakReplies: false,
          // (Nothing of memory's in the background: the chapter's name is the next call the fake answers.)
          memoryLearnChats: false,
        },
      });
    },
    { scope: 'worker' },
  ],
});

const box = (main) => main.getByRole('textbox', { name: 'Message' });
const thread = (main) => main.locator('.msgs');
const questions = (main) => thread(main).locator('.cbubble.user');
const answers = (main) => thread(main).locator('.cbubble.assistant');
const rows = (main) => thread(main).getByTestId('chat-answer-actions');
const labels = (row) => row.locator('button').evaluateAll((bs) => bs.map((b) => b.getAttribute('aria-label')));

async function freshChat(main) {
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
}

async function ask(main, fakeLlm, text, reply) {
  fakeLlm.scriptChat([{ content: reply }]);
  await box(main).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
  // (As rendered: its markdown marks drawn, not shown.)
  await expect(answers(main).last()).toHaveText(reply.replaceAll('**', ''));
}

async function theme(yap, main, which) {
  await setConfig(yap, main, { theme: which });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe(which);
}

/** The page's clipboard, stubbed (the real one is never touched). */
async function stubClipboard(page) {
  await page.evaluate(() => {
    window.__copied = null;
    navigator.clipboard.writeText = async (t) => {
      window.__copied = t;
    };
  });
  return () => page.evaluate(() => window.__copied);
}

const activeConv = (main) => main.locator('.conv.active').getAttribute('data-conv').then(Number);

test('under an answer: Copy, Fork from here, Pin as chapter, Read aloud and its time, no thumbs or Retry; Copy and Edit under the question', async ({ yap, main, fakeLlm, shot }) => {
  await freshChat(main);
  await ask(main, fakeLlm, 'What should I pack for a weekend of hill walking?', 'Pack **layers**, a waterproof and a flask.');
  const row = rows(main).last();
  await expect.poll(() => labels(row)).toEqual(['Copy', 'Fork from here', 'Pin as chapter', 'Read aloud']);
  await expect(row.locator('time.when')).toHaveText('just now');
  await expect(row.locator('.byline')).toContainText('fake-chat');
  for (const gone of ['chat-retry', 'chat-thumbs-up', 'chat-thumbs-down', 'chat-feedback-card']) await expect(main.getByTestId(gone)).toHaveCount(0);
  // The app's tooltips: the name, and what it does.
  await row.getByTestId('chat-answer-fork').hover();
  await expect(main.getByTestId('action-tip')).toContainText('Fork from here');
  await expect(main.getByTestId('action-tip')).toContainText('Starts a new chat from this point. This one stays as it is.');
  await shot(main, '01-answer-row-light');
  await row.getByTestId('chat-pin-chapter').hover();
  await expect(main.getByTestId('action-tip')).toHaveText('Pin as chapter');
  // Under the question, on hover: Copy and Edit (no ticks, no Rewind: those are a Code chat's).
  await questions(main).first().hover();
  const urow = thread(main).getByTestId('chat-message-actions').first();
  await expect.poll(() => labels(urow)).toEqual(['Copy', 'Edit']);
  await expect(urow.getByTestId('chat-read')).toHaveCount(0);
  await theme(yap, main, 'dark');
  await row.getByTestId('chat-answer-copy').hover();
  await expect(main.getByTestId('action-tip')).toHaveText('Copy');
  await shot(main, '02-answer-row-dark');
  await theme(yap, main, 'light');

  // Copy: the answer as text; Read aloud: read (■ Stop reading while it is).
  const copied = await stubClipboard(main);
  await row.getByTestId('chat-answer-copy').click();
  await expect.poll(copied).toBe('Pack layers, a waterproof and a flask.');
  await yap.invoke('tts_e2e_spoken', { clear: true });
  await row.getByRole('button', { name: 'Read aloud', exact: true }).click();
  await expect.poll(() => yap.invoke('tts_e2e_spoken').then((s) => s.length)).toBeGreaterThan(0);
  await main.keyboard.press('Escape');
});

test('its size is the Claude app’s, in proportion to the answer’s text, at any reading size', async ({ yap, main, fakeLlm }) => {
  await freshChat(main);
  await ask(main, fakeLlm, 'Is a 35-litre rucksack enough?', 'Yes, for two days.');
  const measure = () =>
    rows(main)
      .last()
      .evaluate((row) => {
        const text = parseFloat(getComputedStyle(row.closest('.msgsin').querySelector('.cbubble.assistant')).fontSize);
        const b = row.querySelector('button.act').getBoundingClientRect();
        const icon = row.querySelector('button.act svg').getBoundingClientRect();
        const when = parseFloat(getComputedStyle(row.querySelector('time.when')).fontSize);
        return { text, square: b.height / text, icon: icon.width / text, when: when / text };
      });
  let last = 0;
  for (const size of ['default', 'larger']) {
    await setConfig(yap, main, { readingSize: size });
    await expect.poll(async () => (await measure()).text).not.toBe(last);
    const m = await measure();
    last = m.text;
    // (The app: a 24 px square and a 12 px icon under 15.7 px text, its time 12.8 px.)
    expect(m.square).toBeGreaterThan(1.5);
    expect(m.square).toBeLessThan(1.7);
    expect(m.icon).toBeGreaterThan(0.95);
    expect(m.icon).toBeLessThan(1.1);
    expect(m.when).toBeGreaterThan(0.78);
    expect(m.when).toBeLessThan(0.86);
  }
  await setConfig(yap, main, { readingSize: 'default' });
});

test('Fork from here under an answer: a new chat up to that answer, opened; the original as it was', async ({ yap, main, fakeLlm, shot }) => {
  await freshChat(main);
  await ask(main, fakeLlm, 'Which bus goes to the harbour?', 'The 42, every ten minutes.');
  await ask(main, fakeLlm, 'And on Sundays?', 'Every twenty minutes.');
  const original = await activeConv(main);
  await rows(main).first().getByTestId('chat-answer-fork').click();
  await expect.poll(() => activeConv(main), { timeout: 10_000 }).not.toBe(original);
  await expect(questions(main)).toHaveText(['Which bus goes to the harbour?']);
  await expect(answers(main)).toHaveText(['The 42, every ten minutes.']);
  const fork = await activeConv(main);
  expect((await yap.invoke('chat_get', { id: fork })).title).toMatch(/\(fork\)$/);
  expect((await yap.invoke('chat_get', { id: original })).messages).toHaveLength(4);
  await shot(main, '03-forked');
  // The fork goes on by itself, with the history up to there.
  const from = fakeLlm.requests.length;
  await ask(main, fakeLlm, 'And at night?', 'Until midnight.');
  const sent = JSON.stringify(fakeLlm.requests.slice(from).find((r) => JSON.stringify(r.messages || []).includes('And at night?')).messages);
  expect(sent).toContain('The 42, every ten minutes.');
  expect(sent).not.toContain('Every twenty minutes.');
  expect((await yap.invoke('chat_get', { id: original })).messages).toHaveLength(4);
});

test('Pin as chapter: a title above the answer and in the contents, the pin filled; a second click takes it off', async ({ yap, main, fakeLlm, shot }) => {
  await freshChat(main);
  await ask(main, fakeLlm, 'Where should we stay in Lisbon?', 'Alfama, for the views.');
  await ask(main, fakeLlm, 'How much should we budget a day?', 'About 90 euros each.');
  const pin = rows(main).last().getByTestId('chat-pin-chapter');
  await expect(pin).toHaveAttribute('aria-pressed', 'false');
  // Named by Chat's model, as its own chapters are (a short noun phrase; its quotes and full stop go).
  fakeLlm.scriptChat([{ content: '"Trip budget."', plain: true }]);
  await pin.click();
  await expect(pin).toHaveAttribute('aria-pressed', 'true', { timeout: 20_000 });
  const divider = thread(main).getByTestId('chat-chapter');
  await expect(divider).toHaveText('Trip budget');
  // Under its question, above the answer.
  expect(await divider.evaluate((d) => d.nextElementSibling?.classList.contains('assistant'))).toBe(true);
  expect(await divider.evaluate((d) => d.previousElementSibling?.matches('[data-testid="chat-message-actions"]'))).toBe(true);
  // In the contents, after the chat's start.
  await main.getByTestId('chat-chapters').hover();
  await expect(main.getByTestId('chat-chapters-item')).toHaveText(['Trip budget']);
  const conv = await activeConv(main);
  expect((await yap.invoke('chat_get', { id: conv })).messages[3].chapter).toEqual({ title: 'Trip budget', pinned: true });
  await shot(main, '04-pinned-light');
  await theme(yap, main, 'dark');
  await shot(main, '05-pinned-dark');
  await theme(yap, main, 'light');
  // The answer's menu says it too.
  await answers(main).last().click({ button: 'right', position: { x: 8, y: 8 } });
  await expect(main.getByRole('menuitem', { name: 'Unpin chapter' })).toBeVisible();
  await main.keyboard.press('Escape');
  // Unpinned: gone from the thread, the contents and the chat.
  await pin.click();
  await expect(divider).toHaveCount(0);
  await expect(pin).toHaveAttribute('aria-pressed', 'false');
  await expect(main.getByTestId('chat-chapters')).toHaveCount(0);
  expect((await yap.invoke('chat_get', { id: conv })).messages[3].chapter).toBeUndefined();
});

// ---- a chat saved with the thumbs of that day (its own instance)

const NOW = Math.floor(Date.now() / 1000);
const BUSES = {
  id: 31,
  title: 'Buses to the harbour',
  createdTs: NOW - 900,
  updatedTs: NOW - 600,
  messages: [
    { role: 'user', text: 'Which bus goes to the harbour?', ts: NOW - 900 },
    { role: 'assistant', text: 'The 42, every ten minutes.', ts: NOW - 720, feedback: { rating: 'bad', reasons: ['untrue'], note: 'It is the 43 now', ts: NOW - 700 } },
  ],
};
const MARKS = { version: 1, conversations: [{ id: 31, marks: [{ ts: NOW - 720, len: 26, feedback: { rating: 'bad', reasons: ['untrue'], note: 'It is the 43 now' } }] }] };

test('a chat saved with a thumb still opens, its answer row as any; the thumbs’ file is left as it was', async ({ shot }) => {
  test.setTimeout(120_000);
  const app = await launchYap({ name: 'chat-answer-row-old', data: { 'chats.json': { conversations: [BUSES] }, 'chat_feedback.json': MARKS }, config: { meetingReminder: 'never' } });
  try {
    const main = app.main;
    const file = path.join(app.dataDir, 'chat_feedback.json');
    const before = fs.readFileSync(file, 'utf8');
    await openView(main, 'Chat');
    await main.locator('.conv').filter({ hasText: 'Buses to the harbour' }).click();
    await expect(answers(main)).toHaveText(['The 42, every ten minutes.']);
    await expect.poll(() => labels(rows(main).last())).toEqual(['Copy', 'Fork from here', 'Pin as chapter', 'Read aloud']);
    await expect(rows(main).last().locator('time.when')).toHaveText(/^1[2-5] minutes ago$/);
    await expect(main.getByTestId('chat-thumbs-down')).toHaveCount(0);
    await shot(main, '06-old-chat-opens');
    // A save of the chat (its answer pinned, words alone: no model here) leaves the thumbs' file as it was.
    await rows(main).last().getByTestId('chat-pin-chapter').click();
    await expect(thread(main).getByTestId('chat-chapter')).toHaveText('Which bus goes to the harbour');
    await expect.poll(() => app.readJson('chats.json')?.conversations?.[0]?.messages?.[1]?.chapter?.pinned ?? false).toBe(true);
    expect(fs.existsSync(file)).toBe(true);
    expect(fs.readFileSync(file, 'utf8')).toBe(before);
  } finally {
    await app.stop();
  }
});
