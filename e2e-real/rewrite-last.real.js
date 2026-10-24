// The robot on rewriting the last dictation (as_said.rs, pipeline.rs
// `run_agent`, 2026-10-23): the rewrite shortcut with nothing selected,
// within a minute of a dictation into the same window, applies the spoken
// instruction to that dictation — found exactly as Yap pasted it with the
// cursor right after it, selected over UI Automation, the AI's answer
// pasted over it — in the test pad (a WinForms box: EM_SETSEL), an Edge
// text box and a chat app's contenteditable (TextPattern). The rewrite is
// then the last paste ("now shorter" works on it). Typed since, or the
// cursor moved away: nothing pasted, the AI not asked, and Yap says so.
// Something selected, another window, a terminal: the rewrite shortcut as
// before (the selection rewritten, or new text written at the cursor), and
// so is everything with Settings → Commands' "Rewrite your last dictation
// when nothing is selected" off. The minute itself is the unit tests'
// (`as_said::rewritable`).
//
// `realtest_rewrite_text` stands in for the shortcut and the speech: the
// window in front, its selection read as at the key press (no Ctrl+C in
// robot mode), the instruction as heard. A stand-in AI model answers
// (e2e/support/fake-llm.js: the cleanup, and the rewrite by instruction).
import { test, expect } from './support/robot.js';
import { edgeExe, hasWindowsTerminal } from './support/targets.js';
import { startFakeLlm, cleanupTranscript, rewriteText, rewriteInstruction } from '../e2e/support/fake-llm.js';

const SAID = 'hey can you send me the report by friday';
const CLEANED = 'Hey, can you send me the report by Friday?';
const FORMAL = 'Could you please send me the report by Friday?';
const SHORTER = 'Report by Friday, please.';
const WRITTEN = 'Thanks for your help.';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

let llm;
test.beforeAll(async ({ robot }) => {
  llm = await startFakeLlm();
  llm.reply('cleanup', (user) => (cleanupTranscript(user) === SAID ? CLEANED : cleanupTranscript(user)));
  llm.reply('rewrite', (user) => {
    const asked = rewriteInstruction(user);
    if (/formal/i.test(asked)) return FORMAL;
    if (/shorter/i.test(asked)) return SHORTER;
    return WRITTEN;
  });
  // Styles by app off: the model's text as it is everywhere (a terminal's
  // command style would make it "hey, can you…"; cleanup-styles.real.js has
  // the styles).
  await robot.configure({ postProcessEnabled: true, ppProvider: 'custom', ppBaseUrl: llm.base, ppModel: 'fake-cleanup', ppApiKey: '', cleanupStyles: false });
});

test.afterAll(async ({ robot }) => {
  await robot.configure({ postProcessEnabled: false, ppBaseUrl: '' }).catch(() => {});
  await llm?.close();
});

/** The rewrite shortcut in `target` (brought to the front), `instruction` as what was said. */
const rewrite = (robot, target, instruction) =>
  robot.withFocus(target, true, () => robot.yap.invoke('realtest_rewrite_text', { text: instruction, hwnd: target.hwnd }));

/** What the AI was last asked to rewrite, and how. */
function lastAsked() {
  const req = llm.requests.filter((r) => r.kind === 'rewrite').at(-1);
  return req && { text: rewriteText(req.system), instruction: rewriteInstruction(req.user) };
}

const rewrites = () => llm.requests.filter((r) => r.kind === 'rewrite').length;

test('the test pad: "make it more formal" with nothing selected rewrites the dictation where it is, then "shorter" rewrites that', async ({
  robot,
}) => {
  const { pad } = robot;
  await pad.set('box', '');
  const r = await robot.dictate(pad, SAID);
  expect(r.text).toBe(CLEANED);
  await expect.poll(() => pad.text('box')).toBe(CLEANED);

  let out = await rewrite(robot, pad, 'make it more formal');
  expect(out).toMatchObject({ last: true, outcome: 'replaced', text: FORMAL });
  await expect.poll(() => pad.text('box')).toBe(FORMAL);
  // The AI got the dictation as the text to change.
  expect(lastAsked()).toEqual({ text: CLEANED, instruction: 'make it more formal' });

  // The rewrite is the last paste now: again, with nothing selected.
  out = await rewrite(robot, pad, 'now make it shorter');
  expect(out).toMatchObject({ last: true, outcome: 'replaced', text: SHORTER });
  await expect.poll(() => pad.text('box')).toBe(SHORTER);
  expect(lastAsked().text).toBe(FORMAL);

  // After text that was there before: only the dictation changes.
  await pad.set('box', 'Note: ');
  await robot.dictate(pad, SAID);
  await expect.poll(() => pad.text('box')).toBe(`Note: ${CLEANED}`);
  out = await rewrite(robot, pad, 'more formal please');
  expect(out.outcome).toBe('replaced');
  await expect.poll(() => pad.text('box')).toBe(`Note: ${FORMAL}`);
});

