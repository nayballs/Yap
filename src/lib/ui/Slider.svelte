<script>
  let {
    value = $bindable(0),
    min = 0,
    max = 1,
    step = 0.1,
    label = '',
    hint = '',
    disabled = false,
    format = null,
    oninput,
  } = $props();

  const readout = $derived(format ? format(value) : `${value}`);

  function handle() {
    oninput?.(value);
  }
</script>

<div class="slider-row" class:disabled>
  <div class="head">
    {#if label}<span class="label">{label}</span>{/if}
    <span class="readout">{hint || readout}</span>
  </div>
  <input
    type="range"
    {min}
    {max}
    {step}
    {disabled}
    bind:value
    oninput={handle}
  />
</div>

<style>
  .slider-row {
    display: flex;
    flex-direction: column;
    gap: 8px;
    width: 100%;
  }
  .slider-row.disabled {
    opacity: 0.5;
  }
  .head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 12px;
  }
  .label {
    color: #e5e7eb;
  }
  .readout {
    color: #6b7280;
    font-size: 11px;
  }
  input[type='range'] {
    width: 100%;
    accent-color: #3b82f6;
    cursor: pointer;
  }
  input[type='range']:disabled {
    cursor: default;
  }
</style>
