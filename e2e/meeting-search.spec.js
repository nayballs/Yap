// Meeting search and Ask across meetings (src-tauri/src/search/): Settings →
// Privacy & data → "Search my meetings by meaning" downloads its model (and
// the on-device engine) from stand-ins, switches it on and reads every
// meeting and note in the background; the Meetings view's search lists the
// moments that match by meaning, each opening its meeting at the line; the
// Ask bar asks Chat across the meetings, whose answer cites passages as
// pills that open the moment, keeps the conversation's scope for a
// follow-up, loses a citation that points nowhere, and — with no AI model —
// shows the passages themselves; Rebuild and Delete; the network receipts
// say it all stayed on this PC. The embedding model is a stand-in server
// (support/fake-embed.js: words hashed into vectors, a few words sharing a
// sense) and the chat model a scripted fake (support/fake-llm.js); nothing
// is downloaded from the internet or run but Yap.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, openSettings, closeSettings, closeToasts, setConfig, sidebar } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { startFakeEmbed } from './support/fake-embed.js';
import { startFakeLocalModels } from './support/local-llm.js';
import { RUNS } from './support/yap.js';

const NOW = Math.floor(Date.now() / 1000);
const DAY = 86_400;
const BUDGET_TS = NOW - 9 * DAY;
const LAUNCH_TS = NOW - 2 * DAY;

/** A made-up team's two meetings and a personal note. */
const NOTES = {
  folders: ['Personal', 'Meetings'],
  notes: [
    {
      id: 1,
      title: 'Q3 budget review',
      noteType: 'meeting',
      folder: 'Meetings',
      source: 'meeting',
      participants: ['Priya Shah'],
      createdTs: BUDGET_TS,
      updatedTs: BUDGET_TS,
      speakers: { 'them-1': { name: 'Priya Shah', source: 'user', locked: true } },
      transcript: [
        { source: 'them', speaker: 'them-1', text: 'The venue quote came in at four thousand pounds, over what finance allowed.', ts: BUDGET_TS },
        { source: 'you', text: 'Can we move some of the catering money across?', ts: BUDGET_TS + 20 },
        { source: 'them', speaker: 'them-1', text: 'Only if Tom signs it off by Friday.', ts: BUDGET_TS + 40 },
        { source: 'them', speaker: 'them-1', text: "There's a hiring freeze until January, except the support role.", ts: BUDGET_TS + 60 },
      ],
    },
    {
      id: 2,
      title: 'Launch review',
      noteType: 'meeting',
      folder: 'Meetings',
      source: 'meeting',
      createdTs: LAUNCH_TS,
      updatedTs: LAUNCH_TS,
      transcript: [
        { source: 'them', text: 'We agreed the launch moves to March third so the pricing page ships with it.', ts: LAUNCH_TS },
        { source: 'you', text: "I'll update the pricing copy by Friday.", ts: LAUNCH_TS + 20 },
      ],
      enhancedContent: '## Decisions\n- The launch moves to March third',
    },
    {
      id: 3,
      title: 'Dentist',
      folder: 'Personal',
      source: 'manual',
      createdTs: NOW - 5 * DAY,
      updatedTs: NOW - 5 * DAY,
      content: 'Appointment with Dr Patel on Thursday at 9:40 for a scale and polish.',
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
  // The engine and the search model, as stand-ins on 127.0.0.1 (4 MB of
  // random bytes for the model: its size goes in the sources file too).
  fakeModels: [
    async ({}, use) => {
      const dir = path.join(RUNS, 'meeting-search-sources');
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
        name: 'meeting-search',
        config: {
          llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat' } },
        },
        data: { 'notes.json': NOTES },
        env: { ...fakeModels.env, YAP_E2E_EMBED_URL: fakeEmbed.url },
      });
    },
    { scope: 'worker' },
  ],
});

const status = (yap) => yap.invoke('search_status');

/** The search model installed and every passage read: the first test does it
 *  through Settings; a test on a fresh Yap (after a failure, Playwright starts
 *  the worker again) does it here. */
