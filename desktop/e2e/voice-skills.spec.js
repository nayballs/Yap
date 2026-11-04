// Skills by voice (desktop/src-tauri/src/chat/assistant/skill.rs): a Skill said to
// the Voice Assistant runs as Chat runs a typed one (chat/skills.rs):
// - "Prep me for my next meeting" (/prep's own question, in other words)
//   with no calendar: Yap asks out loud which meeting, no model called; the
//   answer said next runs /prep on it, with its passages, its citations in
//   the conversation and the answer read aloud without them;
// - "slash tickets, the supplier call" from another app: /tickets (off the
//   `/` menu, named whole) writes its document, and the voice says it's in
//   Chat; with Chat in front, "run my tickets skill…" says it's open on the
//   right, the document filling the panel;
// - a question that only mentions a Skill's words stays a question.
// Questions are staged with the debug-only `e2e_assistant_ask` (the
// shortcut's own steps, the words in the mic's place), what Yap says is kept
// for `tts_e2e_spoken` (test mode plays nothing), and the AI is the local
// fake (support/fake-llm.js). The meetings are fictional, seeded in
// notes.json.
import { test as base, expect, openView, inAnotherApp, closeToasts } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';

const NOW = Math.floor(Date.now() / 1000);
const DAY = 86_400;

const NOTES = {
  folders: ['Personal', 'Meetings'],
  notes: [
    {
      id: 1,
      title: 'Supplier call',
      noteType: 'meeting',
      folder: 'Meetings',
      source: 'meeting',
      createdTs: NOW - 3 * DAY,
      updatedTs: NOW - 3 * DAY,
      participants: ['Priya Shah'],
      enhancedContent:
        '## Recap\nThe packaging quote came in over budget.\n\n## Action plan\n### You\n- [ ] Draft the finance request (due: Wednesday)\n\n## Decisions\n- Ask finance for the extra budget, because the quote is final',
      transcript: [
        { source: 'you', text: 'Priya, is the quote final now?', ts: NOW - 3 * DAY },
        { source: 'them', text: 'Yes, it is final. We need the finance request by Wednesday.', ts: NOW - 3 * DAY + 20 },
        { source: 'you', text: 'I will draft the finance request then.', ts: NOW - 3 * DAY + 40 },
      ],
    },
    {
      id: 2,
      title: 'Offsite planning',
      noteType: 'meeting',
      folder: 'Meetings',
      source: 'meeting',
      createdTs: NOW - 10 * DAY,
      updatedTs: NOW - 10 * DAY,
      participants: ['Alice Moreno'],
      enhancedContent: '## Recap\nThe team planned the spring offsite.\n\n## Decisions\n- Hold the offsite in Lisbon, because flights are cheaper',
      transcript: [
        { source: 'you', text: 'Berlin or Lisbon for the offsite?', ts: NOW - 10 * DAY },
        { source: 'them', text: 'Lisbon, the flights are cheaper for most of us.', ts: NOW - 10 * DAY + 15 },
      ],
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
  yapOptions: [
    async ({ fakeLlm }, use) => {
      await use({
        name: 'voice-skills',
        config: {
          llmScopes: {
            noteFormatting: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-notes' },
            chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat' },
          },
          // The shortcut's answers are read aloud.
          speakReplies: true,
          meetingReminder: 'never',
          // A new install's config: no one-time migration to run.
          configVersion: 7,
        },
        data: { 'notes.json': NOTES },
      });
    },
    { scope: 'worker' },
  ],
});

/** A question to the Voice Assistant, as its shortcut asks it. */
const ask = (yap, text) => yap.invoke('e2e_assistant_ask', { text, listenMs: 0 });
/** Everything Yap has said, in order. */
const said = async (yap) => (await yap.invoke('tts_e2e_spoken')).map((s) => s.text);
const thread = (main) => main.locator('.msgs');
const panel = (main) => main.getByTestId('chat-doc');
/** Every message of a request, its system prompt too, as one text. */
const sentText = (r) => [r?.system ?? '', ...(r?.messages ?? []).map((m) => (typeof m.content === 'string' ? m.content : JSON.stringify(m.content ?? '')))].join('\n');
const chats = (yap) => yap.readJson('chats.json')?.conversations ?? [];
/** The conversation a question went to. */
const conversationOf = (yap, question) => chats(yap).find((c) => c.messages.some((m) => m.role === 'user' && m.text === question));

/** The document `create_document` made earlier in this turn (its result in the messages). */
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

/** A /tickets run's document, as the fake model writes it. */
function ticketsScript(line) {
  return [
    { tools: [{ name: 'create_document', args: { title: 'Tickets: Supplier call', summary: 'The work agreed on the supplier call.', sections: [{ key: 'Tickets', intent: 'a ticket each' }] } }] },
    {
      tools: [
        {
          name: 'write_section',
          args: (r) => ({ note_id: madeDoc(r), key: 'Tickets', markdown: '### Draft the finance request\n**What:** Ask finance for the extra budget [1].\n**Owner:** You' }),
        },
      ],
    },
    { content: line },
  ];
}

async function freshChat(main) {
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).first().click();
}

