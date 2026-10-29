// A paste that can't land (pasting/text_injector.rs `Left`): Windows didn't
// take the paste keys (SendInput put in fewer events than asked), nothing
// that takes text had the focus (pasting/selection.rs `focus_editable`), or
// no app read the text off the clipboard (pasting/read_check.rs). Either
// way the text stays on the clipboard and Yap says so once, through the
// notice policy: "Couldn't paste · it's copied, press Ctrl+V" / "Select a
// text box first". A test run pastes nothing and
// touches no clipboard: `e2e_paste_fails` makes every paste fail that way
// (the real keys, focus and clipboard are the robot's:
// e2e-real/paste-safety.real.js). Also `e2e_held_keys`, the robot's say on
// what the person holds (there's no keyboard hook in a test run).
import { test, expect, closeToasts, expectStore } from './support/fixtures.js';

test.use({ yapOptions: { name: 'paste-failures' } });

test.afterEach(async ({ yap }) => {
  await yap.invoke('e2e_paste_fails', { why: null }).catch(() => {});
});

const toast = (main, title) => main.getByRole('status').filter({ hasText: title });

test("Windows didn't take the keys: on the clipboard, said once, kept in history as not pasted", async ({ yap, main, shot }) => {
  await yap.invoke('e2e_paste_fails', { why: 'keysBlocked' });
  const r = await yap.invoke('e2e_dictation', { text: 'send the report to sam' });
  expect(r.leftOnClipboard).toBe(true);
  expect(r.leftWhy).toBe('keysBlocked');
  const t = toast(main, "Couldn't paste · it's copied, press Ctrl+V");
  await expect(t).toHaveCount(1);
  await expect(t).toContainText("Windows didn't let Yap press keys just then");
  await expect(t).toContainText('Paste'); // its chip
  await shot(main, '01-keys-blocked');
  // One message for one failure: nothing else said about it.
  await main.waitForTimeout(800);
  await expect(main.getByRole('status')).toHaveCount(1);
  await expectStore(yap, 'history.json', (h) =>
    h.some((e) => e.status === 'notPasted' && /Windows didn't take the paste keys/.test(e.error || '')),
  );
  await closeToasts(main);
});

test('nothing that takes text had the focus: "Select a text box first", the text copied', async ({ yap, main, shot }) => {
  await yap.invoke('e2e_paste_fails', { why: 'noTextBox' });
  const r = await yap.invoke('e2e_dictation', { text: 'book the room for friday' });
  expect(r.leftWhy).toBe('noTextBox');
  expect(r.submit).toBeNull();
  const t = toast(main, 'Select a text box first');
  await expect(t).toHaveCount(1);
  await expect(t).toContainText('click where it goes and press Ctrl+V');
  await shot(main, '02-select-a-text-box');
  await expectStore(yap, 'history.json', (h) => h.some((e) => /no text box had the focus/.test(e.error || '')));
  await closeToasts(main);

  // "Paste last transcript" says the same.
  expect(await yap.invoke('paste_last_transcript')).toMatch(/Book the room for friday/i);
  await expect(toast(main, 'Select a text box first')).toHaveCount(1);
  await closeToasts(main);

  // Pastes as usual again.
  await yap.invoke('e2e_paste_fails', { why: null });
  const ok = await yap.invoke('e2e_dictation', { text: 'and the projector' });
  expect(ok.leftOnClipboard).toBe(false);
  await main.waitForTimeout(500);
  await expect(main.getByRole('status')).toHaveCount(0);
});

// The clipboard-read check (pasting/read_check.rs): the keys went, but no
// app read the text off the clipboard within a second. The robot's
// e2e-real/paste-read.real.js and paste-late.real.js do it for real.
test("no app read the paste: on the clipboard, \"Couldn't paste\", kept in history as not pasted", async ({ yap, main, shot }) => {
  await yap.invoke('e2e_paste_fails', { why: 'notRead' });
  const r = await yap.invoke('e2e_dictation', { text: 'move the stand-up to ten' });
  expect(r.leftOnClipboard).toBe(true);
  expect(r.leftWhy).toBe('notRead');
  expect(r.submit).toBeNull();
  const t = toast(main, "Couldn't paste · it's copied, press Ctrl+V");
  await expect(t).toHaveCount(1);
  await expect(t).toContainText("The app didn't take the paste");
  await shot(main, '03-not-read');
  await main.waitForTimeout(800);
  await expect(main.getByRole('status')).toHaveCount(1);
  await expectStore(yap, 'history.json', (h) =>
    h.some((e) => e.status === 'notPasted' && /the app didn't read the paste/.test(e.error || '')),
  );
  await closeToasts(main);
});

test('the test hooks refuse what they don’t know', async ({ yap }) => {
  await expect(yap.invoke('e2e_paste_fails', { why: 'sometimes' })).rejects.toThrow(/Unknown failure/);
  await expect(yap.invoke('e2e_held_keys', { keys: ['hyper'] })).rejects.toThrow(/Unknown key/);
  await yap.invoke('e2e_held_keys', { keys: ['alt', 'shift'] });
  await yap.invoke('e2e_held_keys', { keys: null });
});
