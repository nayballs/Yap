<script>
  // A setting row inside a Group. If `children` (the control) is provided it
  // renders on the right of the label; otherwise the whole row is the slot
  // (e.g. a Toggle/Slider that draws its own label). The `hint` shows as a ⓘ
  // tooltip next to the label rather than always-on text underneath.
  import Tooltip from './Tooltip.svelte';
  // `desc` renders as a muted line under the label (nicer than a tooltip for
  // short explanations); `hint` keeps the ⓘ tooltip for longer help text.
  let { label = '', hint = '', desc = '', children } = $props();
</script>

<div class="row" class:bare={!label}>
  {#if label}
    <span class="lhs">
      <span class="ldesc">
        <span class="label">{label}</span>
        {#if hint}<Tooltip text={hint} />{/if}
      </span>
      {#if desc}<span class="desc">{desc}</span>{/if}
    </span>
    <span class="control">{@render children?.()}</span>
  {:else}
    {@render children?.()}
  {/if}
</div>

<style>
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 12px 16px;
  }
  .row.bare {
    display: block;
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
  .control {
    flex: 0 0 auto;
  }
</style>
