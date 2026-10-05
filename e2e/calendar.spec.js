// The calendar (src-tauri/src/calendar.rs): Settings → Connectors, the
// Meetings view, the reminder card before a meeting, notes that take the
// meeting's name and attendees, call detection not asking twice, switching
// notes between back-to-back meetings, and Google Calendar in one click.
//
// Nothing here touches a real calendar or account. The "private iCal link"
// is a local server serving a feed built at test time, relative to now
// (support/calendar-feed.js); Google is a local fake of its OAuth endpoints
// and Calendar API (support/fake-google.js), reached through the debug-only
// YAP_GOOGLE_*_URL variables with a fake client. Test mode opens no browser:
// the links Yap would open are kept for `calendar_e2e_opened`. Secrets go to
// a file in the instance's own data folder instead of Credential Manager.
// Calls are simulated (`meeting_detect_simulate`), and recordings are silent.
import fs from 'node:fs';
import path from 'node:path';
import {
  test as base,
  expect,
  openView,
  openSettings,
  closeSettings,
  closeToasts,
  expectStore,
  settingsDialog,
} from './support/fixtures.js';
import { startCalendarFeed, calendarFile, vevent } from './support/calendar-feed.js';
import { startFakeGoogle } from './support/fake-google.js';

const CLIENT_ID = 'e2e-client.apps.googleusercontent.com';
const OWNER = { name: 'Nathan', email: 'nathan@example.com' };
const TANAY = { name: 'Tanay Kothari', email: 'tanay@example.com' };
const PRIYA = { name: 'Priya Shah', email: 'priya@example.com', partstat: 'NEEDS-ACTION' };
const SAM = { name: 'Sam Lee', email: 'sam@example.com' };
const TEAMS_LINK = 'https://teams.microsoft.com/l/meetup-join/19%3ameeting_e2e%40thread.v2/0?context=%7b%7d';
const TEAMS_NEXT = 'https://teams.microsoft.com/l/meetup-join/19%3ameeting_next%40thread.v2/0';
const ZOOM_LINK = 'https://us02web.zoom.us/j/81234567890?pwd=e2e';
const SECOND = 1000;
const MINUTE = 60 * SECOND;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/** A time `ms` from now, on the minute grid's second 0 (as calendars write them). */
const from = (ms) => new Date(Math.floor((Date.now() + ms) / SECOND) * SECOND);

/** The feed most tests start from: one of each kind of event Yap must show or hide. */
function baseFeed() {
  const me = { ...OWNER, partstat: 'ACCEPTED' };
  const tomorrow = from(DAY);
  return calendarFile(OWNER.email, [
    // About to start: two guests, a Teams link in the description.
    vevent({
      uid: 'design-review@e2e',
      summary: 'Design review',
      start: from(80 * SECOND),
      end: from(80 * SECOND + 30 * MINUTE),
      organizer: TANAY,
      attendees: [TANAY, PRIYA, me],
      description: `Agenda: the Q4 launch checklist\n\nJoin: ${TEAMS_LINK}\nMeeting ID: 312 456 789 012`,
    }),
    // Every day from tomorrow for five days, one taken out (EXDATE): four.
    vevent({
      uid: 'standup@e2e',
      summary: 'Daily stand-up',
      start: tomorrow,
      end: new Date(tomorrow.getTime() + 15 * MINUTE),
      rrule: 'FREQ=DAILY;COUNT=5',
      exdates: [new Date(tomorrow.getTime() + 2 * DAY)],
      attendees: [SAM, me],
      location: ZOOM_LINK,
    }),
    // You said maybe, and it overlaps "1:1 with Sam": Maybe + Conflict.
    vevent({
      uid: 'budget@e2e',
      summary: 'Budget check-in',
      start: from(3 * HOUR),
      end: from(3 * HOUR + 45 * MINUTE),
      attendees: [PRIYA, { ...OWNER, partstat: 'TENTATIVE' }],
    }),
    vevent({
      uid: 'one-on-one@e2e',
      summary: '1:1 with Sam',
      start: from(3 * HOUR + 30 * MINUTE),
      end: from(4 * HOUR),
      attendees: [SAM, me],
    }),
    // Not meetings: all day, just you, over 6 hours, declined.
    vevent({ uid: 'offsite@e2e', summary: 'Company offsite', start: tomorrow, end: new Date(tomorrow.getTime() + DAY), allDay: true, attendees: [TANAY, me] }),
    vevent({ uid: 'focus@e2e', summary: 'Focus time', start: from(2 * HOUR), end: from(3 * HOUR) }),
    vevent({ uid: 'workshop@e2e', summary: 'Quarterly planning workshop', start: from(DAY + 2 * HOUR), end: from(DAY + 10 * HOUR), attendees: [TANAY, me] }),
    vevent({ uid: 'vendor@e2e', summary: 'Vendor demo', start: from(5 * HOUR), end: from(6 * HOUR), attendees: [TANAY, { ...OWNER, partstat: 'DECLINED' }] }),
  ]);
}

