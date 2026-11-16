// 👍 / 👎 under Chat's answers (16 Nov 2026, the Claude app's thumbs: desktop/src-tauri/src/chat/feedback.rs,
// chat/chats.rs `set_feedback`; desktop/src/lib/views/chat/ChatMessage.svelte, FeedbackCard.svelte).
// Yap is local, so the feedback goes to no one: it's kept with the answer on this PC, and put to use here.
// - 👍 and 👎 between Copy and ↻: pressed, kept with the answer in chats.json (and its copy in chat_feedback.json);
//   pressed again, taken back. Light and dark.
// - 👎 opens its card: what was wrong (Not true, Didn't do what I asked, Too long, Too short, Wrong tone, Other with
//   a line). Retry with this note asks the question again with what was wrong given to the model — the question once,
//   never saved again — and both versions are kept, ‹ 1/2 ›, the old one still 👎. Light and dark.
// - Learn from this, only on a click: a line in memory's Preferences (memory's own toast, Undo), and the chat's style
//   (Concise for Too long), the Style chip following.
// - An older Yap's save (no `feedback` on its messages) loses no mark: the copy puts it back.
//
// The model is the local fake (support/fake-llm.js), scripted answer by answer. Every detail is fictional.
import { test as base, expect, openView, expectStore, setConfig } from './support/fixtures.js';
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
        name: 'chat-feedback',
        config: {
          llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat' } },
          meetingReminder: 'never',
          webSearch: false,
          speakReplies: false,
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
const up = (main) => thread(main).getByTestId('chat-thumbs-up');
const down = (main) => thread(main).getByTestId('chat-thumbs-down');
const card = (main) => thread(main).getByTestId('chat-feedback-card');
const count = (main) => thread(main).getByTestId('chat-branch-count');

async function freshChat(main) {
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
}

async function ask(main, fakeLlm, text, reply) {
  fakeLlm.scriptChat([{ content: reply }]);
  await box(main).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
  await expect(answers(main).last()).toHaveText(reply);
}

async function theme(yap, main, which) {
  await setConfig(yap, main, { theme: which });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe(which);
}

/** The conversation saved whose title starts with `title`. */
const saved = (yap, title) => yap.readJson('chats.json')?.conversations?.find((c) => c.title.startsWith(title));

/** The chat request (of those from `from` on) whose messages carry `words`: what the model got. */
function sentWith(fakeLlm, from, words) {
  const req = fakeLlm.requests.slice(from).find((r) => JSON.stringify(r.messages || []).includes(words));
  expect(req, `a request with "${words}"`).toBeTruthy();
  return JSON.stringify(req.messages);
}

test('👍 and 👎 sit between Copy and ↻; pressed, kept with the answer on this PC; pressed again, taken back', async ({ yap, main, fakeLlm, shot }) => {
  await freshChat(main);
  await ask(main, fakeLlm, 'When does the library open on Saturdays?', 'It opens at nine on Saturdays.');
  // The row's order: Read aloud, Copy, 👍, 👎, Retry (once the answer is in: its id, its ↻).
  await expect(thread(main).getByTestId('chat-retry')).toBeVisible();
  const row = thread(main).getByTestId('chat-retry').locator('xpath=..');
  await expect.poll(() => row.evaluate((r) => [...r.querySelectorAll('button')].map((b) => b.getAttribute('aria-label')))).toEqual(['Read aloud', 'Copy', 'Good answer', 'Not a good answer', 'Retry']);
  await expect(up(main)).toHaveAttribute('aria-pressed', 'false');

  await up(main).click();
  await expect(up(main)).toHaveAttribute('aria-pressed', 'true');
  await expect(down(main)).toHaveAttribute('aria-pressed', 'false');
  await expect(card(main)).toHaveCount(0);
  await expectStore(yap, 'chats.json', () => saved(yap, 'When does the library')?.messages?.[1]?.feedback?.rating === 'good');
  await expectStore(yap, 'chat_feedback.json', (m) => m?.answers?.length === 1 && m.answers[0].feedback.rating === 'good');
  await shot(main, '01-thumbs-up-light');
  await theme(yap, main, 'dark');
  await shot(main, '02-thumbs-up-dark');
  await theme(yap, main, 'light');

  // Shown again when the chat opens again.
  await main.getByRole('button', { name: 'New chat' }).click();
  await main.locator('.conv').filter({ hasText: 'When does the library' }).click();
  await expect(up(main)).toHaveAttribute('aria-pressed', 'true');

  // Pressed again: taken back, and its copy goes with it.
  await up(main).click();
  await expect(up(main)).toHaveAttribute('aria-pressed', 'false');
  await expectStore(yap, 'chats.json', () => saved(yap, 'When does the library')?.messages?.[1]?.feedback === undefined);
  await expect.poll(() => yap.readJson('chat_feedback.json')).toBeNull();
});

test('👎 opens its card: what was wrong, and Retry with this note gives the model that, both versions kept', async ({ yap, main, fakeLlm, shot }) => {
  test.setTimeout(120_000);
  await freshChat(main);
  const old = 'The museum is open every day from 8am until 10pm, with late openings on Fridays, free entry for everyone, a café on the roof, guided tours every hour and a gift shop by the main door.';
  await ask(main, fakeLlm, 'When is the town museum open?', old);

  await down(main).click();
  await expect(down(main)).toHaveAttribute('aria-pressed', 'true');
  await expect(card(main)).toBeVisible();
  await expect(card(main).getByRole('button')).toContainText(['Not true', 'Didn’t do what I asked', 'Too long', 'Too short', 'Wrong tone', 'Other']);
  await expect(card(main)).toContainText('Kept on this PC: Yap sends it nowhere.');
  await expectStore(yap, 'chats.json', () => saved(yap, 'When is the town museum')?.messages?.[1]?.feedback?.rating === 'bad');
  await shot(main, '03-thumbs-down-card-light');

  // Reasons as they're picked; Other opens a line.
  await card(main).getByTestId('chat-feedback-reason-long').click();
  await card(main).getByTestId('chat-feedback-reason-untrue').click();
  await expect(card(main).getByTestId('chat-feedback-note')).toHaveCount(0);
  await card(main).getByTestId('chat-feedback-reason-other').click();
  const note = card(main).getByTestId('chat-feedback-note');
  await expect(note).toBeFocused();
  await note.fill('It shuts at 5pm and is closed on Mondays');
  await expect(card(main).getByTestId('chat-feedback-reason-long')).toHaveAttribute('aria-pressed', 'true');
  await theme(yap, main, 'dark');
  await shot(main, '04-thumbs-down-reasons-dark');
  await theme(yap, main, 'light');
  await expectStore(yap, 'chats.json', () => saved(yap, 'When is the town museum')?.messages?.[1]?.feedback?.reasons?.join(',') === 'untrue,long,other');

  // Retry with this note: the question answered again, the model told what was wrong; the question once.
  const from = fakeLlm.requests.length;
  fakeLlm.scriptChat([{ content: 'It opens 10am to 5pm, Tuesday to Sunday.' }]);
  await card(main).getByTestId('chat-feedback-retry').click();
  await expect(answers(main)).toHaveText(['It opens 10am to 5pm, Tuesday to Sunday.']);
  await expect(card(main)).toHaveCount(0);
  await expect(questions(main)).toHaveText(['When is the town museum open?']);
  const sent = sentWith(fakeLlm, from, 'When is the town museum open?');
  expect(sent.split('When is the town museum open?').length - 1).toBe(1);
  for (const words of ['Answering again', 'it was too long', "it said something that isn't true", 'It shuts at 5pm and is closed on Mondays', '<earlier-answer>', 'with late openings on Fridays']) {
    expect(sent, words).toContain(words);
  }
  // Both kept: ‹ 2/2 ›, the new one unrated; ‹ back to the old one, still 👎 with its reasons.
  await expect(count(main)).toHaveText('2/2');
  await expect(down(main)).toHaveAttribute('aria-pressed', 'false');
  await shot(main, '05-retried-with-note-2-of-2');
  await thread(main).getByTestId('chat-branch-prev').click();
  await expect(answers(main)).toHaveText([old]);
  await expect(down(main)).toHaveAttribute('aria-pressed', 'true');
  await expectStore(yap, 'chats.json', (s) => {
    const c = s.conversations.find((x) => x.title.startsWith('When is the town museum'));
    return c?.messages?.[1]?.feedback?.note === 'It shuts at 5pm and is closed on Mondays' && c.branches?.[0]?.others?.[0]?.messages?.[0]?.text?.startsWith('It opens 10am');
  });
  // The question as saved: as it was asked, never the note.
  expect(JSON.stringify(saved(yap, 'When is the town museum').messages[0].text)).not.toContain('Answering again');

  // ↻ on an answer marked 👎 is a Retry with its note too.
  await thread(main).getByTestId('chat-branch-next').click();
  await expect(answers(main)).toHaveText(['It opens 10am to 5pm, Tuesday to Sunday.']);
  await expect(down(main)).toHaveAttribute('aria-pressed', 'false');
  await down(main).click();
  await card(main).getByTestId('chat-feedback-reason-short').click();
  await card(main).getByTestId('chat-feedback-done').click();
  await expect(card(main)).toHaveCount(0);
  await expect(thread(main).getByTestId('chat-retry')).toHaveAttribute('aria-label', 'Retry with your note');
  const again = fakeLlm.requests.length;
  fakeLlm.scriptChat([{ content: 'Tuesday to Sunday, 10am to 5pm; last entry 4:30pm. Closed on Mondays.' }]);
  await thread(main).getByTestId('chat-retry').click();
  await expect(answers(main)).toHaveText(['Tuesday to Sunday, 10am to 5pm; last entry 4:30pm. Closed on Mondays.']);
  expect(sentWith(fakeLlm, again, 'When is the town museum open?')).toContain('it was too short');
  await expect(count(main)).toHaveText('3/3');
});

test('Learn from this keeps a line in Preferences and switches the chat’s style, each only on a click', async ({ yap, main, fakeLlm, shot }) => {
  await freshChat(main);
  await ask(main, fakeLlm, 'How do I repot a fern?', 'Repotting a fern is a rewarding job. First, a little history of ferns…');
  await down(main).click();
  // Nothing to learn yet: no reason a lasting one.
  await card(main).getByTestId('chat-feedback-reason-untrue').click();
  await expect(card(main).getByTestId('chat-feedback-learn')).toHaveCount(0);
  await card(main).getByTestId('chat-feedback-reason-long').click();
  await card(main).getByTestId('chat-feedback-learn').click();
  const fact = card(main).getByTestId('chat-feedback-fact');
  await expect(fact).toHaveValue('Prefers shorter, more direct answers');
  await expect(card(main).getByTestId('chat-feedback-style')).toHaveText('Use Concise in this chat');
  await shot(main, '06-learn-from-this');
  // Nothing kept until it's pressed.
  expect(JSON.stringify(yap.readJson('memory.json') ?? {})).not.toContain('Prefers shorter');

  await fact.fill('Prefers short answers, steps first');
  await card(main).getByTestId('chat-feedback-remember').click();
  await expect(card(main).getByTestId('chat-feedback-remember')).toHaveText('Remembered');
  await expectStore(yap, 'memory.json', (m) => m?.items?.some((i) => i.section === 'preferences' && i.details?.includes('Prefers short answers, steps first') && i.source?.kind === 'tell'));

  await card(main).getByTestId('chat-feedback-style').click();
  await expect(main.getByTestId('chat-style-chip')).toHaveAttribute('aria-label', 'Style: Concise');
  await expect(card(main).getByTestId('chat-feedback-style')).toHaveText('Concise in this chat');
  await shot(main, '07-learned');
  await theme(yap, main, 'dark');
  await shot(main, '08-learned-dark');
  await theme(yap, main, 'light');
});

// ---- the saved conversation: an older Yap saved it (its own instance)

const NOW = Math.floor(Date.now() / 1000);
const m = (role, text, ts) => ({ role, text, ts });
const BUS = {
  id: 31,
  title: 'Bus to the coast',
  createdTs: NOW - 900,
  updatedTs: NOW - 600,
  messages: [m('user', 'Which bus goes to the coast?', NOW - 900), m('assistant', 'The 42, every ten minutes.', NOW - 890)],
};

test('an older Yap’s save loses no mark: chat_feedback.json puts it back on its answer', async ({ shot }) => {
  test.setTimeout(120_000);
  const answer = BUS.messages[1];
  const yap = await launchYap({
    name: 'chat-feedback-saved',
    data: {
      // An older Yap saved the chat: no `feedback` on its answer.
      'chats.json': { conversations: [BUS] },
      'chat_feedback.json': { version: 1, answers: [{ conversation: 31, ts: answer.ts, chars: answer.text.length, feedback: { rating: 'bad', reasons: ['untrue'], note: 'It is the 43 now', ts: NOW - 700 } }] },
    },
    config: { meetingReminder: 'never' },
  });
  try {
    const main = yap.main;
    await openView(main, 'Chat');
    await main.locator('.conv').filter({ hasText: 'Bus to the coast' }).click();
    await expect(answers(main)).toHaveText(['The 42, every ten minutes.']);
    await expect(down(main)).toHaveAttribute('aria-pressed', 'true');
    await expect(up(main)).toHaveAttribute('aria-pressed', 'false');
    // ↻ knows it: a Retry there gives the model what was said.
    await expect(thread(main).getByTestId('chat-retry')).toHaveAttribute('aria-label', 'Retry with your note');
    await shot(main, '09-older-save-restored');
  } finally {
    await yap.stop();
  }
});
