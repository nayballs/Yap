// The robot on pastes that can go wrong (pasting/text_injector.rs,
// pasting/modifiers.rs, pasting/selection.rs):
//
// - a modifier left down (an injected key the harness presses: down as far
//   as Windows knows, no finger on it): Yap lets go of it before Ctrl+V, so
//   the paste is a paste; one the person holds (`e2e_held_keys`: there's no
//   keyboard hook in a test run) is let go for the keys and pressed again;
// - nothing that takes text has the focus (the pad's button, a web page's
//   button, a page with nothing focused): the text stays on the clipboard
//   and Yap says "Select a text box first";
// - Windows doesn't take the keys (`e2e_paste_fails`: every SendInput
//   counts as refused): the text stays on the clipboard, said once;
// - edit mode's Ctrl+C with a busy clipboard (the pad holds it open, as a
//   clipboard manager would): waited for and put back exactly, every format;
//   held too long, left alone and said; held after the copy, said.
import { test, expect, waitForLog } from './support/robot.js';
import { keyState, modKey } from './support/targets.js';

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** The toasts in Yap's main window with `title`. */
const toasts = (robot, title) => robot.yap.main.getByRole('status').filter({ hasText: title });

test.describe('a modifier down before the paste', () => {
  test.afterEach(async ({ robot }) => {
    // Whatever happened, no key left down on this PC, and the hook's say back.
    for (const key of ['lalt', 'lshift', 'lctrl']) {
      if (keyState().includes(key)) modKey(robot.pad.hwnd, `pid:${robot.pad.pid}`, key, 'up');
    }
    await robot.yap.invoke('e2e_held_keys', { keys: null });
  });

  test('a stuck Alt is let go before Ctrl+V, and stays up', async ({ robot }) => {
    const { pad } = robot;
    await pad.set('box', '');
    await pad.focus('box');
    modKey(pad.hwnd, `pid:${pad.pid}`, 'lalt', 'down');
    expect(keyState()).toContain('lalt');
    const mark = robot.logMark();
    const r = await robot.dictate(pad, 'The meeting moved to Friday.');
    expect(r.error).toBeNull();
    // Pasted (not Ctrl+Alt+V, which the box ignores, then typed).
    expect(r.typed).toBe(false);
    await expect.poll(() => pad.text('box')).toBe('The meeting moved to Friday.');
    expect(keyState()).not.toContain('lalt');
    const log = robot.logSince(mark);
    expect(log).toContain("Modifiers down before Ctrl+V: let go of around Yap's keys");
    expect(log).toMatch(/stuck=Alt/);
  });

  test("a held Alt is let go for the keys and pressed again: never up under the person's finger", async ({ robot }) => {
    const { pad } = robot;
    await pad.set('box', '');
    await pad.focus('box');
    await robot.yap.invoke('e2e_held_keys', { keys: ['alt'] });
    modKey(pad.hwnd, `pid:${pad.pid}`, 'lalt', 'down');
    const r = await robot.dictate(pad, 'Ship it on Monday.');
    expect(r.typed).toBe(false);
    await expect.poll(() => pad.text('box')).toBe('Ship it on Monday.');
    // Down again, as the finger still is.
    expect(keyState()).toContain('lalt');
  });

  test('a stuck Shift is let go before the text is typed into a box that ignores Ctrl+V', async ({ robot }) => {
    const { pad } = robot;
    await pad.set('nopaste', '');
    await pad.focus('nopaste');
    modKey(pad.hwnd, `pid:${pad.pid}`, 'lshift', 'down');
    const r = await robot.dictate(pad, 'Lower case stays lower case.');
    // The paste never reaches this box: typed, and not in capitals.
    expect(r.typed).toBe(true);
    await expect.poll(() => pad.text('nopaste')).toBe('Lower case stays lower case.');
    expect(keyState()).not.toContain('lshift');
  });
});

test.describe('nothing that takes text has the focus', () => {
  test("the pad's button: nothing pasted, the text copied, \"Select a text box first\"", async ({ robot }) => {
    const { pad } = robot;
    await pad.set('box', 'Untouched.');
    await pad.focus('button');
    const r = await robot.dictate(pad, 'Send it to the team.');
    expect(r.leftOnClipboard).toBe(true);
    expect(r.leftWhy).toBe('noTextBox');
    expect(await pad.text('box')).toBe('Untouched.');
    const clip = await robot.clipboard('check', r.text);
    expect(clip.matchesText).toBe(true);
    expect(clip.private).toBe(true);
    await expect(toasts(robot, 'Select a text box first')).toHaveCount(1);
    await expect(toasts(robot, 'Select a text box first')).toContainText('press Ctrl+V');
  });

  test('a web page: its button, or nothing focused at all', async ({ robot }) => {
    const page = await robot.page('<p>The plan for the week.</p><button id="box">Send</button><textarea id="note"></textarea>');
    let r = await robot.dictate(page, 'Book the room.');
    expect(r.leftWhy).toBe('noTextBox');
    expect((await robot.clipboard('check', r.text)).matchesText).toBe(true);
    expect(await page.evaluate(() => document.getElementById('note').value)).toBe('');
    // The page itself has the focus (nothing in it does).
    await page.evaluate(() => document.activeElement.blur());
    await sleep(300);
    r = await robot.dictate(page, 'Book the room again.');
    expect(r.leftWhy).toBe('noTextBox');
    // A text box: pasted as ever.
    await page.evaluate(() => document.getElementById('note').focus());
    await sleep(300);
    r = await robot.dictate(page, 'Book the big room.');
    expect(r.leftOnClipboard).toBe(false);
    await expect.poll(() => page.evaluate(() => document.getElementById('note').value)).toBe('Book the big room.');
  });
});