test('typed since, or the cursor moved away: nothing pasted, the AI not asked, and Yap says so', async ({ robot }) => {
  const { pad, yap } = robot;
  // Typed after it.
  await pad.set('box', '');
  await robot.dictate(pad, SAID);
  await expect.poll(() => pad.text('box')).toBe(CLEANED);
  await pad.set('box', `${CLEANED} Thanks.`);
  let asked = rewrites();
  let out = await rewrite(robot, pad, 'make it more formal');
  expect(out).toMatchObject({ last: true, outcome: 'refused' });
  expect(out.why).toMatch(/changed, or the cursor isn't right after it/);
  expect(rewrites()).toBe(asked);
  await sleep(500);
  expect(await pad.text('box')).toBe(`${CLEANED} Thanks.`);
  await expect(yap.main.getByRole('status').filter({ hasText: "Couldn't rewrite your last dictation" })).toBeVisible();

  // Untouched, but the cursor moved to the start: "it" may be something
  // else now — refused too (Use what I said would still find it there).
  await pad.set('box', '');
  await robot.dictate(pad, SAID);
  await expect.poll(() => pad.text('box')).toBe(CLEANED);
  await pad.set('box', CLEANED, 0);
  asked = rewrites();
  out = await rewrite(robot, pad, 'make it more formal');
  expect(out.outcome).toBe('refused');
  expect(rewrites()).toBe(asked);
  await sleep(500);
  expect(await pad.text('box')).toBe(CLEANED);
});

test('an Edge text box and a chat app\'s contenteditable: rewritten through UI Automation (TextPattern)', async ({ robot }) => {
  test.skip(!edgeExe(), 'Microsoft Edge is not installed');
  const page = await robot.editor();
  await page.set('');
  await robot.dictate(page, SAID);
  await expect.poll(() => page.value()).toBe(CLEANED);
  let out = await rewrite(robot, page, 'make it more formal');
  expect(out).toMatchObject({ last: true, outcome: 'replaced' });
  await expect.poll(() => page.value()).toBe(FORMAL);

  // WhatsApp Web's kind of box.
  const editable = await robot.chat('', { editable: true });
  await editable.set('');
  await robot.dictate(editable, SAID);
  await expect.poll(() => editable.value()).toBe(CLEANED);
  out = await rewrite(robot, editable, 'make it more formal');
  expect(out).toMatchObject({ last: true, outcome: 'replaced' });
  await expect.poll(() => editable.value()).toBe(FORMAL);
});

test('something selected, or another window: the rewrite shortcut as before', async ({ robot }) => {
  test.skip(!edgeExe(), 'Microsoft Edge is not installed');
  const { pad } = robot;
  // A selection wins over the last dictation: only the selected words.
  const page = await robot.editor();
  await page.set('');
  await robot.dictate(page, SAID);
  await expect.poll(() => page.value()).toBe(CLEANED);
  const from = CLEANED.indexOf('send me the report');
  await page.set(CLEANED, from, from + 'send me the report'.length);
  let out = await rewrite(robot, page, 'make it more formal');
  expect(out).toMatchObject({ last: false, outcome: 'pasted', text: FORMAL });
  expect(lastAsked().text).toBe('send me the report');
  await expect.poll(() => page.value()).toBe(`Hey, can you ${FORMAL} by Friday?`);

  // The last dictation went into the pad; the page in front: new text
  // written at its cursor, the pad left alone.
  await pad.set('box', '');
  await robot.dictate(pad, SAID);
  await expect.poll(() => pad.text('box')).toBe(CLEANED);
  await page.set('');
  out = await rewrite(robot, page, 'write a thank-you line');
  expect(out).toMatchObject({ last: false, outcome: 'pasted', text: WRITTEN });
  expect(lastAsked()).toEqual({ text: '', instruction: 'write a thank-you line' });
  await expect.poll(() => page.value()).toBe(WRITTEN);
  await sleep(300);
  expect(await pad.text('box')).toBe(CLEANED);
});

test('"Rewrite your last dictation" off: nothing selected right after a dictation writes new text at the cursor, as before', async ({
  robot,
}) => {
  const { pad } = robot;
  await robot.configure({ rewriteLastDictation: false });
  try {
    await pad.set('box', '');
    await robot.dictate(pad, SAID);
    await expect.poll(() => pad.text('box')).toBe(CLEANED);
    const out = await rewrite(robot, pad, 'make it more formal');
    expect(out).toMatchObject({ last: false, outcome: 'pasted', text: FORMAL });
    // The AI was never given the dictation: new text, written after it.
    expect(lastAsked()).toEqual({ text: '', instruction: 'make it more formal' });
    await expect.poll(() => pad.text('box')).toBe(`${CLEANED}${FORMAL}`);
  } finally {
    await robot.configure({ rewriteLastDictation: true });
  }
  // On again: the next dictation is rewritten in place.
  await pad.set('box', '');
  await robot.dictate(pad, SAID);
  await expect.poll(() => pad.text('box')).toBe(CLEANED);
  expect((await rewrite(robot, pad, 'make it more formal')).outcome).toBe('replaced');
  await expect.poll(() => pad.text('box')).toBe(FORMAL);
});

test('a terminal: never rewritten in place — new text written at its prompt, as before', async ({ robot }) => {
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
  await robot.dictate(sh, SAID);
  await expect.poll(() => lines().at(-1)).toBe(`PS> ${CLEANED}`);
  const out = await rewrite(robot, sh, 'write a thank-you line');
  expect(out).toMatchObject({ last: false, outcome: 'pasted', text: WRITTEN });
  await expect.poll(() => lines().at(-1)).toBe(`PS> ${CLEANED}${WRITTEN}`);
});
