// Typed questions (2026-11-03): the composer's text box sends `ask` to Yap
// on the PC, the answer comes in the same thread and isn't spoken; ▶ under
// it has it read (`read_aloud`). Enter sends, Shift+Enter makes a new line,
// the box grows; the composer rides above the on-screen keyboard; a PC from
// before typed questions says so plainly. Yap on the PC mocked (a spoken
// question beside them on a PC whose Done sends it, as a PC from before
// words does: `asks`; Talk's words for the box are words.spec.js's).
import { test, expect } from '@playwright/test';
import {
  servePage,
  seedDevice,
  mockPc,
  open,
  ready,
  ask,
  type,
  snapshot,
  downFrame,
  voice,
  sentences,
  CAN_BEFORE_WORDS,
} from './support/harness.js';

/** A PC whose Done sends the spoken question, and says its answer. */
const asks = { can: CAN_BEFORE_WORDS };

const QUESTION = 'When does the bakery on Elm Street open on Sundays?';

test.beforeEach(async ({ page }) => {
  await servePage(page);
  await seedDevice(page);
});

/** The PC answers the last typed question: heard, a step, the answer as it's written, then checked with its id. */
function answer(pc, text, id = 'chat:7:1') {
  pc.send({ type: 'heard', text: pc.of('ask').at(-1).text });
  pc.send({ type: 'state', phase: 'thinking' });
  pc.send({ type: 'step', label: 'Searching the web · Elm Street bakery hours' });
  pc.send({ type: 'answer', text: text.split(' ').slice(0, 3).join(' '), final: false });
  pc.send({ type: 'answer', text, final: true, id });
  pc.send({ type: 'state', phase: 'idle' });
}

test('a typed question: sent as ask, answered in the thread, and nothing spoken', async ({ page }) => {
  const pc = await mockPc(page, asks);
  await open(page);
  await ready(page, pc);
  const input = page.getByTestId('input');
  await expect(input).toHaveAttribute('placeholder', 'Ask Yap, or hold to talk');
  // Send is there, dim, until there are words to send; the mic beside it.
  await expect(page.getByTestId('send')).toBeDisabled();
  await input.fill(QUESTION);
  await expect(page.getByTestId('send')).toBeEnabled();
  await expect(page.getByTestId('talk')).toBeVisible();
  await input.press('Enter');
  await expect.poll(() => pc.of('ask')).toEqual([{ type: 'ask', text: QUESTION }]);
  // The box empties; the question is in the thread at once, typed (no 🎙);
  // Send turns into ■ while Yap works it out.
  await expect(input).toHaveValue('');
  await expect(page.getByTestId('send')).toBeHidden();
  await expect(page.getByTestId('heard').last()).toHaveText(QUESTION);
  await expect(page.getByRole('img', { name: 'Asked by voice' })).toHaveCount(0);
  // Chat's activity line while Yap works it out, with ■ to stop it.
  await expect(page.getByTestId('step')).toContainText('Thinking');
  await expect(page.getByTestId('phase')).toHaveText('Working it out');
  await expect(page.getByTestId('cancel')).toBeVisible();

  answer(pc, 'It opens at **8 am** on Sundays, and closes at 1 pm.');
  const a = page.getByTestId('answer').last();
  await expect(a).toHaveText('It opens at 8 am on Sundays, and closes at 1 pm.');
  await expect(a).toHaveClass(/final/);
  await expect(a.locator('strong')).toHaveText('8 am');
  await expect(page.getByTestId('step')).toBeHidden();
  await expect(page.getByTestId('phase')).toHaveText('Ready');
  await expect(page.getByTestId('turn')).toHaveCount(1);
  // ▶ under it, to hear it.
  await expect(page.getByTestId('read-aloud')).toHaveText('Read aloud');

  // Nothing spoken, no mic: no start, no frames, nothing played.
  expect(pc.of('start')).toHaveLength(0);
  expect(pc.frames).toHaveLength(0);
  const snap = await snapshot(page);
  expect(snap.player.active).toBe(false);
  expect(snap.micOpen).toBe(false);
  expect(snap.turns).toEqual([{ q: QUESTION, typed: true, final: true, answerId: 'chat:7:1' }]);

  // A spoken question after it goes in the same thread, its bubble with 🎙.
  await ask(page, pc, 300);
  pc.send({ type: 'heard', text: 'And on Saturdays?' });
  await expect(page.getByTestId('turn')).toHaveCount(2);
  await expect(page.getByRole('img', { name: 'Asked by voice' })).toHaveCount(1);
});

