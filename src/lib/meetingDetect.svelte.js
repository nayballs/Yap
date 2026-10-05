// Call detection prompts in the main window (meeting_detect.rs). Rust notices
// a call (Teams, Zoom, Meet, Slack, Discord, Webex, …) and owns the decisions;
// this mirrors its snapshot (`meeting_detect_status` + the
// `yap-meeting-detect` event):
//   { enabled, calls: [{ id, app, label, noun, since, noteId }],
//     prompt: { id, kind: 'start' | 'end', callId, app, title, body, accept,
//               decline, noteId, inApp } | null }
// and shows the prompt as a sticky toast while `inApp` (Rust decides that:
// WebView2 reports `visible` even for a hidden window, so the page can't).
// "Record notes" / "Not now" answer a start prompt, "Stop and summarise" /
// "Keep recording" an end prompt; the toast's ✕ counts as the quiet answer.
// A prompt Rust withdraws (the call ended, the other surface answered) takes
// its toast with it.
//
// `yap-meeting-open-note` { noteId, stop } opens a note in Notes (the new
// meeting note, or the one to stop): the main window switches view and
// NotesView consumes `noteRequest`, stopping the recording itself when asked
// so its usual Meeting Notes summary runs.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { toast, dismiss, isToastLive } from './ui/toast.svelte.js';

export const meetingDetect = $state({ enabled: true, calls: [], prompt: null });

/** A note for NotesView to open: { id, stop } (it clears `pending`). */
export const noteRequest = $state({ pending: null });

let started = false;
let showNotes = () => {};
// The prompt on screen: { id, toastId }.
let shown = null;

/**
 * Load the snapshot and follow changes. Idempotent; the main window calls it
 * with `showNotes`, which switches it to the Notes view.
 */
export function initMeetingDetect(opts = {}) {
  if (opts.showNotes) showNotes = opts.showNotes;
  if (started) return;
  started = true;
  invoke('meeting_detect_status')
    .then(apply)
    .catch(() => {});
  listen('yap-meeting-detect', (e) => apply(e.payload));
  listen('yap-meeting-open-note', (e) => openNote(e.payload));
}

function apply(s) {
  if (!s) return;
  Object.assign(meetingDetect, s);
  const p = s.prompt && s.prompt.inApp ? s.prompt : null;
  if (shown && shown.id !== p?.id) {
    // Answered elsewhere, moot, or replaced.
    if (isToastLive(shown.toastId)) dismiss(shown.toastId);
    shown = null;
  }
  if (p && !shown) showPrompt(p);
}

function showPrompt(p) {
  const [yes, no] = p.kind === 'start' ? ['record', 'dismiss'] : ['stop', 'keep'];
  const toastId = toast({
    title: p.title,
    description: p.body,
    chip: p.kind === 'start' ? 'Call' : 'Call ended',
    icon: 'call',
    duration: 0,
    action: { label: p.accept, onClick: () => answer(p, yes) },
    secondary: { label: p.decline, onClick: () => answer(p, no) },
    onClose: () => answer(p, no),
  });
  shown = { id: p.id, toastId };
}

async function answer(p, action) {
  if (shown?.id === p.id) shown = null;
  try {
    const noteId = await invoke('meeting_detect_respond', { promptId: p.id, action });
    if (action === 'record' && noteId != null) {
      toast({
        title: 'Recording notes',
        description: 'Your mic is "You", the call is "Them". Let people know you\'re taking notes.',
        variant: 'success',
      });
    }
  } catch (e) {
    toast({
      title: action === 'record' ? "Couldn't record the call" : "Couldn't do that",
      description: String(e),
      variant: 'destructive',
    });
  }
}

function openNote(payload) {
  const id = payload?.noteId;
  if (id == null) return;
  showNotes();
  noteRequest.pending = { id, stop: !!payload.stop };
}
