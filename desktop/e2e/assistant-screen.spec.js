// The Voice Assistant sees the window in front (desktop/src-tauri/src/chat/assistant/screen.rs):
// a spoken question whose words point at the screen takes a screenshot of the
// window that was in front when the shortcut was pressed, at once, and the
// model answers from it.
// - On a model on this PC: the window's picture goes with the question (cut
//   from the fake monitors exactly, as a snip is), the model is told what it
//   is, the question shows in Chat with it, and the network receipts say
//   "Voice Assistant + the window in front".
// - No guessing (8 Nov 2026): a lone "this" or "that" takes nothing and asks
//   no model first — two spoken follow-ups about a convention once sent the
//   game in front. The answering model is offered look_at_screen instead
//   (chat/see_screen.rs, the window in front its look): when it calls it, the
//   window goes to it and shows on the question in Chat ("Looking at the
//   window…" on the capsule while the words' own look takes it).
// - A model that can't read images: the window's text, read on this PC (a
//   stand-in for Windows' OCR), after the question, named in the sources.
// - A cloud model: not without asking — the model is told it can't see the
//   screen, the alert asks once, **Allow** lets it.
// - Settings → Voice Assistant: the two switches.
//
// A test run never captures the real screen: `e2e_snip {start: false}` sets
// fake monitors and the window in front (the first fake window), as the snip
// specs do. The questions are staged with `e2e_assistant_ask` (no
// microphone), the model is a local fake (support/fake-llm.js).
import fs from 'node:fs';
import { test as base, expect, openSettings, closeSettings, expectStore, inAnotherApp, setConfig } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { readDataUrl, worstError } from './support/png.js';

const TEST_KEY = 'e2e-fake-key';

/** A Chat scope on the fake (config.rs `LlmScope`). */
function chatOn(fakeLlm, provider, model) {
  return { enabled: true, provider, baseUrl: fakeLlm.base, model, apiKey: TEST_KEY, apiKeys: {}, prompt: '', disableThinking: false };
}

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
        name: 'assistant-screen',
        config: {
          ppApiKeys: { groq: TEST_KEY },
          // A server on this PC that reads images (custom, at 127.0.0.1).
          llmScopes: { chat: chatOn(fakeLlm, 'custom', 'qwen2.5-vl-7b') },
          meetingReminder: 'never',
        },
      });
    },
    { scope: 'worker' },
  ],
});

const MONITORS = [{ x: 0, y: 0, width: 1280, height: 720, scale: 1, hdr: true, whiteNits: 240 }];
// The window in front: the light grey block on the fake monitor.
const WINDOW = { x: 40, y: 40, w: 200, h: 100 };
const GREY = [0xc0, 0xc0, 0xc0];

// Asked from another app (the window in front is that app's): with Yap's
// own window in front, Chat's model looks itself (chat-pages.spec.js).
const ask = async (yap, text) => {
  await yap.invoke('e2e_focus', { focused: false });
  return yap.invoke('e2e_assistant_ask', { text, listenMs: 50 });
};
/** The fake monitors and the window in front, for the next look (never the real screen). */
const windowInFront = (yap) => yap.invoke('e2e_snip', { monitors: MONITORS, windows: [WINDOW], start: false });
const log = (yap) => fs.readFileSync(yap.logPath, 'utf8');
const chats = (yap) => yap.readJson('chats.json').conversations;
const questionNamed = (yap, text) => chats(yap).flatMap((c) => c.messages).find((m) => m.role === 'user' && m.text.startsWith(text));
/** The question's context as the model got it (its own message). */
const userOf = (r) => r.user ?? '';

test.beforeEach(async ({ yap }) => {
  await yap.invoke('assistant_stop');
  await yap.invoke('tts_stop');
});

test.afterEach(async ({ yap }) => {
  await yap.invoke('e2e_focus', { focused: true });
});

