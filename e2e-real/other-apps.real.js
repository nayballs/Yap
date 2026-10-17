// Other apps' sound while you dictate (src-tauri/src/mute.rs,
// media_pause.rs): "Silence other apps while you dictate" turns other
// apps' audio sessions down once the start chime has played and brings
// each back to exactly its level after the stop chime; "Pause music and
// videos while you dictate" pauses what's playing in Windows' media
// controls and plays it again afterwards.
//
// Only the test's own sound is ever touched: robot mode lets Yap change the
// audio session of a process the test started (the test pad, here playing
// a near-silent tone on a loop) or of one whose windows all carry the run's
// nonce, and pause only media titled with the nonce (an Edge app window
// with a throwaway profile; `realtest::audio_permits` / `media_permits`).
// Your own music, calls and browser are never lowered or paused, and
// `realtest_other_apps` only ever reports the test's own sessions.
import fs from 'node:fs';
import path from 'node:path';
import { test, expect } from './support/robot.js';
import { speechWav } from './support/tts.js';
import { CACHE } from './support/targets.js';
import { writeWav } from '../e2e/support/wav.js';

/** A near-silent 440 Hz tone (about −58 dBFS), a minute long, to loop. */
function quietTone() {
  const file = path.join(CACHE, 'tone-quiet-60s.wav');
  if (!fs.existsSync(file)) {
    fs.mkdirSync(path.dirname(file), { recursive: true });
    writeWav(file, { seconds: 60, amplitude: 40 });
  }
  return file;
}

/** The test pad's audio session as Yap sees it (its level), or undefined. */
async function padLevel(robot) {
  const view = await robot.otherApps();
  return view.sessions.find((s) => s.pid === robot.pad.pid)?.level;
}

test.afterEach(async ({ robot }) => {
  await robot.configure({ quietOtherApps: false, pauseMedia: false });
  await robot.pad.stopPlaying().catch(() => {});
});

test('Silence other apps: the test pad goes quiet after the start chime, then back to exactly its level', async ({ robot }) => {
  const { pad } = robot;
  await robot.configure({ quietOtherApps: true, pauseMedia: false });
  await pad.play(quietTone());
  // Yap sees the pad's session (its process is the test's) and nothing else.
  await expect.poll(() => padLevel(robot), { timeout: 10_000 }).toBeGreaterThan(0);
  const before = await padLevel(robot);
  const view = await robot.otherApps();
  expect(view.sessions.every((s) => s.pid === pad.pid)).toBe(true);
  expect(view.quieted).toEqual([]);

  await pad.set('box', '');
  const mark = robot.logMark();
  const dictation = robot.dictateWav(pad, speechWav('Silence the other apps while I talk.'));
  // While it records: down to silence.
  await expect.poll(() => padLevel(robot), { timeout: 8_000, intervals: [100] }).toBe(0);
  const during = await robot.otherApps();
  expect(during.quieted).toHaveLength(1);
  expect(during.quieted[0]).toMatchObject({ exe: 'yaprobot.exe', setTo: 0 });
  expect(during.quieted[0].before).toBeCloseTo(before, 3);

  const r = await dictation;
  expect(r?.raw).toMatch(/STT stub: received/);
  // After the stop chime: exactly its level again, nothing held down.
  await expect.poll(() => padLevel(robot), { timeout: 10_000 }).toBeCloseTo(before, 3);
  expect((await robot.otherApps()).quieted).toEqual([]);
  const log = robot.logSince(mark);
  expect(log).toContain('other apps: quieted');
  expect(log).toContain('other apps: back to its level');
  // The start chime first: the recording began before anything went quiet.
  expect(log.indexOf('Recording started')).toBeLessThan(log.indexOf('other apps: quieted'));
});

test('Pause music and videos: the test page pauses while you dictate, then plays again', async ({ robot }) => {
  const { pad } = robot;
  await robot.configure({ quietOtherApps: false, pauseMedia: true });
  const media = await robot.mediaPage(quietTone());
  await expect.poll(() => media.state().then((s) => !s.paused && s.time > 0.3), { timeout: 15_000 }).toBe(true);
  // In Windows' media controls, playing, under the test's title (nothing
  // else is reported: only media titled with the nonce).
  const status = async () => (await robot.otherApps()).media.find((m) => m.title === media.title)?.status;
  await expect.poll(status, { timeout: 20_000 }).toBe('playing');
  expect((await robot.otherApps()).media.every((m) => m.title.includes(robot.nonce))).toBe(true);

  await pad.set('box', '');
  const mark = robot.logMark();
  const dictation = robot.dictateWav(pad, speechWav('Pause the music while I talk.'));
  await expect.poll(() => media.state().then((s) => s.paused), { timeout: 8_000, intervals: [100] }).toBe(true);
  expect((await robot.otherApps()).paused).toHaveLength(1);
  // Its sound wasn't turned down (that's the other switch).
  expect((await robot.otherApps()).quieted).toEqual([]);

  await dictation;
  await expect.poll(() => media.state().then((s) => s.paused), { timeout: 10_000 }).toBe(false);
  await expect.poll(status, { timeout: 10_000 }).toBe('playing');
  expect((await robot.otherApps()).paused).toEqual([]);
  const log = robot.logSince(mark);
  expect(log).toContain('other apps: paused media');
  expect(log).toContain('other apps: media playing again');
});
