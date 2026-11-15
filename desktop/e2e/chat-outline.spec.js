// A new document's outline, seen before the writing starts (desktop/src-tauri/src/chat/cards/outline.rs,
// 15 Nov 2026; the Claude app's plan review), with "Show the outline before writing" on (Settings → Language
// Models → Chat & Voice Assistant, `chatOutlineReview`: on by default, off in the suite's BASE_CONFIG):
// - `create_document`'s outline is a card in the thread first — the title, each heading and its one line —
//   with Go and Edit; nothing is written (no panel) until the person answers.
// - Go: written as outlined, the card kept, answered ("Written as outlined").
// - Edit: a heading renamed, one moved up, one taken out, one added, a note — "Go with these changes" makes the
//   document with those sections in that order, and the model reads what changed and the note.
// - Asked out loud (the Voice Assistant from another app): the headings said, the Yap bar lists them with
//   Go ahead and Edit in Chat, and the next words answer it ("drop the second part").
// The model is a local fake (support/fake-llm.js) scripted call by call; its write_section calls follow the
// sections create_document's result gave it. Light and dark.
import { test as base, expect, openView, openSettings, setConfig, inAnotherApp } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { draftOf, moved, sectionsOf, answerOf, changedFrom, endingOf, MAX_SECTIONS } from '../src/lib/views/chat/outlineEdit.js';
import { quoteOf, withComment, beforeLabel, QUOTE_MAX } from '../src/lib/views/chat/planComments.js';

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
        name: 'chat-outline',
        config: {
          webSearch: false,
          memoryLearnChats: false,
          speakReplies: true,
          meetingReminder: 'never',
          llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat' } },
        },
      });
    },
    { scope: 'worker' },
  ],
});

const box = (main) => main.getByRole('textbox', { name: 'Message' });
const thread = (main) => main.locator('.msgs');
const panel = (main) => main.getByTestId('chat-doc');
const docBody = (main) => panel(main).locator('.docbody');
const card = (main) => thread(main).getByTestId('chat-outline-card');

const OUTLINE = {
  title: 'Lone wolf ranger',
  summary: 'A solo ranger build for a fictional game.',
  sections: [
    { key: 'Gear', intent: 'what to wear and where to find it' },
    { key: 'Skills', intent: 'which skills, and when' },
    { key: 'Talents', intent: 'the talents to take' },
  ],
};
const WORDS = { Gear: '- A hood of the fox', Skills: '- Quick step at level 1', Talents: '1. Lone Wolf', Equipment: '- A hood of the fox', Costs: '- About 200 gold' };

/** The latest create_document result the model read (its document's id and sections, what it was told). */
function made(record) {
  for (const m of [...record.messages].reverse()) {
    if (m.role !== 'tool') continue;
    try {
      const r = JSON.parse(m.content);
      if (Array.isArray(r?.data?.sections)) return r;
    } catch {
      /* not JSON */
    }
  }
  return null;
}

/** The fake model's turn: the outline, then each section its result says, in that order, then the answer. */
function scriptDocument(fakeLlm, outline = OUTLINE, answer = "It's open on the right.", writes = 3) {
  const write = (i) => ({
    tools: [
      {
        name: 'write_section',
        args: (r) => {
          const res = made(r);
          const key = res.data.sections[i] ?? res.data.sections.at(-1);
          return { note_id: res.data.id, key, markdown: WORDS[key] || `- ${key}` };
        },
      },
    ],
  });
  // A write per section the document will have (`writes`), then the answer.
  fakeLlm.scriptChat([{ tools: [{ name: 'create_document', args: outline }] }, ...Array.from({ length: writes }, (_, i) => write(i)), { content: answer }]);
}

async function freshChat(main) {
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
}
async function ask(main, text) {
  await box(main).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
}
async function theme(yap, main, which) {
  await setConfig(yap, main, { theme: which });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe(which);
}

