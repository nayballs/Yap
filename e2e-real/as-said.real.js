// The robot on "Use what I said" (pasting/as_said.rs): right after a dictation AI
// cleanup changed (a stand-in model here), what Yap pasted is swapped for
// the words as said — selected exactly over UI Automation, then pasted over
// — in the test pad (a WinForms text box: EM_SETSEL), Notepad and an Edge
// text box (UI Automation's TextPattern) and a chat app (Edge's WhatsApp
// stand-in). Where that can't be done safely — a terminal, the text edited
// since, another window in front — nothing in the app changes: the words as
// said go on the clipboard instead (privately), and Yap says so. Later, from
// Home ("AI edited · N changes" → Paste what I said): the latest dictation
// swapped the same way, an earlier one copied.
import { test, expect, lf } from './support/robot.js';
import { edgeExe, hasWindowsTerminal, notepadBusy } from './support/targets.js';
import { startFakeLlm, cleanupTranscript } from '../e2e/support/fake-llm.js';

const SAID = 'um so send the report to sam by friday';
const CLEANED = 'Send the report to Sam by Friday.';
// An earlier dictation, for Home.
const EARLIER_SAID = 'um book the big room for tuesday';
const EARLIER_CLEANED = 'Book the big room for Tuesday.';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

let llm;
test.beforeAll(async ({ robot }) => {
  llm = await startFakeLlm();
  llm.reply('cleanup', (user) => (cleanupTranscript(user) === EARLIER_SAID ? EARLIER_CLEANED : CLEANED));
  // Styles by app off: the model's text as it is everywhere (a terminal's
  // command style would make it "send the report…"; cleanup-styles.real.js
  // has the styles).
  await robot.configure({ postProcessEnabled: true, ppProvider: 'custom', ppBaseUrl: llm.base, ppModel: 'fake-cleanup', ppApiKey: '', cleanupStyles: false });
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

test('later, from Home: "Paste what I said" swaps the latest dictation in its window; an earlier one is copied, the app left alone', async ({
  robot,
}) => {
  const { pad, yap } = robot;
  const main = yap.main;
  await pad.set('box', '');
  await robot.dictate(pad, EARLIER_SAID);
  await expect.poll(() => pad.text('box')).toBe(EARLIER_CLEANED);
  await pad.set('box', 'Note: ');
  await robot.dictate(pad, SAID);
  await expect.poll(() => pad.text('box')).toBe(`Note: ${CLEANED}`);

  // Home, its newest row first. (Clicked over DevTools: the pad keeps the
  // focus, as when Yap brings the dictation's window back to the front.)
  await main.getByRole('navigation', { name: 'Main' }).getByRole('button', { name: 'Home', exact: true }).click();
  // Home re-pulls its list a moment after a dictation is saved (60 ms,
  // HomeView's `refreshSoon`): wait for both new dictations at its top, or
  // the first row with this text is test 1's, an earlier dictation — which
  // Yap rightly only copies (failed every run from 2026-10-29 until this).
  const items = main.locator('.daylist .item');
  await expect(items.nth(0)).toContainText(CLEANED);
  await expect(items.nth(1)).toContainText(EARLIER_CLEANED);
  const row = (text) => items.filter({ hasText: text }).first();
  const latest = items.nth(0);
  await latest.getByRole('button', { name: 'AI edited · 1 change' }).click();
  await expect(latest.getByRole('region', { name: 'What AI cleanup changed' }).locator('del')).toHaveText(['taken out: um so']);
  await latest.getByRole('button', { name: 'Paste what I said' }).click();
  await expect.poll(() => pad.text('box')).toBe(`Note: ${SAID}`);
  await expect(main.getByRole('status').filter({ hasText: 'Swapped for what you said' })).toBeVisible();

  // An earlier dictation: never looked for in the app — copied, and said.
  const earlier = row(EARLIER_CLEANED);
  await earlier.getByRole('button', { name: 'AI edited · 1 change' }).click();
  await earlier.getByRole('button', { name: 'Paste what I said' }).click();
  await expect(main.getByRole('status').filter({ hasText: 'Copied what you said' })).toContainText(
    'Yap only swaps your latest dictation, right after it. Press Ctrl+V to paste it.'
  );
  await sleep(500);
  expect(await pad.text('box')).toBe(`Note: ${SAID}`);
  const clip = await robot.clipboard('check', EARLIER_SAID);
  expect(clip.matchesText).toBe(true);
  expect(clip.private).toBe(true);
});

test('the tray\'s "Paste last transcript (as said)" after a dictation cleanup left alone: the dictation pasted again, and Yap still answers', async ({
  robot,
}) => {
  const { pad, yap } = robot;
  // Cleanup gives the words back unchanged: no words as said, the case of
  // cleanup off (the default). It froze Yap: the last dictation's lock
  // taken twice (pasting/text_injector.rs), the tray's paste hung and everything
  // after it.
  llm.reply('cleanup', (user) => cleanupTranscript(user));
  try {
    await pad.set('box', '');
    const r = await robot.dictate(pad, 'Paste me as said.');
    expect(r.asSaid ?? null).toBeNull();
    await expect.poll(() => pad.text('box')).toBe('Paste me as said.');
    await pad.set('box', 'Once more: ');
    await pad.focus();
    const frozen = new Promise((resolve) => setTimeout(() => resolve('Yap froze'), 5_000));
    expect(await Promise.race([yap.invoke('paste_last_transcript', { asSaid: true }), frozen])).toBe('Paste me as said.');
    await expect.poll(() => pad.text('box')).toBe('Once more: Paste me as said.');
    // Still answering: the next dictation pastes.
    await pad.set('box', '');
    await robot.dictate(pad, 'Still here.');
    await expect.poll(() => pad.text('box')).toBe('Still here.');
  } finally {
    llm.reply('cleanup', (user) => (cleanupTranscript(user) === EARLIER_SAID ? EARLIER_CLEANED : CLEANED));
  }
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
