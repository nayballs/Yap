// Yap Link, the phone's page: talk to Yap's AI on the PC from a phone on the
// home Wi-Fi, through a Bluetooth headset or the phone itself. Yap serves this
// page over HTTPS (a self-signed certificate, accepted once) and a WebSocket
// at /ws; the protocol is docs/architecture/link.md's (v1), and this page
// keeps to it exactly.
//
// It's Yap's Chat on a phone, in the Claude app's layout (2026-11-03): ☰
// opens the drawer of chats (drawer.js: Chat's conversations on the PC, a
// search, + New chat), the title in the middle with the connection's dot,
// ⋯ the settings sheet; the conversation fills the screen (the answer as
// plain text, your question in a bubble, Copy and ▶ under an answer); the
// composer card at the bottom carries the pictures (photos.js), the text
// box, +, Chat's model chip with its effort (Chat's own picker, picker.js),
// the mic (Talk: a spoken question, never dictation into the PC's apps) and
// Send (■ while Yap answers). The phone keeps its own conversation
// (`yap.link.chat`).
//
// Talk puts its words in the text box to check first (the Claude and
// ChatGPT apps' way: talk, see what you said, fix it, send it yourself):
// `start {mode: "words"}`, then the PC's `words` go in at the caret, after
// whatever was typed; Send asks them as said (`ask {speak: true}`: answered
// out loud, as a spoken question is) when any of the box's words came from
// Talk since it was last empty or sent. Hands-free (or a PC from before,
// without "words" in `ready.can`) sends the question at once and Yap
// answers out loud.
//
// What it does: pairing (the QR's #pair=<token>, the 4-digit code, the device
// kept in localStorage; a hello carries the device and any code this tab
// has, the device counted first), typed questions (`ask`: answered in the
// same conversation, not spoken; ▶ under an answer has it read, `read_aloud`),
// Talk (tap or hold; hands-free ends by itself after the wait chosen — 1 s,
// 1.5 s or 2.5 s — of quiet), the mic resampled to 16 kHz Int16 in an
// AudioWorklet (mic-worklet.js, dsp.js), the spoken answer as one stream
// (resampled here frame after frame, played from a jitter buffer in
// player-worklet.js), "Clearer sound in a headset" (on by default: the mic
// closed while Yap talks), opening the mic a timed step at a time (never a
// silent "Wait…"), Stop and barge-in, the conversation, the step Yap is on
// and its time, the model chip and Chat's picker, Sound details, the
// on-screen keyboard (the composer rides above it), reconnects with
// backoff, keep-alive pings and a screen wake lock.
//
// No service worker: on an origin whose certificate Chrome only accepted
// after a warning, registration fails ("An SSL certificate error occurred
// when fetching the script"). The page always comes from the PC, which is
// fine: without the PC there's nothing to talk to.
//
// The headset's button: a page can't own it while the mic is open. With the
// mic open, a Bluetooth headset is in call mode (HFP), where its button
// belongs to the phone's call handling (a press may mean "hang up"), and
// Media Session handlers only fire for media that plays (phone-remote design,
// §4.5). So it isn't faked here: it's the native app's job (§6.2).

import {
  FRAME_SAMPLES,
  LINK_RATE,
  DOWN_RATE,
  WAITS,
  EnergyVad,
  Resampler,
  fromInt16,
  elapsed,
  clock,
  toDb,
} from './dsp.js';
import { renderAnswer } from './markdown.js';
import { createPicker, markEl, modelMark, chipWords, chipEffort } from './picker.js';
import { createDrawer } from './drawer.js';
import { labelFor } from './core/modelCatalog.js';
import { PROVIDER_LABELS } from './core/providerIds.js';
import { MAX_PICTURES, prepare as preparePicture, sendPictures } from './photos.js';

const KIND_UP = 0x01;
const KIND_DOWN = 0x02;
const MAX_FRAMES = (120 * LINK_RATE) / FRAME_SAMPLES; // a question: 2 minutes at most
const HOLD_MS = 350; // a press this long is a hold: release sends
const NO_SPEECH_MS = 10000; // hands-free: nothing said this long → cancelled
const MIC_WARM_MS = 15000; // the mic kept open after a turn, for a follow-up
const MIC_LIVE_MS = 3000; // the longest a new mic's wait for sound (a headset switching to its call link)
// Opening the mic never waits silently: each step has a time limit (doOpenMic).
const QUERY_MS = 1000; // the permission's state, where the browser says
const MODULE_MS = 5000; // an AudioWorklet module loading
const PERMISSION_MS = 6000; // getUserMedia: then how to allow the mic is said (and it waits on)
const RESUME_MS = 3000; // the phone's sound starting, once Talk is let go
const PING_MS = 15000;
const PONG_WAIT_MS = 8000;
const KEEP_TURNS = 40;
const MAX_TYPED = 4000; // the PC's limit for a typed question (link/session.rs)

const KEY = {
  device: 'yap.link.device',
  pc: 'yap.link.pc',
  mic: 'yap.link.mic',
  handsFree: 'yap.link.handsFree',
  wait: 'yap.link.wait',
  clearer: 'yap.link.clearer',
  // The conversation, for this tab only (sessionStorage): a reload keeps it.
  thread: 'yap.link.thread',
  // The conversation this phone is in on the PC: its id, or "new".
  chat: 'yap.link.chat',
};

/** A PC from before typed questions: said plainly, before anything's sent. */
const OLD_PC_TYPING = 'Yap on your PC needs an update to answer typed questions. Update Yap there; talking works as before.';

/** Talk's words for the box, when none came back. */
const NO_WORDS = "Yap didn't hear anything. Try again, or type it.";
/** The mic gone mid-talk, its words on their way to the box. */
const MIC_GONE_WORDS = 'The microphone went away (was the headset switched off?). What it heard is in the box.';
/** A note that the words are in the box goes when none came. */
const forgetBoxNote = () => {
  if (/is in the box\.$/.test(S.note)) S.note = '';
};

/** Hands-free's hint: what it changes, where the PC can put Talk's words in the box (else it always sends). */
const HANDS_FREE_HINT = {
  words: 'On: sends when you stop talking, and Yap answers out loud. Off: your words go in the box to check first.',
  sends: 'Sends when you stop talking',
};

// ------------------------------------------------------------------ storage

function load(key) {
  try {
    const v = localStorage.getItem(key);
    return v == null ? null : JSON.parse(v);
  } catch {
    return null;
  }
}
function save(key, value) {
  try {
    if (value == null) localStorage.removeItem(key);
    else localStorage.setItem(key, JSON.stringify(value));
  } catch {
    /* private mode: the page still works, it just won't remember */
  }
}

/** The conversation kept in this tab (a reload keeps it; closing the tab doesn't). */
function loadThread() {
  try {
    const v = JSON.parse(sessionStorage.getItem(KEY.thread) || 'null');
    if (!v || !Array.isArray(v.turns)) return { turns: [], id: 0 };
    const turns = v.turns
      .filter((t) => t && typeof t.id === 'number')
      .map((t) => ({
        id: t.id,
        q: String(t.q || ''),
        a: String(t.a || ''),
        final: true,
        cancelled: !!t.cancelled,
        typed: !!t.typed,
        answerId: String(t.answerId || ''),
        pictures: Math.max(0, Number(t.pictures) || 0),
        pics: [],
      }))
      .filter((t) => t.q || t.a || t.pictures);
    return { turns: turns.slice(-KEEP_TURNS), id: Math.max(0, ...turns.map((t) => t.id)), more: !!v.more };
  } catch {
    return { turns: [], id: 0, more: false };
  }
}
function saveThread() {
  try {
    const turns = S.turns
      .filter((t) => t.final && (t.q || t.a || t.pictures))
      .map(({ pics, ...t }) => t); // (the pictures' thumbnails live as long as the page)
    sessionStorage.setItem(KEY.thread, JSON.stringify({ turns, more: S.more }));
  } catch {
    /* a convenience only */
  }
}

/** The conversation this phone is in on the PC: an id, "new", or none (never said). */
function loadChat() {
  const v = load(KEY.chat);
  return (typeof v === 'number' && v > 0) || v === 'new' ? v : null;
}

/**
 * The paired device this phone keeps (shared by every tab of the page), or
 * null. Read afresh for every hello: another tab may have paired since.
 */
function loadDevice() {
  const d = load(KEY.device);
  return d && typeof d.id === 'string' && d.id && typeof d.secret === 'string' && d.secret ? d : null;
}

/** The QR's token in this address (`#pair=<token>`), or ''. */
const pairInAddress = () => new URLSearchParams(location.hash.slice(1)).get('pair') || '';

// ------------------------------------------------------------------- state

/** One of WAITS' names: what's stored, or Normal. */
function waitChoice(w) {
  return typeof w === 'string' && Object.prototype.hasOwnProperty.call(WAITS, w) ? w : 'normal';
}

const keptThread = loadThread();

const S = {
  ws: null,
  ready: false,
  denied: false,
  deniedWhy: '',
  pc: load(KEY.pc) || '',
  // The QR's token, while this tab has one: it goes with the device (a tab
  // restored with an old `#pair=` keeps its address), and the PC counts the
  // device first.
  pairToken: pairInAddress(),
  pairCode: '',
  device: loadDevice(),
  // Refused, then another tab paired the phone: this one tries again with
  // that device once it's in front.
  comeBack: false,
  retries: 0,
  retryTimer: 0,
  pingTimer: 0,
  pongTimer: 0,
  // The PC's phase, and this page's mic: idle | opening | listening | stopping.
  phase: 'idle',
  local: 'idle',
  pcListening: false,
  // After a cancel or an interrupt, the old turn's late messages are dropped
  // until the PC says where it is (idle, or listening to the new question),
  // or 1.5 s on (on the home Wi-Fi they'd have come by then).
  ignoreOldAt: 0,
  holdRelease: false,
  // What the question being asked is for: 'ask' (sent at Done, answered out
  // loud: hands-free, or a PC from before) or 'words' (its words back for
  // the box, to check and send); and waiting for those words: false, 'sent'
  // (Done said), 'pc' (the PC is transcribing them).
  talk: 'ask',
  transcribing: false,
  seq: 0,
  frames: 0,
  qStarted: 0,
  workSince: 0,
  step: '',
  turns: keptThread.turns,
  turnId: keptThread.id,
  // Older messages of the conversation left on the PC (`thread.more`).
  more: keptThread.more,
  // The conversation on the PC this phone is in (an id, "new", or none: the
  // Voice Assistant chooses), and its title.
  chat: loadChat(),
  title: '',
  // Asked the PC for the conversation's messages once this page is up.
  synced: false,
  // The pictures in the composer: {buf, url, w, h, bytes} (photos.js).
  attached: [],
  // Pictures still being made ready (a moment each).
  preparing: 0,
  // The question last sent, until its answer comes: back in the composer
  // when Chat's model can't read its pictures.
  pending: null,
  // Chat's line for pictures its model can't read (`cant_read`, or known
  // from the picker's status).
  cantRead: '',
  model: null,
  models: [],
  // What the PC answers beyond v1 (`ready.can`): "ask", "read_aloud", "picker", "effort".
  can: [],
  // What Chat's picker shows (`ready.picker`), when the PC sends it.
  picker: null,
  // ▶ asked for this answer (its id), and the turn whose answer is being said.
  reading: '',
  speakingTurn: 0,
  // Words in the composer.
  typed: false,
  // Some of them came from Talk (since the box was last empty or sent; an
  // edit doesn't change it): Send asks them as said, answered out loud.
  said: false,
  error: '',
  note: '',
  handsFree: load(KEY.handsFree) === true,
  // Hands-free's wait before sending: short | normal | long (WAITS).
  wait: waitChoice(load(KEY.wait)),
  // "Clearer sound in a headset": the mic closed while Yap talks. On unless
  // the person turned it off: with the mic open a headset stays in its call
  // mode, which crackles.
  clearer: load(KEY.clearer) !== false,
  tick: 0,
  stopTimer: 0,
  gen: 0,
};

const FRAME_MS = (FRAME_SAMPLES / LINK_RATE) * 1000;
// Made afresh for each question, with the wait chosen then.
let vad = new EnergyVad({ frameMs: FRAME_MS, endMs: WAITS[S.wait] });

const ignoringOld = () => S.ignoreOldAt > 0 && performance.now() - S.ignoreOldAt < 1500;

// ---------------------------------------------------------------------- DOM

const $ = (id) => document.getElementById(id);
const el = {
  conn: $('conn'),
  dot: $('dot'),
  titleName: $('title-name'),
  titleSub: $('title-sub'),
  busy: $('busy'),
  menuBtn: $('menu-btn'),
  older: $('older'),
  vision: $('vision'),
  visionText: $('vision-text'),
  visionSwitch: $('vision-switch'),
  visionAsText: $('vision-text-btn'),
  visionPick: $('vision-pick'),
  visionWithout: $('vision-without'),
  shots: $('shots'),
  plusWrap: $('plus-wrap'),
  plusBtn: $('plus-btn'),
  plusMenu: $('plus-menu'),
  takePhoto: $('take-photo'),
  choosePhotos: $('choose-photos'),
  camera: $('camera'),
  photos: $('photos'),
  pair: $('pair'),
  pairCodeBox: $('pair-code-box'),
  pairCode: $('pair-code'),
  pairHow: $('pair-how'),
  pairWhy: $('pair-why'),
  pairWait: $('pair-wait'),
  retry: $('retry'),
  chat: $('chat'),
  thread: $('thread'),
  convo: $('convo'),
  empty: $('empty'),
  phase: $('phase'),
  step: $('step'),
  stepLine: $('step-line'),
  stepTime: $('step-time'),
  note: $('note'),
  error: $('error'),
  composer: $('composer'),
  input: $('input'),
  sendBtn: $('send-btn'),
  liveLabel: $('live-label'),
  meter: $('meter'),
  talkBtn: $('talk-btn'),
  talkLabel: $('talk-label'),
  talkTime: $('talk-time'),
  stopBtn: $('stop-btn'),
  cancelBtn: $('cancel-btn'),
  settingsBtn: $('settings-btn'),
  settings: $('settings'),
  settingsClose: $('settings-close'),
  handsFree: $('handsfree'),
  handsFreeHint: $('handsfree-hint'),
  wait: $('wait'),
  clearer: $('clearer'),
  sound: $('sound'),
  soundRate: $('sound-rate'),
  soundLatency: $('sound-latency'),
  soundStart: $('sound-start'),
  soundGaps: $('sound-gaps'),
  soundMic: $('sound-mic'),
  soundMicNow: $('sound-mic-now'),
  soundMicStep: $('sound-mic-step'),
  soundBrowser: $('sound-browser'),
  mic: $('mic'),
  micHint: $('mic-hint'),
  modelBtn: $('model'),
  modelMark: $('model-mark'),
  modelLabel: $('model-label'),
  modelWhere: $('model-where'),
  models: $('models'),
};

