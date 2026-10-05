// The calendar in the main window (calendar.rs). Rust owns it: the
// connections, the next week of meetings and the reminder card; this mirrors
// its snapshot (`calendar_status` + the `yap-calendar` event):
//   { connections: [{ id, kind: 'google'|'outlook'|'ics', label, detail,
//                     syncedTs, error, reconnect }],
//     events: [{ key, title, start, end, attendees, with, service,
//                serviceLabel, hasLink, tentative, conflict, noteId }],
//     syncing, google: { available, waiting },
//     card: { id, kind: 'remind'|'switch', key, title, start, end, with,
//             serviceLabel, hasLink } | null,
//     nudge: { hub, afterMeeting }, maxConnections }
//
// The card ("Design review · In 1 min") shows as a sticky toast that counts
// down: Join & take notes / Start notes / Snooze; ✕ or Esc dismisses it. A
// `switch` card (recording another meeting) offers to switch notes instead.
// Rust takes it back when it's answered elsewhere (the Windows notification)
// or runs out, 5 minutes after the start.
//
// Also here: the one-time "Connect your calendar" nudge after a meeting ends
// (with no calendar connected; "Not now" means never), and refreshing the
// open note when a recording that just started was tied to a meeting
// (`yap-calendar-note-linked`).
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { toast, dismiss, isToastLive, updateToast } from './ui/toast.svelte.js';
import { noteRequest } from './meetingDetect.svelte.js';

export const calendar = $state({
  loaded: false,
  connections: [],
  events: [],
  syncing: false,
  google: { available: false, waiting: false },
  card: null,
  nudge: { hub: false, afterMeeting: false },
  maxConnections: 8,
});

let started = false;
// Set by the main window: switch view, open a Settings section, which view is up.
let nav = { showView: () => {}, openSettings: () => {}, activeView: () => '' };
// The card on screen: { id, toastId, timer }.
let shown = null;
let wasRecording = false;

/**
 * Load the snapshot and follow changes. Idempotent; the main window calls it
 * with `showView(view)`, `openSettings(section)` and `activeView()`.
 */
export function initCalendar(opts = {}) {
  nav = { ...nav, ...opts };
  if (started) return;
  started = true;
  invoke('calendar_status')
    .then(apply)
    .catch(() => {});
  invoke('meeting_state')
    .then((s) => (wasRecording = !!s?.recording))
    .catch(() => {});
  listen('yap-calendar', (e) => apply(e.payload));
  listen('yap-calendar-connected', (e) => {
    const label = e.payload?.label;
    toast({ title: 'Calendar connected', description: label ? `Yap can now see the meetings on ${label}.` : undefined, variant: 'success' });
  });
  listen('yap-calendar-error', (e) => {
    toast({ title: "Couldn't connect your calendar", description: String(e.payload || ''), variant: 'destructive' });
  });
  listen('yap-calendar-note-linked', (e) => {
    // The open note just got its meeting's title and attendees: reload it.
    const id = e.payload?.noteId;
    if (id != null && nav.activeView() === 'notes') noteRequest.pending = { id, stop: false };
  });
  listen('yap-meeting-state', (e) => onMeetingState(e.payload));
  window.addEventListener('keydown', onKeydown);
}

function apply(s) {
  if (!s) return;
  Object.assign(calendar, s, { loaded: true });
  const c = s.card;
  if (shown && shown.id !== c?.id) clearShown();
  if (c && !shown) showCard(c);
}

/** "In 2 min", "In 1 min", "Starting now", "Started 3 min ago". */
export function whenText(start, now = Date.now() / 1000) {
  const diff = start - now;
  if (diff >= 90) return `In ${Math.round(diff / 60)} min`;
  if (diff >= 30) return 'In 1 min';
  if (diff > 0) return 'Starting now';
  const ago = Math.floor(-diff / 60);
  return ago >= 1 ? `Started ${ago} min ago` : 'Started just now';
}

/** "14:30" in the PC's own time. */
export function clockTime(secs) {
  return new Date(secs * 1000).toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' });
}

function cardText(c) {
  const parts = [whenText(c.start)];
  if (c.serviceLabel) parts.push(c.serviceLabel);
  if (c.with) parts.push(`with ${c.with}`);
  const line = parts.join(' · ');
  return c.kind === 'switch' ? `${line}. Switch your notes to it?` : line;
}

