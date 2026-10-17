// The robot on a dictation's extras: a paste into an app running as
// administrator (elevated.rs), and "Dictation cancelled · Undo" (pipeline.rs).
//
// A real elevated window needs a UAC prompt, which a robot can't (and
// mustn't) answer, so the admin app is a stand-in: `e2e_paste_elevated`
// makes every target count as one, and the rest is real — no keys go to the
// test pad, the text really is left on the clipboard (privately), and the
// main window says so. Yap's own check of a process's integrity level is
// unit-tested on its own process.
import { test, expect } from './support/robot.js';
import { edgeExe } from './support/targets.js';
import { speechWav } from './support/tts.js';

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

test('an admin app (a stand-in): no keys, the text left on the clipboard, the person told', async ({ robot, shot }) => {
  const { yap, pad } = robot;
  await pad.set('box', 'untouched');
  await pad.pastes(); // (forget earlier ones)
  await yap.invoke('e2e_paste_elevated', { on: true });
  try {
    const r = await robot.dictate(pad, 'Deploy it to staging.');
    expect(r.leftOnClipboard).toBe(true);
    expect(r.error ?? null).toBeNull();
    // Nothing reached the pad: no Ctrl+V, its text as it was.
    await sleep(800);
    expect(await pad.text('box')).toBe('untouched');
    expect(await pad.pastes()).toEqual([]);
    // The dictation (fitted to the pad's text, as a paste would be) is on
    // the clipboard, kept out of Win+V history.
    const clip = await robot.clipboard('check', r.text);
    expect(clip.matchesText).toBe(true);
    expect(clip.private).toBe(true);
    const toast = yap.main.getByRole('status').filter({ hasText: 'Your text is copied: press Ctrl+V' });
    await expect(toast).toBeVisible();
    await shot(yap.main, '01-paste-blocked-toast');

    // "Paste last transcript" into it: the same.
    await pad.focus('box');
    const last = await yap.invoke('paste_last_transcript');
    expect(last).toContain('eploy it to staging');
    await sleep(800);
    expect(await pad.text('box')).toBe('untouched');
    expect(await pad.pastes()).toEqual([]);
    expect((await robot.clipboard('check', last)).matchesText).toBe(true);
  } finally {
    await yap.invoke('e2e_paste_elevated', { on: false });
  }
  // Back to normal: the next dictation pastes.
  await pad.set('box', '');
  const r = await robot.dictate(pad, 'Done.');
  expect(r.leftOnClipboard).toBe(false);
  await expect.poll(() => pad.text('box')).toBe('Done.');
});

test('✕ then Undo: the cancelled dictation is transcribed and pasted where it started', async ({ robot }) => {
  const { yap, pad } = robot;
  await pad.set('box', '');
  const mark = robot.logMark();
  const r = await robot.dictateWav(pad, speechWav('Testing the undo button.'), { cancel: true });
  // Cancelled: nothing transcribed or pasted, the audio kept.
  expect(r).toBeNull();
  await sleep(500);
  expect(await pad.text('box')).toBe('');
  expect(robot.logSince(mark)).toContain('Recording cancelled (kept 10 s for Undo)');

  // Undo (the bar's button: `bar_action`), the pad still in front.
  await yap.invoke('realtest_delivered'); // (forget earlier reports)
  await yap.invoke('bar_action', { action: 'dictation-undo' });
  await expect.poll(() => pad.text('box'), { timeout: 15_000 }).toContain('STT stub: received');
  await expect.poll(() => yap.invoke('realtest_delivered').then((d) => d?.raw ?? null)).toMatch(/STT stub/);
  expect(robot.logSince(mark)).toContain('Undo: transcribing the cancelled dictation for its own window');
});

test('Undo pastes into the window the dictation started in, not the one in front', async ({ robot }) => {
  test.skip(!edgeExe(), 'Microsoft Edge is not installed');
  const { yap, pad } = robot;
  const page = await robot.editor();
  await page.set('');
  await pad.set('box', '');
  const r = await robot.dictateWav(pad, speechWav('Undo into the pad.'), { cancel: true });
  expect(r).toBeNull();
  // Another window (one the test opened) comes to the front meanwhile.
  await page.focus();
  await sleep(300);
  await yap.invoke('bar_action', { action: 'dictation-undo' });
  // It lands in the pad, which Yap brought back to the front for it — and
  // never in the page that was in front.
  await expect.poll(() => pad.text('box'), { timeout: 15_000 }).toContain('STT stub: received');
  expect(await page.value()).toBe('');
});
