<script>
  // A Settings section: a plain sentence-case heading over flat rows with
  // hairlines between them (Claude's and Wispr's settings; no card around
  // them — cards are for things that are objects: a model, a connection, an
  // AI app). `desc` is an optional line under the heading; `id` makes it a
  // deep-link target ("general#bar" → #settings-bar). `bare`: no hairlines,
  // for a group that holds a card (your speech model) or its own list.
  let { title = '', desc = '', id = undefined, bare = false, children } = $props();
  // Named after its heading, so assistive tech (and the e2e tests) can find
  // a group by its title.
  const hid = $props.id();
</script>

<div class="group" {id} role={title ? 'group' : undefined} aria-labelledby={title ? hid : undefined}>
  {#if title}<h2 id={hid}>{title}</h2>{/if}
  {#if desc}<p class="gdesc">{desc}</p>{/if}
  <div class="rows" class:bare>
    {@render children?.()}
  </div>
</div>

<style>
  .group {
    margin: 0 0 30px;
    scroll-margin-top: 16px;
  }
  h2 {
    margin: 0;
    font-size: 14px;
    font-weight: 640;
    letter-spacing: -0.003em;
    line-height: 1.4;
    color: var(--yap-fg);
  }
  .gdesc {
    margin: 2px 0 0;
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--yap-muted);
  }
  .rows {
    margin-top: 8px;
    border-top: 1px solid var(--yap-rule);
  }
  /* Every row (each direct child) ends in a hairline. */
  .rows:not(.bare) > :global(*) {
    border-bottom: 1px solid var(--yap-rule);
  }
  .rows > :global(*:empty) {
    display: none;
  }
  .rows.bare {
    border-top: none;
  }
</style>
