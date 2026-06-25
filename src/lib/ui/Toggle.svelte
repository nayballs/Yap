<script>
  let { checked = $bindable(false), label = '', hint = '', disabled = false, onchange } = $props();

  function toggle() {
    if (disabled) return;
    checked = !checked;
    onchange?.(checked);
  }
</script>

<button
  type="button"
  class="toggle-row"
  class:disabled
  onclick={toggle}
  aria-pressed={checked}
  {disabled}
>
  {#if label}
    <span class="ldesc">
      <span class="label">{label}</span>
      {#if hint}<span class="hint">{hint}</span>{/if}
    </span>
  {/if}
  <span class="switch" class:on={checked}><span class="knob"></span></span>
</button>

<style>
  .toggle-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 14px;
    width: 100%;
    background: none;
    border: none;
    padding: 0;
    cursor: pointer;
    color: inherit;
    font: inherit;
    text-align: left;
  }
  .toggle-row.disabled {
    cursor: default;
    opacity: 0.5;
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
  .switch {
    flex: 0 0 auto;
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
