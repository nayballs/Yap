// The voice separation's labels on a meeting's lines (src-tauri/src/speakers/
// labels.rs): the speaker engine (speakers/diarize.rs) says who spoke when on the
// call side, and the "Them" lines become "Speaker 1", "Speaker 2"… as its
// labels settle (~10 s behind the talk), a line split where the speaker
// changes, the rest when the recording stops — before the action plan reads
// them. A line the user named stays theirs; an engine that stops early
// leaves You/Them and says so; a 1:1 invite's guess goes to the one voice.
//
// No model and no audio: lines with places on the recording's clock (and
// word times) are handed to the recorder with `e2e_meeting_feed`, and the
// engine's timelines are scripted with `e2e_diarize_timeline` (a `done` one
// is held until the recording stops, where the engine's final labels come).
// The real engine on real audio: diarization-real.spec.js.
import fs from 'node:fs';
import { test as base, expect, closeToasts } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { newMeetingNote, meetingNote } from './support/meetings.js';
import { startCalendarFeed, calendarFile, vevent } from './support/calendar-feed.js';
import { hasLine, withLineSplice } from '../src/lib/core/notepadText.js';

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
        name: 'speaker-labels',
        config: {
          llmScopes: {
            noteFormatting: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-notes' },
          },
          meetingReminder: 'never',
        },
      });
    },
    { scope: 'worker' },
  ],
});

const visible = (yap, label) => yap.invoke('plugin:window|is_visible', { label });
const recording = (yap) => yap.invoke('meeting_state').then((s) => s.recording);
const tab = (yap, name) => yap.notepad.getByRole('tab', { name });
const transcript = (pad) => pad.getByRole('log', { name: 'Transcript' });
const notesLog = (main) => main.getByRole('log', { name: 'Meeting transcript' });
/** The notepad's speaker groups, in order. */
const groupNames = (pad) => transcript(pad).locator(':scope > .group .who').allTextContents();
/** Each group's bubbles' text. */
const groupLines = (pad) =>
  transcript(pad)
    .locator(':scope > .group')
    .evaluateAll((groups) => groups.map((g) => [...g.querySelectorAll('.bubble')].map((b) => b.textContent.trim())));
const notesNames = (main) => notesLog(main).locator('.bubble .who').allTextContents();

let clockTs = Math.floor(Date.now() / 1000);

/**
 * A line on the recording's clock as the recorder makes one: placed from
 * `startMs`, a word every 500 ms (400 ms long), dated from the clock.
 */
function placed(base, source, text, startMs) {
  const n = text.split(/\s+/).filter(Boolean).length;
  return {
    source,
    text,
    ts: base + Math.floor(startMs / 1000),
    startMs,
    endMs: startMs + (n - 1) * 500 + 400,
    words: Array.from({ length: n }, (_, k) => [k * 500, k * 500 + 400]),
  };
}

/** Hand the recorder lines (`[source, text, startMs]`) of a recording that began at `base`. */
async function say(yap, base, ...lines) {
  const segments = lines.map(([source, text, startMs]) => placed(base, source, text, startMs));
  expect(await yap.invoke('e2e_meeting_feed', { segments })).toBe(segments.length);
}

/** The engine's labels: `[startMs, endMs, speaker]` turns, settled up to `labelledMs`. */
async function labels(yap, turns, labelledMs, extra = {}) {
  const segments = turns.map(([startMs, endMs, speaker]) => ({ startMs, endMs, speaker }));
  await yap.invoke('e2e_diarize_timeline', { segments, labelledMs, done: false, ...extra });
}

async function startMeeting(yap, title) {
  const note = await yap.invoke('note_create', { title, folder: 'Meetings' });
  await yap.invoke('meeting_start', { noteId: note.id });
  await expect.poll(() => visible(yap, 'notepad')).toBe(true);
  await expect.poll(() => recording(yap)).toBe(true);
  await tab(yap, 'Transcript').click();
  clockTs += 600;
  return { id: note.id, base: clockTs };
}

async function stopMeeting(yap) {
  await yap.notepad.getByRole('button', { name: 'Stop', exact: true }).click();
  await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
}

test.afterEach(async ({ yap }) => {
  if (await recording(yap)) {
    await yap.invoke('meeting_pause');
    await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
  }
  for (let i = 0; i < 3; i += 1) await yap.notepad.keyboard.press('Escape');
  await closeToasts(yap.notepad);
});

