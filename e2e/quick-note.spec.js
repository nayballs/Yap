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
// {name: 'quick-note'}` sends the shortcut's tap as the hook would (its
// `phase` 'press' and 'release' alone: a hold), the tray is driven through
// the debug-only `e2e_tray`, and in a test run the window counts as in front
// while it's shown (it takes a dictation then, `quick_note::takes_dictation`).
// Dictations come from `e2e_quick_note_dictation` (its mic, a transcript),
// `e2e_dictate_audio` (a real recording of a tone in the mic's place, the
// stub's text) and a made-up microphone (`YAP_E2E_MICS`: a quiet tone) for
// the shortcut's hold.
//
// Since 2026-10-29 (quick_note/key.rs, tabs.rs, send.rs): holding the
// shortcut dictates into the note until it comes up; several notes open as
// tabs, remembered across restarts in quick_note_tabs.json; "Send to {app}"
// pastes the note into the app it came up over (a stand-in app here,
// `e2e_quick_note_before`; a test run pastes nothing, `e2e_quick_note_sent`
// says what it would have); and a quick-note button on the Yap bar, offered
// on the empty page and in Settings → Yap bar.
import fs from 'node:fs';
import { test as base, expect, openView, openSettings, closeSettings, expectStore, setConfig, pressHotkey, settingsDialog } from './support/fixtures.js';
import { launchYap } from './support/yap.js';
import { dictationPiece, withPiece, statusLine, tabLabel, nextTab, afterClose, sendText } from '../src/lib/quicknote/quickNote.js';

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

// A made-up microphone (dictation/mics.rs): the shortcut's hold records a quiet tone.
const MIC = { id: 'laptop', name: 'Microphone Array (Northwind Audio)' };

