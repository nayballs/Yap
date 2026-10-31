// A strong model's documents, researched as the Claude app does (31 Oct 2026;
// src-tauri/src/chat/tools.rs `refused_after_web` / `STRONG_LOCKED`,
// web/turn.rs `Reach`, chat/think.rs `auto_on`, ai/llm/anthropic.rs):
// - After reading the web, a strong model (a fake named like GPT-5: its
//   rich tools) still changes a note and writes a document — each a version
//   — while opening an app stays refused.
// - A new document's outline opens before any search (a search asked for
//   first is refused until it's up, with no chip); the model is told to read
//   the pages its facts come from (chat/documents.rs `research_note`).
// - The Sources row (src/lib/views/chat/sources.js, SourcesRow.svelte): each
//   page once, the pages read and cited first, four, then "+N more".
// - The on-device-sized model (a fake 4B) is refused the same note change.
// - A document's turn may search more than 3 times; a plain question can't.
// - Claude on Anthropic's Messages API: its own web search and web fetch in
//   Chat's request (the dynamic-filtering versions for Opus, 10 searches for
//   a document, its fetches sized to its request) — but not in the first
//   step, which makes the outline — Yap's web_search / read_page not
//   offered; its searches and page reads are chips and sources, its
//   citation a pill; the document written after its search; Think on by
//   itself for the document (the button lit while it's written, off after),
//   and not for a plain question.
// The models are a local fake (support/fake-llm.js, which speaks the
// Messages API's blocks and events too); the web a local fake
// (support/fake-web.js). Nothing goes on the internet.
import { test as base, expect, openView, setConfig, expectStore } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { startFakeWeb } from './support/fake-web.js';
import { sourceRow, pageKey, addressTitle, citedIn, FEW } from '../src/lib/views/chat/sources.js';

const EXA_KEY = 'fake-exa-key-for-tests';
const CLAUDE_KEY = 'sk-ant-e2e-fake-1234567890';

