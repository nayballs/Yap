<script>
  // "Search in your browser" under an answer Yap's free web sources couldn't
  // give (src-tauri/src/web/sources.rs: no search service for the chat, and
  // a question about prices, the news, reviews, timetables… or nothing
  // found). The button opens the search, on the engine chosen in Settings →
  // Voice Assistant → Web search, in the person's own browser
  // (`open_external`): they do the searching, and Yap never reads the
  // results. One line under it says how Yap could search the web itself:
  // a cloud model has its provider's search too; a model on this PC or the
  // person's own server only a key.
  //
  //   search: the answer's `browserSearch` ({ query, url, engine }).
  //   model:  the answer's model ({ provider, … }), if known.
  import { emit } from '@tauri-apps/api/event';
  import { openExternalLink } from '../../core/externalLinks.js';
  import { isKeyed } from '../../core/providers.js';

  let { search, model = null } = $props();

  const how = $derived(
    isKeyed(model?.provider) ? 'Use your AI provider’s search, or add a free Exa key.' : 'Add a free Exa or Tavily key.'
  );

  function settings() {
    emit('yap-open-settings', 'voice#web-search').catch(() => {});
  }
</script>

<div class="browsersearch" role="group" aria-label="Search the web yourself" data-testid="chat-browser-search">
  <button
    type="button"
    class="bsbtn"
    title={`Search ${search.engine} for “${search.query}” in your browser`}
    onclick={() => openExternalLink(search.url)}
  >
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <circle cx="10.5" cy="10.5" r="6.5" />
      <path d="m20 20-4.6-4.6" />
    </svg>
    Search in your browser
  </button>
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
