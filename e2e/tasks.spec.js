// Tasks, the rest (src-tauri/src/notes/tasks.rs; src/lib/components/MeetingTasks.svelte,
// src/lib/views/TasksView.svelte), end to end on a seeded meeting with an
// action plan:
// - a meeting's **Tasks tab** (the notepad and the Notes view): "1 of 4
//   done", a task added there ("Priya: book flights (due: Monday)" reads its
//   owner and date, kept beside the notes, never in the summary), Complete
//   all — each tick where it lives — and Undo;
// - the **Tasks view**: a task added with no meeting ("Your tasks"), ticked,
//   then **Archive done** takes the done ones out of the list (still ticked
//   in their meeting) and Undo brings them back; an added task removed.
// No AI is needed: the summary is seeded.
import { test as base, expect, openView, expectStore } from './support/fixtures.js';

const NOW = Math.floor(Date.now() / 1000);
const PLAN = [
  'The team set the offsite budget.',
  '',
  '## Action plan',
  '',
  '### You',
  '- [ ] Draft the agenda (due: tonight)',
  '- [x] Share the slides',
  '',
  '### Priya',
  '- [ ] Send the revised budget (due: Friday)',
  '',
  '## Unassigned',
  '- [ ] Book the venue',
].join('\n');

const SEED = {
  folders: ['Personal', 'Meetings'],
  notes: [
    {
      id: 1,
      title: 'Offsite budget',
      noteType: 'meeting',
      folder: 'Meetings',
      source: 'meeting',
      content: 'Ask about Lisbon',
      enhancedContent: PLAN,
      createdTs: NOW - 3_600,
      updatedTs: NOW - 3_600,
      transcript: [
        { source: 'you', text: "Let's settle the offsite budget today.", ts: NOW - 3_590, startMs: 0, endMs: 3_000 },
        { source: 'them', text: 'I will send the revised budget by Friday.', ts: NOW - 3_560, startMs: 30_000, endMs: 33_000 },
      ],
    },
  ],
};

const test = base.extend({
  yapOptions: [
    async ({}, use) => {
      await use({ name: 'tasks', data: { 'notes.json': SEED }, config: { meetingReminder: 'never' } });
    },
    { scope: 'worker' },
  ],
});

const store = (yap) => yap.readJson('notes.json');
const toastIn = (page, title) => page.getByRole('status').filter({ hasText: title });

test("a meeting's Tasks tab: done of total, add a task, Complete all and Undo", async ({ yap, main, shot }) => {
  await yap.invoke('notepad_open', { noteId: 1 });
  const pad = yap.notepad;
  await pad.getByRole('tab', { name: 'Tasks' }).click();
  const tab = pad.getByRole('region', { name: 'Tasks' });
  await expect(tab.locator('.mlabel')).toHaveText('1 of 4 done');
  await expect(tab.getByRole('checkbox')).toHaveCount(4);
  await expect(tab.getByRole('checkbox', { name: 'Share the slides' })).toBeChecked();

  // Added here: read like the summary's, kept beside it (never in it).
  const add = tab.getByRole('textbox', { name: 'Add a task' });
  await add.fill('Priya: book flights (due: Monday)');
  await add.press('Enter');
  await expect(tab.locator('.mlabel')).toHaveText('1 of 5 done');
  const added = tab.locator('.mtask').filter({ hasText: 'book flights' });
  await expect(added.locator('.mowner')).toHaveText('Priya');
  await expect(added.locator('.mdue')).toHaveText('Due Monday');
  await expect(added).toContainText('Added by you');
  await expectStore(yap, 'notes.json', (s) => s.tasks?.[0]?.text === 'Priya: book flights (due: Monday)' && s.tasks[0].noteId === 1);
  expect(store(yap).notes[0].enhancedContent).toBe(PLAN);
  expect(store(yap).notes[0].summaryEditedMs).toBeUndefined();
  await shot(pad, '01-tasks-tab-added');

  // Complete all: the summary's ticks in its markdown, the added one's its own.
  await tab.getByRole('button', { name: 'Complete all' }).click();
  await expect(tab.locator('.mlabel')).toHaveText('5 of 5 done');
  await expectStore(yap, 'notes.json', (s) => !s.notes[0].enhancedContent.includes('- [ ]') && s.tasks[0].done === true);
  expect(store(yap).notes[0].summaryEditedMs, 'ticks are not an edit').toBeUndefined();
  await expect(tab.getByRole('button', { name: 'Complete all' })).toHaveCount(0);
  // The Summary tab ticks too.
  await pad.getByRole('tab', { name: 'Summary' }).click();
  await expect(pad.getByRole('region', { name: 'Summary' }).getByRole('checkbox', { name: /Book the venue/ })).toBeChecked();
  await pad.getByRole('tab', { name: 'Tasks' }).click();
  const done = toastIn(pad, '4 tasks done');
  await expect(done).toBeVisible();
  await shot(pad, '02-tasks-complete-all');
  // Undo: the four ticked back, the one ticked before stays ticked.
  await done.getByRole('button', { name: 'Undo' }).click();
  await expectStore(yap, 'notes.json', (s) => s.notes[0].enhancedContent === PLAN && !s.tasks[0].done);
  await expect(pad.getByRole('region', { name: 'Tasks' }).locator('.mlabel')).toHaveText('1 of 5 done');

  // The Notes view's Tasks tab: the same tasks.
  await openView(main, 'Notes');
  await main.locator('.folder').filter({ hasText: 'Meetings' }).click();
  await main.locator('.item').filter({ hasText: 'Offsite budget' }).click();
  await main.locator('.tabs .tab').filter({ hasText: 'Tasks' }).click();
  const notesTab = main.getByRole('region', { name: 'Tasks' });
  await expect(notesTab.locator('.mlabel')).toHaveText('1 of 5 done');
  await notesTab.getByRole('checkbox', { name: 'book flights' }).check();
  await expectStore(yap, 'notes.json', (s) => s.tasks[0].done === true);
  // …and the notepad follows (yap-tasks-changed).
  await expect(pad.getByRole('region', { name: 'Tasks' }).locator('.mlabel')).toHaveText('2 of 5 done');
  await notesTab.getByRole('checkbox', { name: 'book flights' }).uncheck();
  await expectStore(yap, 'notes.json', (s) => !s.tasks[0].done);
  await shot(main, '03-notes-view-tasks-tab');
});

