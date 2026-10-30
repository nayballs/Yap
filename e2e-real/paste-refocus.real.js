// The exact text box (src-tauri/src/pasting/selection.rs `keep_focus`,
// `follow_focus`, `restore_focus`). At a dictation's start Yap keeps the
// element with the focus; at its paste:
//
// - the focus wandered off within the window to something that takes no
//   text (a click on a button while you talk) → it's put back on that box,
//   and the paste lands there;
// - you clicked into another text box → the paste goes there, fitted to
//   the text around its cursor (read afresh);
// - you went to another window → as before: the dictation's window comes
//   back to the front and the paste goes into its box.
//
// `robot.dictateMoving(target, text, move)` starts a dictation, runs
// `move()` while "you talk", then pastes.
//
// Run alone (it takes the keyboard focus for a minute):
//   npm run test:real:build && npx playwright test -c e2e-real paste-refocus
import { test, expect } from './support/robot.js';
import { edgeExe } from './support/targets.js';

test('a click on a button while you talk: the paste puts the focus back in the box', async ({ robot }) => {
  const { pad } = robot;
  await pad.set('box', '');
  const r = await robot.dictateMoving(pad, 'Back in the box.', () => pad.call({ cmd: 'focus', field: 'button' }));
  expect(r.leftOnClipboard).toBe(false);
  await expect.poll(() => pad.text('box')).toBe('Back in the box.');
  expect((await robot.lastPaste()).refocus).toBe('putBack');
});

test('another text box while you talk: the paste goes there, fitted to its own text', async ({ robot }) => {
  const { pad } = robot;
  await pad.set('rich', 'Dear Sam,');
  await pad.set('box', '');
  const r = await robot.dictateMoving(pad, 'The plan is fine.', () => pad.call({ cmd: 'focus', field: 'rich' }));
  expect(r.leftOnClipboard).toBe(false);
  // Read afresh there: mid-sentence after "Dear Sam," (the box it started
  // in was empty, which would have given "The plan is fine.").
  expect(r.text).toBe(' the plan is fine.');
  await expect.poll(() => pad.text('rich')).toBe('Dear Sam, the plan is fine.');
  expect(await pad.text('box')).toBe('');
  expect((await robot.lastPaste()).refocus).toBe('stay');
});

test('nothing moved: the paste goes where it always did, nothing refocused', async ({ robot }) => {
  const { pad } = robot;
  await pad.set('box', '');
  const r = await robot.dictateMoving(pad, 'Right where I was.', async () => {});
  expect(r.leftOnClipboard).toBe(false);
  await expect.poll(() => pad.text('box')).toBe('Right where I was.');
  expect((await robot.lastPaste()).refocus).toBe('stay');
});

test.describe('a web page', () => {
  test.skip(!edgeExe(), 'Microsoft Edge is not installed');

  test("a click on the page's button while you talk: back in its text box for the paste", async ({ robot }) => {
    const page = await robot.page('<textarea id="box" aria-label="Message"></textarea><button id="send">Send</button>');
    const r = await robot.dictateMoving(page, 'Back in the page box.', () =>
      page.evaluate(() => document.getElementById('send').focus()),
    );
    expect(r.leftOnClipboard).toBe(false);
    await expect.poll(() => page.evaluate(() => document.getElementById('box').value)).toBe(r.text);
    expect((await robot.lastPaste()).refocus).toBe('putBack');
  });

  // The submit key (text_injector.rs `press_submit`, `submit_goes`): a
  // window that comes up over the dictation's after the paste — a dialog,
  // a popup — never gets the Enter meant for the message box.
  test('a window that comes up after the paste never gets the submit key', async ({ robot }) => {
    const page = await robot.page(`<textarea id="box" aria-label="Message"></textarea>
<script>
  window.enters = 0;
  window.popupEnters = 0;
  const box = document.getElementById('box');
  box.addEventListener('keydown', (e) => { if (e.key === 'Enter') window.enters++; });
  // The paste opens a popup, which takes the front (a paste is a key press:
  // the page may open one).
  box.addEventListener('input', () => {
    if (window.pop) return;
    window.pop = window.open('', 'pop', 'popup,width=320,height=200,left=200,top=200');
    window.pop.document.write('<title>Popup</title><input id="p" autofocus>');
    window.pop.document.addEventListener('keydown', (e) => { if (e.key === 'Enter') window.popupEnters++; });
    window.pop.focus();
  });
</script>`);
    await robot.configure({ autoSubmit: true });
    try {
      await page.focus();
      const r = await robot.yap.invoke('realtest_dictate_text', { text: 'Ready when you are.', hwnd: page.hwnd });
      expect(r.leftOnClipboard).toBe(false);
      await expect.poll(() => page.evaluate(() => document.getElementById('box').value)).toBe(r.text);
      // The key went nowhere: not into the popup, not into the box behind it.
      expect(r.error).toMatch(/isn't in front|lost the focus|the test didn't open it/);
      expect(await page.evaluate(() => window.popupEnters)).toBe(0);
      expect(await page.evaluate(() => window.enters)).toBe(0);
    } finally {
      await page.evaluate(() => window.pop?.close()).catch(() => {});
      await robot.configure({ autoSubmit: false });
    }
  });

  test('another window while you talk: the dictation goes back to its own window and box, as before', async ({ robot }) => {
    const { pad } = robot;
    const page = await robot.editor();
    await page.set('');
    await pad.set('box', '');
    const r = await robot.dictateMoving(pad, 'Still for the pad.', () => page.focus());
    expect(r.leftOnClipboard).toBe(false);
    await expect.poll(() => pad.text('box')).toBe('Still for the pad.');
    expect(await page.value()).toBe('');
    expect((await robot.lastPaste()).refocus).toBe('stay');
  });
});