test('a question about the screen, on a model on this PC: the window in front goes with it', async ({ yap, main, fakeLlm }) => {
  await yap.invoke('connections_clear');
  await windowInFront(yap);
  const before = fakeLlm.requests.length;
  fakeLlm.scriptChat([{ content: 'It says access to the file is denied.' }]);
  await ask(yap, "What's this error?");
  await expect.poll(() => fakeLlm.requests.slice(before).filter((r) => r.images?.length).length, { timeout: 10_000 }).toBe(1);
  const asked = fakeLlm.requests.slice(before).find((r) => r.images?.length);
  // The window, cut exactly from the fake monitor (HDR converted back).
  const png = readDataUrl(asked.images[0]);
  expect([png.width, png.height]).toEqual([WINDOW.w, WINDOW.h]);
  expect(worstError(png, GREY)).toBeLessThanOrEqual(1);
  // The model's told what the picture is; the words said were enough (no
  // yes-or-no request).
  expect(userOf(asked)).toContain("What's this error?");
  expect(userOf(asked)).toContain('a screenshot Yap took of the window the user had in front of them');
  expect(fakeLlm.requests.slice(before).some((r) => r.kind === 'screenQuestion')).toBe(false);
  // Kept with the question, as a snip is.
  await expectStore(yap, 'chats.json', (s) =>
    s.conversations.some((c) => c.messages.some((m) => m.text === "What's this error?" && m.voice && m.images?.[0]?.source === 'window'))
  );
  await expectStore(yap, 'chats.json', (s) =>
    s.conversations.some((c) => c.messages.some((m) => m.role === 'assistant' && m.text.includes('access to the file is denied')))
  );
  // The receipts name it.
  await expect
    .poll(async () => (await yap.invoke('connections_summary')).groups.map((g) => g.label), { timeout: 10_000 })
    .toContain('Voice Assistant + the window in front · Custom endpoint');
  // In Chat (asked from another app: its conversation opened from the list): the question with its thumbnail.
  await main.getByRole('navigation', { name: 'Main' }).getByRole('button', { name: 'Chat', exact: true }).click();
  await main.locator('.conv').first().click(); // the latest conversation (a follow-up joins the last one)
  const bubble = main.locator('.cbubble.user', { hasText: "What's this error?" }).last();
  await expect(bubble.locator('.shots img')).toBeVisible();
});

/** The chat requests (they offer tools) from `from` on. */
const chatCalls = (fakeLlm, from) => fakeLlm.requests.slice(from).filter((r) => r.tools?.length);
/** A request's latest tool result, parsed. */
const toolResult = (request) => JSON.parse(request.messages.filter((m) => m.role === 'tool').at(-1).content);
/** The look's own line in the system prompt, asked from another app (chat/see_screen.rs). */
const WINDOW_LINE = 'look_at_screen sees the window the user had in front of them when they asked out loud';

// The two spoken follow-ups of 8 Nov 2026 (each sent a screenshot of the game in front).
const FOLLOW_UPS = [
  "The one that's on the 5th to the 8th of February, where is it and can you bring up the website in the browser for it?",
  'How many people usually attend that specific one? Can you look?',
];

