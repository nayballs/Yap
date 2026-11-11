<script>
  // **The pictures at the end of a step's line** (11 Nov 2026, after Claude
  // Code's own transcript): when a tool call came back with a picture — a
  // picture file read, a screenshot a tool took — its line ends with it,
  // small (core/agents.js `stepPicture`; kept by agents/pictures.rs, served
  // by the `yapsnip` scheme). A click opens it (`onview(picture)`); its
  // right-click menu copies or saves it. Each a tile filled with the
  // picture's middle, Rust's at exactly its device pixels (shares.js
  // `coverPlaced`); the first few, then "+N". One that's gone (its agent's
  // log swept) isn't drawn.
  import { openContextMenu } from '../../ui/contextMenu.svelte.js';
  import { chatImageUrl, copyImage, saveImage, shown } from './chatImages.js';
  import { coverPlaced } from './shares.js';
  import { screenPixels } from './chatShots.svelte.js';

  /** A tile's CSS size. */
  const W = 30;
  const H = 20;

  /** `pictures`: `[{ id, w, h, caption }]`; `max`: how many a line shows; `onview(picture)`. */
  let { pictures = [], max = 3, onview = null } = $props();

  let gone = $state({});
  const there = $derived(pictures.filter((p) => p?.id && !gone[p.id]));
  const menu = (e, p) =>
    openContextMenu(
      e,
      [
        { label: 'Open', icon: 'open', onselect: () => onview?.(p) },
        { label: 'Copy image', icon: 'copy', onselect: () => copyImage(p.id, 'Picture') },
        { label: 'Save image as…', icon: 'download', onselect: () => saveImage(p.id, 'Picture') },
      ],
      { label: 'Picture' }
    );
</script>

{#if there.length}
  <span class="steppics" data-testid="agent-step-pictures" data-count={there.length}>
    {#each there.slice(0, max) as p (p.id)}
      {@const t = coverPlaced(p, W, H, screenPixels.ratio, chatImageUrl(p.id))}
      <button
        type="button"
        class="steppic"
        data-testid="agent-step-picture"
        style:width="{t.frame.w}px"
        style:height="{t.frame.h}px"
        title={p.caption ? `${p.caption}: open the picture` : 'Open the picture'}
        aria-label={p.caption ? `Open the picture: ${p.caption}` : 'Open the picture'}
        onclick={() => onview?.(p)}
        oncontextmenu={(e) => menu(e, p)}
      >
        <img src={t.src} alt="" draggable="false" decoding="async" use:shown onerror={() => (gone = { ...gone, [p.id]: true })} />
      </button>
    {/each}
    {#if there.length > max}<span class="stepmore" title="{there.length} pictures">+{there.length - max}</span>{/if}
  </span>
{/if}

<style>
  .steppics {
    display: inline-flex;
    flex: 0 0 auto;
    align-items: center;
    gap: 4px;
  }
  .steppic {
    position: relative;
    flex: 0 0 auto;
    display: block;
    margin: 0;
    padding: 0;
    border: 0;
    border-radius: 4px;
    overflow: hidden;
    background: var(--yap-s2);
    /* The ring laid round it, one device pixel wide (as a screenshot's frame). */
    box-shadow: 0 0 0 var(--yap-hairline) var(--yap-shot-ring);
    cursor: zoom-in;
    transition: box-shadow var(--yap-dur);
  }
  .steppic:hover {
    box-shadow: 0 0 0 var(--yap-hairline) var(--yap-shot-ring-hover);
  }
  .steppic:focus-visible {
    outline: 2px solid var(--yap-primary-line);
    outline-offset: 1px;
  }
  .steppic img {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: cover;
    opacity: 0;
    transition: opacity 200ms ease-out;
    user-select: none;
  }
  .steppic :global(img.shown) {
    opacity: 1;
  }
  .stepmore {
    color: var(--yap-muted);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }
  :global(:root[data-motion='reduced']) .steppic img {
    transition: none;
  }
</style>
