<script>
  // **The workspace picker** (Code chats, codeChats.svelte.js; after the
  // Claude app's environment menu): the person's workspaces — the job
  // folders — most recently used first, each its path on hover, a check on
  // the one in use; **Add workspace…** (Windows' folder picker, in Rust:
  // `job_folders_pick`) and **Manage workspaces…** (Settings → Jobs); in a
  // Code chat's own chip, **Plain chat** too (it leaves Code). Opened from
  // "New code chat" (below it) or the composer's workspace chip (above it):
  // ↑ ↓, Enter, Home / End, Esc, Tab out.
  import { onMount, tick } from 'svelte';
  import { toast } from '../../ui/toast.svelte.js';
  import { pickerRows } from '../../core/codeChats.js';

  /**
   * `code`: the view's Code chats. `anchor`: the element it opens from;
   * `place`: 'below' (New code chat) or 'above' (the composer's chip).
   * `current`: the workspace in use. `plain`: offer "Plain chat".
   * `onpick(ws)`, `onplain()`, `onclose(focusBack)`.
   */
  let { code, anchor, place = 'below', current = null, plain = false, title = 'Workspace', onpick, onplain = null, onclose } = $props();

  let menuEl = $state(null);
  let pos = $state({ left: 12, top: null, bottom: null });
  let active = $state(0);
  let adding = $state(false);

  const rows = $derived(pickerRows(code.workspaces, current));

  function placeIt() {
    if (!anchor) return;
    const r = anchor.getBoundingClientRect();
    const left = Math.max(8, Math.min(r.left, window.innerWidth - 300 - 8));
    if (place === 'above') {
      const top = (anchor.closest('[data-model-picker-above]') ?? anchor).getBoundingClientRect().top;
      pos = { left, top: null, bottom: window.innerHeight - top + 8 };
    } else {
      pos = { left, top: r.bottom + 6, bottom: null };
    }
  }

  const items = () => [...(menuEl?.querySelectorAll('[role="menuitemradio"], [role="menuitem"]') ?? [])];

  onMount(async () => {
    placeIt();
    code.refresh();
    await tick();
    const on = rows.findIndex((w) => w.on);
    active = Math.max(0, on);
    items()[active]?.focus();
  });

  function close(focusBack) {
    onclose?.(focusBack);
  }

  function pick(ws) {
    close(true);
    onpick?.(ws);
  }

  async function add() {
    if (adding) return;
    adding = true;
    try {
      const ws = await code.addWorkspace();
      if (!ws) {
        adding = false;
        await tick();
        items()[active]?.focus();
        return;
      }
      pick(ws);
    } catch (e) {
      toast({ title: "Couldn't add that workspace", description: String(e?.message ?? e), variant: 'destructive' });
    }
    adding = false;
  }

  function manage() {
    close(false);
    code.manage();
  }

  function toPlain() {
    close(true);
    onplain?.();
  }

  function onKey(e) {
    const list = items();
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      close(true);
    } else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      active = (active + (e.key === 'ArrowDown' ? 1 : -1) + list.length) % list.length;
      list[active]?.focus();
    } else if (e.key === 'Home' || e.key === 'End') {
      e.preventDefault();
      active = e.key === 'Home' ? 0 : list.length - 1;
      list[active]?.focus();
    } else if (e.key === 'Tab') {
      close(false);
    }
  }

  function onWindowPointer(e) {
    if (menuEl && !e.composedPath().includes(menuEl) && !(anchor && e.composedPath().includes(anchor))) close(false);
  }
</script>

<svelte:window onpointerdowncapture={onWindowPointer} onresize={placeIt} />

<!-- svelte-ignore a11y_interactive_supports_focus -->
<div
  class="wsmenu"
  role="menu"
  aria-label={title}
  tabindex="-1"
  bind:this={menuEl}
  style:left="{pos.left}px"
  style:top={pos.top == null ? null : `${pos.top}px`}
  style:bottom={pos.bottom == null ? null : `${pos.bottom}px`}
  onkeydown={onKey}
  data-testid="chat-workspace-menu"
