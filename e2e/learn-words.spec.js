// "Suggest words from your corrections" (learn.rs): after a paste Yap
// watches the field, and when you fix a word it typed it offers to add it to
// the dictionary. A test run never reads another app's text, so the
// debug-only `e2e_learn_correction` hands Yap the edit its watch would have
// seen (pasted → edited); the rest is the real thing: the rules, the prompt
// (a toast with the main window on screen, else a Yap bar card), the answers
// and the dictionary they write. The rules are unit-tested in Rust, and the
// watch's UI Automation reads by `--ignored reads_real_text_fields`.
import { test, expect, openView, openSettings, closeSettings, expectStore } from './support/fixtures.js';

test.use({ yapOptions: { name: 'learn-words' } });

/** Stage an edit; `fresh` (default) forgets every earlier answer first. */
const stage = (yap, pasted, edited, { fresh = true } = {}) =>
  yap.invoke('e2e_learn_correction', { pasted, edited, fresh });

/** The suggestion toast in the main window. */
const suggestion = (main) => main.getByRole('status').filter({ hasText: 'to your dictionary?' });

/** Settings auto-saves its whole copy of the config: flip a switch there and back. */
async function settingsAutoSave(yap, main) {
  const dialog = await openSettings(main, 'Advanced');
  const space = dialog.getByRole('button', { name: 'Append trailing space' });
  const before = await space.getAttribute('aria-pressed');
  await space.click();
  await expectStore(yap, 'config.json', (c) => String(c.appendTrailingSpace) !== before);
  await space.click();
  await expectStore(yap, 'config.json', (c) => String(c.appendTrailingSpace) === before);
  await closeSettings(main);
}

test('a word you fixed is offered, and Add puts it in the dictionary', async ({ yap, main, shot }) => {
  const offered = await stage(
    yap,
    'We should move the app to cube and eighties next week.',
    'We should move the app to Kubernetes next week.'
  );
  expect(offered).toEqual([{ heard: 'cube and eighties', term: 'Kubernetes' }]);
  const prompt = suggestion(main);
  await expect(prompt).toBeVisible();
  await expect(prompt).toContainText('Add “Kubernetes” to your dictionary?');
  await expect(prompt).toContainText('Yap heard “cube and eighties”.');
  await expect(prompt).toContainText('Dictionary'); // its chip
  await expect(prompt.getByRole('button', { name: "Don't suggest this" })).toBeVisible();
  await shot(main, '01-suggestion');

  await prompt.getByRole('button', { name: 'Add', exact: true }).click();
  await expectStore(yap, 'config.json', (c) =>
    c.dictionary.some((e) => e.from === 'cube and eighties' && e.to === 'Kubernetes' && e.fuzzy && !e.snippet)
  );
  const done = main.getByRole('status').filter({ hasText: '“Kubernetes” is in your dictionary' });
  await expect(done).toBeVisible();
  await shot(main, '02-added');

  // Open Dictionary: the new correction is in the list.
  await done.getByRole('button', { name: 'Open Dictionary' }).click();
  await expect(main.getByRole('heading', { level: 1, name: 'Dictionary' })).toBeVisible();
  const corrections = main.getByRole('region', { name: 'Corrections' });
  await expect(corrections.getByRole('textbox', { name: 'Heard' }).last()).toHaveValue('cube and eighties');
  await expect(corrections.getByRole('textbox', { name: 'Replace with' }).last()).toHaveValue('Kubernetes');
  await shot(main, '03-dictionary');

  // Settings' copy of the config took the word too: an auto-save keeps it.
  await settingsAutoSave(yap, main);
  expect(yap.readConfig().dictionary.filter((e) => e.to === 'Kubernetes')).toHaveLength(1);

  // In the dictionary now: not offered again.
  expect(await stage(yap, 'Deploy it to cube and eighties.', 'Deploy it to Kubernetes.')).toEqual([]);
  await expect(suggestion(main)).toHaveCount(0);
});

test('a word added while the Dictionary view is open shows up there', async ({ yap, main }) => {
  await openView(main, 'Dictionary');
  expect(await stage(yap, 'Open the graph ana dashboard.', 'Open the Grafana dashboard.')).toHaveLength(1);
  await suggestion(main).getByRole('button', { name: 'Add', exact: true }).click();
  const corrections = main.getByRole('region', { name: 'Corrections' });
  await expect(corrections.getByRole('textbox', { name: 'Replace with' }).last()).toHaveValue('Grafana');
  await expect(corrections.getByRole('textbox', { name: 'Heard' }).last()).toHaveValue('graph ana');
  // …and the view's own save doesn't drop it.
  await main.getByRole('button', { name: 'Catch near-misses' }).click();
  await expectStore(yap, 'config.json', (c) => c.dictionaryFuzzy === false);
  expect(yap.readConfig().dictionary.some((e) => e.to === 'Grafana')).toBe(true);
  await main.getByRole('button', { name: 'Catch near-misses' }).click();
  await expectStore(yap, 'config.json', (c) => c.dictionaryFuzzy === true);
});

