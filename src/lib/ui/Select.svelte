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
    color: #e5e7eb;
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
    height: 34px;
    background: #181b22;
    border: 1px solid #2a2f3a;
    border-radius: 8px;
    color: #e5e7eb;
    padding: 0 30px 0 10px;
    font: inherit;
    font-size: 13px;
    cursor: pointer;
    transition: border-color 0.15s ease, box-shadow 0.15s ease;
  }
  select:focus {
    outline: none;
    border-color: #3b82f6;
    box-shadow: 0 0 0 3px rgba(59, 130, 246, 0.18);
  }
  select:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .chev {
    position: absolute;
    right: 9px;
    top: 50%;
    transform: translateY(-50%);
    color: #9ca3af;
    pointer-events: none;
  }
  .select-wrap.disabled .chev {
    opacity: 0.5;
  }
</style>