// Chat's model picker (picker.js); the chip shows a pick at once.
const picker = createPicker({
  dialog: el.models,
  send,
  onPicked: (m) => {
    S.model = { ...(S.model || {}), id: '', label: m.label, onThisPc: m.onThisPc, picked: m };
    // Another model: whether it reads the pictures is the PC's to say next.
    S.cantRead = '';
    render();
  },
  // The effort changed: the chip says it at once.
  onChanged: () => render(),
});

// The drawer of chats (drawer.js).
const drawer = createDrawer({
  dialog: $('drawer'),
  list: $('recents'),
  empty: $('recents-empty'),
  search: $('drawer-search'),
  pc: $('drawer-pc'),
  newChat: $('new-chat'),
  closeBtn: $('drawer-close'),
  send,
  onOpen: (item) => openChat(item.id, item.title),
  onNew: () => newChat(),
  current: () => (typeof S.chat === 'number' ? S.chat : null),
});

// The Yap bar's waveform: thin bars, the newest on the right (link.css .meter).
const BARS = 60;
const levels = new Array(BARS).fill(0);
for (let i = 0; i < BARS; i++) el.meter.appendChild(document.createElement('i'));
const bars = [...el.meter.children];

const pcName = () => S.pc || 'your PC';

// ------------------------------------------------------------------ the wire

const LITTLE = new Uint8Array(new Uint16Array([1]).buffer)[0] === 1;

/** An up frame: kind 0x01, seq (u32 LE), the samples (Int16 LE). */
function upFrame(seq, samples) {
  const out = new Uint8Array(5 + samples.byteLength);
  const dv = new DataView(out.buffer);
  out[0] = KIND_UP;
  dv.setUint32(1, seq >>> 0, true);
  if (LITTLE) out.set(new Uint8Array(samples.buffer, samples.byteOffset, samples.byteLength), 5);
  else for (let i = 0; i < samples.length; i++) dv.setInt16(5 + i * 2, samples[i], true);
  return out.buffer;
}

/** A binary frame from the PC: {kind, seq, samples: Int16Array}. */
function readFrame(buf) {
  const dv = new DataView(buf);
  if (buf.byteLength < 5) return null;
  const kind = dv.getUint8(0);
  const seq = dv.getUint32(1, true);
  const n = (buf.byteLength - 5) >> 1;
  const samples = new Int16Array(n);
  for (let i = 0; i < n; i++) samples[i] = dv.getInt16(5 + i * 2, true);
  return { kind, seq, samples };
}

function nonce() {
  const b = crypto.getRandomValues(new Uint8Array(16));
  return btoa(String.fromCharCode(...b)).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

/** "Android · Chrome", for the PC's list of paired phones. */
function deviceName() {
  const ua = navigator.userAgent;
  const os = /Android/.test(ua)
    ? 'Android'
    : /iPhone/.test(ua)
      ? 'iPhone'
      : /iPad/.test(ua)
        ? 'iPad'
        : /CrOS/.test(ua)
          ? 'ChromeOS'
          : /Windows/.test(ua)
            ? 'Windows'
            : /Macintosh|Mac OS X/.test(ua)
              ? 'Mac'
              : /Linux/.test(ua)
                ? 'Linux'
                : 'Phone';
  const browser = /SamsungBrowser/.test(ua)
    ? 'Samsung Internet'
    : /EdgA?\/|EdgiOS/.test(ua)
      ? 'Edge'
      : /Firefox|FxiOS/.test(ua)
        ? 'Firefox'
        : /CriOS|Chrome\//.test(ua)
          ? 'Chrome'
          : /Safari/.test(ua)
            ? 'Safari'
            : 'Browser';
  return `${os} · ${browser}`;
}

/** "Firefox 131 on Android", for Sound details. */
function browserLine() {
  const ua = navigator.userAgent;
  const [os, browser] = deviceName().split(' · ');
  const version = {
    Firefox: /(?:Firefox|FxiOS)\/(\d+)/,
    Edge: /Edg(?:A|iOS)?\/(\d+)/,
    'Samsung Internet': /SamsungBrowser\/(\d+)/,
    Chrome: /(?:Chrome|CriOS)\/(\d+)/,
    Safari: /Version\/(\d+)/,
  }[browser];
  const v = version && ua.match(version);
  return `${browser}${v ? ` ${v[1]}` : ''} on ${os === 'Phone' ? 'a phone' : os}`;
}

function send(msg) {
  const ws = S.ws;
  if (!ws || ws.readyState !== WebSocket.OPEN) return false;
  ws.send(JSON.stringify(msg));
  return true;
}

function sendBinary(buf) {
  const ws = S.ws;
  if (!ws || ws.readyState !== WebSocket.OPEN) return false;
  ws.send(buf);
  return true;
}

/** A question's `chat`: the conversation it goes to, when this phone has said (a PC that keeps one). */
function chatField() {
  return can('chats') && S.chat != null ? { chat: S.chat } : {};
}

// -------------------------------------------------------------- connection

function connect() {
  clearTimeout(S.retryTimer);
  S.retryTimer = 0;
  if (S.ws) {
    // One connection at a time.
    const old = S.ws;
    S.ws = null;
    old.onopen = old.onmessage = old.onclose = old.onerror = null;
    try {
      old.close();
    } catch {
      /* gone */
    }
  }
  // The device as every tab keeps it now: another tab may have paired this
  // phone since this one loaded (its old device would be refused).
  S.device = loadDevice() || S.device;
  if (!S.device && !S.pairToken) {
    render();
    return;
  }
  const ws = new WebSocket(`${location.protocol === 'http:' ? 'ws' : 'wss'}://${location.host}/ws`);
  ws.binaryType = 'arraybuffer';
  S.ws = ws;
  S.denied = false;
  ws.onopen = () => {
    if (S.ws !== ws) return;
    const hello = { type: 'hello', v: 1, name: deviceName(), nonce: nonce() };
    // Both when this tab has both: the PC counts the device first, so a tab
    // restored with an old code in its address still connects; the code
    // pairs only a phone the PC doesn't know (a new one, or one removed).
    // (Until 3 Nov the code went instead of the device: restored tabs were
    // refused, and their refusals locked the phone out.)
    if (S.device) hello.device = { id: S.device.id, secret: S.device.secret };
    if (S.pairToken) hello.pair = { token: S.pairToken };
    ws.send(JSON.stringify(hello));
    startPings(ws);
  };
  ws.onmessage = (e) => {
    if (S.ws !== ws) return;
    clearTimeout(S.pongTimer);
    if (typeof e.data === 'string') onText(e.data);
    else onBinary(e.data);
  };
  ws.onclose = () => dropped(ws);
  ws.onerror = () => {}; // a close follows
  render();
}

/** The socket's gone (or looks dead): tidy up, then try again unless denied. */
function dropped(ws) {
  if (S.ws !== ws) return;
  ws.onopen = ws.onmessage = ws.onclose = ws.onerror = null;
  try {
    ws.close();
  } catch {
    /* already closed */
  }
  S.ws = null;
  clearInterval(S.pingTimer);
  clearTimeout(S.pongTimer);
  const wasReady = S.ready;
  S.ready = false;
  // A question can't carry on across a reconnect (nor words on their way
  // to the box: what's typed there stays).
  if (S.local !== 'idle' || S.phase !== 'idle' || player.active || S.transcribing) {
    const listening = S.local !== 'idle';
    const words = S.transcribing || (listening && S.talk === 'words');
    endLocalQuestion();
    player.stopAll();
    S.transcribing = false;
    S.phase = 'idle';
    S.step = '';
    S.reading = '';
    S.speakingTurn = 0;
    // A question that never got heard goes; one with words stays, ended.
    const t = currentTurn(false);
    if (t && !t.final && !t.q && !t.a) S.turns.pop();
    else if (t) t.final = true;
    saveThread();
    renderConvo();
    if (wasReady) {
      S.error = words
        ? `Lost the connection to ${pcName()} before your words came back. Try again once it's back.`
        : listening
          ? `Lost the connection to ${pcName()} while listening. Ask again once it's back.`
          : `Lost the connection to ${pcName()}.`;
    }
  }
  if (!S.denied) {
    const delay = Math.min(15000, 500 * 2 ** S.retries) * (0.8 + Math.random() * 0.4);
    S.retries = Math.min(S.retries + 1, 8);
    S.retryTimer = setTimeout(connect, delay);
  }
  render();
}

function reconnectNow() {
  if (S.ws || S.denied) return;
  S.retries = 0;
  connect();
}

function startPings(ws) {
  clearInterval(S.pingTimer);
  S.pingTimer = setInterval(() => ping(ws), PING_MS);
}

function ping(ws = S.ws) {
  if (!ws || S.ws !== ws || ws.readyState !== WebSocket.OPEN) return;
  ws.send(JSON.stringify({ type: 'ping' }));
  clearTimeout(S.pongTimer);
  // Anything from the PC counts as an answer; nothing at all, and it's dead.
  S.pongTimer = setTimeout(() => dropped(ws), PONG_WAIT_MS);
}

function onText(text) {
  let m;
  try {
    m = JSON.parse(text);
  } catch {
    return;
  }
  if (!m || typeof m.type !== 'string') return;
  // (A cancelled question's late news, where it went and its pictures' fate
  // included; and words cancelled on their way to the box.)
  if (ignoringOld() && ['heard', 'step', 'answer', 'speak_start', 'speak_end', 'chat', 'cant_read', 'words'].includes(m.type)) return;
  switch (m.type) {
    case 'pair_wait':
      S.pairCode = String(m.code ?? '');
      S.retries = 0;
      break;
    case 'paired':
      if (m.device && m.device.id && m.device.secret) {
        S.device = { id: String(m.device.id), secret: String(m.device.secret) };
        save(KEY.device, S.device);
      }
      S.pairToken = '';
      S.pairCode = '';
      clearFragment();
      break;
    case 'denied': {
      const pairing = !!S.pairToken;
      S.denied = true;
      S.ready = false;
      const why = typeof m.reason === 'string' && /\s/.test(m.reason.trim()) ? m.reason.trim() : '';
      S.deniedWhy =
        why ||
        (pairing ? "Yap on your PC didn't allow this phone." : "This phone isn't paired with Yap on your PC any more.");
      // A code that was refused (or ran out) is no use again.
      S.pairToken = '';
      S.pairCode = '';
      clearFragment();
      break;
    }
    case 'ready':
      S.ready = true;
      S.retries = 0;
      // Connected: a code this tab had is spent (its device counted, or it
      // just paired). Out of the address, so a reload or a restored tab
      // never brings it again.
      S.pairToken = '';
      S.pairCode = '';
      clearFragment();
      if (typeof m.pc === 'string' && m.pc) {
        S.pc = m.pc;
        save(KEY.pc, S.pc);
      }
      S.model = m.model || null;
      if (Array.isArray(m.models)) S.models = m.models;
      S.can = Array.isArray(m.can) ? m.can.map(String) : [];
      S.picker = m.picker && typeof m.picker === 'object' ? m.picker : null;
      picker.update(m);
      drawer.setPc(S.pc);
      if (S.error.startsWith('Lost the connection')) S.error = '';
      if (S.error === OLD_PC_TYPING && can('ask')) S.error = '';
      // Once a page is up: the conversation it was in, as the PC has it now
      // (answers written while it was away, a chat deleted meanwhile).
      if (!S.synced && can('chats')) {
        S.synced = true;
        if (typeof S.chat === 'number') send({ type: 'open_chat', id: S.chat });
      }
      renderConvo();
      break;
    case 'chats':
      drawer.update(m.items);
      return;
    case 'thread':
      onThread(m);
      break;
    case 'chat':
      // This phone's question went to conversation `id`: its own from now on.
      if (typeof m.id === 'number' && m.id > 0) {
        S.chat = m.id;
        save(KEY.chat, S.chat);
        S.title = String(m.title || '');
        drawer.titled(m.id, S.title || 'New chat');
      }
      break;
    case 'title':
      if (typeof m.id === 'number') {
        if (m.id === S.chat) S.title = String(m.title || '');
        drawer.titled(m.id, String(m.title || 'New chat'));
      }
      break;
    case 'cant_read':
      onCantRead(String(m.message || ''));
      break;
    case 'state':
      onState(String(m.phase || 'idle'));
      break;
    case 'heard':
      currentTurn(true).q = String(m.text ?? '');
      renderConvo();
      break;
    case 'words':
      onWords(String(m.text ?? ''));
      break;
    case 'step':
      S.step = String(m.label ?? '');
      if (!S.workSince) S.workSince = performance.now();
      break;
    case 'answer': {
      const t = currentTurn(true);
      t.a = String(m.text ?? '');
      if (m.final) {
        t.final = true;
        // Its id in Chat: ▶ under it asks the PC to read it.
        if (typeof m.id === 'string') t.answerId = m.id;
        S.step = '';
        S.pending = null;
        saveThread();
      }
      renderConvo();
      break;
    }
    case 'speak_start': {
      player.begin(Number(m.rate) || DOWN_RATE);
      // Clearer sound: the headset's call link let go, for its music one.
      if (S.clearer && S.local === 'idle') closeMic();
      // Whose answer it is: the one ▶ asked for, else the latest question's.
      const read = S.reading && S.turns.find((t) => t.answerId === S.reading);
      const latest = currentTurn(false);
      S.speakingTurn = read ? read.id : latest ? latest.id : 0;
      renderConvo();
      break;
    }
    case 'speak_end':
      player.end();
      break;
    case 'error':
      S.error = String(m.message || 'Something went wrong on the PC.');
      S.step = '';
      if (S.local !== 'idle') endLocalQuestion();
      // Words that didn't come (nothing heard, or the PC couldn't make them
      // out): the box keeps what it has.
      if (S.transcribing) {
        S.transcribing = false;
        forgetBoxNote();
      }
      // ▶ that couldn't be read.
      if (S.reading && !player.active) doneSpeaking();
      else {
        // The question went no further: it stays in the thread, ended, so
        // the next one has its own bubble.
        const t = currentTurn(false);
        if (t && !t.final && t.q && S.local === 'idle') {
          t.final = true;
          saveThread();
          renderConvo();
        }
      }
      break;
    case 'pong':
      break;
    default:
      return;
  }
  render();
}

function onState(phase) {
  if (ignoringOld()) {
    // Still the old turn's (it was speaking or thinking when it was stopped).
    if (phase !== 'idle' && phase !== 'listening') return;
    S.ignoreOldAt = 0;
  }
  const was = S.phase;
  S.phase = phase;
  if (phase === 'listening') {
    if (S.local === 'listening') S.pcListening = true;
  } else if (phase === 'transcribing' || phase === 'thinking') {
    if (!S.workSince) S.workSince = performance.now();
    // Talk's words: the PC has them.
    if (phase === 'transcribing' && S.transcribing) S.transcribing = 'pc';
  } else if (phase === 'speaking') {
    S.step = '';
  } else if (phase === 'idle') {
    S.step = '';
    S.workSince = 0;
    // The PC done with Talk's words without sending any (nor saying why).
    // (An idle from before it took them — a cancel's — isn't that.)
    if (S.transcribing === 'pc') {
      S.transcribing = false;
      if (!S.error) S.error = NO_WORDS;
      forgetBoxNote();
    }
    if (player.accepting) player.end(); // no speak_end: what's queued still plays
    else if (!S.reading && S.speakingTurn) doneSpeaking();
    // The PC ended the question itself (its own limit, say).
    if (S.local === 'listening' && S.pcListening) endLocalQuestion();
    if (was !== 'idle') armMicClose();
  }
  syncTicker();
}

function onBinary(buf) {
  const f = readFrame(buf);
  if (!f || f.kind !== KIND_DOWN || ignoringOld()) return;
  player.push(f.samples);
}

function clearFragment() {
  if (!location.hash) return;
  try {
    history.replaceState(null, '', location.pathname + location.search);
  } catch {
    location.hash = '';
  }
}

// ---------------------------------------------------------------- the turns

function currentTurn(create, typed = false) {
  let t = S.turns[S.turns.length - 1];
  if ((!t || t.final) && create) {
    t = { id: ++S.turnId, q: '', a: '', final: false, cancelled: false, typed, answerId: '', pictures: 0, pics: [] };
    S.turns.push(t);
    if (S.turns.length > KEEP_TURNS) S.turns.shift();
  }
  return t;
}

const can = (what) => S.can.includes(what);

/** The PC's messages of a conversation (`thread`) as the page's turns. Pure. */
function turnsOf(messages) {
  const turns = [];
  for (const m of Array.isArray(messages) ? messages : []) {
    if (!m || typeof m.text !== 'string') continue;
    const blank = { id: ++S.turnId, q: '', a: '', final: true, cancelled: false, typed: true, answerId: '', pictures: 0, pics: [] };
    if (m.role === 'user') {
      turns.push({ ...blank, q: m.text, typed: !m.voice, pictures: Math.max(0, Number(m.pictures) || 0) });
    } else if (m.role === 'assistant') {
      let t = turns[turns.length - 1];
      if (!t || t.a) turns.push((t = blank));
      t.a = m.text;
      t.answerId = typeof m.id === 'string' ? m.id : '';
    }
  }
  return turns.slice(-KEEP_TURNS);
}

/**
 * **A conversation from the PC** (`thread`: one opened in the drawer, a new
 * chat, or the one this page was in, as the PC has it now): the thread
 * shows its messages, and this phone's questions go there.
 */
function onThread(m) {
  const id = typeof m.id === 'number' && m.id > 0 ? m.id : null;
  S.chat = id ?? 'new';
  save(KEY.chat, S.chat);
  S.title = id ? String(m.title || '') : '';
  // Its pictures' thumbnails, while this page lives, where the same question's still there.
  const kept = new Map(S.turns.filter((t) => t.pics.length).map((t) => [`${t.q}|${t.answerId}`, t.pics]));
  S.turns = turnsOf(m.messages);
  for (const t of S.turns) t.pics = kept.get(`${t.q}|${t.answerId}`) || [];
  S.more = !!m.more;
  S.pending = null;
  threadPinned = true;
  saveThread();
  renderConvo();
}

/** Chat's model can't read the question's pictures: the question back in the composer with them, and Chat's line. */
function onCantRead(message) {
  S.cantRead = message || "This model can't read images";
  const p = S.pending;
  S.pending = null;
  // The question goes back where it was asked, its bubble from the thread.
  const t = currentTurn(false);
  const text = p ? p.text || (t && t.q) || '' : (t && t.q) || '';
  if (t && !t.a) {
    S.turns.pop();
    saveThread();
  }
  if (p && p.pics.length) S.attached = p.pics;
  if (text && !el.input.value.trim()) {
    el.input.value = text;
    S.typed = true;
    // Talk's words back where they were: still said, when they go again.
    S.said = !!(p && p.said);
    fitInput();
  }
  S.step = '';
  renderShots();
  renderConvo();
}

/**
 * **Talk's words, back from the PC** (`words`): into the text box, to check
 * and send. Nothing was asked: Send asks them as the spoken question they
 * were (`speak`), answered out loud.
 */
function onWords(text) {
  S.transcribing = false;
  // The PC ended it itself (its 2 minutes): this page stops listening too,
  // and the mic goes as after a Done.
  if (S.local !== 'idle') {
    endLocalQuestion();
    afterQuestionSent();
  }
  // Its `state idle` follows; not "working" meanwhile.
  if (S.phase === 'transcribing') S.phase = 'idle';
  S.workSince = 0;
  const words = text.trim();
  if (!words) {
    if (!S.error) S.error = NO_WORDS;
    forgetBoxNote();
    return;
  }
  putWords(words);
}

/**
 * The words in the box **at the caret — after what's typed, one space
 * between** — never over it: talking never wipes the box (a selection
 * stays, the words after it). The caret after them, Send lit. The caret is
 * the box's own, kept while it's out of focus (a box Yap filled has it at
 * its end).
 */
function putWords(words) {
  const box = el.input;
  const v = box.value;
  let at = typeof box.selectionEnd === 'number' ? box.selectionEnd : v.length;
  if (!(at >= 0 && at <= v.length)) at = v.length;
  const before = v.slice(0, at);
  const after = v.slice(at);
  const lead = before && !/\s$/.test(before) ? ' ' : '';
  // No space before what closes a sentence or a bracket.
  const trail = after && !/^[\s.,;:!?)\]}]/.test(after) ? ' ' : '';
  box.value = before + lead + words + trail + after;
  // Said: Send asks the box's words as a spoken question, answered out loud.
  S.said = true;
  const caret = (before + lead + words).length;
  try {
    box.setSelectionRange(caret, caret);
  } catch {
    /* a box that can't say: the words are in all the same */
  }
  S.typed = box.value.trim() !== '';
  // The box back (it was the capsule till now), then grown to fit them.
  render();
  fitInput();
  if (caret === box.value.length) box.scrollTop = box.scrollHeight;
}

