// The quick note (src-tauri/src/notes/quick_note.rs, src/lib/quicknote/QuickNote.svelte):
// a small window on its own shortcut (Settings → Shortcuts, unbound by
// default), the tray's "New quick note" and the Notes view's "Quick note",
// to jot or dictate a note from any app. Saved as you type as an ordinary
// note (`source` "quick", the "Quick notes" folder, its first words as the
// title); a blank note is never kept; it opens on the last quick note
// changed within a day, else a new one; Esc or ✕ puts it away and the
// shortcut again while it's in front does too; "Open in Notes".
//
// A test run has no hook and never takes the focus: `e2e_shortcut
// {name: 'quick-note'}` sends the shortcut's press as the hook would, the
// tray is driven through the debug-only `e2e_tray`, and in a test run the
// window counts as in front while it's shown (it takes a dictation then,
// `quick_note::takes_dictation`). Dictations come from
// `e2e_quick_note_dictation` (its mic, a transcript) and `e2e_dictate_audio`
// (a real recording of a tone in the mic's place, the stub's text).
import { test as base, expect, openView, openSettings, closeSettings, expectStore, setConfig, pressHotkey } from './support/fixtures.js';
import { dictationPiece, withPiece, statusLine } from '../src/lib/quicknote/quickNote.js';

const NOW = Math.floor(Date.now() / 1000);
// Two quick notes from before: one from an hour ago (the one it opens on),
// one left alone for three days; and a note of the person's own.
const SEED = {
  folders: ['Personal', 'Meetings', 'Quick notes'],
  notes: [
    { id: 1, title: 'Garden ideas', titleAuto: true, content: 'Garden ideas for the spring', folder: 'Quick notes', source: 'quick', noteType: 'personal', createdTs: NOW - 3 * 86_400, updatedTs: NOW - 3 * 86_400 },
    { id: 2, title: 'Ask Sam about the boiler', titleAuto: true, content: 'Ask Sam about the boiler', folder: 'Quick notes', source: 'quick', noteType: 'personal', createdTs: NOW - 3_600, updatedTs: NOW - 3_600 },
    { id: 3, title: 'Book club', content: 'Next book: a mystery', folder: 'Personal', source: 'manual', noteType: 'personal', createdTs: NOW - 7_200, updatedTs: NOW - 7_200 },
  ],
};

const test = base.extend({
  yapOptions: [{ name: 'quick-note', data: { 'notes.json': SEED } }, { scope: 'worker', option: true }],
});

const visible = (yap) => yap.invoke('plugin:window|is_visible', { label: 'quick-note' });
const note = (yap) => yap.quickNote.getByRole('textbox', { name: 'Quick note' });
const notes = (yap) => yap.readJson('notes.json')?.notes || [];
const quickNotes = (yap) => notes(yap).filter((n) => n.source === 'quick');
const byId = (yap, id) => notes(yap).find((n) => n.id === id);
const shortcut = (yap) => yap.invoke('e2e_shortcut', { name: 'quick-note' });
const status = (yap) => yap.quickNote.locator('.foot').getByRole('status');

/** The window up (by its shortcut), and the page caught up with it. */
async function openIt(yap) {
  if (!(await visible(yap))) await shortcut(yap);
  await expect.poll(() => visible(yap)).toBe(true);
}

/** Put away (Esc), as the person would. */
async function putAway(yap) {
  if (!(await visible(yap))) return;
  await note(yap).press('Escape');
  await expect.poll(() => visible(yap)).toBe(false);
}

/** A key going down and up in a page, as a keyboard does (F13 and up: CDP directly). */
async function pressKey(page, key, vk) {
  const cdp = await page.context().newCDPSession(page);
  try {
    const ev = { key, code: key, windowsVirtualKeyCode: vk, nativeVirtualKeyCode: vk };
    await cdp.send('Input.dispatchKeyEvent', { type: 'rawKeyDown', ...ev });
    await cdp.send('Input.dispatchKeyEvent', { type: 'keyUp', ...ev });
  } finally {
    await cdp.detach().catch(() => {});
  }
}

test.afterEach(async ({ yap }) => {
  await putAway(yap);
});

