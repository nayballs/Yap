// Quiet meetings and check-ins (src-tauri/src/calls/meeting_guard.rs): after a
// stretch with nobody speaking, "Ending notes in 30 s…" counts down (Keep
// going / Stop now; anyone speaking cancels it), then the recording stops
// with a notice that says why; "Still in your meeting?" after a calendar
// meeting's end and an hour in, held back while anyone spoke lately; both
// on the meeting notepad when it's on screen, else on the Yap bar; and
// Settings → Meetings → "End notes when the meeting goes quiet".
//
// Test mode: a recording opens no audio device. The debug-only
// `e2e_meeting_checkins` shortens the timings to seconds and keeps the test
// player's "mic" delivering digital silence meanwhile, as a real one does,
// so the meeting's audio clock (what quiet time is counted on) runs;
// `e2e_meeting_feed` stands in for someone speaking (a transcribed line).
// The calendar meeting comes from a local iCal feed (support/calendar-feed.js).
import {
  test as base,
  expect,
  openSettings,
  closeSettings,
  closeToasts,
  expectStore,
  inAnotherApp,
  settingsDialog,
} from './support/fixtures.js';
import { newMeetingNote } from './support/meetings.js';
import { startCalendarFeed, calendarFile, vevent } from './support/calendar-feed.js';

const test = base.extend({
  feed: [
    async ({}, use) => {
      const feed = await startCalendarFeed();
      await use(feed);
      await feed.close();
    },
    { scope: 'worker' },
  ],
  // No calendar reminder cards in the way.
  yapOptions: [{ name: 'meeting-quiet', config: { meetingReminder: 'never' } }, { scope: 'worker' }],
});

const SECOND = 1000;
const MINUTE = 60 * SECOND;
const toast = (page, text) => page.getByRole('status').filter({ hasText: text });
const recording = (yap) => yap.invoke('meeting_state').then((s) => s.recording);
const prompt = (yap) => yap.invoke('meeting_checkin_status').then((s) => s.prompt);
const visible = (yap, label) => yap.invoke('plugin:window|is_visible', { label });
const barSim = (yap, args) => yap.invoke('bar_simulate', args);
const COUNTDOWN = /Ending notes in \d+ s…/;
/** A silent recording's end: "Started by mistake?" (fewer than 20 words). */
const ENDED = /Nothing to summarise yet|Started by mistake\?/;
/** Long enough never to come during a test. */
const NEVER = 60 * MINUTE;

let clockTs = Math.floor(Date.now() / 1000);
/** Someone says something: a transcribed line, as the recorder hands it in. */
async function say(yap, text = 'Shall we go over the launch plan?') {
  const segments = [{ source: 'them', text, ts: (clockTs += 5) }];
  expect(await yap.invoke('e2e_meeting_feed', { segments })).toBe(1);
}

/** Record into a new meeting note titled `title`. */
async function record(yap, main, title) {
  await newMeetingNote(main, title);
  await main.getByRole('button', { name: 'Record' }).click();
  await expect.poll(() => recording(yap)).toBe(true);
}

/** Pause whatever records (no summary) and wait for the last chunk. */
async function pauseAll(yap) {
  if (await recording(yap)) {
    await yap.invoke('meeting_pause');
    await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
  }
}

/** The meeting notepad on screen (it opens with a recording; a test may have closed it). */
async function notepadOnScreen(yap) {
  if (!(await visible(yap, 'notepad'))) {
    const note = (await yap.invoke('meeting_state')).noteId;
    await yap.invoke('notepad_open', { noteId: note });
  }
  await expect.poll(() => visible(yap, 'notepad')).toBe(true);
}

async function closeNotepad(yap) {
  if (await visible(yap, 'notepad')) {
    await yap.notepad.getByRole('button', { name: 'Close notepad' }).click();
    await expect.poll(() => visible(yap, 'notepad')).toBe(false);
  }
}

test.afterEach(async ({ yap }) => {
  await pauseAll(yap);
  await yap.invoke('e2e_meeting_checkins', {});
  await barSim(yap, { pointer: 'away' }).catch(() => {});
  await closeToasts(yap.main);
  if (await visible(yap, 'notepad')) await closeToasts(yap.notepad);
});

