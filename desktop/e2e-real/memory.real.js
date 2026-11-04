// Memory feeds dictation (src-tauri/src/memory/vocab.rs), for real: the
// names of the people and projects Yap remembers are fixed when a dictation
// mishears them a little (Parakeet, the default model, has only the near-miss
// pass), and everyday words near those names are never touched — into the
// test pad, through the paste, with this PC's own spell checker deciding what
// an ordinary word is.
import { test, expect, lf } from './support/robot.js';

/** Remember a person or a project, as Settings → Memory saves one. */
const remember = (yap, section, title, summary = '') =>
  yap.invoke('memory_save_item', { item: { section, title, summary, details: [] } });

test('names Yap remembers are spelled right in a dictation; everyday words near them are left alone', async ({ robot }) => {
  const { yap, pad } = robot;
  const people = [
    ['people', 'Siobhan', 'Sister'],
    ['people', 'Priya', 'Colleague at Acme'],
    ['people', 'Lucy', 'Friend from school'],
    ['people', 'Olivia', 'Neighbour'],
    ['people', 'Archie', 'Nephew'],
    ['people', 'Rochelle', 'Cousin'],
    ['projects', 'Grafana', 'Their dashboards'],
  ];
  for (const [section, title, summary] of people) await remember(yap, section, title, summary);
  const ids = Object.fromEntries((await yap.invoke('memory_list')).items.map((i) => [i.title, i.id]));

  const dictate = async (said) => {
    await pad.set('box', '');
    const r = await robot.dictate(pad, said);
    expect(r.error).toBeNull();
    await expect.poll(async () => lf(await pad.text('box'))).toBe(r.text);
    return r.text;
  };

  // A different spelling of a name, or its case: fixed.
  expect(await dictate("I'll ask Shiobhan about the venue.")).toBe("I'll ask Siobhan about the venue.");
  expect(await dictate('Pria said the build is green.')).toBe('Priya said the build is green.');
  expect(await dictate('Check the grafana dashboard first.')).toBe('Check the Grafana dashboard first.');
  expect(await dictate("Tell rochelle I'm running late.")).toBe("Tell Rochelle I'm running late.");

  // Ordinary words near those names (heard as words): exactly as said.
  for (const said of [
    "I'm feeling lucky today.",
    'Pass me the olive oil.',
    'They had to rescue the cat from the tree.',
    'Archive the old files first.',
    // Near enough to "Rochelle" that the plain near-miss pass would take it.
    'We took the row shell out on the river.',
    'We should mention it at the meeting.',
    'The mission went well.',
  ]) {
    expect(await dictate(said)).toBe(said);
  }

  // "Spell this name in dictation" off: Siobhan is left as heard.
  await yap.invoke('memory_set_dictation', { id: ids.Siobhan, on: false });
  expect(await dictate("I'll ask Shiobhan about the venue.")).toBe("I'll ask Shiobhan about the venue.");
  await yap.invoke('memory_set_dictation', { id: ids.Siobhan, on: true });
  expect(await dictate("I'll ask Shiobhan about the venue.")).toBe("I'll ask Siobhan about the venue.");

  // "Catch near-misses" off (the Dictionary view): no name pass either.
  await robot.configure({ dictionaryFuzzy: false });
  try {
    expect(await dictate('Pria said the build is green.')).toBe('Pria said the build is green.');
  } finally {
    await robot.configure({ dictionaryFuzzy: true });
  }
  await yap.invoke('memory_clear');
});
