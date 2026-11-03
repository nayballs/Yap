// Talk's words in the box (2026-11-03, the Claude and ChatGPT apps' way):
// with hands-free off, Talk — a tap then Done, or a hold then release —
// sends `start {mode: "words"}` and `stop`; the PC's `words` go in the
// composer's text box, at the caret, after anything typed, to check and
// fix; Send asks them as the spoken question they were (`ask {speak:
// true}`: the bubble's 🎙, the answer said on the phone, ■, barge-in) —
// while a question only typed stays typed, answered on screen. Nothing
// heard keeps the box; ✕ while it transcribes drops the words; the pictures
// wait in the composer for Send. Hands-free on — and a PC from before,
// without "words" in `ready.can` — still send at once. Yap on the PC
// mocked; the mic the looped 440 Hz tone.
import { test, expect } from '@playwright/test';
import {
  servePage,
  seedDevice,
  mockPc,
  open,
  ready,
  ask,
  snapshot,
  openSettings,
  closeSettings,
  pickerData,
  photo,
  choosePhotos,
  downFrame,
  voice,
  CAN_BEFORE_WORDS,
  CAN_BEFORE_SPEAK,
} from './support/harness.js';

const NO_WORDS = "Yap didn't hear anything. Try again, or type it.";

test.beforeEach(async ({ page }) => {
  await servePage(page);
  await seedDevice(page);
});

/** The PC's answer to Talk's words: transcribing, the words, idle. */
function words(pc, text) {
  pc.send({ type: 'state', phase: 'transcribing' });
  pc.send({ type: 'words', text });
  pc.send({ type: 'state', phase: 'idle' });
}

/** The text box's words and its caret. */
const box = (page) =>
  page.getByTestId('input').evaluate((t) => ({ value: t.value, start: t.selectionStart, end: t.selectionEnd }));

/** The PC's answer said out loud: heard, thinking, the answer, then its voice (`ms` of it). */
function spokenAnswer(pc, { heard, answer, id, ms = 600 }) {
  pc.send({ type: 'heard', text: heard });
  pc.send({ type: 'state', phase: 'thinking' });
  pc.send({ type: 'answer', text: answer, final: true, id });
  pc.send({ type: 'state', phase: 'speaking' });
  pc.send({ type: 'speak_start', rate: 24000 });
  for (let i = 0; i < ms / 100; i++) pc.sendBinary(downFrame(i, voice(24000, 100)));
}

/** Talk's words in the box: a tap, a moment of the mic, Done, then the PC's `words`. */
async function talkWords(page, pc, text) {
  await ask(page, pc, 300);
  words(pc, text);
  await expect.poll(() => page.getByTestId('input').inputValue()).toContain(text);
}

