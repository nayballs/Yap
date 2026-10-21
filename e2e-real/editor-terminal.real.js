// VS Code's integrated terminal: the window is the editor's whatever has the
// focus, so Yap asks UI Automation whether the focus is xterm.js's input
// (class "xterm-helper-textarea", labelled "Terminal 1, <title>") and treats
// it as a terminal: one line, no space after it, no Enter of its own,
// Ctrl+V (VS Code's terminal paste). The stand-in is an Edge page with that
// input, which this spec's Yap counts as an editor (YAP_REALTEST_EDITORS,
// robot mode only); it runs nothing and shows what arrived. A real VS Code
// isn't driven here.
import { test, expect } from './support/robot.js';
import { edgeExe } from './support/targets.js';

test.skip(!edgeExe(), 'Microsoft Edge is not installed');
test.use({ robotOptions: { name: 'editor-terminal', env: { YAP_REALTEST_EDITORS: 'msedge' } } });

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

test("an editor's terminal gets a dictation as one line, no space after it, nothing sent", async ({ robot }) => {
  const term = await robot.editorTerminal('pwsh');
  const r = await robot.dictate(term, 'List the files:\n- the logs\n- the notes.');
  expect(r.kind).toBe("terminal (an editor's terminal)");
  expect(r.paste).toBe('Ctrl+V');
  expect(r.text).toBe('List the files: the logs; the notes.');
  expect(r.submit).toBeNull();
  await expect.poll(() => term.received()).toBe('List the files: the logs; the notes.');
  await sleep(1_000);
  expect(await term.received()).toBe('List the files: the logs; the notes.');
});

test('"press enter" and auto-submit there: Enter for the voice command, auto-submit only in a coding CLI', async ({ robot }) => {
  const shell = await robot.editorTerminal('pwsh');
  const r = await robot.dictate(shell, 'Show the status, press enter.');
  expect(r.submit).toBe('enter');
  await expect.poll(() => shell.received()).toBe('Show the status.\r');

  await robot.configure({ autoSubmit: true });
  try {
    // A plain shell: never auto-submitted.
    const r2 = await robot.dictate(shell, 'Explain the build.');
    expect(r2.submit).toBeNull();
    await expect.poll(() => shell.received()).toBe('Show the status.\rExplain the build.');
    // Claude Code in it (its title in the terminal's label): auto-submit sends it.
    const claude = await robot.editorTerminal('claude');
    const r3 = await robot.dictate(claude, 'Explain the build.');
    expect(r3.kind).toBe("terminal (Claude Code in an editor's terminal)");
    expect(r3.submit).toBe('enter');
    await expect.poll(() => claude.received()).toBe('Explain the build.\r');
  } finally {
    await robot.configure({ autoSubmit: false });
  }
});

test('the editor itself, beside its terminal, is an ordinary app', async ({ robot }) => {
  const page = await robot.editorTerminal('pwsh');
  // The window in front with the editor's input focused, not the terminal's.
  const editor = {
    ...page,
    async focus() {
      await page.focus();
      await page.focusEditor();
      await sleep(300); // Chromium's accessibility tree follows the focus
    },
  };
  const r = await robot.dictate(editor, 'Hello there.');
  expect(r.kind).toBe('other');
  await expect.poll(() => page.editorText()).toMatch(/^Hello there\. ?$/);
  expect(await page.received()).toBe('');
});
