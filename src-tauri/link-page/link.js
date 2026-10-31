// Yap Link, the phone's page: talk to Yap's AI on the PC from a phone on the
// home Wi-Fi, through a Bluetooth headset or the phone itself. Yap serves this
// page over HTTPS (a self-signed certificate, accepted once) and a WebSocket
// at /ws; the protocol is docs/architecture/link.md's (v1), and this page
// keeps to it exactly.
//
// What it does: pairing (the QR's #pair=<token>, the 4-digit code, the device
// kept in localStorage), Talk (tap or hold; hands-free ends by itself after
// the wait chosen — 1 s, 1.5 s or 2.5 s — of quiet), the mic resampled to
// 16 kHz Int16 in an AudioWorklet (mic-worklet.js, dsp.js), the spoken answer
// as one stream (resampled here frame after frame, played from a jitter
// buffer in player-worklet.js), "Clearer sound in a headset" (the mic closed
// while Yap talks), Stop and barge-in, the conversation, the step Yap is on
// and its time, the model chip, Sound details, reconnects with backoff,
// keep-alive pings and a screen wake lock.
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

const KIND_UP = 0x01;
const KIND_DOWN = 0x02;
const MAX_FRAMES = (120 * LINK_RATE) / FRAME_SAMPLES; // a question: 2 minutes at most
const HOLD_MS = 350; // a press this long is a hold: release sends
const NO_SPEECH_MS = 10000; // hands-free: nothing said this long → cancelled
const MIC_WARM_MS = 15000; // the mic kept open after a turn, for a follow-up
const MIC_LIVE_MS = 3000; // the longest a new mic's wait for sound (a headset switching to its call link)
const PING_MS = 15000;
const PONG_WAIT_MS = 8000;
const KEEP_TURNS = 6;

