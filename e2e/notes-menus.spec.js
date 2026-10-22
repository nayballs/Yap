// Right-click menus in Notes (NotesView.svelte + ui/ContextMenu.svelte) and
// the folder commands behind them (notes.rs `folder_rename` / `folder_delete`),
// plus the WebView2 context-menu filter (context_menu.rs) every window has.
//
// A folder row: Rename in place (its notes go along), Delete… after a
// question (its notes move to Personal); Personal and Meetings can't be
// either, in the menu or in Rust. A note row: Move to ▸, Read aloud, Copy as
// Markdown, Delete… (a recording meeting stops first, the notepad's way). The menus
// open from a right-click, the row's ⋯ and the keyboard (Shift+F10), close
// on Esc with the focus back on the row, and cancel the `contextmenu` event so
// WebView2's own menu never shows there. Elsewhere WebView2's menu is cut
// down to the editing commands: nothing on plain UI, Cut/Copy/Paste in a
// text field (test mode records the menu and never puts one on screen).
import { test, expect, openView, expectStore, closeToasts } from './support/fixtures.js';

// A folder's row: its name, then its count if it has notes ("Camping 2").
const folderRow = (main, name) =>
  main.locator('.folderlist').getByRole('button', { name: new RegExp(`^${name}( \\d+)?$`) });
const noteRow = (main, title) => main.locator('.items .item').filter({ hasText: title });
const menu = (main) => main.getByRole('menu').first();
const dialog = (main) => main.getByRole('alertdialog');
const folderOf = (yap, id) => yap.readJson('notes.json')?.notes.find((n) => n.id === id)?.folder;
const recording = (yap) => yap.invoke('meeting_state').then((s) => s.recording);
const visible = (yap, label) => yap.invoke('plugin:window|is_visible', { label });

test.afterEach(async ({ yap, main }) => {
  await main.keyboard.press('Escape');
  if (await recording(yap)) {
    await yap.invoke('meeting_pause');
    await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
  }
  if (await visible(yap, 'notepad')) {
    await yap.notepad.getByRole('button', { name: 'Close notepad' }).click();
  }
});

