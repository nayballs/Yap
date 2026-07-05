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
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.09em;
    color: var(--yap-muted-55);
    margin: 0 0 9px;
    padding-left: 2px;
  }
  .card {
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
    overflow: hidden;
  }
  /* Divided rows: each direct child gets a hairline top border except the first. */
  .card > :global(* + *) {
    border-top: 1px solid var(--yap-border-subtle);
  }
</style>
