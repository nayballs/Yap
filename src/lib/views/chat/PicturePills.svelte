<script>
  // The pictures an answer found online and showed beside the chat (find and
  // show, chat/pictures.rs): a pill each under it, beside its documents' —
  // a 48 px thumbnail at its device pixels in the screenshot frame
  // (chatImages.js `placed`, app.css .shotframe), the caption and its site;
  // a click opens the panel on it. Its own small chunk (ChatMessage loads it
  // for an answer that has some): the main window's stays as it was.
  import { placed, shown } from './chatImages.js';
  import { screenPixels } from './chatShots.svelte.js';

  /** `pics`: the answer's `ShownPicture`s; `onpicture(p)` opens one in the panel. */
  let { pics, onpicture = null } = $props();

  const THUMB = 48;
  const nameOf = (p) => p?.caption || p?.alt || 'Picture';
</script>

{#each pics as p (p.id)}
  {@const t = placed(p, { maxW: THUMB, maxH: THUMB, minW: THUMB, minH: THUMB, ratio: screenPixels.ratio })}
  <button
    type="button"
    class="picpill"
    data-testid="chat-picture-pill"
    onclick={() => onpicture?.(p)}
    aria-label={`Open the picture ${nameOf(p)}`}
    title={p.page?.title ? `${nameOf(p)} — from ${p.page.title}` : nameOf(p)}
  >
    <span class="shotframe picthumb" aria-hidden="true">
      <img src={t.src} alt="" draggable="false" decoding="async" use:shown style="left: {t.pic.x}; top: {t.pic.y}; width: {t.pic.w}; height: {t.pic.h}" />
    </span>
    <span class="pictext">
      <span class="piccap">{nameOf(p)}</span>
      <span class="picfrom">{p.page?.site || p.page?.title || 'Picture'}<span class="open">{' · Open'}</span></span>
    </span>
  </button>
{/each}

<style>
  .picpill {
    display: inline-flex;
    align-items: center;
    gap: 9px;
    max-width: 300px;
    height: 58px;
    padding: 4px 12px 4px 4px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    background: var(--yap-s1);
    color: var(--yap-fg);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .picpill:hover {
    border-color: var(--yap-border-hover);
    background: var(--yap-s2);
  }
  .picpill:focus-visible {
    outline: 2px solid var(--yap-primary-line);
    outline-offset: 1px;
  }
  .picthumb {
    flex: 0 0 auto;
    width: 48px;
    aspect-ratio: 1;
    border-radius: 6px;
    overflow: hidden;
  }
  .pictext {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }
  .piccap,
  .picfrom {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .piccap {
    font-size: 12px;
    font-weight: 600;
  }
  .picfrom {
    font-size: 11px;
    color: var(--yap-muted);
  }
  .open {
    color: var(--yap-primary);
    font-weight: 600;
  }
</style>