test('a dictation goes in at the cursor, spaced as typing would (pure)', () => {
  expect(dictationPiece('Buy', '', 'eggs')).toBe(' eggs');
  expect(dictationPiece('Buy ', 'and flour', 'eggs')).toBe('eggs ');
  expect(dictationPiece('Buy eggs', '. Then', 'and milk')).toBe(' and milk');
  expect(dictationPiece('', '', 'Buy eggs')).toBe('Buy eggs');
  expect(dictationPiece('Line one\n', '', 'Line two')).toBe('Line two');
  // "press enter" said: a new line after it.
  expect(dictationPiece('List:', '', 'eggs', true)).toBe(' eggs\n');
  expect(dictationPiece('', '', '', true)).toBe('\n');
  expect(dictationPiece('x', 'y', '  ')).toBe('');
  expect(withPiece('Buy flour', 4, 4, 'eggs and ')).toEqual({ value: 'Buy eggs and flour', caret: 13 });
  expect(withPiece('Buy flour', 4, 9, 'eggs')).toEqual({ value: 'Buy eggs', caret: 8 });
  expect(statusLine('saved', 'Quick notes')).toBe('Saved in Quick notes');
  expect(statusLine('new', '')).toBe('Saved as you type, in Notes');
});

test('the shortcut brings it up on the last quick note from today; Esc and the shortcut put it away', async ({ yap, main, shot }) => {
  expect(await visible(yap)).toBe(false);
  await shortcut(yap);
  await expect.poll(() => visible(yap)).toBe(true);
  // The one from an hour ago, not the one left alone for three days.
  await expect(note(yap)).toHaveValue('Ask Sam about the boiler');
  await expect(yap.quickNote.locator('.hdr .title')).toHaveText('Ask Sam about the boiler');
  await expect(note(yap)).toBeFocused();
  await expect(status(yap)).toHaveText('Saved in Quick notes');
  await shot(yap.quickNote, '01-opened-on-the-last-quick-note');

  // The shortcut again while it's in front: away.
  await shortcut(yap);
  await expect.poll(() => visible(yap)).toBe(false);
  // Back on the same note; Esc puts it away too.
  await shortcut(yap);
  await expect.poll(() => visible(yap)).toBe(true);
  await expect(note(yap)).toHaveValue('Ask Sam about the boiler');
  await note(yap).press('Escape');
  await expect.poll(() => visible(yap)).toBe(false);
  // ✕ as well.
  await shortcut(yap);
  await expect.poll(() => visible(yap)).toBe(true);
  await yap.quickNote.getByRole('button', { name: 'Close' }).click();
  await expect.poll(() => visible(yap)).toBe(false);
  // Hidden, its webview keeps less in memory (shell/webview_memory.rs).
  await expect.poll(() => yap.invoke('e2e_webview_memory'), { timeout: 20_000 }).toMatchObject({ 'quick-note': 'low' });
});

test('saved as you type as a quick note: its first words the title, in the Quick notes folder, never a blank one', async ({ yap, main, shot }) => {
  await openIt(yap);
  // A new note: nothing is made until there's a word.
  const before = quickNotes(yap).length;
  await yap.quickNote.getByRole('button', { name: 'New note' }).click();
  await expect(note(yap)).toHaveValue('');
  await expect(yap.quickNote.locator('.hdr .title')).toHaveText('New quick note');
  await expect(note(yap)).toBeFocused();
  await yap.quickNote.waitForTimeout(700);
  expect(quickNotes(yap).length).toBe(before);

  await note(yap).pressSequentially('Call the plumber about the leak under the sink');
  await expectStore(yap, 'notes.json', (s) => s.notes.some((n) => n.content === 'Call the plumber about the leak under the sink'));
  const made = quickNotes(yap).find((n) => n.content.startsWith('Call the plumber'));
  expect(made).toMatchObject({ source: 'quick', folder: 'Quick notes', title: 'Call the plumber about the leak…', titleAuto: true, noteType: 'personal' });
  expect(quickNotes(yap).length).toBe(before + 1);
  await expect(yap.quickNote.locator('.hdr .title')).toHaveText('Call the plumber about the leak…');
  await expect(status(yap)).toHaveText('Saved in Quick notes');
  await shot(yap.quickNote, '02-saved-as-you-type');

  // A second line doesn't change the title; the text is saved as typed.
  await note(yap).press('Enter');
  await note(yap).pressSequentially('Before Friday');
  await expectStore(yap, 'notes.json', (s) => byIdIn(s, made.id)?.content === 'Call the plumber about the leak under the sink\nBefore Friday');
  expect(byId(yap, made.id).title).toBe('Call the plumber about the leak…');

  // In Notes: the Quick notes folder, the note in it.
  await openView(main, 'Notes');
  const folder = main.locator('.folderlist').getByRole('button', { name: /^Quick notes( \d+)?$/ });
  await folder.click();
  const row = main.locator('.items .item').filter({ hasText: 'Call the plumber about the leak…' });
  await expect(row).toBeVisible();
  await row.click();
  await expect(main.getByPlaceholder('Untitled Note')).toHaveValue('Call the plumber about the leak…');
  await shot(main, '03-the-quick-note-in-notes');

  // Emptied again: the note goes (a blank note is never kept).
  await note(yap).fill('');
  await expectStore(yap, 'notes.json', (s) => !s.notes.some((n) => n.id === made.id));
  await expect(status(yap)).toHaveText('Saved as you type, in Notes');
  await expect(row).toHaveCount(0);
  expect(quickNotes(yap).length).toBe(before);
});

