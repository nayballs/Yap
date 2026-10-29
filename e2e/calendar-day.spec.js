// The calendar day to day (src-tauri/src/calendar.rs and calendar/): the
// next meeting in the tray (its menu item and tooltip, through the
// debug-only `e2e_tray` hook: a test run's tray is never clicked), folder
// rules for recurring meetings (calendar/filing.rs: the Meetings view's
// "File this meeting in…" and "Always file … in…", the earlier notes when
// asked, the new ones as they're made, a renamed folder taking its rule
// along), a call matched to its meeting by the join code its window shows
// (calendar/codes.rs; the window is the debug-only
// `calendar_e2e_call_window`), and the Meetings view's day: the free time
// between today's meetings, and "Done for today" after the last one.
//
// The calendar is a local iCal feed built at test time, relative to now
// (support/calendar-feed.js). The day view's tests need their meetings on
// today's date, so near midnight they skip (they say why).
import { test as base, expect, openView, expectStore, closeToasts } from './support/fixtures.js';
import { startCalendarFeed, calendarFile, vevent } from './support/calendar-feed.js';

const SECOND = 1000;
const MINUTE = 60 * SECOND;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;
const OWNER = { name: 'Imogen', email: 'imogen@example.com', partstat: 'ACCEPTED' };
const PRIYA = { name: 'Priya Shah', email: 'priya@example.com' };
const SAM = { name: 'Sam Lee', email: 'sam@example.com' };

/** A time `ms` from now, on a whole second (as calendars write them). */
const from = (ms) => new Date(Math.floor((Date.now() + ms) / SECOND) * SECOND);
const now = () => Math.floor(Date.now() / SECOND);
/** Minutes since local midnight, now. */
const minutesToday = () => {
  const d = new Date();
  return d.getHours() * 60 + d.getMinutes();
};

/** Two earlier notes of the weekly sync, and a one-off meeting's. */
function seededNotes() {
  const weeks = (n) => now() - n * 7 * 24 * 3600;
  const note = (id, title, when, key) => ({
    id,
    title,
    noteType: 'meeting',
    folder: 'Meetings',
    source: 'calendar',
    createdTs: when,
    updatedTs: when + 3600,
    participants: ['Priya Shah'],
    content: 'Notes from the call.',
    enhancedContent: '',
    transcript: [],
    event: { key, title, start: when, end: when + 1800 },
  });
  return {
    folders: ['Personal', 'Meetings', 'Design'],
    notes: [
      note(1, 'Weekly sync', weeks(2), `9:weekly@e2e:${weeks(2)}`),
      note(2, 'Weekly sync', weeks(1), `9:weekly@e2e:${weeks(1)}`),
      note(3, 'Vendor call', now() - 3 * 24 * 3600, '9:vendor@e2e'),
    ],
  };
}

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
        name: 'calendar-day',
        // No reminder cards: a simulated call is asked about as it starts.
        config: { meetingReminder: 'never', meetingBrief: false },
        data: { 'notes.json': seededNotes() },
      });
    },
    { scope: 'worker' },
  ],
});

const status = (yap) => yap.invoke('calendar_status');
const recording = (yap) => yap.invoke('meeting_state').then((s) => s.recording);
const notes = (yap) => yap.readJson('notes.json')?.notes ?? [];
const tray = (yap) => yap.invoke('e2e_tray');
const trayItem = (yap) => tray(yap).then((items) => items.find(([id]) => id === 'calendar_next')?.[1] ?? null);
const tooltip = (yap) => tray(yap).then((items) => items.find(([id]) => id === 'tooltip')?.[1] ?? '');
const menu = (main) => main.getByRole('menu').first();
const dialog = (main) => main.getByRole('alertdialog');
const toast = (main, text) => main.getByRole('status').filter({ hasText: text });
const today = (main) => main.getByRole('region', { name: 'Today' });
const pastRow = (main, title) => main.getByRole('listitem').filter({ has: main.getByRole('button', { name: title, exact: true }) });

/** Serve `ics`, connected and synced. */
async function serve(yap, feed, ics) {
  feed.set(ics);
  const s = await status(yap);
  if (!s.connections.length) await yap.invoke('calendar_add_link', { link: feed.url, kind: 'ics' });
  await yap.invoke('calendar_sync');
}

async function pauseAll(yap) {
  if (await recording(yap)) {
    await yap.invoke('meeting_pause');
    await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
  }
}

test.afterEach(async ({ yap, main }) => {
  await pauseAll(yap);
  await yap.invoke('meeting_detect_simulate', { appId: 'meet', active: false });
  await yap.invoke('calendar_e2e_call_window', { appId: 'meet' });
  await yap.notepad?.getByRole('button', { name: 'Close notepad' }).click({ timeout: 2_000 }).catch(() => {});
  await closeToasts(main).catch(() => {});
});