/** Google's side of the one-click connect: one meeting and four things that aren't. */
function googleEvents() {
  const t0 = Date.now();
  const iso = (ms) => new Date(t0 + ms).toISOString();
  const me = { email: 'tester@example.com', self: true, responseStatus: 'accepted' };
  return [
    {
      id: 'roadmap_e2e',
      status: 'confirmed',
      summary: 'Roadmap review',
      start: { dateTime: iso(2 * HOUR) },
      end: { dateTime: iso(2 * HOUR + 30 * MINUTE) },
      attendees: [me, { email: 'alex.chen@example.com', displayName: 'Alex Chen', responseStatus: 'accepted' }],
      hangoutLink: 'https://meet.google.com/abc-defg-hij',
    },
    { id: 'ooo_e2e', status: 'confirmed', eventType: 'outOfOffice', summary: 'Out of office', start: { dateTime: iso(DAY) }, end: { dateTime: iso(DAY + HOUR) } },
    { id: 'holiday_e2e', status: 'confirmed', summary: 'Holiday', start: { date: new Date(t0 + DAY).toISOString().slice(0, 10) }, end: { date: new Date(t0 + 2 * DAY).toISOString().slice(0, 10) } },
    { id: 'gym_e2e', status: 'confirmed', summary: 'Gym', start: { dateTime: iso(3 * HOUR) }, end: { dateTime: iso(4 * HOUR) } },
    {
      id: 'declined_e2e',
      status: 'confirmed',
      summary: 'Declined sync',
      start: { dateTime: iso(5 * HOUR) },
      end: { dateTime: iso(6 * HOUR) },
      attendees: [{ ...me, responseStatus: 'declined' }, { email: 'alex.chen@example.com' }],
    },
  ];
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
  google: [
    async ({}, use) => {
      const google = await startFakeGoogle({ clientId: CLIENT_ID, clientSecret: 'e2e-not-secret', events: googleEvents });
      await use(google);
      await google.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({ google }, use) => {
      // No reminders until a test turns them on (Settings → General).
      await use({ name: 'calendar', config: { meetingReminder: 'never' }, env: google.env });
    },
    { scope: 'worker' },
  ],
});

