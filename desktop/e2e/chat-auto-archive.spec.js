// Auto-archive (desktop/src-tauri/src/chat/auto_archive.rs; 15 Nov 2026, the Claude app's):
// with "Archive chats you haven't used in" set (Settings → Privacy & data),
// a chat whose last message is that old moves to Archived by itself — soon
// after Yap starts and once a day (a test run asks for a run now:
// `e2e_chat_auto_archive`) — and Yap says so in its window, quietly, with
// Undo. Never a chat at work (here: a Code chat with a ▶ Run running), nor
// a meeting's Ask thread; a Code chat is archived like any other once its
// work is over.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, openSettings, closeSettings, closeToasts } from './support/fixtures.js';
import { RUNS } from './support/yap.js';

const NOW = Math.floor(Date.now() / 1000);
const DAY = 86_400;
const GARDEN = path.join(RUNS, 'chat-auto-archive-spec', 'garden-planner');

const conv = (id, title, ago, extra = {}) => ({
  id,
  title,
  createdTs: NOW - ago,
  updatedTs: NOW - ago,
  messages: [
    { role: 'user', text: `About ${title.toLowerCase()}?`, ts: NOW - ago },
    { role: 'assistant', text: 'Here is a short answer.', ts: NOW - ago + 1 },
  ],
  ...extra,
});
const CHATS = {
  conversations: [
    conv(101, 'Bread recipes', 40 * DAY),
    conv(102, 'Build scripts', 45 * DAY),
    conv(103, 'Holiday plans', 2 * DAY),
    // A meeting's Ask thread: the meeting's, never archived.
    conv(104, 'Ask about the team sync', 50 * DAY, { note: 5 }),
  ],
};
const NOTES = {
  folders: ['Meetings'],
  notes: [{ id: 5, title: 'Team sync', noteType: 'meeting', folder: 'Meetings', source: 'meeting', createdTs: NOW - 50 * DAY, updatedTs: NOW - 50 * DAY }],
};

const test = base.extend({
  yapOptions: [
    async ({}, use) => {
      fs.rmSync(path.dirname(GARDEN), { recursive: true, force: true });
      fs.mkdirSync(GARDEN, { recursive: true });
      await use({
        name: 'chat-auto-archive',
        config: { chatArchiveDays: 30, memoryLearnChats: false },
        data: { 'chats.json': CHATS, 'notes.json': NOTES },
      });
    },
    { scope: 'worker' },
  ],
});

const archivedTitles = async (yap) => (await yap.invoke('chats_archived')).map((c) => c.title).sort();
const listTitles = async (yap) => (await yap.invoke('chats_list')).map((c) => c.title).sort();
const notice = (main) => main.getByRole('status').filter({ hasText: /Archived (a chat|\d+ chats) you hadn't used/ });

test('at start, the chats not used in 30 days are archived; a newer one and a meeting’s Ask stay', async ({ yap, main, shot }) => {
  await expect.poll(() => archivedTitles(yap), { timeout: 20_000 }).toEqual(['Bread recipes', 'Build scripts']);
  expect(await listTitles(yap)).toEqual(['Holiday plans']);
  // The meeting's thread is untouched (never listed in Chat either way).
  expect(yap.readJson('chats.json').conversations.find((c) => c.id === 104).archivedTs ?? 0).toBe(0);
  // Its place in time stays its own: archiving isn't using it.
  expect(yap.readJson('chats.json').conversations.find((c) => c.id === 101).updatedTs).toBe(NOW - 40 * DAY);

  await openView(main, 'Chat');
  const row = (title) => main.locator('.conv').filter({ hasText: title });
  await expect(row('Holiday plans')).toBeVisible();
  await expect(row('Bread recipes')).toHaveCount(0);
  await expect(main.getByRole('button', { name: /^Archived/ })).toContainText('2');

  // The setting, on Privacy & data.
  const dialog = await openSettings(main, 'Privacy & data');
  const setting = dialog.locator('[data-setting="chat-archive"]');
  await setting.scrollIntoViewIfNeeded();
  await expect(setting).toContainText("Archive chats you haven't used in");
  await expect(setting).toContainText('A chat moves to Archived 30 days after its last message');
  await shot(main, '01-the-setting');
  await closeSettings(main);

  // Undo (the notice's own): both back, the open Chat view's list too.
  expect(await yap.invoke('chats_auto_archive_undo', { ids: [101, 102] })).toBe(2);
  await expect(row('Bread recipes')).toBeVisible();
  await expect(main.getByRole('button', { name: /^Archived/ })).toHaveCount(0);
  expect(await archivedTitles(yap)).toEqual([]);
  await closeToasts(main);
});

test('a run says what it archived, with Undo; a Code chat with a ▶ Run running is kept until it ends', async ({ yap, main, shot }) => {
  await openView(main, 'Chat');
  await closeToasts(main);
  // "Build scripts" is a Code chat in garden-planner, a long ▶ Run running in it.
  if (!(await yap.invoke('workspaces_list')).some((f) => f.name === 'garden-planner')) await yap.invoke('workspaces_add', { path: GARDEN });
  await yap.invoke('code_chat_workspace_set', { conversation: 102, folder: 'garden-planner' });
  const run = await yap.invoke('code_run_start', { conversation: 102, command: 'Start-Sleep -Seconds 60', lang: 'powershell', asked: true });
  expect(run.state).toBe('running');

  // A run: the plain chat goes, the working one stays.
  expect(await yap.invoke('e2e_chat_auto_archive')).toEqual([101]);
  expect(await archivedTitles(yap)).toEqual(['Bread recipes']);
  // Said once, quietly, in the window: one chat, the days, Undo.
  const said = notice(main);
  await expect(said).toHaveCount(1);
  await expect(said).toContainText("Archived a chat you hadn't used in 30 days");
  await expect(said).toContainText('in Archived');
  await shot(main, '02-the-notice');
  await said.getByRole('button', { name: 'Undo' }).click();
  await expect(main.getByRole('status').filter({ hasText: 'Chat restored' })).toBeVisible();
  await expect(main.locator('.conv').filter({ hasText: 'Bread recipes' })).toBeVisible();
  expect(await archivedTitles(yap)).toEqual([]);
  await closeToasts(main);

  // Its work over, the next run takes the Code chat too (and "Bread recipes",
  // which Undo brought back as it was: still 40 days since its last message).
  await yap.invoke('code_run_stop', { id: run.id });
  await expect.poll(async () => (await yap.invoke('code_runs', { conversation: 102 }))[0]?.state, { timeout: 15_000 }).not.toBe('running');
  expect((await yap.invoke('e2e_chat_auto_archive')).sort()).toEqual([101, 102]);
  await expect(notice(main)).toContainText("Archived 2 chats you hadn't used in 30 days");
  expect(await archivedTitles(yap)).toEqual(['Bread recipes', 'Build scripts']);
  // Already archived: the next run has nothing to do.
  expect(await yap.invoke('e2e_chat_auto_archive')).toEqual([]);
  await closeToasts(main);
});
