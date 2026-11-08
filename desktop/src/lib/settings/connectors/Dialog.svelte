<script>
  // The connectors' dialogs' frame (+ Add's, Discover's sheet), as Delete some
  // of your data…'s: a scrim, a card that keeps Tab inside, Esc and the scrim
  // close it (not while `busy`), the focus back where it was.
  //
  //   title, lead?: its heading and line; head?: a snippet before the title
  //   (a logo); foot?: its buttons; children: the body; onclose.
  import { onMount, tick } from 'svelte';

  let { title, lead = '', head, foot, children, busy = false, wide = false, returnFocus = null, onclose } = $props();

  const uid = Math.random().toString(36).slice(2, 8);
  let card = $state(null);

  onMount(() => {
    tick().then(() => (card?.querySelector('[data-autofocus]') ?? card?.querySelector('input:not(:disabled), textarea:not(:disabled), button:not(:disabled)') ?? card)?.focus({ preventScroll: true }));
  });

  function close() {
    if (busy) return;
    // Read before onclose: the caller's props may be gone after it.
    const back = returnFocus;
    onclose?.();
    tick().then(() => back?.isConnected && back.focus?.({ preventScroll: true }));
  }

  function onKeydown(e) {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      close();
      return;
    }
    if (e.key !== 'Tab') return;
    const focusable = [...card.querySelectorAll('button:not(:disabled), input:not(:disabled), textarea:not(:disabled), select:not(:disabled), a[href], summary')].filter((el) => el.offsetParent !== null);
    if (!focusable.length) return;
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (e.shiftKey && (document.activeElement === first || document.activeElement === card)) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  }
</script>

<div class="scrim" role="presentation" onclick={(e) => e.target === e.currentTarget && close()}>
  <div
    bind:this={card}
    class="sheet"
    class:wide
    role="dialog"
    aria-modal="true"
    aria-labelledby="cd-title-{uid}"
    aria-describedby={lead ? `cd-lead-${uid}` : undefined}
    tabindex="-1"
    onkeydown={onKeydown}
  >
    <div class="headrow">
      {@render head?.()}
      <div class="titles">
        <h2 id="cd-title-{uid}">{title}</h2>
        {#if lead}<p id="cd-lead-{uid}">{lead}</p>{/if}
      </div>
      <button class="x" type="button" aria-label="Close" onclick={close} disabled={busy}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
      </button>
    </div>
    <div class="body">{@render children?.()}</div>
    {#if foot}<div class="foot">{@render foot()}</div>{/if}
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 90;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 16px;
    background: var(--yap-paper-scrim);
    animation: scrim-in 0.15s ease-out;
  }
  @keyframes scrim-in {
    from {
      opacity: 0;
    }
  }
  .sheet {
    display: flex;
    flex-direction: column;
    gap: 16px;
    width: min(540px, 100%);
    max-height: min(720px, calc(100vh - 48px));
    box-sizing: border-box;
    padding: 20px 22px 18px;
    border: 1px solid var(--yap-border);
    border-radius: 14px;
    background: var(--yap-s2);
    color: var(--yap-fg);
    box-shadow: var(--yap-shadow-dialog);
    outline: none;
    animation: sheet-in 0.15s var(--yap-ease-spring) both;
  }
  .sheet.wide {
    width: min(640px, 100%);
  }
  @keyframes sheet-in {
    from {
      opacity: 0;
      transform: scale(0.96);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .scrim,
    .sheet {
      animation: none;
    }
  }
  .headrow {
    display: flex;
    align-items: flex-start;
    gap: 12px;
  }
  .titles {
    flex: 1 1 auto;
    min-width: 0;
  }
  h2 {
    margin: 0;
    font-size: 17px;
    font-weight: 620;
    line-height: 24px;
  }
  .titles p {
    margin: 4px 0 0;
    font-size: 13px;
    line-height: 19px;
    color: var(--yap-muted);
  }
  .x {
    flex: 0 0 auto;
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    margin: -4px -6px 0 0;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-fg-62);
    cursor: pointer;
  }
  .x:hover {
    background: var(--yap-raised-soft);
    color: var(--yap-fg);
  }
  .x:focus-visible {
    outline: none;
    box-shadow: var(--yap-focus);
  }
  .x svg {
    width: 15px;
    height: 15px;
  }
  .body {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 2px 4px 2px 0;
  }
  .foot {
    display: flex;
    justify-content: flex-end;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  /* The dialogs' shared parts: fields, buttons. */
  .sheet :global(.field) {
    display: flex;
    flex-direction: column;
    gap: 5px;
  }
  .sheet :global(.field > span:first-child) {
    font-size: 12.5px;
    font-weight: 620;
  }
  .sheet :global(.field .hint) {
    font-size: 12px;
    color: var(--yap-muted);
    line-height: 1.45;
  }
  .sheet :global(.inp) {
    height: 34px;
    padding: 0 10px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    background: var(--yap-s1);
    color: var(--yap-fg);
    font: inherit;
    font-size: 13px;
    box-sizing: border-box;
    min-width: 0;
  }
  .sheet :global(textarea.inp) {
    height: auto;
    min-height: 140px;
    padding: 8px 10px;
    font-family: ui-monospace, 'Cascadia Mono', Consolas, monospace;
    font-size: 12px;
    resize: vertical;
  }
  .sheet :global(.inp:focus) {
    outline: none;
    border-color: var(--yap-border-hover);
    box-shadow: var(--yap-focus);
  }
  .sheet :global(.dbtn) {
    height: 34px;
    padding: 0 16px;
    border: 1px solid var(--yap-border);
    border-radius: 9999px;
    background: var(--yap-s2);
    color: var(--yap-fg);
    font: inherit;
    font-size: 13.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .sheet :global(.dbtn:hover:not(:disabled)) {
    border-color: var(--yap-border-hover);
  }
  .sheet :global(.dbtn.ink) {
    border-color: var(--yap-ink);
    background: var(--yap-ink);
    color: var(--yap-ink-fg);
  }
  .sheet :global(.dbtn.ink:hover:not(:disabled)) {
    background: var(--yap-ink-hover);
  }
  .sheet :global(.dbtn:disabled) {
    opacity: 0.55;
    cursor: default;
  }
  .sheet :global(.dbtn:focus-visible) {
    outline: none;
    box-shadow: var(--yap-focus);
  }
  .sheet :global(.err) {
    margin: 0;
    font-size: 12.5px;
    color: var(--yap-danger);
  }
  .sheet :global(.fine) {
    margin: 0;
    font-size: 12px;
    line-height: 1.5;
    color: var(--yap-muted);
  }
</style>
