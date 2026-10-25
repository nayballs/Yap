<script>
  // Yap's right-click menu, in every window (one per window, drawn by
  // ContextMenuHost; views open it through contextMenu.svelte.js's
  // `openContextMenu`). Opens at the pointer — or under a row's ⋯ button, or
  // from the keyboard (the ContextMenu key, Shift+F10) — clamped to the
  // window. Arrow keys, Home/End and Enter work; → opens a submenu, ← or Esc
  // goes back; Esc, Tab, a click outside, a resize or a scroll close it, and
  // the focus goes back to `returnFocus` (the row) when it was in the menu.
  // `openContextMenu` cancels the `contextmenu` event first, so WebView2's
  // own menu never comes up there.
  //
  // `items`: { label, onselect, icon?, disabled?, danger?, checked?, toggle?
  // (a checkbox item rather than a choice), items? (a submenu) } |
  // { separator: true } | { note } (a quiet line, e.g. why the items above
  // are off). Icons: pencil, trash, folder, copy, download, check, open,
  // play, stop, sparkle, join, record, person, sync, unlink, signout,
  // star, approx, use, archive, restore, undo, split, audio.
  import { tick } from 'svelte';
  import ContextMenu from './ContextMenu.svelte';

  let {
    open = $bindable(false),
    x = 0,
    y = 0,
    items = [],
    label = 'Menu',
    returnFocus = null,
    // Opened from the keyboard: the first item takes the focus (from the
    // pointer, the menu does, and the arrow keys go from there).
    keyboard = false,
    // `x` is where the menu's right edge goes (under a row's ⋯ button).
    alignEnd = false,
    // A submenu's own: the item it hangs off, and how it hands back.
    anchor = null,
    onback = null,
    onclosed = null,
  } = $props();

  let menuEl = $state(null);
  let pos = $state({ left: 0, top: 0, ready: false });
  let subIndex = $state(-1); // the item whose submenu is open
  let subByKey = $state(false); // …opened by a key or a click (it takes the focus)
  let subEls = $state([]);
  const isSub = $derived(!!anchor);

  const enabled = (it) => it && !it.separator && !it.note && !it.disabled;

  let openedAt = 0;
  $effect(() => {
    if (!open) {
      subIndex = -1;
      pos = { left: 0, top: 0, ready: false };
      return;
    }
    openedAt = performance.now();
    place();
  });

  /** Where the menu goes: at the pointer (a submenu beside its item), kept inside the window. */
  async function place() {
    await tick();
    if (!menuEl) return;
    const m = 8;
    const w = menuEl.offsetWidth;
    const h = menuEl.offsetHeight;
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    let left;
    let top;
    if (anchor) {
      const r = anchor.getBoundingClientRect();
      left = r.right + w + 2 <= vw - m ? r.right + 2 : r.left - w - 2;
      top = r.top - 5;
    } else {
      left = alignEnd ? x - w : x;
      top = y;
      // No room below the pointer: open upwards, as Windows does.
      if (top + h > vh - m && y - h >= m) top = y - h;
    }
    left = Math.max(m, Math.min(left, vw - w - m));
    top = Math.max(m, Math.min(top, vh - h - m));
    pos = { left, top, ready: true };
    // Visible first: a hidden element can't take the focus.
    await tick();
    if (!menuEl) return;
    const first = items.findIndex(enabled);
    if (keyboard && first >= 0) focusItem(first);
    else if (!isSub) menuEl.focus({ preventScroll: true });
  }

  function buttons() {
    return menuEl ? [...menuEl.querySelectorAll(':scope > [role^="menuitem"]')] : [];
  }
  function focusItem(i) {
    const el = buttons().find((b) => Number(b.dataset.index) === i);
    el?.focus({ preventScroll: true });
  }
  function current() {
    return Number(document.activeElement?.dataset?.index ?? -1);
  }
  function step(dir) {
    const order = items.map((it, i) => (enabled(it) ? i : -1)).filter((i) => i >= 0);
    if (!order.length) return;
    const at = order.indexOf(current());
    const next = at < 0 ? (dir > 0 ? 0 : order.length - 1) : (at + dir + order.length) % order.length;
    focusItem(order[next]);
  }

  /** Close this menu (and its parent, unless going `back` one level). */
  function close(refocus = true) {
    if (!open) return;
    const hadFocus = menuEl?.contains(document.activeElement);
    open = false;
    if (refocus && hadFocus) returnFocus?.focus?.({ preventScroll: true });
    onclosed?.();
  }

  async function activate(it, i) {
    if (!enabled(it)) return;
    if (it.items) {
      subIndex = i;
      subByKey = true;
      await tick();
      return;
    }
    // A submenu's choice closes every level; the root returns the focus first,
    // so a dialog or an inline edit the choice opens can take it.
    if (isSub) onback?.({ all: true });
    else close(true);
    it.onselect?.();
  }

  function onKeydown(e) {
    switch (e.key) {
      case 'ArrowDown':
        step(1);
        break;
      case 'ArrowUp':
        step(-1);
        break;
      case 'Home':
        focusItem(items.findIndex(enabled));
        break;
      case 'End':
        focusItem(items.findLastIndex(enabled));
        break;
      case 'ArrowRight': {
        const i = current();
        if (items[i]?.items && enabled(items[i])) activate(items[i], i);
        else return;
        break;
      }
      case 'ArrowLeft':
        if (!isSub) return;
        onback?.({ all: false });
        break;
      case 'Escape':
        if (isSub) onback?.({ all: false });
        else close(true);
        break;
      case 'Tab':
        if (isSub) onback?.({ all: true });
        else close(true);
        break;
      case 'Enter':
      case ' ': {
        const i = current();
        if (i >= 0) activate(items[i], i);
        break;
      }
      default:
        return;
    }
    e.preventDefault();
    e.stopPropagation();
  }

  // A submenu handing back: one level (focus its item), or everything.
  function fromSub({ all }) {
    const i = subIndex;
    subIndex = -1;
    if (all) {
      if (isSub) onback?.({ all: true });
      else close(true);
    } else {
      focusItem(i);
    }
  }

  // Outside the menu (and its submenus): close, leaving the focus where the
  // click put it.
  function onWindowPointerdown(e) {
    if (!open || isSub) return;
    if (e.target.closest?.('[data-context-menu]')) return;
    close(false);
  }
  function onWindowScroll(e) {
    if (!open || isSub) return;
    if (e.target?.closest?.('[data-context-menu]')) return;
    // The right-click focusing a row half out of view scrolls it in a frame
    // later: that's not the person scrolling away.
    if (performance.now() - openedAt < 250) return;
    // Something else scrolling (a thread loading beside the row) leaves the
    // row where it was.
    const t = e.target;
    if (returnFocus && t?.nodeType === 1 && !t.contains(returnFocus)) return;
    close(false);
  }
  function onWindowAway() {
    if (open && !isSub) close(false);
  }
  // Esc with the focus somewhere else (a click on a disabled item's note…).
  function onWindowKeydown(e) {
    if (!open || isSub || e.key !== 'Escape') return;
    if (menuEl?.contains(document.activeElement)) return;
    e.preventDefault();
    open = false;
    returnFocus?.focus?.({ preventScroll: true });
    onclosed?.();
  }
