// Calendar tools in Chat (src-tauri/src/chat/tools/calendar.rs): "what's on
// tomorrow?" answered from the calendar Yap keeps on this PC — a local iCal
// feed (support/calendar-feed.js) connected and synced as Settings does it —
// through the scripted fake AI (support/fake-llm.js), so this checks Yap's
// side: the tool is offered with a calendar connected (never without), Yap
// reads "tomorrow" as the person's date and answers with that day's
// meetings and busy time, each meeting numbered; the answer's citations are
// pills, a meeting it names without one gets its number, the Sources row is
// "From your calendar", and a pill opens the meeting's notes (made from the
// invite). Asked out loud, the answer is said whole: not cut short as a web
// answer is ("The details and sources are in Chat."). With no calendar the
// tool isn't offered and the model is told to say so.
import { test as base, expect, openView, sidebar, inAnotherApp, mainOnScreen, setConfig } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { startCalendarFeed, calendarFile, vevent } from './support/calendar-feed.js';

const test = base.extend({
  fakeLlm: [
    async ({}, use) => {
      const fake = await startFakeLlm();
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  feed: [
    async ({}, use) => {
      const feed = await startCalendarFeed();
      await use(feed);
      await feed.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({ fakeLlm }, use) => {
      await use({
        name: 'chat-calendar',
        config: {
          meetingReminder: 'never',
          meetingBrief: false,
          llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat' } },
        },
      });
    },
    { scope: 'worker' },
  ],
});

const ME = { name: 'Imogen', email: 'imogen@example.com' };
/** Tomorrow at `h:m` on this PC's clock (the app reads "tomorrow" there too). */
function tomorrowAt(h, m = 0) {
  const d = new Date();
  d.setDate(d.getDate() + 1);
  d.setHours(h, m, 0, 0);
  return d;
}
const minutes = (d, n) => new Date(d.getTime() + n * 60_000);

function feedFile() {
  const design = tomorrowAt(10);
  const budget = tomorrowAt(14, 30);
  const dentist = tomorrowAt(16);
  return calendarFile(ME.email, [
    vevent({
      uid: 'design-review',
      summary: 'Design review',
      start: design,
      end: minutes(design, 60),
      attendees: [ME, { name: 'Tanay Kothari', email: 'tanay@example.com' }],
      location: 'https://teams.microsoft.com/l/meetup-join/19%3a1',
    }),
    vevent({
      uid: 'budget',
      summary: 'Budget catch-up',
      start: budget,
      end: minutes(budget, 30),
      attendees: [ME, { name: 'Sam Lee', email: 'sam@example.com' }],
    }),
    // Nobody else on it: busy time, not a meeting.
    vevent({ uid: 'dentist', summary: 'Dentist', start: dentist, end: minutes(dentist, 45), attendees: [ME] }),
  ]);
}

const status = (yap) => yap.invoke('calendar_status');
const box = (main) => main.getByRole('textbox', { name: 'Message' });
const thread = (main) => main.locator('.msgs');
const toolNames = (r) => r.tools ?? [];
const toolResults = (r) => (r.messages ?? []).filter((m) => m.role === 'tool').map((m) => String(m.content));

async function connect(yap, feed) {
  feed.set(feedFile());
  if (!(await status(yap)).connections.some((c) => c.kind === 'ics')) {
    await yap.invoke('calendar_add_link', { link: feed.url, kind: 'ics' });
  }
  await yap.invoke('calendar_sync');
  await expect.poll(async () => (await status(yap)).events.map((e) => e.title)).toEqual(['Design review', 'Budget catch-up']);
}

async function ask(main, text) {
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
  await box(main).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
}

test('"What\'s on tomorrow?": the calendar tool, its chip, and the meetings cited and opened', async ({ yap, main, fakeLlm, feed, shot }) => {
  await connect(yap, feed);
  const before = fakeLlm.requests.length;
  fakeLlm.scriptChat([
    { tools: [{ name: 'check_calendar', args: { day: 'tomorrow' } }] },
    // The second meeting named but not cited: Yap cites it.
    { content: "Tomorrow you've got the **Design review** at 10:00 [1] with Tanay, then the Budget catch-up at 14:30. You're at the dentist at 16:00." },
  ]);
  await ask(main, "What's on tomorrow?");
  await expect(thread(main)).toContainText('then the Budget catch-up');

  // The tool was offered, and its result held tomorrow's meetings (numbered) and the dentist.
  const calls = fakeLlm.requests.slice(before);
  expect(toolNames(calls[0])).toContain('check_calendar');
  expect(calls[0].system).toContain('Use check_calendar');
  const result = JSON.parse(toolResults(calls[1]).at(-1));
  expect(result.success).toBe(true);
  expect(result.data.meetings.map((m) => [m.n, m.title])).toEqual([
    [1, 'Design review'],
    [2, 'Budget catch-up'],
  ]);
  expect(result.data.meetings[0].with).toBe('Tanay Kothari');
  expect(result.data.meetings[0].on).toBe('Teams');
  expect(result.data.busy).toEqual([expect.objectContaining({ title: 'Dentist', own_event: true })]);
  const day = tomorrowAt(0).toLocaleDateString('en-GB', { weekday: 'long', day: 'numeric', month: 'long', year: 'numeric' }).replace(',', '');
  expect(result.data.asked).toBe(day);

  // The chip, with the calendar's mark; both meetings pills; the Sources row.
  const answer = thread(main).locator('.cbubble.assistant').last();
  await expect(thread(main).locator('.toolchip.calendar')).toHaveText(`Checked your calendar: 2 meetings, ${day}`);
  await expect(answer.locator('a.md-passage')).toHaveCount(2);
  const sources = main.getByRole('group', { name: 'From your calendar' });
  await expect(sources.getByTestId('chat-calendar-source')).toHaveCount(2);
  await expect(sources).toContainText('Design review');
  await expect(sources).toContainText('Budget catch-up');
  await shot(main, '01-calendar-answer');

  // Kept with the answer: the event keys its pills open.
  const convs = await yap.invoke('chats_list');
  const conv = await yap.invoke('chat_get', { id: convs[0].id });
  const saved = conv.messages.at(-1);
  expect(saved.text).toContain('Budget catch-up [2]');
  expect(saved.sources.map((s) => [s.n, s.title, s.field])).toEqual([
    [1, 'Design review', 'calendar'],
    [2, 'Budget catch-up', 'calendar'],
  ]);
  expect(saved.sources[0].event).toMatch(/design-review$/);

  // A pill opens the meeting's notes: made from the invite (no note yet).
  await answer.locator('a.md-passage', { hasText: '1' }).click();
  await expect(sidebar(main).getByRole('button', { name: 'Notes', exact: true })).toHaveAttribute('aria-current', 'page');
  await expect(main.getByPlaceholder('Untitled Note')).toHaveValue('Design review');
  await shot(main, '02-pill-opened-the-meetings-notes');
  const notes = yap.readJson('notes.json')?.notes ?? [];
  expect(notes.filter((n) => n.title === 'Design review')).toHaveLength(1);

  // Opened again (the pill, the Meetings view's row): the same note, not a second.
  const id = await yap.invoke('calendar_event', { key: saved.sources[0].event, action: 'open' });
  expect(notes.find((n) => n.title === 'Design review').id).toBe(id);
  expect((yap.readJson('notes.json')?.notes ?? []).filter((n) => n.title === 'Design review')).toHaveLength(1);
});

test('asked out loud, a calendar answer is said whole, never cut short for its sources', async ({ yap, main, fakeLlm, feed }) => {
  await connect(yap, feed);
  await setConfig(yap, main, { speakReplies: true });
  await yap.invoke('tts_e2e_spoken', { clear: true });
  await inAnotherApp(yap);
  const answer = 'Your next meeting is the Design review at 10:00 tomorrow [1].';
  fakeLlm.scriptChat([{ tools: [{ name: 'check_calendar', args: {} }] }, { content: answer }]);
  await yap.invoke('e2e_assistant_ask', { text: "what's my next meeting", listenMs: 0 });
  await expect
    .poll(async () => (await yap.invoke('tts_e2e_spoken')).map((s) => s.text), { timeout: 15_000 })
    // (Its citation never read; the time as said aloud.)
    .toEqual(["Your next meeting is the Design review at ten o'clock tomorrow."]);
  await yap.invoke('tts_stop');
  await mainOnScreen(yap);
});

test('with no calendar connected the tool is not offered, and the model is told to say so', async ({ yap, main, fakeLlm }) => {
  for (const c of (await status(yap)).connections) await yap.invoke('calendar_disconnect', { id: c.id });
  await expect.poll(async () => (await status(yap)).connections.length).toBe(0);
  const before = fakeLlm.requests.length;
  fakeLlm.scriptChat([{ content: "I can't see your calendar: connect one in Settings → Connectors." }]);
  await ask(main, 'Am I free at 3 on Friday?');
  await expect(thread(main)).toContainText("I can't see your calendar");
  const call = fakeLlm.requests.slice(before).find((r) => r.tools?.length);
  expect(toolNames(call)).not.toContain('check_calendar');
  expect(call.system).not.toContain('check_calendar');
  expect(call.user).toContain('No calendar is connected to Yap');
});