/** Talk's words on their way to the box, dropped (✕, Talk again, a typed question, another chat). */
function dropWords() {
  if (!S.transcribing) return false;
  send({ type: 'cancel' });
  S.transcribing = false;
  S.ignoreOldAt = performance.now();
  S.phase = 'idle';
  S.workSince = 0;
  return true;
}

/** Whatever Yap is doing on this phone stops (the drawer's another chat, a new chat). */
function letGo() {
  if (S.local !== 'idle') cancelTalk();
  const now = phaseNow();
  if (now === 'speaking') interrupt();
  else if (now === 'transcribing') dropWords();
  else if (now === 'working') {
    send({ type: 'cancel' });
    markCancelled();
    S.ignoreOldAt = performance.now();
    S.phase = 'idle';
    S.step = '';
    S.workSince = 0;
  }
}

/** The drawer's chat: this phone's questions go there, its messages shown. */
function openChat(id, title) {
  letGo();
  S.chat = id;
  save(KEY.chat, id);
  S.title = title || '';
  S.turns = [];
  S.more = false;
  S.error = '';
  S.cantRead = '';
  saveThread();
  send({ type: 'open_chat', id });
  renderConvo();
  render();
}

/** + New chat: the next question starts a conversation on the PC. */
function newChat() {
  letGo();
  S.chat = 'new';
  save(KEY.chat, 'new');
  S.title = '';
  S.turns = [];
  S.more = false;
  S.error = '';
  S.note = '';
  S.cantRead = '';
  saveThread();
  if (can('chats')) send({ type: 'new_chat' });
  renderConvo();
  render();
}

const NS = 'http://www.w3.org/2000/svg';
/** A small inline icon (never HTML). */
function icon(cls, paths, filled = false) {
  const s = document.createElementNS(NS, 'svg');
  s.setAttribute('viewBox', '0 0 24 24');
  s.setAttribute('aria-hidden', 'true');
  s.setAttribute('focusable', 'false');
  if (cls) s.setAttribute('class', cls);
  for (const d of paths) {
    const p = document.createElementNS(NS, 'path');
    p.setAttribute('d', d);
    if (filled) p.setAttribute('fill', 'currentColor');
    s.appendChild(p);
  }
  return s;
}
const MIC = ['M12 3a3 3 0 0 0-3 3v5a3 3 0 0 0 6 0V6a3 3 0 0 0-3-3z', 'M5.5 11a6.5 6.5 0 0 0 13 0', 'M12 17.5V21'];
const SQUARE = ['M7.5 5h9A2.5 2.5 0 0 1 19 7.5v9a2.5 2.5 0 0 1-2.5 2.5h-9A2.5 2.5 0 0 1 5 16.5v-9A2.5 2.5 0 0 1 7.5 5z'];
// The Claude app's two under an answer: copy, and the speaker (▶ Read aloud).
const COPY = ['M9 9.5A1.5 1.5 0 0 1 10.5 8h8A1.5 1.5 0 0 1 20 9.5v9a1.5 1.5 0 0 1-1.5 1.5h-8A1.5 1.5 0 0 1 9 18.5z', 'M15 8V5.5A1.5 1.5 0 0 0 13.5 4h-8A1.5 1.5 0 0 0 4 5.5v9A1.5 1.5 0 0 0 5.5 16H9'];
const COPIED = ['M5 12.5l4.5 4.5L19 7.5'];
const SPEAKER = ['M4 9.5h3.5L12 5.5v13l-4.5-4H4z', 'M15.5 9a4 4 0 0 1 0 6', 'M18 6.5a7.5 7.5 0 0 1 0 11'];
const PHOTO = ['M4 6.5A1.5 1.5 0 0 1 5.5 5h13A1.5 1.5 0 0 1 20 6.5v11a1.5 1.5 0 0 1-1.5 1.5h-13A1.5 1.5 0 0 1 4 17.5z', 'M5 17l4.5-4.5 3 3L15 13l4 4'];

/** An answer's words on the clipboard (the Copy under it). */
async function copyAnswer(t, b) {
  const text = t.a.trim();
  let done = false;
  try {
    await navigator.clipboard.writeText(text);
    done = true;
  } catch {
    // Without the async clipboard: the old way, from a hidden box.
    const box = document.createElement('textarea');
    box.value = text;
    box.setAttribute('readonly', '');
    box.style.cssText = 'position:fixed;opacity:0;top:0;left:0';
    document.body.appendChild(box);
    box.select();
    try {
      done = document.execCommand('copy');
    } catch {
      done = false;
    }
    box.remove();
  }
  if (!done) {
    S.note = "Couldn't copy it here. Press and hold the answer to select it.";
    render();
    return;
  }
  b.replaceChildren(icon('', COPIED), vh('Copied'));
  b.classList.add('done');
  setTimeout(() => {
    if (!b.isConnected) return;
    b.replaceChildren(icon('', COPY), vh('Copy'));
    b.classList.remove('done');
  }, 1600);
}

/** Words for a screen reader only. */
function vh(text) {
  const s = document.createElement('span');
  s.className = 'vh';
  s.textContent = text;
  return s;
}

/** Is turn `t`'s answer being said now? */
const speakingNow = (t) => S.speakingTurn === t.id && phaseNow() === 'speaking';

/**
 * The answer ▶ Read aloud is saying, when it's one in the thread: its ▶ is
 * the one Stop then (in place, where the thumb just was), and the
 * composer's Stop stays away. Otherwise the composer's Stop is the one.
 */
const readingInThread = () => !!S.reading && S.turns.some((t) => t.answerId === S.reading);

/**
 * The thread, as the Claude app has it: your question in a bubble on the
 * right (Chat's amber wash, 🎙 when it was said, its pictures above it), the
 * answer as plain text the width of the screen with its markdown
 * (markdown.js: built as nodes, never HTML), and under a finished one Copy
 * and ▶ Read aloud. While an answer is said, the bar's three bars and
 * "Speaking…" under it, a status only; ▶ that started it turns into Stop in
 * place. One Stop at a time (the composer's for a spoken answer).
 */
