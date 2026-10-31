// Folder rules by company (src-tauri/src/calendar/filing/companies.rs):
// "Meetings with people from example.com go in Clients", from the calendar
// invite's attendees' email domains — the person's own company and free
// mail left out. A meeting with people from another company filed by hand
// in a folder gives the offer ("File every meeting with example.com in
// Clients? (2 past ones too)": Yes / Just this one / Never for
// example.com, notices/companyFolders.js), at most one a day; Yes keeps the
// rule and files the past meetings there (never one the person moved
// themselves), with Undo; a new meeting with the company is made in the
// folder; Settings → Meetings → Folders by company lists the rule, with
// Remove.
//
// The calendar is a local iCal feed built at test time (support/
// calendar-feed.js), named after its owner's address (so Yap knows the
// person's own company, example.org). The past meetings' companies are
// seeded in meeting_folders.json (`noteDomains`), as Yap keeps them. The
// one-a-day wait starts afresh with each test (`e2e_notice_reset`).
import { test as base, expect, openView, expectStore, closeToasts, openSettings, closeSettings } from './support/fixtures.js';
import { startCalendarFeed, calendarFile, vevent } from './support/calendar-feed.js';

const SECOND = 1000;
const MINUTE = 60 * SECOND;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;
const OWNER = { name: 'Imogen', email: 'imogen@example.org', partstat: 'ACCEPTED' };
const SAM = { name: 'Sam Lee', email: 'sam@example.org' }; // a colleague
const PRIYA = { name: 'Priya Shah', email: 'priya@example.com' }; // a client
const ALEX = { name: 'Alex Kim', email: 'alex@example.com' };
const JORDAN = { name: 'Jordan Ray', email: 'jordan@example.net' }; // an agency

/** A time `ms` from now, on a whole second (as calendars write them). */
const from = (ms) => new Date(Math.floor((Date.now() + ms) / SECOND) * SECOND);
const now = () => Math.floor(Date.now() / SECOND);

/**
 * Past meeting notes: two with the client (one with a colleague too), one
 * with colleagues only, one with the client the person moved to Personal
 * themselves, and two with the agency — with the companies Yap kept.
 */
function seeded() {
  const ago = (days) => now() - days * 24 * 3600;
  const note = (id, title, when, extra = {}) => ({
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
    event: { key: `9:${title.toLowerCase().replace(/\W+/g, '-')}-${id}@e2e`, title, start: when, end: when + 1800 },
    ...extra,
  });
  const notes = [
    note(1, 'Supplier call', ago(14)),
    note(2, 'Pricing review', ago(7)),
    note(3, 'Team retro', ago(5), { participants: ['Sam Lee'] }),
    note(4, 'Contract call', ago(3), { folder: 'Personal', folderChosen: true }),
    note(5, 'Agency catch-up', ago(10), { participants: ['Jordan Ray'] }),
    note(6, 'Agency catch-up', ago(4), { participants: ['Jordan Ray'] }),
  ];
  const domains = {
    1: ['example.com'],
    2: ['example.com', 'example.org'],
    3: ['example.org'],
    4: ['example.com'],
    5: ['example.net'],
    6: ['example.net'],
  };
  return {
    'notes.json': { folders: ['Personal', 'Meetings', 'Clients', 'Agencies'], notes },
    'meeting_folders.json': {
      rules: {},
      noteDomains: Object.fromEntries(notes.map((n) => [String(n.id), { createdTs: n.createdTs, domains: domains[n.id] }])),
    },
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
      await use({ name: 'company-folders', config: { meetingReminder: 'never', meetingBrief: false }, data: seeded() });
    },
    { scope: 'worker' },
  ],
});

const notes = (yap) => yap.readJson('notes.json')?.notes ?? [];
const folderOf = (yap, id) => notes(yap).find((n) => n.id === id)?.folder;
const menu = (main) => main.getByRole('menu').first();
const toast = (main, text) => main.getByRole('status').filter({ hasText: text });
const pastRow = (main, title) => main.getByRole('listitem').filter({ has: main.getByRole('button', { name: title, exact: true }) });