test('a folder is renamed in place and deleted after asking; its notes go to Personal', async ({
  yap,
  main,
  shot,
}) => {
  await yap.invoke('notes_folder_create', { name: 'Camping' });
  const tent = await yap.invoke('note_create', { title: 'Tent list', content: 'Pegs, poles', folder: 'Camping' });
  const stove = await yap.invoke('note_create', { title: 'Stove fuel', folder: 'Camping' });
  await openView(main, 'Notes');
  const before = yap.readJson('notes.json').folders;
  await folderRow(main, 'Camping').click();
  await noteRow(main, 'Tent list').click();
  await expect(main.getByPlaceholder('Untitled Note')).toHaveValue('Tent list');

  // Right-click the folder: Rename, Delete….
  await folderRow(main, 'Camping').click({ button: 'right' });
  await expect(menu(main)).toBeVisible();
  await expect(menu(main)).toHaveAccessibleName('Folder Camping');
  await expect(menu(main).getByRole('menuitem')).toHaveText(['Rename', 'Delete…']);
  await shot(main, '01-folder-menu');

  // Rename: an input in the row; Esc keeps the name…
  await menu(main).getByRole('menuitem', { name: 'Rename' }).click();
  const input = main.getByRole('textbox', { name: 'Rename folder Camping' });
  await expect(input).toBeFocused();
  await input.fill('Trips');
  await shot(main, '02-folder-rename');
  await input.press('Escape');
  await expect(folderRow(main, 'Camping')).toBeVisible();
  expect(yap.readJson('notes.json').folders).toContain('Camping');

  // …a name another folder has (any case) is refused…
  await folderRow(main, 'Camping').click({ button: 'right' });
  await menu(main).getByRole('menuitem', { name: 'Rename' }).click();
  await input.fill('personal');
  await input.press('Enter');
  await expect(main.getByRole('status').filter({ hasText: "Couldn't rename the folder" })).toContainText(
    "There's already a folder called personal"
  );
  await closeToasts(main);

  // …Enter renames it, in its place, and the notes follow.
  await folderRow(main, 'Camping').click({ button: 'right' });
  await menu(main).getByRole('menuitem', { name: 'Rename' }).click();
  await input.fill('  Trips ');
  await input.press('Enter');
  await expect(folderRow(main, 'Trips')).toBeVisible();
  await expect(folderRow(main, 'Trips')).toBeFocused();
  await expect(folderRow(main, 'Camping')).toHaveCount(0);
  await expectStore(yap, 'notes.json', (s) =>
    s.folders.indexOf('Trips') === before.indexOf('Camping') &&
    !s.folders.includes('Camping') &&
    s.notes.filter((n) => n.folder === 'Trips').length === 2
  );
  // Still the folder on screen, and the open note's folder chip says so.
  await expect(folderRow(main, 'Trips')).toHaveClass(/active/);
  await expect(noteRow(main, 'Tent list')).toBeVisible();
  await expect(main.locator('.metarow .chip').filter({ hasText: 'Trips' })).toBeVisible();
  await shot(main, '03-folder-renamed');

  // Delete… asks first, saying where its notes go. Cancel keeps it.
  await folderRow(main, 'Trips').click({ button: 'right' });
  await menu(main).getByRole('menuitem', { name: 'Delete…' }).click();
  await expect(dialog(main)).toHaveAccessibleName('Delete “Trips”?');
  await expect(dialog(main)).toContainText('Its 2 notes will move to Personal.');
  await expect(dialog(main).getByRole('button', { name: 'Cancel' })).toBeFocused();
  await shot(main, '04-folder-delete-confirm');
  await dialog(main).getByRole('button', { name: 'Cancel' }).click();
  await expect(dialog(main)).toHaveCount(0);
  await expect(folderRow(main, 'Trips')).toBeFocused();

  // Delete: the folder goes, its notes are in Personal, and so is the view.
  await folderRow(main, 'Trips').click({ button: 'right' });
  await menu(main).getByRole('menuitem', { name: 'Delete…' }).click();
  await dialog(main).getByRole('button', { name: 'Delete' }).click();
  await expect(dialog(main)).toHaveCount(0);
  await expect(folderRow(main, 'Trips')).toHaveCount(0);
  await expect(folderRow(main, 'Personal')).toHaveClass(/active/);
  await expectStore(yap, 'notes.json', (s) => !s.folders.includes('Trips'));
  expect(folderOf(yap, tent.id)).toBe('Personal');
  expect(folderOf(yap, stove.id)).toBe('Personal');
  await expect(noteRow(main, 'Tent list')).toBeVisible();
  await expect(main.locator('.metarow .chip').filter({ hasText: 'Personal' })).toBeVisible();
  await shot(main, '05-folder-deleted');

  // An empty folder says so.
  await yap.invoke('notes_folder_create', { name: 'Empty one' });
  await main.getByRole('button', { name: 'New note' }).first().click(); // refreshes the list
  await folderRow(main, 'Empty one').click({ button: 'right' });
  await menu(main).getByRole('menuitem', { name: 'Delete…' }).click();
  await expect(dialog(main)).toContainText('The folder is empty.');
  await dialog(main).getByRole('button', { name: 'Delete' }).click();
  await expect(folderRow(main, 'Empty one')).toHaveCount(0);
});

test("Personal and Meetings can't be renamed or deleted, in the menu or in Rust", async ({ yap, main, shot }) => {
  await openView(main, 'Notes');
  for (const [name, why] of [
    ['Personal', 'new notes go here.'],
    ['Meetings', 'meeting notes go here.'],
  ]) {
    await folderRow(main, name).click({ button: 'right' });
    const items = menu(main).getByRole('menuitem');
    await expect(items).toHaveText(['Rename', 'Delete…']);
    for (const item of await items.all()) await expect(item).toHaveAttribute('aria-disabled', 'true');
    await expect(menu(main)).toContainText(`Yap's own folder: ${why}`);
    if (name === 'Meetings') await shot(main, '06-protected-folder-menu');
    // A disabled item does nothing; Esc closes, the focus back on the row.
    await items.first().click({ force: true });
    await expect(menu(main)).toBeVisible();
    await main.keyboard.press('Escape');
    await expect(menu(main)).toHaveCount(0);
    await expect(folderRow(main, name)).toBeFocused();
  }
  await expect(yap.invoke('notes_folder_rename', { from: 'personal', to: 'Mine' })).rejects.toThrow(
    "can't be renamed"
  );
  await expect(yap.invoke('notes_folder_delete', { name: 'Meetings' })).rejects.toThrow("can't be deleted");
  expect(yap.readJson('notes.json')?.folders.slice(0, 2) ?? ['Personal', 'Meetings']).toEqual(['Personal', 'Meetings']);
});

