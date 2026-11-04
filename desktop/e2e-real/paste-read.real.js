// The clipboard-read check (src-tauri/src/pasting/read_check.rs): a
// dictation goes on the clipboard *promised* (delayed rendering), so Yap's
// own window is asked for the text the moment an app reads it. That read is
// the paste's proof: the target read it → it landed, and your clipboard
// comes back half a second after the read; nothing read it within a second
// → "Couldn't paste · it's copied, press Ctrl+V". A read by another app (a
// clipboard manager) never counts: refused before the keys, and never
// taken for the target's after them. `robot.lastPaste()` says who read it.
//
// Run alone (it takes the keyboard focus for a couple of minutes):
//   npm run test:real:build && npx playwright test -c e2e-real paste-read
// The Electron test needs a runtime (YAP_ROBOT_ELECTRON=<electron.exe>, or
// `npm i --no-save electron`); the VS Code one needs VS Code. Both skip
// without them.
import { test, expect } from './support/robot.js';
import { edgeExe, electronExe, hasWindowsTerminal, notepadBusy, startClipReader, vscodeExe } from './support/targets.js';

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const COULDNT = "Couldn't paste · it's copied, press Ctrl+V";
/** The toasts in Yap's main window with `title`. */
const toasts = (robot, title) => robot.yap.main.getByRole('status').filter({ hasText: title });

test.afterEach(async ({ robot }) => {
  await robot.yap.invoke('e2e_paste_fails', { why: null });
});

test('the test pad: its box reads the paste, and your clipboard comes back after the read', async ({ robot }) => {
  const { pad } = robot;
  await robot.clipboard('setText', 'What you had copied');
  await pad.set('box', '');
  const r = await robot.dictate(pad, 'The read says it landed.');
  expect(r.leftOnClipboard).toBe(false);
  await expect.poll(() => pad.text('box')).toBe('The read says it landed.');
  const lp = await robot.lastPaste();
  expect(lp).toMatchObject({ offered: true, reader: 'target', sure: true, verdict: 'pasted' });
  await expect.poll(async () => (await robot.lastPaste()).restored).toBe(true);
  expect((await robot.clipboard('check', 'What you had copied')).matchesText).toBe(true);
  // What came back is kept out of Win+V history, as ever.
  expect((await robot.clipboard('check')).private).toBe(true);
});

test("a clipboard manager's read never counts: refused before the keys, and the paste lands", async ({ robot }) => {
  const { pad } = robot;
  // (A test value on the clipboard: the stand-in sees only the test's own.)
  await robot.clipboard('setText', 'What you had copied');
  await pad.set('box', '');
  // It listens 3 s, then says what it read (`result()` waits for that).
  const manager = await startClipReader({ ms: 3_000, delay: 0 });
  let lp;
  try {
    await robot.dictate(pad, 'Not for the manager.');
    await expect.poll(() => pad.text('box')).toBe('Not for the manager.');
    lp = await robot.lastPaste();
  } catch (e) {
    manager.stop();
    throw e;
  }
  expect(lp.reader).toBe('target');
  expect(lp.refused).toBeGreaterThanOrEqual(1);
  const seen = await manager.result();
  // Its read as the dictation went on the clipboard got nothing.
  expect(seen?.reads?.[0]).toMatchObject({ open: true, got: false });
});

test('a clipboard manager reading late, after the keys: it gets the text (a paste is never broken), the target is who counts', async ({ robot }) => {
  const { pad } = robot;
  await robot.clipboard('setText', 'What you had copied');
  await pad.set('box', '');
  const manager = await startClipReader({ ms: 3_000, delay: 400 });
  let lp;
  try {
    await robot.dictate(pad, 'The box reads it first.');
    await expect.poll(() => pad.text('box')).toBe('The box reads it first.');
    lp = await robot.lastPaste();
  } catch (e) {
    manager.stop();
    throw e;
  }
  expect(lp.reader).toBe('target');
  const seen = await manager.result();
  expect(seen?.reads?.[0]).toMatchObject({ open: true, got: true });
});

test("robot mode's stand-in for an app that never reads: an edit control is typed into, as before", async ({ robot }) => {
  const { pad } = robot;
  await pad.set('box', '');
  await robot.yap.invoke('e2e_paste_fails', { why: 'notRead' });
  const r = await robot.dictate(pad, 'Typed instead.');
  // The box's own check (unchanged for a second) decides there.
  expect(r.typed).toBe(true);
  await expect.poll(() => pad.text('box')).toBe('Typed instead.');
  expect((await robot.lastPaste()).verdict).toBe('typed');
});

