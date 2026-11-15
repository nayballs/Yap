// The cards Chat's own model puts in the thread (desktop/src-tauri/src/chat/cards.rs, 15 Nov 2026; three of
// the Claude app's): a question to the person (`ask_user`), the next-step box at the end of a turn
// (`suggest_next_step`) and chapters in a long chat (`mark_chapter`). The model is a local fake
// (support/fake-llm.js) scripted call by call, so this checks Yap's side: the cards, how they're answered,
// what the model reads back, and what stays in the thread.
// - A question: its options with their descriptions and a preview beside them; a click on an option answers
//   it; the question and the choice stay in the thread, answered, when the chat is opened again. Several
//   questions are sent together; Let Yap decide leaves the choice to the model.
// - Chapters and the next-step box: a divider with its title above the question that started it, the
//   floating contents jumping to it; never a chapter on a chat's first question. The box under the latest
//   answer only, a button's click sending its words as the next message.
// - Asked out loud (the Voice Assistant, from another app): the question and its options said, the Yap bar
//   listing them, and whatever is said next answers it ("the second one").
// Fictional data only; nothing leaves the PC.
import { test as base, expect, openView, inAnotherApp } from './support/fixtures.js';
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
        name: 'chat-cards',
        config: {
          webSearch: false,
          memoryLearnChats: false,
          speakReplies: true,
          llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat' } },
        },
      });
    },
    { scope: 'worker' },
  ],
});

const FORMAT = {
  question: 'Which format should the trip notes use?',
  header: 'Format',
  options: [
    { label: 'Markdown (Recommended)', description: 'Headings and lists.', preview: '## Lakeside weekend\n- Friday: drive up\n- Saturday: walk' },
    { label: 'Plain text', description: 'No marks at all.', preview: 'Lakeside weekend. Friday: drive up. Saturday: walk.' },
  ],
};
const LENGTH = {
  question: 'How long should they be?',
  header: 'Length',
  options: [
    { label: 'Short', description: 'A few lines.' },
    { label: 'Detailed', description: 'Every day, hour by hour.' },
  ],
};
const PARTS = {
  question: 'Which parts should they have?',
  header: 'Parts',
  multiSelect: true,
  options: [
    { label: 'Route', description: 'How to get there.' },
    { label: 'Packing', description: 'What to bring.' },
    { label: 'Budget', description: 'What it costs.' },
  ],
};

/** The tool results the model read in the last request that offered `tool`. */
function resultsOf(fakeLlm, from, tool = 'ask_user') {
  const last = fakeLlm.requests.slice(from).filter((r) => r.tools?.includes(tool)).at(-1);
  return (last?.messages ?? []).filter((m) => m.role === 'tool').map((m) => JSON.parse(m.content));
}

async function send(main, text) {
  await main.getByRole('textbox', { name: 'Message' }).fill(text);
  await main.getByRole('button', { name: 'Send', exact: true }).click();
}

test('a question card: an option answers it, its preview shows, and the question and the choice stay in the thread', async ({ main, fakeLlm, shot }) => {
  const from = fakeLlm.requests.length;
  fakeLlm.scriptChat([{ tools: [{ name: 'ask_user', args: { questions: [FORMAT] } }] }, { content: 'Done: the trip notes are in **plain text**.' }]);
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
  await send(main, 'Write up my lakeside weekend trip notes');
  const thread = main.locator('.msgs');
  const card = thread.locator('[data-testid="chat-question-card"]');
  await expect(card).toHaveAttribute('data-state', 'asking', { timeout: 20_000 });
  await expect(card).toContainText('A question for you');
  await expect(card.getByTestId('chat-question').locator('.qtext')).toContainText(FORMAT.question);
  await expect(card.getByTestId('chat-question').locator('.qhead')).toHaveText('Format');
  await expect(card.getByTestId('chat-question-option').locator('.qlabel')).toHaveText(['Markdown (Recommended)', 'Plain text']);
  await expect(card.getByTestId('chat-question-option').locator('.qdesc')).toHaveText(['Headings and lists.', 'No marks at all.']);
  // The preview beside the options: the first's, then the one pointed at.
  const preview = card.getByTestId('chat-question-preview');
  // (Yap's answers draw a heading a level down: ## is an h3.)
  await expect(preview.getByRole('heading', { name: 'Lakeside weekend' })).toBeVisible();
  await expect(preview.locator('li')).toHaveText(['Friday: drive up', 'Saturday: walk']);
  await card.getByRole('button', { name: /Plain text/ }).hover();
  await expect(preview).toContainText('Lakeside weekend. Friday: drive up.');
  // The model was told when to ask, and offered the tool.
  const asked = fakeLlm.requests.slice(from).find((r) => r.tools?.includes('ask_user'));
  expect(asked.system).toContain('call ask_user rather than guessing');
  await shot(main, '01-a-question-card');

  // One question, one answer: the click is the answer.
  await card.getByRole('button', { name: /Plain text/ }).click();
  await expect(thread).toContainText('Done: the trip notes are in plain text.');
  await expect(card).toHaveAttribute('data-state', 'answered');
  await expect(card.locator('.aq')).toHaveText(FORMAT.question);
  await expect(card.getByTestId('chat-question-answer')).toHaveText('Plain text');
  const [result] = resultsOf(fakeLlm, from);
  expect(result).toMatchObject({ success: true, data: { answers: { [FORMAT.question]: 'Plain text' } } });
  await expect(thread.locator('.toolchip')).toContainText(['Asked you a question']);
  await expect(thread.getByTestId('chat-steps').locator('.sumtext')).toHaveText('Asked you a question');
  // Where it asked: above the answer that follows from it, as the Claude app keeps it.
  const order = await thread.evaluate((el) => {
    const card = el.querySelector('[data-testid="chat-question-card"]');
    const answer = [...el.querySelectorAll('.cbubble.assistant')].at(-1);
    return card && answer ? card.compareDocumentPosition(answer) & Node.DOCUMENT_POSITION_FOLLOWING : 0;
  });
  expect(order).toBeTruthy();
  await shot(main, '02-answered-and-kept');

  // Opened again: still there, answered.
  await main.getByRole('button', { name: 'New chat' }).click();
  await main.locator('.conv').filter({ hasText: 'Write up my lakeside weekend trip notes' }).click();
  await expect(thread.locator('[data-testid="chat-question-card"]')).toHaveAttribute('data-state', 'answered');
  await expect(thread.getByTestId('chat-question-answer')).toHaveText('Plain text');
});