async function installed(yap) {
  if (!(await status(yap)).installed) await yap.invoke('search_install');
  await expect.poll(async () => (await status(yap)).phase, { timeout: 20_000 }).toBe('ready');
}

test('Settings: the search model downloads, and every meeting and note is read', async ({ yap, main, fakeEmbed, fakeModels, shot }) => {
  const dialog = await openSettings(main, 'Privacy & data');
  const group = dialog.locator('[data-setting="meeting-search"]').locator('..');
  const toggle = dialog.getByRole('button', { name: 'Search by meaning', exact: true });
  await expect(toggle).toBeDisabled();
  const row = dialog.locator('[data-setting="meeting-search-model"]');
  await expect(row).toContainText('EmbeddingGemma 300M');
  await expect(row).toContainText('the on-device AI engine included');
  // "What leaves this PC" says where it's searched.
  await expect(dialog.getByRole('row', { name: /^Search\b/ })).toContainText('Searched on this PC');
  await shot(main, '01-settings-before-download');

  // The download, held half-way to see its progress.
  fakeModels.hold();
  await row.getByRole('button', { name: /^Download/ }).click();
  await expect(row).toContainText(/Downloading .*\d+%/);
  await shot(main, '02-settings-downloading');
  fakeModels.release();
  await expect(toggle).toHaveAttribute('aria-pressed', 'true', { timeout: 20_000 });
  await expect(row).toContainText('EmbeddingGemma 300M · 4.0 MB');
  expect(fakeModels.requests).toEqual(expect.arrayContaining(['/llm/runtime', '/llm/embeddinggemma.gguf']));

  // Read in the background: every passage of the three notes.
  await expect.poll(async () => (await status(yap)).phase, { timeout: 20_000 }).toBe('ready');
  const s = await status(yap);
  expect(s.passages).toBeGreaterThanOrEqual(4);
  expect(s.indexed).toBe(s.passages);
  expect(s.notes).toBe(3);
  expect(s.indexBytes).toBeGreaterThan(0);
  expect(fakeEmbed.requests.some((r) => r.texts >= 4)).toBe(true);
  expect(fs.existsSync(path.join(yap.dataDir, 'search-index', 'vectors.bin'))).toBe(true);
  expect(fs.existsSync(path.join(yap.dataDir, 'models', 'search', 'NOTICE.txt'))).toBe(true);
  const index = dialog.locator('[data-setting="meeting-search-index"]');
  await expect(index).toContainText(`${s.passages} passages from 3 notes`);
  await expect(dialog.getByRole('row', { name: /^Search\b/ })).toContainText('read and searched by meaning on this PC');
  await group.scrollIntoViewIfNeeded();
  await shot(main, '03-settings-indexed');
  await closeSettings(main);
});

test('the Meetings view finds moments by meaning, and each opens at its line', async ({ yap, main, shot }) => {
  await installed(yap);
  await openView(main, 'Meetings');
  const past = main.getByRole('region', { name: 'Past meeting notes' });
  const search = past.getByRole('searchbox', { name: 'Search meetings' });
  // No word of it is in the meeting: "staff" means "hiring" here. The
  // passage is found; with no word to go by, it opens at its first line.
  await search.fill('can we take on new staff');
  const moments = main.getByRole('region', { name: 'Moments that match' });
  await expect(moments).toContainText('by meaning');
  const first = moments.getByRole('button').first();
  await expect(first).toContainText('Q3 budget review');
  await expect(first).toContainText('Priya Shah');
  // The speaker is said once, above the line.
  await expect(first).not.toContainText('Priya Shah: ');
  // No "nothing matches" under a moment that did.
  await expect(main.getByText('No meeting notes match that.')).toHaveCount(0);
  await shot(main, '04-meetings-moments-by-meaning');

  // A word of the line itself picks the line.
  await search.fill('a freeze on new staff');
  await expect(first).toContainText("There's a hiring freeze until January");
  await first.click();
  await expect(sidebar(main).getByRole('button', { name: 'Notes', exact: true })).toHaveAttribute('aria-current', 'page');
  await expect(main.getByPlaceholder('Untitled Note')).toHaveValue('Q3 budget review');
  // The line it came from: scrolled to, focused, lit.
  await expect(main.locator('.bubble:focus')).toContainText("There's a hiring freeze until January");
  await shot(main, '05-notes-at-the-moment');

  // A personal note by meaning: its notes open.
  await openView(main, 'Meetings');
  await past.getByRole('searchbox', { name: 'Search meetings' }).fill('when is my teeth cleaning');
  await expect(moments.getByRole('button').first()).toContainText('Dentist');
  await moments.getByRole('button').first().click();
  await expect(main.getByPlaceholder('Untitled Note')).toHaveValue('Dentist');
});