const KEY = {
  device: 'yap.link.device',
  pc: 'yap.link.pc',
  mic: 'yap.link.mic',
  handsFree: 'yap.link.handsFree',
  wait: 'yap.link.wait',
  clearer: 'yap.link.clearer',
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

// ------------------------------------------------------------------- state

/** One of WAITS' names: what's stored, or Normal. */
function waitChoice(w) {
  return typeof w === 'string' && Object.prototype.hasOwnProperty.call(WAITS, w) ? w : 'normal';
}

const fragment = new URLSearchParams(location.hash.slice(1));
const storedDevice = load(KEY.device);

const S = {
  ws: null,
  ready: false,
  denied: false,
  deniedWhy: '',
  pc: load(KEY.pc) || '',
  pairToken: fragment.get('pair') || '',
  pairCode: '',
  device: storedDevice && storedDevice.id && storedDevice.secret ? storedDevice : null,
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
  seq: 0,
  frames: 0,
  qStarted: 0,
  workSince: 0,
  step: '',
  turns: [],
  turnId: 0,
  model: null,
  models: [],
  error: '',
  note: '',
  handsFree: load(KEY.handsFree) === true,
  // Hands-free's wait before sending: short | normal | long (WAITS).
  wait: waitChoice(load(KEY.wait)),
  // "Clearer sound in a headset": the mic closed while Yap talks.
  clearer: load(KEY.clearer) === true,
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
  pair: $('pair'),
  pairCodeBox: $('pair-code-box'),
  pairCode: $('pair-code'),
  pairHow: $('pair-how'),
  pairWhy: $('pair-why'),
  pairWait: $('pair-wait'),
  retry: $('retry'),
  talk: $('talk'),
  convo: $('convo'),
  empty: $('empty'),
  phase: $('phase'),
  step: $('step'),
  stepLine: $('step-line'),
  stepTime: $('step-time'),
  note: $('note'),
  error: $('error'),
  meter: $('meter'),
  talkBtn: $('talk-btn'),
  talkLabel: $('talk-label'),
  talkTime: $('talk-time'),
  stopBtn: $('stop-btn'),
  cancelBtn: $('cancel-btn'),
  handsFree: $('handsfree'),
  wait: $('wait'),
  clearer: $('clearer'),
  sound: $('sound'),
  soundRate: $('sound-rate'),
  soundLatency: $('sound-latency'),
  soundStart: $('sound-start'),
  soundGaps: $('sound-gaps'),
  soundMic: $('sound-mic'),
  soundMicNow: $('sound-mic-now'),
  soundBrowser: $('sound-browser'),
  mic: $('mic'),
  micHint: $('mic-hint'),
  modelBtn: $('model'),
  modelLabel: $('model-label'),
  modelWhere: $('model-where'),
  models: $('models'),
  modelsList: $('models-list'),
};

const BARS = 28;
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
    // A code scanned again pairs afresh, in place of the device kept here.
    if (S.pairToken) hello.pair = { token: S.pairToken };
    else if (S.device) hello.device = { id: S.device.id, secret: S.device.secret };
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
  // A question can't carry on across a reconnect.
  if (S.local !== 'idle' || S.phase !== 'idle' || player.active) {
    const listening = S.local !== 'idle';
    endLocalQuestion();
    player.stopAll();
    S.phase = 'idle';
    S.step = '';
    // A question that never got heard goes; one with words stays, ended.
    const t = currentTurn(false);
    if (t && !t.q && !t.a) S.turns.pop();
    else if (t) t.final = true;
    renderConvo();
    if (wasReady) {
      S.error = listening
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
  if (ignoringOld() && ['heard', 'step', 'answer', 'speak_start', 'speak_end'].includes(m.type)) return;
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
      if (typeof m.pc === 'string' && m.pc) {
        S.pc = m.pc;
        save(KEY.pc, S.pc);
      }
      if (m.model) S.model = m.model;
      if (Array.isArray(m.models)) S.models = m.models;
      if (S.error.startsWith('Lost the connection')) S.error = '';
      break;
    case 'state':
      onState(String(m.phase || 'idle'));
      break;
    case 'heard':
      currentTurn(true).q = String(m.text ?? '');
      renderConvo();
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
        S.step = '';
      }
      renderConvo();
      break;
    }
    case 'speak_start':
      player.begin(Number(m.rate) || DOWN_RATE);
      // Clearer sound: the headset's call link let go, for its music one.
      if (S.clearer && S.local === 'idle') closeMic();
      break;
    case 'speak_end':
      player.end();
      break;
    case 'error':
      S.error = String(m.message || 'Something went wrong on the PC.');
      S.step = '';
      if (S.local !== 'idle') endLocalQuestion();
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
  } else if (phase === 'speaking') {
    S.step = '';
  } else if (phase === 'idle') {
    S.step = '';
    S.workSince = 0;
    if (player.accepting) player.end(); // no speak_end: what's queued still plays
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

function currentTurn(create) {
  let t = S.turns[S.turns.length - 1];
  if ((!t || t.final) && create) {
    t = { id: ++S.turnId, q: '', a: '', final: false };
    S.turns.push(t);
    if (S.turns.length > KEEP_TURNS) S.turns.shift();
  }
  return t;
}

/** Answers are said, not read: markdown's marks and citation numbers go. */
function plain(text) {
  return text
    .replace(/\[([^\]]+)\]\((?:[^)]+)\)/g, '$1')
    .replace(/[ \t]*\[\d+\]/g, '')
    .replace(/(\*\*|__|`)/g, '')
    .replace(/^#{1,6}\s+/gm, '')
    .replace(/[ \t]+\n/g, '\n')
    .trim();
}

function renderConvo() {
  const frag = document.createDocumentFragment();
  for (const t of S.turns) {
    const li = document.createElement('li');
    li.className = 'turn';
    li.dataset.testid = 'turn';
    const q = document.createElement('p');
    q.className = 'q';
    q.dataset.testid = 'heard';
    q.textContent = t.q || '…';
    li.appendChild(q);
    if (t.a || t.cancelled) {
      const a = document.createElement('p');
      a.className = 'a' + (t.final ? ' final' : '');
      a.dataset.testid = 'answer';
      a.textContent = t.cancelled && !t.a ? 'Cancelled' : plain(t.a);
      li.appendChild(a);
    }
    frag.appendChild(li);
  }
  el.convo.replaceChildren(frag);
  el.empty.hidden = S.turns.length > 0;
  el.convo.scrollTop = el.convo.scrollHeight;
}

// --------------------------------------------------------------- the mic

const mic = {
  ctx: null,
  stream: null,
  source: null,
  node: null,
  opening: null,
  closeTimer: 0,
  deviceId: '',
};

function audioContext() {
  if (!mic.ctx) {
    const AC = window.AudioContext || window.webkitAudioContext;
    mic.ctx = new AC({ latencyHint: 'interactive' });
  }
  return mic.ctx;
}

/** The page's AudioWorklet modules, each loaded once (again after a failure). */
const modules = {};
function worklet(ctx, file) {
  if (!ctx.audioWorklet) return Promise.reject(Object.assign(new Error('worklet'), { name: 'NoWorklet' }));
  if (!modules[file]) {
    modules[file] = ctx.audioWorklet.addModule(new URL(file, import.meta.url).href).catch((err) => {
      modules[file] = null;
      throw err;
    });
  }
  return modules[file];
}

/**
 * Resolves once the mic's track is live and sending sound: a Bluetooth
 * headset takes a moment to switch to its call link (longer after it was
 * playing music), and what it says meanwhile isn't the person. At most
 * MIC_LIVE_MS; then it goes ahead anyway.
 */
function whenLive(track) {
  if (!track || track.readyState !== 'live' || !track.muted) return Promise.resolve();
  return new Promise((resolve) => {
    const done = () => {
      clearTimeout(timer);
      track.removeEventListener('unmute', done);
      track.removeEventListener('ended', done);
      resolve();
    };
    const timer = setTimeout(done, MIC_LIVE_MS);
    track.addEventListener('unmute', done);
    track.addEventListener('ended', done);
  });
}

function chosenMic() {
  const c = load(KEY.mic);
  return c && c.id ? c : null;
}

/** Opens the mic (once at a time: a second call waits for the first). */
function openMic() {
  clearTimeout(mic.closeTimer);
  if (!mic.opening) mic.opening = doOpenMic().finally(() => (mic.opening = null));
  return mic.opening;
}

async function doOpenMic() {
  const want = chosenMic();
  if (mic.stream && mic.stream.active && mic.deviceId === (want ? want.id : '')) return;
  closeMic();
  if (!window.isSecureContext || !navigator.mediaDevices || !navigator.mediaDevices.getUserMedia) {
    throw Object.assign(new Error('insecure'), { name: 'Insecure' });
  }
  const ctx = audioContext();
  if (ctx.state !== 'running') await ctx.resume().catch(() => {});
  if (!ctx.audioWorklet) throw Object.assign(new Error('worklet'), { name: 'NoWorklet' });
  // The answer's player loads beside it, ready for the answer.
  player.prepare();
  try {
    await worklet(ctx, 'mic-worklet.js');
  } catch {
    throw Object.assign(new Error('worklet load'), { name: 'WorkletLoad' });
  }
  const audio = { echoCancellation: true, noiseSuppression: true, autoGainControl: true };
  let stream;
  try {
    stream = await navigator.mediaDevices.getUserMedia({
      audio: want ? { ...audio, deviceId: { exact: want.id } } : audio,
    });
  } catch (err) {
    if (!want || (err.name !== 'OverconstrainedError' && err.name !== 'NotFoundError')) throw err;
    // The chosen mic isn't here (the headset's off): the phone's own.
    stream = await navigator.mediaDevices.getUserMedia({ audio });
    S.note = `${want.label || 'The chosen microphone'} isn't connected, so this uses the phone's own.`;
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
      S.note = listening ? 'The microphone went away (was the headset switched off?). What it heard was sent.' : '';
      render();
    };
  }
  // Names show once the mic's allowed.
  listMics();
  // Nothing counts until the mic really sends sound (Talk says "Opening the
  // microphone…" meanwhile).
  await whenLive(track);
  if (mic.stream !== stream) throw Object.assign(new Error('closed'), { name: 'MicClosed' });
  player.noteMic();
  renderSound();
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
 * With "Clearer sound in a headset" on, the mic closes as soon as the
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
      return "Chrome isn't letting Yap use the microphone. Tap the icon beside the address, open Permissions, allow the microphone, then try again.";
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
  const named = mics.some((d) => d.label);
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
  el.micHint.textContent = named
    ? 'Pick your headset here once it’s connected.'
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
        S.note = "Sent at 2 minutes: that's the longest a question can be.";
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
  for (let i = 0; i < BARS; i++) bars[i].style.transform = `scaleY(${(0.08 + levels[i] * 0.92).toFixed(3)})`;
}

function resetMeter() {
  levels.fill(0);
  for (const b of bars) b.style.transform = 'scaleY(0.08)';
}

// --------------------------------------------------------- talking: actions

const busyListening = () => S.local === 'listening' || S.local === 'opening';

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
  // Barge-in: the answer stops at once, then Yap listens.
  if (now === 'speaking') interrupt();
  else if (now === 'working') {
    send({ type: 'cancel' });
    markCancelled();
    S.ignoreOldAt = performance.now();
  }
  S.local = 'opening';
  const gen = ++S.gen;
  render();
  try {
    await openMic();
  } catch (err) {
    if (S.local === 'opening' && gen === S.gen) {
      S.local = 'idle';
      S.error = micError(err);
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
  send({ type: 'start', mode: 'ask' });
  mic.node.port.postMessage({ cmd: 'start' });
  S.local = 'listening';
  currentTurn(true);
  renderConvo();
  syncTicker();
  render();
}

function stopTalk() {
  if (S.local === 'opening') {
    S.local = 'idle';
    if (S.holdRelease) S.note = 'The microphone took a moment to open. Hold Talk a little longer, or tap it to talk.';
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
  send({ type: 'stop' });
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
    endLocalQuestion();
    if (sent) {
      send({ type: 'cancel' });
      S.ignoreOldAt = performance.now();
    }
    const t = currentTurn(false);
    if (t && !t.q && !t.a && !t.final) S.turns.pop();
    S.phase = 'idle';
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
  if (t) t.final = true;
  armMicClose();
  render();
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

  /** Loads the worklet and makes its node, once, ahead of the first answer. */
  prepare() {
    if (this.node || this.loading || this.broken) return;
    const ctx = audioContext();
    this.loading = worklet(ctx, 'player-worklet.js')
      .then(() => {
        const node = new AudioWorkletNode(ctx, 'yap-player', { numberOfOutputs: 1, outputChannelCount: [1] });
        node.port.onmessage = (e) => this.report(e.data || {});
        node.onprocessorerror = () => this.fail();
        node.connect(ctx.destination);
        this.node = node;
        for (const [msg, transfer] of this.waiting) node.port.postMessage(msg, transfer);
        this.waiting = [];
      })
      .catch(() => this.fail())
      .finally(() => {
        this.loading = null;
      });
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

/** offline | opening | listening | working | speaking | idle */
function phaseNow() {
  if (S.local === 'opening') return 'opening';
  if (S.local === 'listening') return 'listening';
  if (S.local === 'stopping') return 'working';
  if (!S.ready) return 'offline';
  if (player.active || S.phase === 'speaking') return 'speaking';
  if (S.phase === 'listening' || S.phase === 'transcribing' || S.phase === 'thinking') return 'working';
  return 'idle';
}

const PHASE_WORDS = {
  offline: '',
  opening: 'Opening the microphone…',
  listening: 'Listening',
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
  const pairing = S.denied || (!S.device && !S.ready) || (!!S.pairToken && !S.ready);
  el.pair.hidden = !pairing;
  el.talk.hidden = pairing;
  el.conn.textContent = connLine();
  el.conn.dataset.state = S.ready ? 'on' : S.denied || (!S.device && !S.pairToken) ? 'off' : 'wait';

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

  // The model chip.
  if (S.model && S.ready) {
    el.modelBtn.hidden = false;
    el.modelLabel.textContent = S.model.label || S.model.id || 'Model';
    el.modelWhere.textContent = S.model.onThisPc ? 'On this PC' : '';
    el.modelWhere.hidden = !S.model.onThisPc;
    el.modelBtn.setAttribute(
      'aria-label',
      `Model: ${S.model.label || S.model.id}${S.model.onThisPc ? ', on this PC' : ''}. Change model`,
    );
  } else el.modelBtn.hidden = true;

  const now = phaseNow();
  document.body.dataset.phase = now;
  el.phase.textContent = PHASE_WORDS[now];

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

  // Talk.
  const listening = now === 'listening';
  el.talkBtn.disabled = !S.ready && S.local === 'idle';
  el.talkBtn.setAttribute('aria-pressed', String(listening || now === 'opening'));
  el.talkLabel.textContent =
    now === 'opening'
      ? 'Wait…'
      : listening
        ? S.holdRelease
          ? 'Release to send'
          : 'Send'
        : S.local === 'stopping'
          ? 'Sending…'
          : 'Talk';
  el.talkTime.textContent = listening ? clock(S.frames * (FRAME_SAMPLES / LINK_RATE)) : '';
  el.meter.dataset.on = String(listening);
  el.stopBtn.hidden = now !== 'speaking';
  el.cancelBtn.hidden = !(listening || now === 'opening' || now === 'working');
  el.handsFree.checked = S.handsFree;
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
  el.soundBrowser.textContent = browserLine();
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

// --------------------------------------------------------- the model list

function openModels() {
  const frag = document.createDocumentFragment();
  for (const m of S.models) {
    const li = document.createElement('li');
    const b = document.createElement('button');
    b.type = 'button';
    b.className = 'model-item';
    b.dataset.id = m.id;
    const current = S.model && S.model.id === m.id;
    if (current) b.setAttribute('aria-current', 'true');
    const name = document.createElement('span');
    name.className = 'mname';
    name.textContent = m.label || m.id;
    const where = document.createElement('span');
    where.className = 'mwhere';
    where.textContent = m.onThisPc ? 'On this PC' : 'Cloud';
    b.append(name, where);
    b.addEventListener('click', () => pickModel(m));
    li.appendChild(b);
    frag.appendChild(li);
  }
  el.modelsList.replaceChildren(frag);
  if (typeof el.models.showModal === 'function') el.models.showModal();
  else el.models.setAttribute('open', '');
}

function pickModel(m) {
  if (!S.model || S.model.id !== m.id) {
    if (send({ type: 'set_model', id: m.id })) S.model = m; // a later `ready` has the last word
  }
  closeModels();
  render();
}

function closeModels() {
  if (typeof el.models.close === 'function') el.models.close();
  else el.models.removeAttribute('open');
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
    // The second tap of tap-to-talk: send.
    press = { t: lastPointer, handled: true };
    stopTalk();
    return;
  }
  press = { t: lastPointer, handled: false };
  S.holdRelease = false;
  // After a moment it's a hold: the label says so.
  press.timer = setTimeout(() => {
    if (press && !press.handled && busyListening()) {
      S.holdRelease = true;
      render();
    }
  }, HOLD_MS);
  beginTalk();
});

function release() {
  lastPointer = performance.now();
  if (!press) return;
  const p = press;
  press = null;
  clearTimeout(p.timer);
  if (p.handled) return;
  // A hold: letting go sends. A tap: Yap listens until the next tap.
  if (performance.now() - p.t >= HOLD_MS && busyListening()) {
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
  if (busyListening()) stopTalk();
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
el.modelBtn.addEventListener('click', openModels);
el.models.addEventListener('click', (e) => {
  // A tap outside the sheet closes it.
  if (e.target === el.models) closeModels();
});
$('models-close').addEventListener('click', closeModels);

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
  if (!S.ws) reconnectNow();
  else ping();
});
window.addEventListener('online', reconnectNow);

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
      ready: S.ready,
      pc: S.pc,
      model: S.model,
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
