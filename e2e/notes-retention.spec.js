// "Delete meeting transcripts after" (Settings → Privacy & data;
// src-tauri/src/notes/retention.rs), end to end on seeded meetings: one from
// 400 days ago, with its audio kept, and one from last week. Set to 365
// days, the old meeting's transcript goes at once — its lines, digests and
// stop marks, and its audio with them — while its notes, its summary and
// its tasks stay, and the note says what happened; last week's meeting is
// untouched. Off (the default), nothing is deleted.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, openSettings, closeSettings, expectStore } from './support/fixtures.js';

const DAY = 86_400;
const NOW = Math.floor(Date.now() / 1000);
const OLD = NOW - 400 * DAY;
const RECENT = NOW - 7 * DAY;
const PLAN = '## Action plan\n\n### You\n- [ ] Send the quote (due: Friday)';

const line = (source, text, ts, startMs) => ({ source, text, ts, startMs, endMs: startMs + 3_000 });

const SEED = {
  folders: ['Personal', 'Meetings'],
  notes: [
    {
      id: 1,
      title: 'Old supplier call',
      noteType: 'meeting',
      folder: 'Meetings',
      source: 'meeting',
      content: 'Ask about the delivery dates',
      enhancedContent: PLAN,
      participants: ['Priya'],
      createdTs: OLD - 600,
      updatedTs: OLD,
      breaks: [2],
      transcript: [line('you', 'Can we move the delivery to March?', OLD - 300, 0), line('them', 'March works for us.', OLD, 300_000)],
      digests: [{ fromSeg: 0, toSeg: 2, startTs: OLD - 300, endTs: OLD, keyPoints: ['Delivery moves to March'] }],
    },
    {
      id: 2,
      title: 'Recent planning call',
      noteType: 'meeting',
      folder: 'Meetings',
      source: 'meeting',
      content: '',
      createdTs: RECENT - 600,
      updatedTs: RECENT,
      transcript: [line('them', 'The launch is on track.', RECENT, 0)],
    },
  ],
};

// The old meeting's audio, as Yap keeps it (the files' contents don't matter here).
function seedAudio(dataDir) {
  const dir = path.join(dataDir, 'meeting-audio', '1');
  fs.mkdirSync(dir, { recursive: true });
  const rec = String((OLD - 600) * 1000);
  fs.writeFileSync(path.join(dir, `${rec}-you.flac`), Buffer.from('fLaC'));
  fs.writeFileSync(path.join(dir, `${rec}-them.flac`), Buffer.from('fLaC'));
  fs.writeFileSync(path.join(dir, `${rec}.json`), JSON.stringify({ noteId: 1, recording: Number(rec), startedMs: Number(rec), endedMs: OLD * 1000, keep: true }));
}

const test = base.extend({
  yapOptions: [
    async ({}, use) => {
      // Kept for 400 days by its own setting: only the transcript rule takes it.
      await use({ name: 'notes-retention', data: { 'notes.json': SEED }, prepare: seedAudio, config: { meetingAudioDays: 500, meetingReminder: 'never' } });
    },
    { scope: 'worker' },
  ],
});

const noteOf = (yap, id) => yap.readJson('notes.json').notes.find((n) => n.id === id);

test('transcripts older than the days set go; the notes, summary and tasks stay', async ({ yap, main, shot }) => {
  // Off by default: nothing goes (the sweep at start left both alone).
  await main.waitForTimeout(1_500);
  expect(noteOf(yap, 1).transcript).toHaveLength(2);
  expect((await yap.invoke('get_config')).meetingTranscriptDays).toBe(0);
  const audio = path.join(yap.dataDir, 'meeting-audio', '1');
  expect(fs.readdirSync(audio)).toHaveLength(3);

  // Settings → Privacy & data → Delete meeting transcripts after: 365 days.
  const dialog = await openSettings(main, 'Privacy & data');
  const select = dialog.getByRole('combobox', { name: 'Delete meeting transcripts after' });
  await expect(select).toHaveValue('0');
  await select.selectOption({ label: '365 days' });
  const row = dialog.locator('[data-setting="meeting-transcripts"]');
  await expect(row).toContainText('go 365 days after it');
  await row.scrollIntoViewIfNeeded();
  await shot(main, '01-privacy-transcript-days');
  await expectStore(yap, 'config.json', (c) => c.meetingTranscriptDays === 365);

  // The old meeting's transcript goes at once, its audio with it.
  await expectStore(yap, 'notes.json', (s) => s.notes.find((n) => n.id === 1)?.transcript?.length === 0, 'the old transcript deleted');
  const old = noteOf(yap, 1);
  expect(old.transcriptDeletedTs).toBeGreaterThanOrEqual(NOW);
  expect(old.digests).toEqual([]);
  expect(old.breaks).toBeUndefined();
  // What the person typed, the summary and its task, the attendees: kept.
  expect(old).toMatchObject({ title: 'Old supplier call', content: 'Ask about the delivery dates', enhancedContent: PLAN, participants: ['Priya'] });
  expect(old.updatedTs, 'not floated to the top as edited now').toBe(OLD);
  await expect.poll(() => fs.existsSync(audio)).toBe(false);
  // Last week's meeting: untouched.
  expect(noteOf(yap, 2).transcript).toHaveLength(1);
  expect(noteOf(yap, 2).transcriptDeletedTs).toBeUndefined();
  await closeSettings(main);

  // The note says what happened; its summary and tasks are there.
  await openView(main, 'Notes');
  await main.locator('.folder').filter({ hasText: 'Meetings' }).click();
  await main.locator('.item').filter({ hasText: 'Old supplier call' }).click();
  await expect(main.locator('.tdeleted')).toContainText("This meeting's transcript was deleted");
  await expect(main.getByRole('log', { name: 'Meeting transcript' })).toHaveCount(0);
  await main.locator('.tabs .tab').filter({ hasText: 'Tasks' }).click();
  await expect(main.getByRole('region', { name: 'Tasks' }).getByRole('checkbox', { name: 'Send the quote' })).toBeVisible();
  await shot(main, '02-transcript-deleted-note');
  // The notepad says it too.
  await yap.invoke('notepad_open', { noteId: 1 });
  await yap.notepad.getByRole('tab', { name: /Transcript/ }).click();
  await expect(yap.notepad.locator('.tempty')).toContainText('Transcript deleted');
});
