<script>
  import Tooltip from './Tooltip.svelte';
  // `desc` renders as a muted line under the label (preferred, OpenWhispr-style);
  // `hint` keeps the ⓘ tooltip for longer help text.
  let { checked = $bindable(false), label = '', hint = '', desc = '', disabled = false, onchange } = $props();

  function toggle() {
    if (disabled) return;
    checked = !checked;
    onchange?.(checked);
  }
</script>

<div class="toggle-row" class:disabled>
  {#if label}
    <span class="lhs">
      <span class="ldesc">
        <span class="label">{label}</span>
        {#if hint}<Tooltip text={hint} />{/if}
      </span>
      {#if desc}<span class="desc">{desc}</span>{/if}
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
  .lhs {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
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
  .desc {
    color: var(--yap-muted-70);
    font-size: 11.5px;
    line-height: 1.5;
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
    background: #fff;
    box-shadow: 0 1px 2px rgba(60, 50, 30, 0.25);
    transition: transform var(--yap-dur) ease;
  }
  .switch.on .knob {
    transform: translateX(18px);
    background: #fff;
  }
</style>
