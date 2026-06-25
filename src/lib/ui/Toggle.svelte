<script>
  import Tooltip from './Tooltip.svelte';
  let { checked = $bindable(false), label = '', hint = '', disabled = false, onchange } = $props();

  function toggle() {
    if (disabled) return;
    checked = !checked;
    onchange?.(checked);
  }
</script>

<div class="toggle-row" class:disabled>
  {#if label}
    <span class="ldesc">
      <span class="label">{label}</span>
      {#if hint}<Tooltip text={hint} />{/if}
    </span>
  {/if}
  <button
    type="button"
    class="switch-btn"
    onclick={toggle}
    aria-pressed={checked}
    aria-label={label}
    {disabled}
  >
    <span class="switch" class:on={checked}><span class="knob"></span></span>
  </button>
</div>

<style>
  .toggle-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 14px;
    width: 100%;
  }
  .toggle-row.disabled {
    opacity: 0.5;
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
  .switch-btn {
    flex: 0 0 auto;
    background: none;
    border: none;
    padding: 0;
    cursor: pointer;
  }
  .toggle-row.disabled .switch-btn {
    cursor: default;
  }
  .switch {
    display: block;
    position: relative;
    width: 38px;
    height: 22px;
    border-radius: 999px;
    background: #2a2f3a;
    transition: background 0.18s ease;
  }
  .switch.on {
    background: #3b82f6;
  }
  .knob {
    position: absolute;
    top: 3px;
    left: 3px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: #e5e7eb;
    transition: transform 0.18s ease;
  }
  .switch.on .knob {
    transform: translateX(16px);
  }
</style>
