// Spoken replies, as a page sees them (tts.rs): which message is being read
// aloud (`speech.speakingId`, from `yap-tts-state`), ▶ / ■ for an answer or
// a note — read aloud as Read aloud reads any selection (read_aloud.rs: the
// reading card on the Yap bar, its words lit up as they're said) — and a
// toast when reading one failed. One store per window: `initSpeech`
// names the message ids this window shows ("chat:", "notes:", "preview" in
// the main window; "notepad:" in the notepad), so a failure is said once,
// where its button is.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { toast } from '../ui/toast.svelte.js';

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

/** Where a message id comes from, for the reading card. */
const sourceOf = (id) => (id?.startsWith('note:') ? 'note' : 'chat');

/**
 * Read `text` (markdown) aloud as `id`, with the reading card on the Yap
 * bar, or stop it if it's the one being read.
 */
export async function readAloud(text, id) {
  if (speech.speakingId === id) {
    invoke('tts_stop').catch(() => {});
    return;
  }
  try {
    await invoke('read_aloud_text', { text, messageId: id, markdown: true, source: sourceOf(id) });
  } catch (e) {
    toast({ title: "Couldn't read it aloud", description: String(e), variant: 'destructive' });
  }
}

/**
 * A spoken reply (Voice replies on: an answer read out by itself, chat_send's
 * `speak`): just the voice, as a conversation goes — no reading card, which
 * is for what you asked to have read (▶, Read aloud).
 */
export async function speakReply(text, id) {
  try {
    await invoke('tts_speak', { text, messageId: id });
  } catch (e) {
    toast({ title: "Couldn't read it aloud", description: String(e), variant: 'destructive' });
  }
}

/**
 * The Read aloud shortcut pressed in one of Yap's own windows (the global
 * hook never sees keys there): what's selected on the page — or in the
 * focused text box — read aloud with the reading card. Nothing selected:
 * a reading going on stops (the shortcut is its own Stop), else the card
 * says how.
 */
export function readPageSelection() {
  let text = window.getSelection()?.toString() ?? '';
  const el = document.activeElement;
  if (!text.trim() && el && (el.tagName === 'TEXTAREA' || el.tagName === 'INPUT') && el.selectionEnd > el.selectionStart) {
    text = String(el.value ?? '').slice(el.selectionStart, el.selectionEnd);
  }
  if (!text.trim() && speech.speakingId) {
    invoke('read_aloud_action', { action: 'stop' }).catch(() => {});
    return;
  }
  invoke('read_aloud_text', { text, markdown: false, source: 'yap' }).catch(() => {});
}

/** Stop the voice if it's reading one of `prefix`'s messages (any, without one). */
export function stopReading(prefix = '') {
  if (speech.speakingId && speech.speakingId.startsWith(prefix)) invoke('tts_stop').catch(() => {});
}