test('Talk, then Done: the words go in the box, nothing is asked; Send asks them out loud', async ({ page }) => {
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  const composer = page.getByTestId('composer');
  await page.getByTestId('talk').click();
  await expect.poll(() => pc.of('start')).toEqual([{ type: 'start', mode: 'words' }]);
  await expect(page.getByTestId('phase')).toHaveText('Listening');
  // No question bubble while it listens: nothing's asked yet.
  await expect(page.getByTestId('turn')).toHaveCount(0);
  // Done finishes with a ✓ (nothing is sent), not the ↑ of a send.
  await expect(composer.locator('.check-icon')).toBeVisible();
  await expect(composer.locator('.done-icon')).toBeHidden();
  await expect(page.getByTestId('talk')).toContainText('Done');
  await expect.poll(() => pc.frames.length, { timeout: 5000 }).toBeGreaterThan(20);
  await page.getByTestId('talk').click();
  await expect.poll(() => pc.of('stop')).toEqual([{ type: 'stop' }]);

  // Transcribing: the capsule a moment more, no waveform, no Done, ✕ to cancel.
  await expect(composer).toHaveAttribute('data-state', 'transcribing');
  await expect(page.getByTestId('phase')).toHaveText('Transcribing…');
  await expect(page.locator('#live-label')).toHaveText('Transcribing…');
  await expect(page.getByTestId('meter')).toBeHidden();
  await expect(page.getByTestId('talk')).toBeHidden();
  await expect(page.getByTestId('input')).toBeHidden();
  await expect(page.getByTestId('cancel')).toBeVisible();
  await expect(page.getByTestId('cancel')).toHaveAttribute('aria-label', 'Cancel');
  // No "Thinking" in the thread, no spinner: Yap isn't working out an answer.
  await expect(page.getByTestId('step')).toBeHidden();
  await expect(page.getByTestId('busy')).toBeHidden();

  words(pc, 'Remind me to water the beans tonight');
  await expect(page.getByTestId('input')).toHaveValue('Remind me to water the beans tonight');
  await expect(composer).toHaveAttribute('data-state', 'idle');
  await expect(page.getByTestId('phase')).toHaveText('Ready');
  // The caret at the end, Send lit; still nothing asked, no bubble.
  expect(await box(page)).toEqual({ value: 'Remind me to water the beans tonight', start: 36, end: 36 });
  await expect(page.getByTestId('send')).toBeEnabled();
  await expect(page.getByTestId('turn')).toHaveCount(0);
  expect(pc.of('ask')).toHaveLength(0);
  expect((await snapshot(page)).transcribing).toBe(false);

  expect((await snapshot(page)).said).toBe(true);

  // Fixed by hand, then sent: an `ask` that says they were said, the
  // bubble's 🎙 as a spoken question's.
  await page.getByTestId('input').click();
  await page.keyboard.press('End');
  await page.keyboard.type(' and on Friday');
  await page.getByTestId('send').click();
  await expect
    .poll(() => pc.of('ask'))
    .toEqual([{ type: 'ask', text: 'Remind me to water the beans tonight and on Friday', speak: true }]);
  await expect(page.getByTestId('input')).toHaveValue('');
  await expect(page.getByTestId('heard')).toHaveText('Remind me to water the beans tonight and on Friday');
  await expect(page.getByRole('img', { name: 'Asked by voice' })).toHaveCount(1);
  expect((await snapshot(page)).said).toBe(false);
  // Answered out loud, on the phone, as a spoken question is: the answer
  // in the thread, its voice playing, ■ in the composer meanwhile.
  spokenAnswer(pc, {
    heard: 'Remind me to water the beans tonight and on Friday',
    answer: "I'll remind you tonight at 8 and on Friday.",
    id: 'chat:5:1',
  });
  await expect(page.getByTestId('answer')).toHaveText("I'll remind you tonight at 8 and on Friday.");
  await expect.poll(async () => (await snapshot(page)).player.played, { timeout: 4000 }).toBeGreaterThan(0.2);
  await expect(page.getByTestId('stop')).toBeVisible();
  await expect(page.getByTestId('phase')).toHaveText('Speaking');
  pc.send({ type: 'speak_end' });
  pc.send({ type: 'state', phase: 'idle' });
  await expect.poll(async () => (await snapshot(page)).player.active, { timeout: 4000 }).toBe(false);
  await expect(page.getByTestId('phase')).toHaveText('Ready');
  const snap = await snapshot(page);
  expect(snap.turns).toEqual([{ q: 'Remind me to water the beans tonight and on Friday', typed: false, final: true, answerId: 'chat:5:1' }]);
  // One start, one stop: the words' only; the question went as `ask`.
  expect(pc.of('start')).toHaveLength(1);
  expect(pc.of('stop')).toHaveLength(1);
});