test.describe('Edge', () => {
  test.skip(!edgeExe(), 'Microsoft Edge is not installed');

  // Edge's browser process reads the clipboard with no window of its own
  // (the robot, 2026-10-29): a nameless read after the keys, taken for the
  // target's. It reads on any Ctrl+V — a page that swallows the key, a
  // button focused — so in Edge the read says the keys arrived, not that
  // the page took them; "Select a text box first" still decides there
  // (paste-safety.real.js).
  test("a web page's text box: Edge's browser process reads it, and that counts", async ({ robot }) => {
    const page = await robot.editor();
    await page.set('');
    const r = await robot.dictate(page, 'Read by the browser.');
    expect(r.leftOnClipboard).toBe(false);
    await expect.poll(() => page.value()).toBe('Read by the browser.');
    const lp = await robot.lastPaste();
    expect(['target', 'unknown']).toContain(lp.reader);
    expect(lp.verdict).toBe('pasted');
  });

  test("robot mode's stand-in for an app that never reads: a web page's box gets nothing, it's copied, said once", async ({ robot }) => {
    const page = await robot.editor();
    await page.set('');
    await robot.yap.invoke('e2e_paste_fails', { why: 'notRead' });
    const r = await robot.dictate(page, 'Still copied.');
    expect(r.leftWhy).toBe('notRead');
    expect(await page.value()).toBe('');
    expect((await robot.clipboard('check', r.text)).matchesText).toBe(true);
    await expect(toasts(robot, COULDNT)).toHaveCount(1);
    await sleep(800);
    await expect(toasts(robot, COULDNT)).toHaveCount(1);
  });
});

test('Notepad: its read counts and the text lands', async ({ robot }) => {
  const busy = notepadBusy();
  test.skip(!!busy, busy ?? '');
  const np = await robot.notepad();
  const r = await robot.dictate(np, 'Notepad read this.');
  expect(r.leftOnClipboard).toBe(false);
  await expect.poll(() => np.text()).toContain('Notepad read this.');
  expect(await robot.lastPaste()).toMatchObject({ reader: 'target', verdict: 'pasted' });
});

test.describe('terminals', () => {
  test.skip(!hasWindowsTerminal(), 'Windows Terminal (wt.exe) is not installed');

  /** The terminal's lines, without the blank rows and padding. */
  const lines = (sh) =>
    sh
      .text()
      .split(/\r?\n/)
      .map((l) => l.trimEnd())
      .filter(Boolean);

  test('Windows Terminal reads with no window of its own: after the keys, that counts', async ({ robot }) => {
    const sh = await robot.shell();
    await expect.poll(() => lines(sh).at(-1), { timeout: 20_000 }).toBe('PS>');
    await sleep(500);
    const r = await robot.dictate(sh, 'Read by the terminal.');
    expect(r.leftOnClipboard).toBe(false);
    await expect.poll(() => lines(sh).at(-1)).toBe('PS> Read by the terminal.');
    const lp = await robot.lastPaste();
    expect(['target', 'unknown']).toContain(lp.reader);
    expect(lp.verdict).toBe('pasted');
  });

  test("a classic console's own Paste reads it", async ({ robot }) => {
    const cli = await robot.consoleCli('read-conhost', { title: `Yap robot raw ${robot.nonce}` });
    const r = await robot.dictate(cli, 'Read by the console.');
    expect(r.leftOnClipboard).toBe(false);
    await expect.poll(() => cli.received()).toContain('Read by the console.');
    const lp = await robot.lastPaste();
    expect(['target', 'unknown']).toContain(lp.reader);
    expect(lp.verdict).toBe('pasted');
  });

  test("a long paste into Claude Code's stand-in: each slice read before the next goes on", async ({ robot }) => {
    const cli = await robot.claudeCode('read-slices');
    const words = Array.from({ length: 260 }, (_, i) => `word${i}`).join(' ');
    const r = await robot.dictate(cli, `${words}.`);
    expect(r.leftOnClipboard).toBe(false);
    await expect.poll(() => cli.received().replace(/\x1b\[20[01]~/g, ''), { timeout: 20_000 }).toContain('word259.');
    // The last slice's read: the terminal's.
    const lp = await robot.lastPaste();
    expect(['target', 'unknown']).toContain(lp.reader);
    expect(lp.verdict).toBe('pasted');
  });
});

test.describe('an Electron app', () => {
  test.skip(!electronExe(), 'No Electron runtime: set YAP_ROBOT_ELECTRON to an electron.exe, or `npm i --no-save electron`');

  // Chromium inside, like Edge: its read may come with no window.
  test('its main process reads the paste, and that counts', async ({ robot }) => {
    const app = await robot.electron();
    await app.set('');
    const r = await robot.dictate(app, 'Electron read this.');
    expect(r.leftOnClipboard).toBe(false);
    await expect.poll(() => app.value()).toBe('Electron read this.');
    const lp = await robot.lastPaste();
    expect(['target', 'unknown']).toContain(lp.reader);
    expect(lp.verdict).toBe('pasted');
  });
});

test.describe('VS Code', () => {
  test.skip(!vscodeExe(), 'VS Code is not installed');

  test('its editor: the read counts and the text lands', async ({ robot }) => {
    const code = await robot.vscode();
    const r = await robot.dictate(code, 'Code read this.');
    expect(r.leftOnClipboard).toBe(false);
    const lp = await robot.lastPaste();
    expect(['target', 'unknown']).toContain(lp.reader);
    expect(lp.verdict).toBe('pasted');
    await expect.poll(() => code.text(), { timeout: 15_000 }).toContain('Code read this.');
  });
});
