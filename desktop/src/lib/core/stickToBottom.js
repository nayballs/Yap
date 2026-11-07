// A thread that keeps its newest line in view while it grows, as Claude's
// app, ChatGPT and Slack do (Chat's thread, 2026-11-07: "I had to drag it
// down when it reported back to me"). Any thread that streams can use it.
//
// - **Pinned** while the person is at the bottom: whatever makes the thread
//   taller — streamed text, the checked answer's glide, a card, a pill or a
//   picture that loads later, a report read in, the panel opening beside it
//   and the lines wrapping — keeps it there. A ResizeObserver on the
//   content (and on the scroller, whose height changes with the box under
//   it) sees every one of them: nothing has to call it.
// - **Never yanked.** The person's own scroll up — the wheel, a drag on the
//   scrollbar, the keys, a jump to a search hit — lets go at once, and the
//   thread stays where they put it until they scroll back down to the bottom
//   or `toBottom()` is asked. While the mouse button is held in the thread
//   (a drag on the scrollbar, a selection) it never moves by itself.
// - A thread that gets shorter (a shorter checked answer in the streamed
//   one's place) stays pinned: the browser putting it at its new bottom is
//   no scroll up.
// - `toBottom({ smooth })`: back to the bottom, pinned again — the person's
//   own question, a conversation opened, the "↓" button (smooth unless
//   motion is reduced: core/appearance.js).
//
// No Svelte state of its own: `onchange({ pinned })` says when it changes.
import { motionReduced } from './appearance.js';

/** How near the bottom counts as there (CSS px): scrolling back down to it pins again. */
export const NEAR_BOTTOM_PX = 32;

/** Keys that scroll a thread up (never in a text box). */
const UP_KEYS = new Set(['ArrowUp', 'PageUp', 'Home']);

// What ends a press in the thread: the button up (anywhere), or the window
// losing it; a move with no button down covers a release the scrollbar kept.
const RELEASES = ['pointerup', 'pointercancel', 'mouseup', 'blur', 'pointermove'];

/**
 * Keep `scroller` (the element that scrolls) at its bottom while `content`
 * (what's in it, the part that grows) changes. `onchange({ pinned })` hears
 * each change; `near`: how near the bottom counts as there. Returns
 * `{ pinned, toBottom({ smooth }), stick(), destroy() }`.
 */
export function followBottom(scroller, content, { onchange = () => {}, near = NEAR_BOTTOM_PX } = {}) {
  let pinned = true;
  // Where it was last seen: a scroll event that went up from here is the person's.
  let lastTop = scroller.scrollTop;
  // The mouse button is held in the thread (the scrollbar, a selection).
  let holding = false;
  let frame = 0;

  const max = () => Math.max(0, scroller.scrollHeight - scroller.clientHeight);
  const gap = () => max() - scroller.scrollTop;
  const scrolls = () => scroller.scrollHeight - scroller.clientHeight > 1;

  // The browser's own scroll anchoring only while you read further up (it
  // keeps your place as things above load); pinned, the bottom is ours alone,
  // so no anchor's nudge up ever reads as a scroll of yours.
  const anchor = () => (scroller.style.overflowAnchor = pinned ? 'none' : '');
  anchor();

  function set(next) {
    if (next === pinned) return;
    pinned = next;
    anchor();
    onchange({ pinned });
  }

  function jump() {
    const top = max();
    if (Math.abs(scroller.scrollTop - top) > 0.5) scroller.scrollTop = top;
    lastTop = scroller.scrollTop;
  }

  /** At the bottom again, if pinned (never while the mouse is held in the thread). */
  function stick() {
    if (pinned && !holding) jump();
  }

  /** Back to the bottom, pinned again. */
  function toBottom({ smooth = false } = {}) {
    set(true);
    cancelAnimationFrame(frame);
    if (smooth && !motionReduced() && typeof scroller.scrollTo === 'function') {
      // (Its scroll events go down: it stays pinned all the way.)
      scroller.scrollTo({ top: max(), behavior: 'smooth' });
      return;
    }
    jump();
    // What's being drawn now (a conversation just opened) lands next frame.
    frame = requestAnimationFrame(() => {
      if (pinned) jump();
    });
  }

  function onScroll() {
    const top = scroller.scrollTop;
    const up = top < lastTop - 0.5;
    lastTop = top;
    const g = gap();
    // At the very bottom (or nothing to scroll): pinned — a shorter thread
    // put back at its bottom lands here, never as a scroll up.
    if (!scrolls() || g <= 1) set(true);
    else if (up) set(false);
    // Scrolled back down to it.
    else if (g <= near) set(true);
  }

  // A scroll box inside the thread (a long code block, a table) that takes the wheel itself.
  function innerTakes(target) {
    for (let el = target instanceof Element ? target : null; el && el !== scroller; el = el.parentElement) {
      const oy = getComputedStyle(el).overflowY;
      if ((oy === 'auto' || oy === 'scroll') && el.scrollHeight > el.clientHeight + 1 && el.scrollTop > 0) return true;
    }
    return false;
  }

  // The wheel up lets go before the thread even moves (a smooth wheel's
  // first frame may come after the next stick).
  function onWheel(e) {
    if (e.deltaY >= 0 || e.ctrlKey || !scrolls() || innerTakes(e.target)) return;
    set(false);
  }

  function onKeydown(e) {
    if (!UP_KEYS.has(e.key) || e.altKey || e.ctrlKey || e.metaKey || !scrolls()) return;
    if (e.target instanceof Element && e.target.closest('input, textarea, select, [contenteditable=""], [contenteditable="true"]')) return;
    set(false);
  }

  function onPointerDown(e) {
    if (e.button !== 0 || holding) return;
    holding = true;
    for (const t of RELEASES) window.addEventListener(t, release, true);
  }
  function release(e) {
    // (A move with the button still down is the drag going on.)
    if (e?.type === 'pointermove' && (e.buttons & 1) === 1) return;
    holding = false;
    for (const t of RELEASES) window.removeEventListener(t, release, true);
    stick();
  }

  const ro = new ResizeObserver(() => {
    // A scroll up not heard yet (a jump to a search hit, the keys): it lets go first.
    if (pinned && scroller.scrollTop < lastTop - 0.5 && gap() > 1) {
      lastTop = scroller.scrollTop;
      set(false);
      return;
    }
    if (!scrolls()) {
      lastTop = scroller.scrollTop;
      set(true);
      return;
    }
    stick();
  });
  ro.observe(content);
  ro.observe(scroller);
  scroller.addEventListener('scroll', onScroll, { passive: true });
  scroller.addEventListener('wheel', onWheel, { passive: true });
  scroller.addEventListener('keydown', onKeydown);
  scroller.addEventListener('pointerdown', onPointerDown);

  return {
    get pinned() {
      return pinned;
    },
    toBottom,
    stick,
    destroy() {
      ro.disconnect();
      cancelAnimationFrame(frame);
      scroller.removeEventListener('scroll', onScroll);
      scroller.removeEventListener('wheel', onWheel);
      scroller.removeEventListener('keydown', onKeydown);
      scroller.removeEventListener('pointerdown', onPointerDown);
      for (const t of RELEASES) window.removeEventListener(t, release, true);
      scroller.style.overflowAnchor = '';
    },
  };
}
