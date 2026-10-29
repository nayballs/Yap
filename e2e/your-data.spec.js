// Download your data and Reset app (Settings → Privacy & data → Your data;
// src/lib/settings/YourData.svelte, src-tauri/src/privacy/export.rs and reset.rs):
// - Download your data: one .zip, saved where the test says (Windows' Save
//   dialog in real use, `e2e_save_path` here) — the history as CSV and JSON,
//   each note and meeting as Markdown, chats, tasks, memory, the dictionary
//   as the Dictionary's CSV, settings with no key in them; with the tick box
//   (it says the size), the meetings' and dictations' audio and Chat's
//   screenshots. A big one shows its progress, and Cancel leaves no file.
// - Reset app: Yap's confirm dialog names what goes (with counts) and what
//   stays, and offers Download your data first; then every store is empty on
//   disk and in memory (a new note and a new dictation are all that's saved
//   after), the views show it without a restart, and the settings, the
//   dictionary and the keys are as they were.
//
// Fictional people, fake keys and made-up bytes only.
import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';
import { test as base, expect, openSettings, settingsDialog, closeSettings, openView, expectStore } from './support/fixtures.js';

const GROQ_KEY = 'gsk_FAKEyourdata0123456789abcXYZ';
const EXA_KEY = 'exa-fake-yourdata-0000-1111-2222';
const NOW = Math.floor(Date.now() / 1000);
const AUDIO_ID = String((NOW - 120) * 1000);
const SHOT_ID = 'img_0123456789abcdef';
// A meeting recording's id is when it started (unix ms; meetings/meeting_audio.rs).
const REC = String((NOW - 3_600) * 1000);

const SEED = {
  'history.json': [
    { ts: NOW - 600, raw: 'pick up the leeks from example lane', text: 'Pick up the leeks from Example Lane.', model: 'parakeet-tdt-0.6b-v3', app: 'notepad.exe', audio: AUDIO_ID },
  ],
  'notes.json': {
    notes: [
      { id: 101, title: 'Allotment plans', content: 'Leeks by the shed, beans by the fence.', noteType: 'personal', createdTs: NOW - 86_400, updatedTs: NOW - 86_400 },
      {
        id: 102,
        title: 'Weekly sync',
        noteType: 'meeting',
        enhancedContent: '## Summary\n- Ship the leek tracker',
        transcript: [{ source: 'mic', text: 'Shall we ship the leek tracker?', ts: NOW - 3_600, startMs: 0, endMs: 2_000 }],
        createdTs: NOW - 3_600,
        updatedTs: NOW - 3_600,
      },
    ],
  },
  'chats.json': {
    conversations: [
      {
        id: 201,
        title: 'Leek soil',
        createdTs: NOW - 1_800,
        updatedTs: NOW - 1_800,
        messages: [
          { role: 'user', text: 'What soil do leeks like?', ts: NOW - 1_800 },
          { role: 'assistant', text: 'Rich, damp soil.', ts: NOW - 1_790 },
        ],
      },
    ],
  },
  'tasks.json': { tasks: [{ id: 301, text: 'Water the leeks', createdMs: (NOW - 600) * 1000 }], archived: [] },
  'memory.json': { version: 1, nextId: 2, items: [{ id: 1, section: 'people', title: 'Robin', summary: 'Your neighbour on Example Lane, grows leeks' }] },
};