function renderConvo() {
  const frag = document.createDocumentFragment();
  for (const t of S.turns) {
    const li = document.createElement('li');
    li.className = 'turn';
    li.dataset.testid = 'turn';
    li.dataset.turn = String(t.id);
    // Its pictures: their thumbnails while this page has them, else how many.
    if (t.pics.length) {
      const row = document.createElement('div');
      row.className = 'qpics';
      row.dataset.testid = 'turn-pictures';
      for (const url of t.pics) {
        const img = new Image();
        img.alt = 'Picture';
        img.decoding = 'async';
        img.src = url;
        row.appendChild(img);
      }
      li.appendChild(row);
    } else if (t.pictures > 0) {
      const chip = document.createElement('p');
      chip.className = 'qpics-n';
      chip.dataset.testid = 'turn-pictures';
      chip.append(icon('', PHOTO), document.createTextNode(t.pictures === 1 ? '1 picture' : `${t.pictures} pictures`));
      li.appendChild(chip);
    }
    if (t.q || !t.pictures) {
      const q = document.createElement('p');
      q.className = 'q';
      q.dataset.testid = 'heard';
      if (!t.typed) {
        const mic = icon('viavoice', MIC);
        mic.setAttribute('aria-hidden', 'false');
        mic.setAttribute('role', 'img');
        mic.setAttribute('aria-label', 'Asked by voice');
        q.appendChild(mic);
      }
      q.appendChild(document.createTextNode(t.q || '…'));
      li.appendChild(q);
    }
    if (t.a || t.cancelled) {
      const a = document.createElement('div');
      const cancelled = t.cancelled && !t.a;
      a.className = 'a' + (t.final ? ' final' : ' streaming') + (cancelled ? ' cancelled' : '');
      a.dataset.testid = 'answer';
      if (cancelled) a.textContent = 'Cancelled';
      else a.appendChild(renderAnswer(t.a.trim()));
      li.appendChild(a);
    }
    const speaking = speakingNow(t);
    const done = t.final && !!t.a.trim();
    const readable = done && t.answerId && can('read_aloud');
    // Said because its ▶ was tapped: Stop there, in place.
    const readHere = speaking && !!S.reading && t.answerId === S.reading;
    if (done || speaking) {
      const foot = document.createElement('div');
      foot.className = 'msgfoot';
      if (done) {
        const c = document.createElement('button');
        c.type = 'button';
        c.className = 'footbtn';
        c.dataset.testid = 'copy';
        c.append(icon('', COPY), vh('Copy'));
        c.addEventListener('click', () => copyAnswer(t, c));
        foot.appendChild(c);
      }
      if (readHere || (readable && !speaking)) {
        const b = document.createElement('button');
        b.type = 'button';
        b.className = 'footbtn' + (readHere ? ' on' : '');
        b.dataset.testid = 'read-aloud';
        b.append(readHere ? icon('', SQUARE, true) : icon('', SPEAKER), vh(readHere ? 'Stop' : 'Read aloud'));
        b.addEventListener('click', () => (readHere ? interrupt() : readAloud(t)));
        foot.appendChild(b);
      }
      if (speaking) {
        const s = document.createElement('span');
        s.className = 'speaking';
        s.setAttribute('role', 'status');
        const bars = document.createElement('span');
        bars.className = 'speakbars';
        bars.setAttribute('aria-hidden', 'true');
        bars.append(document.createElement('i'), document.createElement('i'), document.createElement('i'));
        s.append(bars, document.createTextNode('Speaking…'));
        foot.appendChild(s);
      }
      li.appendChild(foot);
    }
    frag.appendChild(li);
  }
  el.convo.replaceChildren(frag);
  el.empty.hidden = S.turns.length > 0;
  el.older.hidden = !(S.more && S.turns.length > 0);
  if (threadPinned) toEnd();
}

// The thread takes the room the composer leaves it, so it shrinks when the
// keyboard comes up, an error or a note shows, or the box grows: the latest
// words stay in view, unless the person has scrolled up to read.
//
// Only the person scrolling up lets go of the end. A scroll event comes a
// frame after the scroll, so by then more may have been drawn below (the
// activity line under a question just sent, the next words of an answer):
// that's the thread growing, not the person leaving the end, so it never
// counts (Firefox read it as leaving, 2026-11-04: the answer then went on
// below the screen). `endTop`: where the thread was put at its end.
let threadPinned = true;
let endTop = 0;
function toEnd() {
  el.thread.scrollTop = el.thread.scrollHeight;
  endTop = el.thread.scrollTop;
}
el.thread.addEventListener(
  'scroll',
  () => {
    const t = el.thread;
    if (t.scrollHeight - t.scrollTop - t.clientHeight < 32) {
      threadPinned = true;
      endTop = t.scrollTop;
    } else if (t.scrollTop < endTop - 1) {
      threadPinned = false;
    }
  },
  { passive: true },
);
if (typeof ResizeObserver === 'function') {
  const keep = new ResizeObserver(() => {
    if (threadPinned) toEnd();
  });
  // The thread's own box, and everything in it that comes, goes or grows.
  for (const node of [el.thread, el.convo, el.step, el.older]) keep.observe(node);
}

/**
 * The on-screen keyboard: where the browser doesn't resize the page for it
 * (iPhones; Chrome does, `interactive-widget=resizes-content`), the page is
 * fitted to the visual viewport, so the composer rides above the keyboard
 * and the thread's end stays in view.
 */
function fitViewport() {
  const vv = window.visualViewport;
  const zoomed = vv && Math.abs((vv.scale || 1) - 1) > 0.01;
  const height = vv && !zoomed ? vv.height : window.innerHeight;
  const top = vv && !zoomed ? vv.offsetTop : 0;
  const root = document.documentElement.style;
  root.setProperty('--app-h', `${Math.round(height)}px`);
  root.setProperty('--app-top', `${Math.round(top)}px`);
  if (threadPinned) toEnd();
}
if (window.visualViewport) {
  window.visualViewport.addEventListener('resize', fitViewport);
  window.visualViewport.addEventListener('scroll', fitViewport);
}
window.addEventListener('resize', fitViewport);
fitViewport();

// --------------------------------------------------------------- the mic

const mic = {
  ctx: null,
  stream: null,
  source: null,
  node: null,
  opening: null,
  closeTimer: 0,
  deviceId: '',
  // Each opening is a run, numbered: one given up (Cancel, a step out of
  // time) has a number that's no longer `attempt`, and its late results go.
  attempt: 0,
  // The last run, for Sound details: {attempt, started, name (the step), at
  // (since), state: now | open | failed | cancelled, ms (its last step's
  // time), total, waiting (past PERMISSION_MS at the permission)}.
  run: null,
};

/** An Error with a name micError() knows. */
const named = (name) => Object.assign(new Error(name), { name });

/** `promise`, or after `ms` an Error named `name` (the promise carries on, unwatched). */
function within(promise, ms, name) {
  let timer;
  const late = new Promise((_, reject) => {
    timer = setTimeout(() => reject(named(name)), ms);
  });
  return Promise.race([promise, late]).finally(() => clearTimeout(timer));
}

function audioContext() {
  if (!mic.ctx) {
    const AC = window.AudioContext || window.webkitAudioContext;
    mic.ctx = new AC({ latencyHint: 'interactive' });
  }
  return mic.ctx;
}

/**
 * A touch's press isn't yet a gesture that lets a page start its sound (its
 * release is): the sound is asked to start again on each of these, so a
 * first Talk's AudioContext doesn't wait on a press that didn't count.
 */
function wakeSound() {
  if (mic.ctx && mic.ctx.state === 'suspended') mic.ctx.resume().catch(() => {});
}
for (const type of ['pointerup', 'touchend', 'click', 'keydown']) {
  document.addEventListener(type, wakeSound, { capture: true, passive: true });
}

/**
 * The page's AudioWorklet modules, each loaded once per context (again after
 * a failure), **one at a time**: the mic's and the player's at once on one
 * context is a suspect for a load that never finishes (Firefox). Each has
 * MODULE_MS from its turn; then it fails as `WorkletSlow`.
 */
const worklets = new WeakMap(); // context → {chain, files: {file: Promise}}
function worklet(ctx, file) {
  if (!ctx.audioWorklet) return Promise.reject(named('NoWorklet'));
  let w = worklets.get(ctx);
  if (!w) worklets.set(ctx, (w = { chain: Promise.resolve(), files: {} }));
  if (!w.files[file]) {
    const url = new URL(file, import.meta.url).href;
    const load = w.chain.then(() => within(ctx.audioWorklet.addModule(url), MODULE_MS, 'WorkletSlow'));
    // The next module waits for this one, loaded or not.
    w.chain = load.catch(() => {});
    w.files[file] = load.catch((err) => {
      w.files[file] = null;
      throw err;
    });
  }
  return w.files[file];
}

/**
 * The sound set up afresh on the next Talk (after a module that never
 * loaded): a new AudioContext, its modules loaded again, the player too.
 */
function resetAudio() {
  closeMic();
  player.reset();
  const ctx = mic.ctx;
  mic.ctx = null;
  if (ctx) ctx.close().catch(() => {});
}

/**
 * Resolves true once the mic's track is live and sending sound: a Bluetooth
 * headset takes a moment to switch to its call link (longer after it was
 * playing music), and what it says meanwhile isn't the person. At most
 * MIC_LIVE_MS; then false, and it goes ahead anyway.
 */
function whenLive(track) {
  if (!track || track.readyState !== 'live' || !track.muted) return Promise.resolve(true);
  return new Promise((resolve) => {
    const done = (e) => {
      clearTimeout(timer);
      track.removeEventListener('unmute', done);
      track.removeEventListener('ended', done);
      resolve(!!e);
    };
    const timer = setTimeout(done, MIC_LIVE_MS);
    track.addEventListener('unmute', done);
    track.addEventListener('ended', done);
  });
}

/**
 * Resolves once the phone's sound runs. While Talk is held this waits on (a
 * touch's press may not count as a gesture until it's let go: wakeSound),
 * then RESUME_MS at most; then `AudioStart`.
 */
function soundStarted(ctx) {
  if (ctx.state === 'running') return Promise.resolve();
  // Asked again now the mic is open: Firefox lets a page that records start its sound.
  ctx.resume().catch(() => {});
  return new Promise((resolve, reject) => {
    let since = performance.now();
    const timer = setInterval(() => {
      if (ctx.state === 'running') {
        clearInterval(timer);
        resolve();
      } else if (ctx.state === 'closed') {
        clearInterval(timer);
        reject(named('AudioStart'));
      } else if (press) since = performance.now();
      else if (performance.now() - since >= RESUME_MS) {
        clearInterval(timer);
        reject(named('AudioStart'));
      }
    }, 50);
  });
}

/** The microphone permission where the browser says ('granted' | 'denied' | 'prompt'), else ''. */
async function permissionState() {
  try {
    if (!navigator.permissions || !navigator.permissions.query) return '';
    const status = await within(navigator.permissions.query({ name: 'microphone' }), QUERY_MS, 'Slow');
    return (status && status.state) || '';
  } catch {
    return ''; // Firefox may not know 'microphone' here
  }
}

const PERMISSION_WAIT =
  'Waiting for the microphone permission. Tap the icon left of the address bar → Microphone → Allow, then tap Talk again.';
const PERMISSION_BLOCKED =
  'The microphone is blocked for this page. Tap the icon left of the address bar → Microphone → Allow, then tap Talk again.';

/** Opening the mic is waiting on the permission (past PERMISSION_MS): Talk tries afresh. */
const waitingPermission = () => !!(mic.opening && mic.run && mic.run.waiting);

function chosenMic() {
  const c = load(KEY.mic);
  return c && c.id ? c : null;
}

/** Opens the mic (once at a time: a second call waits for the first, unless it was given up). */
function openMic() {
  clearTimeout(mic.closeTimer);
  if (mic.opening) return mic.opening;
  const want = chosenMic();
  if (mic.stream && mic.stream.active && mic.deviceId === (want ? want.id : '')) return Promise.resolve();
  const now = performance.now();
  const run = { attempt: ++mic.attempt, started: now, name: '', at: now, state: 'now', ms: 0, total: 0, waiting: false };
  mic.run = run;
  let giveUp;
  const given = new Promise((_, reject) => (giveUp = reject));
  run.giveUp = () => giveUp(named('Abandoned'));
  const end = (state) => {
    if (run.state !== 'now') return;
    const t = performance.now();
    run.state = state;
    run.ms = t - run.at;
    run.total = t - run.started;
  };
  const p = Promise.race([doOpenMic(run), given]).then(
    () => end('open'),
    (err) => {
      end(err && err.name === 'Abandoned' ? 'cancelled' : 'failed');
      // A module that never loaded: the next Talk sets the sound up afresh.
      if (err && err.name === 'WorkletSlow' && run.attempt === mic.attempt) resetAudio();
      throw err;
    },
  );
  mic.opening = p;
  p.catch(() => {}).finally(() => {
    if (mic.opening === p) mic.opening = null;
    renderSound();
  });
  return p;
}

/** The opening under way given up (Cancel, or stuck at the permission): what it gets later is let go. */
function giveUpOpening() {
  const run = mic.run;
  if (!mic.opening || !run) return;
  mic.attempt++;
  mic.opening = null;
  run.giveUp();
}

/** Each step of opening the mic: what Sound details says, and where a given-up run stops. */
function step(run, name) {
  if (run.attempt !== mic.attempt) throw named('Abandoned');
  run.name = name;
  run.at = performance.now();
  renderSound();
}

const STEP_WORDS = {
  check: 'Checking the permission',
  recorder: 'Loading the recorder',
  player: 'Loading the player',
  permission: 'Asking for the microphone',
  sound: "Starting the phone's sound",
  live: 'Waiting for sound from the mic',
};

/**
 * Opening the mic, a step at a time, each with a time limit and a reason on
 * screen when it runs out: the permission's state (QUERY_MS), the recorder's
 * and the player's modules (MODULE_MS each, one after the other),
 * getUserMedia (after PERMISSION_MS, how to allow the mic, waiting on until
 * Cancel), the phone's sound (RESUME_MS once Talk is let go), sound from the
 * mic (MIC_LIVE_MS, then it goes ahead).
 */
