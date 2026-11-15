<script>
  // **‹ 2/3 ›** (15 Nov 2026, the Claude app's): a message's versions, one shown at a time — an answer retried, or
  // the person's message edited (chat/chats/branches.rs). `fork`: `{at, shown, count}` (chat_get's `forks`);
  // `onbranch(place)` shows version `place` (1…count); `why`: why it can't switch now ('' when it can).
  let { fork, onbranch, why = '' } = $props();
  const go = (place) => () => {
    if (!why && place >= 1 && place <= fork.count && place !== fork.shown) onbranch?.(place);
  };
</script>

<span class="branches" role="group" aria-label={`Version ${fork.shown} of ${fork.count}`} data-testid="chat-branch">
  <button
    type="button"
    class="bstep"
    class:off={!!why}
    disabled={fork.shown <= 1}
    aria-disabled={why ? 'true' : undefined}
    aria-label="Previous version"
    title={why || 'Previous version'}
    data-testid="chat-branch-prev"
    onclick={go(fork.shown - 1)}
  >
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m15 18-6-6 6-6" /></svg>
  </button>
  <span class="bcount" data-testid="chat-branch-count">{fork.shown}/{fork.count}</span>
  <button
    type="button"
    class="bstep"
    class:off={!!why}
    disabled={fork.shown >= fork.count}
    aria-disabled={why ? 'true' : undefined}
    aria-label="Next version"
    title={why || 'Next version'}
    data-testid="chat-branch-next"
    onclick={go(fork.shown + 1)}
  >
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m9 18 6-6-6-6" /></svg>
  </button>
</span>

<style>
  .branches {
    display: inline-flex;
    align-items: center;
    gap: 1px;
    font-size: 11px;
    font-variant-numeric: tabular-nums;
    color: var(--yap-muted);
    user-select: none;
  }
  .bcount {
    min-width: 24px;
    text-align: center;
  }
  .bstep {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    padding: 0;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    color: inherit;
    cursor: pointer;
  }
  .bstep svg {
    width: 13px;
    height: 13px;
  }
  .bstep:hover:not(:disabled):not(.off) {
    background: var(--yap-paper-s2);
    color: var(--yap-paper-ink);
  }
  .bstep:disabled,
  .bstep.off {
    opacity: 0.4;
    cursor: default;
  }
  .bstep:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
</style>
