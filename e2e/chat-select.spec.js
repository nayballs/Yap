// Deleting several chats at once (ChatView.svelte, chats.rs `delete_many`,
// Nathan 2026-10-19: "shift click some chats and delete them all together"):
// Shift+click selects a range, Ctrl+click adds or takes out one, the
// "N selected" bar deletes them after asking; Delete, Esc and Ctrl+A from
// the keyboard; the list's ⋯ menu has Select all and Delete all chats….
import { test as base, expect, openView, expectStore } from './support/fixtures.js';

const NOW = Math.floor(Date.now() / 1000);
const conv = (id, title, ago, n = 2) => ({
  id,
  title,
  createdTs: NOW - ago,
  updatedTs: NOW - ago,
  messages: Array.from({ length: n }, (_, i) => ({ role: i % 2 ? 'assistant' : 'user', text: `${title} ${i}`, ts: NOW - ago + i })),
});
// Newest first in the list: Foxtrot, Echo, Delta, Charlie, Bravo, Alpha.
const CHATS = {
  conversations: [
    conv(1, 'Alpha', 600),
    conv(2, 'Bravo', 500),
    conv(3, 'Charlie', 400),
    conv(4, 'Delta', 300),
    conv(5, 'Echo', 200, 1),
    conv(6, 'Foxtrot', 100),
  ],
};

const test = base.extend({
  yapOptions: [
    async ({}, use) => {
      await use({ name: 'chat-select', data: { 'chats.json': CHATS } });
    },
    { scope: 'worker' },
  ],
});

const row = (main, title) => main.locator('.conv').filter({ hasText: title });
const picked = (main) => main.locator('.conv.picked .ctitle');
const bar = (main) => main.getByRole('toolbar', { name: 'Selected chats' });
const dialog = (main) => main.getByRole('alertdialog');
const menu = (main) => main.getByRole('menu').first();
const ids = (yap) => yap.readJson('chats.json').conversations.map((c) => c.id).sort();

test('Shift+click and Ctrl+click select chats; the bar deletes them after asking', async ({ yap, main, shot }) => {
  await openView(main, 'Chat');
  await row(main, 'Foxtrot').click();
  await expect(main.locator('.cbubble.user').first()).toContainText('Foxtrot 0');

  // Shift+click: the range from the open one; Ctrl+click takes one out.
  await row(main, 'Charlie').click({ modifiers: ['Shift'] });
  await expect(picked(main)).toHaveText(['Foxtrot, selected', 'Echo, selected', 'Delta, selected', 'Charlie, selected']);
  await expect(bar(main)).toContainText('4 selected');
  await row(main, 'Echo').click({ modifiers: ['Control'] });
  await expect(bar(main)).toContainText('3 selected');
  await expect(row(main, 'Echo')).not.toHaveClass(/picked/);
  await shot(main, '01-chats-selected');

  // Delete asks first, with the messages they hold; Cancel keeps the selection.
  await bar(main).getByRole('button', { name: 'Delete' }).click();
  await expect(dialog(main)).toHaveAccessibleName('Delete 3 chats?');
  await expect(dialog(main)).toContainText('Their 6 messages go for good. Your notes and meetings stay.');
  await shot(main, '02-delete-3-asks');
  await dialog(main).getByRole('button', { name: 'Cancel' }).click();
  await expect(bar(main)).toContainText('3 selected');

  // A right-click on a selected row offers the selection's delete.
  await row(main, 'Delta').click({ button: 'right' });
  await expect(menu(main).getByRole('menuitem')).toHaveText(['Delete 3 chats…', 'Clear selection']);
  await menu(main).getByRole('menuitem', { name: 'Delete 3 chats…' }).click();
  await dialog(main).getByRole('button', { name: 'Delete' }).click();
  await expect(main.locator('.conv')).toHaveText(['Echo', 'Bravo', 'Alpha']);
  await expect(bar(main)).toHaveCount(0);
  await expect.poll(() => ids(yap)).toEqual([1, 2, 5]);
  // The open chat was one of them: the thread is empty again.
  await expect(main.locator('.cbubble')).toHaveCount(0);
});

test('from the keyboard: Ctrl+A, Esc and Delete', async ({ main }) => {
  await openView(main, 'Chat');
  await row(main, 'Echo').focus();
  await main.keyboard.press('Control+A');
  await expect(bar(main)).toContainText('3 selected');
  await main.keyboard.press('Escape');
  await expect(bar(main)).toHaveCount(0);
  await expect(picked(main)).toHaveCount(0);
  // Delete on a row with nothing selected: that one, asked first.
  await row(main, 'Bravo').focus();
  await main.keyboard.press('Delete');
  await expect(dialog(main)).toHaveAccessibleName('Delete “Bravo”?');
  await dialog(main).getByRole('button', { name: 'Cancel' }).click();
  await expect(row(main, 'Bravo')).toHaveCount(1);
});

test('the list menu: Select all, and Delete all chats… asked first', async ({ yap, main, shot }) => {
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'More for chats' }).click();
  await expect(menu(main).getByRole('menuitem')).toHaveText(['Select all', 'Delete all chats…']);
  await menu(main).getByRole('menuitem', { name: 'Select all' }).click();
  await expect(bar(main)).toContainText('3 selected');
  await bar(main).getByRole('button', { name: 'Cancel' }).click();

  await main.getByRole('button', { name: 'More for chats' }).click();
  await menu(main).getByRole('menuitem', { name: 'Delete all chats…' }).click();
  await expect(dialog(main)).toHaveAccessibleName('Delete all 3 chats?');
  await expect(dialog(main)).toContainText('Their 5 messages go for good. Your notes and meetings stay.');
  await shot(main, '03-delete-all-asks');
  await dialog(main).getByRole('button', { name: 'Delete' }).click();
  await expect(main.getByText('Start your first conversation')).toBeVisible();
  await expect(main.getByRole('button', { name: 'More for chats' })).toHaveCount(0);
  await expectStore(yap, 'chats.json', (s) => s.conversations.length === 0);
});
