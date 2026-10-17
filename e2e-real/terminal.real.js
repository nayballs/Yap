// The robot in Windows Terminal: a PowerShell prompt (read back over UI
// Automation: the terminal's text), and a stand-in for Claude Code — its
// idle title "✳ …" and, like it, bracketed paste — that runs nothing and
// logs what arrives. Only harmless text: nothing but `echo` is ever run.
import crypto from 'node:crypto';
import { test, expect } from './support/robot.js';
import { hasWindowsTerminal } from './support/targets.js';

test.skip(!hasWindowsTerminal(), 'Windows Terminal (wt.exe) is not installed');

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** The terminal's lines, without the blank rows and padding. */
const lines = (sh) =>
  sh
    .text()
    .split(/\r?\n/)
    .map((l) => l.trimEnd())
    .filter(Boolean);

/** Once the PowerShell prompt is up (and has stayed put a moment). */
async function prompt(sh) {
  await expect.poll(() => lines(sh).at(-1), { message: 'the PowerShell prompt', timeout: 20_000 }).toBe('PS>');
  await sleep(500);
  await expect.poll(() => lines(sh).at(-1)).toBe('PS>');
}

test('a PowerShell prompt gets a dictation as one line, and nothing runs', async ({ robot }) => {
  const sh = await robot.shell();
  await prompt(sh);
  const r = await robot.dictate(sh, 'List the files:\n- the logs\n- the notes.');
  expect(r.kind).toBe('terminal (Windows Terminal)');
  expect(r.text).toBe('List the files: the logs; the notes.');
  expect(r.submit).toBeNull();
  await expect.poll(() => lines(sh).at(-1)).toBe('PS> List the files: the logs; the notes.');
  // Still the last line a moment later: not run (no error, no new prompt).
  await sleep(1_500);
  expect(lines(sh).at(-1)).toBe('PS> List the files: the logs; the notes.');
  expect(sh.text()).not.toMatch(/is not recognized|CommandNotFoundException/);
});

test('"press enter" at a PowerShell prompt runs it (a harmless echo)', async ({ robot }) => {
  const sh = await robot.shell();
  await prompt(sh);
  const token = `robot-${crypto.randomBytes(3).toString('hex')}`;
  const r = await robot.dictate(sh, `Echo ${token}, press enter.`);
  expect(r.text).toBe(`Echo ${token}.`);
  expect(r.submit).toBe('enter');
  // The command, its output on a line of its own, and a new prompt.
  await expect.poll(() => lines(sh).slice(-3)).toEqual([`PS> Echo ${token}.`, `${token}.`, 'PS>']);
});

test('Claude Code in Windows Terminal (its title): the line breaks kept, in one paste', async ({ robot }) => {
  const cli = await robot.claudeCode();
  const r = await robot.dictate(cli, 'Fix these:\n- the login bug\n- the slow search.');
  expect(r.kind).toBe('terminal (Claude Code in Windows Terminal)');
  expect(r.text).toBe('Fix these:\n- the login bug\n- the slow search.');
  expect(r.submit).toBeNull();
  await expect.poll(() => cli.received()).toContain('\x1b[201~');
  const got = cli.received();
  const paste = /\x1b\[200~([\s\S]*?)\x1b\[201~/.exec(got)?.[1];
  // One bracketed paste with its three lines…
  expect(paste?.split(/\r\n?|\n/)).toEqual(['Fix these:', '- the login bug', '- the slow search.']);
  // …and no Enter outside it: nothing sent.
  expect(got.replace(/\x1b\[200~[\s\S]*?\x1b\[201~/, '')).not.toMatch(/[\r\n]/);

  // "press enter" sends it: Enter after the paste.
  const r2 = await robot.dictate(cli, 'Run the tests, press enter.');
  expect(r2.submit).toBe('enter');
  await expect.poll(() => cli.received()).toMatch(/\x1b\[200~Run the tests\.\x1b\[201~\r/);
});

test('auto-submit: Enter in a coding CLI, never at a shell prompt', async ({ robot }) => {
  await robot.configure({ autoSubmit: true });
  try {
    const sh = await robot.shell();
    await prompt(sh);
    const r = await robot.dictate(sh, 'Show the status.');
    expect(r.submit).toBeNull();
    await expect.poll(() => lines(sh).at(-1)).toBe('PS> Show the status.');
    await sleep(1_500);
    expect(lines(sh).at(-1)).toBe('PS> Show the status.');

    const cli = await robot.claudeCode();
    const r2 = await robot.dictate(cli, 'Explain the build.');
    expect(r2.submit).toBe('enter');
    await expect.poll(() => cli.received()).toMatch(/\x1b\[200~Explain the build\.\x1b\[201~\r/);
  } finally {
    await robot.configure({ autoSubmit: false });
  }
});

test('with "Safer pasting into terminals" off, a terminal is just another app', async ({ robot }) => {
  await robot.configure({ terminalMode: false });
  try {
    const cli = await robot.claudeCode();
    const r = await robot.dictate(cli, 'One line.');
    expect(r.kind).toBe('other');
    // Smart spacing can't read a terminal: the text as it is.
    await expect.poll(() => cli.received()).toContain('One line.');
  } finally {
    await robot.configure({ terminalMode: true });
  }
});