test('the tray names the next meeting with a countdown; a click opens its note, or takes notes once it\'s on', async ({
  yap,
  main,
  feed,
}) => {
  // Nothing connected: no item.
  expect(await trayItem(yap)).toBeNull();

  // A meeting 40 minutes away, and a declined one before it (never named).
  const later = from(40 * MINUTE);
  await serve(
    yap,
    feed,
    calendarFile(OWNER.email, [
      vevent({ uid: 'declined@e2e', summary: 'Vendor demo', start: from(20 * MINUTE), end: from(50 * MINUTE), attendees: [SAM, { ...OWNER, partstat: 'DECLINED' }] }),
      vevent({ uid: 'review@e2e', summary: 'Design review', start: later, end: new Date(later.getTime() + 30 * MINUTE), attendees: [PRIYA, OWNER] }),
    ])
  );
  await expect.poll(() => trayItem(yap)).toMatch(/^Next: Design review in (39|40) min$/);
  await expect.poll(() => tooltip(yap)).toMatch(/\nNext: Design review in (39|40) min$/);

  // Not on yet: a click opens its note, made ahead of the meeting, in Notes.
  await yap.invoke('e2e_tray', { click: 'calendar_next' });
  await expect(main.getByPlaceholder('Untitled Note')).toHaveValue('Design review', { timeout: 10_000 });
  expect(notes(yap).find((n) => n.title === 'Design review')).toMatchObject({ folder: 'Meetings', source: 'calendar' });
  expect(await recording(yap)).toBe(false);

  // A meeting starting in 80 s: within ten minutes, a click takes notes.
  const soon = from(80 * SECOND);
  await serve(
    yap,
    feed,
    calendarFile(OWNER.email, [
      vevent({ uid: 'standup@e2e', summary: 'Stand-up', start: soon, end: new Date(soon.getTime() + 15 * MINUTE), attendees: [SAM, OWNER] }),
      vevent({ uid: 'review@e2e', summary: 'Design review', start: later, end: new Date(later.getTime() + 30 * MINUTE), attendees: [PRIYA, OWNER] }),
    ])
  );
  await expect.poll(() => trayItem(yap)).toBe('Next: Stand-up in 1 min · Take notes');
  // The tooltip says what's next, not what a click does.
  expect(await tooltip(yap)).toMatch(/\nNext: Stand-up in 1 min$/);
  await yap.invoke('e2e_tray', { click: 'calendar_next' });
  await expect.poll(() => recording(yap), { timeout: 15_000 }).toBe(true);
  const standup = notes(yap).find((n) => n.title === 'Stand-up');
  expect(standup).toMatchObject({ noteType: 'meeting', participants: ['Sam Lee'] });
  expect((await yap.invoke('meeting_state')).noteId).toBe(standup.id);
  // Recording it, the tray says so (before it starts, or once it has).
  await expect.poll(() => trayItem(yap)).toMatch(/^(Next: Stand-up in 1 min|Now: Stand-up) · Recording$/);
});

