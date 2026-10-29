// Right-click is the same everywhere (ui/contextMenu.svelte.js + ui/ContextMenu
// + ui/ConfirmDialog, one of each per window): an item with actions opens the
// app's menu — the same actions as its buttons, main one first, Copy-type
// next, the destructive one last in red behind a confirm — from a
// right-click, its hover ⋯ (where it has one) and the keyboard (Shift+F10,
// the ContextMenu key); Esc closes it with the focus back on the item. A
// right-click anywhere else shows nothing at all: WebView2's own menu is cut
// down to the edit commands (shell/context_menu.rs; Inspect only for a debug
// build's Shift+right-click — never a plain one).
//
// One of each kind: a Chat conversation (Rename in place, Copy as Markdown,
// Delete…) and its messages (Copy, Copy as Markdown, Read aloud), a Home
// dictation (Copy, Copy raw transcript, Delete…), Dictionary rows (Edit, Star, ≈,
// Delete…), a past meeting (Open, Generate summary, Copy as Markdown,
// Delete…) and one from the calendar (Open note, Join, Start…), a calendar
// connection (Sync now, Disconnect…), a speech model (Download / Use /
// Delete…), a transcript line in the meeting notepad (Edit line, Copy line, Who said
// this?). Copies land in the page's stubbed clipboard, never the real one;
// Read aloud is test mode's stand-in voice (nothing plays); Join is never
// clicked (no browser opens).
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, openSettings, closeSettings, expectStore, closeToasts } from './support/fixtures.js';
import { startCalendarFeed, calendarFile, vevent } from './support/calendar-feed.js';

const NOW = Math.floor(Date.now() / 1000);
const MINUTE = 60 * 1000;
const TEAMS_LINK = 'https://teams.microsoft.com/l/meetup-join/19%3ameeting_rc%40thread.v2/0';
const OWNER = { name: 'Imogen', email: 'imogen@example.com', partstat: 'ACCEPTED' };
const SAM = { name: 'Sam Lee', email: 'sam@example.com' };

const HISTORY = [
  { ts: NOW - 120, raw: 'um so the deploy is uh friday', text: 'So the deploy is Friday.', model: 'parakeet-tdt-0.6b-v3', app: 'slack.exe' },
  { ts: NOW - 60, raw: 'Lunch at noon?', text: 'Lunch at noon?', model: 'parakeet-tdt-0.6b-v3', app: 'chrome.exe' },
];
const ANSWER = 'Try **Alfama** for:\n\n- the views\n- the old streets';
const CHATS = {
  conversations: [
    {
      id: 1,
      title: 'Trip plans',
      createdTs: NOW - 3600,
      updatedTs: NOW - 3600,
      messages: [
        { role: 'user', text: 'Where should we stay in Lisbon?', ts: NOW - 3600 },
        { role: 'assistant', text: ANSWER, ts: NOW - 3590 },
      ],
    },
    {
      id: 2,
      title: 'Budget questions',
      createdTs: NOW - 60,
      updatedTs: NOW - 60,
      messages: [{ role: 'user', text: 'What is left in the travel budget?', ts: NOW - 60 }],
    },
  ],
};

