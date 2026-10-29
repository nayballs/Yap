// First-meeting coach marks in the notepad (meetings/coach_marks.rs,
// notepad/CoachMark.svelte; Wispr Flow's notepad pointers): in the
// person's first recorded meeting, "Take notes here" over My thoughts and
// "Try What did I miss?" by the Ask bar — each once (counted in hints.json
// as it shows), each dismissable (✕, or typing notes / opening the Ask
// sheet), and none in the next meeting. Test runs show none unless the
// instance asks for them (YAP_E2E_COACH_MARKS=1, this spec's own instance),
// so no other spec's first meeting has a card in its way.
import { test as base, expect } from './support/fixtures.js';

const test = base.extend({
  yapOptions: [
    async ({}, use) => {
      await use({ name: 'coach-marks', config: { meetingReminder: 'never' }, env: { YAP_E2E_COACH_MARKS: '1' } });
    },
    { scope: 'worker' },
  ],
});

const visible = (yap, label) => yap.invoke('plugin:window|is_visible', { label });
const recording = (yap) => yap.invoke('meeting_state').then((s) => s.recording);
const askInput = (pad) => pad.getByRole('textbox', { name: 'Ask anything' });

let clockTs = Math.floor(Date.now() / 1000);
async function say(yap, ...lines) {
  const segments = lines.map((line) => ({ source: line[0], text: line[1], ts: (clockTs += 6) }));
  expect(await yap.invoke('e2e_meeting_feed', { segments })).toBe(segments.length);
}
async function pauseAll(yap) {
  if (await recording(yap)) {
    await yap.invoke('meeting_pause');
    await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
  }
}
async function startMeeting(yap, title) {
  const note = await yap.invoke('note_create', { title, folder: 'Meetings' });
  await yap.invoke('meeting_start', { noteId: note.id });
  await expect.poll(() => visible(yap, 'notepad')).toBe(true);
  return note;
}

test.afterEach(async ({ yap }) => {
  await pauseAll(yap);
});

test("the first meeting's two coach marks: once each, dismissable, never in the next meeting", async ({ yap, shot }) => {
  expect(yap.readJson('hints.json')?.notepadNotes ?? 0).toBe(0);
  await startMeeting(yap, 'First call');
  const pad = yap.notepad;
  const notesMark = pad.getByRole('note', { name: 'Take notes here' });
  const catchUpMark = pad.getByRole('note', { name: 'Try What did I miss?' });
  await expect(notesMark).toBeVisible();
  await expect(catchUpMark).toBeVisible();
  await expect(catchUpMark).toContainText('catches you up');
  await shot(pad, '01-first-meeting');
  // Counted as they show (hints.json, beside the push-to-talk hints).
  await expect.poll(() => [yap.readJson('hints.json')?.notepadNotes, yap.readJson('hints.json')?.notepadCatchUp]).toEqual([1, 1]);

  // ✕ puts one away.
  await catchUpMark.getByRole('button', { name: 'Dismiss tip' }).click();
  await expect(catchUpMark).toHaveCount(0);
  // Typing notes puts the other away (it's done its job).
  await pad.getByRole('textbox', { name: 'My thoughts' }).click();
  await pad.keyboard.type('Agenda: the launch date');
  await expect(notesMark).toHaveCount(0);
  await say(yap, ['you', 'Thanks for joining, everyone. Today is about the launch date and the budget.']);
  await pauseAll(yap);

  // The next meeting: no marks (the first had talk, and each has shown once).
  await startMeeting(yap, 'Second call');
  await expect(pad.getByRole('textbox', { name: 'My thoughts' })).toBeVisible();
  await expect(askInput(pad)).toBeVisible();
  await expect(pad.getByTestId('coach-mark')).toHaveCount(0);
  expect(await yap.invoke('notepad_coach_marks', { noteId: (await yap.invoke('notepad_state')).noteId })).toEqual({ notes: false, catchUp: false });
  await shot(pad, '02-next-meeting');
});
