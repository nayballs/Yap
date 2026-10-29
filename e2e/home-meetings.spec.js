// Home's meetings card and Welcome back (2026-10-29, after Wispr Flow's Home).
//
// - Meetings (views/home/MeetingsCard.svelte): the latest meeting notes,
//   and with a calendar connected what's coming up (Upcoming / Past). Each
//   opens its note; See all goes to Meetings. Hidden for someone with no
//   calendar and no meetings (insights-stats.spec.js checks that).
// - Welcome back (views/home/WelcomeBack.svelte, dictation/stats.rs
//   `welcome`): after 14 days without a dictation, one card with the
//   shortcut and one new thing — once per return.
import { test as base, expect, openView, closeToasts, expectStore } from './support/fixtures.js';
import { startCalendarFeed, calendarFile, vevent } from './support/calendar-feed.js';

const NOW = Math.floor(Date.now() / 1000);
const DAY = 86_400;

function seededNotes() {
  const note = (id, title, when) => ({
    id,
    title,
    noteType: 'meeting',
    folder: 'Meetings',
    source: 'manual',
    createdTs: when,
    updatedTs: when + 1800,
    participants: ['Priya Shah'],
    content: 'Notes from the meeting.',
    enhancedContent: '',
    transcript: [{ source: 'you', ts: when + 60, text: 'Shall we start?' }],
  });
  return {
    folders: ['Personal', 'Meetings'],
    notes: [note(1, 'Budget review', NOW - 3 * DAY), note(2, 'Design review', NOW - DAY)],
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
      // No reminder cards over the tests.
      await use({ name: 'home-meetings', config: { meetingReminder: 'never' }, data: { 'notes.json': seededNotes() } });
    },
    { scope: 'worker' },
  ],
});

/** A day `back` days ago on this PC's calendar: "2026-10-29". */
function dayKey(back = 0) {
  const d = new Date(Date.now() - back * DAY * 1000);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
}
const meetings = (main) => main.getByRole('region', { name: 'Meetings' });
const welcome = (main) => main.getByRole('region', { name: 'Welcome back' });

test('the latest meeting notes on Home: each opens its note, See all goes to Meetings', async ({ main, shot }) => {
  await openView(main, 'Home');
  const card = meetings(main);
  await expect(card).toBeVisible();
  // No calendar: just what happened, newest first, and no tabs.
  await expect(card.getByRole('tab')).toHaveCount(0);
  await expect(card.locator('.mtitle')).toHaveText(['Design review', 'Budget review']);
  await expect(card.locator('.mrow').first()).toContainText('Yesterday');
  await shot(main, '01-home-past-meetings');

  await card.getByRole('button', { name: /Design review/ }).click();
  await expect(main.getByPlaceholder('Untitled Note')).toHaveValue('Design review');

  await openView(main, 'Home');
  await meetings(main).getByRole('button', { name: 'See all' }).click();
  await expect(main.getByRole('navigation', { name: 'Main' }).getByRole('button', { name: 'Meetings', exact: true })).toHaveAttribute('aria-current', 'page');
});

test('with a calendar: Upcoming and Past, and a meeting to come opens its note', async ({ yap, main, feed, shot }) => {
  const start = new Date((NOW + 2 * 3600) * 1000);
  const end = new Date((NOW + 2 * 3600 + 1800) * 1000);
  // A meeting has people (an event on your own is busy time, not a meeting).
  const me = { name: 'Alex Tester', email: 'tester@example.com', partstat: 'ACCEPTED' };
  const priya = { name: 'Priya Shah', email: 'priya.shah@example.com' };
  feed.set(calendarFile(me.email, [vevent({ uid: 'roadmap@e2e', summary: 'Roadmap sync', start, end, organizer: priya, attendees: [priya, me] })]));
  await yap.invoke('calendar_add_link', { link: feed.url, kind: 'ics' });
  await yap.invoke('calendar_sync');

  await openView(main, 'Home');
  const card = meetings(main);
  const upcoming = card.getByRole('tab', { name: 'Upcoming' });
  await expect(upcoming).toHaveAttribute('aria-selected', 'true');
  await expect(card.locator('.mtitle')).toHaveText(['Roadmap sync']);
  await shot(main, '02-home-upcoming');

  // Past, from the keyboard too.
  await upcoming.focus();
  await main.keyboard.press('ArrowRight');
  await expect(card.getByRole('tab', { name: 'Past' })).toHaveAttribute('aria-selected', 'true');
  await expect(card.locator('.mtitle').first()).toHaveText('Design review');
  await card.getByRole('tab', { name: 'Upcoming' }).click();

  // Its note, made ahead of time.
  await card.getByRole('button', { name: /Roadmap sync/ }).click();
  await expect(main.getByPlaceholder('Untitled Note')).toHaveValue('Roadmap sync');
  await expectStore(yap, 'notes.json', (s) => s.notes.some((n) => n.title === 'Roadmap sync'));
  await closeToasts(main);
});

test('Welcome back after two weeks away: the shortcut and one new thing, once', async ({ yap, main, shot }) => {
  await yap.invoke('e2e_stats_set', { store: { days: [{ date: dayKey(20), words: 50, dictations: 3 }], milestone: 0 } });
  await openView(main, 'Insights');
  await openView(main, 'Home');
  const card = welcome(main);
  await expect(card).toBeVisible();
  await expect(card).toContainText("It's been 3 weeks since your last dictation.");
  // The dictation shortcut (a test run's is F24).
  await expect(card.locator('kbd')).toHaveText([/^F\d+$/]);
  await expect(card.locator('.tag')).toHaveText(/^(New|Worth a try)$/);
  await expect(card.locator('.cta')).toBeVisible();
  await shot(main, '03-welcome-back-light');
  await main.evaluate(() => (document.documentElement.dataset.yapTheme = 'dark'));
  await main.waitForTimeout(150);
  await shot(main, '04-welcome-back-dark');
  await main.evaluate(() => (document.documentElement.dataset.yapTheme = 'light'));

  // ✕: not again for this gap — after a dictation either.
  await card.getByRole('button', { name: 'Close welcome back' }).click();
  await expect(card).toHaveCount(0);
  await expectStore(yap, 'dictation_stats.json', (s) => s.welcomed === dayKey(20));
  await openView(main, 'Insights');
  await openView(main, 'Home');
  await expect(welcome(main)).toHaveCount(0);
  await yap.invoke('e2e_dictation', { text: 'back again' });
  await expectStore(yap, 'dictation_stats.json', (s) => s.returned?.from === dayKey(20));
  await openView(main, 'Insights');
  await openView(main, 'Home');
  await expect(welcome(main)).toHaveCount(0);
});

test('Welcome back for someone who dictated before they looked', async ({ yap, main }) => {
  await yap.invoke('e2e_stats_set', { store: { days: [{ date: dayKey(31), words: 50, dictations: 3 }], milestone: 0 } });
  await yap.invoke('e2e_dictation', { text: 'good to be back' });
  await openView(main, 'Insights');
  await openView(main, 'Home');
  const card = welcome(main);
  await expect(card).toContainText('Good to hear from you again after a month.');
  await card.getByRole('button', { name: 'Close welcome back' }).click();
  await expect(card).toHaveCount(0);
  await expectStore(yap, 'dictation_stats.json', (s) => s.welcomed === dayKey(31));
  await closeToasts(main);
});