const test = base.extend({
  yapOptions: [
    { name: 'quick-note', data: { 'notes.json': SEED }, env: { YAP_E2E_MICS: JSON.stringify([MIC]) } },
    { scope: 'worker', option: true },
  ],
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
  await yap.quickNote.getByRole('button', { name: 'Close', exact: true }).click();
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

// ---------------------------------------------------------------- 2026-10-29: hold, tabs, Send, the bar

const logText = (yap) => fs.readFileSync(yap.logPath, 'utf8');
const tabs = (yap) => yap.quickNote.getByRole('tablist', { name: 'Quick notes' }).getByRole('tab');
const front = (yap) => yap.quickNote.getByRole('tab', { selected: true });
const sendButton = (yap) => yap.quickNote.locator('.send');

test('tabs, Send and the tap and hold, in pure terms', () => {
  expect(tabLabel({ title: '  ' })).toBe('New quick note');
  expect(tabLabel({ title: 'Errands' })).toBe('Errands');
  // Ctrl+Tab round the ends, both ways.
  expect(nextTab(3, 2, 1)).toBe(0);
  expect(nextTab(3, 0, -1)).toBe(2);
  expect(nextTab(1, 0, 1)).toBe(0);
  expect(nextTab(0, 0, 1)).toBe(-1);
  // Closing: the tab that takes its place, else the one before.
  expect(afterClose(3, 0)).toBe(0);
  expect(afterClose(3, 2)).toBe(1);
  expect(afterClose(2, 1)).toBe(0);
  // Send: the selection if it has words, else the note.
  expect(sendText('Running late, sorry', 0, 12)).toBe('Running late');
  expect(sendText('Running late', 3, 3)).toBe('Running late');
  expect(sendText('a   b', 1, 4)).toBe('a   b');
  expect(sendText('', undefined, undefined)).toBe('');
});

test('holding the shortcut opens it and dictates into the note until the key comes up; a tap never records', async ({ yap, shot }) => {
  // Every tap so far only opened or hid the window.
  expect(logText(yap)).not.toContain('Quick note: the shortcut held');
  await openIt(yap);
  await yap.quickNote.getByRole('button', { name: 'New note' }).click();
  await note(yap).pressSequentially('Shopping:');
  await expectStore(yap, 'notes.json', (s) => s.notes.some((n) => n.content === 'Shopping:'));
  const id = quickNotes(yap).find((n) => n.content === 'Shopping:').id;
  await putAway(yap);

  // Down: up it comes at once; held past the hold, the note's dictation records.
  await yap.invoke('e2e_shortcut', { name: 'quick-note', phase: 'press' });
  await expect.poll(() => visible(yap)).toBe(true);
  await expect(note(yap)).toHaveValue('Shopping:');
  await expect(yap.overlay.locator('.capsule')).toBeVisible();
  await expect.poll(() => logText(yap)).toContain('Quick note: the shortcut held, dictating into the note');
  await expect.poll(() => logText(yap)).toContain('Dictation: into the quick note');
  await yap.quickNote.waitForTimeout(1_200);
  await shot(yap.quickNote, '09-held-dictating');
  // Up: it stops, and the words go into the note at its cursor — never
  // pasted, and the window stays up.
  await yap.invoke('e2e_shortcut', { name: 'quick-note', phase: 'release' });
  await expect(note(yap)).toHaveValue(/^Shopping: .*STT stub/, { timeout: 15_000 });
  await expectStore(yap, 'notes.json', (s) => /STT stub/.test(byIdIn(s, id)?.content || ''));
  expect(await visible(yap)).toBe(true);
  await expect(yap.overlay.locator('.capsule')).toBeHidden({ timeout: 15_000 });

  // A tap while it's in front puts it away, with nothing recorded.
  const held = logText(yap).split('Quick note: the shortcut held').length;
  await shortcut(yap);
  await expect.poll(() => visible(yap)).toBe(false);
  await yap.quickNote.waitForTimeout(500);
  expect(logText(yap).split('Quick note: the shortcut held').length).toBe(held);
});

test('held in one of Yap\'s own windows: the quick note takes the focus, and its page hears the key come up', async ({ yap, main }) => {
  await setConfig(yap, main, { quickNoteHotkey: 'kb:132' }); // F21
  try {
    await openIt(yap);
    await yap.quickNote.getByRole('button', { name: 'New note' }).click();
    await note(yap).pressSequentially('From the main window:');
    await expectStore(yap, 'notes.json', (s) => s.notes.some((n) => n.content === 'From the main window:'));
    await putAway(yap);
    const key = { key: 'F21', code: 'F21', windowsVirtualKeyCode: 132, nativeVirtualKeyCode: 132 };
    // Down in the main window (its in-page fallback)…
    const down = await main.context().newCDPSession(main);
    await down.send('Input.dispatchKeyEvent', { type: 'rawKeyDown', ...key });
    await down.detach();
    await expect.poll(() => visible(yap)).toBe(true);
    await expect(yap.overlay.locator('.capsule')).toBeVisible();
    await yap.quickNote.waitForTimeout(1_000);
    // …up in the quick note, which took the focus meanwhile.
    const up = await yap.quickNote.context().newCDPSession(yap.quickNote);
    await up.send('Input.dispatchKeyEvent', { type: 'keyUp', ...key });
    await up.detach();
    await expect(note(yap)).toHaveValue(/^From the main window: .*STT stub/, { timeout: 15_000 });
    expect(await visible(yap)).toBe(true);
    await expect(yap.overlay.locator('.capsule')).toBeHidden({ timeout: 15_000 });
  } finally {
    await setConfig(yap, main, { quickNoteHotkey: '' });
  }
});

test('tabs: new, switch and close with the buttons and Ctrl+T, Ctrl+Tab and Ctrl+W; the open notes remembered', async ({ yap, shot }) => {
  await openIt(yap);
  await yap.quickNote.getByRole('button', { name: 'New note' }).click();
  await note(yap).pressSequentially('Pack the tent');
  // Ctrl+T: another, at once (a blank tab there is used again rather than a second one).
  await note(yap).press('Control+t');
  await expect(note(yap)).toHaveValue('');
  await expect(front(yap)).toHaveText('New quick note');
  await note(yap).pressSequentially('Book the campsite');
  await expectStore(yap, 'notes.json', (s) => ['Pack the tent', 'Book the campsite'].every((t) => s.notes.some((n) => n.content === t)));
  const tent = quickNotes(yap).find((n) => n.content === 'Pack the tent').id;
  const site = quickNotes(yap).find((n) => n.content === 'Book the campsite').id;
  await expect(front(yap)).toHaveText('Book the campsite');
  await expect(tabs(yap).filter({ hasText: 'Pack the tent' })).toHaveCount(1);
  // Rust remembers the open notes, in order, the one in front last.
  await expectStore(yap, 'quick_note_tabs.json', (t) => t.active === site && t.tabs.indexOf(tent) >= 0 && t.tabs.indexOf(tent) < t.tabs.indexOf(site));
  await shot(yap.quickNote, '10-tabs');

  // Ctrl+Shift+Tab: the one before; Ctrl+Tab: back.
  await note(yap).press('Control+Shift+Tab');
  await expect(front(yap)).toHaveText('Pack the tent');
  await expect(note(yap)).toHaveValue('Pack the tent');
  await note(yap).press('Control+Tab');
  await expect(note(yap)).toHaveValue('Book the campsite');
  // A click on a tab.
  await tabs(yap).filter({ hasText: 'Pack the tent' }).click();
  await expect(note(yap)).toHaveValue('Pack the tent');
  await note(yap).press('End');
  await note(yap).pressSequentially(' and the stove');
  await expectStore(yap, 'notes.json', (s) => byIdIn(s, tent)?.content === 'Pack the tent and the stove');
  await expectStore(yap, 'quick_note_tabs.json', (t) => t.active === tent);
  // Put away and opened again: on the tab in front (today's rule).
  await putAway(yap);
  await shortcut(yap);
  await expect.poll(() => visible(yap)).toBe(true);
  await expect(note(yap)).toHaveValue('Pack the tent and the stove');

  // × on a tab behind: it closes; its note stays in Notes.
  await yap.quickNote.getByRole('button', { name: 'Close Book the campsite' }).click();
  await expect(tabs(yap).filter({ hasText: 'Book the campsite' })).toHaveCount(0);
  await expect(note(yap)).toHaveValue('Pack the tent and the stove');
  expect(byId(yap, site)?.content).toBe('Book the campsite');
  await expectStore(yap, 'quick_note_tabs.json', (t) => !t.tabs.includes(site));
  // Ctrl+W: the tab in front closes, the next one comes to the front.
  const left = await tabs(yap).count();
  await note(yap).press('Control+w');
  await expect(tabs(yap)).toHaveCount(left - 1);
  await expect(tabs(yap).filter({ hasText: 'Pack the tent' })).toHaveCount(0);
  expect(byId(yap, tent)?.content).toBe('Pack the tent and the stove');
  await expectStore(yap, 'quick_note_tabs.json', (t) => !t.tabs.includes(tent));
  // Closing every tab leaves a new note.
  while ((await tabs(yap).count()) > 1) await note(yap).press('Control+w');
  await note(yap).press('Control+w');
  await expect(tabs(yap)).toHaveCount(1);
  await expect(front(yap)).toHaveText('New quick note');
  await expect(note(yap)).toHaveValue('');
});

test('Send to the app it came up over: the note, or its selection, pasted there; it steps aside', async ({ yap, shot }) => {
  await putAway(yap);
  // A stand-in for the app in front (a test run reads and pastes into no real window).
  await yap.invoke('e2e_quick_note_before', { app: 'slack.exe' });
  try {
    await shortcut(yap);
    await expect.poll(() => visible(yap)).toBe(true);
    await yap.quickNote.getByRole('button', { name: 'New note' }).click();
    const send = yap.quickNote.getByRole('button', { name: 'Send to Slack' });
    await expect(send).toBeVisible();
    await expect(send).toBeDisabled(); // nothing to send yet
    await note(yap).pressSequentially('Running ten minutes late, start without me');
    await expect(send).toBeEnabled();
    await shot(yap.quickNote, '11-send-to-slack');
    await send.click();
    await expect.poll(() => visible(yap)).toBe(false);
    expect(await yap.invoke('e2e_quick_note_sent')).toMatchObject({ text: 'Running ten minutes late, start without me', app: 'slack.exe', standIn: true });
    // The note is kept as typed.
    await expectStore(yap, 'notes.json', (s) => s.notes.some((n) => n.content === 'Running ten minutes late, start without me'));

    // Opened again on it: only what's selected goes.
    await shortcut(yap);
    await expect.poll(() => visible(yap)).toBe(true);
    await expect(note(yap)).toHaveValue('Running ten minutes late, start without me');
    await note(yap).evaluate((el) => {
      el.focus();
      el.setSelectionRange(0, 24); // "Running ten minutes late"
    });
    await send.click();
    await expect.poll(() => visible(yap)).toBe(false);
    expect(await yap.invoke('e2e_quick_note_sent')).toMatchObject({ text: 'Running ten minutes late' });

    // Opened from Yap itself (nothing else in front): no app to send to.
    await yap.invoke('e2e_quick_note_before', { app: null });
    await shortcut(yap);
    await expect.poll(() => visible(yap)).toBe(true);
    await expect(sendButton(yap)).toHaveCount(0);
  } finally {
    await yap.invoke('e2e_quick_note_before', { app: null });
  }
});

test('a quick-note button on the Yap bar: off by default, offered on the empty page, switched in Settings too', async ({ yap, main, shot }) => {
  expect(yap.readConfig().barQuickNote ?? false).toBe(false);
  await openIt(yap);
  await yap.quickNote.getByRole('button', { name: 'New note' }).click();
  // The empty page offers it.
  const add = yap.quickNote.getByRole('button', { name: 'Add to the Yap bar' });
  await expect(add).toBeVisible();
  await shot(yap.quickNote, '12-offer-the-bar-button');
  // Typing: the offer steps aside.
  await note(yap).pressSequentially('x');
  await expect(add).toHaveCount(0);
  await note(yap).press('Backspace');
  await expect(add).toBeVisible();
  await add.click();
  await expectStore(yap, 'config.json', (c) => c.barQuickNote === true);
  await expect(add).toHaveCount(0);

  // On the open bar: its button, with a tooltip; the pill is wider for it.
  const bar = yap.overlay;
  await yap.invoke('bar_simulate', { pointer: 'pill' });
  try {
    const button = bar.getByRole('button', { name: 'Quick note' });
    await expect(button).toBeVisible();
    await expect.poll(() => bar.locator('.pill.open').evaluate((el) => el.getBoundingClientRect().width)).toBe(230);
    await button.hover();
    await expect(bar.getByRole('tooltip')).toHaveText(/Quick note/);
    await bar.waitForTimeout(300);
    await shot(bar, '13-bar-quick-note-button');
    // In front: it puts the quick note away; again: up it comes.
    await button.click();
    await expect.poll(() => visible(yap)).toBe(false);
    await button.click();
    await expect.poll(() => visible(yap)).toBe(true);
  } finally {
    await yap.invoke('bar_simulate', { pointer: 'away' });
  }

  // Settings → General → Yap bar shows it on, and switches it off.
  await openSettings(main, 'General');
  const toggle = settingsDialog(main).getByRole('group', { name: 'Yap bar' }).getByRole('button', { name: 'Quick note button', exact: true });
  await expect(toggle).toHaveAttribute('aria-pressed', 'true');
  await toggle.click();
  await expectStore(yap, 'config.json', (c) => c.barQuickNote === false);
  await closeSettings(main);
  await yap.invoke('bar_simulate', { pointer: 'pill' });
  try {
    await expect(bar.getByRole('button', { name: 'More' })).toBeVisible();
    await expect(bar.getByRole('button', { name: 'Quick note' })).toHaveCount(0);
  } finally {
    await yap.invoke('bar_simulate', { pointer: 'away' });
  }
});

test('the open tabs are remembered across a restart; a tab left alone a day closes', async ({ shot }) => {
  test.setTimeout(150_000);
  const seed = {
    folders: ['Personal', 'Quick notes'],
    notes: [
      { id: 11, title: 'Old idea', titleAuto: true, content: 'Old idea', folder: 'Quick notes', source: 'quick', noteType: 'personal', createdTs: NOW - 4 * 86_400, updatedTs: NOW - 4 * 86_400 },
      { id: 12, title: 'Call the vet', titleAuto: true, content: 'Call the vet', folder: 'Quick notes', source: 'quick', noteType: 'personal', createdTs: NOW - 7_200, updatedTs: NOW - 7_200 },
      { id: 13, title: 'Gift ideas', titleAuto: true, content: 'Gift ideas', folder: 'Quick notes', source: 'quick', noteType: 'personal', createdTs: NOW - 600, updatedTs: NOW - 600 },
    ],
  };
  const options = { name: 'quick-note-tabs', data: { 'notes.json': seed, 'quick_note_tabs.json': { tabs: [11, 12, 13], active: 13 } } };
  const first = await launchYap(options);
  let second = null;
  try {
    const page = first.quickNote;
    const openTabs = page.getByRole('tablist', { name: 'Quick notes' }).getByRole('tab');
    await first.invoke('e2e_shortcut', { name: 'quick-note' });
    await expect.poll(() => first.invoke('plugin:window|is_visible', { label: 'quick-note' })).toBe(true);
    // The two from today, on the newest; the one left alone four days closed (it's in Notes).
    await expect(openTabs).toHaveText(['Call the vet', 'Gift ideas']);
    await expect(page.getByRole('tab', { selected: true })).toHaveText('Gift ideas');
    await expect(page.getByRole('textbox', { name: 'Quick note' })).toHaveValue('Gift ideas');
    await expectStore(first, 'quick_note_tabs.json', (t) => JSON.stringify(t.tabs) === '[12,13]' && t.active === 13);
    expect(first.readJson('notes.json').notes.some((n) => n.id === 11)).toBe(true);
    await shot(page, '14-tabs-after-a-start');
    // Call the vet in front, then a restart.
    await openTabs.filter({ hasText: 'Call the vet' }).click();
    await expectStore(first, 'quick_note_tabs.json', (t) => t.active === 12);
    await first.stop();

    second = await launchYap({ ...options, data: {}, restart: true });
    const page2 = second.quickNote;
    await second.invoke('e2e_shortcut', { name: 'quick-note' });
    await expect.poll(() => second.invoke('plugin:window|is_visible', { label: 'quick-note' })).toBe(true);
    // The same tabs; the first open since the start is on the newest quick
    // note within a day, as ever.
    await expect(page2.getByRole('tablist', { name: 'Quick notes' }).getByRole('tab')).toHaveText(['Call the vet', 'Gift ideas']);
    await expect(page2.getByRole('tab', { selected: true })).toHaveText('Gift ideas');
  } finally {
    await first.stop();
    await second?.stop();
  }
});