const status = (yap) => yap.invoke('calendar_status');
const opened = (yap) => yap.invoke('calendar_e2e_opened');
const meetingState = (yap) => yap.invoke('meeting_state');
const recording = (yap) => meetingState(yap).then((s) => s.recording);
const notes = (yap) => yap.readJson('notes.json')?.notes ?? [];
const simulate = (yap, appId, active) => yap.invoke('meeting_detect_simulate', { appId, active });
const toast = (main, text) => main.getByRole('status').filter({ hasText: text });
/** The reminder card for `title` (it always offers to snooze). */
const cardFor = (main, title) => toast(main, title).filter({ hasText: 'Snooze 2 min' });
/** The same card on the Yap bar (a call card: its answers in a ^ menu). */
const barCard = (yap, title) => yap.overlay.getByRole('status').filter({ hasText: title });
const barCards = (yap) => yap.invoke('bar_status').then((s) => s.cards.map((c) => c.id));
/** The pretend cursor onto the bar's calendar card (the window takes clicks), or away. */
const pointer = (yap, target) => yap.invoke('bar_simulate', { pointer: target });
const visible = (yap, label) => yap.invoke('plugin:window|is_visible', { label });
/** A meeting row in the Meetings view. */
const row = (main, title) => main.getByRole('listitem').filter({ hasText: title });
const raw = (yap, file) => fs.readFileSync(path.join(yap.dataDir, file), 'utf8');

/** "Notify before scheduled meetings start", saved straight to config. */
async function setReminder(yap, value) {
  const cfg = await yap.invoke('get_config');
  await yap.invoke('save_config', { cfg: { ...cfg, meetingReminder: value } });
}

/** The iCal feed connected (through the command the Connectors form uses). */
async function ensureFeed(yap, feed) {
  const s = await status(yap);
  if (!s.connections.some((c) => c.kind === 'ics')) {
    await yap.invoke('calendar_add_link', { link: feed.url, kind: 'ics' });
  }
}

/** Serve `ics` and sync it in. */
async function serve(yap, feed, ics) {
  feed.set(ics);
  await ensureFeed(yap, feed);
  await yap.invoke('calendar_sync');
}

/** Stop whatever records, without a summary. */
async function pauseAll(yap) {
  if (await recording(yap)) {
    await yap.invoke('meeting_pause');
    await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
  }
}

test.afterEach(async ({ yap }) => {
  await pauseAll(yap);
  for (const app of ['teams', 'zoom', 'webex']) await simulate(yap, app, false);
  await setReminder(yap, 'never');
  await pointer(yap, 'away').catch(() => {});
});

test('Settings → Connectors: what to connect, a link that was reset, and one that works', async ({ yap, main, feed, shot }) => {
  const dialog = await openSettings(main, 'Connectors');
  await expect(dialog.getByRole('heading', { level: 1, name: 'Connectors' })).toBeVisible();
  for (const name of ['Google Calendar', 'Outlook Calendar', 'Other calendar']) {
    await expect(dialog.getByRole('group', { name })).toBeVisible();
  }
  await expect(dialog.getByText('Reminders for Google meetings before they begin')).toBeVisible();
  await expect(dialog.getByText('Give your AI access to your meeting transcripts and notes')).toBeVisible();
  await expect(dialog.getByRole('button', { name: 'Go to MCP' })).toBeVisible();
  await shot(main, '01-connectors');

  // Outlook: no Microsoft app, so a guided ICS link.
  const outlook = dialog.getByRole('group', { name: 'Outlook Calendar' });
  await outlook.getByRole('button', { name: 'Connect' }).click();
  await expect(outlook.getByText('Shared calendars')).toBeVisible();
  await expect(outlook.getByRole('textbox', { name: 'Outlook calendar ICS link' })).toBeVisible();
  await shot(main, '02-outlook-guide');
  await outlook.getByRole('button', { name: 'Cancel' }).click();

  // Any calendar by its private iCal link: a reset one says so in the form…
  const other = dialog.getByRole('group', { name: 'Other calendar' });
  await other.getByRole('button', { name: 'Connect' }).click();
  const input = other.getByRole('textbox', { name: 'Calendar iCal link' });
  await input.fill(feed.missing);
  await other.getByRole('button', { name: 'Add calendar' }).click();
  await expect(other.getByRole('alert')).toContainText("doesn't work any more");
  await shot(main, '03-link-reset');

  // …and the right one connects, named after the calendar.
  feed.set(baseFeed());
  await input.fill(feed.url);
  await other.getByRole('button', { name: 'Add calendar' }).click();
  await expect(other.getByText('nathan@example.com')).toBeVisible();
  await expect(other).toContainText('127.0.0.1 · Synced just now');
  await shot(main, '04-link-connected');

  // The link is a secret: Credential Manager (a file in test runs), never
  // config.json or calendar.json.
  expect(raw(yap, 'config.json')).not.toContain('private-');
  expect(raw(yap, 'calendar.json')).not.toContain('private-');
  expect(Object.values(yap.readJson('calendar-secrets.e2e.json'))).toContain(feed.url);
  await closeSettings(main);
});