test('only typed: asked as typed, answered on screen; emptied, the box is typed again', async ({ page }) => {
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  const input = page.getByTestId('input');
  // Typed, nothing said: no `speak`, no 🎙, answered on screen.
  await input.fill('What time does the market open?');
  await page.getByTestId('send').click();
  await expect.poll(() => pc.of('ask').at(-1)).toEqual({ type: 'ask', text: 'What time does the market open?' });
  await expect(page.getByRole('img', { name: 'Asked by voice' })).toHaveCount(0);
  pc.send({ type: 'heard', text: 'What time does the market open?' });
  pc.send({ type: 'answer', text: 'At 8 am on Saturdays.', final: true, id: 'chat:5:1' });
  pc.send({ type: 'state', phase: 'idle' });
  await expect(page.getByTestId('read-aloud')).toHaveText('Read aloud');
  expect((await snapshot(page)).player.active).toBe(false);

  // Said, then sent: the next one typed is typed again.
  await talkWords(page, pc, 'Book the dentist for next week');
  await page.getByTestId('send').click();
  await expect.poll(() => pc.of('ask').at(-1)).toEqual({ type: 'ask', text: 'Book the dentist for next week', speak: true });
  pc.send({ type: 'heard', text: 'Book the dentist for next week' });
  pc.send({ type: 'answer', text: 'Which day suits you?', final: true, id: 'chat:5:3' });
  pc.send({ type: 'state', phase: 'idle' });
  await input.fill('Tuesday morning');
  await input.press('Enter');
  await expect.poll(() => pc.of('ask').at(-1)).toEqual({ type: 'ask', text: 'Tuesday morning' });

  // Said, then wiped by hand and typed afresh: typed.
  pc.send({ type: 'answer', text: 'Booked for Tuesday at 9.', final: true, id: 'chat:5:5' });
  pc.send({ type: 'state', phase: 'idle' });
  await talkWords(page, pc, 'Something I said by mistake');
  await input.fill('');
  expect((await snapshot(page)).said).toBe(false);
  await input.fill('Never mind, thanks');
  await input.press('Enter');
  await expect.poll(() => pc.of('ask').at(-1)).toEqual({ type: 'ask', text: 'Never mind, thanks' });

  // Typed first, then Talk's words after it: said (some of it came from Talk).
  pc.send({ type: 'answer', text: 'No problem.', final: true, id: 'chat:5:7' });
  pc.send({ type: 'state', phase: 'idle' });
  await input.fill('Add to the list:');
  await talkWords(page, pc, 'bin bags and light bulbs');
  await input.press('Enter');
  await expect.poll(() => pc.of('ask').at(-1)).toEqual({ type: 'ask', text: 'Add to the list: bin bags and light bulbs', speak: true });
  const turns = (await snapshot(page)).turns.map((t) => [t.q, t.typed]);
  expect(turns).toEqual([
    ['What time does the market open?', true],
    ['Book the dentist for next week', false],
    ['Tuesday morning', true],
    ['Never mind, thanks', true],
    ['Add to the list: bin bags and light bulbs', false],
  ]);
});

test("the said question's answer: ■ stops it, and Talk over it stops it and listens", async ({ page }) => {
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await talkWords(page, pc, 'Read me the forecast for tomorrow');
  await page.getByTestId('send').click();
  await expect.poll(() => pc.of('ask').at(-1)?.speak).toBe(true);
  spokenAnswer(pc, { heard: 'Read me the forecast for tomorrow', answer: 'Dry and bright, 14 degrees.', id: 'chat:7:1', ms: 3000 });
  await expect.poll(async () => (await snapshot(page)).player.played, { timeout: 4000 }).toBeGreaterThan(0.2);
  // ■: the voice stops at once, the PC is told.
  await page.getByTestId('stop').click();
  await expect.poll(() => pc.of('interrupt').length).toBe(1);
  await expect.poll(async () => (await snapshot(page)).player.active).toBe(false);
  pc.send({ type: 'state', phase: 'idle' });

  // Again, then Talk over the answer: it stops, and Yap listens (for the box).
  await talkWords(page, pc, 'And the day after?');
  await page.getByTestId('send').click();
  await expect.poll(() => pc.of('ask').at(-1)).toEqual({ type: 'ask', text: 'And the day after?', speak: true });
  spokenAnswer(pc, { heard: 'And the day after?', answer: 'Showers in the afternoon.', id: 'chat:7:3', ms: 3000 });
  await expect.poll(async () => (await snapshot(page)).player.played, { timeout: 4000 }).toBeGreaterThan(0.2);
  const starts = pc.of('start').length;
  await page.getByTestId('talk').click();
  await expect.poll(() => pc.of('interrupt').length).toBe(2);
  await expect.poll(() => pc.of('start').length).toBe(starts + 1);
  expect(pc.of('start').at(-1)).toEqual({ type: 'start', mode: 'words' });
  // The interrupt went before the start.
  const types = pc.types();
  expect(types.lastIndexOf('interrupt')).toBeLessThan(types.lastIndexOf('start'));
  await expect.poll(async () => (await snapshot(page)).player.active).toBe(false);
});

