<script>
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
      {#if hint}<span class="hint">{hint}</span>{/if}
    </span>
  {/if}
  <select bind:value {disabled} onchange={() => onchange?.(value)}>
    {#each options as o (o.value)}
      <option value={o.value}>{o.label}</option>
    {/each}
  </select>
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
    flex-direction: column;
    min-width: 0;
  }
  .label {
    color: #e5e7eb;
  }
  .hint {
    color: #6b7280;
    font-size: 11px;
    margin-top: 1px;
  }
  select {
    flex: 0 0 auto;
    max-width: 260px;
    background: #181b22;
    border: 1px solid #2a2f3a;
    border-radius: 6px;
    color: #e5e7eb;
    padding: 6px 8px;
    font-size: 13px;
    cursor: pointer;
  }
  select:focus {
    outline: none;
    border-color: #3b82f6;
  }
  select:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