function byIdIn(store, id) {
  return store?.notes?.find((n) => n.id === id);
}

test('a save that failed is said with Try again, and the note is still one note', async ({ yap, shot }) => {
  await openIt(yap);
  await yap.quickNote.getByRole('button', { name: 'New note' }).click();
  // The new note's two writes fail, as on a full disk (shell/saving.rs).
  await yap.invoke('e2e_fail_saves', { store: 'notes', count: 2 });
  await note(yap).pressSequentially('Fix the gate');
  await expect(status(yap)).toHaveText('Not saved yet: Yap will try again with your next change');
  const retry = yap.quickNote.getByRole('button', { name: 'Try again' });
  await expect(retry).toBeVisible();
  expect(quickNotes(yap).some((n) => n.content === 'Fix the gate')).toBe(false);
  await shot(yap.quickNote, '08-not-saved-try-again');
  await retry.click();
  await expectStore(yap, 'notes.json', (s) => s.notes.filter((n) => n.content === 'Fix the gate').length === 1);
  await expect(status(yap)).toHaveText('Saved in Quick notes');
  await expect(retry).toHaveCount(0);
  // Typed on: the same note (its id reached the window), not a second one.
  await note(yap).pressSequentially(' latch');
  await expectStore(yap, 'notes.json', (s) => s.notes.some((n) => n.content === 'Fix the gate latch'));
  expect(quickNotes(yap).filter((n) => n.content.startsWith('Fix the gate')).length).toBe(1);
});

test('"Open in Notes" shows the note in the main window and steps aside; Notes and the window follow each other', async ({ yap, main }) => {
  await openIt(yap);
  await yap.quickNote.getByRole('button', { name: 'New note' }).click();
  await note(yap).pressSequentially('Renew the parking permit');
  await expectStore(yap, 'notes.json', (s) => s.notes.some((n) => n.content === 'Renew the parking permit'));
  const id = quickNotes(yap).find((n) => n.content === 'Renew the parking permit').id;
  await openView(main, 'Home');
  await yap.quickNote.getByRole('button', { name: 'Open in Notes' }).click();
  await expect.poll(() => visible(yap)).toBe(false);
  await expect(main.getByPlaceholder('Untitled Note')).toHaveValue('Renew the parking permit');
  await expect(main.locator('.folder.active')).toContainText('Quick notes');

  // Named in Notes: the window keeps that name as it's typed into.
  await main.getByPlaceholder('Untitled Note').fill('Car');
  await expectStore(yap, 'notes.json', (s) => byIdIn(s, id)?.title === 'Car');
  await openIt(yap);
  await expect(note(yap)).toHaveValue('Renew the parking permit');
  await expect(yap.quickNote.locator('.hdr .title')).toHaveText('Car');
  await note(yap).press('End');
  await note(yap).pressSequentially(' online');
  await expectStore(yap, 'notes.json', (s) => byIdIn(s, id)?.content === 'Renew the parking permit online');
  expect(byId(yap, id)).toMatchObject({ title: 'Car' });
  // …and Notes follows the window's saves.
  await expect(main.locator('.items .item').filter({ hasText: 'Car' })).toBeVisible();
});

