// What Chat is doing, while it answers (src-tauri/src/chat/activity.rs,
// views/chat/ActivityLine.svelte, ChatSteps.svelte), as the Claude Code app
// shows it:
// - One line at the thread's foot: a moving mark, the step under way and the
//   turn's time — "Searching the web · Gale location Baldur's Gate 3",
//   "Reading bg3.wiki/wiki/Gale · 4s" (Yap's own search and page read, Exa
//   on the fake web), "Thinking" until a step comes.
// - Claude on Anthropic's Messages API: its own search and page read, with
//   the query and the address from its stream.
// - Once the answer is in, the line goes and the steps fold under it into
//   one line ("Searched the web, read a page ▸") that opens to the chips.
// - The Voice Assistant: the Yap bar's capsule says the same in
//   "Thinking…"'s place.
// `stepsSummary`, `elapsed` and the stream's `step` event are checked on
// their own first (no app). The models are a local fake (support/fake-llm.js,
// which speaks the Messages API too); the web a local fake
// (support/fake-web.js). Nothing goes on the internet.
import { test as base, expect, openView, setConfig, inAnotherApp, mainOnScreen } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { startFakeWeb, GALE_URL } from './support/fake-web.js';
import { stepsSummary, elapsed } from '../src/lib/views/chat/chatSteps.js';
import { liveAnswer, applyEvent } from '../src/lib/core/streaming.js';

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
        name: 'chat-activity',
        env: fakeWeb.env,
        config: {
          llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat' } },
          webSearch: true,
          webSearchNative: true,
          webSearchService: 'exa',
          webSearchKeys: { exa: EXA_KEY },
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
const line = (main) => main.getByTestId('chat-activity');
const steps = (main) => thread(main).getByTestId('chat-steps').last();

async function freshChat(main) {
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
}

async function send(main, text) {
  await box(main).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
}

function gate() {
  let open;
  const promise = new Promise((r) => (open = r));
  return { promise, open };
}

/** Every line the activity line showed, from now on (its step only). */
async function watchLine(main) {
  await main.evaluate(() => {
    window.__lines = [];
    new MutationObserver(() => {
      const t = document.querySelector('[data-testid="chat-activity"] .aline')?.textContent?.trim();
      if (t && window.__lines.at(-1) !== t) window.__lines.push(t);
    }).observe(document.body, { subtree: true, childList: true, characterData: true });
  });
  return () => main.evaluate(() => window.__lines);
}

// ---- on their own (no app)

test("an answer's chips fold into one line, in the order they came", () => {
  expect(stepsSummary(['Searched the web: "Gale bg3"', 'Read bg3.wiki/wiki/Gale', 'Searched the web: "Gale romance"'])).toBe(
    'Searched the web 2 times, read a page'
  );
  expect(stepsSummary(['Searched the web with Anthropic: "x"', 'Read a.example/x', 'Read b.example/y', "Couldn't read c.example/z: the page didn't open"])).toBe(
    "Searched the web, read 2 pages, 1 didn't go through"
  );
  // A document's: started, its sections counted (folded chips say how many).
  expect(stepsSummary(['Started "Mage build" (3 sections)', 'Wrote 3 sections in "Mage build"', 'Rewrote "Gear" in "Mage build"'])).toBe(
    'Started a document, wrote 3 sections, rewrote a section'
  );
  expect(stepsSummary(['Found 1 note for "kitchen budget"', 'Read "Kitchen budget" (notes, lines 1–4 of 4)', 'Edited "Kitchen budget": added "Sam"'])).toBe(
    'Searched your notes, read a note, changed a note'
  );
  expect(stepsSummary(['Checked your calendar: 2 meetings, Friday', 'Remembered: Has a dog', 'Opened Notepad', "Didn't open an app: not after reading the web"])).toBe(
    "Checked your calendar, updated Yap's memory, opened an app, 1 didn't go through"
  );
  expect(stepsSummary(['Searched your meetings by meaning · 3 passages', 'Ran /todos · 2 open tasks', 'Something new'])).toBe(
    'Searched your meetings, ran a skill, used a tool'
  );
  expect(stepsSummary([])).toBe('');
});

test("a turn's time and the stream's step", () => {
  expect([elapsed(0), elapsed(12), elapsed(119), elapsed(390), elapsed(3725)]).toEqual(['0s', '12s', '1m 59s', '6m 30s', '1h 2m']);
  let s = liveAnswer();
  expect(s.step).toBeNull();
  s = applyEvent(s, { kind: 'step', step: { mark: 'search', line: 'Searching the web · Gale' } });
  expect([s.step, s.started]).toEqual([{ mark: 'search', line: 'Searching the web · Gale' }, false]);
  s = applyEvent(s, { kind: 'text', text: 'Gale is' });
  expect(s.step.line).toBe('Searching the web · Gale');
});

// ---- Chat

test("Yap's own search and page read: the line says each with the turn's time, then the steps fold under the answer", async ({
  main,
  fakeLlm,
  shot,
}) => {
  const held = gate();
  fakeLlm.scriptChat([
    { tools: [{ name: 'web_search', args: { query: "Gale location Baldur's Gate 3" } }] },
    { tools: [{ name: 'read_page', args: { url: GALE_URL } }] },
    { gate: held.promise, content: 'Gale is on the beach west of the Nautiloid crash site [1], stuck in a rune circle.', stream: { gapMs: 120 } },
  ]);
  await freshChat(main);
  const lines = await watchLine(main);
  await send(main, 'Where can I find Gale in BG3?');

  // Held after reading the page: the line says so, its time going up.
  await expect(line(main)).toContainText('Reading bg3.wiki/wiki/Gale', { timeout: 15_000 });
  await expect(line(main).locator('.atime')).toHaveText(/^· [2-9]s$/, { timeout: 10_000 });
  await expect(line(main)).toHaveAttribute('data-mark', 'read');
  // The steps done so far, folded under where the answer will be.
  await expect(steps(main).locator('summary')).toHaveText('Searched the web, read a page');
  await shot(main, '01-reading-a-page');
  held.open();

  // Done: the line goes; the steps stay folded under the answer.
  await expect(lastAnswer(main)).toContainText('stuck in a rune circle', { timeout: 15_000 });
  await expect(line(main)).toHaveCount(0);
  const said = await lines();
  expect(said).toEqual(
    expect.arrayContaining(['Thinking', 'Searching the web', "Searching the web · Gale location Baldur's Gate 3", 'Reading a page', 'Reading bg3.wiki/wiki/Gale', 'Writing the answer'])
  );
  expect(said.indexOf("Searching the web · Gale location Baldur's Gate 3")).toBeLessThan(said.indexOf('Reading bg3.wiki/wiki/Gale'));
  // Under the answer, not over it; folded, then open to the chips.
  const folded = steps(main);
  await expect(folded).not.toHaveAttribute('open');
  await expect(folded.locator('summary')).toHaveText('Searched the web, read a page');
  const answerBox = await lastAnswer(main).boundingBox();
  const stepsBox = await folded.boundingBox();
  expect(stepsBox.y).toBeGreaterThan(answerBox.y + answerBox.height - 1);
  await expect(folded.locator('.toolchip').first()).toBeHidden();
  await shot(main, '02-steps-folded');
  await folded.locator('summary').click();
  await expect(folded.locator('.toolchip')).toHaveText(['Searched the web: "Gale location Baldur\'s Gate 3"', 'Read bg3.wiki/wiki/Gale']);
  await expect(folded.locator('.toolchip').first()).toBeVisible();
  await shot(main, '03-steps-open');
});

test('Claude searches and reads a page itself: the line names its query and the page from its stream', async ({ yap, main, fakeLlm, shot }) => {
  const cfg = await yap.invoke('get_config');
  const claude = { enabled: true, provider: 'anthropic', baseUrl: fakeLlm.base, model: 'claude-opus-5-5', apiKey: CLAUDE_KEY, apiKeys: {}, prompt: '', disableThinking: false };
  await setConfig(yap, main, { llmScopes: { ...cfg.llmScopes, chat: claude } });
  try {
    fakeLlm.scriptChat([
      {
        reasoning: 'Search first, then open the talents page.',
        before: "I'll look this up.",
        server: [
          { search: 'tidecaller lone wolf build', results: [{ url: 'https://emberfall.example/wiki/Talents', title: 'Talents - Emberfall Wiki' }] },
          { fetch: 'https://emberfall.example/wiki/Talents', title: 'Talents - Emberfall Wiki', text: 'Lone Wolf doubles your points.' },
        ],
        content: 'Take Lone Wolf at level 1: it doubles your points.',
        cite: [{ url: 'https://emberfall.example/wiki/Talents', title: 'Talents - Emberfall Wiki', text: 'Lone Wolf doubles your points.' }],
        stream: { gapMs: 600 },
      },
    ]);
    await freshChat(main);
    const lines = await watchLine(main);
    await send(main, 'What is the best lone wolf Tidecaller build in Emberfall?');
    await expect(line(main)).toContainText('Searching the web · tidecaller lone wolf build', { timeout: 15_000 });
    await shot(main, '04-claude-searching');
    await expect(line(main)).toContainText('Reading emberfall.example/wiki/Talents', { timeout: 15_000 });
    await shot(main, '05-claude-reading');
    await expect(lastAnswer(main)).toContainText('Take Lone Wolf at level 1', { timeout: 20_000 });
    await expect(line(main)).toHaveCount(0);
    expect(await lines()).toEqual(
      expect.arrayContaining(['Thinking', 'Searching the web · tidecaller lone wolf build', 'Reading emberfall.example/wiki/Talents', 'Writing the answer'])
    );
    // What it wrote before its search was never the answer, nor a step.
    await expect(lastAnswer(main)).not.toContainText("I'll look this up.");
    await expect(steps(main).locator('summary')).toHaveText('Searched the web, read a page');
    await expect(steps(main).locator('.toolchip')).toHaveText(['Searched the web with Anthropic: "tidecaller lone wolf build"', 'Read emberfall.example/wiki/Talents']);
  } finally {
    await setConfig(yap, main, { llmScopes: cfg.llmScopes });
  }
});

// ---- the Voice Assistant

test("asked out loud: the bar's capsule says what the answer is doing", async ({ yap, main, fakeLlm, shot }) => {
  await yap.invoke('note_create', { title: 'Launch plan', content: 'Sam has the launch slides, in the shared folder.' });
  const held = gate();
  fakeLlm.scriptChat([
    { tools: [{ name: 'search_notes', args: { query: 'launch slides' } }] },
    { gate: held.promise, content: 'Sam has the launch slides.' },
  ]);
  await inAnotherApp(yap);
  try {
    await yap.invoke('e2e_assistant_ask', { text: 'who has the launch slides', listenMs: 0 });
    const cap = yap.overlay.locator('.capsule');
    await expect(cap).toHaveAttribute('data-ask', 'thinking');
    await expect(cap).toContainText('Searching your notes · launch slides…', { timeout: 10_000 });
    await shot(yap.overlay, '06-capsule-searching');
    // ■: the capsule goes.
    await yap.invoke('assistant_stop');
    held.open();
    await expect(cap).toHaveCount(0, { timeout: 10_000 });
  } finally {
    held.open();
    await yap.invoke('assistant_stop').catch(() => {});
    await yap.invoke('tts_stop').catch(() => {});
    await mainOnScreen(yap);
  }
});
