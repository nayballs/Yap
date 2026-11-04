// Delete some of your data (Settings → Privacy & data → Your data;
// src/lib/settings/YourData.svelte + DeleteSomeData.svelte,
// src-tauri/src/privacy/reset.rs): a tick box per kind of data, each saying
// how much there is; Yap's confirm dialog names what goes and how much; each
// kind goes the way Reset app clears it, and only that kind:
// - chats alone: Chat's conversations and their screenshots go, the notes,
//   the meetings and their Ask threads stay (and memory forgets what it read
//   of the chats, never what it remembers);
// - meetings alone: each meeting with its audio, tasks and Ask threads goes,
//   the other notes (their tasks, Ask threads and pins) stay;
// - refused while a meeting records, as Reset app is.
//
// Fictional people and made-up bytes only.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openSettings, settingsDialog, closeSettings, openView, expectStore } from './support/fixtures.js';

const NOW = Math.floor(Date.now() / 1000);
const AUDIO_ID = String((NOW - 120) * 1000);
const SHOT_ID = 'img_0123456789abcdef';
// A meeting recording's id is when it started (unix ms; meetings/meeting_audio.rs).
const REC = String((NOW - 3_600) * 1000);

const SEED = {
  'history.json': [
    { ts: NOW - 600, raw: 'pick up the leeks', text: 'Pick up the leeks.', model: 'parakeet-tdt-0.6b-v3', app: 'notepad.exe', audio: AUDIO_ID },
    { ts: NOW - 300, raw: 'water the beans', text: 'Water the beans.', model: 'parakeet-tdt-0.6b-v3', app: 'notepad.exe' },
  ],
  'notes.json': {
    notes: [
      { id: 101, title: 'Allotment plans', content: 'Leeks by the shed, beans by the fence.', noteType: 'personal', createdTs: NOW - 86_400, updatedTs: NOW - 86_400 },
      {
        id: 102,
        title: 'Weekly sync',
        noteType: 'meeting',
        folder: 'Meetings',
        enhancedContent: '## Summary\n- Ship the leek tracker',
        transcript: [{ source: 'mic', text: 'Shall we ship the leek tracker?', ts: NOW - 3_600, startMs: 0, endMs: 2_000 }],
        createdTs: NOW - 3_600,
        updatedTs: NOW - 3_600,
      },
      { id: 103, title: 'Seed list', content: 'Leeks, beans, peas.', noteType: 'personal', source: 'quick', createdTs: NOW - 7_200, updatedTs: NOW - 7_200 },
    ],
  },
  'note_pins.json': { pins: [{ id: 101, created: NOW - 86_400, ms: (NOW - 600) * 1000 }] },
  'chats.json': {
    conversations: [
      {
        id: 201,
        title: 'Leek soil',
        createdTs: NOW - 1_800,
        updatedTs: NOW - 1_800,
        messages: [
          { role: 'user', text: 'What soil do leeks like?', ts: NOW - 1_800, images: [{ id: SHOT_ID, w: 10, h: 10, bytes: 500, source: 'snip' }] },
          { role: 'assistant', text: 'Rich, damp soil.', ts: NOW - 1_790 },
        ],
      },
      // The meeting's Ask thread, and the note's.
      { id: 202, title: 'What was decided?', note: 102, createdTs: NOW - 3_000, updatedTs: NOW - 3_000, messages: [{ role: 'user', text: 'What was decided?', ts: NOW - 3_000 }] },
      { id: 203, title: 'When do leeks go in?', note: 101, createdTs: NOW - 2_000, updatedTs: NOW - 2_000, messages: [{ role: 'user', text: 'When do leeks go in?', ts: NOW - 2_000 }] },
    ],
  },
  'tasks.json': {
    tasks: [
      { id: 301, text: 'Water the leeks', createdMs: (NOW - 600) * 1000 },
      { id: 302, text: 'Ship the leek tracker', noteId: 102, createdMs: (NOW - 3_000) * 1000 },
      { id: 303, text: 'Buy seed', noteId: 101, createdMs: (NOW - 2_000) * 1000 },
    ],
    archived: [],
  },
  'memory.json': {
    version: 1,
    nextId: 2,
    items: [{ id: 1, section: 'people', title: 'Robin', summary: 'Your neighbour on Example Lane, grows leeks' }],
    read: { 201: 2, 203: 1 },
    meetings: [102],
  },
  'word_suggestions.json': { lastOfferedAt: 1, pairs: { 'cooper netties→kubernetes': { notNow: 1 } }, removed: [] },
};

const MEETING_AUDIO = 2 * 1024 * 1024;

const test = base.extend({
  yapOptions: [
    async ({}, use) => {
      await use({
        name: 'your-data-delete',
        data: SEED,
        prepare: (data) => {
          fs.mkdirSync(path.join(data, 'meeting-audio', '102'), { recursive: true });
          fs.writeFileSync(path.join(data, 'meeting-audio', '102', `${REC}-you.flac`), Buffer.alloc(MEETING_AUDIO, 1));
          fs.mkdirSync(path.join(data, 'recent-audio'), { recursive: true });
          fs.writeFileSync(path.join(data, 'recent-audio', `${AUDIO_ID}.wav`), Buffer.alloc(3_000, 3));
          fs.mkdirSync(path.join(data, 'chat-images'), { recursive: true });
          fs.writeFileSync(path.join(data, 'chat-images', `${SHOT_ID}.png`), Buffer.alloc(500, 4));
        },
      });
    },
    { scope: 'worker' },
  ],
});