test('"Not now" twice, or "Don\'t suggest this", and that pair isn\'t offered again', async ({ yap, main }) => {
  const [pasted, edited] = ['Restart the post gress server.', 'Restart the Postgres server.'];
  expect(await stage(yap, pasted, edited)).toEqual([{ heard: 'post gress', term: 'Postgres' }]);
  await suggestion(main).getByRole('button', { name: 'Not now' }).click();
  await expectStore(yap, 'word_suggestions.json', (r) => r.pairs['post gress→postgres']?.notNow === 1);
  await expect(suggestion(main)).toHaveCount(0);

  // Asked once more (the hook skips the few minutes between prompts); its ✕
  // is a "Not now" too.
  expect(await stage(yap, pasted, edited, { fresh: false })).toHaveLength(1);
  await suggestion(main).getByRole('button', { name: 'Close', exact: true }).click();
  await expectStore(yap, 'word_suggestions.json', (r) => r.pairs['post gress→postgres']?.notNow === 2);
  expect(await stage(yap, pasted, edited, { fresh: false })).toEqual([]);
  await expect(suggestion(main)).toHaveCount(0);

  // "Don't suggest this": at once, for good.
  const [p2, e2] = ['Ask chat G P T about it.', 'Ask ChatGPT about it.'];
  expect(await stage(yap, p2, e2, { fresh: false })).toHaveLength(1);
  await suggestion(main).getByRole('button', { name: "Don't suggest this" }).click();
  await expectStore(yap, 'word_suggestions.json', (r) => r.pairs['chat g p t→chatgpt']?.never === true);
  expect(await stage(yap, p2, e2, { fresh: false })).toEqual([]);
  // Nothing was added.
  expect(yap.readConfig().dictionary.some((e) => e.to === 'Postgres' || e.to === 'ChatGPT')).toBe(false);
});

test('fixing an everyday word, or rewriting, offers nothing', async ({ yap, main }) => {
  expect(await stage(yap, "Let's meat at noon.", "Let's meet at noon.")).toEqual([]);
  expect(await stage(yap, 'I go there some times.', 'I go there sometimes.')).toEqual([]);
  expect(await stage(yap, 'We shipped it.', 'We finally shipped it.')).toEqual([]);
  expect(await stage(yap, 'Book the room for Sarah.', 'Book the room for Priya.')).toEqual([]);
  await main.waitForTimeout(300);
  await expect(suggestion(main)).toHaveCount(0);
});

test('"Suggest words from your corrections" is on, and off means no suggestions', async ({ yap, main, shot }) => {
  await openView(main, 'Dictionary');
  const toggle = main.getByRole('button', { name: 'Suggest words from your corrections' });
  await expect(toggle).toHaveAttribute('aria-pressed', 'true');
  await toggle.scrollIntoViewIfNeeded();
  await shot(main, '04-dictionary-switch');

  await toggle.click();
  await expectStore(yap, 'config.json', (c) => c.learnWords === false);
  expect(await stage(yap, 'Use the next cloud server.', 'Use the Nextcloud server.')).toEqual([]);
  await expect(suggestion(main)).toHaveCount(0);

  // Settings' copy of the config keeps it off through an auto-save.
  await settingsAutoSave(yap, main);
  expect(yap.readConfig().learnWords).toBe(false);

  await openView(main, 'Home');
  await openView(main, 'Dictionary');
  await expect(toggle).toHaveAttribute('aria-pressed', 'false');
  await toggle.click();
  await expectStore(yap, 'config.json', (c) => c.learnWords === true);
  expect(await stage(yap, 'Use the next cloud server.', 'Use the Nextcloud server.')).toHaveLength(1);
  await expect(suggestion(main)).toContainText('Add “Nextcloud” to your dictionary?');
});

test('with the main window away, the suggestion is a card on the Yap bar', async ({ yap, shot }) => {
  await yap.invoke('plugin:window|close', { label: 'settings' });
  await expect.poll(() => yap.invoke('plugin:window|is_visible', { label: 'settings' })).toBe(false);
  try {
    expect(await stage(yap, 'Send it to the fig ma file.', 'Send it to the Figma file.')).toHaveLength(1);
    const card = yap.overlay.getByRole('status').filter({ hasText: 'Add “Figma” to your dictionary?' });
    await expect(card).toBeVisible({ timeout: 5_000 });
    await expect(card).toContainText('Yap heard “fig ma”.');
    await expect(card).toContainText('Dictionary'); // its chip
    await expect(card.getByRole('button', { name: 'Not now' })).toBeVisible();
    await expect(card.getByRole('button', { name: "Don't suggest this" })).toBeVisible();
    await yap.overlay.waitForTimeout(300); // the card's entrance
    await shot(yap.overlay, '05-suggestion-on-bar');

    // Add (as the bar reports a click on it): saved, and said so.
    await yap.invoke('bar_card_action', { id: 'word-suggestion', action: 'add' });
    await expectStore(yap, 'config.json', (c) => c.dictionary.some((e) => e.from === 'fig ma' && e.to === 'Figma'));
    await expect(yap.overlay.getByRole('status').filter({ hasText: '“Figma” is in your dictionary' })).toBeVisible();
    await expect(card).toHaveCount(0);
  } finally {
    await yap.invoke('open_settings'); // back as the suite started it
  }
});
