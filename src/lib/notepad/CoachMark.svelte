<script>
  // A first-meeting coach mark (meetings/coach_marks.rs): a small ink card
  // with a pointer, what to try and why, and ✕. Each shows once.
  //   `title`, `body`: its words; `pointer`: 'up' | 'down' (and `align`:
  //   'left' | 'right', where the pointer sits); `ondismiss()`: ✕.
  let { title, body, pointer = 'up', align = 'left', ondismiss } = $props();
</script>

<div class="coach {pointer} {align}" role="note" aria-label={title} data-testid="coach-mark">
  <div class="ctext">
    <p class="ctitle">{title}</p>
    <p class="cbody">{body}</p>
  </div>
  <button class="cx" aria-label="Dismiss tip" onclick={() => ondismiss?.()}>
    <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M4 4l8 8M12 4l-8 8" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" /></svg>
  </button>
</div>

<style>
  .coach {
    position: relative;
    display: flex;
    align-items: flex-start;
    gap: 8px;
    max-width: 300px;
    box-sizing: border-box;
    padding: 10px 8px 10px 12px;
    border-radius: 10px;
    background: var(--yap-paper-ink);
    color: var(--yap-paper);
    box-shadow: var(--yap-shadow-popover);
    pointer-events: auto;
    animation: coachin 0.2s ease-out both;
  }
  /* The pointer: a small square turned, on the edge it points from. */
  .coach::before {
    content: '';
    position: absolute;
    width: 10px;
    height: 10px;
    background: inherit;
    transform: rotate(45deg);
  }
  .coach.up::before {
    top: -5px;
  }
  .coach.down::before {
    bottom: -5px;
  }
  .coach.left::before {
    left: 22px;
  }
  .coach.right::before {
    right: 22px;
  }
  @keyframes coachin {
    from {
      opacity: 0;
      transform: translateY(4px);
    }
  }
  :global(:root[data-motion='reduced']) .coach {
    animation: none;
  }
  .ctext {
    flex: 1 1 auto;
    min-width: 0;
  }
  .ctitle {
    margin: 0 0 2px;
    font-size: 13px;
    font-weight: 650;
  }
  .cbody {
    margin: 0;
    font-size: 12.5px;
    line-height: 17px;
    opacity: 0.85;
  }
  .cx {
    flex: 0 0 auto;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    padding: 0;
    border: none;
    border-radius: 6px;
    background: none;
    color: inherit;
    opacity: 0.8;
    cursor: pointer;
  }
  .cx:hover {
    opacity: 1;
    background: color-mix(in srgb, var(--yap-paper) 15%, transparent);
  }
  .cx:focus-visible {
    outline: none;
    box-shadow: 0 0 0 2px var(--yap-paper);
  }
  .cx svg {
    width: 14px;
    height: 14px;
  }
</style>
