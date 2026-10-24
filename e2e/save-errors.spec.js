// Saves Yap couldn't make (saving.rs, code review 2026-10-23 #7): a full
// disk, or another program holding notes.json, used to be a warning in the
// log while the page said it saved. `e2e_fail_saves` makes a store's next
// writes fail as a full disk would.
//
// - A save the person made (typing in a note) fails: the command says so,
//   and the page shows "Couldn't save your note" with Try again
//   (saveErrors.js); the note stays as typed, and Try again writes it.
// - A save that fails by itself (history after a dictation): one notice
//   through the notice policy, however many fail, with Try again.
import { test, expect, expectStore, openView } from './support/fixtures.js';

test.use({ yapOptions: { name: 'save-errors' } });

/** The toasts in `main` whose text has `title`. */
const toastsWith = (main, title) => main.getByRole('status').filter({ hasText: title });

test('a note that couldn’t be saved says so, keeps the text, and Try again saves it', async ({ yap, main, shot }) => {
  await openView(main, 'Notes');
  await main.getByRole('button', { name: 'New note' }).first().click();
  await main.getByPlaceholder('Untitled Note').fill('Allotment plan');
  await expectStore(yap, 'notes.json', (s) => s.notes.some((n) => n.title === 'Allotment plan'));

  await yap.invoke('e2e_fail_saves', { store: 'notes', count: 1 });
  try {
    await main.getByPlaceholder('Start writing…').fill('Plant the broad beans in March.');
    const toast = toastsWith(main, "Couldn't save your note");
    await expect(toast).toBeVisible();
    await expect(toast).toContainText('The disk is full. Yap is keeping your changes and will try again with the next one.');
    await shot(main, '01-note-not-saved');
    // Kept in Yap, not on disk yet.
    const id = (yap.readJson('notes.json').notes.find((n) => n.title === 'Allotment plan') || {}).id;
    expect((await yap.invoke('note_get', { id })).content).toBe('Plant the broad beans in March.');
    expect(JSON.stringify(yap.readJson('notes.json'))).not.toContain('broad beans');
    // Said once: the notice for saves that fail by themselves stays quiet.
    await main.waitForTimeout(2_500);
    await expect(toastsWith(main, "Couldn't save your note")).toHaveCount(1);

    await toast.getByRole('button', { name: 'Try again' }).click();
    await expect(toastsWith(main, 'Saved')).toBeVisible();
    await expectStore(yap, 'notes.json', (s) => s.notes.some((n) => n.content === 'Plant the broad beans in March.'));
    await shot(main, '02-note-saved-after-try-again');
  } finally {
    await yap.invoke('e2e_fail_saves', { store: 'notes', count: 0 });
  }
});

test('history that fails to save after dictations is said once, with Try again', async ({ yap, main, shot }) => {
  await yap.invoke('e2e_fail_saves', { store: 'history', count: 2 });
  try {
    await yap.invoke('e2e_dictation', { text: 'water the tomatoes tonight' });
    await yap.invoke('e2e_dictation', { text: 'order more compost' });
    const notice = toastsWith(main, "Couldn't save your dictation history");
    await expect(notice).toBeVisible({ timeout: 10_000 });
    await expect(notice).toContainText('The disk is full. Yap is keeping it in memory and will try again with the next change.');
    await shot(main, '03-history-not-saved-notice');
    // Two failed saves, one notice.
    await main.waitForTimeout(2_500);
    await expect(toastsWith(main, "Couldn't save your dictation history")).toHaveCount(1);
    // Nothing on disk yet; both in Home's feed.
    expect(JSON.stringify(yap.readJson('history.json') ?? [])).not.toContain('compost');

    await notice.getByRole('button', { name: 'Try again' }).click();
    await expect(toastsWith(main, 'Saved')).toBeVisible();
    await expectStore(yap, 'history.json', (h) =>
      ['tomatoes', 'compost'].every((w) => h.some((e) => e.text.toLowerCase().includes(w))),
    );
  } finally {
    await yap.invoke('e2e_fail_saves', { store: 'history', count: 0 });
  }
});
