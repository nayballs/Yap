// Streamed answers (src-tauri/src/stream.rs, src/lib/streaming.js): Chat and
// the notepad's Ask sheet show an answer as it's written.
// - The answer grows in place, markdown rendered as it comes (a half-written
//   **bold** or list closed off), the request asked for the stream and its
//   usage.
// - A tool call shows as its chip while it runs; the line a model writes
//   before calling a tool ("I'll search your notes for that.") never shows;
//   the call's arguments, streamed in fragments, come back whole.
// - ■ stops it: the stream closes (the fake sees Yap hang up), what came is
//   kept, checked like any answer, marked Stopped — in chats.json too.
// - A provider error mid-stream: asked again whole, the streamed text
//   replaced.
// - The checked answer replaces what streamed: a search the model claimed
//   but didn't make, and its "[1]" with nothing behind it, never stay.
// - The notepad's Ask sheet: the same, and its ■.
//
// The model is a local fake (support/fake-llm.js) that streams server-sent
// events when asked, paced per step (`stream: { gapMs, hangAfter,
// failAfter }`). `closePartial` and the event state are checked on their own
// first (no app).
import { test as base, expect, openView, expectStore } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { closePartial, liveAnswer, applyEvent } from '../src/lib/streaming.js';

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
        name: 'chat-streaming',
        config: {
          llmScopes: {
            chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat' },
            noteFormatting: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-notes' },
          },
          meetingReminder: 'never',
        },
      });
    },
    { scope: 'worker' },
  ],
});

const box = (main) => main.getByRole('textbox', { name: 'Message' });
const thread = (main) => main.locator('.msgs');
const streamingBubble = (main) => main.getByTestId('chat-streaming');

async function freshChat(main) {
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
}

async function send(main, text) {
  await box(main).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
}

/** Every text the streaming bubble and the "doing" chip showed, from now on. */
async function watchStream(main) {
  await main.evaluate(() => {
    window.__streamed = [];
    window.__doing = [];
    const root = document.querySelector('.thread') ?? document.body;
    new MutationObserver(() => {
      const live = document.querySelector('[data-testid="chat-streaming"] .cbody');
      if (live) window.__streamed.push(live.textContent);
      const doing = document.querySelector('[data-testid="chat-doing"]');
      if (doing) window.__doing.push(doing.textContent.trim());
    }).observe(root, { subtree: true, childList: true, characterData: true });
  });
  return {
    texts: () => main.evaluate(() => window.__streamed),
    doing: () => main.evaluate(() => window.__doing),
  };
}

// ---- on their own (no app)

test('half-written markdown is closed off so it renders as it will', () => {
  expect(closePartial('The budget is **fifteen')).toBe('The budget is **fifteen**');
  expect(closePartial('Use `npm ru')).toBe('Use `npm ru`');
  expect(closePartial('```js\nconst x = 1')).toBe('```js\nconst x = 1\n```');
  // A citation or link still being typed waits until it's whole.
  expect(closePartial('On the beach [1')).toBe('On the beach ');
  expect(closePartial('See [the guide](https://guides.exa')).toBe('See ');
  expect(closePartial('On the beach [1] and')).toBe('On the beach [1] and');
  // Only the last line is still being written.
  expect(closePartial('**Done** here.\n- one **tw')).toBe('**Done** here.\n- one **tw**');
  expect(closePartial('')).toBe('');
});

test("a stream's events make the bubble's state", () => {
  let s = liveAnswer();
  expect(s.started).toBe(false);
  s = applyEvent(s, { kind: 'waiting', waiting: true });
  expect([s.waiting, s.started]).toEqual([true, false]);
  s = applyEvent(s, { kind: 'tool', name: 'search_notes', doing: 'Searching your notes…' });
  expect([s.doing, s.started]).toEqual(['Searching your notes…', true]);
  s = applyEvent(s, { kind: 'chips', chips: ['Found 1 note for "budget"'], sources: [], web: false });
  expect([s.doing, s.chips]).toEqual([null, ['Found 1 note for "budget"']]);
  s = applyEvent(s, { kind: 'text', text: 'The budget' });
  expect([s.text, s.waiting]).toEqual(['The budget', false]);
  s = applyEvent(s, { kind: 'text', text: '' });
  expect(s.text).toBe('');
});

