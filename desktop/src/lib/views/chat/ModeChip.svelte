<script>
  // **The Mode chip** beside the model chip under Chat's box (Jobs,
  // desktop/src-tauri/src/jobs/modes.rs; after Claude Code's own mode menu,
  // its words): "Auto ▾" opens **Mode** — Auto (Recommended) · Manual ·
  // Accept edits · Plan · Bypass permissions, each with Claude Code's line,
  // a check on the one in use, 1–5 to pick while it's open, ↑ ↓ and Enter,
  // Esc to close. It's the mode the jobs this chat starts run at (the chat's
  // model may only lower it, never raise it), kept per conversation
  // (`job_mode_get` / `job_mode_set`); a new chat shows Settings → Jobs'
  // default and keeps a pick until its conversation exists. Shown only with
  // Jobs on. **Bypass permissions** asks once (a ConfirmDialog: "Claude
  // Code can run anything on your PC without asking…"; `job_bypass_confirmed`)
  // and then switches freely; the chip is amber while it's on.
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount, tick, untrack } from 'svelte';
  import ConfirmDialog from '../../ui/ConfirmDialog.svelte';
  import { toast } from '../../ui/toast.svelte.js';
  import { modeForKey, MODE_KEYS } from '../../core/jobs.js';

  /** `chat`: the Chat view's state (its conversation, and whether a question is on its way). */
  let { chat } = $props();

  const BYPASS = 'bypassPermissions';
  const WARNING = 'Claude Code can run anything on your PC without asking. Use it only in folders you trust.';

  let enabled = $state(false);
  let view = $state(null); // { mode, label, bypassConfirmed, modes: [{mode, label, says}] }
  // A new chat's pick, for the conversation its first question makes.
  let pending = $state(null);
  let open = $state(false);
  let active = $state(0);
  let confirming = $state(false);
  let wrapEl = $state(null);
  let chipEl = $state(null);
  let menuEl = $state(null);
  let pos = $state({ left: 12, bottom: 60 });

  const modes = $derived(view?.modes ?? []);
  const shown = $derived(pending ?? view?.mode ?? 'auto');
  const shownLabel = $derived(modes.find((m) => m.mode === shown)?.label ?? view?.label ?? 'Auto');
  const bypass = $derived(shown === BYPASS);

  async function loadEnabled() {
    try {
      enabled = !!(await invoke('get_config'))?.jobsEnabled;
    } catch {
      /* keep */
    }
  }

  async function load(conversation) {
    try {
      const v = await invoke('job_mode_get', { conversation: conversation ?? null });
      if ((chat.activeId ?? null) === (conversation ?? null)) view = v;
    } catch {
      /* keep the last */
    }
  }

  // Another conversation (or a new chat's own, once its first question
  // made it): its Mode — a new chat's pick goes to the conversation its
  // question made, never to one opened from the list.
  let lastId = undefined;
  $effect(() => {
    const id = chat.activeId ?? null;
    untrack(() => {
      if (id === lastId) return;
      const was = lastId;
      lastId = id;
      if (was === null && id !== null && pending && (chat.sending || chat.assistantPending === id)) {
        const mode = pending;
        pending = null;
        invoke('job_mode_set', { conversation: id, mode })
          .then((v) => {
            if (chat.activeId === id) view = v;
          })
          .catch(() => load(id));
        return;
      }
      pending = null;
      load(id);
    });
  });

  onMount(() => {
    loadEnabled();
    const uns = [listen('yap-config-saved', loadEnabled)];
    return () => uns.forEach((u) => u.then((f) => f()).catch(() => {}));
  });

  function place() {
    if (!chipEl) return;
    const r = chipEl.getBoundingClientRect();
    const top = (chipEl.closest('[data-model-picker-above]') ?? chipEl).getBoundingClientRect().top;
    pos = { left: Math.max(12, Math.min(r.left, window.innerWidth - 340 - 12)), bottom: window.innerHeight - top + 8 };
  }

  async function toggle() {
    if (open) {
      close(true);
      return;
    }
    // Open at once (keys pressed right after the click land in it), then
    // read the chat's Mode again behind it.
    active = Math.max(0, modes.findIndex((m) => m.mode === shown));
    place();
    open = true;
    await tick();
    menuEl?.querySelectorAll('[role="menuitemradio"]')[active]?.focus();
    load(chat.activeId ?? null);
  }

  function close(focusChip) {
    open = false;
    if (focusChip) tick().then(() => chipEl?.focus());
  }

  /** Pick `mode`: Bypass asks first, the first time. */
  async function pick(mode) {
    if (!mode) return;
    if (mode === BYPASS && !view?.bypassConfirmed) {
      close(false);
      confirming = true;
      return;
    }
    close(true);
    await apply(mode);
  }

  async function apply(mode) {
    const id = chat.activeId ?? null;
    if (id == null) {
      // A new chat: kept for the conversation its first question makes.
      pending = mode === view?.mode ? null : mode;
      return;
    }
    try {
      view = await invoke('job_mode_set', { conversation: id, mode });
    } catch (e) {
      toast({ title: "Couldn't change the Mode", description: String(e?.message ?? e), variant: 'destructive' });
    }
  }

  async function confirmBypass() {
    try {
      await invoke('job_bypass_confirmed');
      if (view) view = { ...view, bypassConfirmed: true };
    } catch {
      return;
    }
    confirming = false;
    await apply(BYPASS);
    tick().then(() => chipEl?.focus());
  }

  function onMenuKey(e) {
    const key = modeForKey(modes, e.key);
    if (key) {
      e.preventDefault();
      pick(key);
      return;
    }
    const items = [...(menuEl?.querySelectorAll('[role="menuitemradio"]') ?? [])];
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      close(true);
    } else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      active = (active + (e.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length;
      items[active]?.focus();
    } else if (e.key === 'Home' || e.key === 'End') {
      e.preventDefault();
      active = e.key === 'Home' ? 0 : items.length - 1;
      items[active]?.focus();
    } else if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      pick(modes[active]?.mode);
    } else if (e.key === 'Tab') {
      close(false);
    }
  }

  function onWindowPointer(e) {
    if (open && wrapEl && !e.composedPath().includes(wrapEl)) close(false);
  }
