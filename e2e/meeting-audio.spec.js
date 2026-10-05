// The meeting recorder's audio, end to end (src-tauri/src/meeting.rs):
// - a hotkey dictation mid-meeting is kept out of "You" (silence instead of
//   the words, from its pre-roll on) and leaves a "You dictated here" marker
//   that summaries skip;
// - meeting lines (and Upload) get the correction dictionary, as dictation
//   does (pipeline::apply_corrections);
// - Windows' default output changing mid-meeting: "Them" follows it, the gap
//   kept as silence so both sides stay in step;
// - one side quiet for a while as the other talks: Yap says so, and the
//   recording carries on.
// No audio device is opened: the recorder plays you.wav / them.wav, 4× real
// time (src-tauri/src/e2e.rs), whose debug-only hooks stage a dictation
// (`e2e_meeting_dictation`, the pipeline's own dictation signal over a
// stretch of that audio) and a device switch (`e2e_meeting_output_change`),
// and shorten the quiet-side timings (`e2e_meeting_quiet`). The stub engine
// transcribes a chunk as "[STT stub: received 15.6s of audio, …]", so "You"
// talks only where a test needs a line, and a chunk that's all silence gives
// none.
import fs from 'node:fs';
import path from 'node:path';
import { test, expect, openView, expectStore, closeToasts } from './support/fixtures.js';
import { newMeetingNote, meetingNote } from './support/meetings.js';
import { writeWav } from './support/wav.js';
import { RUNS } from './support/yap.js';

const audioDir = path.join(RUNS, 'meeting-audio-wavs');

test.use({
  yapOptions: {
    name: 'meeting-audio',
    env: { YAP_E2E_MEETING_AUDIO: audioDir, YAP_E2E_MEETING_SPEED: '4' },
    config: {
      // "STT stub" is in every stub transcript: an exact correction anyone
      // can see. The other two are for lines handed over as if transcribed:
      // a near-miss ("jaisen") the fuzzy pass catches, and an exact one.
      dictionary: [
        { from: 'STT stub', to: 'Corrected', fuzzy: true },
        { from: 'jaison', to: 'JSON', fuzzy: true },
        { from: 'chat gpt', to: 'ChatGPT', fuzzy: true },
      ],
    },
  },
});

/** The next recording's audio: `seconds` of tone per side, silent in `you` / `them` ([[from, to], …]). */
function meetingAudio({ seconds, you, them }) {
  fs.mkdirSync(audioDir, { recursive: true });
  writeWav(path.join(audioDir, 'you.wav'), { seconds, gaps: you });
  writeWav(path.join(audioDir, 'them.wav'), { seconds, gaps: them });
}

/** Pauses every ~5 s on the call side, where the recorder cuts. */
const THEM_PAUSES = [
  [4.5, 4.8],
  [9.5, 9.9],
  [14.2, 14.6],
  [19.5, 19.9],
  [24.5, 24.9],
  [29.4, 29.8],
];

const recording = (yap) => yap.invoke('meeting_state').then((s) => s.recording);
const transcriptLog = (main) => main.getByRole('log', { name: 'Meeting transcript' });
/** Seconds of audio a stub line says it received. */
const secsOf = (seg) => Number(/received ([\d.]+)s/.exec(seg.text)?.[1] ?? NaN);
const sum = (xs) => xs.reduce((a, b) => a + b, 0);

/** How often the instance's log says `text` so far. */
const logCount = (yap, text) => fs.readFileSync(yap.logPath, 'utf8').split(text).length - 1;
const PLAYED = 'e2e: the test audio has played to the end';

/** Record, and a function that waits until this recording's test audio has played out. */
async function record(main, yap, button = 'Record') {
  const before = logCount(yap, PLAYED);
  await main.getByRole('button', { name: button }).click();
  return () =>
    expect
      .poll(() => logCount(yap, PLAYED), { message: 'the test audio to play out', timeout: 30_000 })
      .toBeGreaterThan(before);
}

/** Pause (no summary) and wait for the recorder's last chunk. */
async function pause(main, yap) {
  await main.getByRole('button', { name: 'Pause recording' }).click();
  await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
}

test.afterEach(async ({ yap }) => {
  if (await recording(yap)) {
    await yap.invoke('meeting_pause');
    await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
  }
  await yap.invoke('e2e_meeting_quiet', {});
});