/**
 * Today's kick-off with the client and check-in with the agency (both over
 * an hour ago), and tomorrow's review with the client.
 */
function calendar() {
  const meeting = (uid, summary, start, attendees) =>
    vevent({ uid, summary, start, end: new Date(start.getTime() + 30 * MINUTE), attendees: [...attendees, OWNER] });
  return calendarFile(OWNER.email, [
    meeting('kickoff@e2e', 'Kick-off', from(-2 * HOUR), [PRIYA, SAM]),
    meeting('checkin@e2e', 'Agency check-in', from(-3 * HOUR), [JORDAN]),
    meeting('review@e2e', 'Quarterly review', from(DAY + 3 * HOUR), [ALEX]),
  ]);
}

async function connect(yap, feed) {
  feed.set(calendar());
  const s = await yap.invoke('calendar_status');
  if (!s.connections.length) await yap.invoke('calendar_add_link', { link: feed.url, kind: 'ics' });
  await yap.invoke('calendar_sync');
}

/**
 * The note of today's meeting `uid` (made from the calendar, as "Add
 * notes" makes it; in Meetings), filed in `folder` from the Meetings
 * view's menu.
 */
async function fileByHand(yap, main, uid, title, folder) {
  const id = await yap.invoke('calendar_event', { key: `1:${uid}`, action: 'open' });
  if (folderOf(yap, id) !== 'Meetings') await yap.invoke('note_update', { id, folder: 'Meetings' });
  await expectStore(yap, 'notes.json', (s) => s.notes.find((n) => n.id === id)?.folder === 'Meetings');
  await openView(main, 'Meetings');
  const row = pastRow(main, title);
  await expect(row).toBeVisible();
  await row.getByRole('button', { name: `More for ${title}` }).click();
  await menu(main).getByRole('menuitem', { name: 'File this meeting in' }).hover();
  await main.getByRole('menu', { name: 'File this meeting in' }).getByRole('menuitemradio', { name: folder }).click();
  await expectStore(yap, 'notes.json', (s) => s.notes.find((n) => n.id === id)?.folder === folder);
  return id;
}

test.afterEach(async ({ main }) => {
  await closeSettings(main).catch(() => {});
  await closeToasts(main).catch(() => {});
});