const test = base.extend({
  yapOptions: [
    async ({}, use) => {
      await use({
        name: 'your-data',
        // A made-up microphone (dictation/mics.rs): the dictation after the reset
        // never opens a real one.
        env: { YAP_E2E_MICS: JSON.stringify([{ id: 'usb', name: 'Microphone (Fabrikam USB Mic)' }]) },
        config: {
          ppProvider: 'groq',
          ppApiKey: GROQ_KEY,
          ppApiKeys: { groq: GROQ_KEY },
          webSearchKeys: { exa: EXA_KEY },
          aboutName: 'Robin Example',
          dictionary: [
            { from: 'cooper netties', to: 'Kubernetes', fuzzy: true, snippet: false },
            { from: 'my address', to: '1 Example Lane', fuzzy: true, snippet: true },
          ],
        },
        data: SEED,
        prepare: (data) => {
          fs.mkdirSync(path.join(data, 'meeting-audio', '102'), { recursive: true });
          fs.writeFileSync(path.join(data, 'meeting-audio', '102', `${REC}-you.flac`), Buffer.alloc(4_000, 1));
          // A bigger one, so the progress and Cancel can be seen.
          fs.writeFileSync(path.join(data, 'meeting-audio', '102', `${REC}-them.flac`), Buffer.alloc(8 * 1024 * 1024, 2));
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

/** The .zip's files by name (local headers carry the sizes; stored or deflated). */
function unzip(file) {
  const zip = fs.readFileSync(file);
  const files = {};
  let at = 0;
  while (zip.readUInt32LE(at) === 0x04034b50) {
    const method = zip.readUInt16LE(at + 8);
    const packed = zip.readUInt32LE(at + 18);
    const nameLen = zip.readUInt16LE(at + 26);
    const extraLen = zip.readUInt16LE(at + 28);
    const name = zip.subarray(at + 30, at + 30 + nameLen).toString();
    const start = at + 30 + nameLen + extraLen;
    const body = zip.subarray(start, start + packed);
    files[name] = method === 8 ? zlib.inflateRawSync(body) : body;
    at = start + packed;
  }
  return files;
}

const group = (main) => settingsDialog(main).getByRole('group', { name: 'Your data' });

test('Download your data: everything in one .zip, the audio with the tick box, no key anywhere', async ({ yap, main, shot }, testInfo) => {
  // A note's earlier version (notes/versions.rs): in notes/versions.json.
  await yap.invoke('note_update', { id: 101, content: 'Leeks by the shed, beans by the fence, peas by the gate.' });
  await openSettings(main, 'Privacy & data');
  const g = group(main);
  await g.scrollIntoViewIfNeeded();
  const tick = g.getByRole('checkbox', { name: /Include audio and screenshots/ });
  // The tick box says how much the audio and screenshots come to.
  await expect(g.locator('.tick')).toContainText('8 MB');
  await tick.check();
  await shot(main, '01-your-data');

  const file = testInfo.outputPath('Yap data.zip');
  await yap.invoke('e2e_save_path', { path: file });
  await g.getByRole('button', { name: 'Download…' }).click();
  await expect(g.getByRole('heading', { name: /^Saved Yap data\.zip/ })).toBeVisible({ timeout: 30_000 });
  await expect(g).toContainText('Nothing was uploaded');
  await shot(main, '02-saved');

  const files = unzip(file);
  const names = Object.keys(files);
  for (const want of [
    'README.txt',
    'history/dictations.csv',
    'history/history.json',
    'notes/notes.json',
    'notes/versions.json',
    'chats/chats.json',
    'tasks.json',
    'memory/People.md',
    'memory/memory.json',
    'dictionary/dictionary.csv',
    'settings.json',
    'audio/meetings/' + names.find((n) => n.startsWith('audio/meetings/'))?.split('/')[2] + `/${REC}-you.flac`,
    `screenshots/${SHOT_ID}.png`,
  ]) {
    expect(names, want).toContain(want);
  }
  expect(names.some((n) => /^notes\/.* Allotment plans\.md$/.test(n))).toBe(true);
  const versions = JSON.parse(files['notes/versions.json'].toString('utf8'));
  expect(versions.find((v) => v.note === 101)?.versions.map((x) => x.text)).toEqual([
    'Leeks by the shed, beans by the fence, peas by the gate.',
    'Leeks by the shed, beans by the fence.',
  ]);
  expect(names.some((n) => /^meetings\/.* Weekly sync\.md$/.test(n))).toBe(true);
  expect(names.some((n) => /^chats\/.* Leek soil\.md$/.test(n))).toBe(true);
  expect(names.filter((n) => n.startsWith('audio/dictations/'))).toHaveLength(1);
  const text = (n) => files[n].toString('utf8');
  expect(text('history/dictations.csv')).toContain('Pick up the leeks from Example Lane.');
  expect(text(names.find((n) => n.endsWith('Weekly sync.md')))).toContain('Shall we ship the leek tracker?');
  expect(text(names.find((n) => n.endsWith('Leek soil.md')))).toContain('Rich, damp soil.');
  expect(text('tasks.json')).toContain('Water the leeks');
  expect(text('memory/People.md')).toContain('Robin');
  // The dictionary as the Dictionary's Export… writes it.
  const dict = text('dictionary/dictionary.csv');
  expect(dict.startsWith('﻿Heard,Written,Kind,Near-misses,Starred\r\n')).toBe(true);
  expect(dict).toContain('cooper netties,Kubernetes,correction,yes,');
  expect(dict).toContain('my address,1 Example Lane,snippet,,');
  // Settings: the person's own, without a key; what was taken out is named.
  const settings = JSON.parse(text('settings.json'));
  expect(settings.aboutName).toBe('Robin Example');
  expect(settings.leftOut).toEqual(expect.arrayContaining(['ppApiKey', 'ppApiKeys', 'webSearchKeys']));
  const raw = fs.readFileSync(file);
  for (const key of [GROQ_KEY, EXA_KEY]) expect(raw.includes(key), key).toBe(false);
  // Done: back to the tick box.
  await g.getByRole('button', { name: 'Done' }).click();
  await expect(tick).toBeVisible();
  await closeSettings(main);
});

test('Download your data: a big one shows its progress, and Cancel leaves nothing', async ({ yap, main }, testInfo) => {
  await yap.invoke('e2e_export_pace', { ms: 500 });
  try {
    await openSettings(main, 'Privacy & data');
    const g = group(main);
    await g.scrollIntoViewIfNeeded();
    await g.getByRole('checkbox', { name: /Include audio and screenshots/ }).check();
    const file = testInfo.outputPath('cancelled.zip');
    await yap.invoke('e2e_save_path', { path: file });
    await g.getByRole('button', { name: 'Download…' }).click();
    const progress = g.getByRole('progressbar');
    await expect(progress).toBeVisible();
    await expect.poll(async () => Number(await progress.getAttribute('aria-valuenow'))).toBeGreaterThan(0);
    await g.getByRole('button', { name: /^Cancel/ }).click();
    await expect(main.getByRole('status').filter({ hasText: 'Nothing was saved' })).toBeVisible();
    await expect(g.getByRole('button', { name: 'Download…' })).toBeVisible();
    expect(fs.existsSync(file)).toBe(false);
    expect(fs.existsSync(`${file}.partial`)).toBe(false);
    await closeSettings(main);
  } finally {
    await yap.invoke('e2e_export_pace', { ms: 0 });
  }
});

test('Reset app: names what goes, clears it everywhere, keeps the settings; no restart', async ({ yap, main, shot }, testInfo) => {
  // A pin and a note's history (notes/pins.rs, notes/versions.rs): they go too.
  await yap.invoke('note_pin', { id: 101, pinned: true });
  await yap.invoke('note_update', { id: 101, content: 'Leeks by the shed, beans by the fence, onions too.' });
  // Before: the dictation is on Home.
  await openView(main, 'Home');
  await expect(main.getByText('Pick up the leeks from Example Lane.').first()).toBeVisible();

  await openSettings(main, 'Privacy & data');
  const g = group(main);
  await g.scrollIntoViewIfNeeded();
  await g.getByRole('button', { name: 'Reset app…' }).click();
  const dialog = main.getByRole('alertdialog');
  await expect(dialog).toContainText('Clear everything and keep your settings?');
  const goes = dialog.getByRole('list', { name: 'What goes' });
  await expect(goes).toContainText('Your dictation history, and the last day of dictation audio · 1 dictation');
  await expect(goes).toContainText('Every note and meeting, with their audio, earlier versions and pins (your folders and actions stay) · 1 note, 1 meeting');
  await expect(goes).toContainText('Every chat, with its screenshots · 1 chat');
  await expect(goes).toContainText('What Yap remembers about you · 1 thing');
  await expect(goes).toContainText('Your tasks · 1 task');
  await expect(goes).toContainText('The search index');
  await expect(goes).toContainText('Usage stats');
  await expect(dialog.getByRole('list', { name: 'What stays' })).toContainText('Your settings, dictionary and snippets');
  await shot(main, '03-reset-dialog');

  // "Download your data first": the dialog closes and the file is saved.
  const file = testInfo.outputPath('before-reset.zip');
  await yap.invoke('e2e_save_path', { path: file });
  await dialog.getByRole('button', { name: 'Download your data first' }).click();
  await expect(dialog).toHaveCount(0);
  await expect(g.getByRole('heading', { name: /^Saved before-reset\.zip/ })).toBeVisible({ timeout: 30_000 });
  expect(Object.keys(unzip(file))).toContain('history/dictations.csv');
  await g.getByRole('button', { name: 'Done' }).click();

  // Now the reset.
  await g.getByRole('button', { name: 'Reset app…' }).click();
  await dialog.getByRole('button', { name: 'Reset app' }).click();
  await expect(main.getByRole('status').filter({ hasText: 'Yap is reset' })).toBeVisible();
  await expect(dialog).toHaveCount(0);

  // On disk: every store empty, the folders gone; the settings kept.
  await expectStore(yap, 'notes.json', (s) => s.notes.length === 0 && s.folders.length > 0);
  await expectStore(yap, 'chats.json', (s) => s.conversations.length === 0);
  await expectStore(yap, 'memory.json', (s) => s.items.length === 0);
  await expectStore(yap, 'tasks.json', (s) => s.tasks.length === 0);
  await expectStore(yap, 'config.json', (c) => c.ppApiKey === GROQ_KEY && c.webSearchKeys.exa === EXA_KEY && c.dictionary.length === 2 && c.aboutName === 'Robin Example');
  const data = yap.dataDir;
  expect(fs.existsSync(path.join(data, 'history.json'))).toBe(false);
  for (const dir of ['meeting-audio', 'recent-audio', 'search-index']) {
    expect(fs.existsSync(path.join(data, dir)), dir).toBe(false);
  }
  await expectStore(yap, 'note_pins.json', (s) => s.pins.length === 0);
  const versionsDir = path.join(data, 'note-versions');
  expect(fs.existsSync(versionsDir) ? fs.readdirSync(versionsDir) : []).toEqual([]);
  const shots = path.join(data, 'chat-images');
  expect(fs.existsSync(shots) ? fs.readdirSync(shots).filter((f) => f.endsWith('.png')) : []).toEqual([]);

  // The views show it at once: no restart.
  await closeSettings(main);
  await openView(main, 'Home');
  await expect(main.getByText('Pick up the leeks from Example Lane.')).toHaveCount(0);
  await openView(main, 'Notes');
  await expect(main.getByText('Allotment plans')).toHaveCount(0);
  await shot(main, '04-after-reset');

  // Nothing old comes back with the next save: a new note and a new
  // dictation are all there is.
  await yap.invoke('note_create', { title: 'Fresh start', content: '' });
  await expectStore(yap, 'notes.json', (s) => s.notes.length === 1 && s.notes[0].title === 'Fresh start');
  await yap.invoke('e2e_dictate_audio', { seconds: 1 });
  await expectStore(yap, 'history.json', (h) => h.length === 1 && !JSON.stringify(h).includes('leeks'));
});
