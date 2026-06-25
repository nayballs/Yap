<script>
  let { title = '', children } = $props();
</script>

<div class="group">
  {#if title}<h2>{title}</h2>{/if}
  <div class="card">
    {@render children?.()}
  </div>
</div>

<style>
  .group {
    margin-bottom: 26px;
  }
  h2 {
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: #6b7280;
    margin: 0 0 10px;
    padding-left: 2px;
  }
  .card {
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 12px;
    background: #161922;
    overflow: hidden;
  }
  /* Divided rows: each direct child gets a top border except the first. */
  .card > :global(* + *) {
    border-top: 1px solid rgba(255, 255, 255, 0.06);
  }
</style>
