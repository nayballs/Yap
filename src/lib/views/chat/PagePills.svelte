<script>
  // The live pages an answer brought up beside the chat (chat/pages.rs
  // `show_page`): a pill each under it, beside its documents' and pictures'
  // — the page's title and site; a click opens its Page tab in the panel. Its
  // own small chunk (ChatMessage loads it for an answer that has some).

  /** `pages`: the answer's `ShownPage`s; `onpage(p)` opens one in the panel. */
  let { pages, onpage = null } = $props();

  const nameOf = (p) => p?.title || p?.site || 'Page';
</script>

{#each pages as p (p.url)}
  <button type="button" class="pagepill" data-testid="chat-page-pill" onclick={() => onpage?.(p)} aria-label={`Open the page ${nameOf(p)} beside the chat`} title={p.url}>
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="9" /><path d="M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18" /></svg>
    <span class="pagename">{nameOf(p)}</span>
    {#if p.site && p.site !== nameOf(p)}<span class="pagesite">{p.site}</span>{/if}
    <span class="pageopen" aria-hidden="true">· Open</span>
  </button>
{/each}

<style>
  .pagepill {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    box-sizing: border-box;
    max-width: min(340px, 100%);
    height: 32px;
    padding: 0 12px 0 10px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    background: var(--yap-s1);
    color: var(--yap-fg);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .pagepill:hover {
    border-color: var(--yap-border-hover);
    background: var(--yap-s2);
  }
  .pagepill:focus-visible {
    outline: 2px solid var(--yap-primary-line);
    outline-offset: 1px;
  }
  .pagepill svg {
    flex: 0 0 auto;
    width: 15px;
    height: 15px;
    color: var(--yap-primary);
  }
  .pagename {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pagesite {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 500;
    color: var(--yap-muted);
  }
  .pageopen {
    flex: 0 0 auto;
    color: var(--yap-primary);
  }
</style>
