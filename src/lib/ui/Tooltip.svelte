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
    color: #6b7280;
    cursor: help;
    border-radius: 50%;
    transition: color 0.15s ease;
  }
  .tip:hover,
  .tip:focus-visible {
    color: #3b82f6;
    outline: none;
  }
  .bubble {
    position: fixed;
    transform: translate(-50%, calc(-100% - 8px));
    z-index: 9999;
    max-width: 240px;
    padding: 8px 10px;
    background: #1f2430;
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: 8px;
    box-shadow: 0 6px 20px rgba(0, 0, 0, 0.45);
    color: #e5e7eb;
    font-size: 12px;
    line-height: 1.45;
    text-align: center;
    white-space: normal;
    pointer-events: none;
  }
</style>