test('the windows follow a labelling pass, a split line included', () => {
  const t = [
    { source: 'them', text: 'Hello all.', ts: 10, startMs: 0 },
    { source: 'you', text: 'Hi.', ts: 12, startMs: 2_000 },
    { source: 'them', text: 'Revenue is up. Thanks, and costs are flat.', ts: 15, startMs: 5_000 },
  ];
  // Line 2 split in two; everything after it moved one along.
  const change = {
    lineSplice: {
      at: 2,
      remove: 1,
      lines: [
        { source: 'them', text: 'Revenue is up.', ts: 15, startMs: 5_000, speaker: 'them-1' },
        { source: 'them', text: 'Thanks, and costs are flat.', ts: 16, startMs: 6_500, speaker: 'them-2' },
      ],
    },
    lineSpeakers: [['them-1', false], ['you', false], ['them-1', false], ['them-2', false]],
  };
  const after = withLineSplice(t, change);
  expect(after.map((s) => [s.text, s.speaker])).toEqual([
    ['Hello all.', 'them-1'],
    ['Hi.', ''],
    ['Revenue is up.', 'them-1'],
    ['Thanks, and costs are flat.', 'them-2'],
  ]);
  // Not the copy it changed (a line arrived meanwhile): fetch the note.
  expect(withLineSplice([...t, { source: 'you', text: 'x', ts: 20, startMs: 9_000 }], change)).toBeNull();
  expect(withLineSplice(t.slice(0, 2), change)).toBeNull();
  // A line that arrives again (its first part keeps its side, date and place).
  expect(hasLine(after, t[2])).toBe(true);
  expect(hasLine(after, { source: 'them', text: 'New.', ts: 30, startMs: 20_000 })).toBe(false);
  expect(hasLine(after, { source: 'them', text: 'Unplaced.', ts: 15 })).toBe(false);
});

test('labels fill in the speakers as the meeting goes, splitting a line where the speaker changes', async ({
  yap,
  main,
  shot,
}) => {
  await newMeetingNote(main, 'Quarterly numbers');
  await main.getByRole('button', { name: 'Record' }).click();
  await expect.poll(() => visible(yap, 'notepad')).toBe(true);
  const pad = yap.notepad;
  await tab(yap, 'Transcript').click();
  clockTs += 600;
  const base = clockTs;
  await say(
    yap,
    base,
    ['them', 'Morning everyone, I have the numbers for the quarter ready.', 0],
    ['you', 'Great, please go ahead with revenue first.', 6_000],
    ['them', 'Revenue is up eight percent. Thanks, and costs stayed flat this time.', 15_000],
    ['them', 'Shall we move on to hiring next?', 30_000]
  );
  const log = transcript(pad);
  await expect(log).toContainText('Shall we move on to hiring next?');
  expect(await groupNames(pad)).toEqual(['Them', 'You', 'Them']);

  // The engine settles its first 10 s: the first line is Speaker 1's.
  await labels(yap, [[0, 5_000, 1]], 10_000);
  await expect.poll(() => groupNames(pad)).toEqual(['Speaker 1', 'You', 'Them']);

  // Then 40 s: a second voice takes over mid-line at "Thanks", and the
  // first one asks the last question.
  const turns = [
    [0, 5_000, 1],
    [15_000, 17_450, 1],
    [17_450, 21_000, 2],
    [30_000, 34_000, 1],
  ];
  await labels(yap, turns, 40_000);
  await expect.poll(() => groupNames(pad)).toEqual(['Speaker 1', 'You', 'Speaker 1', 'Speaker 2', 'Speaker 1']);
  expect(await groupLines(pad)).toEqual([
    ['Morning everyone, I have the numbers for the quarter ready.'],
    ['Great, please go ahead with revenue first.'],
    ['Revenue is up eight percent.'],
    ['Thanks, and costs stayed flat this time.'],
    ['Shall we move on to hiring next?'],
  ]);
  await shot(pad, '01-labelled-live-transcript');
  // The Notes view follows (one name per run of a speaker).
  await expect.poll(() => notesNames(main)).toEqual(['Speaker 1', 'You', 'Speaker 1', 'Speaker 2', 'Speaker 1']);
  await shot(main, '02-notes-view-labelled');

  // Stored: the split line is two lines, each with its words and place.
  const stored = meetingNote(yap.readJson('notes.json'), 'Quarterly numbers');
  const them = stored.transcript.filter((s) => s.source === 'them');
  expect(them.map((s) => s.speaker)).toEqual(['them-1', 'them-1', 'them-2', 'them-1']);
  expect(them[2]).toMatchObject({ startMs: 17_500, endMs: 20_900, ts: base + 17 });
  expect(them[2].words).toHaveLength(7);

  // Naming a voice names its later lines too.
  await yap.invoke('speaker_rename', { noteId: stored.id, speaker: 'them-2', name: 'Priya' });
  await say(yap, base, ['them', 'One more thing about the contractor budget.', 45_000]);
  await labels(yap, [...turns, [45_000, 49_000, 2]], 55_000);
  await expect.poll(() => groupNames(pad)).toEqual(['Speaker 1', 'You', 'Speaker 1', 'Priya', 'Speaker 1', 'Priya']);
});

