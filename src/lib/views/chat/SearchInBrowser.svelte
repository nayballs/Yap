<script>
  // The search under an answer Yap's free web sources couldn't give
  // (src-tauri/src/web/sources.rs: no search service for the chat, and a
  // question about prices, the news, reviews, travel… or nothing found).
  // **Search beside the chat** opens it in the panel's page tab (1 Nov 2026:
  // one click, no app switch); **Open in <browser>** in the person's own
  // browser (`open_external`). Either way it's on the engine chosen in
  // Settings → Voice Assistant → Web search: they do the searching, and Yap
  // never reads the results. One line under it says how Yap could search the
  // web itself: a cloud model has its provider's search too; a model on this
  // PC or the person's own server only a key.
  //
  //   search: the answer's `browserSearch` ({ query, url, engine }).
  //   model:  the answer's model ({ provider, … }), if known.
  //   onpage: opens a page beside the chat (ChatMessage's); none: the browser.
  import { emit } from '@tauri-apps/api/event';
  import { openExternalLink } from '../../core/externalLinks.js';
  import { isKeyed } from '../../core/providers.js';
  import { useBrowserName, openInLabel } from '../../stores/browserName.svelte.js';

  let { search, model = null, onpage = null } = $props();
  const browser = useBrowserName();

  const how = $derived(
    isKeyed(model?.provider) ? 'Use your AI provider’s search, or add a free Exa key.' : 'Add a free Exa or Tavily key.'
  );

  function beside() {
    if (!onpage) return openExternalLink(search.url);
    let site = '';
    try {
      site = new URL(search.url).hostname.replace(/^www\./, '');
    } catch {
      /* not an address: the browser's */
    }
    onpage({ url: search.url, title: `Search: ${search.query}`, site });
  }

  function settings() {
    emit('yap-open-settings', 'voice#web-search').catch(() => {});
  }
</script>

<div class="browsersearch" role="group" aria-label="Search the web yourself" data-testid="chat-browser-search">
  <div class="bsrow">
    <button
      type="button"
      class="bsbtn"
      title={`Search ${search.engine} for “${search.query}” beside the chat`}
      onclick={beside}
      data-testid="chat-browser-search-beside"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <circle cx="10.5" cy="10.5" r="6.5" />
        <path d="m20 20-4.6-4.6" />
      </svg>
      Search beside the chat
    </button>
    <button
      type="button"
      class="bsalt"
      title={`Search ${search.engine} for “${search.query}” in ${browser.label || 'your browser'}`}
      onclick={() => openExternalLink(search.url)}
      data-testid="chat-browser-search-outside"
    >
      {openInLabel(browser.label)}
    </button>
  </div>
  <p class="bsnote">
    Want Yap to search the web itself? {how}
    <button type="button" class="bslink" onclick={settings}>Web search settings</button>
  </p>
</div>

<style>
  .browsersearch {
    align-self: flex-start;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 6px;
    max-width: 78%;
  }
  .bsbtn {
    display: inline-flex;
    align-items: center;
    gap: 7px;
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
  .bsbtn:hover {
    border-color: var(--yap-border-hover);
    background: var(--yap-s2);
  }
  .bsbtn:focus-visible,
  .bslink:focus-visible {
    outline: 2px solid var(--yap-primary-line);
    outline-offset: 1px;
  }
  .bsbtn svg {
    flex: 0 0 auto;
    width: 15px;
    height: 15px;
    color: var(--yap-primary);
  }
  .bsrow {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
  }
  /* Your own browser: the second choice, quieter. */
  .bsalt {
    height: 32px;
    padding: 0 10px;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-link);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .bsalt:hover {
    background: var(--yap-s1);
  }
  .bsalt:focus-visible {
    outline: 2px solid var(--yap-primary-line);
    outline-offset: 1px;
  }
  .bsnote {
    margin: 0;
    font-size: 12px;
    line-height: 1.5;
    color: var(--yap-muted);
  }
  .bslink {
    padding: 0;
    border: none;
    background: none;
    color: var(--yap-link);
    font: inherit;
    font-weight: 600;
    cursor: pointer;
    text-decoration: underline;
    text-decoration-color: color-mix(in srgb, var(--yap-link) 40%, transparent);
    text-underline-offset: 2px;
  }
  .bslink:hover {
    text-decoration-color: currentColor;
  }
</style>
