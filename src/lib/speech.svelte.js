// Spoken replies, as a page sees them (tts.rs): which message is being read
// aloud (`speech.speakingId`, from `yap-tts-state`), ▶ / ■ for an answer,
// and a toast when reading one failed. One store per window: `initSpeech`
// names the message ids this window shows ("chat:", "notes:", "preview" in
// the main window; "notepad:" in the notepad), so a failure is said once,
// where its button is.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { toast } from './ui/toast.svelte.js';

export const speech = $state({ speakingId: null });

let owned = [];
let started = false;

const mine = (id) => !!id && owned.some((p) => id.startsWith(p));

export function initSpeech(prefixes) {
  owned = [...new Set([...owned, ...prefixes])];
  if (started) return;
  started = true;
  invoke('tts_state')
    .then((s) => (speech.speakingId = s?.speaking ? (s.messageId ?? null) : null))
    .catch(() => {});
  listen('yap-tts-state', (e) => {
    const s = e.payload || {};
    if (s.speaking) speech.speakingId = s.messageId ?? null;
    else if (!s.messageId || s.messageId === speech.speakingId) speech.speakingId = null;
    if (s.error && mine(s.messageId)) {
      toast({
        title: s.messageId === 'preview' ? "Couldn't play the preview" : "Couldn't read it aloud",
        description: String(s.error),
        variant: 'destructive',
      });
    }
  });
}

/** Read `text` (markdown) aloud as `id`, or stop it if it's the one being read. */
export async function readAloud(text, id) {
  if (speech.speakingId === id) {
    invoke('tts_stop').catch(() => {});
    return;
  }
  try {
    await invoke('tts_speak', { text, messageId: id });
  } catch (e) {
    toast({ title: "Couldn't read it aloud", description: String(e), variant: 'destructive' });
  }
}

/** Stop the voice if it's reading one of `prefix`'s messages (any, without one). */
export function stopReading(prefix = '') {
  if (speech.speakingId && speech.speakingId.startsWith(prefix)) invoke('tts_stop').catch(() => {});
}
