<script>
  // Commands and the Voice Assistant: what the page does, in one example,
  // and the way to the other one (they're easy to mix up).
  //
  //   children: the example (a sentence or two, quotes in <q>).
  //   ask:      the line before the link ("Want an answer you can hear instead?").
  //   to:       the other page's name; `onclick` opens it.
  let { children, ask, to, onclick } = $props();
</script>

<div class="explain" role="note">
  <p class="example">
    {@render children()}
  </p>
  <p class="crossref">
    {ask}
    <button class="set-link" {onclick}>{to}</button>
  </p>
</div>

<style>
  /* Commands / Voice Assistant: what the page does, in one example, and the
     way to the other one (they're easy to mix up). */
  .explain {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin: -8px 0 28px;
    padding: 12px 14px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg);
    background: var(--yap-raised-soft);
  }
  .explain p {
    margin: 0;
    font-size: 13px;
    line-height: 1.55;
  }
  .explain .example {
    color: var(--yap-fg);
  }
  .explain :global(q) {
    font-style: italic;
  }
  .explain .crossref {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px 8px;
    color: var(--yap-muted);
  }
</style>