test('the Meetings view: today and the next 7 days, Conflict and Maybe, and nothing that isn\'t a meeting', async ({
  yap,
  main,
  feed,
  shot,
}) => {
  await serve(yap, feed, baseFeed());
  await openView(main, 'Meetings');
  await expect(main.getByRole('heading', { level: 1, name: 'Meetings' })).toBeVisible();

  const design = row(main, 'Design review');
  await expect(design).toContainText('Teams');
  await expect(design).toContainText('Tanay Kothari, Priya Shah');
  // From 10 minutes before: join and take notes in one go.
  await expect(design.getByRole('button', { name: 'Join + Start' })).toBeVisible();
  await expect(design.getByRole('button', { name: 'Start' , exact: true })).toBeVisible();
  await expect(design.getByRole('button', { name: 'Join meeting' })).toBeVisible();
  await expect(row(main, 'Budget check-in')).toContainText('Maybe');
  await expect(row(main, 'Budget check-in')).toContainText('Conflict');
  await expect(row(main, '1:1 with Sam')).toContainText('Conflict');
  for (const hidden of ['Company offsite', 'Focus time', 'Quarterly planning workshop', 'Vendor demo']) {
    await expect(row(main, hidden)).toHaveCount(0);
  }
  await shot(main, '05-meetings');

  // The next 7 days, three at a time: four stand-ups (five days, one off).
  const upcoming = main.getByRole('region', { name: 'Upcoming' });
  await expect(upcoming.getByRole('listitem')).toHaveCount(3);
  while (await upcoming.getByRole('button', { name: 'Show more' }).isVisible()) {
    await upcoming.getByRole('button', { name: 'Show more' }).click();
  }
  await expect(upcoming.getByRole('listitem').filter({ hasText: 'Daily stand-up' })).toHaveCount(4);
  await upcoming.scrollIntoViewIfNeeded();
  await shot(main, '06-meetings-show-more');

  const titles = (await status(yap)).events.map((e) => e.title);
  expect(titles).toEqual(expect.arrayContaining(['Design review', 'Budget check-in', '1:1 with Sam', 'Daily stand-up']));
  expect(titles).not.toEqual(expect.arrayContaining(['Focus time']));

  // A meeting still to come opens a note made ahead of time: its title,
  // attendees and date, in Meetings.
  await row(main, '1:1 with Sam').getByRole('button', { name: /1:1 with Sam/ }).click();
  await expect(main.getByPlaceholder('Untitled Note')).toHaveValue('1:1 with Sam', { timeout: 10_000 });
  const draft = notes(yap).find((n) => n.title === '1:1 with Sam');
  expect(draft).toMatchObject({ folder: 'Meetings', noteType: 'meeting', source: 'calendar', participants: ['Sam Lee'] });
  expect(draft.createdTs).toBe(draft.event.start); // dated to the meeting
  await shot(main, '07-draft-note');
  // Opened again, it's the same note.
  await openView(main, 'Meetings');
  await expect(row(main, '1:1 with Sam')).toContainText('Note ready');
  await row(main, '1:1 with Sam').getByRole('button', { name: /1:1 with Sam/ }).click();
  await expect(main.getByPlaceholder('Untitled Note')).toHaveValue('1:1 with Sam');
  expect(notes(yap).filter((n) => n.title === '1:1 with Sam')).toHaveLength(1);
});