test.beforeEach(async ({ yap, main }) => {
  await setConfig(yap, main, { chatOutlineReview: true, theme: 'light' });
});

// ---------------------------------------------------------------- no app

test('outlineEdit.js and planComments.js: the draft, its moves, the answer sent; a comment’s quote and the folded label', () => {
  const rows = draftOf(OUTLINE.sections);
  expect(rows.map((r) => r.key)).toEqual(['Gear', 'Skills', 'Talents']);
  expect(new Set(rows.map((r) => r.id)).size).toBe(3);
  expect(moved(rows, 2, -1).map((r) => r.key)).toEqual(['Gear', 'Talents', 'Skills']);
  expect(moved(rows, 0, -1)).toBe(rows);
  expect(sectionsOf([{ key: '## Gear ' }, { key: 'gear' }, { key: '  ' }, { key: 'Costs', intent: ' x ' }])).toEqual([{ key: 'Gear', intent: '' }, { key: 'Costs', intent: 'x' }]);
  expect(answerOf(rows, OUTLINE.sections, '')).toBeNull();
  expect(answerOf(rows, OUTLINE.sections, ' Short please ')).toEqual({ sections: OUTLINE.sections, note: 'Short please' });
  expect(answerOf([{ key: '' }], OUTLINE.sections)).toBeUndefined();
  expect(changedFrom(rows.slice(1), OUTLINE.sections)).toBe(true);
  expect(sectionsOf(Array.from({ length: 20 }, (_, i) => ({ key: `S${i}` })))).toHaveLength(MAX_SECTIONS);
  expect(endingOf('allowed')).toBe('Written as outlined');
  expect(endingOf('changed')).toBe('Written with your changes');
  expect(endingOf('timedOut')).toBe('Not answered in time: written as outlined');
  // Plan comments.
  expect(quoteOf('  Tomatoes twice\n a week ')).toBe('Tomatoes twice a week');
  expect(quoteOf('x'.repeat(QUOTE_MAX + 10))).toHaveLength(QUOTE_MAX);
  expect(withComment([], 'a', '  ')).toEqual([]);
  expect(withComment([], 'a  b', ' Change this ')).toEqual([{ quote: 'a b', text: 'Change this' }]);
  expect(beforeLabel({ comments: [{}, {}] })).toBe('Plan before your comments · 2 comments');
  expect(beforeLabel({ comments: [{}] }, 0, 2)).toBe('Version 1, before your comments · 1 comment');
});

// ---------------------------------------------------------------- in Chat

test('the outline shows before the writing: Go writes it as outlined, and the card stays, answered', async ({ yap, main, fakeLlm, shot }) => {
  scriptDocument(fakeLlm);
  await freshChat(main);
  await ask(main, 'Build me a lone wolf ranger guide');
  // The card: the title, each heading and its line, Go and Edit; nothing written yet.
  await expect(card(main)).toHaveAttribute('data-state', 'asking');
  await expect(card(main).locator('.otitle')).toContainText('The outline · Lone wolf ranger');
  await expect(card(main).locator('.osum')).toHaveText(OUTLINE.summary);
  await expect(card(main).locator('.okey')).toHaveText(['Gear', 'Skills', 'Talents']);
  await expect(card(main).locator('.ointent')).toHaveText(OUTLINE.sections.map((s) => s.intent));
  await expect(card(main).getByRole('button', { name: 'Go', exact: true })).toBeVisible();
  await expect(card(main).getByRole('button', { name: 'Edit' })).toBeVisible();
  await expect(panel(main)).toHaveCount(0);
  expect((yap.readJson('notes.json')?.notes ?? []).some((n) => n.title === OUTLINE.title)).toBe(false);
  await expect(main.getByTestId('chat-activity')).toContainText('Waiting for you: the outline');
  await shot(main, '01-outline-card-light');
  await theme(yap, main, 'dark');
  await shot(main, '02-outline-card-dark');
  await theme(yap, main, 'light');

  // Go: written as outlined.
  await card(main).getByRole('button', { name: 'Go', exact: true }).click();
  await expect(thread(main)).toContainText("It's open on the right.", { timeout: 20_000 });
  await expect(docBody(main).locator('h3')).toHaveText(['Gear', 'Skills', 'Talents']);
  await expect(card(main)).toHaveAttribute('data-state', 'allowed');
  await expect(card(main).getByTestId('chat-outline-written')).toHaveText('Gear · Skills · Talents');
  await expect(card(main)).toContainText('Written as outlined');
  const res = made(fakeLlm.requests.filter((r) => r.tools.includes('write_section')).at(-1));
  expect(res.data.outline).toContain('The user read the outline and said go');
  // Kept with the answer: back with the chat. (The list stepped aside for the document: brought back first.)
  const showChats = main.getByRole('button', { name: 'Show your chats' });
  if (await showChats.isVisible()) await showChats.click();
  await main.getByRole('button', { name: 'New chat' }).click();
  await main.locator('.conv').filter({ hasText: 'Build me a lone wolf' }).first().click();
  await expect(card(main)).toHaveAttribute('data-state', 'allowed');
  await shot(main, '03-outline-answered-light');
});

