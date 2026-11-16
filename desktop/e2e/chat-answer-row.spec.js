// The rows under Chat's messages, as the Claude app's (16 Nov 2026: desktop/src/lib/views/chat/ChatMessage.svelte,
// ChatThread.svelte, ui/actionTip.js, core/whenSaid.js; chat/commands.rs `chat_fork` / `chat_chapter_pin`,
// chat/cards/chapters.rs `pin_title`).
// - Under an answer: Copy, Fork from here, Pin as chapter, Read aloud (a speaker), then its time ("just now") and its
//   model — never 👍 / 👎 or ↻ Retry (taken out the same day). Under the person's message, on hover: Copy and Edit.
//   Each button's tooltip as the app's: its name, and what it does. Light and dark.
// - Their size: the Claude app's own, measured with Yap beside it on one screen (24 px squares, 16.5 px icons, the time
//   13.5 px), in step with the reading size from there.
// - On hover, as the app's: every row (the latest's too) shows on hovering its message or on a keyboard focus in it,
//   and always with Settings → Appearance → Message actions → Always show.
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
import { test as base, expect, openView, setConfig, openSettings, closeSettings, expectStore } from './support/fixtures.js';
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

test('its size is the Claude app’s, measured beside it on one screen: 24 px squares, 16.5 px icons, 13.5 px time, in step with the reading size', async ({ yap, main, fakeLlm }) => {
  await freshChat(main);
  await ask(main, fakeLlm, 'Is a 35-litre rucksack enough?', 'Yes, for two days.');
  const measure = (which) =>
    thread(main)
      .getByTestId(which)
      .last()
      .evaluate((row) => {
        const text = parseFloat(getComputedStyle(row.closest('.msgsin').querySelector('.cbubble.assistant')).fontSize);
        const b = row.querySelector('button.act').getBoundingClientRect();
        const icon = row.querySelector('button.act svg').getBoundingClientRect();
        const t = row.querySelector('time.when');
        return { text, square: b.height, wide: b.width, icon: icon.width, when: t ? parseFloat(getComputedStyle(t).fontSize) : null };
      });
  // At the default reading size: the Claude app's own (a screenshot of Yap and the app side by side, one Windows scale).
  let m = await measure('chat-answer-actions');
  expect(m).toMatchObject({ text: 12.5, square: 24, wide: 24, icon: 16.5, when: 13.5 });
  // The person's message row: the same squares (Copy, Edit).
  const u = await measure('chat-message-actions');
  expect([u.square, u.icon]).toEqual([24, 16.5]);
  // Larger reading: in step with it.
  await setConfig(yap, main, { readingSize: 'larger' });
  await expect.poll(async () => (await measure('chat-answer-actions')).text).toBeGreaterThan(12.5);
  m = await measure('chat-answer-actions');
  const k = m.text / 12.5;
  expect(m.square).toBeCloseTo(24 * k, 0);
  expect(m.icon).toBeCloseTo(16.5 * k, 0);
  expect(m.when).toBeCloseTo(13.5 * k, 0);
  await setConfig(yap, main, { readingSize: 'default' });
});

test('on hover, as the Claude app’s: a row shows on hovering its message or a keyboard focus in it, the latest’s too; always with Message actions → Always show', async ({ yap, main, fakeLlm, shot }) => {
  await freshChat(main);
  await ask(main, fakeLlm, 'Do I need a map for the ridge path?', 'Yes, and a compass.');
  await ask(main, fakeLlm, 'Is the summit busy at dawn?', 'Rarely before eight.');
  const shown = (row) => row.locator('.reveal').first();
  const urows = thread(main).getByTestId('chat-message-actions');
  // Pointing elsewhere (the message box): no row shows, the latest's neither.
  await box(main).hover();
  for (const row of [rows(main).first(), rows(main).last(), urows.first(), urows.last()]) await expect(shown(row)).toHaveCSS('opacity', '0');
  await shot(main, '06-rows-at-rest');
  // Hovering the latest answer: its row, no other.
  await answers(main).last().hover();
  await expect(shown(rows(main).last())).toHaveCSS('opacity', '1');
  await expect(shown(rows(main).first())).toHaveCSS('opacity', '0');
  await expect(shown(urows.last())).toHaveCSS('opacity', '0');
  // …and from the bubble down to its row, the row stays (the pointer crosses the gap between them).
  await rows(main).last().getByTestId('chat-answer-copy').hover();
  await expect(shown(rows(main).last())).toHaveCSS('opacity', '1');
  await shot(main, '07-answer-row-hovered');
  // Hovering a question: its Copy and Edit.
  await questions(main).first().hover();
  await expect(shown(urows.first())).toHaveCSS('opacity', '1');
  await expect(shown(rows(main).last())).toHaveCSS('opacity', '0');
  // The keyboard: Tab from the answer reaches its Copy, and its row shows while the pointer's elsewhere.
  await box(main).hover();
  await answers(main).first().focus();
  await main.keyboard.press('Tab');
  await expect(rows(main).first().getByTestId('chat-answer-copy')).toBeFocused();
  await expect(shown(rows(main).first())).toHaveCSS('opacity', '1');
  await expect(shown(rows(main).last())).toHaveCSS('opacity', '0');
  await main.keyboard.press('Shift+Tab');
  await expect(answers(main).first()).toBeFocused();
  await expect(shown(rows(main).first())).toHaveCSS('opacity', '1');
  // A click's focus (selecting the answer's words) leaves it at rest once the pointer goes.
  await answers(main).last().click();
  await box(main).hover();
  await expect(shown(rows(main).last())).toHaveCSS('opacity', '0');

  // Settings → General → Appearance → Message actions → Always show: every row shows, hovered or not.
  const dialog = await openSettings(main, 'General');
  const group = dialog.getByRole('group', { name: 'Appearance' });
  const choice = group.getByRole('tablist', { name: 'Message actions' });
  await expect(choice.getByRole('tab', { name: 'Show on hover', exact: true })).toHaveAttribute('aria-selected', 'true');
  await choice.getByRole('tab', { name: 'Always show', exact: true }).click();
  await expectStore(yap, 'config.json', (c) => c.messageActions === 'always', 'Always show to be saved');
  await expect(dialog.locator('[data-setting="message-actions"]')).toContainText('always show under each message');
  await shot(main, '08-message-actions-setting');
  await closeSettings(main);
  await box(main).hover();
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.messageActions)).toBe('always');
  for (const row of [rows(main).first(), rows(main).last(), urows.first(), urows.last()]) await expect(shown(row)).toHaveCSS('opacity', '1');
  await shot(main, '09-rows-always');
  await theme(yap, main, 'dark');
  await shot(main, '10-rows-always-dark');
  await theme(yap, main, 'light');
  // Back to Show on hover.
  await setConfig(yap, main, { messageActions: 'hover' });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.messageActions ?? '')).toBe('');
  await expect(shown(rows(main).last())).toHaveCSS('opacity', '0');
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
  // Under its question, above the answer (each message's parts in one box-less group, .chatmsg).
  expect(await divider.evaluate((d) => !!d.nextElementSibling?.querySelector('.cbubble.assistant'))).toBe(true);
  expect(await divider.evaluate((d) => d.previousElementSibling?.querySelector('.cbubble.user')?.innerText.trim())).toBe('How much should we budget a day?');
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
