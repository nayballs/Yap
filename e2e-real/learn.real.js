// "Suggest words from your corrections" (learn.rs), for real: after the
// robot's paste Yap watches the field over UI Automation; the field is then
// edited the way a person fixes a word Yap got wrong, and Enter ends the
// watch. The suggestion shows in Yap's window (a toast: the main window is
// on screen), and its answer is what lands in the dictionary.
import { test, expect, waitForLog, lf } from './support/robot.js';
import { edgeExe } from './support/targets.js';

/** The suggestion toast in the robot's main window. */
const suggestion = (main) => main.getByRole('status').filter({ hasText: 'to your dictionary?' });

/** Forget earlier answers and the gap between prompts (the e2e hook's `fresh`). */
const forgetAnswers = (yap) => yap.invoke('e2e_learn_correction', { pasted: 'x', edited: 'x', fresh: true });

test('the test pad: a word fixed after the paste is offered, and Add puts it in the dictionary', async ({ robot }) => {
  const { yap, pad } = robot;
  await forgetAnswers(yap);
  await pad.set('box', '');
  const mark = robot.logMark();
  const r = await robot.dictate(pad, 'We should move the app to cube and eighties next week.');
  expect(r.error).toBeNull();
  await expect.poll(() => pad.text('box')).toBe('We should move the app to cube and eighties next week.');
  await waitForLog(yap, 'learn: watching the field for corrections', { after: mark });

  // The fix, then Enter (a new line after it): the watch ends and judges it.
  await pad.set('box', 'We should move the app to Kubernetes next week.');
  await new Promise((done) => setTimeout(done, 1_500));
  await pad.set('box', 'We should move the app to Kubernetes next week.\r\n');
  await waitForLog(yap, /learn: stopped watching the field end="?enter_pressed/, { after: mark });

  const prompt = suggestion(yap.main);
  await expect(prompt).toBeVisible();
  await expect(prompt).toContainText('Add “Kubernetes” to your dictionary?');
  await expect(prompt).toContainText('Yap heard “cube and eighties”.');
  await prompt.getByRole('button', { name: 'Add', exact: true }).click();
  await expect
    .poll(() => yap.readConfig().dictionary.some((e) => e.from === 'cube and eighties' && e.to === 'Kubernetes'))
    .toBe(true);

  // And the next dictation of it is corrected (the dictionary, live).
  await pad.set('box', '');
  const next = await robot.dictate(pad, 'We use cube and eighties.');
  expect(next.text).toBe('We use Kubernetes.');
  await expect.poll(async () => lf(await pad.text('box'))).toBe('We use Kubernetes.');
});

// Found by this robot (2026-10-17): with a learned word (near-misses on, as
// every new correction is), the fuzzy pass took the short word before the
// term into its 2-word n-gram — "to Kubernetes" is within the threshold of
// "Kubernetes" — and swallowed it. fuzzy.rs now leaves a phrase alone when
// one of its words already is the term.
test('a learned word never swallows the short word before it', async ({ robot }) => {
  const { pad } = robot;
  await robot.configure({ dictionary: [{ from: 'cube and eighties', to: 'Kubernetes', fuzzy: true, snippet: false }] });
  try {
    await pad.set('box', '');
    const r = await robot.dictate(pad, 'Deploy it to cube and eighties.');
    expect(r.text).toBe('Deploy it to Kubernetes.');
    await expect.poll(async () => lf(await pad.text('box'))).toBe('Deploy it to Kubernetes.');
  } finally {
    await robot.configure({ dictionary: [] });
  }
});

test('a web page: a fix there is offered too, and Not now leaves the dictionary alone', async ({ robot }) => {
  test.skip(!edgeExe(), 'Microsoft Edge is not installed');
  const { yap } = robot;
  await forgetAnswers(yap);
  await robot.configure({ dictionary: [] });
  const page = await robot.editor();
  // (Not an empty box: Edge reads an empty one as holding its label; see
  // browser.real.js.)
  await page.set('Notes: ');
  const mark = robot.logMark();
  await robot.dictate(page, 'Open the graph ana dashboard.');
  await expect.poll(() => page.value()).toBe('Notes: Open the graph ana dashboard.');
  await waitForLog(yap, 'learn: watching the field for corrections', { after: mark });

  await page.set('Notes: Open the Grafana dashboard.');
  await new Promise((done) => setTimeout(done, 1_500));
  await page.set('Notes: Open the Grafana dashboard.\n');
  await waitForLog(yap, /learn: stopped watching the field end="?enter_pressed/, { after: mark });

  const prompt = suggestion(yap.main);
  await expect(prompt).toBeVisible();
  await expect(prompt).toContainText('Add “Grafana” to your dictionary?');
  await prompt.getByRole('button', { name: 'Not now' }).click();
  await expect(prompt).toBeHidden();
  expect(yap.readConfig().dictionary.some((e) => e.to === 'Grafana')).toBe(false);
});
