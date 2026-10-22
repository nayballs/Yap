<script>
  // A setting row inside a Group: its label and one grey line on the left,
  // the control right-aligned in one column. Without `label` the whole row
  // is the slot (a Toggle or Slider that draws its own label).
  //
  // When the control is wider than ~40% of the row (a long segmented
  // control, a select with a button, a narrow window), it goes on its own
  // line under the label (OpenWhispr's isCompact, per row): measured, so it
  // follows the window and the control's own width. `stack` forces it.
  //
  //   desc:    a string, or a snippet (for a line that's more than text).
  //   setting: the row's search/deep-link anchor ([data-setting]).
  //   hint:    an ⓘ tooltip, only where it changes a decision.
  //   badge:   a small chip after the label ("Beta").
  import Tooltip from './Tooltip.svelte';
  let { label = '', hint = '', desc = '', badge = '', setting = undefined, stack = false, children } = $props();

  let rowEl = $state(null);
  let ctlEl = $state(null);
  let narrow = $state(false);

  $effect(() => {
    if (!label || stack || !rowEl || !ctlEl || typeof ResizeObserver === 'undefined') return;
    let frame = 0;
    const measure = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        const row = rowEl?.clientWidth ?? 0;
        const ctl = ctlEl?.scrollWidth ?? 0;
        narrow = row > 0 && ctl > row * 0.42;
      });
    };
    const ro = new ResizeObserver(measure);
    ro.observe(rowEl);
    ro.observe(ctlEl);
    measure();
    return () => {
      cancelAnimationFrame(frame);
      ro.disconnect();
    };
  });
</script>

<div class="row" class:bare={!label} class:stacked={!!label && (stack || narrow)} data-setting={setting} bind:this={rowEl}>
  {#if label}
    <div class="lhs">
      <div class="ldesc">
        <span class="label">{label}</span>
        {#if badge}<span class="badge">{badge}</span>{/if}
        {#if hint}<Tooltip text={hint} />{/if}
      </div>
      {#if desc}<div class="desc">{#if typeof desc === 'function'}{@render desc()}{:else}{desc}{/if}</div>{/if}
    </div>
    <div class="control">
      <div class="ctl-in" bind:this={ctlEl}>{@render children?.()}</div>
    </div>
  {:else}
    {@render children?.()}
  {/if}
</div>

<style>
  .row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    column-gap: 28px;
    row-gap: 10px;
    min-height: 54px;
    padding: 12px 0;
    box-sizing: border-box;
    scroll-margin-top: 16px;
  }
  .row.bare {
    display: flex;
    flex-direction: column;
    justify-content: center;
    align-items: stretch;
  }
  .row.stacked {
    grid-template-columns: minmax(0, 1fr);
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
  .control {
    display: flex;
    justify-content: flex-end;
    min-width: 0;
  }
  .stacked .control {
    justify-content: flex-start;
  }
  .ctl-in {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    max-width: 100%;
  }
</style>