async function doOpenMic(run) {
  const want = chosenMic();
  closeMic();
  step(run, 'check');
  if (!window.isSecureContext || !navigator.mediaDevices || !navigator.mediaDevices.getUserMedia) throw named('Insecure');
  const ctx = audioContext();
  if (!ctx.audioWorklet) throw named('NoWorklet');
  // Asked to start in Talk's own press; waited for once the mic is open (below).
  if (ctx.state !== 'running') ctx.resume().catch(() => {});
  // Blocked already: said at once, where the browser can tell.
  const blocked = (await permissionState()) === 'denied';

  // The recorder, then the answer's player, ready for the answer.
  step(run, 'recorder');
  try {
    await worklet(ctx, 'mic-worklet.js');
  } catch (err) {
    throw err && err.name === 'WorkletSlow' ? err : named('WorkletLoad');
  }
  step(run, 'player');
  await player.prepare();
  if (player.slow) throw named('WorkletSlow');

  step(run, 'permission');
  const stream = await askForMic(run, want, blocked);
  try {
    step(run, 'sound');
    await soundStarted(ctx);
    step(run, 'live');
  } catch (err) {
    for (const t of stream.getTracks()) t.stop();
    throw err;
  }
  const track = stream.getAudioTracks()[0];
  mic.stream = stream;
  mic.deviceId = want ? want.id : '';
  mic.source = ctx.createMediaStreamSource(stream);
  mic.node = new AudioWorkletNode(ctx, 'yap-mic', { numberOfInputs: 1, numberOfOutputs: 1, outputChannelCount: [1] });
  mic.node.port.onmessage = onMicMessage;
  mic.source.connect(mic.node);
  // A node only runs while it's pulled: its output (silence) goes out.
  mic.node.connect(ctx.destination);
  if (track) {
    track.onended = () => {
      if (mic.stream !== stream) return;
      const listening = S.local === 'listening';
      if (listening) stopTalk();
      closeMic();
      S.note = !listening
        ? ''
        : S.talk === 'words'
          ? MIC_GONE_WORDS
          : 'The microphone went away (was the headset switched off?). What it heard was sent.';
      render();
    };
  }
  // Names show once the mic's allowed.
  listMics();
  // Nothing counts until the mic really sends sound (Talk says "Opening the
  // microphone…" meanwhile).
  const live = await whenLive(track);
  if (mic.stream !== stream) throw named('MicClosed');
  if (!live && run.attempt === mic.attempt) {
    S.note = "The microphone hasn't sent any sound yet (a headset switching over?). Yap is listening anyway.";
  }
  player.noteMic();
  renderSound();
}

/**
 * getUserMedia, the chosen mic or else the phone's own. Past PERMISSION_MS
 * (at once when the browser says the mic is blocked) the page says how to
 * allow it and waits on: answering the prompt late still works, Talk tries
 * afresh, Cancel gives up.
 */
async function askForMic(run, want, blocked) {
  const timer = setTimeout(
    () => {
      if (run.attempt !== mic.attempt) return;
      run.waiting = true;
      if (S.local === 'opening') S.error = blocked ? PERMISSION_BLOCKED : PERMISSION_WAIT;
      render();
    },
    blocked ? 0 : PERMISSION_MS,
  );
  const audio = { echoCancellation: true, noiseSuppression: true, autoGainControl: true };
  let stream;
  let fellBack = false;
  try {
    try {
      stream = await navigator.mediaDevices.getUserMedia({
        audio: want ? { ...audio, deviceId: { exact: want.id } } : audio,
      });
    } catch (err) {
      if (!want || (err.name !== 'OverconstrainedError' && err.name !== 'NotFoundError')) throw err;
      // The chosen mic isn't here (the headset's off): the phone's own.
      stream = await navigator.mediaDevices.getUserMedia({ audio });
      fellBack = true;
    }
  } finally {
    clearTimeout(timer);
  }
  if (run.attempt !== mic.attempt) {
    // Given up meanwhile: this mic goes straight back.
    for (const t of stream.getTracks()) t.stop();
    throw named('Abandoned');
  }
  if (run.waiting) {
    run.waiting = false;
    if (S.error === PERMISSION_WAIT || S.error === PERMISSION_BLOCKED) S.error = '';
  }
  if (fellBack) S.note = `${want.label || 'The chosen microphone'} isn't connected, so this uses the phone's own.`;
  return stream;
}

function closeMic() {
  clearTimeout(mic.closeTimer);
  if (mic.node) {
    mic.node.port.onmessage = null;
    try {
      mic.node.disconnect();
    } catch {
      /* gone */
    }
  }
  if (mic.source) {
    try {
      mic.source.disconnect();
    } catch {
      /* gone */
    }
  }
  const was = !!mic.stream;
  if (mic.stream) for (const t of mic.stream.getTracks()) t.stop();
  mic.stream = mic.source = mic.node = null;
  if (was) {
    player.noteMic();
    renderSound();
  }
}

/**
 * The mic stays open a little after a turn: from Talk to the end of the
 * answer a Bluetooth headset stays in one call link (SCO), and a follow-up
 * starts at once. Then it closes, so the headset goes back to its music link.
 *
 * With "Clearer sound in a headset" on (the default), the mic closes as soon as the
 * question is sent (so the headset has the time Yap spends working it out to
 * switch to its music link, A2DP, before the answer starts) and, in any
 * case, when the answer starts (`speak_start`); it opens again on Talk, or
 * once the answer has played out when hands-free is on (then kept warm as
 * above).
 */
function afterQuestionSent() {
  if (S.clearer) closeMic();
  else armMicClose();
}

/** The answer has played out (or was stopped): the mic's next move. */
function afterAnswer() {
  if (S.clearer && S.handsFree && S.local === 'idle' && !mic.stream) {
    // Back to the call link, ready for a follow-up (warm, then closed).
    openMic()
      .then(armMicClose)
      .catch(() => {});
  } else armMicClose();
}

function armMicClose() {
  clearTimeout(mic.closeTimer);
  if (!mic.stream) return;
  mic.closeTimer = setTimeout(function check() {
    if (S.local === 'idle' && S.phase === 'idle' && !player.active) closeMic();
    else mic.closeTimer = setTimeout(check, MIC_WARM_MS);
  }, MIC_WARM_MS);
}

function micError(err) {
  switch (err && err.name) {
    case 'NotAllowedError':
    case 'SecurityError':
      return PERMISSION_BLOCKED;
    case 'WorkletSlow':
      return "The audio setup didn't finish: reload the page, then try again.";
    case 'AudioStart':
      return "The phone's sound didn't start. Tap Talk again, or reload the page.";
    case 'NotFoundError':
      return 'No microphone found on this phone.';
    case 'NotReadableError':
    case 'AbortError':
      return 'The microphone is busy. Finish any call or recording, then try again.';
    case 'Insecure':
      return "The microphone only works on Yap's secure page. Open it from the code in Yap on your PC.";
    case 'NoWorklet':
      return "This browser can't record here. Open the page in Chrome.";
    case 'WorkletLoad':
      return "Couldn't load the page's recorder from your PC. Reload the page and try again.";
    case 'MicClosed':
      return 'The microphone closed while it was opening. Try again.';
    default:
      return "Couldn't open the microphone. Try again.";
  }
}

async function listMics() {
  if (!navigator.mediaDevices || !navigator.mediaDevices.enumerateDevices) return;
  let all;
  try {
    all = await navigator.mediaDevices.enumerateDevices();
  } catch {
    return;
  }
  const mics = all.filter((d) => d.kind === 'audioinput' && d.deviceId && d.deviceId !== 'default');
  const hasNames = mics.some((d) => d.label);
  let chosen = chosenMic();
  // Device ids can change; the same name is the same mic.
  if (chosen && !mics.some((d) => d.deviceId === chosen.id)) {
    const same = mics.find((d) => d.label && d.label === chosen.label);
    if (same) {
      chosen = { id: same.deviceId, label: same.label };
      save(KEY.mic, chosen);
    }
  }
  const opts = [new Option("Phone's default microphone", '')];
  mics.forEach((d, i) => opts.push(new Option(d.label || `Microphone ${i + 1}`, d.deviceId)));
  if (chosen && !mics.some((d) => d.deviceId === chosen.id)) {
    opts.push(new Option(`${chosen.label || 'Chosen microphone'} (not connected)`, chosen.id));
  }
  el.mic.replaceChildren(...opts);
  el.mic.value = chosen ? chosen.id : '';
  el.micHint.textContent = hasNames
    ?'Pick your headset here once it’s connected.'
    : 'Names show once you’ve allowed the microphone.';
}

function onMicMessage(e) {
  const d = e.data || {};
  if (d.frame && (S.local === 'listening' || S.local === 'stopping')) {
    const samples = new Int16Array(d.frame);
    if (S.ws && S.ws.readyState === WebSocket.OPEN) S.ws.send(upFrame(S.seq, samples));
    S.seq = (S.seq + 1) >>> 0;
    S.frames++;
    meter(d.level || 0);
    if (S.local === 'listening') {
      if (S.handsFree) {
        const said = vad.push(d.level || 0);
        if (said === 'end') stopTalk();
        else if (!vad.heard && S.frames * FRAME_MS >= NO_SPEECH_MS) {
          cancelTalk();
          S.note = "Didn't hear anything. Tap Talk to try again.";
          render();
          return;
        }
      }
      if (S.frames >= MAX_FRAMES) {
        stopTalk();
        S.note =
          S.talk === 'words'
            ? "Stopped at 2 minutes: that's the longest Yap listens. What it heard is in the box."
            : "Sent at 2 minutes: that's the longest a question can be.";
        render();
      }
    }
  }
  if (d.last && S.local === 'stopping') finishStop();
}

function meter(level) {
  const v = Math.max(0, Math.min(1, (toDb(level) + 60) / 50));
  levels.push(v);
  levels.shift();
  for (let i = 0; i < BARS; i++) bars[i].style.transform = `scaleY(${(0.07 + levels[i] * 0.93).toFixed(3)})`;
}

function resetMeter() {
  levels.fill(0);
  for (const b of bars) b.style.transform = 'scaleY(0.07)';
}

// --------------------------------------------------------- talking: actions

const busyListening = () => S.local === 'listening' || S.local === 'opening';

/**
 * Does Talk put its words in the box (to check, then send) rather than
 * send them? Yes unless hands-free is on, where the PC can (`words` in
 * `ready.can`; a PC from before always takes the question).
 */
const wordsMode = () => can('words') && !S.handsFree;

async function beginTalk() {
  S.error = '';
  S.note = '';
  if (!S.ready) {
    S.error = `Not connected to ${pcName()} yet.`;
    render();
    return;
  }
  if (S.local !== 'idle') return;
  keepAwake();
  const now = phaseNow();
  // What this one's for, chosen now (hands-free switched meanwhile changes the next).
  S.talk = wordsMode() ? 'words' : 'ask';
  // Barge-in: the answer stops at once, then Yap listens.
  if (now === 'speaking') interrupt();
  else if (now === 'transcribing') dropWords();
  else if (now === 'working') {
    send({ type: 'cancel' });
    markCancelled();
    S.ignoreOldAt = performance.now();
  }
  // An opening stuck at the permission: Talk tries afresh (the person may
  // have allowed it from the address bar meanwhile).
  if (waitingPermission()) giveUpOpening();
  S.local = 'opening';
  const gen = ++S.gen;
  render();
  try {
    await openMic();
  } catch (err) {
    if (S.local === 'opening' && gen === S.gen) {
      S.local = 'idle';
      if (err && err.name !== 'Abandoned') S.error = micError(err);
      render();
    }
    return;
  }
  if (S.local !== 'opening' || gen !== S.gen) {
    // Let go (or tapped again) before the mic was open: nothing was said.
    armMicClose();
    return;
  }
  if (!S.ready) {
    S.local = 'idle';
    render();
    return;
  }
  S.seq = 0;
  S.frames = 0;
  S.qStarted = performance.now();
  S.pcListening = false;
  S.step = '';
  S.workSince = 0;
  vad = new EnergyVad({ frameMs: FRAME_MS, endMs: WAITS[S.wait] });
  resetMeter();
  send({ type: 'start', mode: S.talk, ...chatField() });
  mic.node.port.postMessage({ cmd: 'start' });
  S.local = 'listening';
  // A question's bubble once it's asked: words for the box have none (the
  // question they become is typed, from Send).
  if (S.talk === 'ask') currentTurn(true);
  renderConvo();
  syncTicker();
  render();
}

/** Talk pressed again while the mic opens: a tap that tries afresh when it's stuck at the permission. */
function restartTalk() {
  giveUpOpening();
  S.local = 'idle';
  beginTalk();
}

function stopTalk() {
  if (S.local === 'opening') {
    S.local = 'idle';
    // Stuck at the permission: given up (its message stays, saying what to do).
    if (waitingPermission()) giveUpOpening();
    else if (S.holdRelease) S.note = 'The microphone took a moment to open. Hold Talk a little longer, or tap it to talk.';
    syncTicker();
    render();
    return;
  }
  if (S.local !== 'listening') return;
  S.local = 'stopping';
  // The worklet sends its part-frame, then `last`: then `stop` goes.
  if (mic.node) mic.node.port.postMessage({ cmd: 'stop' });
  clearTimeout(S.stopTimer);
  S.stopTimer = setTimeout(finishStop, 400);
  render();
}

function finishStop() {
  clearTimeout(S.stopTimer);
  if (S.local !== 'stopping') return;
  if (S.talk === 'words') {
    // Its words come back for the box (`words`): nothing asked, the
    // pictures still in the composer for the question Send makes.
    send({ type: 'stop' });
    S.local = 'idle';
    S.transcribing = 'sent';
    afterQuestionSent();
    syncTicker();
    render();
    return;
  }
  // The pictures in the composer go with the spoken question too, sent just
  // before its end.
  const pics = S.attached.slice();
  const extra = {};
  if (pics.length && can('pictures') && sendPictures(pics, send, sendBinary)) {
    extra.pictures = pics.length;
    const t = currentTurn(false);
    if (t && !t.final) {
      t.pictures = pics.length;
      t.pics = pics.map((p) => p.url);
    }
    // (Back in the box if Chat's model can't read them: still said.)
    S.pending = { text: '', pics, said: true };
    S.attached = [];
    S.cantRead = '';
    renderShots();
    renderConvo();
  }
  send({ type: 'stop', ...extra });
  S.local = 'idle';
  if (S.phase === 'idle' || S.phase === 'listening') S.phase = 'transcribing';
  S.workSince = performance.now();
  afterQuestionSent();
  syncTicker();
  render();
}

