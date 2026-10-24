// Toast store — port of OpenWhispr's ui/useToast + ToastProvider timer logic,
// rendered Wispr-Flow-style (ToastHost.svelte), and run by Yap's notice
// policy (noticePolicy.js, the page's half of src-tauri/src/notices/notice.rs):
// one notice and one piece of feedback on screen at a time, the rest
// queued by priority; tips and notices held while a dictation runs; stale
// waits dropped; the same toast again refreshing the one up instead of
// stacking.
//
// Usage: toast({ title, description?, variant?: 'default'|'success'|
// 'destructive', kind?, key?, duration?, chip?, icon?, action?: { label,
// onClick, keepOpen? }, secondary?: { label, onClick }, tertiary?: { label,
// onClick }, progress?, busy?, expand?: { label, markdown } | { label, brief } }).
// `kind` picks the policy (noticePolicy.js KINDS: prompt | update | alert |
// info | warning | tip, or error | done for the answer to a click; by
// default destructive → error, success → done, else info) and with it the
// default time on screen (`duration`, ms; `<= 0` = sticky). `key` (default:
// kind + title + description) is what counts as "the same toast".
// `chip` overrides the little category pill (defaults: Tip / Done / Error
// per variant) and `icon: 'update'` swaps its glyph; `action` renders a
// light button bottom-right (Wispr's "Open Settings"), `secondary` a quiet
// one beside it ("Later") and `tertiary` a small text link on its own line
// under them ("Don't ask for Teams"); all close the toast unless
// `keepOpen`. `progress` (0–100) draws a determinate bar, `busy` a spinner
// in the chip, and `expand` a "What's new"-style toggle that unfolds
// markdown notes inside the card (or, with `brief`, a meeting's pre-meeting
// brief: BriefView.svelte). Destructive toasts render the description
// as a copyable mono error box. Hovering a toast pauses its timer (and keeps
// it in place); leaving resumes with the remaining time. `onClose` runs when
// the person closes the card with its ✕, `onExpire` when its timer runs out
// or it waited too long to be shown (a call prompt takes both as "Not
// now"); `icon: 'call'` gives the chip a phone, `'screen'` a monitor (the
// screen-share tip), `'timer'` a stopwatch (a meeting's length limit),
// `'alert'` a warning triangle ("Started by mistake?") and `'calendar'` a
// calendar (a meeting reminder), `'dictionary'` a book (a word to add to
// the dictionary), `'mic'` a microphone and `'clipboard'` a clipboard (a
// dictation's alerts, alerts.js), `'memory'` a bookmark (what Yap remembered,
// memory.svelte.js).
// `updateToast(id, patch)` changes a toast in place, on screen or waiting
// (the update toast goes ready → downloading → restarting without stacking
// new cards); `isToastLive(id)` is true until it's closed.
import { listen } from '@tauri-apps/api/event';
import { rule, kindOf, held, stale, next, preempts, cooling, SETTLE_MS } from './noticePolicy.js';

/** On screen (ToastHost renders `list`). */
export const toastStore = $state({ list: [] });

/** Waiting their turn. */
let waiting = [];
let seq = 0;
const timers = new Map();
/** When each key last went on screen (cooldowns). */
const lastShown = new Map();
/** What's going on: { dictating, meeting }. */
const ctx = { dictating: false, meeting: false };
let settleTimer = null;
let staleTimer = null;
let listening = false;

/** Follow the pipeline and meetings (once, as the window loads). */
function follow() {
  if (listening) return;
  listening = true;
  try {
    listen('yap-state', (e) => {
      const busy = ['recording', 'processing', 'processing-slow'].includes(e.payload);
      if (busy) {
        clearTimeout(settleTimer);
        settleTimer = null;
        if (!ctx.dictating) {
          ctx.dictating = true;
          stepBack();
        }
      } else if (ctx.dictating && !settleTimer) {
        settleTimer = setTimeout(() => {
          settleTimer = null;
          ctx.dictating = false;
          promote();
        }, SETTLE_MS);
      }
    }).catch(() => {});
    listen('yap-meeting-state', (e) => {
      const recording = !!e.payload?.recording;
      if (recording === ctx.meeting) return;
      ctx.meeting = recording;
      if (recording) stepBack();
      else promote();
    }).catch(() => {});
  } catch {
    /* outside Tauri (never in Yap): no hold-backs */
  }
}

function laneOf(t) {
  return rule(t.kind).lane;
}

function shownIn(lane) {
  return toastStore.list.find((t) => !t.isExiting && laneOf(t) === lane) ?? null;
}

function startExit(id) {
  const t = toastStore.list.find((x) => x.id === id);
  if (!t || t.isExiting) return;
  t.isExiting = true;
  clearTimer(id);
  setTimeout(() => {
    const i = toastStore.list.findIndex((x) => x.id === id);
    if (i >= 0) toastStore.list.splice(i, 1);
    promote();
  }, 200);
}

function arm(id, ms) {
  if (ms <= 0) return;
  timers.set(
    id,
    setTimeout(() => {
      timers.delete(id);
      const t = toastStore.list.find((x) => x.id === id);
      try {
        if (t && !t.isExiting) t.onExpire?.();
      } finally {
        startExit(id);
      }
    }, ms)
  );
}

function clearTimer(id) {
  const timer = timers.get(id);
  if (timer) {
    clearTimeout(timer);
    timers.delete(id);
  }
}

