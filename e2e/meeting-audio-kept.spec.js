// Meeting audio kept on disk (src-tauri/src/meetings/meeting_audio.rs), end to end:
// - a recording's two sides written to Data/meeting-audio/<note>/ as it
//   records (FLAC, with a <recording>.json beside them), kept when it ends;
// - a transcript line's ▶ plays its stretch (`meeting_audio_clip`: both
//   sides, mixed, as a WAV);
// - "Transcribe again" (the notepad's ⋯, behind the confirm dialog) puts new
//   lines in place of the old — the stub engine's lines say how much audio
//   each chunk had, and a changed correction dictionary marks the new ones —
//   keeping My thoughts and a name the person put on a line;
// - deleting the note deletes its audio; Settings → Meetings says what's
//   kept and "Delete all meeting audio" clears it.
// No audio device is opened: the recorder plays you.wav / them.wav, 4× real
// time (shell/e2e.rs). Crash recovery is meeting-crash.spec.js.
import fs from 'node:fs';
import path from 'node:path';
import { test, expect, openView, openSettings, closeSettings, expectStore } from './support/fixtures.js';
import { newMeetingNote, meetingNote } from './support/meetings.js';
import { writeWav } from './support/wav.js';
import { RUNS } from './support/yap.js';

const audioDir = path.join(RUNS, 'meeting-audio-kept-wavs');

test.use({
  yapOptions: {
    name: 'meeting-audio-kept',
    env: { YAP_E2E_MEETING_AUDIO: audioDir, YAP_E2E_MEETING_SPEED: '4' },
    // "STT stub" is in every stub line: what it turns into says which pass made it.
    config: { dictionary: [{ from: 'STT stub', to: 'Heard', fuzzy: false }] },
  },
});

/** The next recording's audio: `seconds` of tone per side, silent in `you` / `them` ([[from, to], …]). */
function meetingAudio({ seconds, you, them }) {
  fs.mkdirSync(audioDir, { recursive: true });
  writeWav(path.join(audioDir, 'you.wav'), { seconds, gaps: you });
  writeWav(path.join(audioDir, 'them.wav'), { seconds, gaps: them });
}

const recording = (yap) => yap.invoke('meeting_state').then((s) => s.recording);
const logCount = (yap, text) => fs.readFileSync(yap.logPath, 'utf8').split(text).length - 1;
const PLAYED = 'e2e: the test audio has played to the end';
/** Seconds of audio a stub line says it received. */
const secsOf = (seg) => Number(/received ([\d.]+)s/.exec(seg.text)?.[1] ?? NaN);
const sum = (xs) => xs.reduce((a, b) => a + b, 0);

/** A note's audio folder, and what's in it. */
const audioFolder = (yap, noteId) => path.join(yap.dataDir, 'meeting-audio', String(noteId));
const filesOf = (yap, noteId) => (fs.existsSync(audioFolder(yap, noteId)) ? fs.readdirSync(audioFolder(yap, noteId)).sort() : []);

/** A new correction for "STT stub", adopted by Settings' copy too, then saved (read-aloud.spec.js's way). */
async function correctTo(yap, main, to) {
  const patch = { dictionary: [{ from: 'STT stub', to, fuzzy: false, snippet: false }] };
  const cfg = await yap.invoke('get_config');
  await yap.invoke('save_config', { cfg: { ...cfg, ...patch } });
  await main.evaluate((p) => window.dispatchEvent(new CustomEvent('yap-config-patched', { detail: p })), patch);
  // Settings' copy auto-saves what it adopted (400 ms later): settled.
  await main.waitForTimeout(700);
  await expectStore(yap, 'config.json', (c) => c.dictionary?.length === 1 && c.dictionary[0].to === to);
}

/** Record the note open in Notes until its test audio has played, then pause (no summary). */
async function recordAll(main, yap) {
  const before = logCount(yap, PLAYED);
  await main.getByRole('button', { name: 'Record' }).click();
  await expect
    .poll(() => logCount(yap, PLAYED), { message: 'the test audio to play out', timeout: 30_000 })
    .toBeGreaterThan(before);
  await main.getByRole('button', { name: 'Pause recording' }).click();
  await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
}

