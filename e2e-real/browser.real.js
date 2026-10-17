// The robot in Edge (app windows with throwaway profiles): a web page's text
// box, where most dictation lands, and a page titled like WhatsApp, which
// Yap treats as a chat app. What's asserted is the text box's value, read
// over the DevTools protocol.
import { test, expect } from './support/robot.js';
import { edgeExe } from './support/targets.js';

test.skip(!edgeExe(), 'Microsoft Edge is not installed');

test('a web page: fitted to the text either side of the cursor', async ({ robot }) => {
  const page = await robot.editor();
  // Mid-text: lower case, a space before it, no full stop before what follows.
  await page.set('I want  please', 7);
  let r = await robot.dictate(page, 'The blue one.');
  expect(r.kind).toBe('other');
  expect(r.text).toBe('the blue one');
  await expect.poll(() => page.value()).toBe('I want the blue one please');

  // At the end, after a sentence.
  await page.set('I checked it.');
  r = await robot.dictate(page, 'The rest is fine.');
  await expect.poll(() => page.value()).toBe('I checked it. The rest is fine.');

  // Replacing a selected word mid-sentence: no full stop.
  await page.set('We can meet Tuesday at noon.', 12, 19);
  r = await robot.dictate(page, 'Wednesday.');
  await expect.poll(() => page.value()).toBe('We can meet Wednesday at noon.');
});

// Found by this robot (2026-10-17): Chromium's text ranges are positions in
// the whole page, so reading 500 characters either side of the caret ran on
// past the box into the page's own text (here, words on the box's line).
// selection.rs keeps the read inside the focused field.
test('a web page: only the text box is read, never the page around it', async ({ robot }) => {
  const page = await robot.editor('', ['We talked about it', 'and then left']);
  // Empty: the dictation as it is (not " the plan" after the page's words).
  await page.set('');
  let r = await robot.dictate(page, 'The plan.');
  expect(r.text).toBe('The plan.');
  await expect.poll(() => page.value()).toBe('The plan.');

  // At the box's start: its capital, fitted to the box's own text after it.
  await page.set('and more', 0);
  r = await robot.dictate(page, 'The plan.');
  expect(r.text).toBe('The plan ');
  await expect.poll(() => page.value()).toBe('The plan and more');

  // At the box's end: its full stop (the page's "and then left" isn't the
  // sentence going on).
  await page.set('I want ');
  r = await robot.dictate(page, 'The blue one.');
  expect(r.text).toBe('the blue one.');
  await expect.poll(() => page.value()).toBe('I want the blue one.');
});

test('a chat app: a short message without its full stop, three sentences with it', async ({ robot }) => {
  // Smart spacing off: the rule on its own (the tests below have it on,
  // as it is by default).
  await robot.configure({ smartSpacing: false });
  try {
    const chat = await robot.chat();
    let r = await robot.dictate(chat, 'Sounds good.');
    expect(r.kind).toBe('chat app');
    expect(r.text).toBe('Sounds good');
    await expect.poll(() => chat.value()).toBe('Sounds good');

    // A question keeps its mark.
    await chat.set('');
    await robot.dictate(chat, 'Are you in?');
    await expect.poll(() => chat.value()).toBe('Are you in?');

    // Three sentences read as a message that ends properly.
    await chat.set('');
    r = await robot.dictate(chat, 'Thanks for the update. I will check it tomorrow. Talk soon.');
    await expect.poll(() => chat.value()).toBe('Thanks for the update. I will check it tomorrow. Talk soon.');

    // With "Leave the full stop off chat messages" off, it's kept.
    await robot.configure({ chatNoFullStop: false });
    await chat.set('');
    r = await robot.dictate(chat, 'Sounds good.');
    expect(r.kind).toBe('other');
    await expect.poll(() => chat.value()).toBe('Sounds good.');
  } finally {
    await robot.configure({ smartSpacing: true, chatNoFullStop: true });
  }
});

// Found by this robot (2026-10-17): Chromium reads an empty text box's text
// as its label ("Type a message", from aria-label), or as U+FFFC when it has
// none, so Yap took the empty message box for a draft: the chat full stop
// stayed and a space followed it ("Sounds good. "). selection.rs now reads
// such a box as empty (its value is "", its text only stands in).
test('an empty chat box in Edge reads as empty: no full stop, no space after it', async ({ robot }) => {
  const chat = await robot.chat();
  await chat.set('');
  const r = await robot.dictate(chat, 'Sounds good.');
  expect(r.kind).toBe('chat app');
  expect(r.text).toBe('Sounds good');
  await expect.poll(() => chat.value()).toBe('Sounds good');
});

// A box labelled by a <label> (no aria-label): Chromium names it after the
// label (UIA Name, and LabeledBy the label element) and reads it empty as
// that name or U+FFFC. Empty either way.
test('an empty box labelled by a <label> reads as empty', async ({ robot }) => {
  const chat = await robot.chat('<label for="box">Your reply</label>', { attrs: '' });
  await chat.set('');
  const r = await robot.dictate(chat, 'Sounds good.');
  expect(r.kind).toBe('chat app');
  expect(r.text).toBe('Sounds good');
  await expect.poll(() => chat.value()).toBe('Sounds good');
});

// The same in a contenteditable message box (WhatsApp Web's, Gmail's): an
// empty one reads as its label too.
test('an empty contenteditable message box reads as empty', async ({ robot }) => {
  const chat = await robot.chat('', { editable: true });
  await chat.set('');
  const r = await robot.dictate(chat, 'Sounds good.');
  expect(r.text).toBe('Sounds good');
  await expect.poll(() => chat.value()).toBe('Sounds good');

  // A draft there: the dictation follows it.
  await chat.set('I can do Friday');
  const next = await robot.dictate(chat, 'Or Monday.');
  expect(next.text).toBe(' or Monday');
  await expect.poll(() => chat.value()).toBe('I can do Friday or Monday');
});
