<script>
  // "Show summary" (chat/compact.rs): what a long chat's older messages were
  // summarised to, read-only — the notes the model gets in their place. Esc,
  // the scrim and Close close it; the focus goes back to the divider's
  // button. Its own chunk, loaded the first time it opens.
  import { tick, onMount } from 'svelte';
  import { modelLine } from '../../core/aiModel.js';

  /** `summary`: { text, upto, model }; `onclose()`. */
  let { summary, onclose } = $props();

  let card = $state(null);
  const lines = $derived(
    String(summary?.text ?? '')
      .split('\n')
      .map((l) => l.trim())
      .filter(Boolean)
  );
  const covers = $derived(summary?.upto === 1 ? 'the first message' : `the first ${summary?.upto ?? 0} messages`);

  onMount(() => {
    tick().then(() => card?.querySelector('[data-close]')?.focus({ preventScroll: true }));
  });

  function onKeydown(e) {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      onclose?.();
    } else if (e.key === 'Tab') {
      // One button: the focus stays on it.
      e.preventDefault();
      card?.querySelector('[data-close]')?.focus();
    }
  }
</script>

<div class="scrim" role="presentation" onclick={(e) => e.target === e.currentTarget && onclose?.()}>
  <div
    bind:this={card}
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-labelledby="chat-summary-title"
    tabindex="-1"
    onkeydown={onKeydown}
    data-testid="chat-summary-dialog"
  >
    <div class="head">
      <h2 id="chat-summary-title">Earlier in this chat</h2>
      <p class="sub">
        What {covers} said, summarised so the chat stays within the model's memory. The messages themselves are still here.
      </p>
    </div>
    <ul class="notes" data-testid="chat-summary-text">
      {#each lines as l, i (i)}
        <li>{l.replace(/^[-*•]\s*/, '')}</li>
      {/each}
    </ul>
    <div class="foot">
      {#if summary?.model}<span class="by">Summarised by {modelLine(summary.model)}</span>{/if}
      <button type="button" class="btn" data-close onclick={() => onclose?.()}>Close</button>
    </div>
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 95;
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
  .dialog {
    display: flex;
    flex-direction: column;
    gap: 16px;
    width: min(520px, 100%);
    max-height: min(640px, 85vh);
    box-sizing: border-box;
    padding: 22px 24px 18px;
    border: 1px solid var(--yap-paper-s2);
    border-radius: 14px;
    background: var(--yap-paper);
    color: var(--yap-paper-ink);
    box-shadow: var(--yap-shadow-dialog);
    outline: none;
    animation: dialog-in 0.15s var(--yap-ease-spring) both;
  }
  @keyframes dialog-in {
    from {
      opacity: 0;
      transform: scale(0.96);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .scrim,
    .dialog {
      animation: none;
    }
  }
  :global([data-yap-motion='reduced']) .scrim,
  :global([data-yap-motion='reduced']) .dialog {
    animation: none;
  }
  h2 {
    margin: 0 0 6px;
    font-size: 17px;
    font-weight: 600;
    line-height: 24px;
  }
  .sub {
    margin: 0;
    font-size: 13px;
    line-height: 19px;
    color: var(--yap-paper-text-2);
  }
  .notes {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    margin: 0;
    padding: 12px 14px 12px 30px;
    border: 1px solid var(--yap-paper-s2);
    border-radius: 10px;
    background: var(--yap-paper-card);
    font-family: var(--yap-font-reading);
    font-size: calc(13px * var(--yap-reading-scale));
    line-height: calc(1.6 * var(--yap-reading-leading));
  }
  .notes li + li {
    margin-top: 4px;
  }
  .foot {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 10px;
  }
  .by {
    flex: 1 1 auto;
    font-size: 12px;
    color: var(--yap-paper-text-2);
  }
  .btn {
    height: 34px;
    padding: 0 16px;
    border: 1px solid var(--yap-paper-s2);
    border-radius: 9999px;
    background: var(--yap-paper-card);
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
  }
  .btn:hover {
    border-color: var(--yap-paper-s2-strong);
  }
  .btn:focus-visible {
    outline: none;
    box-shadow: 0 0 0 3px var(--yap-paper-ring);
  }
</style>
