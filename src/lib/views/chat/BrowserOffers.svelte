<script>
  // The buttons an answer offers (chat/pages.rs `offer_browser`): "Open the
  // interactive map in Brave" — the page in the person's own browser, the one
  // Settings → General → Links chose (`open_external`). Chat never opens a
  // browser itself: the person clicks. Its own small chunk (ChatMessage loads
  // it for an answer that has some).
  import { openExternalLink } from '../../core/externalLinks.js';
  import { useBrowserName } from '../../stores/browserName.svelte.js';

  /** `offers`: the answer's `BrowserOffer`s (`{url, why, site}`). */
  let { offers } = $props();

  const browser = useBrowserName();
  const words = (o) => `Open ${o.why || 'the page'} in ${browser.label}`;
</script>

<div class="offers" role="group" aria-label="Open in your browser">
  {#each offers as o (o.url)}
    <button type="button" class="offer" data-testid="chat-offer" onclick={() => openExternalLink(o.url)} title={o.url}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M14 4h6v6M20 4l-9 9" /><path d="M18 14v5H5V6h5" /></svg>
      <span class="offertext">{words(o)}</span>
      {#if o.site}<span class="offersite">{o.site}</span>{/if}
    </button>
  {/each}
</div>

<style>
  .offers {
    align-self: flex-start;
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    max-width: 78%;
  }
  .offer {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    box-sizing: border-box;
    max-width: min(420px, 100%);
    height: 32px;
    padding: 0 14px 0 11px;
    border: 1px solid var(--yap-primary-line);
    border-radius: 999px;
    background: var(--yap-s1);
    color: var(--yap-fg);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .offer:hover {
    background: var(--yap-s2);
  }
  .offer:focus-visible {
    outline: 2px solid var(--yap-primary-line);
    outline-offset: 1px;
  }
  .offer svg {
    flex: 0 0 auto;
    width: 14px;
    height: 14px;
    color: var(--yap-primary);
  }
  .offertext {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .offersite {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 500;
    color: var(--yap-muted);
  }
</style>
