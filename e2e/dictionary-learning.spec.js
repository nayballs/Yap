// The dictionary learning more by itself (2026-10-25), end to end through
// the stub engine's dictations (`e2e_dictation`: a transcript run through the
// real steps from cleanup on — the corrections, the names, snippets, the
// history):
// - your own name (About you, memory/vocab.rs `own_name`): spelled right in
//   a dictation without being a dictionary entry; Settings says how;
// - "Type that often? Save it as a snippet" (dictionary/snippet_offer.rs):
//   the same address three times in a week, offered once, saved with a
//   phrase to say; "Don't suggest this again", and the Dictionary view's
//   switch to have them back; the Yap bar's card with the window away;
// - a meeting summary's edit (dictionary/learn.rs `offer_for_summary`): a
//   name swapped for another offered for the dictionary at Done; a rewrite
//   offers nothing.
// The rules themselves are unit-tested in Rust. Every name, address and
// link here is made up.
import { test as base, expect, openView, openSettings, closeSettings, expectStore, setConfig } from './support/fixtures.js';

const NOW = Math.floor(Date.now() / 1000);

const SUMMARY = [
  '## Summary',
  '',
  'Shivon walked us through the cube and eighties move. The team agreed to ship on Friday.',
  '',
  '## Action items',
  '',
  '- [ ] **Shivon** to send the migration plan',
  '- [ ] Sam to book the room',
].join('\n');

const NOTE = {
  id: 801,
  title: 'Platform sync',
  noteType: 'meeting',
  folder: 'Meetings',
  source: 'meeting',
  participants: [],
  createdTs: NOW - 3600,
  updatedTs: NOW - 3600,
  content: 'Migration plan',
  enhancedContent: SUMMARY,
  transcript: [
    { source: 'them', text: 'I can walk you through the cube and eighties move.', ts: NOW - 3590 },
    { source: 'you', text: 'Great, we ship on Friday then.', ts: NOW - 3580 },
  ],
};

const test = base.extend({
  yapOptions: [
    { name: 'dictionary-learning', data: { 'notes.json': { folders: ['Personal', 'Meetings'], notes: [NOTE] } } },
    { scope: 'worker' },
  ],
});

const toastWith = (main, text) => main.getByRole('status').filter({ hasText: text });
const ADDRESS = '12 Station Road, Norwich NR1 4AB';
const LINK = 'https://example.com/book/sam-example';

test('your own name is spelled right in a dictation, without a dictionary entry', async ({ yap, main, shot }) => {
  await setConfig(yap, main, { aboutName: 'Siobhan', dictionary: [] });
  const { nameHint } = await yap.invoke('about_you_status', {});
  expect(['fixed', 'prompt']).toContain(nameHint);

  // Settings says what dictation does with it.
  const dialog = await openSettings(main, 'General');
  const about = dialog.getByRole('group', { name: 'About you' });
  await expect(about).toContainText(
    nameHint === 'fixed'
      ? 'Dictation spells it this way too, and fixes near-misses of it.'
      : 'Whisper models are told how to spell it when you dictate.'
  );
  await about.scrollIntoViewIfNeeded();
  await shot(main, '01-about-you-name-in-dictation');
  await closeSettings(main);

  const said = await yap.invoke('e2e_dictation', { text: 'can you ask shiobhan about the budget' });
  const asSaid = await yap.invoke('e2e_dictation', { text: 'and tell shivon the plan' });
  if (nameHint === 'fixed') {
    // A near-miss of it, and the way it's said (spoken_names.rs).
    expect(said.text).toContain('ask Siobhan about the budget');
    expect(asSaid.text).toContain('tell Siobhan the plan');
  } else {
    // No spell checker on this PC: Yap can't tell names from words, so no
    // name is a near-miss target (memory::Words) — only Whisper's prompt.
    expect(said.text).toContain('shiobhan');
  }
  // Ordinary words stay as said.
  expect((await yap.invoke('e2e_dictation', { text: 'show me the shiny version' })).text).toContain('show me the shiny version');
  // It's never an entry to manage.
  expect(yap.readConfig().dictionary).toEqual([]);

  // No name, nothing.
  await setConfig(yap, main, { aboutName: '' });
  expect((await yap.invoke('e2e_dictation', { text: 'can you ask shiobhan about the budget' })).text).toContain('shiobhan');
});