/** Put `t` on screen now, its timer running. (By id from here on: the
 * list holds a reactive proxy of it.) */
function show(t) {
  const ms = t.remaining ?? t.duration;
  t.seen = true;
  t.hovered = false;
  t.remaining = null;
  t.createdAt = Date.now();
  lastShown.set(t.key, t.createdAt);
  toastStore.list.push(t);
  arm(t.id, ms);
}

/** Take toast `id` off screen back into the queue, keeping its time left. */
function park(id) {
  const i = toastStore.list.findIndex((x) => x.id === id);
  if (i < 0) return;
  const t = toastStore.list[i];
  clearTimer(id);
  if (t.duration > 0) t.remaining = Math.max(1000, t.duration - (Date.now() - t.createdAt));
  toastStore.list.splice(i, 1);
  waiting.push(t);
}

/** A dictation (or a meeting) began: what should wait steps back. */
function stepBack() {
  for (const t of [...toastStore.list]) {
    if (!t.isExiting && !t.hovered && held(t.kind, ctx)) park(t.id);
  }
}

/** Drop the stale; fill each free lane with the next in line. */
function promote() {
  const now = Date.now();
  const gone = waiting.filter((t) => stale(t, now));
  if (gone.length) {
    waiting = waiting.filter((t) => !gone.includes(t));
    for (const t of gone) {
      try {
        t.onExpire?.();
      } catch {
        /* the caller's problem */
      }
    }
  }
  for (const lane of ['notice', 'feedback']) {
    if (shownIn(lane)) continue;
    const t = next(waiting, lane, ctx, now);
    if (!t) continue;
    waiting = waiting.filter((x) => x !== t);
    show(t);
  }
  // Something may go stale while it waits: look again then.
  clearTimeout(staleTimer);
  staleTimer = null;
  const due = waiting
    .map((t) => (rule(t.kind).maxWaitMs == null || t.seen ? null : t.queuedAt + rule(t.kind).maxWaitMs))
    .filter((x) => x != null);
  if (due.length) staleTimer = setTimeout(promote, Math.max(250, Math.min(...due) - now + 50));
}

follow();

export function toast({
  title = '',
  description = '',
  variant = 'default',
  kind,
  key,
  duration,
  chip = '',
  icon = '',
  action = null,
  secondary = null,
  tertiary = null,
  progress = null,
  busy = false,
  expand = null,
  onClose = null,
  onExpire = null,
} = {}) {
  kind = kind ?? kindOf(variant);
  const r = rule(kind);
  key = key ?? `${kind}|${title}|${description}`;
  const dur = duration ?? r.duration;
  const content = { title, description, variant, chip, icon, action, secondary, tertiary, progress, busy, expand, onClose, onExpire };
  const now = Date.now();

  // The same toast again: refresh the one up (or waiting) instead.
  if (r.dedupe) {
    const same = [...toastStore.list, ...waiting].find((t) => t.key === key && !t.isExiting);
    if (same) {
      Object.assign(same, content, { duration: dur });
      if (toastStore.list.some((x) => x.id === same.id) && !same.hovered) {
        clearTimer(same.id);
        same.createdAt = now;
        arm(same.id, dur);
      }
      return same.id;
    }
  }
  const id = ++seq;
  if (cooling(kind, lastShown.get(key), now)) return id;

  const t = { id, kind, key, ...content, duration: dur, createdAt: now, queuedAt: now, isExiting: false, seen: false, hovered: false, remaining: null };
  const lane = r.lane;
  const up = shownIn(lane);
  if (!held(kind, ctx) && (!up || preempts(t, up))) {
    if (up) {
      if (lane === 'feedback') startExit(up.id);
      else park(up.id);
    }
    show(t);
  } else {
    waiting.push(t);
    promote();
  }
  return id;
}

/** Patch a toast in place, on screen or waiting. False once it's closed or expiring. */
export function updateToast(id, patch) {
  const t = toastStore.list.find((x) => x.id === id) ?? waiting.find((x) => x.id === id);
  if (!t || t.isExiting) return false;
  Object.assign(t, patch);
  if ('duration' in patch && toastStore.list.some((x) => x.id === id)) {
    clearTimer(id);
    t.createdAt = Date.now();
    arm(id, t.duration);
  }
  return true;
}

/** Whether a toast is still on screen or waiting its turn (not closed). */
export function isToastLive(id) {
  return toastStore.list.some((x) => x.id === id && !x.isExiting) || waiting.some((x) => x.id === id);
}

export function dismiss(id) {
  const queued = waiting.findIndex((x) => x.id === id);
  if (queued >= 0) {
    waiting.splice(queued, 1);
    return;
  }
  clearTimer(id);
  startExit(id);
}

export function pauseToast(id) {
  const t = toastStore.list.find((x) => x.id === id);
  if (t) t.hovered = true;
  clearTimer(id);
}

export function resumeToast(id) {
  const t = toastStore.list.find((x) => x.id === id);
  if (!t || t.isExiting) return;
  t.hovered = false;
  // Held back while the pointer kept it here (a dictation started)?
  if (held(t.kind, ctx)) {
    park(id);
    promote();
    return;
  }
  if (t.duration <= 0) return;
  const elapsed = Date.now() - t.createdAt;
  arm(id, Math.max(t.duration - elapsed, 500));
}