test('a dictation mid-meeting stays out of "You" and leaves a marker', async ({ yap, main, shot }) => {
  // You talk to the meeting at 1–9 s, then dictate a reply somewhere else
  // over 18–32 s (talking at 21–30 s); the call talks throughout.
  meetingAudio({ seconds: 35, you: [[0, 1], [9, 21], [30, 35]], them: THEM_PAUSES });
  await yap.invoke('e2e_meeting_dictation', { from: 18, to: 32 });
  await newMeetingNote(main, 'Dictated mid-call');
  const playedOut = await record(main, yap);

  // The stretch with the dictation is cut off in the middle of it and turns
  // out silent: no line for it, and a marker where it began.
  const log = transcriptLog(main);
  const marker = log.locator('.dictated');
  await expect(marker).toHaveCount(1, { timeout: 20_000 });
  await expect(marker).toHaveText('You dictated here · left out of the notes');
  await expect(log.locator('.bubble.you')).toHaveCount(1);
  await expect(log.locator('.bubble.them').first()).toBeVisible();
  await shot(main, '01-dictation-marker');

  await playedOut();
  await pause(main, yap);
  // Still one "You" line (the meeting talk at 1–9 s); the call is all there.
  await expect(log.locator('.bubble.you')).toHaveCount(1);
  await expect(marker).toHaveCount(1);
  await expectStore(yap, 'notes.json', (s) =>
    meetingNote(s, 'Dictated mid-call')?.transcript?.some((seg) => seg.dictated)
  );
  const t = meetingNote(yap.readJson('notes.json'), 'Dictated mid-call').transcript;
  const you = t.filter((seg) => seg.source === 'you' && !seg.dictated);
  const marks = t.filter((seg) => seg.dictated);
  const them = t.filter((seg) => seg.source === 'them');
  expect(you).toHaveLength(1);
  expect(marks).toEqual([{ source: 'you', text: '', ts: marks[0].ts, dictated: true }]);
  // Dated where the dictation began (its pre-roll), between the lines around
  // it; the call carried on through it.
  expect(marks[0].ts).toBeGreaterThanOrEqual(you[0].ts);
  expect(them.some((seg) => seg.ts > marks[0].ts)).toBe(true);
  expect(t.map((seg) => seg.ts)).toEqual([...t.map((seg) => seg.ts)].sort((a, b) => a - b));
  await shot(main, '02-dictation-marker-after-pause');
});

