// The dictation side of the Yap bar (the overlay as it always was): the
// pipeline's state, the scrolling amplitude waveform, the live partial text
// and the last error. Listened to for the page's whole life — not only while
// the capsule shows — because `yap-dictation-error` arrives just before
// the `yap-state` "error" that brings the capsule up.
import { listen } from '@tauri-apps/api/event';

// Scrolling amplitude waveform (Claude Code style): newest bar on the right.
// Each bar comes shaped from Rust (`yap-amp`, dictation/meter.rs: decibels
// over the mic's noise, gently compressed — a whisper about a third of the
// height, a shout 0.92, never the top).
const MAX_BARS = 80;
// "Hold on, still transcribing…" stays this long after the press.
const HOLD_ON_MS = 2_500;
// The length limit's warning shows as words this long, then as a hairline.
const CAP_TEXT_MS = 6_000;

export const dictation = $state({
  // idle | recording | processing | processing-slow | needs-model | error |
  // cancelled ("Dictation cancelled · Undo" while the audio is kept) |
  // cleaned ("Cleaned up · Use what I said" a few seconds after a dictation
  // AI cleanup changed: pasting/as_said.rs; or a transform's "Polish ✓ · Undo")
  state: 'idle',
  errorMsg: 'Transcription failed',
  history: [],
  // Revealed partial words (rendered as keyed spans).
  words: [],
  // Push-to-talk's double-tap locked this recording hands-free
  // (`yap-dictation-locked`, dictation/gesture.rs): the capsule shows a lock.
  locked: false,
  // The push-to-talk key started this recording (`yap-dictation-held`):
  // held until it locks hands-free, the capsule shows no ✕ / ✓ (as Wispr's
  // bar draws them only hands-free).
  held: false,
  // The dead-mic watchdog (dictation/mic_watch.rs) found nothing but digital silence:
  // "No sound from <device> — is it muted?", shown in the waveform's place
  // while the recording carries on (the whole message as its tooltip);
  // '' once sound arrives (or the recording ends).
  micSilent: '',
  micSilentFull: '',
  // The mic opens on demand (dictation/mic.rs): a recording that started while it was
  // still opening waits for its first real audio (`yap-mic-ready` false →
  // true; a Bluetooth headset takes a second or two). Meanwhile the capsule
  // says "Starting the mic…" instead of the waveform, so nobody talks early.
  micWaiting: false,
  // A dictation waiting for the speech model, still loading in the
  // background after Yap started (`yap-model-loading`).
  modelLoading: false,
  // How long a cancelled dictation can be undone (`yap-dictation-cancelled`).
  undoMs: 10_000,
  // How long "Use what I said" is offered (pasting/as_said.rs `OFFER_MS`).
  offerMs: 6_000,
  // A question to the Voice Assistant (chat/assistant.rs, `yap-assistant`):
  // 'idle' | 'listening' | 'thinking' | 'speaking'. Not idle, the capsule is
  // the assistant's ("Ask Yap…", "Thinking…", "Speaking…" with ■).
  assistant: 'idle',
  // What the question is about: 'screenshot' after a spoken "snip" (snip.rs:
  // "Ask about this screenshot…"); 'window' while the window in front is
  // taken and read (chat/assistant/screen.rs: "Looking at the window…");
  // else ''.
  askAbout: '',
  // A saved transform (dictation/transforms.rs, `yap-transform`): its name, and
  // 'running' ("Polish…" while the AI works), 'done' ("Polish ✓ · Undo" for
  // `offerMs`) or 'unchanged' ("No changes needed"); '' for a dictation.
  transform: '',
  transformPhase: '',
  // "Taking longer than usual…" (dictation/slow.rs, `yap-slow`): `{stage,
  // skip}` while the transcription ('transcribing') or the AI cleanup
  // ('cleanup', with Skip cleanup: `skip`) runs well past this PC's usual
  // time; null otherwise.
  slow: null,
  // The shortcut was pressed while the last dictation is still being
  // transcribed (`yap-dictation-busy`): "Hold on, still transcribing…".
  holdOn: false,
  // The length limit is near (dictation/cap.rs, `yap-dictation-cap`): its
  // words ("1 minute left · it'll stop at 15 minutes") for a few seconds
  // (`capShow`), then a hairline running down the time left (`capMs`;
  // `capKey` restarts it).
  capText: '',
  capShow: false,
  capMs: 0,
  capKey: 0,
});

