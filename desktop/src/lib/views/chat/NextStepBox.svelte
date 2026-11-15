<script>
  // **The next-step box** (`suggest_next_step`, chat/cards/next_step.rs,
  // 15 Nov 2026; the Claude app's box at the end of a turn): under the
  // conversation's latest answer, when the model left the person something to
  // decide — a one or two sentence recap, one question and one or two buttons
  // that answer it, the model's pick first. A click sends the button's words as
  // the person's next message, as if typed (chatState's `sendNextStep`); the
  // box goes once anything follows the answer (ChatMessage shows it on the
  // thread's last message only). A recap alone has no buttons.

  /** `next`: `{recap, question?, buttons?: [{label, reply?}]}`; `onpick(button)`; `busy`: a question is being answered. */
  let { next, onpick, busy = false } = $props();
  const buttons = $derived(next?.question ? (next.buttons || []).slice(0, 2) : []);
</script>

<div class="nextstep" role="group" aria-label="Next step" data-testid="chat-next-step">
  <p class="nrecap">{next.recap}</p>
  {#if next.question}
    <p class="nask">{next.question}</p>
  {/if}
  {#if buttons.length}
    <div class="nbtns">
      {#each buttons as b, i (b.label)}
        <button type="button" class="nbtn" class:first={i === 0} disabled={busy} title={b.reply && b.reply !== b.label ? `Sends: ${b.reply}` : undefined} onclick={() => onpick?.(b)} data-testid="chat-next-step-button">{b.label}</button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .nextstep {
    box-sizing: border-box;
    width: min(560px, 100%);
    margin: 8px 0 2px;
    padding: 10px 12px;
    border: 1px solid var(--yap-paper-rule);
    border-radius: 10px;
    background: var(--yap-paper-s1);
    color: var(--yap-paper-ink);
    font-size: 13px;
    line-height: 1.45;
  }
  .nrecap {
    margin: 0;
    color: var(--yap-paper-muted);
    overflow-wrap: anywhere;
  }
  .nask {
    margin: 6px 0 0;
    font-weight: 650;
    overflow-wrap: anywhere;
  }
  .nbtns {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 9px;
  }
  .nbtn {
    height: 28px;
    padding: 0 12px;
    border: 1px solid var(--yap-paper-rule);
    border-radius: 999px;
    background: var(--yap-paper);
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .nbtn.first {
    border-color: var(--yap-primary-line);
    background: var(--yap-primary);
    color: var(--yap-primary-fg);
  }
  .nbtn:hover:not(:disabled) {
    background: var(--yap-paper-s2);
  }
  .nbtn.first:hover:not(:disabled) {
    background: var(--yap-primary-hover);
  }
  .nbtn:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  .nbtn:disabled {
    cursor: default;
    opacity: 0.55;
  }
</style>
