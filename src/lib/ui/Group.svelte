<script>
  let { title = '', children } = $props();
  // Named after its heading, so assistive tech (and the e2e tests) can find
  // a group by its title.
  const id = $props.id();
</script>

<div class="group" role={title ? 'group' : undefined} aria-labelledby={title ? id : undefined}>
  {#if title}<h2 {id}>{title}</h2>{/if}
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