test('dictating into it: at the cursor, cleaned up as any dictation, never pasted; Undo of a cancelled one lands there too', async ({ yap, main, shot }) => {
  await openIt(yap);
  await yap.quickNote.getByRole('button', { name: 'New note' }).click();
  // Its mic (a transcript in the mic's place): the note's, not a paste.
  const said = await yap.invoke('e2e_quick_note_dictation', { text: 'Pick up the parcel from the post office' });
  expect(said.kind).toBe('the quick note');
  expect(said.leftOnClipboard).toBeFalsy();
  await expect(note(yap)).toHaveValue('Pick up the parcel from the post office');
  await expectStore(yap, 'notes.json', (s) => s.notes.some((n) => n.source === 'quick' && n.content === 'Pick up the parcel from the post office'));
  const id = quickNotes(yap).find((n) => n.content.startsWith('Pick up the parcel')).id;
  // Kept in history like any dictation.
  await expectStore(yap, 'history.json', (h) => h.some((e) => e.text === 'Pick up the parcel from the post office'));

  // At the cursor, spaced as typing would be.
  await note(yap).evaluate((el) => {
    el.focus();
    el.setSelectionRange(11, 11); // after "Pick up the"
  });
  await yap.invoke('e2e_quick_note_dictation', { text: 'big' });
  await expect(note(yap)).toHaveValue('Pick up the big parcel from the post office');
  // "press enter" said: a new line after it.
  await note(yap).press('End');
  await yap.invoke('e2e_quick_note_dictation', { text: 'today, press enter' });
  await expect(note(yap)).toHaveValue(/^Pick up the big parcel from the post office today\.?\n$/);
  await expectStore(yap, 'notes.json', (s) => byIdIn(s, id)?.content.startsWith('Pick up the big parcel from the post office today'));
  await shot(yap.quickNote, '04-dictated-into-the-note');

  // A real recording (a tone in the mic's place) while the window is in front: the note's.
  const recorded = await yap.invoke('e2e_dictate_audio', { seconds: 1 });
  expect(recorded.kind).toBe('the quick note');
  await expect(note(yap)).toHaveValue(/STT stub/);

  // Cancelled, then Undo on the bar: transcribed into the note, not pasted.
  const length = (await note(yap).inputValue()).length;
  expect(await yap.invoke('e2e_dictate_audio', { seconds: 1.2, cancel: true })).toBeNull();
  await yap.invoke('bar_action', { action: 'dictation-undo' });
  await expect.poll(async () => (await note(yap).inputValue()).length).toBeGreaterThan(length + 10);
  await expectStore(yap, 'notes.json', (s) => (byIdIn(s, id)?.content.match(/STT stub/g) || []).length === 2);
});

test('the dictation hotkey with the window in front: into the note (in-page fallback)', async ({ yap }) => {
  const mics = await yap.invoke('list_audio_devices');
  test.skip(mics.length === 0, 'no microphone here; the dictation tests above use a tone in its place');
  await openIt(yap);
  await yap.quickNote.getByRole('button', { name: 'New note' }).click();
  await pressHotkey(yap.quickNote);
  await expect(yap.overlay.locator('.capsule')).toBeVisible();
  await yap.quickNote.waitForTimeout(1_200);
  await pressHotkey(yap.quickNote);
  await expect(note(yap)).toHaveValue(/STT stub/, { timeout: 15_000 });
  const typed = await note(yap).inputValue();
  await expectStore(yap, 'notes.json', (s) => s.notes.some((n) => n.source === 'quick' && n.content === typed));
});

test('the tray has "New quick note": the window on a new note; the last one stays in Notes', async ({ yap, shot }) => {
  await openIt(yap);
  await yap.quickNote.getByRole('button', { name: 'New note' }).click();
  await note(yap).pressSequentially('Water the tomatoes');
  await expectStore(yap, 'notes.json', (s) => s.notes.some((n) => n.content === 'Water the tomatoes'));
  await putAway(yap);

  const items = await yap.invoke('e2e_tray');
  expect(items).toContainEqual(['quick_note', 'New quick note']);
  await yap.invoke('e2e_tray', { click: 'quick_note' });
  await expect.poll(() => visible(yap)).toBe(true);
  // A new note, though the last one is minutes old.
  await expect(note(yap)).toHaveValue('');
  await expect(yap.quickNote.locator('.hdr .title')).toHaveText('New quick note');
  expect(quickNotes(yap).some((n) => n.content === 'Water the tomatoes')).toBe(true);
  await shot(yap.quickNote, '05-new-from-the-tray');
  // Put away untouched and opened again: still the new note (nothing kept for it).
  await putAway(yap);
  await shortcut(yap);
  await expect.poll(() => visible(yap)).toBe(true);
  await expect(note(yap)).toHaveValue('');
});

