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

test('a chat app: a short message without its full stop, three sentences with it', async ({ robot }) => {
  // Smart spacing off: the rule on its own (see the known gap below for
  // what Edge's empty message box does to it).
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

// Known gap, found by this robot (2026-10-17): Chromium reports an empty
// text box's text as its label ("Type a message", from aria-label) — or as
// U+FFFC when it has none — so Yap reads something "after the cursor" in an
// empty message box: the chat full stop stays and a space follows it. This
// pins today's result; once selection.rs treats such a box as empty it
// fails — then expect "Sounds good". (Chromium's ranges also move past the
// box into the page's text when Yap reads 500 characters either side; seen
// with `yaprobot caret`, and harmless while a block's line break separates
// them.)
test('known gap: an empty chat box in Edge reads as holding its label', async ({ robot }) => {
  const chat = await robot.chat();
  await chat.set('');
  const r = await robot.dictate(chat, 'Sounds good.');
  expect(r.text).toBe('Sounds good. ');
  await expect.poll(() => chat.value()).toBe('Sounds good. ');
});