/** Pauses every ~5 s on the call side, where the recorder cuts. */
const THEM_PAUSES = [
  [4.5, 4.8],
  [9.5, 9.9],
  [14.2, 14.6],
  [19.5, 19.9],
];

test.afterEach(async ({ yap }) => {
  if (await recording(yap)) {
    await yap.invoke('meeting_pause');
    await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
  }
});

test('a meeting keeps its audio, plays a line, is transcribed again, and its audio goes with it', async ({ yap, main, shot }) => {
  meetingAudio({ seconds: 24, you: [[0, 1], [8, 13], [20, 24]], them: THEM_PAUSES });
  await newMeetingNote(main, 'Kept audio');
  await main.getByPlaceholder('Start writing…').fill('Ask about the budget');
  await recordAll(main, yap);

  // On disk: both sides as FLAC, and the recording's notes, ended and kept.
  await expectStore(yap, 'notes.json', (s) => meetingNote(s, 'Kept audio')?.content === 'Ask about the budget');
  const noteId = meetingNote(yap.readJson('notes.json'), 'Kept audio').id;
  await expect.poll(() => filesOf(yap, noteId).length).toBe(3);
  const files = filesOf(yap, noteId);
  const id = files.find((f) => f.endsWith('.json')).replace('.json', '');
  expect(files).toEqual([`${id}-them.flac`, `${id}-you.flac`, `${id}.json`]);
  for (const side of ['you', 'them']) {
    const bytes = fs.readFileSync(path.join(audioFolder(yap, noteId), `${id}-${side}.flac`));
    expect(bytes.subarray(0, 4).toString()).toBe('fLaC');
    // A quiet tone with pauses, losslessly: well under 16-bit WAV's 768 KB.
    expect(bytes.length).toBeLessThan(24 * 32_000 * 0.8);
  }
  await expect
    .poll(() => JSON.parse(fs.readFileSync(path.join(audioFolder(yap, noteId), `${id}.json`), 'utf8')).endedMs)
    .toBeTruthy();
  const meta = JSON.parse(fs.readFileSync(path.join(audioFolder(yap, noteId), `${id}.json`), 'utf8'));
  expect(meta.keep).toBe(true);
  expect(meta.youLen / 16_000).toBeCloseTo(24, 0);
  expect(Math.abs(meta.youLen - meta.themLen)).toBeLessThan(16_000);
  const first = meetingNote(yap.readJson('notes.json'), 'Kept audio').transcript;
  const secs = (t, source) => sum(t.filter((s) => s.source === source && s.text).map(secsOf));
  expect(secs(first, 'them')).toBeCloseTo(24, 0);
  expect(first.every((s) => !s.text || s.text.startsWith('[Heard: received'))).toBe(true);

  // ▶ on a line: its stretch of both sides, as a WAV (16-bit, 16 kHz),
  // padded a quarter second each side (as far as the recording goes).
  const line = first.find((s) => s.source === 'them' && s.startMs != null);
  const clipBytes = await main.evaluate(
    (args) => window.__TAURI_INTERNALS__.invoke('meeting_audio_clip', args).then((b) => b.byteLength),
    { noteId, ts: line.ts, startMs: line.startMs, endMs: line.endMs }
  );
  const from = Math.max(line.startMs - 250, 0) * 16;
  const samples = Math.min((line.endMs + 250) * 16, Math.max(meta.youLen, meta.themLen)) - from;
  expect(clipBytes).toBe(44 + 2 * samples);
  const log = main.getByRole('log', { name: 'Meeting transcript' });
  const bubble = log.locator('.bubble.them').first();
  await bubble.hover();
  await bubble.getByRole('button', { name: 'Play this line' }).click();
  await expect(bubble.getByRole('button', { name: 'Stop playing' })).toBeVisible();
  await shot(main, '01-play-a-line');
  await bubble.getByRole('button', { name: 'Stop playing' }).click();
  await expect(bubble.getByRole('button', { name: 'Play this line' })).toBeAttached();

  // A name the person put on a line, and a new spelling in the dictionary.
  const named = first.indexOf(line);
  await yap.invoke('speaker_rename_lines', { noteId, lines: [named], name: 'Morgan Lee' });
  await correctTo(yap, main, 'Again');

  // The notepad's ⋯ → Transcribe again, asked first.
  await yap.invoke('notepad_open', { noteId });
  const pad = yap.notepad;
  await pad.getByRole('tab', { name: /Transcript/ }).click();
  await pad.getByRole('button', { name: 'More', exact: true }).click();
  await pad.getByRole('menuitem', { name: 'Transcribe again' }).click();
  const confirm = pad.getByRole('alertdialog');
  await expect(confirm).toContainText('Transcribe “Kept audio” again?');
  await expect(confirm).toContainText('Your notes stay as they are');
  await expect(confirm).toContainText('The summary stays as it is until you generate it again.');
  await shot(pad, '02-transcribe-again-confirm');
  await confirm.getByRole('button', { name: 'Transcribe again' }).click();

  // The new lines, in place of the old: the same audio, the same length.
  await expectStore(
    yap,
    'notes.json',
    (s) => {
      const t = meetingNote(s, 'Kept audio')?.transcript || [];
      return t.some((seg) => seg.text.startsWith('[Again:')) && !t.some((seg) => seg.text.startsWith('[Heard:'));
    },
    'the meeting transcribed again'
  );
  const again = meetingNote(yap.readJson('notes.json'), 'Kept audio');
  expect(secs(again.transcript, 'them')).toBeCloseTo(24, 0);
  expect(secs(again.transcript, 'you')).toBeCloseTo(secs(first, 'you'), 0);
  // What's kept: the person's notes, and the name on the line that matches.
  expect(again.content).toBe('Ask about the budget');
  const match = again.transcript.find((s) => s.source === 'them' && s.speaker && s.speaker !== 'them');
  expect(match?.speakerLocked).toBe(true);
  expect(again.speakers[match.speaker].name).toBe('Morgan Lee');
  await expect(pad.getByRole('log', { name: 'Transcript' })).toContainText('[Again: received');
  await expect(pad.getByRole('log', { name: 'Transcript' })).toContainText('Morgan Lee');
  await shot(pad, '03-transcribed-again');
  await yap.invoke('notepad_open', { noteId }).catch(() => {});

  // Deleting the meeting deletes its audio.
  await yap.invoke('meeting_delete', { noteId });
  await expect.poll(() => fs.existsSync(audioFolder(yap, noteId))).toBe(false);
});

