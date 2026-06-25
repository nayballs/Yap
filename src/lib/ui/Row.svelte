<script>
  // A setting row inside a Group. If `children` (the control) is provided it
  // renders on the right of the label; otherwise the whole row is the slot
  // (e.g. a Toggle/Slider that draws its own label). The `hint` shows as a ⓘ
  // tooltip next to the label rather than always-on text underneath.
  import Tooltip from './Tooltip.svelte';
  let { label = '', hint = '', children } = $props();
</script>

<div class="row" class:bare={!label}>
  {#if label}
    <span class="ldesc">
      <span class="label">{label}</span>
      {#if hint}<Tooltip text={hint} />{/if}
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
    gap: 14px;
    padding: 10px 14px;
  }
  .row.bare {
    display: block;
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
  .control {
    flex: 0 0 auto;
  }
</style>
