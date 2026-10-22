// The robot on "Use what I said" (as_said.rs): right after a dictation AI
// cleanup changed (a stand-in model here), what Yap pasted is swapped for
// the words as said — selected exactly over UI Automation, then pasted over
// — in the test pad (a WinForms text box: EM_SETSEL), Notepad and an Edge
// text box (UI Automation's TextPattern) and a chat app (Edge's WhatsApp
// stand-in). Where that can't be done safely — a terminal, the text edited
// since, another window in front — nothing in the app changes: the words as
// said go on the clipboard instead (privately), and Yap says so.
import { test, expect, lf } from './support/robot.js';
import { edgeExe, hasWindowsTerminal, notepadBusy } from './support/targets.js';
import { startFakeLlm } from '../e2e/support/fake-llm.js';

const SAID = 'um so send the report to sam by friday';
const CLEANED = 'Send the report to Sam by Friday.';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

let llm;
test.beforeAll(async ({ robot }) => {
  llm = await startFakeLlm();
  llm.reply('cleanup', () => CLEANED);
  await robot.configure({ postProcessEnabled: true, ppProvider: 'custom', ppBaseUrl: llm.base, ppModel: 'fake-cleanup', ppApiKey: '' });
});

test.afterAll(async ({ robot }) => {
  await robot.configure({ postProcessEnabled: false, ppBaseUrl: '' }).catch(() => {});
  await llm?.close();
});

/** "Use what I said", as its shortcut and the bar's button run it. */
const swap = (robot) => robot.yap.invoke('as_said_swap');

test('the test pad (a WinForms text box): the paste swapped for the words as said', async ({ robot }) => {
  const { pad } = robot;
  await pad.set('box', '');
  const r = await robot.dictate(pad, SAID);
  expect(r.text).toBe(CLEANED);
  expect(r.asSaid).toBe(SAID);
  await expect.poll(() => pad.text('box')).toBe(CLEANED);
  const out = await swap(robot);
  expect(out).toMatchObject({ outcome: 'swapped', how: 'edit', checked: true });
  await expect.poll(() => pad.text('box')).toBe(SAID);

  // After text that was there before: only the paste changes.
  await pad.set('box', 'Note: ');
  await robot.dictate(pad, SAID);
  await expect.poll(() => pad.text('box')).toBe(`Note: ${CLEANED}`);
  expect((await swap(robot)).outcome).toBe('swapped');
  await expect.poll(() => pad.text('box')).toBe(`Note: ${SAID}`);
  // Once per dictation.
  await expect(swap(robot)).rejects.toThrow(/Nothing to swap/);

  // The Yap bar's "Use what I said" (its button: `bar_action`), the pad
  // still in front.
  await pad.set('box', '');
  await robot.dictate(pad, SAID);
  await expect.poll(() => pad.text('box')).toBe(CLEANED);
  await robot.yap.invoke('bar_action', { action: 'as-said' });
  await expect.poll(() => pad.text('box')).toBe(SAID);
});

test('edited since: the app is left alone, the words as said copied instead', async ({ robot }) => {
  const { pad, yap } = robot;
  // The paste edited before the swap.
  await pad.set('box', '');
  await robot.dictate(pad, SAID);
  await expect.poll(() => pad.text('box')).toBe(CLEANED);
  await pad.set('box', 'Send the report to Pam by Friday.');
  let out = await swap(robot);
  expect(out.outcome).toBe('copied');
  expect(out.why).toMatch(/changed or moved/);
  await sleep(500);
  expect(await pad.text('box')).toBe('Send the report to Pam by Friday.');
  const clip = await robot.clipboard('check', SAID);
  expect(clip.matchesText).toBe(true);
  expect(clip.private).toBe(true);
  await expect(
    yap.main.getByRole('status').filter({ hasText: "Couldn't replace it here — copied the raw text instead" })
  ).toBeVisible();
});

test('another window in front: the app is left alone, the words as said copied instead', async ({ robot }) => {
  test.skip(!edgeExe(), 'Microsoft Edge is not installed');
  const { pad } = robot;
  await pad.set('box', '');
  await robot.dictate(pad, SAID);
  await expect.poll(() => pad.text('box')).toBe(CLEANED);
  const page = await robot.editor();
  await page.set('');
  await page.focus();
  await sleep(300);
  const out = await swap(robot);
  expect(out.outcome).toBe('copied');
  expect(out.why).toMatch(/another window/);
  await sleep(500);
  expect(await pad.text('box')).toBe(CLEANED);
  expect(await page.value()).toBe('');
  expect((await robot.clipboard('check', SAID)).matchesText).toBe(true);
});

test('Notepad: swapped through UI Automation (TextPattern)', async ({ robot }) => {
  const busy = notepadBusy();
  test.skip(!!busy, busy ?? '');
  const np = await robot.notepad();
  await robot.dictate(np, SAID);
  await expect.poll(() => lf(np.text()).trimEnd()).toBe(CLEANED);
  const out = await swap(robot);
  expect(out).toMatchObject({ outcome: 'swapped', checked: true });
  await expect.poll(() => lf(np.text()).trimEnd()).toBe(SAID);
});

test('an Edge text box and a chat app: swapped through UI Automation (TextPattern)', async ({ robot }) => {
  test.skip(!edgeExe(), 'Microsoft Edge is not installed');
  const page = await robot.editor();
  await page.set('');
  let r = await robot.dictate(page, SAID);
  expect(r.kind).toBe('other');
  await expect.poll(() => page.value()).toBe(CLEANED);
  let out = await swap(robot);
  expect(out).toMatchObject({ outcome: 'swapped', how: 'text', checked: true });
  await expect.poll(() => page.value()).toBe(SAID);

  // A chat app (the WhatsApp stand-in): its message without the full stop,
  // then as said.
  const chat = await robot.chat();
  await chat.set('');
  r = await robot.dictate(chat, SAID);
  expect(r.kind).toBe('chat app');
  expect(r.text).toBe('Send the report to Sam by Friday');
  await expect.poll(() => chat.value()).toBe('Send the report to Sam by Friday');
  out = await swap(robot);
  expect(out).toMatchObject({ outcome: 'swapped', how: 'text', checked: true });
  await expect.poll(() => chat.value()).toBe(SAID);
});

test("a contenteditable message box (WhatsApp Web's kind): swapped through UI Automation", async ({ robot }) => {
  test.skip(!edgeExe(), 'Microsoft Edge is not installed');
  const editable = await robot.chat('', { editable: true });
  await editable.set('');
  await robot.dictate(editable, SAID);
  await expect.poll(() => editable.value()).toBe('Send the report to Sam by Friday');
  const out = await swap(robot);
  expect(out).toMatchObject({ outcome: 'swapped', how: 'text', checked: true });
  await expect.poll(() => editable.value()).toBe(SAID);
});

test('a terminal: never selected or typed into — the words as said copied instead', async ({ robot }) => {
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
  const r = await robot.dictate(sh, SAID);
  expect(r.kind).toBe('terminal (Windows Terminal)');
  await expect.poll(() => lines().at(-1)).toBe(`PS> ${CLEANED}`);
  const out = await swap(robot);
  expect(out.outcome).toBe('copied');
  expect(out.why).toMatch(/terminal/);
  await sleep(800);
  expect(lines().at(-1)).toBe(`PS> ${CLEANED}`);
  const clip = await robot.clipboard('check', SAID);
  expect(clip.matchesText).toBe(true);
  expect(clip.private).toBe(true);
});
