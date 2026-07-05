<script>
  // A small ⓘ info icon that reveals a floating bubble on hover/focus.
  // The bubble is fixed-positioned (measured from the icon) so it escapes the
  // settings content's overflow clip and never gets cut off.
  let { text = '' } = $props();

  let show = $state(false);
  let coords = $state({ top: 0, left: 0 });
  let iconEl;

  function place() {
    if (!iconEl) return;
    const r = iconEl.getBoundingClientRect();
    coords = { top: r.top, left: r.left + r.width / 2 };
  }
  function open() {
    place();
    show = true;
  }
  function close() {
    show = false;
  }
</script>

<button
  bind:this={iconEl}
  type="button"
  class="tip"
  aria-label="More information"
  onmouseenter={open}
  onmouseleave={close}
  onfocus={open}
  onblur={close}
  onclick={(e) => e.stopPropagation()}
>
  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor"
    stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <circle cx="12" cy="12" r="9" />
    <path d="M12 16v-4" />
    <path d="M12 8h.01" />
  </svg>
</button>

{#if show && text}
  <span class="bubble" style="top:{coords.top}px; left:{coords.left}px;">{text}</span>
{/if}

<style>
  .tip {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: none;
    border: none;
    color: var(--yap-fg-45);
    cursor: help;
    border-radius: 50%;
    transition: color var(--yap-dur) ease;
  }
  .tip:hover,
  .tip:focus-visible {
    color: var(--yap-primary);
    outline: none;
  }
  .bubble {
    position: fixed;
    transform: translate(-50%, calc(-100% - 8px));
    z-index: 9999;
    max-width: 240px;
    padding: 8px 10px;
    background: var(--yap-s3);
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r-lg);
    box-shadow: 0 12px 28px -8px rgba(0, 0, 0, 0.55);
    color: var(--yap-fg);
    font-size: 12px;
    line-height: 1.45;
    text-align: center;
    white-space: normal;
    pointer-events: none;
  }
</style>