</script>

<svelte:window onpointerdowncapture={onWindowPointer} onresize={() => open && place()} />

{#if enabled}
  <div class="modechip" bind:this={wrapEl}>
    <button
      type="button"
      class="chip"
      class:bypass
      bind:this={chipEl}
      aria-haspopup="menu"
      aria-expanded={open}
      aria-label={`Mode for jobs: ${shownLabel}. Change mode`}
      title={bypass ? `Bypass permissions: ${WARNING}` : 'The mode jobs from this chat run in'}
      data-testid="chat-mode-chip"
      data-mode={shown}
      onclick={toggle}
    >
      {#if bypass}
        <svg class="warn" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 9v4" /><path d="M12 17h.01" /><path d="M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z" /></svg>
      {/if}
      <span class="clabel">{shownLabel}</span>
      <svg class="chev" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m6 9 6 6 6-6" /></svg>
    </button>

    {#if open}
      <!-- svelte-ignore a11y_interactive_supports_focus -->
      <div class="menu" role="menu" aria-label="Mode" tabindex="-1" bind:this={menuEl} style="left: {pos.left}px; bottom: {pos.bottom}px" onkeydown={onMenuKey} data-testid="chat-mode-menu">
        <p class="mtitle" aria-hidden="true">Mode</p>
        {#each modes as m, i (m.mode)}
          {@const on = m.mode === shown}
          <button
            type="button"
            class="mrow"
            class:on
            class:danger={m.mode === BYPASS}
            role="menuitemradio"
            aria-checked={on}
            tabindex={i === active ? 0 : -1}
            data-mode={m.mode}
            onclick={() => pick(m.mode)}
            onpointerenter={() => (active = i)}
          >
            <span class="mtext">
              <span class="mname">{m.label}{#if m.mode === 'auto'}<span class="rec">Recommended</span>{/if}</span>
              <span class="msays">{m.says}</span>
            </span>
            <span class="mcheck" aria-hidden="true">
              {#if on}
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12.5l4.5 4.5L19 7.5" /></svg>
              {/if}
            </span>
            <kbd class="mkey" aria-hidden="true">{MODE_KEYS[i]}</kbd>
          </button>
        {/each}
      </div>
    {/if}
  </div>

  <ConfirmDialog
    bind:open={confirming}
    title="Use Bypass permissions?"
    body={WARNING}
    confirmLabel="Use Bypass permissions"
    returnFocus={chipEl}
    onconfirm={confirmBypass}
  />
{/if}

<style>
  .modechip {
    position: relative;
    flex: none;
    display: flex;
    align-items: center;
  }
  /* The model chip's quiet look (ModelChip `quiet`): words, a hover surface. */
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 26px;
    padding: 0 7px 0 8px;
    border: 1px solid transparent;
    border-radius: var(--yap-r-full);
    background: none;
    color: var(--yap-muted);
    font: inherit;
    font-size: 11.5px;
    white-space: nowrap;
    cursor: pointer;
    transition:
      border-color var(--yap-dur),
      background var(--yap-dur);
  }
  .chip .clabel {
    font-weight: 500;
    color: var(--yap-fg-80);
  }
  .chip:hover,
  .chip[aria-expanded='true'] {
    background: var(--yap-s3);
  }
  .chip:focus-visible {
    outline: 2px solid var(--yap-primary);
    outline-offset: 1px;
  }
  /* Bypass permissions on: amber, so it's never on by accident. */
  .chip.bypass {
    border-color: color-mix(in srgb, var(--yap-warning) 45%, transparent);
    background: color-mix(in srgb, var(--yap-warning) 14%, transparent);
    color: var(--yap-warning);
  }
  .chip.bypass .clabel {
    color: var(--yap-warning);
    font-weight: 600;
  }
  .chip.bypass:hover,
  .chip.bypass[aria-expanded='true'] {
    background: color-mix(in srgb, var(--yap-warning) 22%, transparent);
  }
  .warn {
    width: 12px;
    height: 12px;
  }
  .chev {
    width: 11px;
    height: 11px;
    color: var(--yap-muted-70);
  }
  .chip.bypass .chev {
    color: var(--yap-warning);
  }

  /* The menu: the model picker's surface (ChatModelPicker `.pop`). */
  .menu {
    position: fixed;
    z-index: 40;
    width: 330px;
    max-width: calc(100vw - 24px);
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
    .menu {
      animation: pop-in 140ms ease-out;
    }
  }
  :global(:root[data-motion='reduced']) .menu {
    animation: none;
  }
  @keyframes pop-in {
    from {
      opacity: 0;
      transform: translateY(4px);
    }
  }
  .mtitle {
    margin: 2px 8px 6px;
    font-size: 11.5px;
    font-weight: 650;
    color: var(--yap-muted);
  }
  .mrow {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 16px 18px;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 7px 8px;
    border: none;
    border-radius: 9px;
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .mrow:hover,
  .mrow:focus-visible {
    background: var(--yap-s3);
    outline: none;
  }
  .mrow:focus-visible {
    box-shadow: inset 0 0 0 1.5px var(--yap-primary-line);
  }
  .mtext {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .mname {
    display: inline-flex;
    align-items: baseline;
    gap: 7px;
    font-weight: 600;
    color: var(--yap-fg);
  }
  .rec {
    font-size: 11px;
    font-weight: 500;
    color: var(--yap-muted);
  }
  .msays {
    font-size: 12px;
    color: var(--yap-muted);
  }
  .mrow.danger .mname {
    color: var(--yap-warning);
  }
  .mcheck {
    display: inline-flex;
    color: var(--yap-primary);
  }
  .mcheck svg {
    width: 15px;
    height: 15px;
  }
  .mkey {
    justify-self: end;
    min-width: 18px;
    padding: 1px 4px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: 5px;
    background: var(--yap-s1);
    color: var(--yap-muted);
    font: inherit;
    font-size: 10.5px;
    text-align: center;
  }
</style>