test('Enter sends, Shift+Enter makes a new line; the box grows to a few lines, then scrolls', async ({ page }) => {
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  const input = page.getByTestId('input');
  await input.click();
  const one = (await input.boundingBox()).height;
  await page.keyboard.type('A shopping list:');
  await page.keyboard.press('Shift+Enter');
  await page.keyboard.type('eggs');
  await page.keyboard.press('Shift+Enter');
  await page.keyboard.type('flour');
  expect(pc.of('ask')).toHaveLength(0);
  const three = (await input.boundingBox()).height;
  expect(three).toBeGreaterThan(one * 1.8);
  for (const w of ['milk', 'butter', 'sugar', 'lemons', 'yeast']) {
    await page.keyboard.press('Shift+Enter');
    await page.keyboard.type(w);
  }
  // Five lines at most, then it scrolls inside.
  const most = (await input.boundingBox()).height;
  expect(most).toBeLessThan(one * 4.6);
  expect(await input.evaluate((t) => t.scrollHeight > t.clientHeight)).toBe(true);
  await page.keyboard.press('Enter');
  await expect.poll(() => pc.of('ask').length).toBe(1);
  expect(pc.of('ask')[0].text).toBe('A shopping list:\neggs\nflour\nmilk\nbutter\nsugar\nlemons\nyeast');
  // Back to one line.
  await expect.poll(async () => Math.round((await input.boundingBox()).height)).toBe(Math.round(one));
  // An empty box sends nothing.
  await input.press('Enter');
  await page.waitForTimeout(200);
  expect(pc.of('ask')).toHaveLength(1);
});

test('▶ Read aloud: read_aloud with the answer’s id, Yap’s voice on the phone, ■ stops it', async ({ page }) => {
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await type(page, pc, QUESTION);
  answer(pc, 'It opens at 8 am on Sundays.', 'chat:7:1');
  const read = page.getByTestId('read-aloud');
  await expect(read).toHaveText('Read aloud');
  await read.click();
  await expect.poll(() => pc.of('read_aloud')).toEqual([{ type: 'read_aloud', id: 'chat:7:1' }]);
  // The PC: idle (whatever was going on ended), then the voice.
  pc.send({ type: 'state', phase: 'idle' });
  pc.send({ type: 'state', phase: 'speaking' });
  pc.send({ type: 'speak_start', rate: 24000 });
  for (const f of sentences(3000)) pc.sendBinary(f);
  await expect.poll(async () => (await snapshot(page)).player.state).toBe('playing');
  // Through the media player, the standard way.
  expect((await snapshot(page)).player.via).toBe('media');
  // ▶ turned into Stop in place, "Speaking…" beside it: the one Stop (none in the composer).
  await expect(page.getByTestId('read-aloud')).toHaveText('Stop');
  await expect(page.getByText('Speaking…')).toBeVisible();
  await expect(page.getByTestId('stop')).toBeHidden();
  await expect(page.getByRole('button', { name: 'Stop' })).toHaveCount(1);
  expect((await snapshot(page)).reading).toBe('chat:7:1');
  // ■ under it: the voice stops at once, the PC told.
  await page.getByTestId('read-aloud').click();
  await expect.poll(() => pc.of('interrupt').length).toBe(1);
  await expect.poll(async () => (await snapshot(page)).player.active).toBe(false);
  await expect(page.getByTestId('read-aloud')).toHaveText('Read aloud');
  await expect(page.getByText('Speaking…')).toBeHidden();
  // Read again, and played to its end this time.
  await page.getByTestId('read-aloud').click();
  await expect.poll(() => pc.of('read_aloud').length).toBe(2);
  pc.send({ type: 'state', phase: 'idle' });
  pc.send({ type: 'state', phase: 'speaking' });
  pc.send({ type: 'speak_start', rate: 24000 });
  for (let i = 0; i < 4; i++) pc.sendBinary(downFrame(i, voice(24000, 100)));
  pc.send({ type: 'speak_end' });
  pc.send({ type: 'state', phase: 'idle' });
  await expect.poll(async () => (await snapshot(page)).player.played, { timeout: 4000 }).toBeCloseTo(0.4, 2);
  await expect.poll(async () => (await snapshot(page)).reading).toBe('');
  await expect(page.getByTestId('read-aloud')).toHaveText('Read aloud');
  // Still no mic for any of it.
  expect(pc.of('start')).toHaveLength(0);
});

