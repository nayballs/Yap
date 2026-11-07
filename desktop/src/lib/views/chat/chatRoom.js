// **Chat's room** (2026-11-07): how Chat's width is shared between the
// conversation list, the thread and the panel beside it (a document, a
// picture, a page, a job). Pure, no Svelte state: ChatView (with
// chatLayout.svelte.js) asks it every time a width or a choice changes, and
// the no-app spec checks it.
//
// What gives way first, as the room runs out (predictable, one rule each):
// 1. **The list** steps aside while a panel is open and Chat is narrower
//    than `LIST_STAYS` — the same state the list's own button (and Ctrl+B)
//    controls, so its button shows at the thread's top left ("Show your
//    chats") and it comes back when the panel closes. Shown again by the
//    person, it stays for that panel; with no room for it beside both, it
//    floats over the thread instead of squeezing it.
// 2. **The panel's width**: the remembered one, down to `MIN_WIDTH`, so the
//    thread keeps `THREAD_MIN`.
// 3. **The panel covers the conversation** when even that doesn't fit (a
//    narrow window), ✕ or "Back beside the chat" going back.
// The person's own "hide the list" is theirs: remembered, never undone by Yap.

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
/** The conversation list's width. */
export const LIST_WIDTH = 224;
/**
 * Chat at least this wide keeps its conversation list beside an open panel;
 * narrower, the list steps aside for it.
 */
export const LIST_STAYS = 1240;

/**
 * The layout for Chat `chat` px wide (0: not measured yet).
 * - `panelOpen`, `expanded`: the panel's state (chatDoc.svelte.js);
 * - `want`: the panel's remembered width;
 * - `hidden`: the person hid the list (its button, Ctrl+B);
 * - `shownForPanel`: the person brought it back while this panel is open;
 * - `list`: the list's width.
 * Returns `{ listShown, listFloats, listAuto, narrow, cover, maxWidth, panelWidth }`:
 * the list shown (beside the thread, or floating over it), whether Yap has
 * stepped it aside for the panel, the panel covering the conversation, the
 * most the split may give the panel, and its width now.
 */
export function chatRoom({ chat = 0, panelOpen = false, expanded = false, want = DEFAULT_WIDTH, hidden = false, shownForPanel = false, list = LIST_WIDTH } = {}) {
  const narrow = chat > 0 && chat - THREAD_MIN < MIN_WIDTH;
  const cover = panelOpen && (narrow || expanded);
  // (Not while it's covered: expanding or narrowing never moves the list behind it.)
  const listAuto = panelOpen && chat > 0 && chat < LIST_STAYS && !shownForPanel;
  const listShown = !hidden && !listAuto;
  // Asked back with no room beside both: over the thread, never squeezing it.
  const listFloats = listShown && panelOpen && !cover && chat > 0 && chat - list - THREAD_MIN < MIN_WIDTH;
  const beside = listShown && !listFloats ? list : 0;
  const wanted = Math.max(MIN_WIDTH, Number(want) || DEFAULT_WIDTH);
  // (Not measured yet: the remembered width as it is.)
  const maxWidth = chat > 0 ? Math.max(MIN_WIDTH, Math.round(chat - beside - THREAD_MIN)) : wanted;
  const panelWidth = Math.round(Math.min(wanted, maxWidth));
  return { listShown, listFloats, listAuto, narrow, cover, maxWidth, panelWidth };
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
