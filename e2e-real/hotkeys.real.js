// The robot on Yap's global shortcuts with real keys (SendInput) through the
// real low-level hook (dictation/input_hook.rs): robot mode runs the hook, acting
// only on keys the robot marks as its own (`hookkeys`), so the developer's
// keys are never seen. The dictation records a made-up microphone's quiet
// tone (`YAP_E2E_MICS`), transcribed by the stub engine, and is pasted into
// the test pad.
//
// - a second key for dictation ("+ Add another", `moreHotkeys`): either key
//   starts it and either stops it;
// - a key stuck "down": its key-down heard, its key-up not (marked for the
//   hook to miss, as the lock screen or a UAC prompt would take it) — the
//   sweep finds input the hook missed and lets the key go without its
//   release: the recording goes on hands-free (never stopped by the sweep),
//   and the next press stops it, nothing lost;
// - a push-to-talk press that grows into a chord within a second (another
//   key down while it's held): dropped silently, the other key reaching the
//   app as a shortcut's would — and a press on its own still dictates.
import { test, expect } from './support/robot.js';

const MIC = { id: 'desk', name: 'Microphone (Contoso Desk Mic)' };
const F24 = 135; // the suite's dictation hotkey
const F23 = 134;
const A = 65;

test.use({
  robotOptions: {
    name: 'hotkeys',
    env: { YAP_E2E_MICS: JSON.stringify([MIC]) },
    config: { streamingPartials: false, moreHotkeys: { hotkey: [`kb:${F23}`] } },
  },
});

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const capsule = (robot) => robot.yap.overlay.locator('.capsule');
const bindings = (robot) => robot.yap.invoke('e2e_hotkey_bindings');

/** Wait for the last dictation to be over (the capsule gone). */
async function idle(robot) {
  await expect(capsule(robot)).toBeHidden({ timeout: 20_000 });
}

test('a second key for dictation: either key starts it and either stops it', async ({ robot }) => {
  const { pad } = robot;
  await robot.configure({ recordingMode: 'toggle', moreHotkeys: { hotkey: [`kb:${F23}`] } });
  await expect.poll(async () => (await bindings(robot)).actions.hotkey.map((b) => b.key).slice(0, 2)).toEqual([
    `keyboard vkey ${F24}`,
    `keyboard vkey ${F23}`,
  ]);
  await idle(robot);

  // F23 (the second key) starts it, F24 (the first) stops it.
  await pad.set('box', '');
  let mark = robot.logMark();
  await robot.hookKeys(`d:${F23},w:60,u:${F23}`);
  await expect(capsule(robot)).toBeVisible();
  await sleep(1_500);
  await robot.hookKeys(`d:${F24},w:60,u:${F24}`);
  await expect.poll(() => pad.text('box'), { timeout: 20_000 }).toContain('STT stub');
  expect(robot.logSince(mark)).toContain('Input hook: queueing dictation-key-pressed');
  await idle(robot);

  // …and the other way round. The keys themselves never reach the pad.
  await pad.set('box', '');
  mark = robot.logMark();
  await robot.hookKeys(`d:${F24},w:60,u:${F24}`);
  await expect(capsule(robot)).toBeVisible();
  await sleep(1_500);
  await robot.hookKeys(`d:${F23},w:60,u:${F23}`);
  await expect.poll(() => pad.text('box'), { timeout: 20_000 }).toContain('STT stub');
  expect((await pad.text('box')).match(/STT stub/g)).toHaveLength(1);
  await idle(robot);
});

test('a key stuck down: the sweep lets it go, the recording goes on hands-free, the next press stops it', async ({ robot }) => {
  const { pad } = robot;
  await robot.configure({ recordingMode: 'pushToTalk' });
  await idle(robot);
  await pad.set('box', '');
  const mark = robot.logMark();
  // Held: push-to-talk records.
  await robot.hookKeys(`d:${F24}`);
  await expect(capsule(robot)).toBeVisible();
  await sleep(1_500);
  // Its key-up goes by unheard (the lock screen took it).
  await robot.hookKeys(`U:${F24}`);
  await sleep(300);
  expect((await bindings(robot)).actions.hotkey[0].active).toBe(true);
  // The sweep (every 5 s) finds the input the hook missed, and lets go.
  await expect.poll(() => robot.logSince(mark), { timeout: 15_000 }).toContain('was stuck down');
  const log = robot.logSince(mark);
  expect(log).toContain('the hook missed input');
  expect(log).toContain('it goes on hands-free');
  expect((await bindings(robot)).actions.hotkey[0].active).toBe(false);
  // Never stopped by the sweep: still recording, hands-free on the bar.
  await expect(capsule(robot)).toBeVisible();
  await expect(capsule(robot).getByRole('img', { name: /Hands-free/ })).toBeVisible();
  expect(robot.logSince(mark)).not.toContain('Recording stopped, transcribing');
  expect(await pad.text('box')).toBe('');

  // The next press stops it, and what was said is pasted.
  await robot.hookKeys(`d:${F24},w:80,u:${F24}`);
  await expect.poll(() => pad.text('box'), { timeout: 20_000 }).toContain('STT stub');
  await idle(robot);
});

test('a push-to-talk press that grows into a chord within a second is dropped; one on its own dictates', async ({ robot }) => {
  const { pad } = robot;
  await robot.configure({ recordingMode: 'pushToTalk' });
  await idle(robot);
  await pad.set('box', '');
  const mark = robot.logMark();
  // F24 down, then A within a quarter of a second: a shortcut, not a dictation.
  await robot.hookKeys(`d:${F24},w:250,d:${A},u:${A},w:150,u:${F24}`);
  await expect.poll(() => robot.logSince(mark), { timeout: 5_000 }).toContain('grew into a chord within a second');
  await idle(robot);
  await sleep(800);
  // Nothing transcribed or pasted: the box has only the other key's own "a".
  expect(await pad.text('box')).toBe('a');
  expect(robot.logSince(mark)).not.toContain('Recording stopped, transcribing');

  // A press held on its own still dictates.
  await pad.set('box', '');
  await robot.hookKeys(`d:${F24},w:1500,u:${F24}`);
  await expect.poll(() => pad.text('box'), { timeout: 20_000 }).toContain('STT stub');
  await idle(robot);
});
