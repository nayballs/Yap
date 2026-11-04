<script>
  // One look for every jump to another page or list (2026-10-22 review):
  // the label and one grey line, the current value in grey, a chevron, and
  // the whole row clickable — "Dictionary · 12 corrections · 3 snippets ›".
  //
  //   value:   what it's set to now, in grey before the chevron.
  //   extra:   an optional snippet before the value (the call apps' icons).
  //   setting: the row's search/deep-link anchor ([data-setting]).
  let { label, desc = '', value = '', setting = undefined, disabled = false, onclick, extra } = $props();
</script>

<button type="button" class="linkrow" data-setting={setting} {disabled} {onclick}>
  <span class="lhs">
    <span class="label">{label}</span>
    {#if desc}<span class="desc">{desc}</span>{/if}
  </span>
  <span class="rhs">
    {@render extra?.()}
    {#if value}<span class="value">{value}</span>{/if}
    <svg class="chev" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m9 6 6 6-6 6" /></svg>
  </span>
</button>

<style>
  .linkrow {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    column-gap: 24px;
    width: 100%;
    min-height: 54px;
    padding: 12px 0;
    margin: 0;
    box-sizing: border-box;
    border: none;
    border-radius: 0;
    background: none;
    font: inherit;
    text-align: left;
    color: var(--yap-fg);
    cursor: pointer;
    scroll-margin-top: 16px;
    transition: background var(--yap-dur) ease;
  }
  /* The hover wash runs 8 px past the text on both sides (shadows, so
     the row's text stays in line with the rows around it). */
  .linkrow:hover:not(:disabled) {
    background: var(--yap-raised-soft);
    box-shadow:
      -8px 0 0 var(--yap-raised-soft),
      8px 0 0 var(--yap-raised-soft);
  }
  .linkrow:hover:not(:disabled) .chev {
    color: var(--yap-fg);
  }
  .linkrow:focus-visible {
    outline: none;
    border-radius: 2px;
    box-shadow: var(--yap-focus);
  }
  .linkrow:disabled {
    cursor: default;
    opacity: 0.5;
  }
  .lhs {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .label {
    font-size: 13.5px;
    font-weight: 560;
    line-height: 1.4;
  }
  .desc {
    font-size: 12.5px;
    line-height: 1.45;
    color: var(--yap-muted);
  }
  .rhs {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .value {
    font-size: 12.5px;
    color: var(--yap-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 260px;
  }
  .chev {
    flex: 0 0 auto;
    width: 15px;
    height: 15px;
    color: var(--yap-fg-62);
  }
</style>
