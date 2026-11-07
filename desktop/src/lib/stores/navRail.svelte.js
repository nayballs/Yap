// The main window's rail (core/navRail.js has its rules): collapsed or
// expanded, in every view.
//
// - `collapsed`: the person's own choice (its ☰, Ctrl+\), remembered in this
//   window's storage (`yapNavCollapsed`); expanded on a first run.
// - `auto`: a view wants its room right now — Chat, while its panel is open
//   and everything doesn't fit beside the rail expanded (chatRoom.js
//   `railWants`) — and so it's narrow: the same narrow state the ☰ shows,
//   never remembered, back as the panel closes.
// - `keptOpen`: the person expanded it meanwhile (☰ while `auto`, from
//   Yap's narrowing or their own): it stays open for as long as that lasts.
// - `animate`: the latest change is the person's, so it glides (one short
//   transition; never with reduced motion). A change Yap makes for the room
//   lands in the same frame as the panel that needs it, nothing gliding.
import { RAIL_KEY, railToggle } from '../core/navRail.js';

function readCollapsed() {
  try {
    return localStorage.getItem(RAIL_KEY) === '1';
  } catch {
    return false;
  }
}

function writeCollapsed(on) {
  try {
    if (on) localStorage.setItem(RAIL_KEY, '1');
    else localStorage.removeItem(RAIL_KEY);
  } catch {
    /* private storage refused: expanded next time */
  }
}

export const navRail = $state({ collapsed: readCollapsed(), auto: false, keptOpen: false, animate: false });

/** Narrow now: the person's, or a view's for the room. */
export function railIsNarrow() {
  return navRail.collapsed || (navRail.auto && !navRail.keptOpen);
}

let settle = null;

/** The ☰ and Ctrl+\. */
export function toggleRail() {
  const next = railToggle({ narrow: railIsNarrow(), auto: navRail.auto });
  if (next.collapsed !== navRail.collapsed) writeCollapsed(next.collapsed);
  navRail.animate = true;
  clearTimeout(settle);
  // (Past the glide's 180 ms — its end, `glided`, or this, with no glide
  // under reduced motion: a change Yap makes later lands at once.)
  settle = setTimeout(glided, 320);
  navRail.collapsed = next.collapsed;
  navRail.keptOpen = next.keptOpen;
}

/** The glide's over (the rail's `transitionend`). */
export function glided() {
  clearTimeout(settle);
  if (navRail.animate) navRail.animate = false;
}

/** A view asks for the room (or gives it back: `on` false). */
export function railForRoom(on) {
  if (navRail.auto !== !!on) navRail.auto = !!on;
}

/** The view that asked has gone (or its panel closed): nothing narrowed or kept open for it. */
export function railRoomDone({ leaving = false } = {}) {
  if (leaving && navRail.auto) navRail.auto = false;
  if (navRail.keptOpen) navRail.keptOpen = false;
}
