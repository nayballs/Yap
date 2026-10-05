// Helpers for the meeting specs (meetings.spec.js, meeting-summary.spec.js).
import { openView } from './fixtures.js';

/** A new note with `title` and attendees, open in the editor. */
export async function newMeetingNote(main, title, attendees = []) {
  await openView(main, 'Notes');
  await main.getByRole('button', { name: 'New note' }).first().click();
  await main.getByPlaceholder('Untitled Note').fill(title);
  if (attendees.length) {
    await main.getByRole('button', { name: 'Add attendees' }).click();
    const input = main.getByPlaceholder('Add attendees…');
    for (const name of attendees) {
      await input.fill(name);
      await input.press('Enter');
    }
    await input.press('Escape');
  }
}

/** The note titled `title` in a notes.json store. */
export const meetingNote = (store, title) => store?.notes?.find((n) => n.title === title);