test('no guessing: spoken follow-ups with "that" send no screenshot and ask no model first; the answering model may look itself', async ({ yap, fakeLlm }) => {
  await inAnotherApp(yap);
  try {
    // A question, then the two follow-ups about what was just said, in the same conversation.
    await windowInFront(yap);
    let before = fakeLlm.requests.length;
    fakeLlm.scriptChat([{ content: 'Two this winter: Kellbrook Comic Con, 5 to 8 February, and the Northfold Expo in March.' }]);
    await ask(yap, 'Which comic conventions are on near Kellbrook this winter?');
    await expect.poll(() => chatCalls(fakeLlm, before).length, { timeout: 10_000 }).toBeGreaterThan(0);
    await yap.invoke('assistant_stop');
    const answers = ["It's at the Kellbrook Exhibition Centre; its website is kellbrookcomiccon.example.com.", 'Usually about 12,000 people over the four days.'];
    for (const [i, q] of FOLLOW_UPS.entries()) {
      before = fakeLlm.requests.length;
      fakeLlm.scriptChat([{ content: answers[i] }]);
      const started = Date.now();
      await ask(yap, q);
      await expect.poll(() => chatCalls(fakeLlm, before).some((r) => userOf(r).includes(q)), { timeout: 10_000 }).toBe(true);
      const asked = fakeLlm.requests.slice(before);
      // Nothing taken, no yes/no request first…
      expect(asked.some((r) => r.images?.length), q).toBe(false);
      expect(asked.some((r) => r.kind === 'screenQuestion'), q).toBe(false);
      expect(userOf(chatCalls(fakeLlm, before)[0])).not.toContain('screenshot');
      // …the model has the look itself, the window in front its look, with when to use it.
      const call = chatCalls(fakeLlm, before)[0];
      expect(call.tools).toContain('look_at_screen');
      expect(call.tools).not.toContain('browser_navigate');
      expect(call.system).toContain(WINDOW_LINE);
      expect(call.system).toContain('never for "it" or "that one" from the conversation, and never to look something up');
      expect(Date.now() - started).toBeLessThan(8_000);
      await yap.invoke('assistant_stop');
    }
    // Kept with no screenshot, in one conversation.
    await expectStore(yap, 'chats.json', (s) =>
      s.conversations.some((c) => FOLLOW_UPS.every((q) => c.messages.some((m) => m.role === 'user' && m.text === q && m.voice && !(m.images?.length))))
    );
    expect(log(yap)).not.toContain('asking whether the question is about the screen');

    // Words that point at the screen still take the window at once: with the
    // question itself, before the model says a word (no look to offer then).
    await windowInFront(yap);
    before = fakeLlm.requests.length;
    fakeLlm.scriptChat([{ content: 'It says the file is read-only.' }]);
    await ask(yap, "What's this error on my screen?");
    await expect.poll(() => chatCalls(fakeLlm, before).length, { timeout: 10_000 }).toBeGreaterThan(0);
    const first = chatCalls(fakeLlm, before)[0];
    expect(first.images).toHaveLength(1);
    expect(userOf(first)).toContain('a screenshot Yap took of the window the user had in front of them');
    expect(first.tools).not.toContain('look_at_screen');
    await yap.invoke('assistant_stop');

    // Nothing about the screen: not taken either.
    before = fakeLlm.requests.length;
    fakeLlm.scriptChat([{ content: 'Six to seven minutes.' }]);
    await ask(yap, 'How long do I boil an egg?');
    await expect.poll(() => fakeLlm.requests.slice(before).some((r) => userOf(r).includes('How long do I boil an egg?')), { timeout: 10_000 }).toBe(true);
    expect(fakeLlm.requests.slice(before).some((r) => r.kind === 'screenQuestion' || r.images?.length)).toBe(false);
    await yap.invoke('assistant_stop');

    // An ask for something to make: its "this" is the thing to make, never the screen.
    await windowInFront(yap);
    before = fakeLlm.requests.length;
    fakeLlm.scriptChat([{ content: "It's in Chat." }]);
    const make = 'Can you build me this level by level, a solo mage for Emberfall, in a document?';
    await ask(yap, make);
    await expect.poll(() => fakeLlm.requests.slice(before).some((r) => userOf(r).includes('solo mage for Emberfall')), { timeout: 10_000 }).toBe(true);
    expect(fakeLlm.requests.slice(before).some((r) => r.kind === 'screenQuestion' || r.images?.length)).toBe(false);
  } finally {
    await yap.invoke('assistant_stop');
  }
});