test('the reminder card: Join & take notes records into a note with the meeting\'s name and attendees, and its call isn\'t asked about again', async ({
  yap,
  main,
  feed,
  shot,
}) => {
  await serve(yap, feed, baseFeed()); // "Design review" starts in 80 s
  const dialog = await openSettings(main, 'General');
  const notify = dialog.getByRole('combobox', { name: 'Notify before scheduled meetings start' });
  await notify.scrollIntoViewIfNeeded();
  await expect(notify).toHaveValue('never');
  await notify.selectOption('2min');
  await expectStore(yap, 'config.json', (c) => c.meetingReminder === '2min');
  await shot(main, '08-settings-reminder');
  await closeSettings(main);

  const card = cardFor(main, 'Design review');
  await expect(card).toBeVisible();
  await expect(card).toContainText(/In \d min|Starting now/);
  await expect(card).toContainText('Teams · with Tanay Kothari, Priya Shah');
  await expect(card.getByRole('button', { name: 'Join & take notes' })).toBeVisible();
  await expect(card.getByRole('button', { name: 'Start notes' })).toBeVisible();
  await expect(card.getByRole('button', { name: 'Snooze 2 min' })).toBeVisible();
  await shot(main, '09-reminder-card');

  // The main window never counts as focused in a test run, so the card is
  // on the Yap bar too: call detection's card, with Teams' mark, the
  // meeting over "● In 1 min · with Tanay +1", Join & take notes.
  const onBar = barCard(yap, 'Design review');
  await expect(onBar).toBeVisible();
  await expect(onBar).toContainText(/In \d min|Starting now/);
  await expect(onBar).toContainText('with Tanay +1');
  await expect(onBar.locator('.dot.soon')).toBeVisible();
  await expect(onBar.getByRole('button', { name: 'Join & take notes' })).toBeVisible();
  await expect(onBar.getByRole('button', { name: 'Close' })).toBeVisible();
  await yap.overlay.waitForTimeout(300); // the card's entrance
  await shot(yap.overlay, '10-reminder-on-the-bar');

  // Teams starts while the card is up: no second "call detected" prompt.
  await simulate(yap, 'teams', true);
  await main.waitForTimeout(1_500);
  await expect(toast(main, 'Teams call detected')).toHaveCount(0);
  expect(await barCards(yap)).toEqual(['calendar']);
  await expect(card).toBeVisible();

  // Answered in the window, it leaves the bar too.
  await card.getByRole('button', { name: 'Join & take notes' }).click();
  await expect.poll(() => recording(yap), { timeout: 15_000 }).toBe(true);
  await expect(onBar).toHaveCount(0);
  await expect.poll(() => barCards(yap)).not.toContain('calendar');
  // The link opened (in a test run, only noted down)…
  expect(await opened(yap)).toContain(TEAMS_LINK);
  // …and the meeting's note records, with its name, attendees and agenda.
  const note = notes(yap).find((n) => n.title === 'Design review' && n.source === 'calendar');
  expect(note).toMatchObject({ folder: 'Meetings', noteType: 'meeting', participants: ['Tanay Kothari', 'Priya Shah'] });
  expect(note.event).toMatchObject({ title: 'Design review', service: 'teams' });
  expect(note.event.description).toBe('Agenda: the Q4 launch checklist');
  expect((await meetingState(yap)).noteId).toBe(note.id);
  await expect(card).toHaveCount(0);

  // The notepad opened on it.
  await expect.poll(() => visible(yap, 'notepad')).toBe(true);
  await expect(yap.notepad.getByRole('textbox', { name: 'Meeting title' })).toHaveValue('Design review');
  await shot(yap.notepad, '11-notepad-from-the-card');

  // The Meetings view shows it recording.
  await openView(main, 'Meetings');
  await expect(row(main, 'Design review')).toContainText('Recording');
  await shot(main, '12-meetings-recording');

  // Its call ending asks to stop and summarise, as for any recorded call.
  await simulate(yap, 'teams', false);
  const ended = toast(main, 'Teams call ended');
  await expect(ended).toBeVisible();
  await ended.getByRole('button', { name: 'Keep recording' }).click();
  await yap.notepad.getByRole('button', { name: 'Close notepad' }).click();
});