const group = (main) => settingsDialog(main).getByRole('group', { name: 'Your data' });
const picker = (main) => main.getByRole('dialog', { name: 'Delete some of your data' });
const amount = (main, kind) => picker(main).locator(`.amount[data-kind="${kind}"]`);
const tickBox = (main, label) => picker(main).getByRole('checkbox', { name: new RegExp(`^${label}`) });

/** Settings → Privacy & data → Your data → Delete…: the picker. */
async function openPicker(main) {
  await openSettings(main, 'Privacy & data');
  const g = group(main);
  await g.scrollIntoViewIfNeeded();
  await g.getByRole('button', { name: 'Delete…', exact: true }).click();
  await expect(picker(main)).toBeVisible();
  return picker(main);
}

const ids = (list) => list.map((x) => x.id);

test('the dialog says how much there is of each kind; nothing goes until it is asked', async ({ yap, main, shot }) => {
  const p = await openPicker(main);
  for (const heading of ['Dictation', 'Notes and meetings', 'Chat and memory', 'Dictionary', "Yap's own records"]) {
    await expect(p.getByRole('heading', { name: heading, exact: true })).toBeVisible();
  }
  await expect(amount(main, 'history')).toHaveText(/^2 dictations · \d+ KB$/);
  await expect(amount(main, 'dictationAudio')).toHaveText('1 recording · 3 KB');
  await expect(amount(main, 'meetings')).toHaveText('1 meeting · 2 MB');
  await expect(amount(main, 'notes')).toHaveText('2 notes');
  await expect(amount(main, 'tasks')).toHaveText('1 task');
  await expect(amount(main, 'chats')).toHaveText('1 chat, 1 screenshot · 1 KB');
  await expect(amount(main, 'memory')).toHaveText(/^1 thing · \d+ KB$/);
  await expect(amount(main, 'suggestions')).toHaveText(/^1 word offered · \d+ KB$/);
  // Search by meaning is off: no index, nothing to tick.
  await expect(amount(main, 'searchIndex')).toHaveText('Nothing kept');
  await expect(tickBox(main, 'Search index')).toBeDisabled();
  // Each says what it removes.
  await expect(p).toContainText('Every chat, archived ones too, with its screenshots. Documents Chat wrote stay in Notes');
  // The pages beside the chat keep their own cookies and sign-ins (their WebView2 profile).
  await expect(p).toContainText('Cookies, sign-ins and site data from the web pages opened beside the chat');
  await expect(p).toContainText('Your settings, dictionary, snippets, Skills, transforms, writing samples, models, paired phones and account stay.');
  const next = p.getByRole('button', { name: 'Delete…', exact: true });
  await expect(next).toBeDisabled();
  await tickBox(main, 'Chats').check();
  await tickBox(main, 'Meetings').check();
  await expect(p).toContainText('2 kinds ticked · 2 MB');
  await shot(main, '01-delete-some');
  // The confirm dialog names what goes and how much; Cancel goes back to
  // the ticks, and Cancel there leaves everything as it was.
  await next.click();
  const confirm = main.getByRole('alertdialog');
  await expect(confirm).toContainText('Delete these for good?');
  const goes = confirm.getByRole('list', { name: 'What goes' });
  await expect(goes).toContainText('Meetings · 1 meeting · 2 MB');
  await expect(goes).toContainText('Chats · 1 chat, 1 screenshot · 1 KB');
  await shot(main, '02-confirm');
  await confirm.getByRole('button', { name: 'Cancel' }).click();
  await expect(confirm).toHaveCount(0);
  await expect(tickBox(main, 'Chats')).toBeChecked();
  await p.getByRole('button', { name: 'Cancel' }).click();
  await expect(p).toHaveCount(0);
  await closeSettings(main);
  expect(ids(yap.readJson('chats.json').conversations)).toEqual([201, 202, 203]);
  expect(ids(yap.readJson('notes.json').notes)).toEqual([101, 102, 103]);
});