test('the answering model looks at the window in front when it needs to: the window goes to it, its frame on the question in Chat', async ({ yap, main, fakeLlm, shot }) => {
  await inAnotherApp(yap);
  try {
    await windowInFront(yap);
    const before = fakeLlm.requests.length;
    fakeLlm.scriptChat([{ tools: [{ name: 'look_at_screen', args: {} }] }, { content: 'Yes: the three amounts add up to the total shown.' }]);
    await ask(yap, 'Is this right?');
    await expect.poll(() => chatCalls(fakeLlm, before).length, { timeout: 15_000 }).toBeGreaterThanOrEqual(2);
    const calls = chatCalls(fakeLlm, before);
    // Asked with no picture: the model chose to look.
    expect(calls[0].images ?? []).toHaveLength(0);
    expect(calls[0].tools).toContain('look_at_screen');
    expect(fakeLlm.requests.slice(before).some((r) => r.kind === 'screenQuestion')).toBe(false);
    // The window in front, exactly as the clear words' look cuts it — not every monitor.
    expect(toolResult(calls[1])).toMatchObject({ success: true, data: { window: true }, displayText: 'Looked at the window in front' });
    expect(calls[1].images).toHaveLength(1);
    const png = readDataUrl(calls[1].images[0]);
    expect([png.width, png.height]).toEqual([WINDOW.w, WINDOW.h]);
    expect(worstError(png, GREY)).toBeLessThanOrEqual(1);
    const told = calls[1].messages.at(-1).content.filter((p) => p.type === 'text').map((p) => p.text).join(' ');
    expect(told).toContain('The window the user had in front of them when they asked');
    // Kept with the question, as the window's own look is.
    await expectStore(yap, 'chats.json', (s) =>
      s.conversations.some((c) => c.messages.some((m) => m.role === 'user' && m.text === 'Is this right?' && m.voice && m.images?.[0]?.source === 'window'))
    );
    await expectStore(yap, 'chats.json', (s) => s.conversations.some((c) => c.messages.some((m) => m.role === 'assistant' && m.text.includes('add up to the total'))));
    // In Chat: the question with the window in its screenshot frame, and the look's chip.
    await main.getByRole('navigation', { name: 'Main' }).getByRole('button', { name: 'Chat', exact: true }).click();
    await main.locator('.conv').first().click();
    const bubble = main.locator('.cbubble.user', { hasText: 'Is this right?' }).last();
    await expect(bubble.locator('.shots img')).toBeVisible();
    await expect(main.getByRole('group', { name: 'Screenshots' }).last().getByRole('button', { name: /^Screenshot,/ })).toHaveCount(1);
    // (The look's chip, folded under the answer with its steps.)
    await expectStore(yap, 'chats.json', (s) => s.conversations.some((c) => c.messages.some((m) => m.role === 'assistant' && m.tools?.includes('Looked at the window in front'))));
    await shot(main, '01-the-model-looked-at-the-window-in-front');
  } finally {
    await yap.invoke('assistant_stop');
  }
});

test("a model that can't read images gets the window's text, named in the answer's sources", async ({ yap, main, fakeLlm, shot }) => {
  await setConfig(yap, main, { llmScopes: { chat: chatOn(fakeLlm, 'custom', 'gpt-oss-20b') } });
  try {
    await windowInFront(yap);
    await yap.invoke('e2e_snip_ocr', { text: 'Error 0x80070005: Access is denied.' });
    const before = fakeLlm.requests.length;
    fakeLlm.scriptChat([{ content: 'Windows refused access to the file.' }]);
    await ask(yap, 'What does this error mean?');
    await expect
      .poll(() => fakeLlm.requests.slice(before).some((r) => userOf(r).includes('Access is denied.')), { timeout: 10_000 })
      .toBe(true);
    const asked = fakeLlm.requests.slice(before).find((r) => userOf(r).includes('Access is denied.'));
    expect(asked.images ?? []).toHaveLength(0);
    expect(userOf(asked)).toContain('[Text read from the screenshot on this PC, sent instead of the picture]');
    expect(userOf(asked)).toContain('was read by Yap from the window the user had in front of them');
    // The answer names its source; the screenshot stays with the question, sent as text.
    await expectStore(yap, 'chats.json', (s) =>
      s.conversations.some((c) => c.messages.some((m) => m.role === 'assistant' && m.sources?.some((x) => x.screen && x.title === "The window's text, read on this PC")))
    );
    const q = questionNamed(yap, 'What does this error mean?');
    expect(q.images[0]).toMatchObject({ source: 'window', asText: true });
    // In Chat (its conversation opened from the list): the source chip, which opens nothing.
    await main.getByRole('navigation', { name: 'Main' }).getByRole('button', { name: 'Chat', exact: true }).click();
    await main.locator('.conv').first().click(); // the latest conversation (a follow-up joins the last one)
    await expect(main.getByTestId('chat-screen-source').last()).toContainText("The window's text, read on this PC");
    await shot(main, '02-the-windows-text-as-a-source');
  } finally {
    await yap.invoke('e2e_snip_ocr', { text: null });
    await setConfig(yap, main, { llmScopes: { chat: chatOn(fakeLlm, 'custom', 'qwen2.5-vl-7b') } });
  }
});