test('the Tasks view: add a task with no meeting, archive the done ones with Undo', async ({ yap, main, shot }) => {
  await openView(main, 'Tasks');
  const view = main.locator('.tasksview');
  const add = view.getByRole('textbox', { name: 'Add a task' });
  await add.fill('Renew the passport');
  await add.press('Enter');
  const mine = view.locator('section').filter({ has: main.getByRole('heading', { name: 'Your tasks' }) });
  await expect(mine.locator('.ttext')).toHaveText(['Renew the passport']);
  await expect(mine.locator('.owner')).toHaveText('You');
  await expectStore(yap, 'notes.json', (s) => s.tasks?.some((t) => t.text === 'Renew the passport' && t.noteId == null));
  // "Your tasks" comes first.
  await expect(view.locator('section h2').first()).toHaveText('Your tasks');
  await shot(main, '04-tasks-view-your-tasks');

  // Ticked, then the done ones archived: out of the list, still ticked in their meeting.
  await mine.getByRole('checkbox', { name: 'Renew the passport' }).check();
  await expectStore(yap, 'notes.json', (s) => s.tasks.find((t) => t.text === 'Renew the passport')?.done === true);
  await view.locator('.showdone input').check();
  await expect(view.locator('.ttext').filter({ hasText: 'Share the slides' })).toBeVisible();
  await view.getByRole('button', { name: /^Archive done \(2\)/ }).click();
  const archived = toastIn(main, '2 done tasks archived');
  await expect(archived).toBeVisible();
  await expect(view.locator('.ttext').filter({ hasText: 'Share the slides' })).toHaveCount(0);
  await expect(view.locator('.ttext').filter({ hasText: 'Renew the passport' })).toHaveCount(0);
  await expectStore(
    yap,
    'notes.json',
    (s) => s.notes[0].archivedTasks?.includes('Share the slides') && s.tasks.find((t) => t.text === 'Renew the passport')?.archived === true
  );
  expect(store(yap).notes[0].enhancedContent).toContain('- [x] Share the slides');
  await shot(main, '05-tasks-archived');
  // Undo: back in the list.
  await archived.getByRole('button', { name: 'Undo' }).click();
  await expect(view.locator('.ttext').filter({ hasText: 'Share the slides' })).toBeVisible();
  await expectStore(yap, 'notes.json', (s) => !s.notes[0].archivedTasks?.length && !s.tasks.find((t) => t.text === 'Renew the passport')?.archived);

  // A task you added can be removed.
  const passport = view.locator('.task').filter({ hasText: 'Renew the passport' });
  await passport.hover();
  await passport.getByRole('button', { name: 'Remove “Renew the passport”' }).click();
  await expect(passport).toHaveCount(0);
  await expectStore(yap, 'notes.json', (s) => !s.tasks?.some((t) => t.text === 'Renew the passport'));
});