test('several questions are sent together, and Let Yap decide leaves the choice to the model', async ({ main, fakeLlm }) => {
  let from = fakeLlm.requests.length;
  fakeLlm.scriptChat([{ tools: [{ name: 'ask_user', args: { questions: [LENGTH, PARTS] } }] }, { content: 'Short notes with the route and the budget.' }]);
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
  await send(main, 'Plan the trip notes with me');
  const thread = main.locator('.msgs');
  const card = thread.locator('[data-testid="chat-question-card"]');
  await expect(card).toHaveAttribute('data-state', 'asking', { timeout: 20_000 });
  await expect(card).toContainText('2 questions for you');
  const sets = card.getByTestId('chat-question');
  await expect(sets).toHaveCount(2);
  const sendAnswers = card.getByRole('button', { name: 'Send', exact: true });
  await expect(sendAnswers).toBeDisabled();
  await sets.nth(0).getByRole('button', { name: /Short/ }).click();
  await sets.nth(1).getByRole('button', { name: /Route/ }).click();
  await sets.nth(1).getByRole('button', { name: /Budget/ }).click();
  await expect(sendAnswers).toBeEnabled();
  await sendAnswers.click();
  await expect(thread).toContainText('Short notes with the route and the budget.');
  await expect(card.getByTestId('chat-question-answer')).toHaveText(['Short', 'Route, Budget']);
  expect(resultsOf(fakeLlm, from)[0].data.answers).toEqual({ [LENGTH.question]: 'Short', [PARTS.question]: 'Route, Budget' });

  // Let Yap decide: told to choose, and say what it chose.
  from = fakeLlm.requests.length;
  fakeLlm.scriptChat([{ tools: [{ name: 'ask_user', args: { questions: [LENGTH] } }] }, { content: 'I kept them short.' }]);
  await send(main, 'And the packing list?');
  const second = thread.locator('[data-testid="chat-question-card"]').nth(1);
  await expect(second).toHaveAttribute('data-state', 'asking', { timeout: 20_000 });
  await second.getByRole('button', { name: 'Let Yap decide' }).click();
  await expect(thread).toContainText('I kept them short.');
  await expect(second).toHaveAttribute('data-state', 'skipped');
  await expect(second).toContainText('Left to Yap to decide');
  const [left] = resultsOf(fakeLlm, from);
  expect(left.note).toContain('The user left this to you');
});