test('folder rules: a recurring meeting\'s notes go to its folder — earlier ones when asked, new ones as they\'re made', async ({
  yap,
  main,
  feed,
  shot,
}) => {
  // The weekly sync's next occurrence tomorrow (the same series as the
  // seeded notes: its UID), and a one-off.
  const tomorrow = from(DAY);
  await serve(
    yap,
    feed,
    calendarFile(OWNER.email, [
      vevent({ uid: 'weekly@e2e', summary: 'Weekly sync', start: tomorrow, end: new Date(tomorrow.getTime() + 30 * MINUTE), rrule: 'FREQ=WEEKLY;COUNT=4', attendees: [PRIYA, OWNER] }),
    ])
  );
  await openView(main, 'Meetings');
  const sync = pastRow(main, 'Weekly sync').first();
  await expect(sync).toBeVisible();

  // A one-off meeting: only "File this meeting in".
  await pastRow(main, 'Vendor call').getByRole('button', { name: 'More for Vendor call' }).click();
  await expect(menu(main).getByRole('menuitem', { name: 'File this meeting in' })).toBeVisible();
  await expect(menu(main).getByRole('menuitem', { name: /^Always file/ })).toHaveCount(0);
  await menu(main).getByRole('menuitem', { name: 'File this meeting in' }).hover();
  const fileIn = main.getByRole('menu', { name: 'File this meeting in' });
  await expect(fileIn.getByRole('menuitemradio', { name: 'Meetings' })).toHaveAttribute('aria-checked', 'true');
  await fileIn.getByRole('menuitemradio', { name: 'Personal' }).click();
  await expectStore(yap, 'notes.json', (s) => s.notes.find((n) => n.id === 3)?.folder === 'Personal');
  expect(notes(yap).find((n) => n.id === 3).folderChosen).toBe(true);
  await expect(toast(main, 'Filed in Personal')).toBeVisible();
  await closeToasts(main);

  // The weekly sync: always in Design.
  await sync.getByRole('button', { name: 'More for Weekly sync' }).click();
  await menu(main).getByRole('menuitem', { name: 'Always file “Weekly sync” in' }).hover();
  const always = main.getByRole('menu', { name: 'Always file “Weekly sync” in' });
  await expect(always.getByRole('menuitemradio')).toHaveText(['Personal', 'Meetings', 'Design']);
  await shot(main, '01-always-file-in');
  await always.getByRole('menuitemradio', { name: 'Design' }).click();
  // This one moves; the earlier one is asked about.
  await expect(dialog(main)).toContainText('Move the earlier “Weekly sync” notes too?');
  await expect(dialog(main)).toContainText("1 earlier note of this meeting isn't in Design");
  await shot(main, '02-move-the-earlier-ones');
  await dialog(main).getByRole('button', { name: 'Move them' }).click();
  await expectStore(yap, 'notes.json', (s) => [1, 2].every((id) => s.notes.find((n) => n.id === id)?.folder === 'Design'));
  expect(notes(yap).find((n) => n.id === 3).folder).toBe('Personal');
  await expectStore(yap, 'meeting_folders.json', (s) => s.rules['weekly@e2e']?.folder === 'Design' && s.rules['weekly@e2e']?.title === 'Weekly sync');

  // The next one, made from the calendar ahead of the meeting: in Design.
  await main.getByRole('region', { name: 'Upcoming' }).getByRole('listitem').filter({ hasText: 'Weekly sync' }).first()
    .getByRole('button', { name: /Weekly sync/ }).first().click();
  await expect(main.getByPlaceholder('Untitled Note')).toHaveValue('Weekly sync', { timeout: 10_000 });
  const next = notes(yap).find((n) => n.source === 'calendar' && n.event?.key?.startsWith('1:weekly@e2e:'));
  expect(next).toMatchObject({ folder: 'Design', title: 'Weekly sync' });

  // A folder renamed takes its rule along…
  await yap.invoke('notes_folder_rename', { from: 'Design', to: 'Product design' });
  await expectStore(yap, 'meeting_folders.json', (s) => s.rules['weekly@e2e']?.folder === 'Product design');

  // …and the menu stops it: the notes stay where they are.
  await openView(main, 'Meetings');
  await pastRow(main, 'Weekly sync').first().getByRole('button', { name: 'More for Weekly sync' }).click();
  await expect(menu(main).getByRole('menuitem', { name: 'Stop filing it in Product design' })).toBeVisible();
  await shot(main, '03-stop-filing');
  await menu(main).getByRole('menuitem', { name: 'Stop filing it in Product design' }).click();
  await expectStore(yap, 'meeting_folders.json', (s) => Object.keys(s.rules).length === 0);
  expect(notes(yap).filter((n) => n.title === 'Weekly sync').every((n) => n.folder === 'Product design')).toBe(true);
});

test('a call is matched to its meeting by the join code its window shows, over the one starting nearest', async ({ yap, main, feed }) => {
  // Two Meet meetings on now: the interview starts nearest, but the call's
  // tab shows the planning meeting's code.
  await serve(
    yap,
    feed,
    calendarFile(OWNER.email, [
      vevent({ uid: 'planning@e2e', summary: 'Planning', start: from(-5 * MINUTE), end: from(25 * MINUTE), attendees: [PRIYA, OWNER], location: 'https://meet.google.com/abc-defg-hij' }),
      vevent({ uid: 'interview@e2e', summary: 'Interview', start: from(MINUTE), end: from(31 * MINUTE), attendees: [SAM, OWNER], location: 'https://meet.google.com/xyz-wvut-srq' }),
    ])
  );
  await yap.invoke('calendar_e2e_call_window', { appId: 'meet', title: 'Meet - abc-defg-hij - Google Chrome' });
  await yap.invoke('meeting_detect_simulate', { appId: 'meet', active: true });
  const prompt = toast(main, 'Google Meet call detected');
  await expect(prompt).toBeVisible({ timeout: 10_000 });
  await prompt.getByRole('button', { name: 'Record notes' }).click();
  await expect.poll(() => recording(yap), { timeout: 15_000 }).toBe(true);
  const id = (await yap.invoke('meeting_state')).noteId;
  await expectStore(yap, 'notes.json', (s) => s.notes.find((n) => n.id === id)?.event?.key === '1:planning@e2e');
  expect(notes(yap).find((n) => n.id === id)).toMatchObject({ title: 'Planning', participants: ['Priya Shah'] });
});

