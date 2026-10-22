// "Spell names the way they're written on screen" (src-tauri/src/
// screen_terms.rs), for real: the window a dictation goes into shows names
// and terms (an Edge page of the test's own, the test pad's rich text box);
// Yap reads its visible text over UI Automation at the dictation's start and
// spells what it heard as the screen does — and never touches everyday
// words near those names. The robot's transcript stands in for the STT here;
// screen-names-stt.real.js has real speech, screen-names-slow.real.js a read
// that takes its time.
import { test, expect, lf } from './support/robot.js';
import { SCREEN } from './support/screen.js';
import { startFakeLlm } from '../e2e/support/fake-llm.js';

/** A dictation of `said` (as if heard) into the Edge page; the text that landed. */
async function dictateInto(robot, page, said) {
  await page.set('');
  const r = await robot.dictate(page, said);
  expect(r.error).toBeNull();
  await expect.poll(() => page.value()).toBe(r.text);
  return r.text;
}

test('a name heard a little wrong is spelled as the page shows it; everyday words near it are not', async ({ robot }) => {
  const page = await robot.editor(SCREEN);
  const mark = robot.logMark();
  // Fixed: the spelling on screen, for what the speech model wrote as a
  // name (a capital), and any word with the same letters.
  expect(await dictateInto(robot, page, 'Rachelle wants the Zentrics pilot on kubernetes.')).toBe(
    'Rochelle wants the Zentrix pilot on Kubernetes.',
  );
  expect(await dictateInto(robot, page, 'Ask Shiobhan whether we run GPT OSS.')).toBe('Ask Siobhan whether we run GPT-OSS.');
  expect(await dictateInto(robot, page, 'Siobhan Ni Bhriain signed it off.')).toBe('Siobhan Ní Bhriain signed it off.');
  // Everyday words that look or sound like them: exactly as said.
  for (const said of [
    'We took the row shell out on the river.',
    'I learned some zen tricks at the retreat.',
    'Rachel is coming to the meeting too.',
    'The cooper made a barrel.',
    'Pass me the olive oil.',
    'That was a tricky one.',
    // Parakeet's garble of a spoken "row shell" (found with real speech,
    // screen-names-stt.real.js): no capital, so no name.
    'We took the rouchell out on the river.',
  ]) {
    expect(await dictateInto(robot, page, said)).toBe(said);
  }
  // The read: counted in the log (debug), never its text.
  const reads = robot
    .logSince(mark)
    .split('\n')
    .filter((l) => l.includes('Read the names on screen'));
  expect(reads.length).toBeGreaterThan(0);
  for (const l of reads) {
    expect(l).toMatch(/chars=\d+.*terms=\d+/);
    expect(l).not.toMatch(/Zentrix|Rochelle|Siobhan|Kubernetes/);
  }
});

test('off: what was heard is pasted as before', async ({ robot }) => {
  const page = await robot.editor(SCREEN);
  await robot.configure({ screenNames: false });
  try {
    expect(await dictateInto(robot, page, 'Rachelle wants the Zentrics pilot on kubernetes.')).toBe(
      'Rachelle wants the Zentrics pilot on kubernetes.',
    );
  } finally {
    await robot.configure({ screenNames: true });
  }
  expect(await dictateInto(robot, page, 'Rachelle wants the Zentrics pilot.')).toBe('Rochelle wants the Zentrix pilot.');
});

test('the test pad: names in another text box of the window count too', async ({ robot }) => {
  const { pad } = robot;
  await pad.set('rich', SCREEN);
  await pad.set('box', '');
  try {
    const r = await robot.dictate(pad, 'The Zentrics pilot moves to kubernetes.');
    expect(r.text).toBe('The Zentrix pilot moves to Kubernetes.');
    await expect.poll(async () => lf(await pad.text('box'))).toBe('The Zentrix pilot moves to Kubernetes.');
    // Nothing of the screen's in an everyday sentence.
    await pad.set('box', '');
    const plain = await robot.dictate(pad, 'We took the row shell out on the river.');
    expect(plain.text).toBe('We took the row shell out on the river.');
  } finally {
    await pad.set('rich', '');
    await pad.set('box', '');
  }
});

test('the dictionary has the final say over the screen', async ({ robot }) => {
  const page = await robot.editor(SCREEN);
  await robot.configure({ dictionary: [{ from: 'zen tricks', to: 'ZenTrix', fuzzy: true, snippet: false }] });
  try {
    expect(await dictateInto(robot, page, 'The zen tricks pilot is live.')).toBe('The ZenTrix pilot is live.');
  } finally {
    await robot.configure({ dictionary: [] });
  }
});

test('AI cleanup is told the names on screen the transcript seems to hold, and only those', async ({ robot }) => {
  const page = await robot.editor(SCREEN);
  const llm = await startFakeLlm();
  await robot.configure({ postProcessEnabled: true, ppProvider: 'custom', ppBaseUrl: llm.base, ppModel: 'fake-cleanup', ppApiKey: '' });
  const cleanups = () => llm.requests.filter((q) => q.kind === 'cleanup');
  try {
    await dictateInto(robot, page, 'so the zentrics pilot moves to kubernetes');
    const asked = cleanups().at(-1);
    expect(asked.user).toMatch(/Spell these names as written: [^\n]*Zentrix/);
    expect(asked.user).toMatch(/Spell these names as written: [^\n]*Kubernetes/);
    // Only what it seems to say: nothing else of the screen's goes.
    expect(asked.user).not.toMatch(/Rochelle|Siobhan|GPT-OSS/);
    // Never in the system prompt (the same for every dictation).
    expect(asked.system).not.toContain('Spell these names');

    // A dictation holding none of them: no line at all.
    await dictateInto(robot, page, 'see you at the station at noon');
    expect(cleanups().at(-1).user).not.toContain('Spell these names');

    // Off: none either.
    await robot.configure({ screenNames: false });
    await dictateInto(robot, page, 'so the zentrics pilot moves to kubernetes');
    expect(cleanups().at(-1).user).not.toContain('Spell these names');
  } finally {
    await robot.configure({ screenNames: true, postProcessEnabled: false, ppBaseUrl: '' });
    await llm.close();
  }
});
