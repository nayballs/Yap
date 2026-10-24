// Read aloud, in real apps (voice/read_aloud.rs, pasting/selection.rs `read_selected`):
// text selected in a window the robot opened — the test pad's WinForms
// boxes, Notepad, an Edge page — is read over UI Automation (or the edit
// control's own selection) and read aloud, exactly as the Read aloud
// shortcut does (`realtest_read_selection` runs the shortcut's code with
// the test's window in front). Nothing is played: robot mode is test mode,
// whose voice is a silent stand-in on a simulated clock; what would have
// been said is in `tts_e2e_spoken`. The reading card on the Yap bar, its
// word lighting up, ⏸ and Stop are checked on the robot's own Yap.
import { test, expect, lf } from './support/robot.js';
import { edgeExe, hasWindowsTerminal, notepadBusy } from './support/targets.js';

const spoken = (yap) => yap.invoke('tts_e2e_spoken');
const state = (yap) => yap.invoke('read_aloud_state');
const card = (yap) => yap.overlay.locator('[data-region="reading"].card');

const TEXT =
  'Reading aloud helps anyone who finds long pages hard going. Yap reads the text you select, one sentence at a time.';

test.beforeEach(async ({ robot }) => {
  await robot.yap.invoke('tts_e2e_spoken', { clear: true });
});

test.afterEach(async ({ robot }) => {
  await robot.yap.invoke('read_aloud_action', { action: 'stop' }).catch(() => {});
});

test('the test pad: a text box selection is read from the edit control, and read aloud', async ({ robot }) => {
  const { yap, pad } = robot;
  await pad.set('box', `Not this part. ${TEXT} Nor this.`);
  await pad.call({ cmd: 'select', field: 'box', start: 15, length: TEXT.length });
  const r = await robot.readSelection(pad);
  // A WinForms text box has no TextPattern: its own selection (EM_GETSEL).
  expect(r).toMatchObject({ via: 'edit', chars: TEXT.length, terminal: false, copyAllowed: true, outcome: 'reading', text: TEXT });
  await expect.poll(async () => (await spoken(yap)).map((s) => s.sentences)).toEqual([
    ['Reading aloud helps anyone who finds long pages hard going.', 'Yap reads the text you select, one sentence at a time.'],
  ]);
  // The reading card on the bar, its word moving on.
  await expect(card(yap)).toBeVisible();
  await expect(card(yap).locator('.text')).toHaveText('Reading aloud helps anyone who finds long pages hard going.');
  await expect.poll(async () => (await state(yap)).word ?? -1, { timeout: 5_000 }).toBeGreaterThanOrEqual(2);
  // ⏸ holds it where it is.
  await yap.invoke('read_aloud_action', { action: 'pause' });
  const held = await state(yap);
  await new Promise((res) => setTimeout(res, 600));
  expect((await state(yap)).word).toBe(held.word);
  expect(held.phase).toBe('paused');
  await yap.invoke('read_aloud_action', { action: 'resume' });
  await expect.poll(async () => (await state(yap)).word, { timeout: 5_000 }).not.toBe(held.word);
  // The field is untouched: nothing was typed or pasted.
  expect(await pad.text('box')).toBe(`Not this part. ${TEXT} Nor this.`);

  // The same selection again: the shortcut is its own Stop.
  const again = await robot.readSelection(pad);
  expect(again.outcome).toBe('stopped');
  await expect(card(yap)).toHaveCount(0);
  expect((await yap.invoke('tts_state')).speaking).toBe(false);

  // The rich text box (a RichEdit: UI Automation's TextPattern).
  await pad.set('rich', 'Before. Read this one sentence. After.');
  await pad.call({ cmd: 'select', field: 'rich', start: 8, length: 23 });
  const rich = await robot.readSelection(pad);
  expect(rich.text).toBe('Read this one sentence.');
  expect(['uia', 'edit']).toContain(rich.via);
  await expect.poll(async () => (await spoken(yap)).at(-1)?.text).toBe('Read this one sentence.');
});

