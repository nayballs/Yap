// The end of a meeting, as Rust runs it (src-tauri/src/meeting_end.rs), for
// every window that shows a meeting (the Notes view, the meeting notepad):
//
// - `summaries.byNote[noteId]`: the latest action-plan job for a note,
//   `{ noteId, run, state: 'running' | 'done' | 'error' | 'needsAi' |
//   'nothing', step, steps, error }` (`meeting_summary_status` + the
//   `yap-meeting-summary` event). Rust writes the plan when a meeting ends,
//   whichever window is open; `summarise(noteId)` runs it again ("Generate
//   summary", Retry, the Notes view's "Action plan").
// - `summaries.progress[noteId]`: `{ done, total }` while a long meeting's
//   last digests are written first (`yap-meeting-summary-progress`).
// - `askStartedByMistake(...)`: the "Started by mistake?" toast, which the
//   window Rust picks (`yap-meeting-ended`'s `surface`) shows; Discard
//   deletes the meeting note (`meeting_discard`).
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { toast, dismiss, isToastLive } from './ui/toast.svelte.js';

export const summaries = $state({ byNote: {}, progress: {} });

let started = false;

/** Follow the action-plan jobs (idempotent; each window calls it once). */
export function initMeetingSummary() {
  if (started) return;
  started = true;
  listen('yap-meeting-summary', (e) => {
    const s = e.payload;
    if (s?.noteId == null) return;
    summaries.byNote[s.noteId] = s;
    if (s.state !== 'running') delete summaries.progress[s.noteId];
  });
  listen('yap-meeting-summary-progress', (e) => {
    const p = e.payload;
    if (p?.noteId != null) summaries.progress[p.noteId] = p;
  });
  // A resumed meeting's earlier job is moot (Rust forgets it too).
  listen('yap-meeting-state', (e) => {
    const id = e.payload?.recording ? e.payload.noteId : null;
    if (id != null) {
      delete summaries.byNote[id];
      delete summaries.progress[id];
      dismissMistake(id);
    }
  });
}

/** Load a note's latest job (a window opened after it ran). */
export async function loadSummary(noteId) {
  if (noteId == null) return;
  try {
    const s = await invoke('meeting_summary_status', { noteId });
    if (s) summaries.byNote[noteId] = s;
  } catch {
    /* keep what we have */
  }
}

/** Write a meeting's action plan (again). Throws when it can't start. */
export function summarise(noteId) {
  return invoke('meeting_summarise', { noteId });
}

/** "Writing your action plan" — the job's step as a friendly line. */
export function stepLine(job, progress, minutes) {
  if (progress?.total > 0) {
    return `Catching up on the meeting: part ${progress.done + 1} of ${progress.total}…`;
  }
  const talk =
    minutes == null ? 'your meeting' : minutes < 1 ? 'under a minute of talk' : `${minutes} minute${minutes === 1 ? '' : 's'} of talk`;
  if (job?.step === 2) return `Turning ${talk} into an action plan…`;
  if (job?.step === 3) return 'Checking who does what, and by when…';
  return `Reading through ${talk}…`;
}

// noteId → the "Started by mistake?" toast on screen.
const mistakes = new Map();

function dismissMistake(noteId) {
  const id = mistakes.get(noteId);
  if (id != null && isToastLive(id)) dismiss(id);
  mistakes.delete(noteId);
}

/**
 * "Started by mistake? Only a few words were captured." with Discard / Keep
 * (Keep, or its ✕, leaves the meeting as it is: "Generate summary" is there
 * when wanted). `onDiscarded` runs once the note is gone.
 */
export function askStartedByMistake(noteId, { onDiscarded } = {}) {
  if (mistakes.has(noteId) && isToastLive(mistakes.get(noteId))) return;
  const id = toast({
    title: 'Started by mistake?',
    description: 'Only a few words were captured. Keep this meeting or discard it.',
    chip: 'Meeting',
    icon: 'alert',
    duration: 0,
    action: { label: 'Keep', onClick: () => mistakes.delete(noteId) },
    secondary: {
      label: 'Discard',
      onClick: async () => {
        mistakes.delete(noteId);
        try {
          await invoke('meeting_discard', { noteId });
          toast({ title: 'Meeting discarded', variant: 'success' });
          onDiscarded?.();
        } catch (e) {
          toast({ title: "Couldn't discard it", description: String(e), variant: 'destructive' });
        }
      },
    },
    onClose: () => mistakes.delete(noteId),
  });
  mistakes.set(noteId, id);
}