test('the keyboard and the ⋯ button open the same menus; the page never gets WebView2’s', async ({ main }) => {
  await openView(main, 'Notes');
  const personal = folderRow(main, 'Personal');

  // Shift+F10 on a focused row: the menu, its first item focused; arrows move.
  await personal.focus();
  await main.keyboard.press('Shift+F10');
  await expect(menu(main)).toBeVisible();
  await main.keyboard.press('Escape');
  await expect(personal).toBeFocused();

  await yap_folder(main, 'Keys');
  const keys = folderRow(main, 'Keys');
  await keys.focus();
  await main.keyboard.press('Shift+F10');
  const rename = menu(main).getByRole('menuitem', { name: 'Rename' });
  await expect(rename).toBeFocused();
  await main.keyboard.press('ArrowDown');
  await expect(menu(main).getByRole('menuitem', { name: 'Delete…' })).toBeFocused();
  await main.keyboard.press('ArrowDown'); // wraps
  await expect(rename).toBeFocused();
  await main.keyboard.press('Enter');
  await expect(main.getByRole('textbox', { name: 'Rename folder Keys' })).toBeFocused();
  await main.keyboard.press('Escape');
  await expect(keys).toBeFocused();

  // The ⋯ button (shown on hover) opens it under itself.
  await keys.hover();
  await main.getByRole('button', { name: 'More for folder Keys' }).click();
  await expect(menu(main)).toHaveAccessibleName('Folder Keys');
  // A click outside closes it.
  await main.locator('.editor').click({ position: { x: 300, y: 300 } });
  await expect(menu(main)).toHaveCount(0);

  // Yap's rows cancel the contextmenu event (WebView2 shows nothing there);
  // plain UI doesn't (WebView2's menu is filtered natively instead).
  const prevented = (selector) =>
    main.evaluate((sel) => {
      const ev = new MouseEvent('contextmenu', { bubbles: true, cancelable: true, button: 2, clientX: 40, clientY: 40 });
      document.querySelector(sel).dispatchEvent(ev);
      return ev.defaultPrevented;
    }, selector);
  expect(await prevented('.folderlist .folder')).toBe(true);
  await main.keyboard.press('Escape');
  await expect(menu(main)).toHaveCount(0);
  expect(await prevented('.seccap span')).toBe(false);

  await main.evaluate(() => window.__TAURI_INTERNALS__.invoke('notes_folder_delete', { name: 'Keys' }));
});

/** A folder, made through the command and shown (a list refresh). */
async function yap_folder(main, name) {
  await main.evaluate((n) => window.__TAURI_INTERNALS__.invoke('notes_folder_create', { name: n }), name);
  await main.getByRole('button', { name: 'New note' }).first().click();
  await expect(folderRow(main, name)).toBeVisible();
}

test('a note row: Move to, Copy as Markdown, Delete…; a recording meeting stops first', async ({
  yap,
  main,
  shot,
}) => {
  await yap.invoke('notes_folder_create', { name: 'Work' });
  const plan = await yap.invoke('note_create', { title: 'Launch plan', content: 'Ship it Friday.' });
  await openView(main, 'Notes');
  await folderRow(main, 'Personal').click();
  const row = noteRow(main, 'Launch plan');
  await expect(row).toBeVisible();

  // Right-click: the note's menu.
  await row.click({ button: 'right' });
  await expect(menu(main)).toHaveAccessibleName('Note Launch plan');
  await expect(menu(main).getByRole('menuitem')).toHaveText([
    'Rename',
    'Move to',
    'Read aloud',
    'Copy as Markdown',
    'Export…',
    'Delete…',
  ]);
  // Move to ▸: the folders, this one ticked.
  await menu(main).getByRole('menuitem', { name: 'Move to' }).hover();
  const sub = main.getByRole('menu', { name: 'Move to' });
  await expect(sub.getByRole('menuitemradio')).toHaveText(yap.readJson('notes.json').folders);
  await expect(sub.getByRole('menuitemradio', { name: 'Personal' })).toHaveAttribute('aria-checked', 'true');
  await shot(main, '07-note-menu-move-to');
  await sub.getByRole('menuitemradio', { name: 'Work' }).click();
  await expect(main.getByRole('menu')).toHaveCount(0);
  await expectStore(yap, 'notes.json', (s) => s.notes.find((n) => n.id === plan.id)?.folder === 'Work');
  await expect(row).toHaveCount(0); // it left the folder on screen
  await closeToasts(main);

  // From the keyboard, in Work: → opens Move to, ← comes back.
  await folderRow(main, 'Work').click();
  await row.focus();
  await main.keyboard.press('Shift+F10');
  await main.keyboard.press('ArrowDown');
  await main.keyboard.press('ArrowRight');
  await expect(sub.getByRole('menuitemradio', { name: 'Personal' })).toBeFocused();
  await main.keyboard.press('ArrowLeft');
  await expect(menu(main).getByRole('menuitem', { name: 'Move to' })).toBeFocused();
  await main.keyboard.press('Escape');
  await expect(row).toBeFocused();

  // Copy as Markdown (the page's clipboard stubbed: never the real one).
  await main.evaluate(() => {
    window.__copied = null;
    navigator.clipboard.writeText = async (t) => {
      window.__copied = t;
    };
  });
  await row.click({ button: 'right' });
  await menu(main).getByRole('menuitem', { name: 'Copy as Markdown' }).click();
  await expect.poll(() => main.evaluate(() => window.__copied)).toBe('# Launch plan\n\nShip it Friday.\n');
  await closeToasts(main);

  // Delete… asks; Delete removes it.
  await row.click({ button: 'right' });
  await menu(main).getByRole('menuitem', { name: 'Delete…' }).click();
  await expect(dialog(main)).toHaveAccessibleName('Delete “Launch plan”?');
  await expect(dialog(main)).toContainText('The note goes for good.');
  await dialog(main).getByRole('button', { name: 'Delete' }).click();
  await expectStore(yap, 'notes.json', (s) => !s.notes.some((n) => n.id === plan.id));
  await expect(row).toHaveCount(0);

  // A meeting recording into its note: Delete stops it first (meeting_delete),
  // never leaving a recording without its note; the notepad lets go.
  const live = await yap.invoke('note_create', { title: 'Live sync', folder: 'Meetings' });
  await yap.invoke('meeting_start', { noteId: live.id });
  await expect.poll(() => recording(yap)).toBe(true);
  await expect.poll(() => visible(yap, 'notepad')).toBe(true);
  await expect(note_delete(yap, live.id)).rejects.toThrow('still recording');
  await openView(main, 'Home'); // back to Notes: the list as stored now
  await openView(main, 'Notes');
  await folderRow(main, 'Meetings').click();
  await noteRow(main, 'Live sync').click({ button: 'right' });
  await menu(main).getByRole('menuitem', { name: 'Delete…' }).click();
  await expect(dialog(main)).toContainText('The recording stops first.');
  await shot(main, '08-delete-recording-meeting');
  await dialog(main).getByRole('button', { name: 'Delete' }).click();
  await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
  await expectStore(yap, 'notes.json', (s) => !s.notes.some((n) => n.id === live.id));
  await expect.poll(() => visible(yap, 'notepad')).toBe(false);
  await expect(noteRow(main, 'Live sync')).toHaveCount(0);
});

