// **Chat's layout** (2026-11-07, after the Claude app's sidebar button and
// its split): the conversation list shown or hidden, and the split between
// the thread and the panel beside it. ChatView makes it; the rules are
// chatRoom.js's (pure).
//
// - **The list's button** ("Hide your chats · Ctrl+B" at the list's top
//   left; "Show your chats · Ctrl+B" at the thread's while it's hidden): the
//   person's own choice, remembered in this window's storage
//   (`yapChatListHidden`). Yap only steps the list aside while a panel needs
//   the room (Chat narrower than `LIST_STAYS`) — the same hidden state, its
//   button showing — and brings it back when the panel closes. Shown again
//   meanwhile, it stays for that panel (`shownForPanel`).
// - **The split**: dragged (its grip on hover, the resize cursor over the
//   whole of Chat while it's held, Esc putting it back), the arrow keys on
//   it (Shift: further; Home / End: widest / narrowest), a double-click back
//   to the default; the width remembered (chatDoc.svelte.js `keepWidth`).
import { chatRoom, toggleList, splitKeyWidth, DEFAULT_WIDTH, LIST_WIDTH } from './chatRoom.js';

const HIDDEN_KEY = 'yapChatListHidden';

function readHidden() {
  try {
    return localStorage.getItem(HIDDEN_KEY) === '1';
  } catch {
    return false;
  }
}

function writeHidden(on) {
  try {
    if (on) localStorage.setItem(HIDDEN_KEY, '1');
    else localStorage.removeItem(HIDDEN_KEY);
  } catch {
    /* private storage refused: shown next time */
  }
}

/** `doc`: the panel's state (chatDoc.svelte.js). Call while the component initialises. */
export function createChatLayout(doc) {
  // Chat's own width (ChatView measures it), and the list's.
  let chatWidth = $state(0);
  let listWidth = $state(LIST_WIDTH);
  let hidden = $state(readHidden());
  let shownForPanel = $state(false);
  // The split held: its width when the drag began (Esc puts it back).
  let dragFrom = $state(null);

  const room = $derived(
    chatRoom({ chat: chatWidth, panelOpen: doc.open, expanded: doc.expanded, want: doc.width, hidden, shownForPanel, list: listWidth })
  );

  // Brought back for one panel: until it closes.
  $effect(() => {
    if (!doc.open) shownForPanel = false;
  });

  /** The list's button and Ctrl+B. */
  function toggle() {
    const next = toggleList({ listShown: room.listShown, panelOpen: doc.open });
    if (next.hidden !== hidden) writeHidden(next.hidden);
    hidden = next.hidden;
    shownForPanel = next.shownForPanel;
  }

  /** The list floating over the thread (no room beside it) goes back aside: a conversation picked, Esc, a click in the thread. */
  function dismissFloating() {
    if (room.listFloats) shownForPanel = false;
  }

  // ---- The split

  /** Its handle pressed: `right` is Chat's right edge (client px). */
  function dragStart() {
    dragFrom = room.panelWidth;
  }
  function dragTo(clientX, right) {
    if (dragFrom == null) return;
    doc.setWidth(right - clientX, room.maxWidth);
  }
  /** Let go: kept. `cancel` (Esc): back where it was. */
  function dragEnd(cancel = false) {
    if (dragFrom == null) return;
    if (cancel) doc.setWidth(dragFrom, room.maxWidth);
    dragFrom = null;
    doc.keepWidth();
  }
  /** A key on the handle; true when it was the split's. */
  function key(e) {
    const w = splitKeyWidth(e.key, { shift: e.shiftKey, width: room.panelWidth, maxWidth: room.maxWidth });
    if (w == null) return false;
    doc.setWidth(w, room.maxWidth);
    doc.keepWidth();
    return true;
  }
  /** A double-click on the handle: back to the default width. */
  function reset() {
    doc.setWidth(DEFAULT_WIDTH, room.maxWidth);
    doc.keepWidth();
  }

  return {
    get chatWidth() { return chatWidth; },
    set chatWidth(v) { chatWidth = v; },
    get listWidth() { return listWidth; },
    set listWidth(v) { listWidth = v; },
    get room() { return room; },
    get hidden() { return hidden; },
    get listShown() { return room.listShown; },
    get dragging() { return dragFrom != null; },
    toggle,
    dismissFloating,
    dragStart,
    dragTo,
    dragEnd,
    key,
    reset,
  };
}
