<script>
  // A setting row inside a Group. If `children` (the control) is provided it
  // renders on the right of the label/hint; otherwise the whole row is the
  // slot (e.g. a Toggle/Slider that draws its own label).
  let { label = '', hint = '', children } = $props();
</script>

<div class="row" class:bare={!label}>
  {#if label}
    <span class="ldesc">
      <span class="label">{label}</span>
      {#if hint}<span class="hint">{hint}</span>{/if}
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
    padding: 11px 14px;
  }
  .row.bare {
    display: block;
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
  .control {
    flex: 0 0 auto;
  }
</style>