test('chapters and the next-step box: a divider and the contents, and a button that answers', async ({ main, fakeLlm, shot }) => {
  let from = fakeLlm.requests.length;
  // A chapter on a chat's first question is refused: its start is the first already.
  fakeLlm.scriptChat([{ tools: [{ name: 'mark_chapter', args: { title: 'Getting started' } }] }, { content: 'The lake is two hours north.' }]);
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
  await send(main, 'How far is the lake?');
  const thread = main.locator('.msgs');
  await expect(thread).toContainText('The lake is two hours north.');
  expect(resultsOf(fakeLlm, from, 'mark_chapter')[0].error).toContain("chat's first question");
  await expect(thread.getByTestId('chat-chapter')).toHaveCount(0);
  await expect(main.getByTestId('chat-chapters')).toHaveCount(0);

  // A new piece of work: a chapter, and a box with a decision at the end.
  from = fakeLlm.requests.length;
  fakeLlm.scriptChat([
    { tools: [{ name: 'mark_chapter', args: { title: 'Packing list', summary: 'What to bring for the weekend.' } }] },
    { tools: [{ name: 'suggest_next_step', args: { recap: 'The packing list is drafted below. Nothing is saved yet.', question: 'Save it as a note?', buttons: [{ label: 'Yes, save it' }, { label: 'Not now' }] } }] },
    { content: 'Tent, sleeping bags, a stove and a map.' },
  ]);
  await send(main, 'Now make me a packing list');
  await expect(thread).toContainText('Tent, sleeping bags, a stove and a map.');
  // The divider above the question that started it; the chips have neither call.
  const divider = thread.getByTestId('chat-chapter');
  await expect(divider).toHaveCount(1);
  await expect(divider).toHaveText('Packing list');
  await expect(thread.locator('.toolchip')).toHaveCount(0);
  const box = thread.getByTestId('chat-next-step');
  await expect(box).toContainText('The packing list is drafted below. Nothing is saved yet.');
  await expect(box.locator('.nask')).toHaveText('Save it as a note?');
  await expect(box.getByTestId('chat-next-step-button')).toHaveText(['Yes, save it', 'Not now']);
  // The contents: the start, then the chapter; a click goes there.
  const contents = main.getByTestId('chat-chapters');
  await expect(contents).toBeVisible();
  await expect(contents.getByRole('button', { name: /^Chapters/ })).toHaveAccessibleName('Chapters: Packing list');
  await contents.getByRole('button', { name: /^Chapters/ }).click();
  await expect(contents.getByRole('menuitem')).toHaveText(['Start', 'Packing list']);
  await shot(main, '03-chapters-and-next-step');
  await contents.getByRole('menuitem', { name: 'Packing list' }).click();
  await expect(contents.getByRole('menu')).toHaveCount(0);
  const asked = fakeLlm.requests.slice(from).find((r) => r.tools?.includes('suggest_next_step'));
  expect(asked.system).toContain('call mark_chapter with a short title');
  expect(asked.system).toContain('end it by calling suggest_next_step');

  // A button sends its words as the next message; the box goes once something follows.
  fakeLlm.scriptChat([{ content: 'OK, I left it unsaved.' }]);
  await box.getByRole('button', { name: 'Not now' }).click();
  await expect(thread).toContainText('OK, I left it unsaved.');
  await expect(thread.locator('.cbubble.user').last()).toHaveText('Not now');
  await expect(thread.getByTestId('chat-next-step')).toHaveCount(0);
  // Opened again: the divider and the contents are back.
  await main.getByRole('button', { name: 'New chat' }).click();
  await main.locator('.conv').filter({ hasText: 'How far is the lake?' }).click();
  await expect(thread.getByTestId('chat-chapter')).toHaveText('Packing list');
  await expect(main.getByTestId('chat-chapters')).toBeVisible();
});

test('asked out loud: the question and its options said, on the Yap bar, and the next words answer it', async ({ yap, fakeLlm }) => {
  await yap.invoke('tts_e2e_spoken', { clear: true });
  await inAnotherApp(yap);
  const from = fakeLlm.requests.length;
  const spoken = async () => (await yap.invoke('tts_e2e_spoken')).map((s) => s.text).join('\n');
  const said = (text) => expect.poll(spoken, { timeout: 20_000, message: `Yap to say “${text}”` }).toContain(text);
  const barCard = async () => (await yap.invoke('bar_status')).cards.find((c) => c.id === 'askFirst') ?? null;
  const ask = (text) => yap.invoke('e2e_assistant_ask', { text, listenMs: 0 });
  fakeLlm.scriptChat([{ tools: [{ name: 'ask_user', args: { questions: [FORMAT] } }] }, { content: 'Plain text it is.' }]);
  await ask('Write up my lakeside trip notes');
  await said('Which format should the trip notes use: Markdown (Recommended) or Plain text? Say one, or pick on the card.');
  await expect.poll(barCard, { timeout: 15_000 }).not.toBeNull();
  const card = await barCard();
  expect(card).toMatchObject({ title: FORMAT.question, icon: 'assistant' });
  expect([card.primary.label, card.secondary.label, card.link.label]).toEqual(['Markdown (Recommended)', 'Plain text', 'Let Yap decide']);
  // The next words in that conversation: the answer, matched to an option.
  await ask('The second one');
  await said('Plain text it is.');
  await expect.poll(barCard).toBeNull();
  const [result] = resultsOf(fakeLlm, from);
  expect(result.data).toEqual({ answers: { [FORMAT.question]: 'Plain text' }, said: 'The second one' });
  // It was an answer, not a question of its own; the card stays, answered out loud.
  const conv = (yap.readJson('chats.json')?.conversations ?? []).find((c) => c.messages.some((m) => m.text === 'Write up my lakeside trip notes'));
  expect(conv.messages.some((m) => m.text === 'The second one')).toBe(false);
  const answer = conv.messages.at(-1);
  expect(answer.confirms?.[0]).toMatchObject({ act: 'question', state: 'answered', question: { said: 'The second one', voice: true } });
});