test('Edit: a heading renamed, one moved, one taken out, one added and a note — the writing follows it', async ({ yap, main, fakeLlm, shot }) => {
  scriptDocument(fakeLlm);
  await freshChat(main);
  await ask(main, 'Make me a ranger build to keep');
  await expect(card(main)).toHaveAttribute('data-state', 'asking');
  await card(main).getByRole('button', { name: 'Edit' }).click();
  const edit = card(main).getByTestId('chat-outline-edit');
  await expect(edit.locator('li')).toHaveCount(3);
  // Gear → Equipment; Talents up above Skills; Skills out; Costs added; a note.
  await edit.getByRole('textbox', { name: 'Section 1: its heading' }).fill('Equipment');
  await card(main).getByRole('button', { name: 'Move section 3 up' }).click();
  await expect.poll(() => edit.locator('.okeyin').evaluateAll((els) => els.map((e) => e.value))).toEqual(['Equipment', 'Talents', 'Skills']);
  await card(main).getByRole('button', { name: 'Take out section 3' }).click();
  await card(main).getByRole('button', { name: '+ Add a section' }).click();
  await expect(edit.getByRole('textbox', { name: 'Section 3: its heading' })).toBeFocused();
  await edit.getByRole('textbox', { name: 'Section 3: its heading' }).fill('Costs');
  await card(main).getByTestId('chat-outline-note').fill('Keep each part short.');
  await shot(main, '04-outline-edit-light');
  await theme(yap, main, 'dark');
  await shot(main, '05-outline-edit-dark');
  await theme(yap, main, 'light');
  await card(main).getByRole('button', { name: 'Go with these changes' }).click();

  await expect(thread(main)).toContainText("It's open on the right.", { timeout: 20_000 });
  await expect(docBody(main).locator('h3')).toHaveText(['Equipment', 'Talents', 'Costs']);
  await expect(docBody(main)).not.toContainText('Skills');
  await expect(card(main)).toHaveAttribute('data-state', 'changed');
  await expect(card(main).getByTestId('chat-outline-written')).toHaveText('Equipment · Talents · Costs');
  await expect(card(main).getByTestId('chat-outline-note-kept')).toHaveText('Your note: “Keep each part short.”');
  await expect(card(main)).toContainText('Written with your changes');
  const res = made(fakeLlm.requests.filter((r) => r.tools.includes('write_section')).at(-1));
  expect(res.data.sections).toEqual(['Equipment', 'Talents', 'Costs']);
  expect(res.data.outline).toContain('The user changed the outline before you wrote it');
  expect(res.data.outline).toContain('“Keep each part short.”');
  await shot(main, '06-outline-changed-written-light');
});