/** Cancel: a question being asked is dropped; one being worked on is called off. */
function cancelTalk() {
  if (busyListening() || S.local === 'stopping') {
    const sent = S.local !== 'opening';
    if (!sent) {
      // The mic was still opening: that's given up too, whatever step it was on.
      giveUpOpening();
      if (S.error === PERMISSION_WAIT || S.error === PERMISSION_BLOCKED) S.error = '';
    }
    endLocalQuestion();
    if (sent) {
      send({ type: 'cancel' });
      S.ignoreOldAt = performance.now();
    }
    const t = currentTurn(false);
    if (t && !t.q && !t.a && !t.final) S.turns.pop();
    S.phase = 'idle';
  } else if (S.transcribing) {
    // Words on their way to the box: dropped, the box as it was.
    dropWords();
  } else if (phaseNow() === 'working') {
    send({ type: 'cancel' });
    markCancelled();
    S.ignoreOldAt = performance.now();
    S.phase = 'idle';
    S.step = '';
    S.workSince = 0;
  }
  armMicClose();
  renderConvo();
  syncTicker();
  render();
}

function markCancelled() {
  const t = currentTurn(false);
  if (t && !t.final) {
    t.final = true;
    t.cancelled = true;
    saveThread();
  }
  renderConvo();
}

/** Stop: the answer stops at once, and the PC stops making it. */
function interrupt() {
  player.stopAll();
  send({ type: 'interrupt' });
  S.ignoreOldAt = performance.now();
  S.phase = 'idle';
  S.step = '';
  const t = currentTurn(false);
  if (t && !t.final) {
    t.final = true;
    saveThread();
  }
  armMicClose();
  doneSpeaking();
  render();
}

/** No answer is being said any more: its ■ goes back to ▶. */
function doneSpeaking() {
  if (!S.reading && !S.speakingTurn) return;
  S.reading = '';
  S.speakingTurn = 0;
  renderConvo();
}

/** This page stops listening, without telling the PC. */
function endLocalQuestion() {
  clearTimeout(S.stopTimer);
  if (mic.node) mic.node.port.postMessage({ cmd: 'stop' });
  S.local = 'idle';
  S.holdRelease = false;
  syncTicker();
}

// ------------------------------------------------------------ the answer

const idleStats = () => ({
  state: 'idle',
  queued: 0,
  played: 0,
  buffered: 0,
  underruns: 0,
  startDelayMs: null,
  startBufferedMs: null,
  gapMs: 0,
  maxStep: 0,
});

/**
 * Yap's voice, as one stream. Each down frame (24 kHz Int16) is resampled to
 * the context's rate as it comes, in one filter carried on from frame to
 * frame (never a clip at a time, which clicked at every join), and posted to
 * the player worklet (player-worklet.js), which plays it from a jitter buffer
 * (dsp.js `Playout`): the sound starts once 250 ms is waiting, a late frame
 * fades out and back in (an underrun, counted) instead of a hard gap, Stop is
 * a 5 ms fade. `active` from speak_start until the end has played out, or
 * Stop.
 */
const player = {
  rate: DOWN_RATE, // speak_start's
  id: 0, // this answer's, on every message to and from the worklet
  node: null,
  loading: null,
  waiting: [], // messages for the worklet before it's loaded
  broken: false,
  slow: false, // its module never loaded (MODULE_MS)
  rs: null,
  accepting: false,
  ending: false,
  queued: 0, // seconds of the answer received
  stats: idleStats(), // the worklet's last report
  level: 0, // the loudest sample it played since the report before
  last: null, // the last answer, for Sound details
  get active() {
    return this.accepting;
  },
  /** While the answer is being played (not before its sound starts). */
  get sounding() {
    return this.accepting && (this.stats.state === 'playing' || this.stats.state === 'starved');
  },

  /**
   * Loads the worklet and makes its node, once, ahead of the first answer.
   * Resolves when that's done or has failed (`broken`; `slow` when the
   * module never loaded), never rejects.
   */
  prepare() {
    if (this.node || this.broken) return Promise.resolve();
    if (this.loading) return this.loading;
    const ctx = audioContext();
    const p = worklet(ctx, 'player-worklet.js')
      .then(() => {
        if (mic.ctx !== ctx) return; // the sound was set up afresh since
        const node = new AudioWorkletNode(ctx, 'yap-player', { numberOfOutputs: 1, outputChannelCount: [1] });
        node.port.onmessage = (e) => this.report(e.data || {});
        node.onprocessorerror = () => this.fail();
        node.connect(ctx.destination);
        this.node = node;
        for (const [msg, transfer] of this.waiting) node.port.postMessage(msg, transfer);
        this.waiting = [];
      })
      .catch((err) => {
        if (mic.ctx !== ctx) return;
        this.slow = !!err && err.name === 'WorkletSlow';
        this.fail();
      })
      .finally(() => {
        if (this.loading === p) this.loading = null;
      });
    this.loading = p;
    return p;
  },
  /** Back to before it was loaded (the sound set up afresh): nothing playing, nothing waiting. */
  reset() {
    this.stopAll();
    if (this.node) {
      this.node.port.onmessage = null;
      try {
        this.node.disconnect();
      } catch {
        /* gone */
      }
    }
    this.node = null;
    this.loading = null;
    this.waiting = [];
    this.broken = false;
    this.slow = false;
  },
  /** The worklet couldn't load (or broke): nothing plays, nothing waits for it. */
  fail() {
    this.broken = true;
    this.waiting = [];
    if (this.node) {
      this.node.port.onmessage = null;
      try {
        this.node.disconnect();
      } catch {
        /* gone */
      }
      this.node = null;
    }
    if (this.accepting) {
      S.note = "Couldn't play Yap's voice on this phone. Reload the page, then ask again.";
      if (this.ending) this.finished();
      else render();
    }
  },
  post(msg, transfer = []) {
    if (this.node) this.node.port.postMessage(msg, transfer);
    else if (!this.broken) {
      this.waiting.push([msg, transfer]);
      this.prepare();
    }
  },

  begin(rate) {
    this.stopAll();
    const ctx = audioContext();
    if (ctx.state !== 'running') ctx.resume().catch(() => {});
    this.rate = rate;
    this.id++;
    this.accepting = true;
    this.ending = false;
    this.queued = 0;
    this.level = 0;
    this.rs = new Resampler(rate, ctx.sampleRate);
    this.stats = { ...idleStats(), state: 'buffering' };
    this.last = { startDelayMs: null, startBufferedMs: null, underruns: 0, played: 0, micOpen: false, micClosed: false };
    this.post({ cmd: 'begin', id: this.id });
    if (this.broken) this.fail();
  },
  push(samples) {
    if (!this.accepting || this.ending || !samples.length) return;
    this.queued += samples.length / this.rate;
    this.send(this.rs.process(fromInt16(samples)));
  },
  send(out) {
    if (!out.length) return;
    const copy = new Float32Array(out); // the resampler reuses its buffer
    this.post({ cmd: 'push', id: this.id, samples: copy.buffer }, [copy.buffer]);
  },
  /** No more frames: the filter's last samples, then what's waiting plays out. */
  end() {
    if (!this.accepting || this.ending) return;
    this.ending = true;
    this.send(this.rs.flush());
    this.post({ cmd: 'end', id: this.id });
    if (this.broken) this.finished();
  },
  /** A report from the worklet. */
  report(d) {
    if (d.id !== this.id || !d.stats) return; // an answer since replaced
    this.stats = d.stats;
    this.level = d.level || 0;
    const L = this.last;
    if (L) {
      if (d.stats.startDelayMs != null) {
        L.startDelayMs = d.stats.startDelayMs;
        L.startBufferedMs = d.stats.startBufferedMs;
      }
      L.underruns = d.stats.underruns;
      L.played = d.stats.played;
    }
    this.noteMic();
    if (Array.isArray(d.events) && d.events.includes('done')) this.finished();
    else renderSound();
  },
  /** While the answer plays: the mic open (the headset in its call link) or not. */
  noteMic() {
    const L = this.last;
    if (!L || !this.sounding) return;
    if (mic.stream) L.micOpen = true;
    else L.micClosed = true;
  },
  /** The end has played out. */
  finished() {
    if (!this.accepting) return;
    this.accepting = false;
    this.ending = false;
    afterAnswer();
    doneSpeaking();
    render();
  },
  /** Silent at once (a 5 ms fade in the worklet); what's left is dropped. */
  stopAll() {
    if (this.accepting) this.post({ cmd: 'stop', id: this.id });
    this.accepting = false;
    this.ending = false;
  },
};

// ------------------------------------------------------------ what it shows

/** offline | opening | listening | transcribing (Talk's words, for the box) | working | speaking | idle */
function phaseNow() {
  if (S.local === 'opening') return 'opening';
  if (S.local === 'listening') return 'listening';
  if (S.local === 'stopping') return S.talk === 'words' ? 'transcribing' : 'working';
  if (!S.ready) return 'offline';
  if (S.transcribing) return 'transcribing';
  if (player.active || S.phase === 'speaking') return 'speaking';
  if (S.phase === 'listening' || S.phase === 'transcribing' || S.phase === 'thinking') return 'working';
  return 'idle';
}

const PHASE_WORDS = {
  offline: '',
  opening: 'Opening the microphone…',
  listening: 'Listening',
  transcribing: 'Transcribing…',
  working: 'Working it out',
  speaking: 'Speaking',
  idle: 'Ready',
};

function connLine() {
  if (S.ready) return `Connected to ${pcName()}`;
  if (S.denied || (!S.device && !S.pairToken)) return 'Not paired';
  if (S.retries > 0) return `Reconnecting to ${pcName()}…`;
  return S.pc ? `Connecting to ${S.pc}…` : 'Connecting to Yap…';
}

