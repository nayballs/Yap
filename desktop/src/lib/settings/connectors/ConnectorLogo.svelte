<script>
  // A connector's mark on its tile (catalog.js): the vendor's, or its
  // initial on a coloured tile. Decorative: the name is always beside it.
  import { logoUrl, initialOf, tileOf } from './catalog.js';

  let { logo = null, name = '', color = null, size = 36 } = $props();
  const url = $derived(logoUrl(logo));
</script>

<span class="tile" class:mark={!!url} style:width="{size}px" style:height="{size}px" style:--tile={url ? null : tileOf(name, color)} aria-hidden="true">
  {#if url}
    <img src={url} alt="" style:width="{Math.round(size * 0.56)}px" style:height="{Math.round(size * 0.56)}px" />
  {:else}
    <span class="ini" style:font-size="{Math.round(size * 0.44)}px">{initialOf(name)}</span>
  {/if}
</span>

<style>
  .tile {
    flex: 0 0 auto;
    display: inline-grid;
    place-items: center;
    box-sizing: border-box;
    border-radius: 9px;
    background: var(--tile);
  }
  /* A vendor's mark: a light tile in both themes (the marks are dark), as the
     AI apps' logos are. */
  .tile.mark {
    background: var(--yap-mark-tile);
    border: 1px solid var(--yap-border-subtle);
  }
  .ini {
    color: #fff;
    font-weight: 700;
    line-height: 1;
  }
</style>
