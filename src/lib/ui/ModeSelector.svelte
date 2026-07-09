<script>
  // A radio-list "pick one" panel — icon chip + label + description + a custom
  // radio, one option per row. Richer than a dropdown for a small set of
  // important choices (e.g. the AI-cleanup provider). Ported from OpenWhispr's
  // InferenceModeSelector.
  //
  //   options: [{ value, label, desc }]
  //   icon:    optional snippet (value) => inline <svg>, drawn in the chip
  let { value = $bindable(), options = [], icon, onchange, disabled = false } = $props();

  function pick(v) {
    if (disabled || v === value) return;
    value = v;
    onchange?.(v);
  }
</script>

<div class="modes" class:disabled>
  {#each options as o (o.value)}
    <button type="button" class="mode" class:sel={value === o.value} {disabled} onclick={() => pick(o.value)}>
      <span class="chip">{@render icon?.(o.value)}</span>
      <span class="m-body">
        <span class="m-top">
          <span class="m-name">{o.label}</span>
          {#if value === o.value}<span class="tag">Active</span>{/if}
        </span>
        {#if o.desc}<span class="m-desc">{o.desc}</span>{/if}
      </span>
      <span class="radio"></span>
    </button>
  {/each}
</div>

<style>
  .modes {
    display: flex;
    flex-direction: column;
    width: 100%;
  }
  .modes.disabled {
    opacity: 0.5;
    pointer-events: none;
  }
  .mode {
    display: flex;
    align-items: center;
    gap: 13px;
    padding: 13px 16px;
    border: 0;
    background: transparent;
    width: 100%;
    cursor: pointer;
    text-align: left;
    font: inherit;
    transition: background var(--yap-dur) ease;
  }
  .mode + .mode {
    border-top: 1px solid var(--yap-border-subtle);
  }
  .mode:hover {
    background: var(--yap-s3);
  }
  .chip {
    width: 34px;
    height: 34px;
    flex: 0 0 34px;
    border-radius: var(--yap-r);
    display: grid;
    place-items: center;
    background: var(--yap-raised);
    color: var(--yap-muted);
    transition:
      background var(--yap-dur) ease,
      color var(--yap-dur) ease;
  }
  .chip :global(svg) {
    width: 17px;
    height: 17px;
  }
  .m-body {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .m-top {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .m-name {
    font-size: 12.5px;
    font-weight: 500;
    color: var(--yap-fg);
  }
  .m-desc {
    font-size: 11.5px;
    color: var(--yap-muted-70);
    line-height: 1.5;
  }
  .tag {
    font-size: 10.5px;
    font-weight: 600;
    color: var(--yap-primary);
    background: var(--yap-primary-tint);
    padding: 1px 7px;
    border-radius: var(--yap-r-sm);
    letter-spacing: 0.02em;
  }
  .radio {
    width: 17px;
    height: 17px;
    flex: 0 0 17px;
    border-radius: var(--yap-r-full);
    border: 2px solid var(--yap-border-hover);
    position: relative;
    transition: border-color var(--yap-dur) ease, background var(--yap-dur) ease;
  }
  .mode.sel .chip {
    background: var(--yap-primary-tint);
    color: var(--yap-primary);
  }
  .mode.sel .radio {
    border-color: var(--yap-primary);
    background: var(--yap-primary);
  }
  .mode.sel .radio::after {
    content: '';
    position: absolute;
    inset: 0;
    margin: auto;
    width: 6px;
    height: 6px;
    border-radius: var(--yap-r-full);
    background: #fff;
  }
</style>
