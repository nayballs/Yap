<script>
  // **A chat's Outputs**, as rows (after the Claude app's right column): an
  // icon by kind, the name, its folder quieter, a word at the right
  // ("Page", "Document", "Changed", "Shared", "Picture"). The rows come from outputs.js
  // `outputItems`, the one list every kind adds to; `onopen(item)` opens one.
  // Shown first in a Code chat's summary (SummaryTab.svelte); usable
  // anywhere that lists what a chat made.
  // A right-click (or the ContextMenu key, Shift+F10) on a row: `onmenu(event, item)` opens its menu
  // (outputsOpen.js `outputMenuItems`: Open, Open in Yap, Show in folder, Copy path, Open in editor).
  import { menuKey, menuOpenFor } from '../../ui/contextMenu.svelte.js';
  /** `items`: outputs.js's rows; `onopen(item)`; `onmenu(event, item)`: its menu; `max`: rows shown before "Show N more". */
  let { items = [], onopen = null, onmenu = null, max = 8 } = $props();
  let all = $state(false);
  const rowEls = $state({});
  const shown = $derived(all ? items : items.slice(0, max));
</script>

<ul class="outputs" data-testid="chat-outputs">
  {#each shown as it (it.key)}
    <li>
      <button
        type="button"
        class="orow"
        class:menuopen={onmenu && menuOpenFor(rowEls[it.key])}
        data-testid="chat-output"
        data-kind={it.kind}
        title={[it.detail, it.name].filter(Boolean).join('/')}
        bind:this={rowEls[it.key]}
        onclick={() => onopen?.(it)}
        oncontextmenu={(e) => onmenu?.(e, it)}
        onkeydown={(e) => onmenu && menuKey(e, (ev) => onmenu(ev, it))}
      >
        {#if it.kind === 'page'}
          <svg class="oicon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="8" y="3" width="13" height="13" rx="2" /><path d="M16 16v3a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-9a2 2 0 0 1 2-2h3" /></svg>
        {:else if it.kind === 'doc'}
          <svg class="oicon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z" /><path d="M14 3v5h5" /><path d="M9 13h6M9 17h4" /></svg>
        {:else if it.picture}
          <svg class="oicon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3" y="4" width="18" height="16" rx="2" /><circle cx="9" cy="10" r="1.6" /><path d="m21 16-5-5-9 9" /></svg>
        {:else if it.kind === 'changed'}
          <svg class="oicon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z" /><path d="M14 3v5h5" /><path d="m10 13-2 2 2 2M14 13l2 2-2 2" /></svg>
        {:else}
          <svg class="oicon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z" /><path d="M14 3v5h5" /></svg>
        {/if}
        <span class="oname">{it.name}{#if it.detail}<span class="odir">{` · ${it.detail}`}</span>{/if}</span>
        {#if it.label}<span class="olabel">{it.label}</span>{/if}
      </button>
    </li>
  {/each}
</ul>
{#if items.length > max}
  <button type="button" class="omore" onclick={() => (all = !all)}>{all ? 'Show fewer' : `Show ${items.length - max} more`}</button>
{/if}

<style>
  .outputs {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .orow {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    min-height: 28px;
    padding: 3px 6px;
    border: none;
    border-radius: 7px;
    background: none;
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 12.5px;
    text-align: left;
    cursor: pointer;
  }
  .orow:hover,
  .orow.menuopen {
    background: var(--yap-paper-s2);
  }
  .orow:focus-visible,
  .omore:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  .oicon {
    flex: 0 0 auto;
    width: 15px;
    height: 15px;
    color: var(--yap-paper-muted);
  }
  .oname {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .odir {
    color: var(--yap-paper-muted);
  }
  .olabel {
    flex: 0 0 auto;
    color: var(--yap-paper-muted);
    font-size: 11.5px;
  }
  .omore {
    margin: 2px 0 0 6px;
    padding: 2px 0;
    border: none;
    background: none;
    color: var(--yap-paper-muted);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }
  .omore:hover {
    color: var(--yap-paper-ink);
  }
</style>
