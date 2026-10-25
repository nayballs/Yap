// The speaker features that listen to a meeting's audio on disk
// (src-tauri/src/speakers/), on a recording that has some — a tone with
// pauses, played 4× real time (the stub engine's lines say how much audio
// each chunk had):
// - "Transcribe again" makes new lines; with "Tell speakers apart" on, the
//   speaker engine runs again over the same audio by itself (rerun.rs,
//   `after_transcribe_again`), so the new lines get voices too;
// - a voice's ▶ (clip.rs): a few seconds of its clearest speech, from its
//   speaker menu — the longest clear line, only its side, cut at 8 s.
// The engine's runs are staged (`e2e_find_speakers`), its live labels
// scripted (`e2e_diarize_timeline`). Find speakers from the menus, Merge
// and a resumed meeting's voices: speaker-voices.spec.js.
import fs from 'node:fs';
import path from 'node:path';
import { test, expect } from './support/fixtures.js';
import { writeWav } from './support/wav.js';
import { RUNS } from './support/yap.js';

const audioDir = path.join(RUNS, 'speaker-audio-wavs');

test.use({
  yapOptions: {
    name: 'speaker-audio',
    env: { YAP_E2E_MEETING_AUDIO: audioDir, YAP_E2E_MEETING_SPEED: '4' },
    config: { meetingReminder: 'never', meetingDiarization: true, meetingRefine: 'never' },
  },
});

const recording = (yap) => yap.invoke('meeting_state').then((s) => s.recording);
const logCount = (yap, text) => fs.readFileSync(yap.logPath, 'utf8').split(text).length - 1;
const PLAYED = 'e2e: the test audio has played to the end';
const job = (yap, id) => yap.invoke('meeting_audio_note', { noteId: id }).then((a) => `${a.job?.kind}:${a.job?.state}`);

/** Record a new meeting note from the tone until it has played; `labels`: the engine's turns, final at the stop. */
async function record(yap, title, labels = null) {
  fs.mkdirSync(audioDir, { recursive: true });
  writeWav(path.join(audioDir, 'you.wav'), { seconds: 24, gaps: [[0, 24]] });
  writeWav(path.join(audioDir, 'them.wav'), { seconds: 24, gaps: [[4.5, 4.8], [9.5, 9.9], [14.2, 14.6], [19.5, 19.9]] });
  const { id } = await yap.invoke('note_create', { title, folder: 'Meetings' });
  const before = logCount(yap, PLAYED);
  await yap.invoke('meeting_start', { noteId: id });
  await expect.poll(() => recording(yap)).toBe(true);
  if (labels) {
    const segments = labels.map(([startMs, endMs, speaker]) => ({ startMs, endMs, speaker }));
    await yap.invoke('e2e_diarize_timeline', { segments, labelledMs: 24_000, done: true });
  }
  await expect.poll(() => logCount(yap, PLAYED), { timeout: 30_000 }).toBeGreaterThan(before);
  await yap.invoke('meeting_pause');
  await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
  return id;
}

test('Transcribe again is followed by Find speakers, which puts voices on the new lines', async ({ yap }) => {
  const id = await record(yap, 'Stand-up');
  const first = await yap.invoke('note_get', { id });
  const them = first.transcript.filter((s) => s.source === 'them' && s.text);
  expect(them.length).toBeGreaterThan(0);
  expect(them.every((s) => !s.speaker), 'no speaker engine while it recorded').toBe(true);

  // The engine, run again over the audio: one voice to 12 s, another after.
  await yap.invoke('e2e_find_speakers', {
    noteId: id,
    recordings: [[{ startMs: 0, endMs: 12_000, speaker: 1 }, { startMs: 12_000, endMs: 24_000, speaker: 2 }]],
    paceMs: 500,
  });
  await yap.invoke('meeting_transcribe_again', { noteId: id });
  // Transcribed again, then the speakers found by themselves.
  await expect.poll(() => job(yap, id), { timeout: 30_000 }).toBe('speakers:done');
  const after = await yap.invoke('note_get', { id });
  const lines = after.transcript.filter((s) => s.source === 'them' && s.text);
  expect(lines.length).toBeGreaterThan(0);
  expect(lines.every((s) => s.speaker === 'them-1' || s.speaker === 'them-2')).toBe(true);
  expect(lines.filter((s) => s.endMs <= 12_000).every((s) => s.speaker === 'them-1')).toBe(true);
  expect(lines.filter((s) => s.startMs >= 12_000).every((s) => s.speaker === 'them-2')).toBe(true);
});

test("a voice's ▶ plays its clearest few seconds, from its menu", async ({ yap, main }) => {
  const id = await record(yap, 'Supplier call', [[0, 24_000, 1]]);
  await expect.poll(async () => (await yap.invoke('note_get', { id })).transcript.filter((s) => s.speaker === 'them-1').length).toBeGreaterThan(0);

  // The clip: the longest of the voice's lines (none over another voice
  // here), only the call's side, padded 150 ms either side.
  const n = await yap.invoke('note_get', { id });
  const lines = n.transcript.filter((s) => s.speaker === 'them-1');
  const longest = lines.reduce((a, b) => (Math.min(b.endMs - b.startMs, 8_000) > Math.min(a.endMs - a.startMs, 8_000) ? b : a));
  const bytes = await main.evaluate(
    (args) => window.__TAURI_INTERNALS__.invoke('speaker_clip', args).then((b) => b.byteLength),
    { noteId: id, speaker: 'them-1' }
  );
  const cut = Math.min(longest.endMs, longest.startMs + 8_000) + 150;
  const from = Math.max(longest.startMs - 150, 0);
  // (At the recording's very end the padding can run out.)
  expect(bytes).toBeLessThanOrEqual(44 + 2 * (cut - from) * 16);
  expect(bytes).toBeGreaterThan(44 + 2 * (cut - from - 300) * 16);
  // Not the whole meeting, not both sides: one line's stretch.
  expect(bytes).toBeLessThan(44 + 2 * 9_000 * 16);

  // From the speaker's menu in the notepad.
  await yap.invoke('notepad_open', { noteId: id });
  const pad = yap.notepad;
  await pad.getByRole('tab', { name: /Transcript/ }).click();
  const log = pad.getByRole('log', { name: 'Transcript' });
  await log.locator(':scope > .group .who').first().click();
  const menu = pad.getByRole('menu', { name: /Speaker: Speaker 1/ });
  await menu.getByRole('menuitem', { name: 'Play their voice' }).click();
  await expect(menu).toHaveCount(0);
  await pad.waitForTimeout(500);
  await expect(pad.getByText("Couldn't play their voice")).toHaveCount(0);
});
