<script>
  import Tooltip from './Tooltip.svelte';
  let {
    value = $bindable(0),
    min = 0,
    max = 1,
    step = 0.1,
    label = '',
    hint = '',
    tip = '',
    disabled = false,
    format = null,
    oninput,
  } = $props();

  // `hint` is the value readout on the right (e.g. "85%"); `tip` is an optional
  // ⓘ description next to the label.
  const readout = $derived(format ? format(value) : `${value}`);

  function handle() {
    oninput?.(value);
  }
</script>

<div class="slider-row" class:disabled>
  <div class="head">
    <span class="lbl">
      {#if label}<span class="label">{label}</span>{/if}
      {#if tip}<Tooltip text={tip} />{/if}
    </span>
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
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }
  .lbl {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .label {
    color: #e5e7eb;
  }
  .readout {
    color: #9ca3af;
    font-size: 11.5px;
    font-variant-numeric: tabular-nums;
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
