<script>
  // A screenshot opened large (chatState's `viewing`: from its tile in the
  // box, its picture in a question, or a menu's Open), as the Claude app's
  // viewer: over a darker scrim, in the screenshot frame (app.css
  // .shotframe), the whole picture at one image pixel per device pixel when
  // it fits the window, else as big as fits — Rust's thumbnail at exactly
  // those device pixels (chatImages.js `openPlaced`, snip/thumbs.rs), never
  // the browser's scaling. A click or Esc closes it (ChatView); its
  // right-click menu copies or saves it.
  import { untrack } from 'svelte';
  import { openContextMenu } from '../../ui/contextMenu.svelte.js';
  import { imageItems, openPlaced, shown } from './chatImages.js';
  import { screenPixels } from './chatShots.svelte.js';

  /** `img`: { id, w, h }; `onclose()`. */
  let { img, onclose } = $props();

  // Room for it: the window less the padding all round, settled a moment
  // after a resize (a new size is a new thumbnail).
  const PAD = 32;
  let winW = $state(0);
  let winH = $state(0);
  let room = $state(null);
  $effect(() => {
    const next = { w: Math.max(1, winW - 2 * PAD), h: Math.max(1, winH - 2 * PAD) };
    if (!winW || !winH) return;
    if (!untrack(() => room)) {
      room = next;
      return;
    }
    const t = setTimeout(() => (room = next), 200);
    return () => clearTimeout(t);
  });
  const p = $derived(room ? openPlaced(img, screenPixels.ratio, room.w, room.h) : null);
</script>

<svelte:window bind:innerWidth={winW} bind:innerHeight={winH} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div class="shotview" role="dialog" aria-label="Screenshot" aria-modal="true" tabindex="-1" onclick={onclose} onkeydown={(e) => e.key === 'Escape' && onclose()}>
  {#if p}
    <div class="shotframe big" style:width="{p.frame.w}px" style:aspect-ratio="{p.frame.w} / {p.frame.h}">
      <img
        src={p.src}
        alt="Screenshot"
        draggable="false"
        use:shown
        style:left={p.pic.x}
        style:top={p.pic.y}
        style:width={p.pic.w}
        style:height={p.pic.h}
        oncontextmenu={(e) => openContextMenu(e, imageItems(img), { label: 'Screenshot' })}
      />
    </div>
  {/if}
</div>

<style>
  .shotview {
    position: fixed;
    inset: 0;
    z-index: 60;
    display: grid;
    place-items: center;
    padding: 32px;
    background: var(--yap-shot-scrim);
    cursor: zoom-out;
  }
  .shotview:focus {
    outline: none;
  }
  .shotframe.big {
    box-shadow: var(--yap-shadow-modal);
  }
  /* Its own clip (no button round it here). */
  .shotframe.big img {
    border-radius: inherit;
  }
</style>