let holdOnTimer = null;
let capTimer = null;

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
    if (state !== 'recording') {
      dictation.history = [];
      dictation.locked = false;
      dictation.held = false;
      dictation.micSilent = '';
      dictation.micSilentFull = '';
      dictation.micWaiting = false;
      clearTimeout(capTimer);
      dictation.capText = '';
      dictation.capShow = false;
      dictation.capMs = 0;
    }
    if (state !== 'processing' && state !== 'processing-slow') {
      dictation.modelLoading = false;
      clearTimeout(holdOnTimer);
      dictation.holdOn = false;
    }
    // "Taking longer than usual…" goes with its own state only.
    if (state !== 'processing-slow') dictation.slow = null;
    // A transform's name goes with its own states only (it says them just
    // before: "running" → processing, "done"/"unchanged" → cleaned).
    const transformState =
      (state === 'processing' && dictation.transformPhase === 'running') ||
      (state === 'cleaned' && (dictation.transformPhase === 'done' || dictation.transformPhase === 'unchanged'));
    if (!transformState) {
      dictation.transform = '';
      dictation.transformPhase = '';
    }
    // Keep the partial visible through the brief "processing" state, then
    // drop it once we're idle/needs-model/error so it never lingers.
    if (state === 'idle' || state === 'needs-model' || state === 'error' || state === 'recording' || state === 'cancelled' || state === 'cleaned') {
      clearPartial();
    }
  });
  // Said before the assistant's recording starts (dictation/pipeline.rs), so its
  // first frame is already the assistant's.
  listen('yap-assistant', (e) => {
    dictation.assistant = e.payload?.phase || 'idle';
    dictation.askAbout = e.payload?.about || '';
  });
  listen('yap-transform', (e) => {
    dictation.transform = e.payload?.name || '';
    dictation.transformPhase = e.payload?.phase || '';
    if (e.payload?.offerMs > 0) dictation.offerMs = e.payload.offerMs;
  });
  listen('yap-dictation-cancelled', (e) => {
    if (e.payload?.undoMs > 0) dictation.undoMs = e.payload.undoMs;
  });
  // A dictation's own errors come only here (`yap-dictation-error`: the
  // overlay is where the dictation is); `yap-error` too, as before.
  for (const name of ['yap-dictation-error', 'yap-error']) {
    listen(name, (e) => {
      if (e.payload) dictation.errorMsg = e.payload;
    });
  }
  listen('yap-dictation-locked', (e) => {
    dictation.locked = !!e.payload;
  });
  // Said just before the recording starts (dictation/pipeline.rs), so it's never
  // reset by that recording's own "recording" state.
  listen('yap-dictation-held', (e) => {
    dictation.held = !!e.payload;
  });
  listen('yap-mic-silent', (e) => {
    dictation.micSilent = e.payload?.short || '';
    dictation.micSilentFull = e.payload?.message || '';
    // A recording that ends on it ends in the error state, saying the same.
    if (dictation.micSilent) dictation.errorMsg = dictation.micSilent;
  });
  // Said just after "recording" when the mic was still opening, then true
  // at its first real audio (dictation/pipeline.rs `fire_pending_start`).
  listen('yap-mic-ready', (e) => {
    dictation.micWaiting = e.payload === false;
  });
  listen('yap-model-loading', (e) => {
    dictation.modelLoading = !!e.payload;
  });
  listen('yap-partial', (e) => {
    if (typeof e.payload === 'string') onPartial(e.payload);
  });
  // Said just before `yap-state` "processing-slow" (and null before
  // "processing" again).
  listen('yap-slow', (e) => {
    dictation.slow = e.payload?.stage ? { stage: e.payload.stage, skip: !!e.payload.skip } : null;
  });
  listen('yap-dictation-busy', () => {
    if (dictation.state !== 'processing' && dictation.state !== 'processing-slow') return;
    clearTimeout(holdOnTimer);
    dictation.holdOn = true;
    holdOnTimer = setTimeout(() => (dictation.holdOn = false), HOLD_ON_MS);
  });
  listen('yap-dictation-cap', (e) => {
    if (dictation.state !== 'recording') return;
    dictation.capText = e.payload?.text || '';
    dictation.capMs = Math.max(0, e.payload?.leftMs || 0);
    dictation.capKey += 1;
    dictation.capShow = !!dictation.capText;
    clearTimeout(capTimer);
    // At most half the time left (a test run's short limit), so the
    // waveform comes back before the end.
    capTimer = setTimeout(() => (dictation.capShow = false), Math.min(CAP_TEXT_MS, (dictation.capMs || CAP_TEXT_MS) / 2));
  });
  listen('yap-amp', (e) => {
    const v = Math.max(0, Math.min(1, Number(e.payload) || 0));
    const h = dictation.history;
    const next = h.length >= MAX_BARS ? h.slice(1) : h.slice();
    next.push(v);
    dictation.history = next;
  });
}
