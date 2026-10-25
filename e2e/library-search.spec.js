// Search beyond the meetings (src-tauri/src/search/library.rs): the Notes
// view's search over every note (title, My thoughts, summary, transcript)
// by its words — marked, and opening the note at the match — and by
// meaning; Chat's sidebar search over titles and messages, and the
// archive (out of the list, restored, deleted from there); memory's
// people found for a question by meaning; Chat's own `search_notes` by
// meaning; a deleted chat's vectors gone from the index; and Settings →
// Privacy & data counting notes, chats and memory items. The embedding
// model is meeting-search.spec.js's stand-in (support/fake-embed.js: words
// hashed into vectors, "teeth"/"dentist"/"cleaning" one sense, "staff"/
// "recruit"/"hiring" another), the chat model a scripted fake.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, openSettings, closeSettings, closeToasts, sidebar } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { startFakeEmbed } from './support/fake-embed.js';
import { startFakeLocalModels } from './support/local-llm.js';
import { RUNS } from './support/yap.js';

const NOW = Math.floor(Date.now() / 1000);
const DAY = 86_400;
const T = NOW - 3 * DAY;

/** A made-up team's meeting, a few notes of one person's life. */
const NOTES = {
  folders: ['Personal', 'Meetings', 'Work'],
  notes: [
    {
      id: 1,
      title: 'Q3 budget review',
      noteType: 'meeting',
      folder: 'Meetings',
      source: 'meeting',
      createdTs: T,
      updatedTs: T,
      transcript: [
        { source: 'them', text: 'The venue quote came in at four thousand pounds.', ts: T },
        { source: 'you', text: 'Can we move some of the catering money across?', ts: T + 20 },
        { source: 'them', text: "There's a hiring freeze until January, except the support role.", ts: T + 40 },
      ],
    },
    {
      id: 2,
      title: 'Dentist',
      folder: 'Personal',
      source: 'manual',
      createdTs: NOW - 5 * DAY,
      updatedTs: NOW - 5 * DAY,
      content: 'Dentist on Thursday',
    },
    {
      id: 3,
      title: 'Garden plans',
      folder: 'Work',
      source: 'manual',
      createdTs: NOW - 2 * DAY,
      updatedTs: NOW - 2 * DAY,
      content: 'Buy compost and seed trays.\n\nMove the rhubarb to the sunny bed in spring.',
      enhancedContent: '## Summary\nThe greenhouse needs a new hinge before winter.',
    },
  ],
};

const conv = (id, title, ago, messages) => ({
  id,
  title,
  createdTs: NOW - ago,
  updatedTs: NOW - ago,
  messages: messages.map(([role, text], i) => ({ role, text, ts: NOW - ago + i })),
});
const CHATS = {
  conversations: [
    conv(1, 'Trip ideas', 3 * DAY, [
      ['user', 'Where should we eat in Lisbon?'],
      ['assistant', 'Try the seafood places in Alfama, and the market hall for variety.'],
    ]),
    conv(2, 'Team plans', 2 * DAY, [
      ['user', 'Should we recruit staff?'],
      ['assistant', 'Hire two staff for support.'],
    ]),
    conv(3, 'Sourdough', DAY, [
      ['user', 'How long do I prove sourdough?'],
      ['assistant', 'Usually four to six hours at room temperature.'],
    ]),
  ],
};
const MEMORY = {
  version: 1,
  nextId: 2,
  items: [
    {
      id: 1,
      section: 'people',
      title: 'Dr Ruiz',
      summary: 'Dentist',
      details: [],
      source: { kind: 'manual', title: '' },
      createdTs: NOW - DAY,
      updatedTs: NOW - DAY,
      dictation: false,
    },
  ],
};

