// Call detection prompts in the main window (meeting_detect.rs). Rust notices
// a call (Teams, Zoom, Meet, Slack, Discord, Webex, …) and owns the decisions;
// this mirrors its snapshot (`meeting_detect_status` + the
// `yap-meeting-detect` event):
//   { enabled, style: 'popup' | 'quiet',
//     apps: [{ id, label, asks, asksByDefault }],
//     calls: [{ id, app, label, noun, since, noteId }],
//     prompt: { id, kind: 'start' | 'end', callId, app, title, body, accept,
//               decline, never, noteId, inApp, quiet, fadeMs,
//               autoStartAt } | null }
// and shows the prompt as a sticky toast while `inApp` (Rust decides that:
// WebView2 reports `visible` even for a hidden window, so the page can't;
// quiet prompts stay out of the window). "Record notes" / "Not now" answer a
// start prompt, "Stop and summarise" / "Keep recording" an end prompt; the
// toast's ✕ counts as the quiet answer. A start prompt also offers `never`
// ("Don't ask for Teams", a small link under the buttons) and, left alone,
// fades after `fadeMs` (30 s, OpenWhispr's auto-dismiss) as "Not now" — or,
// with `autoStartAt` (the opt-in "Start notes automatically"), counts down
// "Starting notes in 7…" instead, Rust starting the notes when it runs out
// (Start now / Not now / Esc answer it sooner). A
// prompt Rust withdraws (the call ended, the other surface answered) takes
// its toast with it. `apps` feeds Settings → General → Meetings.
//
// `yap-meeting-detect-choice` { app, asks, confirm }: Yap switched an app
// off itself ("Don't ask for Teams", from either surface). Settings adopts it
// into its config copy; `confirm` (the window is on screen) is shown here as
// a brief toast with a way to Settings → General → Meetings.
//
// `yap-meeting-open-note` { noteId, stop } opens a note in Notes (the new
// meeting note, or the one to stop): the main window switches view and
// NotesView consumes `noteRequest`, stopping the recording itself when asked
// so its usual Meeting Notes summary runs.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { toast, dismiss, isToastLive, updateToast } from './ui/toast.svelte.js';

export const meetingDetect = $state({ enabled: true, style: 'popup', apps: [], calls: [], prompt: null });

/** A note for NotesView to open: { id, stop } (it clears `pending`). */
export const noteRequest = $state({ pending: null });

let started = false;
let showNotes = () => {};
let openSettings = () => {};
// The prompt on screen: { id, toastId }.
let shown = null;

/**
 * Load the snapshot and follow changes. Idempotent; the main window calls it
 * with `showNotes`, which switches it to the Notes view, and `openSettings`
 * (a Settings section, e.g. 'general#meetings').
 */
export function initMeetingDetect(opts = {}) {
  if (opts.showNotes) showNotes = opts.showNotes;
  if (opts.openSettings) openSettings = opts.openSettings;
  if (started) return;
  started = true;
  invoke('meeting_detect_status')
    .then(apply)
    .catch(() => {});
  listen('yap-meeting-detect', (e) => apply(e.payload));
  listen('yap-meeting-detect-choice', (e) => confirmChoice(e.payload));
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
    stopCountdown();
  }
  if (p && !shown) showPrompt(p);
}

function showPrompt(p) {
  const [yes, no] = p.kind === 'start' ? ['record', 'dismiss'] : ['stop', 'keep'];
  // `autoStartAt` (Settings → "Start notes automatically"): Rust starts the
  // notes when it runs out; the toast counts down and stays till then.
  const counting = p.autoStartAt != null;
  const toastId = toast({
    title: p.title,
    description: counting ? countdownText(p.autoStartAt) : p.body,
    chip: p.kind === 'start' ? 'Call' : 'Call ended',
    icon: 'call',
    duration: counting ? 0 : (p.fadeMs ?? 0),
    action: { label: counting ? 'Start now' : p.accept, onClick: () => answer(p, yes) },
    secondary: { label: p.decline, onClick: () => answer(p, no) },
    tertiary: p.never ? { label: p.never, onClick: () => answer(p, 'never') } : null,
    onClose: () => answer(p, no),
    onExpire: () => answer(p, no),
  });
  shown = { id: p.id, toastId };
  if (counting) startCountdown(p, toastId);
}

// ---- the auto-start countdown in the toast (Esc cancels, as on the bar) ----
let countdown = null; // { timer, prompt, toastId }

function countdownText(at) {
  const secs = Math.max(0, Math.ceil((at - Date.now()) / 1000));
  return secs > 0
    ? `Starting notes in ${secs}… Let people know you're taking notes.`
    : 'Starting notes…';
}

function startCountdown(p, toastId) {
  stopCountdown();
  const timer = setInterval(() => {
    if (!updateToast(toastId, { description: countdownText(p.autoStartAt) })) stopCountdown();
  }, 250);
  countdown = { timer, prompt: p, toastId };
  window.addEventListener('keydown', onCountdownKey, true);
}

function stopCountdown() {
  if (!countdown) return;
  clearInterval(countdown.timer);
  countdown = null;
  window.removeEventListener('keydown', onCountdownKey, true);
}

function onCountdownKey(e) {
  if (e.key !== 'Escape' || !countdown) return;
  e.stopPropagation();
  const { prompt, toastId } = countdown;
  stopCountdown();
  dismiss(toastId);
  answer(prompt, 'dismiss');
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

/** "Won't ask about Teams calls", when Rust says the window is on screen. */
function confirmChoice(c) {
  if (!c?.confirm) return;
  toast({
    title: c.confirm,
    variant: 'success',
    duration: 6000,
    action: { label: 'Open Settings', onClick: () => openSettings('general#meetings') },
  });
}

function openNote(payload) {
  const id = payload?.noteId;
  if (id == null) return;
  showNotes();
  noteRequest.pending = { id, stop: !!payload.stop };
}
