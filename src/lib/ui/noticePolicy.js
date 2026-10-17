// Yap's notice policy on the page (the Rust side is src-tauri/src/notice.rs;
// same rules, same words). Pure: the toast store (toast.svelte.js) asks it
// what to show; nothing here touches the DOM or Tauri.
//
// Two lanes, one card each (an offer that stays until answered — "Started
// by mistake?", an update — waits its turn behind the card up, gives way
// to anything as important that comes in after it, and comes back once
// that's answered):
// - **notice**: what Yap has to say by itself — a question (a call, a
//   meeting's limit), an alert, a notice, a tip. One at a time; the rest
//   wait, the most important first, then the oldest. A question pushes a
//   tip back (unless the pointer is on it).
// - **feedback**: the answer to something the person just did (an error,
//   "Copied"). Never queued behind a notice: it's what they're looking at.
//   A newer one replaces the older.
// Tips and notices wait while a dictation records or transcribes (and a
// moment after); an update also while a meeting records. A tip or notice that
// waited too long unseen is dropped (its `onExpire` runs), so nothing pops
// up minutes late. The same toast again (same kind and text) while one is
// up or waiting refreshes that one instead of stacking, and kinds with a
// cooldown don't repeat within it.

/** After a dictation ends, held toasts wait this much longer. */
export const SETTLE_MS = 1500;

const MIN = 60_000;

/**
 * Kinds: lane, priority (higher shows first), whether it yields to its
 * equals, whether it waits out a dictation / a meeting, how long it may wait
 * unseen, its cooldown, and its default time on screen (ms; 0 = until
 * answered: 3 s for a click's answer, 6 s for a notice, longer to read).
 */
export const KINDS = {
  // A question waiting for an answer, against the clock (call prompts,
  // calendar cards, the length warning, the quiet countdown, check-ins).
  prompt: { lane: 'notice', priority: 3, holdDictation: false, holdMeeting: false, maxWaitMs: null, cooldownMs: 0, duration: 0, dedupe: false },
  // A question that can wait its turn ("Started by mistake?"): gives way
  // to a call or a meeting card.
  offer: { lane: 'notice', priority: 3, yields: true, holdDictation: true, holdMeeting: false, maxWaitMs: null, cooldownMs: 0, duration: 0, dedupe: false },
  // Something went wrong by itself and the person should act (the text is
  // on the clipboard, Windows blocks the mic).
  alert: { lane: 'notice', priority: 2, holdDictation: false, holdMeeting: false, maxWaitMs: null, cooldownMs: 0, duration: 10_000, dedupe: true },
  // An update to install: waits out a dictation and a meeting (it can't
  // install during one), as long as it takes; stays until answered.
  update: { lane: 'notice', priority: 1, yields: true, holdDictation: true, holdMeeting: true, maxWaitMs: null, cooldownMs: 0, duration: 0, dedupe: false },
  // Something happened ("Stopped at 2 hours", "Updated to 0.2.0").
  info: { lane: 'notice', priority: 1, holdDictation: true, holdMeeting: false, maxWaitMs: MIN, cooldownMs: 0, duration: 6000, dedupe: true },
  // A meeting recording's own trouble (falling behind, a side gone quiet):
  // not twice in ten minutes.
  warning: { lane: 'notice', priority: 1, holdDictation: true, holdMeeting: false, maxWaitMs: MIN, cooldownMs: 10 * MIN, duration: 12_000, dedupe: true },
  // Nice to know, fine to miss: coaching hints, word suggestions, nudges.
  tip: { lane: 'notice', priority: 0, holdDictation: true, holdMeeting: false, maxWaitMs: 30_000, cooldownMs: 0, duration: 10_000, dedupe: true },
  // The answer to a click: what went wrong…
  error: { lane: 'feedback', priority: 2, holdDictation: false, holdMeeting: false, maxWaitMs: null, cooldownMs: 0, duration: 6000, dedupe: true },
  // …or that it worked, where nothing else shows it.
  done: { lane: 'feedback', priority: 1, holdDictation: false, holdMeeting: false, maxWaitMs: null, cooldownMs: 0, duration: 3000, dedupe: true },
};

/** The kind's rule (an unknown kind is a notice). */
export function rule(kind) {
  return KINDS[kind] ?? KINDS.info;
}

/** How it ranks: priority, an offer that yields just under its equals. */
export function rank(kind) {
  const r = rule(kind);
  return r.priority * 2 + (r.yields ? 0 : 1);
}

/** The kind a toast gets when its caller names none, from its variant. */
export function kindOf(variant) {
  if (variant === 'destructive') return 'error';
  if (variant === 'success') return 'done';
  return 'info';
}

/** Whether a toast of `kind` waits right now (`ctx`: { dictating, meeting }). */
export function held(kind, ctx) {
  const r = rule(kind);
  return (r.holdDictation && !!ctx.dictating) || (r.holdMeeting && !!ctx.meeting);
}

/** Waited longer than its kind allows, never seen. */
export function stale(t, now) {
  const max = rule(t.kind).maxWaitMs;
  return !t.seen && max != null && now - t.queuedAt > max;
}

/**
 * Which waiting toast shows next in `lane` (or null): not held, not stale,
 * the highest priority, then the oldest.
 */
export function next(waiting, lane, ctx, now) {
  let best = null;
  for (const t of waiting) {
    if (rule(t.kind).lane !== lane || held(t.kind, ctx) || stale(t, now)) continue;
    const better =
      !best || rank(t.kind) > rank(best.kind) || (rank(t.kind) === rank(best.kind) && t.queuedAt < best.queuedAt);
    if (better) best = t;
  }
  return best;
}

/**
 * Whether `incoming` takes the place of `shown` (same lane) at once:
 * feedback always (the newest answer wins); a notice only if it matters
 * more (or the one up is an offer that yields to its equals) and the
 * pointer isn't on the one up.
 */
export function preempts(incoming, shown) {
  if (!shown) return true;
  const r = rule(incoming.kind);
  if (r.lane === 'feedback') return true;
  // An offer waits its turn.
  if (r.yields) return false;
  const s = rule(shown.kind);
  const outranks = rank(incoming.kind) > rank(shown.kind) || (s.yields && r.priority >= s.priority);
  return outranks && !shown.hovered;
}

/** Within its cooldown: `lastShownAt` is when this key last showed. */
export function cooling(kind, lastShownAt, now) {
  const c = rule(kind).cooldownMs;
  return c > 0 && lastShownAt != null && now - lastShownAt < c;
}
