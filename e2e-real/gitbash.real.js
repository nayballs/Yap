// The robot in Git Bash's terminal, mintty: Ctrl+V doesn't paste there (it
// reaches the program as ^V — bash's "insert the next key as is"), so Yap
// pastes with Shift+Insert, mintty's own paste. The window runs nothing:
// a raw reader logs every byte that arrives. Skipped without Git for
// Windows.
import { test, expect } from './support/robot.js';
import { minttyExe } from './support/targets.js';

test.skip(!minttyExe(), 'Git Bash (mintty) is not installed');

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

test('Git Bash pastes with Shift+Insert: the dictation arrives as one line, no ^V, nothing sent', async ({ robot }) => {
  const sh = await robot.mintty();
  const r = await robot.dictate(sh, 'List the files:\n- the logs\n- the notes.');
  expect(r.kind).toBe('terminal (mintty)');
  expect(r.paste).toBe('Shift+Insert');
  expect(r.text).toBe('List the files: the logs; the notes.');
  expect(r.submit).toBeNull();
  await expect.poll(() => sh.received()).toBe('List the files: the logs; the notes.');
  // A moment later: nothing more (no Enter, no ^V from a Ctrl+V).
  await sleep(1_000);
  expect(sh.received()).toBe('List the files: the logs; the notes.');
});

test('"press enter" in Git Bash: the text, then Enter', async ({ robot }) => {
  const sh = await robot.mintty();
  const r = await robot.dictate(sh, 'Show the status, press enter.');
  expect(r.text).toBe('Show the status.');
  expect(r.submit).toBe('enter');
  await expect.poll(() => sh.received()).toBe('Show the status.\r');
});

test('with "Safer pasting into terminals" off, Git Bash still gets Shift+Insert', async ({ robot }) => {
  await robot.configure({ terminalMode: false });
  try {
    const sh = await robot.mintty();
    const r = await robot.dictate(sh, 'One line.');
    // Fitted like any app, but pasted with the terminal's own keys.
    expect(r.kind).toBe('other');
    expect(r.paste).toBe('Shift+Insert');
    await expect.poll(() => sh.received()).toContain('One line.');
    expect(sh.received()).not.toContain('\x16');
  } finally {
    await robot.configure({ terminalMode: true });
  }
});
