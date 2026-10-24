// "Spell names the way they're written on screen" (dictionary/screen_terms.rs): a
// dictation never waits for the read of the screen. Every read in this
// spec's Yap waits 8 s before it starts (YAP_REALTEST_SCREEN_DELAY_MS, robot
// mode only); a spoken dictation still pastes at once, without the names.
import { test, expect } from './support/robot.js';
import { SCREEN } from './support/screen.js';
import { speechWav } from './support/tts.js';

test.use({ robotOptions: { name: 'screen-slow', env: { YAP_REALTEST_SCREEN_DELAY_MS: '8000' } } });

test('the dictation never waits for the read: pasted at once, without the names', async ({ robot }) => {
  const page = await robot.editor(SCREEN);
  await page.set('');
  const mark = robot.logMark();
  const started = Date.now();
  const r = await robot.dictateWav(page, speechWav('Testing the screen names.'));
  const took = Date.now() - started;
  expect(r).not.toBeNull();
  await expect.poll(() => page.value()).toBe(r.text);
  // The WAV is about 2 s: the whole dictation well inside the read's 8 s.
  expect(took, `the dictation took ${took} ms`).toBeLessThan(6_000);
  const log = robot.logSince(mark);
  expect(log).toContain("The screen's names are still being read; pasting without them");
  expect(log).not.toContain('Read the names on screen');
  console.log(`a dictation with the read still going: ${took} ms from the WAV's start to the paste`);
});
