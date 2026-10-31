// The robot on a mouse's side buttons with real XBUTTON events (SendInput,
// marked as the robot's: `hookKeys` "xd:1" / "xu:1") through the real
// low-level mouse hook (dictation/input_hook.rs `mouse_button`), set up the
// way Settings → Shortcuts' dialog does it (`side_buttons_apply`,
// dictation/side_buttons.rs). The dictation records a made-up microphone's
// quiet tone (`YAP_E2E_MICS`), transcribed by the stub engine, and is
// pasted into the test pad.
//
// - the setup's listening: a side button pressed is heard for the dialog
//   (and goes nowhere else);
// - Back set to dictate (toggle): a click starts a dictation, the next stops
//   it, and it's pasted; push-to-talk: held while it records;
// - Forward set to Send: the send key (Enter) pressed in the test pad;
// - unbound (Remove): the hook leaves them alone — no dictation, no Enter.
//
// Written 2026-10-31; not run yet (the maintainer runs the robot suite).
import { test, expect, lf } from './support/robot.js';

const MIC = { id: 'desk', name: 'Microphone (Contoso Desk Mic)' };

test.use({
  robotOptions: {
    name: 'side-buttons',
    env: { YAP_E2E_MICS: JSON.stringify([MIC]) },
    config: { streamingPartials: false, recordingMode: 'toggle', sendHotkey: '', moreHotkeys: {} },
  },
});

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const capsule = (robot) => robot.yap.overlay.locator('.capsule');
const bindings = (robot) => robot.yap.invoke('e2e_hotkey_bindings');

/** Wait for the last dictation to be over (the capsule gone). */
async function idle(robot) {
  await expect(capsule(robot)).toBeHidden({ timeout: 20_000 });
}

test.afterAll(async ({ robot }) => {
  await robot.yap.invoke('side_buttons_listen', { on: false }).catch(() => {});
  await robot.configure({ recordingMode: 'toggle', sendHotkey: '', moreHotkeys: {} });
});

test("the setup's listening hears a real side button, and nothing else gets it", async ({ robot }) => {
  const { pad } = robot;
  await pad.set('box', '');
  await robot.yap.invoke('side_buttons_listen', { on: true });
  try {
    const mark = robot.logMark();
    await robot.hookKeys('xd:2,w:60,xu:2');
    await expect.poll(() => robot.logSince(mark)).toContain('Side buttons: heard one for the setup');
    expect(robot.logSince(mark)).toMatch(/button="?Forward"?/);
    expect(robot.logSince(mark)).not.toContain('Input hook: emitting');
  } finally {
    await robot.yap.invoke('side_buttons_listen', { on: false });
  }
  expect(await pad.text('box')).toBe('');
});

test('Back set to dictate starts and stops a dictation; held, push-to-talk records while it is down', async ({ robot }) => {
  const { pad } = robot;
  await robot.yap.invoke('side_buttons_apply', { choice: { dictate: 4, send: 5, hold: false } });
  await expect.poll(async () => (await bindings(robot)).actions.hotkey.map((b) => b.key)).toContain('mouse button 4');
  await idle(robot);

  // Toggle: a click starts it, the next stops it, and it's pasted.
  await pad.set('box', '');
  let mark = robot.logMark();
  await robot.hookKeys('xd:1,w:60,xu:1');
  await expect(capsule(robot)).toBeVisible();
  await sleep(1_500);
  await robot.hookKeys('xd:1,w:60,xu:1');
  await expect.poll(() => pad.text('box'), { timeout: 20_000 }).toContain('STT stub');
  expect(robot.logSince(mark)).toContain('Input hook: emitting dictation-key-pressed');
  await idle(robot);

  // Push-to-talk: held while it records, let go to transcribe.
  await robot.yap.invoke('side_buttons_apply', { choice: { dictate: 4, send: 5, hold: true } });
  await pad.set('box', '');
  mark = robot.logMark();
  await robot.hookKeys('xd:1,w:1500,xu:1');
  await expect.poll(() => pad.text('box'), { timeout: 20_000 }).toContain('STT stub');
  expect(robot.logSince(mark)).toContain('Input hook: emitting dictation-key-released');
  await idle(robot);
});

test('Forward set to Send presses Enter where you are typing; removed, the buttons do nothing in Yap', async ({ robot }) => {
  const { pad } = robot;
  await robot.yap.invoke('side_buttons_apply', { choice: { dictate: 4, send: 5, hold: false } });
  await expect.poll(async () => (await bindings(robot)).actions.sendHotkey[0].key).toBe('mouse button 5');
  await idle(robot);

  await pad.set('box', 'Ready');
  let mark = robot.logMark();
  await robot.hookKeys('xd:2,w:60,xu:2');
  await expect.poll(async () => lf(await pad.text('box'))).toBe('Ready\n');
  expect(robot.logSince(mark)).toContain('Send shortcut: pressed the send key');

  // Removed: the hook lets them by — no dictation starts, nothing is sent.
  await robot.yap.invoke('side_buttons_remove');
  await expect.poll(async () => (await bindings(robot)).actions.sendHotkey[0].key).toBe('none');
  await pad.set('box', 'Ready');
  mark = robot.logMark();
  await robot.hookKeys('xd:2,w:60,xu:2,w:200,xd:1,w:60,xu:1');
  await sleep(1_000);
  await expect(capsule(robot)).toBeHidden();
  expect(lf(await pad.text('box'))).toBe('Ready');
  expect(robot.logSince(mark)).not.toContain('Input hook: emitting');
});
