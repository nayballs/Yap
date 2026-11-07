// **Chat's layout** (2026-11-07, after the Claude app's sidebar button and
// its split): the conversation list shown or hidden and its width, the split
// between the thread and the panel beside it, and the app's rail narrowed
// for the room. ChatView makes it; the rules are chatRoom.js's (pure).
//
// - **The list's button** ("Hide your chats · Ctrl+B" at the list's top
//   left; "Show your chats · Ctrl+B" at the thread's while it's hidden): the
//   person's own choice, remembered in this window's storage
//   (`yapChatListHidden`). Yap only steps the list aside while a panel needs
//   the room (Chat narrower than `LIST_STAYS`) — the same hidden state, its
//   button showing — and brings it back when the panel closes. Shown again
//   meanwhile, it stays for that panel (`shownForPanel`).
// - **The list's edge**: dragged (the resize cursor over the whole of Chat
//   while it's held, Esc putting it back), the arrow keys on it (Shift:
//   further; Home / End: narrowest / widest), a double-click back to 224 px;
//   between 200 and 420, the thread (and a panel beside it) keeping its
//   least; the width remembered (`yapChatListWidth`).
// - **The split**: dragged (its grip on hover, the resize cursor over the
//   whole of Chat while it's held, Esc putting it back), the arrow keys on
//   it (Shift: further; Home / End: widest / narrowest), a double-click back
//   to the default; the width remembered (chatDoc.svelte.js `keepWidth`).
// - **The app's rail** (stores/navRail.svelte.js): narrowed first while a
//   panel needs the room (`railAuto`), the same state its ☰ shows; expanded
//   again by the person, it stays for that panel (`keptOpen`).
import { chatRoom, toggleList, splitKeyWidth, listKeyWidth, listDragWidth, DEFAULT_WIDTH, LIST_WIDTH, LIST_MIN, LIST_MAX } from './chatRoom.js';
import { untrack } from 'svelte';
import { navRail, railForRoom, railRoomDone } from '../../stores/navRail.svelte.js';

const HIDDEN_KEY = 'yapChatListHidden';
const WIDTH_KEY = 'yapChatListWidth';

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

function readListWidth() {
  try {
    const w = Number(localStorage.getItem(WIDTH_KEY));
    return w >= LIST_MIN && w <= LIST_MAX ? w : LIST_WIDTH;
  } catch {
    return LIST_WIDTH;
  }
}

function writeListWidth(w) {
  try {
    if (w === LIST_WIDTH) localStorage.removeItem(WIDTH_KEY);
    else localStorage.setItem(WIDTH_KEY, String(w));
  } catch {
    /* private storage refused: the default next time */
  }
}

/** `doc`: the panel's state (chatDoc.svelte.js). Call while the component initialises. */
export function createChatLayout(doc) {
  // Chat's own width and the app's rail's, measured together (ChatView).
  let chatWidth = $state(0);
  let railWidth = $state(0);
  // The list's width as the person left it.
  let listWant = $state(readListWidth());
  let hidden = $state(readHidden());
  let shownForPanel = $state(false);
  // The split held: its width when the drag began (Esc puts it back).
  let dragFrom = $state(null);
  // The list's edge held: likewise.
  let listFrom = $state(null);

  const room = $derived(
    chatRoom({
      chat: chatWidth,
      rail: railWidth,
      railCollapsed: navRail.collapsed,
      railKeptOpen: navRail.keptOpen,
      gliding: navRail.animate,
      panelOpen: doc.open,
      expanded: doc.expanded,
      want: doc.width,
      hidden,
      shownForPanel,
      list: listWant,
    })
  );

  // Brought back for one panel: until it closes (the rail kept open likewise).
  $effect(() => {
    if (!doc.open) shownForPanel = false;
  });
  // The rail narrowed for the room while it's needed (unless the person's ☰
  // says otherwise: navRail.svelte.js), in the frame the panel opens in.
  $effect.pre(() => {
    const on = room.railWants;
    untrack(() => railForRoom(on));
  });
  $effect(() => {
    if (!doc.open) untrack(() => railRoomDone());
  });
  $effect(() => () => railRoomDone({ leaving: true }));

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

  // ---- The list's edge

  function listDragStart() {
    listFrom = room.listWidth;
  }
  /** Dragged to `clientX`, the list's left edge at `left` (client px). */
  function listDragTo(clientX, left) {
    if (listFrom == null) return;
    listWant = listDragWidth(clientX, { left, maxWidth: room.listMax });
  }
  /** Let go: kept. `cancel` (Esc): back where it was. */
  function listDragEnd(cancel = false) {
    if (listFrom == null) return;
    if (cancel) listWant = listFrom;
    listFrom = null;
    listWant = room.listWidth;
    writeListWidth(listWant);
  }
  /** A key on the edge; true when it was the edge's. */
  function listKey(e) {
    const w = listKeyWidth(e.key, { shift: e.shiftKey, width: room.listWidth, maxWidth: room.listMax });
    if (w == null) return false;
    listWant = w;
    writeListWidth(w);
    return true;
  }
  /** A double-click on the edge: back to its own width. */
  function listReset() {
    listWant = LIST_WIDTH;
    writeListWidth(LIST_WIDTH);
  }

  return {
    get chatWidth() { return chatWidth; },
    set chatWidth(v) { chatWidth = v; },
    get railWidth() { return railWidth; },
    set railWidth(v) { railWidth = v; },
    get room() { return room; },
    get hidden() { return hidden; },
    get listShown() { return room.listShown; },
    get dragging() { return dragFrom != null || listFrom != null; },
    get listDragging() { return listFrom != null; },
    toggle,
    dismissFloating,
    dragStart,
    dragTo,
    dragEnd,
    key,
    reset,
    listDragStart,
    listDragTo,
    listDragEnd,
    listKey,
    listReset,
  };
}