>
  <p class="wtitle" aria-hidden="true">{title}</p>
  {#each rows as w, i (w.path)}
    <button
      type="button"
      class="wrow"
      class:on={w.on}
      role="menuitemradio"
      aria-checked={w.on}
      title={w.path}
      tabindex={i === active ? 0 : -1}
      data-workspace={w.name}
      onclick={() => pick(w)}
      onpointerenter={() => (active = i)}
    >
      <svg class="wicon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" /></svg>
      <span class="wname">{w.name}</span>
      <span class="wcheck" aria-hidden="true">
        {#if w.on}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12.5l4.5 4.5L19 7.5" /></svg>
        {/if}
      </span>
    </button>
  {:else}
    <p class="wnone">No workspaces yet. Add a folder on this PC for Code chats to work in.</p>
  {/each}
  <div class="wsep" role="separator"></div>
  <button
    type="button"
    class="wrow act"
    role="menuitem"
    tabindex={active === rows.length ? 0 : -1}
    disabled={adding}
    onclick={add}
    onpointerenter={() => (active = rows.length)}
  >
    <svg class="wicon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>
    <span class="wname">Add workspace…</span>
  </button>
  <button
    type="button"
    class="wrow act"
    role="menuitem"
    tabindex={active === rows.length + 1 ? 0 : -1}
    onclick={manage}
    onpointerenter={() => (active = rows.length + 1)}
  >
    <svg class="wicon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 7h10M18 7h2M4 17h4M12 17h8" /><circle cx="16" cy="7" r="2" /><circle cx="10" cy="17" r="2" /></svg>
    <span class="wname">Manage workspaces…</span>
  </button>
  {#if plain}
    <div class="wsep" role="separator"></div>
    <button
      type="button"
      class="wrow act"
      role="menuitem"
      tabindex={active === rows.length + 2 ? 0 : -1}
      onclick={toPlain}
      onpointerenter={() => (active = rows.length + 2)}
    >
      <svg class="wicon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M20 12.5a7.5 7.5 0 0 1-11.2 6.5L4 20l1.1-4.3A7.5 7.5 0 1 1 20 12.5z" /></svg>
      <span class="wtext">
        <span class="wname">Plain chat</span>
        <span class="wsays">Move this chat to Chat</span>
      </span>
    </button>
  {/if}
</div>

<style>
  /* The Mode menu's surface (ModeChip `.menu`, ChatModelPicker `.pop`). */
  .wsmenu {
    position: fixed;
    z-index: 40;
    width: 280px;
    max-width: calc(100vw - 16px);
    max-height: min(420px, calc(100vh - 24px));
    overflow-y: auto;
    padding: 6px;
    border: 1px solid var(--yap-border);
    border-radius: 14px;
    background: var(--yap-s2);
    box-shadow: var(--yap-shadow-menu);
    color: var(--yap-fg);
    font-size: 13px;
    line-height: 1.4;
    outline: none;
  }
  @media (prefers-reduced-motion: no-preference) {
    .wsmenu {
      animation: pop-in 140ms ease-out;
    }
  }
  :global(:root[data-motion='reduced']) .wsmenu {
    animation: none;
  }
  @keyframes pop-in {
    from {
      opacity: 0;
      transform: translateY(4px);
    }
  }
  .wtitle {
    margin: 2px 8px 6px;
    font-size: 11.5px;
    font-weight: 650;
    color: var(--yap-muted);
  }
  .wrow {
    display: flex;
    align-items: center;
    gap: 9px;
    width: 100%;
    min-height: 32px;
    padding: 6px 8px;
    border: none;
    border-radius: 9px;
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .wrow:hover,
  .wrow:focus-visible {
    background: var(--yap-s3);
    outline: none;
  }
  .wrow:focus-visible {
    box-shadow: inset 0 0 0 1.5px var(--yap-primary-line);
  }
  .wrow:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .wicon {
    flex: 0 0 auto;
    width: 15px;
    height: 15px;
    color: var(--yap-muted);
  }
  .wrow.on .wicon {
    color: var(--yap-code);
  }
  .wname {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 550;
  }
  .wrow.act .wname {
    font-weight: 500;
    color: var(--yap-fg-80);
  }
  .wtext {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .wsays {
    font-size: 11.5px;
    color: var(--yap-muted);
  }
  .wcheck {
    flex: 0 0 auto;
    display: inline-flex;
    width: 15px;
    color: var(--yap-code);
  }
  .wcheck svg {
    width: 15px;
    height: 15px;
  }
  .wnone {
    margin: 4px 8px 6px;
    font-size: 12px;
    color: var(--yap-muted);
  }
  .wsep {
    height: 1px;
    margin: 4px 6px;
    background: var(--yap-border-subtle);
  }
</style>