test('a meeting with a client filed by hand offers its company\'s folder; Yes files the past ones, the next one goes there, Settings lists it', async ({
  yap,
  main,
  feed,
  shot,
}) => {
  await connect(yap, feed);
  // Yap learnt the person's own company from the calendar (its owner).
  const kickoff = await fileByHand(yap, main, 'kickoff@e2e', 'Kick-off', 'Clients');

  // The offer: by the client's company, not the colleague's (the person's
  // own), with the two past meetings it would move.
  const offer = toast(main, 'File every meeting with example.com in Clients?');
  await expect(offer).toBeVisible({ timeout: 10_000 });
  await expect(offer).toContainText('File every meeting with example.com in Clients? (2 past ones too)');
  await expect(offer.getByRole('button', { name: 'Just this one' })).toBeVisible();
  await expect(offer.getByRole('button', { name: 'Never for example.com' })).toBeVisible();
  await shot(main, '01-the-offer');

  // Yes: the rule, and the past meetings with the client in Clients — not
  // the colleagues' retro, not the one the person moved to Personal.
  await offer.getByRole('button', { name: 'Yes', exact: true }).click();
  const filed = toast(main, 'Meetings with example.com go in Clients');
  await expect(filed).toBeVisible();
  await expect(filed).toContainText('Moved 2 past meetings to Clients too.');
  await expect(filed.getByRole('button', { name: 'Undo' })).toBeVisible();
  await shot(main, '02-filed-with-undo');
  await expectStore(yap, 'notes.json', (s) => [1, 2].every((id) => s.notes.find((n) => n.id === id)?.folder === 'Clients'));
  expect(folderOf(yap, 3)).toBe('Meetings');
  expect(folderOf(yap, 4)).toBe('Personal');
  expect(folderOf(yap, kickoff)).toBe('Clients');
  await expectStore(yap, 'meeting_folders.json', (s) => s.rules['company:example.com']?.folder === 'Clients');
  // Kept beside the series rules: the answer and the kick-off's companies.
  const kept = yap.readJson('meeting_folders.json');
  expect(kept.companyOffers.asked).toContain('example.com|clients');
  expect(kept.noteDomains[String(kickoff)].domains).toEqual(['example.com', 'example.org']);
  await closeToasts(main);

  // Another company's meeting filed by hand the same day: no second offer.
  await fileByHand(yap, main, 'checkin@e2e', 'Agency check-in', 'Agencies');
  await main.waitForTimeout(2_000);
  await expect(main.getByRole('status').filter({ hasText: 'File every meeting with' })).toHaveCount(0);
  await closeToasts(main);

  // A new meeting with the client, its note made ahead from the calendar:
  // straight into Clients.
  await openView(main, 'Meetings');
  await main
    .getByRole('region', { name: 'Upcoming' })
    .getByRole('listitem')
    .filter({ hasText: 'Quarterly review' })
    .first()
    .getByRole('button', { name: /Quarterly review/ })
    .first()
    .click();
  await expect(main.getByPlaceholder('Untitled Note')).toHaveValue('Quarterly review', { timeout: 10_000 });
  const review = notes(yap).find((n) => n.event?.key === '1:review@e2e');
  expect(review).toMatchObject({ folder: 'Clients', title: 'Quarterly review' });

  // Settings → Meetings: the rule, with Remove.
  const dialog = await openSettings(main, 'Meetings');
  const list = dialog.getByRole('list', { name: 'Folders by company' });
  await list.scrollIntoViewIfNeeded();
  const rule = list.getByRole('listitem').filter({ hasText: 'example.com' });
  await expect(rule).toContainText('Meetings with people from example.com go in Clients');
  await shot(main, '03-settings-list');
  await rule.getByRole('button', { name: 'Remove' }).click();
  await expect(list.getByRole('listitem')).toHaveCount(0);
  await expect(list).toContainText('No company rules yet');
  await expectStore(yap, 'meeting_folders.json', (s) => !s.rules['company:example.com']);
  // The meetings stay where they are.
  expect(folderOf(yap, 1)).toBe('Clients');
  expect(folderOf(yap, review.id)).toBe('Clients');
});

test('Undo puts the past meetings back where they were and takes the rule away', async ({ yap, main, feed }) => {
  await connect(yap, feed);
  const checkin = await fileByHand(yap, main, 'checkin@e2e', 'Agency check-in', 'Agencies');
  const offer = toast(main, 'File every meeting with example.net in Agencies?');
  await expect(offer).toBeVisible({ timeout: 10_000 });
  await expect(offer).toContainText('(2 past ones too)');
  await offer.getByRole('button', { name: 'Yes', exact: true }).click();
  await expectStore(yap, 'notes.json', (s) => [5, 6].every((id) => s.notes.find((n) => n.id === id)?.folder === 'Agencies'));
  await expectStore(yap, 'meeting_folders.json', (s) => s.rules['company:example.net']?.folder === 'Agencies');

  await toast(main, 'Meetings with example.net go in Agencies').getByRole('button', { name: 'Undo' }).click();
  await expect(toast(main, 'Undone')).toContainText('2 meetings are back where they were.');
  await expectStore(yap, 'notes.json', (s) => [5, 6].every((id) => s.notes.find((n) => n.id === id)?.folder === 'Meetings'));
  expect(notes(yap).filter((n) => [5, 6].includes(n.id)).every((n) => !n.folderChosen)).toBe(true);
  await expectStore(yap, 'meeting_folders.json', (s) => !s.rules['company:example.net']);
  // The one filed by hand stays where the person put it.
  expect(folderOf(yap, checkin)).toBe('Agencies');
});
