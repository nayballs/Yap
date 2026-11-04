// The quiet-meeting countdown and the check-ins (calls/meeting_guard.rs), in the
// window Rust picked: the meeting notepad if it's on screen, else the main
// window if it's focused (else Rust puts them on the Yap bar or in a Windows
// notification, and no window shows them). Rust decides; this shows what it
// says, in whichever window calls `initMeetingCheckIns(label)`:
//
// `yap-meeting-checkin` { prompt: { id, noteId, kind, title, body, until,
//   surface, primary, secondary } | null }:
//   - kind "quiet": "Ending notes in 28 s…" counting down to `until`, with
//     Keep going (primary) / Stop now and a draining bar. Its ✕ only hides
//     it: the countdown carries on, as on the bar.
//   - kind "calendar" | "hour": "Still in your meeting?" with Stop and
//     summarise (primary) / Keep recording. Its ✕ is Keep recording.
//   - the meeting's other questions, the same way (✕ answers "keep"):
//     "mic" — "Teams is using Contoso H2 but Yap is using Northwind Audio",
//     Use Contoso H2 / Not now (calls/call_audio.rs); "asleep" — "Your PC was
//     asleep", Resume / Summarise (meetings/sleep.rs); "autostart" — "Start
//     notes automatically for Teams calls?" (calls/meeting_detect.rs).
//   Answers go back as `meeting_checkin_respond { id, answer }`; Rust takes
//   the prompt down everywhere (`null`). `meeting_checkin_status` restores
//   it after a reload.
// `yap-meeting-notice` with `surface` = this window (not the main one, whose
//   notices meetingGuard.js shows): "Stopped: the meeting went quiet".
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { toast, dismiss, isToastLive, updateToast } from '../ui/toast.svelte.js';

/** "28 s", "1 min 5 s" — rounded up, as Rust writes it. */
export function secondsLeft(ms) {
  const secs = Math.max(1, Math.ceil(ms / 1000));
  const m = Math.floor(secs / 60);
  const s = secs % 60;
  if (!m) return `${s} s`;
  return s ? `${m} min ${s} s` : `${m} min`;
}

const started = new Set();

/**
 * Show the check-ins Rust addresses to window `label` ("settings" for the
 * main window, "notepad"). Idempotent per label.
 */
export function initMeetingCheckIns(label) {
  if (started.has(label)) return;
  started.add(label);
  // The prompt on screen here: { id, toastId, timer }.
  let shown = null;

  function clear() {
    if (!shown) return;
    clearInterval(shown.timer);
    if (isToastLive(shown.toastId)) dismiss(shown.toastId);
    shown = null;
  }

  function answer(p, a) {
    invoke('meeting_checkin_respond', { id: p.id, answer: a }).catch((e) =>
      toast({ title: "Couldn't answer", description: String(e), variant: 'destructive' })
    );
  }

  function apply(p) {
    if (shown && shown.id !== p?.id) clear();
    if (!p || p.surface !== label || shown) return;
    const counting = typeof p.until === 'number';
    const total = counting ? Math.max(1000, p.until - Date.now()) : 0;
    const progress = () => (counting ? Math.max(0, ((p.until - Date.now()) / total) * 100) : null);
    const toastId = toast({
      kind: 'prompt',
      title: counting ? `Ending notes in ${secondsLeft(p.until - Date.now())}…` : p.title,
      description: p.body,
      chip: 'Meeting',
      icon: p.icon || 'timer',
      duration: 0,
      progress: progress(),
      action: { label: p.primary.label, onClick: () => answer(p, p.primary.id) },
      secondary: { label: p.secondary.label, onClick: () => answer(p, p.secondary.id) },
      // The countdown's ✕ only hides it; a check-in's is Keep recording.
      onClose: () => {
        if (!counting) answer(p, 'keep');
        clear();
      },
    });
    const timer = counting
      ? setInterval(() => {
          const left = p.until - Date.now();
          const patch =
            left > 0
              ? { title: `Ending notes in ${secondsLeft(left)}…`, progress: progress() }
              : { title: 'Ending notes…', progress: 0 };
          if (!updateToast(toastId, patch)) clearInterval(timer);
        }, 250)
      : null;
    shown = { id: p.id, toastId, timer };
  }

  invoke('meeting_checkin_status')
    .then((s) => apply(s?.prompt))
    .catch(() => {});
  listen('yap-meeting-checkin', (e) => apply(e.payload?.prompt));

  // The main window shows its own notices (meetingGuard.js).
  if (label !== 'settings') {
    listen('yap-meeting-notice', (e) => {
      const n = e.payload;
      if (!n?.title || n.surface !== label) return;
      toast({
        kind: 'info',
        title: n.title,
        description: n.body,
        chip: 'Meeting',
        icon: n.icon || '',
        duration: 8_000,
      });
    });
  }
}
