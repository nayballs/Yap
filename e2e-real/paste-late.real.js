// A paste read late (src-tauri/src/pasting/read_check.rs). Before the
// clipboard-read check, your clipboard came back half a second after the
// paste keys whatever had happened, so an app that read the clipboard later
// than that pasted your OLD clipboard instead of the dictation. Now it comes
// back only once the target has read the dictation; and when nothing reads
// it within a second, it never comes back (the dictation stays copied, and
// that's said), so a late read still gets the dictation — noted as late.
//
// The slow reader is the test pad's "slow" box (no edit control: the read
// is what decides there): its Ctrl+V reads the clipboard `slowPaste(ms)`
// later, or never.
//
// Run alone (it takes the keyboard focus for a minute):
//   npm run test:real:build && npx playwright test -c e2e-real paste-late
import { test, expect } from './support/robot.js';

const COULDNT = "Couldn't paste · it's copied, press Ctrl+V";
/** The toasts in Yap's main window with `title`. */
const toasts = (robot, title) => robot.yap.main.getByRole('status').filter({ hasText: title });

test.afterEach(async ({ robot }) => {
  await robot.pad.slowPaste(0);
});

test('read 0.7 s after the keys: the dictation lands (not your old clipboard), and yours comes back after', async ({ robot }) => {
  const { pad } = robot;
  await robot.clipboard('setText', 'What you had copied');
  await pad.slowPaste(700);
  await pad.set('slow', '');
  const r = await robot.dictate(pad, 'Read late but right.');
  expect(r.leftOnClipboard).toBe(false);
  await expect.poll(() => pad.text('slow')).toBe(r.text);
  const lp = await robot.lastPaste();
  expect(lp).toMatchObject({ reader: 'target', verdict: 'pasted', late: false });
  expect(lp.readMs).toBeGreaterThanOrEqual(600);
  await expect.poll(async () => (await robot.lastPaste()).restored).toBe(true);
  expect((await robot.clipboard('check', 'What you had copied')).matchesText).toBe(true);
  // Still just the dictation in the box, once yours is back.
  expect(await pad.text('slow')).toBe(r.text);
});

test('read 2 s after the keys: "Couldn\'t paste" at a second, and the late read still gets the dictation', async ({ robot }) => {
  const { pad } = robot;
  await robot.clipboard('setText', 'What you had copied');
  await pad.slowPaste(2_000);
  await pad.set('slow', '');
  const r = await robot.dictate(pad, 'Late, and still mine.');
  expect(r.leftOnClipboard).toBe(true);
  expect(r.leftWhy).toBe('notRead');
  await expect(toasts(robot, COULDNT)).toHaveCount(1);
  // Never "What you had copied".
  await expect.poll(() => pad.text('slow'), { timeout: 5_000 }).toBe(r.text);
  await expect.poll(async () => (await robot.lastPaste()).late).toBe(true);
  expect((await robot.lastPaste()).lateTarget).toBe(true);
  // You were told it's copied: it is, and yours isn't put back over it.
  const clip = await robot.clipboard('check', r.text);
  expect(clip.matchesText).toBe(true);
  expect(clip.private).toBe(true);
});

test('never read: copied, said once, and nothing lands', async ({ robot }) => {
  const { pad } = robot;
  await pad.slowPaste(-1);
  await pad.set('slow', '');
  const r = await robot.dictate(pad, 'Nobody read this.');
  expect(r.leftWhy).toBe('notRead');
  expect(r.submit).toBeNull();
  expect(await pad.text('slow')).toBe('');
  const clip = await robot.clipboard('check', r.text);
  expect(clip.matchesText).toBe(true);
  expect(clip.private).toBe(true);
  await expect(toasts(robot, COULDNT)).toHaveCount(1);
  await expect(toasts(robot, COULDNT)).toContainText("The app didn't take the paste");
  expect(await robot.lastPaste()).toMatchObject({ verdict: 'notRead', reader: null, late: false });
});
