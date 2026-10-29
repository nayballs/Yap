// The robot on the quick note's Send to {app} (notes/quick_note/send.rs): the
// quick note comes up over the test pad (its shortcut, sent as the input
// hook sends it: `e2e_shortcut`), remembers the pad as the app it came up
// over, and Send pastes the note — or only its selection — back into the
// pad by the ordinary paste path (`text_injector::inject_text`, strict), the
// quick note stepping aside first. Written 2026-10-29; not run yet (the
// robot takes the keyboard for a while: run it when nobody's at the PC).
import { test, expect } from './support/robot.js';

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const visible = (yap) => yap.invoke('plugin:window|is_visible', { label: 'quick-note' });

async function openOverPad(robot) {
  const { yap, pad } = robot;
  await pad.focus('box');
  await sleep(300);
  if (await visible(yap)) await yap.invoke('quick_note_hide');
  await yap.invoke('e2e_shortcut', { name: 'quick-note' });
  await expect.poll(() => visible(yap)).toBe(true);
  return yap.quickNote;
}

test("Send pastes the note into the window the quick note came up over", async ({ robot }) => {
  const { yap, pad } = robot;
  await pad.set('box', '');
  const qn = await openOverPad(robot);
  await qn.getByRole('button', { name: 'New note' }).click();
  const box = qn.getByRole('textbox', { name: 'Quick note' });
  await box.fill('Meet at the north gate at six');
  const send = qn.getByRole('button', { name: /^Send to / });
  await expect(send).toBeEnabled();
  const mark = robot.logMark();
  await send.click();
  await expect.poll(() => visible(yap)).toBe(false);
  await expect.poll(() => pad.text('box')).toBe('Meet at the north gate at six');
  // The log says how much, never what.
  expect(robot.logSince(mark)).toContain('Quick note: sending the note to the app it came from');
  expect(robot.logSince(mark)).not.toContain('north gate');
});

test('Send with words selected in the note pastes only those', async ({ robot }) => {
  const { pad } = robot;
  await pad.set('box', 'Note: ');
  const qn = await openOverPad(robot);
  await qn.getByRole('button', { name: 'New note' }).click();
  const box = qn.getByRole('textbox', { name: 'Quick note' });
  await box.fill('Bring the blue folder and the keys');
  await box.evaluate((el) => {
    el.focus();
    el.setSelectionRange(0, 21); // "Bring the blue folder"
  });
  await qn.getByRole('button', { name: /^Send to / }).click();
  await expect.poll(() => pad.text('box')).toBe('Note: Bring the blue folder');
});
