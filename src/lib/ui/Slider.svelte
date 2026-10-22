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
    // `inline`: just the track and its readout, for a Row's control column
    // (the chime's volume beside its switch); `label` is then its name.
    inline = false,
  } = $props();

  // `hint` is the value readout on the right (e.g. "85%"); `tip` is an optional
  // ⓘ description next to the label.
  const readout = $derived(format ? format(value) : `${value}`);
  const fill = $derived(`${(((Number(value) || 0) - min) / (max - min || 1)) * 100}%`);

  function handle() {
    oninput?.(value);
  }
</script>

{#if inline}
  <span class="inline" class:disabled>
    <input
      type="range"
      {min}
      {max}
      {step}
      {disabled}
      aria-label={label || undefined}
      style="--fill: {fill}"
      bind:value
      oninput={handle}
    />
    <span class="readout">{hint || readout}</span>
  </span>
{:else}
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
      aria-label={label || undefined}
      style="--fill: {fill}"
      bind:value
      oninput={handle}
    />
  </div>
{/if}

<style>
  .slider-row {
    display: flex;
    flex-direction: column;
    gap: 8px;
    width: 100%;
  }
  .disabled {
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
    color: var(--yap-fg);
    font-size: 13.5px;
    font-weight: 560;
  }
  .readout {
    color: var(--yap-muted);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }
  .inline {
    display: inline-flex;
    align-items: center;
    gap: 10px;
  }
  .inline input {
    width: 150px;
  }
  .inline .readout {
    min-width: 34px;
    text-align: right;
  }
  /* An ink track and a white knob (amber is for keycaps and focus). */
  input[type='range'] {
    -webkit-appearance: none;
    appearance: none;
    width: 100%;
    height: 18px;
    margin: 0;
    background: transparent;
    cursor: pointer;
  }
  input[type='range']::-webkit-slider-runnable-track {
    height: 4px;
    border-radius: 9px;
    background: linear-gradient(to right, var(--yap-ink) var(--fill), var(--yap-raised) var(--fill));
  }
  input[type='range']::-webkit-slider-thumb {
    -webkit-appearance: none;
    width: 16px;
    height: 16px;
    margin-top: -6px;
    border-radius: 50%;
    background: #fff;
    border: 1px solid var(--yap-switch-edge);
    box-shadow: var(--yap-shadow-sm);
  }
  input[type='range']:focus {
    outline: none;
  }
  input[type='range']:focus-visible::-webkit-slider-thumb {
    box-shadow: var(--yap-focus);
  }
  input[type='range']:disabled {
    cursor: default;
  }
</style>
