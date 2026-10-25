// The speaker engine's voices after the fact (src-tauri/src/speakers/):
// - **Find speakers** (rerun.rs): the engine run again over a past
//   meeting's audio from the notepad's ⋯ and the Meetings view's menu, with
//   progress and Cancel on the job line; only a run that completes changes
//   the labels, names the person gave stay on their voices (matched by the
//   lines they share), and a line they said who said keeps its speaker;
// - a voice's ▶ on its card (clip.rs; the clip itself, from audio, is
//   speaker-audio.spec.js's);
// - "These may be one voice · Merge" (flags.rs): two voices that sound
//   alike, offered after the recording, never merged by themselves;
// - a resumed meeting's voice matched with the same voice before the pause
//   (labels.rs, voiceprints), so it isn't a new "Speaker 3".
//
// No model, no real voices, no audio: lines are handed to the recorder
// with `e2e_meeting_feed` (dated as if 8× real time, the player's pace), the
// live engine's labels are scripted (`e2e_diarize_timeline`), a Find
// speakers run is staged (`e2e_find_speakers`: its turns and how long it
// takes), and what stretches of a recording sound like are staged as
// voiceprints (`e2e_voice_prints`). The real engine and model:
// `cargo test --features engines speakers::voices::real` (speakers.md).
import fs from 'node:fs';
import path from 'node:path';
import { test, expect, openView, closeToasts } from './support/fixtures.js';

test.use({
  yapOptions: {
    name: 'speaker-voices',
    env: { YAP_E2E_MEETING_SPEED: '8' },
    config: { meetingReminder: 'never', meetingDiarization: true, meetingRefine: 'never' },
  },
});

const visible = (yap, label) => yap.invoke('plugin:window|is_visible', { label });
const recording = (yap) => yap.invoke('meeting_state').then((s) => s.recording);
const tab = (yap, name) => yap.notepad.getByRole('tab', { name });
const transcript = (pad) => pad.getByRole('log', { name: 'Transcript' });
const groupNames = (pad) => transcript(pad).locator(':scope > .group .who').allTextContents();
const note = (yap, id) => yap.invoke('note_get', { id });
/** Each line's voice as shown: "Speaker 1: Morning…". */
const shown = (n) =>
  n.transcript
    .filter((s) => s.text)
    .map((s) => {
      let id = s.speaker || s.source;
      for (let i = 0; i < 8 && n.speakers?.[id]?.mergedInto; i += 1) id = n.speakers[id].mergedInto;
      const named = n.speakers?.[id]?.name;
      const label = id === 'you' ? 'You' : named || (/^them-(\d+)$/.exec(id) ? `Speaker ${id.slice(5)}` : 'Them');
      return `${label}: ${s.text}`;
    });

/** The recordings of note `id` on disk, oldest first: `{ recording, startedMs }`. */
function recordingsOf(yap, id) {
  const dir = path.join(yap.dataDir, 'meeting-audio', String(id));
  if (!fs.existsSync(dir)) return [];
  return fs
    .readdirSync(dir)
    .filter((f) => f.endsWith('.json'))
    .map((f) => JSON.parse(fs.readFileSync(path.join(dir, f), 'utf8')))
    .sort((a, b) => a.startedMs - b.startedMs);
}

/**
 * A line on the recording's clock: a word every 500 ms from `startMs`,
 * dated from the recording's start at the player's 8× pace (so a line is
 * dated within its recording, before a resume, as a real one is).
 */
function placed(base, source, text, startMs) {
  const n = text.split(/\s+/).filter(Boolean).length;
  return {
    source,
    text,
    ts: base + Math.floor(startMs / 8_000),
    startMs,
    endMs: startMs + (n - 1) * 500 + 400,
    words: Array.from({ length: n }, (_, k) => [k * 500, k * 500 + 400]),
  };
}