test('the same address three times in a week: offered once as a snippet, saved with a phrase to say', async ({ yap, main, shot }) => {
  await setConfig(yap, main, { dictionary: [], aboutName: '' });
  await yap.invoke('e2e_snippet_offer_reset');
  const offer = toastWith(main, 'Type that often? Save it as a snippet');

  await yap.invoke('e2e_dictation', { text: `please send it to ${ADDRESS}` });
  await yap.invoke('e2e_dictation', { text: `the parcel goes to ${ADDRESS} on Monday` });
  // Twice: not yet.
  await main.waitForTimeout(2_500);
  await expect(offer).toHaveCount(0);

  await yap.invoke('e2e_dictation', { text: `my home address is ${ADDRESS}` });
  await expect(offer).toBeVisible({ timeout: 10_000 });
  await expect(offer).toContainText(`You've used “${ADDRESS}” 3 times this week. Say “my address” and Yap types it.`);
  await expect(offer.getByRole('button', { name: "Don't suggest this again" })).toBeVisible();
  await shot(main, '02-snippet-offer');

  await offer.getByRole('button', { name: 'Save snippet' }).click();
  await expectStore(yap, 'config.json', (c) => c.dictionary.some((e) => e.snippet && e.from === 'my address' && e.to === ADDRESS));
  await expect(toastWith(main, 'Snippet saved')).toContainText('Say “my address” and Yap types it.');
  // Offered once: never again (the store keeps no text, only fingerprints).
  await expectStore(yap, 'snippet_suggestions.json', (s) => s.done.length === 1 && s.lastOfferedAt > 0);
  expect(JSON.stringify(yap.readJson('snippet_suggestions.json'))).not.toContain('Station');

  // Said, it's typed.
  const r = await yap.invoke('e2e_dictation', { text: 'deliver it to my address' });
  expect(r.text).toContain(`deliver it to ${ADDRESS}`);
  await main.waitForTimeout(2_000);
  await expect(offer).toHaveCount(0);

  // In the Dictionary view, with its length.
  await openView(main, 'Dictionary');
  const snippets = main.getByRole('region', { name: 'Snippets' });
  await expect(snippets.getByRole('textbox', { name: 'Phrase you say' })).toHaveValue('my address');
  await expect(snippets.locator('.snip-count')).toHaveText(`${ADDRESS.length} characters`);
});

test('"Don\'t suggest this again" stops the offers; the Dictionary view has them back; the bar card', async ({ yap, main }) => {
  await setConfig(yap, main, { dictionary: [] });
  await yap.invoke('e2e_snippet_offer_reset');
  const offer = toastWith(main, 'Type that often? Save it as a snippet');
  for (const lead of ['book a slot at', 'you can book at', 'grab a time at']) {
    await yap.invoke('e2e_dictation', { text: `${lead} ${LINK}` });
  }
  await expect(offer).toContainText('Say “my example link” and Yap types it.', { timeout: 10_000 });
  await offer.getByRole('button', { name: "Don't suggest this again" }).click();
  await expectStore(yap, 'snippet_suggestions.json', (s) => s.off === true);
  expect(yap.readConfig().dictionary).toEqual([]);

  // The switch in the Dictionary view says so, and turns them back on.
  await openView(main, 'Dictionary');
  const toggle = main.getByRole('button', { name: 'Suggest snippets for text you use often' });
  await expect(toggle).toHaveAttribute('aria-pressed', 'false');
  await toggle.click();
  await expectStore(yap, 'snippet_suggestions.json', (s) => s.off === false);

  // With the main window away: a card on the Yap bar.
  await yap.invoke('e2e_snippet_offer_reset');
  await yap.invoke('plugin:window|close', { label: 'settings' });
  await expect.poll(() => yap.invoke('plugin:window|is_visible', { label: 'settings' })).toBe(false);
  try {
    await yap.invoke('e2e_dictation', { text: `or book a time at ${LINK}` });
    const card = yap.overlay.getByRole('status').filter({ hasText: 'Type that often? Save it as a snippet' });
    await expect(card).toBeVisible({ timeout: 10_000 });
    await expect(card.getByRole('button', { name: 'Save snippet' })).toBeVisible();
    await yap.invoke('bar_card_action', { id: 'snippet-suggestion', action: 'save' });
    await expectStore(yap, 'config.json', (c) => c.dictionary.some((e) => e.snippet && e.from === 'my example link' && e.to === LINK));
    await expect(yap.overlay.getByRole('status').filter({ hasText: 'Snippet saved' })).toBeVisible();
  } finally {
    await yap.invoke('open_settings');
  }
  await setConfig(yap, main, { dictionary: [] });
});

test("a summary's name fixed by hand is offered for the dictionary; a rewrite isn't", async ({ yap, main, shot }) => {
  await setConfig(yap, main, { dictionary: [], learnWords: true });
  await openView(main, 'Notes');
  await main.locator('.folder').filter({ hasText: 'Meetings' }).click();
  await main.locator('.item').filter({ hasText: 'Platform sync' }).click();
  await main.locator('.tabs').getByRole('button', { name: 'Summary' }).click();

  const edit = async (text) => {
    await main.getByRole('button', { name: 'Edit summary' }).click();
    await main.getByRole('textbox', { name: 'Edit the summary (Markdown)' }).fill(text);
    await main.locator('.tabs').getByRole('button', { name: 'Done' }).click();
  };

  // The name fixed in both places, the task ticked as well.
  await edit(SUMMARY.replaceAll('Shivon', 'Siobhan').replace('- [ ] **Siobhan**', '- [x] **Siobhan**'));
  const prompt = toastWith(main, 'to your dictionary?');
  await expect(prompt).toContainText('Add “Siobhan” to your dictionary?');
  await expect(prompt).toContainText('Yap heard “Shivon”.');
  await shot(main, '03-summary-word-offer');
  await prompt.getByRole('button', { name: 'Add', exact: true }).click();
  await expectStore(yap, 'config.json', (c) => c.dictionary.some((e) => e.from === 'Shivon' && e.to === 'Siobhan' && !e.snippet));
  await expectStore(yap, 'notes.json', (s) => s.notes.find((n) => n.id === NOTE.id)?.enhancedContent.includes('Siobhan walked us'));

  // Rewritten: nothing offered.
  await edit('## Summary\n\nWe agreed the move to Kubernetes, and Priya books the room for Friday.');
  await main.waitForTimeout(2_000);
  await expect(toastWith(main, 'to your dictionary?')).toHaveCount(0);
});
