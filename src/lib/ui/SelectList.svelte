<script>
  // Radio-dot select rows, ported from OpenWhispr's ModelCardList (cloud mode):
  // status dot → brand icon → bold label → muted inline description → Active
  // chip on the selected row. Pure selection — no download affordances.
  //   options: [{ value, label, desc?, icon?, mono?, tag? }]
  //   tag: 'new' (a model the provider added since Yap's list was made) or
  //   'gone' (the saved model, which the provider no longer offers).
  let { value = $bindable(''), options = [], onchange, disabled = false } = $props();

  function pick(v) {
    if (disabled || v === value) return;
    value = v;
    onchange?.(v);
  }
</script>

<div class="list" class:disabled>
  {#each options as o (o.value)}
    <button type="button" class="srow" class:sel={value === o.value} {disabled} onclick={() => pick(o.value)}>
      <span class="dot" aria-hidden="true"></span>
      {#if o.icon}
        <img class="sicon" class:mono={o.mono} src={o.icon} alt="" aria-hidden="true" />
      {/if}
      <span class="slabel">{o.label}</span>
      {#if o.tag === 'new'}<span class="snew">New</span>{:else if o.tag === 'gone'}<span class="sgone">No longer offered</span>{/if}
      {#if o.desc}<span class="sdesc">{o.desc}</span>{/if}
      <span class="spacer"></span>
      {#if value === o.value}<span class="stag">Active</span>{/if}
    </button>
  {/each}
</div>

<style>
  .list {
    display: flex;
    flex-direction: column;
    gap: 6px;
    width: 100%;
  }
  .list.disabled {
    opacity: 0.5;
    pointer-events: none;
  }
  .srow {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 40px;
    box-sizing: border-box;
    padding: 7px 12px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    background: var(--yap-s2);
    font: inherit;
    text-align: left;
    cursor: pointer;
    transition:
      background var(--yap-dur) ease,
      border-color var(--yap-dur) ease;
  }
  .srow:hover {
    background: var(--yap-s3);
    border-color: var(--yap-border-hover);
  }
  .srow:focus-visible {
    outline: none;
    box-shadow: var(--yap-focus);
  }
  /* The picked one: the same quiet "Active" as the speech models'. */
  .srow.sel {
    border-color: var(--yap-border-hover);
    background: var(--yap-s2);
    cursor: default;
  }

  .dot {
    width: 6px;
    height: 6px;
    flex: 0 0 auto;
    border-radius: var(--yap-r-full);
    background: var(--yap-raised);
  }
  .srow.sel .dot {
    background: var(--yap-success);
    box-shadow: 0 0 4px color-mix(in srgb, var(--yap-success) 50%, transparent);
  }

  .sicon {
    width: 14px;
    height: 14px;
    flex: 0 0 auto;
  }
  .sicon.mono {
    filter: none;
  }
  :global([data-yap-theme='dark']) .sicon.mono {
    filter: invert(1);
  }

  .slabel {
    font-size: 13px;
    font-weight: 600;
    letter-spacing: -0.01em;
    color: var(--yap-fg);
    white-space: nowrap;
  }
  .sdesc {
    font-size: 12px;
    color: var(--yap-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
    flex: 0 1 auto;
  }
  .spacer {
    flex: 1 1 auto;
  }
  .snew,
  .sgone {
    flex: 0 0 auto;
    padding: 0 6px;
    border-radius: var(--yap-r-full);
    font-size: 10px;
    font-weight: 700;
    line-height: 16px;
    letter-spacing: 0.02em;
    white-space: nowrap;
  }
  .snew {
    color: var(--yap-link);
    background: var(--yap-primary-wash);
  }
  .sgone {
    color: var(--yap-warning);
    background: color-mix(in srgb, var(--yap-warning) 14%, transparent);
  }
  .stag {
    font-size: 10.5px;
    font-weight: 600;
    color: var(--yap-success-text);
    background: color-mix(in srgb, var(--yap-success) 12%, var(--yap-s2));
    padding: 1px 7px;
    border-radius: var(--yap-r-sm);
    letter-spacing: 0.02em;
    white-space: nowrap;
    flex: 0 0 auto;
  }
</style>