test.beforeEach(async ({ yap }) => {
  await yap.invoke('assistant_stop');
  await yap.invoke('tts_stop');
  await yap.invoke('tts_e2e_spoken', { clear: true });
  await yap.invoke('e2e_focus', { focused: true });
});

test.afterEach(async ({ yap, main, fakeLlm }) => {
  fakeLlm.reply('other', undefined);
  await yap.invoke('tts_stop');
  await yap.invoke('e2e_focus', { focused: true });
  await closeToasts(main);
});

test('a spoken /prep with nothing coming up: Yap asks out loud which meeting, and the answer said runs it', async ({ yap, main, fakeLlm, shot }) => {
  await freshChat(main);
  const before = fakeLlm.requests.length;

  // /prep's own question, in other words: no calendar, so Yap asks which
  // meeting — out loud, no model called, in the chat as Yap's answer.
  await ask(yap, 'Prep me for my next meeting.');
  const question = "I can't see your calendar. Which meeting should I prepare you for?";
  await expect(thread(main).locator('.cbubble.assistant').last()).toContainText(question);
  await expect(thread(main).locator('.toolchip').filter({ hasText: 'Ran /prep' })).toBeAttached();
  await expect.poll(() => said(yap), { timeout: 15_000 }).toContain(question);
  expect(fakeLlm.requests.slice(before).filter((r) => r.kind === 'other')).toHaveLength(0);
  await yap.invoke('tts_stop');

  // The answer, said: /prep runs on it, with its passages.
  fakeLlm.scriptChat([{ content: '**Supplier call** with Priya Shah.\n- **Last time:** the packaging quote came in over budget [1].\n- **To raise:** when finance will answer.' }]);
  await ask(yap, 'The supplier call.');
  const answer = thread(main).locator('.cbubble.assistant').last();
  await expect(answer).toContainText('the packaging quote came in over budget');
  await expect(answer.locator('a.md-passage').first()).toBeVisible();
  await expect(thread(main).locator('.toolchip').filter({ hasText: /Ran \/prep · \d+ passages?/ })).toBeAttached();
  const req = fakeLlm.requests.slice(before).filter((r) => r.kind === 'other').at(-1);
  const sent = sentText(req);
  expect(sent).toContain('Skill: Prepare me (/prep)');
  expect(sent).toContain('No meeting matching "The supplier call" is coming up');
  expect(sent).toContain('The packaging quote came in over budget');
  // Chat's run, not a spoken one's: no "one or two short sentences" asked for.
  expect(sent).not.toContain('answer in one or two short sentences');
  // Read aloud, its citation left out.
  const all = async () => (await said(yap)).join(' ');
  await expect.poll(all, { timeout: 15_000 }).toContain('the packaging quote came in over budget');
  await expect.poll(all, { timeout: 15_000 }).toContain('when finance will answer');
  expect(await all()).not.toContain('[1]');
  // One conversation: the request, Yap's question, the answer, the run.
  const conv = conversationOf(yap, 'Prep me for my next meeting.');
  expect(conv.messages.map((m) => [m.role, !!m.voice])).toEqual([
    ['user', true],
    ['assistant', false],
    ['user', true],
    ['assistant', false],
  ]);
  expect(conv.messages[2].text).toBe('The supplier call.');
  expect(conv.messages[3].sources?.length).toBeGreaterThan(0);
  await shot(main, '01-spoken-prep');
});