</script>

<svelte:window
  onpointerdowncapture={onWindowPointerdown}
  onscrollcapture={onWindowScroll}
  onresize={onWindowAway}
  onblur={onWindowAway}
  onkeydown={onWindowKeydown}
/>

{#snippet icon(name)}
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    {#if name === 'pencil'}
      <path d="M12 20h9" /><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4z" />
    {:else if name === 'trash'}
      <path d="M3 6h18M8 6V4a1 1 0 0 1 1-1h6a1 1 0 0 1 1 1v2m3 0v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6" />
    {:else if name === 'folder'}
      <path d="M4 20h16a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.7-.9L9.2 3.9A2 2 0 0 0 7.5 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2z" />
    {:else if name === 'copy'}
      <rect x="9" y="9" width="13" height="13" rx="2" /><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
    {:else if name === 'paste'}
      <rect x="8" y="2.5" width="8" height="4" rx="1" /><path d="M16 4.5h2a2 2 0 0 1 2 2V20a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6.5a2 2 0 0 1 2-2h2" />
    {:else if name === 'download'}
      <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" /><path d="M7 10l5 5 5-5" /><path d="M12 15V3" />
    {:else if name === 'check'}
      <path d="M4 12l5 5L20 6" />
    {:else if name === 'open'}
      <path d="M14 4h6v6" /><path d="M20 4l-9 9" /><path d="M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5" />
    {:else if name === 'play'}
      <path d="M8 5.5v13l10.5-6.5z" fill="currentColor" stroke="none" />
    {:else if name === 'stop'}
      <rect x="6.5" y="6.5" width="11" height="11" rx="2" fill="currentColor" stroke="none" />
    {:else if name === 'sparkle'}
      <path d="M12 3l1.9 5.1L19 10l-5.1 1.9L12 17l-1.9-5.1L5 10l5.1-1.9z" />
    {:else if name === 'join'}
      <rect x="3" y="6.5" width="12.5" height="11" rx="2" /><path d="M15.5 10.5l5-3v9l-5-3" />
    {:else if name === 'record'}
      <circle cx="12" cy="12" r="7.5" /><circle cx="12" cy="12" r="3.2" fill="currentColor" stroke="none" />
    {:else if name === 'person'}
      <circle cx="12" cy="8" r="3.6" /><path d="M5 20a7 7 0 0 1 14 0" />
    {:else if name === 'sync'}
      <path d="M21 12a9 9 0 0 1-15.5 6.2L3 16" /><path d="M3 21v-5h5" /><path d="M3 12a9 9 0 0 1 15.5-6.2L21 8" /><path d="M21 3v5h-5" />
    {:else if name === 'unlink'}
      <path d="M9.5 14.5l5-5" /><path d="M7 11l-2 2a3.5 3.5 0 0 0 5 5l2-2" /><path d="M17 13l2-2a3.5 3.5 0 0 0-5-5l-2 2" />
    {:else if name === 'signout'}
      <path d="M9 4H5a1 1 0 0 0-1 1v14a1 1 0 0 0 1 1h4" /><path d="M16 16l4-4-4-4" /><path d="M20 12H9" />
    {:else if name === 'star'}
      <path d="M12 3.2l2.7 5.5 6 .9-4.35 4.2 1 6-5.35-2.8-5.35 2.8 1-6L3.3 9.6l6-.9z" />
    {:else if name === 'approx'}
      <path d="M4 9.5c2.5-2 5.5 2 8 0s5.5-2 8 0" /><path d="M4 15c2.5-2 5.5 2 8 0s5.5-2 8 0" />
    {:else if name === 'use'}
      <circle cx="12" cy="12" r="8" /><path d="M8.5 12.2l2.4 2.4 4.6-4.9" />
    {:else if name === 'archive'}
      <rect x="3" y="4" width="18" height="5" rx="1" /><path d="M5 9v10a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1V9M10 13h4" />
    {:else if name === 'restore'}
      <path d="M3 12a9 9 0 1 0 3-6.7L3 8" /><path d="M3 3v5h5" />
    {:else if name === 'undo'}
      <path d="M9 14L4 9l5-5" /><path d="M4 9h10.5a5.5 5.5 0 0 1 0 11H11" />
    {:else if name === 'split'}
      <path d="M4 7h16" /><path d="M4 17h10" /><path d="M17 13v8" /><path d="M14 18l3 3 3-3" />
    {:else if name === 'audio'}
      <path d="M11 5L6 9H3v6h3l5 4z" /><path d="M15.5 8.5a5 5 0 0 1 0 7" /><path d="M18.5 5.5a9 9 0 0 1 0 13" />
    {/if}
  </svg>
{/snippet}

{#if open}
  <div
    bind:this={menuEl}
    class="menu"
    class:ready={pos.ready}
    role="menu"
    aria-label={label}
    tabindex="-1"
    data-context-menu
    style="left: {pos.left}px; top: {pos.top}px"
    onkeydown={onKeydown}
    oncontextmenu={(e) => e.preventDefault()}
  >
    {#each items as it, i (i)}
      {#if it.separator}
        <div class="sep" role="separator"></div>
      {:else if it.note}
        <p class="note">{it.note}</p>
      {:else}
        <button
          bind:this={subEls[i]}
          type="button"
          class="item"
          class:danger={it.danger}
          class:open={subIndex === i}
          role={it.checked != null ? (it.toggle ? 'menuitemcheckbox' : 'menuitemradio') : 'menuitem'}
          aria-checked={it.checked != null ? !!it.checked : undefined}
          aria-disabled={it.disabled ? 'true' : undefined}
          aria-haspopup={it.items ? 'menu' : undefined}
          aria-expanded={it.items ? subIndex === i : undefined}
          tabindex="-1"
          data-index={i}
          onclick={() => activate(it, i)}
          onpointerenter={() => {
            if (it.items && enabled(it)) {
              if (subIndex !== i) subByKey = false;
              subIndex = i;
            } else if (subIndex >= 0) {
              subIndex = -1;
            }
          }}
        >
          <span class="ico">{#if it.icon}{@render icon(it.icon)}{/if}</span>
          <span class="lbl">{it.label}</span>
          {#if it.checked}
            <span class="tick">{@render icon('check')}</span>
          {:else if it.items}
            <svg class="chev" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 6l6 6-6 6" /></svg>
          {/if}
        </button>
      {/if}
    {/each}
  </div>
  {#if subIndex >= 0 && items[subIndex]?.items}
    {#key `${subIndex}:${subByKey}`}
      <ContextMenu
        open={true}
        items={items[subIndex].items}
        label={items[subIndex].label}
        anchor={subEls[subIndex]}
        keyboard={subByKey}
        onback={fromSub}
      />
    {/key}
  {/if}
{/if}

<style>
  .menu {
    position: fixed;
    z-index: 90;
    min-width: 176px;
    max-width: 280px;
    padding: 4px;
    box-sizing: border-box;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
    box-shadow: var(--yap-shadow-menu);
    color: var(--yap-fg);
    font-size: 12px;
    outline: none;
    visibility: hidden;
  }
  .menu.ready {
    visibility: visible;
    animation: menu-in 0.12s var(--yap-ease-spring, ease-out) both;
  }
  @keyframes menu-in {
    from {
      opacity: 0;
      transform: scale(0.97);
    }
  }
  .item {
    display: flex;
    align-items: center;
    gap: 9px;
    width: 100%;
    height: 30px;
    padding: 0 9px 0 8px;
    border: none;
    border-radius: var(--yap-r-sm);
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: default;
    outline: none;
  }
  .item:hover:not([aria-disabled='true']),
  .item:focus-visible,
  .item.open {
    background: var(--yap-s3);
  }
  .item:focus-visible {
    box-shadow: inset 0 0 0 1.5px var(--yap-primary-line);
  }
  .item[aria-disabled='true'] {
    color: var(--yap-muted-55);
  }
  .item.danger:not([aria-disabled='true']) {
    color: var(--yap-danger);
  }
  .ico {
    display: inline-flex;
    width: 13px;
    flex: 0 0 auto;
    color: var(--yap-muted);
  }
  .item.danger:not([aria-disabled='true']) .ico,
  .item[aria-disabled='true'] .ico {
    color: inherit;
  }
  .ico :global(svg),
  .tick :global(svg),
  .chev {
    width: 13px;
    height: 13px;
  }
  .lbl {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tick {
    display: inline-flex;
    color: var(--yap-primary);
  }
  .chev {
    flex: 0 0 auto;
    color: var(--yap-muted-55);
  }
  .sep {
    height: 1px;
    margin: 4px 6px;
    background: var(--yap-border-subtle);
  }
  .note {
    margin: 2px 9px 4px 30px;
    font-size: 11px;
    line-height: 15px;
    color: var(--yap-muted-55);
  }
</style>
