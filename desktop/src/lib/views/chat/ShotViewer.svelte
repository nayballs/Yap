<script>
  // A screenshot opened large (chatState's `viewing`: from its tile in the
  // box, its picture in a question, or a menu's Open), as the Claude app's
  // viewer: over a darker scrim, in the screenshot frame (app.css
  // .shotframe), the whole picture at one image pixel per device pixel when
  // it fits the window, else as big as fits — Rust's thumbnail at exactly
  // those device pixels (chatImages.js `openPlaced`, snip/thumbs.rs), never
  // the browser's scaling. A click or Esc closes it (ChatView); its
  // right-click menu copies or saves it.
  //
  // **Paged** (15 Nov 2026, as Claude Code's app pages a turn's pictures): a
  // step's picture in a Code chat's thread comes with its turn's (`list`,
  // `at`: AgentWork's `onpaged`) — ‹ 2 of 4 › at the foot, ← and → too, the
  // step's name ("Read beds.png") in the top corner.
  import { untrack, onMount } from 'svelte';
  import { openContextMenu } from '../../ui/contextMenu.svelte.js';
  import { imageItems, openPlaced, shown } from './chatImages.js';
  import { screenPixels } from './chatShots.svelte.js';

  /** `img`: { id, w, h, caption?, list?, at? } (`list`: the pictures to page through, `at`: which one); `onclose()`. */
  let { img, onclose } = $props();

  // Room for it: the window less the padding all round, settled a moment
  // after a resize (a new size is a new thumbnail).
  const PAD = 32;
  /** Room kept at the foot for ‹ n of N ›, and at the top for its name. */
  const BAR = 44;
  let winW = $state(0);
  let winH = $state(0);
  let room = $state(null);
  const list = $derived(Array.isArray(img?.list) && img.list.length ? img.list : null);
  let at = $state(0);
  $effect(() => {
    const start = img?.at ?? 0;
    untrack(() => (at = Math.min(Math.max(0, start), (list?.length ?? 1) - 1)));
  });
  const current = $derived(list ? (list[at] ?? list[0]) : img);
  const paged = $derived(!!list && list.length > 1);
  const bars = $derived(paged || !!current?.caption);
  $effect(() => {
    const next = { w: Math.max(1, winW - 2 * PAD), h: Math.max(1, winH - 2 * PAD - (bars ? 2 * BAR : 0)) };
    if (!winW || !winH) return;
    if (!untrack(() => room)) {
      room = next;
      return;
    }
    const t = setTimeout(() => (room = next), 200);
    return () => clearTimeout(t);
  });
  const p = $derived(room && current ? openPlaced(current, screenPixels.ratio, room.w, room.h) : null);

  const step = (d) => {
    if (list) at = (at + d + list.length) % list.length;
  };
  function keydown(e) {
    if (e.key === 'Escape') onclose();
    else if (paged && e.key === 'ArrowLeft') step(-1);
    else if (paged && e.key === 'ArrowRight') step(1);
    else return;
    e.preventDefault();
    e.stopPropagation();
  }

  // The keys are its own while it's open: the focus to it, and back where it was after.
  let el = $state(null);
  onMount(() => {
    const before = document.activeElement;
    el?.focus({ preventScroll: true });
    return () => {
      if (before?.isConnected) before.focus?.({ preventScroll: true });
    };
  });
</script>

<svelte:window bind:innerWidth={winW} bind:innerHeight={winH} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
  class="shotview"
  bind:this={el}
  role="dialog"
  aria-label={current?.caption || 'Screenshot'}
  aria-modal="true"
  tabindex="-1"
  data-testid="shot-viewer"
  onclick={onclose}
  onkeydown={keydown}
>
  {#if current?.caption}
    <p class="vcaption" data-testid="shot-viewer-caption">{current.caption}</p>
  {/if}
  {#if p}
    <div class="shotframe big" style:width="{p.frame.w}px" style:aspect-ratio="{p.frame.w} / {p.frame.h}">
      <img
        src={p.src}
        alt={current?.caption || 'Screenshot'}
        draggable="false"
        use:shown
        data-id={current.id}
        style:left={p.pic.x}
        style:top={p.pic.y}
        style:width={p.pic.w}
        style:height={p.pic.h}
        oncontextmenu={(e) => openContextMenu(e, imageItems(current), { label: current?.caption ? 'Picture' : 'Screenshot' })}
      />
    </div>
  {/if}
  {#if paged}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="vpager" onclick={(e) => e.stopPropagation()} data-testid="shot-viewer-pager">
      <button type="button" class="vstep" aria-label="Previous picture" title="Previous (←)" onclick={() => step(-1)}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m15 6-6 6 6 6" /></svg>
      </button>
      <span class="vcount" data-testid="shot-viewer-count" aria-live="polite">{at + 1} of {list.length}</span>
      <button type="button" class="vstep" aria-label="Next picture" title="Next (→)" onclick={() => step(1)}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m9 6 6 6-6 6" /></svg>
      </button>
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
  /* The step's name, in the top corner, clear of the title bar (light on the scrim, in either theme). */
  .vcaption {
    position: absolute;
    top: 52px;
    left: 24px;
    max-width: calc(100% - 36px);
    margin: 0;
    overflow: hidden;
    color: var(--yap-shot-on-scrim);
    font-size: 12.5px;
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
    opacity: 0.9;
    pointer-events: none;
  }
  /* ‹ 2 of 4 ›, at the foot. */
  .vpager {
    position: absolute;
    bottom: 12px;
    left: 50%;
    transform: translateX(-50%);
    display: inline-flex;
    align-items: center;
    gap: 10px;
    color: var(--yap-shot-on-scrim);
    font-size: 13px;
    font-variant-numeric: tabular-nums;
    cursor: default;
  }
  .vstep {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 30px;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: none;
    color: inherit;
    cursor: pointer;
  }
  .vstep:hover {
    background: color-mix(in srgb, currentColor 14%, transparent);
  }
  .vstep:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  .vstep svg {
    width: 17px;
    height: 17px;
  }
</style>