test('the Notes view has "Quick note"', async ({ yap, main }) => {
  await openView(main, 'Notes');
  await main.locator('.tools').getByRole('button', { name: 'Quick note' }).click();
  await expect.poll(() => visible(yap)).toBe(true);
});

test('Settings → Shortcuts: Quick note is unbound until set, refuses another shortcut\'s key, and works from Yap\'s own windows', async ({ yap, main, shot }) => {
  expect(yap.readConfig().quickNoteHotkey ?? '').toBe('');
  const dialog = await openSettings(main, 'Shortcuts');
  const row = dialog.locator('[data-setting="shortcut-quick-note"]');
  await expect(row).toContainText('Quick note');
  await expect(row).toContainText('Jot or dictate a note from any app');
  await expect(row.getByRole('button', { name: 'Set a shortcut' })).toBeVisible();

  // The dictation hotkey (F24 in the suite) refused; F21 taken.
  await row.getByRole('button', { name: 'Set a shortcut' }).click();
  await pressKey(main, 'F24', 135);
  await expect(row).toContainText('Already used by the dictation hotkey');
  await pressKey(main, 'F21', 132);
  await expectStore(yap, 'config.json', (c) => c.quickNoteHotkey === 'kb:132');
  await expect(row.getByRole('button', { name: /^F21\s*Change$/ })).toBeVisible();
  await expect.poll(() => yap.invoke('e2e_hotkey_bindings').then((b) => b.actions.quickNoteHotkey[0].key)).not.toBe('none');
  await row.scrollIntoViewIfNeeded();
  await shot(main, '06-shortcuts-quick-note');
  // Settings' search finds it.
  const search = dialog.getByRole('searchbox', { name: 'Search settings' });
  await search.fill('quick note');
  await expect(dialog.getByRole('listbox', { name: 'Settings found' }).getByRole('option').first()).toContainText('Quick note');
  await search.fill('');
  await closeSettings(main);

  // Pressed in the main window (its in-page fallback): up it comes…
  await pressKey(main, 'F21', 132);
  await expect.poll(() => visible(yap)).toBe(true);
  // …and pressed in the window itself, in front: away.
  await pressKey(yap.quickNote, 'F21', 132);
  await expect.poll(() => visible(yap)).toBe(false);

  // Removed again.
  await openSettings(main, 'Shortcuts');
  await row.getByRole('button', { name: 'Remove hotkey' }).click();
  await expectStore(yap, 'config.json', (c) => c.quickNoteHotkey === '');
  await closeSettings(main);
});

test('light and dark: the paper and ink of the notepad, and the webview behind it', async ({ yap, main, shot }) => {
  await openIt(yap);
  const bg = () => yap.quickNote.evaluate(() => getComputedStyle(document.body).backgroundColor);
  expect(await bg()).toBe('rgb(250, 249, 246)');
  expect(await yap.invoke('e2e_webview_background', { label: 'quick-note' })).toBe('#faf9f6');
  await setConfig(yap, main, { theme: 'dark' });
  await expect.poll(() => yap.quickNote.evaluate(() => document.documentElement.dataset.yapTheme)).toBe('dark');
  await expect.poll(bg).toBe('rgb(28, 26, 22)');
  await expect.poll(() => yap.invoke('e2e_webview_background', { label: 'quick-note' })).toBe('#1c1a16');
  await yap.quickNote.getByRole('button', { name: 'New note' }).click();
  await note(yap).pressSequentially('Dark mode check');
  await shot(yap.quickNote, '07-dark');
  await setConfig(yap, main, { theme: 'light' });
  await expect.poll(bg).toBe('rgb(250, 249, 246)');
});