/** Start recording into a new meeting note; resolves when its audio is on disk: `{ id, base }`. */
async function startMeeting(yap, title, id = null) {
  if (id == null) id = (await yap.invoke('note_create', { title, folder: 'Meetings' })).id;
  const before = recordingsOf(yap, id).length;
  await yap.invoke('meeting_start', { noteId: id });
  await expect.poll(() => recording(yap)).toBe(true);
  await expect.poll(() => recordingsOf(yap, id).length).toBe(before + 1);
  await expect.poll(() => visible(yap, 'notepad')).toBe(true);
  // Its clock's start, once the capture is up (the meta is rewritten then).
  await new Promise((r) => setTimeout(r, 500));
  const rec = recordingsOf(yap, id).at(-1);
  return { id, base: Math.ceil(rec.startedMs / 1000), index: before };
}

async function say(yap, base, ...lines) {
  const segments = lines.map(([source, text, startMs]) => placed(base, source, text, startMs));
  expect(await yap.invoke('e2e_meeting_feed', { segments })).toBe(segments.length);
}

/** Scripted engine labels: `[startMs, endMs, speaker]`. */
async function labels(yap, turns, labelledMs, extra = {}) {
  const segments = turns.map(([startMs, endMs, speaker]) => ({ startMs, endMs, speaker }));
  await yap.invoke('e2e_diarize_timeline', { segments, labelledMs, done: false, ...extra });
}

