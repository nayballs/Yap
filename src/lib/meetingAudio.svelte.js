// A meeting note's audio kept on disk (meeting_audio.rs), for the Notes view
// and the meeting notepad alike:
// - what each note has (`meeting_audio_note`: recordings, length, what a
//   crash left to transcribe, the speech model "Transcribe again" would use),
//   kept current from `yap-meeting-audio`;
// - its transcription jobs ("Transcribe again", or the part a crash left),
//   from `yap-meeting-redo` { noteId, kind: again|recover, state:
//   running|done|error, percent, model, error };
// - "Transcribe again…" behind the window's confirm dialog
//   (ui/ConfirmDialog via askConfirm), and the crash banner's "Transcribe it";
// - a transcript line's ▶ (`meeting_audio_clip`: both sides of its stretch,
//   as a WAV), one line playing at a time.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { askConfirm } from './ui/contextMenu.svelte.js';
import { toast } from './ui/toast.svelte.js';

export const meetingAudio = $state({
  /** noteId → `NoteAudio` (meeting_audio.rs). */
  byNote: {},
  /** noteId → its latest job. */
  jobs: {},
  /** The line playing: `${noteId}:${index}`, or null. */
  playing: null,
});

let started = false;
const doneListeners = new Set();
// Jobs this window started: it says how they went (every window shows the
// running line; one toast is enough).
const startedHere = new Set();

/** Follow the events (idempotent; every view that shows a meeting calls it). */
export function initMeetingAudio() {
  if (started) return;
  started = true;
  listen('yap-meeting-audio', (e) => {
    const id = e.payload?.noteId;
    const ids = id == null ? Object.keys(meetingAudio.byNote).map(Number) : [id];
    for (const n of ids) if (n in meetingAudio.byNote) refreshAudio(n);
  });
  listen('yap-meeting-redo', (e) => {
    const job = e.payload;
    if (job?.noteId == null) return;
    meetingAudio.jobs[job.noteId] = job;
    if (job.state !== 'running') {
      refreshAudio(job.noteId);
      for (const f of doneListeners) f(job);
      if (!startedHere.delete(job.noteId)) return;
      if (job.state === 'error') {
        toast({
          title: job.kind === 'recover' ? "Couldn't transcribe the rest of the meeting" : "Couldn't transcribe the meeting again",
          description: job.error || '',
          variant: 'destructive',
        });
      } else if (job.kind === 'again') {
        toast({
          title: 'Transcribed again',
          description: job.summary
            ? `With ${job.model}. The summary is still the earlier one: generate it again to match.`
            : `With ${job.model}.`,
          variant: 'success',
        });
      }
    }
  });
}

/** Call `f(job)` when a job ends (a view reloads its note). Returns the unsubscribe. */
export function onJobDone(f) {
  doneListeners.add(f);
  return () => doneListeners.delete(f);
}

/** Ask Rust what note `noteId` has kept. */
export async function refreshAudio(noteId) {
  if (noteId == null) return;
  try {
    const a = await invoke('meeting_audio_note', { noteId });
    meetingAudio.byNote[noteId] = a;
    if (a?.job && !meetingAudio.jobs[noteId]) meetingAudio.jobs[noteId] = a.job;
  } catch {
    /* not a meeting, or gone */
  }
}

/** The job running on a note, if one is. */
export function runningJob(noteId) {
  const j = meetingAudio.jobs[noteId];
  return j?.state === 'running' ? j : null;
}

/**
 * "The last …": "12 seconds weren't", "minute wasn't", "5 minutes weren't"
 * (as meeting_audio.rs `spoken_length` says it in the offer).
 */
export function lastPart(secs) {
  const s = Math.max(1, Math.round(secs || 0));
  if (s === 1) return "second wasn't";
  if (s < 60) return `${s} seconds weren't`;
  const m = Math.floor((s + 30) / 60);
  return m === 1 ? "minute wasn't" : `${m} minutes weren't`;
}

/** What a running job's line says. */
export function jobLine(job) {
  if (!job) return '';
  const pct = job.percent > 0 ? ` · ${job.percent}%` : '';
  return job.kind === 'recover'
    ? `Transcribing the rest of the meeting${pct}`
    : `Transcribing again with ${job.model || 'your speech model'}${pct}`;
}

/** Why "Transcribe again" can't run on `note` now (null: it can). */
export function cantTranscribeAgain(note, recordingThis) {
  const a = meetingAudio.byNote[note?.id];
  if (recordingThis) return 'Stop the recording first';
  if (runningJob(note?.id)) return "It's being transcribed already";
  if (a && !a.recordings) return "This meeting's audio isn't kept";
  return null;
}

/** "Transcribe again…": asked first, then started (progress comes as events). */
export function askTranscribeAgain(note, { title = '', returnFocus = null } = {}) {
  if (!note) return;
  const a = meetingAudio.byNote[note.id];
  const model = a?.model || 'the speech model you use now';
  askConfirm({
    title: title ? `Transcribe “${title}” again?` : 'Transcribe this meeting again?',
    body:
      `Yap runs ${model} over the meeting's audio and replaces its transcript. ` +
      'Your notes stay as they are, and names stay on the lines that still match. ' +
      'The summary stays as it is until you generate it again.',
    confirmLabel: 'Transcribe again',
    busyLabel: 'Starting…',
    danger: false,
    returnFocus,
    run: async () => {
      try {
        startedHere.add(note.id);
        await invoke('meeting_transcribe_again', { noteId: note.id });
      } catch (e) {
        startedHere.delete(note.id);
        toast({ title: "Couldn't transcribe it again", description: String(e), variant: 'destructive' });
        return false;
      }
    },
  });
}

/** The crash banner's "Transcribe it". */
export async function recoverMeeting(noteId) {
  try {
    startedHere.add(noteId);
    await invoke('meeting_recover', { noteId });
  } catch (e) {
    startedHere.delete(noteId);
    toast({ title: "Couldn't transcribe it", description: String(e), variant: 'destructive' });
  }
}

/** Whether line `seg` can be played (placed on its recording's clock, the audio kept). */
export function canPlay(noteId, seg) {
  const a = meetingAudio.byNote[noteId];
  return !!a?.recordings && seg?.startMs != null && seg?.endMs != null && !seg.dictated;
}

let player = null;

/** Stop the line playing. */
export function stopLine() {
  if (player) {
    player.pause();
    URL.revokeObjectURL(player.src);
    player = null;
  }
  meetingAudio.playing = null;
}

/** ▶ on line `i` (`seg`) of note `noteId`; again: stop. */
export async function playLine(noteId, i, seg) {
  const key = `${noteId}:${i}`;
  const again = meetingAudio.playing === key;
  stopLine();
  if (again) return;
  try {
    const bytes = await invoke('meeting_audio_clip', { noteId, ts: seg.ts, startMs: seg.startMs, endMs: seg.endMs });
    const a = new Audio(URL.createObjectURL(new Blob([bytes], { type: 'audio/wav' })));
    a.onended = () => player === a && stopLine();
    player = a;
    meetingAudio.playing = key;
    await a.play();
  } catch (e) {
    stopLine();
    toast({ title: "Couldn't play it", description: String(e), variant: 'destructive' });
  }
}
