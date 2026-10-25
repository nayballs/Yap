// Dictation alerts in the main window (notices/alert.rs): things the person has to
// act on, shown here while the window is on screen (else Rust puts them on
// the Yap bar, or in a Windows notification).
//
// `yap-alert` { kind, icon, title, body, action: { id, label } | null, timeoutMs }:
//   "pasteBlocked" — the dictation is on the clipboard instead of pasted:
//     the app runs as administrator ("Your text is copied: press Ctrl+V",
//     pasting/elevated.rs), Windows didn't take the keys ("Couldn't paste ·
//     it's copied, press Ctrl+V") or nothing that takes text had the focus
//     ("Select a text box first"): pasting/text_injector.rs `Left`;
//   "clipboardBusy" — edit mode's Ctrl+C left the clipboard alone, or
//     couldn't put it back (pasting/text_injector.rs `Trouble`);
//   "micBlocked" — Windows' privacy settings keep desktop apps off the
//     microphone, with Open settings (dictation/mic_privacy.rs);
//   "configReset" — config.json couldn't be read: set aside and the
//     settings reset (all, or the ones it couldn't read), with Show file
//     (config.rs);
//   "dictationCapped" — a dictation stopped at its length limit, pasted (or
//     not), with Recover text (dictation/cap.rs);
//   "lastTranscript" — "Copied your last dictation", "Nothing to paste
//     yet" (pasting/last.rs: Paste / Copy last transcript's shortcuts).
// A button's id goes back to Rust (`alert_action`), which does it. Rust has
// already applied the cooldowns (notices/notice.rs); here they're toasts of kind
// "alert" (ui/noticePolicy.js): never held behind a dictation.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { toast } from '../ui/toast.svelte.js';

// "assistant": the Voice Assistant has no AI model, or couldn't answer
// (chat/assistant.rs), with Open settings → Language Models.
// "notes": Yap closed during a meeting, with Transcribe it (meetings/meeting_audio.rs).
// "snip": Snip to Chat couldn't start or save (snip.rs).
// "save": a save that failed by itself, with Try again, and "Saved" after it
// (shell/saving.rs; a command's failed save is saveErrors.js's toast).
const CHIPS = { clipboard: 'Paste', mic: 'Microphone', call: 'Calls', assistant: 'Voice Assistant', notes: 'Meetings', settings: 'Settings', snip: 'Snip to Chat', save: 'Saving' };

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
