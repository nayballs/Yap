// The robot on asking about selected text without replacing it
// (dictation/ask_or_edit.rs, chat/assistant/selected.rs, 2026-10-31): a real
// selection in a real window, and nothing replaced.
// - The rewrite shortcut on text selected in an Edge text box
//   (TextPattern): "What does this mean?" is answered — the question in
//   Chat with the selection as its quote — and the box's text, the
//   selection and the clipboard are exactly as they were; "make it
//   shorter" still replaces the selection.
// - Read-only text (a web page's paragraph): the rewrite shortcut's
//   "make it shorter" is answered, never pasted (nothing could go in its
//   place); Ask about the selection's shortcut reads it (as Read aloud does)
//   and answers.
//
// `realtest_rewrite_text` / `realtest_ask_selection` stand in for the
// shortcuts and the speech: the window in front, its selection read as at
// the key press (UI Automation; no Ctrl+C in robot mode). Stand-in AI
// models answer (desktop/e2e/support/fake-llm.js: Commands' rewrite, Chat's
// answer). Written 2026-10-31; its first run found the page's text (the
// focus on a `tabindex` block of it) taken for a text box: the selected
// range's own IsReadOnly decides now (pasting/selection.rs).
import { test, expect } from './support/robot.js';
import { edgeExe } from './support/targets.js';
import { startFakeLlm, rewriteText, rewriteInstruction } from '../e2e/support/fake-llm.js';

const TEST_KEY = 'e2e-fake-key';
const ERROR = 'error[E0425]: cannot find value `total` in this scope';
const SHORTER = 'Unknown name: total.';
const ANSWER = 'It means total is used before it was declared.';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

let llm;
test.beforeAll(async ({ robot }) => {
  llm = await startFakeLlm();
  llm.reply('rewrite', (user) => (/shorter/i.test(rewriteInstruction(user)) ? SHORTER : 'Rewritten.'));
  llm.reply('other', () => ANSWER);
  const cfg = await robot.yap.invoke('get_config');
  await robot.configure({
    postProcessEnabled: true,
    ppProvider: 'custom',
    ppBaseUrl: llm.base,
    ppModel: 'fake-cleanup',
    ppApiKey: '',
    cleanupStyles: false,
    askAboutSelection: true,
    ppApiKeys: { ...(cfg.ppApiKeys || {}), groq: TEST_KEY },
    llmScopes: {
      ...cfg.llmScopes,
      chat: { enabled: true, provider: 'groq', baseUrl: llm.base, model: 'openai/gpt-oss-20b', apiKey: TEST_KEY, apiKeys: {}, prompt: '', disableThinking: false },
    },
  });
});

test.afterAll(async ({ robot }) => {
  await robot.configure({ postProcessEnabled: false, ppBaseUrl: '' }).catch(() => {});
  await llm?.close();
});

/** The rewrite shortcut in `target` (brought to the front), `instruction` as what was said. */
const rewrite = (robot, target, instruction) =>
  robot.withFocus(target, true, () => robot.yap.invoke('realtest_rewrite_text', { text: instruction, hwnd: target.hwnd }));

/** Ask about the selection's shortcut in `target`, `question` as what was said. */
const askAbout = (robot, target, question) =>
  robot.withFocus(target, true, () => robot.yap.invoke('realtest_ask_selection', { text: question, hwnd: target.hwnd }));

/** The question asked as `text` in chats.json, and the answer after it. */
function asked(yap, text) {
  for (const c of yap.readJson('chats.json')?.conversations ?? []) {
    const i = c.messages.findIndex((m) => m.role === 'user' && m.text === text);
    if (i >= 0) return { question: c.messages[i], answer: c.messages[i + 1] ?? null };
  }
  return null;
}

const rewrites = () => llm.requests.filter((r) => r.kind === 'rewrite').length;

test.describe('in Edge', () => {
  test.skip(!edgeExe(), 'Microsoft Edge is not installed');

  test('a text box: a question is answered with nothing replaced (the text, the selection, the clipboard); an edit still replaces', async ({
    robot,
  }) => {
    const { yap } = robot;
    const page = await robot.editor();
    const text = `Build log: ${ERROR} (line 12)`;
    const from = text.indexOf(ERROR);
    await page.set(text, from, from + ERROR.length);
    // Something of the person's own on the clipboard, in several formats.
    await robot.clipboard('setRich', 'Copied before');
    const clipBefore = await robot.clipboard('fingerprint');
    const asked0 = rewrites();

    const out = await rewrite(robot, page, 'What does this mean?');
    expect(out).toMatchObject({ outcome: 'answered', decided: 'rule:ask', last: false });
    await expect.poll(() => asked(yap, 'What does this mean?')?.answer?.text ?? null, { timeout: 20_000 }).toBe(ANSWER);
    expect(asked(yap, 'What does this mean?').question.quote).toMatchObject({ noteId: 0, text: ERROR });
    // Nothing replaced or typed, the Commands model never asked to rewrite.
    await sleep(1_000);
    expect(await page.value()).toBe(text);
    expect(rewrites()).toBe(asked0);
    expect(await robot.clipboard('fingerprint')).toEqual(clipBefore);

    // An edit: the selection replaced, as ever.
    await page.set(text, from, from + ERROR.length);
    const edit = await rewrite(robot, page, 'make it shorter');
    expect(edit).toMatchObject({ outcome: 'pasted', decided: 'rule:edit', text: SHORTER });
    expect(rewriteText(llm.requests.filter((r) => r.kind === 'rewrite').at(-1).system)).toBe(ERROR);
    await expect.poll(() => page.value()).toBe(`Build log: ${SHORTER} (line 12)`);
  });

  test("read-only text: the rewrite shortcut answers even an edit, and Ask about the selection reads it as Read aloud does", async ({ robot }) => {
    const { yap } = robot;
    const page = await robot.readingPage(['Not this one.', 'The quarterly figures came in under the forecast by four percent.']);
    await page.selectText(1);
    const asked0 = rewrites();
    const out = await rewrite(robot, page, 'Make it shorter');
    // Read-only (UI Automation says so): nothing could be put in its place.
    expect(out).toMatchObject({ outcome: 'answered', decided: 'readOnly:answer' });
    await expect.poll(() => asked(yap, 'Make it shorter')?.answer?.text ?? null, { timeout: 20_000 }).toBe(ANSWER);
    expect(rewrites()).toBe(asked0);

    // Its own shortcut: always a question about the selection.
    await page.selectText(1);
    await askAbout(robot, page, 'Is this good news?');
    await expect.poll(() => asked(yap, 'Is this good news?')?.answer?.text ?? null, { timeout: 20_000 }).toBe(ANSWER);
    expect(asked(yap, 'Is this good news?').question.quote.text).toContain('under the forecast by four percent');
    expect(rewrites()).toBe(asked0);
  });
});

test('the test pad with nothing selected: Ask about the selection says so and asks nothing', async ({ robot }) => {
  const { yap, pad } = robot;
  await pad.set('box', 'Some text, none of it selected.');
  await expect(askAbout(robot, pad, 'What is this?')).rejects.toThrow(/Nothing selected/);
  await expect(yap.main.getByRole('status').filter({ hasText: 'Select some text first' })).toBeVisible();
  expect(asked(yap, 'What is this?')).toBeNull();
  expect(await pad.text('box')).toBe('Some text, none of it selected.');
});
