<script>
  // **The @ picker** (agents/mention.rs, 15 Nov 2026, as Claude Code's own `@`): a Code chat's box, an `@` typed —
  // its workspace's files, the best match first (each its name, then its folder, quiet). The keys stay in the box
  // (↑ ↓ move, Enter or Tab puts it in the message, Esc closes); a click puts it in too.

  /** `picker`: chatState's mention ({ open, items: [{ path, name, dir }], active, query, loading }); `onpick(hit)`. */
  let { picker, onpick } = $props();

  let listEl = $state(null);
  $effect(() => {
    const i = picker.active;
    listEl?.querySelector(`#mention-opt-${i}`)?.scrollIntoView({ block: 'nearest' });
  });

  // The pointer picks a row only when it moves (a row scrolled under a still pointer never takes the highlight).
  let pointer = { x: -1, y: -1 };
  function hover(e, i) {
    if (e.clientX === pointer.x && e.clientY === pointer.y) return;
    pointer = { x: e.clientX, y: e.clientY };
    picker.active = i;
  }
</script>

{#if picker.open}
  <div class="mentionmenu" data-testid="mention-menu">
    <!-- The focus stays in the box: its keys move the highlight (aria-activedescendant there). -->
    <div class="list" role="listbox" aria-label="Files in this workspace" id="mention-list" bind:this={listEl}>
      {#each picker.items as h, i (h.path)}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <div
          role="option"
          tabindex="-1"
          id={`mention-opt-${i}`}
          class="opt"
          aria-selected={i === picker.active}
          class:active={i === picker.active}
          title={h.path}
          onmousedown={(e) => e.preventDefault()}
          onmousemove={(e) => hover(e, i)}
          onclick={() => onpick(h)}
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z" /><path d="M14 3v5h5" /></svg>
          <span class="name">{h.name}</span>
          {#if h.dir}<span class="dir">{h.dir}</span>{/if}
        </div>
      {:else}
        <div class="none">{picker.loading ? 'Looking…' : picker.query ? `No file matches “${picker.query}”` : 'No files in this workspace'}</div>
      {/each}
    </div>
    <div class="foot" aria-hidden="true">
      <span><kbd>↑</kbd><kbd>↓</kbd> move</span>
      <span><kbd>Enter</kbd> add</span>
      <span><kbd>Esc</kbd> close</span>
    </div>
  </div>
{/if}

<style>
  .mentionmenu {
    position: absolute;
    left: 0;
    right: 0;
    bottom: calc(100% + 6px);
    z-index: 30;
    max-height: 300px;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
    color: var(--yap-fg);
    box-shadow: var(--yap-shadow-menu);
    pointer-events: auto;
  }
  .list {
    flex: 1 1 auto;
    min-height: 0;
    padding: 4px;
    overflow-y: auto;
  }
  .opt {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 30px;
    padding: 4px 8px;
    border-radius: 8px;
    cursor: pointer;
  }
  .opt.active {
    background: var(--yap-s3);
  }
  .opt svg {
    flex: 0 0 auto;
    width: 14px;
    height: 14px;
    color: var(--yap-muted);
  }
  .name {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12.5px;
    font-weight: 600;
  }
  .dir {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--yap-muted);
    font-size: 11.5px;
  }
  .none {
    padding: 10px 8px;
    color: var(--yap-muted);
    font-size: 12px;
  }
  .foot {
    display: flex;
    gap: 12px;
    padding: 5px 10px;
    border-top: 1px solid var(--yap-border-subtle);
    color: var(--yap-muted);
    font-size: 11px;
  }
  kbd {
    margin-right: 2px;
    padding: 0 4px;
    border: 1px solid var(--yap-border);
    border-radius: 4px;
    font: inherit;
    font-size: 10px;
  }
</style>