const note_delete = (yap, id) => yap.invoke('note_delete', { id });

test('WebView2’s own menu: nothing on plain UI, the edit commands in a text field', async ({ yap, main }) => {
  const menus = () => yap.invoke('e2e_context_menus');
  // Every window is hooked.
  const hooks = await menus();
  for (const label of ['settings', 'onboarding', 'notepad', 'overlay']) {
    expect(hooks[label]?.hooked, `${label} hooked`).toBe(true);
  }
  const seen = () => menus().then((m) => m.settings.menus);
  const last = () => menus().then((m) => m.settings.last);
  const kept = (m) => m.items.filter((i) => i.kept && i.kind !== 'separator').map((i) => i.name);

  // Plain UI: Back, Refresh, Save as, Print, Inspect… all dropped — Inspect
  // too, though this is a debug build: only a Shift+right-click offers it
  // there (context_menu.rs), never a plain one; no menu on screen.
  await openView(main, 'Home');
  let before = await seen();
  await main.getByRole('navigation', { name: 'Main' }).click({ button: 'right', position: { x: 20, y: 200 } });
  await expect.poll(seen).toBeGreaterThan(before);
  let m = await last();
  expect(m.editable).toBe(false);
  expect(m.items.map((i) => i.name)).toEqual(expect.arrayContaining(['back', 'reload', 'saveAs', 'print']));
  expect(kept(m)).toEqual([]);
  expect(m.inspect).toBe(false);
  expect(m.handled).toBe(true);

  // A text field: the edit commands, nothing that navigates or prints.
  await openView(main, 'Notes');
  await main.getByRole('button', { name: 'New note' }).first().click();
  const raw = main.getByPlaceholder('Start writing…');
  await raw.fill('Some text to edit');
  before = await seen();
  await raw.click({ button: 'right', position: { x: 300, y: 80 } });
  await expect.poll(seen).toBeGreaterThan(before);
  m = await last();
  expect(m.editable).toBe(true);
  expect(kept(m)).toEqual(expect.arrayContaining(['cut', 'copy', 'paste', 'selectAll']));
  for (const name of kept(m)) {
    expect(['undo', 'redo', 'cut', 'copy', 'paste', 'pasteAndMatchStyle', 'selectAll', 'emoji', 'spellcheck']).toContain(name);
  }
  // Test mode puts no native menu on screen at all.
  expect(m.handled).toBe(true);
});
