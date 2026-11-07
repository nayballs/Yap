// **Chat's room** (2026-11-07): how Chat's width is shared between the app's
// own rail (ControlPanel's navigation, core/navRail.js), the conversation
// list, the thread and the panel beside it (a document, a picture, a page, a
// job). Pure, no Svelte state: ChatView (with chatLayout.svelte.js) asks it
// every time a width or a choice changes, and the no-app specs check it.
//
// What gives way first, as the room runs out (predictable, one rule each):
// 1. **The app's rail** narrows to its icons while a panel is open and
//    everything (the list as the person has it, a comfortable thread, the
//    panel) doesn't fit beside it expanded — the same narrow state its ☰
//    (and Ctrl+\) controls, back when the panel closes. Expanded again by
//    the person meanwhile, it stays for that panel; collapsed by them, it
//    stays collapsed.
// 2. **The list** steps aside while a panel is open and Chat (the rail
//    narrowed) is still narrower than `LIST_STAYS` — the same state the
//    list's own button (and Ctrl+B) controls, so its button shows at the
//    thread's top left ("Show your chats") and it comes back when the panel
//    closes. Shown again by the person, it stays for that panel; with no
//    room for it beside both, it floats over the thread instead of
//    squeezing it.
// 3. **The panel's width**: the remembered one, down to `MIN_WIDTH`, so the
//    thread keeps `THREAD_MIN`.
// 4. **The panel covers the conversation** when even that doesn't fit (a
//    narrow window), ✕ or "Back beside the chat" going back.
// The person's own choices (the rail collapsed, the list hidden, its width)
// are theirs: remembered, never undone by Yap. Yap's own steps land in the
// frame the panel opens in, nothing gliding (a job starting: one change).
import { RAIL_WIDE, RAIL_NARROW } from '../../core/navRail.js';

export { RAIL_WIDE, RAIL_NARROW };

/** The panel's least width (CSS px). */
export const MIN_WIDTH = 320;
/** The panel's width until the split is moved (and a double-click on it). */
export const DEFAULT_WIDTH = 460;
/**
 * The least the conversation keeps beside the panel: the composer's chips
 * and the thread's bubbles still fit. Less room than that and the panel's
 * least (a narrow window): it covers the conversation instead.
 */
export const THREAD_MIN = 340;
/** The conversation list's width until its edge is dragged (and a double-click on it). */
export const LIST_WIDTH = 224;
/** Its edge's least and most. */
export const LIST_MIN = 200;
export const LIST_MAX = 420;
/**
 * Chat at least this wide keeps its conversation list beside an open panel;
 * narrower, the list steps aside for it. (The list at its own width, a
 * comfortable 556 px thread and the panel at its default.)
 */
export const LIST_STAYS = 1240;

/**
 * The layout for Chat `chat` px wide (0: not measured yet).
 * - `rail`: the app's rail's width now (0: no rail), gliding or not — Chat
 *   and the rail measured together always add up to the same;
 * - `railCollapsed`: the person collapsed it; `railKeptOpen`: they expanded
 *   it again while this panel is open; `gliding`: it's on its way after
 *   their ☰ (stores/navRail.svelte.js `animate`);
 * - `panelOpen`, `expanded`: the panel's state (chatDoc.svelte.js);
 * - `want`: the panel's remembered width;
 * - `hidden`: the person hid the list (its button, Ctrl+B);
 * - `shownForPanel`: the person brought it back while this panel is open;
 * - `list`: the list's remembered width.
 * Returns `{ railWants, railAuto, railNarrow, width, listShown, listFloats,
 * listAuto, listWidth, listMax, narrow, cover, maxWidth, panelWidth }`:
 * whether the panel wants the rail's room, whether Yap narrows it for that
 * (and whether it's narrow), Chat's width
 * once the rail is where it's going, the list shown (beside the thread, or
 * floating over it), whether Yap has stepped it aside for the panel, the
 * list's width and the most its edge may be dragged to, the panel covering
 * the conversation, the most the split may give the panel, and its width now.
 */