test('Ask across meetings: an answer cites passages, each pill opens the moment, a follow-up stays in the meetings', async ({
  yap,
  main,
  fakeLlm,
  shot,
}) => {
  await installed(yap);
  await closeToasts(main);
  // It cites passage 1, and a passage that isn't there.
  fakeLlm.scriptChat([{ content: 'Priya said there is a hiring freeze until January [1], and the rest is unclear [9].' }]);
  await openView(main, 'Meetings');
  const ask = main.getByRole('textbox', { name: 'Ask about your meetings' });
  await ask.fill('Are we allowed to hire new staff?');
  await ask.press('Enter');
  await expect(sidebar(main).getByRole('button', { name: 'Chat', exact: true })).toHaveAttribute('aria-current', 'page');
  const thread = main.locator('.msgs');
  await expect(thread).toContainText('Priya said there is a hiring freeze until January');
  // The number that points nowhere is gone; the real one is a pill.
  await expect(thread).not.toContainText('[9]');
  const pill = thread.locator('a.md-passage', { hasText: '1' });
  await expect(pill).toHaveCount(1);
  await expect(main.getByTestId('chat-across-meetings')).toContainText('Asking across your meetings and notes');
  await expect(thread.locator('.toolchip').first()).toContainText(/Searched your meetings by meaning · \d+ passages?/);
  const sources = main.getByRole('group', { name: 'From your meetings' });
  await expect(sources.getByRole('button', { name: /^Passage 1:/ })).toBeVisible();
  await shot(main, '06-chat-cited-answer');

  // What the model was given: numbered passages, and the rule to cite them.
  const req = fakeLlm.requests.filter((r) => r.kind === 'other').at(-1);
  expect(req.system).toContain('The user is asking about their meetings');
  expect(req.system).toContain('cite the passage it comes from');
  expect(req.user).toMatch(/<passage n="1" title="Q3 budget review" date="[^"]+" at="\d+:\d\d" from="transcript">/);
  expect(req.user).toContain("Priya Shah: There's a hiring freeze until January");

  // The saved answer's first source: the moment the pill opens.
  const convs = await yap.invoke('chats_list');
  const conv = await yap.invoke('chat_get', { id: convs[0].id });
  expect(conv.scope).toBe('meetings');
  const src = conv.messages.at(-1).sources.find((s) => s.n === 1);
  expect(src).toMatchObject({ note: 1, field: 'transcript' });
  const note = NOTES.notes.find((n) => n.id === src.note);
  await pill.click();
  await expect(sidebar(main).getByRole('button', { name: 'Notes', exact: true })).toHaveAttribute('aria-current', 'page');
  await expect(main.getByPlaceholder('Untitled Note')).toHaveValue(note.title);
  await expect(main.locator('.bubble:focus')).toContainText(note.transcript[src.line].text);
  await shot(main, '07-pill-opened-the-moment');

  // A follow-up in that conversation is asked across the meetings too.
  fakeLlm.scriptChat([{ content: 'The venue quote was four thousand pounds [1].' }]);
  await openView(main, 'Chat');
  await main.locator('.conv').filter({ hasText: 'Are we allowed to hire new staff?' }).click();
  await expect(main.getByTestId('chat-across-meetings')).toBeVisible();
  await main.getByRole('textbox', { name: 'Message' }).fill('And how much was the venue?');
  await main.getByRole('button', { name: 'Send' }).click();
  await expect(thread).toContainText('The venue quote was four thousand pounds');
  const follow = fakeLlm.requests.filter((r) => r.kind === 'other').at(-1);
  expect(follow.user).toContain('<passage n="1"');
  expect(follow.user).toContain('four thousand pounds');
  await expect(thread.locator('a.md-passage')).toHaveCount(2);
});

