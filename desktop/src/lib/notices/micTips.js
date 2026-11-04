// "Yap didn't hear you clearly" in the main window (dictation/mic_tips.rs): after
// a dictation whose signal was weak — quiet, noisy or clipped — or that came
// through a Bluetooth headset in its call profile, one concrete tip, while the
// person is looking at Yap (else Rust puts it on the Yap bar). Rare: Rust
// counts them (mic_tips.json) and "Show tips" turns them off.
//
// `yap-mic-tip` { kind, title, body, action, label }: the button's id goes
// back to Rust (`alert_action`: Windows' sound settings, or Settings →
// Dictation → Microphone). A toast of kind "tip" (ui/noticePolicy.js):
// held while a dictation runs, dropped if it waits too long unseen.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { toast } from '../ui/toast.svelte.js';

let started = false;

/** Show mic-quality tips as toasts. Idempotent; the main window calls it. */
export function initMicTips() {
  if (started) return;
  started = true;
  listen('yap-mic-tip', (e) => show(e.payload));
}

function show(t) {
  if (!t?.title) return;
  toast({
    kind: 'tip',
    title: t.title,
    description: t.body || '',
    chip: 'Microphone',
    icon: 'mic',
    duration: 12_000,
    action: t.action
      ? {
          label: t.label || 'Settings',
          onClick: () => invoke('alert_action', { action: t.action }).catch(() => {}),
        }
      : null,
  });
}
