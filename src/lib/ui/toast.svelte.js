// Toast store — port of OpenWhispr's ui/useToast + ToastProvider timer logic,
// rendered Wispr-Flow-style (ToastHost.svelte). Usage: toast({ title,
// description?, variant?: 'default'|'success'|'destructive', duration?,
// chip?, icon?, action?: { label, onClick, keepOpen? }, secondary?: { label,
// onClick }, tertiary?: { label, onClick }, progress?, busy?, expand?:
// { label, markdown } }). `chip` overrides the little category pill
// (defaults: Tip / Done / Error per variant) and `icon: 'update'` swaps its
// glyph; `action` renders a light button bottom-right (Wispr's "Open
// Settings"), `secondary` a quiet one beside it ("Later") and `tertiary` a
// small text link on its own line under them ("Don't ask for Teams"); all
// close the toast unless `keepOpen`. `progress` (0–100) draws a determinate
// bar, `busy` a spinner in the chip, and `expand` a "What's new"-style toggle
// that unfolds markdown notes inside the card. Destructive toasts linger
// longer (6 s vs 3.5 s) and render the description as a copyable mono error
// box; `duration <= 0` = sticky. Hovering a toast pauses its timer; leaving
// resumes with the remaining time. `onClose` runs when the person closes the
// card with its ✕, `onExpire` when its timer runs out (a call prompt takes
// both as "Not now"); `icon: 'call'` gives the chip a phone, `'screen'` a
// monitor (the screen-share tip), `'timer'` a stopwatch (a meeting's
// length limit) and `'alert'` a warning triangle ("Started by mistake?").
// `updateToast(id, patch)` changes a live toast in place (the update toast
// goes ready → downloading → restarting without stacking new cards).

export const toastStore = $state({ list: [] });

let seq = 0;
const timers = new Map();

function startExit(id) {
  const t = toastStore.list.find((x) => x.id === id);
  if (!t || t.isExiting) return;
  t.isExiting = true;
  setTimeout(() => {
    const i = toastStore.list.findIndex((x) => x.id === id);
    if (i >= 0) toastStore.list.splice(i, 1);
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

export function toast({
  title = '',
  description = '',
  variant = 'default',
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
  const id = ++seq;
  const dur = duration ?? (variant === 'destructive' ? 6000 : 3500);
  toastStore.list.push({
    id,
    title,
    description,
    variant,
    chip,
    icon,
    action,
    secondary,
    tertiary,
    progress,
    busy,
    expand,
    onClose,
    onExpire,
    duration: dur,
    createdAt: Date.now(),
    isExiting: false,
  });
  arm(id, dur);
  return id;
}

/** Patch a live toast in place. Returns false once it's closed or expiring. */
export function updateToast(id, patch) {
  const t = toastStore.list.find((x) => x.id === id);
  if (!t || t.isExiting) return false;
  Object.assign(t, patch);
  if ('duration' in patch) {
    clearTimer(id);
    t.createdAt = Date.now();
    arm(id, t.duration);
  }
  return true;
}

/** Whether a toast is still on screen (not closed, not on its way out). */
export function isToastLive(id) {
  return toastStore.list.some((x) => x.id === id && !x.isExiting);
}

export function dismiss(id) {
  clearTimer(id);
  startExit(id);
}

export function pauseToast(id) {
  clearTimer(id);
}

export function resumeToast(id) {
  const t = toastStore.list.find((x) => x.id === id);
  if (!t || t.duration <= 0) return;
  const elapsed = Date.now() - t.createdAt;
  arm(id, Math.max(t.duration - elapsed, 500));
}
