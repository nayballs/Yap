// Read aloud on the Yap bar (read_aloud.rs): the reading card's state — the
// sentence being read (its words and what's between them), the word being
// said, paused or not, the speed — or a short note ("Nothing selected").
// `yap-read` brings the whole card on every new sentence or state;
// `yap-read-word` only the word, as it changes (a few times a second).
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';

export const reading = $state({
  active: false,
  session: 0,
  // 'reading' | 'paused' | 'note' | 'idle'
  phase: 'idle',
  messageId: null,
  source: null,
  index: 0,
  count: 0,
  // [{ text, w? }]: w = the word's index (none between words).
  parts: [],
  word: null,
  rate: 1,
  // 'engine' | 'audio' | 'estimate': how the sentence's word times were found.
  timing: null,
  engine: null,
  note: null,
  cut: false,
  made: 0,
  hotkey: '',
});

let started = false;

/** Listen for the page's whole life (the overlay's App calls this once). */
export function initReading() {
  if (started) return;
  started = true;
  invoke('read_aloud_state')
    .then((v) => v && Object.assign(reading, v))
    .catch(() => {});
  listen('yap-read', (e) => {
    if (e.payload) Object.assign(reading, e.payload);
  });
  listen('yap-read-word', (e) => {
    const p = e.payload || {};
    if (p.session !== reading.session) return;
    if (p.index !== reading.index) return;
    reading.word = p.word ?? null;
  });
}

/** A button on the card: pause, resume, toggle, back, next, faster, slower, stop. */
export function readAction(action) {
  return invoke('read_aloud_action', { action }).catch((e) =>
    invoke('frontend_log', { msg: `bar: read aloud ${action} failed: ${e}` }).catch(() => {})
  );
}