test('a said question with pictures: they go with Send, and Chat\'s line puts it back still said', async ({ page }) => {
  const pc = await mockPc(page, {
    picker: pickerData({ model: 'opus' }),
    model: { id: 'anthropic|claude-opus-5-5', label: 'Claude Opus 5.5 · Anthropic', onThisPc: false },
  });
  await open(page);
  await ready(page, pc);
  await choosePhotos(page, [await photo(page, { w: 800, h: 600 })]);
  await talkWords(page, pc, 'Which bird is this?');
  await page.getByTestId('send').click();
  await expect.poll(() => pc.of('ask')).toEqual([{ type: 'ask', text: 'Which bird is this?', pictures: 1, speak: true }]);
  expect(pc.pictures).toHaveLength(1);
  // The PC's model turned them down after all: back in the box with them, still said.
  pc.send({ type: 'cant_read', message: "Claude Opus 5.5 can't read images." });
  await expect(page.getByTestId('input')).toHaveValue('Which bird is this?');
  await expect(page.getByTestId('shot')).toHaveCount(1);
  expect((await snapshot(page)).said).toBe(true);
  await page.getByTestId('vision-as-text').click();
  await expect.poll(() => pc.of('ask').at(-1)).toEqual({ type: 'ask', text: 'Which bird is this?', pictures: 1, asText: true, speak: true });
});

test('a PC from before "speak": Send says so harmlessly, and it answers on screen with no error', async ({ page }) => {
  const pc = await mockPc(page, { can: CAN_BEFORE_SPEAK });
  await open(page);
  await ready(page, pc);
  await talkWords(page, pc, 'How long do I boil an egg?');
  await page.getByTestId('send').click();
  // The field goes (the PC reads it as nothing); the bubble is typed, as the PC keeps it.
  await expect.poll(() => pc.of('ask')).toEqual([{ type: 'ask', text: 'How long do I boil an egg?', speak: true }]);
  await expect(page.getByRole('img', { name: 'Asked by voice' })).toHaveCount(0);
  pc.send({ type: 'heard', text: 'How long do I boil an egg?' });
  pc.send({ type: 'state', phase: 'thinking' });
  pc.send({ type: 'answer', text: 'Six minutes for a soft yolk.', final: true, id: 'chat:2:1' });
  pc.send({ type: 'state', phase: 'idle' });
  await expect(page.getByTestId('answer')).toHaveText('Six minutes for a soft yolk.');
  await expect(page.getByTestId('read-aloud')).toHaveText('Read aloud');
  await expect(page.getByTestId('error')).toBeHidden();
  expect((await snapshot(page)).player.active).toBe(false);
});

test('what was typed first stays: the words after it, one space between, or at the caret', async ({ page }) => {
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  const input = page.getByTestId('input');
  await input.fill('Add to the shopping list:');
  await ask(page, pc, 300);
  words(pc, 'eggs, flour and two lemons');
  await expect(input).toHaveValue('Add to the shopping list: eggs, flour and two lemons');
  expect((await box(page)).start).toBe('Add to the shopping list: eggs, flour and two lemons'.length);

  // Ending in a space already: no second one.
  await input.fill('Note to self ');
  await ask(page, pc, 300);
  words(pc, 'the boiler service is on Tuesday');
  await expect(input).toHaveValue('Note to self the boiler service is on Tuesday');

  // The caret in the middle (put there, then Talk): the words go in there,
  // what's around them kept, the caret after them.
  await input.fill('Ask Sam about Saturday.');
  await input.evaluate((t) => t.setSelectionRange(7, 7));
  await ask(page, pc, 300);
  words(pc, 'and Robin');
  await expect(input).toHaveValue('Ask Sam and Robin about Saturday.');
  expect(await box(page)).toMatchObject({ start: 17, end: 17 });
  // Before a full stop: no space in front of it.
  await input.fill('Book the table for.');
  await input.evaluate((t) => t.setSelectionRange(18, 18));
  await ask(page, pc, 300);
  words(pc, 'four at seven');
  await expect(input).toHaveValue('Book the table for four at seven.');
  // A selection is never typed over: the words after it.
  await input.fill('Pick up the parcel');
  await input.evaluate((t) => t.setSelectionRange(8, 18));
  await ask(page, pc, 300);
  words(pc, 'from the post office');
  await expect(input).toHaveValue('Pick up the parcel from the post office');
  expect(pc.of('ask')).toHaveLength(0);
});