test('one Stop while Yap speaks: the composer’s for a spoken answer, ▶’s own for an answer read aloud', async ({ page }) => {
  const pc = await mockPc(page, asks);
  await open(page);
  await ready(page, pc);
  await type(page, pc, QUESTION);
  answer(pc, 'It opens at 8 am on Sundays.', 'chat:7:1');
  await expect(page.getByTestId('read-aloud')).toHaveText('Read aloud');
  // A spoken question, its answer said.
  await ask(page, pc, 300);
  pc.send({ type: 'heard', text: 'And when does it close?' });
  pc.send({ type: 'state', phase: 'thinking' });
  pc.send({ type: 'answer', text: 'At 1 pm.', final: true, id: 'chat:7:3' });
  pc.send({ type: 'state', phase: 'speaking' });
  pc.send({ type: 'speak_start', rate: 24000 });
  for (let i = 0; i < 30; i++) pc.sendBinary(downFrame(i, voice(24000, 100)));
  await expect(page.getByTestId('stop')).toBeVisible();
  // Under it only the status: the bars and "Speaking…", no Stop (nor ▶) of its own.
  const spoken = page.getByTestId('turn').last();
  await expect(spoken.locator('.speaking')).toHaveText('Speaking…');
  await expect(spoken.getByRole('button', { name: /Stop|Read aloud/ })).toHaveCount(0);
  // The earlier answer keeps its ▶; one Stop on the page, the composer's.
  await expect(page.getByTestId('turn').first().getByTestId('read-aloud')).toHaveText('Read aloud');
  await expect(page.getByRole('button', { name: 'Stop' })).toHaveCount(1);
  // ▶ on the earlier answer: that one plays, its ▶ the Stop now.
  await page.getByTestId('turn').first().getByTestId('read-aloud').click();
  await expect.poll(() => pc.of('read_aloud')).toEqual([{ type: 'read_aloud', id: 'chat:7:1' }]);
  pc.send({ type: 'state', phase: 'idle' });
  pc.send({ type: 'state', phase: 'speaking' });
  pc.send({ type: 'speak_start', rate: 24000 });
  for (let i = 0; i < 30; i++) pc.sendBinary(downFrame(i, voice(24000, 100)));
  await expect(page.getByTestId('turn').first().getByTestId('read-aloud')).toHaveText('Stop');
  await expect(page.getByTestId('turn').first().locator('.speaking')).toHaveText('Speaking…');
  await expect(page.getByTestId('stop')).toBeHidden();
  await expect(page.getByRole('button', { name: 'Stop' })).toHaveCount(1);
  // The spoken one's ▶ is back.
  await expect(page.getByTestId('turn').last().getByTestId('read-aloud')).toHaveText('Read aloud');
  // Its Stop: the PC told (▶ over the spoken answer needed none: read_aloud abandons what plays).
  await page.getByTestId('turn').first().getByTestId('read-aloud').click();
  await expect.poll(() => pc.of('interrupt').length).toBe(1);
  await expect(page.getByRole('button', { name: 'Stop' })).toHaveCount(0);
});

test('typing while Yap speaks: its voice stops (interrupt), then the typed question goes', async ({ page }) => {
  const pc = await mockPc(page, asks);
  await open(page);
  await ready(page, pc);
  await ask(page, pc, 300);
  pc.send({ type: 'heard', text: 'What is on this evening?' });
  pc.send({ type: 'state', phase: 'speaking' });
  pc.send({ type: 'speak_start', rate: 24000 });
  for (let i = 0; i < 30; i++) pc.sendBinary(downFrame(i, voice(24000, 100)));
  await expect(page.getByTestId('stop')).toBeVisible();
  await type(page, pc, 'Actually, what about tomorrow?');
  expect(pc.types().filter((t) => t !== 'ping').slice(-2)).toEqual(['interrupt', 'ask']);
  expect((await snapshot(page)).player.active).toBe(false);
  await expect(page.getByTestId('turn')).toHaveCount(2);
  await expect(page.getByTestId('heard').last()).toHaveText('Actually, what about tomorrow?');
});