test('nothing selected: nothing is read, and the card says so', async ({ robot }) => {
  const { yap, pad } = robot;
  await pad.set('box', 'Some text, none of it selected.');
  const r = await robot.readSelection(pad);
  expect(r).toMatchObject({ via: '', chars: 0, outcome: 'nothing' });
  await expect(card(yap)).toContainText('Nothing selected');
  expect(await spoken(yap)).toEqual([]);
});

test.describe('Notepad', () => {
  const busy = notepadBusy();
  test.skip(!!busy, busy ?? '');

  test('a Notepad selection is read over UI Automation', async ({ robot }) => {
    const { yap } = robot;
    const np = await robot.notepad();
    await robot.dictate(np, TEXT);
    await expect.poll(() => lf(np.text()).trim()).toBe(TEXT);
    await np.selectAll();
    const r = await robot.readSelection(np);
    expect(r).toMatchObject({ via: 'uia', outcome: 'reading', terminal: false });
    expect(r.text.trim()).toBe(TEXT);
    await expect.poll(async () => (await spoken(yap)).at(-1)?.sentences).toEqual([
      'Reading aloud helps anyone who finds long pages hard going.',
      'Yap reads the text you select, one sentence at a time.',
    ]);
    await expect(card(yap)).toBeVisible();
    await expect.poll(async () => (await state(yap)).word ?? -1, { timeout: 5_000 }).toBeGreaterThanOrEqual(1);
  });
});

test.describe('an Edge page', () => {
  test.skip(!edgeExe(), 'Microsoft Edge is not installed');

  test("a web page's text (not a text box) is read, a paragraph or all of it", async ({ robot }) => {
    const { yap } = robot;
    const page = await robot.readingPage([
      'The quick brown fox jumps over the lazy dog.',
      'It came to £12,400 on 18 Oct, about 45% over.',
    ]);
    await page.selectText(1);
    let r = await robot.readSelection(page);
    expect(r).toMatchObject({ via: 'uia', outcome: 'reading' });
    expect(r.text.trim()).toBe('It came to £12,400 on 18 Oct, about 45% over.');
    // Said for the ear; shown as written, its words lit up in turn.
    await expect.poll(async () => (await spoken(yap)).at(-1)?.text).toBe(
      'It came to twelve thousand four hundred pounds on the eighteenth of October, about forty-five percent over.'
    );
    await expect(card(yap).locator('.text')).toHaveText('It came to £12,400 on 18 Oct, about 45% over.');
    await expect.poll(() => card(yap).locator('.w.now').textContent().catch(() => null), { timeout: 5_000 }).toBe('12,400');

    // Both paragraphs.
    await yap.invoke('read_aloud_action', { action: 'stop' });
    await page.selectText();
    r = await robot.readSelection(page);
    expect(lf(r.text).split('\n').filter(Boolean)).toEqual([
      'The quick brown fox jumps over the lazy dog.',
      'It came to £12,400 on 18 Oct, about 45% over.',
    ]);
    await expect.poll(async () => (await spoken(yap)).at(-1)?.sentences.length).toBe(2);
    await expect(card(yap)).toContainText('1 of 2');
  });
});

test.describe('a terminal', () => {
  test.skip(!hasWindowsTerminal(), 'Windows Terminal (wt.exe) is not installed');

  test('in a terminal Yap never falls back to Ctrl+C', async ({ robot }) => {
    const { yap } = robot;
    const sh = await robot.shell('read');
    const mark = robot.logMark();
    const r = await robot.readSelection(sh);
    // Nothing selected in the terminal: nothing read, and Ctrl+C was never
    // allowed (with nothing selected it stops what's running there).
    expect(r).toMatchObject({ terminal: true, copyAllowed: false, outcome: 'nothing' });
    expect(r.via).not.toBe('copy');
    const log = robot.logSince(mark);
    expect(log).toMatch(/Read aloud: the selection.*terminal=true/);
    expect(log).not.toContain('by Ctrl+C');
    expect(sh.text()).not.toContain('^C');
  });
});