const test = base.extend({
  fakeLlm: [
    async ({}, use) => {
      const fake = await startFakeLlm();
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  fakeWeb: [
    async ({}, use) => {
      const fake = await startFakeWeb();
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({ fakeLlm, fakeWeb }, use) => {
      await use({
        name: 'chat-documents-research',
        env: fakeWeb.env,
        config: {
          llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-gpt-5' } },
          webSearch: true,
          webSearchKeys: { exa: EXA_KEY },
          openApps: true,
          meetingReminder: 'never',
        },
      });
    },
    { scope: 'worker' },
  ],
});

const box = (main) => main.getByRole('textbox', { name: 'Message' });
const thread = (main) => main.locator('.msgs');
const lastAnswer = (main) => thread(main).locator('.cbubble.assistant').last();
const chipsOfLast = (main) => thread(main).locator('.toolchips').last().locator('.toolchip');
const thinkButton = (main) => main.getByRole('button', { name: 'Think it through' });

async function freshChat(main) {
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
}

async function send(main, text) {
  await box(main).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
}

/** The document `create_document` made earlier in this turn. */
function madeDoc(record) {
  for (const m of [...record.messages].reverse()) {
    if (m.role !== 'tool') continue;
    try {
      const r = JSON.parse(m.content);
      if (Array.isArray(r?.data?.sections)) return r.data.id;
    } catch {
      /* not JSON */
    }
  }
  return null;
}

function gate() {
  let open;
  const promise = new Promise((r) => (open = r));
  return { promise, open };
}

test('the Sources row: each page once, the cited and read ones first, a few then +N more', () => {
  // One page however its address is written.
  expect(pageKey('https://www.forum.example/t/tidecaller-levels/')).toBe(pageKey('https://forum.example/t/tidecaller-levels'));
  expect(pageKey('https://steam.example/sharedfiles/filedetails/?id=1&utm_source=x#c')).toBe(pageKey('https://steam.example/sharedfiles/filedetails?id=1'));
  expect(pageKey('https://steam.example/sharedfiles/filedetails?id=2')).not.toBe(pageKey('https://steam.example/sharedfiles/filedetails?id=1'));
  // What an address says, when a title says nothing.
  expect(addressTitle('https://wiki.example/wiki/Fort+Joy')).toBe('Fort Joy');
  expect(addressTitle('https://guides.example/emberfall/tidecaller-solo.html')).toBe('tidecaller solo');
  expect(addressTitle('https://steam.example/sharedfiles/filedetails/?id=1')).toBe('');
  expect(addressTitle('https://forum.example/t/101')).toBe('');
  expect([...citedIn('A [2], B [3, 5] and C [6–7]; a [link](https://x.example) and [9](https://y.example).')]).toEqual([2, 3, 5, 6, 7]);

  const src = (n, url, title, more = {}) => ({ n, url, title, site: new URL(url).hostname.replace(/^www\./, ''), ...more });
  const sources = [
    src(1, 'https://steam.example/sharedfiles/filedetails/?id=1', 'Divinity: Original Sin 2'),
    src(2, 'https://steam.example/sharedfiles/filedetails/?id=2', 'Divinity: Original Sin 2'),
    src(3, 'https://wiki.example/wiki/Nether_Swap', 'Nether Swap', { opened: true, cited: true }),
    src(4, 'https://www.steam.example/sharedfiles/filedetails?id=1', 'Divinity: Original Sin 2'),
    src(5, 'https://wiki.example/wiki/Fort+Joy', 'Divinity: Original Sin 2'),
    src(6, 'https://forum.example/t/42', 'Lone wolf mage?'),
    src(7, 'https://forum.example/t/43', 'Hydro or aero first?'),
  ];
  const row = sourceRow(sources, 'It’s open on the right [7].');
  // Each page once (#4 is #1), the read-and-cited page and the answer's own
  // citation first, then the rest in order.
  expect(row.all.map((s) => s.n)).toEqual([3, 7, 1, 2, 5, 6]);
  expect(row.few.map((s) => s.n)).toEqual([3, 7, 1, 2]);
  expect(row.few.length).toBe(FEW);
  expect(row.more).toBe(2);
  // The game's name on several pages gives way to what an address says.
  expect(row.all.find((s) => s.n === 5).label).toBe('Fort Joy');
  expect(row.all.find((s) => s.n === 1).label).toBe('Divinity: Original Sin 2');
  expect(row.all.find((s) => s.n === 3).label).toBe('Nether Swap');
  // One more than a few: shown, never "+1 more".
  expect(sourceRow(sources.slice(0, 6)).all.length).toBe(FEW + 1);
  expect(sourceRow(sources.slice(0, 6)).more).toBe(0);
  // Older answers (no marks): their [n] first; meetings' passages kept apart.
  const passages = [
    { n: 1, note: 4, line: 2, title: 'Stand-up' },
    { n: 2, note: 4, line: 9, title: 'Stand-up' },
  ];
  expect(sourceRow(passages, 'Yes [2].').all.map((s) => s.n)).toEqual([2, 1]);
});

test('after the web a strong model still edits notes and writes its document; an app stays refused', async ({ yap, main, fakeLlm, shot }) => {
  await yap.invoke('e2e_opened_apps', { clear: true });
  const mine = await yap.invoke('note_create', { title: 'Wizard notes', content: 'Recruit Gale later.\n' });
  fakeLlm.scriptChat([
    // A new document: its outline first, then the web.
    { tools: [{ name: 'create_document', args: { title: 'Gale guide', summary: 'Where Gale is and how to recruit him.', sections: [{ key: 'Where', intent: 'where to find him' }] } }] },
    { tools: [{ name: 'web_search', args: { query: "Gale Baldur's Gate 3" } }] },
    {
      tools: [
        { name: 'edit_note', args: { note_id: mine.id, old_text: 'Recruit Gale later.', new_text: 'Recruit Gale on the beach by the crash site [1].' } },
        { name: 'open_app', args: { name: 'Calculator' } },
        { name: 'write_section', args: (r) => ({ note_id: madeDoc(r), key: 'Where', markdown: 'On the beach west of the crash site [1].' }) },
      ],
    },
    { content: "It's open on the right." },
  ]);
  await freshChat(main);
  await send(main, 'Make me a short guide to recruiting Gale in BG3, and fix my wizard notes');
  await expect(lastAnswer(main)).toContainText("It's open on the right.", { timeout: 20_000 });
  const chips = chipsOfLast(main);
  await expect(chips.filter({ hasText: 'Searched the web' })).toHaveCount(1);
  await expect(chips.filter({ hasText: "Didn't open an app: not after reading the web" })).toHaveCount(1);
  await expect(chips.filter({ hasText: 'not after reading the web' })).toHaveCount(1);
  await expect(chips.filter({ hasText: 'Wizard notes' })).toHaveCount(1);
  await expect(chips.filter({ hasText: 'Gale guide' }).first()).toBeVisible();
  await shot(main, '01-edits-after-the-web');
  // The note changed (a version: Undo), the document written, no app opened.
  await expectStore(yap, 'notes.json', (s) => s.notes.some((n) => n.title === 'Wizard notes' && n.content.includes('crash site')));
  await expectStore(yap, 'notes.json', (s) => s.notes.some((n) => n.title === 'Gale guide' && n.content.includes('On the beach west of the crash site')));
  expect((await yap.invoke('note_versions', { id: mine.id })).length).toBeGreaterThan(0);
  expect(await yap.invoke('e2e_opened_apps')).toEqual([]);
  // The prompt never told it it couldn't change notes after the web.
  const first = fakeLlm.requests.find((r) => r.user?.includes('recruiting Gale'));
  expect(first.system).toContain('writing and changing notes and documents goes on as usual');
  expect(first.system).not.toContain("won't open apps, change notes");
});

test('the on-device-sized model is still refused a note change after the web', async ({ yap, main, fakeLlm }) => {
  const cfg = await yap.invoke('get_config');
  await setConfig(yap, main, { llmScopes: { ...cfg.llmScopes, chat: { ...cfg.llmScopes.chat, model: 'qwen3.5-4b' } } });
  try {
    const mine = await yap.invoke('note_create', { title: 'Druid notes', content: 'Find Halsin.\n' });
    fakeLlm.scriptChat([
      { tools: [{ name: 'web_search', args: { query: "Gale Baldur's Gate 3" } }] },
      { tools: [{ name: 'edit_note', args: { note_id: mine.id, old_text: 'Find Halsin.', new_text: 'Find Gale.' } }] },
      { content: 'You can ask me to change it now.' },
    ]);
    await freshChat(main);
    await send(main, 'Update my druid notes with where Gale is in BG3');
    await expect(lastAnswer(main)).toContainText('You can ask me to change it now.', { timeout: 20_000 });
    await expect(chipsOfLast(main).filter({ hasText: "Didn't change a note: not after reading the web" })).toHaveCount(1);
    await expectStore(yap, 'notes.json', (s) => s.notes.some((n) => n.title === 'Druid notes' && n.content.includes('Find Halsin.')));
    const first = fakeLlm.requests.find((r) => r.user?.includes('druid notes'));
    expect(first.system).toContain("won't open apps, change notes");
  } finally {
    await setConfig(yap, main, { llmScopes: cfg.llmScopes });
  }
});

test('a new document’s outline opens before any search; a search asked for first waits for it', async ({ main, fakeLlm, shot }) => {
  const held = gate();
  const before = fakeLlm.requests.length;
  fakeLlm.scriptChat([
    // Searched first: refused until the outline is up (no chip).
    { tools: [{ name: 'web_search', args: { query: 'Gale BG3 recruit' } }] },
    {
      tools: [
        {
          name: 'create_document',
          args: { title: 'Recruiting Gale', summary: 'Where Gale is and how to recruit him.', sections: [{ key: 'Where', intent: 'where to find him' }, { key: 'How', intent: 'how to recruit him' }] },
        },
      ],
    },
    // Held while the spec looks at the outline, before the search runs.
    { gate: held.promise, tools: [{ name: 'web_search', args: { query: 'Gale BG3 location' } }] },
    { tools: [{ name: 'read_page', args: { url: 'https://bg3.wiki/wiki/Gale' } }] },
    {
      tools: [
        { name: 'write_section', args: (r) => ({ note_id: madeDoc(r), key: 'Where', markdown: 'On the beach west of the crash site [1].' }) },
        { name: 'write_section', args: (r) => ({ note_id: madeDoc(r), key: 'How', markdown: 'Pull him out of the rune circle [1].' }) },
      ],
    },
    { content: "It's open on the right." },
  ]);
  await freshChat(main);
  await send(main, 'Make me a short guide to recruiting Gale in BG3');
  // The outline is open beside the chat, and nothing has been looked up yet.
  const panel = main.getByTestId('chat-doc');
  await expect(panel).toBeVisible({ timeout: 20_000 });
  await expect(panel.locator('.doctitle')).toHaveText('Recruiting Gale');
  await expect(panel.locator('.docbody .md-pending')).toHaveText(['Writing: Where…', 'Writing: How…']);
  await expect.poll(() => fakeLlm.requests.slice(before).some((r) => r.gated)).toBe(true);
  await expect(chipsOfLast(main).filter({ hasText: 'Searched the web' })).toHaveCount(0);
  await shot(main, '04-outline-before-any-search');
  held.open();
  await expect(lastAnswer(main)).toContainText("It's open on the right.", { timeout: 20_000 });
  const chips = chipsOfLast(main);
  await expect(chips.filter({ hasText: 'Searched the web' })).toHaveCount(1);
  await expect(chips.filter({ hasText: 'Read bg3.wiki/wiki/Gale' })).toHaveCount(1);
  await expect(chips.filter({ hasText: 'Outline' })).toHaveCount(0);
  await expect(panel.locator('.docbody')).toContainText('On the beach west of the crash site');
  // The model was told: the outline first, then read what it quotes.
  const steps = fakeLlm.requests.slice(before).filter((r) => r.tools?.length);
  const first = steps.find((r) => r.user?.includes('recruiting Gale'));
  expect(first.user).toContain('outline it first — create_document now, before any search');
  expect(first.user).toContain("A search result's snippet isn't a source");
  expect(first.system).toContain('create_document first, before any search');
  // Its first search went back refused, with what to do.
  const refused = steps[1].messages.filter((m) => m.role === 'tool').map((m) => m.content);
  expect(refused.some((c) => c.includes('Outline first: call create_document now'))).toBe(true);
  // The Sources row: the page it read and cites first.
  const row = thread(main).getByTestId('chat-sources').last();
  await expect(row.getByTestId('chat-source').first()).toContainText('Gale - bg3.wiki');
  await expect(row.getByTestId('chat-sources-more')).toHaveCount(0);
});

test('a document’s turn may search more than three times; a plain question may not', async ({ main, fakeLlm }) => {
  const four = (topic) => ({ tools: [1, 2, 3, 4].map((n) => ({ name: 'web_search', args: { query: `Gale ${topic} part ${n}` } })) });
  fakeLlm.scriptChat([
    { tools: [{ name: 'create_document', args: { title: 'Gale in detail', summary: 'Gale, level by level.', sections: [{ key: 'Levels', intent: 'level by level' }] } }] },
    four('guide'),
    { tools: [{ name: 'write_section', args: (r) => ({ note_id: madeDoc(r), key: 'Levels', markdown: '| Level | Pick |\n|---|---|\n| 1 | Magic Missile [1] |' }) }] },
    { content: 'Here it is.' },
  ]);
  await freshChat(main);
  await send(main, 'Make me a detailed guide to Gale in BG3, level by level');
  await expect(lastAnswer(main)).toContainText('Here it is.', { timeout: 20_000 });
  await expect(chipsOfLast(main).filter({ hasText: 'Searched the web' })).toHaveCount(4);
  await expect(chipsOfLast(main).filter({ hasText: 'searches a turn' })).toHaveCount(0);
  // The model was told how far it may look.
  const asked = fakeLlm.requests.find((r) => r.user?.includes('detailed guide to Gale'));
  expect(asked.user).toMatch(/up to 10 searches and \d+ page reads this turn/);

  fakeLlm.scriptChat([four('where'), { content: 'On the beach.' }]);
  await send(main, 'Where is Gale in BG3?');
  await expect(lastAnswer(main)).toContainText('On the beach.', { timeout: 20_000 });
  await expect(chipsOfLast(main).filter({ hasText: 'Searched the web' })).toHaveCount(3);
  await expect(chipsOfLast(main).filter({ hasText: '3 searches a turn' })).toHaveCount(1);
});

test('Claude searches and reads pages itself in Chat’s request, thinks first for a document, and cites what it read', async ({ yap, main, fakeLlm, shot }) => {
  const cfg = await yap.invoke('get_config');
  const claude = { enabled: true, provider: 'anthropic', baseUrl: fakeLlm.base, model: 'claude-opus-5-5', apiKey: CLAUDE_KEY, apiKeys: {}, prompt: '', disableThinking: false };
  await setConfig(yap, main, { webSearchNative: true, llmScopes: { ...cfg.llmScopes, chat: claude } });
  try {
    const held = gate();
    const searching = gate();
    const before = fakeLlm.requests.length;
    // Its search finds several pages, some titled only with the game.
    const found = [
      { url: 'https://emberfall.example/wiki/Tidecaller', title: 'Tidecaller - Emberfall Wiki' },
      { url: 'https://forum.example/t/tidecaller-levels', title: 'Emberfall' },
      { url: 'https://guides.example/emberfall/tidecaller-solo', title: 'Emberfall' },
      { url: 'https://forum.example/t/101', title: 'Emberfall' },
      { url: 'https://builds.example/tidecaller', title: 'Tidecaller builds' },
      { url: 'https://www.forum.example/t/tidecaller-levels/', title: 'Emberfall' },
      { url: 'https://news.example/emberfall-patch-1-2', title: 'Emberfall patch 1.2' },
      { url: 'https://wiki.example/emberfall/gear', title: 'Gear' },
    ];
    fakeLlm.scriptChat([
      {
        // Held while the spec looks at the Think button mid-turn. The
        // outline first: no web tools in this step's request.
        gate: held.promise,
        reasoning: 'Outline first: levels, then the gear.',
        tools: [{ name: 'create_document', args: { title: 'Tidecaller guide', summary: 'A solo Tidecaller, level by level.', sections: [{ key: 'Levels', intent: 'level by level' }] } }],
      },
      {
        // Held while the spec looks at the outline, before its search.
        gate: searching.promise,
        before: "I'll look this up.",
        server: [
          { search: 'Emberfall tidecaller levels', results: found },
          { fetch: 'https://emberfall.example/wiki/Tidecaller', title: 'Tidecaller - Emberfall Wiki', text: 'Rain Bolt at 1.' },
          { fetch: 'https://forum.example/t/tidecaller-help', error: 'url_not_accessible' },
        ],
        tools: [
          {
            name: 'write_section',
            args: (r) => ({
              note_id: madeDoc(r),
              key: 'Levels',
              markdown: '| Level | Skill |\n|---|---|\n| 1 | Rain Bolt ([emberfall.example](https://emberfall.example/wiki/Tidecaller)) |\n| 2 | Not confirmed: no page gave the level-2 skill |\n\n*The tips here are my suggestions, not from a source.*',
            }),
          },
        ],
      },
      { content: "It's open on the right. Rain Bolt comes first.", cite: [{ url: 'https://emberfall.example/wiki/Tidecaller', title: 'Tidecaller - Emberfall Wiki', text: 'Rain Bolt at 1.' }] },
    ]);
    await freshChat(main);
    await send(main, 'Make me a detailed level-by-level Tidecaller guide for Emberfall');
    // Think lit by Yap while the document's written.
    await expect.poll(() => fakeLlm.requests.slice(before).some((r) => r.gated)).toBe(true);
    await expect(thinkButton(main)).toHaveAttribute('aria-pressed', 'true');
    await shot(main, '02-claude-thinks-for-a-document');
    held.open();
    // The outline opens before any search: the next step (its search) held.
    const panel = main.getByTestId('chat-doc');
    await expect(panel.locator('.docbody .md-pending')).toHaveText(['Writing: Levels…'], { timeout: 20_000 });
    await expect.poll(() => fakeLlm.requests.slice(before).filter((r) => r.api === 'messages' && r.gated).length).toBe(2);
    await expect(chipsOfLast(main).filter({ hasText: 'Searched the web' })).toHaveCount(0);
    searching.open();
    await expect(lastAnswer(main)).toContainText('Rain Bolt comes first', { timeout: 20_000 });
    await expect(thinkButton(main)).toHaveAttribute('aria-pressed', 'false');
    // Its searches and pages: chips; its citation: a pill; the Sources row.
    const chips = chipsOfLast(main);
    await expect(chips.filter({ hasText: 'Searched the web with Anthropic: "Emberfall tidecaller levels"' })).toHaveCount(1);
    await expect(chips.filter({ hasText: 'Read emberfall.example/wiki/Tidecaller' })).toHaveCount(1);
    await expect(chips.filter({ hasText: "Couldn't read forum.example/t/tidecaller-help: the page didn't open" })).toHaveCount(1);
    await expect(lastAnswer(main).locator('a.md-cite')).toHaveCount(1);
    await expect(lastAnswer(main)).not.toContainText("I'll look this up.");
    // **The Sources row, compact**: each page once (the forum thread found
    // twice), the page it read and cites first, a few, then "+N more".
    const row = thread(main).getByTestId('chat-sources').last();
    const shownChips = row.getByTestId('chat-source');
    await expect(shownChips).toHaveCount(4);
    await expect(shownChips.first()).toContainText('Tidecaller - Emberfall Wiki');
    const more = row.getByTestId('chat-sources-more');
    await expect(more).toHaveText('+3 more');
    await expect(more).toHaveAttribute('aria-expanded', 'false');
    await shot(main, '03-claude-searched-itself');
    await more.click();
    await expect(shownChips).toHaveCount(7);
    await expect(more).toHaveText('Show fewer');
    // A title several pages share gives way to what the address says.
    await expect(row).toContainText('tidecaller solo');
    await expect(row).toContainText('tidecaller levels');
    await shot(main, '05-sources-expanded');
    await more.click();
    await expect(shownChips).toHaveCount(4);
    // Saved with the page it read marked (chats.json; an older Yap drops it).
    await expectStore(yap, 'chats.json', (s) =>
      s.conversations.some((c) => c.messages.some((m) => m.role === 'assistant' && m.sources?.some((x) => x.url === 'https://emberfall.example/wiki/Tidecaller' && x.opened && x.cited)))
    );
    // The document written after its search.
    await expectStore(yap, 'notes.json', (s) => s.notes.some((n) => n.title === 'Tidecaller guide' && n.content.includes('| 2 | Not confirmed')));

    // The requests: the Messages API, its server tools beside Yap's, deep —
    // none in the first, which makes the outline.
    const steps = fakeLlm.requests.slice(before).filter((r) => r.api === 'messages');
    expect(steps.length).toBe(3);
    const [one, two, three] = steps;
    expect(one.url).toBe('/v1/messages');
    expect(one.headers['x-api-key']).toBe(CLAUDE_KEY);
    expect(one.headers['anthropic-version']).toBe('2023-06-01');
    expect(one.serverTools).toEqual([]);
    expect(two.serverTools).toEqual([
      { type: 'web_search_20260209', name: 'web_search', maxUses: 10 },
      { type: 'web_fetch_20260209', name: 'web_fetch', maxUses: expect.any(Number) },
    ]);
    // Its page reads sized to what its request holds, and said so.
    const reads = two.serverTools[1].maxUses;
    expect(reads).toBeGreaterThanOrEqual(3);
    expect(reads).toBeLessThanOrEqual(10);
    expect(one.user).toContain(`up to 10 searches and ${reads} page reads this turn`);
    expect(one.user).toContain('web_fetch the pages that requirements, levels, numbers and locations come from');
    expect(one.tools).not.toContain('web_search');
    expect(one.tools).not.toContain('read_page');
    expect(one.tools).toContain('edit_document');
    // Thought through: adaptive, high effort, its summary shown.
    expect(one.think.thinking).toEqual({ type: 'adaptive', display: 'summarized' });
    expect(one.think.outputConfig).toEqual({ effort: 'high' });
    // Cached: the tools and system prompt, and the conversation.
    expect(one.systemCache).toEqual({ type: 'ephemeral' });
    expect(one.cacheControl).toEqual({ type: 'ephemeral' });
    // Its own blocks went back each next step: the thinking and its call,
    // then the search, the pages and its call.
    const sentOf = (step) => step.body.messages.filter((m) => m.role === 'assistant').flatMap((m) => m.content.map((b) => b.type));
    expect(sentOf(two)).toEqual(expect.arrayContaining(['thinking', 'tool_use']));
    expect(sentOf(three)).toEqual(expect.arrayContaining(['server_tool_use', 'web_search_tool_result', 'web_fetch_tool_result', 'tool_use']));
    expect(one.system).toContain('Use web_fetch to open a page');
    await expectStore(yap, 'chats.json', (s) => s.conversations.some((c) => c.messages.some((m) => m.role === 'assistant' && m.thinking?.text?.includes('Outline first: levels, then the gear.'))));

    // A plain question: no Think, the usual reach.
    const plain = fakeLlm.requests.length;
    fakeLlm.scriptChat([{ content: 'Rain Bolt.' }]);
    await send(main, 'Which skill comes first?');
    await expect(lastAnswer(main)).toHaveText('Rain Bolt.', { timeout: 20_000 });
    const q = fakeLlm.requests.slice(plain).find((r) => r.api === 'messages');
    expect(q.think.thinking).toBeNull();
    expect(q.serverTools[0].maxUses).toBe(5);
  } finally {
    await setConfig(yap, main, { llmScopes: cfg.llmScopes });
  }
});