test('a quiet meeting counts down in the notepad, then stops and says why', async ({ yap, main, shot }) => {
  // Quiet after 3 s of meeting audio, a 5 s countdown; no check-ins.
  await yap.invoke('e2e_meeting_checkins', { quietMs: 3_000, countdownMs: 5_000, recentMs: NEVER, hourMs: NEVER });
  await record(yap, main, 'Quiet stand-up');
  await notepadOnScreen(yap);

  // The notepad is on screen, so it asks there (not in the main window).
  const countdown = toast(yap.notepad, COUNTDOWN);
  await expect(countdown).toBeVisible({ timeout: 10_000 });
  await expect(countdown).toContainText('Nobody has spoken for 3 seconds. Yap will stop recording and write your action plan.');
  await expect(countdown.getByRole('button', { name: 'Keep going' })).toBeVisible();
  await expect(countdown.getByRole('button', { name: 'Stop now' })).toBeVisible();
  await expect(toast(main, COUNTDOWN)).toHaveCount(0);
  expect(await prompt(yap)).toMatchObject({ kind: 'quiet', surface: 'notepad', until: expect.any(Number) });
  await shot(yap.notepad, '01-countdown-in-the-notepad');

  // Left alone: it stops, writes (here: asks about) the action plan, and says why.
  const stopped = toast(yap.notepad, 'Stopped: the meeting went quiet');
  await expect(stopped).toBeVisible({ timeout: 15_000 });
  await expect(stopped).toContainText('Nobody had spoken for 3 seconds, so Yap stopped recording and is writing your action plan.');
  await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
  await expect(countdown).toHaveCount(0);
  expect(await prompt(yap)).toBeNull();
  await shot(yap.notepad, '02-stopped-because-quiet');
  // One at a time (notices/notice.rs): the question waits until the reason has
  // been on screen, then takes its place.
  await expect(toast(yap.notepad, ENDED)).toBeVisible({ timeout: 20_000 });
  await expect(stopped).toHaveCount(0);
});

test('anyone speaking cancels the countdown; Keep going buys time; Stop now stops', async ({ yap, main }) => {
  await yap.invoke('e2e_meeting_checkins', { quietMs: 3_000, countdownMs: 8_000, recentMs: NEVER, hourMs: NEVER });
  await record(yap, main, 'Planning call');
  await notepadOnScreen(yap);
  const countdown = toast(yap.notepad, COUNTDOWN);

  // Someone speaks mid-countdown: it's off, and the recording goes on.
  await expect(countdown).toBeVisible({ timeout: 10_000 });
  await say(yap);
  await expect(countdown).toHaveCount(0);
  expect(await prompt(yap)).toBeNull();
  expect(await recording(yap)).toBe(true);

  // Quiet again: another countdown. Keep going: gone, still recording.
  await expect(countdown).toBeVisible({ timeout: 10_000 });
  await countdown.getByRole('button', { name: 'Keep going' }).click();
  await expect(countdown).toHaveCount(0);
  expect(await prompt(yap)).toBeNull();
  expect(await recording(yap)).toBe(true);

  // Another quiet stretch after that; Stop now stops at once, no notice.
  await expect(countdown).toBeVisible({ timeout: 10_000 });
  await countdown.getByRole('button', { name: 'Stop now' }).click();
  await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
  await expect(toast(yap.notepad, 'Stopped: the meeting went quiet')).toHaveCount(0);
  await expect(toast(yap.notepad, ENDED)).toBeVisible({ timeout: 20_000 });
});

test('with the notepad closed, the countdown is a Yap bar card you can read at a glance', async ({ yap, main, shot }) => {
  await yap.invoke('e2e_meeting_checkins', { quietMs: 3_000, countdownMs: 20_000, recentMs: NEVER, hourMs: NEVER });
  await record(yap, main, 'Bar check');
  await closeNotepad(yap);
  await inAnotherApp(yap); // in the call app, the notepad closed

  const bar = yap.overlay;
  const card = bar.getByRole('status').filter({ hasText: 'Nobody has spoken for 3 seconds' });
  await expect(card).toBeVisible({ timeout: 10_000 });
  await expect(card.getByRole('timer')).toContainText(/Ending notes in \d+ s…/);
  await expect(card).toContainText('Yap will stop recording and write your action plan.');
  expect(await prompt(yap)).toMatchObject({ kind: 'quiet', surface: 'bar' });
  expect((await yap.invoke('bar_status')).cards.map((c) => c.id)).toContain('meeting-checkin');
  // Nowhere else.
  await expect(toast(main, COUNTDOWN)).toHaveCount(0);
  await bar.waitForTimeout(300); // the card's entrance
  await shot(bar, '03-countdown-on-the-bar');

  // Keep going from the bar: the card goes, the recording carries on.
  await barSim(yap, { pointer: 'card:meeting-checkin' });
  await card.getByRole('button', { name: 'Keep going' }).click();
  await expect(card).toHaveCount(0);
  await expect.poll(() => prompt(yap)).toBeNull();
  expect(await recording(yap)).toBe(true);
});

