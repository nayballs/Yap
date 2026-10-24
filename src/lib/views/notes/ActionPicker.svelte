<script>
  // The right end of the Notes view's bottom bar. While the note records:
  // the end-of-meeting moment, "End meeting & summarise". Otherwise the
  // ActionPicker split button (OpenWhispr ActionPicker.tsx): the left half
  // runs the last-used action, the chevron opens the action menu (and
  // Manage actions, the ActionManager dialog).

  /** `notes`: the view's state (notesState.svelte.js). */
  let { notes } = $props();

</script>

{#if notes.recordingThisNote}
  <button class="enhance endmeeting" onclick={notes.endMeeting}>
    <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="6" y="6" width="12" height="12" rx="2" fill="currentColor" /></svg>
    End meeting &amp; summarise
  </button>
{:else}
<div class="picker">
  <button
    class="enhance runhalf"
    onclick={(e) => notes.runAction(notes.activeAction, e.currentTarget)}
    disabled={!notes.activeAction ||
      notes.enhancing.has(notes.selected.id) ||
      notes.jobRunning ||
      (!notes.selected.content?.trim() && !notes.selected.transcript?.length)}
    title={notes.activeAction ? `Run "${notes.activeAction.name}"` : 'No actions'}
  >
    {#if notes.enhancing.has(notes.selected.id) || notes.jobRunning}
      <span class="spin"></span> Running…
    {:else}
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3v3M12 18v3M3 12h3M18 12h3M5.6 5.6l2.1 2.1M16.3 16.3l2.1 2.1M18.4 5.6l-2.1 2.1M7.7 16.3l-2.1 2.1" /></svg>
      {notes.activeAction?.name ?? 'Enhance'}
    {/if}
  </button>
  <button
    class="enhance chevron"
    aria-label="Select action"
    disabled={notes.enhancing.has(notes.selected.id) || notes.jobRunning}
    onclick={(e) => {
      e.stopPropagation();
      notes.pickerOpen = !notes.pickerOpen;
    }}
  >
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m6 9 6 6 6-6" /></svg>
  </button>
  {#if notes.pickerOpen}
    <div class="menu up" role="menu">
      {#each notes.actions as a (a.id)}
        <button class="mitem" class:current={a.id === notes.activeAction?.id} role="menuitem" onclick={() => notes.runAction(a)}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3v3M12 18v3M3 12h3M18 12h3M5.6 5.6l2.1 2.1M16.3 16.3l2.1 2.1M18.4 5.6l-2.1 2.1M7.7 16.3l-2.1 2.1" /></svg>
          <span class="mtext">
            <span class="mname">{a.name}</span>
            {#if a.description}<span class="mdesc">{a.description}</span>{/if}
          </span>
        </button>
      {/each}
      <div class="msep"></div>
      <button
        class="mitem manage"
        role="menuitem"
        onclick={() => {
          notes.pickerOpen = false;
          notes.actionsOpen = true;
        }}
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M20 7h-9M14 17H5" /><circle cx="17" cy="17" r="3" /><circle cx="7" cy="7" r="3" /></svg>
        Manage actions
      </button>
    </div>
  {/if}
</div>
{/if}

<style>
  .picker {
    position: relative;
    display: inline-flex;
  }
  .enhance {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 30px;
    padding: 0 13px;
    border: none;
    background: var(--yap-ink, var(--yap-primary));
    color: var(--yap-ink-fg, var(--yap-primary-fg));
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .enhance.runhalf {
    border-radius: var(--yap-r) 0 0 var(--yap-r);
  }
  .enhance.endmeeting {
    border-radius: var(--yap-r);
    background: var(--yap-danger-fill);
    color: #fff;
    padding: 0 14px;
  }
  .enhance.endmeeting:hover {
    background: var(--yap-danger-fill-hover);
  }
  .enhance.endmeeting svg {
    width: 10px;
    height: 10px;
  }
  .enhance.chevron {
    border-radius: 0 var(--yap-r) var(--yap-r) 0;
    border-left: 1px solid color-mix(in srgb, var(--yap-ink-fg) 22%, transparent);
    padding: 0 7px;
  }
  .enhance:hover:not(:disabled) {
    background: var(--yap-ink-hover, var(--yap-primary-hover));
  }
  .enhance:disabled {
    opacity: 0.55;
    cursor: default;
  }
  .enhance svg {
    width: 13px;
    height: 13px;
  }
  .enhance.chevron svg {
    width: 11px;
    height: 11px;
  }

  .menu {
    position: absolute;
    top: calc(100% + 6px);
    right: 0;
    z-index: 20;
    min-width: 230px;
    padding: 5px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s1);
    box-shadow: var(--yap-shadow-menu);
  }
  .menu.up {
    top: auto;
    bottom: calc(100% + 6px);
  }
  .mitem {
    display: flex;
    align-items: flex-start;
    gap: 9px;
    width: 100%;
    padding: 7px 9px;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-fg);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .mitem:hover {
    background: var(--yap-s2);
  }
  .mitem.current {
    background: var(--yap-primary-wash);
  }
  .mitem svg {
    width: 12px;
    height: 12px;
    flex: 0 0 auto;
    margin-top: 2px;
    color: var(--yap-primary);
  }
  .mitem.manage {
    color: var(--yap-muted);
    font-size: 11.5px;
    align-items: center;
  }
  .mitem.manage svg {
    color: var(--yap-muted-55);
    margin-top: 0;
  }
  .mtext {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .mname {
    font-size: 12px;
    font-weight: 600;
  }
  .mdesc {
    font-size: 10.5px;
    color: var(--yap-muted-55);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .msep {
    height: 1px;
    margin: 4px 6px;
    background: var(--yap-border-subtle);
  }
  .spin {
    width: 11px;
    height: 11px;
    border: 2px solid currentColor;
    border-top-color: transparent;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
