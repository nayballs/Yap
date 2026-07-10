<script>
  // A segmented control with an animated sliding indicator — for small, equal
  // "pick one" choices (e.g. Toggle vs Push-to-talk). Segments share equal
  // width so the thumb can slide by simple percentage, no measuring.
  //
  //   options: [{ value, label }]
  //   icon:    optional snippet (value) => inline <svg>
  let { value = $bindable(), options = [], icon } = $props();

  const index = $derived(Math.max(0, options.findIndex((o) => o.value === value)));
  const n = $derived(options.length || 1);
</script>

<div class="seg" role="tablist" style="--n:{n}">
  <span class="thumb" style="transform: translateX({index * 100}%)"></span>
  {#each options as o (o.value)}
    <button
      type="button"
      role="tab"
      aria-selected={value === o.value}
      class="seg-btn"
      class:on={value === o.value}
      onclick={() => (value = o.value)}
    >
      {@render icon?.(o.value)}
      <span>{o.label}</span>
    </button>
  {/each}
</div>

<style>
  .seg {
    position: relative;
    /* Equal-width columns (widest label wins) so the thumb's simple
       percentage slide always lands exactly on the segment boundary —
       flex:1 couldn't shrink a long nowrap label below its text width. */
    display: inline-grid;
    grid-auto-flow: column;
    grid-auto-columns: 1fr;
    padding: 3px;
    background: var(--yap-s1);
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
  }
  .thumb {
    position: absolute;
    z-index: 0;
    top: 3px;
    bottom: 3px;
    left: 3px;
    width: calc((100% - 6px) / var(--n));
    background: var(--yap-raised);
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r-sm);
    transition: transform 0.22s cubic-bezier(0.3, 0.7, 0.3, 1);
  }
  .seg-btn {
    position: relative;
    z-index: 1;
    border: 0;
    background: transparent;
    padding: 5px 13px;
    font: inherit;
    font-size: 12px;
    font-weight: 500;
    color: var(--yap-muted);
    cursor: pointer;
    border-radius: var(--yap-r-sm);
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    white-space: nowrap;
    transition: color var(--yap-dur) ease;
  }
  .seg-btn.on {
    color: var(--yap-fg);
  }
  .seg-btn :global(svg) {
    width: 13px;
    height: 13px;
  }
  @media (prefers-reduced-motion: reduce) {
    .thumb {
      transition: none;
    }
  }
</style>
