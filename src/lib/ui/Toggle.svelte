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
    color: var(--yap-fg);
    font-size: 12.5px;
    font-weight: 500;
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
    width: 42px;
    height: 24px;
    border-radius: var(--yap-r-full);
    background: var(--yap-raised);
    transition: background var(--yap-dur) ease;
  }
  .switch.on {
    background: var(--yap-primary);
  }
  .knob {
    position: absolute;
    top: 3px;
    left: 3px;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background: #d9dade;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.4);
    transition: transform var(--yap-dur) ease;
  }
  .switch.on .knob {
    transform: translateX(18px);
    background: #fff;
  }
</style>
