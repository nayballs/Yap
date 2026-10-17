// The robot in Windows 11's Notepad (its WinUI text editor), on a temp file
// named after the run's nonce, read back over UI Automation. Skipped while
// Notepad is open: Windows 11 would open the file as a tab in your window.
import { test, expect, lf } from './support/robot.js';
import { notepadBusy } from './support/targets.js';

const busy = notepadBusy();
test.skip(!!busy, busy ?? '');

test('Notepad: a dictation lands, and the next one follows it', async ({ robot }) => {
  const np = await robot.notepad();
  let r = await robot.dictate(np, 'Hello from the robot.');
  expect(r.kind).toBe('other');
  await expect.poll(() => lf(np.text()).trimEnd()).toBe('Hello from the robot.');
  // After a sentence: a space, its capital.
  r = await robot.dictate(np, 'And then we left.');
  expect(r.text).toBe(' And then we left.');
  await expect.poll(() => lf(np.text()).trimEnd()).toBe('Hello from the robot. And then we left.');
});