test('deleting chats alone: the notes, meetings and their Ask threads stay', async ({ yap, main }) => {
  const p = await openPicker(main);
  await tickBox(main, 'Chats').check();
  await p.getByRole('button', { name: 'Delete…', exact: true }).click();
  const confirm = main.getByRole('alertdialog');
  await expect(confirm.getByRole('list', { name: 'What goes' })).toHaveText('Chats · 1 chat, 1 screenshot · 1 KB');
  await confirm.getByRole('button', { name: 'Delete', exact: true }).click();
  await expect(main.getByRole('status').filter({ hasText: 'Deleted from this PC' })).toContainText('Chats. No restart needed.');
  await expect(p).toHaveCount(0);

  // On disk: Chat's conversation and its screenshot are gone; the Ask threads stay.
  await expectStore(yap, 'chats.json', (s) => JSON.stringify(ids(s.conversations)) === '[202,203]');
  const shots = path.join(yap.dataDir, 'chat-images');
  expect(fs.existsSync(shots) ? fs.readdirSync(shots).filter((f) => f.endsWith('.png')) : []).toEqual([]);
  // Memory forgets what it read of the chat, and remembers what it remembers.
  await expectStore(yap, 'memory.json', (m) => m.items.length === 1 && !('201' in m.read) && m.read['203'] === 1 && m.meetings.includes(102));
  // Everything else is as it was.
  expect(ids(yap.readJson('notes.json').notes)).toEqual([101, 102, 103]);
  expect(ids(yap.readJson('tasks.json').tasks)).toEqual([301, 302, 303]);
  expect(yap.readJson('history.json')).toHaveLength(2);
  expect(fs.existsSync(path.join(yap.dataDir, 'meeting-audio', '102', `${REC}-you.flac`))).toBe(true);
  expect(fs.existsSync(path.join(yap.dataDir, 'recent-audio', `${AUDIO_ID}.wav`))).toBe(true);

  // The picker says so: no chats left to tick.
  await group(main).getByRole('button', { name: 'Delete…', exact: true }).click();
  await expect(amount(main, 'chats')).toHaveText('Nothing kept');
  await expect(tickBox(main, 'Chats')).toBeDisabled();
  await expect(amount(main, 'meetings')).toHaveText('1 meeting · 2 MB');
  await picker(main).getByRole('button', { name: 'Cancel' }).click();
  await closeSettings(main);

  // The views show it at once.
  await openView(main, 'Chat');
  await expect(main.getByText('Leek soil')).toHaveCount(0);
  await openView(main, 'Notes');
  await expect(main.getByText('Allotment plans').first()).toBeVisible();
  await openView(main, 'Meetings');
  await expect(main.getByText('Weekly sync').first()).toBeVisible();
});

test('deleting meetings alone: their audio, tasks and Ask threads go, the other notes stay', async ({ yap, main, shot }) => {
  const p = await openPicker(main);
  await tickBox(main, 'Meetings').check();
  await p.getByRole('button', { name: 'Delete…', exact: true }).click();
  const confirm = main.getByRole('alertdialog');
  await expect(confirm.getByRole('list', { name: 'What goes' })).toHaveText('Meetings · 1 meeting · 2 MB');
  await confirm.getByRole('button', { name: 'Delete', exact: true }).click();
  await expect(main.getByRole('status').filter({ hasText: 'Deleted from this PC' })).toContainText('Meetings. No restart needed.');

  // On disk: the meeting and everything kept with it.
  await expectStore(yap, 'notes.json', (s) => JSON.stringify(ids(s.notes)) === '[101,103]' && s.folders.length > 0);
  expect(fs.existsSync(path.join(yap.dataDir, 'meeting-audio'))).toBe(false);
  await expectStore(yap, 'chats.json', (s) => JSON.stringify(ids(s.conversations)) === '[203]');
  await expectStore(yap, 'tasks.json', (s) => JSON.stringify(ids(s.tasks)) === '[301,303]');
  await expectStore(yap, 'memory.json', (m) => !m.meetings.includes(102) && m.read['203'] === 1 && m.items.length === 1);
  // The other notes keep their pin, and the history stays.
  expect(yap.readJson('note_pins.json').pins.map((x) => x.id)).toEqual([101]);
  expect(yap.readJson('history.json')).toHaveLength(2);

  await closeSettings(main);
  await openView(main, 'Notes');
  await expect(main.getByText('Allotment plans').first()).toBeVisible();
  await openView(main, 'Meetings');
  await expect(main.getByText('Weekly sync')).toHaveCount(0);
  await shot(main, '03-after-meetings');
});

test('nothing is deleted while a meeting records, as with Reset app', async ({ yap, main }) => {
  const recording = () => yap.invoke('meeting_state').then((s) => s.recording);
  const note = await yap.invoke('note_create', { title: 'Standup', folder: 'Meetings' });
  await yap.invoke('meeting_start', { noteId: note.id });
  try {
    await expect.poll(recording).toBe(true);
    await openSettings(main, 'Privacy & data');
    const g = group(main);
    await g.scrollIntoViewIfNeeded();
    await g.getByRole('button', { name: 'Delete…', exact: true }).click();
    await expect(main.getByRole('status').filter({ hasText: "Yap can't delete your data right now" })).toContainText(
      'Stop the meeting first: Yap is recording it.'
    );
    await expect(picker(main)).toHaveCount(0);
    // The command says so too, whatever the page does.
    await expect(yap.invoke('data_delete', { kinds: ['notes'] })).rejects.toThrow(/Stop the meeting first/);
    await expect(yap.invoke('data_reset')).rejects.toThrow(/Stop the meeting first/);
    expect(ids(yap.readJson('notes.json').notes)).toContain(101);
    await closeSettings(main);
  } finally {
    if (await recording()) {
      await yap.invoke('meeting_pause');
      await expect.poll(recording, { timeout: 20_000 }).toBe(false);
    }
  }
});
