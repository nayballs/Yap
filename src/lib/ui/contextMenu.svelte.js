// Yap's right-click menus, the same everywhere: one menu (ui/ContextMenu)
// and one confirm dialog (ui/ConfirmDialog) per window, both drawn by
// ui/ContextMenuHost.svelte (App.svelte mounts it in the main window,
// onboarding and the meeting notepad).
//
// An item with actions — a note, a conversation, a dictation, a meeting, a
// dictionary row, a model… — opens its menu three ways, all through
// `openContextMenu`: a right-click (at the pointer), its hover ⋯ button
// (`data-more`: under the button), and the keyboard on the focused item (the
// ContextMenu key, Shift+F10: `menuKey`). The menu holds the item's own
// actions, in one order: the main action first, Copy-type ones next, the
// destructive one last, in red, behind `askConfirm`. The event is cancelled,
// so WebView2's own menu never comes up there (context_menu.rs cuts that one
// down to the edit commands anyway: nothing on plain UI).
//
// Left alone, so the native menu shows: a right-click in a text field (its
// edit menu), on selected text (Copy), and — in a dev build only — a
// Shift+right-click (context_menu.rs then offers Inspect). `import.meta.env.DEV`
// is false in `vite build`, so release builds never let it through.

/** The window's menu: `ui/ContextMenu.svelte`'s props. */
export const contextMenu = $state({
  open: false,
  x: 0,
  y: 0,
  items: [],
  label: '',
  keyboard: false,
  alignEnd: false,
  returnFocus: null,
});

/** The window's confirm dialog: `ui/ConfirmDialog.svelte`'s props + what Confirm runs. */
export const confirmDialog = $state({
  open: false,
  title: '',
  body: '',
  confirmLabel: 'Delete',
  busyLabel: 'Deleting…',
  danger: true,
  busy: false,
  returnFocus: null,
  run: null,
});

/** A dev build's Shift+right-click: the developer's way to Inspect (context_menu.rs). */
export function devInspect(e) {
  return import.meta.env.DEV && e?.type === 'contextmenu' && !!e.shiftKey;
}

const EDITABLE = 'input, textarea, select, [contenteditable=""], [contenteditable="true"]';

/** A right-click the native menu should have: a text field, or selected text under the pointer. */
function nativeTarget(e) {
  if (e?.type !== 'contextmenu') return false;
  const t = e.target;
  if (t?.closest?.(EDITABLE) && t.closest(EDITABLE) !== e.currentTarget) return true;
  const sel = window.getSelection?.();
  if (sel && !sel.isCollapsed && String(sel).trim() && t) {
    for (let i = 0; i < sel.rangeCount; i++) {
      if (sel.getRangeAt(i).intersectsNode(t)) return true;
    }
  }
  return false;
}

/** Items as shown: no empty ones, no separator first, last or twice. */
function tidy(items) {
  const out = [];
  for (const it of items || []) {
    if (!it) continue;
    if (it.separator) {
      if (out.length && !out[out.length - 1].separator) out.push(it);
      continue;
    }
    out.push(it);
  }
  while (out.length && out[out.length - 1].separator) out.pop();
  return out;
}

/**
 * Open an item's menu from `e`: a right-click (at the pointer), a click on
 * its ⋯ button (`data-more`: under the button) or a key on the focused item
 * (at the item). `row` gets the focus back when the menu closes. Returns
 * whether it opened; with nothing to offer (or a right-click that's the
 * native menu's) it leaves the event alone.
 */
export function openContextMenu(e, items, { label = 'Menu', row = null } = {}) {
  if (devInspect(e) || nativeTarget(e)) return false;
  const list = tidy(items);
  if (!list.some((it) => !it.separator && !it.note)) return false;
  // (A `contextmenu` the key may send after its keydown goes to the open
  // menu, which has the focus by then, and cancels it.)
  e.preventDefault();
  e.stopPropagation();
  const anchor = e.currentTarget;
  const fromButton = anchor?.dataset?.more != null;
  const byKey =
    e.type === 'keydown' || (e.type === 'contextmenu' && e.button !== 2) || (e.type === 'click' && e.detail === 0);
  let x = e.clientX;
  let y = e.clientY;
  let alignEnd = false;
  const at = fromButton ? anchor : (row ?? anchor);
  if ((fromButton || byKey) && at?.getBoundingClientRect) {
    const r = at.getBoundingClientRect();
    x = fromButton ? r.right : r.left + 12;
    y = r.bottom + 4;
    alignEnd = fromButton;
  }
  Object.assign(contextMenu, {
    open: true,
    x,
    y,
    items: list,
    label,
    keyboard: byKey,
    alignEnd,
    returnFocus: row ?? anchor ?? null,
  });
  return true;
}

/** The keyboard's way in, on a focused item: the ContextMenu key or Shift+F10. */
export function menuKey(e, open) {
  // In a text field inside the item, the key is the field's (its edit menu).
  if (e.target !== e.currentTarget && e.target?.closest?.(EDITABLE)) return false;
  if (e.key === 'ContextMenu' || (e.key === 'F10' && e.shiftKey)) {
    open(e);
    return true;
  }
  return false;
}

/** This item's menu is open (its row keeps its hover look and ⋯ meanwhile). */
export function menuOpenFor(el) {
  return contextMenu.open && !!el && contextMenu.returnFocus === el;
}

/**
 * Ask before something that can't be undone ("Delete “Launch plan”?"). `run`
 * does it: return `false` (or throw) to keep the dialog open — say why in a
 * toast — anything else closes it. `returnFocus` gets the focus back on
 * Cancel; after a delete the item is gone, so `after` (if any) says where
 * the focus goes.
 */
export function askConfirm({
  title,
  body = '',
  confirmLabel = 'Delete',
  busyLabel = 'Deleting…',
  danger = true,
  returnFocus = null,
  run,
  after = null,
}) {
  Object.assign(confirmDialog, {
    open: true,
    title,
    body,
    confirmLabel,
    busyLabel,
    danger,
    busy: false,
    returnFocus,
    run: async () => {
      const ok = await run?.();
      if (ok !== false) setTimeout(() => after?.()?.focus?.({ preventScroll: true }), 0);
      return ok;
    },
  });
}

/** Confirm pressed: run it; done → the dialog goes (the item it asked about is gone, so no focus back). */
export async function runConfirm() {
  if (!confirmDialog.run || confirmDialog.busy) return;
  confirmDialog.busy = true;
  let ok = false;
  try {
    ok = (await confirmDialog.run()) !== false;
  } catch {
    ok = false;
  }
  confirmDialog.busy = false;
  if (ok) Object.assign(confirmDialog, { open: false, returnFocus: null, run: null });
}

/** A menu or the confirm dialog is up (a window's own Esc handling waits for them). */
export function menuOrDialogOpen() {
  return contextMenu.open || confirmDialog.open;
}