test('typing while Yap works on a question: cancel, then the typed question', async ({ page }) => {
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await type(page, pc, 'Plan a picnic for Saturday.');
  pc.send({ type: 'heard', text: 'Plan a picnic for Saturday.' });
  pc.send({ type: 'state', phase: 'thinking' });
  await type(page, pc, 'No, Sunday.');
  expect(pc.types().filter((t) => t !== 'ping').slice(-2)).toEqual(['cancel', 'ask']);
  // The first ends as cancelled, the new one waits for its answer.
  await expect(page.getByTestId('answer').first()).toHaveText('Cancelled');
  pc.send({ type: 'state', phase: 'idle' }); // the cancel's
  pc.send({ type: 'heard', text: 'No, Sunday.' });
  pc.send({ type: 'state', phase: 'thinking' });
  pc.send({ type: 'answer', text: 'Sunday it is: sandwiches, fruit and a flask of tea.', final: true, id: 'chat:8:3' });
  pc.send({ type: 'state', phase: 'idle' });
  await expect(page.getByTestId('answer').last()).toHaveText('Sunday it is: sandwiches, fruit and a flask of tea.');
  await expect(page.getByTestId('turn')).toHaveCount(2);
});

test('a PC from before typed questions: said plainly, nothing sent', async ({ page }) => {
  const pc = await mockPc(page, { old: true });
  await open(page);
  await ready(page, pc);
  await page.getByTestId('input').fill(QUESTION);
  await page.getByTestId('input').press('Enter');
  await expect(page.getByTestId('error')).toHaveText(
    'Yap on your PC needs an update to answer typed questions. Update Yap there; talking works as before.',
  );
  expect(pc.of('ask')).toHaveLength(0);
  // What was typed stays, to send once Yap's updated.
  await expect(page.getByTestId('input')).toHaveValue(QUESTION);
  // Talking still works.
  await ask(page, pc, 200);
  await expect(page.getByTestId('error')).toBeHidden();
});

/** How far the thread is from its end, in pixels. */
const fromEnd = (page) => page.getByTestId('thread').evaluate((t) => t.scrollHeight - t.scrollTop - t.clientHeight);
/** Two frames: the browser's scroll events for what was drawn have fired. */
const frames = (page) => page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));

test('the thread follows an answer as it’s written, Thinking under the question and all; scrolled up, it stays put', async ({ page }) => {
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  // Enough of a conversation that it scrolls.
  for (let i = 1; i <= 4; i++) {
    await type(page, pc, `Question ${i} about the allotment rota?`);
    pc.send({ type: 'answer', text: `Answer ${i}: ${'The rota is on the shed door. '.repeat(3)}`, final: true, id: `chat:9:${i * 2 - 1}` });
    pc.send({ type: 'state', phase: 'idle' });
  }
  await expect.poll(() => fromEnd(page)).toBeLessThan(4);
  await type(page, pc, 'Who waters the beans this week?');
  // Thinking shows under the question, after the thread went to its end
  // (its scroll event comes a frame later): still at the end, Thinking in view.
  await expect(page.getByTestId('step')).toBeVisible();
  await frames(page);
  await expect.poll(() => fromEnd(page)).toBeLessThan(4);
  await expect(page.getByTestId('step')).toBeInViewport();
  pc.send({ type: 'state', phase: 'thinking' });
  pc.send({ type: 'step', label: 'Reading the rota' });
  await frames(page);
  // The answer as it's written, a sentence at a time: the end stays in view.
  const words = 'Sam waters the beans this week, in the evenings once the sun is off them. ';
  for (let n = 1; n <= 6; n++) {
    pc.send({ type: 'answer', text: words.repeat(n), final: false });
    await frames(page);
    await expect.poll(() => fromEnd(page)).toBeLessThan(4);
  }
  pc.send({ type: 'answer', text: words.repeat(7), final: true, id: 'chat:9:9' });
  pc.send({ type: 'state', phase: 'idle' });
  await frames(page);
  await expect.poll(() => fromEnd(page)).toBeLessThan(4);
  await expect(page.getByTestId('read-aloud').last()).toBeInViewport();

  // Scrolled up to read: the next answer's words don't pull it down.
  await type(page, pc, 'And next week?');
  await page.getByTestId('thread').evaluate((t) => {
    t.scrollTop = 0;
  });
  await frames(page);
  pc.send({ type: 'answer', text: words.repeat(3), final: false });
  await frames(page);
  expect(await page.getByTestId('thread').evaluate((t) => t.scrollTop)).toBe(0);
  // Back at the end: it follows again.
  await page.getByTestId('thread').evaluate((t) => {
    t.scrollTop = t.scrollHeight;
  });
  await frames(page);
  pc.send({ type: 'answer', text: words.repeat(6), final: false });
  await frames(page);
  await expect.poll(() => fromEnd(page)).toBeLessThan(4);
});

