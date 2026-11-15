<script>
  // **A page Yap keeps, in the thread** (chat/made/pages.rs, 15 Nov 2026; after the Claude app's artifact
  // card, "Published ›" — Yap's are kept on this PC, never published): the page's title, whether this
  // answer saved it or a new version of it, and › to open it beside the chat. One card a version, in the
  // answer that saved it.

  /** `page`: the page (`Made`); `version`: this card's (`{n, title}`); `onopen(page, n)`. */
  let { page, version, onopen = null } = $props();
  const n = $derived(version?.n ?? 1);
  const state = $derived(n > 1 ? `Version ${n}` : 'Saved');
  const title = $derived(version?.title || page.title || 'Page');
</script>

<button
  type="button"
  class="pagecard"
  data-testid="chat-page-card"
  data-page-id={page.id}
  data-version={n}
  onclick={() => onopen?.(page, n)}
  aria-label={`Open ${title}, ${state.toLowerCase()}, beside the chat`}
  title="Open it beside the chat"
>
  <svg class="picon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="8" y="3" width="13" height="13" rx="2" /><path d="M16 16v3a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-9a2 2 0 0 1 2-2h3" /></svg>
  <span class="ptitle">{title}</span>
  <span class="pstate">{state}</span>
  <svg class="pgo" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 6l6 6-6 6" /></svg>
</button>

<style>
  .pagecard {
    display: inline-flex;
    align-items: center;
    gap: 10px;
    max-width: min(100%, 420px);
    margin: 4px 0 10px;
    padding: 8px 10px 8px 12px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r, 10px);
    background: var(--yap-s2);
    color: var(--yap-fg);
    font: inherit;
    font-size: 13.5px;
    text-align: left;
    cursor: pointer;
  }
  .pagecard:hover {
    border-color: var(--yap-border);
    background: var(--yap-s3);
  }
  .pagecard:focus-visible {
    outline: 2px solid var(--yap-focus-ring, var(--yap-primary));
    outline-offset: 1px;
  }
  .picon,
  .pgo {
    flex: none;
    width: 15px;
    height: 15px;
    color: var(--yap-muted);
  }
  .ptitle {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 500;
  }
  .pstate {
    flex: none;
    margin-left: 18px;
    color: var(--yap-muted);
    font-size: 12.5px;
  }
</style>
