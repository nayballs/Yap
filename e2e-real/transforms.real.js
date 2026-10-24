// The robot on saved transforms (transforms.rs, 2026-10-24): a transform's
// shortcut in a real window — Polish on text selected in the test pad (a
// WinForms box: its own selection, EM_GETSEL), an Edge text box and a chat
// app's contenteditable (TextPattern), the result pasted over the
// selection with the text around it untouched, and the bar's Undo putting
// it back; Translate on the last dictation with nothing selected (the
// rewrite shortcut's rules: selected exactly again, pasted over); and a
// terminal refused, nothing typed there.
//
// `realtest_transform` stands in for the shortcut: the window in front,
// its selection read as at the key press (no Ctrl+C in robot mode). A
// stand-in AI model answers (e2e/support/fake-llm.js: the cleanup, and the
// transform by its instruction). Written 2026-10-24, not run yet.
import { test, expect } from './support/robot.js';
import { edgeExe, hasWindowsTerminal } from './support/targets.js';
import { startFakeLlm, cleanupTranscript, transformText, transformInstruction } from '../e2e/support/fake-llm.js';

const SAID = 'hey can you send me the report by friday';
const CLEANED = 'Hey, can you send me the report by Friday?';
const ROUGH = 'so um we should like ship it friday';
const POLISHED = 'We should ship it on Friday.';
const FRENCH = 'Salut, peux-tu m’envoyer le rapport vendredi ?';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

let llm;
test.beforeAll(async ({ robot }) => {
  llm = await startFakeLlm();
  llm.reply('cleanup', (user) => (cleanupTranscript(user) === SAID ? CLEANED : cleanupTranscript(user)));
  llm.reply('transform', (user) => {
    const asked = transformInstruction(user);
    if (/^Polish/.test(asked)) return POLISHED;
    if (/into French/.test(asked)) return FRENCH;
    return transformText(user);
  });
  const cfg = await robot.yap.invoke('get_config');
  await robot.configure({
    postProcessEnabled: true,
    ppProvider: 'custom',
    ppBaseUrl: llm.base,
    ppModel: 'fake-cleanup',
    ppApiKey: '',
    transforms: cfg.transforms.map((t) => (t.id === 'translate' ? { ...t, language: 'French' } : t)),
  });
});

test.afterAll(async ({ robot }) => {
  await robot.configure({ postProcessEnabled: false, ppBaseUrl: '' }).catch(() => {});
  await llm?.close();
});

/** Transform `id`'s shortcut in `target` (brought to the front). */
const transform = (robot, target, id) =>
  robot.withFocus(target, true, () => robot.yap.invoke('realtest_transform', { id, hwnd: target.hwnd }));

/** What the AI was last asked to transform, and how. */
function lastAsked() {
  const req = llm.requests.filter((r) => r.kind === 'transform').at(-1);
  return req && { text: transformText(req.user), instruction: transformInstruction(req.user) };
}

const transforms = () => llm.requests.filter((r) => r.kind === 'transform').length;

test('the test pad: Polish on the selected words replaces only them, and the bar\'s Undo puts them back', async ({ robot }) => {
  const { pad, yap } = robot;
  const before = `Note: ${ROUGH} Thanks.`;
  await pad.set('box', before);
  await pad.call({ cmd: 'select', field: 'box', start: 'Note: '.length, length: ROUGH.length });
  const out = await transform(robot, pad, 'polish');
  expect(out).toMatchObject({ name: 'Polish', source: 'selection', outcome: 'pasted', text: POLISHED });
  expect(lastAsked()).toEqual({ text: ROUGH, instruction: expect.stringMatching(/^Polish the text/) });
  await expect.poll(() => pad.text('box')).toBe(`Note: ${POLISHED} Thanks.`);

  // Undo (the bar's button): the words from before, exactly where they were.
  await robot.withFocus(pad, true, () => yap.invoke('bar_action', { action: 'transform-undo' }));
  await expect.poll(() => pad.text('box')).toBe(before);
});

test('an Edge text box and a chat app\'s contenteditable: Polish over the selection (TextPattern)', async ({ robot }) => {
  test.skip(!edgeExe(), 'Microsoft Edge is not installed');
  const page = await robot.editor();
  const text = `Plan: ${ROUGH}`;
  await page.set(text, 'Plan: '.length, text.length);
  let out = await transform(robot, page, 'polish');
  expect(out).toMatchObject({ source: 'selection', outcome: 'pasted' });
  await expect.poll(() => page.value()).toBe(`Plan: ${POLISHED}`);

  // WhatsApp Web's kind of box.
  const editable = await robot.chat('', { editable: true });
  await editable.set(ROUGH, 0, ROUGH.length);
  out = await transform(robot, editable, 'polish');
  expect(out).toMatchObject({ source: 'selection', outcome: 'pasted' });
  await expect.poll(() => editable.value()).toBe(POLISHED);
});

test('Translate with nothing selected, right after a dictation: that dictation translated where it is', async ({ robot }) => {
  const { pad } = robot;
  await pad.set('box', '');
  await robot.dictate(pad, SAID);
  await expect.poll(() => pad.text('box')).toBe(CLEANED);
  const out = await transform(robot, pad, 'translate');
  expect(out).toMatchObject({ name: 'Translate to French', source: 'last', outcome: 'replaced', text: FRENCH });
  expect(lastAsked().text).toBe(CLEANED);
  expect(lastAsked().instruction).toContain('into French');
  await expect.poll(() => pad.text('box')).toBe(FRENCH);

  // Typed after it since: nothing changed, the AI not asked, said.
  await pad.set('box', '');
  await robot.dictate(pad, SAID);
  await expect.poll(() => pad.text('box')).toBe(CLEANED);
  await pad.set('box', `${CLEANED} Thanks.`);
  const asked = transforms();
  const refused = await transform(robot, pad, 'translate');
  expect(refused).toMatchObject({ source: 'last', outcome: 'refused' });
  expect(transforms()).toBe(asked);
  await sleep(500);
  expect(await pad.text('box')).toBe(`${CLEANED} Thanks.`);
});

test('a terminal: refused — nothing typed at its prompt, the AI not asked', async ({ robot }) => {
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
  const asked = transforms();
  const out = await transform(robot, sh, 'polish');
  expect(out.outcome).toBe('terminal');
  expect(transforms()).toBe(asked);
  await sleep(500);
  expect(lines().at(-1)).toBe(`PS> ${CLEANED}`);
  await expect(robot.yap.main.getByRole('status').filter({ hasText: "Transforms don't work in a terminal" })).toBeVisible();
});
