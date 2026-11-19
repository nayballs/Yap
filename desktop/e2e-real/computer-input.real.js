// Computer use, phase 2, on the REAL screen, mouse and keyboard (desktop/src-tauri/src/computer/act.rs,
// input.rs, checks.rs): with only the robot's test pad allowed (its exe, which nothing else runs),
// computer use takes a screenshot, clicks into the pad's box at the screenshot's coordinates (SendInput,
// absolute virtual-desktop points), types into it, selects it all with Ctrl+A and types over it — and
// what's in the box is exactly that. The pad's password box is never typed into (UI Automation's
// IsPassword). Robot mode's window gate (shell/realtest.rs) keeps every event inside the run's own
// windows.
//
// It takes the mouse and the keyboard for a few seconds: hands off the keyboard.
import { test, expect } from './support/robot.js';
import { helper, HELPER } from './support/targets.js';

test.use({ robotOptions: { name: 'robot-computer-input', config: { computerUse: true, assistantScreen: true, assistantScreenCloud: true } } });

const CONV = 4242;
const run = (yap, name, args = {}) => yap.invoke('e2e_computer_do', { conversation: CONV, exes: [HELPER], name, args });

/** A field's middle-left on the last screenshot (its frame: `f`). */
function on(f, rect, dx = 16) {
  const sx = f.w / f.rect.w;
  const sy = f.h / f.rect.h;
  return [Math.round((rect.x + dx - f.rect.x) * sx), Math.round((rect.y + rect.h / 2 - f.rect.y) * sy)];
}

test('computer use clicks into the pad, types, selects all and types over it', async ({ robot }) => {
  const { yap, pad } = robot;
  expect(helper('place', pad.hwnd, `pid:${pad.pid}`, 140, 140).ok, 'the pad placed').toBe(true);
  await pad.set('box', '');
  // The focus somewhere else: the click has to move it.
  await pad.focus('rich');
  const shot = await run(yap, 'screenshot');
  expect(shot.result.success, JSON.stringify(shot.result)).toBe(true);
  expect(shot.result.data.windows.length, 'the pad, allowed').toBeGreaterThanOrEqual(1);
  const box = helper('winrect', (await pad.call({ cmd: 'control', field: 'box' })).hwnd);

  const clicked = await run(yap, 'left_click', { coordinate: on(shot.frame, box), action_summary: 'Clicks into the box' });
  expect(clicked.result.success, JSON.stringify(clicked.result)).toBe(true);
  const typed = await run(yap, 'type', { text: 'Typed by computer use', action_summary: 'Types a line' });
  expect(typed.result.success, JSON.stringify(typed.result)).toBe(true);
  await expect.poll(() => pad.text('box')).toBe('Typed by computer use');

  expect((await run(yap, 'key', { text: 'ctrl+a', action_summary: 'Selects it all' })).result.success).toBe(true);
  expect((await run(yap, 'type', { text: 'Over it', action_summary: 'Types over it' })).result.success).toBe(true);
  await expect.poll(() => pad.text('box')).toBe('Over it');

  // The password box: clicked into, then never typed into.
  const pw = helper('winrect', (await pad.call({ cmd: 'control', field: 'password' })).hwnd);
  expect((await run(yap, 'left_click', { coordinate: on(shot.frame, pw), action_summary: 'Clicks into the password box' })).result.success).toBe(true);
  const refused = await run(yap, 'type', { text: 'never', action_summary: 'Types a password' });
  expect(refused.result.success).toBe(false);
  expect(refused.result.error).toContain('password');
  expect(await pad.text('password')).toBe('');
});
