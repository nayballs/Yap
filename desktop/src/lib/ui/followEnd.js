// **Follow the live end**, as a terminal does: a Svelte action on a scroll
// box whose content grows at the bottom (a job's steps). While the person
// is at the bottom it stays pinned there, the newest line in view as the
// content grows — a ResizeObserver on the content (the box's first child)
// and the box, never a scroll per event. Scrolled up to read, it never
// moves; back at the bottom (by hand, or `jumpToEnd`), it's pinned again.
// `onchange(pinned)` says when that changes (for a "↓" button).
//
//   <div class="box" use:followEnd={{ onchange: (p) => (pinned = p) }}><ol>…</ol></div>
//
// Used by the Job tab (views/chat/JobTimeline.svelte).

/** How close to the bottom still counts as there (px). */
const NEAR = 8;

/** Box `box` scrolled to its end. */
export function jumpToEnd(box) {
  if (box) box.scrollTop = box.scrollHeight;
}

export function followEnd(box, options = {}) {
  let opts = options;
  let pinned = true;
  // Where this last put the box: a scroll event that isn't above it is this
  // one's own (content may have grown again before it was dispatched), never
  // the person scrolling up.
  let placed = 0;
  const atEnd = () => box.scrollHeight - box.scrollTop - box.clientHeight <= NEAR;
  const set = (p) => {
    if (p === pinned) return;
    pinned = p;
    opts.onchange?.(p);
  };
  const toEnd = () => {
    jumpToEnd(box);
    placed = box.scrollTop;
  };
  const onScroll = () => {
    if (pinned && box.scrollTop >= placed - 1) {
      placed = box.scrollTop;
      return;
    }
    set(atEnd());
  };
  // The content grew (or the box changed size): pinned, to the end;
  // otherwise left where the person is reading.
  const ro = new ResizeObserver(() => {
    if (pinned) toEnd();
    else set(atEnd());
  });
  let content = null;
  const watch = () => {
    const now = box.firstElementChild;
    if (now === content) return;
    if (content) ro.unobserve(content);
    content = now;
    if (content) ro.observe(content);
  };
  box.addEventListener('scroll', onScroll, { passive: true });
  ro.observe(box);
  watch();
  // The content element swapped (a list shown where there was none).
  const mo = new MutationObserver(watch);
  mo.observe(box, { childList: true });
  toEnd();
  return {
    update(next) {
      opts = next || {};
    },
    destroy() {
      box.removeEventListener('scroll', onScroll);
      ro.disconnect();
      mo.disconnect();
    },
  };
}