test('the day view: the free time between today\'s meetings, busy time left out', async ({ yap, main, feed, shot }) => {
  test.skip(minutesToday() > 21 * 60, "today's meetings would run past midnight");
  // 30 min, then 45 min apart with 15 min of focus time in it, then 10 min.
  const at = (m) => from(m * MINUTE);
  await serve(
    yap,
    feed,
    calendarFile(OWNER.email, [
      vevent({ uid: 'gap-a@e2e', summary: 'Roadmap', start: at(20), end: at(50), attendees: [PRIYA, OWNER] }),
      vevent({ uid: 'gap-focus@e2e', summary: 'Focus', start: at(60), end: at(75) }),
      vevent({ uid: 'gap-b@e2e', summary: 'Hiring sync', start: at(95), end: at(125), attendees: [SAM, OWNER] }),
      vevent({ uid: 'gap-c@e2e', summary: 'Budget', start: at(135), end: at(165), attendees: [PRIYA, OWNER] }),
    ])
  );
  await openView(main, 'Meetings');
  const day = today(main);
  await expect(day.getByRole('listitem').filter({ hasText: 'Roadmap' })).toBeVisible();
  // Between Roadmap and Hiring sync: 45 minutes less the focus time. Ten
  // minutes before Budget is too little to say.
  const gaps = day.getByRole('separator');
  await expect(gaps).toHaveCount(1);
  await expect(gaps).toHaveAccessibleName('30 min free');
  await expect(gaps).toHaveText('30 min free');
  await shot(main, '04-free-between-meetings');
});

test('"Done for today": today\'s meetings, their open tasks and the ones without notes', async ({ yap, main, feed, shot }) => {
  test.skip(minutesToday() < 110, "today's meetings would start before midnight");
  await serve(
    yap,
    feed,
    calendarFile(OWNER.email, [
      vevent({ uid: 'wrap-a@e2e', summary: 'Pricing review', start: from(-100 * MINUTE), end: from(-70 * MINUTE), attendees: [PRIYA, OWNER] }),
      vevent({ uid: 'wrap-b@e2e', summary: 'Supplier call', start: from(-50 * MINUTE), end: from(-20 * MINUTE), attendees: [SAM, OWNER] }),
    ])
  );
  // The pricing review's note, with an action plan.
  const d = await yap.invoke('calendar_day');
  expect(d.meetings.map((m) => m.title)).toEqual(['Pricing review', 'Supplier call']);
  expect(d.pending).toBe(0);
  const pricing = await yap.invoke('calendar_event', { key: d.meetings[0].key, action: 'open' });
  await yap.invoke('note_summary_update', {
    id: pricing,
    text: '## Action plan\n### You\n- [ ] Send Priya the revised pricing table (due: Monday)\n- [x] Book the room\n### Priya Shah\n- [ ] Draft the pricing FAQ',
    origin: 'e2e',
  });

  await openView(main, 'Meetings');
  const done = main.getByRole('region', { name: 'Done for today' });
  await expect(done).toBeVisible();
  await expect(done).toContainText('2 meetings · 2 open tasks · 1 without notes');
  const tasks = done.getByRole('list', { name: 'Open tasks from Pricing review' });
  await expect(tasks.getByRole('checkbox')).toHaveCount(2);
  await expect(tasks).toContainText('Send Priya the revised pricing table · Monday');
  await expect(tasks).toContainText('Draft the pricing FAQ · Priya Shah');
  await expect(done.getByRole('listitem').filter({ hasText: 'Supplier call' })).toContainText('No notes');
  await shot(main, '05-done-for-today');

  // A task ticked here is ticked in the note's summary.
  await tasks.getByRole('checkbox').first().check();
  await expect(tasks.getByRole('checkbox')).toHaveCount(1);
  await expectStore(yap, 'notes.json', (s) =>
    s.notes.find((n) => n.id === pricing)?.enhancedContent.includes('- [x] Send Priya the revised pricing table')
  );
  await expect(done).toContainText('2 meetings · 1 open task · 1 without notes');

  // The meeting without notes: Add notes opens one for it.
  await done.getByRole('button', { name: 'Add notes' }).click();
  await expect(main.getByPlaceholder('Untitled Note')).toHaveValue('Supplier call', { timeout: 10_000 });
});
