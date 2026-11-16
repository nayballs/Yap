// **The time the rows under Chat's messages go by** ("12 minutes ago",
// core/whenSaid.js): now, in seconds, moved on every 30 seconds while
// anything reads it — one timer for the whole thread, none when nothing does.

let now = $state(Date.now() / 1000);
let readers = 0;
let timer = 0;

/** Now (seconds), followed while the calling component lives: read it in a `$derived`. */
export const clock = {
  get now() {
    return now;
  },
};

/** Keep the clock going while a component that shows a time is mounted (an `$effect`'s body: returns its stop). */
export function followClock() {
  readers += 1;
  if (!timer) {
    now = Date.now() / 1000;
    timer = setInterval(() => (now = Date.now() / 1000), 30_000);
  }
  return () => {
    readers -= 1;
    if (readers <= 0 && timer) {
      clearInterval(timer);
      timer = 0;
      readers = 0;
    }
  };
}