test("Windows doesn't take the paste keys: the text stays copied, said once", async ({ robot }) => {
  const { pad } = robot;
  await pad.set('box', 'Before.');
  await robot.yap.invoke('e2e_paste_fails', { why: 'keysBlocked' });
  try {
    const r = await robot.dictate(pad, 'This stays on the clipboard.');
    expect(r.leftOnClipboard).toBe(true);
    expect(r.leftWhy).toBe('keysBlocked');
    expect(await pad.text('box')).toBe('Before.');
    expect((await robot.clipboard('check', r.text)).matchesText).toBe(true);
    const toast = toasts(robot, "Couldn't paste · it's copied, press Ctrl+V");
    await expect(toast).toHaveCount(1);
    await sleep(1_000);
    await expect(toast).toHaveCount(1);
  } finally {
    await robot.yap.invoke('e2e_paste_fails', { why: null });
  }
  // And the next dictation pastes as usual.
  const r = await robot.dictate(pad, 'Then this one lands.');
  expect(r.leftOnClipboard).toBe(false);
  await expect.poll(() => pad.text('box')).toBe('Before. Then this one lands.');
});

test.describe("edit mode's Ctrl+C with a busy clipboard", () => {
  /** Edit mode's Ctrl+C trick in the pad (the window in front). */
  const copySelection = (robot) =>
    robot.withFocus(robot.pad, true, async () => {
      await robot.pad.select('box', 4, 9); // "quarterly"
      return robot.yap.invoke('realtest_copy_selection', { hwnd: robot.pad.hwnd });
    });

  test.beforeEach(async ({ robot }) => {
    const { pad } = robot;
    await pad.set('box', 'The quarterly plan is ready.');
    // What was copied before: text as a word processor copies it.
    await robot.clipboard('setRich', 'What you had copied');
  });

  test('held a moment: waited for, the selection read, every format put back exactly', async ({ robot }) => {
    const before = (await robot.clipboard('fingerprint')).formats;
    expect(before.length).toBeGreaterThanOrEqual(3);
    expect(await robot.pad.holdClipboard(400)).toBe(true);
    const r = await copySelection(robot);
    expect(r).toEqual({ text: 'quarterly', trouble: null });
    expect((await robot.clipboard('fingerprint')).formats).toEqual(before);
    // The pad's text as it was.
    expect(await robot.pad.text('box')).toBe('The quarterly plan is ready.');
  });

  test('held the whole time: left alone, nothing pressed, and said', async ({ robot }) => {
    const before = (await robot.clipboard('fingerprint')).formats;
    expect(await robot.pad.holdClipboard(3_000)).toBe(true);
    const r = await copySelection(robot);
    expect(r).toEqual({ text: null, trouble: 'Busy' });
    await expect.poll(() => robot.pad.clipboardHeld(), { timeout: 5_000 }).toBe(false);
    expect((await robot.clipboard('fingerprint')).formats).toEqual(before);
    await expect(toasts(robot, "Couldn't read your selection")).toHaveCount(1);
  });

  test('held right after the copy: waited out when it lets go in time', async ({ robot }) => {
    const before = (await robot.clipboard('fingerprint')).formats;
    expect(await robot.pad.holdAfterCopy(600)).toBe(true);
    const r = await copySelection(robot);
    expect(r).toEqual({ text: 'quarterly', trouble: null });
    expect((await robot.clipboard('fingerprint')).formats).toEqual(before);
  });

  test("held after the copy for too long: it couldn't be put back, and that's said", async ({ robot }) => {
    expect(await robot.pad.holdAfterCopy(4_500)).toBe(true);
    const mark = robot.logMark();
    const r = await copySelection(robot);
    expect(r.trouble).toBe('NotRestored');
    await expect(toasts(robot, "Your clipboard couldn't be put back")).toHaveCount(1);
    await waitForLog(robot.yap, "Edit mode's Ctrl+C: the clipboard", { after: mark });
    await expect.poll(() => robot.pad.clipboardHeld(), { timeout: 8_000 }).toBe(false);
  });
});