const test = base.extend({
  feed: [
    async ({}, use) => {
      const feed = await startCalendarFeed();
      await use(feed);
      await feed.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({}, use) => {
      await use({
        name: 'right-click',
        config: {
          meetingReminder: 'never',
          dictionary: [
            { from: 'power to keep', to: 'Parakeet', fuzzy: true, snippet: false },
            { from: 'my sign-off', to: 'Speak soon,\nSam', fuzzy: true, snippet: true },
          ],
        },
        data: { 'history.json': HISTORY, 'chats.json': CHATS },
        // An installed speech model (an empty folder counts), never the active one.
        prepare: (dataDir) => fs.mkdirSync(path.join(dataDir, 'models', 'moonshine-base'), { recursive: true }),
      });
    },
    { scope: 'worker' },
  ],
});

const menu = (page) => page.getByRole('menu').first();
const dialog = (page) => page.getByRole('alertdialog');
const recording = (yap) => yap.invoke('meeting_state').then((s) => s.recording);
const visible = (yap, label) => yap.invoke('plugin:window|is_visible', { label });

/** The page's clipboard, stubbed (the real one is never touched). */
async function stubClipboard(page) {
  await page.evaluate(() => {
    window.__copied = null;
    navigator.clipboard.writeText = async (t) => {
      window.__copied = t;
    };
  });
  return () => page.evaluate(() => window.__copied);
}

/** Every item of the open menu, checkbox ones included, by its text. */
const allItems = (page) => menu(page).locator('[role^="menuitem"]');

test.afterEach(async ({ yap, main }) => {
  await main.keyboard.press('Escape');
  await closeSettings(main);
  if (await recording(yap)) {
    await yap.invoke('meeting_pause');
    await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
  }
  if (await visible(yap, 'notepad')) await yap.notepad.getByRole('button', { name: 'Close notepad' }).click();
});

test('Chat: a conversation is renamed in place, copied and deleted; messages copy and read aloud', async ({
  yap,
  main,
  shot,
}) => {
  await openView(main, 'Chat');
  const copied = await stubClipboard(main);
  const conv = (title) => main.locator('.conv').filter({ hasText: title });
  await expect(conv('Trip plans')).toBeVisible();

  // A right-click: its menu, and it opens, as a click does.
  await conv('Trip plans').click({ button: 'right' });
  await expect(menu(main)).toHaveAccessibleName('Conversation Trip plans');
  await expect(allItems(main)).toHaveText(['Rename', 'Copy as Markdown', 'Archive', 'Delete…']);
  await expect(allItems(main).last()).toHaveClass(/danger/);
  await expect(main.locator('.msgs')).toContainText('Where should we stay in Lisbon?');
  await expect(conv('Trip plans')).toHaveClass(/active/);
  await shot(main, '01-chat-conversation-menu');

  // Esc closes it, the focus back on the row.
  await main.keyboard.press('Escape');
  await expect(main.getByRole('menu')).toHaveCount(0);
  await expect(conv('Trip plans')).toBeFocused();

  // Shift+F10 on the focused row: the menu, its first item focused.
  await main.keyboard.press('Shift+F10');
  await expect(allItems(main).first()).toBeFocused();

  // Rename, in place: Esc keeps the name, Enter saves it (chat/chats.rs `rename`).
  await main.keyboard.press('Enter');
  const input = main.getByRole('textbox', { name: 'Rename conversation Trip plans' });
  await expect(input).toBeFocused();
  await input.fill('Nope');
  await input.press('Escape');
  await expect(conv('Trip plans')).toBeVisible();
  await conv('Trip plans').click({ button: 'right' });
  await menu(main).getByRole('menuitem', { name: 'Rename' }).click();
  await input.fill('  Lisbon   trip ');
  await shot(main, '02-chat-rename');
  await input.press('Enter');
  await expect(conv('Lisbon trip')).toBeVisible();
  await expect(conv('Lisbon trip')).toBeFocused();
  await expectStore(yap, 'chats.json', (s) => s.conversations.find((c) => c.id === 1)?.title === 'Lisbon trip');
  // Its place in the list stays (renaming isn't an update).
  await expect(main.locator('.conv .ctitle')).toHaveText(['Budget questions', 'Lisbon trip']);

  // The hover ⋯ opens the same menu, under itself; Copy as Markdown copies
  // the whole conversation.
  await conv('Lisbon trip').hover();
  await main.getByRole('button', { name: 'More for Lisbon trip', exact: true }).click();
  await menu(main).getByRole('menuitem', { name: 'Copy as Markdown' }).click();
  await expect
    .poll(copied)
    .toBe(`# Lisbon trip\n\n**You:** Where should we stay in Lisbon?\n\n**Yap:** ${ANSWER}\n`);
  await closeToasts(main);

  // An answer: Copy (as text), Copy as Markdown, Read aloud. A question: Copy.
  const answer = main.locator('.cbubble.assistant').first();
  await answer.click({ button: 'right', position: { x: 8, y: 8 } });
  await expect(allItems(main)).toHaveText(['Copy', 'Copy as Markdown', 'Read aloud']);
  await shot(main, '03-chat-answer-menu');
  await menu(main).getByRole('menuitem', { name: 'Copy', exact: true }).click();
  await expect.poll(copied).toBe('Try Alfama for:\n\n• the views\n• the old streets');
  await answer.click({ button: 'right', position: { x: 8, y: 8 } });
  await menu(main).getByRole('menuitem', { name: 'Copy as Markdown' }).click();
  await expect.poll(copied).toBe(ANSWER);
  // Read aloud: test mode's stand-in voice (nothing plays), as the ▶ does.
  await yap.invoke('tts_e2e_spoken', { clear: true });
  await answer.click({ button: 'right', position: { x: 8, y: 8 } });
  await menu(main).getByRole('menuitem', { name: 'Read aloud' }).click();
  await expect.poll(() => yap.invoke('tts_e2e_spoken').then((s) => s.map((x) => x.messageId))).toContain('chat:1:1');
  await main.keyboard.press('Escape'); // stops the reading
  const question = main.locator('.cbubble.user').first();
  await question.click({ button: 'right', position: { x: 8, y: 8 } });
  await expect(allItems(main)).toHaveText(['Copy']);
  await menu(main).getByRole('menuitem', { name: 'Copy' }).click();
  await expect.poll(copied).toBe('Where should we stay in Lisbon?');
  // From the keyboard too: a message takes the focus, Shift+F10 opens it.
  await question.focus();
  await main.keyboard.press('Shift+F10');
  await expect(menu(main)).toHaveAccessibleName('Question');
  await main.keyboard.press('Escape');
  await expect(question).toBeFocused();
  await closeToasts(main);

  // Delete… asks first; Cancel keeps it, Delete removes it.
  await conv('Lisbon trip').click({ button: 'right' });
  await menu(main).getByRole('menuitem', { name: 'Delete…' }).click();
  await expect(dialog(main)).toHaveAccessibleName('Delete “Lisbon trip”?');
  await expect(dialog(main)).toContainText('Its 2 messages go for good.');
  await expect(dialog(main).getByRole('button', { name: 'Cancel' })).toBeFocused();
  await shot(main, '04-chat-delete-asks');
  await dialog(main).getByRole('button', { name: 'Cancel' }).click();
  await expect(conv('Lisbon trip')).toBeFocused();
  await conv('Lisbon trip').click({ button: 'right' });
  await menu(main).getByRole('menuitem', { name: 'Delete…' }).click();
  await dialog(main).getByRole('button', { name: 'Delete' }).click();
  await expect(conv('Lisbon trip')).toHaveCount(0);
  await expectStore(yap, 'chats.json', (s) => !s.conversations.some((c) => c.id === 1));
  // The open conversation went: a new chat.
  await expect(main.locator('.msgs')).toHaveCount(0);
});

test('Home: a dictation copies, copies its raw words and is deleted after asking', async ({ yap, main, shot }) => {
  await openView(main, 'Home');
  const copied = await stubClipboard(main);
  const row = (text) => main.locator('.daylist .item').filter({ hasText: text });
  await expect(row('So the deploy is Friday.')).toBeVisible();

  // Cleaned up: Copy, Copy raw transcript, Delete….
  await row('So the deploy is Friday.').click({ button: 'right' });
  await expect(menu(main)).toHaveAccessibleName('Dictation');
  await expect(allItems(main)).toHaveText(['Copy', 'Copy raw transcript', 'Delete…']);
  await shot(main, '05-home-dictation-menu');
  await menu(main).getByRole('menuitem', { name: 'Copy raw transcript' }).click();
  await expect.poll(copied).toBe('um so the deploy is uh friday');
  await row('So the deploy is Friday.').click({ button: 'right' });
  await menu(main).getByRole('menuitem', { name: 'Copy', exact: true }).click();
  await expect.poll(copied).toBe('So the deploy is Friday.');
  await closeToasts(main);

  // Nothing cleaned up: no raw words to offer.
  await row('Lunch at noon?').click({ button: 'right' });
  await expect(allItems(main)).toHaveText(['Copy', 'Delete…']);
  await main.keyboard.press('Escape');
  await expect(row('Lunch at noon?')).toBeFocused();

  // Shift+F10 on the focused row.
  await main.keyboard.press('Shift+F10');
  await expect(allItems(main).first()).toBeFocused();
  await main.keyboard.press('Escape');

  // The row's trash button asks too, and so does Delete…; Cancel keeps it.
  await row('Lunch at noon?').hover();
  await row('Lunch at noon?').getByRole('button', { name: 'Delete' }).click();
  await expect(dialog(main)).toHaveAccessibleName('Delete this dictation?');
  await expect(dialog(main)).toContainText('“Lunch at noon?” leaves your history. Your stats still count it');
  await dialog(main).getByRole('button', { name: 'Cancel' }).click();
  expect(yap.readJson('history.json')).toHaveLength(2);
  await row('Lunch at noon?').click({ button: 'right' });
  await menu(main).getByRole('menuitem', { name: 'Delete…' }).click();
  await shot(main, '06-home-delete-asks');
  await dialog(main).getByRole('button', { name: 'Delete' }).click();
  await expect(row('Lunch at noon?')).toHaveCount(0);
  await expectStore(yap, 'history.json', (h) => h.length === 1 && h[0].text === 'So the deploy is Friday.');
});

test('Dictionary: Edit, near-misses and Delete… on a row; in its boxes, the boxes’ own edit menu', async ({
  yap,
  main,
  shot,
}) => {
  await openView(main, 'Dictionary');
  const corrections = main.getByRole('region', { name: 'Corrections' });
  const snippets = main.getByRole('region', { name: 'Snippets' });
  await expect(corrections.getByRole('textbox', { name: 'Replace with' })).toHaveValue('Parakeet');

  // A right-click on the row (not in a box): Edit, Star, Add another way it's heard, ≈ Catch
  // near-misses (on), Delete….
  await corrections.locator('.dict-row .arrow').first().click({ button: 'right' });
  await expect(menu(main)).toHaveAccessibleName('Correction');
  await expect(allItems(main)).toHaveText(['Edit', 'Star', 'Add another way it’s heard', 'Catch near-misses', 'Delete…']);
  const near = menu(main).getByRole('menuitemcheckbox', { name: 'Catch near-misses' });
  await expect(near).toHaveAttribute('aria-checked', 'true');
  await shot(main, '07-dictionary-row-menu');
  // ≈ off, as the row's ≈ button does.
  await near.click();
  await expectStore(yap, 'config.json', (c) => c.dictionary.find((e) => e.to === 'Parakeet')?.fuzzy === false);
  await expect(corrections.locator('.fz').first()).not.toHaveClass(/\bon\b/);
  // Edit: the "Heard" box, selected.
  await corrections.locator('.dict-row .arrow').first().click({ button: 'right' });
  await menu(main).getByRole('menuitem', { name: 'Edit' }).click();
  await expect(corrections.getByRole('textbox', { name: 'Heard' })).toBeFocused();

  // In a box, the box's own menu (the edit commands): never the row's.
  const menus = () => yap.invoke('e2e_context_menus').then((m) => m.settings);
  const before = (await menus()).menus;
  await corrections.getByRole('textbox', { name: 'Heard' }).click({ button: 'right' });
  await expect.poll(() => menus().then((m) => m.menus)).toBeGreaterThan(before);
  expect((await menus()).last.editable).toBe(true);
  await expect(main.getByRole('menu')).toHaveCount(0);

  // From the keyboard: Shift+F10 on the row's ≈ button.
  await corrections.locator('.fz').first().focus();
  await main.keyboard.press('Shift+F10');
  await expect(menu(main)).toHaveAccessibleName('Correction');
  await main.keyboard.press('Escape');

  // A snippet: Edit, Star, Delete… (asked first).
  await snippets.locator('.snip-row .arrow').first().click({ button: 'right' });
  await expect(menu(main)).toHaveAccessibleName('Snippet');
  await expect(allItems(main)).toHaveText(['Edit', 'Star', 'Delete…']);
  await menu(main).getByRole('menuitem', { name: 'Delete…' }).click();
  await expect(dialog(main)).toHaveAccessibleName('Delete the snippet “my sign-off”?');
  await dialog(main).getByRole('button', { name: 'Delete' }).click();
  await expectStore(yap, 'config.json', (c) => !c.dictionary.some((e) => e.snippet));
  await expect(snippets.getByText(/No snippets yet/)).toBeVisible();
  // The correction stays.
  expect(yap.readConfig().dictionary.map((e) => e.to)).toEqual(['Parakeet']);
});

test('Meetings: a past meeting and one from the calendar; a calendar connection', async ({ yap, main, feed, shot }) => {
  // A meeting with talk and no summary yet (the notepad opens; it's closed).
  const note = await yap.invoke('note_create', { title: 'Budget review', folder: 'Meetings' });
  await yap.invoke('meeting_start', { noteId: note.id });
  await yap.invoke('e2e_meeting_feed', {
    segments: [{ source: 'them', text: 'Travel is up twelve percent this quarter.', ts: NOW - 300 }],
  });
  await yap.invoke('meeting_pause');
  await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
  if (await visible(yap, 'notepad')) await yap.notepad.getByRole('button', { name: 'Close notepad' }).click();
  // And one from the calendar, starting in 4 minutes, with a Teams link.
  feed.set(
    calendarFile(OWNER.email, [
      vevent({
        uid: 'design@rc',
        summary: 'Design review',
        start: new Date(Date.now() + 4 * MINUTE),
        end: new Date(Date.now() + 34 * MINUTE),
        attendees: [SAM, OWNER],
        description: `Join: ${TEAMS_LINK}`,
      }),
    ])
  );
  await yap.invoke('calendar_add_link', { link: feed.url, kind: 'ics' });
  await yap.invoke('calendar_sync');

  await openView(main, 'Meetings');
  const copied = await stubClipboard(main);
  const past = main.getByRole('region', { name: 'Past meeting notes' });
  const budget = past.getByRole('listitem').filter({ hasText: 'Budget review' });
  await expect(budget).toBeVisible();

  // A past meeting: Open, Generate summary (it has none), File this meeting
  // in ▸ (calendar.md, folder rules), Copy as Markdown,
  // Find speakers (the engine again over its audio), Delete….
  // (Its audio is kept: Export audio… once the view knows, meetings/meeting_audio/export.rs.)
  await expect(async () => {
    await main.keyboard.press('Escape');
    await budget.click({ button: 'right' });
    await expect(menu(main)).toHaveAccessibleName('Meeting note Budget review');
    await expect(allItems(main)).toHaveText(
      ['Open', 'Generate summary', 'File this meeting in', 'Copy as Markdown', 'Find speakers', 'Export audio…', 'Export each side…', 'Delete…'],
      { timeout: 1_000 }
    );
  }).toPass({ timeout: 10_000 });
  await shot(main, '08-past-meeting-menu');
  await menu(main).getByRole('menuitem', { name: 'Copy as Markdown' }).click();
  await expect.poll(copied).toContain('# Budget review');
  await expect.poll(copied).toContain('Travel is up twelve percent this quarter.');
  await closeToasts(main);
  // Its ⋯ and the keyboard: the same menu.
  await budget.hover();
  await budget.getByRole('button', { name: 'More for Budget review' }).click();
  await expect(menu(main)).toHaveAccessibleName('Meeting note Budget review');
  await main.keyboard.press('Escape');
  await budget.getByRole('button', { name: 'Budget review', exact: true }).focus();
  await main.keyboard.press('Shift+F10');
  await expect(allItems(main).first()).toBeFocused();
  await main.keyboard.press('Escape');

  // A meeting from the calendar, about to start: Open note, then its row's
  // own buttons (Join is never clicked here: no browser).
  const design = main.getByRole('listitem').filter({ hasText: 'Design review' });
  await expect(design).toBeVisible();
  await design.click({ button: 'right', position: { x: 30, y: 20 } });
  await expect(menu(main)).toHaveAccessibleName('Meeting Design review');
  await expect(allItems(main)).toHaveText(['Open note', 'Join meeting', 'Start', 'Join + Start']);
  await shot(main, '09-calendar-meeting-menu');
  await menu(main).getByRole('menuitem', { name: 'Open note' }).click();
  await expect(main.getByPlaceholder('Untitled Note')).toHaveValue('Design review');

  // Delete… goes through the meeting delete path, after asking.
  await openView(main, 'Meetings');
  await budget.click({ button: 'right' });
  await menu(main).getByRole('menuitem', { name: 'Delete…' }).click();
  await expect(dialog(main)).toHaveAccessibleName('Delete “Budget review”?');
  await dialog(main).getByRole('button', { name: 'Delete' }).click();
  await expect(budget).toHaveCount(0);
  await expectStore(yap, 'notes.json', (s) => !s.notes.some((n) => n.id === note.id));

  // Settings → Connectors: a calendar's row, right-clicked — its ⋯'s items.
  const settings = await openSettings(main, 'Connectors');
  const other = settings.getByRole('group', { name: 'Other calendar' });
  await other.getByText(OWNER.email, { exact: true }).click({ button: 'right' });
  await expect(menu(main)).toHaveAccessibleName(`${OWNER.email} options`);
  await expect(allItems(main)).toHaveText(['Sync now', 'Disconnect…']);
  await shot(main, '10-connection-menu');
  await menu(main).getByRole('menuitem', { name: 'Disconnect…' }).click();
  await expect(dialog(main)).toHaveAccessibleName(`Disconnect ${OWNER.email}?`);
  // Esc closes the question (not Settings), nothing disconnected.
  await main.keyboard.press('Escape');
  await expect(dialog(main)).toHaveCount(0);
  await expect(settings).toBeVisible();
  expect((await yap.invoke('calendar_status')).connections).toHaveLength(1);
  await other.getByRole('button', { name: `More options for ${OWNER.email}` }).click();
  await menu(main).getByRole('menuitem', { name: 'Disconnect…' }).click();
  await dialog(main).getByRole('button', { name: 'Disconnect' }).click();
  await expect.poll(() => yap.invoke('calendar_status').then((s) => s.connections.length)).toBe(0);
});

test('Settings: a speech model row has its buttons’ actions; Delete… asks', async ({ main, shot }) => {
  const settings = await openSettings(main, 'Speech-to-Text');
  const installed = settings.locator('.manager .mrow').filter({ hasText: 'Moonshine Base' });
  await installed.scrollIntoViewIfNeeded();
  await installed.click({ button: 'right' });
  await expect(menu(main)).toHaveAccessibleName('Moonshine Base');
  await expect(allItems(main)).toHaveText(['Use', 'Delete…']);
  await shot(main, '11-model-row-menu');
  await menu(main).getByRole('menuitem', { name: 'Delete…' }).click();
  // The speech models' own question (ModelManager), as the trash button.
  const ask = main.getByRole('alertdialog', { name: 'Delete Moonshine Base?' });
  await expect(ask).toBeVisible();
  await ask.getByRole('button', { name: 'Cancel' }).click();
  // Not downloaded: Download only (never clicked here: no network).
  const another = settings.locator('.manager .mrow.downloadable').first();
  await another.click({ button: 'right' });
  await expect(allItems(main)).toHaveText(['Download']);
  // Esc closes the menu, not Settings; the row has the focus.
  await main.keyboard.press('Escape');
  await expect(main.getByRole('menu')).toHaveCount(0);
  await expect(settings).toBeVisible();
  await expect(another).toBeFocused();
  // Shift+F10 on the focused row.
  await main.keyboard.press('Shift+F10');
  await expect(allItems(main).first()).toBeFocused();
});

test('Notes → Actions: a custom action is edited or deleted after asking; a built-in can’t be deleted', async ({
  yap,
  main,
}) => {
  await yap.invoke('action_create', { name: 'Tweet thread', description: '', prompt: 'Turn the note into a thread.' });
  await openView(main, 'Notes');
  await main.getByRole('button', { name: 'Actions', exact: true }).click();
  const manager = main.getByRole('dialog', { name: 'Manage actions' });
  const row = (name) => manager.locator('.items .item').filter({ hasText: name });
  await expect(row('Tweet thread')).toBeVisible();

  // A built-in: Edit, Delete… off with the reason.
  await row('Generate Notes').click({ button: 'right' });
  await expect(menu(main)).toHaveAccessibleName('Action Generate Notes');
  await expect(allItems(main)).toHaveText(['Edit', 'Delete…']);
  await expect(menu(main).getByRole('menuitem', { name: 'Delete…' })).toHaveAttribute('aria-disabled', 'true');
  await expect(menu(main)).toContainText("Yap's own action: it can be edited, not deleted.");
  await main.keyboard.press('Escape');

  // A custom one: a right-click picks it (nothing unsaved), Edit focuses its
  // name, Delete… asks first.
  await row('Tweet thread').click({ button: 'right' });
  await expect(row('Tweet thread')).toHaveClass(/active/);
  await menu(main).getByRole('menuitem', { name: 'Edit' }).click();
  await expect(manager.getByPlaceholder('Action name')).toBeFocused();
  await expect(manager.getByPlaceholder('Action name')).toHaveValue('Tweet thread');
  await row('Tweet thread').click({ button: 'right' });
  await menu(main).getByRole('menuitem', { name: 'Delete…' }).click();
  await expect(dialog(main)).toHaveAccessibleName('Delete “Tweet thread”?');
  await dialog(main).getByRole('button', { name: 'Delete' }).click();
  await expect(row('Tweet thread')).toHaveCount(0);
  await expectStore(yap, 'notes.json', (s) => !s.actions.some((a) => a.name === 'Tweet thread'));
  await manager.getByRole('button', { name: 'Close' }).click();
});

test('the meeting notepad: a transcript line copies, and says who said it — in Notes too', async ({ yap, main, shot }) => {
  const note = await yap.invoke('note_create', { title: 'Offsite plans', folder: 'Meetings' });
  await yap.invoke('meeting_start', { noteId: note.id });
  await expect.poll(() => visible(yap, 'notepad')).toBe(true);
  const pad = yap.notepad;
  const copied = await stubClipboard(pad);
  await pad.getByRole('tab', { name: 'Transcript' }).click();
  await yap.invoke('e2e_meeting_feed', {
    segments: [
      { source: 'you', text: 'Shall we book the venue?', ts: NOW - 30 },
      { source: 'them', text: 'Yes, the one by the river.', ts: NOW - 20 },
    ],
  });
  const log = pad.getByRole('log', { name: 'Transcript' });
  const row = log.locator('.row').filter({ hasText: 'by the river' });
  await expect(row).toBeVisible();

  await row.locator('.bubble').click({ button: 'right' });
  await expect(menu(pad)).toHaveAccessibleName('Transcript line');
  await expect(allItems(pad)).toHaveText(['Edit line', 'Split line…', 'Copy line', 'Who said this?', 'Delete line']);
  await shot(pad, '12-notepad-line-menu');
  await menu(pad).getByRole('menuitem', { name: 'Copy line' }).click();
  await expect.poll(copied).toBe('Yes, the one by the river.');

  // Who said this? — the speaker menu, at the line.
  await row.locator('.bubble').click({ button: 'right' });
  await menu(pad).getByRole('menuitem', { name: 'Who said this?' }).click();
  const form = pad.getByRole('dialog', { name: 'Rename speaker' });
  await expect(form).toContainText('Who said this line?');
  await shot(pad, '13-notepad-who-said-this');
  await pad.keyboard.press('Escape');
  await expect(form).toHaveCount(0);

  // Esc closes the menu (and only the menu); Shift+F10 on the line's copy button.
  await row.locator('.bubble').click({ button: 'right' });
  await pad.keyboard.press('Escape');
  await expect(pad.getByRole('menu')).toHaveCount(0);
  await row.getByRole('button', { name: 'Copy line' }).focus();
  await pad.keyboard.press('Shift+F10');
  await expect(allItems(pad).first()).toBeFocused();
  await pad.keyboard.press('Escape');

  // The same line in the Notes view: the same menu.
  await openView(main, 'Notes');
  await main.locator('.folderlist').getByRole('button', { name: /^Meetings( \d+)?$/ }).click();
  await main.locator('.items .item').filter({ hasText: 'Offsite plans' }).click();
  const notesLog = main.getByRole('log', { name: 'Meeting transcript' });
  const line = notesLog.locator('.bubble').filter({ hasText: 'by the river' });
  await expect(line).toBeVisible();
  const mainCopied = await stubClipboard(main);
  await line.click({ button: 'right' });
  await expect(menu(main)).toHaveAccessibleName('Transcript line');
  await expect(allItems(main)).toHaveText(['Edit line', 'Split line…', 'Copy line', 'Who said this?', 'Delete line']);
  await menu(main).getByRole('menuitem', { name: 'Copy line' }).click();
  await expect.poll(mainCopied).toBe('Yes, the one by the river.');
  await line.click({ button: 'right' });
  await menu(main).getByRole('menuitem', { name: 'Who said this?' }).click();
  await expect(main.getByRole('dialog', { name: 'Rename speaker' })).toContainText('Who said this line?');
  await main.keyboard.press('Escape');
});

test('anywhere else, a right-click shows nothing — and a Shift+right-click on an item still opens its menu here', async ({
  yap,
  main,
}) => {
  const menus = () => yap.invoke('e2e_context_menus').then((m) => m.settings);
  const kept = (m) => m.items.filter((i) => i.kept && i.kind !== 'separator').map((i) => i.name);

  // Plain UI (the Home greeting): WebView2 asks, every item goes — no
  // Inspect for a plain right-click, though this is a debug build.
  await openView(main, 'Home');
  const before = (await menus()).menus;
  await main.locator('.hello').click({ button: 'right' });
  await expect.poll(() => menus().then((m) => m.menus)).toBeGreaterThan(before);
  const m = (await menus()).last;
  expect(kept(m)).toEqual([]);
  expect(m.inspect).toBe(false);
  expect(m.handled).toBe(true);
  await expect(main.getByRole('menu')).toHaveCount(0);

  // The page lets a right-click on plain UI through (to that filter), and
  // cancels one on an item (its own menu).
  const prevented = (selector, shiftKey = false) =>
    main.evaluate(
      ({ sel, shift }) => {
        const ev = new MouseEvent('contextmenu', { bubbles: true, cancelable: true, button: 2, clientX: 60, clientY: 60, shiftKey: shift });
        document.querySelector(sel).dispatchEvent(ev);
        return ev.defaultPrevented;
      },
      { sel: selector, shift: shiftKey }
    );
  expect(await prevented('.hello')).toBe(false);
  expect(await prevented('.daylist .item')).toBe(true);
  await main.keyboard.press('Escape');
  // Shift+right-click is Inspect's in a dev build only (Vite's dev server);
  // this build's page is `vite build`'s, as a release's: its menu opens.
  expect(await prevented('.daylist .item', true)).toBe(true);
  await expect(menu(main)).toBeVisible();
  await main.keyboard.press('Escape');
  await expect(main.getByRole('menu')).toHaveCount(0);
});
