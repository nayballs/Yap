// Pinned notes, each note's version history, and the saved transforms on a
// note (src-tauri/src/notes/pins.rs, notes/versions.rs, notes/transform.rs;
// src/lib/views/notes/: NoteList, NoteMeta's ⋯, VersionHistory, noteMenus.js):
// - a pin from the note's menu and from its row: pinned notes first in
//   Notes and in their folder, a pin mark; kept in note_pins.json (never
//   notes.json, which an older Yap rewrites), gone with the note;
// - Version history: what the note said before its history began, typing
//   and an Action, newest first, each marked how it came; the preview marks
//   what a version changed, or how it differs from now; Restore, and the
//   toast's Undo, each a version of its own; the file goes with the note;
// - a transform on the selection (Shift+F10 and a right-click on selected
//   text in My thoughts) and on the whole note (its ⋯), on the fake model:
//   the result in place and selected, Undo, and both in Version history.
// The AI is a local fake (support/fake-llm.js).
import { test as base, expect, openView, expectStore, setConfig, closeToasts } from './support/fixtures.js';
import { startFakeLlm, transformText, transformInstruction } from './support/fake-llm.js';

const test = base.extend({
  fakeLlm: [
    async ({}, use) => {
      const fake = await startFakeLlm();
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({ fakeLlm }, use) => {
      await use({
        name: 'notes-history',
        config: {
          llmScopes: { noteFormatting: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-notes' } },
          meetingReminder: 'never',
        },
      });
    },
    { scope: 'worker' },
  ],
});

const folderRow = (main, name) => main.locator('.folderlist').getByRole('button', { name: new RegExp(`^${name}( \\d+)?$`) });
const noteRow = (main, title) => main.locator('.items .item').filter({ hasText: title });
const titles = (main) => main.locator('.items .item .ittext').allTextContents();
const menu = (main) => main.getByRole('menu').first();
const raw = (main) => main.getByRole('textbox', { name: 'My thoughts' });
const history = (main) => main.getByRole('dialog', { name: 'Version history' });
const toastWith = (main, text) => main.getByRole('status').filter({ hasText: text });
const noteIn = (store, id) => store?.notes?.find((n) => n.id === id);
const versionsOf = (yap, id) => yap.readJson(`note-versions/${id}.json`);

test.afterEach(async ({ main }) => {
  await main.keyboard.press('Escape');
  await closeToasts(main);
});

test('a note is pinned from its menu or its row: first in Notes and in its folder, a pin mark, kept apart, gone with it', async ({
  yap,
  main,
  shot,
}) => {
  const groceries = await yap.invoke('note_create', { title: 'Groceries', content: 'Milk, eggs' });
  const packing = await yap.invoke('note_create', { title: 'Packing list', content: 'Tent, stove' });
  await yap.invoke('note_create', { title: 'Book ideas', content: 'A robot learns to garden' });
  await openView(main, 'Notes');
  await folderRow(main, 'Personal').click();
  await expect(noteRow(main, 'Book ideas')).toBeVisible();
  const before = await titles(main);

  // The note's menu: Pin. It comes first, with a pin mark.
  await noteRow(main, 'Packing list').click({ button: 'right' });
  await expect(menu(main).getByRole('menuitem', { name: 'Pin' })).toBeVisible();
  await shot(main, '01-note-menu-pin');
  await menu(main).getByRole('menuitem', { name: 'Pin' }).click();
  await expect.poll(() => titles(main)).toEqual(['Packing list', ...before.filter((t) => t !== 'Packing list')]);
  await expect(noteRow(main, 'Packing list').getByRole('img', { name: 'Pinned' })).toBeVisible();
  await expectStore(yap, 'note_pins.json', (s) => s.pins.some((p) => p.id === packing.id && p.created === packing.createdTs));
  // Never in notes.json (an older Yap rewriting it would drop it).
  expect(JSON.stringify(yap.readJson('notes.json'))).not.toContain('pinned');

  // The row's own pin button: both pinned ones first, the rest as they were.
  await noteRow(main, 'Groceries').hover();
  await main.getByRole('button', { name: 'Pin Groceries', exact: true }).click();
  await expect.poll(async () => (await titles(main)).slice(0, 2).sort()).toEqual(['Groceries', 'Packing list']);
  await expect.poll(async () => (await titles(main)).slice(2)).toEqual(before.filter((t) => !['Groceries', 'Packing list'].includes(t)));
  await expect(main.getByRole('button', { name: 'Unpin Groceries', exact: true })).toHaveAttribute('aria-pressed', 'true');
  await shot(main, '02-pinned-first');

  // Unpin from the menu: back in its place.
  await noteRow(main, 'Groceries').click({ button: 'right' });
  await menu(main).getByRole('menuitem', { name: 'Unpin' }).click();
  await expect.poll(() => titles(main)).toEqual(['Packing list', ...before.filter((t) => t !== 'Packing list')]);
  await expect(noteRow(main, 'Groceries').getByRole('img', { name: 'Pinned' })).toHaveCount(0);
  await expectStore(yap, 'note_pins.json', (s) => !s.pins.some((p) => p.id === groceries.id));

  // In a folder of its own: first there too.
  await yap.invoke('notes_folder_create', { name: 'Work' });
  await yap.invoke('note_create', { title: 'Budget', content: 'Q4', folder: 'Work' });
  const roadmap = await yap.invoke('note_create', { title: 'Roadmap', content: 'Next year', folder: 'Work' });
  await yap.invoke('note_create', { title: 'Hiring', content: 'Two roles', folder: 'Work' });
  await yap.invoke('note_pin', { id: roadmap.id, pinned: true });
  await folderRow(main, 'Work').click();
  await expect.poll(async () => (await titles(main))[0]).toBe('Roadmap');
  await expect(noteRow(main, 'Roadmap').getByRole('img', { name: 'Pinned' })).toBeVisible();

  // Deleted: its pin goes with it.
  await yap.invoke('note_delete', { id: packing.id });
  await expectStore(yap, 'note_pins.json', (s) => !s.pins.some((p) => p.id === packing.id) && s.pins.some((p) => p.id === roadmap.id));
});

test('Version history: what was there, typing and an Action, newest first; changes marked; Restore and Undo, each a version', async ({
  yap,
  main,
  shot,
  fakeLlm,
}) => {
  fakeLlm.reply('other', () => '## Plan\n- [ ] Ship the beta on Friday');
  const n = await yap.invoke('note_create', { title: 'Launch plan', content: 'Ship on Friday' });
  // An Action (Generate Notes) writes its summary.
  await yap.invoke('note_enhance', { id: n.id, actionId: 1 });
  await openView(main, 'Notes');
  await folderRow(main, 'Personal').click();
  await noteRow(main, 'Launch plan').click();
  await expect(raw(main)).toHaveValue('Ship on Friday');

  // Typing: folded into one version as it's saved.
  await raw(main).click();
  await raw(main).press('End');
  await raw(main).pressSequentially(' with the beta');
  await expectStore(yap, `note-versions/${n.id}.json`, (f) => f.versions.at(-1)?.text === 'Ship on Friday with the beta' && f.versions.at(-1)?.kind === 'typed');
  const file = versionsOf(yap, n.id);
  expect(file).toMatchObject({ note: n.id, created: n.createdTs });
  expect(file.versions.map((v) => [v.kind, v.field])).toEqual([
    ['action', 'summary'],
    ['earlier', 'notes'],
    ['typed', 'notes'],
  ]);
  // Never in notes.json.
  expect(Object.keys(noteIn(yap.readJson('notes.json'), n.id))).not.toContain('versions');

  // The note's ⋯ → Version history…
  await main.getByRole('button', { name: 'More for this note' }).click();
  await expect(menu(main).getByRole('menuitem')).toHaveText(['Pin', 'Transform note', 'Version history…', 'Copy as Markdown', 'Export…']);
  await menu(main).getByRole('menuitem', { name: 'Version history…' }).click();
  const dialog = history(main);
  await expect(dialog).toBeVisible();
  const rows = dialog.getByRole('option');
  await expect(rows.locator('.ikind')).toHaveText(['Typed', 'Earlier', 'Action: Generate Notes']);
  await expect(rows.locator('.ifield')).toHaveText(['My thoughts', 'My thoughts', 'Summary']);
  await expect(rows.nth(0)).toContainText('Current');
  await expect(rows.nth(1)).not.toContainText('Current');
  await expect(rows.nth(2)).toContainText('Current');
  await expect(rows.nth(0)).toBeFocused();
  // The newest: the words it put in, marked.
  await expect(dialog.locator('ins')).toHaveText(['put in: with the beta']);
  await expect(dialog).toContainText('1 change from the version before.');
  await expect(dialog.getByRole('button', { name: 'Restore this version' })).toBeDisabled();
  await shot(main, '03-version-history');

  // ↓: the one before; compared with now, what a restore takes out is struck.
  await main.keyboard.press('ArrowDown');
  await expect(rows.nth(1)).toHaveAttribute('aria-selected', 'true');
  await expect(dialog).toContainText('What the note said before its history began.');
  await dialog.getByRole('radio', { name: 'Compared with now' }).click();
  await expect(dialog.locator('del')).toHaveText(['taken out: with the beta']);
  await shot(main, '04-version-compared-with-now');

  // Restore: the note says it again; the toast's Undo puts it back.
  await dialog.getByRole('button', { name: 'Restore this version' }).click();
  await expect(dialog).toHaveCount(0);
  await expect(raw(main)).toHaveValue('Ship on Friday');
  await expectStore(yap, 'notes.json', (s) => noteIn(s, n.id)?.content === 'Ship on Friday');
  await toastWith(main, 'Version restored').getByRole('button', { name: 'Undo' }).click();
  await expect(raw(main)).toHaveValue('Ship on Friday with the beta');
  await expectStore(yap, `note-versions/${n.id}.json`, (f) => f.versions.slice(-2).every((v) => v.kind === 'restored'));
  await closeToasts(main);

  // Each restore is in the history, with where it came from.
  await main.getByRole('button', { name: 'More for this note' }).click();
  await menu(main).getByRole('menuitem', { name: 'Version history…' }).click();
  await expect(rows.locator('.ikind')).toHaveText(['Restored', 'Restored', 'Typed', 'Earlier', 'Action: Generate Notes']);
  await expect(rows.nth(1)).toContainText('from ');
  // Esc closes it, the focus back on the ⋯.
  await main.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);
  await expect(main.getByRole('button', { name: 'More for this note' })).toBeFocused();

  // Dark.
  await setConfig(yap, main, { theme: 'dark' });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe('dark');
  await noteRow(main, 'Launch plan').click({ button: 'right' });
  await menu(main).getByRole('menuitem', { name: 'Version history…' }).click();
  await expect(rows).toHaveCount(5);
  await shot(main, '05-version-history-dark');
  await main.keyboard.press('Escape');
  await setConfig(yap, main, { theme: 'light' });

  // Deleted: its history goes with it.
  await yap.invoke('note_delete', { id: n.id });
  await expect.poll(() => versionsOf(yap, n.id)).toBeNull();
});

const LINE = 'so basically we should like ship it on friday';
const POLISHED = 'We should ship it on Friday.';
const SHORT = 'Agenda: ship Friday. Thanks!';

test('a transform on the selection (Shift+F10, a right-click) and on the whole note (its ⋯) on the fake model; Undo; in Version history', async ({
  yap,
  main,
  shot,
  fakeLlm,
}) => {
  await setConfig(yap, main, { postProcessEnabled: true, ppProvider: 'custom', ppBaseUrl: fakeLlm.base, ppModel: 'fake-cleanup', ppApiKey: '' });
  fakeLlm.reply('transform', (user) => {
    const asked = transformInstruction(user);
    if (/^Polish/.test(asked)) return `Here is the polished text:\n${POLISHED}`;
    if (/shorter/i.test(asked)) return SHORT;
    return transformText(user);
  });
  const asked = () => fakeLlm.requests.filter((r) => r.kind === 'transform');
  const original = `Agenda\n${LINE}\nThanks all`;
  const n = await yap.invoke('note_create', { title: 'Release notes', content: original });
  await openView(main, 'Notes');
  await folderRow(main, 'Personal').click();
  await noteRow(main, 'Release notes').click();
  await expect(raw(main)).toHaveValue(original);

  // The middle line selected; Shift+F10: the transforms on it, then Cut, Copy, Select all.
  const start = 'Agenda\n'.length;
  await raw(main).evaluate((el, [a, b]) => {
    el.focus();
    el.setSelectionRange(a, b);
  }, [start, start + LINE.length]);
  await main.keyboard.press('Shift+F10');
  await expect(menu(main)).toHaveAccessibleName('Selected text');
  await expect(menu(main).getByRole('menuitem')).toHaveText(['Transform selection', 'Cut', 'Copy', 'Select all']);
  await menu(main).getByRole('menuitem', { name: 'Transform selection' }).hover();
  const sub = main.getByRole('menu', { name: 'Transform selection' });
  await expect(sub.getByRole('menuitem')).toHaveText(['Polish', 'Shorter', 'Fix grammar only', 'Translate to Spanish', 'Make it a prompt', 'Manage transforms…']);
  await shot(main, '06-transform-selection-menu');
  const before = asked().length;
  await sub.getByRole('menuitem', { name: 'Polish' }).click();

  // In place, its preamble off, the result selected; the AI got the selection only.
  const polished = `Agenda\n${POLISHED}\nThanks all`;
  await expect(raw(main)).toHaveValue(polished);
  expect(asked().length).toBe(before + 1);
  expect(transformText(asked().at(-1).user)).toBe(LINE);
  expect(transformInstruction(asked().at(-1).user)).toMatch(/^Polish the text/);
  await expect.poll(() => raw(main).evaluate((el) => el.value.slice(el.selectionStart, el.selectionEnd))).toBe(POLISHED);
  await expectStore(yap, 'notes.json', (s) => noteIn(s, n.id)?.content === polished);
  await expectStore(yap, `note-versions/${n.id}.json`, (f) => f.versions.at(-1)?.kind === 'transform' && f.versions.at(-1)?.label === 'Polish');
  const done = toastWith(main, 'Polish ✓');
  await expect(done).toBeVisible();
  await shot(main, '07-transform-done');

  // Undo: the text as it was.
  await done.getByRole('button', { name: 'Undo' }).click();
  await expect(raw(main)).toHaveValue(original);
  await expectStore(yap, 'notes.json', (s) => noteIn(s, n.id)?.content === original);
  await closeToasts(main);

  // A right-click on selected text: the same menu. With nothing selected,
  // the box's own (no menu of Yap's).
  await raw(main).evaluate((el) => {
    el.focus();
    el.select();
  });
  await raw(main).click({ button: 'right', position: { x: 14, y: 10 } });
  await expect(menu(main)).toHaveAccessibleName('Selected text');
  await main.keyboard.press('Escape');
  await expect(main.getByRole('menu')).toHaveCount(0);
  await raw(main).evaluate((el) => {
    el.focus();
    el.setSelectionRange(0, 0);
  });
  await raw(main).click({ button: 'right', position: { x: 14, y: 10 } });
  await expect(main.getByRole('menu')).toHaveCount(0);

  // The whole note, from its ⋯: Transform note ▸ Shorter.
  await raw(main).evaluate((el) => el.setSelectionRange(0, 0));
  await main.getByRole('button', { name: 'More for this note' }).click();
  await menu(main).getByRole('menuitem', { name: 'Transform note' }).hover();
  await main.getByRole('menu', { name: 'Transform note' }).getByRole('menuitem', { name: 'Shorter' }).click();
  await expect(raw(main)).toHaveValue(SHORT);
  expect(transformText(asked().at(-1).user)).toBe(original);
  await expectStore(yap, 'notes.json', (s) => noteIn(s, n.id)?.content === SHORT);
  await closeToasts(main);

  // Both in Version history, each marked with its name; the restore between them too.
  await main.getByRole('button', { name: 'More for this note' }).click();
  await menu(main).getByRole('menuitem', { name: 'Version history…' }).click();
  await expect(history(main).getByRole('option').locator('.ikind')).toHaveText(['Shorter', 'Restored', 'Polish', 'Earlier']);
  await shot(main, '08-version-history-transforms');
  await main.keyboard.press('Escape');

  // No AI model: said, with the way to set one up; the note as it was.
  await setConfig(yap, main, { ppBaseUrl: '', ppProvider: 'groq', ppApiKey: '' });
  await main.getByRole('button', { name: 'More for this note' }).click();
  await menu(main).getByRole('menuitem', { name: 'Transform note' }).hover();
  await main.getByRole('menu', { name: 'Transform note' }).getByRole('menuitem', { name: 'Polish' }).click();
  const failed = toastWith(main, "Polish didn't work");
  await expect(failed).toBeVisible();
  await expect(failed.getByRole('button', { name: 'Open Language Models' })).toBeVisible();
  await expect(raw(main)).toHaveValue(SHORT);
});
