// Home's meetings (MeetingsCard.svelte): the past meeting notes, newest
// first (`calendar_meeting_notes`, as the Meetings view lists them), asked
// for after Home's first paint and again when notes change. What's coming
// up is the calendar store's (stores/calendar.svelte.js).
import { invoke } from '@tauri-apps/api/core';

/** How many of each Home shows. */
export const SHOWN = 3;

export const homeMeetings = $state({ past: [], loaded: false });

let asked = 0;

/** Re-read the past meeting notes (the newest few). */
export async function loadPastMeetings() {
  const mine = ++asked;
  try {
    const all = (await invoke('calendar_meeting_notes', { query: null })) || [];
    if (mine !== asked) return;
    homeMeetings.past = all.slice(0, SHOWN);
  } catch {
    if (mine === asked) homeMeetings.past = [];
  }
  homeMeetings.loaded = true;
}