test('a cloud model: never without asking — the model says it can\'t see, the alert asks once, Allow lets it', async ({
  yap,
  main,
  fakeLlm,
  shot,
}) => {
  // (The answer read aloud without its card, so the bar shows the ask.)
  await setConfig(yap, main, {
    llmScopes: { chat: chatOn(fakeLlm, 'groq', 'qwen/qwen3.8-27b') },
    assistantScreenCloud: false,
    assistantScreenAsked: false,
    speakReplies: true,
    assistantAnswerCard: false,
  });
  await inAnotherApp(yap);
  try {
    await windowInFront(yap);
    let before = fakeLlm.requests.length;
    fakeLlm.scriptChat([{ content: "I can't see your screen." }]);
    await ask(yap, 'Summarise this page.');
    await expect.poll(() => fakeLlm.requests.slice(before).some((r) => userOf(r).includes('Summarise this page.')), { timeout: 10_000 }).toBe(true);
    const asked = fakeLlm.requests.slice(before).find((r) => userOf(r).includes('Summarise this page.'));
    expect(asked.images ?? []).toHaveLength(0);
    expect(userOf(asked)).toContain("haven't let Yap send their screen to Groq");
    // Asked once, on the bar (Yap's window isn't in front).
    const card = yap.overlay.getByRole('status').filter({ hasText: "Let Yap's AI see your screen?" });
    await expect(card).toBeVisible({ timeout: 10_000 });
    await expect(card).toContainText('Groq');
    await shot(yap.overlay, '03-ask-once-for-the-cloud');
    expect((await yap.invoke('get_config')).assistantScreenAsked).toBe(true);
    await card.getByRole('button', { name: 'Allow' }).click();
    await expect.poll(async () => (await yap.invoke('get_config')).assistantScreenCloud).toBe(true);
    await yap.invoke('assistant_stop');

    // Allowed: the window goes to Groq, and the receipts say so.
    await yap.invoke('connections_clear');
    await windowInFront(yap);
    before = fakeLlm.requests.length;
    fakeLlm.scriptChat([{ content: 'A grey page.' }]);
    await ask(yap, 'Summarise this page.');
    await expect.poll(() => fakeLlm.requests.slice(before).filter((r) => r.images?.length).length, { timeout: 10_000 }).toBe(1);
    await expect
      .poll(async () => (await yap.invoke('connections_summary')).groups.map((g) => g.label), { timeout: 10_000 })
      .toContain('Voice Assistant + the window in front · Groq');
  } finally {
    await yap.invoke('assistant_stop');
    await yap.invoke('tts_stop');
    await setConfig(yap, main, {
      llmScopes: { chat: chatOn(fakeLlm, 'custom', 'qwen2.5-vl-7b') },
      assistantScreenCloud: false,
      speakReplies: false,
      assistantAnswerCard: true,
    });
  }
});

test('Settings → Voice Assistant: the window in front, on by default, the cloud off; off means never', async ({ yap, main, fakeLlm, shot }) => {
  const dialog = await openSettings(main, 'Voice Assistant');
  const see = dialog.getByRole('button', { name: 'Let Yap’s AI see your screen when you ask about it' });
  const cloud = dialog.getByRole('button', { name: 'With cloud models too' });
  await expect(see).toHaveAttribute('aria-pressed', 'true');
  await expect(cloud).toHaveAttribute('aria-pressed', 'false');
  await see.scrollIntoViewIfNeeded();
  await shot(main, '04-settings-the-window-in-front');
  await see.click();
  await expect(see).toHaveAttribute('aria-pressed', 'false');
  await expect(cloud).toBeDisabled();
  await expect.poll(async () => (await yap.invoke('get_config')).assistantScreen).toBe(false);
  await closeSettings(main);
  try {
    await windowInFront(yap);
    const before = fakeLlm.requests.length;
    fakeLlm.scriptChat([{ content: 'An error.' }]);
    await ask(yap, "What's this error?");
    await expect.poll(() => fakeLlm.requests.slice(before).some((r) => userOf(r).includes("What's this error?")), { timeout: 10_000 }).toBe(true);
    expect(fakeLlm.requests.slice(before).some((r) => r.images?.length || r.kind === 'screenQuestion')).toBe(false);
    expect(userOf(fakeLlm.requests.slice(before).find((r) => userOf(r).includes("What's this error?")))).not.toContain('screenshot');
    // …nor may the model look itself.
    expect(fakeLlm.requests.slice(before).some((r) => r.tools?.includes('look_at_screen'))).toBe(false);
  } finally {
    await yap.invoke('assistant_stop');
    await setConfig(yap, main, { assistantScreen: true });
  }
});