test('hold, then release: the words go in the box', async ({ page }) => {
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  const talk = await page.getByTestId('talk').boundingBox();
  await page.mouse.move(talk.x + talk.width / 2, talk.y + talk.height / 2);
  await page.mouse.down();
  await expect.poll(() => pc.of('start')).toEqual([{ type: 'start', mode: 'words' }]);
  // Letting go doesn't send: it finishes.
  await expect(page.getByTestId('talk')).toContainText('Release when done');
  await page.waitForTimeout(700);
  expect(pc.of('stop')).toHaveLength(0);
  await page.mouse.up();
  await expect.poll(() => pc.of('stop').length).toBe(1);
  await expect(page.getByTestId('composer')).toHaveAttribute('data-state', 'transcribing');
  await page.waitForTimeout(400);
  // The release's click doesn't start another.
  expect(pc.of('start')).toHaveLength(1);
  words(pc, 'What time is the recycling collected?');
  await expect(page.getByTestId('input')).toHaveValue('What time is the recycling collected?');
  await expect(page.getByTestId('turn')).toHaveCount(0);
  await expect(page.getByTestId('send')).toBeEnabled();
  expect(pc.of('ask')).toHaveLength(0);
});

test("nothing heard: the box keeps its words, and the page says so", async ({ page }) => {
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  const input = page.getByTestId('input');
  await input.fill('Draft a reply to the school');
  // The PC's own words for it.
  await ask(page, pc, 300);
  pc.send({ type: 'state', phase: 'transcribing' });
  pc.send({ type: 'error', message: NO_WORDS });
  pc.send({ type: 'state', phase: 'idle' });
  await expect(page.getByTestId('error')).toHaveText(NO_WORDS);
  await expect(input).toHaveValue('Draft a reply to the school');
  await expect(page.getByTestId('composer')).toHaveAttribute('data-state', 'idle');
  await expect(page.getByTestId('send')).toBeEnabled();
  // The PC done with nothing said at all: the page says the same.
  await ask(page, pc, 300);
  await expect(page.getByTestId('error')).toBeHidden();
  pc.send({ type: 'state', phase: 'transcribing' });
  pc.send({ type: 'state', phase: 'idle' });
  await expect(page.getByTestId('error')).toHaveText(NO_WORDS);
  await expect(input).toHaveValue('Draft a reply to the school');
  // An empty `words` too.
  await ask(page, pc, 300);
  pc.send({ type: 'state', phase: 'transcribing' });
  pc.send({ type: 'words', text: '  ' });
  pc.send({ type: 'state', phase: 'idle' });
  await expect(page.getByTestId('error')).toHaveText(NO_WORDS);
  await expect(input).toHaveValue('Draft a reply to the school');
  expect(pc.of('ask')).toHaveLength(0);
  await expect(page.getByTestId('turn')).toHaveCount(0);
});

test('✕ while it transcribes: cancel, the box as it was, the late words dropped', async ({ page }) => {
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  const input = page.getByTestId('input');
  await input.fill('Keep this part');
  await ask(page, pc, 300);
  await expect(page.getByTestId('composer')).toHaveAttribute('data-state', 'transcribing');
  await page.getByTestId('cancel').click();
  await expect.poll(() => pc.of('cancel').length).toBe(1);
  await expect(page.getByTestId('composer')).toHaveAttribute('data-state', 'idle');
  await expect(input).toBeVisible();
  await expect(input).toHaveValue('Keep this part');
  // Already on the way when ✕ was tapped: dropped; then the cancel's idle.
  pc.send({ type: 'state', phase: 'transcribing' });
  pc.send({ type: 'words', text: 'and not these words' });
  pc.send({ type: 'state', phase: 'idle' });
  pc.send({ type: 'state', phase: 'idle' });
  await page.waitForTimeout(300);
  await expect(input).toHaveValue('Keep this part');
  await expect(page.getByTestId('error')).toBeHidden();
  // Talk works again.
  await ask(page, pc, 300);
  words(pc, 'and this');
  await expect(input).toHaveValue('Keep this part and this');
});

