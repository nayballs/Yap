<script>
  // A question before something that can't be undone ("Delete “Camping”?"):
  // the meeting notepad's confirm dialog (Wispr's: a scrim, a card scaling
  // in, pill buttons, the action in red), for the main window. Cancel has
  // the focus; Esc, the scrim and Cancel close it; Tab stays inside; the
  // focus goes back where it was (or to `returnFocus`).
  import { tick } from 'svelte';

  let {
    open = $bindable(false),
    title = '',
    body = '',
    confirmLabel = 'Delete',
    busyLabel = 'Deleting…',
    danger = true,
    busy = false,
    returnFocus = null,
    onconfirm = null,
  } = $props();

  const uid = Math.random().toString(36).slice(2, 8);
  let card = $state(null);
  let back = null;

  $effect(() => {
    if (!open) return;
    back = returnFocus ?? document.activeElement;
    tick().then(() => card?.querySelector('[data-cancel]')?.focus({ preventScroll: true }));
  });

  function close() {
    if (!open) return;
    open = false;
    const to = returnFocus ?? back;
    tick().then(() => to?.isConnected && to.focus?.({ preventScroll: true }));
  }

  function onKeydown(e) {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      if (!busy) close();
      return;
    }
    if (e.key !== 'Tab') return;
    const buttons = [...card.querySelectorAll('button:not(:disabled)')];
    if (!buttons.length) return;
    const first = buttons[0];
    const last = buttons[buttons.length - 1];
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  }
</script>

{#if open}
  <div
    class="scrim"
    role="presentation"
    onclick={(e) => e.target === e.currentTarget && !busy && close()}
    oncontextmenu={(e) => e.preventDefault()}
  >
    <div
      bind:this={card}
      class="dialog"
      role="alertdialog"
      aria-modal="true"
      aria-labelledby="confirm-title-{uid}"
      aria-describedby={body ? `confirm-body-${uid}` : undefined}
      tabindex="-1"
      onkeydown={onKeydown}
    >
      <div>
        <h2 id="confirm-title-{uid}">{title}</h2>
        {#if body}<p id="confirm-body-{uid}">{body}</p>{/if}
      </div>
      <div class="btns">
        <button type="button" class="btn" data-cancel onclick={close} disabled={busy}>Cancel</button>
        <button type="button" class="btn" class:danger onclick={() => onconfirm?.()} disabled={busy}>
          {busy ? busyLabel : confirmLabel}
        </button>
      </div>
    </div>
  </div>
{/if}

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
    gap: 28px;
    width: min(380px, 100%);
    box-sizing: border-box;
    padding: 24px;
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
      transform: scale(0.9);
    }
  }
  h2 {
    margin: 0 0 8px;
    font-size: 17px;
    font-weight: 600;
    line-height: 24px;
    overflow-wrap: anywhere;
  }
  p {
    margin: 0;
    font-size: 14px;
    line-height: 21px;
    color: var(--yap-paper-text-2);
  }
  .btns {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
  .btn {
    height: 36px;
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
  .btn:hover:not(:disabled) {
    border-color: var(--yap-paper-s2-strong);
  }
  .btn:focus-visible {
    outline: none;
    box-shadow: 0 0 0 3px var(--yap-paper-ring);
  }
  .btn.danger {
    border-color: transparent;
    background: var(--yap-danger-fill);
    color: #fff;
  }
  .btn.danger:hover:not(:disabled) {
    background: var(--yap-danger-fill-hover);
  }
  .btn:disabled {
    opacity: 0.6;
    cursor: default;
  }
</style>
