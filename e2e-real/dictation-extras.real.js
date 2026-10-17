// The robot on a dictation's extras: a paste into an app running as
// administrator (elevated.rs).
//
// A real elevated window needs a UAC prompt, which a robot can't (and
// mustn't) answer, so the admin app is a stand-in: `e2e_paste_elevated`
// makes every target count as one, and the rest is real — no keys go to the
// test pad, the text really is left on the clipboard (privately), and the
// main window says so. Yap's own check of a process's integrity level is
// unit-tested on its own process.
import { test, expect } from './support/robot.js';

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
