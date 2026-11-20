// Computer use, phase 5, on the REAL screen and real UI Automation (desktop/src-tauri/src/computer/elements.rs,
// computer/uia/tree.rs): with only the robot's test pad allowed (a WinForms window), read_window lists its
// controls by ref — its text boxes, its password box marked and without a value, its button. set_value
// focuses the pad's box and types the text over what was there (the app sees it typed: a dialog's box that
// shows text set through its Value pattern can still ignore it, 20 Nov 2026), the pattern only its fallback;
// a password box is refused; click_element presses the pad's button through its Invoke pattern, no mouse
// moved. Robot mode's window gate (shell/realtest.rs) keeps it to the run's own windows.
import { test, expect } from './support/robot.js';
import { helper, HELPER } from './support/targets.js';

test.use({ robotOptions: { name: 'robot-computer-elements', config: { computerUse: true, assistantScreen: true, assistantScreenCloud: true } } });

const CONV = 4444;
const run = (yap, name, args = {}) => yap.invoke('e2e_computer_do', { conversation: CONV, exes: [HELPER], name, args });

test("the pad read by ref: its box typed over, its password box refused, its button pressed", async ({ robot }) => {
  const { yap, pad } = robot;
  expect(helper('place', pad.hwnd, `pid:${pad.pid}`, 180, 180).ok, 'the pad placed').toBe(true);
  await pad.set('box', 'Old text to be replaced');
  await pad.focus('box');
  const read = await run(yap, 'read_window', {});
  expect(read.result.success, JSON.stringify(read.result)).toBe(true);
  const tree = read.result.data.tree;
  console.log(`computer use elements: ${read.result.data.elements} element(s) read`);
  const lines = tree.split('\n');
  const edits = lines.filter((l) => /^\s*e\d+ edit\b/.test(l));
  expect(edits.length, tree).toBeGreaterThanOrEqual(3);
  const refOf = (line) => /\b(e\d+)\b/.exec(line)[1];
  // The password box: marked, never its value.
  const pw = edits.find((l) => l.includes('(password)'));
  expect(pw, tree).toBeTruthy();
  // The pad's box is its first edit box.
  const box = edits.find((l) => !l.includes('(password)'));
  const set = await run(yap, 'set_value', { ref: refOf(box), text: 'Set through its pattern', action_summary: 'Fills the box' });
  expect(set.result.success, JSON.stringify(set.result)).toBe(true);
  expect(set.result.note, JSON.stringify(set.result)).toContain('typed it in');
  await expect.poll(() => pad.text('box')).toBe('Set through its pattern');
  const refused = await run(yap, 'set_value', { ref: refOf(pw), text: 'never' });
  expect(refused.result.success).toBe(false);
  expect(refused.result.error).toContain('password');
  expect(await pad.text('password')).toBe('');
  const button = lines.find((l) => /\bbutton "A button"/.test(l));
  expect(button, tree).toBeTruthy();
  const pressed = await run(yap, 'click_element', { ref: refOf(button), action_summary: 'Presses the button' });
  expect(pressed.result.success, JSON.stringify(pressed.result)).toBe(true);
});
