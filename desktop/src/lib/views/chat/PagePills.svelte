<script>
  // The live pages an answer brought up beside the chat (chat/pages.rs): a card each under it, as the Claude Code
  // app shows a page it opened (14 Nov 2026; the maintainer: "it shows a thumbnail… ours doesn't") — the top of the
  // page as it looked once loaded (chat/pages/thumb.rs; a card without one is just its words), its title and site,
  // Open (its Page tab in the panel) and ⋯ (Open in your browser, Copy link). In Chat and in a Code chat alike. Its
  // own small chunk (ChatMessage loads it for an answer that has some).
  import { placed } from './chatImages.js';
  import { screenPixels } from './chatShots.svelte.js';
  import { openContextMenu } from '../../ui/contextMenu.svelte.js';
  import { openExternalLink } from '../../core/externalLinks.js';
  import { useBrowserName, openInLabel } from '../../stores/browserName.svelte.js';
  import { toast } from '../../ui/toast.svelte.js';

  /** `pages`: the answer's `ShownPage`s; `onpage(p)` opens one in the panel; `thumbFor(url)` what was taken of it since, live (`{thumb, title}`). */
  let { pages, onpage = null, thumbFor = null } = $props();

  const browser = useBrowserName();
  /** A card's picture: CSS px, twice as wide as tall (thumb.rs `ASPECT`). */
  const CARD_W = 360;
  const CARD_H = 180;
  /** Pictures that wouldn't load (gone with "Keep chat screenshots"): their cards go on without them. */
  let broken = $state({});

  // The page's own title once it's known (one brought up unread was named by its site: thumb.rs `apply`).
  const nameOf = (p) => (p?.title && p.title !== p.site ? p.title : thumbFor?.(p?.url)?.title || p?.title || p?.site || 'Page');
  const thumbOf = (p) => p?.thumb ?? thumbFor?.(p?.url)?.thumb ?? null;

  function more(e, p) {
    openContextMenu(
      e,
      [
        { label: 'Open beside the chat', icon: 'open', onselect: () => onpage?.(p) },
        { label: openInLabel(browser.label), onselect: () => openExternalLink(p.url) },
        { separator: true },
        { label: 'Copy link', icon: 'copy', onselect: () => copyLink(p.url) },
      ],
      { label: 'Page' }
    );
  }

  async function copyLink(url) {
    try {
      await navigator.clipboard.writeText(url);
      toast({ title: 'Link copied' });
    } catch {
      /* the clipboard said no */
    }
  }
</script>

{#each pages as p (p.url)}
  {@const t = thumbOf(p)}
  {@const pic = t && !broken[t.id] ? placed(t, { maxW: CARD_W, maxH: CARD_H, minW: CARD_W, minH: CARD_H, ratio: screenPixels.ratio }) : null}
  <div class="pagecard" class:withpic={!!pic} data-testid="chat-page-card" role="group" aria-label={nameOf(p)}>
    {#if pic}
      <button type="button" class="pic" onclick={() => onpage?.(p)} aria-label={`Open the page ${nameOf(p)} beside the chat`} title={p.url} tabindex="-1">
        <img src={pic.src} alt="" draggable="false" data-testid="chat-page-thumb" onerror={() => (broken = { ...broken, [t.id]: true })} />
      </button>
    {/if}
    <div class="foot">
      <svg class="globe" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="9" /><path d="M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18" /></svg>
      <div class="words" title={p.url}>
        <span class="name">{nameOf(p)}</span>
        <span class="where">{p.site ? `${p.site}, opened beside the chat` : 'Opened beside the chat'}</span>
      </div>
      <button type="button" class="open" onclick={() => onpage?.(p)} aria-label={`Open the page ${nameOf(p)} beside the chat`} data-testid="chat-page-pill">Open</button>
      <button type="button" class="more" data-more aria-label={`More for the page ${nameOf(p)}`} title="More" aria-haspopup="menu" onclick={(e) => more(e, p)}>
        <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><circle cx="5.5" cy="12" r="1.6" /><circle cx="12" cy="12" r="1.6" /><circle cx="18.5" cy="12" r="1.6" /></svg>
      </button>
    </div>
  </div>
{/each}

<style>
  .pagecard {
    display: flex;
    flex-direction: column;
    box-sizing: border-box;
    width: min(360px, 100%);
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg, 10px);
    background: var(--yap-s1);
    overflow: hidden;
  }
  .pagecard:hover {
    border-color: var(--yap-border-hover);
  }
  .pic {
    display: block;
    width: 100%;
    aspect-ratio: 2 / 1;
    padding: 0;
    border: none;
    border-bottom: 1px solid var(--yap-border-subtle);
    background: var(--yap-s2);
    cursor: pointer;
    overflow: hidden;
  }
  .pic img {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: cover;
    object-position: top center;
  }
  .foot {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    padding: 8px 8px 8px 10px;
  }
  .globe {
    flex: 0 0 auto;
    width: 15px;
    height: 15px;
    color: var(--yap-primary);
  }
  .words {
    display: flex;
    flex-direction: column;
    flex: 1 1 auto;
    min-width: 0;
    line-height: 1.3;
  }
  .name,
  .where {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .name {
    font-size: 12.5px;
    font-weight: 600;
    color: var(--yap-fg);
  }
  .where {
    font-size: 11.5px;
    color: var(--yap-muted);
  }
  .open {
    flex: 0 0 auto;
    height: 26px;
    padding: 0 10px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    background: var(--yap-s2);
    color: var(--yap-fg);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .open:hover {
    border-color: var(--yap-border-hover);
  }
  .more {
    display: inline-flex;
    flex: 0 0 auto;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    padding: 0;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-muted);
    cursor: pointer;
  }
  .more:hover {
    background: var(--yap-s2);
    color: var(--yap-fg);
  }
  .more svg {
    width: 16px;
    height: 16px;
  }
  .pic:focus-visible,
  .open:focus-visible,
  .more:focus-visible {
    outline: 2px solid var(--yap-primary-line);
    outline-offset: 1px;
  }
</style>
