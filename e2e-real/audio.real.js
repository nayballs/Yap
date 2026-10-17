// The robot's microphone path on the stub build: a WAV played into the
// dictation in the mic's place, between a real start and a real stop
// (`realtest_dictate_wav`), so the recording, the live preview, the
// dead-mic watchdog, the STT step and the paste all run as with a voice.
// (The stub's "transcript" says how much audio it got; stt.real.js runs the
// real model on an engines build.)
import { test, expect } from './support/robot.js';
import { speechWav, silenceWav } from './support/tts.js';

test('a spoken WAV goes through the recording to the paste', async ({ robot }) => {
  const { pad } = robot;
  await pad.set('box', '');
  const mark = robot.logMark();
  const r = await robot.dictateWav(pad, speechWav('Testing the robot microphone.'));
  expect(r).not.toBeNull();
  // The stub transcribes every recording as "[STT stub: received N.Ns of audio…]".
  expect(r.raw).toMatch(/STT stub: received \d+\.\ds of audio/);
  await expect.poll(() => pad.text('box')).toContain('STT stub: received');
  const log = robot.logSince(mark);
  expect(log).toContain('Recording started');
  expect(log).toContain('Recording stopped, transcribing');
  expect(log).not.toContain('nothing but silence');
});

test('a dead mic (digital silence): reopened, "No sound from…", nothing pasted', async ({ robot, shot }) => {
  const { yap, pad } = robot;
  await pad.set('box', 'untouched');
  const mark = robot.logMark();
  const r = await robot.dictateWav(pad, silenceWav(7));
  // Nothing transcribed, nothing pasted.
  expect(r).toBeNull();
  expect(await pad.text('box')).toBe('untouched');
  const log = robot.logSince(mark);
  expect(log).toContain('Dictation mic: nothing but silence — reopening it');
  expect(log).toMatch(/Dictation mic: No sound from .* — check it isn't muted or unplugged/);
  expect(log).toContain('Recording stopped: the mic gave nothing but silence, not transcribed');
  // Said in the main window.
  await expect(yap.main.getByText(/No sound from .* check it isn't muted or unplugged/).first()).toBeVisible();
  await shot(yap.main, '01-no-sound-toast');
});
