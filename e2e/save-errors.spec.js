// Saves Yap couldn't make (shell/saving.rs, code review 2026-10-23 #7): a full
// disk, or another program holding notes.json, used to be a warning in the
// log while the page said it saved. `e2e_fail_saves` makes a store's next
// writes fail as a full disk would.
//
// - A save the person made (typing in a note) fails: the command says so,
//   and the page shows "Couldn't save your note" with Try again
//   (saveErrors.js); the note stays as typed, and Try again writes it.
// - A save that fails by itself (history after a dictation): one notice
//   through the notice policy, however many fail, with Try again.
import { test, expect, expectStore, openView, openSettings, closeSettings } from './support/fixtures.js';

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

// A Skill saved on a full disk is made (Yap keeps it): until 2026-10-30 the
// form stayed open with an error, and Save again said "You already have a
// Skill with /garden-jobs".
test('a Skill that couldn’t be saved is kept, its form closes, and Try again saves it', async ({ yap, main, shot }) => {
  const dialog = await openSettings(main, 'Skills');
  await dialog.getByRole('button', { name: 'New Skill' }).click();
  const form = dialog.getByRole('group', { name: 'New Skill' });
  await form.getByRole('textbox', { name: 'Skill name' }).fill('Garden jobs');
  await expect(form.getByRole('textbox', { name: 'Skill command' })).toHaveValue('garden-jobs');
  await form.getByRole('textbox', { name: 'Skill instructions' }).fill('List every job in the garden that was mentioned, and when.');
  await yap.invoke('e2e_fail_saves', { store: 'skills', count: 1 });
  try {
    await form.getByRole('button', { name: 'Save Skill' }).click();
    const toast = toastsWith(main, "Couldn't save your Skills");
    await expect(toast).toBeVisible();
    await expect(form).toHaveCount(0);
    await expect(dialog.locator('[data-skill="garden-jobs"]')).toBeVisible();
    expect(JSON.stringify(yap.readJson('skills.json') ?? {})).not.toContain('garden-jobs');
    await expect(toastsWith(main, 'You already have a Skill')).toHaveCount(0);
    await shot(main, '04-skill-not-saved');
    await toast.getByRole('button', { name: 'Try again' }).click();
    await expect(toastsWith(main, 'Saved')).toBeVisible();
    await expectStore(yap, 'skills.json', (s) => s.skills?.some((k) => k.command === 'garden-jobs'));
  } finally {
    await yap.invoke('e2e_fail_saves', { store: 'skills', count: 0 });
    await closeSettings(main);
  }
});

// The Dictionary view saves config.json itself, and until 2026-10-30 a
// failed save was swallowed (code reviews 2026-10-23 and 2026-10-30): a word
// added on a full disk looked saved and was gone at the next start.
test('a dictionary word that couldn’t be saved says so, stays in the list, and Try again saves it', async ({ yap, main, shot }) => {
  await openView(main, 'Dictionary');
  const corrections = main.getByRole('region', { name: 'Corrections' });
  // Every save of config.json fails until it's let go (an edit saves more
  // than once while it's typed).
  await yap.invoke('e2e_fail_saves', { store: 'config', count: 100 });
  try {
    await corrections.getByRole('button', { name: '+ Add correction' }).click();
    await corrections.getByRole('textbox', { name: 'Heard' }).last().fill('fern dale');
    await corrections.getByRole('textbox', { name: 'Replace with' }).last().fill('Ferndale');
    const toast = toastsWith(main, "Couldn't save your settings");
    await expect(toast).toBeVisible();
    await expect(toast).toContainText('The disk is full. Yap is keeping your changes and will try again with the next one.');
    await shot(main, '05-dictionary-not-saved');
    // In the list, not on disk.
    await expect(corrections.getByRole('textbox', { name: 'Replace with' }).last()).toHaveValue('Ferndale');
    expect(JSON.stringify(yap.readJson('config.json')?.dictionary ?? [])).not.toContain('Ferndale');

    await yap.invoke('e2e_fail_saves', { store: 'config', count: 0 });
    await toast.getByRole('button', { name: 'Try again' }).click();
    await expect(toastsWith(main, 'Saved')).toBeVisible();
    await expectStore(yap, 'config.json', (c) => (c.dictionary ?? []).some((e) => e.from === 'fern dale' && e.to === 'Ferndale'));
  } finally {
    await yap.invoke('e2e_fail_saves', { store: 'config', count: 0 });
  }
});