test('"Still in your meeting?" an hour in (seconds here), held back while anyone spoke lately', async ({ yap, main, shot }) => {
  // The hour check-in after 3 s, once nobody has spoken for 2 s.
  await yap.invoke('e2e_meeting_checkins', { quietMs: NEVER, recentMs: 2_000, hourMs: 3_000, fadeMs: NEVER });
  await record(yap, main, 'Long workshop');
  await notepadOnScreen(yap);
  const checkIn = toast(yap.notepad, 'Still in your meeting?');

  // Talk keeps it away: a line every second until 6 s in.
  for (let i = 0; i < 6; i++) {
    await say(yap, `Point number ${i + 1} on the agenda.`);
    await main.waitForTimeout(1_000);
    expect(await prompt(yap)).toBeNull();
  }
  await expect(checkIn).toBeVisible({ timeout: 10_000 });
  await expect(checkIn).toContainText(/Yap has been taking notes for \d+ seconds, and nobody has spoken for \d+ seconds?\./);
  await expect(checkIn.getByRole('button', { name: 'Stop and summarise' })).toBeVisible();
  expect(await prompt(yap)).toMatchObject({ kind: 'hour', surface: 'notepad' });
  await shot(yap.notepad, '04-still-in-your-meeting');

  // Keep recording: gone, and not asked again this recording.
  await checkIn.getByRole('button', { name: 'Keep recording' }).click();
  await expect(checkIn).toHaveCount(0);
  await expect.poll(() => prompt(yap)).toBeNull();
  await main.waitForTimeout(4_000);
  expect(await prompt(yap)).toBeNull();
  expect(await recording(yap)).toBe(true);
});

test('"Still in your meeting?" after the calendar meeting ran over; Stop and summarise stops', async ({ yap, main, feed, shot }) => {
  // A meeting on now that ends in 12 s (the recording must start before
  // the check-in's time for it to count).
  const now = Date.now();
  const at = (ms) => new Date(Math.floor((now + ms) / SECOND) * SECOND);
  feed.set(
    calendarFile('imogen@example.com', [
      vevent({
        uid: 'quiet-review@e2e',
        summary: 'Quiet review',
        start: at(-20 * MINUTE),
        end: at(12 * SECOND),
        attendees: [
          { name: 'Sam Lee', email: 'sam@example.com' },
          { name: 'Imogen', email: 'imogen@example.com' },
        ],
      }),
    ])
  );
  await yap.invoke('calendar_add_link', { link: feed.url, kind: 'ics' });
  await yap.invoke('calendar_sync');
  // 2 s after its end, once nobody has spoken for 1 s; no hour check-in.
  await yap.invoke('e2e_meeting_checkins', { quietMs: NEVER, recentMs: 1_000, afterEndMs: 2_000, hourMs: NEVER, fadeMs: NEVER });
  await record(yap, main, 'Review notes');
  // The recording belongs to the meeting on now.
  const noteId = (await yap.invoke('meeting_state')).noteId;
  await expect
    .poll(() => (yap.readJson('notes.json')?.notes ?? []).find((n) => n.id === noteId)?.event?.title)
    .toBe('Quiet review');
  await notepadOnScreen(yap);

  const checkIn = toast(yap.notepad, 'Still in your meeting?');
  await expect(checkIn).toBeVisible({ timeout: 25_000 });
  await expect(checkIn).toContainText(/“Quiet review” was due to end \d+ seconds? ago\. Yap is still taking notes\./);
  expect(await prompt(yap)).toMatchObject({ kind: 'calendar', surface: 'notepad' });
  await shot(yap.notepad, '05-calendar-meeting-ran-over');

  await checkIn.getByRole('button', { name: 'Stop and summarise' }).click();
  await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
  await expect.poll(() => prompt(yap)).toBeNull();
  await expect(toast(yap.notepad, ENDED)).toBeVisible({ timeout: 20_000 });
  for (const c of (await yap.invoke('calendar_status')).connections) await yap.invoke('calendar_disconnect', { id: c.id });
});

test('Settings → Meetings: "End notes when the meeting goes quiet", on by default', async ({ yap, main, shot }) => {
  await openSettings(main, 'Meetings');
  const card = settingsDialog(main).getByRole('group', { name: 'While a meeting records' });
  const toggle = card.getByRole('button', { name: 'End notes when the meeting goes quiet' });
  await toggle.scrollIntoViewIfNeeded();
  await expect(toggle).toHaveAttribute('aria-pressed', 'true');
  await expect(card).toContainText('After 10 minutes with nobody speaking, Yap counts down 30 seconds');
  await shot(main, '06-settings-end-when-quiet');

  // Off: a quiet meeting just carries on.
  await toggle.click();
  await expectStore(yap, 'config.json', (c) => c.meetingEndWhenQuiet === false);
  await closeSettings(main);
  await yap.invoke('e2e_meeting_checkins', { quietMs: 2_000, countdownMs: 3_000, recentMs: NEVER, hourMs: NEVER });
  await record(yap, main, 'Silent on purpose');
  await main.waitForTimeout(6_000);
  expect(await prompt(yap)).toBeNull();
  expect(await recording(yap)).toBe(true);
  await pauseAll(yap);

  // And back on.
  await openSettings(main, 'Meetings');
  await toggle.click();
  await expectStore(yap, 'config.json', (c) => c.meetingEndWhenQuiet === true);
  await closeSettings(main);
});
