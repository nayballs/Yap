// Retry, Edit and ‹ 1/2 › (15 Nov 2026, the Claude app's message actions: desktop/src-tauri/src/chat/chats/branches.rs,
// chat/commands.rs `chat_retry` / `chat_send`'s `edit` / `chat_branch`; desktop/src/lib/views/chat/BranchSwitch.svelte,
// MessageEdit.svelte).
// - Retry under an answer: its question answered again, never asked twice; the old answer and what followed kept as
//   a version, ‹ 2/2 › on the new one, ‹ back to the first.
// - Edit on the person's message: open in place with its words (Esc leaves it as it was); Save and send asks the new
//   words in its place, ‹ 2/2 › on it, the old message, its answer and the follow-up one switch away.
// - What the model gets is the branch shown: a question after switching goes with that branch's history only.
// - A retry that gets no answer puts the old one back.
// - The saved conversation: chats.json's `messages` is the branch shown, the others in `branches` and copied to
//   chat_branches.json; after a restart the versions are there, as switched; an older Yap's save (no `branches`)
//   loses none that still fit: the copy puts them back.
//
// The model is the local fake (support/fake-llm.js), scripted answer by answer.
import { test as base, expect, openView, expectStore } from './support/fixtures.js';
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
        name: 'chat-branches',
        config: {
          llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat' } },
          meetingReminder: 'never',
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
const count = (main) => thread(main).getByTestId('chat-branch-count');

async function freshChat(main) {
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
}

async function send(main, text) {
  await box(main).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
}

/** The chat request (of those from `from` on) whose messages carry `words`: what the model got. */
function sentWith(fakeLlm, from, words) {
  const req = fakeLlm.requests.slice(from).find((r) => JSON.stringify(r.messages || []).includes(words));
  expect(req, `a request with "${words}"`).toBeTruthy();
  return JSON.stringify(req.messages);
}

/** The conversation saved whose title starts with `title`. */
const saved = (yap, title) => yap.readJson('chats.json').conversations.find((c) => c.title.startsWith(title));

test('Retry answers the question again and keeps both answers, ‹ 1/2 › to switch between them', async ({ yap, main, fakeLlm, shot }) => {
  await freshChat(main);
  fakeLlm.scriptChat([{ content: 'Pack warm socks.' }]);
  await send(main, 'What should I pack for the trip?');
  await expect(answers(main)).toHaveText(['Pack warm socks.']);
  await expect(count(main)).toHaveCount(0);

  const from = fakeLlm.requests.length;
  fakeLlm.scriptChat([{ content: 'Pack walking boots.' }]);
  await answers(main).first().hover();
  await thread(main).getByTestId('chat-retry').click();
  await expect(answers(main)).toHaveText(['Pack walking boots.']);
  // The question once — in the thread and to the model — and the old answer never in what it got.
  await expect(questions(main)).toHaveText(['What should I pack for the trip?']);
  const sent = sentWith(fakeLlm, from, 'What should I pack for the trip?');
  expect(sent.split('What should I pack for the trip?').length - 1).toBe(1);
  expect(sent).not.toContain('Pack warm socks.');
  await expect(count(main)).toHaveText('2/2');
  await shot(main, '01-retried-2-of-2');

  // ‹ back to the first answer; › to the second.
  await thread(main).getByTestId('chat-branch-prev').click();
  await expect(answers(main)).toHaveText(['Pack warm socks.']);
  await expect(count(main)).toHaveText('1/2');
  await expect(thread(main).getByTestId('chat-branch-prev')).toBeDisabled();
  await shot(main, '02-switched-1-of-2');

  // Saved: the branch shown is the conversation's messages, the other kept in `branches` and in its copy.
  await expectStore(yap, 'chats.json', (s) => {
    const c = s.conversations.find((x) => x.title.startsWith('What should I pack'));
    return c?.messages.map((m) => m.text).join('|') === 'What should I pack for the trip?|Pack warm socks.' && c.branches?.[0]?.others?.[0]?.messages?.[0]?.text === 'Pack walking boots.';
  });
  expect(saved(yap, 'What should I pack').forks).toBeUndefined();
  await expectStore(yap, 'chat_branches.json', (m) => m.conversations?.some((c) => c.forks?.[0]?.at === 1));

  // A third: ‹ 3/3 ›, the one before still a click away.
  fakeLlm.scriptChat([{ content: 'Pack a rain jacket.' }]);
  await thread(main).getByTestId('chat-retry').click();
  await expect(answers(main)).toHaveText(['Pack a rain jacket.']);
  await expect(count(main)).toHaveText('3/3');
  await thread(main).getByTestId('chat-branch-prev').click();
  await expect(answers(main)).toHaveText(['Pack walking boots.']);
  await expect(count(main)).toHaveText('2/3');
});

test('Edit opens the message in place; Save and send asks the new words, ‹ 1/2 › on it, and the model gets the branch shown', async ({ yap, main, fakeLlm, shot }) => {
  await freshChat(main);
  fakeLlm.scriptChat([{ content: 'Try the harbour walk.' }, { content: 'Bring a light jacket.' }]);
  await send(main, 'Ideas for Saturday?');
  await expect(answers(main)).toHaveText(['Try the harbour walk.']);
  await send(main, 'What should I wear?');
  await expect(answers(main)).toHaveText(['Try the harbour walk.', 'Bring a light jacket.']);

  // Edit: its words in place; Esc leaves it as it was.
  const first = questions(main).first();
  await first.hover();
  await thread(main).getByTestId('chat-edit-message').first().click();
  const input = thread(main).getByTestId('chat-edit-input');
  await expect(input).toHaveValue('Ideas for Saturday?');
  await expect(input).toBeFocused();
  await shot(main, '03-edit-open');
  await input.press('Escape');
  await expect(input).toHaveCount(0);
  await expect(questions(main)).toHaveText(['Ideas for Saturday?', 'What should I wear?']);

  // New words, Save and send: asked in its place, the old ones set aside.
  await questions(main).first().hover();
  await thread(main).getByTestId('chat-edit-message').first().click();
  await input.fill('Ideas for a rainy Saturday?');
  const from = fakeLlm.requests.length;
  fakeLlm.scriptChat([{ content: 'Visit the museum.' }]);
  await thread(main).getByTestId('chat-edit-save').click();
  await expect(answers(main)).toHaveText(['Visit the museum.']);
  await expect(questions(main)).toHaveText(['Ideas for a rainy Saturday?']);
  const sent = sentWith(fakeLlm, from, 'Ideas for a rainy Saturday?');
  expect(sent).not.toContain('harbour walk');
  expect(sent).not.toContain('What should I wear?');
  await expect(count(main)).toHaveText('2/2');
  await shot(main, '04-edited-2-of-2');

  // ‹ back: the old message, its answer and the follow-up.
  await thread(main).getByTestId('chat-branch-prev').click();
  await expect(questions(main)).toHaveText(['Ideas for Saturday?', 'What should I wear?']);
  await expect(answers(main)).toHaveText(['Try the harbour walk.', 'Bring a light jacket.']);
  await expect(count(main)).toHaveText('1/2');

  // A question now goes with this branch's history, never the other's.
  const next = fakeLlm.requests.length;
  fakeLlm.scriptChat([{ content: 'A scarf, too.' }]);
  await send(main, 'Anything else?');
  await expect(answers(main).last()).toHaveText('A scarf, too.');
  const later = sentWith(fakeLlm, next, 'Anything else?');
  expect(later).toContain('Bring a light jacket.');
  expect(later).not.toContain('museum');
  await expectStore(yap, 'chats.json', (s) => s.conversations.find((c) => c.title.startsWith('Ideas for Saturday'))?.messages.length === 6);
  // The other version kept, as it was answered.
  await thread(main).getByTestId('chat-branch-next').click();
  await expect(answers(main)).toHaveText(['Visit the museum.']);
});

test('a retry that gets no answer puts the old one back, and says so', async ({ main, fakeLlm }) => {
  await freshChat(main);
  fakeLlm.scriptChat([{ content: 'It opens at nine.' }]);
  await send(main, 'When does the library open?');
  await expect(answers(main)).toHaveText(['It opens at nine.']);
  const kind = fakeLlm.requests.find((r) => JSON.stringify(r.messages || []).includes('When does the library open?')).kind;
  fakeLlm.failNext(kind, 6, 400, { error: { message: 'fake failure: no answer today' } });
  try {
    await answers(main).first().hover();
    await thread(main).getByTestId('chat-retry').click();
    await expect(main.getByText("Couldn't try again")).toBeVisible({ timeout: 20_000 });
    await expect(answers(main)).toHaveText(['It opens at nine.']);
    await expect(count(main)).toHaveCount(0);
  } finally {
    fakeLlm.failNext(kind, 0);
  }
});

// ---- the saved conversation (its own instances: one restarted, one an older Yap saved)

const NOW = Math.floor(Date.now() / 1000);
const m = (role, text, ts) => ({ role, text, ts });
/** A chat whose first answer was retried: "Boots." shown (2/2), "Socks." and its follow-up the other version. */
const TRIP = {
  id: 21,
  title: 'Trip planning',
  createdTs: NOW - 900,
  updatedTs: NOW - 600,
  messages: [m('user', 'What should I pack?', NOW - 900), m('assistant', 'Boots.', NOW - 600)],
};
const TRIP_FORKS = [{ at: 1, shown: 2, ts: NOW - 600, others: [{ n: 1, messages: [m('assistant', 'Socks.', NOW - 890), m('user', 'More?', NOW - 880), m('assistant', 'A hat.', NOW - 870)] }] }];
/** One an older Yap saved: its `branches` dropped, its copy still in chat_branches.json. */
const GARDEN = {
  id: 22,
  title: 'Garden beds',
  createdTs: NOW - 500,
  updatedTs: NOW - 400,
  messages: [m('user', 'Which vegetables in shade?', NOW - 500), m('assistant', 'Chard and lettuce.', NOW - 400), m('user', 'And herbs?', NOW - 300), m('assistant', 'Mint.', NOW - 290)],
};
const GARDEN_FORKS = [{ at: 1, shown: 2, ts: NOW - 400, others: [{ n: 1, messages: [m('assistant', 'Kale.', NOW - 490)] }] }];

test('the versions are there after a restart, as switched, and an older Yap’s save loses none that still fit', async ({ shot }) => {
  test.setTimeout(180_000);
  const options = {
    name: 'chat-branches-saved',
    data: {
      'chats.json': { conversations: [{ ...TRIP, branches: TRIP_FORKS }, GARDEN] },
      'chat_branches.json': { version: 1, conversations: [{ id: 22, forks: GARDEN_FORKS }] },
    },
    config: { meetingReminder: 'never' },
  };
  const first = await launchYap(options);
  let second = null;
  try {
    const main = first.main;
    await openView(main, 'Chat');
    await main.locator('.conv').filter({ hasText: 'Trip planning' }).click();
    await expect(answers(main)).toHaveText(['Boots.']);
    await expect(count(main)).toHaveText('2/2');
    // ‹ to the first: its follow-up with it.
    await thread(main).getByTestId('chat-branch-prev').click();
    await expect(answers(main)).toHaveText(['Socks.', 'A hat.']);
    await expect(questions(main)).toHaveText(['What should I pack?', 'More?']);
    await expectStore(first, 'chats.json', (s) => s.conversations.find((c) => c.id === 21)?.messages.length === 4);
    // The one an older Yap saved: its version put back from the copy.
    await main.locator('.conv').filter({ hasText: 'Garden beds' }).click();
    await expect(answers(main)).toHaveText(['Chard and lettuce.', 'Mint.']);
    await expect(count(main)).toHaveText('2/2');
    await thread(main).getByTestId('chat-branch-prev').click();
    await expect(answers(main)).toHaveText(['Kale.']);
    await shot(main, '05-older-save-restored');
    await expectStore(first, 'chats.json', (s) => s.conversations.find((c) => c.id === 22)?.branches?.length === 1);
    await first.stop();

    // Started again: each as it was left.
    second = await launchYap({ ...options, data: {}, restart: true });
    const again = second.main;
    await openView(again, 'Chat');
    await again.locator('.conv').filter({ hasText: 'Trip planning' }).click();
    await expect(answers(again)).toHaveText(['Socks.', 'A hat.']);
    await expect(count(again)).toHaveText('1/2');
    await again.locator('.conv').filter({ hasText: 'Garden beds' }).click();
    await expect(answers(again)).toHaveText(['Kale.']);
    await expect(count(again)).toHaveText('1/2');
    await shot(again, '06-after-a-restart');
  } finally {
    await first.stop();
    await second?.stop();
  }
});