test('the end labels the rest before the action plan reads them; a line the user named stays theirs', async ({
  yap,
  fakeLlm,
  shot,
}) => {
  const { id, base } = await startMeeting(yap, 'Hiring sync');
  const pad = yap.notepad;
  await say(
    yap,
    base,
    ['them', 'Thanks for joining, I will share the hiring plan with finance by Friday.', 0],
    ['you', 'Sounds good, and I can review the job descriptions tomorrow morning.', 8_000],
    ['them', 'I can also book the interview rooms for next week.', 16_000],
    ['them', 'And I will send the offer letter template to legal today.', 30_000]
  );
  await expect(transcript(pad)).toContainText('offer letter template');
  // The user says who said the third line before any labels come.
  const before = await yap.invoke('note_get', { id });
  const rooms = before.transcript.findIndex((s) => s.text.startsWith('I can also book'));
  await yap.invoke('speaker_rename_lines', { noteId: id, lines: [rooms], name: 'Sam' });
  await expect.poll(() => groupNames(pad)).toEqual(['Them', 'You', 'Sam', 'Them']);

  // Live labels reach the first line; the final ones (held until the stop)
  // say the rooms line was Speaker 2's too, and the last one is.
  await labels(yap, [[0, 7_000, 1]], 10_000);
  await expect.poll(() => groupNames(pad)).toEqual(['Speaker 1', 'You', 'Sam', 'Them']);
  await labels(yap, [[0, 7_000, 1], [16_000, 21_000, 2], [30_000, 36_000, 2]], 40_000, { done: true });
  // Not yet: the engine's final labels come when the recording stops.
  expect(await groupNames(pad)).toEqual(['Speaker 1', 'You', 'Sam', 'Them']);

  const plans = fakeLlm.requests.filter((r) => r.kind === 'actionPlan').length;
  await stopMeeting(yap);
  await tab(yap, 'Transcript').click();
  await expect.poll(() => groupNames(pad)).toEqual(['Speaker 1', 'You', 'Sam', 'Speaker 2']);
  const stored = await yap.invoke('note_get', { id });
  expect(stored.transcript[rooms]).toMatchObject({ speaker: 'p-1', speakerLocked: true });
  expect(stored.transcript.at(-1).speaker).toBe('them-2');

  // The action plan was written from the labelled lines.
  await expect.poll(() => fakeLlm.requests.filter((r) => r.kind === 'actionPlan').length, { timeout: 20_000 }).toBeGreaterThan(
    plans
  );
  const plan = fakeLlm.requests.filter((r) => r.kind === 'actionPlan').at(-1);
  expect(plan.user).toContain('Speaker 1: Thanks for joining, I will share the hiring plan');
  expect(plan.user).toContain('Sam: I can also book the interview rooms');
  expect(plan.user).toContain('Speaker 2: And I will send the offer letter template');
  expect(plan.user).toMatch(/Speakers besides You: Speaker 1 \(not named yet\); Sam; Speaker 2 \(not named yet\)/);
  await shot(pad, '03-labelled-at-the-end');
});

