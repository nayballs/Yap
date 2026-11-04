// "Contoso H2 connected · Use it for dictation?" in the main window
// (dictation/mics/offer.rs): a microphone new to Yap was plugged in while the
// person is looking at Yap (else Rust puts it on the Yap bar). Use it puts it
// first in the order (Settings → Dictation → Microphone); Not now (or ✕)
// means don't ask about it again; running out is no answer at all.
//
// `yap-mic-offer` { id, title, body } → `mic_offer_respond` { id, answer:
// "use" | "later" | "expire" }. A toast of kind "info": held while a
// dictation runs (ui/noticePolicy.js), as Rust holds it too.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { toast } from '../ui/toast.svelte.js';

let started = false;

/** Ask about new microphones as toasts. Idempotent; the main window calls it. */
export function initMicOffers() {
  if (started) return;
  started = true;
  listen('yap-mic-offer', (e) => ask(e.payload));
}

function ask(o) {
  if (!o?.id || !o.title) return;
  const answer = (a) => invoke('mic_offer_respond', { id: o.id, answer: a }).catch(() => {});
  toast({
    kind: 'info',
    title: o.title,
    description: o.body || '',
    chip: 'Microphone',
    icon: 'mic',
    duration: 30_000,
    action: { label: 'Use it', onClick: () => answer('use') },
    secondary: { label: 'Not now', onClick: () => answer('later') },
    onClose: () => answer('later'),
    onExpire: () => answer('expire'),
  });
}
