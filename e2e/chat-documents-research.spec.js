// A strong model's documents, researched as the Claude app does (31 Oct 2026;
// src-tauri/src/chat/tools.rs `refused_after_web` / `STRONG_LOCKED`,
// web/turn.rs `Reach`, chat/think.rs `auto_on`, ai/llm/anthropic.rs):
// - After reading the web, a strong model (a fake named like GPT-5: its
//   rich tools) still changes a note and writes a document — each a version
//   — while opening an app stays refused.
// - The on-device-sized model (a fake 4B) is refused the same note change.
// - A document's turn may search more than 3 times; a plain question can't.
// - Claude on Anthropic's Messages API: its own web search and web fetch in
//   Chat's request (the dynamic-filtering versions for Opus, 10 uses for a
//   document), Yap's web_search / read_page not offered; its searches and
//   page reads are chips and sources, its citation a pill; the document
//   written after its search; Think on by itself for the document (the
//   button lit while it's written, off after), and not for a plain question.
// The models are a local fake (support/fake-llm.js, which speaks the
// Messages API's blocks and events too); the web a local fake
// (support/fake-web.js). Nothing goes on the internet.
import { test as base, expect, openView, setConfig, expectStore } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { startFakeWeb } from './support/fake-web.js';

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

test('after the web a strong model still edits notes and writes its document; an app stays refused', async ({ yap, main, fakeLlm, shot }) => {
  await yap.invoke('e2e_opened_apps', { clear: true });
  const mine = await yap.invoke('note_create', { title: 'Wizard notes', content: 'Recruit Gale later.\n' });
  fakeLlm.scriptChat([
    { tools: [{ name: 'web_search', args: { query: "Gale Baldur's Gate 3" } }] },
    {
      tools: [
        { name: 'edit_note', args: { note_id: mine.id, old_text: 'Recruit Gale later.', new_text: 'Recruit Gale on the beach by the crash site [1].' } },
        { name: 'open_app', args: { name: 'Calculator' } },
        { name: 'create_document', args: { title: 'Gale guide', summary: 'Where Gale is and how to recruit him.', sections: [{ key: 'Where', intent: 'where to find him' }] } },
      ],
    },
    { tools: [{ name: 'write_section', args: (r) => ({ note_id: madeDoc(r), key: 'Where', markdown: 'On the beach west of the crash site [1].' }) }] },
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
  await expect(chips.filter({ hasText: 'Gale guide' }).first()).toBeAttached();
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

test('a document’s turn may search more than three times; a plain question may not', async ({ main, fakeLlm }) => {
  const four = (topic) => ({ tools: [1, 2, 3, 4].map((n) => ({ name: 'web_search', args: { query: `Gale ${topic} part ${n}` } })) });
  fakeLlm.scriptChat([four('guide'), { content: 'Here it is.' }]);
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
    const before = fakeLlm.requests.length;
    fakeLlm.scriptChat([
      {
        // Held while the spec looks at the Think button mid-turn.
        gate: held.promise,
        reasoning: 'A level table first, then the gear.',
        before: "I'll look this up.",
        server: [
          { search: 'Emberfall tidecaller levels', results: [{ url: 'https://emberfall.example/wiki/Tidecaller', title: 'Tidecaller - Emberfall Wiki' }] },
          { fetch: 'https://emberfall.example/wiki/Tidecaller', title: 'Tidecaller - Emberfall Wiki', text: 'Rain Bolt at 1.' },
          { fetch: 'https://forum.example/t/tidecaller-help', error: 'url_not_accessible' },
        ],
        tools: [{ name: 'create_document', args: { title: 'Tidecaller guide', summary: 'A solo Tidecaller, level by level.', sections: [{ key: 'Levels', intent: 'level by level' }] } }],
      },
      {
        tools: [
          {
            name: 'write_section',
            args: (r) => ({
              note_id: madeDoc(r),
              key: 'Levels',
              markdown: '| Level | Skill |\n|---|---|\n| 1 | Rain Bolt ([emberfall.example](https://emberfall.example/wiki/Tidecaller)) |\n| 2 | Kindle |\n\n*Level 2: my suggestion, not from a source.*',
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
    await expect(lastAnswer(main)).toContainText('Rain Bolt comes first', { timeout: 20_000 });
    await expect(thinkButton(main)).toHaveAttribute('aria-pressed', 'false');
    // Its searches and pages: chips; its citation: a pill; the Sources row.
    const chips = chipsOfLast(main);
    await expect(chips.filter({ hasText: 'Searched the web with Anthropic: "Emberfall tidecaller levels"' })).toHaveCount(1);
    await expect(chips.filter({ hasText: 'Read emberfall.example/wiki/Tidecaller' })).toHaveCount(1);
    await expect(chips.filter({ hasText: "Couldn't read forum.example/t/tidecaller-help: the page didn't open" })).toHaveCount(1);
    await expect(lastAnswer(main).locator('a.md-cite')).toHaveCount(1);
    await expect(thread(main).locator('.sources').last()).toContainText('emberfall.example');
    await expect(lastAnswer(main)).not.toContainText("I'll look this up.");
    await shot(main, '03-claude-searched-itself');
    // The document written after its search.
    await expectStore(yap, 'notes.json', (s) => s.notes.some((n) => n.title === 'Tidecaller guide' && n.content.includes('| 2 | Kindle |')));

    // The requests: the Messages API, its server tools beside Yap's, deep.
    const steps = fakeLlm.requests.slice(before).filter((r) => r.api === 'messages');
    expect(steps.length).toBe(3);
    const [one, two] = steps;
    expect(one.url).toBe('/v1/messages');
    expect(one.headers['x-api-key']).toBe(CLAUDE_KEY);
    expect(one.headers['anthropic-version']).toBe('2023-06-01');
    expect(one.serverTools).toEqual([
      { type: 'web_search_20260209', name: 'web_search', maxUses: 10 },
      { type: 'web_fetch_20260209', name: 'web_fetch', maxUses: 10 },
    ]);
    expect(one.tools).not.toContain('web_search');
    expect(one.tools).not.toContain('read_page');
    expect(one.tools).toContain('edit_document');
    // Thought through: adaptive, high effort, its summary shown.
    expect(one.think.thinking).toEqual({ type: 'adaptive', display: 'summarized' });
    expect(one.think.outputConfig).toEqual({ effort: 'high' });
    // Cached: the tools and system prompt, and the conversation.
    expect(one.systemCache).toEqual({ type: 'ephemeral' });
    expect(one.cacheControl).toEqual({ type: 'ephemeral' });
    // Its own blocks went back next step: the thinking, the search, its call.
    const sent = two.body.messages.find((m) => m.role === 'assistant').content.map((b) => b.type);
    expect(sent).toEqual(expect.arrayContaining(['thinking', 'server_tool_use', 'web_search_tool_result', 'web_fetch_tool_result', 'tool_use']));
    expect(one.system).toContain('Use web_fetch to open a page');
    await expectStore(yap, 'chats.json', (s) => s.conversations.some((c) => c.messages.some((m) => m.role === 'assistant' && m.thinking?.text?.includes('A level table first'))));

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