// ---- Chat

test('an answer grows in place as it is written, then stays put', async ({ main, fakeLlm, shot }) => {
  await freshChat(main);
  const answer =
    'Here is the plan for **Friday**:\n\n- Book the room for ten people\n- Send the agenda by Thursday\n- Order lunch from the usual place\n\nSam has the slides, and the demo runs last.';
  fakeLlm.scriptChat([{ content: answer, stream: { gapMs: 140 } }]);
  const seen = await watchStream(main);
  await send(main, "What's the plan for Friday?");

  // Mid-stream: part of it, markdown already rendered (the bold, the list).
  const live = streamingBubble(main);
  await expect(live).toBeVisible();
  await expect(live).toContainText('Book the room');
  await expect(live.locator('strong')).toHaveText('Friday');
  await expect(live).toHaveAttribute('aria-busy', 'true');
  await expect(live).not.toContainText('the demo runs last');
  await expect(main.getByRole('button', { name: 'Stop the answer' })).toBeVisible();
  await shot(main, '01-answer-mid-stream');

  // Done: the same bubble, the whole answer, the model under it.
  await expect(thread(main)).toContainText('the demo runs last');
  await expect(live).toHaveCount(0);
  const done = thread(main).locator('.cbubble.assistant').last();
  await expect(done.locator('li')).toHaveCount(3);
  await expect(main.getByRole('button', { name: 'Stop the answer' })).toHaveCount(0);
  await expect(thread(main).locator('.byline').last()).toBeVisible();
  await shot(main, '02-answer-done');
  // It grew: many partial states before the end.
  const texts = await seen.texts();
  expect(new Set(texts).size).toBeGreaterThan(4);
  expect(texts.some((t) => t.includes('Book the room') && !t.includes('demo runs last'))).toBe(true);

  // Yap asked for the stream and its usage.
  const req = fakeLlm.requests.filter((r) => r.kind === 'other').at(-1);
  expect(req.stream).toBe(true);
  expect(req.streamOptions).toEqual({ include_usage: true });
});

test('a tool call shows as its chip; the line before it never shows; its arguments come back whole', async ({
  yap,
  main,
  fakeLlm,
  shot,
}) => {
  await yap.invoke('note_create', { title: 'Kitchen budget', content: 'Agreed the kitchen budget at £15,000. Sam checks the quotes.' });
  await freshChat(main);
  fakeLlm.scriptChat([
    {
      preamble: "I'll search your notes for that.",
      tools: [{ name: 'search_notes', args: { query: 'kitchen budget' } }],
      stream: { gapMs: 90 },
    },
    { content: 'The kitchen budget is **£15,000**; Sam checks the quotes.', stream: { gapMs: 90 } },
  ]);
  const seen = await watchStream(main);
  await send(main, "What's the kitchen budget?");
  await expect(thread(main)).toContainText('The kitchen budget is £15,000; Sam checks the quotes.');
  await expect(thread(main).locator('.toolchip')).toHaveText(['Found 1 note for "kitchen budget"']);
  await shot(main, '03-tool-chip-then-answer');

  // The preamble never showed; the call's chip did while it ran.
  const texts = await seen.texts();
  expect(texts.some((t) => t.includes('search your notes'))).toBe(false);
  // The arguments, streamed in fragments, reached the tool whole.
  const second = fakeLlm.requests.filter((r) => r.tools.includes('search_notes')).at(-1);
  const call = second.messages.find((m) => m.tool_calls)?.tool_calls[0];
  expect(call.function.name).toBe('search_notes');
  expect(JSON.parse(call.function.arguments)).toEqual({ query: 'kitchen budget' });
  const result = JSON.parse(second.messages.find((m) => m.role === 'tool').content);
  expect(result.success).toBe(true);
});

