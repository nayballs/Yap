<script>
  import Tooltip from './Tooltip.svelte';
  let {
    value = $bindable(),
    options = [],
    label = '',
    hint = '',
    disabled = false,
    onchange,
  } = $props();
</script>

<div class="select-row">
  {#if label}
    <span class="ldesc">
      <span class="label">{label}</span>
      {#if hint}<Tooltip text={hint} />{/if}
    </span>
  {/if}
  <div class="select-wrap" class:disabled>
    <select bind:value {disabled} onchange={() => onchange?.(value)}>
      {#each options as o (o.value)}
        <option value={o.value}>{o.label}</option>
      {/each}
    </select>
    <svg class="chev" width="14" height="14" viewBox="0 0 24 24" fill="none"
      stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <path d="M6 9l6 6 6-6" />
    </svg>
  </div>
</div>

<style>
  .select-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 14px;
    width: 100%;
  }
  .ldesc {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .label {
    color: var(--yap-fg);
    font-size: 13px;
    font-weight: 650;
  }
  .select-wrap {
    position: relative;
    flex: 0 0 auto;
    max-width: 260px;
  }
  select {
    appearance: none;
    -webkit-appearance: none;
    width: 100%;
    height: 32px;
    background: var(--yap-s1);
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    color: var(--yap-fg);
    padding: 0 32px 0 12px;
    font: inherit;
    font-size: 12.5px;
    cursor: pointer;
    transition: border-color var(--yap-dur) ease, box-shadow var(--yap-dur) ease;
  }
  select:hover:not(:disabled) {
    border-color: var(--yap-border-hover);
  }
  select:focus {
    outline: none;
    border-color: var(--yap-primary);
    box-shadow: 0 0 0 3px var(--yap-primary-wash);
  }
  select:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .chev {
    position: absolute;
    right: 10px;
    top: 50%;
    transform: translateY(-50%);
    color: var(--yap-muted);
    pointer-events: none;
  }
  .select-wrap.disabled .chev {
    opacity: 0.5;
  }
</style>
