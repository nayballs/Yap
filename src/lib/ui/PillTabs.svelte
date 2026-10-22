<script>
  // Pill-shaped tab bar, ported from OpenWhispr's ProviderTabs: small rounded
  // pills, the selected one white with an ink edge; optional brand icon.
  //   options: [{ value, label, icon?, mono? }]
  //   renderIcon: optional snippet (value) => inline <svg>, drawn before the label
  //   (takes precedence over an option's `icon` image — used for the scope bubbles).
  let { value = $bindable(''), options = [], onchange, renderIcon } = $props();

  function pick(v) {
    if (v === value) return;
    value = v;
    onchange?.(v);
  }
</script>

<div class="tabs" role="tablist">
  {#each options as o (o.value)}
    <button
      type="button"
      role="tab"
      class="pill"
      class:sel={value === o.value}
      aria-selected={value === o.value}
      onclick={() => pick(o.value)}
    >
      {#if renderIcon}<span class="picon svgicon">{@render renderIcon(o.value)}</span>{:else if o.icon}<img class="picon" class:mono={o.mono} src={o.icon} alt="" aria-hidden="true" />{/if}
      {o.label}
    </button>
  {/each}
</div>

<style>
  .tabs {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
  }
  .pill {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 28px;
    padding: 0 11px;
    border: none;
    border-radius: var(--yap-r-full);
    background: transparent;
    box-shadow: inset 0 0 0 1px var(--yap-border);
    color: var(--yap-fg-62);
    font: inherit;
    font-size: 12px;
    font-weight: 560;
    white-space: nowrap;
    cursor: pointer;
    transition:
      background var(--yap-dur) ease,
      color var(--yap-dur) ease,
      box-shadow var(--yap-dur) ease;
  }
  .pill:hover {
    color: var(--yap-fg);
    background: var(--yap-s3);
  }
  /* The picked one: white and outlined in ink (amber is for keycaps, the
     active page and focus). */
  .pill.sel {
    color: var(--yap-fg);
    background: var(--yap-s2);
    box-shadow: inset 0 0 0 1px var(--yap-fg-62), var(--yap-shadow-sm);
  }
  .pill:focus-visible {
    outline: none;
    box-shadow: inset 0 0 0 1px var(--yap-border), var(--yap-focus);
  }
  .pill.sel:focus-visible {
    box-shadow: inset 0 0 0 1px var(--yap-fg-62), var(--yap-focus);
  }
  .picon {
    width: 13px;
    height: 13px;
  }
  .picon.mono {
    filter: none;
  }
  :global([data-yap-theme='dark']) .picon.mono {
    filter: invert(1);
  }
  .picon.svgicon {
    display: inline-flex;
    align-items: center;
  }
  .picon.svgicon :global(svg),
  .picon.svgicon :global(img) {
    width: 13px;
    height: 13px;
  }
</style>