/** Stop once the recording has `secs` of audio (8× real time). */
async function stopAfter(yap, secs) {
  await expect.poll(() => yap.invoke('meeting_state').then((s) => (s.elapsedSecs || 0) * 8), { timeout: 20_000 }).toBeGreaterThan(secs);
  await yap.invoke('meeting_pause');
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

const BUDGET = [
  ['them', 'Morning everyone the budget numbers are finally in', 0],
  ['them', 'Revenue is up eight percent on last quarter', 6_000],
  ['you', 'Great what about the hiring costs this time', 12_000],
  ['them', 'Hiring costs stayed flat apart from the contractors', 17_000],
  ['them', 'I can send the list today', 24_000],
  ['them', 'And I will book the review for next Tuesday', 31_000],
];

test('Find speakers runs the engine again: progress, Cancel, and names that stay', async ({ yap, main, shot }) => {
  const { id, base } = await startMeeting(yap, 'Budget review');
  await say(yap, base, ...BUDGET);
  // The live engine: Speaker 1 said the first two lines, Speaker 2 the rest.
  await labels(yap, [[0, 10_500, 1], [17_000, 36_000, 2]], 40_000, { done: true });
  await stopAfter(yap, 40);
  await expect.poll(async () => shown(await note(yap, id))).toEqual([
    'Speaker 1: Morning everyone the budget numbers are finally in',
    'Speaker 1: Revenue is up eight percent on last quarter',
    'You: Great what about the hiring costs this time',
    'Speaker 2: Hiring costs stayed flat apart from the contractors',
    'Speaker 2: I can send the list today',
    'Speaker 2: And I will book the review for next Tuesday',
  ]);
  // The person names Speaker 1, and says who said the last line.
  await yap.invoke('speaker_rename', { noteId: id, speaker: 'them-1', name: 'Priya Shah' });
  const last = (await note(yap, id)).transcript.findIndex((s) => s.text.startsWith('And I will book'));
  await yap.invoke('speaker_rename_lines', { noteId: id, lines: [last], name: 'Sam Patel' });

  // ⋯ → Find speakers: a slow run, its progress on the job line, cancelled.
  const run = [
    // The engine numbers by first heard; this time the contractor voice
    // came first, Priya second, and the contractor list is a third voice.
    { startMs: 0, endMs: 10_500, speaker: 2 },
    { startMs: 17_000, endMs: 22_000, speaker: 1 },
    { startMs: 24_000, endMs: 27_000, speaker: 3 },
    { startMs: 31_000, endMs: 36_000, speaker: 1 },
  ];
  await yap.invoke('e2e_find_speakers', { noteId: id, recordings: [run], paceMs: 8_000 });
  await yap.invoke('notepad_open', { noteId: id });
  const pad = yap.notepad;
  await tab(yap, 'Transcript').click();
  await pad.getByRole('button', { name: 'More', exact: true }).click();
  await pad.getByRole('menuitem', { name: 'Find speakers' }).click();
  const job = pad.getByRole('status').filter({ hasText: 'Finding speakers' });
  await expect(job).toContainText(/Finding speakers · \d+%/);
  await shot(pad, '01-finding-speakers');
  const before = shown(await note(yap, id));
  await job.getByRole('button', { name: 'Cancel' }).click();
  await expect(job).toHaveCount(0, { timeout: 10_000 });
  await expect.poll(() => yap.invoke('meeting_audio_note', { noteId: id }).then((a) => a.job?.state)).toBe('cancelled');
  expect(shown(await note(yap, id))).toEqual(before);

  // Again from the Meetings view's menu, to the end this time.
  await yap.invoke('e2e_find_speakers', { noteId: id, recordings: [run], paceMs: 1_500 });
  await openView(main, 'Meetings');
  await main.getByRole('button', { name: 'More for Budget review' }).click();
  await main.getByRole('menu').first().getByRole('menuitem', { name: 'Find speakers' }).click();
  await expect.poll(() => yap.invoke('meeting_audio_note', { noteId: id }).then((a) => a.job?.state), { timeout: 15_000 }).toBe('done');
  const after = await note(yap, id);
  expect(shown(after)).toEqual([
    // Priya's lines are the new run's Speaker 2: her name stays on them.
    'Priya Shah: Morning everyone the budget numbers are finally in',
    'Priya Shah: Revenue is up eight percent on last quarter',
    'You: Great what about the hiring costs this time',
    'Speaker 2: Hiring costs stayed flat apart from the contractors',
    // A voice the live engine missed: a new number.
    'Speaker 3: I can send the list today',
    // The person said who said this one: it stays theirs.
    'Sam Patel: And I will book the review for next Tuesday',
  ]);
  expect(after.transcript.find((s) => s.text.startsWith('And I will book'))).toMatchObject({ speakerLocked: true });
  await expect(pad.getByRole('log', { name: 'Transcript' })).toContainText('Speaker 3');
  await shot(pad, '02-speakers-found');
});

test('two voices that sound alike are offered as one, and merged only when asked', async ({ yap, shot }) => {
  const { id, base, index } = await startMeeting(yap, 'Design sync');
  const lines = [
    ['them', 'Let us start with the new onboarding screens today', 0],
    ['them', 'I tested them with five people on Monday and Tuesday', 5_000],
    ['them', 'Most of them finished the setup in under two minutes', 10_000],
    ['them', 'The settings page still confuses people quite a lot though', 15_000],
    ['them', 'We could move the advanced options behind a single button', 20_000],
    ['them', 'I like that and we should test it again soon', 25_000],
    ['them', 'Then the onboarding flow is ready for the beta group', 30_000],
    ['them', 'And I will write up the notes for everyone tonight', 35_000],
    ['them', 'Great work everyone see you all on Thursday morning', 40_000],
  ];
  await say(yap, base, ...lines);
  // The live engine: three voices — but its Speaker 3 is Speaker 1 again
  // (an engine restart's new number, say).
  const who = [1, 1, 1, 2, 2, 2, 3, 3, 3];
  await labels(
    yap,
    lines.map(([, , start], i) => [start, start + 4_900, who[i]]),
    50_000,
    { done: true }
  );
  // How they sound: 1 and 3 alike, 2 apart.
  const sound = { 1: [1, 0, 0], 2: [0, 1, 0], 3: [0.97, 0.12, 0] };
  await yap.invoke('e2e_voice_prints', {
    prints: lines.map(([, , start], i) => ({ noteId: id, recording: index, side: 'them', startMs: start, endMs: start + 4_900, print: sound[who[i]] })),
  });
  await stopAfter(yap, 50);

  // Looked for after the recording: the card, before any name card.
  await yap.invoke('notepad_open', { noteId: id });
  const pad = yap.notepad;
  await tab(yap, 'Transcript').click();
  const card = pad.getByRole('region', { name: 'Speakers that may be wrong' });
  await expect(card).toContainText('Speaker 1 and Speaker 3 may be one voice', { timeout: 20_000 });
  await expect(card.getByRole('button', { name: "Play Speaker 1's voice" })).toBeVisible();
  await expect(card.getByRole('button', { name: "Play Speaker 3's voice" })).toBeVisible();
  // Offered, not done.
  expect(await groupNames(pad)).toEqual(['Speaker 1', 'Speaker 2', 'Speaker 3']);
  await shot(pad, '03-may-be-one-voice');

  await card.getByRole('button', { name: 'Merge' }).click();
  await expect.poll(() => groupNames(pad)).toEqual(['Speaker 1', 'Speaker 2', 'Speaker 1']);
  await expect(card).toHaveCount(0);
  const n = await note(yap, id);
  expect(n.speakers['them-3'].mergedInto).toBe('them-1');
  await shot(pad, '04-merged');
});

test("a resumed meeting's voice is the same voice as before the pause", async ({ yap }) => {
  const first = await startMeeting(yap, 'Weekly planning');
  await say(
    yap,
    first.base,
    ['them', 'First item is the release date for the mobile app', 0],
    ['them', 'Marketing needs the final screenshots by Friday please', 6_000],
    ['them', 'I can have the screenshots ready by Thursday afternoon for sure', 12_000],
    ['them', 'Yes and I will put them in the team channel too', 18_000],
    ['them', 'Perfect then we keep the release date as planned', 24_000]
  );
  await labels(yap, [[0, 10_500, 1], [12_000, 22_900, 2], [24_000, 28_500, 1]], 30_000, { done: true });
  await stopAfter(yap, 48);
  await expect.poll(async () => shown(await note(yap, first.id)).map((l) => l.split(':')[0])).toEqual([
    'Speaker 1',
    'Speaker 1',
    'Speaker 2',
    'Speaker 2',
    'Speaker 1',
  ]);

  // Resumed: a new recording, a new engine — whose Speaker 1 is the
  // earlier Speaker 2, by the sound of it.
  const second = await startMeeting(yap, 'Weekly planning', first.id);
  expect(second.index).toBe(1);
  await yap.invoke('e2e_voice_prints', {
    prints: [
      { noteId: first.id, recording: 0, startMs: 0, endMs: 10_500, print: [1, 0, 0] },
      { noteId: first.id, recording: 0, startMs: 12_000, endMs: 22_900, print: [0, 1, 0] },
      { noteId: first.id, recording: 0, startMs: 24_000, endMs: 28_500, print: [1, 0, 0] },
      { noteId: first.id, recording: 1, startMs: 0, endMs: 9_000, print: [0.08, 0.98, 0.05] },
    ],
  });
  await say(
    yap,
    second.base,
    ['them', 'Sorry I dropped off what did I miss after the screenshots', 0],
    ['them', 'Ah great then I will send them over on Thursday', 5_000]
  );
  // Live labels (not the end yet): matched as they come.
  await labels(yap, [[0, 9_000, 1]], 12_000);
  await expect.poll(async () => shown(await note(yap, first.id)).slice(5).map((l) => l.split(':')[0]), { timeout: 15_000 }).toEqual([
    'Speaker 2',
    'Speaker 2',
  ]);
  const n = await note(yap, first.id);
  expect(n.transcript.slice(5).map((s) => s.speaker)).toEqual(['them-2', 'them-2']);
  expect(Object.keys(n.speakers || {})).not.toContain('them-3');
  await labels(yap, [[0, 9_000, 1]], 12_000, { done: true });
  await stopAfter(yap, 12);
});
