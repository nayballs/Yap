<script>
  // Radio-dot select rows, ported from OpenWhispr's ModelCardList (cloud mode):
  // status dot → brand icon → bold label → muted inline description → Active
  // chip on the selected row. Pure selection — no download affordances.
  //   options: [{ value, label, desc?, icon?, mono? }]
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
    gap: 7px;
    padding: 8px 10px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    background: var(--yap-s1);
    font: inherit;
    text-align: left;
    cursor: pointer;
    transition:
      background var(--yap-dur) ease,
      border-color var(--yap-dur) ease;
  }
  .srow:hover {
    background: var(--yap-s2);
    border-color: var(--yap-border-hover);
  }
  .srow.sel {
    border-color: var(--yap-primary-line);
    background: var(--yap-primary-wash);
    cursor: default;
  }

  .dot {
    width: 6px;
    height: 6px;
    flex: 0 0 auto;
    border-radius: var(--yap-r-full);
    background: rgba(255, 255, 255, 0.14);
  }
  .srow.sel .dot {
    background: var(--yap-primary);
    box-shadow: 0 0 6px rgba(109, 92, 245, 0.65);
    animation: pulse-glow 2s ease-in-out infinite;
  }

  .sicon {
    width: 14px;
    height: 14px;
    flex: 0 0 auto;
  }
  .sicon.mono {
    filter: invert(1);
  }

  .slabel {
    font-size: 12.5px;
    font-weight: 600;
    letter-spacing: -0.01em;
    color: var(--yap-fg);
    white-space: nowrap;
  }
  .sdesc {
    font-size: 11px;
    color: var(--yap-fg-45);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
    flex: 0 1 auto;
  }
  .spacer {
    flex: 1 1 auto;
  }
  .stag {
    font-size: 10.5px;
    font-weight: 600;
    color: #cfc9ff;
    background: var(--yap-primary-tint);
    padding: 1px 7px;
    border-radius: var(--yap-r-sm);
    letter-spacing: 0.02em;
    white-space: nowrap;
    flex: 0 0 auto;
  }

  @keyframes pulse-glow {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.55;
    }
  }
</style>
