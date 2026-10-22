<script>
  import Tooltip from './Tooltip.svelte';
  // A switch, with its label and one grey line when it's a row of its own
  // (inside a bare <Row>). `desc` renders as a muted line under the label;
  // `hint` keeps the ⓘ tooltip for help that changes a decision; `badge` is
  // a small chip after the label ("Beta").
  //
  // Ink when on (Wispr's settings), amber kept for keycaps, the active page
  // and focus. Off, it's an outlined track (Windows 11's switch): its edge
  // is 3.4:1 against the page, so an off switch never reads as disabled.
  // `ariaLabel`: the switch's name when its label is drawn elsewhere (a
  // Row's label, a switch beside a slider).
  let { checked = $bindable(false), label = '', hint = '', desc = '', badge = '', disabled = false, ariaLabel = '', onchange } = $props();

  function toggle() {
    if (disabled) return;
    checked = !checked;
    onchange?.(checked);
  }
</script>

<div class="toggle-row" class:disabled class:bare={!label}>
  {#if label}
    <span class="lhs">
      <span class="ldesc">
        <span class="label">{label}</span>
        {#if badge}<span class="badge">{badge}</span>{/if}
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
    aria-label={ariaLabel || label}
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
    gap: 28px;
    width: 100%;
  }
  .toggle-row.bare {
    display: inline-flex;
    width: auto;
  }
  .toggle-row.disabled .lhs,
  .toggle-row.disabled .switch-btn {
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
    gap: 7px;
    min-width: 0;
  }
  .label {
    color: var(--yap-fg);
    font-size: 13.5px;
    font-weight: 560;
    line-height: 1.4;
  }
  .badge {
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--yap-link);
    background: var(--yap-primary-wash);
    padding: 1px 6px;
    border-radius: 4px;
    white-space: nowrap;
  }
  .desc {
    color: var(--yap-muted);
    font-size: 12.5px;
    line-height: 1.45;
  }
  .switch-btn {
    flex: 0 0 auto;
    display: inline-flex;
    background: none;
    border: none;
    padding: 0;
    border-radius: var(--yap-r-full);
    cursor: pointer;
  }
  .switch-btn:focus-visible {
    outline: none;
    box-shadow: var(--yap-focus);
  }
  .toggle-row.disabled .switch-btn {
    cursor: default;
  }
  .switch {
    display: block;
    position: relative;
    width: 36px;
    height: 20px;
    box-sizing: border-box;
    border-radius: var(--yap-r-full);
    border: 1.5px solid var(--yap-switch-edge);
    background: var(--yap-s2);
    transition:
      background var(--yap-dur) ease,
      border-color var(--yap-dur) ease;
  }
  .switch-btn:hover:not(:disabled) .switch:not(.on) {
    border-color: var(--yap-muted);
    background: var(--yap-s3);
  }
  .switch.on {
    border-color: var(--yap-ink);
    background: var(--yap-ink);
  }
  .switch-btn:hover:not(:disabled) .switch.on {
    border-color: var(--yap-ink-hover);
    background: var(--yap-ink-hover);
  }
  .knob {
    position: absolute;
    top: 50%;
    left: 3px;
    width: 11px;
    height: 11px;
    border-radius: 50%;
    background: var(--yap-switch-edge);
    transform: translateY(-50%);
    transition:
      transform var(--yap-dur) ease,
      width var(--yap-dur) ease,
      height var(--yap-dur) ease,
      background var(--yap-dur) ease;
  }
  .switch.on .knob {
    width: 13px;
    height: 13px;
    background: #fff;
    transform: translate(14px, -50%);
  }
</style>