test('with no AI model, Ask across meetings shows the passages themselves', async ({ yap, main, shot }) => {
  await setConfig(yap, main, { llmScopes: {}, ppProvider: 'groq', ppApiKey: '', ppApiKeys: {} });
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
  await main.getByRole('button', { name: 'Ask across your meetings' }).click();
  await expect(main.getByText('Ask across your meetings and notes')).toBeVisible();
  await main.getByRole('textbox', { name: 'Message' }).fill('When does the launch happen?');
  await main.getByRole('button', { name: 'Send' }).click();
  const thread = main.locator('.msgs');
  await expect(thread).toContainText('No AI model is set up, so here are the passages that match your question');
  await expect(thread).toContainText('We agreed the launch moves to March third');
  await expect(thread.locator('a.md-passage').first()).toBeVisible();
  await shot(main, '08-no-model-passages');
  // Its pill opens the moment too.
  const convs = await yap.invoke('chats_list');
  const conv = await yap.invoke('chat_get', { id: convs[0].id });
  const answer = conv.messages.at(-1);
  expect(answer.model ?? null).toBeNull();
  expect(answer.sources.length).toBeGreaterThan(0);
  await thread.locator('a.md-passage').first().click();
  await expect(sidebar(main).getByRole('button', { name: 'Notes', exact: true })).toHaveAttribute('aria-current', 'page');
});

test('Rebuild reads everything again, Delete takes the model and the index, and it all stayed on this PC', async ({ yap, main, fakeEmbed, shot }) => {
  await installed(yap);
  const before = fakeEmbed.requests.length;
  const dialog = await openSettings(main, 'Privacy & data');
  const index = dialog.locator('[data-setting="meeting-search-index"]');
  await index.getByRole('button', { name: 'Rebuild' }).click();
  await expect.poll(async () => (await status(yap)).phase, { timeout: 20_000 }).toBe('ready');
  await expect.poll(() => fakeEmbed.requests.slice(before).filter((r) => !r.query).length).toBeGreaterThan(0);
  await expect(index).toContainText('passages from 3 notes');

  // Every request to the model was to this PC, and said what it was for.
  const sum = await yap.invoke('connections_summary');
  const labels = JSON.stringify(sum);
  expect(labels).toContain('Search by meaning · on-device model');
  expect(labels).toContain('Search model download');

  await index.getByRole('button', { name: 'Delete…' }).click();
  const confirm = main.getByRole('alertdialog', { name: 'Delete the search model and its index?' });
  await confirm.getByRole('button', { name: 'Delete', exact: true }).click();
  const row = dialog.locator('[data-setting="meeting-search-model"]');
  await expect(row.getByRole('button', { name: /^Download/ })).toBeVisible();
  await expect(dialog.getByRole('button', { name: 'Search by meaning', exact: true })).toHaveAttribute('aria-pressed', 'false');
  expect(fs.existsSync(path.join(yap.dataDir, 'search-index'))).toBe(false);
  expect(fs.existsSync(path.join(yap.dataDir, 'models', 'search', 'embeddinggemma-300M-Q8_0.gguf'))).toBe(false);
  // The engine stays: the on-device model may use it.
  expect(fs.existsSync(path.join(yap.dataDir, 'llm', 'llamafile.exe'))).toBe(true);
  await shot(main, '09-settings-deleted');
  await closeSettings(main);
});