test('Settings says how much meeting audio is kept, and deletes it all', async ({ yap, main, shot }) => {
  meetingAudio({ seconds: 8, you: [[0, 8]], them: [[3, 3.4]] });
  await newMeetingNote(main, 'Short call');
  await recordAll(main, yap);
  const noteId = meetingNote(yap.readJson('notes.json'), 'Short call').id;
  await expect.poll(() => filesOf(yap, noteId).length).toBe(3);

  const dialog = await openSettings(main, 'Meetings');
  const keep = dialog.locator('[data-setting="meeting-audio"]');
  await expect(keep).toContainText('Keep meeting audio');
  await expect(dialog.locator('[data-setting="meeting-audio-days"] select')).toHaveValue('14');
  const clear = dialog.locator('[data-setting="clear-meeting-audio"]');
  await expect(clear).toContainText(/1 meeting · \d+ KB/);
  await clear.scrollIntoViewIfNeeded();
  await shot(main, '04-settings-meeting-audio');
  await clear.getByRole('button', { name: 'Delete all…' }).click();
  const confirm = main.getByRole('alertdialog');
  await expect(confirm).toContainText('Delete all meeting audio?');
  await confirm.getByRole('button', { name: 'Delete audio' }).click();
  await expect(clear).toContainText('Nothing kept right now');
  expect(filesOf(yap, noteId)).toEqual([]);
  // The meeting itself stays.
  expect(meetingNote(yap.readJson('notes.json'), 'Short call').transcript.length).toBeGreaterThan(0);
  await closeSettings(main);

  // Its line has nothing to play any more.
  await openView(main, 'Notes');
  const bubble = main.getByRole('log', { name: 'Meeting transcript' }).locator('.bubble').first();
  await bubble.hover();
  await expect(bubble.getByRole('button', { name: 'Play this line' })).toHaveCount(0);
});
