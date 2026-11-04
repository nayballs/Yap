// The robot on Paste last transcript's and Copy last transcript's shortcuts
// (pasting/last.rs): each press sent as the input hook sends it
// (`e2e_shortcut`: a robot-mode Yap has no hook either — it must never see
// the developer's keys), then the real paste and the real clipboard.
//
// - Paste last: the latest dictation pasted again into the window in front,
//   fitted to its cursor, as the tray's item does.
// - Copy last: the latest dictation on the clipboard, kept out of Win+V
//   history (`realtest_clipboard` "check" reads the private formats), and
//   nothing typed anywhere.
import { test, expect, lf } from './support/robot.js';
import { edgeExe, notepadBusy } from './support/targets.js';

const SAID = 'Book the big room for Tuesday.';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

test("Paste last's shortcut: the latest dictation pasted again where you're typing", async ({ robot }) => {
  const { yap, pad } = robot;
  await pad.set('box', '');
  const r = await robot.dictate(pad, SAID);
  expect(r.text.trim()).toBe(SAID);
  await expect.poll(() => pad.text('box')).toBe(SAID);

  // Somewhere else in the pad's text: once more, after what's there.
  await pad.set('box', 'Again: ');
  await pad.focus('box');
  const mark = robot.logMark();
  await yap.invoke('e2e_shortcut', { name: 'paste-last' });
  await expect.poll(() => pad.text('box')).toBe(`Again: ${SAID}`);
  // (Said once the paste was checked.)
  await expect.poll(() => robot.logSince(mark)).toContain('Paste last transcript: its shortcut');
  // The log says how much, never what.
  expect(robot.logSince(mark)).not.toContain('big room');
});

test("Paste last's shortcut into Notepad (another app than the dictation's)", async ({ robot }) => {
  const busy = notepadBusy();
  test.skip(!!busy, busy ?? '');
  const { yap, pad } = robot;
  await pad.set('box', '');
  await robot.dictate(pad, SAID);
  await expect.poll(() => pad.text('box')).toBe(SAID);
  const notepad = await robot.notepad();
  await notepad.focus();
  await sleep(300);
  await yap.invoke('e2e_shortcut', { name: 'paste-last' });
  await expect.poll(() => lf(notepad.text()).trim()).toBe(SAID);
  // The pad, where it was dictated, left alone.
  expect(await pad.text('box')).toBe(SAID);
});

test("Paste last's shortcut into an Edge text box (another app than the dictation's)", async ({ robot }) => {
  test.skip(!edgeExe(), 'Microsoft Edge is not installed');
  const { yap, pad } = robot;
  await pad.set('box', '');
  await robot.dictate(pad, SAID);
  await expect.poll(() => pad.text('box')).toBe(SAID);
  const page = await robot.editor();
  await page.set('');
  await page.focus();
  await sleep(300);
  await yap.invoke('e2e_shortcut', { name: 'paste-last' });
  await expect.poll(() => page.value().then((v) => v.trim())).toBe(SAID);
  expect(await pad.text('box')).toBe(SAID);
});

test("Copy last's shortcut: on the clipboard, out of Win+V history, and nothing typed", async ({ robot }) => {
  const { yap, pad } = robot;
  await pad.set('box', '');
  await robot.dictate(pad, SAID);
  await expect.poll(() => pad.text('box')).toBe(SAID);
  await pad.set('box', 'untouched');
  await pad.focus('box');
  await pad.pastes(); // (forget earlier ones)
  await robot.clipboard('setText', 'something else');

  await yap.invoke('e2e_shortcut', { name: 'copy-last' });
  await expect.poll(async () => (await robot.clipboard('check', SAID)).matchesText).toBe(true);
  expect((await robot.clipboard('check', SAID)).private).toBe(true);
  await expect(yap.main.getByRole('status').filter({ hasText: 'Copied your last dictation' })).toBeVisible();
  // Nothing reached the pad.
  await sleep(600);
  expect(await pad.text('box')).toBe('untouched');
  expect(await pad.pastes()).toEqual([]);
});