test('Esc dismisses a card and call detection stays quiet about that meeting; a snoozed card comes back with its call', async ({
  yap,
  main,
  feed,
  shot,
}) => {
  const me = { ...OWNER, partstat: 'ACCEPTED' };
  await serve(
    yap,
    feed,
    calendarFile(OWNER.email, [
      vevent({ uid: 'pricing@e2e', summary: 'Pricing sync', start: from(70 * SECOND), end: from(30 * MINUTE), attendees: [SAM, me], location: ZOOM_LINK }),
      vevent({ uid: 'roadmap@e2e', summary: 'Roadmap chat', start: from(100 * SECOND), end: from(40 * MINUTE), attendees: [PRIYA, me] }),
    ])
  );
  await setReminder(yap, '2min');

  const pricing = cardFor(main, 'Pricing sync');
  await expect(pricing).toBeVisible();
  await expect(barCard(yap, 'Pricing sync')).toBeVisible();
  await main.keyboard.press('Escape');
  await expect(pricing).toHaveCount(0);
  await expect(barCard(yap, 'Pricing sync')).toHaveCount(0);
  await expectStore(yap, 'calendar.json', (c) => Object.keys(c.answered ?? {}).some((k) => k.includes('pricing@e2e')));
  // Its Zoom call starting isn't asked about: the card was the question.
  await simulate(yap, 'zoom', true);
  await main.waitForTimeout(1_500);
  await expect(toast(main, 'Zoom call detected')).toHaveCount(0);
  await simulate(yap, 'zoom', false);

  // Next in line: a meeting with no link only offers notes.
  const roadmap = cardFor(main, 'Roadmap chat');
  await expect(roadmap).toBeVisible();
  await expect(roadmap.getByRole('button', { name: 'Join & take notes' })).toHaveCount(0);
  await expect(roadmap.getByRole('button', { name: 'Start notes' })).toBeVisible();
  await shot(main, '13-card-without-a-link');

  // On the bar: a calendar for a mark (no call app), Start notes, and the
  // other answers in its ^ menu. Snoozed there, it leaves the window too.
  const onBar = barCard(yap, 'Roadmap chat');
  await expect(onBar).toBeVisible();
  await expect(onBar.getByRole('button', { name: 'Start notes' })).toBeVisible();
  await pointer(yap, 'card:calendar');
  await expect.poll(() => yap.invoke('bar_debug').then((d) => d.interactive)).toBe(true);
  await onBar.getByRole('button', { name: 'More answers' }).click();
  await expect(onBar.getByRole('menuitem', { name: 'Snooze 2 min' })).toBeVisible();
  await shot(yap.overlay, '14-bar-card-menu');
  await onBar.getByRole('menuitem', { name: 'Snooze 2 min' }).click();
  await expect(onBar).toHaveCount(0);
  await expect(roadmap).toHaveCount(0);
  expect((await status(yap)).card).toBeNull();
  await pointer(yap, 'away');

  // A call starting during it brings the snoozed card back, instead of a
  // second question; the bar's ✕ dismisses it everywhere.
  await simulate(yap, 'webex', true);
  await expect(cardFor(main, 'Roadmap chat')).toBeVisible();
  await main.waitForTimeout(1_000);
  await expect(toast(main, 'Webex call detected')).toHaveCount(0);
  await expect(onBar).toBeVisible();
  await onBar.getByRole('button', { name: 'Close' }).click();
  await expect(onBar).toHaveCount(0);
  await expect(cardFor(main, 'Roadmap chat')).toHaveCount(0);
  await expectStore(yap, 'calendar.json', (c) => Object.keys(c.answered ?? {}).some((k) => k.includes('roadmap@e2e')));
});

