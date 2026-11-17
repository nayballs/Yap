<script>
  // **A Code chat's "/" list** (17 Nov 2026, as Claude Code's own box has it; codeMenu.svelte.js
  // `CommandPicker`): the commands and skills the chat's Claude Code offers in its workspace — the person's own and
  // their plugins', then Claude Code's built-in ones — each with its one line and what it takes. A click (or Enter,
  // Tab) puts `/name ` in the box; the keys stay in the box. Nothing known yet: said, while it's looked at.
  /** `picker`: the box's CommandPicker; `onpick(command)`. */
  let { picker, onpick } = $props();

  let listEl = $state(null);
  $effect(() => {
    const i = picker.active;
    listEl?.querySelector(`#cmd-opt-${i}`)?.scrollIntoView({ block: 'nearest' });
  });

  // The pointer picks a row only when it moves (a row scrolled under a still pointer keeps the keys' highlight).
  let pointer = { x: -1, y: -1 };
  function hover(e, i) {
    if (e.clientX === pointer.x && e.clientY === pointer.y) return;
    pointer = { x: e.clientX, y: e.clientY };
    picker.active = i;
  }
  const menu = $derived(picker.menu);
</script>

{#snippet option(c, i)}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    role="option"
    tabindex="-1"
    id={`cmd-opt-${i}`}
    class="opt"
    aria-selected={i === picker.active}
    class:active={i === picker.active}
    title={c.description || c.name}
    onmousedown={(e) => e.preventDefault()}
    onmousemove={(e) => hover(e, i)}
    onclick={() => onpick(c)}
  >
    <span class="cmd">/{c.name}</span>
    {#if c.hint}<span class="hint">{c.hint}</span>{/if}
    <span class="line">{c.description}</span>
  </div>
{/snippet}

{#if picker.open}
  <div class="cmdmenu" data-testid="code-command-menu">
    <!-- The focus stays in the box: its keys move the highlight (aria-activedescendant there). -->
    <div class="list" role="listbox" aria-label="Slash commands" id="cmd-list" bind:this={listEl}>
      {#if picker.items.length}
        {#each picker.groups as g, gi (g.label || gi)}
          <div class="group" role="group" aria-label={g.label || undefined}>
            {#if g.label}<div class="ghead" aria-hidden="true">{g.label}</div>{/if}
            {#each g.items as c, k (c.name)}{@render option(c, g.start + k)}{/each}
          </div>
        {/each}
      {:else if !menu.loaded || menu.looking}
        <p class="empty" role="status">Asking Claude Code what it offers here…</p>
      {:else if menu.error}
        <p class="empty" role="status">{menu.error}</p>
      {:else if picker.query}
        <p class="empty" role="status">No command starts with /{picker.query}. Enter sends it as typed.</p>
      {:else}
        <p class="empty" role="status">Claude Code's commands show here once it has answered in this workspace.</p>
      {/if}
    </div>
    <div class="foot" aria-hidden="true">
      <span><kbd>↑</kbd><kbd>↓</kbd> move</span>
      <span><kbd>Enter</kbd> or <kbd>Tab</kbd> put it in the box</span>
      <span><kbd>Esc</kbd> close</span>
    </div>
  </div>
{/if}

<style>
  .cmdmenu {
    position: absolute;
    left: 0;
    right: 0;
    bottom: calc(100% + 6px);
    z-index: 30;
    max-height: 320px;
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
  .group + .group {
    margin-top: 4px;
  }
  .ghead {
    padding: 6px 8px 3px;
    color: var(--yap-muted);
    font-size: 11px;
    font-weight: 600;
  }
  .opt {
    display: flex;
    align-items: baseline;
    gap: 10px;
    min-height: 30px;
    box-sizing: border-box;
    padding: 6px 8px;
    border-radius: var(--yap-r-sm);
    font-size: 13px;
    line-height: 1.35;
    cursor: pointer;
  }
  .opt.active {
    background: var(--yap-s3);
  }
  .cmd {
    flex: 0 0 auto;
    max-width: 45%;
    overflow: hidden;
    text-overflow: ellipsis;
    font-weight: 650;
    white-space: nowrap;
  }
  .hint {
    flex: 0 1 auto;
    max-width: 25%;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--yap-muted);
    font-family: var(--yap-font-mono, ui-monospace, monospace);
    font-size: 11.5px;
    white-space: nowrap;
  }
  .line {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--yap-muted);
    white-space: nowrap;
  }
  .empty {
    margin: 0;
    padding: 10px 10px;
    color: var(--yap-muted);
    font-size: 12.5px;
  }
  .foot {
    flex: 0 0 auto;
    display: flex;
    flex-wrap: wrap;
    gap: 4px 12px;
    padding: 6px 12px;
    border-top: 1px solid var(--yap-border-subtle);
    color: var(--yap-muted);
    font-size: 11px;
  }
  .foot > span {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    white-space: nowrap;
  }
  kbd {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 16px;
    height: 16px;
    box-sizing: border-box;
    padding: 0 4px;
    border: 1px solid var(--yap-border);
    border-radius: 4px;
    background: var(--yap-s1);
    color: var(--yap-fg-80);
    font: inherit;
    font-size: 10px;
    font-weight: 600;
  }
</style>