test('the pictures wait in the composer at Done, and go with Send', async ({ page }) => {
  const pc = await mockPc(page, {
    picker: pickerData({ model: 'opus' }),
    model: { id: 'anthropic|claude-opus-5-5', label: 'Claude Opus 5.5 · Anthropic', onThisPc: false },
  });
  await open(page);
  await ready(page, pc);
  await choosePhotos(page, [await photo(page, { w: 800, h: 600 })]);
  await expect(page.getByTestId('shot')).toHaveCount(1);
  await ask(page, pc, 300);
  // Done sends no pictures: they're still in the card while it transcribes.
  expect(pc.of('stop').at(-1)).toEqual({ type: 'stop' });
  expect(pc.of('picture')).toHaveLength(0);
  words(pc, 'Which bird is this?');
  await expect(page.getByTestId('input')).toHaveValue('Which bird is this?');
  await expect(page.getByTestId('shot')).toHaveCount(1);
  // Send: the picture, then the question with it (said: Talk's words).
  await page.getByTestId('send').click();
  await expect.poll(() => pc.of('ask')).toEqual([{ type: 'ask', text: 'Which bird is this?', pictures: 1, speak: true }]);
  expect(pc.pictures).toHaveLength(1);
  expect(pc.pictures[0].bytes.length).toBe(pc.pictures[0].size);
  await expect(page.getByTestId('shot')).toHaveCount(0);
  await expect(page.getByTestId('turn').last().getByTestId('turn-pictures').locator('img')).toHaveCount(1);
});

test('the PC ends it itself (its 2 minutes): the page stops listening, the words in the box', async ({ page }) => {
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await page.getByTestId('talk').click();
  await expect.poll(() => pc.of('start').length).toBe(1);
  pc.send({ type: 'state', phase: 'listening' });
  await expect.poll(() => pc.frames.length).toBeGreaterThan(10);
  words(pc, 'Everything said until then');
  await expect(page.getByTestId('input')).toHaveValue('Everything said until then');
  expect((await snapshot(page)).local).toBe('idle');
  await expect(page.getByTestId('composer')).toHaveAttribute('data-state', 'idle');
  // The mic let go as after a Done (Clearer sound, on by default).
  await expect.poll(async () => (await snapshot(page)).micOpen).toBe(false);
  // No stop needed: the PC had it.
  await page.waitForTimeout(500);
  expect(pc.of('stop')).toHaveLength(0);
});

test('the mic gone mid-talk: what it heard goes in the box, and the page says so', async ({ page }) => {
  await page.addInitScript(() => {
    window.__tracks = [];
    const md = navigator.mediaDevices;
    const real = md.getUserMedia.bind(md);
    md.getUserMedia = async (c) => {
      const s = await real(c);
      window.__tracks.push(...s.getAudioTracks());
      return s;
    };
  });
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await page.getByTestId('talk').click();
  await expect.poll(() => pc.frames.length, { timeout: 5000 }).toBeGreaterThan(10);
  // The headset switched off: its track ends.
  await page.evaluate(() => window.__tracks.at(-1).dispatchEvent(new Event('ended')));
  await expect.poll(() => pc.of('stop').length).toBe(1);
  await expect(page.getByTestId('note')).toHaveText(
    'The microphone went away (was the headset switched off?). What it heard is in the box.',
  );
  words(pc, 'Is the leisure centre open on bank holidays?');
  await expect(page.getByTestId('input')).toHaveValue('Is the leisure centre open on bank holidays?');
  expect(pc.of('ask')).toHaveLength(0);
});

