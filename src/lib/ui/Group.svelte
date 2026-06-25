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
    margin-bottom: 22px;
  }
  h2 {
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.6px;
    color: #60a5fa;
    margin: 0 0 8px;
  }
  .card {
    border: 1px solid #2a2f3a;
    border-radius: 10px;
    background: #161922;
    overflow: hidden;
  }
  /* Divided rows: each direct child gets a top border except the first. */
  .card > :global(* + *) {
    border-top: 1px solid #2a2f3a;
  }
</style>
