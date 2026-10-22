<script>
  // A segmented control with a sliding indicator — for two or three short
  // "pick one" choices (Toggle / Push-to-talk). Each segment is as wide as
  // its own label (the thumb is measured, not a fixed share), so a short
  // choice doesn't stretch to the longest one.
  //
  //   options:  [{ value, label }]
  //   icon:     optional snippet (value) => inline <svg>
  //   disabled: greyed out, not clickable
  //   label:    accessible name for the group
  let { value = $bindable(), options = [], icon, disabled = false, label = undefined } = $props();

  let btns = $state([]);
  let thumb = $state({ x: 0, w: 0 });
  const index = $derived(Math.max(0, options.findIndex((o) => o.value === value)));

  function place() {
    const b = btns[index];
    if (b) thumb = { x: b.offsetLeft, w: b.offsetWidth };
  }
  $effect(() => {
    // Re-place when the choice, the labels or the control's size change.
    index;
    options;
    place();
    const b = btns[index];
    if (!b || typeof ResizeObserver === 'undefined') return;
    const ro = new ResizeObserver(place);
    ro.observe(b.parentElement);
    return () => ro.disconnect();
  });
</script>

<div class="seg" class:disabled role="tablist" aria-label={label}>
  <span class="thumb" class:ready={thumb.w > 0} style="transform: translateX({thumb.x}px); width: {thumb.w}px"></span>
  {#each options as o, i (o.value)}
    <button
      type="button"
      role="tab"
      aria-selected={value === o.value}
      class="seg-btn"
      class:on={value === o.value}
      {disabled}
      bind:this={btns[i]}
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
    display: inline-flex;
    padding: 2px;
    background: var(--yap-raised-soft);
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
  }
  .thumb {
    position: absolute;
    z-index: 0;
    top: 2px;
    bottom: 2px;
    left: 0;
    opacity: 0;
    background: var(--yap-s2);
    border-radius: 6px;
    box-shadow: var(--yap-shadow-sm), 0 0 0 1px var(--yap-border-subtle);
    transition:
      transform 0.22s cubic-bezier(0.3, 0.7, 0.3, 1),
      width 0.22s cubic-bezier(0.3, 0.7, 0.3, 1);
  }
  .thumb.ready {
    opacity: 1;
  }
  .seg-btn {
    position: relative;
    z-index: 1;
    border: 0;
    background: transparent;
    padding: 5px 12px;
    font: inherit;
    font-size: 12px;
    font-weight: 560;
    color: var(--yap-muted);
    cursor: pointer;
    border-radius: 6px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    white-space: nowrap;
    transition: color var(--yap-dur) ease;
  }
  .seg-btn:hover:not(:disabled):not(.on) {
    color: var(--yap-fg);
  }
  /* (Same weight on and off: a bolder label would change its width.) */
  .seg-btn.on {
    color: var(--yap-fg);
  }
  .seg-btn:focus-visible {
    outline: none;
    box-shadow: var(--yap-focus);
  }
  .seg.disabled {
    opacity: 0.5;
  }
  .seg.disabled .seg-btn {
    cursor: default;
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
