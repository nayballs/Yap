// The dictation side of the Yap bar (the overlay as it always was): the
// pipeline's state, the scrolling amplitude waveform, the live partial text
// and the last error. Listened to for the page's whole life — not only while
// the capsule shows — because `yap-error` arrives just before the
// `yap-state` "error" that brings the capsule up.
import { listen } from '@tauri-apps/api/event';

// Scrolling amplitude waveform (Claude Code style): newest bar on the right.
const MAX_BARS = 80;
const AMP_GAIN = 3.5;

export const dictation = $state({
  // idle | recording | processing | processing-slow | needs-model | error
  state: 'idle',
  errorMsg: 'Transcription failed',
  history: [],
  // Revealed partial words (rendered as keyed spans).
  words: [],
});

// ---- Live partial transcript (streaming preview) ----
// The backend emits a full partial line every ~500 ms tick. Dumping a whole
// tick's words at once reads chunky, so the reveal is paced word-by-word
// between ticks (typewriter feel, drained fast enough to stay real-time).
let queue = []; // words waiting to be revealed
let revealTimer = null;

function stopReveal() {
  if (revealTimer) {
    clearTimeout(revealTimer);
    revealTimer = null;
  }
}
function clearPartial() {
  stopReveal();
  queue = [];
  dictation.words = [];
}
function revealStep() {
  revealTimer = null;
  const w = queue.shift();
  if (w === undefined) return;
  dictation.words = [...dictation.words, w];
  if (queue.length) {
    // Drain the pending words within ~400 ms — comfortably before the next
    // backend tick — but never faster than 40 ms/word or slower than 110 ms.
    const pace = Math.max(40, Math.min(110, 400 / queue.length));
    revealTimer = setTimeout(revealStep, pace);
  }
}
function onPartial(text) {
  const tw = text.split(/\s+/).filter(Boolean);
  const words = dictation.words;
  if (tw.length < words.length) {
    // The decode changed its mind and the line shrank (rare wholesale
    // replace) — snap; pacing a rewrite would just look laggy.
    clearPartial();
    dictation.words = tw;
    return;
  }
  // Words already on screen that changed (smart_diff tail rewrite) are wrong
  // — correct them in place immediately; only genuinely-new words get paced.
  let common = 0;
  while (common < words.length && words[common] === tw[common]) common++;
  if (common < words.length) dictation.words = tw.slice(0, words.length);
  queue = tw.slice(dictation.words.length);
  if (!revealTimer && queue.length) revealStep();
}

let started = false;

export function initDictation() {
  if (started) return;
  started = true;
  listen('yap-state', (e) => {
    const state = e.payload;
    dictation.state = state;
    if (state !== 'recording') dictation.history = [];
    // Keep the partial visible through the brief "processing" state, then
    // drop it once we're idle/needs-model/error so it never lingers.
    if (state === 'idle' || state === 'needs-model' || state === 'error' || state === 'recording') {
      clearPartial();
    }
  });
  listen('yap-error', (e) => {
    if (e.payload) dictation.errorMsg = e.payload;
  });
  listen('yap-partial', (e) => {
    if (typeof e.payload === 'string') onPartial(e.payload);
  });
  listen('yap-amp', (e) => {
    const v = Math.min(1, Math.pow(Math.max(0, e.payload ?? 0) * AMP_GAIN, 0.7));
    const h = dictation.history;
    const next = h.length >= MAX_BARS ? h.slice(1) : h.slice();
    next.push(v);
    dictation.history = next;
  });
}