test('back-to-back meetings: a call during the next one offers to switch notes', async ({ yap, main, feed, shot }) => {
  const me = { ...OWNER, partstat: 'ACCEPTED' };
  await serve(
    yap,
    feed,
    calendarFile(OWNER.email, [
      vevent({ uid: 'standup-now@e2e', summary: 'Team standup', start: from(-20 * MINUTE), end: from(40 * SECOND), attendees: [SAM, me], description: `Join: ${TEAMS_LINK}` }),
      vevent({ uid: 'planning@e2e', summary: 'Sprint planning', start: from(60 * SECOND), end: from(30 * MINUTE), attendees: [PRIYA, me], description: `Join: ${TEAMS_NEXT}` }),
    ])
  );
  // Reminders off: the call alone offers the switch.
  await openView(main, 'Meetings');
  await row(main, 'Team standup').getByRole('button', { name: 'Join + Start' }).click();
  await expect.poll(() => recording(yap), { timeout: 15_000 }).toBe(true);
  const standup = notes(yap).find((n) => n.title === 'Team standup');
  expect(standup.participants).toEqual(['Sam Lee']);
  expect(await opened(yap)).toContain(TEAMS_LINK);

  // The next meeting's call starts while the standup's notes record.
  await simulate(yap, 'teams', true);
  const next = cardFor(main, 'Sprint planning');
  await expect(next).toBeVisible();
  await expect(next).toContainText('Switch your notes to it?');
  await expect(next.getByRole('button', { name: 'Join & switch notes' })).toBeVisible();
  await shot(main, '15-switch-card');
  // The bar's copy says it's the next meeting.
  const nextOnBar = barCard(yap, 'Sprint planning');
  await expect(nextOnBar).toContainText('Next meeting');
  await expect(nextOnBar.getByRole('button', { name: 'Join & switch notes' })).toBeVisible();
  await yap.overlay.waitForTimeout(300);
  await shot(yap.overlay, '16-switch-card-on-the-bar');
  await next.getByRole('button', { name: 'Switch notes', exact: true }).click();
  await expect(nextOnBar).toHaveCount(0);

  // The standup ends (Rust writes its action plan) and the planning's notes record.
  await expect
    .poll(async () => {
      const s = await meetingState(yap);
      return s.recording && s.noteId !== standup.id;
    }, { timeout: 30_000 })
    .toBe(true);
  const planning = notes(yap).find((n) => n.title === 'Sprint planning');
  expect((await meetingState(yap)).noteId).toBe(planning.id);
  expect(planning.participants).toEqual(['Priya Shah']);
  expect(notes(yap).find((n) => n.id === standup.id)).toBeTruthy();
  await expect(yap.notepad.getByRole('textbox', { name: 'Meeting title' })).toHaveValue('Sprint planning');
  await shot(yap.notepad, '17-notepad-after-the-switch');
  await closeToasts(main);
  await yap.notepad.getByRole('button', { name: 'Close notepad' }).click();
});