test('meeting lines and an uploaded file get the correction dictionary', async ({ yap, main, shot }) => {
  meetingAudio({
    seconds: 20,
    you: [[3, 3.3], [8, 8.3], [16, 16.4]],
    them: [[5.5, 5.8], [11, 11.3], [16.8, 17.2]],
  });
  await newMeetingNote(main, 'Corrected meeting');
  await main.getByRole('button', { name: 'Record' }).click();

  // Transcribed audio: the stub's "STT stub" comes out corrected, both sides.
  const log = transcriptLog(main);
  await expect(log.locator('.bubble.you')).toHaveCount(1, { timeout: 20_000 });
  await expect(log.locator('.bubble.them')).toHaveCount(1);
  await expect(log.locator('.bubble.you')).toContainText(/\[Corrected: received 1\d\.\ds of audio/);
  await expect(log.locator('.bubble.them')).toContainText('[Corrected: received');
  await expect(log).not.toContainText('STT stub');

  // Lines handed over as if just transcribed: a near-miss and an exact one.
  const ts = Math.floor(Date.now() / 1000);
  await yap.invoke('e2e_meeting_feed', {
    segments: [
      { source: 'them', text: 'Can you send the jaisen file over?', ts },
      { source: 'you', text: 'Sure, and I will ask chat GPT to check it.', ts: ts + 5 },
    ],
  });
  await expect(log.locator('.bubble.them').last()).toContainText('Can you send the JSON file over?');
  await expect(log.locator('.bubble.you').last()).toContainText('Sure, and I will ask ChatGPT to check it.');
  await shot(main, '03-meeting-lines-corrected');

  await pause(main, yap);
  const t = meetingNote(yap.readJson('notes.json'), 'Corrected meeting').transcript;
  expect(t.map((seg) => seg.text).join('\n')).not.toMatch(/STT stub|jaisen|chat GPT/);
  expect(t.filter((seg) => seg.text.startsWith('[Corrected: received')).length).toBeGreaterThanOrEqual(2);

  // Upload: the whole file's text, corrected the same way.
  await openView(main, 'Upload');
  const wav = writeWav(path.join(yap.runDir, 'voice-memo.wav'));
  // CDP can't drag a file in from Explorer (see no-mic.spec.js).
  await expect(async () => {
    await yap.invoke('plugin:event|emit', {
      event: 'tauri://drag-drop',
      payload: { paths: [wav], position: { x: 600, y: 400 } },
    });
    await expect(main.getByText('voice-memo.wav')).toBeVisible({ timeout: 1_000 });
  }).toPass();
  await main.getByRole('button', { name: 'Transcribe', exact: true }).click();
  await expect(main.getByRole('heading', { name: 'Transcript' })).toBeVisible({ timeout: 20_000 });
  await expect(main.getByText('[Corrected: received 1.5s of audio')).toBeVisible();
  // History keeps what was heard and what came out.
  await expectStore(yap, 'history.json', (h) =>
    JSON.stringify(h).includes('[Corrected: received 1.5s of audio')
  );
  const entry = yap.readJson('history.json').find((e) => e.app === 'voice-memo.wav');
  expect(entry.raw).toContain('[STT stub: received 1.5s of audio');
  expect(entry.text).toContain('[Corrected: received 1.5s of audio');
  await shot(main, '04-upload-corrected');
});

test('a switch of output device mid-meeting keeps "Them" going, in step', async ({ yap, main, shot }) => {
  // Two people taking turns (pauses that never line up, so the echo check
  // keeps "You"), each pausing late enough that the last chunk is a few
  // seconds long and gets transcribed.
  meetingAudio({
    seconds: 22,
    you: [[3, 3.3], [11, 11.3], [18, 18.4]],
    them: [[5.5, 5.8], [12.5, 12.8], [18.6, 19]],
  });
  // Windows' default output changes 6 s in: "Them" hears nothing for a
  // moment, then follows the new device.
  await yap.invoke('e2e_meeting_output_change', { at: 6 });
  await newMeetingNote(main, 'Headset plugged in');
  const playedOut = await record(main, yap);

  await expect
    .poll(() => logCount(yap, '"Them" now follows it'), {
      message: 'the recorder followed the new output device',
    })
    .toBe(1);
  // The recording carries on, on both sides.
  const log = transcriptLog(main);
  await expect(log.locator('.bubble.them')).toHaveCount(1, { timeout: 20_000 });
  await expect(log.locator('.bubble.you')).toHaveCount(1);
  expect(await recording(yap)).toBe(true);
  await expect(main.getByRole('button', { name: 'End meeting & summarise' })).toBeVisible();
  await shot(main, '05-output-switch-recording-goes-on');

  await playedOut();
  await pause(main, yap);
  // The gap was kept as silence: "Them" is as long as "You", 22 s each
  // (without it, the call would come up over a second short and run early).
  const t = meetingNote(yap.readJson('notes.json'), 'Headset plugged in').transcript;
  expect(t.some((seg) => seg.echo)).toBe(false);
  const secs = (source) => sum(t.filter((seg) => seg.source === source).map(secsOf));
  expect(secs('you')).toBeCloseTo(22, 0);
  expect(secs('them')).toBeCloseTo(22, 0);
  expect(Math.abs(secs('them') - secs('you'))).toBeLessThanOrEqual(0.2);
});

test('a side gone quiet while the other talks says so, and the recording carries on', async ({
  yap,
  main,
  shot,
}) => {
  // 6 s of the meeting instead of 3 minutes (2 s of talk instead of 10).
  await yap.invoke('e2e_meeting_quiet', { quietSecs: 6, talkSecs: 2 });
  const warning = main.getByRole('status').filter({ hasText: 'Meeting recording' });

  // The call never comes through (Teams on a headset that isn't the
  // default output), while you talk.
  meetingAudio({ seconds: 20, you: [[16, 16.4]], them: [[0, 20]] });
  await newMeetingNote(main, 'Call on the headset');
  await main.getByRole('button', { name: 'Record' }).click();
  await expect(warning).toContainText(
    "Yap hasn't heard the call for 6 seconds. If your call plays through a headset, make it Windows' default output device.",
    { timeout: 15_000 }
  );
  await shot(main, '06-quiet-call-warning');
  // Said once, and the recording goes on.
  await expect(transcriptLog(main).locator('.bubble.you')).toHaveCount(1, { timeout: 20_000 });
  expect(await recording(yap)).toBe(true);
  await expect(warning).toHaveCount(1);
  await pause(main, yap);
  await closeToasts(main);

  // The other way round: the call talks, the mic hears nothing.
  meetingAudio({ seconds: 20, you: [[0, 20]], them: [[16.8, 17.2]] });
  await main.getByRole('button', { name: 'Resume' }).click();
  await expect(warning).toContainText("Yap hasn't heard your mic for 6 seconds.", { timeout: 15_000 });
  await expect(warning).toContainText('Settings → General');
  await shot(main, '07-quiet-mic-warning');
  await expect(transcriptLog(main).locator('.bubble.them')).toHaveCount(1, { timeout: 20_000 });
  expect(await recording(yap)).toBe(true);
});
