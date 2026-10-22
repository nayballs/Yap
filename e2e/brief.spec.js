// Pre-meeting briefs (src-tauri/src/calendar/brief.rs): before a calendar
// meeting, what to know going in — last time with these people, their open
// items, what Yap remembers about them, the invite's agenda, every line
// linked to its note — on the reminder card (in the window and on the Yap
// bar), in the Meetings view and at the top of the meeting notepad's
// Summary, with two or three lines from the meeting's AI on top that Yap
// checks first (a line naming someone the notes never mention is left out).
//
// The calendar is a local iCal feed built at test time (support/
// calendar-feed.js); last week's meeting note and memory are seeded; the AI
// is the local fake (support/fake-llm.js), scripted to write two good lines
// and one invented one.
import {
  test as base,
  expect,
  openView,
  openSettings,
  closeSettings,
  closeToasts,
  inAnotherApp,
  expectStore,
} from './support/fixtures.js';
import { startCalendarFeed, calendarFile, vevent } from './support/calendar-feed.js';
import { startFakeLlm } from './support/fake-llm.js';

const SECOND = 1000;
const MINUTE = 60 * SECOND;
const DAY = 24 * 60 * MINUTE;
const OWNER = { name: 'Imogen', email: 'imogen@example.com', partstat: 'ACCEPTED' };
const PRIYA = { name: 'Priya Shah', email: 'priya@example.com' };
const SIOBHAN = { name: 'Siobhan Byrne', email: 'siobhan@example.com' };
const TITLE = 'Weekly 1:1 with Priya';

/** A time `ms` from now, on a whole second (as calendars write them). */
const from = (ms) => new Date(Math.floor((Date.now() + ms) / SECOND) * SECOND);
const now = () => Math.floor(Date.now() / SECOND);

const PLAN = [
  'Priya and you went through the pricing page and the support hiring plan.',
  '',
  '## Action plan',
  '### You',
  '- [ ] Send Priya the revised pricing table (due: Monday)',
  '- [ ] Book interviews for the two support roles',
  '### Priya',
  '- [ ] Draft the pricing FAQ (due: Thursday)',
  '',
  '## Decisions',
  '- Keep the free tier at three projects.',
  '',
  '## Open questions',
  '- Should the annual plan get a discount?',
].join('\n');

/** Last week's 1:1 (the same series as this week's) and someone else's meeting. */
function seededNotes() {
  const lastWeek = now() - 7 * 24 * 3600;
  const note = (id, title, when, participants, plan, event) => ({
    id,
    title,
    noteType: 'meeting',
    folder: 'Meetings',
    source: 'calendar',
    createdTs: when,
    updatedTs: when + 3600,
    participants,
    content: '',
    enhancedContent: plan,
    transcript: [],
    ...(event ? { event } : {}),
  });
  return {
    folders: ['Personal', 'Meetings'],
    notes: [
      note(1, TITLE, lastWeek, ['Priya Shah'], PLAN, {
        key: `9:weekly-priya@e2e:${lastWeek}`,
        title: TITLE,
        start: lastWeek,
        end: lastWeek + 1800,
      }),
      note(2, 'Budget review', now() - 3 * 24 * 3600, ['Rochelle Adams'], '## Action plan\n### Rochelle\n- [ ] Send the budget', null),
    ],
  };
}

const MEMORY = {
  version: 1,
  nextId: 2,
  items: [
    {
      id: 1,
      section: 'people',
      title: 'Priya Shah',
      summary: 'Product lead at Acme',
      details: ['Works from the Bristol office'],
      source: { kind: 'manual', title: '' },
      createdTs: now() - DAY / SECOND,
      updatedTs: now() - DAY / SECOND,
      dictation: false,
    },
  ],
};

/** This week's 1:1 (recurring, about to start) and a first meeting tomorrow. */
function feed() {
  const start = from(80 * SECOND);
  const tomorrow = from(DAY);
  return calendarFile(OWNER.email, [
    vevent({
      uid: 'weekly-priya@e2e',
      summary: TITLE,
      start,
      end: new Date(start.getTime() + 30 * MINUTE),
      rrule: 'FREQ=WEEKLY;COUNT=3',
      attendees: [PRIYA, OWNER],
      description: 'Agenda: pricing page, hiring plan',
    }),
    vevent({
      uid: 'intro-siobhan@e2e',
      summary: 'Intro call with Siobhan',
      start: tomorrow,
      end: new Date(tomorrow.getTime() + 30 * MINUTE),
      attendees: [SIOBHAN, OWNER],
      description: 'Agenda: introductions',
    }),
  ]);
}

