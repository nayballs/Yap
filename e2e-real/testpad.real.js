// The robot in the test pad (a WinForms window the harness opens: a text
// box, a rich text box, a password box, a box that ignores Ctrl+V): the
// guard, smart spacing, a lost paste typed instead, snippets, "press enter",
// Paste last transcript, the clipboard, AI cleanup.
// What's asserted is what actually landed in the pad's fields.
import crypto from 'node:crypto';
import { test, expect, lf } from './support/robot.js';
import { startTestPad } from './support/targets.js';
import { startFakeLlm, cleanupTranscript } from '../e2e/support/fake-llm.js';

test('the guard: Yap pastes only into the windows the robot opened', async ({ robot }) => {
  const { yap, pad } = robot;
  expect(await yap.invoke('realtest_window', { hwnd: pad.hwnd })).toBe('the test started its process');
  // Yap's own windows, never.
  const bar = await yap.invoke('bar_debug');
  await expect(yap.invoke('realtest_window', { hwnd: bar.hwnd })).rejects.toThrow(/one of Yap's own windows/);

  // A window the robot didn't hand Yap: a second pad, started after Yap (so
  // not in YAP_REALTEST_PIDS) and titled without the run's nonce.
  const stranger = await startTestPad(`stranger-${crypto.randomBytes(6).toString('hex')}`);
  try {
    await expect(yap.invoke('realtest_window', { hwnd: stranger.hwnd })).rejects.toThrow(/didn't open it/);
    await stranger.set('box', 'untouched');
    const mark = robot.logMark();
    const r = await robot.dictate(stranger, 'This must not land.', { retry: false });
    expect(r.error).toMatch(/^Robot mode refused to paste: the test didn't open it/);
    expect(await stranger.text('box')).toBe('untouched');
    // Its field wasn't read either, nor its title (an ordinary app to Yap).
    const log = robot.logSince(mark);
    expect(log).not.toMatch(/Read the text around the cursor|Couldn't read the text around the cursor/);
    expect(r.kind).toBe('other');
    // "Paste last transcript" with it in front: refused too.
    await stranger.focus();
    await expect(yap.invoke('paste_last_transcript')).rejects.toThrow(/Robot mode refused to paste the last transcript/);
    expect(await stranger.text('box')).toBe('untouched');
    // Said in Yap's window, and listed for the harness.
    await expect(yap.main.getByText(/Robot mode refused to paste/).first()).toBeVisible();
    expect((await yap.invoke('realtest_refusals')).length).toBeGreaterThanOrEqual(2);
  } finally {
    await stranger.close();
  }
});

test('smart spacing: fitted to the text before the cursor', async ({ robot }) => {
  const { pad } = robot;
  // Mid-sentence: lower case, a space before it.
  await pad.set('box', 'We went home');
  let r = await robot.dictate(pad, 'Because it was late.');
  expect(r.text).toBe(' because it was late.');
  await expect.poll(() => pad.text('box')).toBe('We went home because it was late.');

  // After a full sentence: its capital, a space before it.
  await pad.set('box', 'I checked it.');
  r = await robot.dictate(pad, 'The rest is fine.');
  await expect.poll(() => pad.text('box')).toBe('I checked it. The rest is fine.');

  // After a space: none added.
  await pad.set('box', 'We had dinner ');
  await robot.dictate(pad, 'And then we left.');
  await expect.poll(() => pad.text('box')).toBe('We had dinner and then we left.');

  // A name keeps its capital mid-sentence.
  await pad.set('box', 'I asked ');
  await robot.dictate(pad, 'Sarah said yes.');
  await expect.poll(() => pad.text('box')).toBe('I asked Sarah said yes.');

  // The same in the rich text box.
  await pad.set('rich', 'We went home');
  await robot.dictate(pad, 'Because it was late.');
  await expect.poll(() => pad.text('rich')).toBe('We went home because it was late.');
});

// A WinForms text box gives UI Automation a value but no caret (its focused
// element has no TextPattern), so Yap asks the edit control itself where the
// caret is (EM_GETSEL, selection.rs) — found by this robot (2026-10-17),
// when a dictation into the middle of a line was fitted as if at its end.
test('mid-text in a WinForms text box: fitted to the text either side of the caret', async ({ robot }) => {
  const { pad } = robot;
  await pad.set('box', 'I want  please', 7);
  const mark = robot.logMark();
  let r = await robot.dictate(pad, 'The blue one.');
  expect(r.text).toBe('the blue one');
  await expect.poll(() => pad.text('box')).toBe('I want the blue one please');
  expect(robot.logSince(mark)).toContain('Found the caret by asking the edit control');

  // Replacing a selection: its case and its spaces.
  await pad.set('box', 'Pick the red one please', 0);
  await pad.call({ cmd: 'select', field: 'box', start: 5, length: 11 });
  r = await robot.dictate(pad, 'The blue one.');
  expect(r.text).toBe('the blue one');
  await expect.poll(() => pad.text('box')).toBe('Pick the blue one please');

  // The rich text box, over a line break.
  await pad.set('rich', 'Dear Sam,\nThanks for  and the notes.', 21);
  r = await robot.dictate(pad, 'The draft.');
  expect(r.text).toBe('the draft');
  await expect.poll(async () => lf(await pad.text('rich'))).toBe('Dear Sam,\nThanks for the draft and the notes.');
});

test('a password box is pasted into but never read', async ({ robot }) => {
  const { pad } = robot;
  await pad.set('password', 'hunter2');
  const mark = robot.logMark();
  const r = await robot.dictate(pad, 'Typed in.');
  // Nothing was known about the field, so the dictation goes in as it is.
  expect(r.text).toBe('Typed in.');
  await expect.poll(() => pad.text('password')).toBe('hunter2Typed in.');
  const log = robot.logSince(mark);
  expect(log).toContain("Couldn't read the text around the cursor");
  expect(log).not.toContain('Read the text around the cursor');
});

// Verify-after-paste: a paste into a Win32 edit control that never shows up
// (a busy clipboard, an app ignoring Ctrl+V — now and then the pad's own box
// lost one) is typed instead, once. The "nopaste" box has its keyboard
// shortcuts off: Ctrl+V does nothing there, typing works.
test('a paste that never lands in a text box is typed instead, once', async ({ robot }) => {
  const { pad } = robot;
  await pad.set('nopaste', '');
  const mark = robot.logMark();
  const r = await robot.dictate(pad, 'Ship it tomorrow.');
  expect(r.paste).toBe('Ctrl+V');
  expect(r.typed).toBe(true);
  await expect.poll(() => pad.text('nopaste')).toBe('Ship it tomorrow.');
  expect(robot.logSince(mark)).toContain("The paste didn't reach the field");
  // Still once a moment later: typed a single time.
  await new Promise((resolve) => setTimeout(resolve, 1_500));
  expect(await pad.text('nopaste')).toBe('Ship it tomorrow.');

  // A box that takes the paste: pasted, never typed as well.
  await pad.set('box', '');
  const r2 = await robot.dictate(pad, 'Hello there.');
  expect(r2.typed).toBe(false);
  await expect.poll(() => pad.text('box')).toBe('Hello there.');
  await new Promise((resolve) => setTimeout(resolve, 1_200));
  expect(await pad.text('box')).toBe('Hello there.');
});

test('snippets: a sign-off on its lines, its full stop dropped; a link mid-sentence keeps the sentence\'s', async ({ robot }) => {
  const { pad } = robot;
  await robot.configure({
    dictionary: [
      { from: 'my sign off', to: 'Best regards,\nNathan', fuzzy: false, snippet: true },
      { from: 'the docs link', to: 'https://example.com/docs', fuzzy: false, snippet: true },
    ],
  });
  try {
    await pad.set('rich', '');
    let r = await robot.dictate(pad, 'Thanks for the update. My sign off.');
    expect(r.text).toBe('Thanks for the update. Best regards,\nNathan');
    await expect.poll(async () => lf(await pad.text('rich'))).toBe('Thanks for the update. Best regards,\nNathan');

    // Said on its own: exactly the saved text.
    await pad.set('rich', '');
    r = await robot.dictate(pad, 'My sign off.');
    await expect.poll(async () => lf(await pad.text('rich'))).toBe('Best regards,\nNathan');

    // A one-line snippet mid-sentence: the sentence keeps its full stop.
    await pad.set('box', '');
    r = await robot.dictate(pad, 'Read the docs link.');
    await expect.poll(() => pad.text('box')).toBe('Read https://example.com/docs.');
  } finally {
    await robot.configure({ dictionary: [] });
  }
});

test('"press enter": the text, then Enter in the field', async ({ robot }) => {
  const { pad } = robot;
  await pad.set('box', '');
  const r = await robot.dictate(pad, 'First line, press enter.');
  expect(r.text).toBe('First line.');
  expect(r.submit).toBe('enter');
  await expect.poll(async () => lf(await pad.text('box'))).toBe('First line.\n');

  // Only "press enter": just the key.
  await robot.dictate(pad, 'Press enter.');
  await expect.poll(async () => lf(await pad.text('box'))).toBe('First line.\n\n');
});

test('Paste last transcript pastes the last dictation again', async ({ robot }) => {
  const { yap, pad } = robot;
  await pad.set('box', '');
  await robot.dictate(pad, 'Paste me again.');
  await expect.poll(() => pad.text('box')).toBe('Paste me again.');
  await pad.set('box', 'Once more: ');
  await pad.focus();
  // Fitted to the field it lands in, as a dictation is.
  expect(await yap.invoke('paste_last_transcript')).toBe('Paste me again.');
  await expect.poll(() => pad.text('box')).toBe('Once more: Paste me again.');
});

test('the clipboard: yours comes back, the dictation never stays, and it was private while pasted', async ({ robot }) => {
  const { pad } = robot;
  // Text on the clipboard (the robot's, put there privately).
  await robot.clipboard('setText', 'robot clipboard text');
  await pad.set('box', '');
  await pad.pastes(); // forget earlier pastes
  await robot.dictate(pad, 'Clipboard check.');
  await expect.poll(() => pad.text('box')).toBe('Clipboard check.');
  expect((await robot.clipboard('check', 'robot clipboard text')).matchesText).toBe(true);
  expect((await robot.clipboard('check', 'Clipboard check.')).matchesText).toBe(false);
  // At the pad's Ctrl+V the clipboard held the dictation with every
  // keep-it-out-of-history format.
  const pastes = await pad.pastes();
  expect(pastes).toHaveLength(1);
  expect(pastes[0].private).toEqual({
    ExcludeClipboardContentFromMonitorProcessing: true,
    CanIncludeInClipboardHistory: true,
    CanUploadToCloudClipboard: true,
    'Clipboard Viewer Ignore': true,
  });
  // …and what came back is kept out of Win+V history too.
  expect((await robot.clipboard('check')).private).toBe(true);

  // An image comes back byte for byte.
  await robot.clipboard('setImage');
  await pad.set('box', '');
  await robot.dictate(pad, 'Image check.');
  await expect.poll(() => pad.text('box')).toBe('Image check.');
  const after = await robot.clipboard('check');
  expect(after.matchesImage).toBe(true);
  expect(after.hasText).toBe(false);
});

test('AI cleanup (a stand-in model): its text is pasted, and "press enter" it dropped still sends', async ({ robot }) => {
  const { pad } = robot;
  const llm = await startFakeLlm();
  // A model that capitalises, punctuates and drops the command.
  llm.reply('cleanup', (user) => {
    const t = cleanupTranscript(user).replace(/,?\s*press enter\.?$/i, '');
    return `${t.charAt(0).toUpperCase()}${t.slice(1)}.`;
  });
  await robot.configure({
    postProcessEnabled: true,
    ppProvider: 'custom',
    ppBaseUrl: llm.base,
    ppModel: 'fake-cleanup',
    ppApiKey: '',
  });
  try {
    await pad.set('box', '');
    const r = await robot.dictate(pad, 'send the report press enter');
    expect(llm.requests.filter((q) => q.kind === 'cleanup')).toHaveLength(1);
    expect(r.raw).toBe('send the report press enter');
    expect(r.text).toBe('Send the report.');
    expect(r.submit).toBe('enter');
    await expect.poll(async () => lf(await pad.text('box'))).toBe('Send the report.\n');
  } finally {
    await robot.configure({ postProcessEnabled: false, ppBaseUrl: '' });
    await llm.close();
  }
});