test('a spoken /tickets from another app writes its document, and the voice says it’s in Chat', async ({ yap, main, fakeLlm }) => {
  await freshChat(main);
  await inAnotherApp(yap);
  const before = fakeLlm.requests.length;
  fakeLlm.scriptChat(ticketsScript("It's in Chat."));
  // Off the `/` menu, named whole: it runs.
  await ask(yap, 'Slash tickets, the supplier call.');
  await expect.poll(async () => (await said(yap)).join(' '), { timeout: 20_000 }).toBe("It's in Chat.");
  const asked = fakeLlm.requests.slice(before).find((r) => r.tools.includes('create_document'));
  const sent = sentText(asked);
  expect(sent).toContain('Skill: Draft tickets (/tickets)');
  expect(sent).toContain('Write this as a document now, not in the chat: create_document');
  expect(sent).toContain('Then answer in one short sentence: "It\'s in Chat."');
  expect(sent).toContain('The passages are from the meeting "Supplier call"');
  // The document is kept with the answer, in Notes under Chat documents.
  await expect
    .poll(() => conversationOf(yap, 'Slash tickets, the supplier call.')?.messages.at(-1)?.documents?.length ?? 0, { timeout: 10_000 })
    .toBe(1);
  const notes = yap.readJson('notes.json').notes;
  const doc = notes.find((n) => n.title === 'Tickets: Supplier call');
  expect(doc?.folder).toBe('Chat documents');
  expect(doc.content).toContain('Ask finance for the extra budget');
});

test('with Chat in front, a Skill named with "skill" writes its document beside the chat: it’s open on the right', async ({ yap, main, fakeLlm }) => {
  await freshChat(main);
  const before = fakeLlm.requests.length;
  fakeLlm.scriptChat(ticketsScript("It's open on the right."));
  await ask(yap, 'Run my tickets skill for the supplier call.');
  await expect(panel(main)).toBeVisible({ timeout: 20_000 });
  await expect(panel(main).locator('.doctitle')).toHaveText('Tickets: Supplier call');
  await expect(panel(main).locator('.docbody')).toContainText('Ask finance for the extra budget', { timeout: 20_000 });
  await expect(thread(main).locator('.cbubble.assistant').last()).toContainText("It's open on the right.");
  await expect.poll(async () => (await said(yap)).join(' '), { timeout: 20_000 }).toBe("It's open on the right.");
  const asked = fakeLlm.requests.slice(before).find((r) => r.tools.includes('create_document'));
  expect(sentText(asked)).toContain('Then answer in one short sentence: "It\'s open on the right."');
});

test('a question that only mentions a Skill’s words stays a question', async ({ yap, main, fakeLlm }) => {
  await freshChat(main);
  for (const [q, a] of [
    ['What are the risks of a late launch?', 'A late launch can miss the season.'],
    ['How do I write a stand-up update?', 'Say what you did, what is next and what blocks you.'],
  ]) {
    const before = fakeLlm.requests.length;
    fakeLlm.scriptChat([{ content: a }]);
    await ask(yap, q);
    await expect(thread(main).locator('.cbubble.assistant').last()).toContainText(a);
    const req = fakeLlm.requests.slice(before).filter((r) => r.kind === 'other').at(-1);
    expect(sentText(req)).not.toContain('The user ran a Skill');
    await expect(thread(main).locator('.toolchip').filter({ hasText: 'Ran /' })).toHaveCount(0);
    await yap.invoke('tts_stop');
  }
});