test('■ stops the answer: the stream closes, what came stays, checked and marked Stopped', async ({
  yap,
  main,
  fakeLlm,
  shot,
}) => {
  await freshChat(main);
  fakeLlm.scriptChat([
    {
      content: 'Part one of the answer is here. Part two goes on and on and never seems to end at all.',
      stream: { gapMs: 60, hangAfter: 7 },
    },
  ]);
  await send(main, 'Tell me a long story');
  const live = streamingBubble(main);
  await expect(live).toContainText('Part one of the answer is here.');
  await shot(main, '04-before-stop');
  await main.getByRole('button', { name: 'Stop the answer' }).click();

  // What came stays, marked Stopped; the composer is free again.
  await expect(live).toHaveCount(0);
  const done = thread(main).locator('.cbubble.assistant').last();
  await expect(done).toContainText('Part one of the answer is here.');
  await expect(done).not.toContainText('never seems to end');
  await expect(thread(main).locator('.byline.stopped')).toHaveText('· Stopped');
  await shot(main, '05-stopped');
  // The fake saw Yap hang up mid-stream.
  await expect.poll(() => fakeLlm.requests.at(-1).aborted).toBe(true);
  // Saved as stopped, with what showed.
  await expectStore(yap, 'chats.json', (s) => {
    const c = s.conversations.find((x) => x.title === 'Tell me a long story');
    const last = c?.messages.at(-1);
    return last?.role === 'assistant' && last.stopped === true && last.text.startsWith('Part one of the answer is here.');
  });

  // Stopped before any of it came: the question stays, "Stopped" under it.
  fakeLlm.scriptChat([{ content: 'Never shown.', stream: { hangAfter: 1 } }]);
  await send(main, 'And another?');
  await expect(main.locator('.thinking')).toBeVisible();
  await main.getByRole('button', { name: 'Stop the answer' }).click();
  await expect(thread(main).locator('.stoppednote')).toHaveText('Stopped');
  await expect(thread(main).locator('.cbubble.user').last()).toContainText('And another?');
  await expect(thread(main)).not.toContainText('Never shown.');
});

test('a provider error mid-stream: asked again whole, the streamed text replaced', async ({ main, fakeLlm }) => {
  await freshChat(main);
  fakeLlm.scriptChat([
    { content: 'First try here. It breaks half way through the answer, sadly.', stream: { gapMs: 60, failAfter: 6 } },
    { content: 'The whole answer, asked again.' },
  ]);
  const seen = await watchStream(main);
  await send(main, 'Will it break?');
  await expect(thread(main).locator('.cbubble.assistant').last()).toHaveText('The whole answer, asked again.');
  await expect(thread(main)).not.toContainText('breaks half way');
  // What streamed before the error showed, then gave way.
  expect((await seen.texts()).some((t) => t.startsWith('First try here.'))).toBe(true);
  const asked = fakeLlm.requests.filter((r) => r.kind === 'other').slice(-2);
  expect(asked.map((r) => r.stream)).toEqual([true, false]);
});

test('the checked answer replaces what streamed: no claimed search, no citation with nothing behind it', async ({
  main,
  fakeLlm,
  shot,
}) => {
  await freshChat(main);
  fakeLlm.scriptChat([
    {
      content: 'I searched the web: the newest version is 3.13 [1]. It came out in October with a faster interpreter.',
      stream: { gapMs: 70 },
    },
  ]);
  const seen = await watchStream(main);
  await send(main, 'What is the newest Python?');
  const last = thread(main).locator('.cbubble.assistant').last();
  await expect(last).toContainText("Yap didn't search the web for this answer");
  // The "[1]" streamed for a moment, and is gone now.
  expect((await seen.texts()).some((t) => t.includes('[1]'))).toBe(true);
  await expect(last).not.toContainText('[1]');
  await expect(last.locator('.md-cite')).toHaveCount(0);
  await shot(main, '06-checked-answer-replaced');
});

