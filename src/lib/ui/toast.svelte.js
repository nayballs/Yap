// Toast store — port of OpenWhispr's ui/useToast + ToastProvider timer logic,
// rendered Wispr-Flow-style (ToastHost.svelte). Usage: toast({ title,
// description?, variant?: 'default'|'success'|'destructive', duration?,
// chip?, action?: { label, onClick } }). `chip` overrides the little category
// pill (defaults: Tip / Done / Error per variant); `action` renders a light
// button bottom-right (Wispr's "Open Settings"). Destructive toasts linger
// longer (6 s vs 3.5 s) and render the description as a copyable mono error
// box. Hovering a toast pauses its timer; leaving resumes with the remaining
// time.

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
      startExit(id);
    }, ms)
  );
}

export function toast({ title = '', description = '', variant = 'default', duration, chip = '', action = null } = {}) {
  const id = ++seq;
  const dur = duration ?? (variant === 'destructive' ? 6000 : 3500);
  toastStore.list.push({
    id,
    title,
    description,
    variant,
    chip,
    action,
    duration: dur,
    createdAt: Date.now(),
    isExiting: false,
  });
  arm(id, dur);
  return id;
}

export function dismiss(id) {
  const timer = timers.get(id);
  if (timer) {
    clearTimeout(timer);
    timers.delete(id);
  }
  startExit(id);
}

export function pauseToast(id) {
  const timer = timers.get(id);
  if (timer) {
    clearTimeout(timer);
    timers.delete(id);
  }
}

export function resumeToast(id) {
  const t = toastStore.list.find((x) => x.id === id);
  if (!t || t.duration <= 0) return;
  const elapsed = Date.now() - t.createdAt;
  arm(id, Math.max(t.duration - elapsed, 500));
}
