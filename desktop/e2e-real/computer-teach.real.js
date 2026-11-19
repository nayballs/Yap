// Computer use, phase 4, on the REAL screen: teach mode's tooltip (desktop/src-tauri/src/computer/teach.rs,
// teach/tooltip.rs). With only the robot's test pad allowed, a tour's step shows Yap's native tooltip
// beside the pad's box (a ring on the spot); the robot clicks its Next as the person would — and the
// focus stays where it was (the tooltip never takes it) — then the step's actions run: a click into the
// box and typing, there in the pad afterwards. Robot mode's window gate (shell/realtest.rs) keeps every
// event inside the run's own windows.
//
// It takes the mouse and the keyboard for a few seconds: hands off the keyboard.
import { test, expect } from './support/robot.js';
import { helper, HELPER } from './support/targets.js';

test.use({ robotOptions: { name: 'robot-computer-teach', config: { computerUse: true, assistantScreen: true, assistantScreenCloud: true } } });

const CONV = 4343;
const run = (yap, name, args = {}) => yap.invoke('e2e_computer_do', { conversation: CONV, exes: [HELPER], name, args });

/** A field's middle-left on the last screenshot (its frame: `f`). */
function on(f, rect, dx = 16) {
  const sx = f.w / f.rect.w;
  const sy = f.h / f.rect.h;
  return [Math.round((rect.x + dx - f.rect.x) * sx), Math.round((rect.y + rect.h / 2 - f.rect.y) * sy)];
}

test("a guided tour's tooltip beside the pad: Next clicked as the person would, the focus kept, the step done", async ({ robot }) => {
  const { yap, pad } = robot;
  expect(helper('place', pad.hwnd, `pid:${pad.pid}`, 160, 160).ok, 'the pad placed').toBe(true);
  await pad.set('box', '');
  await pad.focus('rich');
  expect((await run(yap, 'request_teach_access', { apps: ['yaprobot'], reason: 'the pad' })).result.success).toBe(true);
  const shot = await run(yap, 'screenshot');
  expect(shot.result.success, JSON.stringify(shot.result)).toBe(true);
  const box = helper('winrect', (await pad.call({ cmd: 'control', field: 'box' })).hwnd);
  const at = on(shot.frame, box);
  // Not awaited: the step waits for Next.
  const stepping = run(yap, 'teach_step', {
    explanation: 'This is the box: what you type here stays in the pad.',
    next_preview: "Next: I'll click it and type for you.",
    anchor: at,
    actions: [
      { action: 'left_click', coordinate: at, action_summary: 'Clicks into the box' },
      { action: 'type', text: 'Taught by Yap', action_summary: 'Types a line' },
    ],
  });
  await expect.poll(async () => (await yap.invoke('e2e_computer_teach')).placed, { timeout: 15_000, message: 'the tooltip on screen' }).not.toBeNull();
  const shown = await yap.invoke('e2e_computer_teach');
  console.log(`computer use teach: tooltip ${JSON.stringify(shown.placed.card)}, its Next ${JSON.stringify(shown.placed.next)}`);
  expect(shown.tip.explanation).toContain('This is the box');
  expect(await pad.text('box'), 'nothing done before Next').toBe('');
  const next = shown.placed.next;
  const clicked = helper('barclick', yap.child.pid, next.x + Math.round(next.w / 2), next.y + Math.round(next.h / 2), 250);
  expect(clicked.ok, JSON.stringify(clicked)).toBe(true);
  expect(clicked.fgAfter, 'the tooltip never takes the focus').toBe(clicked.fgBefore);
  const done = await stepping;
  expect(done.result.success, JSON.stringify(done.result)).toBe(true);
  await expect.poll(() => pad.text('box')).toBe('Taught by Yap');
  expect((await yap.invoke('e2e_computer_teach')).placed, 'the tooltip gone after Next').toBeNull();
});
