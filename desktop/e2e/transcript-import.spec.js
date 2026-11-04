// Import a transcript… (meetings/import.rs, TranscriptImportDialog.svelte):
// a Teams WebVTT download and a Teams Word transcript, picked together
// from the Meetings view's ⋯, are previewed (format, speakers, lines,
// length, the date, which speaker is you) before anything is made; then
// each becomes a meeting note with named lines, filed in Meetings, and its
// action plan is written by the fake model (support/fake-llm.js), the AI
// title too where the file names no meeting. Files dropped on Upload open
// the same dialog, and one it can't read says why. Every person and
// company here is made up.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, expectStore, openView, importDialog, closeToasts } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { writeDocx } from './support/docx.js';

const test = base.extend({
  fakeLlm: [
    async ({}, use) => {
      const fake = await startFakeLlm();
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({ fakeLlm }, use) => {
      await use({
        name: 'transcript-import',
        config: {
          aboutName: 'Imogen Reyes',
          llmScopes: {
            noteFormatting: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-notes' },
          },
        },
      });
    },
    { scope: 'worker' },
  ],
});

// Teams' WebVTT: cue ids, voice tags, a sentence cut across two cues.
const TEAMS_VTT = `WEBVTT

7c1d2e3f-4a5b-4c6d-8e9f-0a1b2c3d4e5f/11-0
00:00:02.400 --> 00:00:06.100
<v Priya Shah>Good morning, everyone. Let's settle the offsite venue and</v>

7c1d2e3f-4a5b-4c6d-8e9f-0a1b2c3d4e5f/11-1
00:00:06.100 --> 00:00:09.300
<v Priya Shah>the dates before the end of the week.</v>

7c1d2e3f-4a5b-4c6d-8e9f-0a1b2c3d4e5f/12-0
00:00:09.800 --> 00:00:15.200
<v Tom Baker>The venue in Lisbon has rooms free on both days, so I will book the venue by Friday.</v>

7c1d2e3f-4a5b-4c6d-8e9f-0a1b2c3d4e5f/13-0
00:01:02.000 --> 00:01:08.500
<v Priya Shah>Great, then we decided to hold the offsite in Lisbon. Thanks, Tom.</v>
`;

// Teams' Word download: the meeting's name, date and length, then blocks.
const TEAMS_DOCX = [
  'Supplier call-20261005_143000-Meeting Recording',
  'October 5, 2026, 2:30PM',
  '4m 12s',
  'Amara Okafor started transcription',
  'Amara Okafor   0:02',
  'Thanks for joining. The new prices start in November and the first delivery is in two weeks.',
  'Imogen Reyes   0:15',
  'That works for us. I will send the signed order form by Monday.',
  'Amara Okafor   1:05:40',
  'Perfect, we are done for today.',
];

const meeting = (store, pred) => store?.notes?.find((n) => n.source === 'import' && pred(n));

test('a Teams VTT and a Teams Word transcript become meetings with named speakers and a summary', async ({ yap, main, fakeLlm, shot }) => {
  const vtt = path.join(yap.runDir, 'Weekly sync-20261005_100000-Meeting Recording.vtt');
  fs.writeFileSync(vtt, TEAMS_VTT);
  const docx = writeDocx(path.join(yap.runDir, 'Supplier call.docx'), TEAMS_DOCX);

  // The Meetings view's ⋯ → Import a transcript…
  await openView(main, 'Meetings');
  await main.getByRole('button', { name: 'More', exact: true }).click();
  await main.getByRole('menuitem', { name: 'Import a transcript…' }).click();
  const dialog = importDialog(main);
  await expect(dialog).toBeVisible();
  await shot(main, '01-dialog-empty');

  // Choose files… (a test run names them in place of Windows' dialog).
  await yap.invoke('e2e_transcript_files', { paths: [vtt, docx] });
  await dialog.getByRole('button', { name: /Drop transcript files here/ }).click();
  const cards = dialog.getByRole('listitem').filter({ has: main.locator('.fname') });
  await expect(cards).toHaveCount(2);

  // The dry preview: nothing is made yet.
  const vttCard = cards.filter({ hasText: 'Weekly sync-20261005_100000-Meeting Recording.vtt' });
  await expect(vttCard).toContainText('Teams transcript (WebVTT)');
  // Two cues of one sentence are one line: 3 lines, a minute long.
  await expect(vttCard).toContainText('2 speakers · 3 lines · 1 min');
  await expect(vttCard.getByRole('list', { name: /Speakers in/ })).toContainText('Priya Shah');
  await expect(vttCard.getByRole('list', { name: /Speakers in/ })).toContainText('Tom Baker');
  await expect(vttCard).toContainText("From the file's name");
  await expect(vttCard.getByLabel(/^When /)).toHaveValue('2026-10-05T10:00');
  await expect(vttCard.getByLabel(/^Which speaker in/)).toHaveValue('');
  await expect(vttCard.getByLabel(/^Title for/)).toHaveAttribute('placeholder', 'Yap names it from what was said');

  const docxCard = cards.filter({ hasText: 'Supplier call.docx' });
  await expect(docxCard).toContainText('Teams transcript (Word)');
  await expect(docxCard).toContainText('2 speakers · 3 lines · 1 h 6 min');
  await expect(docxCard).toContainText('From the transcript');
  await expect(docxCard.getByLabel(/^When /)).toHaveValue('2026-10-05T14:30');
  // About you's name picks the person.
  await expect(docxCard.getByLabel(/^Which speaker in/)).toHaveValue('Imogen Reyes');
  await expect(docxCard).toContainText('Imogen Reyes (you)');
  await expect(docxCard.getByLabel(/^Title for/)).toHaveAttribute('placeholder', 'Supplier call');
  expect(yap.readJson('notes.json')?.notes?.some((n) => n.source === 'import') ?? false).toBe(false);
  await shot(main, '02-preview');

  await dialog.getByRole('button', { name: 'Import 2 meetings' }).click();
  await expect(dialog).toBeHidden();
  await expect(main.getByRole('status').filter({ hasText: '2 meetings imported' })).toBeVisible();

  // Two meeting notes, filed in Meetings, with named lines on the meeting's clock.
  await expectStore(yap, 'notes.json', (s) => s.notes.filter((n) => n.source === 'import').length === 2);
  const store = yap.readJson('notes.json');
  const supplier = meeting(store, (n) => n.title === 'Supplier call');
  expect(supplier).toBeTruthy();
  expect(supplier.noteType).toBe('meeting');
  expect(supplier.folder).toBe('Meetings');
  expect(supplier.titleAuto ?? false).toBe(false);
  expect(supplier.participants).toEqual(['Amara Okafor']);
  const amara = Object.entries(supplier.speakers).find(([, s]) => s.name === 'Amara Okafor');
  expect(amara?.[1].source).toBe('platform');
  expect(supplier.transcript.map((l) => [l.source, l.speaker ?? '', l.startMs])).toEqual([
    ['them', amara[0], 2_000],
    ['you', '', 15_000],
    ['them', amara[0], 3_940_000],
  ]);
  expect(supplier.transcript[0].ts).toBe(supplier.createdTs + 2);
  // (Its made-up title, "Teams call · 5 Oct, 10:00", is the AI's to replace: below.)
  const weekly = meeting(store, (n) => n.transcript.some((l) => l.text.startsWith('Good morning')));
  expect(weekly.folder).toBe('Meetings');
  expect(weekly.transcript).toHaveLength(3);
  expect(weekly.transcript[0].text).toBe("Good morning, everyone. Let's settle the offsite venue and the dates before the end of the week.");
  expect(Object.values(weekly.speakers).map((s) => s.name).sort()).toEqual(['Priya Shah', 'Tom Baker']);

  // Then the usual end of a meeting: the AI title where the file had none,
  // and both action plans, one after the other, reading the names.
  await expectStore(yap, 'notes.json', (s) => s.notes.filter((n) => n.source === 'import' && n.enhancedContent).length === 2, 'both summaries');
  await expectStore(yap, 'notes.json', (s) => meeting(s, (n) => n.id === weekly.id)?.title === 'Team Sync', 'the AI title');
  const plans = fakeLlm.requests.filter((r) => r.kind === 'actionPlan');
  expect(plans).toHaveLength(2);
  expect(plans.some((r) => r.user.includes('Priya Shah') && r.user.includes('Tom Baker'))).toBe(true);
  expect(plans.some((r) => r.user.includes('Amara Okafor'))).toBe(true);
  expect(fakeLlm.requests.filter((r) => r.kind === 'title')).toHaveLength(1);
  await closeToasts(main);

  // In Notes: the named lines, and no ▶ or Transcribe again (no audio).
  await openView(main, 'Meetings');
  await main.getByRole('button', { name: 'Supplier call', exact: true }).click();
  const log = main.getByRole('log', { name: 'Meeting transcript' });
  await expect(log.getByRole('article', { name: /^Amara Okafor: Thanks for joining/ })).toBeVisible();
  await expect(log.getByRole('article', { name: /^You: That works for us/ })).toBeVisible();
  await expect(main.getByRole('button', { name: 'Play this line' })).toHaveCount(0);
  await shot(main, '03-imported-in-notes');
  const item = main.locator('.items .item').filter({ hasText: 'Supplier call' });
  await item.click({ button: 'right' });
  await expect(main.getByRole('menuitem', { name: 'Copy as Markdown' })).toBeVisible();
  await expect(main.getByRole('menuitem', { name: 'Transcribe again…' })).toHaveCount(0);
  await expect(main.getByRole('menuitem', { name: 'Find speakers' })).toHaveCount(0);
  await main.keyboard.press('Escape');

  // The Meetings view marks them, and Rust refuses the audio jobs.
  await openView(main, 'Meetings');
  await expect(main.getByRole('listitem').filter({ hasText: 'Supplier call' }).getByText('Imported', { exact: true })).toBeVisible();
  await expect(yap.invoke('meeting_transcribe_again', { noteId: supplier.id })).rejects.toThrow(/imported from a transcript file/);
});

test("files dropped on Upload open the import, and one it can't read says why", async ({ yap, main, shot }) => {
  const srt = path.join(yap.runDir, 'design-review.srt');
  fs.writeFileSync(
    srt,
    '1\r\n00:00:01,000 --> 00:00:04,000\r\nSam Lee: Shall we start with the sketches?\r\n\r\n2\r\n00:00:04,500 --> 00:00:07,000\r\nRuth Adeyemi: Yes, the second one is closest.\r\n'
  );
  const prose = path.join(yap.runDir, 'shopping list.txt');
  fs.writeFileSync(prose, 'Eggs and flour.\nMilk for the week.\n');

  await openView(main, 'Upload');
  await expect(main.getByRole('button', { name: 'Import a transcript…' })).toBeVisible();
  await expect(async () => {
    await yap.invoke('plugin:event|emit', {
      event: 'tauri://drag-drop',
      payload: { paths: [srt, prose], position: { x: 600, y: 400 } },
    });
    await expect(importDialog(main)).toBeVisible({ timeout: 1_000 });
  }).toPass();
  const dialog = importDialog(main);
  const cards = dialog.getByRole('listitem').filter({ has: main.locator('.fname') });
  await expect(cards).toHaveCount(2);
  await expect(cards.filter({ hasText: 'design-review.srt' })).toContainText('SubRip captions (.srt)');
  await expect(cards.filter({ hasText: 'design-review.srt' })).toContainText('2 speakers · 2 lines');
  const bad = cards.filter({ hasText: 'shopping list.txt' });
  await expect(bad.getByRole('alert')).toContainText("Couldn't read it. Yap found no transcript in this file");
  // Only the file it can read counts.
  await expect(dialog.getByRole('button', { name: 'Import 1 meeting' })).toBeEnabled();
  await shot(main, '04-drop-with-an-unreadable-file');

  // Taken out of the list, then Cancel: nothing was made.
  await bad.getByRole('button', { name: 'Remove shopping list.txt' }).click();
  await expect(cards).toHaveCount(1);
  await dialog.getByRole('button', { name: 'Cancel' }).click();
  await expect(dialog).toBeHidden();
  expect(yap.readJson('notes.json')?.notes?.some((n) => n.transcript?.some((l) => l.text.includes('sketches'))) ?? false).toBe(false);
});