const test = base.extend({
  calFeed: [
    async ({}, use) => {
      const f = await startCalendarFeed();
      f.set(feed());
      await use(f);
      await f.close();
    },
    { scope: 'worker' },
  ],
  fakeLlm: [
    async ({}, use) => {
      const fake = await startFakeLlm();
      // Two good lines, and one about someone no note mentions (Yap drops it).
      fake.reply('brief', (user) => {
        const invite = /\[(\d+)\] The invite:/.exec(user)?.[1];
        return [
          "Here's your brief:",
          '- Last time you kept the free tier at three projects. [1]',
          `- Whether the annual plan gets a discount is still open, and the pricing page is on the agenda. [1${invite ? `, ${invite}` : ''}]`,
          '- Tom will bring the budget to this meeting. [1]',
        ].join('\n');
      });
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({ fakeLlm }, use) => {
      await use({
        name: 'brief',
        config: {
          meetingReminder: '2min',
          llmScopes: { noteFormatting: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-notes' } },
        },
        data: { 'notes.json': seededNotes(), 'memory.json': MEMORY },
      });
    },
    { scope: 'worker' },
  ],
});

const status = (yap) => yap.invoke('calendar_status');
const recording = (yap) => yap.invoke('meeting_state').then((s) => s.recording);
const toast = (main, text) => main.getByRole('status').filter({ hasText: text });
const cardFor = (main, title) => toast(main, title).filter({ hasText: 'Snooze 2 min' });
const barCard = (yap, title) => yap.overlay.getByRole('status').filter({ hasText: title });
const row = (main, title) => main.getByRole('listitem').filter({ hasText: title });
const briefCalls = (fake) => fake.requests.filter((r) => r.kind === 'brief');

/**
 * The feed connected and synced, this week's 1:1 starting 80 s from now (a
 * fresh start each test: an answered card is answered for that start), and
 * briefed, its summary written.
 */
async function connected(yap, calFeed) {
  calFeed.set(feed());
  const s = await status(yap);
  if (!s.connections.length) await yap.invoke('calendar_add_link', { link: calFeed.url, kind: 'ics' });
  await yap.invoke('calendar_sync');
  await expect
    .poll(async () => (await status(yap)).card?.brief?.ai, { message: "the card's brief, with its summary", timeout: 30_000 })
    .toBe(true);
}

test.afterEach(async ({ yap }) => {
  if (await recording(yap)) {
    await yap.invoke('meeting_pause');
    await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
  }
  await yap.invoke('bar_simulate', { pointer: 'away' }).catch(() => {});
});

test('the reminder card unfolds the brief: last time, open items, memory, the agenda, each line linked to its note', async ({
  yap,
  main,
  calFeed,
  fakeLlm,
  shot,
}) => {
  await connected(yap, calFeed);
  const card = cardFor(main, TITLE);
  await expect(card).toBeVisible();
  await card.getByRole('button', { name: 'Brief' }).click();
  const brief = card.locator('[data-brief]');
  await expect(brief).toBeVisible();

  // The AI's two lines, cited; the one about "Tom" (in no note) left out.
  const inShort = brief.locator('[data-kind="summary"]');
  await expect(inShort).toContainText('Last time you kept the free tier at three projects.');
  await expect(inShort).toContainText('Whether the annual plan gets a discount is still open');
  await expect(brief).not.toContainText('Tom');
  // Last time: last week's 1:1 (the same series), not Rochelle's budget review.
  await expect(brief.locator('[data-kind="last"]')).toContainText(TITLE);
  await expect(brief).not.toContainText('Budget review');
  // Open items: You's and Priya's, with what was said about when.
  const open = brief.locator('[data-kind="open"]');
  await expect(open).toContainText('Send Priya the revised pricing table');
  await expect(open).toContainText('due Monday');
  await expect(open).toContainText('Draft the pricing FAQ');
  await expect(open).not.toContainText('Send the budget');
  // What Yap remembers about Priya, and the invite.
  await expect(brief.locator('[data-kind="memory"]')).toContainText('Product lead at Acme');
  await expect(brief.locator('[data-kind="agenda"]')).toContainText('Agenda: pricing page, hiring plan');
  // Every line but "first meeting" lines cites a source.
  const lines = brief.locator('li:not(.absence)');
  const count = await lines.count();
  expect(count).toBeGreaterThan(5);
  for (let i = 0; i < count; i += 1) await expect(lines.nth(i).locator('.cite').first()).toBeVisible();
  await main.waitForTimeout(250);
  await shot(main, '01-brief-on-the-reminder-card');

  // What the model was given: last week's notes, numbered, under 8k.
  const call = briefCalls(fakeLlm).at(-1);
  expect(call.user).toContain(`[1] Notes from "${TITLE}"`);
  expect(call.user).toContain('What Yap remembers about Priya Shah');
  expect(call.user).not.toContain('Budget review');
  expect(call.tokens).toBeLessThan(2_500);

  // A line's source opens its note in Notes.
  await open.locator('li').first().locator('.cite').first().click();
  await expect(main.getByPlaceholder('Untitled Note')).toHaveValue(TITLE, { timeout: 10_000 });
  await openView(main, 'Meetings');
});

test('on the Yap bar the card keeps one line for the brief, and unfolds it', async ({ yap, main, calFeed, shot }) => {
  await connected(yap, calFeed);
  await inAnotherApp(yap);
  const onBar = barCard(yap, TITLE);
  await expect(onBar).toBeVisible();
  const line = onBar.getByRole('button', { name: /^Brief/ });
  await expect(line).toContainText(/Last met .* · 3 open items/);
  await yap.overlay.waitForTimeout(300);
  await shot(yap.overlay, '02-brief-folded-on-the-bar');
  await line.click();
  const brief = onBar.locator('[data-brief]');
  await expect(brief.locator('[data-kind="summary"]')).toContainText('free tier');
  await expect(brief.locator('[data-kind="open"]')).toContainText('Draft the pricing FAQ');
  await yap.overlay.waitForTimeout(300);
  await shot(yap.overlay, '03-brief-on-the-bar');
  await yap.invoke('e2e_focus', { focused: true });
  await expect(cardFor(main, TITLE)).toBeVisible();
});

test('the Meetings view: a Brief for every meeting to come, and a first meeting says so without the AI', async ({
  yap,
  main,
  calFeed,
  fakeLlm,
  shot,
}) => {
  await connected(yap, calFeed);
  await closeToasts(main);
  await openView(main, 'Meetings');
  const weekly = row(main, TITLE).first();
  await weekly.getByRole('button', { name: 'Brief' }).click();
  const panel = main.getByRole('region', { name: `Brief for ${TITLE}` });
  await expect(panel.locator('[data-kind="open"]')).toContainText('Send Priya the revised pricing table');
  await expect(panel.locator('[data-kind="summary"]')).toContainText('free tier');

  const callsBefore = briefCalls(fakeLlm).length;
  const intro = row(main, 'Intro call with Siobhan');
  await intro.getByRole('button', { name: 'Brief' }).click();
  const first = main.getByRole('region', { name: 'Brief for Intro call with Siobhan' });
  await expect(first).toContainText('First meeting with these people');
  await expect(first.locator('[data-kind="agenda"]')).toContainText('Agenda: introductions');
  // Nothing for a model to add: no call.
  expect(briefCalls(fakeLlm).length).toBe(callsBefore);
  await panel.scrollIntoViewIfNeeded();
  await main.waitForTimeout(250);
  await shot(main, '04-brief-in-the-meetings-view');
  await first.scrollIntoViewIfNeeded();
  await shot(main, '04b-first-meeting-brief');
});

test("the meeting's notepad: a line over My thoughts while it records, the brief on top of Summary", async ({
  yap,
  main,
  calFeed,
  shot,
}) => {
  await connected(yap, calFeed);
  const card = cardFor(main, TITLE);
  await expect(card).toBeVisible();
  await card.getByRole('button', { name: 'Start notes' }).click();
  await expect.poll(() => recording(yap), { timeout: 15_000 }).toBe(true);
  const pad = yap.notepad;
  await expect(pad.getByRole('textbox', { name: 'Meeting title' })).toHaveValue(TITLE, { timeout: 10_000 });

  const line = pad.getByRole('button', { name: /^Brief/ });
  await expect(line).toContainText(/Last met .* · 3 open items/);
  await shot(pad, '05-brief-line-in-the-notepad');
  await line.click();
  await expect(pad.getByRole('tab', { name: /Summary/ })).toHaveAttribute('aria-selected', 'true');
  const brief = pad.getByRole('region', { name: 'Brief before the meeting' });
  await expect(brief.locator('[data-kind="open"]')).toContainText('Book interviews for the two support roles');
  await expect(brief.locator('[data-kind="summary"]')).toContainText('free tier');
  await pad.waitForTimeout(250);
  await shot(pad, '06-brief-in-the-notepad-summary');
  await pad.getByRole('button', { name: 'Close notepad' }).click();
});

test('Settings → Meetings: "Prepare a brief before meetings", on by default, and off takes the briefs away', async ({
  yap,
  main,
  calFeed,
  shot,
}) => {
  await connected(yap, calFeed);
  await closeToasts(main);
  const dialog = await openSettings(main, 'Meetings');
  const toggle = dialog.getByRole('group', { name: 'Calendar' }).getByRole('button', { name: 'Prepare a brief before meetings' });
  await toggle.scrollIntoViewIfNeeded();
  await expect(toggle).toHaveAttribute('aria-pressed', 'true');
  await shot(main, '07-settings-brief');
  await toggle.click();
  await expectStore(yap, 'config.json', (c) => c.meetingBrief === false);
  await closeSettings(main);
  await expect.poll(async () => (await status(yap)).briefs).toBe(false);
  await openView(main, 'Meetings');
  await expect(row(main, TITLE).first()).toBeVisible();
  await expect(row(main, TITLE).first().getByRole('button', { name: 'Brief' })).toHaveCount(0);
  // And back on.
  const again = await openSettings(main, 'Meetings');
  await again.getByRole('group', { name: 'Calendar' }).getByRole('button', { name: 'Prepare a brief before meetings' }).click();
  await expectStore(yap, 'config.json', (c) => c.meetingBrief === true);
  await closeSettings(main);
});