test('Google Calendar in one click (a local fake of Google), read-only, then disconnecting clears it', async ({
  yap,
  main,
  feed,
  google,
  shot,
}) => {
  const dialog = await openSettings(main, 'Connectors');
  const card = dialog.getByRole('group', { name: 'Google Calendar' });

  // Cancelled on Google's consent page: it says so, nothing connects.
  await card.getByRole('button', { name: 'Connect' }).click();
  await expect(card.getByText('Finish in your browser…')).toBeVisible();
  await shot(main, '18-google-waiting');
  const denied = await fetch(google.deny((await opened(yap)).at(-1)));
  expect(await denied.text()).toContain('Not connected');
  await expect(toast(main, "Couldn't connect your calendar")).toContainText("You didn't let Yap see your calendar");
  await expect(card.getByRole('button', { name: 'Connect' })).toBeVisible();
  await closeToasts(main);

  // Allowed: the browser comes back to Yap's loopback listener, which swaps
  // the code (with its PKCE verifier) for tokens and reads the calendars.
  await card.getByRole('button', { name: 'Connect' }).click();
  const authUrl = (await opened(yap)).at(-1);
  const auth = new URL(authUrl).searchParams;
  expect(authUrl.startsWith(google.env.YAP_GOOGLE_AUTH_URL)).toBe(true);
  expect(auth.get('client_id')).toBe(CLIENT_ID);
  expect(auth.get('scope').split(' ')).toEqual([
    'https://www.googleapis.com/auth/calendar.calendarlist.readonly',
    'https://www.googleapis.com/auth/calendar.events.owned.readonly',
  ]);
  expect(auth.get('code_challenge_method')).toBe('S256');
  expect(auth.get('redirect_uri')).toMatch(/^http:\/\/127\.0\.0\.1:\d+$/);
  expect(auth.get('access_type')).toBe('offline');
  const back = await fetch(google.approve(authUrl));
  expect(back.status).toBe(200);
  expect(await back.text()).toContain('Calendar connected');
  await expect(card.getByText('tester@example.com')).toBeVisible();
  await expect(card).toContainText(/Google Calendar · Synced/);
  await shot(main, '19-google-connected');

  // Its meeting is in; Google's out-of-office, all-day, solo and declined
  // entries aren't.
  await expect.poll(() => status(yap).then((s) => s.events.map((e) => e.title))).toContain('Roadmap review');
  const titles = (await status(yap)).events.map((e) => e.title);
  for (const hidden of ['Out of office', 'Holiday', 'Gym', 'Declined sync']) expect(titles).not.toContain(hidden);
  // Tokens stay out of Yap's files.
  expect(raw(yap, 'calendar.json')).not.toContain('e2e-refresh');
  expect(raw(yap, 'config.json')).not.toContain('e2e-refresh');
  expect(raw(yap, 'calendar-secrets.e2e.json')).toContain('e2e-refresh');
  await closeSettings(main);
  await openView(main, 'Meetings');
  await expect(row(main, 'Roadmap review')).toContainText('Google Meet');
  await expect(row(main, 'Roadmap review')).toContainText('Alex Chen');
  await shot(main, '20-meetings-with-google');

  // Disconnect, from the ⋯ menu: Google's access is revoked, its meetings go.
  await ensureFeed(yap, feed);
  const settings = await openSettings(main, 'Connectors');
  const google_ = settings.getByRole('group', { name: 'Google Calendar' });
  await google_.getByRole('button', { name: 'More options for tester@example.com' }).click();
  await shot(main, '21-connection-menu');
  await google_.getByRole('menuitem', { name: 'Disconnect' }).click();
  await expect(google_.getByText('tester@example.com')).toHaveCount(0);
  expect(google.revoked).toHaveLength(1);
  expect((await status(yap)).events.map((e) => e.title)).not.toContain('Roadmap review');

  // The link too: nothing left, and the secrets are gone.
  const other = settings.getByRole('group', { name: 'Other calendar' });
  await other.getByRole('button', { name: 'More options for nathan@example.com' }).click();
  await other.getByRole('menuitem', { name: 'Disconnect' }).click();
  await expect.poll(() => status(yap).then((s) => s.connections.length)).toBe(0);
  expect((await status(yap)).events).toEqual([]);
  expect(Object.keys(yap.readJson('calendar-secrets.e2e.json'))).toEqual([]);
  await shot(main, '22-connectors-disconnected');
  await closeSettings(main);
  await openView(main, 'Meetings');
  await expect(main.getByText('No meetings found')).toBeVisible();
  await expect(settingsDialog(main)).toBeHidden();
});