export function chatRoom({
  chat = 0,
  rail = 0,
  railCollapsed = false,
  railKeptOpen = false,
  gliding = false,
  panelOpen = false,
  expanded = false,
  want = DEFAULT_WIDTH,
  hidden = false,
  shownForPanel = false,
  list = LIST_WIDTH,
} = {}) {
  const measured = chat > 0;
  const hasRail = rail > 0;
  // Chat's width with the rail expanded, and with it narrow.
  const atWide = hasRail ? chat + rail - RAIL_WIDE : chat;
  const atNarrow = hasRail ? chat + rail - RAIL_NARROW : chat;
  // 1. The rail, for an open panel: everything beside it expanded, or its
  // icons — wanted whatever the person chose (☰ pressed meanwhile keeps it
  // open for this panel), Yap's own unless they collapsed it themselves.
  const railWants = hasRail && measured && panelOpen && atWide < (hidden ? LIST_STAYS - LIST_WIDTH : LIST_STAYS);
  const railAuto = railWants && !railCollapsed && !railKeptOpen;
  const railNarrow = hasRail && (railCollapsed || railAuto);
  // Chat's width once the rail is where it's going.
  const width = hasRail ? (railNarrow ? atNarrow : atWide) : chat;
  const narrow = measured && width - THREAD_MIN < MIN_WIDTH;
  const cover = panelOpen && (narrow || expanded);
  // 2. The list. (Not while it's covered: expanding or narrowing never moves the list behind it.)
  const listAuto = panelOpen && measured && width < LIST_STAYS && !shownForPanel;
  const listShown = !hidden && !listAuto;
  // Its width: the person's, between its least and most, the thread keeping its own least.
  const listRoom = measured ? Math.max(LIST_MIN, Math.min(LIST_MAX, width - THREAD_MIN)) : LIST_MAX;
  const listWidth = Math.round(Math.min(listRoom, Math.max(LIST_MIN, Number(list) || LIST_WIDTH)));
  // Asked back with no room beside both: over the thread, never squeezing it.
  const listFloats = listShown && panelOpen && !cover && measured && width - listWidth - THREAD_MIN < MIN_WIDTH;
  const beside = listShown && !listFloats ? listWidth : 0;
  // Its edge, dragged with the panel beside: the panel keeps its least too (never floating mid-drag).
  const besidePanel = measured && panelOpen && !cover && !listFloats;
  const listMax = Math.round(Math.max(LIST_MIN, besidePanel ? Math.min(listRoom, width - THREAD_MIN - MIN_WIDTH) : listRoom));
  // 3. The panel. (Not measured yet: the remembered width as it is. While the
  // person's ☰ glides the rail narrower Chat isn't that wide yet: the room it
  // has now. Yap's own step lands at once: the room it's about to have, so
  // the panel opens at its width in that same frame.)
  const wanted = Math.max(MIN_WIDTH, Number(want) || DEFAULT_WIDTH);
  const room = gliding ? Math.min(chat, width) : width;
  const maxWidth = measured ? Math.max(MIN_WIDTH, Math.round(room - beside - THREAD_MIN)) : wanted;
  const panelWidth = Math.round(Math.min(wanted, maxWidth));
  return { railWants, railAuto, railNarrow, width, listShown, listFloats, listAuto, listWidth, listMax, narrow, cover, maxWidth, panelWidth };
}

/**
 * The list's button: what a press (or Ctrl+B) does, from the layout now.
 * Returns the next `{ hidden, shownForPanel }`: shown → hidden (the
 * person's own, remembered); hidden → shown, also over an open panel that
 * had stepped it aside.
 */
export function toggleList({ listShown, panelOpen }) {
  if (listShown) return { hidden: true, shownForPanel: false };
  return { hidden: false, shownForPanel: !!panelOpen };
}

/** The split's keys on its handle → the panel's next width (null: not the split's key). */
export function splitKeyWidth(key, { shift = false, width, maxWidth }) {
  const step = shift ? 80 : 24;
  const clamp = (w) => Math.round(Math.max(MIN_WIDTH, Math.min(w, maxWidth)));
  if (key === 'ArrowLeft') return clamp(width + step);
  if (key === 'ArrowRight') return clamp(width - step);
  if (key === 'Home') return clamp(maxWidth);
  if (key === 'End') return clamp(MIN_WIDTH);
  return null;
}

/**
 * The list's edge's keys → the list's next width (null: not its key). As
 * the split's, by where the edge goes: ← narrower, → wider (Shift:
 * further), Home the narrowest, End the widest it may be.
 */
export function listKeyWidth(key, { shift = false, width, maxWidth = LIST_MAX }) {
  const step = shift ? 80 : 24;
  const most = Math.max(LIST_MIN, Math.min(LIST_MAX, maxWidth));
  const clamp = (w) => Math.round(Math.max(LIST_MIN, Math.min(w, most)));
  if (key === 'ArrowRight') return clamp(width + step);
  if (key === 'ArrowLeft') return clamp(width - step);
  if (key === 'Home') return LIST_MIN;
  if (key === 'End') return clamp(most);
  return null;
}

/** The list's edge dragged to `x` (client px), the list's left edge at `left`: its width, held to its room. */
export function listDragWidth(x, { left, maxWidth = LIST_MAX }) {
  const most = Math.max(LIST_MIN, Math.min(LIST_MAX, maxWidth));
  return Math.round(Math.max(LIST_MIN, Math.min(x - left, most)));
}
