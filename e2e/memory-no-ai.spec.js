// Settings → Memory with no AI model set up: it says so and links to
// Language Models, "Tell Yap what to change" waits for one, and what the
// person adds by hand still works (memory.rs needs a model only to learn
// and to turn words into changes).
import { test, expect, openSettings, closeSettings, expectStore } from './support/fixtures.js';

test.use({ yapOptions: { name: 'memory-no-ai' } });

test('no AI model: memory says so, links to Language Models, and hand edits still work', async ({ yap, main, shot }) => {
  const dialog = await openSettings(main, 'Memory');
  const card = dialog.getByRole('note').filter({ hasText: 'Yap needs an AI model for memory' });
  await expect(card).toBeVisible();
  await expect(dialog.getByRole('button', { name: 'Suggest changes' })).toBeDisabled();
  await shot(main, '01-no-ai');

  // A hand edit doesn't need a model.
  const topics = dialog.getByRole('group', { name: 'Topics', exact: true });
  await topics.getByRole('button', { name: 'Add a topic' }).click();
  const add = topics.getByRole('group', { name: 'Add a topic' });
  await add.getByRole('textbox', { name: 'Name' }).fill('Running');
  await add.getByRole('button', { name: 'Save' }).click();
  await expectStore(yap, 'memory.json', (m) => m.items.some((i) => i.title === 'Running'));

  await card.getByRole('button', { name: 'Language Models' }).click();
  await expect(dialog.getByRole('heading', { level: 1, name: 'Language Models' })).toBeVisible();
  await closeSettings(main);
});