test('an engine that stops early leaves You and Them, and the notepad says so', async ({ yap, shot }) => {
  const { id, base } = await startMeeting(yap, 'Vendor call');
  const pad = yap.notepad;
  await say(
    yap,
    base,
    ['them', 'We can ship the first batch of laptops by the end of the month.', 0],
    ['you', 'That works, please send the invoice to accounts payable.', 7_000],
    ['them', 'Will do, and I will include the warranty details as well.', 14_000]
  );
  await expect(transcript(pad)).toContainText('warranty details');
  await labels(yap, [], 0, { done: true, error: "The speaker engine couldn't load its model." });
  await stopMeeting(yap);
  await tab(yap, 'Transcript').click();
  await expect(pad.getByRole('note').filter({ hasText: "Couldn't tell speakers apart this time." })).toBeVisible();
  expect(await groupNames(pad)).toEqual(['Them', 'You', 'Them']);
  const stored = await yap.invoke('note_get', { id });
  expect(stored.transcript.every((s) => !s.speaker)).toBe(true);
  expect(stored.speakers ?? {}).toEqual({});
  await shot(pad, '04-speakers-not-told-apart');
  expect(fs.readFileSync(yap.logPath, 'utf8')).toContain('Speakers: labels stopped early');
});

test('a 1:1 invite names the one voice, and not a second one who joins', async ({ yap, fakeLlm, shot }) => {
  const feed = await startCalendarFeed();
  const owner = { name: 'Imogen', email: 'imogen@example.com' };
  feed.set(
    calendarFile(owner.email, [
      vevent({
        uid: 'labels-one-to-one@e2e',
        summary: 'Alice and Imogen weekly',
        start: new Date(Date.now() - 60_000),
        end: new Date(Date.now() + 30 * 60_000),
        attendees: [{ name: 'Alice Moreau', email: 'alice@example.com' }, owner],
      }),
    ])
  );
  try {
    await yap.invoke('calendar_add_link', { link: feed.url, kind: 'ics' });
    const synced = await yap.invoke('calendar_sync');
    const event = synced.events.find((e) => e.title === 'Alice and Imogen weekly');
    const id = await yap.invoke('calendar_event', { key: event.key, action: 'start' });
    await expect.poll(() => visible(yap, 'notepad')).toBe(true);
    await expect.poll(() => recording(yap)).toBe(true);
    const pad = yap.notepad;
    await tab(yap, 'Transcript').click();
    clockTs += 600;
    const base = clockTs;
    await say(
      yap,
      base,
      ['them', 'Hi Imogen, the launch is going well so far.', 0],
      ['you', 'Great to hear, anything blocking you?', 6_000],
      ['them', 'I still need to send the slides to legal.', 10_000]
    );
    await expect(transcript(pad)).toContainText('slides to legal');
    expect(await groupNames(pad)).toEqual(['Alice Moreau?', 'You', 'Alice Moreau?']);

    // One voice so far: it's the invite's other person (still a guess).
    await labels(yap, [[0, 15_000, 1]], 20_000);
    await expect
      .poll(async () => (await yap.invoke('note_get', { id })).speakers['them-1'])
      .toMatchObject({ name: 'Alice Moreau', email: 'alice@example.com', source: 'calendar' });
    expect(await groupNames(pad)).toEqual(['Alice Moreau?', 'You', 'Alice Moreau?']);

    // Someone else joins: a second voice, and the guess stays with one.
    await say(yap, base, ['them', 'Sorry I am late, Bob here from legal.', 22_000]);
    await labels(yap, [[0, 15_000, 1], [22_000, 27_000, 2]], 30_000);
    await expect.poll(() => groupNames(pad)).toEqual(['Alice Moreau?', 'You', 'Alice Moreau?', 'Speaker 2']);
    await shot(pad, '05-one-to-one-guess-on-the-voice');

    await labels(yap, [[0, 15_000, 1], [22_000, 27_000, 2]], 30_000, { done: true });
    const plans = fakeLlm.requests.filter((r) => r.kind === 'actionPlan').length;
    await stopMeeting(yap);
    await expect.poll(() => fakeLlm.requests.filter((r) => r.kind === 'actionPlan').length, { timeout: 20_000 }).toBeGreaterThan(
      plans
    );
    const plan = fakeLlm.requests.filter((r) => r.kind === 'actionPlan').at(-1);
    expect(plan.user).toContain(
      'Speakers besides You: Alice Moreau (a guess: the only other person invited); Speaker 2 (not named yet)'
    );
    expect(plan.user).toContain('Alice Moreau: I still need to send the slides to legal.');
    expect(plan.user).toContain('Speaker 2: Sorry I am late, Bob here from legal.');
    const stored = await yap.invoke('note_get', { id });
    expect(stored.speakers['them-2']).toBeUndefined();
  } finally {
    if (await recording(yap)) await yap.invoke('meeting_pause');
    const s = await yap.invoke('calendar_status');
    for (const c of s.connections) await yap.invoke('calendar_disconnect', { id: c.id });
    await feed.close();
  }
});
