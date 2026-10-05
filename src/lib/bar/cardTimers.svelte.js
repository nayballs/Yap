// The timers both bar card layouts share (call them while a card component
// initialises): the fade — a timed card reports its `expireAction` when it
// runs out, paused while the pointer is on the card or the bar is hidden —
// and the clock behind a countdown ("Starting notes in 7…").
import { untrack } from 'svelte';

export function fadeTimer(getCard, getPaused, onaction) {
  // The time left; the card's own timeout is read once, at the start.
  let remaining = untrack(() => getCard().timeoutMs ?? 0);
  let armedAt = 0;
  let timer = null;
  function disarm() {
    if (timer) {
      clearTimeout(timer);
      timer = null;
      remaining = Math.max(500, remaining - (Date.now() - armedAt));
    }
  }
  $effect(() => {
    const card = getCard();
    if (!card.timeoutMs || getPaused()) {
      disarm();
      return;
    }
    armedAt = Date.now();
    timer = setTimeout(() => {
      timer = null;
      onaction(card.expireAction ?? '');
    }, remaining);
    return disarm;
  });
}

export function countdownClock(getCard) {
  const clock = $state({ now: Date.now() });
  // Its full length, for a ring (10 s from Rust, but the card may come up a
  // moment after it started).
  const total = untrack(() => {
    const c = getCard().countdown;
    return c ? Math.max(1000, c.until - Date.now()) : 0;
  });
  $effect(() => {
    if (!getCard().countdown) return;
    const tick = setInterval(() => (clock.now = Date.now()), 200);
    return () => clearInterval(tick);
  });
  return {
    get left() {
      const c = getCard().countdown;
      return c ? Math.max(0, c.until - clock.now) : 0;
    },
    get secs() {
      return Math.ceil(this.left / 1000);
    },
    /** 1 → 0 as it runs out. */
    get fraction() {
      return total ? this.left / total : 0;
    },
  };
}