// A model on this PC (the fake is on 127.0.0.1) never slows a dictation:
// a dictation starting while the answer streams drops the stream, the page
// says it's waiting, and the step is asked again once the dictation is done.
// The dictation is staged with `e2e_assistant_ask` and nothing to ask: the
// shortcut's recording, as the pipeline counts dictations, for 1.5 s.
test('a dictation starting mid-stream: the local stream steps aside, then answers', async ({ yap, main, fakeLlm, shot }) => {
  await freshChat(main);
  fakeLlm.scriptChat([
    { content: 'First words of a long answer. It keeps on coming, slowly, word after word, for quite a while yet.', stream: { gapMs: 250 } },
    { content: 'The answer, asked again once the dictation was done.' },
  ]);
  await send(main, 'Take your time');
  await expect(streamingBubble(main)).toContainText('First words of a long answer.');
  const asked = fakeLlm.requests.length;
  await yap.invoke('e2e_assistant_ask', { text: '', listenMs: 1500 }).catch(() => {});
  await expect(streamingBubble(main)).toContainText('Waiting for your dictation to finish…');
  await shot(main, '09-waiting-for-a-dictation');
  // The first stream was dropped; the answer came from the step asked again.
  await expect(thread(main).locator('.cbubble.assistant').last()).toHaveText(
    'The answer, asked again once the dictation was done.',
    { timeout: 15_000 }
  );
  expect(fakeLlm.requests[asked - 1].aborted).toBe(true);
  expect(fakeLlm.requests.length).toBe(asked + 1);
});

test('a server that ignores "stream" still answers, whole', async ({ main, fakeLlm }) => {
  fakeLlm.setStreaming(false);
  try {
    await freshChat(main);
    fakeLlm.scriptChat([{ content: 'A whole answer, no stream.' }]);
    await send(main, 'Plain please');
    await expect(thread(main).locator('.cbubble.assistant').last()).toHaveText('A whole answer, no stream.');
  } finally {
    fakeLlm.setStreaming(true);
  }
});

// ---- the notepad's Ask sheet

test("the notepad's Ask sheet streams its answer, and ■ stops it", async ({ yap, fakeLlm, shot }) => {
  const note = await yap.invoke('note_create', { title: 'Budget sync', folder: 'Meetings' });
  await yap.invoke('meeting_start', { noteId: note.id });
  await expect.poll(() => yap.invoke('plugin:window|is_visible', { label: 'notepad' })).toBe(true);
  const pad = yap.notepad;
  const ts = Math.floor(Date.now() / 1000);
  await yap.invoke('e2e_meeting_feed', {
    segments: [
      { source: 'you', text: 'Alice, can you own the budget and send it by Friday?', ts },
      { source: 'them', text: 'Yes, I will send the revised budget by Friday.', ts: ts + 6 },
    ],
  });
  try {
    fakeLlm.reply('meetingAsk', () => 'Alice owns the budget: she said she would send it by **Friday**, and copy you on it.');
    fakeLlm.streamPace('meetingAsk', { gapMs: 150 });
    const input = pad.getByRole('textbox', { name: 'Ask anything' });
    await input.fill('Who owns the budget?');
    await input.press('Enter');
    const sheet = pad.getByRole('dialog', { name: 'Ask about this meeting' });
    const live = sheet.getByTestId('ask-streaming');
    await expect(live).toContainText('Alice owns the budget');
    await expect(live).not.toContainText('copy you on it');
    await expect(pad.getByRole('button', { name: 'Stop the answer' })).toBeVisible();
    await shot(pad, '07-ask-sheet-mid-stream');
    await expect(sheet.locator('.ma').last()).toContainText('copy you on it');
    await expect(live).toHaveCount(0);
    await expect(sheet.locator('.ma').last().locator('strong')).toHaveText('Friday');
    await shot(pad, '08-ask-sheet-done');

    // ■: what came stays, marked Stopped.
    fakeLlm.streamPace('meetingAsk', { gapMs: 80, hangAfter: 6 });
    await input.fill('And the venue?');
    await input.press('Enter');
    await expect(sheet.getByTestId('ask-streaming')).toContainText('Alice owns');
    await pad.getByRole('button', { name: 'Stop the answer' }).click();
    await expect(sheet.getByTestId('ask-streaming')).toHaveCount(0);
    await expect(sheet.locator('.ma').last().locator('.mamodel').last()).toHaveText('· Stopped');
    await expect.poll(() => fakeLlm.requests.filter((r) => r.kind === 'meetingAsk').at(-1).aborted).toBe(true);
  } finally {
    fakeLlm.streamPace('meetingAsk', null);
    await yap.invoke('meeting_pause').catch(() => {});
    await expect.poll(() => yap.invoke('meeting_state').then((s) => s.recording), { timeout: 20_000 }).toBe(false);
    for (let i = 0; i < 3; i += 1) await pad.keyboard.press('Escape');
    await pad.getByRole('button', { name: 'Close notepad' }).click();
  }
});
