// Dictation alerts in the main window (alert.rs): things the person has to
// act on, shown here while the window is on screen (else Rust puts them on
// the Yap bar, or in a Windows notification).
//
// `yap-alert` { kind, icon, title, body, action: { id, label } | null, timeoutMs }:
//   "pasteBlocked" — the app runs as administrator, so the dictation is on
//     the clipboard instead: "Your text is copied: press Ctrl+V" (elevated.rs);
//   "micBlocked" — Windows' privacy settings keep desktop apps off the
//     microphone, with Open settings (mic_privacy.rs).
// A button's id goes back to Rust (`alert_action`), which does it. Rust has
// already applied the cooldowns (notice.rs); here they're toasts of kind
// "alert" (ui/noticePolicy.js): never held behind a dictation.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { toast } from './ui/toast.svelte.js';

const CHIPS = { clipboard: 'Paste', mic: 'Microphone', call: 'Calls' };

let started = false;

/** Show dictation alerts as toasts. Idempotent; the main window calls it. */
export function initAlerts() {
  if (started) return;
  started = true;
  listen('yap-alert', (e) => show(e.payload));
}

function show(a) {
  if (!a?.title) return;
  toast({
    // An alert (a test run's staged notices may name another kind).
    kind: a.level || 'alert',
    title: a.title,
    description: a.body || '',
    chip: CHIPS[a.icon] || '',
    icon: a.icon || '',
    duration: a.timeoutMs || 10_000,
    action: a.action
      ? {
          label: a.action.label,
          onClick: () => invoke('alert_action', { action: a.action.id }).catch(() => {}),
        }
      : null,
  });
}
