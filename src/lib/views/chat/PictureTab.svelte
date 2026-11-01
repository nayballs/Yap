<script module>
  /** Each picture's zoom (its id → `{view, fitted}`), kept while Yap runs: switching tabs and back. */
  const kept = new Map();
</script>

<script>
  // A picture Chat found online, in the panel beside the chat (find and
  // show, 2026-10-31; chatDoc.svelte.js `picture`, chat/pictures.rs): its
  // tab of the workspace, in DocPanel's own chunk.
  // - It fits the panel at its device pixels, as Chat's screenshots are
  //   placed (chatImages.js `placed`: Rust's thumbnail at exactly that size,
  //   snip/thumbs.rs), with room round it.
  // - Zoom: the wheel around the pointer, − / + in the header (the label
  //   between them goes back to the fit), a double-click toggles the fit and
  //   1:1 (one image pixel per device pixel; a picture that fits at 1:1
  //   already goes to 2×); the keys + / − / 0, the arrows move it. Zoomed
  //   past the panel, it's dragged about (pictureZoom.js: never off the
  //   stage). Each picture's zoom is kept while Yap runs.
  // - Under it, its caption and where it came from: "From <page title> ·
  //   Open page" (the browser, `open_external`).
  // - ⋯ (and a right-click on it): Copy image and Save image as… (the chat
  //   screenshots' own, snip/copy_save.rs), Open original (where the
  //   picture itself came from), Open page.
  // - Colours from the --yap-* tokens; reduced motion: a zoom jumps.
  import { untrack } from 'svelte';
  import { openContextMenu } from '../../ui/contextMenu.svelte.js';
  import { openExternalLink } from '../../core/externalLinks.js';
  import { chatImageUrl, placed, copyImage, saveImage, shown } from './chatImages.js';
  import { screenPixels } from './chatShots.svelte.js';
  import { FIT_PAD, fitScale, fitView, clampView, zoomAt, panBy, isFit, toggleScale, stepScale, wheelScale, zoomLabel, limits } from './pictureZoom.js';

  /** `doc`: the panel's state (its `picture`); `narrow`: no room beside the chat (no expand). */
  let { doc, narrow = false } = $props();

  const pic = $derived(doc.picture);
  const name = $derived(pic?.caption || pic?.alt || 'Picture');
  const page = $derived(pic?.page ?? null);
  const pageName = $derived(page?.title || page?.site || '');

  const reduced = () => matchMedia('(prefers-reduced-motion: reduce)').matches || document.documentElement.dataset.motion === 'reduced';

  // Its own size: as Rust said, else as it loaded.
  let natural = $state({ id: null, w: 0, h: 0 });
  const iw = $derived(pic?.w > 0 ? pic.w : natural.id === pic?.id ? natural.w : 0);
  const ih = $derived(pic?.h > 0 ? pic.h : natural.id === pic?.id ? natural.h : 0);
  const ratio = $derived(screenPixels.ratio || 1);
  let broken = $state(null);

  // ---- the stage and where the picture sits on it ----------------------------------------

  let stageEl = $state(null);
  let room = $state({ w: 0, h: 0 });
  $effect(() => {
    const el = stageEl;
    if (!el || typeof ResizeObserver === 'undefined') return;
    const measure = () => (room = { w: el.clientWidth, h: el.clientHeight });
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    measure();
    return () => ro.disconnect();
  });

  const sizes = $derived({ w: iw, h: ih, stageW: room.w, stageH: room.h, ratio });
  const fit = $derived(iw && ih && room.w ? fitScale(iw, ih, room.w - 2 * FIT_PAD, room.h - 2 * FIT_PAD, ratio) : 1);
  let view = $state({ s: 1, x: 0, y: 0 });
  // Fitted: it follows the panel's size (the split dragged, expanded).
  let fitted = $state(true);
  let animate = $state(false);
  let animTimer = 0;

  // Another picture (or its size now known): its kept zoom, or the fit.
  let shownFor = null;
  $effect(() => {
    const id = pic?.id;
    const ready = iw > 0 && ih > 0 && room.w > 0 && room.h > 0;
    untrack(() => {
      if (!ready) return;
      if (shownFor !== id) {
        shownFor = id;
        broken = null;
        const k = kept.get(id);
        if (k && !k.fitted) {
          fitted = false;
          view = clampView(k.view, iw, ih, room.w, room.h, ratio);
        } else {
          fitted = true;
          view = fitView(iw, ih, room.w, room.h, ratio);
        }
        return;
      }
      // The same picture, the stage resized (or another monitor's scaling).
      view = fitted ? fitView(iw, ih, room.w, room.h, ratio) : clampView(view, iw, ih, room.w, room.h, ratio);
    });
    void room.w;
    void room.h;
    void ratio;
  });
  $effect(() => {
    const id = pic?.id;
    const v = view;
    const f = fitted;
    if (id && shownFor === id) kept.set(id, { view: v, fitted: f });
  });
  $effect(() => () => clearTimeout(animTimer));

  /** Glide to `next` (buttons, keys, a double-click); the wheel and a drag follow at once. */
  function go(next, { glide = false, fitNow = false } = {}) {
    if (glide && !reduced()) {
      animate = true;
      clearTimeout(animTimer);
      animTimer = setTimeout(() => (animate = false), 220);
    } else {
      animate = false;
    }
    fitted = fitNow || isFit(next.s, fit);
    view = fitted ? fitView(iw, ih, room.w, room.h, ratio) : next;
  }

  const centre = () => [room.w / 2, room.h / 2];

  function step(dir) {
    if (!iw) return;
    const [px, py] = centre();
    go(zoomAt(view, stepScale(view.s, dir, fit), px, py, { ...sizes, fit }), { glide: true });
  }

  function toFit() {
    go(view, { glide: true, fitNow: true });
  }

  /** The label between − and +: zoomed, back to the fit; fitted, 1:1 (or 2×). */
  function toggleAt(px, py) {
    if (!iw) return;
    const s = toggleScale(view.s, fit);
    if (isFit(s, fit)) toFit();
    else go(zoomAt(view, s, px, py, { ...sizes, fit }), { glide: true });
  }

  const at = (e) => {
    const r = stageEl.getBoundingClientRect();
    return [e.clientX - r.left, e.clientY - r.top];
  };

  // The wheel zooms around the pointer (a listener that may cancel the scroll).
  $effect(() => {
    const el = stageEl;
    if (!el) return;
    const wheel = (e) => {
      if (!iw) return;
      e.preventDefault();
      const [px, py] = at(e);
      const s = wheelScale(view.s, e.deltaY, e.deltaMode === 1);
      if (s <= fit + 1e-6) go(view, { fitNow: true });
      else go(zoomAt(view, s, px, py, { ...sizes, fit }));
    };
    el.addEventListener('wheel', wheel, { passive: false });
    return () => el.removeEventListener('wheel', wheel);
  });

  // ---- dragging it about when it's bigger than the stage --------------------------------

  const cw = $derived((iw * view.s) / ratio);
  const ch = $derived((ih * view.s) / ratio);
  const movable = $derived(cw > room.w + 0.5 || ch > room.h + 0.5);
  let drag = $state(null);

  function down(e) {
    if (e.button !== 0 || !movable) return;
    drag = { x: e.clientX, y: e.clientY, id: e.pointerId };
    e.currentTarget.setPointerCapture?.(e.pointerId);
    e.preventDefault();
  }
  function move(e) {
    if (!drag || e.pointerId !== drag.id) return;
    const dx = e.clientX - drag.x;
    const dy = e.clientY - drag.y;
    drag = { ...drag, x: e.clientX, y: e.clientY };
    go(panBy(view, dx, dy, sizes));
  }
  function up(e) {
    if (drag && e.pointerId === drag.id) drag = null;
  }

  function dbl(e) {
    if (!stageEl) return;
    e.preventDefault();
    const [px, py] = at(e);
    toggleAt(px, py);
  }

  /** The keys while the picture tab has the focus: + / − / 0, the arrows to move it. */
  function key(e) {
    if (e.ctrlKey || e.metaKey || e.altKey || e.defaultPrevented) return;
    const k = e.key;
    if (k === '+' || k === '=' || k === 'Add') step(1);
    else if (k === '-' || k === '_' || k === 'Subtract') step(-1);
    else if (k === '0') toFit();
    else if (movable && e.target === stageEl && k.startsWith('Arrow')) {
      const d = e.shiftKey ? 160 : 48;
      const dx = k === 'ArrowLeft' ? d : k === 'ArrowRight' ? -d : 0;
      const dy = k === 'ArrowUp' ? d : k === 'ArrowDown' ? -d : 0;
      go(panBy(view, dx, dy, sizes), { glide: true });
    } else return;
    e.preventDefault();
    e.stopPropagation();
  }

  const lim = $derived(limits(fit));
  const atMin = $derived(view.s <= lim.min + 1e-6);
  const atMax = $derived(view.s >= lim.max - 1e-6);
  const label = $derived(zoomLabel(view.s, fit));

  // ---- the pictures drawn: the whole one, and at the fit Rust's thumbnail -----------------

  const full = $derived(pic?.id ? chatImageUrl(pic.id) : '');
  // The fit's thumbnail at exactly its device pixels: made for the stage's
  // size once it has settled (dragging the split asks for none meanwhile).
  let thumbRoom = $state(null);
  $effect(() => {
    const next = { w: room.w, h: room.h };
    if (!next.w || !next.h) return;
    if (!untrack(() => thumbRoom)) {
      thumbRoom = next;
      return;
    }
    const t = setTimeout(() => (thumbRoom = next), 200);
    return () => clearTimeout(t);
  });
  const thumb = $derived.by(() => {
    if (!pic?.id || !iw || !ih || !thumbRoom || thumbRoom.w !== room.w || thumbRoom.h !== room.h) return null;
    const p = placed({ id: pic.id, w: iw, h: ih }, { maxW: room.w - 2 * FIT_PAD, maxH: room.h - 2 * FIT_PAD, ratio });
    return p.thumb ? p : null;
  });

  function loadedFull(e) {
    const el = e.currentTarget;
    if (pic?.id && !(pic.w > 0 && pic.h > 0)) natural = { id: pic.id, w: el.naturalWidth, h: el.naturalHeight };
  }

  // ---- ⋯ ---------------------------------------------------------------------------------

  function items() {
    if (!pic?.id) return [];
    return [
      { label: 'Copy image', icon: 'copy', onselect: () => copyImage(pic.id, 'Picture'), disabled: broken === pic.id },
      { label: 'Save image as…', icon: 'download', onselect: () => saveImage(pic.id, 'Picture'), disabled: broken === pic.id },
      { separator: true },
      { label: 'Open original', icon: 'open', onselect: () => openExternalLink(pic.imageUrl), disabled: !pic.imageUrl },
      { label: 'Open page', icon: 'open', onselect: () => openExternalLink(page.url), disabled: !page?.url },
    ];
  }
  const moreMenu = (e) => openContextMenu(e, items(), { label: 'Picture' });
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<section class="pictab" aria-label="Picture" data-testid="chat-picture" onkeydown={key}>
  <header class="pichead">
    <svg class="picicon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3" y="4" width="18" height="16" rx="2" /><circle cx="9" cy="10" r="1.8" /><path d="m4 18 5-5 3 3 3-3 5 5" /></svg>
    <h2 class="pictitle" title={name}>{name}</h2>
    <div class="picactions">
      <div class="zoomseg" role="group" aria-label="Zoom">
        <button type="button" class="picbtn" onclick={() => step(-1)} disabled={atMin || !iw} aria-label="Zoom out" title="Zoom out (−)">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" aria-hidden="true"><path d="M6 12h12" /></svg>
        </button>
        <button
          type="button"
          class="zoomlabel"
          data-testid="chat-picture-zoom"
          onclick={() => toggleAt(...centre())}
          disabled={!iw}
          aria-label={fitted ? 'Fitted to the panel: show at 1:1' : `Zoomed to ${label}: fit to the panel`}
          title={fitted ? 'Show at 1:1 (double-click the picture)' : 'Fit to the panel (0)'}>{label}</button
        >
        <button type="button" class="picbtn" onclick={() => step(1)} disabled={atMax || !iw} aria-label="Zoom in" title="Zoom in (+)">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" aria-hidden="true"><path d="M6 12h12M12 6v12" /></svg>
        </button>
      </div>
      {#if !narrow}
        <button
          type="button"
          class="picbtn"
          onclick={() => (doc.expanded = !doc.expanded)}
          aria-label={doc.expanded ? 'Back beside the chat' : 'Expand to full width'}
          title={doc.expanded ? 'Back beside the chat' : 'Expand to full width'}
        >
          {#if doc.expanded}
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 14h6v6M20 10h-6V4M14 10l7-7M3 21l7-7" /></svg>
          {:else}
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M15 3h6v6M9 21H3v-6M21 3l-7 7M3 21l7-7" /></svg>
          {/if}
        </button>
      {/if}
      <button type="button" class="picbtn" data-more aria-label="More for the picture" title="Copy, save, open the original" aria-haspopup="menu" onclick={moreMenu}>
        <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><circle cx="5.5" cy="12" r="1.6" /><circle cx="12" cy="12" r="1.6" /><circle cx="18.5" cy="12" r="1.6" /></svg>
      </button>
      <button type="button" class="picbtn" onclick={() => doc.close()} aria-label="Close the panel" title="Close">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
      </button>
    </div>
  </header>

  <!-- The picture on its stage: the wheel and the keys zoom, a drag moves it. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div
    class="stage"
    class:movable
    class:dragging={!!drag}
    class:zoomable={!movable && !atMax}
    bind:this={stageEl}
    role="group"
    aria-roledescription="picture"
    aria-label={`${name}. Plus and minus zoom, 0 fits it, the arrows move it.`}
    tabindex="0"
    data-testid="chat-picture-stage"
    onpointerdown={down}
    onpointermove={move}
    onpointerup={up}
    onpointercancel={up}
    ondblclick={dbl}
    oncontextmenu={moreMenu}
  >
    {#if broken === pic?.id}
      <p class="picgone">This picture is no longer on this PC.</p>
    {:else if pic?.id}
      {#key pic.id}
        <img
          class="picfull"
          class:animate
          class:placed={iw > 0 && room.w > 0}
          src={full}
          alt={pic.alt || name}
          draggable="false"
          decoding="async"
          data-testid="chat-picture-img"
          use:shown
          onload={loadedFull}
          onerror={() => (broken = pic.id)}
          style:width={iw ? `${cw}px` : undefined}
          style:height={ih ? `${ch}px` : undefined}
          style:transform={`translate(${view.x}px, ${view.y}px)`}
        />
        {#if thumb}
          <!-- At the fit, Rust's thumbnail over it, at exactly its device pixels. -->
          <img
            class="picthumb"
            class:on={fitted && !animate}
            src={thumb.src}
            alt=""
            aria-hidden="true"
            draggable="false"
            decoding="async"
            data-testid="chat-picture-thumb"
            style:width={`${thumb.frame.w}px`}
            style:height={`${thumb.frame.h}px`}
            style:transform={`translate(${Math.round(((room.w - thumb.frame.w) / 2) * ratio) / ratio}px, ${Math.round(((room.h - thumb.frame.h) / 2) * ratio) / ratio}px)`}
          />
        {/if}
      {/key}
    {/if}
  </div>

  <footer class="picinfo">
    {#if pic?.caption}<p class="piccaption" data-testid="chat-picture-caption">{pic.caption}</p>{/if}
    {#if pageName}
      <p class="picsource" data-testid="chat-picture-source">
        <span class="from">From <span class="pagename" title={page?.url || pageName}>{pageName}</span></span>
        {#if page?.url}
          <span class="sep" aria-hidden="true">·</span>
          <button type="button" class="openpage" onclick={() => openExternalLink(page.url)}>Open page</button>
        {/if}
      </p>
    {/if}
  </footer>
</section>

<style>
  .pictab {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }
  .pichead {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 12px 8px 18px;
    border-bottom: 1px solid var(--yap-paper-rule);
  }
  .picicon {
    flex: 0 0 auto;
    width: 15px;
    height: 15px;
    color: var(--yap-primary);
  }
  .pictitle {
    flex: 1 1 auto;
    min-width: 0;
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--yap-paper-text-2);
  }
  .picactions {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    gap: 2px;
  }
  .picbtn,
  .zoomlabel {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 28px;
    height: 28px;
    padding: 0;
    border: 1px solid transparent;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-paper-muted);
    font: inherit;
    cursor: pointer;
  }
  .picbtn:hover:not(:disabled),
  .zoomlabel:hover:not(:disabled) {
    background: var(--yap-paper-s2);
    color: var(--yap-paper-ink);
  }
  .picbtn:disabled,
  .zoomlabel:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .picbtn:focus-visible,
  .zoomlabel:focus-visible,
  .openpage:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  .picbtn svg {
    width: 15px;
    height: 15px;
  }
  /* − 100% +: one small group, as the document's Preview | Source. */
  .zoomseg {
    display: inline-flex;
    align-items: center;
    margin: 0 4px;
    padding: 0 2px;
    border-radius: 8px;
    background: var(--yap-paper-s1);
  }
  .zoomseg .picbtn {
    min-width: 24px;
    height: 24px;
  }
  .zoomlabel {
    min-width: 44px;
    height: 24px;
    padding: 0 4px;
    font-size: 11.5px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    color: var(--yap-paper-ink);
  }
  /* The stage: the picture on a quiet ground, the rest of it out of sight. */
  .stage {
    position: relative;
    flex: 1 1 auto;
    min-height: 0;
    overflow: hidden;
    background: var(--yap-paper-s1);
    touch-action: none;
    user-select: none;
  }
  .stage:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: -2px;
  }
  .stage.zoomable {
    cursor: zoom-in;
  }
  .stage.movable {
    cursor: grab;
  }
  .stage.dragging {
    cursor: grabbing;
  }
  .stage img {
    position: absolute;
    top: 0;
    left: 0;
    display: block;
    max-width: none;
    transform-origin: 0 0;
    pointer-events: none;
    user-select: none;
  }
  /* The whole picture: its size known, placed exactly; not yet, contained. */
  .picfull {
    background: var(--yap-shot-fill);
    box-shadow: 0 0 0 var(--yap-hairline) var(--yap-shot-ring);
    opacity: 0;
    transition: opacity 200ms ease-out;
  }
  .picfull:not(.placed) {
    width: 100%;
    height: 100%;
    object-fit: contain;
    background: none;
    box-shadow: none;
  }
  .picfull:global(.shown) {
    opacity: 1;
  }
  .picfull.animate {
    transition:
      opacity 200ms ease-out,
      transform 200ms ease-out,
      width 200ms ease-out,
      height 200ms ease-out;
  }
  /* The fit's thumbnail: shown once the picture has come to rest at the fit
     (at once, never faded out: the whole picture is right under it). */
  .picthumb {
    opacity: 0;
  }
  .picthumb.on {
    opacity: 1;
  }
  @media (prefers-reduced-motion: reduce) {
    .picfull,
    .picfull.animate {
      transition: none;
    }
  }
  :global(:root[data-motion='reduced']) .picfull,
  :global(:root[data-motion='reduced']) .picfull.animate {
    transition: none;
  }
  .picgone {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    margin: 0;
    padding: 18px;
    font-size: 12.5px;
    color: var(--yap-paper-muted);
    text-align: center;
  }
  /* Its caption and where it came from, as a figure's. */
  .picinfo {
    flex: 0 0 auto;
    padding: 10px 18px 12px;
    border-top: 1px solid var(--yap-paper-s2);
  }
  .piccaption {
    margin: 0 0 3px;
    font-family: var(--yap-font-reading);
    font-size: calc(13px * var(--yap-reading-scale));
    line-height: 1.45;
    color: var(--yap-paper-ink);
    overflow-wrap: anywhere;
    text-wrap: pretty;
  }
  .picsource {
    display: flex;
    align-items: baseline;
    gap: 6px;
    min-width: 0;
    margin: 0;
    font-size: 11.5px;
    color: var(--yap-paper-muted);
  }
  .from {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pagename {
    color: var(--yap-paper-text-2);
    font-weight: 600;
  }
  .openpage {
    flex: 0 0 auto;
    padding: 0;
    border: none;
    background: none;
    color: var(--yap-link);
    font: inherit;
    font-weight: 600;
    cursor: pointer;
  }
  .openpage:hover {
    text-decoration: underline;
  }
</style>