function showCard(c) {
  const switching = c.kind === 'switch';
  const primary = c.hasLink
    ? { label: switching ? 'Join & switch notes' : 'Join & take notes', answer: 'join' }
    : { label: switching ? 'Switch notes' : 'Start notes', answer: 'start' };
  const toastId = toast({
    title: c.title,
    description: cardText(c),
    chip: switching ? 'Next meeting' : 'Meeting',
    icon: 'calendar',
    duration: 0,
    action: { label: primary.label, onClick: () => answer(c, primary.answer) },
    secondary: c.hasLink
      ? { label: switching ? 'Switch notes' : 'Start notes', onClick: () => answer(c, 'start') }
      : null,
    tertiary: { label: 'Snooze 2 min', onClick: () => answer(c, 'snooze') },
    onClose: () => answer(c, 'dismiss'),
  });
  // Keep "In 1 min" true while it's up.
  const timer = setInterval(() => {
    if (!updateToast(toastId, { description: cardText(c) })) clearInterval(timer);
  }, 1000);
  shown = { id: c.id, toastId, timer };
}

function clearShown() {
  if (!shown) return;
  clearInterval(shown.timer);
  if (isToastLive(shown.toastId)) dismiss(shown.toastId);
  shown = null;
}

async function answer(c, action) {
  if (shown?.id === c.id) clearShown();
  try {
    const noteId = await invoke('calendar_card', { id: c.id, action });
    if (noteId != null) {
      toast({
        title: c.kind === 'switch' ? 'Switched notes' : 'Taking notes',
        description: `${c.title}. Let people know you're taking notes.`,
        variant: 'success',
      });
    }
  } catch (e) {
    toast({ title: "Couldn't start meeting notes", description: String(e), variant: 'destructive' });
  }
}

// Esc dismisses the card, unless it's meant for something else: a dialog
// (Settings) is open, or the person is typing.
function onKeydown(e) {
  if (e.key !== 'Escape' || !shown || e.defaultPrevented) return;
  const typing = e.target?.closest?.('input, textarea, select, [contenteditable="true"]');
  const modal = [...document.querySelectorAll('[aria-modal="true"]')].some((el) => el.offsetParent !== null);
  if (typing || modal) return;
  const c = calendar.card;
  if (c && c.id === shown.id) {
    e.preventDefault();
    answer(c, 'dismiss');
  }
}

// ---- the nudge after a meeting ------------------------------------------------------------

function onMeetingState(s) {
  const recording = !!s?.recording;
  const ended = wasRecording && !recording;
  wasRecording = recording;
  if (!ended || !calendar.nudge.afterMeeting) return;
  // After the meeting's own toasts ("Meeting ended", the action plan).
  setTimeout(() => {
    if (!calendar.nudge.afterMeeting) return;
    invoke('calendar_nudge', { action: 'shown' }).catch(() => {});
    toast({
      title: 'Connect your calendar',
      description: 'See your upcoming meetings, and get notes and action items with everyone’s names.',
      chip: 'Tip',
      icon: 'calendar',
      duration: 0,
      action: { label: 'Connect calendar', onClick: () => openConnectors() },
      secondary: { label: 'Not now', onClick: () => dismissNudge() },
    });
  }, 6_000);
}

// ---- actions --------------------------------------------------------------------------------

/** Settings → Connectors. */
export function openConnectors() {
  nav.openSettings('connectors');
}

/** "Not now" on "Connect your calendar": it never comes back. */
export function dismissNudge() {
  calendar.nudge = { hub: false, afterMeeting: false };
  invoke('calendar_nudge', { action: 'dismiss' }).catch(() => {});
}

export async function syncCalendar() {
  try {
    const s = await invoke('calendar_sync');
    apply(s);
  } catch (e) {
    toast({ title: "Couldn't sync your calendar", description: String(e), variant: 'destructive' });
  }
}

/**
 * A meeting row's action: 'open' (its note, made ahead of time if needed),
 * 'start', 'joinStart', 'join', 'switch', 'joinSwitch'.
 */
export async function eventAction(ev, action) {
  try {
    const noteId = await invoke('calendar_event', { key: ev.key, action });
    if (action === 'open') {
      if (noteId != null) {
        nav.showView('notes');
        noteRequest.pending = { id: noteId, stop: false };
      }
      return;
    }
    if (action === 'join') {
      toast({ title: `Joining ${ev.title}`, description: 'Opening the meeting in your browser or its app.' });
      return;
    }
    toast({
      title: action.includes('witch') ? 'Switched notes' : 'Taking notes',
      description: `${ev.title}. Let people know you're taking notes.`,
      variant: 'success',
    });
  } catch (e) {
    toast({ title: "Couldn't do that", description: String(e), variant: 'destructive' });
  }
}