const test = base.extend({
  fakeLlm: [
    async ({}, use) => {
      const fake = await startFakeLlm();
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  fakeEmbed: [
    async ({}, use) => {
      const fake = await startFakeEmbed();
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  fakeModels: [
    async ({}, use) => {
      const dir = path.join(RUNS, 'library-search-sources');
      const fake = await startFakeLocalModels(dir, ['embeddinggemma']);
      fake.sources.embeddinggemma.size = 4 * 1024 * 1024;
      fs.writeFileSync(fake.env.YAP_LLM_SOURCES, JSON.stringify(fake.sources, null, 2));
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({ fakeLlm, fakeEmbed, fakeModels }, use) => {
      await use({
        name: 'library-search',
        config: {
          llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat' } },
          memorySearchChats: true,
        },
        data: { 'notes.json': NOTES, 'chats.json': CHATS, 'memory.json': MEMORY },
        env: { ...fakeModels.env, YAP_E2E_EMBED_URL: fakeEmbed.url },
      });
    },
    { scope: 'worker' },
  ],
});

const status = (yap) => yap.invoke('search_status');

/** The search model installed and everything read (notes, chats, memory). */
async function installed(yap) {
  if (!(await status(yap)).installed) await yap.invoke('search_install');
  await expect.poll(async () => (await status(yap)).phase, { timeout: 20_000 }).toBe('ready');
}

/** Every passage read, and the index at rest (`also`: and this holds). */
async function settled(yap, also = () => true) {
  await expect
    .poll(
      async () => {
        const s = await status(yap);
        return s.phase === 'ready' && s.indexed === s.passages && also(s);
      },
      { timeout: 20_000 }
    )
    .toBe(true);
}

/** The index's records on disk (`store.rs`: a header, then 16 + 4 + 768 bytes each). */
function records(yap) {
  const file = path.join(yap.dataDir, 'search-index', 'vectors.bin');
  if (!fs.existsSync(file)) return 0;
  const header = 8 + 4 + 2 + 'embeddinggemma-300m-q8_0'.length;
  return (fs.statSync(file).size - header) / (16 + 4 + 768);
}

async function searchNotes(main, words) {
  await openView(main, 'Notes');
  const box = main.getByRole('searchbox', { name: 'Search all notes' });
  // (The box stays open after a result is opened.)
  if (!(await box.isVisible())) await main.getByRole('button', { name: 'Search notes' }).click();
  await box.fill(words);
  return main.getByRole('group', { name: 'Notes that match' });
}

test('Notes: every note holding the words, marked, each opening at its match', async ({ yap, main, shot }) => {
  // Before the search model: the words alone.
  expect((await status(yap)).installed).toBe(false);
  let results = await searchNotes(main, 'freeze');
  // From another folder than the one open: every note is searched.
  await expect(results.getByRole('button')).toHaveCount(1);
  const first = results.getByRole('button').first();
  await expect(first).toContainText('Q3 budget review');
  await expect(first.locator('mark.found')).toHaveText('freeze');
  await expect(first).toContainText('Transcript · 0:40');
  await shot(main, '01-notes-search-by-words');
  await first.click();
  await expect(main.getByPlaceholder('Untitled Note')).toHaveValue('Q3 budget review');
  await expect(main.locator('.bubble:focus')).toContainText("There's a hiring freeze until January");

  // My thoughts: the note opens with the words selected on their line.
  results = await searchNotes(main, 'rhub');
  await expect(results.getByRole('button').first()).toContainText('Garden plans');
  await expect(results.locator('mark.found').first()).toHaveText('rhub');
  await expect(results.getByRole('button').first()).toContainText('My thoughts');
  await results.getByRole('button').first().click();
  await expect(main.getByPlaceholder('Untitled Note')).toHaveValue('Garden plans');
  const raw = main.getByRole('textbox', { name: 'My thoughts' });
  await expect.poll(() => raw.evaluate((el) => el.value.slice(el.selectionStart, el.selectionEnd))).toBe('rhub');

  // The summary: its tab, the words marked there.
  results = await searchNotes(main, 'greenhouse hinge');
  await expect(results.getByRole('button').first()).toContainText('Summary');
  await results.getByRole('button').first().click();
  await expect(main.locator('.rendered')).toContainText('The greenhouse needs a new hinge');
  await expect.poll(() => main.evaluate(() => (CSS.highlights.get('yap-found')?.size ?? 0))).toBe(2);
  await shot(main, '02-notes-summary-marked');

  // Every word must be there; none of them: nothing.
  results = await searchNotes(main, 'greenhouse rhubarb hiring');
  await expect(main.getByText('No notes match that')).toBeVisible();
});

test('Notes by meaning: a note that means the words comes too, marked as such', async ({ yap, main, shot }) => {
  await installed(yap);
  // No note holds "teeth" or "cleaning"; the dentist's note means them.
  const results = await searchNotes(main, 'teeth cleaning');
  await expect(main.getByText('1 note · by meaning too')).toBeVisible();
  const first = results.getByRole('button').first();
  await expect(first).toContainText('Dentist');
  await expect(first).toContainText('by meaning');
  await shot(main, '03-notes-by-meaning');
  await first.click();
  await expect(main.getByPlaceholder('Untitled Note')).toHaveValue('Dentist');
  // Words and meaning together: the words' note stays first.
  await searchNotes(main, 'hiring');
  await expect(main.getByRole('group', { name: 'Notes that match' }).getByRole('button').first()).toContainText('Q3 budget review');
});

test('Chat: the sidebar searches titles and messages, by meaning too, and opens at the message', async ({ yap, main, shot }) => {
  await installed(yap);
  await openView(main, 'Chat');
  const box = main.getByRole('searchbox', { name: 'Search chats' });
  await box.fill('alfama');
  const hits = main.getByRole('group', { name: 'Chats that match' });
  await expect(hits.getByRole('button')).toHaveCount(1);
  await expect(hits.getByRole('button').first()).toContainText('Trip ideas');
  await expect(hits.locator('mark.found')).toHaveText('Alfama');
  await shot(main, '04-chat-search-words');
  await hits.getByRole('button').first().click();
  // Its conversation, at the answer that holds the word, the word marked.
  await expect(main.locator('[data-msg-id="chat:1:1"]')).toBeFocused();
  await expect.poll(() => main.evaluate(() => CSS.highlights.get('yap-found')?.size ?? 0)).toBe(1);

  // By meaning: "hiring" is in no chat; the one about recruiting means it.
  await box.fill('hiring');
  await expect(main.getByText('1 chat · by meaning too')).toBeVisible();
  await expect(hits.getByRole('button').first()).toContainText('Team plans');
  await expect(hits.getByRole('button').first()).toContainText('by meaning');
  await shot(main, '05-chat-search-meaning');
  await box.press('Escape');
  await expect(main.locator('.conv').filter({ hasText: 'Sourdough' })).toBeVisible();
});

test('Chat: archive a conversation, find it in Archived, restore it, delete one from there', async ({ yap, main, shot }) => {
  await openView(main, 'Chat');
  await closeToasts(main);
  const row = (title) => main.locator('.conv').filter({ hasText: title });
  await row('Sourdough').click({ button: 'right' });
  await main.getByRole('menuitem', { name: 'Archive' }).click();
  await expect(row('Sourdough')).toHaveCount(0);
  const archivedBtn = main.getByRole('button', { name: /^Archived/ });
  await expect(archivedBtn).toContainText('1');
  expect(yap.readJson('chats.json').conversations.find((c) => c.title === 'Sourdough').archivedTs).toBeGreaterThan(0);
  // Still searched: it says it's archived.
  await main.getByRole('searchbox', { name: 'Search chats' }).fill('sourdough');
  await expect(main.getByRole('group', { name: 'Chats that match' }).getByRole('button').first()).toContainText('Archived');
  await main.getByRole('searchbox', { name: 'Search chats' }).fill('');

  await archivedBtn.click();
  const archive = main.getByRole('region', { name: 'Archived chats' });
  await expect(archive.locator('.row')).toHaveCount(1);
  await shot(main, '06-chat-archived');
  await archive.getByRole('button', { name: 'Restore Sourdough', exact: true }).click();
  await expect(row('Sourdough')).toBeVisible();
  expect(yap.readJson('chats.json').conversations.find((c) => c.title === 'Sourdough').archivedTs ?? 0).toBe(0);

  // Several at once, then one deleted from the archive (asked first).
  await row('Trip ideas').click();
  await row('Sourdough').click({ modifiers: ['Control'] });
  await main.getByRole('toolbar', { name: 'Selected chats' }).getByRole('button', { name: 'Archive' }).click();
  await expect(main.getByRole('button', { name: /^Archived/ })).toContainText('2');
  await main.getByRole('button', { name: /^Archived/ }).click();
  await archive.getByRole('button', { name: 'Delete Sourdough', exact: true }).click();
  const confirm = main.getByRole('alertdialog', { name: 'Delete “Sourdough”?' });
  await confirm.getByRole('button', { name: 'Delete', exact: true }).click();
  await expect(archive.locator('.row')).toHaveCount(1);
  expect(yap.readJson('chats.json').conversations.some((c) => c.title === 'Sourdough')).toBe(false);
  await archive.getByRole('button', { name: 'Restore Trip ideas', exact: true }).click();
  await expect(main.getByRole('button', { name: /^Archived/ })).toHaveCount(0);
});

test('a deleted chat takes its vectors with it', async ({ yap }) => {
  await installed(yap);
  // A chat of its own, read into the index.
  await yap.invoke('chat_send', { conversationId: null, text: 'Note the bike shed colour: teal' });
  const conv = yap.readJson('chats.json').conversations.find((c) => c.title.startsWith('Note the bike shed'));
  expect(conv).toBeTruthy();
  const chats = yap.readJson('chats.json').conversations.length;
  await settled(yap, (s) => s.chats === chats);
  const before = await status(yap);
  const kept = records(yap);
  // (An edit's old vectors may still be there; the store drops them in bulk.)
  expect(kept).toBeGreaterThanOrEqual(before.passages);
  await yap.invoke('chat_delete', { id: conv.id });
  // Its passages are gone from the index file at once (a sweep, not a rebuild).
  await expect.poll(async () => (await status(yap)).kinds.chatPassages, { timeout: 20_000 }).toBeLessThan(before.kinds.chatPassages);
  await expect.poll(async () => records(yap) === (await status(yap)).passages, { timeout: 20_000 }).toBe(true);
  expect(records(yap)).toBeLessThan(kept);
});

test('memory: a person the question means comes with it, by meaning', async ({ yap, main, fakeLlm }) => {
  await installed(yap);
  fakeLlm.scriptChat([{ content: 'Dr Ruiz does.' }]);
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
  // No word of the question is in memory ("Dr Ruiz", "Dentist"): by meaning.
  await main.getByRole('textbox', { name: 'Message' }).fill('Who does my teeth cleaning?');
  await main.getByRole('button', { name: 'Send' }).click();
  await expect(main.locator('.msgs')).toContainText('Dr Ruiz does.');
  const req = fakeLlm.requests.filter((r) => r.kind === 'other').at(-1);
  expect(req.user).toContain("From Yap's memory");
  expect(req.user).toContain('Dr Ruiz (person): Dentist.');
});

test("Chat's search_notes finds by meaning, and gives the passage that matched", async ({ yap, main, fakeLlm }) => {
  await installed(yap);
  fakeLlm.scriptChat([{ tools: [{ name: 'search_notes', args: { query: 'teeth cleaning appointment' } }] }, { content: 'On Thursday.' }]);
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
  await main.getByRole('textbox', { name: 'Message' }).fill('When is my next check-up?');
  await main.getByRole('button', { name: 'Send' }).click();
  await expect(main.locator('.msgs')).toContainText('On Thursday.');
  await expect(main.locator('.msgs .toolchip').first()).toContainText('Found 1 note for "teeth cleaning appointment" by meaning');
  // The tool's result, as the model read it: the note and its passage.
  const req = fakeLlm.requests.filter((r) => r.kind === 'other').at(-1);
  const result = req.messages.find((m) => m.role === 'tool');
  const data = JSON.parse(result.content).data;
  expect(data[0]).toMatchObject({ id: 2, title: 'Dentist' });
  expect(data[0].content).toContain('(notes) Dentist on Thursday');
});

test('Privacy & data counts the notes, chats and memory items read', async ({ yap, main, shot }) => {
  await installed(yap);
  await settled(yap);
  const s = await status(yap);
  expect(s.notes).toBe(3);
  expect(s.chats).toBeGreaterThanOrEqual(2);
  expect(s.memories).toBe(1);
  expect(s.kinds.memoryPassages).toBe(1);
  expect(s.passages).toBe(s.kinds.notePassages + s.kinds.chatPassages + s.kinds.memoryPassages);
  const dialog = await openSettings(main, 'Privacy & data');
  const index = dialog.locator('[data-setting="meeting-search-index"]');
  // The row and a fresh status agree (a chat still being read under load
  // moves both).
  await expect(async () => {
    const now = await status(yap);
    await expect(index).toContainText(new RegExp(`${now.passages} passages from 3 notes, ${now.chats} chats and 1 memory item`), { timeout: 1_000 });
  }).toPass({ timeout: 15_000 });
  await expect(dialog.getByRole('row', { name: /^Search\b/ })).toContainText('Your meetings, notes, chats and memory: read and searched by meaning on this PC');
  await index.scrollIntoViewIfNeeded();
  await shot(main, '07-settings-counts');
  await closeSettings(main);
});