test('hands-free on: Done sends at once, the question in the thread, answered out loud', async ({ page }) => {
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await openSettings(page);
  // The hint says what the switch changes.
  await expect(page.locator('#handsfree-hint')).toHaveText(
    'On: sends when you stop talking, and Yap answers out loud. Off: your words go in the box to check first.',
  );
  await page.getByText('Hands-free').click();
  await expect(page.getByTestId('handsfree')).toBeChecked();
  await closeSettings(page);
  await page.getByTestId('input').fill('typed before');
  await page.getByTestId('talk').click();
  await expect.poll(() => pc.of('start')).toEqual([{ type: 'start', mode: 'ask' }]);
  // The question's bubble while it listens, Done the ↑ of a send.
  await expect(page.getByTestId('turn')).toHaveCount(1);
  await expect(page.getByTestId('composer').locator('.done-icon')).toBeVisible();
  await expect.poll(() => pc.frames.length, { timeout: 5000 }).toBeGreaterThan(10);
  await page.getByTestId('talk').click();
  await expect.poll(() => pc.of('stop').length).toBe(1);
  await expect(page.getByTestId('phase')).toHaveText('Working it out');
  pc.send({ type: 'state', phase: 'transcribing' });
  pc.send({ type: 'heard', text: 'Is it going to rain this afternoon?' });
  pc.send({ type: 'state', phase: 'thinking' });
  pc.send({ type: 'answer', text: 'Light showers after 3 pm.', final: true, id: 'chat:6:1' });
  pc.send({ type: 'state', phase: 'speaking' });
  pc.send({ type: 'speak_start', rate: 24000 });
  for (let i = 0; i < 4; i++) pc.sendBinary(downFrame(i, voice(24000, 100)));
  pc.send({ type: 'speak_end' });
  pc.send({ type: 'state', phase: 'idle' });
  await expect(page.getByTestId('heard')).toHaveText('Is it going to rain this afternoon?');
  await expect(page.getByRole('img', { name: 'Asked by voice' })).toHaveCount(1);
  await expect.poll(async () => (await snapshot(page)).player.played, { timeout: 4000 }).toBeGreaterThan(0.3);
  // What was typed stays where it was.
  await expect(page.getByTestId('input')).toHaveValue('typed before');
  expect(pc.of('ask')).toHaveLength(0);
});

for (const [name, opts] of [
  ['a PC from before words in the box', { can: CAN_BEFORE_WORDS }],
  ['a PC from before typed questions', { old: true }],
]) {
  test(`${name}: Done sends, as before`, async ({ page }) => {
    const pc = await mockPc(page, opts);
    await open(page);
    await ready(page, pc);
    await openSettings(page);
    await expect(page.locator('#handsfree-hint')).toHaveText('Sends when you stop talking');
    await closeSettings(page);
    await ask(page, pc, 300);
    expect(pc.of('start')).toEqual([{ type: 'start', mode: 'ask' }]);
    await expect(page.getByTestId('turn')).toHaveCount(1);
    await expect(page.getByTestId('phase')).toHaveText('Working it out');
    pc.send({ type: 'heard', text: 'How far is the lighthouse walk?' });
    await expect(page.getByTestId('heard')).toHaveText('How far is the lighthouse walk?');
  });
}

for (const [label, size] of [
  ['at the largest text (200 %)', { width: 390, height: 844, text: 200 }],
  ['on its side', { width: 844, height: 390, text: 100 }],
]) {
  test(`${label}: the transcribing capsule and the words in the box still fit`, async ({ page }) => {
    await page.setViewportSize({ width: size.width, height: size.height });
    const pc = await mockPc(page);
    await open(page);
    await ready(page, pc);
    await page.evaluate((s) => (document.documentElement.style.fontSize = `${s}%`), size.text);
    const fits = () => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth);
    await ask(page, pc, 300);
    await expect(page.getByTestId('composer')).toHaveAttribute('data-state', 'transcribing');
    expect(await fits()).toBe(true);
    const cancel = await page.getByTestId('cancel').boundingBox();
    const label = await page.locator('#live-label').boundingBox();
    expect(cancel.height).toBeGreaterThanOrEqual(44);
    expect(label.width).toBeGreaterThan(40);
    expect(label.x + label.width).toBeLessThanOrEqual(size.width);
    words(pc, 'Find me a quiet café near the station with good wifi and somewhere to sit outside');
    await expect(page.getByTestId('input')).toHaveValue(/^Find me a quiet café/);
    expect(await fits()).toBe(true);
    // The box grown to show them all (they came while it was the capsule).
    expect(await page.getByTestId('input').evaluate((t) => [t.clientHeight > 60, t.scrollHeight <= t.clientHeight + 1])).toEqual([true, true]);
    const send = await page.getByTestId('send').boundingBox();
    const talk = await page.getByTestId('talk').boundingBox();
    expect(send.x + send.width).toBeLessThanOrEqual(size.width);
    expect(send.y + send.height).toBeLessThanOrEqual(size.height);
    expect(talk.y + talk.height).toBeLessThanOrEqual(size.height);
    await expect(page.getByTestId('send')).toBeEnabled();
  });
}