test('a spoken question answered out loud: the thread follows the answer and its Speaking line', async ({ page }) => {
  const pc = await mockPc(page, asks);
  await open(page);
  await ready(page, pc);
  for (let i = 1; i <= 4; i++) {
    await type(page, pc, `Question ${i} about the allotment rota?`);
    pc.send({ type: 'answer', text: `Answer ${i}: ${'The rota is on the shed door. '.repeat(3)}`, final: true, id: `chat:9:${i * 2 - 1}` });
    pc.send({ type: 'state', phase: 'idle' });
  }
  await ask(page, pc, 300);
  pc.send({ type: 'heard', text: 'Who has the shed key?' });
  pc.send({ type: 'state', phase: 'thinking' });
  await frames(page);
  const words = 'Priya has the shed key until Sunday, then it goes back on the hook. ';
  for (let n = 1; n <= 4; n++) {
    pc.send({ type: 'answer', text: words.repeat(n), final: false });
    await frames(page);
  }
  pc.send({ type: 'answer', text: words.repeat(5), final: true, id: 'chat:9:9' });
  pc.send({ type: 'state', phase: 'speaking' });
  pc.send({ type: 'speak_start', rate: 24000 });
  for (let i = 0; i < 4; i++) pc.sendBinary(downFrame(i, voice(24000, 100)));
  await frames(page);
  await expect(page.getByTestId('answer').last()).toHaveClass(/final/);
  await expect.poll(() => fromEnd(page)).toBeLessThan(4);
  await expect(page.locator('.speaking').last()).toBeInViewport();
});

test('the keyboard up (the page resized for it): the composer above it, the thread at its end', async ({ page }) => {
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  for (let i = 1; i <= 4; i++) {
    await type(page, pc, `Question ${i} about the allotment rota?`);
    pc.send({ type: 'answer', text: `Answer ${i}: ${'The rota is on the shed door. '.repeat(3)}`, final: true, id: `chat:9:${i * 2 - 1}` });
    pc.send({ type: 'state', phase: 'idle' });
  }
  await page.getByTestId('input').focus();
  // Chrome on a phone (interactive-widget=resizes-content): the page is what's above the keyboard.
  await page.setViewportSize({ width: 390, height: 844 - 330 });
  const composer = await page.getByTestId('composer').boundingBox();
  expect(composer.y + composer.height).toBeLessThanOrEqual(844 - 330);
  // The latest answer in view, just over the composer.
  await expect
    .poll(() => page.getByTestId('thread').evaluate((t) => t.scrollHeight - t.scrollTop - t.clientHeight))
    .toBeLessThan(4);
  await expect(page.getByTestId('answer').last()).toBeInViewport();
  await expect(page.getByTestId('model-chip')).toBeInViewport();
});

test('the keyboard up where the page isn’t resized (iPhone): fitted to the visual viewport', async ({ page }) => {
  // The visual viewport as an iPhone's Safari reports it, the keyboard up.
  await page.addInitScript(() => {
    const vv = new EventTarget();
    Object.assign(vv, { height: innerHeight, width: innerWidth, offsetTop: 0, offsetLeft: 0, scale: 1 });
    Object.defineProperty(window, 'visualViewport', { configurable: true, value: vv });
    window.__keyboard = (h) => {
      vv.height = innerHeight - h;
      vv.dispatchEvent(new Event('resize'));
    };
  });
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await page.getByTestId('input').focus();
  await page.evaluate(() => window.__keyboard(300));
  await expect.poll(() => page.evaluate(() => document.documentElement.style.getPropertyValue('--app-h'))).toBe('544px');
  const composer = await page.getByTestId('composer').boundingBox();
  expect(composer.y + composer.height).toBeLessThanOrEqual(544);
  expect(composer.y).toBeGreaterThan(400);
  // Down again: the whole screen.
  await page.evaluate(() => window.__keyboard(0));
  await expect.poll(() => page.evaluate(() => document.documentElement.style.getPropertyValue('--app-h'))).toBe('844px');
});

test('a reload keeps the conversation (this tab only)', async ({ page }) => {
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await type(page, pc, QUESTION);
  answer(pc, 'It opens at 8 am on Sundays.', 'chat:7:1');
  await expect(page.getByTestId('read-aloud')).toBeVisible();
  await open(page);
  await ready(page, pc);
  await expect(page.getByTestId('turn')).toHaveCount(1);
  await expect(page.getByTestId('heard')).toHaveText(QUESTION);
  await expect(page.getByTestId('answer')).toHaveText('It opens at 8 am on Sundays.');
  await expect(page.getByTestId('read-aloud')).toBeVisible();
  // Never in the phone's lasting storage.
  expect(await page.evaluate(() => JSON.stringify(localStorage))).not.toContain('bakery');
});
