// Meeting guard rails in the main window (meeting_guard.rs, capture.rs).
// Rust decides; this shows what it says:
//
// Rust puts each in one place (notice.rs): this window only gets what it's
// meant to show.
// `yap-meeting-limit` { warning: { noteId, stopAt, title, body, inApp } | null }:
//   the maximum-length warning ("Notes stop in 5 minutes") as a sticky toast
//   with Keep going (`meeting_keep_going`), counting down while it's up —
//   when `inApp` (else it's on the Yap bar or a Windows notification).
//   Rust takes it back with `null`: kept going (here or in the Windows
//   notification), the limit moved in Settings, or the recording ended.
//   `meeting_limit_status` restores it after a reload.
// `yap-meeting-notice` { kind, title, body, icon, variant, settings, noteId }:
//   a one-off toast: the screen-share tip when hiding is off ("Your meeting
//   notes show up in screen shares and screenshots", with Update settings →
//   `settings`), a stop Yap made itself (the length limit, a call ending),
//   "Taking notes" from the meeting shortcut, or something about the
//   recording itself (kind "warning", meeting.rs: transcription falling
//   behind; "Yap can't hear the call" when one side went quiet; "Yap
//   couldn't transcribe part of this meeting", once per meeting).
//   A notice addressed to another window (`surface`: "notepad") isn't
//   shown here.
// The quiet-meeting countdown and the "Still in your meeting?" check-ins
// come through meetingCheckIn.js, when Rust picks this window for them.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { toast, dismiss, isToastLive, updateToast } from '../ui/toast.svelte.js';
import { initMeetingCheckIns } from './meetingCheckIn.js';

let started = false;
let openSettings = () => {};
// The length warning on screen: { stopAt, toastId, timer }.
let shown = null;

/**
 * Follow the guard rails. Idempotent; the main window calls it with
 * `openSettings` (a Settings section, e.g. 'meetings').
 */
export function initMeetingGuard(opts = {}) {
  if (opts.openSettings) openSettings = opts.openSettings;
  if (started) return;
  started = true;
  invoke('meeting_limit_status')
    .then((s) => applyLimit(s?.warning))
    .catch(() => {});
  listen('yap-meeting-limit', (e) => applyLimit(e.payload?.warning));
  listen('yap-meeting-notice', (e) => showNotice(e.payload));
  initMeetingCheckIns('settings');
}

/** "5 minutes", "1 minute", "40 seconds" — rounded up, for a countdown. */
export function stopsIn(ms) {
  const plural = (n, unit) => `${n} ${unit}${n === 1 ? '' : 's'}`;
  if (ms >= 60_000) return plural(Math.ceil(ms / 60_000), 'minute');
  return plural(Math.max(1, Math.ceil(ms / 1000)), 'second');
}

function clearShown() {
  if (!shown) return;
  clearInterval(shown.timer);
  if (isToastLive(shown.toastId)) dismiss(shown.toastId);
  shown = null;
}

function applyLimit(w) {
  // In one place: on the bar (or a Windows notification) it isn't here too.
  if (w && w.inApp === false) w = null;
  if (shown && shown.stopAt !== w?.stopAt) clearShown();
  if (!w || shown) return;
  const toastId = toast({
    kind: 'prompt',
    title: w.title,
    description: w.body,
    chip: 'Meeting',
    icon: 'timer',
    duration: 0,
    action: { label: 'Keep going', onClick: keepGoing },
    // ✕ just hides it: the recording still stops at the limit.
    onClose: () => clearShown(),
  });
  // Keep "Notes stop in …" true while it's up.
  const timer = setInterval(() => {
    const left = w.stopAt - Date.now();
    if (!updateToast(toastId, { title: `Notes stop in ${stopsIn(Math.max(0, left))}` })) {
      clearInterval(timer);
    }
  }, 1000);
  shown = { stopAt: w.stopAt, toastId, timer };
}

async function keepGoing() {
  clearShown();
  try {
    await invoke('meeting_keep_going');
  } catch (e) {
    toast({ title: "Couldn't keep going", description: String(e), variant: 'destructive' });
  }
}

const NOTICE_CHIPS = { screenShare: 'Meeting', stopped: 'Meeting', recording: 'Recording', warning: 'Meeting' };

function showNotice(n) {
  if (!n?.title || (n.surface && n.surface !== 'settings')) return;
  toast({
    // A recording's own trouble is a warning (not twice in ten minutes);
    // the rest are notices. Long enough to read two sentences (hovering
    // pauses it).
    kind: n.kind === 'warning' ? 'warning' : 'info',
    title: n.title,
    description: n.body,
    variant: n.variant === 'success' ? 'success' : 'default',
    chip: NOTICE_CHIPS[n.kind] ?? '',
    icon: n.icon || '',
    duration: n.kind === 'warning' ? 12_000 : n.kind === 'screenShare' ? 10_000 : 6_000,
    action: n.settings ? { label: 'Update settings', onClick: () => openSettings(n.settings) } : null,
  });
}