function render() {
  // Pairing: no device, or a code being checked. A tab with a device and a
  // code is connecting as any tab is, until the PC says which (`ready`, or
  // the code's `pair_wait` when it doesn't know the device).
  const pairing = S.denied || (!S.device && !S.ready) || (!!S.pairToken && !S.ready && (!S.device || !!S.pairCode));
  el.pair.hidden = !pairing;
  el.chat.hidden = pairing;
  el.settingsBtn.hidden = pairing;
  // ☰: the drawer of chats, where the PC keeps them.
  el.menuBtn.hidden = pairing || !can('chats');
  if (pairing) {
    if (el.settings.open) closeSettings();
    if (picker.isOpen) picker.close();
    if (drawer.isOpen) drawer.close();
  }
  const conn = connLine();
  const state = S.ready ? 'on' : S.denied || (!S.device && !S.pairToken) ? 'off' : 'wait';
  el.conn.textContent = conn;
  el.conn.dataset.state = state;
  // The title: Yap and this PC, with the connection's dot; under it the
  // chat's title, or how the connection is while it isn't.
  el.dot.dataset.state = state;
  el.titleName.textContent = S.pc ? `Yap · ${S.pc}` : 'Yap';
  const sub = !S.ready ? (pairing ? '' : conn) : typeof S.chat === 'number' ? S.title : '';
  el.titleSub.textContent = sub;
  el.titleSub.hidden = !sub;

  if (pairing) {
    const showCode = !!S.pairCode && !S.denied;
    el.pairCodeBox.hidden = !showCode;
    el.pairCode.textContent = S.pairCode;
    el.pairCode.setAttribute('aria-label', S.pairCode.split('').join(' '));
    const waiting = !!S.pairToken && !S.pairCode && !S.denied;
    el.pairWait.hidden = !waiting;
    el.pairHow.hidden = showCode || waiting;
    el.pairWhy.textContent = S.denied ? S.deniedWhy : '';
    el.pairWhy.hidden = !S.denied;
    el.retry.hidden = !(S.denied && S.device);
  }

  // The model chip in the composer, as the Claude app has it: the model's
  // mark and name, then its effort ("Claude Opus 5.5 · Extra") or, without
  // one, where it runs; a PC from before the picker, its label.
  if (S.model && S.ready) {
    el.modelBtn.hidden = false;
    const picked = S.model.picked;
    const words = chipWords(S.model, S.picker);
    const effort = picked ? '' : chipEffort(S.picker);
    const where = picked ? picked.where : words.where;
    // "Claude" stays for a screen reader; a narrow card drops it (the mark
    // says it), as the Claude app's chip says "Opus 5.5".
    const name = picked ? picked.label : words.name;
    const fam = /^Claude\s/.test(name) ? 'Claude ' : '';
    const famEl = document.createElement('span');
    famEl.className = 'chip-fam';
    famEl.textContent = fam;
    el.modelLabel.replaceChildren(...(fam ? [famEl] : []), name.slice(fam.length));
    el.modelWhere.textContent = effort || where;
    el.modelWhere.hidden = !(effort || where);
    el.modelWhere.dataset.effort = String(!!effort);
    const mark = picked ? { icon: picked.icon, glyph: picked.glyph } : modelMark(S.model, S.picker);
    const markKey = `${mark.icon}|${mark.glyph}`;
    if (el.modelMark.dataset.key !== markKey) {
      const fresh = markEl(mark.icon, mark.glyph, 18);
      fresh.id = 'model-mark';
      fresh.dataset.key = markKey;
      el.modelMark.replaceWith(fresh);
      el.modelMark = fresh;
    }
    el.modelBtn.setAttribute(
      'aria-label',
      `Model: ${el.modelLabel.textContent}${where ? `, ${where}` : ''}${effort ? `, ${effort} effort` : ''}. Change model`,
    );
  } else el.modelBtn.hidden = true;

  const now = phaseNow();
  document.body.dataset.phase = now;
  el.phase.textContent = PHASE_WORDS[now];
  // A small spinner in the header while Yap works the question out.
  el.busy.hidden = now !== 'working';

  // The step Yap is on, as Chat's activity line shows it, until the answer's in.
  const turn = currentTurn(false);
  const working = now === 'working' && S.local === 'idle' && !(turn && turn.final);
  el.step.hidden = !working;
  if (working) {
    el.stepLine.textContent = S.step || 'Thinking';
    const s = S.workSince ? (performance.now() - S.workSince) / 1000 : 0;
    el.stepTime.textContent = s >= 1 ? ` · ${elapsed(s)}` : '';
  }

  el.error.hidden = !S.error;
  el.error.textContent = S.error;
  el.note.hidden = !S.note;
  el.note.textContent = S.note;

  // The composer card: the pictures, the text box, then +, the chip, the
  // mic and Send (■ while Yap answers); while Yap listens, the Yap bar's
  // capsule (✕, the waveform, the time, Done), and while Talk's words are
  // transcribed for the box, the capsule with "Transcribing…" (✕ still
  // cancels).
  const listening = now === 'listening';
  const capsule = listening || now === 'opening' || now === 'transcribing';
  const words = S.talk === 'words' && capsule;
  el.composer.dataset.state = now === 'offline' ? 'idle' : now;
  el.composer.dataset.talk = words ? 'words' : 'ask';
  el.composer.dataset.typed = String(S.typed);
  el.composer.dataset.pictures = String(S.attached.length > 0 || S.preparing > 0);
  el.liveLabel.textContent = now === 'opening' || now === 'transcribing' ? PHASE_WORDS[now] : '';
  el.talkBtn.disabled = (!S.ready && S.local === 'idle') || now === 'transcribing';
  el.talkBtn.setAttribute('aria-pressed', String(listening || now === 'opening'));
  el.talkLabel.textContent =
    now === 'opening'
      ? waitingPermission()
        ? 'Talk' // the message says "tap Talk again": it tries afresh
        : 'Wait…'
      : listening
        ? S.holdRelease
          ? words
            ? 'Release when done'
            : 'Release to send'
          : 'Done'
        : S.local === 'stopping'
          ? words
            ? 'Transcribing…'
            : 'Sending…'
          : 'Talk';
  el.talkTime.textContent = listening ? clock(S.frames * (FRAME_SAMPLES / LINK_RATE)) : '';
  el.meter.dataset.on = String(listening);
  // + for a picture, where the PC takes them.
  el.plusWrap.hidden = capsule || !S.ready || !can('pictures');
  if (el.plusWrap.hidden) closePlusMenu();
  // One button where Send is: ↑ to send; ■ while Yap works the question out
  // (cancel) or says an answer (Stop) — unless there are words to send
  // instead, or ▶ on an answer in the thread is what's playing (then its own
  // Stop, in place): one Stop at a time.
  const stopSpeaking = now === 'speaking' && !readingInThread() && !S.typed;
  const stopWorking = now === 'working' && !S.typed;
  el.stopBtn.hidden = capsule || !stopSpeaking;
  el.sendBtn.hidden = capsule || stopSpeaking || stopWorking;
  el.sendBtn.disabled = !S.typed || S.preparing > 0;
  // ✕ while it listens (on the left, as the bar's capsule has it); ■ while
  // it works the question out.
  el.cancelBtn.hidden = !(capsule || stopWorking);
  el.cancelBtn.setAttribute('aria-label', capsule ? 'Cancel' : 'Stop the answer');
  // Chat's line for pictures its model can't read.
  const line = capsule ? '' : visionLine();
  el.vision.hidden = !line;
  if (line) {
    const name = el.modelLabel.textContent || 'This model';
    el.visionText.textContent = /can't read images/i.test(line) && !/couldn't/i.test(line) ? `${name} can't read images.` : `${name} couldn't read the picture.`;
    const sw = visionSwitch();
    el.visionSwitch.hidden = !sw;
    if (sw) {
      // As Chat names it: "Use Qwen3.8 27B on Groq".
      const named = sw.name || labelFor(sw.provider, sw.model, S.picker.catalog?.[sw.provider]) || sw.model;
      const on = PROVIDER_LABELS[sw.provider];
      el.visionSwitch.textContent = `Use ${named}${on && on !== named ? ` on ${on}` : ''}`;
    }
    el.visionPick.textContent = sw ? 'Choose another model…' : 'Choose a model…';
    for (const b of [el.visionSwitch, el.visionAsText, el.visionWithout]) b.disabled = !S.typed;
  }
  el.handsFree.checked = S.handsFree;
  // What hands-free changes: where the PC can put Talk's words in the box.
  // (Not known yet: as a PC today has it.)
  const known = S.ready || S.can.length > 0;
  el.handsFreeHint.textContent = !known || can('words') ? HANDS_FREE_HINT.words : HANDS_FREE_HINT.sends;
  el.wait.hidden = !S.handsFree;
  for (const r of el.wait.querySelectorAll('input[name=wait]')) r.checked = r.value === S.wait;
  el.clearer.checked = S.clearer;
  renderSound();
}

const num = (n) => Math.round(n).toLocaleString('en-GB');
const msOf = (s) => `${num(s * 1000)} ms`;

/** Sound details: how the last answer played, to tell a stream that ran dry from the headset's own crackle. */
function renderSound() {
  if (!el.sound.open) return;
  const ctx = mic.ctx;
  el.soundRate.textContent = ctx ? `${num(ctx.sampleRate)} Hz` : 'Shown once you’ve asked something';
  const lat = [];
  if (ctx && typeof ctx.baseLatency === 'number') lat.push(`Base ${msOf(ctx.baseLatency)}`);
  if (ctx && typeof ctx.outputLatency === 'number' && ctx.outputLatency > 0) lat.push(`Output ${msOf(ctx.outputLatency)}`);
  el.soundLatency.textContent = !ctx ? '—' : lat.length ? lat.join(' · ') : 'Not given by this browser';

  const L = player.last;
  if (!L) {
    el.soundStart.textContent = 'No answer yet';
    el.soundGaps.textContent = '—';
    el.soundMic.textContent = '—';
  } else {
    el.soundStart.textContent =
      L.startDelayMs == null
        ? player.accepting
          ? 'Waiting for its sound…'
          : 'No sound came'
        : `${L.played.toFixed(1)} s played; started after ${num(L.startDelayMs)} ms, with ${num(L.startBufferedMs)} ms buffered`;
    el.soundGaps.textContent =
      L.underruns === 0 ? 'None' : `${L.underruns} (the sound ran dry and paused)`;
    el.soundMic.textContent =
      L.micOpen && L.micClosed
        ? 'Open for part of it'
        : L.micOpen
          ? 'Open (the headset in call mode)'
          : L.micClosed
            ? 'Closed'
            : '—';
  }
  el.soundMicNow.textContent = mic.stream ? 'Open' : 'Closed';
  el.soundMicStep.textContent = micStepLine();
  el.soundBrowser.textContent = browserLine();
}

const took = (ms) => (ms < 1000 ? `${num(ms)} ms` : `${(ms / 1000).toFixed(1)} s`);

/** Where opening the mic got to, and how long that step took: a stuck step reads off the screen. */
function micStepLine() {
  const r = mic.run;
  if (!r || !r.name) return 'Not opened yet';
  const what = STEP_WORDS[r.name] || 'Opening the microphone';
  if (r.state === 'now') return `${what}… ${took(performance.now() - r.at)} so far`;
  if (r.state === 'open') return `${what}: ${took(r.ms)} (open after ${took(r.total)})`;
  if (r.state === 'cancelled') return `${what}: cancelled after ${took(r.ms)}`;
  return `${what}: stopped after ${took(r.ms)}`;
}

/** Times on screen tick while Yap listens or works. */
function syncTicker() {
  const on = S.local !== 'idle' || S.phase === 'transcribing' || S.phase === 'thinking' || S.phase === 'listening';
  if (on && !S.tick) S.tick = setInterval(render, 250);
  else if (!on && S.tick) {
    clearInterval(S.tick);
    S.tick = 0;
  }
}

// ------------------------------------------------------- typing, ▶, sheets

/** The box grows with what's typed, up to five lines (then it scrolls). */
function fitInput() {
  const t = el.input;
  t.style.height = 'auto';
  t.style.height = `${t.scrollHeight}px`;
}

/**
 * **The composer's question** (`ask`): answered by Yap's AI on the PC in
 * the same conversation as the spoken ones. Typed, it's answered on screen
 * — ▶ under the answer has it read; with Talk's words in it (`S.said`), as
 * the spoken question it was (`speak`): the bubble's 🎙, the answer said on
 * the phone with its ■, as a spoken question's is. Whatever Yap was doing
 * is let go first (as Talk does): its voice stopped, or the question it
 * was working on cancelled.
 */
function sendTyped(how = {}) {
  const text = el.input.value.trim();
  if (!text) return;
  const pics = how.without ? [] : S.attached.slice();
  // Pictures Chat's model can't read: its line says what can be done (the
  // send goes from one of its buttons).
  if (pics.length && !how.asText && !how.anyway && visionLine()) {
    render();
    return;
  }
  S.note = '';
  if (!S.ready) {
    S.error = `Not connected to ${pcName()} yet.`;
    render();
    return;
  }
  if (!can('ask')) {
    S.error = OLD_PC_TYPING;
    render();
    return;
  }
  if (text.length > MAX_TYPED) {
    S.error = 'That question is too long to send from the phone. Shorten it, then send it again.';
    render();
    return;
  }
  if (S.local !== 'idle') return; // listening: the capsule's Send is the way
  if (S.transcribing) return; // Talk's words on their way here: they come first
  S.error = '';
  keepAwake();
  const now = phaseNow();
  if (now === 'speaking') interrupt();
  else if (now === 'working') {
    send({ type: 'cancel' });
    markCancelled();
    S.ignoreOldAt = performance.now();
  }
  const msg = { type: 'ask', text, ...chatField() };
  if (pics.length) {
    msg.pictures = pics.length;
    if (how.asText) msg.asText = true;
  }
  // Talk's words (some of them, checked and maybe fixed here): asked as the
  // spoken question they were, and answered out loud as one is. A PC from
  // before `"speak"` reads it as nothing and answers on screen.
  const said = S.said;
  if (said) msg.speak = true;
  const aloud = said && can('speak');
  if ((pics.length && !sendPictures(pics, send, sendBinary)) || !send(msg)) {
    S.error = `Lost the connection to ${pcName()}. Ask again once it's back.`;
    render();
    return;
  }
  const t = currentTurn(true, !aloud);
  t.q = text;
  t.pictures = pics.length;
  t.pics = pics.map((p) => p.url);
  S.pending = { text, pics, said };
  if (how.without) for (const p of S.attached) URL.revokeObjectURL(p.url);
  S.attached = [];
  S.cantRead = '';
  renderShots();
  el.input.value = '';
  S.typed = false;
  S.said = false;
  fitInput();
  if (aloud) {
    // The answer is said here: the sound starts within this tap (a phone
    // wants a gesture for it), and the mic goes as after a spoken question
    // (Clearer sound: the headset can switch to its music link meanwhile).
    const ctx = audioContext();
    if (ctx.state !== 'running') ctx.resume().catch(() => {});
    player.prepare();
    afterQuestionSent();
  }
  S.phase = 'thinking';
  S.step = '';
  S.workSince = performance.now();
  threadPinned = true;
  renderConvo();
  syncTicker();
  render();
}

/**
 * **▶ Read aloud** on an answer: the PC says the whole of it on the phone,
 * in Yap's voice (`read_aloud`). Whatever was going on stops first.
 */
function readAloud(t) {
  if (!S.ready || !t.answerId) return;
  keepAwake();
  S.error = '';
  S.note = '';
  const now = phaseNow();
  if (S.local !== 'idle') cancelTalk();
  else if (now === 'working') markCancelled();
  // (Words on their way to the box: the PC drops them for this, as it drops a question.)
  else if (now === 'transcribing') S.transcribing = false;
  player.stopAll();
  // The PC answers with `state idle` (the old news ends there), then the voice.
  S.ignoreOldAt = performance.now();
  if (!send({ type: 'read_aloud', id: t.answerId })) return;
  S.reading = t.answerId;
  S.speakingTurn = t.id;
  S.phase = 'idle';
  S.step = '';
  // The sound starts within this tap (a phone wants a gesture for it).
  const ctx = audioContext();
  if (ctx.state !== 'running') ctx.resume().catch(() => {});
  player.prepare();
  renderConvo();
  render();
}

// ------------------------------------------------------------- pictures

/**
 * Photos taken or chosen (+): each made smaller here (photos.js) and shown
 * as a thumbnail inside the composer card, with ✕; four at most, as Chat
 * takes them.
 */
async function addPictures(files) {
  const list = [...(files || [])].filter((f) => f && (!f.type || f.type.startsWith('image/')));
  if (!list.length) return;
  const room = MAX_PICTURES - S.attached.length - S.preparing;
  if (room <= 0) {
    S.note = `A question can carry ${MAX_PICTURES} pictures at most.`;
    render();
    return;
  }
  const take = list.slice(0, room);
  S.note = list.length > room ? `A question can carry ${MAX_PICTURES} pictures at most: the first ${room === 1 ? 'one was' : `${room} were`} added.` : '';
  S.preparing += take.length;
  S.cantRead = '';
  render();
  for (const file of take) {
    try {
      const p = await preparePicture(file);
      S.attached.push({ ...p, id: `p${++pictureIds}` });
    } catch {
      S.note = "That picture couldn't be read on this phone. Try another one.";
    } finally {
      S.preparing--;
    }
    renderShots();
    render();
  }
}
let pictureIds = 0;

/** The thumbnails in the composer card, each with ✕. */
function renderShots() {
  const frag = document.createDocumentFragment();
  for (const p of S.attached) {
    const li = document.createElement('li');
    li.className = 'shot';
    li.dataset.testid = 'shot';
    const img = new Image();
    img.alt = '';
    img.decoding = 'async';
    img.src = p.url;
    const x = document.createElement('button');
    x.type = 'button';
    x.className = 'shot-x';
    x.dataset.testid = 'shot-remove';
    x.setAttribute('aria-label', 'Remove the picture');
    x.appendChild(icon('', ['M7 7l10 10M17 7L7 17']));
    x.addEventListener('click', () => {
      S.attached = S.attached.filter((a) => a !== p);
      URL.revokeObjectURL(p.url);
      if (!S.attached.length) S.cantRead = '';
      renderShots();
      render();
    });
    li.append(img, x);
    frag.appendChild(li);
  }
  for (let i = 0; i < S.preparing; i++) {
    const li = document.createElement('li');
    li.className = 'shot loading';
    li.setAttribute('aria-label', 'Getting the picture ready');
    frag.appendChild(li);
  }
  el.shots.replaceChildren(frag);
  el.shots.hidden = !S.attached.length && !S.preparing;
}

