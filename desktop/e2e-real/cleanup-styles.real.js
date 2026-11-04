// The robot on **styles by app** (ai/cleanup_styles.rs): Yap tells the kind of
// app from the real window it pastes into — a web mail page by the site its
// title ends with, a chat page, a terminal and the coding CLI in it as
// pasting/app_kind.rs reads them, anything else — and the cleanup it asks for says
// so, just before the transcript (a stand-in model shows what it was told).
// What it pastes is fitted to the style: an email without a sign-off the
// model made up, a shell command without the capital and full stop a
// sentence would have — then the paste rules as ever (a chat message
// without its full stop, a terminal's one line).
import path from 'node:path';
import { test, expect } from './support/robot.js';
import { edgeExe, hasWindowsTerminal, startPage } from './support/targets.js';
import { startFakeLlm, cleanupTranscript } from '../e2e/support/fake-llm.js';

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

let llm;
test.beforeAll(async ({ robot }) => {
  llm = await startFakeLlm();
  await robot.configure({
    postProcessEnabled: true,
    ppProvider: 'custom',
    ppBaseUrl: llm.base,
    ppModel: 'fake-cleanup',
    ppApiKey: '',
    cleanupStyles: true,
  });
});

test.afterAll(async ({ robot }) => {
  await robot.configure({ postProcessEnabled: false, ppBaseUrl: '' }).catch(() => {});
  await llm?.close();
});

const lastCleanup = () => llm.requests.filter((r) => r.kind === 'cleanup').at(-1);

test('a web mail page: the email style, and a sign-off the model made up left out', async ({ robot }) => {
  test.skip(!edgeExe(), 'Microsoft Edge is not installed');
  // An Edge app window titled like Gmail's tab (the nonce first: the robot's
  // guard wants it in every window's title).
  const mail = await robot.track(
    await startPage({ nonce: robot.nonce, dir: path.join(robot.dir, `mail-${robot.targets.length}`), title: `${robot.nonce} - Gmail` }),
  );
  llm.reply('cleanup', () => 'Hi Sam,\n\nThank you for the figures.\n\nBest regards,\n[Your Name]');
  try {
    await mail.set('');
    const r = await robot.dictate(mail, 'hi sam thanks for the figures');
    expect(r.style).toBe('email/formal');
    expect(lastCleanup().user).toContain('This dictation is the body of an email. Tone: formal and polished');
    await expect.poll(() => mail.value()).toBe('Hi Sam,\n\nThank you for the figures.');
  } finally {
    llm.reply('cleanup', cleanupTranscript);
  }
});

test('a chat page: the personal chat style, then its paste rule (no full stop)', async ({ robot }) => {
  test.skip(!edgeExe(), 'Microsoft Edge is not installed');
  const chat = await robot.chat();
  await chat.set('');
  llm.reply('cleanup', () => "Running five minutes late; I'll be there soon.");
  try {
    const r = await robot.dictate(chat, "running five minutes late i'll be there soon");
    expect(r.style).toBe('personalChat/casual');
    expect(r.kind).toBe('chat app');
    expect(lastCleanup().user).toContain('This dictation is a personal chat message. Tone: casual and friendly');
    // Casual: no semicolon; a chat app: no full stop.
    await expect.poll(() => chat.value()).toBe("Running five minutes late, I'll be there soon");
  } finally {
    llm.reply('cleanup', cleanupTranscript);
  }
});

test('an ordinary page: everything else, in your own words — no style line, as before', async ({ robot }) => {
  test.skip(!edgeExe(), 'Microsoft Edge is not installed');
  const page = await robot.editor();
  await page.set('');
  const r = await robot.dictate(page, 'The plan is fine.');
  expect(r.style).toBe('other/natural');
  expect(lastCleanup().user).not.toContain('This dictation is');
  await expect.poll(() => page.value()).toBe('The plan is fine.');
});

test('a shell: the command style, without the capital and full stop the model added', async ({ robot }) => {
  test.skip(!hasWindowsTerminal(), 'Windows Terminal (wt.exe) is not installed');
  const sh = await robot.shell();
  const lines = () =>
    sh
      .text()
      .split(/\r?\n/)
      .map((l) => l.trimEnd())
      .filter(Boolean);
  await expect.poll(() => lines().at(-1), { timeout: 20_000 }).toBe('PS>');
  await sleep(500);
  llm.reply('cleanup', () => 'Git status.');
  try {
    const r = await robot.dictate(sh, 'um git status');
    expect(r.style).toBe('terminal/command');
    expect(r.kind).toBe('terminal (Windows Terminal)');
    expect(lastCleanup().user).toContain('This dictation is a terminal command.');
    // Typed at the prompt, never run (no Enter without "press enter").
    await expect.poll(() => lines().at(-1)).toBe('PS> git status');
  } finally {
    llm.reply('cleanup', cleanupTranscript);
  }
});

test('Claude Code in a terminal: an AI prompt, word for word', async ({ robot }) => {
  test.skip(!hasWindowsTerminal(), 'Windows Terminal (wt.exe) is not installed');
  const cli = await robot.claudeCode();
  const r = await robot.dictate(cli, 'Explain why the build fails on Windows.');
  expect(r.style).toBe('aiPrompts/literal');
  expect(lastCleanup().user).toContain('This dictation is a prompt for an AI. Tone: literal');
  // Claude Code turns bracketed paste on: the words arrive inside its
  // markers (\e[200~ … \e[201~), as terminal.real.js reads them.
  await expect.poll(() => cli.received()).toContain('\x1b[201~');
  const got = cli.received();
  const paste = /\x1b\[200~([\s\S]*?)\x1b\[201~/.exec(got)?.[1];
  expect(paste).toBe('Explain why the build fails on Windows.');
  // …and nothing else: no Enter (nothing sent), no second paste.
  expect(got.replace(/\x1b\[200~[\s\S]*?\x1b\[201~/, '')).toBe('');
});
