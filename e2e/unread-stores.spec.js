// Stores Yap couldn't read when it loaded them (shell/saving.rs, review
// 2026-10-30). Until then any read error at load — another program holding
// notes.json (an antivirus, a backup tool, a sync client), a cloud file
// offline — counted as "no file", the store started empty, and its next
// save wrote that over every note. `YAP_E2E_FAIL_READS` makes every read of
// notes.json and history.json fail from the start, as another program
// holding them would.
//
// - The person is told once: "Yap couldn't read your notes", with Try again.
// - Yap keeps working in memory: a note made and a dictation made meanwhile
//   are kept there, said once ("Couldn't save your note: Yap couldn't read
//   the file…"), and the files on disk are never written over.
// - Once the files read again, what was in them and what was added are kept
//   together, and saved.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, expectStore, openView } from './support/fixtures.js';

const NOTES = {
  folders: ['Personal', 'Meetings', 'Garden'],
  notes: [{ id: 1, title: 'Allotment plan', content: 'Broad beans in March.', folder: 'Garden', createdTs: 1761800000, updatedTs: 1761800000 }],
};
const HISTORY = [{ ts: 1761800000, raw: 'order more compost', text: 'Order more compost.', model: 'stub', app: 'notepad.exe' }];

const test = base.extend({
  yapOptions: [
    async ({}, use) => {
      await use({
        name: 'unread-stores',
        data: { 'notes.json': NOTES, 'history.json': HISTORY },
        env: { YAP_E2E_FAIL_READS: 'notes,history' },
      });
    },
    { scope: 'worker' },
  ],
});

const raw = (yap, file) => fs.readFileSync(path.join(yap.dataDir, file), 'utf8');
const toastsWith = (main, title) => main.getByRole('status').filter({ hasText: title });

test("a store Yap couldn't read is never written over, and what was added is kept with it", async ({ yap, main, shot }) => {
  const notesBefore = raw(yap, 'notes.json');
  const historyBefore = raw(yap, 'history.json');

  // Said once, as a read Yap couldn't make.
  const notice = toastsWith(main, "Yap couldn't read your");
  await expect(notice).toBeVisible({ timeout: 15_000 });
  await expect(notice).toContainText('Another program may be using the file. Nothing was changed');
  await expect(notice.getByRole('button', { name: 'Try again' })).toBeVisible();
  await shot(main, '01-couldnt-read');

  // Working in memory: a new note, said once as a save that can't happen yet.
  await openView(main, 'Notes');
  await main.getByRole('button', { name: 'New note' }).first().click();
  const failed = toastsWith(main, "Couldn't save your note");
  await expect(failed).toBeVisible();
  await expect(failed).toContainText("Yap couldn't read the file (another program may be using it), so nothing was changed.");
  await main.getByPlaceholder('Untitled Note').fill('Seed potatoes');
  await main.getByPlaceholder('Start writing…').fill('Earlies by the shed.');
  await shot(main, '02-new-note-kept-in-memory');
  // …and a dictation.
  await yap.invoke('e2e_dictation', { text: 'water the tomatoes tonight' });

  // Never written over.
  await main.waitForTimeout(1_500);
  expect(raw(yap, 'notes.json')).toBe(notesBefore);
  expect(raw(yap, 'history.json')).toBe(historyBefore);

  // The files read again: what was there and what was added, together.
  expect(await yap.invoke('e2e_fail_reads', { store: 'notes', count: 0 })).toBe(1);
  expect(await yap.invoke('e2e_fail_reads', { store: 'history', count: 0 })).toBe(0);
  await expectStore(yap, 'notes.json', (s) =>
    ['Allotment plan', 'Seed potatoes'].every((t) => s.notes.some((n) => n.title === t)) && s.folders.includes('Garden'),
  );
  await expectStore(yap, 'history.json', (h) =>
    ['compost', 'tomatoes'].every((w) => h.some((e) => e.text.toLowerCase().includes(w))),
  );
  const notes = yap.readJson('notes.json').notes;
  expect(new Set(notes.map((n) => n.id)).size).toBe(notes.length);
  expect((await yap.invoke('note_get', { id: 1 })).content).toBe('Broad beans in March.');
  // Nothing set aside: the files were whole all along.
  expect(fs.readdirSync(yap.dataDir).filter((f) => f.includes('.corrupt-'))).toEqual([]);
});