/**
 * Chat's line for pictures its model can't read (as the desktop's
 * composer has it): the reason, or '' when there's none — known from the
 * picker's status (`images` false), or said by the PC (`cant_read`).
 */
function visionLine() {
  if (!S.attached.length) return '';
  if (S.cantRead) return S.cantRead;
  const st = S.picker && S.picker.status;
  if (st && st.ready && st.images === false) return "This model can't read images";
  return '';
}

/** The model the PC offers instead, one that reads images (`imagesSwitch`). */
function visionSwitch() {
  const st = S.picker && S.picker.status;
  const sw = st && st.imagesSwitch;
  return sw && sw.provider && sw.model ? sw : null;
}

function closePlusMenu() {
  el.plusMenu.hidden = true;
  el.plusBtn.setAttribute('aria-expanded', 'false');
}

function openSettings() {
  listMics();
  if (typeof el.settings.showModal === 'function') {
    if (!el.settings.open) el.settings.showModal();
  } else el.settings.setAttribute('open', '');
  renderSound();
}

function closeSettings() {
  if (typeof el.settings.close === 'function' && el.settings.open) el.settings.close();
  else el.settings.removeAttribute('open');
}

// ---------------------------------------------------------- the wake lock

let wakeLock = null;
async function keepAwake() {
  if (!('wakeLock' in navigator) || document.visibilityState !== 'visible') return;
  if (wakeLock && !wakeLock.released) return;
  try {
    wakeLock = await navigator.wakeLock.request('screen');
    wakeLock.addEventListener('release', () => {
      wakeLock = null;
    });
  } catch {
    /* battery saver, or not allowed: the page still works */
  }
}

// ------------------------------------------------------------------ input

let press = null;
let lastPointer = 0; // the last press or release, so its click isn't a second one

/**
 * When an input really happened: its timeStamp, so a tap stays a tap when
 * the page was busy as it came (its release handled late); now, when the
 * browser's stamp isn't on the page's clock.
 */
function eventTime(e) {
  const now = performance.now();
  const t = e && e.timeStamp;
  return typeof t === 'number' && t > 0 && t <= now + 1 && now - t < 10_000 ? t : now;
}

el.talkBtn.addEventListener('pointerdown', (e) => {
  if (e.button !== 0 || el.talkBtn.disabled) return;
  e.preventDefault();
  lastPointer = performance.now();
  try {
    el.talkBtn.setPointerCapture(e.pointerId);
  } catch {
    /* fine without */
  }
  if (busyListening()) {
    // The second tap of tap-to-talk: send (or, stuck at the permission, try afresh).
    press = { t: eventTime(e), handled: true };
    if (S.local === 'opening' && waitingPermission()) restartTalk();
    else stopTalk();
    return;
  }
  press = { t: eventTime(e), handled: false };
  S.holdRelease = false;
  // After a moment it's a hold: the label says so.
  press.timer = setTimeout(() => {
    if (press && !press.handled && busyListening()) {
      S.holdRelease = true;
      render();
    }
  }, HOLD_MS);
  beginTalk();
  // A hold counts from when the press was handled: setting the sound up can
  // keep the page busy (Firefox's new AudioContext: 0.1–0.4 s), and a finger
  // lifted meanwhile is a tap.
  press.t = Math.max(press.t, performance.now());
});

function release(e) {
  lastPointer = performance.now();
  if (!press) return;
  const p = press;
  press = null;
  clearTimeout(p.timer);
  if (p.handled) return;
  // A hold: letting go sends. A tap: Yap listens until the next tap.
  if (eventTime(e) - p.t >= HOLD_MS && busyListening()) {
    S.holdRelease = true;
    stopTalk();
  }
  S.holdRelease = false;
  render();
}
el.talkBtn.addEventListener('pointerup', release);
el.talkBtn.addEventListener('pointercancel', release);
el.talkBtn.addEventListener('contextmenu', (e) => e.preventDefault());
// The keyboard, a switch or a screen reader: a click with no press before it.
el.talkBtn.addEventListener('click', (e) => {
  if (e.detail !== 0 && performance.now() - lastPointer < 1000) return;
  if (S.local === 'opening' && waitingPermission()) restartTalk();
  else if (busyListening()) stopTalk();
  else beginTalk();
});

el.stopBtn.addEventListener('click', () => interrupt());
el.cancelBtn.addEventListener('click', () => cancelTalk());
el.retry.addEventListener('click', () => {
  S.denied = false;
  S.retries = 0;
  connect();
});
el.handsFree.addEventListener('change', () => {
  S.handsFree = el.handsFree.checked;
  save(KEY.handsFree, S.handsFree);
  render();
});
el.wait.addEventListener('change', (e) => {
  if (e.target.name !== 'wait') return;
  S.wait = waitChoice(e.target.value);
  save(KEY.wait, S.wait);
  // Listening now: the question being asked waits this long too.
  if (S.local === 'listening') vad.endFrames = Math.round(WAITS[S.wait] / FRAME_MS);
  render();
});
el.clearer.addEventListener('change', () => {
  S.clearer = el.clearer.checked;
  save(KEY.clearer, S.clearer);
  // On while Yap talks: the mic goes now, so the headset can switch.
  if (S.clearer && S.local === 'idle' && player.active) closeMic();
  render();
});
el.sound.addEventListener('toggle', renderSound);
el.mic.addEventListener('change', () => {
  const opt = el.mic.selectedOptions[0];
  save(KEY.mic, el.mic.value ? { id: el.mic.value, label: opt ? opt.textContent : '' } : null);
  // The next question opens the one picked.
  if (S.local === 'idle') closeMic();
});
el.modelBtn.addEventListener('click', () => picker.open());
el.menuBtn.addEventListener('click', () => drawer.open());
// +: Take photo (the camera) or Choose photos (the phone's pictures).
el.plusBtn.addEventListener('click', () => {
  const open = el.plusMenu.hidden;
  el.plusMenu.hidden = !open;
  el.plusBtn.setAttribute('aria-expanded', String(open));
  if (open) el.takePhoto.focus({ preventScroll: true });
});
el.takePhoto.addEventListener('click', () => {
  closePlusMenu();
  el.camera.click();
});
el.choosePhotos.addEventListener('click', () => {
  closePlusMenu();
  el.photos.click();
});
for (const input of [el.camera, el.photos]) {
  input.addEventListener('change', () => {
    const files = [...(input.files || [])];
    input.value = '';
    addPictures(files);
  });
}
document.addEventListener('click', (e) => {
  if (!el.plusMenu.hidden && !el.plusWrap.contains(e.target)) closePlusMenu();
});
el.plusMenu.addEventListener('keydown', (e) => {
  if (e.key === 'Escape') {
    closePlusMenu();
    el.plusBtn.focus();
  }
});
// Chat's line for pictures its model can't read: another model (then the
// question goes), their text instead, choose a model, or without them.
el.visionSwitch.addEventListener('click', () => {
  const sw = visionSwitch();
  if (!sw) return;
  send({ type: 'set_model', provider: sw.provider, model: sw.model, baseUrl: null, file: null });
  S.cantRead = '';
  // (The PC switches first: the socket keeps the order.)
  sendTyped({ anyway: true });
});
el.visionAsText.addEventListener('click', () => sendTyped({ asText: true }));
el.visionPick.addEventListener('click', () => picker.open());
el.visionWithout.addEventListener('click', () => sendTyped({ without: true }));
el.settingsBtn.addEventListener('click', openSettings);
el.settingsClose.addEventListener('click', closeSettings);
el.settings.addEventListener('click', (e) => {
  // A tap outside the sheet closes it.
  if (e.target === el.settings) closeSettings();
});

// The composer: Enter sends, Shift+Enter makes a new line; the box grows.
el.input.addEventListener('input', () => {
  fitInput();
  // Emptied: what's written next is typed, until Talk adds to it.
  if (el.input.value === '') S.said = false;
  const typed = el.input.value.trim() !== '';
  if (typed !== S.typed) {
    S.typed = typed;
    render();
  }
});
el.input.addEventListener('keydown', (e) => {
  if (e.key === 'Enter' && !e.shiftKey && !e.isComposing && e.keyCode !== 229) {
    e.preventDefault();
    sendTyped();
  }
});
el.composer.addEventListener('submit', (e) => {
  e.preventDefault();
  sendTyped();
});
// The keyboard coming up: the thread's end stays in view.
el.input.addEventListener('focus', () => {
  threadPinned = true;
  setTimeout(toEnd, 300);
});

if (navigator.mediaDevices && navigator.mediaDevices.addEventListener) {
  navigator.mediaDevices.addEventListener('devicechange', () => {
    listMics();
    // A headset came or went: the next question opens the mic afresh.
    if (S.local === 'idle' && phaseNow() !== 'speaking') closeMic();
  });
}

document.addEventListener('visibilitychange', () => {
  if (document.visibilityState !== 'visible') return;
  keepAwake();
  // Back from the lock screen: the sound may have been suspended.
  if (mic.ctx && mic.ctx.state !== 'running') mic.ctx.resume().catch(() => {});
  // Refused, and another tab paired the phone meanwhile: back with its device.
  if (S.comeBack) comeBack();
  else if (!S.ws) reconnectNow();
  else ping();
});
window.addEventListener('online', reconnectNow);

// Another tab of this page paired the phone (or paired it again): a tab
// refused for the device it had comes back with the new one — once, on
// purpose; now if it's in front, else when it's next shown (a phone's
// background tabs needn't all say hello at once). A tab connected,
// connecting or showing a code carries on.
window.addEventListener('storage', (e) => {
  if (e.key !== KEY.device && e.key !== null) return;
  const d = loadDevice();
  if (!d || (S.device && d.id === S.device.id && d.secret === S.device.secret)) return;
  S.device = d;
  if (S.denied && !S.ws) {
    S.comeBack = true;
    if (document.visibilityState === 'visible') comeBack();
  }
});

/** A refused tab tries again with the device another tab paired. */
function comeBack() {
  if (!S.comeBack) return;
  S.comeBack = false;
  if (!S.denied || S.ws) return;
  S.denied = false;
  S.deniedWhy = '';
  S.retries = 0;
  connect();
}

// A code opened in this very tab (the link pasted into its address bar, or
// the QR opened where the page already was): only its address's fragment
// changes, so the page isn't loaded again. A hello with it now (and the
// device: the PC counts that first).
window.addEventListener('hashchange', () => {
  const token = pairInAddress();
  if (!token || token === S.pairToken) return;
  S.pairToken = token;
  S.pairCode = '';
  S.denied = false;
  S.deniedWhy = '';
  S.retries = 0;
  // A connection open now ends as a dropped one does (anything under way
  // goes with it), then the hello goes again.
  if (S.ws) dropped(S.ws);
  else connect();
});

// ------------------------------------------------------------- the tests'

// Read-only, for e2e-link/: what the page is doing and what the player has
// queued and played. Nothing in it is a secret or changes anything.
window.__yapLink = {
  snapshot() {
    const p = player.stats;
    return {
      local: S.local,
      phase: S.phase,
      now: phaseNow(),
      // Talk's words: for the box ('words') or a question ('ask'), and on their way back.
      talk: S.talk,
      transcribing: !!S.transcribing,
      ready: S.ready,
      pc: S.pc,
      model: S.model,
      can: [...S.can],
      // The conversation: each question (typed or said) and its answer's id.
      turns: S.turns.map((t) => ({ q: t.q, typed: !!t.typed, final: !!t.final, answerId: t.answerId || '', ...(t.pictures ? { pictures: t.pictures } : {}) })),
      reading: S.reading,
      speakingTurn: S.speakingTurn,
      frames: S.frames,
      retries: S.retries,
      handsFree: S.handsFree,
      wait: S.wait,
      waitMs: WAITS[S.wait],
      clearer: S.clearer,
      wakeLock: !!wakeLock && !wakeLock.released,
      contextRate: mic.ctx ? mic.ctx.sampleRate : 0,
      contextState: mic.ctx ? mic.ctx.state : '',
      micOpen: !!mic.stream,
      // Some of the box's words came from Talk (Send asks them out loud).
      said: S.said,
      // The QR's token this tab still has (a stale one goes with the device).
      pairing: !!S.pairToken,
      // The conversation on the PC this phone is in, and its pictures waiting.
      chat: S.chat,
      title: S.title,
      attached: S.attached.map((p) => ({ w: p.w, h: p.h, bytes: p.bytes })),
      cantRead: S.cantRead,
      // Opening the mic: the step it's on (or ended on), its state and time.
      micStep: mic.run
        ? { name: mic.run.name, state: mic.run.state, ms: mic.run.ms, total: mic.run.total, waiting: mic.run.waiting }
        : null,
      player: {
        // speak_start's rate, and the page's view: an answer playing or to come.
        rate: player.rate,
        active: player.active,
        // Seconds of the answer received (at its own rate).
        queued: player.queued,
        // The worklet's last report: idle | buffering | playing | starved |
        // stopping; seconds played and waiting; gaps; how it started; the
        // largest jump between two samples played (a hard cut shows here);
        // the loudest sample played since the report before.
        state: p.state,
        played: p.played,
        buffered: p.buffered,
        underruns: p.underruns,
        startDelayMs: p.startDelayMs,
        startBufferedMs: p.startBufferedMs,
        gapMs: p.gapMs,
        maxStep: p.maxStep,
        level: player.level,
        loaded: !!player.node,
        last: player.last ? { ...player.last } : null,
      },
    };
  },
};

// ------------------------------------------------------------------ start

listMics();
keepAwake();
render();
renderConvo();
connect();