test('switched off, the document is written straight away, with no card', async ({ yap, main, fakeLlm }) => {
  await setConfig(yap, main, { chatOutlineReview: false });
  scriptDocument(fakeLlm);
  await freshChat(main);
  await ask(main, 'Write me a ranger guide');
  await expect(thread(main)).toContainText("It's open on the right.", { timeout: 20_000 });
  await expect(card(main)).toHaveCount(0);
  await expect(docBody(main).locator('h3')).toHaveText(['Gear', 'Skills', 'Talents']);
});

test('the switch is in Settings → Language Models → Chat & Voice Assistant, and Settings search finds it', async ({ yap, main }) => {
  await setConfig(yap, main, { chatOutlineReview: true });
  const dialog = await openSettings(main, 'Language Models');
  await dialog.getByRole('tab', { name: 'Chat & Voice Assistant' }).click();
  const row = dialog.locator('[data-setting="chat-outline-review"]');
  await expect(row).toContainText('Show the outline before writing');
  const toggle = row.getByRole('button', { name: 'Show the outline before writing' });
  await expect(toggle).toHaveAttribute('aria-pressed', 'true');
  await toggle.click();
  await expect.poll(async () => (await yap.invoke('get_config')).chatOutlineReview, { timeout: 10_000 }).toBe(false);
  await toggle.click();
  await expect.poll(async () => (await yap.invoke('get_config')).chatOutlineReview, { timeout: 10_000 }).toBe(true);
  await main.keyboard.press('Escape');
});

test('asked out loud: the headings said, on the Yap bar, and “drop the second part” answers it', async ({ yap, main, fakeLlm }) => {
  await yap.invoke('tts_e2e_spoken', { clear: true });
  await inAnotherApp(yap);
  const spoken = async () => (await yap.invoke('tts_e2e_spoken')).map((s) => s.text).join('\n');
  const said = (text) => expect.poll(spoken, { timeout: 20_000, message: `Yap to say “${text}”` }).toContain(text);
  const barCard = async () => (await yap.invoke('bar_status')).cards.find((c) => c.id === 'askFirst') ?? null;
  const askAloud = (text) => yap.invoke('e2e_assistant_ask', { text, listenMs: 0 });
  scriptDocument(fakeLlm, OUTLINE, "It's in Chat.", 2);
  await askAloud('Make me a lone wolf ranger guide to keep');
  await said("Here's the outline for “Lone wolf ranger”: Gear, Skills and Talents. Say go ahead, or what to change.");
  await expect.poll(barCard, { timeout: 15_000 }).not.toBeNull();
  const bar = await barCard();
  expect(bar).toMatchObject({ title: 'The outline for “Lone wolf ranger”', body: '1. Gear\n2. Skills\n3. Talents', icon: 'assistant' });
  expect([bar.primary.label, bar.secondary.label]).toEqual(['Go ahead', 'Edit in Chat']);
  // The next words in that conversation: the answer.
  await askAloud('Go ahead, but drop the second part');
  await said("It's in Chat.");
  await expect.poll(barCard).toBeNull();
  // Written without the part dropped (notes.json as saved).
  const docText = () => (yap.readJson('notes.json')?.notes ?? []).find((n) => n.title === OUTLINE.title)?.content ?? '';
  await expect.poll(docText, { timeout: 10_000 }).toContain('Talents');
  expect(docText()).toContain('Gear');
  expect(docText()).not.toContain('Skills');
  const conv = (yap.readJson('chats.json')?.conversations ?? []).find((c) => c.messages.some((m) => m.text === 'Make me a lone wolf ranger guide to keep'));
  expect(conv.messages.some((m) => m.text === 'Go ahead, but drop the second part')).toBe(false);
  expect(conv.messages.at(-1).confirms?.[0]).toMatchObject({ act: 'outline', state: 'changed', outline: { said: 'Go ahead, but drop the second part', voice: true } });
  await openView(main, 'Chat');
});
