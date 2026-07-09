<script>
  // Toast viewport — Wispr-Flow-style cards (see docs screenshots 2026-07-09):
  // dark rounded card, small category chip top-left (their lavender "Tip"
  // pill), always-visible circular ✕ top-right, bold white title, soft grey
  // body, optional light action button bottom-right ("Open Settings").
  // Keeps OpenWhispr's timer behaviour: hover-pause, hairline progress bar,
  // destructive descriptions in a copyable mono error box. Mounted once in
  // ControlPanel.
  import { toastStore, dismiss, pauseToast, resumeToast } from './toast.svelte.js';

  let copiedId = $state(null);

  const CHIP_LABELS = { default: 'Tip', success: 'Done', destructive: 'Error' };

  async function copyError(t) {
    if (!t.description) return;
    try {
      await navigator.clipboard.writeText(t.description);
      copiedId = t.id;
      setTimeout(() => (copiedId = null), 2000);
    } catch {
      /* clipboard unavailable */
    }
  }

  function runAction(t) {
    try {
      t.action?.onClick?.();
    } finally {
      dismiss(t.id);
    }
  }
</script>

{#if toastStore.list.length > 0}
  <div class="viewport">
    {#each toastStore.list as t (t.id)}
      <div
        class="toast {t.variant}"
        class:exiting={t.isExiting}
        role="status"
        onmouseenter={() => pauseToast(t.id)}
        onmouseleave={() => resumeToast(t.id)}
      >
        <div class="toprow">
          <span class="chip">
            {#if t.variant === 'success'}
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 12l5 5L20 6" /></svg>
            {:else if t.variant === 'destructive'}
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 9v4" /><path d="M12 17h.01" /><path d="M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z" /></svg>
            {:else}
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3v2M12 19v2M3 12h2M19 12h2M5.6 5.6l1.4 1.4M17 17l1.4 1.4M18.4 5.6 17 7M7 17l-1.4 1.4" /><circle cx="12" cy="12" r="4" /></svg>
            {/if}
            {t.chip || CHIP_LABELS[t.variant] || CHIP_LABELS.default}
          </span>
          <button class="close" aria-label="Close" onclick={() => dismiss(t.id)}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
          </button>
        </div>
        {#if t.title}<div class="title">{t.title}</div>{/if}
        {#if t.description && t.variant === 'destructive'}
          <div class="errbox">
            <span class="errtext">{t.description}</span>
            <button class="errcopy" aria-label="Copy error" onclick={() => copyError(t)}>
              {#if copiedId === t.id}
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 12l5 5L20 6" /></svg>
              {:else}
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="9" width="12" height="12" rx="2" /><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" /></svg>
              {/if}
            </button>
          </div>
        {:else if t.description}
          <div class="desc">{t.description}</div>
        {/if}
        {#if t.action?.label}
          <div class="actions">
            <button class="actionbtn" onclick={() => runAction(t)}>{t.action.label}</button>
          </div>
        {/if}
        {#if t.duration > 0 && !t.isExiting}
          <div class="progresswrap">
            <div class="progress" style={`animation-duration:${t.duration}ms`}></div>
          </div>
        {/if}
      </div>
    {/each}
  </div>
{/if}

<style>
  .viewport {
    position: fixed;
    bottom: 20px;
    right: 20px;
    z-index: 100;
    display: flex;
    flex-direction: column;
    gap: 8px;
    pointer-events: none;
  }
  /* Wispr card: warm near-black, big radius, generous padding. */
  .toast {
    pointer-events: auto;
    position: relative;
    display: flex;
    flex-direction: column;
    width: 320px;
    padding: 14px 16px 15px;
    border-radius: 16px;
    overflow: hidden;
    background: #1c1a16;
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.4);
    animation: toast-in 300ms ease-out;
    transition:
      opacity 200ms ease-out,
      transform 200ms ease-out;
  }
  .toast.exiting {
    opacity: 0;
    transform: translateY(6px) scale(0.98);
  }
  @keyframes toast-in {
    from {
      opacity: 0;
      transform: translateY(12px) scale(0.98);
    }
    to {
      opacity: 1;
      transform: translateY(0) scale(1);
    }
  }
  .toprow {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 9px;
  }
  /* Category pill — Wispr's "Tip" chip, in Yap's warm palette per variant. */
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 24px;
    padding: 0 10px;
    border-radius: 8px;
    background: #ecd9b8;
    color: #453413;
    font-size: 12px;
    font-weight: 650;
  }
  .chip svg {
    width: 12px;
    height: 12px;
  }
  .toast.success .chip {
    background: #c6e3c4;
    color: #1e4620;
  }
  .toast.destructive .chip {
    background: #f0c4bf;
    color: #5c1a14;
  }
  .title {
    font-size: 14.5px;
    font-weight: 700;
    line-height: 1.35;
    color: rgba(255, 255, 255, 0.95);
  }
  .desc {
    margin-top: 3px;
    font-size: 13px;
    line-height: 1.5;
    color: rgba(255, 255, 255, 0.55);
  }
  .errbox {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 6px;
    margin-top: 7px;
    padding: 6px 8px;
    border-radius: 8px;
    background: rgba(255, 255, 255, 0.05);
    border: 1px solid rgba(255, 255, 255, 0.07);
  }
  .errtext {
    min-width: 0;
    font-family: ui-monospace, Consolas, monospace;
    font-size: 11px;
    line-height: 1.45;
    color: rgba(252, 165, 165, 0.85);
    overflow-wrap: anywhere;
    user-select: all;
  }
  .errcopy {
    flex: 0 0 auto;
    display: inline-flex;
    width: 18px;
    height: 18px;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: 4px;
    background: none;
    color: rgba(255, 255, 255, 0.3);
    cursor: pointer;
  }
  .errcopy:hover {
    color: rgba(255, 255, 255, 0.7);
    background: rgba(255, 255, 255, 0.06);
  }
  .errcopy svg {
    width: 11px;
    height: 11px;
  }
  /* Always-visible circular ✕ in the card's corner (Wispr). */
  .close {
    flex: 0 0 auto;
    display: flex;
    width: 26px;
    height: 26px;
    align-items: center;
    justify-content: center;
    border-radius: 50%;
    border: none;
    background: rgba(255, 255, 255, 0.1);
    color: rgba(255, 255, 255, 0.85);
    cursor: pointer;
    transition: background 150ms ease;
  }
  .close:hover {
    background: rgba(255, 255, 255, 0.2);
    color: #fff;
  }
  .close svg {
    width: 11px;
    height: 11px;
  }
  /* Light action button bottom-right — Wispr's "Open Settings". */
  .actions {
    display: flex;
    justify-content: flex-end;
    margin-top: 12px;
  }
  .actionbtn {
    height: 32px;
    padding: 0 14px;
    border: none;
    border-radius: 10px;
    background: #f5f3ee;
    color: #26231c;
    font: inherit;
    font-size: 13px;
    font-weight: 650;
    cursor: pointer;
    transition: background 150ms ease;
  }
  .actionbtn:hover {
    background: #ffffff;
  }
  .progresswrap {
    position: absolute;
    bottom: 0;
    left: 16px;
    right: 16px;
    height: 2px;
    overflow: hidden;
    border-radius: 2px 2px 0 0;
  }
  .progress {
    height: 100%;
    background: rgba(255, 255, 255, 0.14);
    animation-name: toast-progress;
    animation-timing-function: linear;
    animation-fill-mode: forwards;
  }
  .toast.success .progress {
    background: rgba(140, 205, 140, 0.35);
  }
  .toast.destructive .progress {
    background: rgba(248, 113, 113, 0.35);
  }
  @keyframes toast-progress {
    from {
      width: 100%;
    }
    to {
      width: 0%;
    }
  }
</style>
