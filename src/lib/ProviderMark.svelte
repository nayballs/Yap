<script>
  // A provider's or model family's mark (providerIcons.js), or one of Yap's
  // own glyphs when there's none: the Chat model chip and its picker.
  // Monochrome marks (OpenAI, Anthropic, xAI, OpenRouter) are drawn as a
  // mask in `currentColor`, so they stay visible on a dark surface too;
  // Claude's in its brand colour; the rest are the files as they are.
  //   icon:  an id of PROVIDER_ICONS ("groq", "qwen"…), or null
  //   glyph: Yap's own when `icon` is null — 'pc' | 'recent' | 'server' |
  //          'chip' | 'none' | 'spark'
  //   size:  px; tile: on a small card (the picker's header)
  import { PROVIDER_ICONS, MONOCHROME_PROVIDERS, ICON_TINTS } from './core/providerIcons.js';

  let { icon = null, glyph = 'spark', size = 18, tile = false } = $props();

  const src = $derived(icon ? PROVIDER_ICONS[icon] : null);
  const tint = $derived(icon ? ICON_TINTS[icon] : null);
  const masked = $derived(!!src && (MONOCHROME_PROVIDERS.has(icon) || !!tint));
  const maskStyle = $derived(
    masked ? `-webkit-mask-image:url("${src}");mask-image:url("${src}");${tint ? `background-color:${tint};` : ''}` : ''
  );
</script>

<span class="pmark mark" class:tile style="--s: {size}px" data-icon={src ? icon : glyph} aria-hidden="true">
  {#if masked}
    <span class="pm-mask" style={maskStyle}></span>
  {:else if src}
    <img {src} alt="" />
  {:else if glyph === 'pc'}
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="12" rx="2" /><path d="M8 20h8M12 16v4" /></svg>
  {:else if glyph === 'recent'}
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="8.5" /><path d="M12 7.5V12l3 2" /></svg>
  {:else if glyph === 'server'}
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="7" rx="1.5" /><rect x="3" y="13" width="18" height="7" rx="1.5" /><path d="M7 7.5h.01M7 16.5h.01" /></svg>
  {:else if glyph === 'chip'}
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="5" y="5" width="14" height="14" rx="2" /><rect x="9.5" y="9.5" width="5" height="5" /><path d="M9 2v3M15 2v3M9 19v3M15 19v3M2 9h3M2 15h3M19 9h3M19 15h3" /></svg>
  {:else if glyph === 'none'}
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="9" /><path d="M12 8v4.5M12 16h.01" /></svg>
  {:else}
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3l1.8 4.9L19 9.7l-4.2 3.1L16.4 18 12 15l-4.4 3 1.6-5.2L5 9.7l5.2-1.8z" /></svg>
  {/if}
</span>

<style>
  .pmark {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: 0 0 auto;
    width: var(--s);
    height: var(--s);
    color: var(--yap-fg);
    overflow: hidden;
  }
  .pmark img,
  .pmark svg,
  .pm-mask {
    display: block;
    width: 100%;
    height: 100%;
  }
  .pmark img {
    object-fit: contain;
  }
  /* Yap's own glyphs: muted, unless the parent sets --pm-glyph. */
  .pmark svg {
    color: var(--pm-glyph, var(--yap-muted));
  }
  .pm-mask {
    background-color: currentColor;
    -webkit-mask-repeat: no-repeat;
    mask-repeat: no-repeat;
    -webkit-mask-position: center;
    mask-position: center;
    -webkit-mask-size: contain;
    mask-size: contain;
  }
  .pmark.tile {
    box-sizing: border-box;
    padding: calc(var(--s) * 0.16);
    border: 1px solid var(--yap-border-subtle);
    border-radius: calc(var(--s) * 0.29);
    background: var(--yap-s2);
  }
</style>
